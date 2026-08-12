//! Port of `net.minecraft.world.level.levelgen.blockpredicates.MatchingFluidsPredicate`
//! (class, 26.2).
//!
//! Java: a `StateTestingPredicate` whose `test(BlockState)` is
//! `state.getFluidState().is(this.fluids)` (the state's fluid type holder is a
//! member of the `HolderSet<Fluid>`) and whose `type()` is
//! `BlockPredicateType.MATCHING_FLUIDS`. Its `CODEC` is the shared
//! state-testing offset field plus the required `"fluids"` field —
//! `RegistryCodecs.homogeneousList(Registries.FLUID)`, a `HolderSetCodec` whose
//! element codec is `RegistryFixedCodec(Registries.FLUID)`.
//!
//! `Fluid` is the id-handle placeholder [`FluidId`]; the state's fluid type id
//! (`state.fluid_id()`, 0..=4) is the element id in the fluid registry, so
//! `state.getFluidState().is(this.fluids)` becomes `set.contains_id(fluid_id)`.

use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use crate::levelgen::blockpredicates::state_testing_predicate::{
    StateTestingPredicate, offset_field, state_testing_test,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Vec3i};
use rivet_registry::fluid_id::FluidId;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.MatchingFluidsPredicate`.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchingFluidsPredicate {
    /// `this.offset` — the offset applied to the tested position.
    offset: Vec3i,
    /// `this.fluids` — the matching fluid holder set.
    fluids: HolderSet<FluidId>,
}

impl MatchingFluidsPredicate {
    /// `new MatchingFluidsPredicate(Vec3i, HolderSet<Fluid>)`.
    pub fn new(offset: Vec3i, fluids: HolderSet<FluidId>) -> Self {
        MatchingFluidsPredicate { offset, fluids }
    }

    /// `this.offset`.
    pub fn offset(&self) -> &Vec3i {
        &self.offset
    }

    /// `this.fluids`.
    pub fn fluids(&self) -> &HolderSet<FluidId> {
        &self.fluids
    }
}

impl StateTestingPredicate for MatchingFluidsPredicate {
    fn offset(&self) -> &Vec3i {
        &self.offset
    }

    fn test_state(&self, state: &BlockState) -> bool {
        // `state.getFluidState().is(this.fluids)` — the fluid type holder is a
        // `Reference` in the fluid registry whose element id is the state's
        // fluid id (0..=4).
        self.fluids.contains_id(state.fluid_id() as u32)
    }
}

impl BlockPredicate for MatchingFluidsPredicate {
    fn test(&self, level: &dyn crate::level::WorldGenLevel, origin: &BlockPos) -> bool {
        state_testing_test(self, level, origin)
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::MATCHING_FLUIDS
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `RegistryCodecs.homogeneousList(Registries.FLUID)` — the `"fluids"` field
/// codec: a `HolderSetCodec` over the fluid registry, whose element codec is a
/// `RegistryFixedCodec` (tag key `#minecraft:...` or element-list form).
///
/// The concrete codec is not `Send + Sync` (its `RegistryOps` carries the
/// single-threaded `HolderLookupAdapter`, `RefCell` memo — OWNERSHIP's single
/// sync tick); the `Arc` is held by the ops-parameterized predicate codec and
/// never crosses threads.
fn fluids_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<HolderSet<FluidId>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<rivet_registry::holder::Holder<FluidId>, Ops>> = Arc::new(
        rivet_registry::registry_file_codec::RegistryFixedCodec::create(
            &rivet_registry::registries::FLUID,
        ),
    );
    #[allow(clippy::arc_with_non_send_sync)]
    let holder_set: Arc<dyn Codec<HolderSet<FluidId>, Ops>> =
        Arc::new(rivet_registry::registry_file_codec::HolderSetCodec::create(
            &rivet_registry::registries::FLUID,
            element,
            false,
        ));
    codec::field_of(holder_set, "fluids".to_string())
}

