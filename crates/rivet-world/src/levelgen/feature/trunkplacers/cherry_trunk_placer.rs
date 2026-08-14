//! Port of `net.minecraft.world.level.levelgen.feature.trunkplacers.
//! CherryTrunkPlacer` (class, 26.2).
//!
//! `CODEC` is `trunkPlacerParts(i).and(i.group(branch_count,
//! branch_horizontal_length, branch_start_offset_from_top,
//! branch_end_offset_from_top)).apply(i, CherryTrunkPlacer::new)` — the shared
//! three-field trunk record plus the nested four-field group. The
//! `branch_start_offset_from_top` field is `IntProviders.validateCodec(-16, 0,
//! BRANCH_START_CODEC)` where `BRANCH_START_CODEC` is the `UniformInt.MAP_CODEC`
//! validated to require at least 2 blocks of variation (so both branch starts
//! fit). `placeTrunk` grows the straight trunk to a height derived from the
//! branch offsets, then the one or two side branches (`generateBranch`) with the
//! `RotatedPillarBlock.AXIS` sideways state modifier.

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
use rivet_registry::core::BlockPos;
use rivet_registry::core::Direction;
use rivet_registry::core::MutableBlockPos;
use rivet_registry::core::Plane;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use rivet_util::valueproviders::uniform_int::{UniformInt, uniform_int_map_codec};
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.trunkplacers.CherryTrunkPlacer`.
#[derive(Debug, Clone, PartialEq)]
pub struct CherryTrunkPlacer {
    /// `this.baseHeight`.
    base_height: i32,
    /// `this.heightRandA`.
    height_rand_a: i32,
    /// `this.heightRandB`.
    height_rand_b: i32,
    /// `this.branchCount` — 1, 2, or 3 branches.
    branch_count: IntProvider,
    /// `this.branchHorizontalLength` — the horizontal branch reach.
    branch_horizontal_length: IntProvider,
    /// `this.branchStartOffsetFromTop` — the first branch's start offset.
    branch_start_offset_from_top: UniformInt,
    /// `this.secondBranchStartOffsetFromTop` — the second branch's start offset.
    second_branch_start_offset_from_top: UniformInt,
    /// `this.branchEndOffsetFromTop` — the branch-end offset.
    branch_end_offset_from_top: IntProvider,
}

impl CherryTrunkPlacer {
    /// `new CherryTrunkPlacer(int, int, int, IntProvider, IntProvider,
    /// UniformInt, IntProvider)` — `secondBranchStartOffsetFromTop =
    /// UniformInt.of(minInclusive(), maxInclusive() - 1)`.
    pub fn new(
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
        branch_count: IntProvider,
        branch_horizontal_length: IntProvider,
        branch_start_offset_from_top: UniformInt,
        branch_end_offset_from_top: IntProvider,
    ) -> CherryTrunkPlacer {
        CherryTrunkPlacer {
            base_height,
            height_rand_a,
            height_rand_b,
            branch_count,
            branch_horizontal_length,
            branch_start_offset_from_top,
            second_branch_start_offset_from_top: UniformInt::of(
                branch_start_offset_from_top.min_inclusive(),
                branch_start_offset_from_top.max_inclusive().wrapping_sub(1),
            ),
            branch_end_offset_from_top,
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

    /// `this.branchCount`.
    pub fn branch_count(&self) -> &IntProvider {
        &self.branch_count
    }

    /// `this.branchHorizontalLength`.
    pub fn branch_horizontal_length(&self) -> &IntProvider {
        &self.branch_horizontal_length
    }

    /// `this.branchStartOffsetFromTop`.
    pub fn branch_start_offset_from_top(&self) -> UniformInt {
        self.branch_start_offset_from_top
    }

    /// `this.secondBranchStartOffsetFromTop`.
    pub fn second_branch_start_offset_from_top(&self) -> UniformInt {
        self.second_branch_start_offset_from_top
    }

    /// `this.branchEndOffsetFromTop`.
    pub fn branch_end_offset_from_top(&self) -> &IntProvider {
        &self.branch_end_offset_from_top
    }
}

impl TrunkPlacer for CherryTrunkPlacer {
    fn type_id(&self) -> TrunkPlacerTypeId {
        TrunkPlacerTypes::CHERRY_TRUNK_PLACER
    }

    fn place_trunk<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        tree_height: i32,
        origin: &BlockPos,
        config: &TreeConfiguration,
    ) -> Vec<FoliageAttachment> {
        place_below_trunk_block(level, trunk_setter, random, &origin.below(), config);
        // `Math.max(0, treeHeight - 1 + sample(...))`.
        let first_branch_offset_from_origin = 0.max(
            tree_height
                .wrapping_sub(1)
                .wrapping_add(self.branch_start_offset_from_top.sample(random)),
        );
        let mut second_branch_offset_from_origin = 0.max(
            tree_height
                .wrapping_sub(1)
                .wrapping_add(self.second_branch_start_offset_from_top.sample(random)),
        );
        if second_branch_offset_from_origin >= first_branch_offset_from_origin {
            second_branch_offset_from_origin = second_branch_offset_from_origin.wrapping_add(1);
        }

        let branch_count = self.branch_count.sample(random);
        let has_middle_branch = branch_count == 3;
        let has_both_side_branches = branch_count >= 2;
        let trunk_height = if has_middle_branch {
            tree_height
        } else if has_both_side_branches {
            first_branch_offset_from_origin
                .max(second_branch_offset_from_origin)
                .wrapping_add(1)
        } else {
            first_branch_offset_from_origin.wrapping_add(1)
        };

        for y in 0..trunk_height {
            self.place_log(level, trunk_setter, random, &origin.above_steps(y), config);
        }

        let mut attachments = Vec::new();
        if has_middle_branch {
            attachments.push(FoliageAttachment::new(
                origin.above_steps(trunk_height),
                0,
                false,
            ));
        }

        let mut log_pos = MutableBlockPos::new(0, 0, 0);
        let tree_direction = Plane::Horizontal.get_random_direction(random);
        // `state -> state.trySetValue(RotatedPillarBlock.AXIS,
        // treeDirection.getAxis())`.
        let sideways_state_modifier = |state: BlockState| {
            state
                .try_set_value(BlockStateProperties::AXIS, tree_direction.get_axis())
                .expect("CherryTrunkPlacer set a valid axis")
        };
        attachments.push(self.generate_branch(
            level,
            trunk_setter,
            random,
            tree_height,
            origin,
            config,
            &sideways_state_modifier,
            &tree_direction,
            first_branch_offset_from_origin,
            first_branch_offset_from_origin < trunk_height.wrapping_sub(1),
            &mut log_pos,
        ));
        if has_both_side_branches {
            attachments.push(self.generate_branch(
                level,
                trunk_setter,
                random,
                tree_height,
                origin,
                config,
                &sideways_state_modifier,
                &tree_direction.get_opposite(),
                second_branch_offset_from_origin,
                second_branch_offset_from_origin < trunk_height.wrapping_sub(1),
                &mut log_pos,
            ));
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

impl CherryTrunkPlacer {
    /// `CherryTrunkPlacer.generateBranch(...)` (private instance) — grow one
    /// branch from its trunk offset to its end, walking horizontally first then
    /// diagonally toward the end with the per-step vertical chance.
    #[allow(clippy::too_many_arguments)]
    fn generate_branch<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        tree_height: i32,
        origin: &BlockPos,
        config: &TreeConfiguration,
        sideways_state_modifier: &dyn Fn(BlockState) -> BlockState,
        branch_direction: &Direction,
        offset_from_origin: i32,
        middle_continues_upwards: bool,
        log_pos: &mut MutableBlockPos,
    ) -> FoliageAttachment {
        log_pos.set(origin.get_x(), origin.get_y(), origin.get_z());
        log_pos.move_dir_steps(&Direction::Up, offset_from_origin);
        let branch_end_pos_offset_from_origin = tree_height
            .wrapping_sub(1)
            .wrapping_add(self.branch_end_offset_from_top.sample(random));
        let extend_branch_away_from_trunk =
            middle_continues_upwards || branch_end_pos_offset_from_origin < offset_from_origin;
        let distance_to_trunk = self
            .branch_horizontal_length
            .sample(random)
            .wrapping_add(if extend_branch_away_from_trunk { 1 } else { 0 });
        let branch_end_pos = origin
            .relative_steps(branch_direction, distance_to_trunk)
            .above_steps(branch_end_pos_offset_from_origin);
        let steps_horizontally = if extend_branch_away_from_trunk { 2 } else { 1 };

        for _i in 0..steps_horizontally {
            log_pos.move_dir(branch_direction);
            self.place_log_with_modifier(
                level,
                trunk_setter,
                random,
                &log_pos.immutable(),
                config,
                sideways_state_modifier,
            );
        }

        let vertical_direction = if branch_end_pos.get_y() > log_pos.get_y() {
            Direction::Up
        } else {
            Direction::Down
        };

        loop {
            let distance = log_pos.immutable().dist_manhattan(&branch_end_pos);
            if distance == 0 {
                return FoliageAttachment::new(branch_end_pos.above(), 0, false);
            }

            // `(float)Math.abs(branchEndPos.getY() - logPos.getY()) / distance`.
            let chance_to_grow_vertically =
                rivet_util::mth::abs_i32(branch_end_pos.get_y().wrapping_sub(log_pos.get_y()))
                    as f32
                    / distance as f32;
            let grow_vertically = random.next_float() < chance_to_grow_vertically;
            if grow_vertically {
                log_pos.move_dir(&vertical_direction);
                // `Function.identity()`.
                self.place_log_with_modifier(
                    level,
                    trunk_setter,
                    random,
                    &log_pos.immutable(),
                    config,
                    &|s| s,
                );
            } else {
                log_pos.move_dir(branch_direction);
                self.place_log_with_modifier(
                    level,
                    trunk_setter,
                    random,
                    &log_pos.immutable(),
                    config,
                    sideways_state_modifier,
                );
            }
        }
    }
}

/// `CherryTrunkPlacer.CODEC` — the shared three-field trunk record combined
/// with the nested `i.group(branch_count, branch_horizontal_length,
/// branch_start_offset_from_top, branch_end_offset_from_top)`, as the
/// ops-generic `cherry_trunk_placer_map_codec::<Ops>()` factory.
#[allow(clippy::type_complexity)]
pub fn cherry_trunk_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<CherryTrunkPlacer, Ops>> {
    record_builder::map_codec::<CherryTrunkPlacer, Ops>(move |instance| {
        let (base, height_rand_a, height_rand_b) = trunk_placer_parts::<CherryTrunkPlacer, Ops>(
            Arc::new(|p: &CherryTrunkPlacer| p.base_height),
            Arc::new(|p: &CherryTrunkPlacer| p.height_rand_a),
            Arc::new(|p: &CherryTrunkPlacer| p.height_rand_b),
        );
        // `BRANCH_START_CODEC` — `UniformInt.MAP_CODEC.codec().validate(...)`,
        // requiring `maxInclusive - minInclusive >= 1` so both branch starts
        // fit.
        let branch_start_codec: Arc<dyn Codec<UniformInt, Ops>> = codec::validate(
            map_codec::codec_of(uniform_int_map_codec::<Ops>()),
            Arc::new(|u: &UniformInt| {
                if u.max_inclusive().wrapping_sub(u.min_inclusive()) < 1 {
                    DataResult::error(
                        "Need at least 2 blocks variation for the branch starts to fit both branches"
                            .to_string(),
                    )
                } else {
                    DataResult::success(*u)
                }
            }),
        );
        // `IntProviders.validateCodec(-16, 0, BRANCH_START_CODEC)` — the outer
        // [-16, 0] bound check over the lifted uniform codec.
        let branch_start_codec: Arc<dyn Codec<UniformInt, Ops>> = codec::validate(
            branch_start_codec,
            Arc::new(|u: &UniformInt| {
                if u.min_inclusive() < -16 {
                    DataResult::error(format!(
                        "Value provider too low: {} [{}-{}]",
                        -16,
                        u.min_inclusive(),
                        u.max_inclusive()
                    ))
                } else if u.max_inclusive() > 0 {
                    DataResult::error(format!(
                        "Value provider too high: {} [{}-{}]",
                        0,
                        u.min_inclusive(),
                        u.max_inclusive()
                    ))
                } else {
                    DataResult::success(*u)
                }
            }),
        );
        // `i.group(...)` — the nested four-field group, materialized as the
        // `(IntProvider, IntProvider, UniformInt, IntProvider)` value the outer
        // record's fourth field carries.
        let inner = instance
            .group(RecordCodecBuilder::of(
                Arc::new(|p: &CherryTrunkPlacer| p.branch_count.clone()),
                codec::field_of(
                    int_provider_codec_with_bounds::<Ops>(1, 3),
                    "branch_count".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &CherryTrunkPlacer| p.branch_horizontal_length.clone()),
                codec::field_of(
                    int_provider_codec_with_bounds::<Ops>(2, 16),
                    "branch_horizontal_length".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &CherryTrunkPlacer| p.branch_start_offset_from_top),
                codec::field_of(
                    branch_start_codec,
                    "branch_start_offset_from_top".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &CherryTrunkPlacer| p.branch_end_offset_from_top.clone()),
                codec::field_of(
                    int_provider_codec_with_bounds::<Ops>(-16, 16),
                    "branch_end_offset_from_top".to_string(),
                ),
            ))
            .apply(
                instance,
                Arc::new(
                    |branch_count: IntProvider,
                     branch_horizontal_length: IntProvider,
                     branch_start_offset_from_top: UniformInt,
                     branch_end_offset_from_top: IntProvider| {
                        (
                            branch_count,
                            branch_horizontal_length,
                            branch_start_offset_from_top,
                            branch_end_offset_from_top,
                        )
                    },
                ),
            );
        instance
            .group(base)
            .and(height_rand_a)
            .and(height_rand_b)
            .and(inner)
            .apply(
                instance,
                Arc::new(
                    |base_height: i32,
                     height_rand_a: i32,
                     height_rand_b: i32,
                     group: (IntProvider, IntProvider, UniformInt, IntProvider)| {
                        CherryTrunkPlacer::new(
                            base_height,
                            height_rand_a,
                            height_rand_b,
                            group.0,
                            group.1,
                            group.2,
                            group.3,
                        )
                    },
                ),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;
    use std::collections::BTreeSet;

    fn uniform(min: i32, max: i32) -> IntProvider {
        IntProvider::Uniform(UniformInt::of(min, max))
    }

    fn field_json() -> serde_json::Value {
        json!({
            "base_height": 8,
            "height_rand_a": 3,
            "height_rand_b": 2,
            "branch_count": {"min_inclusive": 2, "max_inclusive": 3, "type": "minecraft:uniform"},
            "branch_horizontal_length": {"min_inclusive": 3, "max_inclusive": 8, "type": "minecraft:uniform"},
            "branch_start_offset_from_top": {"min_inclusive": -5, "max_inclusive": -1},
            "branch_end_offset_from_top": {"min_inclusive": -3, "max_inclusive": 4, "type": "minecraft:uniform"},
        })
    }

    #[test]
    fn codec_round_trips_the_seven_field_record() {
        let codec = map_codec::codec_of(cherry_trunk_placer_map_codec::<JsonOps>());
        let input = field_json();
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            TrunkPlacer::type_id(decoded),
            TrunkPlacerTypes::CHERRY_TRUNK_PLACER
        );
        assert_eq!(decoded.get_base_height(), 8);
        assert_eq!(decoded.branch_count().min_inclusive(), 2);
        assert_eq!(decoded.branch_horizontal_length().max_inclusive(), 8);
        assert_eq!(
            decoded.branch_start_offset_from_top(),
            UniformInt::of(-5, -1)
        );
        // `new` derives the second branch start as `(min, max - 1)`.
        assert_eq!(
            decoded.second_branch_start_offset_from_top(),
            UniformInt::of(-5, -2)
        );
        assert_eq!(decoded.branch_end_offset_from_top().min_inclusive(), -3);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn codec_rejects_branch_start_without_two_blocks_variation() {
        // `BRANCH_START_CODEC.validate(...)` — a constant start cannot fit both
        // branch starts.
        let codec = map_codec::codec_of(cherry_trunk_placer_map_codec::<JsonOps>());
        let result = codec.parse(
            &JsonOps::INSTANCE,
            &json!({
                "base_height": 8,
                "height_rand_a": 3,
                "height_rand_b": 2,
                "branch_count": {"min_inclusive": 2, "max_inclusive": 3, "type": "minecraft:uniform"},
                "branch_horizontal_length": {"min_inclusive": 3, "max_inclusive": 8, "type": "minecraft:uniform"},
                "branch_start_offset_from_top": {"min_inclusive": -2, "max_inclusive": -2},
                "branch_end_offset_from_top": {"min_inclusive": -3, "max_inclusive": 4, "type": "minecraft:uniform"},
            }),
        );
        assert!(result.is_error(), "got: {:?}", result);
    }

    #[test]
    fn codec_rejects_out_of_range_branch_start() {
        // `IntProviders.validateCodec(-16, 0, ...)` — a min below -16 fails.
        let codec = map_codec::codec_of(cherry_trunk_placer_map_codec::<JsonOps>());
        let result = codec.parse(
            &JsonOps::INSTANCE,
            &json!({
                "base_height": 8,
                "height_rand_a": 3,
                "height_rand_b": 2,
                "branch_count": {"min_inclusive": 2, "max_inclusive": 3, "type": "minecraft:uniform"},
                "branch_horizontal_length": {"min_inclusive": 3, "max_inclusive": 8, "type": "minecraft:uniform"},
                "branch_start_offset_from_top": {"min_inclusive": -17, "max_inclusive": -16},
                "branch_end_offset_from_top": {"min_inclusive": -3, "max_inclusive": 4, "type": "minecraft:uniform"},
            }),
        );
        assert!(result.is_error(), "got: {:?}", result);
    }

    #[test]
    fn codec_rejects_out_of_range_branch_count() {
        // `IntProviders.codec(1, 3)` — min 0 fails.
        let codec = map_codec::codec_of(cherry_trunk_placer_map_codec::<JsonOps>());
        let mut input = field_json();
        input["branch_count"] =
            json!({"min_inclusive": 0, "max_inclusive": 3, "type": "minecraft:uniform"});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error(), "got: {:?}", result);
    }

    #[test]
    fn place_trunk_builds_trunk_with_middle_and_side_branches() {
        // `branchCount = 3` forces the middle branch; the trunk runs the full
        // tree height and both side branches (opposite directions) are grown.
        let placer = CherryTrunkPlacer::new(
            1,
            0,
            0,
            uniform(3, 3),
            uniform(2, 2),
            UniformInt::of(-1, 0),
            uniform(-1, 0),
        );
        let config = TreeConfiguration::stub();
        let mut random = rivet_util::random::LegacyRandomSource::new(17);
        let origin = BlockPos::new(0, 0, 0);
        let mut placed = BTreeSet::new();
        let mut setter = |pos: &BlockPos, _state: BlockState| {
            placed.insert(*pos);
        };
        let attachments = placer.place_trunk(
            &TestLevel::air(),
            &mut setter,
            &mut random,
            4,
            &origin,
            &config,
        );

        // The below-trunk block and the full-height trunk column.
        assert!(placed.contains(&BlockPos::new(0, -1, 0)));
        for y in 0..4 {
            assert!(
                placed.contains(&BlockPos::new(0, y, 0)),
                "missing trunk y={y}"
            );
        }
        // The middle branch attaches one above the trunk top.
        assert!(attachments.iter().any(|a| a.pos == BlockPos::new(0, 4, 0)));
        // Both side branches extend the footprint beyond the origin column.
        let max_abs = placed
            .iter()
            .map(|p| p.get_x().abs().max(p.get_z().abs()))
            .max()
            .unwrap();
        assert!(
            max_abs >= 2,
            "branches should extend the footprint, got max {max_abs}"
        );
        // Three attachments: the middle top and the two branch ends.
        assert_eq!(attachments.len(), 3, "attachments: {attachments:?}");
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
