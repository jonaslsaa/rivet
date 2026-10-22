import com.google.gson.*;
import com.mojang.serialization.JsonOps;
import java.io.*;
import java.lang.reflect.Proxy;
import java.security.MessageDigest;
import java.util.*;
import java.util.function.Predicate;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.Fluid;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.feature.TreeFeature;
import net.minecraft.world.level.levelgen.feature.FeaturePlaceContext;
import net.minecraft.world.level.levelgen.feature.configurations.TreeConfiguration;

/** Full Paper TreeFeature including actual block updateShape and scheduled ticks. */
public final class TreePlacementProbe {
    private static final class Ground {
        final Map<BlockPos, BlockState> states = new HashMap<>();
        final ByteArrayOutputStream writes = new ByteArrayOutputStream();
        final DataOutputStream stream = new DataOutputStream(writes);
        final List<String> ticks = new ArrayList<>();
        final BlockPos origin;
        final XoroshiroRandomSource worldRandom = new XoroshiroRandomSource(99);
        int writeCount;
        Ground(BlockPos origin) { this.origin = origin; }
        BlockState get(BlockPos p) {
            return states.getOrDefault(p, p.getY() < origin.getY() ? Blocks.DIRT.defaultBlockState() : Blocks.AIR.defaultBlockState());
        }
        int height(Heightmap.Types type, int x, int z) {
            for (int y = 319; y >= -64; --y) {
                if (type.isOpaque().test(get(new BlockPos(x, y, z)))) return y + 1;
            }
            return -64;
        }
        @SuppressWarnings("unchecked")
        WorldGenLevel level() {
            return (WorldGenLevel) Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(), new Class<?>[]{WorldGenLevel.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getBlockState" -> get((BlockPos) args[0]);
                    case "getFluidState" -> get((BlockPos) args[0]).getFluidState();
                    case "isEmptyBlock" -> get((BlockPos) args[0]).isAir();
                    case "isStateAtPosition" -> ((Predicate<BlockState>) args[1]).test(get((BlockPos) args[0]));
                    case "isFluidAtPosition" -> ((Predicate<net.minecraft.world.level.material.FluidState>) args[1]).test(get((BlockPos) args[0]).getFluidState());
                    case "setBlock" -> {
                        BlockPos p = ((BlockPos) args[0]).immutable(); BlockState state = (BlockState) args[1];
                        states.put(p, state); writeCount++;
                        for (int value : new int[]{p.getX(),p.getY(),p.getZ(),Block.getId(state),(int) args[2]}) stream.writeInt(value);
                        yield true;
                    }
                    case "scheduleTick" -> {
                        BlockPos p = (BlockPos) args[0];
                        String kind = args[1] instanceof Block b ? BuiltInRegistries.BLOCK.getKey(b).toString()
                            : BuiltInRegistries.FLUID.getKey((Fluid) args[1]).toString();
                        ticks.add(p.getX()+","+p.getY()+","+p.getZ()+","+kind+","+args[2]);
                        yield null;
                    }
                    case "getRandom" -> worldRandom;
                    case "getHeight" -> args == null || args.length == 0 ? 384
                        : height((Heightmap.Types) args[0], (int) args[1], (int) args[2]);
                    case "getHeightmapPos" -> {
                        BlockPos p = (BlockPos) args[1];
                        yield new BlockPos(p.getX(), height((Heightmap.Types) args[0], p.getX(), p.getZ()), p.getZ());
                    }
                    case "getMinY" -> -64;
                    case "getMaxY" -> 319;
                    case "getSeaLevel" -> 63;
                    case "isInsideBuildHeight" -> { int y = args[0] instanceof BlockPos p ? p.getY() : (int) args[0]; yield y >= -64 && y <= 319; }
                    default -> throw new UnsupportedOperationException(method.toString());
                });
        }
        String writesHash() throws Exception { return hash(writes.toByteArray()); }
        String blocksHash() throws Exception {
            var bytes = new ByteArrayOutputStream(); var out = new DataOutputStream(bytes);
            var positions = new ArrayList<>(states.keySet());
            positions.sort(Comparator.<BlockPos>comparingInt(p -> p.getX()).thenComparingInt(p -> p.getY()).thenComparingInt(p -> p.getZ()));
            for (var p : positions) for (int value : new int[]{p.getX(),p.getY(),p.getZ(),Block.getId(states.get(p))}) out.writeInt(value);
            return hash(bytes.toByteArray());
        }
        String ticksHash() throws Exception { return hash(String.join("\n", ticks).getBytes(java.nio.charset.StandardCharsets.UTF_8)); }
    }
    private static String hash(byte[] bytes) throws Exception { return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes)); }
    public static void main(String[] args) {
        var errors = System.err;
        try { run(); } catch (Throwable error) { error.printStackTrace(errors); System.exit(1); }
    }
    private static void run() throws Exception {
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
            for (String name : List.of("oak", "fancy_oak", "birch", "acacia", "jungle_bush", "pine", "spruce", "azalea_tree",
                    "swamp_oak", "jungle_tree", "mega_jungle_tree", "mega_pine", "mega_spruce",
                    "oak_leaf_litter", "birch_leaf_litter", "dark_oak_leaf_litter", "fancy_oak_leaf_litter")) {
                JsonObject json;
                try (var reader = resources.getResourceOrThrow(Identifier.withDefaultNamespace("worldgen/configured_feature/" + name + ".json")).openAsReader()) {
                    json = JsonParser.parseReader(reader).getAsJsonObject();
                }
                var config = TreeConfiguration.CODEC.parse(JsonOps.INSTANCE, json.get("config")).getOrThrow();
                if (config.rootPlacer.isPresent()) throw new AssertionError("unsupported roots in " + name);
                for (long seed : new long[]{0, 1, 42, 987654321}) {
                    BlockPos origin = new BlockPos(0, 64, 0);
                    Ground ground = new Ground(origin); var random = new XoroshiroRandomSource(seed);
                    boolean result = new TreeFeature(TreeConfiguration.CODEC).place(
                        new FeaturePlaceContext<>(Optional.empty(), ground.level(), null, random, origin, config));
                    JsonObject row = new JsonObject(); row.addProperty("feature", name); row.addProperty("seed", seed);
                    row.add("config", json.get("config"));
                    row.addProperty("result", result); row.addProperty("next_long", random.nextLong());
                    row.addProperty("world_next_long", ground.worldRandom.nextLong());
                    row.addProperty("writes", ground.writeCount); row.addProperty("writes_sha256", ground.writesHash());
                    row.addProperty("blocks_sha256", ground.blocksHash()); row.addProperty("ticks", ground.ticks.size());
                    row.addProperty("ticks_sha256", ground.ticksHash());
                    output.println("TREE_PLACEMENT\t" + row);
                }
            }
        }
    }
}
