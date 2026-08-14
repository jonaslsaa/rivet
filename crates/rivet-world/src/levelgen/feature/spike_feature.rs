//! Port of `net.minecraft.world.level.levelgen.feature.SpikeFeature`
//! (class, 26.2) — the `mc.world.level.levelgen.feature.spike` manifest unit.
//!
//! Java: `Feature<SpikeConfiguration>` that grows ice spikes. `place` first
//! descends the origin column while the cell is empty and above
//! `getMinY() + 2`; it aborts when `config.canPlaceOn().test(level, origin)`
//! fails. It then lifts the origin by `nextInt(4)`, draws `height =
//! nextInt(4) + 7` and `width = height / 4 + nextInt(2)`, and (when `width > 1`
//! with a `1/60` roll) jumps the origin up by `10 + nextInt(30)`. The spike
//! shell is written layer by layer: for each `yOff < height`, `newWidth =
//! Mth.ceil((1 - yOff/height) * width)`, and every cell of the `-newWidth..=
//! newWidth` square whose `(dx, dz)` with `dx = Mth.abs(xo) - 0.25` lies inside
//! the `scale` circle (with the 75% chance roll for the ring edge) is written
//! at `origin.offset(xo, yOff, zo)`, plus the mirror at `-yOff` when `yOff != 0
//! && newWidth > 1`. A pillar then descends from `origin - 1` over the
//! `-pillarWidth..=pillarWidth` square (`pillarWidth = clamp(width - 1, 0, 1)`),
//! writing the state downward past `y > 50` with `runLength` runs (a `1/5`
//! roll at the corners, and a `nextInt(5) + 1` re-drop + `nextInt(5)` new run
//! length when a run exhausts), stopping at a non-replaceable, non-spike cell.
//! Always returns `true` once the placement gate passes.
//!
//! `Mth.abs(int)` wraps on `Integer.MIN_VALUE` — the port's `Mth::abs_i32`
//! (`wrapping_abs`). The f32 `!(f > 0.75)` roll is written as `f <= 0.75`:
//! `RandomSource.nextFloat()` lies in `[0, 1)`, so the negation is identical.
//!
//! The world reads (`get_block_state`/`is_empty_block`) and writes
//! (`set_block` with `Block.UPDATE_ALL`) go through the `WorldGenLevel`
//! seams (RivetTodo #232); the test double overrides them. The predicates
//! dispatch through the erased `BlockPredicate::test`.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::SpikeConfiguration;
use rivet_util::RandomSource;
use rivet_util::mth;

/// `Block.UPDATE_ALL` — the write-flag constant `Feature.setBlock` reduces
/// to (`UPDATE_NEIGHBORS | UPDATE_CLIENTS`), in contrast to `safeSetBlock`'s
/// `Block.UPDATE_CLIENTS` used by e.g. LakeFeature.
const UPDATE_ALL: u32 = 3;

/// The `cursor.getY() > 50` pillar floor — the hard-coded Java constant.
const PILLAR_FLOOR_Y: i32 = 50;

/// `net.minecraft.world.level.levelgen.feature.SpikeFeature`.
#[derive(Debug)]
pub struct SpikeFeature;

/// `Feature.SPIKE` — the registered `minecraft:spike` singleton.
pub const SPIKE: SpikeFeature = SpikeFeature;

