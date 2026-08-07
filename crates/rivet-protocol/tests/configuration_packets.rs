//! Java-grounded tests for the issue #109 configuration-phase packet bodies
//! (`crates/rivet-protocol/src/protocol/configuration/` + the shared
//! `common::clientbound_update_tags` / `common::tag_network_payload`).
//!
//! Every packet body present in the pinned #194/#219 configuration capture
//! (protocol 776, Paper `26.2-DEV-main@0a99345`) is committed as a hex golden
//! body in `tests/fixtures/config_*.hex`; this suite decodes each one and pins
//! the wire facts:
//! - `clientbound_finish_configuration` / `serverbound_finish_configuration`:
//!   empty `StreamCodec.unit(INSTANCE)` bodies.
//! - `clientbound_select_known_packs` / `serverbound_select_known_packs`: the
//!   `KnownPack` list (the serverbound is `list(64)`-bounded).
//! - `clientbound_update_enabled_features`: the `HashSet<Identifier>` flags.
//! - `clientbound_registry_data` (world_clock, test_environment, test_instance,
//!   timeline): the erased registry key + `PackedRegistryEntry` list with NBT
//!   payloads (the pinned capture's real registry data).
//! - `clientbound_update_tags` (the 35 KB trailer): the 15-registry tag map.
//!
//! Where the wire order is contractually stable (a `Vec`, a single-element set,
//! an empty compound), the re-encode is asserted byte-identical; where the order
//! is a Java `HashMap`/`HashSet` iteration (multi-element set/map), the capture
//! semantics rule applies (`PORTING.md`) and the test asserts decode→encode→decode
//! content equivalence instead.
//!
//! The hostile/truncation tests pin Java's panic-vs-error split: raw
//! `FriendlyByteBuf` scalar paths (`Identifier.parse`, guava `checkNonnegative`,
//! netty EOF) panic; codec-boundary paths return `Err` (a hostile wire value
//! closes just that connection). The controlled-mutation tests are the
//! do-not-weaken counterfactual checks. Registration coverage pins the vanilla
//! configuration ids against the generated tables (#50).
//!
//! Gated on the `packets` feature (the `configuration`/`common` body modules
//! live behind it).

use bytes::BytesMut;
use rivet_protocol::codec::{StreamCodec, StreamDecoder, StreamEncoder, map};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::packets;
use rivet_protocol::generated::protocol::{ConnectionProtocol, PacketFlow};
use rivet_protocol::protocol::common::clientbound_update_tags::ClientboundUpdateTagsPacket;
use rivet_protocol::protocol::common::packet_types::clientbound_update_tags;
use rivet_protocol::protocol::common::tag_network_payload::NetworkPayload;
use rivet_protocol::protocol::configuration::clientbound_finish_configuration::{
    self, ClientboundFinishConfigurationPacket,
};
use rivet_protocol::protocol::configuration::clientbound_registry_data::ClientboundRegistryDataPacket;
use rivet_protocol::protocol::configuration::clientbound_select_known_packs::ClientboundSelectKnownPacks;
use rivet_protocol::protocol::configuration::clientbound_update_enabled_features::ClientboundUpdateEnabledFeaturesPacket;
use rivet_protocol::protocol::configuration::packet_types::{self as config_packet_types};
use rivet_protocol::protocol::configuration::serverbound_finish_configuration::{
    self, ServerboundFinishConfigurationPacket,
};
use rivet_protocol::protocol::configuration::serverbound_select_known_packs::ServerboundSelectKnownPacks;
use rivet_protocol::protocol::{Packet, PacketType, clientbound_protocol, serverbound_protocol};
use rivet_registry::{Identifier, Registry, ResourceKey};
use rivet_util::KnownPack;
use std::fmt;
use std::panic::catch_unwind;

/// Hex body -> `Vec<u8>`.
fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Load a committed golden fixture (`tests/fixtures/{name}.hex`).
fn fixture_hex(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{}.hex", env!("CARGO_MANIFEST_DIR"), name);
    hex(std::fs::read_to_string(path).expect("fixture").trim())
}

fn buf() -> FriendlyByteBuf {
    FriendlyByteBuf::new(BytesMut::new())
}

fn written(b: FriendlyByteBuf) -> Vec<u8> {
    b.into_inner().to_vec()
}

fn panic_message<F: FnOnce() -> R, R>(f: F) -> String {
    let err = match catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(_) => panic!("expected the closure to panic"),
        Err(err) => err,
    };
    err.downcast_ref::<String>()
        .cloned()
        .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "non-string panic payload".to_string())
}

// ---------------------------------------------------------------------------
// The finish_configuration pair: empty unit-codec bodies.
// ---------------------------------------------------------------------------

