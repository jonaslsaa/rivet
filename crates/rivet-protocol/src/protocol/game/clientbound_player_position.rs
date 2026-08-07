//! Port of `net.minecraft.network.protocol.game.ClientboundPlayerPositionPacket`
//! (issue #87) — `player_position` (play clientbound id 72).
//!
//! Java source: `.../network/protocol/game/ClientboundPlayerPositionPacket.java`.
//! Wire body: `VAR_INT` `id`, `PositionMoveRotation.STREAM_CODEC`, then
//! `Relative.SET_STREAM_CODEC` — a big-endian `int` bitmask of the nine
//! `Relative` flags. The captured golden body (`join_clientbound_player_position.hex`,
//! 61 bytes) carries `id 0`, position `(0.0, -63.0, 0.0)`, zero
//! `deltaMovement`, `yRot/xRot 0.0`, and an empty `relatives` set.
//!
//! `Relative` lives in `rivet-registry::core` (the value type), and its
//! `SET_STREAM_CODEC` — `ByteBufCodecs.INT.map(Relative::unpack, Relative::pack)`
//! — composes the four-byte bitmask. The relative flag set ports as a `Vec` in
//! the enum's declaration order (Java's `EnumSet` iteration is that same
//! order), preserving deterministic encode.

use crate::codec::byte_buf_codecs::{int, var_int};
use crate::codec::{StreamCodec, composite_3, map};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_player_position;
use crate::protocol::game::position_move_rotation::PositionMoveRotation;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use rivet_registry::core::Relative;

/// `ClientboundPlayerPositionPacket` — the record `(int id, PositionMoveRotation
/// change, Set<Relative> relatives)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientboundPlayerPositionPacket {
    /// `id`.
    id: i32,
    /// `change`.
    change: PositionMoveRotation,
    /// `relatives`.
    relatives: Vec<Relative>,
}

impl ClientboundPlayerPositionPacket {
    /// The record's canonical constructor (`ClientboundPlayerPositionPacket.of`).
    pub fn new(id: i32, change: PositionMoveRotation, relatives: Vec<Relative>) -> Self {
        ClientboundPlayerPositionPacket {
            id,
            change,
            relatives,
        }
    }

    /// `ClientboundPlayerPositionPacket.id()`.
    pub fn id(&self) -> i32 {
        self.id
    }

    /// `ClientboundPlayerPositionPacket.change()`.
    pub fn change(&self) -> &PositionMoveRotation {
        &self.change
    }

    /// `ClientboundPlayerPositionPacket.relatives()`.
    pub fn relatives(&self) -> &[Relative] {
        &self.relatives
    }

    /// `STREAM_CODEC` — `VAR_INT`, `PositionMoveRotation.STREAM_CODEC`,
    /// `Relative.SET_STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundPlayerPositionPacket> {
        composite_3(
            var_int(),
            ClientboundPlayerPositionPacket::id,
            PositionMoveRotation::stream_codec(),
            |packet: &ClientboundPlayerPositionPacket| packet.change,
            relative_set_stream_codec(),
            |packet: &ClientboundPlayerPositionPacket| packet.relatives.clone(),
            ClientboundPlayerPositionPacket::new,
        )
    }
}

/// `Relative.SET_STREAM_CODEC` — `ByteBufCodecs.INT.map(Relative::unpack,
/// Relative::pack)`: a big-endian `int` bitmask.
pub fn relative_set_stream_codec() -> StreamCodec<FriendlyByteBuf, Vec<Relative>> {
    map(
        int(),
        |value: &i32| Relative::unpack(*value),
        |set: &Vec<Relative>| Relative::pack(set),
    )
}

impl Packet for ClientboundPlayerPositionPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_player_position()
    }
}