/// `MatchingFluidsPredicate.CODEC` — the shared state-testing offset field plus
/// the required `"fluids"` holder-set field, as the ops-generic
/// `matching_fluids_predicate_map_codec::<Ops>()` factory.
pub fn matching_fluids_predicate_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<MatchingFluidsPredicate, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(offset_field::<MatchingFluidsPredicate, Ops>(Arc::new(
                |p: &MatchingFluidsPredicate| p.offset,
            )))
            .and(record_builder::RecordCodecBuilder::of(
                Arc::new(|p: &MatchingFluidsPredicate| p.fluids.clone()),
                fluids_field_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|offset: Vec3i, fluids: HolderSet<FluidId>| {
                    MatchingFluidsPredicate::new(offset, fluids)
                }),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::blockpredicates::block_predicate::block_predicate_codec;
    use rivet_registry::Identifier;
    use rivet_registry::ResourceKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::holder::Holder;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::root::AnyBox;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A fluid registry with `empty` (id 0), `flowing_water` (id 1) and
    /// `water` (id 2), wrapped under `Registries.FLUID`.
    fn fluid_access() -> RegistryAccess {
        let mut builder = RegistryBuilder::new(&*rivet_registry::registries::FLUID);
        for (name, id) in [
            ("minecraft:empty", 0u16),
            ("minecraft:flowing_water", 1),
            ("minecraft:water", 2),
        ] {
            builder.register(
                &ResourceKey::create(&*rivet_registry::registries::FLUID, Identifier::parse(name)),
                Arc::new(FluidId::from_id(id)),
                RegistrationInfo::BUILT_IN,
            );
        }
        let registry = builder.freeze();
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("fluid")),
            Box::new(registry) as AnyBox,
        )])
    }

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, fluid_access())
    }

    #[test]
    fn test_state_checks_membership_by_fluid_id() {
        // `state.getFluidState().is(this.fluids)` — `test_state` compares the
        // state's REAL generated fluid id (water=2, empty=0 in the vanilla
        // fluid registry) against the set members. No test registry involved.
        let set = HolderSet::direct(vec![
            Holder::reference(rivet_registry::holder::RegistryId(0), 2),
            Holder::reference(rivet_registry::holder::RegistryId(0), 1),
        ]);
        let p = MatchingFluidsPredicate::new(Vec3i::ZERO, set);
        let water = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:water").unwrap(),
        );
        let air = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:air").unwrap(),
        );
        assert!(p.test_state(&water));
        assert!(!p.test_state(&air));
    }

    #[test]
    fn codec_round_trips_and_encodes_fluids() {
        // One access builds BOTH the set's reference holders and the ops: each
        // `freeze()` allocates a fresh `RegistryId`, so the holder must carry
        // the same registry id the ops' access reads.
        let access = fluid_access();
        let registry_id = rivet_registry::access::RegistryAccess::lookup(
            &access,
            &*rivet_registry::registries::FLUID,
        )
        .expect("fluid registry")
        .registry_id();
        let p: Arc<dyn BlockPredicate> = Arc::new(MatchingFluidsPredicate::new(
            Vec3i::ZERO,
            HolderSet::direct(vec![Holder::reference(registry_id, 2)]),
        ));
        let codec = block_predicate_codec::<TestOps>();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let encoded = codec
            .encode_start(&ops, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        // A single member compacts to the bare element.
        assert_eq!(
            encoded,
            json!({"type": "minecraft:matching_fluids", "fluids": "minecraft:water"})
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            BlockPredicate::type_id(&*decoded),
            BlockPredicateTypes::MATCHING_FLUIDS
        );
    }

    #[test]
    fn missing_fluids_field_errors() {
        let ops = ops();
        let codec = block_predicate_codec::<TestOps>();
        let result = codec.parse(&ops, &json!({"type": "minecraft:matching_fluids"}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key fluids"), "got: {msg}");
    }

    #[test]
    fn unknown_fluid_name_errors() {
        let ops = ops();
        let codec = block_predicate_codec::<TestOps>();
        let result = codec.parse(
            &ops,
            &json!({"type": "minecraft:matching_fluids", "fluids": "minecraft:not_a_fluid"}),
        );
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Failed to get element minecraft:not_a_fluid"),
            "got: {msg}"
        );
    }
}
