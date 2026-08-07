//! Port of `net.minecraft.network.protocol` — the packet registration surface
//! (issue #84), one module per Java class:
//!
//!   Packet                    -> packet (the erased packet value trait)
//!   PacketType                -> packet_type (the `(flow, identifier)` discriminator)
//!   ProtocolInfoBuilder       -> protocol_info_builder (registration order == network id)
//!   ProtocolCodecBuilder      -> protocol_codec_builder (flow check + id dispatch build)
//!   SimpleUnboundProtocol     -> simple_unbound_protocol (non-context template)
//!   UnboundProtocol           -> unbound_protocol (context template)
//!   BundlerInfo/BundlePacket/BundleDelimiterPacket -> bundle (the bundle trio)
//!
//! `PacketFlow` lives in the generated `ConnectionProtocol` tables
//! (`crate::generated::protocol`); `ProtocolInfo`/`ProtocolInfo.Details` are in
//! `net.minecraft.network` and live at `crate::protocol_info`.
//!
//! The context-dependent `CodecModifier` overload defers with the registry-wired
//! codecs — tracked at `protocol_info_builder` module scope (#126).
//! RivetTodo(#148): `PacketUtils` (thread-confinement helpers) is server-side
//! and deferred with the state machines.
//!
//! Packet-body units (issue #86, join-path slice): the crossover bodies shared
//! by play and configuration — [`common`] (`net.minecraft.network.protocol.common`),
//! [`cookie`] (`...protocol.cookie`), [`ping`] (`...protocol.ping`) — and the
//! shared [`stream_codecs`] (e.g. `Identifier.STREAM_CODEC`). Each body is a
//! value type + `stream_codec()` + `Packet` impl (the `PacketType` constants
//! live in each package's `packet_types` module). `handle()` stays deferred with
//! the listener hierarchy; the game.join/serverbound bodies are #148 (M1.1).

pub mod bundle;
pub mod common;
pub mod cookie;
pub mod game;
pub mod packet;
pub mod packet_type;
pub mod ping;
pub mod protocol_codec_builder;
pub mod protocol_info_builder;
pub mod simple_unbound_protocol;
pub mod stream_codecs;
pub mod unbound_protocol;

pub use bundle::{BundleDelimiterPacket, BundlePacket, BundlerInfo};
pub use game::common_player_spawn_info::CommonPlayerSpawnInfo;
pub use packet::{Packet, codec};
pub use packet_type::PacketType;
pub use protocol_codec_builder::ProtocolCodecBuilder;
pub use protocol_info_builder::{
    ProtocolInfoBuilder, clientbound_protocol, context_clientbound_protocol,
    context_serverbound_protocol, serverbound_protocol,
};
pub use simple_unbound_protocol::SimpleUnboundProtocol;
pub use unbound_protocol::UnboundProtocol;
