//! Port of `net.minecraft.network.protocol.game.ClientboundBlockChangedAckPacket`
//! (MC 26.2) — `block_changed_ack` (play clientbound).
//!
//! Java: `working/Paper/paper-server/src/minecraft/java/net/minecraft/network/
//! protocol/game/ClientboundBlockChangedAckPacket.java` — `record (int
//! sequence)`, wire body `writeVarInt(sequence)`.

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;

/// `ClientboundBlockChangedAckPacket(int sequence)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientboundBlockChangedAckPacket {
    pub sequence: i32,
}

impl ClientboundBlockChangedAckPacket {
    /// `new ClientboundBlockChangedAckPacket(int sequence)`.
    pub fn new(sequence: i32) -> Self {
        ClientboundBlockChangedAckPacket { sequence }
    }

    /// `sequence()`.
    pub fn sequence(&self) -> i32 {
        self.sequence
    }
}

impl Packet for ClientboundBlockChangedAckPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::clientbound("block_changed_ack")
    }
}

/// `STREAM_CODEC` — `Packet.codec(write, new)`.
pub fn block_changed_ack_codec() -> StreamCodec<FriendlyByteBuf, ClientboundBlockChangedAckPacket> {
    codec(
        |value: &ClientboundBlockChangedAckPacket, output: &mut FriendlyByteBuf| {
            output.write_var_int(value.sequence);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| {
            Ok(ClientboundBlockChangedAckPacket::new(input.read_var_int()))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// Paper oracle: `run_interaction_packets_probe.sh` (actual `STREAM_CODEC`).
    #[test]
    fn block_changed_ack_bodies_match_actual_paper() {
        let mut rows = 0;
        for line in
            include_str!("../../../../tools/rivet-oracle/fixtures/interaction-packets.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            if row["packet"] != "block_changed_ack" {
                continue;
            }
            rows += 1;
            let packet =
                ClientboundBlockChangedAckPacket::new(row["sequence"].as_i64().unwrap() as i32);
            let mut out = FriendlyByteBuf::new(BytesMut::new());
            block_changed_ack_codec().encode(&mut out, &packet).unwrap();
            assert_eq!(hex(out.as_slice()), row["body"].as_str().unwrap(), "{line}");
            assert_eq!(block_changed_ack_codec().decode(&mut out).unwrap(), packet);
            assert_eq!(out.readable_bytes(), 0);
        }
        assert!(rows > 0);
    }

    #[test]
    fn packet_type_is_block_changed_ack() {
        assert_eq!(
            ClientboundBlockChangedAckPacket::new(0).packet_type(),
            PacketType::clientbound("block_changed_ack")
        );
    }
}
