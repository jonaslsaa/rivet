import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.PrintStream;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderSet;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.*;
import net.minecraft.world.level.biome.*;
import net.minecraft.world.level.chunk.ChunkGeneratorStructureState;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.levelgen.structure.*;
import net.minecraft.world.level.levelgen.structure.placement.ConcentricRingsStructurePlacement;

/** Actual asynchronous Paper ring generation and actual FixedBiomeSource search.
 * The configuration shell supplies only its nullable stronghold seed field. */
public final class StructureRingProbe {
    @SuppressWarnings("unchecked")
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
                var plains = worldgen.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS);
                var desert = worldgen.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.DESERT);
                var set = worldgen.lookupOrThrow(Registries.STRUCTURE_SET).getOrThrow(BuiltinStructureSets.STRONGHOLDS);
                var ctor = ChunkGeneratorStructureState.class.getDeclaredConstructor(RandomState.class, BiomeSource.class,
                    long.class, long.class, List.class, org.spigotmc.SpigotWorldConfig.class);
                ctor.setAccessible(true);
                var generate = ChunkGeneratorStructureState.class.getDeclaredMethod("generateRingPositions", Holder.class, ConcentricRingsStructurePlacement.class);
                generate.setAccessible(true);
                var unsafeField = sun.misc.Unsafe.class.getDeclaredField("theUnsafe"); unsafeField.setAccessible(true);
                var unsafe = (sun.misc.Unsafe) unsafeField.get(null);
                int[][] configs = {{32, 3, 128}, {0, 0, 1}, {1, 1, 8}, {32, 0, 3}, {1023, 1023, 10}};
                for (long seed : new long[]{0L, 42L, Long.MIN_VALUE}) {
                    var randomState = RandomState.create(worldgen, NoiseGeneratorSettings.OVERWORLD, seed);
                    for (Long override : new Long[]{null, 123456789L}) for (boolean matching : new boolean[]{false, true}) for (int[] config : configs) {
                        var conf = (org.spigotmc.SpigotWorldConfig) unsafe.allocateInstance(org.spigotmc.SpigotWorldConfig.class);
                        var seedField = org.spigotmc.SpigotWorldConfig.class.getDeclaredField("strongholdSeed");
                        seedField.setAccessible(true); seedField.set(conf, override);
                        var source = new FixedBiomeSource(plains);
                        var state = ctor.newInstance(randomState, source, seed, seed, List.of(), conf);
                        var placement = new ConcentricRingsStructurePlacement(config[0], config[1], config[2], HolderSet.direct(matching ? plains : desert));
                        var positions = ((CompletableFuture<List<ChunkPos>>) generate.invoke(state, set, placement)).join();
                        var row = new JsonObject(); row.addProperty("seed", seed); row.addProperty("override", override);
                        row.addProperty("matching", matching); row.addProperty("distance", config[0]); row.addProperty("spread", config[1]); row.addProperty("count", config[2]);
                        var chunks = new JsonArray();
                        for (var pos : positions) { var pair = new JsonArray(); pair.add(pos.x()); pair.add(pos.z()); chunks.add(pair); }
                        row.add("positions", chunks); output.println(row);
                    }
                }
            }
        }
    }
}
