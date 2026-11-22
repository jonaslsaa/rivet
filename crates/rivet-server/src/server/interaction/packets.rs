//! Paper 26.2 wire codecs and typed packet structs for world interaction.
//!
//! Wire layouts match `working/Paper` commit `0a993450f129c4942c2a9ed45ba047412b4667cf`:
//! - `FriendlyByteBuf.readBlockHitResult` / `writeBlockHitResult`
//! - `ServerboundUseItemOnPacket` (protocol 776, play serverbound ID 66)
//! - `ServerboundUseItemPacket` (protocol 776, play serverbound ID 67)
//! - `ClientboundBlockChangedAckPacket` (protocol 776, play clientbound ID 4)
//! - `ClientboundBlockDestructionPacket` (protocol 776, play clientbound ID 5)
//!
//! `ServerboundPlayerActionPacket` and `ClientboundBlockUpdatePacket` are reused directly
//! from `rivet-protocol`, with a non-panicking decode wrapper for malformed wire frames.

use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
pub use rivet_protocol::game::serversbound_player_action_packet::{
    Action as PlayerAction, ServerboundPlayerActionPacket,
};
pub use rivet_protocol::protocol::game::clientbound_block_updates::ClientboundBlockUpdatePacket;
use rivet_registry::core::{BlockPos, Direction, Vec3};
use thiserror::Error;

/// `net.minecraft.world.InteractionHand` — declaration order matches Java ordinals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum InteractionHand {
    MainHand = 0,
    OffHand = 1,
}

impl InteractionHand {
    /// Converts a wire VarInt ordinal to `InteractionHand`.
    pub fn from_ordinal(ordinal: i32) -> Result<Self, InteractionPacketError> {
        match ordinal {
            0 => Ok(Self::MainHand),
            1 => Ok(Self::OffHand),
            other => Err(InteractionPacketError::InvalidHandOrdinal(other)),
        }
    }

    /// Java `Enum.ordinal()`.
    pub fn ordinal(self) -> i32 {
        self as i32
    }
}

/// `net.minecraft.world.phys.BlockHitResult` wire projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockHitResult {
    pub location: Vec3,
    pub direction: Direction,
    pub block_pos: BlockPos,
    pub inside: bool,
    pub world_border_hit: bool,
}

impl BlockHitResult {
    /// `new BlockHitResult(Vec3 location, Direction direction, BlockPos pos, boolean inside, boolean worldBorderHit)`.
    pub fn new(
        location: Vec3,
        direction: Direction,
        block_pos: BlockPos,
        inside: bool,
        world_border_hit: bool,
    ) -> Self {
        Self {
            location,
            direction,
            block_pos,
            inside,
            world_border_hit,
        }
    }
}

/// `net.minecraft.network.protocol.game.ServerboundUseItemOnPacket`.
///
/// Wire body: `VarInt hand`, `BlockHitResult blockHit`, `VarInt sequence`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServerboundUseItemOnPacket {
    pub hand: InteractionHand,
    pub hit_result: BlockHitResult,
    pub sequence: i32,
}

impl ServerboundUseItemOnPacket {
    pub fn new(hand: InteractionHand, hit_result: BlockHitResult, sequence: i32) -> Self {
        Self {
            hand,
            hit_result,
            sequence,
        }
    }

    /// Encodes the packet body matching `ServerboundUseItemOnPacket.write`.
    pub fn encode(&self, output: &mut FriendlyByteBuf) {
        output.write_var_int(self.hand.ordinal());
        write_block_hit_result(output, &self.hit_result);
        output.write_var_int(self.sequence);
    }

    /// Decodes the packet body matching `ServerboundUseItemOnPacket(FriendlyByteBuf)`.
    pub fn decode(input: &mut FriendlyByteBuf) -> Result<Self, InteractionPacketError> {
        let hand = InteractionHand::from_ordinal(read_var_int_checked(input)?)?;
        let hit_result = read_block_hit_result(input)?;
        let sequence = read_var_int_checked(input)?;
        Ok(Self {
            hand,
            hit_result,
            sequence,
        })
    }
}

/// `net.minecraft.network.protocol.game.ServerboundUseItemPacket`.
///
/// Wire body: `VarInt hand`, `VarInt sequence`, `f32 yRot`, `f32 xRot`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServerboundUseItemPacket {
    pub hand: InteractionHand,
    pub sequence: i32,
    pub y_rot: f32,
    pub x_rot: f32,
}