#[test]
fn finish_configuration_fixtures_are_empty_bodies_and_round_trip() {
    // The pinned capture's finish packets have no body.
    assert!(fixture_hex("config_clientbound_finish").is_empty());
    assert!(fixture_hex("config_serverbound_finish").is_empty());

    // `StreamCodec.unit(INSTANCE)`: encode writes nothing, decode reads nothing.
    let mut out = buf();
    clientbound_finish_configuration::stream_codec()
        .encode(&mut out, &ClientboundFinishConfigurationPacket)
        .unwrap();
    assert!(written(out).is_empty());
    let mut input = buf();
    assert_eq!(
        clientbound_finish_configuration::stream_codec()
            .decode(&mut input)
            .unwrap(),
        ClientboundFinishConfigurationPacket
    );
    assert_eq!(input.readable_bytes(), 0);

    let mut out = buf();
    serverbound_finish_configuration::stream_codec()
        .encode(&mut out, &ServerboundFinishConfigurationPacket)
        .unwrap();
    assert!(written(out).is_empty());
    let mut input = buf();
    assert_eq!(
        serverbound_finish_configuration::stream_codec()
            .decode(&mut input)
            .unwrap(),
        ServerboundFinishConfigurationPacket
    );
    assert_eq!(input.readable_bytes(), 0);
}

// ---------------------------------------------------------------------------
// select_known_packs: the clientbound is an unbounded KnownPack list; the
// serverbound is `list(64)`.
// ---------------------------------------------------------------------------

#[test]
fn serverbound_select_known_packs_fixture_is_empty_list() {
    // The pinned capture's serverbound reply is `00` — count 0, no packs.
    let bytes = fixture_hex("config_serverbound_select_known_packs");
    assert_eq!(bytes, vec![0x00]);
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    let decoded = ServerboundSelectKnownPacks::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0);
    assert!(decoded.known_packs().is_empty());

    // Re-encode byte-identically.
    let mut out = buf();
    ServerboundSelectKnownPacks::stream_codec()
        .encode(&mut out, &decoded)
        .unwrap();
    assert_eq!(written(out), vec![0x00]);
}

#[test]
fn clientbound_select_known_packs_fixture_golden_body() {
    // The capture's `select_known_packs` (clientbound id 14) is the single
    // `minecraft/core@26.2` pack — the three STRING_UTF8 fields in record order.
    let bytes = fixture_hex("config_clientbound_select_known_packs");
    assert_eq!(bytes, hex("01096d696e65637261667404636f72650432362e32"));
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    let decoded = ClientboundSelectKnownPacks::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0);
    assert_eq!(
        decoded.known_packs(),
        &[KnownPack::new(
            "minecraft".to_string(),
            "core".to_string(),
            "26.2".to_string()
        )]
    );

    // The list is a Vec — wire order is stable, so the re-encode is byte-exact.
    let mut out = buf();
    ClientboundSelectKnownPacks::stream_codec()
        .encode(&mut out, &decoded)
        .unwrap();
    assert_eq!(written(out), bytes);
}

#[test]
fn serverbound_select_known_packs_bounds_at_64() {
    // `list(64)` decode: a 65-element wire count errors with Java's text.
    let mut out = buf();
    out.write_var_int(65);
    let err = ServerboundSelectKnownPacks::stream_codec()
        .decode(&mut out)
        .unwrap_err();
    assert_eq!(err.message, "65 elements exceeded max size of: 64");

    // Encode side errors the same way.
    let packs: Vec<KnownPack> = (0..65)
        .map(|i| KnownPack::new(format!("m{i}"), "c".to_string(), "1".to_string()))
        .collect();
    let err = ServerboundSelectKnownPacks::stream_codec()
        .encode(&mut buf(), &ServerboundSelectKnownPacks::new(packs))
        .unwrap_err();
    assert_eq!(err.message, "65 elements exceeded max size of: 64");

    // A negative count passes `readCount`'s upper bound and hits `ArrayList(int)`.
    let mut out = buf();
    out.write_var_int(-1);
    let msg = panic_message(|| {
        let _ = ServerboundSelectKnownPacks::stream_codec().decode(&mut out);
    });
    assert_eq!(msg, "Illegal Capacity: -1");
}

// ---------------------------------------------------------------------------
// update_enabled_features: the HashSet<Identifier> flags.
// ---------------------------------------------------------------------------

#[test]
fn update_enabled_features_fixture_golden_body() {
    // The capture's trailing flags packet carries the single "vanilla" flag.
    let bytes = fixture_hex("config_update_enabled_features");
    assert_eq!(bytes, hex("01116d696e6563726166743a76616e696c6c61"));
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    let decoded = ClientboundUpdateEnabledFeaturesPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0);
    assert_eq!(decoded.features().len(), 1);
    assert!(
        decoded
            .features()
            .contains(&Identifier::with_default_namespace("vanilla"))
    );

    // A single-element HashSet iterates deterministically — byte-exact re-encode.
    let mut out = buf();
    ClientboundUpdateEnabledFeaturesPacket::stream_codec()
        .encode(&mut out, &decoded)
        .unwrap();
    assert_eq!(written(out), bytes);
}

