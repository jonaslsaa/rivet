//! Port of `net.minecraft.network.protocol.login` (issue #99) — the offline
//! login-phase packet bodies.
//!
//! Java: `net/minecraft/network/protocol/login/`. `packet_types` holds the
//! `LoginPacketTypes` discriminators; each body is a value type +
//! `stream_codec()` + `Packet` impl, registered in `LoginProtocols` order (the
//! generated table pins the vanilla ids).
//!
//! The M1 join path needs exactly four bodies: `ServerboundHelloPacket` (id 0),
//! `ClientboundLoginFinishedPacket` (id 2), `ClientboundLoginCompressionPacket`
//! (id 3), and `ServerboundLoginAcknowledgedPacket` (id 3) — the offline
//! `HELLO → VERIFYING → PROTOCOL_SWITCHING → ACK` exchange of
//! `ServerLoginPacketListenerImpl` with `online-mode=false`.
//!
//! Deferred, not stubbed (they are not on the M1 offline join path):
//! RivetTodo(#96): `handle()` on every body — the login/configuration listener
//! state machine (`ServerLoginPacketListenerImpl` offline path) is server-side
//! and unwired here; removing the marker when the login state machine lands.
//! RivetTodo(#88): the RSA online-auth pair (`ClientboundHelloPacket`/
//! `ServerboundKeyPacket`, the `KEY` state) — M1 runs offline
//! (`usesAuthentication()` false), so `handleHello` goes straight to
//! `createOfflineProfile`; the cipher/RSA work is #88.
//! RivetTodo(#99): the `login_disconnect`/`custom_query`/`custom_query_answer`
//! bodies are not on the M1 offline join path; they land with their own units.

pub mod clientbound_login_compression_packet;
pub mod clientbound_login_finished_packet;
pub mod packet_types;
pub mod serverbound_hello_packet;
pub mod serverbound_login_acknowledged_packet;