impl FeatureBehavior<SpikeConfiguration> for SpikeFeature {
    /// `SpikeFeature.place(FeaturePlaceContext<SpikeConfiguration>)`.
    ///
    /// ```java
    /// while (level.isEmptyBlock(origin) && origin.getY() > level.getMinY() + 2) {
    ///     origin = origin.below();
    /// }
    /// if (!config.canPlaceOn().test(level, origin)) return false;
    /// origin = origin.above(random.nextInt(4));
    /// int height = random.nextInt(4) + 7;
    /// int width = height / 4 + random.nextInt(2);
    /// if (width > 1 && random.nextInt(60) == 0) {
    ///     origin = origin.above(10 + random.nextInt(30));
    /// }
    /// for (int yOff = 0; yOff < height; yOff++) {
    ///     float scale = (1.0F - (float)yOff / height) * width;
    ///     int newWidth = Mth.ceil(scale);
    ///     for (int xo = -newWidth; xo <= newWidth; xo++) {
    ///         float dx = Mth.abs(xo) - 0.25F;
    ///         for (int zo = -newWidth; zo <= newWidth; zo++) {
    ///             float dz = Mth.abs(zo) - 0.25F;
    ///             if ((xo == 0 && zo == 0 || !(dx * dx + dz * dz > scale * scale))
    ///                 && (xo != -newWidth && xo != newWidth && zo != -newWidth && zo != newWidth
    ///                     || !(random.nextFloat() > 0.75F))) {
    ///                 BlockPos positiveOffset = origin.offset(xo, yOff, zo);
    ///                 BlockState state = level.getBlockState(positiveOffset);
    ///                 if (state.isAir() || config.canReplace().test(level, positiveOffset)) {
    ///                     this.setBlock(level, positiveOffset, config.state());
    ///                 }
    ///                 if (yOff != 0 && newWidth > 1) {
    ///                     BlockPos negativeOffset = origin.offset(xo, -yOff, zo);
    ///                     state = level.getBlockState(negativeOffset);
    ///                     if (state.isAir() || config.canReplace().test(level, negativeOffset)) {
    ///                         this.setBlock(level, negativeOffset, config.state());
    ///                     }
    ///                 }
    ///             }
    ///         }
    ///     }
    /// }
    /// int pillarWidth = width - 1;
    /// if (pillarWidth < 0) pillarWidth = 0;
    /// else if (pillarWidth > 1) pillarWidth = 1;
    /// for (int xo = -pillarWidth; xo <= pillarWidth; xo++) {
    ///     for (int zo = -pillarWidth; zo <= pillarWidth; zo++) {
    ///         BlockPos cursor = origin.offset(xo, -1, zo);
    ///         int runLength = 50;
    ///         if (Math.abs(xo) == 1 && Math.abs(zo) == 1) runLength = random.nextInt(5);
    ///         while (cursor.getY() > 50) {
    ///             BlockState state = level.getBlockState(cursor);
    ///             if (!state.isAir() && !config.canReplace().test(level, cursor)
    ///                 && state != config.state()) break;
    ///             this.setBlock(level, cursor, config.state());
    ///             cursor = cursor.below();
    ///             if (--runLength <= 0) {
    ///                 cursor = cursor.below(random.nextInt(5) + 1);
    ///                 runLength = random.nextInt(5);
    ///             }
    ///         }
    ///     }
    /// }
    /// return true;
    /// ```
    //
    // `!(dx*dx + dz*dz > scale*scale)` is Java's literal circle test; the
    // partially-ordered negation is kept so clippy's `partial_cmp` rewrite
    // cannot change the result.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, SpikeConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            random,
            origin,
            config,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let random: &mut R = random;
        let config = *config;
        // `origin` is reassigned (descent/lift), so it is an owned `BlockPos`
        // (`BlockPos` is `Copy`), unlike the read-only features' borrow.
        let mut origin = **origin;
        while level.is_empty_block(&origin) && origin.get_y() > level.get_min_y().wrapping_add(2) {
            origin = origin.below();
        }
        if !config.can_place_on().test(level, &origin) {
            return false;
        }
        origin = origin.above_steps(random.next_int_bound(4));
        let height = random.next_int_bound(4).wrapping_add(7);
        let width = height / 4 + random.next_int_bound(2);
        if width > 1 && random.next_int_bound(60) == 0 {
            origin = origin.above_steps(10i32.wrapping_add(random.next_int_bound(30)));
        }
        for y_off in 0..height {
            let scale = (1.0f32 - (y_off as f32) / (height as f32)) * (width as f32);
            let new_width = mth::ceil(scale);
            for xo in -new_width..=new_width {
                let dx = mth::abs_i32(xo) as f32 - 0.25f32;
                for zo in -new_width..=new_width {
                    let dz = mth::abs_i32(zo) as f32 - 0.25f32;
                    // `(xo == 0 && zo == 0 || !(dx*dx + dz*dz > scale*scale)) && (...)` —
                    // both disjunctions are short-circuited exactly as Java. The
                    // ring-edge `nextFloat` is drawn only when the cell is inside
                    // the circle AND on the ring edge: the `inside` conjunct gates
                    // the `(...)` group, and `not_on_ring_edge` (an interior cell,
                    // `xo != ±newWidth && zo != ±newWidth`) short-circuits the roll
                    // while ring-edge cells draw it — so outside-circle cells never
                    // consume a draw.
                    let inside = (xo == 0 && zo == 0) || !(dx * dx + dz * dz > scale * scale);
                    let not_on_ring_edge =
                        xo != -new_width && xo != new_width && zo != -new_width && zo != new_width;
                    if inside && (not_on_ring_edge || random.next_float() <= 0.75f32) {
                        let positive_offset = origin.offset(xo, y_off, zo);
                        let state = level.get_block_state(&positive_offset);
                        if state.is_air() || config.can_replace().test(level, &positive_offset) {
                            level.set_block(&positive_offset, config.state(), UPDATE_ALL);
                        }
                        if y_off != 0 && new_width > 1 {
                            let negative_offset = origin.offset(xo, -y_off, zo);
                            let state = level.get_block_state(&negative_offset);
                            if state.is_air() || config.can_replace().test(level, &negative_offset)
                            {
                                level.set_block(&negative_offset, config.state(), UPDATE_ALL);
                            }
                        }
                    }
                }
            }
        }
        // Java `if (pillarWidth < 0) pillarWidth = 0; else if (pillarWidth > 1)
        // pillarWidth = 1;` — `clamp(0, 1)` on `i32`.
        let pillar_width = width.wrapping_sub(1).clamp(0, 1);
        for xo in -pillar_width..=pillar_width {
            for zo in -pillar_width..=pillar_width {
                let mut cursor = origin.offset(xo, -1, zo);
                let mut run_length = 50i32;
                if mth::abs_i32(xo) == 1 && mth::abs_i32(zo) == 1 {
                    run_length = random.next_int_bound(5);
                }
                while cursor.get_y() > PILLAR_FLOOR_Y {
                    let state = level.get_block_state(&cursor);
                    if !state.is_air()
                        && !config.can_replace().test(level, &cursor)
                        && state != config.state()
                    {
                        break;
                    }
                    level.set_block(&cursor, config.state(), UPDATE_ALL);
                    cursor = cursor.below();
                    run_length = run_length.wrapping_sub(1);
                    if run_length <= 0 {
                        cursor = cursor.below_steps(random.next_int_bound(5).wrapping_add(1));
                        run_length = random.next_int_bound(5);
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::levelgen::blockpredicates::{always_true, not};
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::BlockPos;
    use rivet_util::random::LegacyPositionalRandomFactory;
    use std::sync::Arc;

    /// A `RandomSource` that draws `0` for every bounded draw and `0.0` for
    /// `nextFloat` — the deterministic `height = 7`, `width = 1` spike (no rim
    /// jump, no corner run-length roll, every edge cell drawn).
    #[derive(Clone, Copy)]
    struct ZeroRandom;

    impl RandomSource for ZeroRandom {
        type Positional = LegacyPositionalRandomFactory;

        fn fork(&mut self) -> Self {
            ZeroRandom
        }
        fn fork_positional(&mut self) -> Self::Positional {
            LegacyPositionalRandomFactory::new(0)
        }
        fn set_seed(&mut self, _seed: i64) {}
        fn next_int(&mut self) -> i32 {
            0
        }
        fn next_int_bound(&mut self, _bound: i32) -> i32 {
            0
        }
        fn next_long(&mut self) -> i64 {
            0
        }
        fn next_boolean(&mut self) -> bool {
            false
        }
        fn next_float(&mut self) -> f32 {
            0.0
        }
        fn next_double(&mut self) -> f64 {
            0.0
        }
        fn next_gaussian(&mut self) -> f64 {
            0.0
        }
    }

    fn state() -> BlockState {
        BlockState::of(Blocks::PACKED_ICE.id())
    }

    fn config() -> SpikeConfiguration {
        SpikeConfiguration::new(state(), always_true(), always_true())
    }

    fn place_with<R: RandomSource>(
        level: &mut TestLevel,
        origin: BlockPos,
        random: &mut R,
    ) -> bool {
        let generator = TestGenerator;
        SPIKE.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &config(),
        ))
    }

    /// A full spike on a solid origin: descent stops immediately, no lift, and
    /// with `height = 7`, `width = 1` every layer has `newWidth = 1`. Layers 0
    /// and 1 write the five cells inside the radius-1 circle (center + the four
    /// orthogonal neighbours; the four corners have `dx*dx + dz*dz = 1.125 > 1`),
    /// but the circle shrinks each layer: `scale = 1 - yOff/7`, so from layer 2
    /// on (`scale*scale < 0.5625`) only the center lies inside — `5 + 5 + 5 = 15`
    /// shell writes. The pillar floor `y > 50` never fires at `y = 0`. Returns
    /// `true`.
    #[test]
    fn full_spike_writes_layers() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 0, 0);
        level.states.insert(origin, state());
        let mut random = ZeroRandom;
        assert!(place_with(&mut level, origin, &mut random));
        // Layers 0-1 write 5 in-circle cells each, layers 2-6 center only.
        assert_eq!(level.writes.len(), 5 + 5 + 5);
        // First layer's first in-circle cell (xo = -1, zo = 0).
        assert_eq!(level.writes[0], (origin.offset(-1, 0, 0), state()));
        // Every write is the spike state.
        assert!(level.writes.iter().all(|(_, s)| *s == state()));
        // The corners of the first layer are outside the circle.
        assert!(
            !level
                .writes
                .iter()
                .any(|(p, _)| *p == origin.offset(-1, 0, -1))
        );
    }

