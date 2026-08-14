//! Port of `net.minecraft.world.level.levelgen.feature.trunkplacers.
//! FancyTrunkPlacer` (class, 26.2).
//!
//! `CODEC` is the shared `trunkPlacerParts(i).apply(i, FancyTrunkPlacer::new)`
//! three-field record. `placeTrunk` builds the tapered trunk plus the
//! `Mth`-table branch limbs:
//!
//! - `makeLimb` walks a Bresenham-like line between two positions, placing a
//!   log with a `RotatedPillarBlock.AXIS` modifier (`getLogAxis`) when
//!   `doPlace`, or checking `isFree` at every step otherwise.
//! - `makeBranches` grows a limb from each foliage coordinate's base up to its
//!   attachment.
//! - `treeShape` is the static crown-radius function.
//!
//! The `FoliageCoords` inner record pairs an attachment with the branch base
//! height. All float/double widening follows Java exactly (`Mth.cos`/`sin`
//! take the widened `double`, `Mth.floor` the `double`/`float` overloads).

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::foliageplacers::foliage_placer::FoliageAttachment;
use crate::levelgen::feature::trunkplacers::trunk_placer::{
    TrunkPlacer, place_below_trunk_block, trunk_placer_parts,
};
use crate::levelgen::feature::trunkplacers::trunk_placer_type::{
    TrunkPlacerTypeId, TrunkPlacerTypes,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::core::Axis;
use rivet_registry::core::BlockPos;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use std::any::Any;
use std::sync::Arc;

/// `FancyTrunkPlacer.TRUNK_HEIGHT_SCALE` — the `0.618` trunk-height fraction.
const TRUNK_HEIGHT_SCALE: f64 = 0.618;
/// `FancyTrunkPlacer.CLUSTER_DENSITY_MAGIC` — the `1.382` cluster density base.
const CLUSTER_DENSITY_MAGIC: f64 = 1.382;
/// `FancyTrunkPlacer.BRANCH_SLOPE` — the `0.381` branch height slope.
const BRANCH_SLOPE: f64 = 0.381;
/// `FancyTrunkPlacer.BRANCH_LENGTH_MAGIC` — the `0.328` radius offset.
const BRANCH_LENGTH_MAGIC: f64 = 0.328;

/// `net.minecraft.world.level.levelgen.feature.trunkplacers.FancyTrunkPlacer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FancyTrunkPlacer {
    /// `this.baseHeight`.
    base_height: i32,
    /// `this.heightRandA`.
    height_rand_a: i32,
    /// `this.heightRandB`.
    height_rand_b: i32,
}

impl FancyTrunkPlacer {
    /// `new FancyTrunkPlacer(int, int, int)`.
    pub fn new(base_height: i32, height_rand_a: i32, height_rand_b: i32) -> FancyTrunkPlacer {
        FancyTrunkPlacer {
            base_height,
            height_rand_a,
            height_rand_b,
        }
    }

    /// `this.baseHeight`.
    pub fn base_height(&self) -> i32 {
        self.base_height
    }

    /// `this.heightRandA`.
    pub fn height_rand_a(&self) -> i32 {
        self.height_rand_a
    }

    /// `this.heightRandB`.
    pub fn height_rand_b(&self) -> i32 {
        self.height_rand_b
    }

    /// `FancyTrunkPlacer.treeShape(int height, int y)` (private static) — the
    /// crown radius at relative height `y`, `-1.0F` below `height * 0.3F`.
    fn tree_shape(height: i32, y: i32) -> f32 {
        if (y as f32) < height as f32 * 0.3f32 {
            return -1.0f32;
        }

        let radius = height as f32 / 2.0f32;
        let adjacent = radius - y as f32;
        let mut distance = rivet_util::mth::sqrt(radius * radius - adjacent * adjacent);
        if adjacent == 0.0f32 {
            distance = radius;
        } else if rivet_util::mth::abs(adjacent) >= radius {
            return 0.0f32;
        }

        distance * 0.5f32
    }
}

impl TrunkPlacer for FancyTrunkPlacer {
    fn type_id(&self) -> TrunkPlacerTypeId {
        TrunkPlacerTypes::FANCY_TRUNK_PLACER
    }

