//! Port of `net.minecraft.world.level.levelgen.placement.EnvironmentScanPlacement`
//! (class, 26.2).
//!
//! Java: a modifier that scans along `directionOfSearch` (a vertical
//! direction) from the origin up to `maxSteps` steps, returning the first
//! position where `targetCondition` holds — provided every scanned position
//! (and the origin) satisfies `allowedSearchCondition` and stays within the
//! build height. Its `CODEC` is the record `{direction_of_search,
//! target_condition, allowed_search_condition (default alwaysTrue),
//! max_steps}` mapped onto the private constructor, and its `type()` is
//! `PlacementModifierType.ENVIRONMENT_SCAN`.
//!
//! The port mirrors the `#399` block-predicate seam: the conditions are erased
//! `Arc<dyn BlockPredicate>`, and `allowed_search_condition` is stored as
//! `Option` where `None` is the `alwaysTrue()` default. The erased predicate
//! has no `PartialEq`, so Java's `Objects.equals(a, default)` omission-on-encode
//! is reproduced by downcast: a value that IS the `TrueBlockPredicate` singleton
//! (Java's `alwaysTrue()` default) is treated as the default and omitted on
//! encode, exactly as Java omits it when the field is value-equal to the
//! default; the truly absent (`None`) default is likewise omitted.

