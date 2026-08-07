//! Java-grounded tests for the issue #90 clientbound entity-pairing packets
//! (`crates/rivet-protocol/src/game/` + `crates/rivet-protocol/src/syncher/`).
//!
//! The three packets present in the #153 join fixture (protocol 776, Paper
//! `26.2-DEV-main@0a99345`, seed 42, superflat, offline bot) are byte-exact
//! against the captured golden bodies:
//! - `entity_event` (cb 34): `0000000100` — entity id 1 as a 4-byte int, event 0.
//! - `set_entity_data` (cb 99): `01090341a0000010007fff` — entity id 1, items
//!   `{accessor 9, FLOAT, 20.0f}` and `{accessor 16, BYTE, 127}`, then the `0xFF`
//!   terminator.
//! - `update_attributes` (cb 131): `0103084012…1a3fb99999a000000000` — entity
//!   id 1, 3 snapshots (attribute holder ids 8/13/26, bases 4.5/3.0/canonical
//!   0.1, zero modifiers).
//!
//! `set_entity_data` and `update_attributes` decode against a real ATTRIBUTE
//! registry (the `holderRegistry` codec resolves ids through the access);
//! `entity_event` is over the plain `FriendlyByteBuf`.
//!
//! Mutation tests pin Java's behavior: the `0xFF` terminator boundary (id 255
//! is an item id, not the terminator), `"Unknown serializer type {n}"` on an
//! unregistered serializer id, the blocked serializer panic, and the
//! `list(128)` snapshot ceiling.
//!
//! Gated on the `packets` feature (the `game`/`syncher` modules live behind it).

use bytes::BytesMut;
use rivet_protocol::codec::{StreamDecoder, StreamEncoder};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::game::clientbound_entity_event_packet::{
    ClientboundEntityEventPacket, entity_event_codec,
};
use rivet_protocol::game::clientbound_set_entity_data_packet::{
    ClientboundSetEntityDataPacket, set_entity_data_codec,
};
use rivet_protocol::game::clientbound_update_attributes_packet::update_attributes_codec;
use rivet_protocol::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_protocol::syncher::{DataValue, SerializedValue, SerializerId};
use rivet_registry::registries::{ATTRIBUTE, Attribute};
use rivet_registry::{Identifier, RegistrationInfo, RegistryAccess, RegistryBuilder, ResourceKey};
use std::panic::catch_unwind;
use std::sync::Arc;

/// Hex body -> `Vec<u8>`.
fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// A real ATTRIBUTE registry of 40 placeholder entries, wrapped in an access.
/// The `holderRegistry(ATTRIBUTE)` codec resolves ids against `size()` (strict
/// bounds), so any registry with `size() > 26` proves the captured ids 8/13/26.
fn attribute_access() -> RegistryAccess {
    let mut builder = RegistryBuilder::new(&*ATTRIBUTE);
    for i in 0..40 {
        let key = ResourceKey::create(
            &*ATTRIBUTE,
            Identifier::with_default_namespace(&format!("attr_{i}")),
        );
        builder.register(&key, Arc::new(Attribute), RegistrationInfo::BUILT_IN);
    }
    let registry = builder.freeze();
    RegistryAccess::from_single_registry(ATTRIBUTE.clone(), registry)
}

fn registry_buf(access: &RegistryAccess) -> RegistryFriendlyByteBuf {
    RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone())
}