    #[allow(clippy::neg_cmp_op_on_partial_ord)] // `!(treeShape < 0.0F)` mirrors Java's guarded form.
    fn place_trunk<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        tree_height: i32,
        origin: &BlockPos,
        config: &TreeConfiguration,
    ) -> Vec<FoliageAttachment> {
        let height = tree_height.wrapping_add(2);
        // `Mth.floor(height * 0.618)` — the double overload.
        let trunk_height = rivet_util::mth::floor_d(height as f64 * TRUNK_HEIGHT_SCALE);
        place_below_trunk_block(level, trunk_setter, random, &origin.below(), config);
        // `Math.min(1, Mth.floor(1.382 + Math.pow(1.0 * height / 13.0, 2.0)))`
        // — `Math.pow(x, 2.0)` is the fdlibm `y == 2.0` fast path `x * x`.
        let height_ratio = height as f64 / 13.0;
        let clusters_per_y = 1.min(rivet_util::mth::floor_d(
            CLUSTER_DENSITY_MAGIC + height_ratio * height_ratio,
        ));
        let trunk_top = origin.get_y().wrapping_add(trunk_height);
        let mut relative_y = height.wrapping_sub(5);
        let mut foliage_coords = Vec::new();
        foliage_coords.push(FoliageCoords::new(
            origin.above_steps(relative_y),
            trunk_top,
        ));

        while relative_y >= 0 {
            let tree_shape = Self::tree_shape(height, relative_y);
            if !(tree_shape < 0.0f32) {
                for _i in 0..clusters_per_y {
                    // `radius = 1.0 * treeShape * (random.nextFloat() + 0.328)`
                    // — Java widens to double.
                    let radius = 1.0f64
                        * tree_shape as f64
                        * (random.next_float() as f64 + BRANCH_LENGTH_MAGIC);
                    // `angle = random.nextFloat() * 2.0F * Math.PI` — double.
                    let angle = random.next_float() as f64 * 2.0f64 * std::f64::consts::PI;
                    let x = radius * angle.sin() + 0.5;
                    let z = radius * angle.cos() + 0.5;
                    let check_start = origin.offset(
                        rivet_util::mth::floor_d(x),
                        relative_y.wrapping_sub(1),
                        rivet_util::mth::floor_d(z),
                    );
                    let check_end = check_start.above_steps(5);
                    if self.make_limb(
                        level,
                        trunk_setter,
                        random,
                        &check_start,
                        &check_end,
                        false,
                        config,
                    ) {
                        let dx = origin.get_x().wrapping_sub(check_start.get_x());
                        let dz = origin.get_z().wrapping_sub(check_start.get_z());
                        // `checkStart.getY() - Math.sqrt(dx * dx + dz * dz) *
                        // 0.381` — the double `sqrt` widens the int product.
                        let distance_sq =
                            dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) as f64;
                        let branch_height =
                            check_start.get_y() as f64 - distance_sq.sqrt() * BRANCH_SLOPE;
                        let branch_top = if branch_height > trunk_top as f64 {
                            trunk_top
                        } else {
                            branch_height as i32
                        };
                        let check_branch_base =
                            BlockPos::new(origin.get_x(), branch_top, origin.get_z());
                        if self.make_limb(
                            level,
                            trunk_setter,
                            random,
                            &check_branch_base,
                            &check_start,
                            false,
                            config,
                        ) {
                            foliage_coords
                                .push(FoliageCoords::new(check_start, check_branch_base.get_y()));
                        }
                    }
                }
            }

            relative_y = relative_y.wrapping_sub(1);
        }

        self.make_limb(
            level,
            trunk_setter,
            random,
            origin,
            &origin.above_steps(trunk_height),
            true,
            config,
        );
        self.make_branches(
            level,
            trunk_setter,
            random,
            height,
            origin,
            &foliage_coords,
            config,
        );
        let mut attachments = Vec::new();

        for foliage_coord in &foliage_coords {
            if self.trim_branches(
                height,
                foliage_coord.get_branch_base().wrapping_sub(origin.get_y()),
            ) {
                attachments.push(foliage_coord.attachment.clone());
            }
        }

        attachments
    }

    fn get_base_height(&self) -> i32 {
        self.base_height
    }

    fn base_height(&self) -> i32 {
        self.base_height
    }

    fn height_rand_a(&self) -> i32 {
        self.height_rand_a
    }

    fn height_rand_b(&self) -> i32 {
        self.height_rand_b
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl FancyTrunkPlacer {
    /// `FancyTrunkPlacer.makeLimb(...)` (private instance) — the line-walk limb
    /// placer/validator.
    #[allow(clippy::too_many_arguments)] // mirrors Java `makeLimb(WorldGenLevel, Consumer, Random, BlockPos, BlockPos, boolean, TreeConfiguration)`.
    fn make_limb<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        start_pos: &BlockPos,
        end_pos: &BlockPos,
        do_place: bool,
        config: &TreeConfiguration,
    ) -> bool {
        // `if (!doPlace && Objects.equals(startPos, endPos)) return true;`.
        if !do_place && start_pos == end_pos {
            return true;
        }

        let delta = end_pos.offset(-start_pos.get_x(), -start_pos.get_y(), -start_pos.get_z());
        let steps = Self::get_steps(&delta);
        // `(float)delta.getX() / steps` — the float division.
        let dx = delta.get_x() as f32 / steps as f32;
        let dy = delta.get_y() as f32 / steps as f32;
        let dz = delta.get_z() as f32 / steps as f32;

        for i in 0..=steps {
            let block_pos = start_pos.offset(
                rivet_util::mth::floor(0.5f32 + i as f32 * dx),
                rivet_util::mth::floor(0.5f32 + i as f32 * dy),
                rivet_util::mth::floor(0.5f32 + i as f32 * dz),
            );
            if do_place {
                self.place_log_with_modifier(
                    level,
                    trunk_setter,
                    random,
                    &block_pos,
                    config,
                    &|state: BlockState| {
                        state
                            .try_set_value(
                                BlockStateProperties::AXIS,
                                Self::get_log_axis(start_pos, &block_pos),
                            )
                            .expect("FancyTrunkPlacer set a valid axis")
                    },
                );
            } else if !self.is_free(level, &block_pos) {
                return false;
            }
        }

        true
    }

    /// `FancyTrunkPlacer.getSteps(BlockPos)` — `max(abs(x), max(abs(y),
    /// abs(z)))` with Java `Mth.abs(int)` (wrapping for `MIN_VALUE`).
    fn get_steps(pos: &BlockPos) -> i32 {
        let abs_x = rivet_util::mth::abs_i32(pos.get_x());
        let abs_y = rivet_util::mth::abs_i32(pos.get_y());
        let abs_z = rivet_util::mth::abs_i32(pos.get_z());
        abs_x.max(abs_y.max(abs_z))
    }

    /// `FancyTrunkPlacer.getLogAxis(BlockPos, BlockPos)` — the pillar axis of
    /// a limb step: `X` when the x difference dominates, else `Z`, else `Y`.
    fn get_log_axis(start_pos: &BlockPos, block_pos: &BlockPos) -> Axis {
        let mut axis = Axis::Y;
        let xdiff = rivet_util::mth::abs_i32(block_pos.get_x().wrapping_sub(start_pos.get_x()));
        let zdiff = rivet_util::mth::abs_i32(block_pos.get_z().wrapping_sub(start_pos.get_z()));
        let maxdiff = xdiff.max(zdiff);
        if maxdiff > 0 {
            if xdiff == maxdiff {
                axis = Axis::X;
            } else {
                axis = Axis::Z;
            }
        }

        axis
    }

    /// `FancyTrunkPlacer.trimBranches(int height, int localY)` —
    /// `localY >= height * 0.2`.
    fn trim_branches(&self, height: i32, local_y: i32) -> bool {
        (local_y as f64) >= height as f64 * 0.2
    }

    /// `FancyTrunkPlacer.makeBranches(...)` (private instance) — grow the limb
    /// from each foliage coordinate's branch base to its attachment.
    #[allow(clippy::too_many_arguments)] // mirrors Java `makeBranches(WorldGenLevel, Consumer, Random, int, BlockPos, List<FoliageCoords>, TreeConfiguration)`.
    fn make_branches<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        height: i32,
        origin: &BlockPos,
        foliage_coords: &[FoliageCoords],
        config: &TreeConfiguration,
    ) {
        for end_coord in foliage_coords {
            let branch_base = end_coord.get_branch_base();
            let base_coord = BlockPos::new(origin.get_x(), branch_base, origin.get_z());
            if base_coord != end_coord.attachment.pos
                && self.trim_branches(height, branch_base.wrapping_sub(origin.get_y()))
            {
                self.make_limb(
                    level,
                    trunk_setter,
                    random,
                    &base_coord,
                    &end_coord.attachment.pos,
                    true,
                    config,
                );
            }
        }
    }
}

