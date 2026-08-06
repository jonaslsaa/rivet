import java.util.HashMap;
import java.util.Map;
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
 *   3. each block's default state id falls inside its range;
 *   4. representative anchor states match the codegen's golden probes
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
        for (Block block : BuiltInRegistries.BLOCK) {
            java.util.List<BlockState> states = block.getStateDefinition().getPossibleStates();
            int base = Block.getId(states.get(0));
            int count = states.size();
            require(base == expectedBase,
                "block " + BuiltInRegistries.BLOCK.getKey(block) + " base " + base
                    + " != expected " + expectedBase);
            // Contiguous within the block, in getPossibleStates() order.
            for (int i = 0; i < count; i++) {
                require(Block.getId(states.get(i)) == base + i,
                    "block " + BuiltInRegistries.BLOCK.getKey(block) + " state " + i
                        + " id " + Block.getId(states.get(i)) + " != " + (base + i));
            }
            // Default state inside the range.
            int def = Block.getId(block.defaultBlockState());
            require(def >= base && def < base + count,
                "block " + BuiltInRegistries.BLOCK.getKey(block) + " default " + def
                    + " outside [" + base + ", " + (base + count) + ")");
            expectedBase = base + count;
        }
        require(expectedBase == size,
            "block ranges end at " + expectedBase + " != registry size " + size);

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