#[test]
fn update_enabled_features_negative_count_panics_like_java() {
    // The decode ctor binds `HashSet(int initialCapacity)` — a negative wire
    // count throws Java's IllegalArgumentException.
    let mut out = buf();
    out.write_var_int(-1);
    let msg = panic_message(|| {
        let _ = ClientboundUpdateEnabledFeaturesPacket::stream_codec().decode(&mut out);
    });
    assert_eq!(msg, "Illegal initial capacity: -1");
}

#[test]
fn update_enabled_features_malformed_flag_errors_not_panics() {
    // A hostile flag (`minecraft:aA`, uppercase path char) is a Java
    // `IdentifierException`; the codec boundary returns `Err` (the connection
    // closes) — it does not panic.
    let mut out = buf();
    out.write_var_int(1);
    out.write_utf("minecraft:aA");
    let mut input = FriendlyByteBuf::new(out.into_inner());
    let err = ClientboundUpdateEnabledFeaturesPacket::stream_codec()
        .decode(&mut input)
        .unwrap_err();
    assert_eq!(
        err.message,
        "Non [a-z0-9/._-] character in path of location: minecraft:aA"
    );
}

#[test]
fn update_enabled_features_truncated_flag_errors() {
    // A length prefix that runs off the buffer: the string_utf8 codec boundary
    // returns Err with Java's "Not enough bytes..." text (a hostile wire value
    // closes the connection) — it does not panic.
    let mut input = buf();
    input.write_var_int(1);
    input.write_var_int(17);
    input.write_bytes(b"minecraft:vanill"); // 16 of the declared 17 bytes
    let err = ClientboundUpdateEnabledFeaturesPacket::stream_codec()
        .decode(&mut input)
        .unwrap_err();
    assert_eq!(
        err.message,
        "Not enough bytes in buffer, expected 17, but got 16"
    );
}

// ---------------------------------------------------------------------------
// registry_data: the erased registry key + PackedRegistryEntry list with NBT
// payloads. The wire order is a Vec, so the re-encode is byte-exact when the
// NBT itself round-trips; content-equivalence is asserted for the real-data
// fixtures.
// ---------------------------------------------------------------------------

#[test]
fn registry_data_world_clock_fixture_golden_body() {
    // The capture's `world_clock` registry: two entries, each an empty compound
    // (the client is told the values exist but the NBT is empty in this capture
    // — the server skipped the clock data).
    let bytes = fixture_hex("config_registry_data_world_clock");
    assert_eq!(
        bytes,
        hex(
            "156d696e6563726166743a776f726c645f636c6f636b02136d696e6563726166743a6f766572776f726c64010a00116d696e6563726166743a7468655f656e64010a00"
        )
    );
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    let decoded = ClientboundRegistryDataPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0);

    assert_eq!(
        decoded.registry().identifier().to_string(),
        "minecraft:world_clock"
    );
    assert_eq!(decoded.entries().len(), 2);
    assert_eq!(decoded.entries()[0].id().to_string(), "minecraft:overworld");
    // Both entries are present with an empty compound.
    assert!(matches!(
        decoded.entries()[0].data(),
        Some(rivet_nbt::tag::Tag::Compound(c)) if c.is_empty()
    ));
    assert_eq!(decoded.entries()[1].id().to_string(), "minecraft:the_end");
    assert!(matches!(
        decoded.entries()[1].data(),
        Some(rivet_nbt::tag::Tag::Compound(c)) if c.is_empty()
    ));

    // Stable wire order (a Vec + empty compounds) — byte-exact re-encode.
    let mut out = buf();
    ClientboundRegistryDataPacket::stream_codec()
        .encode(&mut out, &decoded)
        .unwrap();
    assert_eq!(written(out), bytes);
}

