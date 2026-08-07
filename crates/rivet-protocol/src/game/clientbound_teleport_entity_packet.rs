//! Port of `net.minecraft.network.protocol.game.ClientboundTeleportEntityPacket`
//! (MC 26.2) — `teleport_entity` (play clientbound id 125).
//!
//! Blocked (see the codec marker below): this packet never occurs in the #153
//! single-player join fixture (no other entities spawn), so its codec cannot be
//! validated byte-for-byte against the capture — #90 blocks non-join entity
//! packets with a blocked note. The wire body (VarInt id, `PositionMoveRotation`
//! = 3 doubles + 3 doubles + 2 floats, a 4-byte big-endian `Relative` bitfield
//! int, a boolean onGround) is straightforward but the `PositionMoveRotation`/
//! `Relative` value types belong to the entity unit; the codec is NOT
//! implemented until a fixture proves it.
//!
//! Java source: `.../network/protocol/game/ClientboundTeleportEntityPacket.java`.
//! `handle` is a documented STUB like the serverbound slice.

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;

/// `ClientboundTeleportEntityPacket` — the teleport destination. STUB (blocked
/// note above); the struct shape is declared for id stability.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientboundTeleportEntityPacket {
    /// The entity id.
    pub id: i32,
    /// The `PositionMoveRotation` — position (3 doubles), delta movement (3
    /// doubles), y rot + x rot (2 floats).
    pub position: [f64; 3],
    pub delta: [f64; 3],
    pub y_rot: f32,
    pub x_rot: f32,
    /// The `Relative` bitfield (packed int).
    pub relatives: i32,
    pub on_ground: bool,
}

impl ClientboundTeleportEntityPacket {
    /// `teleport(int id, PositionMoveRotation, Set<Relative>, boolean onGround)`.
    pub fn teleport(
        id: i32,
        position: [f64; 3],
        delta: [f64; 3],
        y_rot: f32,
        x_rot: f32,
        relatives: i32,
        on_ground: bool,
    ) -> Self {
        ClientboundTeleportEntityPacket {
            id,
            position,
            delta,
            y_rot,
            x_rot,
            relatives,
            on_ground,
        }
    }
}

impl Packet for ClientboundTeleportEntityPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::clientbound("teleport_entity")
    }
}

/// STUB(mc.network.protocol.game): no capture proves this body yet — the codec
/// is blocked (see the module doc).
pub fn teleport_entity_codec() -> StreamCodec<FriendlyByteBuf, ClientboundTeleportEntityPacket> {
    codec(
        |_value, _output| {
            panic!("blocked: teleport_entity codec not ported (#90; no join fixture)")
        },
        |_input| panic!("blocked: teleport_entity codec not ported (#90; no join fixture)"),
    )
}
