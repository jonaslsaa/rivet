//! Port of `net.minecraft.world.level.levelgen.blockpredicates.UnobstructedPredicate`
//! (record, 26.2).
//!
//! Java: a `BlockPredicate` record holding `Vec3i offset` whose `test` is
//! `worldGenLevel.isUnobstructed(null, Shapes.block().move(pos))` — the passed
//! `pos` directly; Paper never applies `this.offset` in `test` (the component
//! is a codec round-trip artifact only). Its `type()` is
//! `BlockPredicateType.UNOBSTRUCTED`. Its `CODEC` is the offset optional field
//! over `Vec3i.CODEC` — the plain (NOT `offsetCodec(16)`-validated) codec, and
//! NOT `lenientOptionalFieldOf` but the non-lenient `optionalFieldOf("offset",
//! Vec3i.ZERO)`.
//!
//! The collision world-access check goes through the [`WorldGenLevel::
//! is_unobstructed`] seam (RivetTodo #399 — unavailable until the
//! world-access lands, then failing explicitly rather than fabricating).

use crate::level::WorldGenLevel;
use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use crate::levelgen::blockpredicates::state_testing_predicate::vec3i_optional_field_codec;
use rivet_registry::core::{BlockPos, Vec3i};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.UnobstructedPredicate`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnobstructedPredicate {
    /// `offset` — the record's `Vec3i` component.
    offset: Vec3i,
}

impl UnobstructedPredicate {
    /// `new UnobstructedPredicate(Vec3i)`.
    pub fn new(offset: Vec3i) -> Self {
        UnobstructedPredicate { offset }
    }

    /// `UnobstructedPredicate.offset()` — the record accessor.
    pub fn offset(&self) -> &Vec3i {
        &self.offset
    }
}

impl BlockPredicate for UnobstructedPredicate {
    fn test(&self, level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
        // `worldGenLevel.isUnobstructed(null, Shapes.block().move(pos))` — the
        // entity is null and the shape is the block shape moved to the passed
        // position. Paper does NOT apply `this.offset` here (it is a codec
        // round-trip artifact only); the collision check is the `#399`
        // world-access seam.
        level.is_unobstructed(pos)
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::UNOBSTRUCTED
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `UnobstructedPredicate.CODEC` — the record codec over the `"offset"` optional
/// field (`Vec3i.CODEC`, default `Vec3i.ZERO`), as the ops-generic
/// `unobstructed_predicate_map_codec::<Ops>()` factory.
pub fn unobstructed_predicate_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<UnobstructedPredicate, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(record_builder::RecordCodecBuilder::of(
                Arc::new(|p: &UnobstructedPredicate| p.offset),
                vec3i_optional_field_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|offset: Vec3i| UnobstructedPredicate::new(offset)),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::blockpredicates::block_predicate::block_predicate_codec;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// The test ops: a `RegistryOps` over JSON — the only ops that implement
    /// `RegistryOpsLookup` (the dispatch's holder-set fields require it). The
    /// unobstructed codec never touches a registry, so an empty access is enough.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    #[test]
    fn codec_round_trips_and_defaults_offset() {
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = block_predicate_codec::<TestOps>();
        let p: Arc<dyn BlockPredicate> = Arc::new(UnobstructedPredicate::new(Vec3i::ZERO));
        let encoded = codec
            .encode_start(&ops, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"type": "minecraft:unobstructed"}));

        let p2: Arc<dyn BlockPredicate> = Arc::new(UnobstructedPredicate::new(Vec3i::new(1, 2, 3)));
        let encoded2 = codec
            .encode_start(&ops, &p2)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded2,
            json!({"type": "minecraft:unobstructed", "offset": [1, 2, 3]})
        );
        let decoded = codec
            .parse(&ops, &encoded2)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            BlockPredicate::type_id(&*decoded),
            BlockPredicateTypes::UNOBSTRUCTED
        );
    }

    #[test]
    fn offset_uses_plain_vec3i_not_offset_codec() {
        // `UnobstructedPredicate.CODEC` reads the `"offset"` field over
        // `Vec3i.CODEC` — NOT the `Vec3i.offsetCodec(16)` the state-testing
        // predicates use — so an axis at 16 (out of the offset codec's range)
        // is accepted.
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = block_predicate_codec::<TestOps>();
        let result = codec.parse(
            &ops,
            &json!({"type": "minecraft:unobstructed", "offset": [16, 0, 0]}),
        );
        assert!(
            result.is_success(),
            "got: {:?}",
            result.error_ref().map(|e| e.message().to_string())
        );
    }

    #[test]
    fn malformed_offset_field_errors_non_lenient() {
        // `optionalFieldOf("offset", Vec3i.ZERO)` is the NON-lenient form: a
        // present-but-malformed offset propagates its decode error (an absent
        // field would default to ZERO).
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = block_predicate_codec::<TestOps>();
        let result = codec.parse(
            &ops,
            &json!({"type": "minecraft:unobstructed", "offset": [1, 2]}),
        );
        assert!(result.is_error());
    }

    /// A `WorldGenLevel` double that records the position passed to
    /// `is_unobstructed` and answers `true` — the regression double proving
    /// `test` passes the argument `pos` straight through.
    #[derive(Default)]
    struct RecordingLevel {
        queried: std::sync::Mutex<Vec<BlockPos>>,
    }

    impl LevelHeightAccessor for RecordingLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for RecordingLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> rivet_registry::block_state::BlockState {
            panic!("unobstructed tests never read block state")
        }

        fn is_unobstructed(&self, pos: &BlockPos) -> bool {
            self.queried.lock().unwrap().push(*pos);
            true
        }
    }

    #[test]
    fn test_does_not_apply_offset() {
        // Paper 26.2 `UnobstructedPredicate.test` is
        // `isUnobstructed(null, Shapes.block().move(pos))` — the `offset`
        // component is never applied. A predicate carrying a non-zero offset
        // must query the ORIGINAL position, not `pos.offset(offset)`.
        let level = RecordingLevel::default();
        let p = UnobstructedPredicate::new(Vec3i::new(1, 2, 3));
        let origin = BlockPos::new(10, 20, 30);
        assert!(p.test(&level, &origin));
        assert_eq!(level.queried.lock().unwrap().as_slice(), &[origin]);
    }
}
