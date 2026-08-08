//! Port of `net.minecraft.network.protocol.common.ClientboundServerLinksPacket`
//! (issue #207).
//!
//! Java: `ClientboundServerLinksPacket.java` in `working/Paper`. Carries the
//! server's link entries as a list of untrusted wire entries (`ServerLinks
//! .UNTRUSTED_LINKS_STREAM_CODEC` — a varint count, then each entry's
//! `Either<KnownLinkType, Component>` + `STRING_UTF8` link). Registered in
//! play and configuration clientbound; the vanilla server only sends it when
//! `ServerLinks` is non-empty (not on a default join).
//!
//! The wire codecs live in the sibling `server_links` module (issue #207, same
//! crate); the server-side `ServerLinks` value type is not ported here.

use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::packet_types::clientbound_server_links;
use crate::protocol::common::server_links::{UntrustedEntry, untrusted_links_stream_codec};
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `net.minecraft.network.protocol.common.ClientboundServerLinksPacket`.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientboundServerLinksPacket {
    links: Vec<UntrustedEntry>,
}

impl ClientboundServerLinksPacket {
    /// `new ClientboundServerLinksPacket(List<ServerLinks.UntrustedEntry>)`.
    pub fn new(links: Vec<UntrustedEntry>) -> Self {
        ClientboundServerLinksPacket { links }
    }

    /// `ClientboundServerLinksPacket.links()`.
    pub fn links(&self) -> &[UntrustedEntry] {
        &self.links
    }

    /// `ClientboundServerLinksPacket.STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundServerLinksPacket> {
        let links_codec = untrusted_links_stream_codec();
        let links_codec_decode = links_codec.clone();
        of(
            move |output: &mut FriendlyByteBuf, value: &ClientboundServerLinksPacket| {
                links_codec.encode(output, &value.links)
            },
            move |input: &mut FriendlyByteBuf| {
                Ok(ClientboundServerLinksPacket::new(
                    links_codec_decode.decode(input)?,
                ))
            },
        )
    }
}

impl Packet for ClientboundServerLinksPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_server_links()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::common::server_links::KnownLinkType;
    use bytes::BytesMut;
    use rivet_serialization::Either;
    use rivet_text::Component;

    fn untrusted_entry(r#type: Either<KnownLinkType, Component>, link: &str) -> UntrustedEntry {
        UntrustedEntry::new(r#type, link.to_string())
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn golden_wire_bytes() {
        // One known-type entry (true, Status id 3, link "https://status") and
        // one custom entry (false, NBT StringTag "Custom", link "https://custom"),
        // then the varint count 2 in front.
        //
        // Layout per entry: either() -> bool; left = varint id, right = the
        // trusted component tag (NBT type byte + u16-BE length-prefixed
        // modified-UTF-8 payload); then STRING_UTF8 link (varint len + bytes).
        // A literal component collapses to the codec's string branch, so the
        // tag is a bare StringTag (type byte 8).
        let packet = ClientboundServerLinksPacket::new(vec![
            untrusted_entry(Either::left(KnownLinkType::Status), "https://status"),
            untrusted_entry(
                Either::right(Component::literal("Custom")),
                "https://custom",
            ),
        ]);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundServerLinksPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        assert_eq!(
            out.into_inner().to_vec(),
            hex(
                "0201030e68747470733a2f2f73746174757300080006437573746f6d0e68747470733a2f2f637573746f6d"
            )
        );

        let bytes = hex(
            "0201030e68747470733a2f2f73746174757300080006437573746f6d0e68747470733a2f2f637573746f6d",
        );
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(
            ClientboundServerLinksPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn empty_links_wire_form() {
        // A vanilla `ServerLinks.EMPTY` -> a single varint count 0 byte.
        let packet = ClientboundServerLinksPacket::new(Vec::new());
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundServerLinksPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        assert_eq!(out.into_inner().to_vec(), vec![0]);
    }

    #[test]
    fn custom_component_is_trusted_tag_not_compound() {
        // The trusted component codec writes the tag type byte first; a literal
        // component is the string branch, so it is a StringTag (type 8), not a
        // CompoundTag (type 10). Pin the wire directly, then round-trip.
        let packet = ClientboundServerLinksPacket::new(vec![untrusted_entry(
            Either::right(Component::literal("Hub")),
            "https://hub",
        )]);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundServerLinksPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes[0], 1); // count
        assert_eq!(bytes[1], 0); // custom branch
        assert_eq!(bytes[2], 8); // StringTag type byte
        // u16-BE length 3, then "Hub".
        assert_eq!(&bytes[3..5], &[0x00, 0x03]);
        assert_eq!(&bytes[5..8], b"Hub");
        assert_eq!(bytes[8], 11); // link "https://hub"

        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(
            ClientboundServerLinksPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }

    #[test]
    fn translatable_custom_link_round_trips_through_full_codec() {
        // A translatable custom display name does not collapse to the codec's
        // string branch, so it exercises the full recursive Component codec:
        // the record codec emits a CompoundTag (type byte 10) whose "type" key
        // dispatches back to TranslatableContents on decode. This is the
        // non-literal case Java's `ServerLinks.Entry.custom(Component, URI)`
        // produces in practice.
        let packet = ClientboundServerLinksPacket::new(vec![untrusted_entry(
            Either::right(Component::translatable("menu.support")),
            "https://support",
        )]);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundServerLinksPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes[0], 1); // count
        assert_eq!(bytes[1], 0); // custom branch
        assert_eq!(bytes[2], 10); // CompoundTag type byte (not StringTag 8)

        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(
            ClientboundServerLinksPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn hostile_link_string_over_bound_errors() {
        // STRING_UTF8 max is 32767 UTF-16 units. A custom link over that bound
        // fails at the codec boundary (Err), not a panic.
        let packet = ClientboundServerLinksPacket::new(vec![untrusted_entry(
            Either::left(KnownLinkType::Website),
            &"a".repeat(32_768),
        )]);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        let err = ClientboundServerLinksPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap_err();
        assert_eq!(
            err.message,
            "String too big (was 32768 characters, max 32767)"
        );
    }

    #[test]
    fn hostile_negative_count_panics_like_arraylist() {
        // A wire count of -1 passes the unbounded list() max check and then
        // panics in the constructor (`ArrayList(int)` IllegalArgumentException),
        // mirroring Java. The established `list()` behavior, surfaced through
        // the packet codec.
        let mut input = FriendlyByteBuf::new(BytesMut::from(
            vec![0xFF, 0xFF, 0xFF, 0xFF, 0x0F].as_slice(),
        ));
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = ClientboundServerLinksPacket::stream_codec().decode(&mut input);
        }));
        assert!(err.is_err());
    }

    #[test]
    fn hostile_count_without_entries_errors() {
        // Count 2 but no entry bytes: the first type decode hits the raw
        // buffer's EOF panic (netty IndexOutOfBounds), the established raw-buf
        // contract — not a codec Err.
        let mut input = FriendlyByteBuf::new(BytesMut::from(vec![2u8].as_slice()));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = ClientboundServerLinksPacket::stream_codec().decode(&mut input);
            }))
            .is_err()
        );
    }

    #[test]
    fn packet_type_is_clientbound_server_links() {
        assert_eq!(
            ClientboundServerLinksPacket::new(Vec::new()).packet_type(),
            clientbound_server_links()
        );
        assert_eq!(
            clientbound_server_links().id().to_string(),
            "minecraft:server_links"
        );
    }
}
