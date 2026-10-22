import java.lang.reflect.Proxy;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.LevelReader;
import net.minecraft.world.level.block.state.BlockState;

/** Run the real Paper canSurvive implementations on fixed supporting blocks. */
public final class PlantSurvivalProbe {
    public static void main(String[] args) {
        java.io.PrintStream output = System.out;
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        var packs = net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
        net.minecraft.server.MinecraftServer.configurePackRepository(packs,
            new net.minecraft.world.level.WorldDataConfiguration(
                new net.minecraft.world.level.DataPackConfig(java.util.List.of("minecraft", "vanilla"), java.util.List.of()),
                net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS), true, false);
        var resources = new net.minecraft.server.packs.resources.MultiPackResourceManager(
            net.minecraft.server.packs.PackType.SERVER_DATA, packs.openAllSelected());
        var layers = net.minecraft.server.RegistryLayer.createRegistryAccess();
        net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,
            layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)).forEach(pending -> pending.apply());
        String[] plants = {"kelp", "kelp_plant", "bamboo", "bamboo_sapling", "sea_pickle"};
        String[] supports = {"stone", "magma_block", "kelp", "kelp_plant", "water", "air",
            "dirt", "sand", "bamboo", "oak_slab", "oak_fence", "soul_sand", "ice"};
        BlockPos pos = new BlockPos(0, 64, 0);
        for (String support : supports) {
            BlockState below = BuiltInRegistries.BLOCK.getValue(Identifier.withDefaultNamespace(support)).defaultBlockState();
            LevelReader level = (LevelReader) Proxy.newProxyInstance(
                LevelReader.class.getClassLoader(), new Class<?>[]{LevelReader.class},
                (proxy, method, arguments) -> switch (method.getName()) {
                    case "getBlockState" -> below;
                    case "getFluidState" -> below.getFluidState();
                    case "getHeight" -> 384;
                    case "getMinY" -> -64;
                    default -> throw new UnsupportedOperationException(method.toString());
                });
            for (String plant : plants) {
                BlockState state = BuiltInRegistries.BLOCK.getValue(Identifier.withDefaultNamespace(plant)).defaultBlockState();
                output.println("PLANT_SURVIVAL\t" + plant + "\t" + support + "\t" + state.canSurvive(level, pos));
            }
        }
        resources.close();
    }
}
