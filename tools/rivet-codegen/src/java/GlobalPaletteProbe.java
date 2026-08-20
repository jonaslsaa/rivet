import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Comparator;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.RedStoneWireBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.AttachFace;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.level.block.state.properties.ChestType;
import net.minecraft.world.level.block.state.properties.Property;
import net.minecraft.world.level.block.state.properties.RedstoneSide;

/**
 * Probes the real Paper block-state registry (`Block.BLOCK_STATE_REGISTRY`) —
 * the exact table `GlobalPalette`/`PalettedContainer` index on the wire — and
 * cross-checks it against the codegen's expected global ids (issue #154).
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" GlobalPaletteProbe
 *
 * Verifies, against a live boot of the pinned Paper 26.2 jar:
 *   1. the registry has 32366 states (the codegen's BLOCK_STATE_COUNT);
 *   2. Paper assigns states block by block in registry order, each block a
 *      contiguous range, partitioning 0..32366 without gaps or overlap;
 *   3. every state's id, block, default marker, and serialized properties feed
 *      the complete cross-language digest checked by the Rust caller;
 *   4. each block's default state id falls inside its range;
 *   5. representative anchor states match the codegen's golden probes
 *      (air=0, acacia_button wall/north/false=10780, redstone_wire 4011..5306,
 *      chest single/north/true=3987).
 *
 * Prints `key=value` probe lines followed by `PROBE OK` on success; exits
 * nonzero (throwing) on any mismatch.
 */
public final class GlobalPaletteProbe {
    private GlobalPaletteProbe() {}

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // 1. Total size — the global palette's in-memory width (ceillog2 -> 15).
        int size = Block.BLOCK_STATE_REGISTRY.size();
        require(size == 32366, "registry size " + size + " != 32366");
        println("count=" + size);

        // 2. Paper's assignment is block-by-block in registry order, each block
        // a contiguous range, partitioning 0..size with no gaps/overlap. Walk
        // the registry and verify every state id falls exactly where the block
        // range math says it must.
        int expectedBase = 0;
        MessageDigest stateDigest = sha256();
        for (Block block : BuiltInRegistries.BLOCK) {
            List<BlockState> states = block.getStateDefinition().getPossibleStates();
            int base = Block.getId(states.get(0));
            int count = states.size();
            String blockName = BuiltInRegistries.BLOCK.getKey(block).toString();
            require(base == expectedBase,
                "block " + blockName + " base " + base + " != expected " + expectedBase);
            // Contiguous within the block, in getPossibleStates() order. Every
            // state also contributes to the complete cross-language digest:
            // id, block name, default marker, and every serialized property.
            for (int i = 0; i < count; i++) {
                BlockState state = states.get(i);
                int id = Block.getId(state);
                require(id == base + i,
                    "block " + blockName + " state " + i
                        + " id " + id + " != " + (base + i));
                updateStateDigest(
                    stateDigest,
                    id,
                    blockName,
                    state == block.defaultBlockState(),
                    state);
            }
            // Default state inside the range.
            int def = Block.getId(block.defaultBlockState());
            require(def >= base && def < base + count,
                "block " + blockName + " default " + def
                    + " outside [" + base + ", " + (base + count) + ")");
            expectedBase = base + count;
        }
        require(expectedBase == size,
            "block ranges end at " + expectedBase + " != registry size " + size);
        println("state_digest_sha256=" + hex(stateDigest.digest()));

        // 3. Representative anchors the codegen bakes into its golden probes.
        int air = Block.getId(Blocks.AIR.defaultBlockState());
        require(air == 0, "air id " + air + " != 0");
        println("air=" + air);

        int button = Block.getId(
            Blocks.ACACIA_BUTTON.defaultBlockState()
                .setValue(BlockStateProperties.ATTACH_FACE, AttachFace.WALL)
                .setValue(BlockStateProperties.HORIZONTAL_FACING, Direction.NORTH)
                .setValue(BlockStateProperties.POWERED, false));
        require(button == 10780, "acacia_button wall/north/false id " + button + " != 10780");
        println("acacia_button_default=" + button);

