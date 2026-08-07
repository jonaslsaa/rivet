//! Port of `net.minecraft.network.protocol.game.ClientboundAddEntityPacket`
//! (MC 26.2) — `add_entity` (play clientbound id 1).
//!
//! Blocked (see the codec marker below): this packet never occurs in the #153
//! single-player join fixture (no other entities spawn), so its codec cannot be
//! validated byte-for-byte against the capture — #90 blocks non-join entity
//! packets with a blocked note. The body needs the `EntityType` registry
//! (`ByteBufCodecs.registry(Registries.ENTITY_TYPE)`), `Vec3.LP_STREAM_CODEC`
//! (the quantized movement delta), and rotation packing — all entity/world-layer;
//! the codec is NOT implemented until a fixture proves it.
//!
//! Java source: `.../network/protocol/game/ClientboundAddEntityPacket.java`.
//! `handle` is a documented STUB like the serverbound slice.

use crate::codec::StreamCodec;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;
use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_util::mth::Uuid;

/// `ClientboundAddEntityPacket` — the spawn packet. STUB (blocked note above);
/// the struct shape is declared so the packet type and the wire fields are
/// stable when the codec lands.
///
/// The Java record's `type` (`EntityType<?>`, `ByteBufCodecs.registry(Registries.
/// ENTITY_TYPE)`) and `movement` (`Vec3.LP_STREAM_CODEC` — the quantized delta)
/// fields are **omitted**: those value types belong to the entity unit (no
/// `EntityType` registry key/element or exported `Vec3` exists yet), so declaring
/// them would be speculative scaffolding for a codec with no fixture. The three
/// rotations mirror Java's wire form — `Mth.packDegrees` bytes (`writeByte`) —
/// not the unpacked degrees the public record constructor takes.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientboundAddEntityPacket {
    pub id: i32,
    pub uuid: Uuid,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// `Mth.packDegrees` of `xRot` — the wire byte.
    pub x_rot: i8,
    /// `Mth.packDegrees` of `yRot` — the wire byte.
    pub y_rot: i8,
    /// `Mth.packDegrees` of `yHeadRot` — the wire byte.
    pub y_head_rot: i8,
    pub data: i32,
}

impl ClientboundAddEntityPacket {
    /// `new ClientboundAddEntityPacket(...)` — the packed-rotation constructor
    /// (the wire values, as the private decode ctor reads them).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: i32,
        uuid: Uuid,
        x: f64,
        y: f64,
        z: f64,
        x_rot: i8,
        y_rot: i8,
        y_head_rot: i8,
        data: i32,
    ) -> Self {
        ClientboundAddEntityPacket {
            id,
            uuid,
            x,
            y,
            z,
            x_rot,
            y_rot,
            y_head_rot,
            data,
        }
    }
}

impl Packet for ClientboundAddEntityPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::clientbound("add_entity")
    }
}

/// STUB(mc.network.protocol.game): no capture proves this body yet — the codec
/// is blocked (see the module doc).
pub fn add_entity_codec() -> StreamCodec<RegistryFriendlyByteBuf, ClientboundAddEntityPacket> {
    codec(
        |_value, _output| panic!("blocked: add_entity codec not ported (#90; no join fixture)"),
        |_input| panic!("blocked: add_entity codec not ported (#90; no join fixture)"),
    )
}
