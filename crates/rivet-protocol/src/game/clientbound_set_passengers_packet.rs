//! Port of `net.minecraft.network.protocol.game.ClientboundSetPassengersPacket`
//! (MC 26.2) — `set_passengers` (play clientbound id 107).
//!
//! Blocked (see the codec marker below): this packet never occurs in the #153
//! single-player join fixture (no other entities spawn), so its codec cannot be
//! validated byte-for-byte against the capture — #90 blocks non-join entity
//! packets with a blocked note. The wire body is simple enough to port now
//! (`writeVarInt(vehicle)` + `writeVarIntArray(passengers)`) but the DoD demands
//! capture-proven bodies, so the codec is NOT implemented until a fixture proves
//! it.
//!
//! Java source: `.../network/protocol/game/ClientboundSetPassengersPacket.java`.
//! `handle` is a documented STUB like the serverbound slice.

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;

/// `ClientboundSetPassengersPacket` — the vehicle and its passenger ids. STUB
/// (blocked note above); the struct shape is declared for id stability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientboundSetPassengersPacket {
    /// The vehicle entity id.
    pub vehicle: i32,
    /// The passenger entity ids (wire: `writeVarIntArray`).
    pub passengers: Vec<i32>,
}

impl ClientboundSetPassengersPacket {
    /// `new ClientboundSetPassengersPacket(Entity vehicle)` — the entity
    /// constructor (ids taken from the vehicle + its passengers).
    pub fn new(vehicle: i32, passengers: Vec<i32>) -> Self {
        ClientboundSetPassengersPacket {
            vehicle,
            passengers,
        }
    }

    /// `getPassengers()`.
    pub fn get_passengers(&self) -> &[i32] {
        &self.passengers
    }

    /// `getVehicle()`.
    pub fn get_vehicle(&self) -> i32 {
        self.vehicle
    }
}

impl Packet for ClientboundSetPassengersPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::clientbound("set_passengers")
    }
}

/// STUB(mc.network.protocol.game): no capture proves this body yet — the codec
/// is blocked (see the module doc).
pub fn set_passengers_codec() -> StreamCodec<FriendlyByteBuf, ClientboundSetPassengersPacket> {
    codec(
        |_value, _output| panic!("blocked: set_passengers codec not ported (#90; no join fixture)"),
        |_input| panic!("blocked: set_passengers codec not ported (#90; no join fixture)"),
    )
}