/// Decode a `registry_data` fixture, assert its structural facts, then prove
/// decode→encode→decode content equivalence (the entries' NBT payloads are real
/// data here, so this is the capture-semantics path).
fn assert_registry_data_content_equivalence(name: &str, expected_entries: usize) {
    let bytes = fixture_hex(name);
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    let decoded = ClientboundRegistryDataPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0, "{name}: trailing bytes");
    assert_eq!(
        decoded.entries().len(),
        expected_entries,
        "{name}: entry count"
    );

    // Every entry id round-trips Display -> parse and each present entry decodes
    // NBT.
    for (i, entry) in decoded.entries().iter().enumerate() {
        assert_eq!(
            Identifier::parse(&entry.id().to_string()),
            entry.id().clone(),
            "{name} entry {i}: id Display does not round-trip"
        );
        if let Some(tag) = entry.data() {
            assert!(
                matches!(tag, rivet_nbt::tag::Tag::Compound(_)),
                "{name} entry {i}: non-compound NBT"
            );
        }
    }

    // Encode back, decode again, and compare content — the HashMap-free wire
    // order (registry key + Vec) plus deterministic NBT makes this a true
    // round-trip, but content equality is what the capture semantics require.
    let mut out = buf();
    ClientboundRegistryDataPacket::stream_codec()
        .encode(&mut out, &decoded)
        .unwrap();
    let mut input = FriendlyByteBuf::new(out.into_inner());
    let redecoded = ClientboundRegistryDataPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(
        input.readable_bytes(),
        0,
        "{name}: trailing after re-decode"
    );
    assert_eq!(redecoded, decoded, "{name}: content equivalence");
}

#[test]
fn registry_data_test_environment_fixture_content_equivalence() {
    // The capture's synthetic `test_environment` registry: one `default` entry
    // with a definitions/type/all_of compound.
    let bytes = fixture_hex("config_registry_data_test_environment");
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    let decoded = ClientboundRegistryDataPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0);
    assert_eq!(
        decoded.registry().identifier().to_string(),
        "minecraft:test_environment"
    );
    assert_eq!(decoded.entries().len(), 1);
    assert_eq!(decoded.entries()[0].id().to_string(), "minecraft:default");
    let data = decoded.entries()[0]
        .data()
        .expect("default entry has NBT data");
    assert!(matches!(data, rivet_nbt::tag::Tag::Compound(_)));

    // Content-equivalence round trip.
    assert_registry_data_content_equivalence("config_registry_data_test_environment", 1);
}

#[test]
fn registry_data_test_instance_fixture_content_equivalence() {
    // The capture's `test_instance` registry: one `always_pass` entry.
    let bytes = fixture_hex("config_registry_data_test_instance");
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    let decoded = ClientboundRegistryDataPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0);
    assert_eq!(
        decoded.registry().identifier().to_string(),
        "minecraft:test_instance"
    );
    assert_eq!(decoded.entries().len(), 1);
    assert_eq!(
        decoded.entries()[0].id().to_string(),
        "minecraft:always_pass"
    );
    assert!(decoded.entries()[0].data().is_some());

    assert_registry_data_content_equivalence("config_registry_data_test_instance", 1);
}

#[test]
fn registry_data_timeline_fixture_content_equivalence() {
    // The capture's `timeline` registry: four entries (day, overworld,
    // midnight/night) with real time-of-day NBT.
    let bytes = fixture_hex("config_registry_data_timeline");
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    let decoded = ClientboundRegistryDataPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0);
    assert_eq!(
        decoded.registry().identifier().to_string(),
        "minecraft:timeline"
    );
    assert_eq!(decoded.entries().len(), 4);
    assert_eq!(decoded.entries()[0].id().to_string(), "minecraft:day");
    assert!(decoded.entries()[0].data().is_some());

    assert_registry_data_content_equivalence("config_registry_data_timeline", 4);
}

#[test]
fn registry_data_negative_entry_count_panics_like_java() {
    // The entries list is `ByteBufCodecs.list()`; a negative count hits
    // `ArrayList(int)`'s IllegalArgumentException (list decode), Java's
    // `newArrayListWithExpectedSize` path.
    let mut out = buf();
    out.write_utf("minecraft:world_clock");
    out.write_var_int(-1);
    let msg = panic_message(|| {
        let _ = ClientboundRegistryDataPacket::stream_codec().decode(&mut out);
    });
    assert_eq!(msg, "Illegal Capacity: -1");
}

#[test]
fn registry_data_hostile_registry_key_errors() {
    // The registry key goes through `identifier_codec` (the codec boundary), so
    // a hostile id returns Err instead of panicking.
    let mut out = buf();
    out.write_utf("minecraft:aA");
    out.write_var_int(0);
    let mut input = FriendlyByteBuf::new(out.into_inner());
    let err = ClientboundRegistryDataPacket::stream_codec()
        .decode(&mut input)
        .unwrap_err();
    assert_eq!(
        err.message,
        "Non [a-z0-9/._-] character in path of location: minecraft:aA"
    );
}

// ---------------------------------------------------------------------------
// update_tags: the 35 KB trailer — a 15-registry map of tag->ids.
// ---------------------------------------------------------------------------