        int chest = Block.getId(
            Blocks.CHEST.defaultBlockState()
                .setValue(BlockStateProperties.CHEST_TYPE, ChestType.SINGLE)
                .setValue(BlockStateProperties.HORIZONTAL_FACING, Direction.NORTH)
                .setValue(BlockStateProperties.WATERLOGGED, true));
        require(chest == 3987, "chest single/north/true id " + chest + " != 3987");
        println("chest_single_north_true=" + chest);

        int wireFirst = Block.getId(
            Blocks.REDSTONE_WIRE.defaultBlockState()
                .setValue(RedStoneWireBlock.NORTH, RedstoneSide.UP)
                .setValue(RedStoneWireBlock.EAST, RedstoneSide.UP)
                .setValue(RedStoneWireBlock.SOUTH, RedstoneSide.UP)
                .setValue(RedStoneWireBlock.WEST, RedstoneSide.UP)
                .setValue(RedStoneWireBlock.POWER, 0));
        require(wireFirst == 4011, "redstone_wire first id " + wireFirst + " != 4011");
        println("redstone_wire_first=" + wireFirst);

        int wireLast = Block.getId(
            Blocks.REDSTONE_WIRE.defaultBlockState()
                .setValue(RedStoneWireBlock.NORTH, RedstoneSide.NONE)
                .setValue(RedStoneWireBlock.EAST, RedstoneSide.NONE)
                .setValue(RedStoneWireBlock.SOUTH, RedstoneSide.NONE)
                .setValue(RedStoneWireBlock.WEST, RedstoneSide.NONE)
                .setValue(RedStoneWireBlock.POWER, 15));
        require(wireLast == 5306, "redstone_wire last id " + wireLast + " != 5306");
        println("redstone_wire_last=" + wireLast);

        // Reverse direction: the codegen's block_of AIR fallback for a missing id.
        BlockState missing = Block.stateById(size);
        require(missing == Blocks.AIR.defaultBlockState(),
            "stateById(" + size + ") -> " + missing + " != AIR default");
        println("reverse_missing_is_air=1");

        println("PROBE OK");
    }

    private static MessageDigest sha256() {
        try {
            return MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException error) {
            throw new IllegalStateException("SHA-256 unavailable", error);
        }
    }

    private static void updateStateDigest(
            MessageDigest digest,
            int id,
            String blockName,
            boolean isDefault,
            BlockState state) {
        List<Property.Value<?>> properties = state.getValues()
            .sorted(Comparator.comparing(value -> value.property().getName()))
            .toList();

        StringBuilder line = new StringBuilder();
        line.append("id=").append(id)
            .append('\t').append("block=").append(blockName)
            .append('\t').append("default=").append(isDefault ? '1' : '0')
            .append('\t').append("properties=");
        for (int i = 0; i < properties.size(); i++) {
            if (i != 0) {
                line.append(',');
            }
            Property.Value<?> entry = properties.get(i);
            line.append(entry.property().getName())
                .append('=')
                .append(entry.valueName());
        }
        line.append('\n');
        digest.update(line.toString().getBytes(StandardCharsets.UTF_8));
    }

    private static String hex(byte[] bytes) {
        char[] alphabet = "0123456789abcdef".toCharArray();
        char[] output = new char[bytes.length * 2];
        for (int i = 0; i < bytes.length; i++) {
            int value = bytes[i] & 0xff;
            output[i * 2] = alphabet[value >>> 4];
            output[i * 2 + 1] = alphabet[value & 0x0f];
        }
        return new String(output);
    }

    private static void println(String line) {
        // Bootstrap.bootStrap() wraps System.out in a logging stream that
        // log4j-off swallows; realStdoutPrintln writes to the captured console.
        net.minecraft.server.Bootstrap.realStdoutPrintln(line);
    }

    private static void require(boolean condition, String message) {
        if (!condition) {
            throw new IllegalStateException("PROBE FAILED: " + message);
        }
    }
}
