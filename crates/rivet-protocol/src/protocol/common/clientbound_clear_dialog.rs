//! Port of `net.minecraft.network.protocol.common.ClientboundClearDialogPacket`
//! (issue #86).
//!
//! Java: `ClientboundClearDialogPacket.java` in `working/Paper`. A singleton
//! (private constructor + `INSTANCE`) whose `STREAM_CODEC = StreamCodec.unit(INSTANCE)`
//! encodes nothing. Registered in play and configuration clientbound.

use crate::codec::{StreamCodec, unit};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::packet_types::clientbound_clear_dialog;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use std::fmt;

/// `net.minecraft.network.protocol.common.ClientboundClearDialogPacket`.
///
/// `Display` prints the packet type (`clear_dialog/minecraft:clear_dialog`),
/// satisfying `unit`'s `PartialEq + Display` bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientboundClearDialogPacket;

impl Packet for ClientboundClearDialogPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_clear_dialog()
    }
}

impl fmt::Display for ClientboundClearDialogPacket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.packet_type())
    }
}

/// `ClientboundClearDialogPacket.STREAM_CODEC` — `StreamCodec.unit(INSTANCE)`.
pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundClearDialogPacket> {
    unit(ClientboundClearDialogPacket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn unit_codec_encodes_nothing() {
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        stream_codec()
            .encode(&mut out, &ClientboundClearDialogPacket)
            .unwrap();
        assert!(out.into_inner().is_empty());
    }

    #[test]
    fn unit_codec_decodes_instance() {
        let mut input = FriendlyByteBuf::new(BytesMut::new());
        assert_eq!(
            stream_codec().decode(&mut input).unwrap(),
            ClientboundClearDialogPacket
        );
    }
}
