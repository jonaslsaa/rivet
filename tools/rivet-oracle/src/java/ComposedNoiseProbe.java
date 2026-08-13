import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import java.io.PrintWriter;
import java.nio.file.Path;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.ResourceKey;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.level.ChunkLevel;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.packs.resources.ResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.levelgen.DensityFunction;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.NoiseRouter;
import net.minecraft.world.level.levelgen.NoiseRouterData;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.presets.WorldPresets;

/**
 * Bit-exact composed-noise golden sampler for the pinned Paper 26.2 overworld
 * generator (the `mc.world.level.levelgen` NOISE checkpoint).
 *
 * Emits the router climate fields, the float-cast weirdness that feeds
 * `peaksAndValleys`, the folded `peaksAndValleys`, the interpolated final
 * density (`NoiseBasedChunkGenerator.getInterpolatedNoiseValue` — the value
 * `doFill` uses to place blocks), the raw `finalDensity` router field, and
 * `preliminarySurfaceLevel` — every value as BOTH the round-tripping JSON
 * double AND the raw IEEE-754 bit pattern (`Double.doubleToLongBits` /
 * `Float.floatToIntBits`), so a Rust port can assert `f64::to_bits` exactly.
 *
 * The coordinate matrix is the #175 chunk-coordinate corpus expressed as block
 * columns: each corpus chunk coordinate (cx, cz) is sampled at its block
 * origin (cx*16, cz*16), covering the positive/negative/region-seam sweep
 * positions as real block columns.
 *
 * Also embeds Paper's live `FULL_CHUNK_STEP` reachability
 * (`ChunkLevel.RADIUS_AROUND_FULL_CHUNK` + `getStatusAroundFullChunk`): the
 * distance-from-forced-center -> serialized ChunkStatus map. This is the
 * authoritative "which statuses a forced FULL capture actually serializes"
 * answer (non-monotonic, gap-containing) that the Rust scoreboard must
 * reproduce from a faithful port of the generation pyramid builder.
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" ComposedNoiseProbe --seed 42 --output dir/ [--paper pin]
 */
public final class ComposedNoiseProbe {
    private ComposedNoiseProbe() {}

    // The #175 chunk-coordinate matrix (the same set `corpus::COORDINATES`
    // pins on the Rust side), as block-origin sampling points.
    private static final int[][] CORPUS_CHUNK_COORDS = {
        {0, 0}, {15, 15}, {31, 31}, {-1, -1}, {-16, -16}, {-31, -31}, {-1, 0}, {0, -1},
    };

    // The vertical slice sampled per column (spans below/at/above the surface).
    private static final int[] DENSITY_YS = {
        -60, -40, -20, 0, 20, 40, 60, 80, 100, 120,
    };

