//! Port of `net.minecraft.network.protocol.game.ServerboundUseItemOnPacket`
//! (MC 26.2) — `use_item_on` (play serverbound).
//!
//! Java: `working/Paper/paper-server/src/minecraft/java/net/minecraft/network/
//! protocol/game/ServerboundUseItemOnPacket.java`. Wire body:
//! `readEnum(InteractionHand)` (VarInt ordinal), `readBlockHitResult()`,
//! `readVarInt()` sequence. Spigot adds the public `timestamp` field, stamped
//! with `System.currentTimeMillis()` by the decoding constructor only (the
//! public constructor leaves it `0`); `ServerGamePacketListenerImpl.checkLimit`
//! reads it.
//!
//! An out-of-range hand or direction ordinal decodes to `Err` with the JVM's
//! `ArrayIndexOutOfBoundsException` text; a truncated body panics like every
//! raw `FriendlyByteBuf` read (the play decode boundary catches both and
//! disconnects, as netty does for a `DecoderException`). `handle` (listener
//! dispatch) is a documented STUB like the rest of this slice.

use std::time::{SystemTime, UNIX_EPOCH};

use rivet_registry::core::{BlockHitResult, InteractionHand};

use crate::codec::{CodecError, StreamCodec};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;

/// `ServerboundUseItemOnPacket`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServerboundUseItemOnPacket {
    pub block_hit: BlockHitResult,
    pub hand: InteractionHand,
    pub sequence: i32,
    /// Spigot `public long timestamp`.
    pub timestamp: i64,
}

impl ServerboundUseItemOnPacket {
    /// `new ServerboundUseItemOnPacket(InteractionHand, BlockHitResult, int sequence)`.
    pub fn new(hand: InteractionHand, block_hit: BlockHitResult, sequence: i32) -> Self {
        ServerboundUseItemOnPacket {
            block_hit,
            hand,
            sequence,
            timestamp: 0,
        }
    }

    /// `getHand()`.
    pub fn get_hand(&self) -> InteractionHand {
        self.hand
    }

    /// `getHitResult()`.
    pub fn get_hit_result(&self) -> BlockHitResult {
        self.block_hit
    }

    /// `getSequence()`.
    pub fn get_sequence(&self) -> i32 {
        self.sequence
    }
}

impl Packet for ServerboundUseItemOnPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::serverbound("use_item_on")
    }
}

/// `input.readEnum(InteractionHand.class)` — `values()[readVarInt()]`.
pub(crate) fn read_interaction_hand(
    input: &mut FriendlyByteBuf,
) -> Result<InteractionHand, CodecError> {
    let ordinal = input.read_var_int();
    usize::try_from(ordinal)
        .ok()
        .and_then(|index| InteractionHand::VALUES.get(index).copied())
        .ok_or_else(|| {
            CodecError::new(format!(
                "Index {ordinal} out of bounds for length {}",
                InteractionHand::VALUES.len()
            ))
        })
}