#[test]
fn update_tags_fixture_content_equivalence() {
    let bytes = fixture_hex("config_update_tags");
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    let decoded = ClientboundUpdateTagsPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0);

    let tags = decoded.tags();
    assert_eq!(tags.len(), 15);
    // The first three registries' tag-count facts (from the pinned capture).
    let banner = tags
        .get(&erased("minecraft:banner_pattern"))
        .expect("banner_pattern present");
    assert_eq!(banner.tags().len(), 11);
    assert_eq!(
        banner.tags()[&Identifier::with_default_namespace("no_item_required")],
        vec![
            1, 3, 5, 7, 8, 9, 10, 14, 15, 17, 18, 19, 20, 23, 25, 26, 27, 28, 29, 30, 31, 32, 33,
            34, 35, 36, 37, 38, 39, 40, 41, 42
        ]
    );
    let block = tags.get(&erased("minecraft:block")).expect("block present");
    assert_eq!(block.tags().len(), 265);
    assert_eq!(
        block.tags()[&Identifier::with_default_namespace("air")],
        vec![0, 794, 795]
    );
    let damage = tags
        .get(&erased("minecraft:damage_type"))
        .expect("damage_type present");
    assert_eq!(damage.tags().len(), 34);
    assert_eq!(
        damage.tags()[&Identifier::with_default_namespace("always_hurts_ender_dragons")],
        vec![1, 9, 15, 35]
    );

    // The registry map is a Java HashMap on the wire (order not contractual);
    // the value equality is what the capture semantics require.
    let mut out = buf();
    ClientboundUpdateTagsPacket::stream_codec()
        .encode(&mut out, &decoded)
        .unwrap();
    let mut input = FriendlyByteBuf::new(out.into_inner());
    let redecoded = ClientboundUpdateTagsPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0);
    assert_eq!(redecoded, decoded);
}

/// The erased `ResourceKey<Registry<()>>` for a registry-name identifier.
fn erased(name: &str) -> ResourceKey<Registry<()>> {
    ResourceKey::<()>::create_registry_key(Identifier::parse(name))
}

#[test]
fn update_tags_negative_registry_count_panics_like_java() {
    // `readMap(Maps::newHashMapWithExpectedSize, ...)` — guava's checkNonnegative.
    let mut out = buf();
    out.write_var_int(-1);
    let msg = panic_message(|| {
        let _ = ClientboundUpdateTagsPacket::stream_codec().decode(&mut out);
    });
    assert_eq!(msg, "expectedSize cannot be negative but was: -1");
}

#[test]
fn update_tags_negative_payload_tag_count_panics_like_java() {
    // One registry key, then a NetworkPayload whose tag count is negative: the
    // inner `readMap` hits the same guava checkNonnegative.
    let mut out = buf();
    out.write_var_int(1);
    out.write_utf("minecraft:block");
    out.write_var_int(-1);
    let msg = panic_message(|| {
        let _ = ClientboundUpdateTagsPacket::stream_codec().decode(&mut out);
    });
    assert_eq!(msg, "expectedSize cannot be negative but was: -1");
}

#[test]
fn update_tags_hostile_registry_key_panics_like_java() {
    // `readRegistryKey` reads a raw identifier and calls `Identifier.parse`,
    // which throws (the codec boundary is not involved on this path).
    let mut out = buf();
    out.write_var_int(1);
    out.write_utf("minecraft:aA");
    assert!(
        catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = ClientboundUpdateTagsPacket::stream_codec().decode(&mut out);
        }))
        .is_err()
    );
}

#[test]
fn update_tags_truncated_registry_key_panics_like_java() {
    // A length prefix that runs off the buffer: the raw `readUtf` path panics
    // with netty's IndexOutOfBounds-equivalent text (EOF on a scalar path).
    let mut input = buf();
    input.write_var_int(1);
    input.write_var_int(10);
    input.write_bytes(b"short"); // 5 bytes for a declared 10
    assert!(
        catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = ClientboundUpdateTagsPacket::stream_codec().decode(&mut input);
        }))
        .is_err()
    );
}

#[test]
fn network_payload_read_write_round_trips_and_guava_negative() {
    // NetworkPayload alone: encode a two-tag payload and decode it back.
    let mut tags = std::collections::HashMap::new();
    tags.insert(Identifier::with_default_namespace("air"), vec![0, 794, 795]);
    tags.insert(
        Identifier::with_default_namespace("acacia_logs"),
        vec![53, 64, 75, 83],
    );
    let payload = NetworkPayload::new(tags);
    let mut out = buf();
    payload.write(&mut out);
    let mut input = FriendlyByteBuf::new(out.into_inner());
    let decoded = NetworkPayload::read(&mut input);
    assert_eq!(input.readable_bytes(), 0);
    assert_eq!(decoded, payload);

    // The guava negative-size panic on the raw read path.
    let mut out = buf();
    out.write_var_int(-1);
    let msg = panic_message(|| {
        let _ = NetworkPayload::read(&mut out);
    });
    assert_eq!(msg, "expectedSize cannot be negative but was: -1");
}

