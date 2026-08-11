//! Registry identity surface for
//! `net.minecraft.world.level.levelgen.feature.featuresize.FeatureSizeType`
//! (MC 26.2, #394).
//!
//! Paper's real value also owns a `MapCodec<P>` per type (`FeatureSizeType<P>`:
//! `TWO_LAYERS_FEATURE_SIZE`/`THREE_LAYERS_FEATURE_SIZE`). Those codecs dispatch
//! on `FeatureSize::type`, which requires the concrete
//! `TwoLayersFeatureSize`/`ThreeLayersFeatureSize` feature configurations — the
//! deferred `mc.world.level.levelgen.feature` unit. This port therefore carries
//! only the generated built-in id. It deliberately exposes no generic
//! constructor or fake codec: resolving a known type does not claim its payload
//! is supported.
//!
//! This identity is the *consumer* of the #394 by-name codec: `FeatureSize.CODEC`
//! is `BuiltInRegistries.FEATURE_SIZE_TYPE.byNameCodec().dispatch(...)`
//! (FeatureSize.java:10). This module's frozen registry is what that codec
//! round-trips against when the worldgen unit lands.

use std::sync::{Arc, LazyLock};

use crate::generated::feature_size_types::{FEATURE_SIZE_TYPE_BY_ID, feature_size_type_id};
use crate::registries::FEATURE_SIZE_TYPE;
use crate::{Identifier, RegistrationInfo, RegistryAccess, RegistryBuilder, ResourceKey};

/// The one built-in feature-size registry instance used by the #394 by-name
/// codec and by every public value lookup. `Registry` encodes values by
/// allocation identity, so rebuilding this table per caller would detach the
/// returned `Arc`s from the registry used to encode them.
static BUILT_IN_REGISTRY_ACCESS: LazyLock<RegistryAccess> = LazyLock::new(|| {
    let mut builder = RegistryBuilder::new(&FEATURE_SIZE_TYPE);
    for (id, name) in FEATURE_SIZE_TYPE_BY_ID.iter().enumerate() {
        let key = ResourceKey::create(&FEATURE_SIZE_TYPE, Identifier::parse(name));
        builder.register(
            &key,
            Arc::new(FeatureSizeTypeId { id: id as u16 }),
            RegistrationInfo::BUILT_IN,
        );
    }
    RegistryAccess::from_single_registry((*FEATURE_SIZE_TYPE).clone(), builder.freeze())
});

/// A known Minecraft 26.2 built-in feature-size type.
///
/// Values can only be obtained from the report-generated id/name space. An
/// unknown numeric id or resource identifier remains `None`, matching the
/// non-defaulted vanilla registry rather than folding to a fabricated type.
#[derive(Debug)]
pub struct FeatureSizeTypeId {
    id: u16,
}

impl FeatureSizeTypeId {
    /// Resolve a built-in registry id. Unknown ids are not representable.
    pub fn from_id(id: u16) -> Option<Arc<Self>> {
        BUILT_IN_REGISTRY_ACCESS
            .lookup(&FEATURE_SIZE_TYPE)
            .expect("built-in feature-size registry is present")
            .by_id_arc(id.into())
            .cloned()
    }

    /// Resolve the registry resource identifier used by Paper's
    /// `BuiltInRegistries.FEATURE_SIZE_TYPE.byNameCodec()`.
    pub fn from_identifier(identifier: &Identifier) -> Option<Arc<Self>> {
        feature_size_type_id(&identifier.to_string()).and_then(Self::from_id)
    }

    /// Resolve a serialized namespaced identifier.
    pub fn from_name(name: &str) -> Option<Arc<Self>> {
        feature_size_type_id(name).and_then(Self::from_id)
    }

    /// Numeric built-in registry id.
    pub const fn id(&self) -> u16 {
        self.id
    }

