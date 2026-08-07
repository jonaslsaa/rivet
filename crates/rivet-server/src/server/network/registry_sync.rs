//! Configuration registry-sync payload construction — the wire content of
//! `SynchronizeRegistriesTask` (issue #109).
//!
//! Java: `SynchronizeRegistriesTask.java` + `RegistrySynchronization.java` +
//! `TagNetworkSerialization.java` in `working/Paper`. Two payload families:
//!
//! - `pack_registries` — the 29 `ClientboundRegistryDataPacket`s
//!   (`RegistryDataLoader.SYNCHRONIZED_REGISTRIES`, one packet per registry),
//!   each element a `PackedRegistryEntry(id, Optional<Tag> data)`. Element ids
//!   and full-content payloads come from the generated `registry_data.rs` table
//!   (ascending registry id order, `registry.listElements()`); the `*synchronized.rs`
//!   name tables are cross-checked against it at generate time. The `data` is skipped
//!   (`Optional.empty()`) when the element's `knownPackInfo` is in the client's
//!   negotiated `KnownPack`s — for the M1 offline client that is the vanilla
//!   `minecraft:core:26.2` pack, so every vanilla element is empty. When the
//!   client did NOT accept (or the element has no matching pack, e.g.
//!   `paper:raw`), Paper encodes the element with its NBT codec; Rivet serves
//!   the canonical capture's pre-baked payloads (`registry_data.rs`) instead of
//!   porting the element codecs — see below.
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

use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::{nbt_io, tag::Tag};
use rivet_protocol::protocol::common::tag_network_payload::NetworkPayload;
use rivet_protocol::protocol::configuration::clientbound_registry_data::ClientboundRegistryDataPacket;
use rivet_protocol::protocol::configuration::packed_registry_entry::PackedRegistryEntry;
use rivet_registry::generated::registry_data::SYNCHRONIZED_NBT;
use rivet_registry::generated::tags::TAG_REGISTRIES;
use rivet_registry::{Identifier, Registry, ResourceKey};
use rivet_util::{DataInputStream, KnownPack};

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
/// otherwise.
///
/// The full-content payloads are the canonical join capture's per-element NBT
/// bytes, pre-baked by `tools/rivet-codegen` into `generated::registry_data`
/// (`SYNCHRONIZED_NBT`). The element codecs (`Biome.NETWORK_CODEC`,
/// `DimensionType.NETWORK_CODEC`, ...) are unported; the pre-baked bytes are
/// exactly what `elementCodec().encodeStart(NbtOps, value)` produced on the
/// pinned server, so serving them reproduces Paper's wire output byte-for-byte.
/// Each payload is decoded back into a `Tag` ([`decode_prebaked`]) and the
/// existing `PackedRegistryEntry` codec re-encodes it; `NbtIo` read/write
/// round-trips byte-for-byte (compound key order preserved via `IndexMap`,
/// DECISIONS.md D12), so the re-encoded bytes match the capture.
pub(crate) fn pack_registries(
    client_known_packs: &[KnownPack],
) -> Result<Vec<ClientboundRegistryDataPacket>, String> {
    // `SynchronizeRegistriesTask.handleResponse`: only when the client's reply
    // EXACTLY equals the requested packs does Paper skip element content
    // (`acceptedPacks.equals(this.requestedPacks)` — a `List.equals`, so the
    // order and multiplicity both matter — then `Set.copyOf(requestedPacks)`);
    // any other reply (empty, partial, superset, reordered, or duplicate)
    // forces the full-content path (`Set.of()`).
    let accepted_exactly = client_known_packs == requested_packs().as_slice();

    // Iterate `SYNCHRONIZED_NBT` (the pre-baked table) rather than
    // `SYNCHRONIZED_REGISTRIES`: the two are cross-checked at generate time
    // (registry set, order, and per-element names), so the table is the single
    // source of both the element ids and their content.
    let mut packets = Vec::with_capacity(SYNCHRONIZED_NBT.len());
    for (registry_key, elements) in SYNCHRONIZED_NBT {
        let entries = elements
            .iter()
            .map(|(name, payload)| {
                // `packRegistry.canSkipContents`: an element's `knownPackInfo`
                // is in the client's negotiated set. Every vanilla element
                // comes from `minecraft:core:26.2` (the only pack an exact
                // match implies); `paper:raw` has no known-pack, so it always
                // carries full content.
                let data = if accepted_exactly && is_vanilla_element(name) {
                    None
                } else {
                    Some(decode_prebaked(payload)?)
                };
                Ok(PackedRegistryEntry::new(Identifier::parse(name), data))
            })
            .collect::<Result<Vec<_>, String>>()?;
        packets.push(ClientboundRegistryDataPacket::new(
            ResourceKey::create_registry_key(Identifier::parse(registry_key)),
            entries,
        ));
    }
    Ok(packets)
}

