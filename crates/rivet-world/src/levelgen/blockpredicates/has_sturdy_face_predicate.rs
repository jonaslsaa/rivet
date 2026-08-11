//! Port of `net.minecraft.world.level.levelgen.blockpredicates.HasSturdyFacePredicate`
//! (class, 26.2).
//!
//! Java: a `BlockPredicate` (not state-testing) whose `test` is
//! `level.getBlockState(testPosition).isFaceSturdy(level, testPosition,
//! this.direction)` with `testPosition = origin.offset(this.offset)`, and whose
//! `type()` is `BlockPredicateType.HAS_STURDY_FACE`. Its `CODEC` is the offset
//! optional field plus the required `"direction"` field (`Direction.CODEC`).
//!
//! The block-state read and the face-sturdiness check go through the
//! [`WorldGenLevel::get_block_state`] / [`WorldGenLevel::is_face_sturdy`] seams
//! (RivetTodo #399 — unavailable until the world-access lands, then failing
//! explicitly rather than fabricating).

use crate::level::WorldGenLevel;
use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use crate::levelgen::blockpredicates::state_testing_predicate::offset_field;
use rivet_registry::core::{BlockPos, Direction, Vec3i};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.HasSturdyFacePredicate`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HasSturdyFacePredicate {
    /// `this.offset` — the offset applied to the tested position.
    offset: Vec3i,
    /// `this.direction` — the face direction whose sturdiness is tested.
    direction: Direction,
}

impl HasSturdyFacePredicate {
    /// `new HasSturdyFacePredicate(Vec3i, Direction)`.
    pub fn new(offset: Vec3i, direction: Direction) -> Self {
        HasSturdyFacePredicate { offset, direction }
    }

    /// `this.offset`.
    pub fn offset(&self) -> &Vec3i {
        &self.offset
    }

    /// `this.direction`.
    pub fn direction(&self) -> Direction {
        self.direction
    }
}

impl BlockPredicate for HasSturdyFacePredicate {
    fn test(&self, level: &dyn WorldGenLevel, origin: &BlockPos) -> bool {
        let test_position = origin.offset_vec(&self.offset);
        // `level.getBlockState(testPosition).isFaceSturdy(level, testPosition,
        // direction)` — both the state read and the shape check are `#399`
        // world-access seams.
        let state = level.get_block_state(&test_position);
        level.is_face_sturdy(&test_position, &state, &self.direction)
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::HAS_STURDY_FACE
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `HasSturdyFacePredicate.CODEC` — the offset optional field (`Vec3i.
/// offsetCodec(16)`, default `Vec3i.ZERO`) plus the required `"direction"`
/// field (`Direction.CODEC`), as the ops-generic
/// `has_sturdy_face_predicate_map_codec::<Ops>()` factory.
pub fn has_sturdy_face_predicate_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<HasSturdyFacePredicate, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(offset_field::<HasSturdyFacePredicate, Ops>(Arc::new(
                |p: &HasSturdyFacePredicate| p.offset,
            )))
            .and(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|p: &HasSturdyFacePredicate| p.direction),
                "direction".to_string(),
                rivet_registry::core::direction_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|offset: Vec3i, direction: Direction| {
                    HasSturdyFacePredicate::new(offset, direction)
                }),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::blockpredicates::block_predicate::block_predicate_codec;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// The test ops: a `RegistryOps` over JSON — the only ops that implement
    /// `RegistryOpsLookup` (the dispatch's holder-set fields require it). The
    /// sturdy-face codec never touches a registry, so an empty access is enough.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    #[test]
    fn codec_round_trips_and_encodes_direction() {
        // `Direction.CODEC` encodes the serialized lowercase name; the offset
        // field defaults to ZERO (omitted on encode).
        let p: Arc<dyn BlockPredicate> = Arc::new(HasSturdyFacePredicate::new(
            Vec3i::new(1, 2, 3),
            Direction::North,
        ));
        let codec = block_predicate_codec::<TestOps>();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let encoded = codec
            .encode_start(&ops, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "type": "minecraft:has_sturdy_face",
                "offset": [1, 2, 3],
                "direction": "north"
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            BlockPredicate::type_id(&*decoded),
            BlockPredicateTypes::HAS_STURDY_FACE
        );
        let as_face = decoded
            .as_any()
            .downcast_ref::<HasSturdyFacePredicate>()
            .expect("decoded has_sturdy_face predicate");
        assert_eq!(as_face.direction(), Direction::North);
        assert_eq!(as_face.offset(), &Vec3i::new(1, 2, 3));
    }

    #[test]
    fn missing_direction_field_errors() {
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = block_predicate_codec::<TestOps>();
        let result = codec.parse(&ops, &json!({"type": "minecraft:has_sturdy_face"}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key direction"), "got: {msg}");
    }

    #[test]
    fn unknown_direction_errors() {
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = block_predicate_codec::<TestOps>();
        let result = codec.parse(
            &ops,
            &json!({"type": "minecraft:has_sturdy_face", "direction": "not_a_direction"}),
        );
        assert!(result.is_error());
    }
}
