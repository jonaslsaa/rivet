//! Paper 26.2 `net.minecraft.network.protocol.game.ClientboundBlockChangedAckPacket` port.

use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;

use super::packets::{InteractionPacketError, read_var_int_checked};

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