/// Decode a pre-baked element payload back into a `Tag`.
///
/// `NbtIo.readAnyTag` over the payload (the `writeAnyTag` form: type byte +
/// payload) with `unlimitedHeap` — the codegen-validated payloads are the
/// capture's compounds. A failure here is a baked-data invariant violation
/// (drift or a codegen bug), surfaced as a deterministic disconnect.
fn decode_prebaked(payload: &[u8]) -> Result<Tag, String> {
    let mut input = DataInputStream::new(std::io::Cursor::new(payload));
    let mut accounter = NbtAccounter::unlimited_heap();
    nbt_io::read_any_tag(&mut input, &mut accounter)
        .map_err(|e| format!("pre-baked registry NBT payload failed to decode: {e}"))
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

    /// Encode a `ClientboundRegistryDataPacket` body the way the listener does
    /// (`encode_body`), for byte-identity comparison against the capture fixture.
    fn encode_registry_data_body(packet: &ClientboundRegistryDataPacket) -> Vec<u8> {
        use bytes::BytesMut;
        use rivet_protocol::codec::StreamEncoder;
        use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundRegistryDataPacket::stream_codec()
            .encode(&mut out, packet)
            .expect("encode registry_data");
        out.into_inner().to_vec()
    }

    // The `registry_data_capture.json` hex decoder, shared with the `offline_login`
    // integration test via a single `include!` (see the helper's header comment). The
    // helper's `include_str!("../fixtures/registry_data_capture.json")` resolves
    // relative to tests/support/ regardless of includer.
    include!("../../../tests/support/registry_data_capture.rs");

    #[test]
    fn requested_packs_is_vanilla_core() {
        assert_eq!(requested_packs(), vec![core_pack()]);
        assert_eq!(core_pack().to_string(), "minecraft:core:26.2");
    }

    #[test]
    fn pack_registries_accepted_pack_skips_vanilla_content() {
        // `handleResponse` with `acceptedPacks.equals(requestedPacks)` passes
        // `Set.copyOf(requestedPacks)`; every vanilla element's `knownPackInfo`
        // (`minecraft:core:26.2`) is in that set, so its content is skipped.
        // `paper:raw` (chat_type) has no known-pack, so it ALWAYS carries full
        // content (Paper `packRegistry.canSkipContents`).
        let accepted = vec![core_pack()];
        let packets = pack_registries(&accepted).unwrap();
        assert_eq!(packets.len(), SYNCHRONIZED_NBT.len());

        for (i, (key, elements)) in SYNCHRONIZED_NBT.iter().enumerate() {
            let packet = &packets[i];
            assert_eq!(packet.registry().identifier().to_string(), *key);
            assert_eq!(packet.entries().len(), elements.len(), "{key} entry count");
            for ((name, _payload), entry) in elements.iter().zip(packet.entries()) {
                assert_eq!(entry.id().to_string(), *name, "{key}");
                if is_vanilla_element(name) {
                    assert!(
                        entry.data().is_none(),
                        "{key}:{name} expected skipped content"
                    );
                } else {
                    assert!(
                        entry.data().is_some(),
                        "{key}:{name} (no known pack) expected full content"
                    );
                }
            }
        }
        // Only `paper:raw` is non-vanilla across the 29 registries.
        let non_vanilla: Vec<&str> = SYNCHRONIZED_NBT
            .iter()
            .flat_map(|(_, elements)| elements.iter())
            .filter(|(name, _)| !is_vanilla_element(name))
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(non_vanilla, vec!["paper:raw"]);
    }

    #[test]
    fn pack_registries_empty_packs_matches_capture_bytes() {
        // A client that accepts no packs (`Set.of()` in Java) forces the
        // full-content path. The committed fixture is exactly what the canonical
        // join capture recorded for that path; re-encoding the pre-baked payloads
        // must reproduce each packet body byte-for-byte (the capture's `registry_data`
        // bodies — the `PackedRegistryEntry` id+data stream order).
        let empty: Vec<KnownPack> = Vec::new();
        let packets = pack_registries(&empty).unwrap();
        let expected = registry_data_capture_bodies();
        assert_eq!(packets.len(), expected.len());
        for packet in &packets {
            let key = packet.registry().identifier().to_string();
            let encoded = encode_registry_data_body(packet);
            assert_eq!(
                &encoded,
                expected.get(&key).expect("fixture covers every registry"),
                "{key} body must match the capture byte-for-byte"
            );
        }
    }

    #[test]
    fn pack_registries_superset_accept_serves_full_content() {
        // `handleResponse` skips content ONLY when the accepted list EQUALS the
        // requested list (`List.equals`); a superset forces `Set.of()` — every
        // element fully encoded.
        let superset = vec![
            core_pack(),
            KnownPack::new("minecraft".into(), "bundle".into(), "26.2".into()),
        ];
        let packets = pack_registries(&superset).unwrap();
        assert_eq!(packets.len(), SYNCHRONIZED_NBT.len());
        for (i, (_, elements)) in SYNCHRONIZED_NBT.iter().enumerate() {
            assert_eq!(packets[i].entries().len(), elements.len());
            for entry in packets[i].entries() {
                assert!(entry.data().is_some(), "expected full content");
            }
        }
    }

    #[test]
    fn pack_registries_reordered_accept_serves_full_content() {
        // `List.equals` is order-sensitive: the reply is only an exact match in
        // the requested order, so a reordered multi-pack reply is NOT — full
        // content (`Set.of()`).
        let reordered = vec![
            KnownPack::new("minecraft".into(), "bundle".into(), "26.2".into()),
            core_pack(),
        ];
        let packets = pack_registries(&reordered).unwrap();
        assert_eq!(packets.len(), SYNCHRONIZED_NBT.len());
        for (i, (_, elements)) in SYNCHRONIZED_NBT.iter().enumerate() {
            assert_eq!(packets[i].entries().len(), elements.len());
            for entry in packets[i].entries() {
                assert!(entry.data().is_some(), "expected full content");
            }
        }
    }

    #[test]
    fn pack_registries_duplicate_accept_serves_full_content() {
        // `List.equals([core, core], [core])` is false (multiplicity), so a
        // duplicated reply is NOT an exact match — full content.
        let duplicate = vec![core_pack(), core_pack()];
        let packets = pack_registries(&duplicate).unwrap();
        assert_eq!(packets.len(), SYNCHRONIZED_NBT.len());
        for (i, (_, elements)) in SYNCHRONIZED_NBT.iter().enumerate() {
            assert_eq!(packets[i].entries().len(), elements.len());
            for entry in packets[i].entries() {
                assert!(entry.data().is_some(), "expected full content");
            }
        }
    }

    #[test]
    fn pack_registries_matches_capture_element_ids() {
        // The pre-baked table's element names (both the ids and their ordering)
        // are cross-checked at generate time against `synchronized_registries.json`/
        // the capture; here we pin that the served packet ids match the table.
        let empty: Vec<KnownPack> = Vec::new();
        let packets = pack_registries(&empty).unwrap();
        for (i, (key, elements)) in SYNCHRONIZED_NBT.iter().enumerate() {
            assert_eq!(packets[i].registry().identifier().to_string(), *key);
            let ids: Vec<String> = packets[i]
                .entries()
                .iter()
                .map(|e| e.id().to_string())
                .collect();
            let names: Vec<&str> = elements.iter().map(|(name, _)| *name).collect();
            assert_eq!(ids, names, "{key}");
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
