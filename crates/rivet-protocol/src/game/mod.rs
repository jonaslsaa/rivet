//! Port of `net.minecraft.network.protocol.game` — the game protocol packet
//! bodies (issue #97). One module per Java class, mirroring the package path
//! (`net.minecraft.network.protocol.game` -> `rivet_protocol::game`).
//!
//! This slice covers the join-critical **serverbound** play packets
//! (`mc.network.protocol.game.serverbound`, MANIFEST line 263): bodies,
//! `STREAM_CODEC`s, and `Packet::packet_type()`. Translation fidelity per
//! PORTING.md: exact field order, flags, enum/error behavior, and raw
//! float/double values.
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

pub use serversbound_accept_teleportation_packet::ServerboundAcceptTeleportationPacket;
pub use serversbound_chunk_batch_received_packet::ServerboundChunkBatchReceivedPacket;
pub use serversbound_client_command_packet::ServerboundClientCommandPacket;
pub use serversbound_client_tick_end_packet::ServerboundClientTickEndPacket;
pub use serversbound_move_player_packet::ServerboundMovePlayerPacket;
pub use serversbound_player_action_packet::ServerboundPlayerActionPacket;
