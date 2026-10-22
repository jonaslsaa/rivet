import com.google.gson.*;
import java.io.PrintStream;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.*;
import net.minecraft.world.level.biome.*;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.levelgen.structure.pieces.StructurePieceSerializationContext;

/** Real noise height queries, biome predicate and BuriedTreasureStructure.generate.
 * Does not invoke the server's cancellable structure-spawn event. */
public final class BuriedTreasureGenerationProbe {
    public static void main(String[] args) throws Exception {
        try (var output = new PrintStream(args[0])) {
            SharedConstants.tryDetectVersion(); Bootstrap.bootStrap();
            var packs = net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
            net.minecraft.server.MinecraftServer.configurePackRepository(packs,
                new WorldDataConfiguration(new DataPackConfig(List.of("vanilla"), List.of()), net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS), true, false);
            try (var resources = new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA, packs.openAllSelected())) {
                var layers = net.minecraft.server.RegistryLayer.createRegistryAccess();
                var lookups = net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),
                    net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources, layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
                var worldgen = RegistryDataLoader.load(resources, lookups, RegistryDataLoader.WORLDGEN_REGISTRIES, Runnable::run).join();
                var access = layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN, worldgen).compositeAccess();
                var selected = access.lookupOrThrow(Registries.STRUCTURE).getOrThrow(ResourceKey.create(Registries.STRUCTURE, Identifier.withDefaultNamespace("buried_treasure")));
                var structure = selected.value();
                var settings = access.lookupOrThrow(Registries.NOISE_SETTINGS).getOrThrow(NoiseGeneratorSettings.OVERWORLD);
                var height = LevelHeightAccessor.create(-64, 384);
                var serialization = new StructurePieceSerializationContext(null, access, null);
                for (long seed : new long[]{0L, 42L, -123456789L}) {
                    var random = RandomState.create(access, NoiseGeneratorSettings.OVERWORLD, seed);
                    for (var biomeKey : List.of(Biomes.BEACH, Biomes.PLAINS)) {
                        var biome = access.lookupOrThrow(Registries.BIOME).getOrThrow(biomeKey);
                        var source = new FixedBiomeSource(biome);
                        var generator = new NoiseBasedChunkGenerator(source, settings);
                        for (int[] pos : new int[][]{{0,0},{-1,-1},{4,4},{-12,7},{100,-100}}) {
                            var chunk = new ChunkPos(pos[0], pos[1]);
                            int y = generator.getFirstOccupiedHeight(chunk.getMiddleBlockX(), chunk.getMiddleBlockZ(), Heightmap.Types.OCEAN_FLOOR_WG, height, random);
                            var start = structure.generate(selected, Level.OVERWORLD, access, generator, source, random, null,
                                seed, chunk, 5, height, structure.biomes()::contains);
                            var row = new JsonObject(); row.addProperty("seed", seed); row.addProperty("biome", biomeKey.identifier().toString());
                            row.addProperty("x", pos[0]); row.addProperty("z", pos[1]); row.addProperty("height", y);
                            row.addProperty("valid", start.isValid()); row.addProperty("references", start.getReferences());
                            row.addProperty("saved", start.createTag(serialization, chunk).toString());
                            output.println(row);
                        }
                    }
                }
            }
        }
    }
}
