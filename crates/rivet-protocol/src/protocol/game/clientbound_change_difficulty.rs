//! Port of `net.minecraft.network.protocol.game.ClientboundChangeDifficultyPacket`
//! (issue #87) — `change_difficulty` (play clientbound id 10).
//!
//! Java source: `.../network/protocol/game/ClientboundChangeDifficultyPacket.java`.
//! Wire body: `Difficulty.STREAM_CODEC` (a VarInt id through the WRAP
//! `BY_ID`), then `ByteBufCodecs.BOOL` `locked`. The captured golden body is
//! `0100` — difficulty id 1 (`EASY`), `locked = false`.

use crate::codec::byte_buf_codecs::{bool, id_mapper};
use crate::codec::{StreamCodec, composite_2};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_change_difficulty;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use rivet_registry::core::Difficulty;

/// `ClientboundChangeDifficultyPacket` — the record `(Difficulty difficulty,
/// boolean locked)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientboundChangeDifficultyPacket {
    /// `difficulty`.
    difficulty: Difficulty,
    /// `locked`.
    locked: bool,
}

impl ClientboundChangeDifficultyPacket {
    /// The record's canonical constructor.
    pub fn new(difficulty: Difficulty, locked: bool) -> Self {
        ClientboundChangeDifficultyPacket { difficulty, locked }
    }

    /// `ClientboundChangeDifficultyPacket.difficulty()`.
    pub fn difficulty(&self) -> Difficulty {
        self.difficulty
    }

    /// `ClientboundChangeDifficultyPacket.locked()`.
    pub fn locked(&self) -> bool {
        self.locked
    }

    /// `STREAM_CODEC` — `Difficulty.STREAM_CODEC`, then `BOOL`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundChangeDifficultyPacket> {
        composite_2(
            difficulty_stream_codec(),
            ClientboundChangeDifficultyPacket::difficulty,
            bool(),
            ClientboundChangeDifficultyPacket::locked,
            ClientboundChangeDifficultyPacket::new,
        )
    }
}

/// `Difficulty.STREAM_CODEC` — `ByteBufCodecs.idMapper(BY_ID, Difficulty::getId)`:
/// a VarInt id decoded through the WRAP `BY_ID`, encoding `getId`.
pub fn difficulty_stream_codec() -> StreamCodec<FriendlyByteBuf, Difficulty> {
    id_mapper(Difficulty::by_id, Difficulty::get_id)
}

impl Packet for ClientboundChangeDifficultyPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_change_difficulty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn captured_golden_body_round_trips() {
        // Capture: `0100` — EASY (id 1), not locked.
        let mut input = FriendlyByteBuf::new(BytesMut::from(vec![0x01, 0x00].as_slice()));
        let decoded = ClientboundChangeDifficultyPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(
            decoded,
            ClientboundChangeDifficultyPacket::new(Difficulty::Easy, false)
        );
        assert_eq!(input.readable_bytes(), 0);

        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundChangeDifficultyPacket::stream_codec()
            .encode(&mut out, &decoded)
            .unwrap();
        assert_eq!(out.as_slice().to_vec(), vec![0x01, 0x00]);
    }

    #[test]
    fn out_of_range_difficulty_id_wraps() {
        // WRAP: `Mth.positiveModulo(id, 4)` — id 5 -> index 1 -> EASY.
        let mut input = FriendlyByteBuf::new(BytesMut::from(vec![0x05, 0x00].as_slice()));
        let decoded = ClientboundChangeDifficultyPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded.difficulty(), Difficulty::Easy);
    }
}
