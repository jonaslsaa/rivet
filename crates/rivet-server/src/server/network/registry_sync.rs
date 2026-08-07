//! Configuration registry-sync payload construction — the wire content of
//! `SynchronizeRegistriesTask` (issue #109).
//!
//! Java: `SynchronizeRegistriesTask.java` + `RegistrySynchronization.java` +
//! `TagNetworkSerialization.java` in `working/Paper`. Two payload families:
//!
//! - `pack_registries` — the 29 `ClientboundRegistryDataPacket`s
//!   (`RegistryDataLoader.SYNCHRONIZED_REGISTRIES`, one packet per registry),
//!   each element a `PackedRegistryEntry(id, Optional<Tag> data)`. The element
//!   *ids* come from the generated `synchronized.rs` tables (ascending registry
//!   id order, `registry.listElements()`). The `data` is skipped
//!   (`Optional.empty()`) when the client accepted the advertised `KnownPack`s
//!   — for the M1 offline client that is the vanilla `minecraft:core:26.2`
//!   pack, so every entry is empty. When the client did NOT accept, Paper
//!   encodes each element with its NBT codec; those codecs are unported, so
//!   that path is a documented partial (see below).
//!
//! - `serialize_tags_to_network` — the `ClientboundUpdateTagsPacket` map
//!   (`TagNetworkSerialization.serializeTagsToNetwork` over
//!   `networkSafeRegistries` = WORLDGEN networkable + STATIC). Every
//!   tag-carrying registry's tag-location -> element ids, resolved from the
//!   generated `*_TAG_BY_NAME` maps through the per-registry `*_BY_NAME` id
//!   tables, with each tag's id list sorted ascending to match the canonical
//!   join capture (`structured::canon_update_tags` normalizes the per-boot
//!   wire order — see `serialize_registry_tags`).

use std::collections::HashMap;

use rivet_protocol::protocol::common::tag_network_payload::NetworkPayload;
use rivet_protocol::protocol::configuration::clientbound_registry_data::ClientboundRegistryDataPacket;
use rivet_protocol::protocol::configuration::packed_registry_entry::PackedRegistryEntry;
use rivet_registry::generated::synchronized::SYNCHRONIZED_REGISTRIES;
use rivet_registry::generated::tags::TAG_REGISTRIES;
use rivet_registry::{Identifier, Registry, ResourceKey};
use rivet_util::KnownPack;

/// The pack the M1 offline client bundles and accepts: `minecraft:core:26.2`.
///
/// Java: `MinecraftServer.getResourceManager().listPacks()...knownPackInfo()` —
/// the vanilla `minecraft:core` pack at the current data version. `26.2` is
/// `SharedConstants.getCurrentVersion().id()` for protocol 776 (the capture
/// advertises exactly `[minecraft:core:26.2]`). A function (not a `const`):
/// `KnownPack` owns `String`s.
pub(crate) fn core_pack() -> KnownPack {
    KnownPack::new(
        KnownPack::VANILLA_NAMESPACE.to_string(),
        "core".to_string(),
        "26.2".to_string(),
    )
}

/// The server's advertised known packs (`ClientboundSelectKnownPacks` body).
pub(crate) fn requested_packs() -> Vec<KnownPack> {
    vec![core_pack()]
}

