//! Port of `net.minecraft.network.protocol.common.ClientboundResourcePackPopPacket`
//! (issue #86).
//!
//! Java: `ClientboundResourcePackPopPacket.java` in `working/Paper`. An
//! `Optional<UUID>` — a present id pops that pack, absent pops the whole stack.
//! Registered in play and configuration clientbound.

use crate::codec::{StreamCodec, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::packet_types::clientbound_resource_pack_pop;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use rivet_util::uuid::Uuid;

/// `net.minecraft.network.protocol.common.ClientboundResourcePackPopPacket`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientboundResourcePackPopPacket {
    id: Option<Uuid>,
}

impl ClientboundResourcePackPopPacket {
    /// `new ClientboundResourcePackPopPacket(Optional<UUID> id)`.
    pub fn new(id: Option<Uuid>) -> Self {
        ClientboundResourcePackPopPacket { id }
    }

    /// `ClientboundResourcePackPopPacket.id()`.
    pub fn id(&self) -> Option<Uuid> {
        self.id
    }

    /// `ClientboundResourcePackPopPacket.STREAM_CODEC` —
    /// `readOptional`/`writeOptional(UUIDUtil.STREAM_CODEC)`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundResourcePackPopPacket> {
        of(
            |output: &mut FriendlyByteBuf, value: &ClientboundResourcePackPopPacket| {
                output.write_optional(value.id.as_ref(), |out, uuid| {
                    out.write_uuid(*uuid);
                });
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                Ok(ClientboundResourcePackPopPacket::new(
                    input.read_optional(|buf| buf.read_uuid()),
                ))
            },
        )
    }
}

impl Packet for ClientboundResourcePackPopPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_resource_pack_pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    fn uuid() -> Uuid {
        Uuid { most: 1, least: 2 }
    }

    #[test]
    fn golden_wire_bytes_present() {
        // `writeOptional(present)` -> true byte, then the 16-byte UUID.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundResourcePackPopPacket::stream_codec()
            .encode(
                &mut out,
                &ClientboundResourcePackPopPacket::new(Some(uuid())),
            )
            .unwrap();
        let mut expected = vec![1u8];
        expected.extend_from_slice(&1i64.to_be_bytes());
        expected.extend_from_slice(&2i64.to_be_bytes());
        assert_eq!(out.into_inner().to_vec(), expected);
    }

    #[test]
    fn golden_wire_bytes_absent() {
        // `writeOptional(empty)` -> false byte only.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundResourcePackPopPacket::stream_codec()
            .encode(&mut out, &ClientboundResourcePackPopPacket::new(None))
            .unwrap();
        assert_eq!(out.into_inner().to_vec(), vec![0u8]);
    }

    #[test]
    fn round_trips() {
        for packet in [
            ClientboundResourcePackPopPacket::new(Some(uuid())),
            ClientboundResourcePackPopPacket::new(None),
        ] {
            let mut out = FriendlyByteBuf::new(BytesMut::new());
            ClientboundResourcePackPopPacket::stream_codec()
                .encode(&mut out, &packet)
                .unwrap();
            let mut input = FriendlyByteBuf::new(out.into_inner());
            assert_eq!(
                ClientboundResourcePackPopPacket::stream_codec()
                    .decode(&mut input)
                    .unwrap(),
                packet
            );
        }
    }
}
