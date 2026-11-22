//! Paper 26.2 `net.minecraft.network.protocol.game.ServerboundUseItemOnPacket` port.

use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;

use super::block_hit_result::BlockHitResult;
use super::interaction_hand::InteractionHand;
use super::packets::{
    InteractionPacketError, read_block_hit_result, read_var_int_checked, write_block_hit_result,
};

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