impl ServerboundUseItemPacket {
    pub fn new(hand: InteractionHand, sequence: i32, y_rot: f32, x_rot: f32) -> Self {
        Self {
            hand,
            sequence,
            y_rot,
            x_rot,
        }
    }

    /// Encodes the packet body matching `ServerboundUseItemPacket.write`.
    pub fn encode(&self, output: &mut FriendlyByteBuf) {
        output.write_var_int(self.hand.ordinal());
        output.write_var_int(self.sequence);
        output.write_float(self.y_rot);
        output.write_float(self.x_rot);
    }

    /// Decodes the packet body matching `ServerboundUseItemPacket(FriendlyByteBuf)`.
    pub fn decode(input: &mut FriendlyByteBuf) -> Result<Self, InteractionPacketError> {
        let hand = InteractionHand::from_ordinal(read_var_int_checked(input)?)?;
        let sequence = read_var_int_checked(input)?;
        let y_rot = read_float_checked(input)?;
        let x_rot = read_float_checked(input)?;
        Ok(Self {
            hand,
            sequence,
            y_rot,
            x_rot,
        })
    }
}

/// `net.minecraft.network.protocol.game.ClientboundBlockChangedAckPacket`.
///
/// Wire body: `VarInt sequence`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientboundBlockChangedAckPacket {
    pub sequence: i32,
}

impl ClientboundBlockChangedAckPacket {
    pub fn new(sequence: i32) -> Self {
        Self { sequence }
    }

    /// Encodes the packet body matching `ClientboundBlockChangedAckPacket.write`.
    pub fn encode(&self, output: &mut FriendlyByteBuf) {
        output.write_var_int(self.sequence);
    }

    /// Decodes the packet body matching `ClientboundBlockChangedAckPacket(FriendlyByteBuf)`.
    pub fn decode(input: &mut FriendlyByteBuf) -> Result<Self, InteractionPacketError> {
        Ok(Self {
            sequence: read_var_int_checked(input)?,
        })
    }
}

/// `net.minecraft.network.protocol.game.ClientboundBlockDestructionPacket`.
///
/// Wire body: `VarInt id`, `i64 packed BlockPos`, `i8 progress`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientboundBlockDestructionPacket {
    pub id: i32,
    pub pos: BlockPos,
    pub progress: i8,
}

impl ClientboundBlockDestructionPacket {
    /// `new ClientboundBlockDestructionPacket(int id, BlockPos pos, int progress)`.
    ///
    /// Java stores `progress` as `int` and writes `(byte) progress`.
    pub fn new(id: i32, pos: BlockPos, progress: i32) -> Self {
        Self {
            id,
            pos,
            progress: progress as i8,
        }
    }

    /// Encodes the packet body matching `ClientboundBlockDestructionPacket.write`.
    pub fn encode(&self, output: &mut FriendlyByteBuf) {
        output.write_var_int(self.id);
        output.write_long(self.pos.as_long());
        output.write_byte(self.progress);
    }

    /// Decodes the packet body matching `ClientboundBlockDestructionPacket(FriendlyByteBuf)`.
    pub fn decode(input: &mut FriendlyByteBuf) -> Result<Self, InteractionPacketError> {
        let id = read_var_int_checked(input)?;
        let pos = BlockPos::of_long(read_long_checked(input)?);
        let progress = read_byte_checked(input)?;
        Ok(Self { id, pos, progress })
    }
}

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

fn read_var_int_checked(input: &mut FriendlyByteBuf) -> Result<i32, InteractionPacketError> {
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

fn read_byte_checked(input: &mut FriendlyByteBuf) -> Result<i8, InteractionPacketError> {
    if input.readable_bytes() < 1 {
        return Err(InteractionPacketError::UnexpectedEof { field: "byte" });
    }
    Ok(input.read_byte())
}

fn read_bool_checked(input: &mut FriendlyByteBuf) -> Result<bool, InteractionPacketError> {
    if input.readable_bytes() < 1 {
        return Err(InteractionPacketError::UnexpectedEof { field: "boolean" });
    }
    Ok(input.read_boolean())
}

fn read_long_checked(input: &mut FriendlyByteBuf) -> Result<i64, InteractionPacketError> {
    if input.readable_bytes() < 8 {
        return Err(InteractionPacketError::UnexpectedEof { field: "long" });
    }
    Ok(input.read_long())
}

fn read_float_checked(input: &mut FriendlyByteBuf) -> Result<f32, InteractionPacketError> {
    if input.readable_bytes() < 4 {
        return Err(InteractionPacketError::UnexpectedEof { field: "float" });
    }
    Ok(input.read_float())
}
