//! Port of `net.minecraft.network.protocol.ping` (issue #86, join-path slice).
//!
//! Java: `net/minecraft/network/protocol/ping/`. The ping protocol: the client
//! sends a `ServerboundPingRequestPacket`, the server replies with a
//! `ClientboundPongResponsePacket`. Registered in play and status.

pub mod clientbound_pong_response;
pub mod packet_types;
pub mod serverbound_ping_request;