// ---------------------------------------------------------------------------
// Registration coverage: the real bodies registered in ConfigurationProtocols
// order, pinned against the generated vanilla ids.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum ConfigServerbound {
    Finish(ServerboundFinishConfigurationPacket),
    SelectKnownPacks(ServerboundSelectKnownPacks),
}

impl Packet for ConfigServerbound {
    fn packet_type(&self) -> PacketType {
        match self {
            ConfigServerbound::Finish(_) => config_packet_types::serverbound_finish_configuration(),
            ConfigServerbound::SelectKnownPacks(_) => {
                config_packet_types::serverbound_select_known_packs()
            }
        }
    }
    fn is_terminal(&self) -> bool {
        matches!(self, ConfigServerbound::Finish(_))
    }
}

impl fmt::Display for ConfigServerbound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.packet_type())
    }
}

fn serverbound_finish_codec() -> StreamCodec<FriendlyByteBuf, ConfigServerbound> {
    map(
        serverbound_finish_configuration::stream_codec(),
        |v: &ServerboundFinishConfigurationPacket| ConfigServerbound::Finish(*v),
        |p: &ConfigServerbound| match p {
            ConfigServerbound::Finish(v) => *v,
            _ => unreachable!("serverbound_finish_codec only handles Finish"),
        },
    )
}

fn serverbound_select_known_packs_codec() -> StreamCodec<FriendlyByteBuf, ConfigServerbound> {
    map(
        ServerboundSelectKnownPacks::stream_codec(),
        |v: &ServerboundSelectKnownPacks| ConfigServerbound::SelectKnownPacks(v.clone()),
        |p: &ConfigServerbound| match p {
            ConfigServerbound::SelectKnownPacks(v) => v.clone(),
            _ => unreachable!("serverbound_select_known_packs_codec only handles SelectKnownPacks"),
        },
    )
}

#[test]
fn configuration_serverbound_registration_matches_generated_ids() {
    // ConfigurationProtocols.SERVERBOUND_TEMPLATE registers finish_configuration
    // at index 3 and select_known_packs at index 7. This slice registers them
    // first/second, so the protocol-local ids are 0/1; the generated table pins
    // the vanilla addPacket-order ids.
    let template =
        serverbound_protocol::<ConfigServerbound>(ConnectionProtocol::Configuration, |b| {
            b.add_packet(
                config_packet_types::serverbound_finish_configuration(),
                serverbound_finish_codec(),
            )
            .add_packet(
                config_packet_types::serverbound_select_known_packs(),
                serverbound_select_known_packs_codec(),
            );
        });
    assert_eq!(
        template.details().list_packets(),
        &[
            (config_packet_types::serverbound_finish_configuration(), 0),
            (config_packet_types::serverbound_select_known_packs(), 1),
        ]
    );
    assert_eq!(
        packets::configuration::serverbound::PacketType::FinishConfiguration.id(),
        3
    );
    assert_eq!(
        packets::configuration::serverbound::PacketType::SelectKnownPacks.id(),
        7
    );
    assert_eq!(
        packets::configuration::serverbound::PacketType::SelectKnownPacks.name(),
        "minecraft:select_known_packs"
    );

    // The bound codec dispatches the real bodies.
    let info = template.bind();
    let mut out = buf();
    info.codec()
        .encode(
            &mut out,
            &ConfigServerbound::SelectKnownPacks(ServerboundSelectKnownPacks::new(vec![])),
        )
        .unwrap();
    // Local id 1, then the empty `list(64)` count.
    let wire = written(out);
    assert_eq!(wire, vec![1, 0]);
    let mut input = FriendlyByteBuf::new(BytesMut::from(wire.as_slice()));
    assert_eq!(
        info.codec().decode(&mut input).unwrap(),
        ConfigServerbound::SelectKnownPacks(ServerboundSelectKnownPacks::new(vec![]))
    );
    assert_eq!(input.readable_bytes(), 0);

    // finish_configuration is terminal: switching the client to game.
    assert!(ConfigServerbound::Finish(ServerboundFinishConfigurationPacket).is_terminal());
}

#[derive(Debug, Clone, PartialEq)]
enum ConfigClientbound {
    Finish(ClientboundFinishConfigurationPacket),
    RegistryData(ClientboundRegistryDataPacket),
    UpdateEnabledFeatures(ClientboundUpdateEnabledFeaturesPacket),
    UpdateTags(ClientboundUpdateTagsPacket),
    SelectKnownPacks(ClientboundSelectKnownPacks),
}