    public static void main(String[] args) throws Exception {
        long seed = 42L;
        String output = null;
        String paper = null;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--seed" -> seed = Long.parseLong(args[++i]);
                case "--output" -> output = args[++i];
                case "--paper" -> paper = args[++i];
                default -> throw new IllegalArgumentException("Unknown arg: " + args[i]);
            }
        }
        if (output == null) {
            throw new IllegalArgumentException("Usage: ComposedNoiseProbe --seed <seed> --output <dir> [--paper <pin>]");
        }

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // Boot the vanilla registries exactly like WorldGenSampler does — no
        // full server boot.
        PackRepository packRepository = ServerPacksSource.createVanillaTrustedRepository();
        MinecraftServer.configurePackRepository(
            packRepository,
            new net.minecraft.world.level.WorldDataConfiguration(
                new net.minecraft.world.level.DataPackConfig(
                    List.of("minecraft", "vanilla"), List.of()
                ),
                FeatureFlags.DEFAULT_FLAGS
            ),
            true,
            false
        );
        ResourceManager resourceManager = new MultiPackResourceManager(PackType.SERVER_DATA, packRepository.openAllSelected());

        var layers = RegistryLayer.createRegistryAccess();
        List<net.minecraft.core.HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(
            layers.getAccessForLoading(RegistryLayer.WORLDGEN),
            TagLoader.loadTagsForExistingRegistries(resourceManager, layers.getLayer(RegistryLayer.STATIC))
        );
        RegistryAccess.Frozen worldGenRegistries = RegistryDataLoader.load(
            resourceManager, lookups, RegistryDataLoader.WORLDGEN_REGISTRIES, Runnable::run
        ).join();
        layers = layers.replaceFrom(RegistryLayer.WORLDGEN, worldGenRegistries);

        List<net.minecraft.core.HolderLookup.RegistryLookup<?>> staticAndWorldgen = new java.util.ArrayList<>(lookups);
        worldGenRegistries.listRegistries().forEach(staticAndWorldgen::add);
        RegistryAccess.Frozen dimensionRegistries = RegistryDataLoader.load(
            resourceManager, staticAndWorldgen, RegistryDataLoader.DIMENSION_REGISTRIES, Runnable::run
        ).join();
        layers = layers.replaceFrom(RegistryLayer.DIMENSIONS, dimensionRegistries);

        RegistryAccess registryAccess = layers.compositeAccess().freeze();

        Holder<net.minecraft.world.level.levelgen.presets.WorldPreset> preset =
            registryAccess.lookupOrThrow(Registries.WORLD_PRESET).getOrThrow(WorldPresets.NORMAL);
        NoiseBasedChunkGenerator generator = (NoiseBasedChunkGenerator) preset.value().createWorldDimensions().overworld();
        RandomState randomState = RandomState.create(registryAccess, NoiseGeneratorSettings.OVERWORLD, seed);
        NoiseRouter router = randomState.router();

        JsonObject root = new JsonObject();
        root.addProperty("seed", seed);
        root.addProperty("paper", paper == null ? "unknown" : paper);
        root.addProperty("dimension", "overworld");
        root.addProperty("generator", "normal");
        root.addProperty("level-type", "minecraft:normal");
        root.addProperty("noise-settings", generator.generatorSettings().unwrapKey().map(ResourceKey::identifier).map(Object::toString).orElse("?"));
        root.addProperty("format", 1);

        // ---- Paper's live FULL_CHUNK_STEP reachability ----------------------
        // The authoritative map from distance-to-forced-center-chunk to the
        // ChunkStatus a chunk at that distance is serialized at. Recorded from
        // the LIVE Paper (ChunkLevel), so the Rust port of the generation
        // pyramid must reproduce it bit-for-bit.
        JsonObject fcs = new JsonObject();
        fcs.addProperty("level", ChunkLevel.FULL_CHUNK_LEVEL);
        fcs.addProperty("accumulated-radius", ChunkLevel.RADIUS_AROUND_FULL_CHUNK);
        fcs.addProperty("max-level", ChunkLevel.MAX_LEVEL);
        JsonArray byDistance = new JsonArray();
        for (int d = 0; d <= ChunkLevel.RADIUS_AROUND_FULL_CHUNK; d++) {
            JsonObject e = new JsonObject();
            e.addProperty("distance", d);
            e.addProperty("status", ChunkLevel.getStatusAroundFullChunk(d).getName());
            byDistance.add(e);
        }
        fcs.add("by-distance", byDistance);
        root.add("full-chunk-step", fcs);

        // ---- climate / router fields per block column -----------------------
        JsonArray climate = new JsonArray();
        for (int[] c : CORPUS_CHUNK_COORDS) {
            int cx = c[0], cz = c[1];
            int x = cx * 16, z = cz * 16;
            DensityFunction.SinglePointContext ctx = new DensityFunction.SinglePointContext(x, 0, z);
            JsonObject e = new JsonObject();
            e.addProperty("x", x);
            e.addProperty("y", 0);
            e.addProperty("z", z);
            e.addProperty("cx", cx);
            e.addProperty("cz", cz);
            e.add("temperature", floatSample((float) router.temperature().compute(ctx)));
            e.add("vegetation", floatSample((float) router.vegetation().compute(ctx)));
            e.add("continents", floatSample((float) router.continents().compute(ctx)));
            e.add("erosion", floatSample((float) router.erosion().compute(ctx)));
            e.add("depth", floatSample((float) router.depth().compute(ctx)));
            e.add("ridges", floatSample((float) router.ridges().compute(ctx)));
            float weirdness = (float) router.ridges().compute(ctx);
            e.add("weirdness", floatSample(weirdness));
            e.add("peaksAndValleys", floatSample(NoiseRouterData.peaksAndValleys(weirdness)));
            climate.add(e);
        }
        root.add("climate", climate);

        // ---- density / preliminary-surface per column slice ----------------
        JsonArray density = new JsonArray();
        for (int[] c : CORPUS_CHUNK_COORDS) {
            int cx = c[0], cz = c[1];
            int x = cx * 16, z = cz * 16;
            for (int y : DENSITY_YS) {
                DensityFunction.SinglePointContext ctx = new DensityFunction.SinglePointContext(x, y, z);
                JsonObject e = new JsonObject();
                e.addProperty("x", x);
                e.addProperty("y", y);
                e.addProperty("z", z);
                e.addProperty("cx", cx);
                e.addProperty("cz", cz);
                e.add("density", doubleSample(generator.getInterpolatedNoiseValue(randomState, ctx)));
                e.add("finalDensity", doubleSample(router.finalDensity().compute(ctx)));
                e.add("preliminarySurfaceLevel", doubleSample(router.preliminarySurfaceLevel().compute(ctx)));
                density.add(e);
            }
        }
        root.add("density", density);

        Path outDir = Path.of(output);
        java.nio.file.Files.createDirectories(outDir);
        try (PrintWriter writer = new PrintWriter(outDir.resolve("composed-noise.json").toFile(), "UTF-8")) {
            writer.println(new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(root));
        }
        System.out.println(
            "sampled " + climate.size() + " climate + " + density.size() + " density entries "
                + "(FULL_CHUNK_STEP radius " + ChunkLevel.RADIUS_AROUND_FULL_CHUNK + ", MAX_LEVEL " + ChunkLevel.MAX_LEVEL + ")"
                + " to " + outDir.resolve("composed-noise.json")
        );
    }

    private static JsonObject doubleSample(double d) {
        JsonObject o = new JsonObject();
        if (Double.isNaN(d)) {
            o.add("value", new JsonPrimitive("NaN"));
        } else {
            o.add("value", new JsonPrimitive(d));
        }
        o.addProperty("bits", Double.doubleToLongBits(d));
        return o;
    }

    private static JsonObject floatSample(float f) {
        JsonObject o = new JsonObject();
        o.add("value", new JsonPrimitive((double) f));
        o.addProperty("bits", Float.floatToIntBits(f));
        return o;
    }
}
