//! Port of `net.minecraft.network.protocol.cookie` (issue #86, join-path slice).
//!
//! Java: `net/minecraft/network/protocol/cookie/`. The cookie protocol: the
//! server requests a stored cookie (`ClientboundCookieRequestPacket`), the
//! client answers with the stored payload (`ServerboundCookieResponsePacket`).
//! Registered in play, configuration, and login.

pub mod clientbound_cookie_request;
pub mod packet_types;
pub mod serverbound_cookie_response;