use crate::levelgen::blockpredicates::block_predicate::{self, BlockPredicate};
use crate::levelgen::blockpredicates::true_block_predicate::TrueBlockPredicate;
use crate::levelgen::placement::placement_modifier_type::PlacementModifierTypes;
use crate::levelgen::placement::{PlacementContext, PlacementModifier};
use rivet_registry::core::BlockPos;
use rivet_registry::core::Direction;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.EnvironmentScanPlacement`.
#[derive(Debug, Clone)]
pub struct EnvironmentScanPlacement {
    /// `this.directionOfSearch` — the scan direction (vertical).
    direction_of_search: Direction,
    /// `this.targetCondition` — the condition a scanned position must satisfy.
    target_condition: Arc<dyn BlockPredicate>,
    /// `this.allowedSearchCondition` — `None` is the `alwaysTrue()` default.
    allowed_search_condition: Option<Arc<dyn BlockPredicate>>,
    /// `this.maxSteps` — the scan length bound, `[1, 32]`.
    max_steps: i32,
}

impl EnvironmentScanPlacement {
    /// `scanningFor(Direction, BlockPredicate, BlockPredicate, int)`.
    pub fn scanning_for(
        direction_of_search: Direction,
        target_condition: Arc<dyn BlockPredicate>,
        allowed_search_condition: Arc<dyn BlockPredicate>,
        max_steps: i32,
    ) -> Self {
        EnvironmentScanPlacement {
            direction_of_search,
            target_condition,
            allowed_search_condition: Some(allowed_search_condition),
            max_steps,
        }
    }

    /// `scanningFor(Direction, BlockPredicate, int)` — the defaulted overload
    /// (`allowedSearchCondition = BlockPredicate.alwaysTrue()`).
    pub fn scanning_for_default(
        direction_of_search: Direction,
        target_condition: Arc<dyn BlockPredicate>,
        max_steps: i32,
    ) -> Self {
        EnvironmentScanPlacement {
            direction_of_search,
            target_condition,
            allowed_search_condition: None,
            max_steps,
        }
    }

    /// `this.allowedSearchCondition` resolved to a concrete predicate — the
    /// `alwaysTrue()` default when the field is absent.
    fn allowed_search_condition(&self) -> Arc<dyn BlockPredicate> {
        match &self.allowed_search_condition {
            Some(p) => p.clone(),
            None => block_predicate::always_true(),
        }
    }

    /// The encode-side `forGetter` of the `allowed_search_condition` optional
    /// field — reproduces Java's `optionalFieldOf` `Objects.equals(a, default)`
    /// omission: a `Some` holding the `TrueBlockPredicate` singleton (Java's
    /// `alwaysTrue()` default) is emitted as `None` so the field is omitted,
    /// exactly as Java omits a value-equal-to-default field. The absent
    /// (`None`) default is likewise `None`.
    fn allowed_search_condition_for_codec(&self) -> Option<Arc<dyn BlockPredicate>> {
        match &self.allowed_search_condition {
            Some(p) if p.as_any().is::<TrueBlockPredicate>() => None,
            other => other.clone(),
        }
    }
}

impl PlacementModifier for EnvironmentScanPlacement {
    fn get_positions<'a, R: RandomSource>(
        &'a self,
        context: &mut PlacementContext,
        _random: &mut R,
        origin: &BlockPos,
    ) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
        // `pos = origin.mutable()`, `level = context.getLevel()`.
        let mut pos = origin.mutable();
        let level = context.get_level();
        let allowed = self.allowed_search_condition();
        if !allowed.test(level, &pos.immutable()) {
            return Box::new(std::iter::empty());
        }

        for _ in 0..self.max_steps {
            if self.target_condition.test(level, &pos.immutable()) {
                return Box::new(std::iter::once(pos.immutable()));
            }

            pos.move_dir(&self.direction_of_search);
            if level.is_outside_build_height(pos.get_y()) {
                return Box::new(std::iter::empty());
            }

            if !allowed.test(level, &pos.immutable()) {
                break;
            }
        }

        if self.target_condition.test(level, &pos.immutable()) {
            Box::new(std::iter::once(pos.immutable()))
        } else {
            Box::new(std::iter::empty())
        }
    }

    fn type_id(
        &self,
    ) -> crate::levelgen::placement::placement_modifier_type::PlacementModifierTypeId {
        // `PlacementModifierType.ENVIRONMENT_SCAN` is insertion index 9 in
        // `PlacementModifierType.java`'s registration order.
        PlacementModifierTypes::ENVIRONMENT_SCAN
    }
}

/// `EnvironmentScanPlacement.CODEC` — the record codec, as the ops-generic
/// `environment_scan_placement_map_codec::<Ops>()` factory.
pub fn environment_scan_placement_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<EnvironmentScanPlacement, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(
                // `Direction.VERTICAL_CODEC.fieldOf("direction_of_search")` —
                // `direction_codec` validated to a vertical direction.
                record_builder::RecordCodecBuilder::of_named(
                    Arc::new(|c: &EnvironmentScanPlacement| c.direction_of_search),
                    "direction_of_search".to_string(),
                    vertical_direction_codec::<Ops>(),
                ),
            )
            .and(
                // `BlockPredicate.CODEC.fieldOf("target_condition")`.
                record_builder::RecordCodecBuilder::of_named(
                    Arc::new(|c: &EnvironmentScanPlacement| c.target_condition.clone()),
                    "target_condition".to_string(),
                    block_predicate::block_predicate_codec::<Ops>(),
                ),
            )
            .and(
                // `BlockPredicate.CODEC.optionalFieldOf("allowed_search_condition",
                // BlockPredicate.alwaysTrue())` — the non-lenient optional over
                // the erased predicate. Java's `optionalFieldOf` omits the field
                // on encode when the value `Objects.equals` the `alwaysTrue()`
                // default; `allowed_search_condition_for_codec` reproduces that
                // omission (a `Some(alwaysTrue)` is emitted as `None`), and the
                // absent (`None`) default is likewise omitted.
                record_builder::RecordCodecBuilder::of(
                    Arc::new(|c: &EnvironmentScanPlacement| c.allowed_search_condition_for_codec()),
                    codec::optional_field(
                        "allowed_search_condition".to_string(),
                        block_predicate::block_predicate_codec::<Ops>(),
                        false,
                    ),
                ),
            )
            .and(
                // `Codec.intRange(1, 32).fieldOf("max_steps")`.
                record_builder::RecordCodecBuilder::of_named(
                    Arc::new(|c: &EnvironmentScanPlacement| c.max_steps),
                    "max_steps".to_string(),
                    codec::int_range::<Ops>(1, 32),
                ),
            )
            .apply(
                instance,
                Arc::new(
                    |direction: Direction,
                     target: Arc<dyn BlockPredicate>,
                     allowed: Option<Arc<dyn BlockPredicate>>,
                     max_steps: i32| {
                        EnvironmentScanPlacement {
                            direction_of_search: direction,
                            target_condition: target,
                            allowed_search_condition: allowed,
                            max_steps,
                        }
                    },
                ),
            )
    })
}

/// `EnvironmentScanPlacement.CODEC` as a `Codec` (`MapCodec.codec()`), the
/// shape the `#181` generated dispatch's registration table consumes.
#[allow(dead_code)]
pub fn environment_scan_placement_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<EnvironmentScanPlacement, Ops>> {
    map_codec::codec_of(environment_scan_placement_map_codec::<Ops>())
}

