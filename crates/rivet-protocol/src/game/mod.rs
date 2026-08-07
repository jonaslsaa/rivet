//! Port of `net.minecraft.network.protocol.game` — the game protocol packet
//! bodies (issues #97, #90). One module per Java class, mirroring the package
//! path (`net.minecraft.network.protocol.game` -> `rivet_protocol::game`).
//!
//! The **serverbound** play packets (`mc.network.protocol.game.serverbound`,
//! MANIFEST line 263): bodies, `STREAM_CODEC`s, and `Packet::packet_type()`.
//! Translation fidelity per PORTING.md: exact field order, flags, enum/error
//! behavior, and raw float/double values.
//!
//! The **clientbound entity-pairing** packets (#90, the `mc.network.syncher` +
//! `mc.network.protocol.game` slice proven by the #153 join fixture):
//! `entity_event` (cb 34), `set_entity_data` (cb 99, syncher-backed), and
//! `update_attributes` (cb 131) are fully ported with byte-exact capture tests.
//! The four entity packets that never occur in the single-player fixture
//! (`add_entity`, `remove_entities`, `set_passengers`, `teleport_entity`) are
//! blocked with `STUB(mc.network.protocol.game)` markers per #90.
//!
//! `handle` (listener dispatch) is a documented STUB: the `Packet` trait has no
//! `handle` yet (see `protocol/packet.rs`), deferred with the
//! `ServerGamePacketListener` hierarchy. Authoritative movement handling
//! (`clamp*`, `movedWrongly`, `movedTooQuickly`, gravity kick, teleport
//! id-matching) is #158 / M3 and lives in `rivet-server`/`rivet-world`, not
//! here.

pub mod serversbound_accept_teleportation_packet;
pub mod serversbound_chunk_batch_received_packet;
pub mod serversbound_client_command_packet;
pub mod serversbound_client_tick_end_packet;
pub mod serversbound_move_player_packet;
pub mod serversbound_player_action_packet;

// #90 clientbound entity-pairing packets. The three capture-proven bodies are
// fully ported; the four absent ones carry blocked-codec STUBs.
pub mod clientbound_add_entity_packet;
pub mod clientbound_entity_event_packet;
pub mod clientbound_remove_entities_packet;
pub mod clientbound_set_entity_data_packet;
pub mod clientbound_set_passengers_packet;
pub mod clientbound_teleport_entity_packet;
pub mod clientbound_update_attributes_packet;

pub use serversbound_accept_teleportation_packet::ServerboundAcceptTeleportationPacket;
pub use serversbound_chunk_batch_received_packet::ServerboundChunkBatchReceivedPacket;
pub use serversbound_client_command_packet::ServerboundClientCommandPacket;
pub use serversbound_client_tick_end_packet::ServerboundClientTickEndPacket;
pub use serversbound_move_player_packet::ServerboundMovePlayerPacket;
pub use serversbound_player_action_packet::ServerboundPlayerActionPacket;

pub use clientbound_add_entity_packet::ClientboundAddEntityPacket;
pub use clientbound_entity_event_packet::ClientboundEntityEventPacket;
pub use clientbound_remove_entities_packet::ClientboundRemoveEntitiesPacket;
pub use clientbound_set_entity_data_packet::ClientboundSetEntityDataPacket;
pub use clientbound_set_passengers_packet::ClientboundSetPassengersPacket;
pub use clientbound_teleport_entity_packet::ClientboundTeleportEntityPacket;
pub use clientbound_update_attributes_packet::{
    AttributeModifier, AttributeSnapshot, ClientboundUpdateAttributesPacket, Operation,
};
