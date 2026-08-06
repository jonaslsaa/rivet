//! Port of `net.minecraft.network.protocol.common.ClientboundTransferPacket`
//! (issue #86).
//!
//! Java: `ClientboundTransferPacket.java` in `working/Paper`. A `utf` host and a
//! `VarInt` port. Registered in play and configuration clientbound.

use crate::codec::byte_buf_codecs;
use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, of};
use crate::friendly_byte_buf::{FriendlyByteBuf, MAX_STRING_LENGTH};
use crate::protocol::common::packet_types::clientbound_transfer;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `net.minecraft.network.protocol.common.ClientboundTransferPacket`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundTransferPacket {
    host: String,
    port: i32,
}

impl ClientboundTransferPacket {
    /// `new ClientboundTransferPacket(String host, int port)`.
    pub fn new(host: String, port: i32) -> Self {
        ClientboundTransferPacket { host, port }
    }

    /// `ClientboundTransferPacket.host()`.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// `ClientboundTransferPacket.port()`.
    pub fn port(&self) -> i32 {
        self.port
    }

    /// `ClientboundTransferPacket.STREAM_CODEC`.
    ///
    /// The `utf` host goes through the `Err`-returning `string_utf8` codec
    /// (Java's `readUtf`/`writeUtf` at the `MAX_STRING_LENGTH` bound): an
    /// over-length host is a `CodecError`, not a panic.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundTransferPacket> {
        let host_codec = byte_buf_codecs::string_utf8(MAX_STRING_LENGTH);
        let host_codec_decode = host_codec.clone();
        of(
            move |output: &mut FriendlyByteBuf, value: &ClientboundTransferPacket| {
                host_codec.encode(output, &value.host)?;
                output.write_var_int(value.port);
                Ok(())
            },
            move |input: &mut FriendlyByteBuf| {
                let host = host_codec_decode.decode(input)?;
                Ok(ClientboundTransferPacket::new(host, input.read_var_int()))
            },
        )
    }
}

impl Packet for ClientboundTransferPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_transfer()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn golden_wire_bytes() {
        // `writeUtf("example.com")` -> varint 11 + "example.com", then port 25565
        // as a varint (0xdd 0xc7 0x01).
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundTransferPacket::stream_codec()
            .encode(
                &mut out,
                &ClientboundTransferPacket::new("example.com".to_string(), 25565),
            )
            .unwrap();
        assert_eq!(
            out.into_inner().to_vec(),
            vec![
                0x0b, b'e', b'x', b'a', b'm', b'p', b'l', b'e', b'.', b'c', b'o', b'm', 0xdd, 0xc7,
                0x01
            ]
        );
    }

    #[test]
    fn round_trips() {
        let packet = ClientboundTransferPacket::new("host".to_string(), 0);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundTransferPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ClientboundTransferPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
    }
}
