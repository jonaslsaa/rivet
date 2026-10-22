import com.google.gson.*;
import com.mojang.serialization.JsonOps;
import java.lang.reflect.Proxy;
import java.util.*;
import java.util.function.Predicate;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.levelgen.feature.FallenTreeFeature;
import net.minecraft.world.level.levelgen.feature.FeaturePlaceContext;
import net.minecraft.world.level.levelgen.feature.configurations.FallenTreeConfiguration;

/** Actual datapack-configured Paper fallen trees; the proxy only provides world storage. */
public final class FallenTreeProbe {
    private static final class Ground {
        final Map<BlockPos, BlockState> states = new HashMap<>();
        final JsonArray writes = new JsonArray();
        final String terrain;
        final BlockPos origin;
        Ground(String terrain, BlockPos origin) { this.terrain = terrain; this.origin = origin; }
        BlockState get(BlockPos p) {
            return states.getOrDefault(p, switch (terrain) {
                case "void" -> Blocks.AIR.defaultBlockState();
                case "wall" -> p.getY() < origin.getY() || Math.abs(p.getX() - origin.getX()) >= 2 || Math.abs(p.getZ() - origin.getZ()) >= 2
                    ? Blocks.STONE.defaultBlockState() : Blocks.AIR.defaultBlockState();
                default -> p.getY() < origin.getY() ? Blocks.DIRT.defaultBlockState() : Blocks.AIR.defaultBlockState();
            });
        }
        @SuppressWarnings("unchecked")
        WorldGenLevel level() {
            return (WorldGenLevel) Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(), new Class<?>[]{WorldGenLevel.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getBlockState" -> get((BlockPos) args[0]);
                    case "isEmptyBlock" -> get((BlockPos) args[0]).isAir();
                    case "isStateAtPosition" -> ((Predicate<BlockState>) args[1]).test(get((BlockPos) args[0]));
                    case "setBlock" -> {
                        BlockPos pos = ((BlockPos) args[0]).immutable();
                        BlockState state = (BlockState) args[1];
                        states.put(pos, state);
                        JsonArray write = new JsonArray();
                        write.add(pos.getX()); write.add(pos.getY()); write.add(pos.getZ());
                        write.add(Block.getId(state)); write.add((int) args[2]);
                        writes.add(write);
                        yield true;
                    }
                    case "getHeight" -> 384;
                    case "getMinY" -> -64;
                    default -> throw new UnsupportedOperationException(method.toString());
                });
        }
    }
    public static void main(String[] args) throws Exception {
        var output = System.out;
        SharedConstants.tryDetectVersion(); Bootstrap.bootStrap();
        var packs = net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
        net.minecraft.server.MinecraftServer.configurePackRepository(packs,
            new net.minecraft.world.level.WorldDataConfiguration(
                new net.minecraft.world.level.DataPackConfig(List.of("vanilla"), List.of()),
                net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS), true, false);
        try (var resources = new net.minecraft.server.packs.resources.MultiPackResourceManager(
                net.minecraft.server.packs.PackType.SERVER_DATA, packs.openAllSelected())) {
            var layers = net.minecraft.server.RegistryLayer.createRegistryAccess();
            net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,
                layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)).forEach(pending -> pending.apply());
            for (String name : List.of("fallen_oak_tree", "fallen_birch_tree", "fallen_super_birch_tree", "fallen_jungle_tree", "fallen_spruce_tree")) {
                JsonObject json;
                try (var reader = resources.getResourceOrThrow(Identifier.withDefaultNamespace("worldgen/configured_feature/" + name + ".json")).openAsReader()) {
                    json = JsonParser.parseReader(reader).getAsJsonObject();
                }
                var config = FallenTreeConfiguration.CODEC.parse(JsonOps.INSTANCE, json.get("config")).getOrThrow();
                for (long seed : new long[]{0, 1, 42, 987654321}) {
                    for (BlockPos origin : List.of(new BlockPos(0, 64, 0), new BlockPos(-17, 70, 31))) {
                        for (String terrain : List.of("flat", "void")) {
                            Ground ground = new Ground(terrain, origin);
                            var random = new XoroshiroRandomSource(seed);
                            boolean result = new FallenTreeFeature(FallenTreeConfiguration.CODEC).place(
                                new FeaturePlaceContext<>(Optional.empty(), ground.level(), null, random, origin, config));
                            JsonObject row = new JsonObject();
                            row.addProperty("feature", name); row.addProperty("seed", seed); row.addProperty("terrain", terrain);
                            JsonArray xyz = new JsonArray(); xyz.add(origin.getX()); xyz.add(origin.getY()); xyz.add(origin.getZ());
                            row.add("origin", xyz); row.addProperty("result", result); row.addProperty("next_long", random.nextLong());
                            row.add("writes", ground.writes);
                            output.println("FALLEN_TREE\t" + row);
                        }
                    }
                }
            }
        }
    }
}
