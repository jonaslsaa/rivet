//! FallenTreeFeature: stump, horizontal log clearance, ground-gap limit and
//! postprocessing. Source: pinned Paper 26.2 FallenTreeFeature.java.
use super::configurations::fallen_tree_configuration::FallenTreeConfiguration;
use super::stateproviders::block_state_provider::block_state_provider_get_state;
use super::tree_feature::{TreeLevel, valid_tree_pos};
use super::treedecorators::{TreeDecorator, TreeDecoratorCallbacks, TreeDecoratorContext};
use super::{FeatureBehavior, FeaturePlaceContext, mark_above_for_post_processing};
use crate::level::WorldGenLevel;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::core::{BlockPos, Direction, Plane};
use rivet_util::RandomSource;
use std::cell::RefCell;

#[derive(Debug)]
pub struct FallenTreeFeature;
pub const FALLEN_TREE: FallenTreeFeature = FallenTreeFeature;

impl FeatureBehavior<FallenTreeConfiguration> for FallenTreeFeature {
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, FallenTreeConfiguration, R>,
    ) -> bool {
        let config = context.config;
        for decorator in config.stump_decorators.iter().chain(&config.log_decorators) {
            decorator.require_supported();
        }
        let level = &mut *context.level;
        let random = &mut *context.random;
        let origin = *context.origin;
        place_log(config, level, random, &origin, None);
        decorate_logs(
            level,
            context.chunk_generator,
            random,
            &[origin],
            &config.stump_decorators,
        );
        let direction = Plane::Horizontal.get_random_direction(random);
        let length = config.log_length.sample(random) - 2;
        let mut start = origin
            .relative_steps(&direction, 2 + random.next_int_bound(2))
            .above();
        for _ in 0..6 {
            if valid_tree_pos(level, &start) && over_solid_ground(level, &start) {
                break;
            }
            start = start.below();
        }
        let mut gap = 0;
        for i in 0..length {
            let pos = start.relative_steps(&direction, i);
            if !valid_tree_pos(level, &pos) {
                return true; // Java succeeds even if only the stump fits.
            }
            if !over_solid_ground(level, &pos) {
                gap += 1;
                if gap > 2 {
                    return true;
                }
            } else {
                gap = 0;
            }
        }
        let mut log_insertions = Vec::new();
        for i in 0..length {
            place_log(
                config,
                level,
                random,
                &start.relative_steps(&direction, i),
                Some(direction),
            );
            log_insertions.push(start.relative_steps(&direction, i));
        }
        decorate_logs(
            level,
            context.chunk_generator,
            random,
            &log_insertions,
            &config.log_decorators,
        );
        true
    }
}
fn decorate_logs<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    generator: &dyn crate::chunk::chunk_generator::ChunkGenerator,
    random: &mut R,
    logs: &[BlockPos],
    decorators: &[TreeDecorator],
) {
    if decorators.is_empty() {
        return;
    }
    let level = TreeLevel(RefCell::new(level));
    let mut setter = |pos: &BlockPos, state| level.write(pos, state);
    let height_at = |ty, x, z| level.0.borrow_mut().get_height_at(ty, x, z);
    let has_beehive_block_entity =
        |pos: &BlockPos| level.0.borrow_mut().has_beehive_block_entity(pos);
    let store_beehive_occupant = |pos: &BlockPos, ticks| {
        level.0.borrow_mut().store_beehive_occupant(pos, ticks);
    };
    let place_configured_feature = |name: &str, pos: &BlockPos, random: &mut R| {
        level.place_configured_feature(name, pos, random, generator)
    };
    let mut context = TreeDecoratorContext::new(
        &level,
        &mut setter,
        random,
        logs,
        &[],
        &[],
        TreeDecoratorCallbacks {
            height_at: &height_at,
            has_beehive_block_entity: &has_beehive_block_entity,
            store_beehive_occupant: &store_beehive_occupant,
            place_configured_feature: &place_configured_feature,
        },
    );
    for decorator in decorators {
        decorator.place(&mut context);
    }
}
fn over_solid_ground(level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
    // Java intentionally passes blockPos, not blockPos.below(), to isFaceSturdy.
    level.is_face_sturdy(pos, &level.get_block_state(&pos.below()), &Direction::Up)
}
fn place_log<R: RandomSource>(
    config: &FallenTreeConfiguration,
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    pos: &BlockPos,
    direction: Option<Direction>,
) {
    let mut state =
        block_state_provider_get_state(config.trunk_provider.as_ref(), level, random, pos);
    if let Some(direction) = direction {
        state = state
            .try_set_value(BlockStateProperties::AXIS, direction.get_axis())
            .expect("valid log axis");
    }
    level.set_block(pos, state, 3);
    mark_above_for_post_processing(level, pos);
}

