//! Port of `net.minecraft.network.protocol.game.ServerboundUseItemPacket`
//! (MC 26.2) — `use_item` (play serverbound).
//!
//! Java: `working/Paper/paper-server/src/minecraft/java/net/minecraft/network/
//! protocol/game/ServerboundUseItemPacket.java`. Wire body:
//! `readEnum(InteractionHand)` (VarInt ordinal), `readVarInt()` sequence,
//! `readFloat()` yRot, `readFloat()` xRot. Spigot's `timestamp` is stamped by
//! the decoding constructor only (see `serverbound_use_item_on_packet`).
//! Rotations are carried raw (NaN/infinite included); the listener wraps them.

use rivet_registry::core::InteractionHand;

use super::serverbound_use_item_on_packet::{current_time_millis, read_interaction_hand};
use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;

/// `ServerboundUseItemPacket`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServerboundUseItemPacket {
    pub hand: InteractionHand,
    pub sequence: i32,
    pub y_rot: f32,
    pub x_rot: f32,
    /// Spigot `public long timestamp`.
    pub timestamp: i64,
}

impl ServerboundUseItemPacket {
    /// `new ServerboundUseItemPacket(InteractionHand, int sequence, float yRot, float xRot)`.
    pub fn new(hand: InteractionHand, sequence: i32, y_rot: f32, x_rot: f32) -> Self {
        ServerboundUseItemPacket {
            hand,
            sequence,
            y_rot,
            x_rot,
            timestamp: 0,
        }
    }

    /// `getHand()`.
    pub fn get_hand(&self) -> InteractionHand {
        self.hand
    }

    /// `getSequence()`.
    pub fn get_sequence(&self) -> i32 {
        self.sequence
    }

    /// `getYRot()`.
    pub fn get_y_rot(&self) -> f32 {
        self.y_rot
    }

    /// `getXRot()`.
    pub fn get_x_rot(&self) -> f32 {
        self.x_rot
    }
}

impl Packet for ServerboundUseItemPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::serverbound("use_item")
    }
}

/// `STREAM_CODEC` — `Packet.codec(write, new)`.
pub fn use_item_codec() -> StreamCodec<FriendlyByteBuf, ServerboundUseItemPacket> {
    codec(
        |value: &ServerboundUseItemPacket, output: &mut FriendlyByteBuf| {
            output.write_enum(|hand: &InteractionHand| hand.ordinal(), &value.hand);
            output.write_var_int(value.sequence);
            output.write_float(value.y_rot);
            output.write_float(value.x_rot);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| {
            let timestamp = current_time_millis();
            let hand = read_interaction_hand(input)?;
            let sequence = input.read_var_int();
            let y_rot = input.read_float();
            let x_rot = input.read_float();
            Ok(ServerboundUseItemPacket {
                hand,
                sequence,
                y_rot,
                x_rot,
                timestamp,
            })
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

    fn from_hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Paper oracle: `run_interaction_packets_probe.sh` (actual `STREAM_CODEC`).
    #[test]
    fn use_item_bodies_match_actual_paper() {
        let mut rows = 0;
        for line in
            include_str!("../../../../tools/rivet-oracle/fixtures/interaction-packets.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            match row["packet"].as_str().unwrap() {
                "use_item" => {
                    rows += 1;
                    let packet = ServerboundUseItemPacket::new(
                        InteractionHand::VALUES[row["hand"].as_u64().unwrap() as usize],
                        row["sequence"].as_i64().unwrap() as i32,
                        f32::from_bits(row["y_rot_bits"].as_u64().unwrap() as u32),
                        f32::from_bits(row["x_rot_bits"].as_u64().unwrap() as u32),
                    );
                    let mut out = FriendlyByteBuf::new(BytesMut::new());
                    use_item_codec().encode(&mut out, &packet).unwrap();
                    assert_eq!(hex(out.as_slice()), row["body"].as_str().unwrap(), "{line}");
                    let decoded = use_item_codec().decode(&mut out).unwrap();
                    assert_eq!(out.readable_bytes(), 0);
                    assert_eq!(decoded.get_hand(), packet.hand);
                    assert_eq!(decoded.get_sequence(), packet.sequence);
                    assert_eq!(decoded.get_y_rot().to_bits(), packet.y_rot.to_bits());
                    assert_eq!(decoded.get_x_rot().to_bits(), packet.x_rot.to_bits());
                }
                "use_item_malformed" => {
                    rows += 1;
                    let body = from_hex(row["body"].as_str().unwrap());
                    let mut input = FriendlyByteBuf::new(BytesMut::from(body.as_slice()));
                    let error = use_item_codec().decode(&mut input).unwrap_err();
                    assert_eq!(error.message, row["error"].as_str().unwrap(), "{line}");
                }
                _ => {}
            }
        }
        assert!(rows > 0);
    }

    #[test]
    fn packet_type_is_use_item() {
        assert_eq!(
            ServerboundUseItemPacket::new(InteractionHand::MainHand, 0, 0.0, 0.0).packet_type(),
            PacketType::serverbound("use_item")
        );
    }
}
