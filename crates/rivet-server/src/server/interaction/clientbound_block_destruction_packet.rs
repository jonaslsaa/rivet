//! Paper 26.2 `net.minecraft.network.protocol.game.ClientboundBlockDestructionPacket` port.

use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_registry::core::BlockPos;

use super::packets::{
    InteractionPacketError, read_byte_checked, read_long_checked, read_var_int_checked,
};

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
