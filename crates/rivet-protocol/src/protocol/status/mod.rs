//! Port of `net.minecraft.network.protocol.status` (issue #86, status slice).
//!
//! Java: `net/minecraft/network/protocol/status/`. The status protocol: the
//! client sends a `ServerboundStatusRequestPacket`, the server replies with a
//! `ClientboundStatusResponsePacket` (a `ServerStatus` JSON body); the client
//! then pings (see [`super::ping`]) and disconnects.
//!
//! The `ServerStatus` body and its nested `Players`/`Version`/`Favicon`
//! codecs are the same DFU codecs the server uses to serialize its list-ping
//! response, so they live here beside the packet bodies. The status-protocol
//! packet ids (serverbound `status_request`=0, `ping_request`=1; clientbound
//! `status_response`=0, `pong_response`=1) are pinned by the generated
//! `generated::packets::status` tables (the vanilla `StatusProtocols`
//! addPacket order); the status listener in `rivet-server`
//! (`server_status_packet_listener`) drives the framing directly against those
//! ids.

pub mod clientbound_status_response_packet;
pub mod name_and_id;
pub mod packet_types;
pub mod server_status;
pub mod serverbound_status_request_packet;

pub use clientbound_status_response_packet::ClientboundStatusResponsePacket;
pub use name_and_id::NameAndId;
pub use server_status::{Favicon, Players, ServerStatus, Version};
pub use serverbound_status_request_packet::ServerboundStatusRequestPacket;
