//! Port of `net.minecraft.world.level.levelgen.feature.EndPodiumFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.endpodium`
//! manifest unit (the end-leaves wave).
//!
//! Java: an *unregistered* `Feature<NoneFeatureConfiguration>` constructed with
//! a `boolean active` (the active variant writes the `END_PORTAL`; the
//! inactive one the surrounding `AIR`). Because it is never `register`ed into
//! `BuiltInRegistries.FEATURE`, it has no `FeatureId` and no `#181` dispatch
//! arm — concrete construction sites build it directly. The port keeps the
//! `active` field; `getLocation` (the `ZERO.offset` helper) is ported for
//! greppability.
//!
//! `place` walks `BlockPos.betweenClosed(origin - (4,1,4), origin + (4,32,4))`
//! in X/Y/Z-major order. A cell is filled when `insideRim = closerThan(origin,
//! 2.5)` or `closerThan(origin, 3.5)`; `Vec3i.closerThan` is
//! `distToLowCornerSqr(pos) < Mth.square(distance)` (double `dx*dx+dy*dy+dz*dz`,
//! the int coordinates widened to double before the per-axis subtraction), so
//! the rim test is `dx²+dy²+dz² < 6.25` and the outer test
//! `dx²+dy²+dz² < 12.25`. Below the origin a rim cell is `BEDROCK` and an outer
//! cell `END_STONE`; above it writes `AIR` (or destroys first when `active`);
//! at the origin level a non-rim cell is `BEDROCK` and the active rim is the
//! `END_PORTAL` (inactive rim writes `AIR`). Then the 4-block `BEDROCK` pillar
//! rises from the origin, and a `WALL_TORCH` (with `HORIZONTAL_FACING` set to
//! each horizontal face) hangs on each side of `origin.above(2)`. Every write
//! goes through `Feature.setBlock` (`UPDATE_ALL`); the `active` path routes the
//! replace-through `dropPreviousAndSetBlock`, which destroys the existing cell
//! (`WorldGenLevel::destroy_block`, RivetTodo #232) before overwriting.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::core::BlockPos;
use rivet_registry::core::Plane;
use rivet_util::RandomSource;

/// `Block.UPDATE_ALL` — the write-flag constant `Feature.setBlock` reduces to
/// (`UPDATE_NEIGHBORS | UPDATE_CLIENTS`).
const UPDATE_ALL: u32 = 3;

/// `BlockStateBase.is(Block)` — the block identity check
/// `dropPreviousAndSetBlock` gates its destroy on.
#[inline]
fn is_block(state: BlockState, block: crate::block::Block) -> bool {
    state.block() == block.id()
}

/// `Vec3i.closerThan(Vec3i, double)` — `distToLowCornerSqr(pos) <
/// Mth.square(distance)`. `distToLowCornerSqr` widens the int coordinates to
/// double and squares the per-axis double deltas: `dx*dx+dy*dy+dz*dz` in f64,
/// no integer arithmetic at all. The `distance` is a `f64` (Java `double`),
/// so `Mth.square(double)` (`x * x`) squares it without an f32 round-trip.
fn closer_than(a: &BlockPos, b: &BlockPos, distance: f64) -> bool {
    let dx = a.get_x() as f64 - b.get_x() as f64;
    let dy = a.get_y() as f64 - b.get_y() as f64;
    let dz = a.get_z() as f64 - b.get_z() as f64;
    let dist_sqr = dx * dx + dy * dy + dz * dz;
    dist_sqr < distance * distance
}

/// `net.minecraft.world.level.levelgen.feature.EndPodiumFeature`.
#[derive(Debug)]
pub struct EndPodiumFeature {
    /// `EndPodiumFeature.active` — whether this podium writes the `END_PORTAL`
    /// (the active portal variant) or plain `AIR` at the center.
    pub active: bool,
}

