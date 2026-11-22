//! Paper 26.2 wire codec helpers and non-panicking frame decoders for world interaction.
//!
//! Wire layouts match `working/Paper` commit `0a993450f129c4942c2a9ed45ba047412b4667cf`:
//! - `FriendlyByteBuf.readBlockHitResult` / `writeBlockHitResult`
//! - `ServerboundPlayerActionPacket` (non-panicking decode wrapper)
//! - `ServerboundUseItemOnPacket` / `ServerboundUseItemPacket` (trailing-byte checking wrappers)

use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
pub use rivet_protocol::game::serversbound_player_action_packet::{
    Action as PlayerAction, ServerboundPlayerActionPacket,
};
pub use rivet_protocol::protocol::game::clientbound_block_updates::ClientboundBlockUpdatePacket;
use rivet_registry::core::{BlockPos, Direction, Vec3};
use thiserror::Error;

pub use super::block_hit_result::BlockHitResult;
pub use super::clientbound_block_changed_ack_packet::ClientboundBlockChangedAckPacket;
pub use super::clientbound_block_destruction_packet::ClientboundBlockDestructionPacket;
pub use super::interaction_hand::InteractionHand;
pub use super::serverbound_use_item_on_packet::ServerboundUseItemOnPacket;
pub use super::serverbound_use_item_packet::ServerboundUseItemPacket;

/// Wire decode errors for malformed serverbound interaction packets.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InteractionPacketError {
    #[error("unexpected end of buffer while decoding {field}")]
    UnexpectedEof { field: &'static str },
    #[error("varint exceeded 5 bytes")]
    VarIntTooBig,
    #[error("invalid InteractionHand ordinal {0}")]
    InvalidHandOrdinal(i32),
    #[error("invalid Direction ordinal {0}")]
    InvalidDirectionOrdinal(i32),
    #[error("invalid ServerboundPlayerActionPacket.Action ordinal {0}")]
    InvalidActionOrdinal(i32),
    #[error("trailing bytes ({remaining}) after decoding packet")]
    TrailingBytes { remaining: usize },
}

/// Non-panicking decoder for `ServerboundPlayerActionPacket`.
///
/// Matches `ServerboundPlayerActionPacket`'s wire format (`VarInt action`, `i64 pos`,
/// `u8 direction`, `VarInt sequence`) while returning `Err` instead of panicking on
/// truncated buffers or out-of-range enum ordinals.
pub fn decode_player_action(
    input: &mut FriendlyByteBuf,
) -> Result<ServerboundPlayerActionPacket, InteractionPacketError> {
    let action_ordinal = read_var_int_checked(input)?;
    let action = match action_ordinal {
        0 => PlayerAction::StartDestroyBlock,
        1 => PlayerAction::AbortDestroyBlock,
        2 => PlayerAction::StopDestroyBlock,
        3 => PlayerAction::DropAllItems,
        4 => PlayerAction::DropItem,
        5 => PlayerAction::ReleaseUseItem,
        6 => PlayerAction::SwapItemWithOffhand,
        7 => PlayerAction::Stab,
        other => return Err(InteractionPacketError::InvalidActionOrdinal(other)),
    };
    let pos = BlockPos::of_long(read_long_checked(input)?);
    let raw_dir = read_byte_checked(input)? as u8;
    let direction = Direction::from_3d_data_value(raw_dir as i32);
    let sequence = read_var_int_checked(input)?;
    if input.readable_bytes() != 0 {
        return Err(InteractionPacketError::TrailingBytes {
            remaining: input.readable_bytes(),
        });
    }
    Ok(ServerboundPlayerActionPacket::new(
        action, pos, direction, sequence,
    ))
}

/// Non-panicking decoder for `ServerboundUseItemOnPacket` that also rejects trailing bytes.
pub fn decode_use_item_on(
    input: &mut FriendlyByteBuf,
) -> Result<ServerboundUseItemOnPacket, InteractionPacketError> {
    let packet = ServerboundUseItemOnPacket::decode(input)?;
    if input.readable_bytes() != 0 {
        return Err(InteractionPacketError::TrailingBytes {
            remaining: input.readable_bytes(),
        });
    }
    Ok(packet)
}