/// `FancyTrunkPlacer.FoliageCoords` (private static) — the pair of the foliage
/// attachment and its branch-base height.
#[derive(Debug, Clone)]
struct FoliageCoords {
    /// `this.attachment` — the `FoliageAttachment(pos, 0, false)`.
    attachment: FoliageAttachment,
    /// `this.branchBase`.
    branch_base: i32,
}

impl FoliageCoords {
    /// `new FoliageCoords(BlockPos, int)`.
    fn new(pos: BlockPos, branch_base: i32) -> FoliageCoords {
        FoliageCoords {
            attachment: FoliageAttachment::new(pos, 0, false),
            branch_base,
        }
    }

    /// `getBranchBase()`.
    fn get_branch_base(&self) -> i32 {
        self.branch_base
    }
}

/// `FancyTrunkPlacer.CODEC` — the shared three-field trunk-placer record, as
/// the ops-generic `fancy_trunk_placer_map_codec::<Ops>()` factory.
#[allow(clippy::type_complexity)]
pub fn fancy_trunk_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<FancyTrunkPlacer, Ops>> {
    record_builder::map_codec::<FancyTrunkPlacer, Ops>(move |instance| {
        let (base, height_rand_a, height_rand_b) = trunk_placer_parts::<FancyTrunkPlacer, Ops>(
            Arc::new(|p: &FancyTrunkPlacer| p.base_height),
            Arc::new(|p: &FancyTrunkPlacer| p.height_rand_a),
            Arc::new(|p: &FancyTrunkPlacer| p.height_rand_b),
        );
        instance
            .group(base)
            .and(height_rand_a)
            .and(height_rand_b)
            .apply(instance, Arc::new(FancyTrunkPlacer::new))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::map_codec;
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn codec_round_trips_the_three_field_record() {
        let codec = map_codec::codec_of(fancy_trunk_placer_map_codec::<JsonOps>());
        let input = json!({
            "base_height": 12,
            "height_rand_a": 5,
            "height_rand_b": 4,
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            TrunkPlacer::type_id(decoded),
            TrunkPlacerTypes::FANCY_TRUNK_PLACER
        );
        assert_eq!(decoded.get_base_height(), 12);
        assert_eq!(decoded.height_rand_a(), 5);
        assert_eq!(decoded.height_rand_b(), 4);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn tree_shape_tapers_with_height() {
        // Below `height * 0.3` the shape is -1; the crown radius widens toward
        // the middle and narrows above it.
        assert_eq!(FancyTrunkPlacer::tree_shape(10, 0), -1.0f32);
        assert_eq!(FancyTrunkPlacer::tree_shape(10, 2), -1.0f32);
        assert!(FancyTrunkPlacer::tree_shape(10, 3) > 0.0f32);
        assert!(FancyTrunkPlacer::tree_shape(10, 5) > 0.0f32);
        // At the very top the adjacent equals the radius, returning 0.
        assert_eq!(FancyTrunkPlacer::tree_shape(10, 10), 0.0f32);
    }

    #[test]
    fn place_trunk_produces_a_trunk_and_limbs() {
        let placer = FancyTrunkPlacer::new(1, 0, 0);
        let config = TreeConfiguration::stub();
        let mut random = rivet_util::random::LegacyRandomSource::new(13);
        let origin = BlockPos::new(0, 64, 0);
        let mut placed = BTreeSet::new();
        let mut setter = |pos: &BlockPos, _state: BlockState| {
            placed.insert(*pos);
        };
        let attachments = placer.place_trunk(
            &TestLevel::air(),
            &mut setter,
            &mut random,
            10,
            &origin,
            &config,
        );
        // Below-trunk block placed; the central trunk column is present.
        assert!(placed.contains(&BlockPos::new(0, 63, 0)));
        assert!(placed.contains(&BlockPos::new(0, 64, 0)));
        // Limbs extend beyond the origin column.
        let max_abs = placed
            .iter()
            .map(|p| p.get_x().abs().max(p.get_z().abs()))
            .max()
            .unwrap();
        assert!(max_abs >= 1, "limbs should branch out, got max {max_abs}");
        // A fixed seed yields a deterministic attachment set.
        assert!(!attachments.is_empty());
        assert!(attachments.iter().all(|a| !a.double_trunk));
    }

    /// A world double with a real per-position block map: air by default, with
    /// positions explicitly seeded to another state (a log, stone, leaves, …).
    /// `is_state_at_position` answers from the queried position, so predicates
    /// evaluate the real column instead of a fabricated AIR everywhere; tests
    /// that seed occupied positions exercise `place_log_if_free`'s skip path
    /// and `is_free`'s `LOGS` branch.
    struct TestLevel {
        blocks: std::collections::BTreeMap<BlockPos, BlockState>,
    }

    impl TestLevel {
        fn air() -> TestLevel {
            TestLevel {
                blocks: std::collections::BTreeMap::new(),
            }
        }
    }

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, pos: &BlockPos) -> BlockState {
            self.blocks
                .get(pos)
                .copied()
                .unwrap_or_else(|| crate::block::blocks::Blocks::AIR.default_block_state())
        }

        fn is_state_at_position(&self, pos: &BlockPos, test: &dyn Fn(&BlockState) -> bool) -> bool {
            test(&self.get_block_state(pos))
        }
    }
}