/// `EndPodiumFeature` — the unregistered podium feature; constructed directly
/// (no registry insertion), so it carries no `FeatureId`. `Feature.END_PODIUM`
/// does not exist in `Feature.java`; `EnderDragonFight` builds the feature
/// per-instance with `new EndPodiumFeature(activated)` (net/minecraft/world/
/// level/dimension/end/EnderDragonFight.java:472).

/// `EndPodiumFeature.PODIUM_RADIUS`.
pub const PODIUM_RADIUS: i32 = 4;
/// `EndPodiumFeature.PODIUM_PILLAR_HEIGHT`.
pub const PODIUM_PILLAR_HEIGHT: i32 = 4;
/// `EndPodiumFeature.RIM_RADIUS`.
pub const RIM_RADIUS: i32 = 1;
/// `EndPodiumFeature.CORNER_ROUNDING`.
pub const CORNER_ROUNDING: f32 = 0.5;

/// `EndPodiumFeature.getLocation(BlockPos)` — `END_PODIUM_LOCATION.offset(offset)`
/// with `END_PODIUM_LOCATION = BlockPos.ZERO`.
pub fn get_location(offset: &BlockPos) -> BlockPos {
    BlockPos::ZERO.offset(offset.get_x(), offset.get_y(), offset.get_z())
}

impl EndPodiumFeature {
    /// `EndPodiumFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`.
    ///
    /// ```java
    /// for (BlockPos pos : BlockPos.betweenClosed(
    ///         new BlockPos(origin.getX() - 4, origin.getY() - 1, origin.getZ() - 4),
    ///         new BlockPos(origin.getX() + 4, origin.getY() + 32, origin.getZ() + 4))) {
    ///     boolean insideRim = pos.closerThan(origin, 2.5);
    ///     if (insideRim || pos.closerThan(origin, 3.5)) {
    ///         if (pos.getY() < origin.getY()) {
    ///             if (insideRim) {
    ///                 this.setBlock(level, pos, Blocks.BEDROCK.defaultBlockState());
    ///             } else if (pos.getY() < origin.getY()) {
    ///                 if (this.active) this.dropPreviousAndSetBlock(level, pos, Blocks.END_STONE);
    ///                 else this.setBlock(level, pos, Blocks.END_STONE.defaultBlockState());
    ///             }
    ///         } else if (pos.getY() > origin.getY()) {
    ///             if (this.active) this.dropPreviousAndSetBlock(level, pos, Blocks.AIR);
    ///             else this.setBlock(level, pos, Blocks.AIR.defaultBlockState());
    ///         } else if (!insideRim) {
    ///             this.setBlock(level, pos, Blocks.BEDROCK.defaultBlockState());
    ///         } else if (this.active) {
    ///             this.dropPreviousAndSetBlock(level, new BlockPos(pos), Blocks.END_PORTAL);
    ///         } else {
    ///             this.setBlock(level, new BlockPos(pos), Blocks.AIR.defaultBlockState());
    ///         }
    ///     }
    /// }
    /// for (int y = 0; y < 4; y++) this.setBlock(level, origin.above(y), Blocks.BEDROCK.defaultBlockState());
    /// BlockPos centerOfPillar = origin.above(2);
    /// for (Direction face : Direction.Plane.HORIZONTAL) {
    ///     this.setBlock(level, centerOfPillar.relative(face),
    ///         Blocks.WALL_TORCH.defaultBlockState().setValue(WallTorchBlock.FACING, face));
    /// }
    /// return true;
    /// ```
    fn place_inner<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, NoneFeatureConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext { level, origin, .. } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let origin = *origin;
        for pos in BlockPos::between_closed(
            origin.get_x().wrapping_sub(4),
            origin.get_y().wrapping_sub(1),
            origin.get_z().wrapping_sub(4),
            origin.get_x().wrapping_add(4),
            origin.get_y().wrapping_add(32),
            origin.get_z().wrapping_add(4),
        ) {
            let inside_rim = closer_than(&pos, origin, 2.5);
            if inside_rim || closer_than(&pos, origin, 3.5) {
                if pos.get_y() < origin.get_y() {
                    if inside_rim {
                        set_block(level, &pos, Blocks::BEDROCK.default_block_state());
                    } else if pos.get_y() < origin.get_y() {
                        if self.active {
                            drop_previous_and_set_block(level, &pos, Blocks::END_STONE);
                        } else {
                            set_block(level, &pos, Blocks::END_STONE.default_block_state());
                        }
                    }
                } else if pos.get_y() > origin.get_y() {
                    if self.active {
                        drop_previous_and_set_block(level, &pos, Blocks::AIR);
                    } else {
                        set_block(level, &pos, Blocks::AIR.default_block_state());
                    }
                } else if !inside_rim {
                    set_block(level, &pos, Blocks::BEDROCK.default_block_state());
                } else if self.active {
                    drop_previous_and_set_block(level, &pos, Blocks::END_PORTAL);
                } else {
                    set_block(level, &pos, Blocks::AIR.default_block_state());
                }
            }
        }
        for y in 0..4 {
            set_block(
                level,
                &origin.above_steps(y),
                Blocks::BEDROCK.default_block_state(),
            );
        }
        let center_of_pillar = origin.above_steps(2);
        for face in Plane::Horizontal.faces() {
            set_block(
                level,
                &center_of_pillar.relative(face),
                Blocks::WALL_TORCH
                    .default_block_state()
                    .set_value(BlockStateProperties::HORIZONTAL_FACING, *face)
                    .expect("wall_torch carries the horizontal facing property"),
            );
        }
        true
    }
}

