import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.PrintWriter;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.SupportType;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.MapColor;

/**
 * Dumps the compact per-{@code StateId} worldgen/heightmap/lighting behavior
 * and full-face support/attachment tables from the real Paper 26.2 block-state registry
 * (issue #228).
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" BlockBehaviourProbe --output block_behaviors.json --version 26.2
 *
 * `Bootstrap.bootStrap()` runs `Blocks` static init, which fills
 * {@link Block#BLOCK_STATE_REGISTRY} and {@code initCache()}s every state
 * against `EmptyBlockGetter.INSTANCE` + `BlockPos.ZERO` (see `Blocks.java`).
 * Every behavior below is therefore baked into the state object by then — none
 * reads the world — which is exactly the "pure table ops, no world types"
 * surface the registry's `BlockState` newtype exposes downstream.
 *
 * For each id in 0..state_count the probe evaluates the behavior fields through
 * the state's real cached accessors:
 *   isAir, blocksMotion, isSolidRender, canOcclude, useShapeForLightOcclusion,
 *   propagatesSkylightDown, getLightDampening, getLightEmission,
 *   isRandomlyTicking, fluidState.isEmpty, and getMapColor(...).id
 * and packs them into a 32-bit word (bits documented below). It separately
 * evaluates {@code SupportType.FULL.isSupporting} directly for all six
 * directions against {@code EmptyBlockGetter} at {@code BlockPos.ZERO}; FULL
 * delegates to {@code getBlockSupportShape}. It also evaluates the full-face
 * collision predicate used by {@code MultifaceBlock.canAttachTo}. Those
 * direction bits are emitted as separate mask tables. All tables are
 * run-length encoded in id order, so the committed fixture stays small and the
 * output is byte-deterministic across runs (the probe's iteration order is
 * fixed). Anchors are printed as key=value lines + `PROBE OK` on success; the
 * fixture itself carries only the RLE runs.
 *
 * Bit layout (shared with the Rust generator and the BlockState newtype):
 *   bit  0  is_air
 *   bit  1  blocks_motion
 *   bit  2  solid_render (isSolidRender)
 *   bit  3  can_occlude
 *   bit  4  use_shape_for_light_occlusion
 *   bit  5  propagates_skylight_down
 *   bit  6  random_ticking (isRandomlyTicking)
 *   bit  7  fluid_empty (!getFluidState().isEmpty() == false)
 *   bits  8-11   light_dampening (0..15)
 *   bits  12-15  light_emission (0..15)
 *   bits  16-21  map_color_id (0..63)
 *   bit  22 is_solid (isSolid() — the cached legacySolid from calculateSolid():
 *            non-empty collision-shape bounds volume >= 35/48 or ysize >= 1.0,
 *            after the forceSolidOn/Off and dynamic-shape guards; NOT hasCollision)
 *   bit  23 can_be_replaced (canBeReplaced() — Properties.replaceable)
 *   bits  24-26  fluid_id (BuiltInRegistries.FLUID.getId(getFluidState().getType()), 0..4)
 *   bits  27-31  reserved (must be 0)
 */
public final class BlockBehaviourProbe {
    private BlockBehaviourProbe() {}