/// `Direction.VERTICAL_CODEC` — `Direction.CODEC.validate(Direction::
/// verifyVertical)` (`v.getAxis().isVertical() ? success : error("Expected a
/// vertical direction")`), as the ops-generic `vertical_direction_codec::<Ops>()`
/// factory. The sibling constant defers with the protocol codec surface
/// (`#126`, see `direction.rs`'s module doc); this local builder reproduces it.
pub fn vertical_direction_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Direction, Ops>> {
    codec::validate(
        rivet_registry::core::direction_codec::<Ops>(),
        Arc::new(|d: &Direction| {
            if d.get_axis().is_vertical() {
                rivet_serialization::data_result::DataResult::success(*d)
            } else {
                rivet_serialization::data_result::DataResult::error(
                    "Expected a vertical direction".to_string(),
                )
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::core::Vec3i;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::json;

    /// A minimal `WorldGenLevel` double over the overworld window. The
    /// predicate tests never touch block state in these scenarios — the
    /// `target_condition` is a local closure predicate, and the allowed check
    /// is `alwaysTrue` — so `get_block_state` panics are never reached.
    struct TestLevel(SimpleLevelHeightAccessor);

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            self.0.get_height()
        }

        fn get_min_y(&self) -> i32 {
            self.0.get_min_y()
        }
    }

    impl crate::level::WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> rivet_registry::block_state::BlockState {
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }

    struct NoopGenerator;

    impl crate::chunk::ChunkGenerator for NoopGenerator {
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            384
        }
    }

    /// A target predicate matching a specific absolute Y — pure, no world
    /// access, so it never touches the `#399` block-state seam.
    #[derive(Debug)]
    struct AtY(i32);
    impl crate::levelgen::blockpredicates::block_predicate::BlockPredicate for AtY {
        fn test(&self, _level: &dyn crate::level::WorldGenLevel, origin: &BlockPos) -> bool {
            origin.get_y() == self.0
        }
        fn type_id(
            &self,
        ) -> crate::levelgen::blockpredicates::block_predicate_type::BlockPredicateTypeId {
            crate::levelgen::blockpredicates::block_predicate_type::BlockPredicateTypes::TRUE
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn scan_positions(modifier: &EnvironmentScanPlacement, origin: &BlockPos) -> Vec<BlockPos> {
        let mut level = TestLevel(create(-64, 384));
        let generator = NoopGenerator;
        let mut context = PlacementContext::new(&mut level, &generator, None);
        let mut random = LegacyRandomSource::new(0);
        modifier
            .get_positions(&mut context, &mut random, origin)
            .collect()
    }

    #[test]
    fn finds_the_first_target_upward() {
        // Scan upward from y=0 with max_steps 4: target at y=3 (absolute),
        // allowed always-true. Positions 1,2 are scanned, 3 matches.
        let modifier =
            EnvironmentScanPlacement::scanning_for_default(Direction::Up, Arc::new(AtY(3)), 4);
        let result = scan_positions(&modifier, &BlockPos::new(0, 0, 0));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], BlockPos::new(0, 3, 0));
    }

    #[test]
    fn empty_when_the_target_is_out_of_range() {
        // Target at y=10, but max_steps 3 — the scan reaches y=3 (the third
        // move), checks allowed/target, then the loop ends and the trailing
        // target check on the final position fails.
        let modifier =
            EnvironmentScanPlacement::scanning_for_default(Direction::Up, Arc::new(AtY(10)), 3);
        let result = scan_positions(&modifier, &BlockPos::new(0, 0, 0));
        assert!(result.is_empty());
    }

    #[test]
    fn finds_the_target_on_the_position_after_the_last_move() {
        // Target at y=2 equals origin_y + max_steps (0 + 2): the loop runs all
        // steps with `allowed` true through the last move (so no mid-loop
        // return/break), and the target check on the position after the final
        // move — the trailing `targetCondition.test(level, pos) ? Stream.of(pos)
        // : Stream.of()` — is what returns the hit.
        let modifier =
            EnvironmentScanPlacement::scanning_for_default(Direction::Up, Arc::new(AtY(2)), 2);
        let result = scan_positions(&modifier, &BlockPos::new(0, 0, 0));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], BlockPos::new(0, 2, 0));
    }

    #[test]
    fn empty_when_the_target_is_below_the_scan_start() {
        // Scanning down from y=5 never reaches y=-1 (target below the build
        // start) within max_steps — the scan returns empty.
        let modifier =
            EnvironmentScanPlacement::scanning_for_default(Direction::Down, Arc::new(AtY(-1)), 3);
        let result = scan_positions(&modifier, &BlockPos::new(0, 5, 0));
        assert!(result.is_empty());
    }

    #[test]
    fn empty_when_build_height_is_exceeded() {
        // Scanning up from the overworld top (y=319) with max_steps 2: the
        // first move goes to y=320, outside build height -> empty.
        let modifier =
            EnvironmentScanPlacement::scanning_for_default(Direction::Up, Arc::new(AtY(400)), 2);
        let result = scan_positions(&modifier, &BlockPos::new(0, 319, 0));
        assert!(result.is_empty());
    }

    #[test]
    fn empty_when_the_origin_fails_allowed() {
        // A disallowed origin: the modifier emits nothing before scanning.
        // Build a modifier with a false allowed condition (an `AtY(-1)`
        // predicate — never true for the origin y=0, and also never true for
        // any scanned position, so the whole scan is empty).
        let modifier = EnvironmentScanPlacement::scanning_for(
            Direction::Up,
            Arc::new(AtY(2)),
            Arc::new(AtY(-1)),
            5,
        );
        let result = scan_positions(&modifier, &BlockPos::new(0, 0, 0));
        assert!(result.is_empty());
    }

    #[test]
    fn type_identity_is_reported() {
        let modifier =
            EnvironmentScanPlacement::scanning_for_default(Direction::Up, Arc::new(AtY(1)), 1);
        assert_eq!(modifier.type_id(), PlacementModifierTypes::ENVIRONMENT_SCAN);
    }

    #[test]
    fn codec_round_trips_with_defaulted_allowed() {
        // The ops must implement `RegistryOpsLookup` (the block-predicate
        // dispatch requires it); an empty registry is enough for the `true`
        // predicate. The target is the real `true` predicate (`AtY` reports the
        // TRUE type id but is not a `True` instance, so the dispatch's encode
        // would reject it).
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = environment_scan_placement_codec::<TestOps>();
        let modifier = EnvironmentScanPlacement::scanning_for_default(
            Direction::Up,
            block_predicate::always_true(),
            4,
        );
        let encoded = codec
            .encode_start(&ops, &modifier)
            .result()
            .expect("encode should succeed")
            .clone();
        // The defaulted allowed field is omitted; `true`'s type name is
        // emitted for the target, and the direction encodes as its lowercase
        // name.
        assert_eq!(
            encoded,
            json!({
                "direction_of_search": "up",
                "target_condition": {"type": "minecraft:true"},
                "max_steps": 4
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert!(decoded.allowed_search_condition.is_none());
        assert_eq!(decoded.max_steps, 4);
        assert_eq!(decoded.direction_of_search, Direction::Up);
    }

    #[test]
    fn codec_omits_an_explicit_always_true_allowed() {
        // Java's `optionalFieldOf("allowed_search_condition", alwaysTrue())`
        // omits the field on encode when the value is value-equal to the
        // default. `scanning_for(..., alwaysTrue(), ...)` passes the default
        // explicitly, so Paper omits the field; the port's
        // `allowed_search_condition_for_codec` reproduces that omission by
        // downcasting the erased predicate to the `TrueBlockPredicate`
        // singleton.
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = environment_scan_placement_codec::<TestOps>();
        let modifier = EnvironmentScanPlacement::scanning_for(
            Direction::Down,
            block_predicate::always_true(),
            block_predicate::always_true(),
            2,
        );
        let encoded = codec
            .encode_start(&ops, &modifier)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "direction_of_search": "down",
                "target_condition": {"type": "minecraft:true"},
                "max_steps": 2
            })
        );
        // The absent field decodes back to the `None` (alwaysTrue) default.
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert!(decoded.allowed_search_condition.is_none());
        assert_eq!(decoded.max_steps, 2);
        assert_eq!(decoded.direction_of_search, Direction::Down);
    }

    #[test]
    fn codec_emits_a_non_default_explicit_allowed() {
        // A non-`alwaysTrue` explicit allowed condition (an `inside_world_bounds`
        // predicate at offset (0, -1, 0)) is NOT value-equal to the default, so
        // the field is emitted on encode and round-trips as a `Some`.
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = environment_scan_placement_codec::<TestOps>();
        let modifier = EnvironmentScanPlacement::scanning_for(
            Direction::Down,
            block_predicate::always_true(),
            Arc::new(block_predicate::inside_world(Vec3i::new(0, -1, 0))),
            2,
        );
        let encoded = codec
            .encode_start(&ops, &modifier)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "direction_of_search": "down",
                "target_condition": {"type": "minecraft:true"},
                "allowed_search_condition": {
                    "type": "minecraft:inside_world_bounds",
                    "offset": [0, -1, 0]
                },
                "max_steps": 2
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert!(decoded.allowed_search_condition.is_some());
        assert_eq!(decoded.max_steps, 2);
        assert_eq!(decoded.direction_of_search, Direction::Down);
    }

    #[test]
    fn codec_rejects_a_horizontal_direction() {
        // `Direction.VERTICAL_CODEC` validate: a horizontal direction errors
        // with the exact Java message.
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = environment_scan_placement_codec::<TestOps>();
        let input = json!({
            "direction_of_search": "north",
            "target_condition": {"type": "minecraft:true"},
            "max_steps": 2
        });
        let result = codec.parse(&ops, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.contains("Expected a vertical direction"), "got: {msg}");
    }

    #[test]
    fn codec_rejects_max_steps_out_of_range() {
        // `Codec.intRange(1, 32)`.
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = environment_scan_placement_codec::<TestOps>();
        let input = json!({
            "direction_of_search": "up",
            "target_condition": {"type": "minecraft:true"},
            "max_steps": 33
        });
        let result = codec.parse(&ops, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Value 33 outside of range [1:32]"),
            "got: {msg}"
        );
    }

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;
}
