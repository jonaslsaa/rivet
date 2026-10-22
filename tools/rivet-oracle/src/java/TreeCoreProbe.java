import java.lang.reflect.*;
import java.util.*;
import java.util.function.Predicate;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.LevelAccessor;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.level.levelgen.feature.TreeFeature;
import net.minecraft.world.level.levelgen.feature.configurations.TreeConfiguration;
import net.minecraft.world.level.levelgen.feature.featuresize.TwoLayersFeatureSize;
import net.minecraft.world.level.levelgen.feature.foliageplacers.BlobFoliagePlacer;
import net.minecraft.world.level.levelgen.feature.stateproviders.BlockStateProvider;
import net.minecraft.world.level.levelgen.feature.trunkplacers.StraightTrunkPlacer;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.phys.shapes.DiscreteVoxelShape;
import net.minecraft.util.valueproviders.ConstantInt;

/** Calls real private TreeFeature helpers. The proxy only stores world states. */
public final class TreeCoreProbe {
    private record Clearance(String name, int x, int y, int z, String block, boolean ignoreVines) {}

    private static BlockState state(String name) {
        return BuiltInRegistries.BLOCK.getValue(Identifier.withDefaultNamespace(name)).defaultBlockState();
    }

    @SuppressWarnings("unchecked")
    private static WorldGenLevel world(Map<BlockPos, BlockState> states) {
        return (WorldGenLevel) Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(), new Class<?>[]{WorldGenLevel.class},
            (proxy, method, arguments) -> switch (method.getName()) {
                case "getBlockState" -> states.getOrDefault((BlockPos) arguments[0], Blocks.AIR.defaultBlockState());
                case "isStateAtPosition" -> ((Predicate<BlockState>) arguments[1]).test(states.getOrDefault((BlockPos) arguments[0], Blocks.AIR.defaultBlockState()));
                case "setBlock" -> {
                    if ((int) arguments[2] != 19) throw new AssertionError("Tree leaf update flags must be 19");
                    states.put(((BlockPos) arguments[0]).immutable(), (BlockState) arguments[1]);
                    yield true;
                }
                case "getHeight" -> 384;
                case "getMinY" -> -64;
                default -> throw new UnsupportedOperationException(method.toString());
            });
    }

    public static void main(String[] args) throws Exception {
        var output = System.out;
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        var packs = net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
        net.minecraft.server.MinecraftServer.configurePackRepository(packs,
            new net.minecraft.world.level.WorldDataConfiguration(
                new net.minecraft.world.level.DataPackConfig(List.of("minecraft", "vanilla"), List.of()),
                net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS), true, false);
        try (var resources = new net.minecraft.server.packs.resources.MultiPackResourceManager(
                net.minecraft.server.packs.PackType.SERVER_DATA, packs.openAllSelected())) {
            var layers = net.minecraft.server.RegistryLayer.createRegistryAccess();
            net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,
                layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)).forEach(pending -> pending.apply());
            Method clearance = TreeFeature.class.getDeclaredMethod("getMaxFreeTreeHeight", WorldGenLevel.class, int.class, BlockPos.class, TreeConfiguration.class);
            clearance.setAccessible(true);
            TreeFeature tree = new TreeFeature(TreeConfiguration.CODEC);
            Clearance[] cases = {
                new Clearance("side_clip", 1, 4, 0, "stone", false),
                new Clearance("outside_lower_radius", 1, 0, 0, "stone", false),
                new Clearance("base_blocked", 0, 0, 0, "stone", false),
                new Clearance("extra_top_layer", 0, 9, 0, "stone", false),
                new Clearance("outside_upper_radius", 2, 4, 0, "stone", false),
                new Clearance("leaves_replaceable", 0, 4, 0, "oak_leaves", false),
                new Clearance("vine_blocks", 0, 4, 0, "vine", false),
                new Clearance("vine_ignored", 0, 4, 0, "vine", true),
                new Clearance("water_replaceable", 0, 4, 0, "water", false)
            };
            for (Clearance c : cases) {
                var states = new HashMap<BlockPos, BlockState>();
                states.put(new BlockPos(c.x, 1 + c.y, c.z), state(c.block));
                var builder = new TreeConfiguration.TreeConfigurationBuilder(
                    BlockStateProvider.simple(Blocks.OAK_LOG), new StraightTrunkPlacer(8, 0, 0),
                    BlockStateProvider.simple(Blocks.OAK_LEAVES), new BlobFoliagePlacer(ConstantInt.of(2), ConstantInt.of(0), 3),
                    new TwoLayersFeatureSize(1, 0, 1), BlockStateProvider.simple(Blocks.DIRT));
                if (c.ignoreVines) builder.ignoreVines();
                int result = (int) clearance.invoke(tree, world(states), 8, new BlockPos(0, 1, 0), builder.build());
                output.println("TREE_CORE\tclearance\t" + c.name + "\t8\t" + c.x + "\t" + c.y + "\t" + c.z + "\t" + c.block + "\t" + c.ignoreVines + "\t" + result);
            }
            Method leaves = TreeFeature.class.getDeclaredMethod("updateLeaves", LevelAccessor.class, BoundingBox.class, Set.class, Set.class, Set.class);
            leaves.setAccessible(true);
            for (String name : List.of("normal", "priority", "extra_log", "decoration", "root", "clipped")) {
                var states = new HashMap<BlockPos, BlockState>();
                BlockPos log = new BlockPos(0, 1, 0), second = new BlockPos(2, 1, 0);
                states.put(log, Blocks.OAK_LOG.defaultBlockState());
                for (int x = 1; x <= 8; ++x) states.put(new BlockPos(x, 1, 0), Blocks.OAK_LEAVES.defaultBlockState());
                if (name.equals("priority")) states.put(second, Blocks.OAK_LEAVES.defaultBlockState().setValue(BlockStateProperties.DISTANCE, 1));
                if (name.equals("extra_log")) states.put(second, Blocks.OAK_LOG.defaultBlockState());
                if (name.equals("decoration") || name.equals("root")) states.put(second, Blocks.STONE.defaultBlockState());
                int maxX = name.equals("clipped") ? 3 : 8;
                DiscreteVoxelShape shape = (DiscreteVoxelShape) leaves.invoke(null, world(states), new BoundingBox(0, 1, 0, maxX, 1, 0), Set.of(log),
                    name.equals("decoration") ? Set.of(second) : Set.of(), name.equals("root") ? Set.of(second) : Set.of());
                var distances = new ArrayList<String>();
                var filled = new ArrayList<String>();
                for (int x = 0; x <= 8; ++x) {
                    BlockState s = states.get(new BlockPos(x, 1, 0));
                    distances.add(s.hasProperty(BlockStateProperties.DISTANCE) ? s.getValue(BlockStateProperties.DISTANCE).toString() : "-");
                    if (x <= maxX && shape.isFull(x, 0, 0)) filled.add(Integer.toString(x));
                }
                output.println("TREE_CORE\tleaves\t" + name + "\t" + maxX + "\t" + String.join(",", distances) + "\t" + String.join(",", filled));
            }
        }
    }
}
