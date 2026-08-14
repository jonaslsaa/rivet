//! STUB(mc.world.level.block) —
//! `net.minecraft.world.level.block.MossyCarpetBlock`.
//!
//! The owning `mc.world.level.block` unit (issue #228) has not ported the
//! block class yet; this unit only needs the `placeAt` helper
//! `SimpleBlockFeature.place` consumes when the placed state is a
//! `MossyCarpetBlock` (only `minecraft:pale_moss_carpet`).
//!
//! `MossyCarpetBlock.placeAt(LevelAccessor, BlockPos, RandomSource, int)` —
//! places `minecraft:pale_moss_carpet` at `pos` (the state written is
//! `getUpdatedState(defaultState, level, pos, true)`, which reads the
//! horizontal neighbors through `MultifaceBlock.canAttachTo` and sets the
//! `WallSide`-valued face properties), then a face-negotiated topper at
//! `pos.above()` (and re-updates the base) when one survives the random side
//! test. The face reads (`WallSide` properties), the attach check
//! (`MultifaceBlock.canAttachTo`) and the level's `getRandom` are not ported
//! (RivetTodo #232), so the topper half is deferred.
//!
//! What this seam DOES implement faithfully is the unconditional base write
//! (`level.setBlock(pos, Blocks.PALE_MOSS_CARPET.defaultBlockState(),
//! updateType)`) — the default state is the `getUpdatedState` result when no
//! horizontal neighbor can attach, and a valid configuration (vanilla
//! `pale_moss_vegetation.json` routes `minecraft:simple_block` here) must
//! never panic the server. The owning unit replaces the seam with the full
//! `getUpdatedState` + topper negotiation when `WallSide`/`getRandom`/
//! `MultifaceBlock.canAttachTo` land.
//!
//! The `RandomSource` is the LEVEL's (`MossyCarpetBlock.placeAt` receives
//! `level.getRandom()`), not the feature-context RNG — `SimpleBlockFeature`
//! must never thread `context.random()` here, so the seam takes no RNG at all
//! and the future wiring adds a level-RNG accessor (RivetTodo #232). The
//! deferred topper's `nextBoolean` side draws must come from that level RNG,
//! never from the feature-context `random`.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use rivet_registry::core::BlockPos;

/// `MossyCarpetBlock.placeAt(LevelAccessor, BlockPos, RandomSource, int)` — the
/// base write only: `level.setBlock(pos, Blocks.PALE_MOSS_CARPET
/// .defaultBlockState(), updateType)`. The Java `getUpdatedState` negotiation
/// and the random topper at `pos.above()` are deferred to RivetTodo #232 (see
/// the module doc); writing the default state is a defined result for every
/// reachable configuration, so this seam cannot panic.
pub fn place_at(level: &mut dyn WorldGenLevel, pos: &BlockPos, update_type: u32) {
    level.set_block(
        pos,
        Blocks::PALE_MOSS_CARPET.default_block_state(),
        update_type,
    );
}
