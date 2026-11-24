//! Port of `net.minecraft.network.protocol.game.ClientboundBlockDestructionPacket`
//! (MC 26.2) — `block_destruction` (play clientbound).
//!
//! Java: `working/Paper/paper-server/src/minecraft/java/net/minecraft/network/
//! protocol/game/ClientboundBlockDestructionPacket.java`. Wire body:
//! `writeVarInt(id)`, `writeBlockPos(pos)` (packed long), `writeByte(progress)`
//! (low 8 bits of the `int`). Decoding reads `readUnsignedByte()`, so a
//! written `-1` reads back as `255`.

use rivet_registry::core::BlockPos;

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;

/// `ClientboundBlockDestructionPacket`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientboundBlockDestructionPacket {
    pub id: i32,
    pub pos: BlockPos,
    pub progress: i32,
}

impl ClientboundBlockDestructionPacket {
    /// `new ClientboundBlockDestructionPacket(int id, BlockPos pos, int progress)`.
    pub fn new(id: i32, pos: BlockPos, progress: i32) -> Self {
        ClientboundBlockDestructionPacket { id, pos, progress }
    }

    /// `getId()`.
    pub fn get_id(&self) -> i32 {
        self.id
    }

    /// `getPos()`.
    pub fn get_pos(&self) -> BlockPos {
        self.pos
    }

    /// `getProgress()`.
    pub fn get_progress(&self) -> i32 {
        self.progress
    }
}

impl Packet for ClientboundBlockDestructionPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::clientbound("block_destruction")
    }
}

/// `STREAM_CODEC` — `Packet.codec(write, new)`.
pub fn block_destruction_codec() -> StreamCodec<FriendlyByteBuf, ClientboundBlockDestructionPacket>
{
    codec(
        |value: &ClientboundBlockDestructionPacket, output: &mut FriendlyByteBuf| {
            output.write_var_int(value.id);
            output.write_long(value.pos.as_long());
            // `ByteBuf.writeByte(int)` keeps the low 8 bits.
            output.write_byte(value.progress as i8);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| {
            let id = input.read_var_int();
            let pos = BlockPos::of_long(input.read_long());
            let progress = input.read_unsigned_byte() as i32;
            Ok(ClientboundBlockDestructionPacket { id, pos, progress })
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

    /// Paper oracle: `run_interaction_packets_probe.sh` (actual `STREAM_CODEC`),
    /// including the `-1` clear stage and out-of-byte-range progress values.
    #[test]
    fn block_destruction_bodies_match_actual_paper() {
        let mut rows = 0;
        for line in
            include_str!("../../../../tools/rivet-oracle/fixtures/interaction-packets.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            if row["packet"] != "block_destruction" {
                continue;
            }
            rows += 1;
            let p = &row["pos"];
            let packet = ClientboundBlockDestructionPacket::new(
                row["id"].as_i64().unwrap() as i32,
                BlockPos::new(
                    p[0].as_i64().unwrap() as i32,
                    p[1].as_i64().unwrap() as i32,
                    p[2].as_i64().unwrap() as i32,
                ),
                row["progress"].as_i64().unwrap() as i32,
            );
            let mut out = FriendlyByteBuf::new(BytesMut::new());
            block_destruction_codec().encode(&mut out, &packet).unwrap();
            assert_eq!(hex(out.as_slice()), row["body"].as_str().unwrap(), "{line}");
            let decoded = block_destruction_codec().decode(&mut out).unwrap();
            assert_eq!(out.readable_bytes(), 0);
            assert_eq!(decoded.get_id(), packet.id);
            assert_eq!(decoded.get_pos(), packet.pos);
            assert_eq!(
                decoded.get_progress(),
                row["decoded_progress"].as_i64().unwrap() as i32
            );
        }
        assert!(rows > 0);
    }

    #[test]
    fn packet_type_is_block_destruction() {
        assert_eq!(
            ClientboundBlockDestructionPacket::new(0, BlockPos::ZERO, 0).packet_type(),
            PacketType::clientbound("block_destruction")
        );
    }
}
