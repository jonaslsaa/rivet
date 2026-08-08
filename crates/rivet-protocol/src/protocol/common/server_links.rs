//! Port of the `net.minecraft.server.ServerLinks` wire surface (issue #207).
//!
//! Java: `ServerLinks.java` in `working/Paper` (MC 26.2). This module ports the
//! codecs `ClientboundServerLinksPacket` needs to put the server's links on the
//! wire: a varint count of "untrusted" entries, each a `KnownLinkType` *or* a
//! custom `Component` display name plus a `STRING_UTF8` link string.
//!
//! The server-side `ServerLinks`/`Entry` value type (`findKnownType`, `untrust`,
//! `displayName`) is not part of the wire and is not ported here; it belongs in
//! the downstream `rivet-server` crate when the server sends links.
//!
//! Wire formats (exact, the `ServerLinks.STREAM_CODEC` family):
//! - `KnownLinkType.STREAM_CODEC` — `ByteBufCodecs.idMapper(BY_ID, e -> e.id)`:
//!   a varint id; `BY_ID = ByIdMap.continuous(...)` maps an out-of-range id to
//!   `ZERO` (`BUG_REPORT`), never failing.
//! - `TYPE_STREAM_CODEC` — `ByteBufCodecs.either(KnownLinkType.STREAM_CODEC,
//!   ComponentSerialization.TRUSTED_CONTEXT_FREE_STREAM_CODEC)`: a boolean
//!   `true` for a known type (varint id), `false` for a custom component (the
//!   trusted NBT-tag wire form,
//!   [`crate::codec::byte_buf_codecs::trusted_component`]).
//! - `UntrustedEntry.STREAM_CODEC` — composite of `TYPE_STREAM_CODEC` then
//!   `STRING_UTF8` (max [`crate::friendly_byte_buf::MAX_STRING_LENGTH`]) for the
//!   link.
//! - `UNTRUSTED_LINKS_STREAM_CODEC` — the untrusted-entry codec applied to
//!   `ByteBufCodecs.list()` (varint count, then entries).
//!
//! Fidelity notes:
//! - `Entry.link` is a `java.net.URI` in Java. The wire form is the
//!   `STRING_UTF8` string, so the link is carried as a `String` (Java's URI
//!   string form); a full `java.net.URI` port is out of scope for this leaf.
//! - The issue body mentions "`KnownPack`" — that is stale from an older
//!   version; current 26.2 `ServerLinks` uses `KnownLinkType` + `Component`.

use crate::codec::byte_buf_codecs;
use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, apply, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use rivet_serialization::Either;
use rivet_text::Component;

/// `ServerLinks.KnownLinkType` — the `id`-indexed known server-link kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownLinkType {
    BugReport = 0,
    CommunityGuidelines = 1,
    Support = 2,
    Status = 3,
    Feedback = 4,
    Community = 5,
    Website = 6,
    Forums = 7,
    News = 8,
    Announcements = 9,
}

impl KnownLinkType {
    /// `KnownLinkType.STREAM_CODEC` — `ByteBufCodecs.idMapper(BY_ID, e -> e.id)`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, KnownLinkType> {
        byte_buf_codecs::id_mapper(KnownLinkType::by_id, KnownLinkType::id)
    }

    /// `KnownLinkType.id` — the enum's wire id.
    fn id(&self) -> i32 {
        *self as i32
    }

    /// `KnownLinkType.BY_ID.get` — `ByIdMap.continuous(id -> id, values(),
    /// OutOfBoundsStrategy.ZERO)`: an out-of-range id maps to `ZERO`
    /// (`BUG_REPORT`).
    fn by_id(id: i32) -> KnownLinkType {
        match id {
            0 => KnownLinkType::BugReport,
            1 => KnownLinkType::CommunityGuidelines,
            2 => KnownLinkType::Support,
            3 => KnownLinkType::Status,
            4 => KnownLinkType::Feedback,
            5 => KnownLinkType::Community,
            6 => KnownLinkType::Website,
            7 => KnownLinkType::Forums,
            8 => KnownLinkType::News,
            9 => KnownLinkType::Announcements,
            _ => KnownLinkType::BugReport,
        }
    }
}