fn plain_buf() -> FriendlyByteBuf {
    FriendlyByteBuf::new(BytesMut::new())
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
// entity_event (cb 34): entity id as int32, event id as byte.
// ---------------------------------------------------------------------------

#[test]
fn entity_event_golden_round_trip() {
    // The entity_event codec is over FriendlyByteBuf (Java uses the plain buf).
    let mut input = FriendlyByteBuf::new(BytesMut::from(hex("0000000100").as_slice()));
    let decoded = entity_event_codec().decode(&mut input).unwrap();
    assert_eq!(decoded, ClientboundEntityEventPacket::new(1, 0));
    assert_eq!(input.readable_bytes(), 0);

    // Re-encode byte-identically.
    let mut out = plain_buf();
    entity_event_codec().encode(&mut out, &decoded).unwrap();
    assert_eq!(out.into_inner().to_vec(), hex("0000000100"));
}

#[test]
fn entity_event_wire_is_int32_not_varint() {
    // A naive VarInt port would emit 1 byte for id 1; Java writes 4 bytes.
    let mut out = plain_buf();
    entity_event_codec()
        .encode(&mut out, &ClientboundEntityEventPacket::new(1, 0))
        .unwrap();
    assert_eq!(out.into_inner().to_vec(), hex("0000000100"));
}

// ---------------------------------------------------------------------------
// set_entity_data (cb 99): varint id + sentinel-terminated DataValue list.
// ---------------------------------------------------------------------------

#[test]
fn set_entity_data_golden_round_trip() {
    let access = attribute_access();
    let mut input = RegistryFriendlyByteBuf::new(
        BytesMut::from(hex("01090341a0000010007fff").as_slice()),
        access.clone(),
    );
    let decoded = set_entity_data_codec().decode(&mut input).unwrap();
    assert_eq!(input.readable_bytes(), 0);
    assert_eq!(decoded.id, 1);
    assert_eq!(
        decoded.packed_items,
        vec![
            DataValue {
                id: 9,
                serializer: SerializerId::Float,
                value: SerializedValue::Float(20.0),
            },
            DataValue {
                id: 16,
                serializer: SerializerId::Byte,
                value: SerializedValue::Byte(127),
            },
        ]
    );

    // Re-encode byte-identically.
    let mut out = registry_buf(&access);
    set_entity_data_codec().encode(&mut out, &decoded).unwrap();
    assert_eq!(out.into_inner().to_vec(), hex("01090341a0000010007fff"));
}

#[test]
fn set_entity_data_empty_list_still_writes_terminator() {
    let access = attribute_access();
    let value = ClientboundSetEntityDataPacket::new(1, vec![]);
    let mut out = registry_buf(&access);
    set_entity_data_codec().encode(&mut out, &value).unwrap();
    // VarInt id 1 then the 0xFF terminator — written even when empty.
    assert_eq!(out.into_inner().to_vec(), vec![0x01, 0xFF]);

    let mut input =
        RegistryFriendlyByteBuf::new(BytesMut::from(vec![0x01, 0xFF].as_slice()), access.clone());
    let decoded = set_entity_data_codec().decode(&mut input).unwrap();
    assert_eq!(decoded.packed_items, vec![]);
    assert_eq!(input.readable_bytes(), 0);
}

#[test]
fn set_entity_data_id_254_is_an_item_id_only_255_terminates() {
    let access = attribute_access();
    // Accessor id 254 (the MAX_ID_VALUE) is a legal item id; the terminator is
    // exactly 255. Encode an item with id 254 -> byte 0xFE, then the serializer
    // varint + payload, then 0xFF.
    let value = ClientboundSetEntityDataPacket::new(
        1,
        vec![DataValue {
            id: 254,
            serializer: SerializerId::Byte,
            value: SerializedValue::Byte(5),
        }],
    );
    let mut out = registry_buf(&access);
    set_entity_data_codec().encode(&mut out, &value).unwrap();
    assert_eq!(
        out.into_inner().to_vec(),
        vec![0x01, 0xFE, 0x00, 0x05, 0xFF]
    );
}

#[test]
fn set_entity_data_id_255_consumed_as_terminator_leaves_trailing() {
    let access = attribute_access();
    // An item with accessor id 255: `writeByte(255)` is indistinguishable from
    // the terminator, so decode stops at the first 0xFF and the serializer id +
    // payload bytes trail as unread input — exactly Java's
    // `readUnsignedByte() != 255` loop.
    let value = ClientboundSetEntityDataPacket::new(
        1,
        vec![DataValue {
            id: 255,
            serializer: SerializerId::Byte,
            value: SerializedValue::Byte(5),
        }],
    );
    let mut out = registry_buf(&access);
    set_entity_data_codec().encode(&mut out, &value).unwrap();
    let bytes = out.into_inner().to_vec();
    // Item id 255 (0xFF), serializer id 0, value 5, then the terminator.
    assert_eq!(bytes, vec![0x01, 0xFF, 0x00, 0x05, 0xFF]);
    let mut input = RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), access.clone());
    let decoded = set_entity_data_codec().decode(&mut input).unwrap();
    assert_eq!(decoded.packed_items, vec![]);
    // The serializer id (0) + payload (5) + the packet's own terminator (0xFF)
    // were never consumed — decode stopped at the first 0xFF (the item id).
    assert_eq!(input.readable_bytes(), 3);
}

