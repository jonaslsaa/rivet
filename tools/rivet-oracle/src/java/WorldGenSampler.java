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
import net.minecraft.core.QuartPos;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.ResourceKey;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.packs.resources.ResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.levelgen.DensityFunction;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.NoiseRouter;
import net.minecraft.world.level.levelgen.NoiseRouterData;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.Heightmap;

/**
 * Samples the pinned Paper 26.2 worldgen pipeline at chosen positions and
 * writes stable semantic JSON fixtures (density / biome / surface).  This is
 * the Paper-side sampling half of issue #51: it runs against the real Paper
 * registries + datapack data (no full server boot) and is deterministic for a
 * fixed seed + generator settings.
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" WorldGenSampler --seed 42 --output samples/ --paper 26.2-DEV-main@0a99345
 */
public final class WorldGenSampler {
    private WorldGenSampler() {}

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
            throw new IllegalArgumentException("Usage: WorldGenSampler --seed <seed> --output <dir> [--paper <pin>]");
        }

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // Load the vanilla datapack registries (worldgen + dimension layers) the
        // same way Paper's RegistryHelper does — no server boot needed.
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

        // The normal overworld generator comes from the minecraft:normal WorldPreset
        // (dimensions are embedded inline in the preset, not in a dimension registry).
        Holder<net.minecraft.world.level.levelgen.presets.WorldPreset> preset =
            registryAccess.lookupOrThrow(Registries.WORLD_PRESET).getOrThrow(net.minecraft.world.level.levelgen.presets.WorldPresets.NORMAL);
        NoiseBasedChunkGenerator generator = (NoiseBasedChunkGenerator) preset.value().createWorldDimensions().overworld();
        RandomState randomState = RandomState.create(registryAccess, NoiseGeneratorSettings.OVERWORLD, seed);
        NoiseRouter router = randomState.router();
        Climate.Sampler sampler = randomState.sampler();
        MultiNoiseBiomeSource biomeSource = (MultiNoiseBiomeSource) generator.getBiomeSource();

        JsonObject root = new JsonObject();
        root.addProperty("seed", seed);
        root.addProperty("paper", paper == null ? "unknown" : paper);
        root.addProperty("dimension", "overworld");
        root.addProperty("generator", "normal");
        root.addProperty("level-type", "minecraft:normal");
        root.addProperty("noise-settings", generator.generatorSettings().unwrapKey().map(ResourceKey::identifier).map(Object::toString).orElse("?"));
        root.addProperty("format", 1);

        // ---- density samples -------------------------------------------------
        JsonArray density = new JsonArray();
        int[][] densityPositions = {
            {0, -60, 0}, {0, -40, 0}, {0, -20, 0}, {0, 0, 0}, {0, 20, 0}, {0, 40, 0}, {0, 60, 0}, {0, 80, 0}, {0, 100, 0}, {0, 120, 0},
            {100, -40, -100}, {100, 0, -100}, {100, 40, -100}, {100, 80, -100},
            {-200, -40, 300}, {-200, 0, 300}, {-200, 60, 300},
            {333, 20, -444}, {512, 0, 512}, {-512, 80, -512}, {1024, 40, 0}, {0, 40, 1024},
            {16, 70, 16}, {31, 70, 31}, {15, 71, 15},
        };
        for (int[] p : densityPositions) {
            int x = p[0], y = p[1], z = p[2];
            DensityFunction.SinglePointContext ctx = new DensityFunction.SinglePointContext(x, y, z);
            JsonObject e = new JsonObject();
            e.addProperty("x", x);
            e.addProperty("y", y);
            e.addProperty("z", z);
            e.add("density", jsonDouble(generator.getInterpolatedNoiseValue(randomState, ctx)));
            e.add("temperature", jsonDouble(router.temperature().compute(ctx)));
            e.add("vegetation", jsonDouble(router.vegetation().compute(ctx)));
            e.add("continents", jsonDouble(router.continents().compute(ctx)));
            e.add("erosion", jsonDouble(router.erosion().compute(ctx)));
            e.add("depth", jsonDouble(router.depth().compute(ctx)));
            e.add("ridges", jsonDouble(router.ridges().compute(ctx)));
            float weirdness = (float) router.ridges().compute(ctx);
            e.add("weirdness", jsonDouble(weirdness));
            e.add("peaksAndValleys", jsonDouble(NoiseRouterData.peaksAndValleys(weirdness)));
            e.add("preliminarySurfaceLevel", jsonDouble(router.preliminarySurfaceLevel().compute(ctx)));
            density.add(e);
        }
        root.add("density", density);

        // ---- biome samples ---------------------------------------------------
        JsonArray biome = new JsonArray();
        int[][] biomePositions = {
            {0, 0}, {0, 8}, {0, 16}, {16, 0}, {32, 32}, {64, 0}, {0, 64}, {100, 100}, {-100, 100}, {100, -100},
            {-200, 200}, {300, -300}, {512, 0}, {0, 512}, {-512, -512}, {1024, 1024}, {1234, 4321}, {-999, -999},
            {256, 256}, {257, 257}, {4000, 0}, {0, 4000},
        };
        for (int[] p : biomePositions) {
            int x = p[0], z = p[1];
            int quartX = QuartPos.fromBlock(x);
            int quartZ = QuartPos.fromBlock(z);
            Holder<Biome> biomeHolder = biomeSource.getNoiseBiome(quartX, 0, quartZ, sampler);
            JsonObject e = new JsonObject();
            e.addProperty("x", x);
            e.addProperty("z", z);
            e.addProperty("quartX", quartX);
            e.addProperty("quartZ", quartZ);
            e.addProperty("biome", biomeHolder.unwrapKey().map(k -> k.identifier().toString()).orElse("?"));
            Registry<Biome> biomeReg = registryAccess.lookupOrThrow(Registries.BIOME);
            e.addProperty("biomeId", biomeReg.getId(biomeHolder.value()));
            Climate.TargetPoint target = sampler.sample(quartX, 0, quartZ);
            e.add("climate_temperature", jsonDouble(Climate.unquantizeCoord(target.temperature())));
            e.add("climate_humidity", jsonDouble(Climate.unquantizeCoord(target.humidity())));
            e.add("climate_continentalness", jsonDouble(Climate.unquantizeCoord(target.continentalness())));
            e.add("climate_erosion", jsonDouble(Climate.unquantizeCoord(target.erosion())));
            e.add("climate_depth", jsonDouble(Climate.unquantizeCoord(target.depth())));
            e.add("climate_weirdness", jsonDouble(Climate.unquantizeCoord(target.weirdness())));
            biome.add(e);
        }
        root.add("biome", biome);

        // ---- surface samples (base column + heightmap) ----------------------
        // The pre-surface noise column + WORLD_SURFACE_WG heightmap per column.
        // These are the deterministic worldgen outputs the surface-rule wave
        // transforms; full post-surface block sampling needs the chunk pipeline
        // (deferred — see the fixture notes / issue #179).
        JsonArray surface = new JsonArray();
        int[][] surfaceColumns = {
            {0, 0}, {0, 8}, {8, 0}, {16, 16}, {32, 0}, {0, 32}, {64, 64}, {100, 50}, {-50, 100},
            {200, 200}, {-300, 300}, {512, 0}, {0, 512}, {-512, -512}, {1000, 1000}, {1234, 4321},
        };
        for (int[] p : surfaceColumns) {
            int x = p[0], z = p[1];
            net.minecraft.world.level.levelgen.NoiseSettings noiseSettings = generator.generatorSettings().value().noiseSettings();
            net.minecraft.world.level.LevelHeightAccessor heightAccessor = net.minecraft.world.level.LevelHeightAccessor.create(noiseSettings.minY(), noiseSettings.height());
            int height = generator.getBaseHeight(x, z, Heightmap.Types.WORLD_SURFACE_WG, heightAccessor, randomState);
            JsonObject e = new JsonObject();
            e.addProperty("x", x);
            e.addProperty("z", z);
            e.addProperty("heightmap.WORLD_SURFACE_WG", height);
            // Sample a few base-column blocks to pin the terrain shape.
            int[] ys = {height - 1, height - 4, height - 20, 0, -10, -60};
            for (int y : ys) {
                if (y < -64 || y > 320) {
                    continue;
                }
                net.minecraft.world.level.block.state.BlockState state = generator.getBaseColumn(x, z, heightAccessor, randomState).getBlock(y);
                e.addProperty("y" + y, state.toString());
            }
            surface.add(e);
        }
        root.add("surface", surface);

        Gson gson = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();
        Path outDir = Path.of(output);
        java.nio.file.Files.createDirectories(outDir);
        try (PrintWriter writer = new PrintWriter(outDir.resolve("samples.json").toFile(), "UTF-8")) {
            gson.toJson(root, writer);
        }
        System.out.println("sampled " + density.size() + " density + " + biome.size() + " biome + "
            + surface.size() + " surface entries to " + outDir.resolve("samples.json"));
    }

    private static JsonPrimitive jsonDouble(double d) {
        if (Double.isNaN(d)) {
            return new JsonPrimitive("NaN");
        }
        return new JsonPrimitive(d);
    }
}
