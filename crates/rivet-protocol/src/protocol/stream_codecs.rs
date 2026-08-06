//! Shared stream codecs for the packet-body units (issue #86).
//!
//! [`identifier_codec`] is `Identifier.STREAM_CODEC` — `ByteBufCodecs.STRING_UTF8
//! .map(Identifier::parse, Identifier::toString)` (`working/Paper/.../resources/
//! Identifier.java:20`). The codec lives in `rivet-protocol` (not `rivet-registry`)
//! per OWNERSHIP.md — `Identifier` is a `rivet-registry` value type and its
//! `STREAM_CODEC` is a wire concern; the `identifier.rs` module doc defers it
//! here. It is built from the `FriendlyByteBuf` identifier helpers (which are
//! registry-independent), so the common/cookie/ping packet bodies can share it.
//!
//! Deferred with the registry-wired units: `Identifier.STREAM_CODEC` variants
//! over `RegistryFriendlyByteBuf` (none needed by the #86 bodies), and
//! `ResourceKey`/registry-key codecs (login/CommonPlayerSpawnInfo/update_tags).

use crate::codec::{StreamCodec, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use rivet_registry::Identifier;

/// `Identifier.STREAM_CODEC` — a `MAX_STRING_LENGTH`-bounded UTF string
/// parsed through `Identifier.parse` on decode and rendered by
/// `Identifier.toString()` on encode.
pub fn identifier_codec() -> StreamCodec<FriendlyByteBuf, Identifier> {
    of(
        |output: &mut FriendlyByteBuf, value: &Identifier| {
            output.write_identifier(value);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| Ok(input.read_identifier()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::StreamDecoder;
    use crate::codec::StreamEncoder;
    use bytes::BytesMut;

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
}
