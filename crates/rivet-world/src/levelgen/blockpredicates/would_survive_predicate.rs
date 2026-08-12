//! Port of `net.minecraft.world.level.levelgen.blockpredicates.WouldSurvivePredicate`
//! (class, 26.2).
//!
//! Java: a `BlockPredicate` whose `test` is `this.state.canSurvive(level,
//! origin.offset(this.offset))` and whose `type()` is
//! `BlockPredicateType.WOULD_SURVIVE`. Its `CODEC` is the offset optional field
//! plus the required `"state"` field — `BlockState.CODEC`, owned by
//! `rivet_registry::block_state_codec` (the `StateHolder.codec` dispatch over
//! the block's by-name registry key + the `StateDefinition` properties fold,
//! ported with the #391 feature-configuration slice). The `"state"` field below
//! delegates to that shared codec.
//!
//! The survival check goes through the [`WorldGenLevel::can_survive`] seam
//! (RivetTodo #399 — unavailable until the world-access lands, then failing
//! explicitly rather than fabricating).

use crate::level::WorldGenLevel;
use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use crate::levelgen::blockpredicates::state_testing_predicate::offset_field;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Vec3i};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.WouldSurvivePredicate`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WouldSurvivePredicate {
    /// `this.offset` — the offset applied to the tested position.
    offset: Vec3i,
    /// `this.state` — the state whose survival is tested.
    state: BlockState,
}

impl WouldSurvivePredicate {
    /// `new WouldSurvivePredicate(Vec3i, BlockState)`.
    pub fn new(offset: Vec3i, state: BlockState) -> Self {
        WouldSurvivePredicate { offset, state }
    }

    /// `this.offset`.
    pub fn offset(&self) -> &Vec3i {
        &self.offset
    }

    /// `this.state`.
    pub fn state(&self) -> BlockState {
        self.state
    }
}

impl BlockPredicate for WouldSurvivePredicate {
    fn test(&self, level: &dyn WorldGenLevel, origin: &BlockPos) -> bool {
        // `this.state.canSurvive(level, origin.offset(this.offset))` — the
        // world-context survival check is the `#399` world-access seam.
        let pos = origin.offset_vec(&self.offset);
        level.can_survive(&self.state, &pos)
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::WOULD_SURVIVE
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `WouldSurvivePredicate.CODEC` — the offset optional field (`Vec3i.
/// offsetCodec(16)`, default `Vec3i.ZERO`) plus the required `"state"` field
/// (`BlockState.CODEC`), as the ops-generic
/// `would_survive_predicate_map_codec::<Ops>()` factory.
pub fn would_survive_predicate_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<WouldSurvivePredicate, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(offset_field::<WouldSurvivePredicate, Ops>(Arc::new(
                |p: &WouldSurvivePredicate| p.offset,
            )))
            .and(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|p: &WouldSurvivePredicate| p.state),
                "state".to_string(),
                rivet_registry::block_state_codec::block_state_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|offset: Vec3i, state: BlockState| {
                    WouldSurvivePredicate::new(offset, state)
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
    use rivet_registry::state_definition::StateDefinition;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// The test ops: a `RegistryOps` over JSON — the only ops that implement
    /// `RegistryOpsLookup` (the dispatch's holder-set fields require it). The
    /// would-survive codec's `"state"` field resolves blocks by name over the
    /// generated table (no registry), so an empty access is enough.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    fn stone() -> BlockState {
        BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:stone").unwrap(),
        )
    }

    fn oak_log_axis_x() -> BlockState {
        // The default oak_log state is `axis=y`; set the axis property to `x`
        // so the properties fold has a non-default value to write.
        let oak_log =
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:oak_log").unwrap();
        let definition = StateDefinition::for_block(oak_log);
        let axis = definition
            .get_property("axis")
            .expect("oak_log has axis property");
        let x = axis.get_value("x").expect("axis value x");
        BlockState::of(oak_log)
            .set_value(axis, x)
            .expect("x is a valid axis value")
    }

    #[test]
    fn singleton_state_encodes_without_properties() {
        // `StateDefinition.isSingletonState()` → `MapCodec.unit(default)`: the
        // singleton stone state encodes as just the `"Name"` dispatch key, no
        // `"Properties"` map.
        let p: Arc<dyn BlockPredicate> = Arc::new(WouldSurvivePredicate::new(Vec3i::ZERO, stone()));
        let codec = block_predicate_codec::<TestOps>();
        let ops = ops();
        let encoded = codec
            .encode_start(&ops, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "type": "minecraft:would_survive",
                "state": {"Name": "minecraft:stone"}
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            BlockPredicate::type_id(&*decoded),
            BlockPredicateTypes::WOULD_SURVIVE
        );
        let as_survive = decoded
            .as_any()
            .downcast_ref::<WouldSurvivePredicate>()
            .expect("decoded would_survive predicate");
        assert_eq!(as_survive.state(), stone());
    }

    #[test]
    fn multi_property_state_round_trips_through_properties_fold() {
        // A multi-property state (oak_log) encodes through the properties fold
        // under the lenient `"Properties"` optional field, preserving the
        // non-default axis.
        let p: Arc<dyn BlockPredicate> = Arc::new(WouldSurvivePredicate::new(
            Vec3i::new(1, 2, 3),
            oak_log_axis_x(),
        ));
        let codec = block_predicate_codec::<TestOps>();
        let ops = ops();
        let encoded = codec
            .encode_start(&ops, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "type": "minecraft:would_survive",
                "offset": [1, 2, 3],
                "state": {
                    "Name": "minecraft:oak_log",
                    "Properties": {"axis": "x"}
                }
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        let as_survive = decoded
            .as_any()
            .downcast_ref::<WouldSurvivePredicate>()
            .expect("decoded would_survive predicate");
        assert_eq!(as_survive.state(), oak_log_axis_x());
    }

    #[test]
    fn missing_state_field_errors() {
        let codec = block_predicate_codec::<TestOps>();
        let ops = ops();
        let result = codec.parse(&ops, &json!({"type": "minecraft:would_survive"}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key state"), "got: {msg}");
    }

    #[test]
    fn unknown_block_name_errors() {
        let codec = block_predicate_codec::<TestOps>();
        let ops = ops();
        let result = codec.parse(
            &ops,
            &json!({
                "type": "minecraft:would_survive",
                "state": {"Name": "minecraft:not_a_block"}
            }),
        );
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:block]: minecraft:not_a_block"),
            "got: {msg}"
        );
    }
}
