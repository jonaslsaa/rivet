//! Port of `net.minecraft.network.protocol.game.ServerboundPlayerActionPacket`
//! (MC 26.2).
//!
//! Java: `working/Paper/paper-server/src/minecraft/java/net/minecraft/network/
//! protocol/game/ServerboundPlayerActionPacket.java`. Wire body (protocol 776):
//!   VarInt action ordinal, i64 packed `BlockPos`, 1 byte direction
//!   (`get3DDataValue`), VarInt sequence.
//!
//! Decode: `readEnum(Action.class)` (out-of-range ordinal panics with the JVM's
//! `ArrayIndexOutOfBoundsException` text — see `serversbound_client_command_packet`),
//! `readBlockPos()` = `BlockPos.of(readLong())`, `Direction.from3DDataValue(
//! readUnsignedByte())` (wrapping `Mth.abs(data % 6)`), `readVarInt()`.
//! `handle` is a documented STUB.

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;
use rivet_registry::core::{BlockPos, Direction};

/// `ServerboundPlayerActionPacket.Action` — the eight player actions, in Java
/// declaration (ordinal) order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    StartDestroyBlock,
    AbortDestroyBlock,
    StopDestroyBlock,
    DropAllItems,
    DropItem,
    ReleaseUseItem,
    SwapItemWithOffhand,
    Stab,
}

impl Action {
    /// `Action.values()` — declaration order, `ordinal() == index`.
    pub const VALUES: [Action; 8] = [
        Action::StartDestroyBlock,
        Action::AbortDestroyBlock,
        Action::StopDestroyBlock,
        Action::DropAllItems,
        Action::DropItem,
        Action::ReleaseUseItem,
        Action::SwapItemWithOffhand,
        Action::Stab,
    ];

    /// `Action.values()[ordinal]` — panics with the JVM's
    /// `ArrayIndexOutOfBoundsException` message on an out-of-range ordinal,
    /// exactly as `FriendlyByteBuf.readEnum(Action.class)` does.
    pub fn from_ordinal(ordinal: i32) -> Action {
        match ordinal {
            0 => Action::StartDestroyBlock,
            1 => Action::AbortDestroyBlock,
            2 => Action::StopDestroyBlock,
            3 => Action::DropAllItems,
            4 => Action::DropItem,
            5 => Action::ReleaseUseItem,
            6 => Action::SwapItemWithOffhand,
            7 => Action::Stab,
            _ => panic!(
                "Index {ordinal} out of bounds for length {}",
                Action::VALUES.len()
            ),
        }
    }
}

/// `ServerboundPlayerActionPacket` — a player block-interaction action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerboundPlayerActionPacket {
    pub action: Action,
    pub pos: BlockPos,
    pub direction: Direction,
    pub sequence: i32,
}

impl ServerboundPlayerActionPacket {
    /// `new ServerboundPlayerActionPacket(Action, BlockPos, Direction, int sequence)`.
    pub fn new(action: Action, pos: BlockPos, direction: Direction, sequence: i32) -> Self {
        ServerboundPlayerActionPacket {
            action,
            pos: pos.immutable(),
            direction,
            sequence,
        }
    }

    /// `getAction()`.
    pub fn get_action(&self) -> Action {
        self.action
    }

    /// `getPos()`.
    pub fn get_pos(&self) -> BlockPos {
        self.pos
    }

    /// `getDirection()`.
    pub fn get_direction(&self) -> Direction {
        self.direction
    }

    /// `getSequence()`.
    pub fn get_sequence(&self) -> i32 {
        self.sequence
    }
}

impl Packet for ServerboundPlayerActionPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::serverbound("player_action")
    }
}

