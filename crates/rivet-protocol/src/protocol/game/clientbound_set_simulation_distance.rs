//! Port of `net.minecraft.network.protocol.game.ClientboundSetSimulationDistancePacket`
//! (MC 26.2) — `set_simulation_distance` (play clientbound id 111).
//!
//! Java source: `.../network/protocol/game/ClientboundSetSimulationDistancePacket.java`.
//! Wire body: `simulationDistance` VarInt. The Moonrise chunk-loader `add` sends
//! this second of the three cache packets (after the cache radius, before the
//! cache center); the captured join body is `04` — distance 4 (the
//! `simulation-distance=4` fixture).

use crate::codec::byte_buf_codecs::var_int;
use crate::codec::{StreamCodec, map};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_set_simulation_distance;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ClientboundSetSimulationDistancePacket` — the record
/// `(int simulationDistance)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientboundSetSimulationDistancePacket {
    /// `simulationDistance`.
    simulation_distance: i32,
}

impl ClientboundSetSimulationDistancePacket {
    /// The record's canonical constructor.
    pub fn new(simulation_distance: i32) -> Self {
        ClientboundSetSimulationDistancePacket {
            simulation_distance,
        }
    }

    /// `ClientboundSetSimulationDistancePacket.simulationDistance()`.
    pub fn simulation_distance(&self) -> i32 {
        self.simulation_distance
    }

    /// `STREAM_CODEC` — `writeVarInt(simulationDistance)`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundSetSimulationDistancePacket> {
        map(
            var_int(),
            |simulation_distance| ClientboundSetSimulationDistancePacket::new(*simulation_distance),
            ClientboundSetSimulationDistancePacket::simulation_distance,
        )
    }
}

impl Packet for ClientboundSetSimulationDistancePacket {
    fn packet_type(&self) -> PacketType {
        clientbound_set_simulation_distance()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn captured_golden_body_round_trips() {
        // Capture: `04` — simulation distance 4 (the `simulation-distance=4`
        // fixture).
        let mut input = FriendlyByteBuf::new(BytesMut::from(vec![0x04].as_slice()));
        let decoded = ClientboundSetSimulationDistancePacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded, ClientboundSetSimulationDistancePacket::new(4));
        assert_eq!(input.readable_bytes(), 0);

        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundSetSimulationDistancePacket::stream_codec()
            .encode(&mut out, &decoded)
            .unwrap();
        assert_eq!(out.as_slice().to_vec(), vec![0x04]);
    }
}