/// Build the 29 `ClientboundRegistryDataPacket`s for the client's accepted
/// packs.
///
/// Mirrors `SynchronizeRegistriesTask.sendRegistries` ->
/// `RegistrySynchronization.packRegistries`: for each SYNCHRONIZED_REGISTRIES
/// registry, each element's `data` is `Optional.empty()` when the element's
/// `RegistrationInfo.knownPackInfo` is present in the negotiated set, else the
/// element codec's NBT encoding. `handleResponse` passes `Set.copyOf
/// (requestedPacks)` when the reply equals the requested list, `Set.of()`
/// otherwise; the M1 server registers every synchronized element from the
/// vanilla `minecraft:core` pack, so when the client accepted that pack every
/// entry is empty.
///
/// The full-content path (the client did NOT accept the pack, so Paper encodes
/// each element) requires the per-registry NBT element codecs (`Biome.NETWORK_CODEC`,
/// `DimensionType.NETWORK_CODEC`, ...), which are unported — see
/// `RegistrySynchronization.packRegistry`. It is therefore a hard error here,
/// surfaced as a deterministic disconnect, with the wire content left to #109.
pub(crate) fn pack_registries(
    client_known_packs: &[KnownPack],
) -> Result<Vec<ClientboundRegistryDataPacket>, String> {
    // `SynchronizeRegistriesTask.handleResponse`: only when the client's reply
    // EXACTLY equals the requested packs does Paper skip element content
    // (`acceptedPacks.equals(this.requestedPacks)` — a `List.equals`, so the
    // order and multiplicity both matter — then `Set.copyOf(requestedPacks)`);
    // any other reply (empty, partial, superset, reordered, or duplicate)
    // forces the full-content path (`Set.of()`).
    let requested = requested_packs();
    let accepted_exactly = client_known_packs == requested.as_slice();
    if !accepted_exactly {
        // RivetTodo(#109): the full NBT element content
        // (`registryData.elementCodec().encodeStart(...)`) when the client does
        // not accept the advertised pack. The element codecs are unported, so a
        // non-accepting client cannot be served — Paper would encode every
        // element, which we cannot produce faithfully.
        return Err(
            "client did not accept exactly the advertised `minecraft:core:26.2` pack; \
             the full NBT registry content (element codecs) is unported (#109)"
                .to_string(),
        );
    }

    let mut packets = Vec::with_capacity(SYNCHRONIZED_REGISTRIES.len());
    for (registry_key, element_names) in SYNCHRONIZED_REGISTRIES {
        let entries = element_names
            .iter()
            // `paper:raw` (chat_type) is a Paper runtime registry addition, not
            // from the `minecraft:core` pack; the M1 server does not register it
            // and its content codec is unported. Excluding it keeps the
            // advertised registry vanilla-faithful.
            // RivetTodo(#109): Paper's custom registry additions (`paper:raw`)
            // and their registration/lifecycle.
            .filter(|name| is_vanilla_element(name))
            .map(|name| PackedRegistryEntry::new(Identifier::parse(name), None))
            .collect();
        packets.push(ClientboundRegistryDataPacket::new(
            ResourceKey::create_registry_key(Identifier::parse(registry_key)),
            entries,
        ));
    }
    Ok(packets)
}

/// The `ClientboundUpdateTagsPacket` body for the M1 registries.
///
/// Mirrors `TagNetworkSerialization.serializeTagsToNetwork`: for each
/// tag-carrying registry, each tag's element ids in tag-file value order
/// (`registry.getId(holder.value())` over the tag's holders). Only registries
/// with at least one tag are emitted (Java filters `!payload.isEmpty()`).
pub(crate) fn serialize_tags_to_network() -> HashMap<ResourceKey<Registry<()>>, NetworkPayload> {
    let mut result = HashMap::new();
    for registry in TAG_REGISTRIES {
        let payload = serialize_registry_tags(registry);
        if !payload.is_empty() {
            result.insert(
                ResourceKey::create_registry_key(Identifier::parse(registry)),
                payload,
            );
        }
    }
    result
}

/// One registry's tag-location -> element ids (`NetworkPayload.tags`).
fn serialize_registry_tags(registry: &str) -> NetworkPayload {
    let (tag_map, id_map) = tag_tables(registry);
    let mut tags = HashMap::with_capacity(tag_map.len());
    for (tag, element_names) in tag_map.entries() {
        let mut ids: Vec<i32> = element_names
            .iter()
            .map(|name| {
                // Every tag element resolves through the registry's dense
                // `*_BY_NAME` id table (element id == holder id == network id).
                *id_map.get(name).unwrap_or_else(|| {
                    panic!("`{registry}` tag `{tag}` references unknown element `{name}`")
                }) as i32
            })
            .collect();
        // The canonical join capture normalizes each tag's id list to ascending
        // id order (`structured::canon_update_tags` sorts the `IntList`, the
        // per-boot order Paper actually sends being a `HashMap`/file-order
        // artifact). Rivet matches the canonical capture's id-list content.
        ids.sort_unstable();
        tags.insert(Identifier::parse(tag), ids);
    }
    NetworkPayload::new(tags)
}

