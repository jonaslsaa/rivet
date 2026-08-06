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
//! The context-dependent `CodecModifier` overload is deferred with the
//! registry-wired codecs (#126/#109) — documented in `protocol_info_builder`.
//! `PacketUtils` (thread-confinement helpers) is server-side and deferred with
//! the state machines.

pub mod bundle;
pub mod packet;
pub mod packet_type;
pub mod protocol_codec_builder;
pub mod protocol_info_builder;
pub mod simple_unbound_protocol;
pub mod unbound_protocol;

pub use bundle::{BundleDelimiterPacket, BundlePacket, BundlerInfo};
pub use packet::{Packet, codec};
pub use packet_type::PacketType;
pub use protocol_codec_builder::ProtocolCodecBuilder;
pub use protocol_info_builder::{
    ProtocolInfoBuilder, clientbound_protocol, context_clientbound_protocol,
    context_serverbound_protocol, serverbound_protocol,
};
pub use simple_unbound_protocol::SimpleUnboundProtocol;
pub use unbound_protocol::UnboundProtocol;