/// `Feature.setBlock(LevelWriter, BlockPos, BlockState)` — `level.setBlock(pos,
/// state, Block.UPDATE_ALL)`, the write `EndPodiumFeature` reduces its
/// placements to.
fn set_block(level: &mut dyn WorldGenLevel, pos: &BlockPos, state: BlockState) {
    level.set_block(pos, state, UPDATE_ALL);
}

/// `EndPodiumFeature.dropPreviousAndSetBlock` — the shared replace helper
/// (`if (!level.getBlockState(pos).is(block)) { level.destroyBlock(pos, true,
/// null); this.setBlock(level, pos, block.defaultBlockState()); }`).
fn drop_previous_and_set_block(
    level: &mut dyn WorldGenLevel,
    pos: &BlockPos,
    block: crate::block::Block,
) {
    if !is_block(level.get_block_state(pos), block) {
        level.destroy_block(pos, true);
        set_block(level, pos, block.default_block_state());
    }
}

impl FeatureBehavior<NoneFeatureConfiguration> for EndPodiumFeature {
    /// `EndPodiumFeature.place` — `place` is the only overridable entry; the
    /// shared logic lives in [`EndPodiumFeature::place_inner`].
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, NoneFeatureConfiguration, R>,
    ) -> bool {
        self.place_inner(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use rivet_registry::block_state_property::PropertyValue;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::random::LegacyRandomSource;

    fn place(active: bool, level: &mut TestLevel, origin: BlockPos) -> bool {
        let generator = TestGenerator;
        let mut random = LegacyRandomSource::new(1);
        let feature = EndPodiumFeature { active };
        feature.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            &mut random,
            &origin,
            &NoneFeatureConfiguration::INSTANCE,
        ))
    }

    /// The inactive podium writes the `BEDROCK` floor/pillar, `END_STONE`
    /// under the rim, `AIR` in the y=0 rim ring and above, and `WALL_TORCH`
    /// hanging on each face of `origin.above(2)`. The `betweenClosed` pass runs
    /// first, then the 4-pillar loop overwrites the center column
    /// (`origin.above(0..3)`) with `BEDROCK` — so the y=0 center is bedrock,
    /// exactly as Java leaves it (the real End exit portal is a portal ring
    /// around a bedrock center).
    #[test]
    fn inactive_podium_writes_bedrock_end_stone_air_and_torches() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 0, 0);
        let placed = place(false, &mut level, origin);
        assert!(placed);
        let bedrock = BlockId::from_name("minecraft:bedrock").unwrap();
        let end_stone = BlockId::from_name("minecraft:end_stone").unwrap();
        let air = BlockId(0);
        // Rim cells at y=-1 are bedrock; the rim-adjacent ring is end stone.
        assert_eq!(level.states[&BlockPos::new(0, -1, 0)].block(), bedrock);
        assert_eq!(level.states[&BlockPos::new(3, -1, 0)].block(), end_stone);
        // The y=0 rim ring is air (inactive), the center is the bedrock pillar.
        assert_eq!(level.states[&BlockPos::new(1, 0, 0)].block(), air);
        assert_eq!(level.states[&BlockPos::new(0, 0, 0)].block(), bedrock);
        // Above the rim the cell is air, except the pillar column.
        assert_eq!(level.states[&BlockPos::new(1, 1, 0)].block(), air);
        assert_eq!(level.states[&BlockPos::new(0, 1, 0)].block(), bedrock);
        // The 4-pillar of bedrock rises above the origin.
        for y in 0..4 {
            assert_eq!(level.states[&origin.above_steps(y)].block(), bedrock);
        }
        // The torch at +X faces east (the +X face is the wall it hangs on).
        let torch = BlockId::from_name("minecraft:wall_torch").unwrap();
        assert_eq!(level.states[&BlockPos::new(1, 2, 0)].block(), torch);
        let east_torch = level.states[&BlockPos::new(1, 2, 0)];
        assert_eq!(
            east_torch.get_value(BlockStateProperties::HORIZONTAL_FACING),
            Some(PropertyValue::Enum("east"))
        );
    }

    /// The y=0 rim ring is `AIR` for the inactive podium, and the bedrock
    /// pillar fills the center column (`origin.above(0..3)` overwrites the
    /// in-loop center cell).
    #[test]
    fn inactive_podium_writes_air_ring_around_a_bedrock_center() {
        let mut level = TestLevel::over(access());
        let placed = place(false, &mut level, BlockPos::new(0, 0, 0));
        assert!(placed);
        assert_eq!(level.states[&BlockPos::new(1, 0, 0)].block(), BlockId(0));
        assert_eq!(
            level.states[&BlockPos::new(0, 0, 0)].block(),
            BlockId::from_name("minecraft:bedrock").unwrap()
        );
    }

    /// The active podium writes `END_PORTAL` into the y=0 rim ring and the
    /// bedrock pillar into the center column. The portal ring cells are
    /// dropped-then-replaced (the `is(block)` gate only destroys cells that
    /// differ).
    #[test]
    fn active_podium_writes_end_portal_ring_around_a_bedrock_center() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 0, 0);
        let placed = place(true, &mut level, origin);
        assert!(placed);
        assert_eq!(
            level.states[&BlockPos::new(1, 0, 0)].block(),
            BlockId::from_name("minecraft:end_portal").unwrap()
        );
        // The center cell was destroyed (it differed from the portal target)
        // and then overwritten by the pillar.
        assert_eq!(
            level.states[&origin].block(),
            BlockId::from_name("minecraft:bedrock").unwrap()
        );
    }

    /// A hostile partial world: a rim cell already holding `END_PORTAL` is
    /// left untouched by the active podium's `dropPreviousAndSetBlock` (the
    /// `is(block)` gate skips both the destroy and the write); a differing
    /// cell is destroyed then replaced.
    #[test]
    fn active_podium_gate_skips_cells_already_matching() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 0, 0);
        // Pre-fill one rim cell with the active podium's target so the
        // `is(block)` gate sees a match and skips the destroy+set.
        let portal_pos = BlockPos::new(1, 0, 0);
        level
            .states
            .insert(portal_pos, Blocks::END_PORTAL.default_block_state());
        let placed = place(true, &mut level, origin);
        assert!(placed);
        assert!(!level.destroyed.contains(&portal_pos));
        assert!(!level.writes.iter().any(|(p, _)| *p == portal_pos));
        assert_eq!(
            level.states[&portal_pos].block(),
            BlockId::from_name("minecraft:end_portal").unwrap()
        );
    }
}