    /// The origin is air above a solid floor: the feature descends the column
    /// to the floor before drawing — the first write lands relative to the
    /// descended origin, not the input origin.
    #[test]
    fn descends_to_floor_before_spike() {
        let mut level = TestLevel::over(access());
        let floor = BlockPos::new(0, 2, 0);
        level.states.insert(floor, state());
        let mut random = ZeroRandom;
        assert!(place_with(&mut level, BlockPos::new(0, 5, 0), &mut random));
        assert_eq!(level.writes[0], (floor.offset(-1, 0, 0), state()));
    }

    /// A `canPlaceOn` failure returns `false` before any draw or write.
    #[test]
    fn can_place_on_failure_returns_false() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 0, 0);
        level.states.insert(origin, state());
        let generator = TestGenerator;
        let reject = SpikeConfiguration::new(state(), Arc::new(not(always_true())), always_true());
        let mut random = ZeroRandom;
        assert!(!SPIKE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &reject,
        )));
        assert!(level.writes.is_empty());
    }

    /// A non-replaceable, non-spike cell below the origin stops the pillar:
    /// the spike shell writes above it but the pillar never overrides it.
    #[test]
    fn pillar_stops_at_foreign_cell() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 52, 0);
        level.states.insert(origin, state());
        // The pillar cursor is `origin.offset(0, -1, 0)`; a foreign (granite,
        // non-air, not replaceable) cell there breaks the pillar descent.
        let foreign = origin.offset(0, -1, 0);
        level
            .states
            .insert(foreign, BlockState::of(Blocks::GRANITE.id()));
        let generator = TestGenerator;
        let config = SpikeConfiguration::new(state(), always_true(), Arc::new(not(always_true())));
        let mut random = ZeroRandom;
        assert!(SPIKE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        )));
        assert!(!level.writes.iter().any(|(p, _)| *p == foreign));
        // Layer 0's origin cell is the non-replaceable spike state (not
        // written), so layer 0 contributes 4 cells, layer 1 five, and layers
        // 2..=6 (circle shrunk to the center) one each.
        assert_eq!(level.writes.len(), 4 + 5 + 5);
    }
}