    public static void main(String[] args) throws Exception {
        String output = null;
        String version = null;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--output" -> output = args[++i];
                case "--version" -> version = args[++i];
                default -> throw new IllegalArgumentException("Unknown arg: " + args[i]);
            }
        }
        if (output == null || version == null) {
            throw new IllegalArgumentException(
                "Usage: BlockBehaviourProbe --output <file.json> --version <mc>");
        }

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        final int count = Block.BLOCK_STATE_REGISTRY.size();
        require(count == 32366, "registry size " + count + " != 32366");
        println("state_count=" + count);

        // Run-length encode the per-state words and full-face support masks in
        // id order. `stateById` is dense 0..count (stateById returns AIR
        // default past the end, so the probe only iterates the valid range).
        JsonArray runs = new JsonArray();
        JsonArray faceSturdyRuns = new JsonArray();
        JsonArray collisionFaceRuns = new JsonArray();
        JsonArray dynamicShapeRuns = new JsonArray();
        int runStart = 0;
        BlockState firstState = Block.stateById(0);
        require(Block.getId(firstState) == 0, "state 0 resolves to " + Block.getId(firstState));
        require(Block.stateById(Block.getId(firstState)) == firstState, "state 0 is not identity-stable");
        long runWord = behaviorWord(firstState);
        int faceSturdyRunStart = 0;
        int faceSturdyMask = faceSturdyMask(firstState);
        int collisionFaceRunStart = 0;
        int collisionFaceMask = collisionFaceMask(firstState);
        int dynamicShapeRunStart = 0;
        boolean dynamicShape = hasDynamicShape(firstState);
        int runCount = 0;
        int faceSturdyRunCount = 0;
        int collisionFaceRunCount = 0;
        for (int id = 1; id < count; id++) {
            BlockState state = Block.stateById(id);
            require(Block.getId(state) == id, "state " + id + " resolves to " + Block.getId(state));
            require(Block.stateById(Block.getId(state)) == state, "state " + id + " is not identity-stable");
            long word = behaviorWord(state);
            if (word != runWord) {
                runs.add(run(runStart, id - runStart, runWord));
                runCount++;
                runStart = id;
                runWord = word;
            }
            int mask = faceSturdyMask(state);
            if (mask != faceSturdyMask) {
                faceSturdyRuns.add(maskRun(faceSturdyRunStart, id - faceSturdyRunStart, faceSturdyMask));
                faceSturdyRunCount++;
                faceSturdyRunStart = id;
                faceSturdyMask = mask;
            }
            int collisionMask = collisionFaceMask(state);
            if (collisionMask != collisionFaceMask) {
                collisionFaceRuns.add(maskRun(collisionFaceRunStart, id - collisionFaceRunStart, collisionFaceMask));
                collisionFaceRunCount++;
                collisionFaceRunStart = id;
                collisionFaceMask = collisionMask;
            }
            boolean dynamic = hasDynamicShape(state);
            if (dynamic != dynamicShape) {
                dynamicShapeRuns.add(boolRun(dynamicShapeRunStart, id - dynamicShapeRunStart, dynamicShape));
                dynamicShapeRunStart = id;
                dynamicShape = dynamic;
            }
        }
        runs.add(run(runStart, count - runStart, runWord));
        runCount++;
        faceSturdyRuns.add(maskRun(faceSturdyRunStart, count - faceSturdyRunStart, faceSturdyMask));
        faceSturdyRunCount++;
        collisionFaceRuns.add(maskRun(collisionFaceRunStart, count - collisionFaceRunStart, collisionFaceMask));
        collisionFaceRunCount++;
        dynamicShapeRuns.add(boolRun(dynamicShapeRunStart, count - dynamicShapeRunStart, dynamicShape));
        println("run_count=" + runCount);
        println("face_sturdy_run_count=" + faceSturdyRunCount);
        println("collision_face_run_count=" + collisionFaceRunCount);
        println("dynamic_shape_state_count=" + dynamicShapeCount(dynamicShapeRuns));

        JsonObject root = new JsonObject();
        root.addProperty("generator",
            "BlockBehaviourProbe (Bootstrap + Block.BLOCK_STATE_REGISTRY)");
        root.addProperty("minecraft_version", version);
        root.addProperty("state_count", count);
        root.add("runs", runs);
        root.add("face_sturdy_runs", faceSturdyRuns);
        root.add("collision_face_runs", collisionFaceRuns);
        root.add("dynamic_shape_runs", dynamicShapeRuns);
        Gson gson = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();
        try (PrintWriter writer = new PrintWriter(output, "UTF-8")) {
            gson.toJson(root, writer);
        }

        // Readable anchors for the gate log (the fixture words are re-derived
        // by the generator; these just document the source of truth).
        println("air=" + behaviorWord(Block.stateById(Block.getId(Blocks.AIR.defaultBlockState()))));
        println("stone=" + behaviorWord(Block.stateById(Block.getId(Blocks.STONE.defaultBlockState()))));
        println("water=" + behaviorWord(Block.stateById(Block.getId(Blocks.WATER.defaultBlockState()))));
        println("lava=" + behaviorWord(Block.stateById(Block.getId(Blocks.LAVA.defaultBlockState()))));
        println("oak_leaves=" + behaviorWord(Block.stateById(Block.getId(Blocks.OAK_LEAVES.defaultBlockState()))));
        println("glass=" + behaviorWord(Block.stateById(Block.getId(Blocks.GLASS.defaultBlockState()))));
        println("torch=" + behaviorWord(Block.stateById(Block.getId(Blocks.TORCH.defaultBlockState()))));
        println("stone_face_sturdy_mask=" + faceSturdyMask(Blocks.STONE.defaultBlockState()));
        println("oak_slab_face_sturdy_mask=" + faceSturdyMask(Blocks.OAK_SLAB.defaultBlockState()));
        println("oak_leaves_collision_face_mask=" + collisionFaceMask(Blocks.OAK_LEAVES.defaultBlockState()));
        println("glass_collision_face_mask=" + collisionFaceMask(Blocks.GLASS.defaultBlockState()));

        println("PROBE OK");
    }

    private static JsonObject run(int start, int length, long word) {
        JsonObject obj = new JsonObject();
        obj.addProperty("start", start);
        obj.addProperty("length", length);
        obj.addProperty("word", word);
        return obj;
    }

    private static JsonObject maskRun(int start, int length, int mask) {
        JsonObject obj = new JsonObject();
        obj.addProperty("start", start);
        obj.addProperty("length", length);
        obj.addProperty("mask", mask);
        return obj;
    }

    private static JsonObject boolRun(int start, int length, boolean value) {
        JsonObject obj = new JsonObject();
        obj.addProperty("start", start);
        obj.addProperty("length", length);
        obj.addProperty("dynamic", value);
        return obj;
    }

    private static boolean hasDynamicShape(BlockState state) {
        return state.getBlock().hasDynamicShape();
    }

    private static int dynamicShapeCount(JsonArray runs) {
        int count = 0;
        for (var value : runs) {
            JsonObject run = value.getAsJsonObject();
            if (run.get("dynamic").getAsBoolean()) {
                count += run.get("length").getAsInt();
            }
        }
        return count;
    }

    /**
     * Evaluate SupportType.FULL.isSupporting directly at the probe origin.
     * FULL delegates to state.getBlockSupportShape(level, pos), so this avoids
     * substituting a solid-render or collision approximation for Paper's exact
     * support predicate. For hasDynamicShape states, the result is only a
     * probe-origin snapshot; production callers must supply live context.
     */
    private static int faceSturdyMask(BlockState state) {
        int mask = 0;
        for (Direction direction : Direction.values()) {
            if (SupportType.FULL.isSupporting(
                    state, EmptyBlockGetter.INSTANCE, BlockPos.ZERO, direction)) {
                mask |= 1 << direction.ordinal();
            }
        }
        return mask;
    }

    /**
     * Evaluate the full collision face predicate used by MultifaceBlock at the
     * probe origin. For hasDynamicShape states, this is only a zero-context
     * sample; production callers must supply live context.
     */
    private static int collisionFaceMask(BlockState state) {
        int mask = 0;
        for (Direction direction : Direction.values()) {
            if (Block.isFaceFull(
                    state.getCollisionShape(EmptyBlockGetter.INSTANCE, BlockPos.ZERO), direction)) {
                mask |= 1 << direction.ordinal();
            }
        }
        return mask;
    }

    /** Evaluate the state's behaviors and pack them into the documented word. */
    private static long behaviorWord(BlockState state) {
        int dampening = state.getLightDampening();
        int emission = state.getLightEmission();
        int mapColor = state.getMapColor(EmptyBlockGetter.INSTANCE, BlockPos.ZERO).id;
        // Bounds the fields can't exceed by bit width, but asserted so a jar
        // change (e.g. a 16-bit dampening) fails the probe loudly instead of
        // silently truncating.
        require(dampening >= 0 && dampening <= 15,
            "light dampening " + dampening + " out of 0..15");
        require(emission >= 0 && emission <= 15,
            "light emission " + emission + " out of 0..15");
        require(mapColor >= 0 && mapColor <= 63,
            "map color id " + mapColor + " out of 0..63");

        long word = 0;
        word |= state.isAir() ? 1L << 0 : 0;
        word |= state.blocksMotion() ? 1L << 1 : 0;
        word |= state.isSolidRender() ? 1L << 2 : 0;
        word |= state.canOcclude() ? 1L << 3 : 0;
        word |= state.useShapeForLightOcclusion() ? 1L << 4 : 0;
        word |= state.propagatesSkylightDown() ? 1L << 5 : 0;
        word |= state.isRandomlyTicking() ? 1L << 6 : 0;
        word |= state.getFluidState().isEmpty() ? 1L << 7 : 0;
        word |= (long) dampening << 8;
        word |= (long) emission << 12;
        word |= (long) mapColor << 16;
        word |= state.isSolid() ? 1L << 22 : 0;
        word |= state.canBeReplaced() ? 1L << 23 : 0;
        int fluidId = BuiltInRegistries.FLUID.getId(state.getFluidState().getType());
        require(fluidId >= 0 && fluidId <= 4,
            "fluid id " + fluidId + " out of 0..4");
        word |= (long) fluidId << 24;
        // bits 27-31 stay 0 by construction.
        return word;
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
