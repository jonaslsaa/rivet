//! Port of `net.minecraft.network.protocol.login.ClientboundLoginCompressionPacket`
//! (issue #99).
//!
//! Java: `ClientboundLoginCompressionPacket.java` in `working/Paper`. Tells the
//! client to switch to zlib compression at `compressionThreshold` bytes.
//! Registered at login clientbound id 3 (`LoginProtocols` / generated table).
//!
//! The compression pipeline this packet switches on (`CompressionEncoder` /
//! `CompressionDecoder`, `Connection.setupCompression`) is #88 (PR #190); this
//! module is just the packet body. The client-side `handleCompression` is
//! deferred with the listener hierarchy.

use crate::codec::{StreamCodec, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::login::packet_types::clientbound_login_compression;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `net.minecraft.network.protocol.login.ClientboundLoginCompressionPacket`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientboundLoginCompressionPacket {
    compression_threshold: i32,
}

impl ClientboundLoginCompressionPacket {
    /// `new ClientboundLoginCompressionPacket(int compressionThreshold)`.
    pub fn new(compression_threshold: i32) -> Self {
        ClientboundLoginCompressionPacket {
            compression_threshold,
        }
    }

    /// `ClientboundLoginCompressionPacket.getCompressionThreshold()`.
    pub fn compression_threshold(&self) -> i32 {
        self.compression_threshold
    }

    /// `ClientboundLoginCompressionPacket.STREAM_CODEC` — a single VarInt.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundLoginCompressionPacket> {
        of(
            |output: &mut FriendlyByteBuf, value: &ClientboundLoginCompressionPacket| {
                output.write_var_int(value.compression_threshold);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                Ok(ClientboundLoginCompressionPacket::new(input.read_var_int()))
            },
        )
    }
}

impl Packet for ClientboundLoginCompressionPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_login_compression()
    }
}