/// `System.currentTimeMillis()`.
pub(crate) fn current_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// `STREAM_CODEC` — `Packet.codec(write, new)`.
pub fn use_item_on_codec() -> StreamCodec<FriendlyByteBuf, ServerboundUseItemOnPacket> {
    codec(
        |value: &ServerboundUseItemOnPacket, output: &mut FriendlyByteBuf| {
            output.write_enum(|hand: &InteractionHand| hand.ordinal(), &value.hand);
            output.write_block_hit_result(&value.block_hit);
            output.write_var_int(value.sequence);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| {
            let timestamp = current_time_millis();
            let hand = read_interaction_hand(input)?;
            let block_hit = input.read_block_hit_result()?;
            let sequence = input.read_var_int();
            Ok(ServerboundUseItemOnPacket {
                block_hit,
                hand,
                sequence,
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
    use rivet_registry::core::{BlockPos, Direction, Vec3};

    fn buf() -> FriendlyByteBuf {
        FriendlyByteBuf::new(BytesMut::new())
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn from_hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Paper oracle: `tools/rivet-oracle/scripts/run_interaction_packets_probe.sh`
    /// encodes and decodes these rows with the actual `STREAM_CODEC`.
    #[test]
    fn use_item_on_bodies_match_actual_paper() {
        let mut rows = 0;
        for line in
            include_str!("../../../../tools/rivet-oracle/fixtures/interaction-packets.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            if row["packet"] != "use_item_on" {
                continue;
            }
            rows += 1;
            let p = &row["pos"];
            let pos = BlockPos::new(
                p[0].as_i64().unwrap() as i32,
                p[1].as_i64().unwrap() as i32,
                p[2].as_i64().unwrap() as i32,
            );
            let hand = InteractionHand::VALUES[row["hand"].as_u64().unwrap() as usize];
            let direction = Direction::VALUES[row["direction"].as_u64().unwrap() as usize];
            let l = &row["location_bits"];
            let location = Vec3::new(
                f64::from_bits(l[0].as_u64().unwrap()),
                f64::from_bits(l[1].as_u64().unwrap()),
                f64::from_bits(l[2].as_u64().unwrap()),
            );
            let packet = ServerboundUseItemOnPacket::new(
                hand,
                BlockHitResult::new_with_world_border_hit(
                    location,
                    direction,
                    pos,
                    row["inside"].as_bool().unwrap(),
                    row["world_border"].as_bool().unwrap(),
                ),
                row["sequence"].as_i64().unwrap() as i32,
            );
            let mut out = buf();
            use_item_on_codec().encode(&mut out, &packet).unwrap();
            let body = row["body"].as_str().unwrap();
            assert_eq!(hex(out.as_slice()), body, "{line}");

            // Decoding the Paper bytes reproduces Paper's decoded location.
            let mut input = FriendlyByteBuf::new(BytesMut::from(from_hex(body).as_slice()));
            let decoded = use_item_on_codec().decode(&mut input).unwrap();
            assert_eq!(input.readable_bytes(), 0);
            let d = &row["decoded_location_bits"];
            let decoded_location = decoded.get_hit_result().get_location();
            assert_eq!(decoded_location.x.to_bits(), d[0].as_u64().unwrap());
            assert_eq!(decoded_location.y.to_bits(), d[1].as_u64().unwrap());
            assert_eq!(decoded_location.z.to_bits(), d[2].as_u64().unwrap());
            assert_eq!(decoded.get_hand(), hand);
            assert_eq!(decoded.get_sequence(), packet.sequence);
            assert_eq!(decoded.get_hit_result().get_block_pos(), pos);
            assert_eq!(decoded.get_hit_result().get_direction(), direction);
        }
        assert!(rows > 0);
    }

    /// Paper oracle controlled negatives: out-of-range ordinals throw
    /// `ArrayIndexOutOfBoundsException` from `readEnum`.
    #[test]
    fn malformed_use_item_on_bodies_fail_like_actual_paper() {
        let mut rows = 0;
        for line in
            include_str!("../../../../tools/rivet-oracle/fixtures/interaction-packets.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            if row["packet"] != "use_item_on_malformed" {
                continue;
            }
            rows += 1;
            let body = from_hex(row["body"].as_str().unwrap());
            let mut input = FriendlyByteBuf::new(BytesMut::from(body.as_slice()));
            let error = use_item_on_codec().decode(&mut input).unwrap_err();
            assert_eq!(error.message, row["error"].as_str().unwrap(), "{line}");
        }
        assert!(rows > 0);
    }

    #[test]
    fn decode_stamps_timestamp_and_constructor_leaves_zero() {
        let packet = ServerboundUseItemOnPacket::new(
            InteractionHand::OffHand,
            BlockHitResult::new(
                Vec3::new(0.5, 0.5, 0.5),
                Direction::Up,
                BlockPos::ZERO,
                false,
            ),
            7,
        );
        assert_eq!(packet.timestamp, 0);
        let mut out = buf();
        use_item_on_codec().encode(&mut out, &packet).unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let decoded = use_item_on_codec().decode(&mut input).unwrap();
        assert!(decoded.timestamp > 0);
        assert_eq!(
            ServerboundUseItemOnPacket {
                timestamp: 0,
                ..decoded
            },
            packet
        );
    }

    #[test]
    fn packet_type_is_use_item_on() {
        let packet = ServerboundUseItemOnPacket::new(
            InteractionHand::MainHand,
            BlockHitResult::new(
                Vec3::new(0.0, 0.0, 0.0),
                Direction::Down,
                BlockPos::ZERO,
                false,
            ),
            0,
        );
        assert_eq!(packet.packet_type(), PacketType::serverbound("use_item_on"));
    }
}