#[test]
fn set_entity_data_unknown_serializer_panics_like_java() {
    let access = attribute_access();
    // Body: id 1, accessor byte 9, serializer varint 999 (unregistered).
    let bytes = hex("0109e707");
    let mut input = RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), access);
    let msg = panic_message(|| {
        let _ = set_entity_data_codec().decode(&mut input);
    });
    assert_eq!(msg, "Unknown serializer type 999");
}

#[test]
fn set_entity_data_negative_serializer_panics_like_java() {
    let access = attribute_access();
    // Body: id 1, accessor byte 9, serializer varint -1 (5-byte varint).
    let bytes = hex("0109ffffffff0f");
    let mut input = RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), access);
    let msg = panic_message(|| {
        let _ = set_entity_data_codec().decode(&mut input);
    });
    assert_eq!(msg, "Unknown serializer type -1");
}

#[test]
fn set_entity_data_blocked_serializer_panics_loudly() {
    let access = attribute_access();
    // Body: id 1, accessor byte 9, serializer varint 7 (ITEM_STACK, blocked).
    let bytes = hex("010907ff");
    let mut input = RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), access);
    let msg = panic_message(|| {
        let _ = set_entity_data_codec().decode(&mut input);
    });
    assert!(
        msg.contains("blocked") && msg.contains("ItemStack"),
        "got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// update_attributes (cb 131): holderRegistry(ATTRIBUTE) + list(128) snapshots.
// ---------------------------------------------------------------------------

#[test]
fn update_attributes_golden_round_trip() {
    let access = attribute_access();
    let mut input = RegistryFriendlyByteBuf::new(
        BytesMut::from(
            hex("0103084012000000000000000d4008000000000000001a3fb99999a000000000").as_slice(),
        ),
        access.clone(),
    );
    let decoded = update_attributes_codec().decode(&mut input).unwrap();
    assert_eq!(input.readable_bytes(), 0);
    assert_eq!(decoded.entity_id, 1);
    assert_eq!(decoded.attributes.len(), 3);

    // Snapshot 1: holder id 8, base 4.5, 0 mods.
    assert_eq!(decoded.attributes[0].base, 4.5);
    assert!(decoded.attributes[0].modifiers.is_empty());
    // Snapshot 2: holder id 13, base 3.0.
    assert_eq!(decoded.attributes[1].base, 3.0);
    assert!(decoded.attributes[1].modifiers.is_empty());
    // Snapshot 3: holder id 26, base canonicalized 0.1 (the fixture normalizes
    // the player's synced base; the raw bits must round-trip exactly).
    //
    // The fixture registry is 40 synthetic `attr_{i}` placeholders, so the
    // holder ids are checked by id only — the real attribute names (order in
    // `Attributes`) are not asserted here.
    assert_eq!(
        decoded.attributes[2].base.to_bits(),
        0x3FB99999A0000000u64,
        "movement_speed base must round-trip the canonicalized wire bits"
    );
    assert!(decoded.attributes[2].modifiers.is_empty());

    // Re-encode byte-identically.
    let mut out = registry_buf(&access);
    update_attributes_codec()
        .encode(&mut out, &decoded)
        .unwrap();
    assert_eq!(
        out.into_inner().to_vec(),
        hex("0103084012000000000000000d4008000000000000001a3fb99999a000000000")
    );
}

#[test]
fn update_attributes_snapshot_over_128_errors() {
    let access = attribute_access();
    // Write entity id 1, then a snapshot count of 129 (list(128) ceiling).
    let mut out = registry_buf(&access);
    out.inner_mut().write_var_int(1);
    out.inner_mut().write_var_int(129);
    let err = update_attributes_codec().decode(&mut out).unwrap_err();
    assert_eq!(err.message, "129 elements exceeded max size of: 128");
}

#[test]
fn update_attributes_negative_snapshot_count_panics_like_java() {
    let access = attribute_access();
    // Entity id 1, then a snapshot count of -1 (5-byte VarInt). Java's
    // `readCount` upper-bounds only, then `new ArrayList<>(-1)` throws
    // `IllegalArgumentException("Illegal Capacity: -1")` — never a capacity
    // overflow.
    let mut out = registry_buf(&access);
    out.inner_mut().write_var_int(1);
    out.inner_mut().write_var_int(-1);
    let msg = panic_message(|| {
        let _ = update_attributes_codec().decode(&mut out);
    });
    assert_eq!(msg, "Illegal Capacity: -1");
}

#[test]
fn update_attributes_negative_modifier_count_panics_like_java() {
    let access = attribute_access();
    // Entity id 1, one snapshot (holder id 8, base 4.5), then a modifier count
    // of -1. The modifier list is `collection(ArrayList::new)` (unbounded), so
    // -1 passes `readCount` and hits `new ArrayList<>(-1)`.
    let mut out = registry_buf(&access);
    out.inner_mut().write_var_int(1);
    out.inner_mut().write_var_int(1);
    out.inner_mut().write_var_int(8);
    out.inner_mut().write_double(4.5);
    out.inner_mut().write_var_int(-1);
    let msg = panic_message(|| {
        let _ = update_attributes_codec().decode(&mut out);
    });
    assert_eq!(msg, "Illegal Capacity: -1");
}

#[test]
fn update_attributes_unknown_holder_id_panics() {
    let access = attribute_access();
    // Body: entity id 1, count 1, then holder id 999 (VarInt) — the strict
    // bounds check fires before any base double is read.
    let full = vec![0x01, 0x01, 0xE7, 0x07];
    let mut input = RegistryFriendlyByteBuf::new(BytesMut::from(full.as_slice()), access);
    let msg = panic_message(|| {
        let _ = update_attributes_codec().decode(&mut input);
    });
    assert_eq!(msg, "No value with id 999");
}

// ---------------------------------------------------------------------------
// The four #90 packets that never occur in the single-player fixture are STUBs:
// their codecs panic loudly with a blocked note (never silently invent values).
// ---------------------------------------------------------------------------

#[test]
fn absent_entity_packets_are_blocked_stubs() {
    use rivet_protocol::game::clientbound_add_entity_packet::add_entity_codec;
    use rivet_protocol::game::clientbound_remove_entities_packet::remove_entities_codec;
    use rivet_protocol::game::clientbound_set_passengers_packet::set_passengers_codec;
    use rivet_protocol::game::clientbound_teleport_entity_packet::teleport_entity_codec;

    let access = attribute_access();
    let mut b = plain_buf();
    let msg = panic_message(|| {
        let _ = remove_entities_codec().decode(&mut b);
    });
    assert!(msg.contains("blocked"), "got: {msg}");

    let mut b = plain_buf();
    let msg = panic_message(|| {
        let _ = set_passengers_codec().decode(&mut b);
    });
    assert!(msg.contains("blocked"), "got: {msg}");

    let mut b = plain_buf();
    let msg = panic_message(|| {
        let _ = teleport_entity_codec().decode(&mut b);
    });
    assert!(msg.contains("blocked"), "got: {msg}");

    let mut r = registry_buf(&access);
    let msg = panic_message(|| {
        let _ = add_entity_codec().decode(&mut r);
    });
    assert!(msg.contains("blocked"), "got: {msg}");
}

// ---------------------------------------------------------------------------
// Syncher value model: DataValue.write/read over the captured serializer ids.
// ---------------------------------------------------------------------------

#[test]
fn data_value_write_read_round_trips_captured_serializers() {
    let access = attribute_access();
    let value = DataValue {
        id: 9,
        serializer: SerializerId::Float,
        value: SerializedValue::Float(20.0),
    };
    let mut out = registry_buf(&access);
    value.write(&mut out);
    // writeByte(9) + writeVarInt(3) + FLOAT raw bits (20.0f).
    assert_eq!(out.as_slice().to_vec(), hex("090341a00000"));
    // `DataValue.read` takes the accessor id as a parameter — Java's packet
    // unpack loop reads that byte before calling read (as the set_entity_data
    // codec does), so the round trip must consume it first.
    let mut input = RegistryFriendlyByteBuf::new(out.into_inner(), access);
    let item_id = input.inner_mut().read_unsigned_byte();
    assert_eq!(item_id, 9);
    let got = DataValue::read(&mut input, item_id);
    assert_eq!(got, value);
    assert_eq!(input.readable_bytes(), 0);
}
