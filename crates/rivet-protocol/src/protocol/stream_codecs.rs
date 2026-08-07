//! Shared stream codecs for the packet-body units (issue #86).
//!
//! [`identifier_codec`] is `Identifier.STREAM_CODEC` — `ByteBufCodecs.STRING_UTF8
//! .map(Identifier::parse, Identifier::toString)` (`working/Paper/.../resources/
//! Identifier.java:20`). The codec lives in `rivet-protocol` (not `rivet-registry`)
//! per OWNERSHIP.md — `Identifier` is a `rivet-registry` value type and its
//! `STREAM_CODEC` is a wire concern; the `identifier.rs` module doc defers it
//! here.
//!
//! Both codec sides go through the `Err`-returning [`crate::codec::byte_buf_codecs::string_utf8`]
//! boundary (Java's `STRING_UTF8`), and the decode side converts the
//! `IdentifierException` from `Identifier.parse` into a `CodecError` — the raw
//! `FriendlyByteBuf::read_identifier` helper still panics, so the codec never
//! reaches it. This matters for the serverbound bodies that read a hostile
//! identifier off the wire (`serverbound_custom_payload`,
//! `serverbound_custom_click_action`, `serverbound_cookie_response`): a
//! malformed id closes just that connection (Java `IdentifierException`), it
//! does not abort the process.
//!
//! Deferred with the registry-wired units: `Identifier.STREAM_CODEC` variants
//! over `RegistryFriendlyByteBuf` (none needed by the #86 bodies). The
//! `ResourceKey`/registry-key codec is here ([`registry_key_codec`], the erased
//! `ResourceKey<? extends Registry<?>>` wire form) — it serves both
//! `ClientboundRegistryDataPacket` and `ClientboundUpdateTagsPacket`.

use crate::codec::byte_buf_codecs;
use crate::codec::{CodecError, StreamCodec, StreamDecoder, StreamEncoder, composite_3, map, of};
use crate::friendly_byte_buf::{FriendlyByteBuf, MAX_STRING_LENGTH};
use rivet_registry::{Identifier, Registry, ResourceKey};
use rivet_util::KnownPack;
use rivet_util::mth::Uuid;

/// `Identifier.STREAM_CODEC` — a `MAX_STRING_LENGTH`-bounded UTF string
/// parsed through `Identifier.parse` on decode and rendered by
/// `Identifier.toString()` on encode.
pub fn identifier_codec() -> StreamCodec<FriendlyByteBuf, Identifier> {
    let string_codec = byte_buf_codecs::string_utf8(MAX_STRING_LENGTH);
    let encode_codec = string_codec.clone();
    of(
        move |output: &mut FriendlyByteBuf, value: &Identifier| {
            // Java `Identifier.toString()` then `STRING_UTF8.encode`.
            encode_codec.encode(output, &value.to_string())
        },
        move |input: &mut FriendlyByteBuf| {
            // Java `STRING_UTF8.decode` then `Identifier.parse` (which throws an
            // `IdentifierException`). The codec boundary surfaces the escaped
            // message as `Err` instead of panicking.
            let s = string_codec.decode(input)?;
            Identifier::by_separator_result(&s, ':')
                .map_err(|e| CodecError::new(e.message().to_string()))
        },
    )
}

