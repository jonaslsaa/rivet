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
import net.minecraft.world.level.block.ShulkerBoxBlock;
import net.minecraft.world.level.block.SupportType;
import net.minecraft.world.level.block.piston.MovingPistonBlock;
import net.minecraft.world.level.block.piston.PistonMovingBlockEntity;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.entity.ShulkerBoxBlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.material.Fluids;
import net.minecraft.world.phys.shapes.CollisionContext;
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
        JsonArray centerSupportRuns = new JsonArray();
        JsonArray rigidSupportRuns = new JsonArray();
        JsonArray collisionFaceRuns = new JsonArray();
        JsonArray occlusionFaceRuns = new JsonArray();
        JsonArray dynamicShapeRuns = new JsonArray();
        int runStart = 0;
        BlockState firstState = Block.stateById(0);
        require(Block.getId(firstState) == 0, "state 0 resolves to " + Block.getId(firstState));
        require(Block.stateById(Block.getId(firstState)) == firstState, "state 0 is not identity-stable");
        long runWord = behaviorWord(firstState);
        int faceSturdyRunStart = 0;
        int faceSturdyMask = faceSturdyMask(firstState);
        int centerSupportRunStart = 0;
        int centerSupportMask = centerSupportMask(firstState);
        int rigidSupportRunStart = 0;
        int rigidSupportMask = rigidSupportMask(firstState);
        int collisionFaceRunStart = 0;
        int collisionFaceMask = collisionFaceMask(firstState);
        int occlusionFaceRunStart = 0;
        int occlusionFaceMask = occlusionFaceMask(firstState);
        int dynamicShapeRunStart = 0;
        boolean dynamicShape = hasDynamicShape(firstState);
        int runCount = 0;
        int faceSturdyRunCount = 0;
        int centerSupportRunCount = 0;
        int rigidSupportRunCount = 0;
        int collisionFaceRunCount = 0;
        int occlusionFaceRunCount = 0;
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
            int centerMask = centerSupportMask(state);
            if (centerMask != centerSupportMask) {
                centerSupportRuns.add(maskRun(centerSupportRunStart, id - centerSupportRunStart, centerSupportMask));
                centerSupportRunCount++;
                centerSupportRunStart = id;
                centerSupportMask = centerMask;
            }
            int rigidMask = rigidSupportMask(state);
            if (rigidMask != rigidSupportMask) {
                rigidSupportRuns.add(maskRun(rigidSupportRunStart, id - rigidSupportRunStart, rigidSupportMask));
                rigidSupportRunCount++;
                rigidSupportRunStart = id;
                rigidSupportMask = rigidMask;
            }
            int collisionMask = collisionFaceMask(state);
            if (collisionMask != collisionFaceMask) {
                collisionFaceRuns.add(maskRun(collisionFaceRunStart, id - collisionFaceRunStart, collisionFaceMask));
                collisionFaceRunCount++;
                collisionFaceRunStart = id;
                collisionFaceMask = collisionMask;
            }
            int occlusionMask = occlusionFaceMask(state);
            if (occlusionMask != occlusionFaceMask) {
                occlusionFaceRuns.add(maskRun(occlusionFaceRunStart, id - occlusionFaceRunStart, occlusionFaceMask));
                occlusionFaceRunCount++;
                occlusionFaceRunStart = id;
                occlusionFaceMask = occlusionMask;
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
        centerSupportRuns.add(maskRun(centerSupportRunStart, count - centerSupportRunStart, centerSupportMask));
        centerSupportRunCount++;
        rigidSupportRuns.add(maskRun(rigidSupportRunStart, count - rigidSupportRunStart, rigidSupportMask));
        rigidSupportRunCount++;
        collisionFaceRuns.add(maskRun(collisionFaceRunStart, count - collisionFaceRunStart, collisionFaceMask));
        collisionFaceRunCount++;
        occlusionFaceRuns.add(maskRun(occlusionFaceRunStart, count - occlusionFaceRunStart, occlusionFaceMask));
        occlusionFaceRunCount++;
        dynamicShapeRuns.add(boolRun(dynamicShapeRunStart, count - dynamicShapeRunStart, dynamicShape));
        println("run_count=" + runCount);
        println("face_sturdy_run_count=" + faceSturdyRunCount);
        println("center_support_run_count=" + centerSupportRunCount);
        println("rigid_support_run_count=" + rigidSupportRunCount);
        println("collision_face_run_count=" + collisionFaceRunCount);
        println("occlusion_face_run_count=" + occlusionFaceRunCount);
        println("dynamic_shape_state_count=" + dynamicShapeCount(dynamicShapeRuns));

        JsonObject root = new JsonObject();
        root.addProperty("generator",
            "BlockBehaviourProbe (Bootstrap + Block.BLOCK_STATE_REGISTRY)");
        root.addProperty("minecraft_version", version);
        root.addProperty("state_count", count);
        root.add("runs", runs);
        root.add("face_sturdy_runs", faceSturdyRuns);
        root.add("center_support_runs", centerSupportRuns);
        root.add("rigid_support_runs", rigidSupportRuns);
        root.add("collision_face_runs", collisionFaceRuns);
        root.add("occlusion_face_runs", occlusionFaceRuns);
        root.add("dynamic_shape_runs", dynamicShapeRuns);
        root.add("dynamic_fixtures", dynamicFixtures());
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
        println("dynamic_fixture_count=4");

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
        return supportMask(state, SupportType.FULL);
    }

    private static int centerSupportMask(BlockState state) {
        return supportMask(state, SupportType.CENTER);
    }

    private static int rigidSupportMask(BlockState state) {
        return supportMask(state, SupportType.RIGID);
    }

    private static int supportMask(BlockState state, SupportType supportType) {
        int mask = 0;
        for (Direction direction : Direction.values()) {
            if (supportType.isSupporting(state, EmptyBlockGetter.INSTANCE, BlockPos.ZERO, direction)) {
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

    private static int occlusionFaceMask(BlockState state) {
        int mask = 0;
        for (Direction direction : Direction.values()) {
            if (Block.isShapeFullBlock(state.getFaceOcclusionShape(direction))) {
                mask |= 1 << direction.ordinal();
            }
        }
        return mask;
    }

    private static JsonArray dynamicFixtures() throws Exception {
        JsonArray fixtures = new JsonArray();
        BlockPos pos = BlockPos.ZERO;
        BlockState shulkerState = Blocks.SHULKER_BOX.defaultBlockState().setValue(ShulkerBoxBlock.FACING, Direction.UP);

        ShulkerBoxBlockEntity closed = new ShulkerBoxBlockEntity(pos, shulkerState);
        fixtures.add(dynamicFixture("shulker_closed_up", shulkerState, new FixtureGetter(shulkerState, closed), pos));

        ShulkerBoxBlockEntity open = new ShulkerBoxBlockEntity(pos, shulkerState);
        setPrivateFloat(open, "progress", 1.0F);
        setPrivateEnum(open, "animationStatus", "OPENED");
        fixtures.add(dynamicFixture("shulker_open_up", shulkerState, new FixtureGetter(shulkerState, open), pos));

        BlockState movingState = Blocks.MOVING_PISTON.defaultBlockState().setValue(MovingPistonBlock.FACING, Direction.EAST);
        PistonMovingBlockEntity moving = new PistonMovingBlockEntity(
            pos, movingState, Blocks.STONE.defaultBlockState(), Direction.EAST, true, false);
        setPrivateFloat(moving, "progress", 0.5F);
        fixtures.add(dynamicFixture("moving_piston_half_east", movingState, new FixtureGetter(movingState, moving), pos));

        PistonMovingBlockEntity movingClosed = new PistonMovingBlockEntity(
            pos, movingState, Blocks.STONE.defaultBlockState(), Direction.EAST, true, false);
        fixtures.add(dynamicFixture("moving_piston_start_east", movingState, new FixtureGetter(movingState, movingClosed), pos));
        return fixtures;
    }

    private static JsonObject dynamicFixture(String name, BlockState state, FixtureGetter level, BlockPos pos) {
        JsonObject fixture = new JsonObject();
        fixture.addProperty("name", name);
        fixture.addProperty("block", BuiltInRegistries.BLOCK.getKey(state.getBlock()).toString());
        fixture.addProperty("state_id", Block.getId(state));
        fixture.addProperty("dynamic", state.getBlock().hasDynamicShape());
        fixture.addProperty("support_full", supportMask(state, SupportType.FULL, level, pos));
        fixture.addProperty("support_center", supportMask(state, SupportType.CENTER, level, pos));
        fixture.addProperty("support_rigid", supportMask(state, SupportType.RIGID, level, pos));
        fixture.addProperty("collision_full", collisionFaceMask(state, level, pos));
        fixture.addProperty("occlusion_full", occlusionFaceMask(state, level, pos));
        return fixture;
    }

    private static int supportMask(BlockState state, SupportType supportType, FixtureGetter level, BlockPos pos) {
        int mask = 0;
        for (Direction direction : Direction.values()) {
            if (supportType.isSupporting(state, level, pos, direction)) {
                mask |= 1 << direction.ordinal();
            }
        }
        return mask;
    }

    private static int collisionFaceMask(BlockState state, FixtureGetter level, BlockPos pos) {
        int mask = 0;
        for (Direction direction : Direction.values()) {
            if (Block.isFaceFull(state.getCollisionShape(level, pos), direction)) {
                mask |= 1 << direction.ordinal();
            }
        }
        return mask;
    }

    private static int occlusionFaceMask(BlockState state, FixtureGetter level, BlockPos pos) {
        int mask = 0;
        for (Direction direction : Direction.values()) {
            if (Block.isFaceFull(state.getShape(level, pos), direction)) {
                mask |= 1 << direction.ordinal();
            }
        }
        return mask;
    }

    private static void setPrivateFloat(Object object, String fieldName, float value) throws Exception {
        var field = object.getClass().getDeclaredField(fieldName);
        field.setAccessible(true);
        field.setFloat(object, value);
    }

    @SuppressWarnings({"rawtypes", "unchecked"})
    private static void setPrivateEnum(Object object, String fieldName, String value) throws Exception {
        var field = object.getClass().getDeclaredField(fieldName);
        field.setAccessible(true);
        field.set(object, Enum.valueOf((Class<? extends Enum>) field.getType(), value));
    }

    private static final class FixtureGetter implements net.minecraft.world.level.BlockGetter {
        private final BlockState state;
        private final BlockEntity entity;

        FixtureGetter(BlockState state, BlockEntity entity) {
            this.state = state;
            this.entity = entity;
        }

        @Override public BlockEntity getBlockEntity(BlockPos pos) { return pos.equals(BlockPos.ZERO) ? entity : null; }
        @Override public BlockState getBlockState(BlockPos pos) { return state; }
        @Override public BlockState getBlockStateIfLoaded(BlockPos pos) { return state; }
        @Override public FluidState getFluidIfLoaded(BlockPos pos) { return Fluids.EMPTY.defaultFluidState(); }
        @Override public FluidState getFluidState(BlockPos pos) { return Fluids.EMPTY.defaultFluidState(); }
        @Override public int getHeight() { return 384; }
        @Override public int getMinY() { return -64; }
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
