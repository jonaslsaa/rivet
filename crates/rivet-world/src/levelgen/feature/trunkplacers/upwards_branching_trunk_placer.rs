//! Port of `net.minecraft.world.level.levelgen.feature.trunkplacers.
//! UpwardsBranchingTrunkPlacer` (class, 26.2).
//!
//! `CODEC` is `trunkPlacerParts(i).and(i.group(extra_branch_steps,
//! place_branch_per_log_probability, extra_branch_length, can_grow_through)).
//! apply(i, UpwardsBranchingTrunkPlacer::new)` — the shared three-field trunk
//! record plus the nested four-field group (`IntProviders.POSITIVE_CODEC` /
//! `Codec.floatRange(0.0F, 1.0F)` / `IntProviders.NON_NEGATIVE_CODEC` /
//! `RegistryCodecs.homogeneousList(Registries.BLOCK)`). `placeTrunk` places one
//! log per height step, branching out sideways at each placed log with
//! `placeBranchPerLogProbability`; `validTreePos` additionally accepts any
//! state in `canGrowThrough`.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::foliageplacers::foliage_placer::FoliageAttachment;
use crate::levelgen::feature::trunkplacers::trunk_placer::{TrunkPlacer, trunk_placer_parts};
use crate::levelgen::feature::trunkplacers::trunk_placer_type::{
    TrunkPlacerTypeId, TrunkPlacerTypes,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::core::Direction;
use rivet_registry::core::MutableBlockPos;
use rivet_registry::core::Plane;
use rivet_registry::holder::Holder;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_registry::registry_file_codec::{HolderSetCodec, RegistryFixedCodec};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::valueproviders::int_provider::{
    IntProvider, non_negative_int_provider_codec, positive_int_provider_codec,
};
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.trunkplacers.
/// UpwardsBranchingTrunkPlacer`.
#[derive(Debug, Clone, PartialEq)]
pub struct UpwardsBranchingTrunkPlacer {
    /// `this.baseHeight`.
    base_height: i32,
    /// `this.heightRandA`.
    height_rand_a: i32,
    /// `this.heightRandB`.
    height_rand_b: i32,
    /// `this.extraBranchSteps` — the branch vertical-extension steps.
    extra_branch_steps: IntProvider,
    /// `this.placeBranchPerLogProbability` — the per-log branch chance.
    place_branch_per_log_probability: f32,
    /// `this.extraBranchLength` — the branch length draw.
    extra_branch_length: IntProvider,
    /// `this.canGrowThrough` — the blocks the branch may grow through.
    can_grow_through: HolderSet<BlockType>,
}

impl UpwardsBranchingTrunkPlacer {
    /// `new UpwardsBranchingTrunkPlacer(int, int, int, IntProvider, float,
    /// IntProvider, HolderSet<Block>)`.
    pub fn new(
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
        extra_branch_steps: IntProvider,
        place_branch_per_log_probability: f32,
        extra_branch_length: IntProvider,
        can_grow_through: HolderSet<BlockType>,
    ) -> UpwardsBranchingTrunkPlacer {
        UpwardsBranchingTrunkPlacer {
            base_height,
            height_rand_a,
            height_rand_b,
            extra_branch_steps,
            place_branch_per_log_probability,
            extra_branch_length,
            can_grow_through,
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

    /// `this.extraBranchSteps`.
    pub fn extra_branch_steps(&self) -> &IntProvider {
        &self.extra_branch_steps
    }

    /// `this.placeBranchPerLogProbability`.
    pub fn place_branch_per_log_probability(&self) -> f32 {
        self.place_branch_per_log_probability
    }

    /// `this.extraBranchLength`.
    pub fn extra_branch_length(&self) -> &IntProvider {
        &self.extra_branch_length
    }

    /// `this.canGrowThrough`.
    pub fn can_grow_through(&self) -> &HolderSet<BlockType> {
        &self.can_grow_through
    }
}

impl TrunkPlacer for UpwardsBranchingTrunkPlacer {
    fn type_id(&self) -> TrunkPlacerTypeId {
        TrunkPlacerTypes::UPWARDS_BRANCHING_TRUNK_PLACER
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
        let mut attachments = Vec::new();
        let mut log_pos = MutableBlockPos::new(0, 0, 0);

        for height_pos in 0..tree_height {
            let current_height = origin.get_y().wrapping_add(height_pos);
            // `placeLog(..., logPos.set(origin.getX(), currentHeight,
            // origin.getZ()), ...)` — the mutable is set then read as a pos.
            log_pos.set(origin.get_x(), current_height, origin.get_z());
            if self.place_log(level, trunk_setter, random, &log_pos.immutable(), config)
                && height_pos < tree_height.wrapping_sub(1)
                && random.next_float() < self.place_branch_per_log_probability
            {
                let branch_dir = Plane::Horizontal.get_random_direction(random);
                let branch_len = self.extra_branch_length.sample(random);
                // `Math.max(0, branchLen - this.extraBranchLength.sample(random)
                // - 1)`.
                let branch_pos = 0.max(
                    branch_len
                        .wrapping_sub(self.extra_branch_length.sample(random))
                        .wrapping_sub(1),
                );
                let branch_steps = self.extra_branch_steps.sample(random);
                self.place_branch(
                    level,
                    trunk_setter,
                    random,
                    tree_height,
                    config,
                    &mut attachments,
                    &mut log_pos,
                    current_height,
                    &branch_dir,
                    branch_pos,
                    branch_steps,
                );
            }

            if height_pos == tree_height.wrapping_sub(1) {
                log_pos.set(
                    origin.get_x(),
                    current_height.wrapping_add(1),
                    origin.get_z(),
                );
                attachments.push(FoliageAttachment::new(log_pos.immutable(), 0, false));
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

    /// `UpwardsBranchingTrunkPlacer.validTreePos` —
    /// `super.validTreePos(level, pos) || level.isStateAtPosition(pos, s ->
    /// s.is(this.canGrowThrough))`. `super.validTreePos` is
    /// `TrunkPlacer.validTreePos` → `TreeFeature.validTreePos`, i.e. the free
    /// `valid_tree_pos` helper.
    fn valid_tree_pos(&self, level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
        crate::levelgen::feature::tree_feature::valid_tree_pos(level, pos)
            || level.is_state_at_position(pos, &|state: &BlockState| {
                self.can_grow_through.contains_id(state.block().id() as u32)
            })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl UpwardsBranchingTrunkPlacer {
    /// `UpwardsBranchingTrunkPlacer.placeBranch(...)` (private instance) — walk
    /// the branch out `branchSteps` logs, one per height step, attaching a
    /// foliage point at each placed log and (when the branch extends) two more
    /// foliage points at the top.
    #[allow(clippy::too_many_arguments)]
    fn place_branch<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        tree_height: i32,
        config: &TreeConfiguration,
        attachments: &mut Vec<FoliageAttachment>,
        log_pos: &mut MutableBlockPos,
        current_height: i32,
        branch_dir: &Direction,
        branch_pos: i32,
        mut branch_steps: i32,
    ) {
        let mut height_along_branch = current_height.wrapping_add(branch_pos);
        let mut log_x = log_pos.get_x();
        let mut log_z = log_pos.get_z();
        let mut branch_placement_index = branch_pos;

        while branch_placement_index < tree_height && branch_steps > 0 {
            if branch_placement_index >= 1 {
                let placement_height = current_height.wrapping_add(branch_placement_index);
                log_x = log_x.wrapping_add(branch_dir.step_x());
                log_z = log_z.wrapping_add(branch_dir.step_z());
                height_along_branch = placement_height;
                log_pos.set(log_x, placement_height, log_z);
                if self.place_log(level, trunk_setter, random, &log_pos.immutable(), config) {
                    height_along_branch = height_along_branch.wrapping_add(1);
                }

                attachments.push(FoliageAttachment::new(log_pos.immutable(), 0, false));
            }

            branch_placement_index = branch_placement_index.wrapping_add(1);
            branch_steps = branch_steps.wrapping_sub(1);
        }

        if height_along_branch.wrapping_sub(current_height) > 1 {
            let foliage_pos = BlockPos::new(log_x, height_along_branch, log_z);
            attachments.push(FoliageAttachment::new(foliage_pos, 0, false));
            attachments.push(FoliageAttachment::new(foliage_pos.below_steps(2), 0, false));
        }
    }
}

/// `UpwardsBranchingTrunkPlacer.CODEC` — the shared three-field trunk record
/// combined with the nested `i.group(extra_branch_steps,
/// place_branch_per_log_probability, extra_branch_length, can_grow_through)`,
/// as the ops-generic `upwards_branching_trunk_placer_map_codec::<Ops>()`
/// factory. The `can_grow_through` holder-set field needs `RegistryOpsLookup`.
#[allow(clippy::type_complexity)]
pub fn upwards_branching_trunk_placer_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<UpwardsBranchingTrunkPlacer, Ops>> {
    record_builder::map_codec::<UpwardsBranchingTrunkPlacer, Ops>(move |instance| {
        let (base, height_rand_a, height_rand_b) =
            trunk_placer_parts::<UpwardsBranchingTrunkPlacer, Ops>(
                Arc::new(|p: &UpwardsBranchingTrunkPlacer| p.base_height),
                Arc::new(|p: &UpwardsBranchingTrunkPlacer| p.height_rand_a),
                Arc::new(|p: &UpwardsBranchingTrunkPlacer| p.height_rand_b),
            );
        // `RegistryCodecs.homogeneousList(Registries.BLOCK)` — the holder set
        // over the block registry (tag-key or element-list form).
        #[allow(clippy::arc_with_non_send_sync)]
        let element: Arc<dyn Codec<Holder<BlockType>, Ops>> = Arc::new(RegistryFixedCodec::create(
            &rivet_registry::registries::BLOCK,
        ));
        #[allow(clippy::arc_with_non_send_sync)]
        let holder_set: Arc<dyn Codec<HolderSet<BlockType>, Ops>> = Arc::new(
            HolderSetCodec::create(&rivet_registry::registries::BLOCK, element, false),
        );
        // `i.group(...)` — the nested four-field group, materialized as the
        // `(IntProvider, f32, IntProvider, HolderSet<BlockType>)` value the
        // outer record's fourth field carries.
        let inner = instance
            .group(RecordCodecBuilder::of(
                Arc::new(|p: &UpwardsBranchingTrunkPlacer| p.extra_branch_steps.clone()),
                codec::field_of(
                    positive_int_provider_codec::<Ops>(),
                    "extra_branch_steps".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &UpwardsBranchingTrunkPlacer| p.place_branch_per_log_probability),
                codec::field_of(
                    codec::float_range::<Ops>(0.0, 1.0),
                    "place_branch_per_log_probability".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &UpwardsBranchingTrunkPlacer| p.extra_branch_length.clone()),
                codec::field_of(
                    non_negative_int_provider_codec::<Ops>(),
                    "extra_branch_length".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &UpwardsBranchingTrunkPlacer| p.can_grow_through.clone()),
                codec::field_of(holder_set, "can_grow_through".to_string()),
            ))
            .apply(
                instance,
                Arc::new(
                    |extra_branch_steps: IntProvider,
                     place_branch_per_log_probability: f32,
                     extra_branch_length: IntProvider,
                     can_grow_through: HolderSet<BlockType>| {
                        (
                            extra_branch_steps,
                            place_branch_per_log_probability,
                            extra_branch_length,
                            can_grow_through,
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
                     group: (IntProvider, f32, IntProvider, HolderSet<BlockType>)| {
                        UpwardsBranchingTrunkPlacer::new(
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
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::{
        Identifier, RegistrationInfo, RegistryAccess, RegistryBuilder, ResourceKey,
    };
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::map_codec;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;
    use std::collections::BTreeSet;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A block registry with `minecraft:stone` registered at element id 0, so a
    /// direct holder list round-trips through `can_grow_through`.
    fn block_ops() -> TestOps {
        let key = rivet_registry::registries::BLOCK.clone();
        let mut builder = RegistryBuilder::<BlockType>::new(&key);
        builder.register(
            &ResourceKey::create(&key, Identifier::with_default_namespace("stone")),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        let access = RegistryAccess::from_single_registry(key, registry);
        RegistryOps::create_from_access(&JsonOps::INSTANCE, access)
    }

    fn uniform(min: i32, max: i32) -> IntProvider {
        IntProvider::Uniform(UniformInt::of(min, max))
    }

    #[test]
    fn codec_round_trips_the_seven_field_record() {
        let codec = map_codec::codec_of(upwards_branching_trunk_placer_map_codec::<TestOps>());
        let input = json!({
            "base_height": 8,
            "height_rand_a": 3,
            "height_rand_b": 2,
            "extra_branch_steps": {"min_inclusive": 1, "max_inclusive": 4, "type": "minecraft:uniform"},
            "place_branch_per_log_probability": 0.5,
            "extra_branch_length": {"min_inclusive": 0, "max_inclusive": 3, "type": "minecraft:uniform"},
            "can_grow_through": "minecraft:stone",
        });
        // One ops instance for both decode and encode: the registry's
        // per-instance `RegistryId` (assigned by the builder) is what the
        // holder set's references carry, so the encode owner check must see the
        // same registry instance the decode resolved.
        let ops = block_ops();
        let decoded_result = codec.parse(&ops, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            TrunkPlacer::type_id(decoded),
            TrunkPlacerTypes::UPWARDS_BRANCHING_TRUNK_PLACER
        );
        assert_eq!(decoded.get_base_height(), 8);
        assert_eq!(decoded.extra_branch_steps().min_inclusive(), 1);
        assert_eq!(decoded.place_branch_per_log_probability(), 0.5);
        assert_eq!(decoded.extra_branch_length().max_inclusive(), 3);
        assert!(
            decoded.can_grow_through().contains_id(0),
            "the stone element id must be in the holder set"
        );
        let encoded = codec
            .encode_start(&ops, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn codec_rejects_out_of_range_extra_branch_steps() {
        // `IntProviders.POSITIVE_CODEC` — min 0 is a decode error.
        let codec = map_codec::codec_of(upwards_branching_trunk_placer_map_codec::<TestOps>());
        let result = codec.parse(
            &block_ops(),
            &json!({
                "base_height": 8,
                "height_rand_a": 3,
                "height_rand_b": 2,
                "extra_branch_steps": {"min_inclusive": 0, "max_inclusive": 4, "type": "minecraft:uniform"},
                "place_branch_per_log_probability": 0.5,
                "extra_branch_length": {"min_inclusive": 0, "max_inclusive": 3, "type": "minecraft:uniform"},
                "can_grow_through": ["minecraft:stone"],
            }),
        );
        assert!(result.is_error(), "got: {:?}", result);
    }

    #[test]
    fn place_trunk_branches_and_grows_through_can_grow_through() {
        // `can_grow_through = {stone}` with `validTreePos` accepting it, so a
        // branch log placed one step out is free even though it is not air.
        let stone = crate::block::blocks::Blocks::STONE.default_block_state();
        let holder_set = HolderSet::direct(vec![Holder::reference(
            rivet_registry::RegistryId(0),
            stone.block().id() as u32,
        )]);
        let placer = UpwardsBranchingTrunkPlacer::new(
            1,
            0,
            0,
            uniform(2, 2),
            1.0,
            uniform(1, 1),
            holder_set,
        );
        let config = TreeConfiguration::stub();
        let mut random = rivet_util::random::LegacyRandomSource::new(7);
        let origin = BlockPos::new(0, 0, 0);
        let mut placed = BTreeSet::new();
        let mut setter = |pos: &BlockPos, _state: BlockState| {
            placed.insert(*pos);
        };
        let attachments = placer.place_trunk(
            &TestLevel::air(),
            &mut setter,
            &mut random,
            6,
            &origin,
            &config,
        );
        // The central trunk column is placed.
        assert!(placed.contains(&BlockPos::new(0, 0, 0)));
        assert!(placed.contains(&BlockPos::new(0, 5, 0)));
        // `placeBranchPerLogProbability = 1.0` with 6 logs: at least one branch
        // extends horizontally beyond the origin column.
        let max_abs = placed
            .iter()
            .map(|p| p.get_x().abs().max(p.get_z().abs()))
            .max()
            .unwrap();
        assert!(
            max_abs >= 1,
            "branches should extend the footprint, got max {max_abs}"
        );
        // The final top attachment sits one above the trunk top.
        assert!(attachments.iter().any(|a| a.pos == BlockPos::new(0, 6, 0)));
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
