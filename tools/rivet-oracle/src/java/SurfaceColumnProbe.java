import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
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
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.packs.resources.ResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.util.Mth;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.Beardifier;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.PositionalRandomFactory;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.SurfaceRules;
import net.minecraft.world.level.levelgen.VerticalAnchor;
import net.minecraft.world.level.levelgen.WorldGenerationContext;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.presets.WorldPresets;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.StructureManager;
import org.jspecify.annotations.Nullable;

/**
 * Independent Paper 26.2 post-surface column oracle for issue #179.
 *
 * Runs the REAL overworld generator pipeline (`NoiseBasedChunkGenerator`
 * `createBiomes` -> `fillFromNoise` -> `buildSurface`) on REAL `ProtoChunk`s at
 * seed 42, then emits deterministic sampled post-surface columns plus enough
 * metadata to detect a pre-surface / no-op capture (a probe that recorded the
 * chunk before buildSurface ran, or whose surface rules never applied).
 *
 * The probe deliberately drives the generator through its own chunk loop rather
 * than a server boot:
 *   - Vanilla registries are booted exactly like `ComposedNoiseProbe`
 *     (SharedConstants.tryDetectVersion + Bootstrap + PackRepository +
 *     RegistryDataLoader for the WORLDGEN + DIMENSION layers).
 *   - Each corpus chunk gets a real `ProtoChunk` (PalettedContainerFactory +
 *     UpgradeData.EMPTY + LevelHeightAccessor.create(-64, 384)).
 *   - The `NoiseChunk` is created up front (mirroring the generator's private
 *     `createNoiseChunk`, with `Beardifier.EMPTY` and a fluid picker that
 *     replicates the private `createFluidPicker`) and pre-set via
 *     `ChunkAccess.getOrCreateNoiseChunk`, so the `StructureManager` / its
 *     `LevelAccessor` are NEVER consulted (verified: the only deref of
 *     `structureManager` on this pipeline path is inside that factory lambda).
 *     A null-holding StructureManager is passed for signature compatibility.
 *   - `possibleBiomes = null` is passed to the @VisibleForTesting
 *     `buildSurface` overload: this disables Paper's canNeverMatch/
 *     willAlwaysMatch biome-condition pruning and forces the full per-column
 *     biome evaluation (honest, not a weakening of the rules).
 *
 * Emitted per sampled column:
 *   - `pre` and `post` block states at every `SAMPLE_STEP` Y (block name via
 *     the block registry key + the raw state id in `Block.BLOCK_STATE_REGISTRY`).
 *   - `surfaceChanged` — whether any sampled Y differs between pre and post
 *     (a probe that skipped buildSurface would emit all-false).
 *   - Pre/post surface heights (WORLD_SURFACE_WG and OCEAN_FLOOR_WG) so a
 *     capture that ran surface before fill (heightmaps unprimed) is visible.
 *   - The exact biome used for the surface at that column (post-surface
 *     `biomeManager.getBiome` result is deterministic and part of the oracle).
 *
 * Run inside the full bundler classpath, e.g.:
 *   java -cp "<server.jar>:<all lib jars>" SurfaceColumnProbe --seed 42 --output dir/ [--paper pin]
 */
public final class SurfaceColumnProbe {
    private SurfaceColumnProbe() {}

    // The #175 chunk-coordinate matrix (the same set the composed-noise probe
    // and `corpus::COORDINATES` use), giving positive/negative/region-seam
    // coverage. All 8 seed-42 chunks are structure-free, so Beardifier.EMPTY is
    // correct (no terrain-adaptation influence).
    private static final int[][] CORPUS_CHUNK_COORDS = {
        {0, 0}, {15, 15}, {31, 31}, {-1, -1}, {-16, -16}, {-31, -31}, {-1, 0}, {0, -1},
    };

    private static final int MIN_Y = -64;
    private static final int HEIGHT = 384;
    private static final int MAX_Y = MIN_Y + HEIGHT;