impl Packet for ConfigClientbound {
    fn packet_type(&self) -> PacketType {
        match self {
            ConfigClientbound::Finish(_) => config_packet_types::clientbound_finish_configuration(),
            ConfigClientbound::RegistryData(_) => config_packet_types::clientbound_registry_data(),
            ConfigClientbound::UpdateEnabledFeatures(_) => {
                config_packet_types::clientbound_update_enabled_features()
            }
            ConfigClientbound::UpdateTags(_) => clientbound_update_tags(),
            ConfigClientbound::SelectKnownPacks(_) => {
                config_packet_types::clientbound_select_known_packs()
            }
        }
    }
    fn is_terminal(&self) -> bool {
        matches!(self, ConfigClientbound::Finish(_))
    }
}

impl fmt::Display for ConfigClientbound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.packet_type())
    }
}

fn clientbound_finish_codec() -> StreamCodec<FriendlyByteBuf, ConfigClientbound> {
    map(
        clientbound_finish_configuration::stream_codec(),
        |v: &ClientboundFinishConfigurationPacket| ConfigClientbound::Finish(*v),
        |p: &ConfigClientbound| match p {
            ConfigClientbound::Finish(v) => *v,
            _ => unreachable!("clientbound_finish_codec only handles Finish"),
        },
    )
}

fn clientbound_registry_data_codec() -> StreamCodec<FriendlyByteBuf, ConfigClientbound> {
    map(
        ClientboundRegistryDataPacket::stream_codec(),
        |v: &ClientboundRegistryDataPacket| ConfigClientbound::RegistryData(v.clone()),
        |p: &ConfigClientbound| match p {
            ConfigClientbound::RegistryData(v) => v.clone(),
            _ => unreachable!("clientbound_registry_data_codec only handles RegistryData"),
        },
    )
}

fn clientbound_update_enabled_features_codec() -> StreamCodec<FriendlyByteBuf, ConfigClientbound> {
    map(
        ClientboundUpdateEnabledFeaturesPacket::stream_codec(),
        |v: &ClientboundUpdateEnabledFeaturesPacket| {
            ConfigClientbound::UpdateEnabledFeatures(v.clone())
        },
        |p: &ConfigClientbound| match p {
            ConfigClientbound::UpdateEnabledFeatures(v) => v.clone(),
            _ => unreachable!(
                "clientbound_update_enabled_features_codec only handles UpdateEnabledFeatures"
            ),
        },
    )
}

fn clientbound_update_tags_codec() -> StreamCodec<FriendlyByteBuf, ConfigClientbound> {
    map(
        ClientboundUpdateTagsPacket::stream_codec(),
        |v: &ClientboundUpdateTagsPacket| ConfigClientbound::UpdateTags(v.clone()),
        |p: &ConfigClientbound| match p {
            ConfigClientbound::UpdateTags(v) => v.clone(),
            _ => unreachable!("clientbound_update_tags_codec only handles UpdateTags"),
        },
    )
}

fn clientbound_select_known_packs_codec() -> StreamCodec<FriendlyByteBuf, ConfigClientbound> {
    map(
        ClientboundSelectKnownPacks::stream_codec(),
        |v: &ClientboundSelectKnownPacks| ConfigClientbound::SelectKnownPacks(v.clone()),
        |p: &ConfigClientbound| match p {
            ConfigClientbound::SelectKnownPacks(v) => v.clone(),
            _ => unreachable!("clientbound_select_known_packs_codec only handles SelectKnownPacks"),
        },
    )
}

