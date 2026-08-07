//! Port of `net.minecraft.network.protocol.game.ClientboundChunkBatchStartPacket`
//! (issue #94).
//!
//! Java: `ClientboundChunkBatchStartPacket.java` in `working/Paper`. A singleton
//! body (the chunk-batch opener) whose codec is `StreamCodec.unit(INSTANCE)` —
//! zero-length on the wire, decoding to the singleton. The `Packet` value type
//! is the unit struct.

use crate::codec::StreamCodec;
use crate::codec::unit;
use crate::protocol::game::packet_types::clientbound_chunk_batch_start;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ClientboundChunkBatchStartPacket` — the chunk-batch opener singleton.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientboundChunkBatchStartPacket;

impl ClientboundChunkBatchStartPacket {
    /// `ClientboundChunkBatchStartPacket.INSTANCE`.
    pub const INSTANCE: ClientboundChunkBatchStartPacket = ClientboundChunkBatchStartPacket;
}

impl Packet for ClientboundChunkBatchStartPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_chunk_batch_start()
    }
}

impl std::fmt::Display for ClientboundChunkBatchStartPacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `StreamCodec.unit` needs `Display` for its encode-mismatch panic.
        f.write_str("ClientboundChunkBatchStartPacket")
    }
}

/// `ClientboundChunkBatchStartPacket.STREAM_CODEC` — `StreamCodec.unit(INSTANCE)`.
///
/// Java's static is `StreamCodec<ByteBuf, ...>`; the Rust port instantiates it
/// over [`FriendlyByteBuf`](crate::friendly_byte_buf::FriendlyByteBuf) (the
/// crate's base buffer).
pub fn stream_codec()
-> StreamCodec<crate::friendly_byte_buf::FriendlyByteBuf, ClientboundChunkBatchStartPacket> {
    unit(ClientboundChunkBatchStartPacket::INSTANCE)
}
