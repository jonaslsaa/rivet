//! The `Holder<BiomeId>` / `HolderSet<BiomeId>` codecs the biome-source family
//! builds on — the pure-id analogue of `Biome.CODEC` / `Biome.LIST_CODEC`
//! (`mc.world.level.biome.source` unit).
//!
//! Java's `Biome.CODEC` is `RegistryFileCodec.create(Registries.BIOME,
//! Biome.DIRECT_CODEC)` over `Holder<Biome>`, and `Biome.LIST_CODEC` is the
//! `HolderSetCodec` over that element codec. The merged `biome.core` model
//! carries the biome reference as the pure [`rivet_registry::biome_id::BiomeId`]
//! handle (`OWNERSHIP`'s id model — the `BiomeResolver`/`NoiseBiomeSource`
//! traits return `Holder<BiomeId>`, and `MatchingBiomesPredicate` builds its
//! holder-set from the same id), so this unit's element codecs mirror the two
//! `Biome` codecs over `BiomeId`:
//!
//! - [`biome_id_codec`] — `RegistryFileCodec.create(Registries.BIOME,
//!   identifierCodec())`, the `Biome.CODEC` analogue (a reference holder
//!   resolves by identifier through the ops; an inline definition decodes to a
//!   `Direct` id).
//! - [`biome_id_list_codec`] — `HolderSetCodec.create(Registries.BIOME, ...)`,
//!   the `Biome.LIST_CODEC` analogue (tag-key or compacted element-list form).
//!
//! The element-value codec is `Identifier.CODEC.comapFlatMap(BiomeId::fromName
//! ...)` — Java's `RegistryFileCodec` encodes a reference by its key's
//! identifier and resolves an unknown name to the exact `"Unknown registry key
//! in <registry>: <name>"` error. Unlike `Biome::codec` (which carries the full
//! `Biome` shell), the id has no inline `DIRECT_CODEC`; the inline path stores
//! the numeric id directly (the pure-id model's `Direct(BiomeId)`), which is
//! how the `HolderSetCodec`'s `encodeWithoutRegistry`/`decodeWithoutRegistry`
//! fallbacks represent a set of built ids.

use rivet_registry::biome_id::BiomeId;
use rivet_registry::holder::Holder;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::identifier::Identifier;
use rivet_registry::registries;
use rivet_registry::registry_file_codec::{HolderSetCodec, RegistryFileCodec};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use std::sync::Arc;

/// `Biome.CODEC` over the id-handle — `RegistryFileCodec.create(
/// Registries.BIOME, identifierCodec())`.
///
/// Java (`Biome.java`): `public static final Codec<Holder<Biome>> CODEC =
/// RegistryFileCodec.create(Registries.BIOME, DIRECT_CODEC);` The id analogue
/// replaces the `DIRECT_CODEC` element with the identifier↔id codec: encode
/// emits the holder's identifier (a `Reference`), decode resolves the
/// identifier through the ops' getter, and an inline definition (the
/// `allowInline` path) produces `Holder::Direct(BiomeId)`.
pub fn biome_id_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Holder<BiomeId>, Ops>> {
    let element = identifier_codec::<Ops>();
    Arc::new(RegistryFileCodec::create(&registries::BIOME, element))
}

/// `Biome.LIST_CODEC` over the id-handle — `HolderSetCodec.create(
/// Registries.BIOME, biome_id_codec(), false)` (a tag-key or compacted
/// element-list set; `alwaysUseList = false`, matching `Biome.LIST_CODEC`).
pub fn biome_id_list_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<HolderSet<BiomeId>, Ops>> {
    let element = biome_id_codec::<Ops>();
    Arc::new(HolderSetCodec::create(&registries::BIOME, element, false))
}

/// The `"biome"` field variant — `Biome.CODEC.fieldOf("biome")`, the
/// `FixedBiomeSource`/`MultiNoiseBiomeSource.ENTRY_CODEC` field.
pub fn biome_id_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>(
    field: &str,
) -> Arc<dyn MapCodec<Holder<BiomeId>, Ops>> {
    codec::field_of(biome_id_codec::<Ops>(), field.to_string())
}

/// `Biome.LIST_CODEC.fieldOf(name)` — the `CheckerboardColumnBiomeSource`
/// `"biomes"` field.
pub fn biome_id_list_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>(
    field: &str,
) -> Arc<dyn MapCodec<HolderSet<BiomeId>, Ops>> {
    codec::field_of(biome_id_list_codec::<Ops>(), field.to_string())
}

/// `Identifier.CODEC.comapFlatMap(BiomeId::fromName, BiomeId::name)` — the
/// element value codec of the biome registry.
///
/// Java's `RegistryFileCodec` element is `Biome.DIRECT_CODEC` (the full biome
/// shell); the id analogue maps an identifier to its generated id on decode
/// and emits `BiomeId::name()` on encode for a `Direct` inline value. The
/// identifier→id mapping (and its `"Unknown registry key in <registry>: <name>"`
/// error) mirrors `Registry.byNameCodec` — the pure-id analogue of the biome
/// registry's value codec. The reference path of the wrapping `RegistryFileCodec`
/// (a string input resolved through the ops' getter) is handled by the
/// `RegistryFileCodec` itself, which reports `"Failed to get element <key>"`
/// for an unknown name; this element codec only runs on the inline (non-string)
/// path.
fn identifier_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BiomeId, Ops>> {
    codec::comap_flat_map::<Identifier, BiomeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(
            |name: &Identifier| match BiomeId::from_name(name.to_string().as_str()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in {}: {}",
                    *registries::BIOME,
                    name
                )),
            },
        ),
        Arc::new(|id: &BiomeId| Identifier::parse(id.name())),
    )
}