#[test]
fn configuration_clientbound_registration_matches_generated_ids() {
    // ConfigurationProtocols.CLIENTBOUND_TEMPLATE pins finish_configuration 3,
    // registry_data 7, update_enabled_features 12, update_tags 13,
    // select_known_packs 14. Registering the ported slice in that order gives
    // local ids 0..4; the generated table is the vanilla oracle.
    let template =
        clientbound_protocol::<ConfigClientbound>(ConnectionProtocol::Configuration, |b| {
            b.add_packet(
                config_packet_types::clientbound_finish_configuration(),
                clientbound_finish_codec(),
            )
            .add_packet(
                config_packet_types::clientbound_registry_data(),
                clientbound_registry_data_codec(),
            )
            .add_packet(
                config_packet_types::clientbound_update_enabled_features(),
                clientbound_update_enabled_features_codec(),
            )
            .add_packet(clientbound_update_tags(), clientbound_update_tags_codec())
            .add_packet(
                config_packet_types::clientbound_select_known_packs(),
                clientbound_select_known_packs_codec(),
            );
        });
    assert_eq!(
        template.details().list_packets(),
        &[
            (config_packet_types::clientbound_finish_configuration(), 0),
            (config_packet_types::clientbound_registry_data(), 1),
            (
                config_packet_types::clientbound_update_enabled_features(),
                2
            ),
            (clientbound_update_tags(), 3),
            (config_packet_types::clientbound_select_known_packs(), 4),
        ]
    );
    assert_eq!(
        packets::configuration::clientbound::PacketType::FinishConfiguration.id(),
        3
    );
    assert_eq!(
        packets::configuration::clientbound::PacketType::RegistryData.id(),
        7
    );
    assert_eq!(
        packets::configuration::clientbound::PacketType::UpdateEnabledFeatures.id(),
        12
    );
    assert_eq!(
        packets::configuration::clientbound::PacketType::UpdateTags.id(),
        13
    );
    assert_eq!(
        packets::configuration::clientbound::PacketType::SelectKnownPacks.id(),
        14
    );

    // A registry_data body round-trips through the id-dispatch machinery: the
    // wire is [id 1, fixture body...] (registry_data is registered at local id 1).
    let info = template.bind();
    let world_clock = fixture_hex("config_registry_data_world_clock");
    let mut wire = vec![0x01];
    wire.extend_from_slice(&world_clock);
    let mut input = FriendlyByteBuf::new(BytesMut::from(wire.as_slice()));
    let packet = info.codec().decode(&mut input).unwrap();
    // The dispatch reads the id first.
    assert_eq!(input.readable_bytes(), 0);
    assert_eq!(
        packet.packet_type(),
        config_packet_types::clientbound_registry_data()
    );
    let mut out = buf();
    info.codec().encode(&mut out, &packet).unwrap();
    assert_eq!(out.into_inner().to_vec(), wire);
    let mut input = FriendlyByteBuf::new(BytesMut::from(wire.as_slice()));
    assert_eq!(info.codec().decode(&mut input).unwrap(), packet);
    assert_eq!(input.readable_bytes(), 0);

    // finish_configuration is terminal.
    assert!(ConfigClientbound::Finish(ClientboundFinishConfigurationPacket).is_terminal());
}

// ---------------------------------------------------------------------------
// Controlled semantic mutations: the do-not-weaken counterfactual checks.
// ---------------------------------------------------------------------------

#[test]
fn mutation_select_known_packs_version_changes_wire_bytes() {
    // The capture pins version "26.2"; a version mutation must change the wire.
    let captured = fixture_hex("config_clientbound_select_known_packs");
    let mut out = buf();
    ClientboundSelectKnownPacks::stream_codec()
        .encode(
            &mut out,
            &ClientboundSelectKnownPacks::new(vec![KnownPack::new(
                "minecraft".to_string(),
                "core".to_string(),
                "27.0".to_string(),
            )]),
        )
        .unwrap();
    assert_ne!(written(out), captured);
}

#[test]
fn mutation_update_enabled_features_different_flag_changes_wire() {
    // The capture's single flag is "vanilla"; a different single flag must not
    // reproduce the golden body.
    let captured = fixture_hex("config_update_enabled_features");
    let mut features = std::collections::HashSet::new();
    features.insert(Identifier::with_default_namespace("minecraft"));
    let mut out = buf();
    ClientboundUpdateEnabledFeaturesPacket::stream_codec()
        .encode(
            &mut out,
            &ClientboundUpdateEnabledFeaturesPacket::new(features),
        )
        .unwrap();
    assert_ne!(written(out), captured);
}

#[test]
fn mutation_registry_data_different_registry_changes_wire() {
    // The world_clock fixture pins registry `minecraft:world_clock`; a mutated
    // registry key must change the first field on the wire.
    let captured = fixture_hex("config_registry_data_world_clock");
    let mut input = FriendlyByteBuf::new(BytesMut::from(captured.as_slice()));
    let decoded = ClientboundRegistryDataPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    let mut out = buf();
    ClientboundRegistryDataPacket::stream_codec()
        .encode(
            &mut out,
            &ClientboundRegistryDataPacket::new(
                erased("minecraft:test_environment"),
                decoded.entries().to_vec(),
            ),
        )
        .unwrap();
    assert_ne!(written(out), captured);
}

/// The configuration-phase flag / flow facts every body carries.
#[test]
fn configuration_packet_flags_match_paper() {
    assert_eq!(
        ClientboundFinishConfigurationPacket.packet_type().flow(),
        PacketFlow::Clientbound
    );
    assert_eq!(
        ServerboundFinishConfigurationPacket.packet_type().flow(),
        PacketFlow::Serverbound
    );
    assert_eq!(
        ClientboundSelectKnownPacks::new(vec![])
            .packet_type()
            .flow(),
        PacketFlow::Clientbound
    );
    assert_eq!(
        ServerboundSelectKnownPacks::new(vec![])
            .packet_type()
            .flow(),
        PacketFlow::Serverbound
    );
    let mut features = std::collections::HashSet::new();
    features.insert(Identifier::with_default_namespace("vanilla"));
    assert_eq!(
        ClientboundUpdateEnabledFeaturesPacket::new(features)
            .packet_type()
            .flow(),
        PacketFlow::Clientbound
    );
}