/// `STREAM_CODEC` — action ordinal, packed `BlockPos`, direction byte, sequence.
pub fn player_action_codec() -> StreamCodec<FriendlyByteBuf, ServerboundPlayerActionPacket> {
    codec(
        |value: &ServerboundPlayerActionPacket, output: &mut FriendlyByteBuf| {
            output.write_var_int(value.action as i32);
            output.write_long(value.pos.as_long());
            output.write_byte(value.direction.get_3d_data_value() as i8);
            output.write_var_int(value.sequence);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| {
            let action = Action::from_ordinal(input.read_var_int());
            let pos = BlockPos::of_long(input.read_long());
            let direction = Direction::from_3d_data_value(input.read_unsigned_byte() as i32);
            let sequence = input.read_var_int();
            Ok(ServerboundPlayerActionPacket {
                action,
                pos,
                direction,
                sequence,
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;
    use std::panic::catch_unwind;

    fn buf() -> FriendlyByteBuf {
        FriendlyByteBuf::new(BytesMut::new())
    }

    fn panic_message<F: FnOnce() -> R, R>(f: F) -> String {
        let err = match catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(_) => panic!("expected the closure to panic"),
            Err(err) => err,
        };
        err.downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "non-string panic payload".to_string())
    }

    #[test]
    fn round_trips_exact_bytes_and_length() {
        // START_DESTROY_BLOCK (0), pos (1,2,3), Down (3D data 0), seq 0.
        let packet = ServerboundPlayerActionPacket::new(
            Action::StartDestroyBlock,
            BlockPos::new(1, 2, 3),
            Direction::Down,
            0,
        );
        let codec = player_action_codec();
        let mut out = buf();
        codec.encode(&mut out, &packet).unwrap();
        let bytes = out.into_inner().to_vec();
        // varint 0 + 8 packed longs + 1 direction byte + varint 0.
        assert_eq!(bytes.len(), 11);
        let mut expected = vec![0x00];
        expected.extend_from_slice(&BlockPos::new(1, 2, 3).as_long().to_be_bytes());
        expected.push(0x00); // Down.get3DDataValue() == 0
        expected.push(0x00); // sequence 0
        assert_eq!(bytes, expected);

        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = codec.decode(&mut input).unwrap();
        assert_eq!(decoded, packet);
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn round_trips_all_actions_and_negative_coords() {
        // Note: BlockPos packing is 26-bit X/Z / 12-bit Y and is lossy for
        // out-of-range coordinates (Java `BlockPos.of(asLong())` is not a round
        // trip there), so the coords stay inside the representable range.
        let codec = player_action_codec();
        for (ordinal, action) in Action::VALUES.iter().enumerate() {
            let packet = ServerboundPlayerActionPacket::new(
                *action,
                BlockPos::new(-1000, 0, -1000),
                Direction::Up,
                300,
            );
            let mut out = buf();
            codec.encode(&mut out, &packet).unwrap();
            let mut input = FriendlyByteBuf::new(out.into_inner());
            assert_eq!(codec.decode(&mut input).unwrap(), packet);
            assert_eq!(input.readable_bytes(), 0);

            // The action ordinal is the first varint byte.
            let mut out = buf();
            codec.encode(&mut out, &packet).unwrap();
            let bytes = out.into_inner();
            assert_eq!(bytes[0], ordinal as u8);
        }
    }

    #[test]
    fn direction_wraps_out_of_range_byte_values() {
        // `Direction.from3DDataValue` wraps via `Mth.abs(data % 6)`: 6 -> Down,
        // 7 -> Up, -1 -> Up.
        assert_eq!(Direction::from_3d_data_value(6), Direction::Down);
        assert_eq!(Direction::from_3d_data_value(7), Direction::Up);
        assert_eq!(Direction::from_3d_data_value(-1), Direction::Up);

        let codec = player_action_codec();
        let mut out = buf();
        codec
            .encode(
                &mut out,
                &ServerboundPlayerActionPacket::new(
                    Action::DropItem,
                    BlockPos::new(0, 0, 0),
                    Direction::Up,
                    0,
                ),
            )
            .unwrap();
        let bytes = out.into_inner();
        // Patch the direction byte to 7 -> wraps to Up.
        let mut mutated = bytes.to_vec();
        mutated[9] = 7;
        let mut input = FriendlyByteBuf::new(BytesMut::from(mutated.as_slice()));
        let decoded = codec.decode(&mut input).unwrap();
        assert_eq!(decoded.direction, Direction::Up);
    }

    #[test]
    fn out_of_range_ordinal_panics_with_java_aioobe_message() {
        let codec = player_action_codec();
        let mut input = buf();
        input.write_var_int(8);
        input.write_long(0);
        input.write_byte(0);
        input.write_var_int(0);
        let msg = panic_message(|| {
            let _ = codec.decode(&mut input);
        });
        assert_eq!(msg, "Index 8 out of bounds for length 8");
    }

    #[test]
    fn getters_expose_fields() {
        let packet = ServerboundPlayerActionPacket::new(
            Action::Stab,
            BlockPos::new(4, 5, 6),
            Direction::East,
            42,
        );
        assert_eq!(packet.get_action(), Action::Stab);
        assert_eq!(packet.get_pos(), BlockPos::new(4, 5, 6));
        assert_eq!(packet.get_direction(), Direction::East);
        assert_eq!(packet.get_sequence(), 42);
        assert_eq!(
            packet.packet_type(),
            PacketType::serverbound("player_action")
        );
    }
}
