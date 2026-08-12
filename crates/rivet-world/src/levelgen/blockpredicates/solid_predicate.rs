//! Port of `net.minecraft.world.level.levelgen.blockpredicates.SolidPredicate`
//! (class, 26.2).
//!
//! Java: a `StateTestingPredicate` whose `test(BlockState)` is
//! `state.isSolid()` and whose `type()` is `BlockPredicateType.SOLID`
//! (deprecated in 26.2, but still a registered predicate type). `isSolid()` is
//! the cached `legacySolid` from `calculateSolid()` — a collision-shape bounds
//! check (volume >= 35/48 or full height), NOT `Properties.hasCollision`. Its `CODEC`
//! is the shared `stateTestingCodec(i)` (the `"offset"` optional field,
//! `Vec3i.offsetCodec(16)`, default `Vec3i.ZERO`) — the same record shape as
//! `InsideWorldBoundsPredicate`, but read from the state at the offset rather
//! than a bounds check.

use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use crate::levelgen::blockpredicates::state_testing_predicate::{
    StateTestingPredicate, offset_field, state_testing_test,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Vec3i};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.SolidPredicate`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidPredicate {
    /// `this.offset` — the offset applied to the tested position.
    offset: Vec3i,
}

impl SolidPredicate {
    /// `new SolidPredicate(Vec3i)`.
    pub fn new(offset: Vec3i) -> Self {
        SolidPredicate { offset }
    }

    /// `this.offset`.
    pub fn offset(&self) -> &Vec3i {
        &self.offset
    }
}

impl StateTestingPredicate for SolidPredicate {
    fn offset(&self) -> &Vec3i {
        &self.offset
    }

    fn test_state(&self, state: &BlockState) -> bool {
        state.is_solid()
    }
}

impl BlockPredicate for SolidPredicate {
    fn test(&self, level: &dyn crate::level::WorldGenLevel, origin: &BlockPos) -> bool {
        state_testing_test(self, level, origin)
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::SOLID
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `SolidPredicate.CODEC` — the shared state-testing record codec (the
/// `"offset"` optional field), as the ops-generic
/// `solid_predicate_map_codec::<Ops>()` factory.
pub fn solid_predicate_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<SolidPredicate, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(offset_field::<SolidPredicate, Ops>(Arc::new(
                |p: &SolidPredicate| p.offset,
            )))
            .apply(
                instance,
                Arc::new(|offset: Vec3i| SolidPredicate { offset }),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::blockpredicates::block_predicate::block_predicate_codec;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// The test ops: a `RegistryOps` over JSON — the only ops that implement
    /// `RegistryOpsLookup` (the dispatch's holder-set fields require it). The
    /// solid codec never touches a registry, so an empty access is enough.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    #[test]
    fn solid_predicate_is_true_for_solid_states() {
        // `isSolid()` is the behavior-word bit 22 (probe-grounded). Stone is
        // solid; air is not; water is not.
        let stone = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:stone").unwrap(),
        );
        let air = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:air").unwrap(),
        );
        let p = SolidPredicate::new(Vec3i::ZERO);
        assert!(p.test_state(&stone));
        assert!(!p.test_state(&air));
    }

    #[test]
    fn solid_codec_round_trips_and_defaults_offset() {
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = block_predicate_codec::<TestOps>();
        let p: Arc<dyn BlockPredicate> = Arc::new(SolidPredicate::new(Vec3i::ZERO));
        let encoded = codec
            .encode_start(&ops, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"type": "minecraft:solid"}));

        let p2: Arc<dyn BlockPredicate> = Arc::new(SolidPredicate::new(Vec3i::new(1, 2, 3)));
        let encoded2 = codec
            .encode_start(&ops, &p2)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded2,
            json!({"type": "minecraft:solid", "offset": [1, 2, 3]})
        );
    }
}