#[cfg(test)]
mod tests {

    #[test]
    fn generated_fallen_trees_match_actual_paper_write_sequences_and_rng_continuation() {
        use super::super::configurations::fallen_tree_configuration::fallen_tree_configuration_codec;
        use rivet_registry::{
            access::RegistryAccess, generated::feature_data::CONFIGURED_FEATURE_BY_NAME,
            registry_ops::RegistryOps,
        };
        use rivet_serialization::json_ops::JsonOps;
        use rivet_util::random::XoroshiroRandomSource;
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = fallen_tree_configuration_codec();
        let fixtures = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/rivet-oracle/fixtures/fallen-trees.jsonl"
        ));
        let mut cases = 0;
        for line in fixtures
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let row: serde_json::Value = serde_json::from_str(line).expect("Paper fixture JSON");
            let feature = format!("minecraft:{}", row["feature"].as_str().unwrap());
            let entry = &CONFIGURED_FEATURE_BY_NAME[feature.as_str()];
            let payload: serde_json::Value = serde_json::from_str(entry.json).unwrap();
            let config = codec
                .parse(&ops, &payload["config"])
                .result()
                .unwrap_or_else(|| panic!("generated fallen config {feature} must decode"))
                .clone();
            let xyz: [i32; 3] = serde_json::from_value(row["origin"].clone()).unwrap();
            let origin = BlockPos::new(xyz[0], xyz[1], xyz[2]);
            let mut ground = Ground {
                states: HashMap::new(),
                writes: Vec::new(),
                has_ground: row["terrain"] == "flat",
                ground_y: xyz[1],
            };
            let mut random = XoroshiroRandomSource::new(row["seed"].as_i64().unwrap());
            let result = FALLEN_TREE.place(&mut FeaturePlaceContext::new(
                None,
                &mut ground,
                &TestGenerator,
                &mut random,
                &origin,
                &config,
            ));
            assert_eq!(
                result,
                row["result"].as_bool().unwrap(),
                "Paper result {line}"
            );
            let actual: Vec<[i64; 5]> = ground
                .writes
                .iter()
                .map(|(pos, state, flags)| {
                    [
                        pos.get_x() as i64,
                        pos.get_y() as i64,
                        pos.get_z() as i64,
                        state.id().0 as i64,
                        *flags as i64,
                    ]
                })
                .collect();
            let expected: Vec<[i64; 5]> = serde_json::from_value(row["writes"].clone()).unwrap();
            assert_eq!(actual, expected, "Paper write sequence {line}");
            assert_eq!(
                random.next_long(),
                row["next_long"].as_i64().unwrap(),
                "Paper RNG continuation {line}"
            );
            cases += 1;
        }
        assert_eq!(
            cases, 80,
            "all actual datapack-configured Paper cases exercised"
        );
    }

    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider;
    use crate::levelgen::feature::test_support::{RecordingRandom, RngCall, TestGenerator};
    use rivet_registry::{block_state::BlockState, generated::blocks::BlockId};
    use rivet_util::valueproviders::{constant_int::ConstantInt, int_provider::IntProvider};
    use std::{collections::HashMap, sync::Arc};

    struct Ground {
        states: HashMap<BlockPos, BlockState>,
        writes: Vec<(BlockPos, BlockState, u32)>,
        has_ground: bool,
        ground_y: i32,
    }
    fn state(name: &str) -> BlockState {
        BlockState::of(BlockId::from_name(name).unwrap())
    }
    impl LevelHeightAccessor for Ground {
        fn get_min_y(&self) -> i32 {
            -64
        }
        fn get_height(&self) -> i32 {
            384
        }
    }
    impl WorldGenLevel for Ground {
        fn get_seed(&self) -> i64 {
            0
        }
        fn get_block_state(&self, pos: &BlockPos) -> BlockState {
            self.states.get(pos).copied().unwrap_or_else(|| {
                if self.has_ground && pos.get_y() < self.ground_y {
                    state("minecraft:dirt")
                } else {
                    state("minecraft:air")
                }
            })
        }
        fn is_face_sturdy(&self, _pos: &BlockPos, s: &BlockState, _direction: &Direction) -> bool {
            s.block() == state("minecraft:dirt").block()
        }
        fn set_block(&mut self, pos: &BlockPos, s: BlockState, flags: u32) -> bool {
            self.states.insert(*pos, s);
            self.writes.push((*pos, s, flags));
            true
        }
    }
    fn config() -> FallenTreeConfiguration {
        FallenTreeConfiguration {
            trunk_provider: Arc::new(SimpleStateProvider::new(state("minecraft:oak_log"))),
            log_length: IntProvider::Constant(ConstantInt::of(7)),
            stump_decorators: Vec::new(),
            log_decorators: Vec::new(),
        }
    }
    #[test]
    fn fallen_tree_places_stump_and_five_horizontal_logs_without_extra_random_draws() {
        let mut ground = Ground {
            states: HashMap::new(),
            writes: Vec::new(),
            has_ground: true,
            ground_y: 1,
        };
        let mut random = RecordingRandom::new(42);
        let origin = BlockPos::new(0, 1, 0);
        assert!(FALLEN_TREE.place(&mut FeaturePlaceContext::new(
            None,
            &mut ground,
            &TestGenerator,
            &mut random,
            &origin,
            &config()
        )));
        assert_eq!(ground.writes.len(), 6);
        assert!(
            ground
                .writes
                .iter()
                .all(|(p, _, flags)| p.get_y() == 1 && *flags == 3)
        );
        assert_eq!(
            ground.writes[0].1.get_value(BlockStateProperties::AXIS),
            Some(rivet_registry::block_state_property::PropertyValue::Enum(
                "y"
            ))
        );
        assert!(
            ground.writes[1..]
                .iter()
                .all(|(_, s, _)| s.get_value(BlockStateProperties::AXIS)
                    != Some(rivet_registry::block_state_property::PropertyValue::Enum(
                        "y"
                    )))
        );
        assert_eq!(
            random.calls,
            vec![RngCall::IntBound(4), RngCall::IntBound(2)]
        );
    }
    #[test]
    fn fallen_tree_still_succeeds_with_only_stump_when_ground_gap_exceeds_two() {
        let mut ground = Ground {
            states: HashMap::new(),
            writes: Vec::new(),
            has_ground: false,
            ground_y: 1,
        };
        let mut random = RecordingRandom::new(42);
        assert!(FALLEN_TREE.place(&mut FeaturePlaceContext::new(
            None,
            &mut ground,
            &TestGenerator,
            &mut random,
            &BlockPos::new(0, 1, 0),
            &config()
        )));
        assert_eq!(ground.writes.len(), 1);
        assert_eq!(ground.writes[0].0, BlockPos::new(0, 1, 0));
        assert_eq!(
            random.calls,
            vec![RngCall::IntBound(4), RngCall::IntBound(2)]
        );
    }
}