    // Every SAMPLE_STEP-th Y down each column is captured pre- and post-surface.
    private static final int SAMPLE_STEP = 4;

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
            throw new IllegalArgumentException("Usage: SurfaceColumnProbe --seed <seed> --output <dir> [--paper <pin>]");
        }

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        PackRepository packRepository = ServerPacksSource.createVanillaTrustedRepository();
        MinecraftServer.configurePackRepository(
            packRepository,
            new net.minecraft.world.level.WorldDataConfiguration(
                new net.minecraft.world.level.DataPackConfig(List.of("minecraft", "vanilla"), List.of()),
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

        PalettedContainerFactory containerFactory = PalettedContainerFactory.create(registryAccess);
        LevelHeightAccessor heightAccessor = LevelHeightAccessor.create(MIN_Y, HEIGHT);
        Blender emptyBlender = Blender.empty();

        // Mirrors WorldGenRegion line ~121: the surface path samples biomes
        // through a BiomeManager built on the generator's noise biome source.
        BiomeManager biomeManager = new BiomeManager(
            (qx, qy, qz) -> generator.getBiomeSource().getNoiseBiome(qx, qy, qz, randomState.sampler()),
            BiomeManager.obfuscateSeed(seed)
        );

        // Signature-compatible stand-in: never dereferenced because the noise
        // chunk is pre-set on every chunk before the generator is invoked.
        StructureManager structureManager = new StructureManager(null, new WorldOptions(seed, false, false), null);

        JsonObject root = new JsonObject();
        root.addProperty("seed", seed);
        root.addProperty("paper", paper == null ? "unknown" : paper);
        root.addProperty("dimension", "overworld");
        root.addProperty("generator", "normal");
        root.addProperty("level-type", "minecraft:normal");
        root.addProperty("noise-settings", generator.generatorSettings().unwrapKey().map(ResourceKey::identifier).map(Object::toString).orElse("?"));
        root.addProperty("min-y", MIN_Y);
        root.addProperty("height", HEIGHT);
        root.addProperty("sea-level", generator.generatorSettings().value().seaLevel());
        root.addProperty("possible-biomes", "null");
        // Paper injects `paper:optionally_flat_bedrock_condition_source` at the
        // top of the overworld surface sequence; the real class derefs
        // context.level() for generateFlatBedrock. This probe builds surface
        // with a Level-free WorldGenerationContext, so the probe registers a
        // shadow of that condition source under the same codec id with the
        // DEFAULT config (generateFlatBedrock = false) — exact for these
        // default-overworld columns. The fixture is pinned to this substitution.
        root.addProperty("flat-bedrock-substitution", "generateFlatBedrock=false (Paper default)");
        root.addProperty("format", 1);

        JsonArray columns = new JsonArray();
        int sampleCount = (MAX_Y - MIN_Y) / SAMPLE_STEP;

        for (int[] c : CORPUS_CHUNK_COORDS) {
            int cx = c[0], cz = c[1];
            ChunkPos chunkPos = new ChunkPos(cx, cz);
            int minBlockX = chunkPos.getMinBlockX();
            int minBlockZ = chunkPos.getMinBlockZ();

            ProtoChunk chunk = new ProtoChunk(chunkPos, UpgradeData.EMPTY, heightAccessor, containerFactory, null);
            NoiseChunk noiseChunk = NoiseChunk.forChunk(
                chunk, randomState, Beardifier.EMPTY, generator.generatorSettings().value(),
                fluidPicker(generator.generatorSettings().value()), emptyBlender
            );
            chunk.getOrCreateNoiseChunk(unused -> noiseChunk);

            // Pre-surface snapshot: record blocks + surface heights before the
            // surface pass so the capture can be proven to have run post-surface.
            JsonObject[] preStates = new JsonObject[sampleCount];
            JsonArray preHeights = new JsonArray();
            int[] preWorldSurface = new int[16 * 16];
            int[] preOceanFloor = new int[16 * 16];
            snapshot(chunk, heightAccessor, preStates, preWorldSurface, preOceanFloor);

            // The real generator pipeline on a real ProtoChunk.
            generator.createBiomes(randomState, emptyBlender, structureManager, chunk).join();
            generator.fillFromNoise(emptyBlender, randomState, structureManager, chunk).join();
            generator.buildSurface(
                chunk,
                new WorldGenerationContext(generator, heightAccessor),
                randomState,
                structureManager,
                biomeManager,
                emptyBlender,
                null
            );

            // Post-surface snapshot + per-column surface biome + change flags.
            JsonObject[] postStates = new JsonObject[sampleCount];
            int[] postWorldSurface = new int[16 * 16];
            int[] postOceanFloor = new int[16 * 16];
            snapshot(chunk, heightAccessor, postStates, postWorldSurface, postOceanFloor);

            JsonObject col = new JsonObject();
            col.addProperty("cx", cx);
            col.addProperty("cz", cz);
            col.addProperty("min-block-x", minBlockX);
            col.addProperty("min-block-z", minBlockZ);

            JsonArray samples = new JsonArray();
            boolean anySurfaceChanged = false;
            boolean anyHeightChanged = false;
            for (int y = MIN_Y, i = 0; y < MAX_Y; y += SAMPLE_STEP, i++) {
                JsonObject s = new JsonObject();
                s.addProperty("y", y);
                s.add("pre", preStates[i]);
                s.add("post", postStates[i]);
                boolean changed = !preStates[i].equals(postStates[i]);
                s.addProperty("changed", changed);
                anySurfaceChanged |= changed;
                samples.add(s);
            }
            col.add("samples", samples);
            col.addProperty("any-surface-changed", anySurfaceChanged);

            JsonArray heights = new JsonArray();
            for (int x = 0; x < 16; x++) {
                for (int z = 0; z < 16; z++) {
                    int preWs = preWorldSurface[x * 16 + z];
                    int postWs = postWorldSurface[x * 16 + z];
                    int preOf = preOceanFloor[x * 16 + z];
                    int postOf = postOceanFloor[x * 16 + z];
                    anyHeightChanged |= (preWs != postWs) || (preOf != postOf);
                    JsonObject h = new JsonObject();
                    h.addProperty("x", x);
                    h.addProperty("z", z);
                    h.addProperty("pre-ws", preWs);
                    h.addProperty("post-ws", postWs);
                    h.addProperty("pre-of", preOf);
                    h.addProperty("post-of", postOf);
                    heights.add(h);
                }
            }
            col.add("heightmap", heights);
            col.addProperty("any-height-changed", anyHeightChanged);

            // The biome the surface pass saw at the top of this column, read
            // back the same way buildSurface reads it (startingHeight = WSH+1).
            int startingHeight = chunk.getHeight(Heightmap.Types.WORLD_SURFACE_WG, 0, 0) + 1;
            Holder<Biome> surfaceBiome = biomeManager.getBiome(
                new net.minecraft.core.BlockPos(minBlockX, startingHeight, minBlockZ)
            );
            col.addProperty("surface-biome", surfaceBiome.unwrapKey().map(ResourceKey::identifier).map(Object::toString).orElse("?"));

            columns.add(col);
        }
        root.add("columns", columns);

        Path outDir = Path.of(output);
        java.nio.file.Files.createDirectories(outDir);
        try (PrintWriter writer = new PrintWriter(outDir.resolve("surface-columns.json").toFile(), "UTF-8")) {
            writer.println(new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(root));
        }
        System.out.println(
            "sampled " + columns.size() + " post-surface columns x " + sampleCount + " y-levels to "
                + outDir.resolve("surface-columns.json")
        );
    }

    /** Replicates NoiseBasedChunkGenerator.createFluidPicker (private in Paper). */
    private static Aquifer.FluidPicker fluidPicker(final NoiseGeneratorSettings settings) {
        Aquifer.FluidStatus lavaStatus = new Aquifer.FluidStatus(-54, Blocks.LAVA.defaultBlockState());
        int seaLevel = settings.seaLevel();
        Aquifer.FluidStatus seaStatus = new Aquifer.FluidStatus(seaLevel, settings.defaultFluid());
        Aquifer.FluidStatus emptyStatus = new Aquifer.FluidStatus(DimensionType.MIN_Y * 2, Blocks.AIR.defaultBlockState());
        return (x, y, z) -> {
            if (SharedConstants.DEBUG_DISABLE_FLUID_GENERATION) {
                return emptyStatus;
            } else {
                return y < Math.min(-54, seaLevel) ? lavaStatus : seaStatus;
            }
        };
    }

    private static void snapshot(
        final ChunkAccess chunk,
        final LevelHeightAccessor heightAccessor,
        final JsonObject[] states,
        final int[] worldSurface,
        final int[] oceanFloor
    ) {
        int sampleCount = states.length;
        for (int y = MIN_Y, i = 0; y < MAX_Y; y += SAMPLE_STEP, i++) {
            states[i] = blockStateJson(chunk.getBlockState(new net.minecraft.core.BlockPos(0, y, 0)));
        }
        // (x, z) -> height for the column at the chunk's own block origin row.
        for (int x = 0; x < 16; x++) {
            for (int z = 0; z < 16; z++) {
                worldSurface[x * 16 + z] = chunk.getHeight(Heightmap.Types.WORLD_SURFACE_WG, x, z);
                oceanFloor[x * 16 + z] = chunk.getHeight(Heightmap.Types.OCEAN_FLOOR_WG, x, z);
            }
        }
    }

    private static JsonObject blockStateJson(final BlockState state) {
        JsonObject o = new JsonObject();
        Block block = state.getBlock();
        o.addProperty("id", Block.BLOCK_STATE_REGISTRY.getId(state));
        o.addProperty("block", net.minecraft.core.registries.BuiltInRegistries.BLOCK.getKey(block).toString());
        o.addProperty("air", state.isAir());
        o.addProperty("fluid-empty", state.getFluidState().isEmpty());
        return o;
    }
}