/// The `(TAG_BY_NAME, BY_NAME)` table pair for a tag-carrying registry.
///
/// The 8 shared surfaces resolve through the tables in
/// `biomes.rs`/`blocks.rs`/`registries.rs`; the 7 datapack registries the
/// report cannot cover carry both tables in `tags.rs`.
fn tag_tables(
    registry: &str,
) -> (
    &'static phf::Map<&'static str, &'static [&'static str]>,
    &'static phf::Map<&'static str, u16>,
) {
    use rivet_registry::generated::biomes::BIOME_BY_NAME;
    use rivet_registry::generated::blocks::BLOCK_BY_NAME;
    use rivet_registry::generated::registries::{
        ENTITY_TYPE_BY_NAME, FLUID_BY_NAME, GAME_EVENT_BY_NAME, ITEM_BY_NAME,
        POINT_OF_INTEREST_TYPE_BY_NAME, POTION_BY_NAME,
    };
    use rivet_registry::generated::tags::{
        BANNER_PATTERN_BY_NAME, BANNER_PATTERN_TAG_BY_NAME, BLOCK_TAG_BY_NAME, DAMAGE_TYPE_BY_NAME,
        DAMAGE_TYPE_TAG_BY_NAME, DIALOG_BY_NAME, DIALOG_TAG_BY_NAME, ENCHANTMENT_BY_NAME,
        ENCHANTMENT_TAG_BY_NAME, ENTITY_TYPE_TAG_BY_NAME, FLUID_TAG_BY_NAME,
        GAME_EVENT_TAG_BY_NAME, INSTRUMENT_BY_NAME, INSTRUMENT_TAG_BY_NAME, ITEM_TAG_BY_NAME,
        PAINTING_VARIANT_BY_NAME, PAINTING_VARIANT_TAG_BY_NAME, POINT_OF_INTEREST_TYPE_TAG_BY_NAME,
        POTION_TAG_BY_NAME, TIMELINE_BY_NAME, TIMELINE_TAG_BY_NAME, WORLDGEN_BIOME_TAG_BY_NAME,
    };

    match registry {
        "minecraft:worldgen/biome" => (&WORLDGEN_BIOME_TAG_BY_NAME, &BIOME_BY_NAME),
        "minecraft:block" => (&BLOCK_TAG_BY_NAME, &BLOCK_BY_NAME),
        "minecraft:item" => (&ITEM_TAG_BY_NAME, &ITEM_BY_NAME),
        "minecraft:entity_type" => (&ENTITY_TYPE_TAG_BY_NAME, &ENTITY_TYPE_BY_NAME),
        "minecraft:fluid" => (&FLUID_TAG_BY_NAME, &FLUID_BY_NAME),
        "minecraft:game_event" => (&GAME_EVENT_TAG_BY_NAME, &GAME_EVENT_BY_NAME),
        "minecraft:potion" => (&POTION_TAG_BY_NAME, &POTION_BY_NAME),
        "minecraft:point_of_interest_type" => (
            &POINT_OF_INTEREST_TYPE_TAG_BY_NAME,
            &POINT_OF_INTEREST_TYPE_BY_NAME,
        ),
        "minecraft:enchantment" => (&ENCHANTMENT_TAG_BY_NAME, &ENCHANTMENT_BY_NAME),
        "minecraft:dialog" => (&DIALOG_TAG_BY_NAME, &DIALOG_BY_NAME),
        "minecraft:painting_variant" => (&PAINTING_VARIANT_TAG_BY_NAME, &PAINTING_VARIANT_BY_NAME),
        "minecraft:timeline" => (&TIMELINE_TAG_BY_NAME, &TIMELINE_BY_NAME),
        "minecraft:instrument" => (&INSTRUMENT_TAG_BY_NAME, &INSTRUMENT_BY_NAME),
        "minecraft:banner_pattern" => (&BANNER_PATTERN_TAG_BY_NAME, &BANNER_PATTERN_BY_NAME),
        "minecraft:damage_type" => (&DAMAGE_TYPE_TAG_BY_NAME, &DAMAGE_TYPE_BY_NAME),
        other => panic!("no tag tables for `{other}`"),
    }
}

