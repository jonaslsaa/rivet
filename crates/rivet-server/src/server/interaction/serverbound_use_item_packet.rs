//! Paper 26.2 `net.minecraft.network.protocol.game.ServerboundUseItemPacket` port.

use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;

use super::interaction_hand::InteractionHand;
use super::packets::{InteractionPacketError, read_float_checked, read_var_int_checked};

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