/// `ServerLinks.UntrustedEntry` — the wire form of `ServerLinks.Entry`: the
/// same type/display-name `Either` plus a `STRING_UTF8` link string.
#[derive(Clone, Debug, PartialEq)]
pub struct UntrustedEntry {
    /// `UntrustedEntry.type`.
    r#type: Either<KnownLinkType, Component>,
    /// `UntrustedEntry.link`.
    link: String,
}

impl UntrustedEntry {
    /// `new UntrustedEntry(Either<KnownLinkType, Component>, String)`.
    pub fn new(r#type: Either<KnownLinkType, Component>, link: String) -> Self {
        UntrustedEntry { r#type, link }
    }

    /// `UntrustedEntry.type()`.
    pub fn r#type(&self) -> &Either<KnownLinkType, Component> {
        &self.r#type
    }

    /// `UntrustedEntry.link()`.
    pub fn link(&self) -> &str {
        &self.link
    }

    /// `UntrustedEntry.STREAM_CODEC` — `TYPE_STREAM_CODEC`, then `STRING_UTF8`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, UntrustedEntry> {
        let type_codec = type_stream_codec();
        let type_codec_decode = type_codec.clone();
        of(
            move |output: &mut FriendlyByteBuf, value: &UntrustedEntry| {
                type_codec.encode(output, &value.r#type)?;
                byte_buf_codecs::string().encode(output, &value.link)
            },
            move |input: &mut FriendlyByteBuf| {
                let r#type = type_codec_decode.decode(input)?;
                let link = byte_buf_codecs::string().decode(input)?;
                Ok(UntrustedEntry::new(r#type, link))
            },
        )
    }
}

/// `ServerLinks.TYPE_STREAM_CODEC` — `ByteBufCodecs.either(KnownLinkType
/// .STREAM_CODEC, ComponentSerialization.TRUSTED_CONTEXT_FREE_STREAM_CODEC)`.
fn type_stream_codec() -> StreamCodec<FriendlyByteBuf, Either<KnownLinkType, Component>> {
    byte_buf_codecs::either(
        KnownLinkType::stream_codec(),
        byte_buf_codecs::trusted_component(),
    )
}

/// `ServerLinks.UNTRUSTED_LINKS_STREAM_CODEC` — the untrusted-entry codec
/// applied to `ByteBufCodecs.list()` (varint count, then entries).
pub fn untrusted_links_stream_codec() -> StreamCodec<FriendlyByteBuf, Vec<UntrustedEntry>> {
    apply(UntrustedEntry::stream_codec(), byte_buf_codecs::list())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    fn buf() -> FriendlyByteBuf {
        FriendlyByteBuf::new(BytesMut::new())
    }

    fn written(b: FriendlyByteBuf) -> Vec<u8> {
        b.into_inner().to_vec()
    }

    #[test]
    fn known_link_type_ids_match_java() {
        // Java's enum constants + `id` (`getSerializedName` name strings are
        // not part of the wire and are not ported).
        let cases = [
            (KnownLinkType::BugReport, 0),
            (KnownLinkType::CommunityGuidelines, 1),
            (KnownLinkType::Support, 2),
            (KnownLinkType::Status, 3),
            (KnownLinkType::Feedback, 4),
            (KnownLinkType::Community, 5),
            (KnownLinkType::Website, 6),
            (KnownLinkType::Forums, 7),
            (KnownLinkType::News, 8),
            (KnownLinkType::Announcements, 9),
        ];
        for (variant, id) in cases {
            assert_eq!(variant.id(), id);
            assert_eq!(KnownLinkType::by_id(id), variant);
        }
    }

    #[test]
    fn known_link_type_by_id_out_of_range_maps_to_zero() {
        // `ByIdMap.continuous(..., OutOfBoundsStrategy.ZERO)` — both negative
        // and beyond-the-last ids resolve to `BUG_REPORT` (id 0).
        assert_eq!(KnownLinkType::by_id(-1), KnownLinkType::BugReport);
        assert_eq!(KnownLinkType::by_id(10), KnownLinkType::BugReport);
        assert_eq!(KnownLinkType::by_id(i32::MAX), KnownLinkType::BugReport);
        assert_eq!(KnownLinkType::by_id(i32::MIN), KnownLinkType::BugReport);
    }

    #[test]
    fn known_link_type_wire_form_is_varint_id() {
        let mut out = buf();
        KnownLinkType::stream_codec()
            .encode(&mut out, &KnownLinkType::Status)
            .unwrap();
        // `Status` id 3 -> varint 3.
        assert_eq!(written(out), vec![3]);
    }

    #[test]
    fn known_link_type_decode_out_of_range_never_fails() {
        // The id-mapper never errors; an out-of-range id decodes to BUG_REPORT.
        let mut input = FriendlyByteBuf::new(BytesMut::from(vec![99u8].as_slice()));
        assert_eq!(
            KnownLinkType::stream_codec().decode(&mut input).unwrap(),
            KnownLinkType::BugReport
        );
    }

    #[test]
    fn type_stream_codec_known_type_wire_form() {
        // `either(knownType, component)` — boolean true, then the varint id.
        let value = Either::<KnownLinkType, Component>::left(KnownLinkType::Website);
        let mut out = buf();
        type_stream_codec().encode(&mut out, &value).unwrap();
        assert_eq!(written(out), vec![1, 6]); // true, Website id 6
    }

    #[test]
    fn type_stream_codec_custom_component_round_trips() {
        // A plain-text custom display name encodes through the trusted
        // component codec: the string-either branch of `ComponentSerialization`
        // produces an NBT `StringTag`, written by the trusted tag codec.
        let value = Either::<KnownLinkType, Component>::right(Component::literal("Rivet"));
        let mut out = buf();
        type_stream_codec().encode(&mut out, &value).unwrap();
        let bytes = written(out);
        // boolean false, then an NBT tag: type byte 8 (String) + payload.
        assert_eq!(bytes[0], 0);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(type_stream_codec().decode(&mut input).unwrap(), value);
    }

    #[test]
    fn untrusted_entry_wire_form() {
        // A known-type entry: boolean true, varint id 3, then STRING_UTF8.
        let entry = UntrustedEntry::new(
            Either::left(KnownLinkType::Status),
            "https://status.rivet.test".to_string(),
        );
        let mut out = buf();
        UntrustedEntry::stream_codec()
            .encode(&mut out, &entry)
            .unwrap();
        let bytes = written(out);
        // bool true, Status id 3, then STRING_UTF8: varint length 25 + bytes.
        let mut expected = vec![1, 3, 25];
        expected.extend_from_slice(b"https://status.rivet.test");
        assert_eq!(bytes, expected);
    }

    #[test]
    fn untrusted_links_stream_codec_wire_form() {
        // list() -> varint count, then each untrusted entry.
        let links = vec![
            UntrustedEntry::new(
                Either::left(KnownLinkType::Support),
                "https://a".to_string(),
            ),
            UntrustedEntry::new(
                Either::right(Component::literal("Community")),
                "https://b".to_string(),
            ),
        ];
        let mut out = buf();
        untrusted_links_stream_codec()
            .encode(&mut out, &links)
            .unwrap();
        let bytes = written(out);
        // count 2; entry 1: true + Support(2) + utf("https://a" len 9);
        // entry 2: false + NBT string tag + utf("https://b" len 9).
        assert_eq!(bytes[0], 2);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(
            untrusted_links_stream_codec().decode(&mut input).unwrap(),
            links
        );
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn untrusted_links_negative_count_panics_like_arraylist() {
        // `list()` constructor replicates `ArrayList(int)`'s negative-capacity
        // IllegalArgumentException. Varint -1 -> count -1 -> panic.
        let mut input = FriendlyByteBuf::new(BytesMut::from(
            vec![0xFF, 0xFF, 0xFF, 0xFF, 0x0F].as_slice(),
        ));
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = untrusted_links_stream_codec().decode(&mut input);
        }));
        assert!(err.is_err());
    }
}