    /// Canonical namespaced identifier text.
    pub fn name(&self) -> &'static str {
        FEATURE_SIZE_TYPE_BY_ID[self.id as usize]
    }

    /// Canonical resource identifier.
    pub fn identifier(&self) -> Identifier {
        Identifier::parse(self.name())
    }

    /// Access Paper's non-defaulted built-in registry in registration order
    /// (two_layers_feature_size, then three_layers_feature_size).
    ///
    /// The returned access shares the canonical registry and element `Arc`
    /// identities. It is the codec context for the #394 by-name codec.
    pub fn built_in_registry_access() -> RegistryAccess {
        BUILT_IN_REGISTRY_ACCESS.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    use rivet_serialization::decoder::Decoder;
    use rivet_serialization::dynamic_ops::DynamicOps;
    use rivet_serialization::encoder::Encoder;
    use rivet_serialization::json_ops::JsonOps;

    /// `FeatureSize.CODEC = BuiltInRegistries.FEATURE_SIZE_TYPE.byNameCodec()
    /// .dispatch(...)` — the #394 end-to-end: the built-in feature-size registry
    /// decodes/encodes identifiers by name, in Paper declaration order.
    #[test]
    fn built_in_registry_by_name_codec_round_trips_declaration_order() {
        let access = FeatureSizeTypeId::built_in_registry_access();
        let registry: &Registry<FeatureSizeTypeId> = access.lookup(&FEATURE_SIZE_TYPE).unwrap();
        let codec = registry.by_name_codec::<JsonOps>();
        let ops = JsonOps::INSTANCE;

        // TWO_LAYERS_FEATURE_SIZE (id 0) is declared first in FeatureSizeType.java.
        let two_layers = FeatureSizeTypeId::from_name("minecraft:two_layers_feature_size").unwrap();
        let two_layers_id = registry.get_id(&two_layers);
        assert_eq!(two_layers_id, 0);
        let encoded = Encoder::encode(codec.as_ref(), &two_layers, &ops, &ops.empty())
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            encoded,
            ops.create_string("minecraft:two_layers_feature_size".to_string())
        );
        // Decode reproduces the stored `Arc` identity (a real round trip).
        let decoded = Decoder::decode(codec.as_ref(), &ops, &encoded)
            .get_or_throw("decode")
            .0
            .clone();
        assert!(Arc::ptr_eq(&decoded, &two_layers));

        // THREE_LAYERS_FEATURE_SIZE (id 1) is declared second.
        let three_layers =
            FeatureSizeTypeId::from_name("minecraft:three_layers_feature_size").unwrap();
        let three_layers_id = registry.get_id(&three_layers);
        assert_eq!(three_layers_id, 1);
        let input = ops.create_string("minecraft:three_layers_feature_size".to_string());
        let decoded_three = Decoder::decode(codec.as_ref(), &ops, &input)
            .get_or_throw("decode")
            .0
            .clone();
        assert!(Arc::ptr_eq(&decoded_three, &three_layers));

        // An unknown feature-size type is a strict decode error (the built-in
        // registry is non-defaulted — there is no fold).
        let unknown = ops.create_string("minecraft:unknown_feature_size".to_string());
        let result = Decoder::decode(codec.as_ref(), &ops, &unknown);
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!(
                "Unknown registry key in {}: minecraft:unknown_feature_size",
                registry.key()
            )
        );
    }

    #[test]
    fn paper_26_2_ids_are_generated_in_declaration_order() {
        // FeatureSizeType.java registers TWO_LAYERS_FEATURE_SIZE first, then
        // THREE_LAYERS_FEATURE_SIZE — the id space follows registration order.
        assert_eq!(
            FeatureSizeTypeId::from_name("minecraft:two_layers_feature_size")
                .unwrap()
                .id(),
            0
        );
        assert_eq!(
            FeatureSizeTypeId::from_name("minecraft:three_layers_feature_size")
                .unwrap()
                .id(),
            1
        );
    }

    #[test]
    fn unknown_ids_and_identifiers_remain_unknown() {
        assert!(FeatureSizeTypeId::from_id(2).is_none());
        assert!(FeatureSizeTypeId::from_id(u16::MAX).is_none());
        assert!(FeatureSizeTypeId::from_name("minecraft:not_a_feature_size").is_none());
        assert!(
            FeatureSizeTypeId::from_identifier(&Identifier::parse("example:two_layers")).is_none()
        );
    }

    #[test]
    fn every_generated_identity_round_trips_through_the_real_registry() {
        let access = FeatureSizeTypeId::built_in_registry_access();
        let registry = access.lookup(&FEATURE_SIZE_TYPE).unwrap();
        assert_eq!(registry.size() as usize, FEATURE_SIZE_TYPE_BY_ID.len());

        for (id, name) in FEATURE_SIZE_TYPE_BY_ID.iter().enumerate() {
            let identifier = Identifier::parse(name);
            let value = registry
                .get_optional(&identifier)
                .expect("generated identifier is registered");
            assert_eq!(value.id(), id as u16);
            assert_eq!(value.name(), *name);
            assert_eq!(value.identifier(), identifier);
            assert_eq!(registry.get_id(value), id as i32);
            assert!(std::ptr::eq(
                registry
                    .by_id(id as i32)
                    .expect("generated id is registered"),
                value
            ));
            assert_eq!(registry.get_key(value), Some(identifier));
        }
    }

    #[test]
    fn registry_lookup_and_encode_identity_preserve_missing_behavior() {
        let access = FeatureSizeTypeId::built_in_registry_access();
        let registry = access.lookup(&FEATURE_SIZE_TYPE).unwrap();
        assert!(
            registry
                .get_optional(&Identifier::parse("minecraft:unknown"))
                .is_none()
        );
        assert!(registry.by_id(2).is_none());

        // Even the same generated id in a fresh allocation is not the
        // registered Java object identity. This can only be manufactured
        // inside this module: the public type has no constructor or `Clone`.
        let registered =
            FeatureSizeTypeId::from_name("minecraft:three_layers_feature_size").unwrap();
        assert_eq!(registry.get_id(&registered), 1);
        let detached = Arc::new(FeatureSizeTypeId {
            id: registered.id(),
        });
        assert!(!Arc::ptr_eq(&registered, &detached));
        assert_eq!(registry.get_id(&detached), -1);
    }
}