/// Non-panicking decoder for `ServerboundUseItemPacket` that also rejects trailing bytes.
pub fn decode_use_item(
    input: &mut FriendlyByteBuf,
) -> Result<ServerboundUseItemPacket, InteractionPacketError> {
    let packet = ServerboundUseItemPacket::decode(input)?;
    if input.readable_bytes() != 0 {
        return Err(InteractionPacketError::TrailingBytes {
            remaining: input.readable_bytes(),
        });
    }
    Ok(packet)
}

/// `FriendlyByteBuf.readBlockHitResult()`.
pub fn read_block_hit_result(
    input: &mut FriendlyByteBuf,
) -> Result<BlockHitResult, InteractionPacketError> {
    let pos = BlockPos::of_long(read_long_checked(input)?);
    let dir_ordinal = read_var_int_checked(input)?;
    let direction = match dir_ordinal {
        0 => Direction::Down,
        1 => Direction::Up,
        2 => Direction::North,
        3 => Direction::South,
        4 => Direction::West,
        5 => Direction::East,
        other => return Err(InteractionPacketError::InvalidDirectionOrdinal(other)),
    };
    let click_x = read_float_checked(input)?;
    let click_y = read_float_checked(input)?;
    let click_z = read_float_checked(input)?;
    let inside = read_bool_checked(input)?;
    let world_border_hit = read_bool_checked(input)?;
    let location = Vec3::new(
        (pos.get_x() as f64) + (click_x as f64),
        (pos.get_y() as f64) + (click_y as f64),
        (pos.get_z() as f64) + (click_z as f64),
    );
    Ok(BlockHitResult::new(
        location,
        direction,
        pos,
        inside,
        world_border_hit,
    ))
}

/// `FriendlyByteBuf.writeBlockHitResult(BlockHitResult)`.
pub fn write_block_hit_result(output: &mut FriendlyByteBuf, hit: &BlockHitResult) {
    output.write_long(hit.block_pos.as_long());
    output.write_var_int(hit.direction.get_3d_data_value());
    output.write_float((hit.location.x - (hit.block_pos.get_x() as f64)) as f32);
    output.write_float((hit.location.y - (hit.block_pos.get_y() as f64)) as f32);
    output.write_float((hit.location.z - (hit.block_pos.get_z() as f64)) as f32);
    output.write_boolean(hit.inside);
    output.write_boolean(hit.world_border_hit);
}

pub(super) fn read_var_int_checked(
    input: &mut FriendlyByteBuf,
) -> Result<i32, InteractionPacketError> {
    let mut result: i32 = 0;
    let mut shift: u32 = 0;
    for _ in 0..5 {
        if input.readable_bytes() == 0 {
            return Err(InteractionPacketError::UnexpectedEof { field: "varint" });
        }
        let byte = input.read_byte() as u8;
        result |= ((byte & 0x7F) as i32) << shift;
        if (byte & 0x80) == 0 {
            return Ok(result);
        }
        shift += 7;
    }
    Err(InteractionPacketError::VarIntTooBig)
}

pub(super) fn read_byte_checked(input: &mut FriendlyByteBuf) -> Result<i8, InteractionPacketError> {
    if input.readable_bytes() < 1 {
        return Err(InteractionPacketError::UnexpectedEof { field: "byte" });
    }
    Ok(input.read_byte())
}

pub(super) fn read_bool_checked(
    input: &mut FriendlyByteBuf,
) -> Result<bool, InteractionPacketError> {
    if input.readable_bytes() < 1 {
        return Err(InteractionPacketError::UnexpectedEof { field: "boolean" });
    }
    Ok(input.read_boolean())
}

pub(super) fn read_long_checked(
    input: &mut FriendlyByteBuf,
) -> Result<i64, InteractionPacketError> {
    if input.readable_bytes() < 8 {
        return Err(InteractionPacketError::UnexpectedEof { field: "long" });
    }
    Ok(input.read_long())
}

pub(super) fn read_float_checked(
    input: &mut FriendlyByteBuf,
) -> Result<f32, InteractionPacketError> {
    if input.readable_bytes() < 4 {
        return Err(InteractionPacketError::UnexpectedEof { field: "float" });
    }
    Ok(input.read_float())
}