/// `UUIDUtil.STREAM_CODEC` — a 16-byte UUID: two big-endian longs (`most`,
/// `least`), exactly `FriendlyByteBuf.readUUID`/`writeUUID`.
///
/// Java: `UUIDUtil.STREAM_CODEC` in `working/Paper` delegates straight to
/// `FriendlyByteBuf.readUUID`/`writeUUID`. Used by
/// `ClientboundLoginFinishedPacket` (the `sessionId` field after the profile).
pub fn uuid_codec() -> StreamCodec<FriendlyByteBuf, Uuid> {
    of(
        |output: &mut FriendlyByteBuf, value: &Uuid| {
            output.write_uuid(*value);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| Ok(input.read_uuid()),
    )
}

/// `KnownPack.STREAM_CODEC` — three `STRING_UTF8` fields in record order
/// (`namespace`, `id`, `version`), exactly `StreamCodec.composite(STRING_UTF8,
/// ::namespace, STRING_UTF8, ::id, STRING_UTF8, ::version, KnownPack::new)`.
///
/// The value type lives in `rivet-util` (the lowest dependency shared by the
/// protocol and server crates, issue #109); this is the wire codec, owned here
/// per the OWNERSHIP line that keeps `Identifier.STREAM_CODEC` in
/// `rivet-protocol`.
pub fn known_pack_stream_codec() -> StreamCodec<FriendlyByteBuf, KnownPack> {
    let string = byte_buf_codecs::string_utf8(MAX_STRING_LENGTH);
    let encode_string = string.clone();
    composite_3(
        string,
        |p: &KnownPack| p.namespace().to_string(),
        encode_string,
        |p: &KnownPack| p.id().to_string(),
        byte_buf_codecs::string_utf8(MAX_STRING_LENGTH),
        |p: &KnownPack| p.version().to_string(),
        KnownPack::new,
    )
}

/// `ClientboundRegistryDataPacket.REGISTRY_KEY_STREAM_CODEC` —
/// `Identifier.STREAM_CODEC.map(ResourceKey::createRegistryKey, ResourceKey::identifier)`
/// — the erased `ResourceKey<? extends Registry<?>>` wire form (an identifier
/// string).
///
/// Erased over `ResourceKey<Registry<()>>`, the rustc stand-in for Java's
/// unbounded-wildcard registry key (the erased boundary pattern of
/// `RegistryAccess`/`AnyRegistry`, OWNERSHIP.md §Registries). Java's
/// `ResourceKey.createRegistryKey`/`ResourceKey.identifier` are generic in the
/// erased element type; the identifier itself carries no element-type
/// information on the wire.
pub fn registry_key_codec() -> StreamCodec<FriendlyByteBuf, ResourceKey<Registry<()>>> {
    map(
        identifier_codec(),
        |id: &Identifier| ResourceKey::create_registry_key(id.clone()),
        |key: &ResourceKey<Registry<()>>| key.identifier().clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::StreamDecoder;
    use crate::codec::StreamEncoder;
    use bytes::BytesMut;

    #[test]
    fn known_pack_round_trips_and_golden_wire_bytes() {
        let codec = known_pack_stream_codec();
        let pack = KnownPack::new(
            "minecraft".to_string(),
            "core".to_string(),
            "26.2".to_string(),
        );
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        codec.encode(&mut out, &pack).unwrap();
        // Three STRING_UTF8 fields: length 9 "minecraft", length 4 "core",
        // length 4 "26.2".
        assert_eq!(
            out.into_inner().to_vec(),
            b"\x09minecraft\x04core\x0426.2".to_vec()
        );
        let mut input = FriendlyByteBuf::new(BytesMut::from(
            b"\x09minecraft\x04core\x0426.2".to_vec().as_slice(),
        ));
        assert_eq!(codec.decode(&mut input).unwrap(), pack);
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn registry_key_round_trips_erased() {
        let codec = registry_key_codec();
        // `create_registry_key` returns `ResourceKey<Registry<T>>` for the
        // impl's `T`; the codec's erased value type is `ResourceKey<Registry<()>>`,
        // so the turbofish is `::<()>`. (`ResourceKey::<Registry<()>>::create_registry_key`
        // would set `T = Registry<()>` and produce a double-wrapped
        // `ResourceKey<Registry<Registry<()>>>`, which the codec does not handle.)
        let key = ResourceKey::<()>::create_registry_key(Identifier::with_default_namespace(
            "worldgen/biome",
        ));
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        codec.encode(&mut out, &key).unwrap();
        // The wire is just the identifier string (24 chars: "minecraft:worldgen/biome").
        assert_eq!(
            out.into_inner().to_vec(),
            b"\x18minecraft:worldgen/biome".to_vec()
        );
        let mut input = FriendlyByteBuf::new(BytesMut::from(
            b"\x18minecraft:worldgen/biome".to_vec().as_slice(),
        ));
        assert_eq!(codec.decode(&mut input).unwrap(), key);
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn identifier_round_trips_via_bounded_utf() {
        let codec = identifier_codec();
        let id = Identifier::with_default_namespace("brand");
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        codec.encode(&mut out, &id).unwrap();
        // `STRING_UTF8`: varint length 15, then "minecraft:brand".
        assert_eq!(out.into_inner().to_vec(), b"\x0Fminecraft:brand".to_vec());
        let mut input =
            FriendlyByteBuf::new(BytesMut::from(b"\x0Fminecraft:brand".to_vec().as_slice()));
        assert_eq!(codec.decode(&mut input).unwrap(), id);
    }

    #[test]
    fn malformed_identifier_errors_not_panics() {
        // A hostile `minecraft:aA` key (uppercase path char) is a Java
        // `IdentifierException`; the codec returns `Err` instead of panicking.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        out.write_utf("minecraft:aA");
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let err = identifier_codec().decode(&mut input).unwrap_err();
        assert_eq!(
            err.message,
            "Non [a-z0-9/._-] character in path of location: minecraft:aA"
        );
    }
}