/// Whether a synchronized element is vanilla (in the `minecraft:core` pack).
/// The M1 server registers only vanilla elements; Paper's custom additions
/// (e.g. `paper:raw` in chat_type) are excluded (see `pack_registries`).
fn is_vanilla_element(name: &str) -> bool {
    name.starts_with("minecraft:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_packs_is_vanilla_core() {
        assert_eq!(requested_packs(), vec![core_pack()]);
        assert_eq!(core_pack().to_string(), "minecraft:core:26.2");
    }

    #[test]
    fn pack_registries_accepted_pack_skips_all_content() {
        let accepted = vec![core_pack()];
        let packets = pack_registries(&accepted).unwrap();
        assert_eq!(packets.len(), SYNCHRONIZED_REGISTRIES.len());

        for (i, packet) in packets.iter().enumerate() {
            let (key, element_names) = SYNCHRONIZED_REGISTRIES[i];
            assert_eq!(packet.registry().identifier().to_string(), key);
            let vanilla: Vec<&&str> = element_names
                .iter()
                .filter(|n| is_vanilla_element(n))
                .collect();
            assert_eq!(packet.entries().len(), vanilla.len(), "{key} entry count");
            for entry in packet.entries() {
                assert!(entry.data().is_none(), "{key}: expected skipped content");
            }
        }
    }

    #[test]
    fn pack_registries_rejects_non_accepting_client() {
        let empty: Vec<KnownPack> = Vec::new();
        assert!(pack_registries(&empty).is_err());
    }

    #[test]
    fn pack_registries_rejects_partial_or_superset_accept() {
        // Java's `handleResponse` only skips content when the accepted list
        // EQUALS the requested list (`List.equals`); a superset, reorder, or
        // duplicate all force full content (unported).
        let superset = vec![
            core_pack(),
            KnownPack::new("minecraft".into(), "bundle".into(), "26.2".into()),
        ];
        assert!(pack_registries(&superset).is_err());
    }

    #[test]
    fn pack_registries_rejects_duplicate_accept() {
        // Java's `List.equals([core, core], [core])` is false (multiplicity),
        // so a duplicated reply is NOT an exact match — full content, unported.
        let duplicate = vec![core_pack(), core_pack()];
        assert!(pack_registries(&duplicate).is_err());
    }

    #[test]
    fn pack_registries_matches_capture_element_ids() {
        // Spot-check against the canonical join capture (the `registry_data`
        // packet bodies): every synchronized registry's packet carries exactly
        // the capture's element names in ascending-id order. The shared 8 are
        // also cross-checked at generate time against the id tables.
        let accepted = vec![core_pack()];
        let packets = pack_registries(&accepted).unwrap();
        for (packet, (key, element_names)) in packets.iter().zip(SYNCHRONIZED_REGISTRIES.iter()) {
            assert_eq!(packet.registry().identifier().to_string(), *key);
            let vanilla: Vec<String> = element_names
                .iter()
                .filter(|n| is_vanilla_element(n))
                .map(|n| n.to_string())
                .collect();
            let ids: Vec<String> = packet
                .entries()
                .iter()
                .map(|e| e.id().to_string())
                .collect();
            assert_eq!(ids, vanilla, "{key}");
        }
    }

    #[test]
    fn serialize_tags_covers_all_15_registries() {
        let tags = serialize_tags_to_network();
        assert_eq!(tags.len(), TAG_REGISTRIES.len(), "15 tag registries");
        // The capture's per-registry tag counts (`biomes_tags.json`, a live
        // Paper 26.2 `serializeTagsToNetwork` dump). A drift in which tags a
        // registry carries fails here.
        let expected_counts: &[(&str, usize)] = &[
            ("minecraft:worldgen/biome", 68),
            ("minecraft:block", 265),
            ("minecraft:item", 224),
            ("minecraft:entity_type", 48),
            ("minecraft:damage_type", 34),
            ("minecraft:enchantment", 22),
            ("minecraft:banner_pattern", 11),
            ("minecraft:fluid", 6),
            ("minecraft:game_event", 5),
            ("minecraft:timeline", 4),
            ("minecraft:instrument", 3),
            ("minecraft:point_of_interest_type", 3),
            ("minecraft:dialog", 2),
            ("minecraft:painting_variant", 1),
            ("minecraft:potion", 1),
        ];
        let mut total = 0;
        for (registry, expected) in expected_counts {
            let key = ResourceKey::create_registry_key(Identifier::parse(registry));
            let payload = &tags[&key];
            assert!(!payload.is_empty(), "{registry} has tags");
            assert_eq!(payload.size(), *expected, "{registry} tag count");
            total += payload.size();
        }
        assert_eq!(total, 697, "capture total tag count");
    }

    #[test]
    fn tag_ids_resolve_through_by_name_tables() {
        let tags = serialize_tags_to_network();
        let biome_key =
            ResourceKey::create_registry_key(Identifier::parse("minecraft:worldgen/biome"));
        let biome = &tags[&biome_key];
        // `minecraft:allows_surface_slime_spawns` = {swamp, mangrove_swamp}.
        // The canonical capture normalizes each tag's id list ascending
        // (`structured::canon_update_tags`), so ids are sorted even though the
        // tag file lists swamp first: swamp id 55, mangrove_swamp id 31.
        let tag = Identifier::parse("minecraft:allows_surface_slime_spawns");
        let ids = &biome.tags()[&tag];
        assert_eq!(*ids, vec![31, 55]);
    }
}
