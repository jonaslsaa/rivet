//! Port of `net.minecraft.network.ProtocolInfo` (MC 26.2).
//!
//! Java: `ProtocolInfo.java` in `working/Paper` (vanilla 26.2). A bound
//! protocol: the connection state, the packet flow, the id-dispatch codec over
//! the packet value type, and the optional bundle info. Produced by a
//! template's `bind` ([`crate::protocol::SimpleUnboundProtocol`] /
//! [`crate::protocol::UnboundProtocol`]).
//!
//! The Java interface also nests `Details` (`id()`/`flow()`/`listPackets(...)`),
//! exposed through the templates' `ProtocolInfo.DetailsProvider`. That is
//! [`ProtocolDetails`] here, returned by the templates (matching Java: the
//! *bound* [`ProtocolInfo`] does not carry the packet list; the template does).

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::generated::protocol::{ConnectionProtocol, PacketFlow};
use crate::protocol::bundle::BundlerInfo;
use crate::protocol::packet_type::PacketType;

/// `net.minecraft.network.ProtocolInfo<T>` — the bound protocol value.
///
/// `V` is the erased packet value type for this state/direction (Java's
/// `Packet<? super T>`). `Clone` is cheap: the codec is an `Arc`ed
/// [`StreamCodec`], so a template can be bound many times (once per connection)
/// with one registration.
pub struct ProtocolInfo<V: 'static> {
    id: ConnectionProtocol,
    flow: PacketFlow,
    codec: StreamCodec<FriendlyByteBuf, V>,
    bundler_info: Option<BundlerInfo>,
}

/// Manual `Clone` (not derived): derived would impose `V: Clone` (the `Arc`ed
/// codec is cloneable for any `V`, matching `StreamCodec`'s own manual impl).
impl<V: 'static> Clone for ProtocolInfo<V> {
    fn clone(&self) -> Self {
        ProtocolInfo {
            id: self.id,
            flow: self.flow,
            codec: self.codec.clone(),
            bundler_info: self.bundler_info.clone(),
        }
    }
}

impl<V: 'static> ProtocolInfo<V> {
    /// Builds the bound value (`ProtocolInfoBuilder.buildUnbound`'s anonymous
    /// `Implementation`).
    pub(crate) fn new(
        id: ConnectionProtocol,
        flow: PacketFlow,
        codec: StreamCodec<FriendlyByteBuf, V>,
        bundler_info: Option<BundlerInfo>,
    ) -> Self {
        ProtocolInfo {
            id,
            flow,
            codec,
            bundler_info,
        }
    }

    /// `ProtocolInfo.id()` — the connection state.
    pub const fn id(&self) -> ConnectionProtocol {
        self.id
    }

    /// `ProtocolInfo.flow()` — the packet direction.
    pub const fn flow(&self) -> PacketFlow {
        self.flow
    }

    /// `ProtocolInfo.codec()` — the id-dispatch codec. A `varint packet id`
    /// prefix selects the per-packet codec (see [`crate::codec::IdDispatchCodec`]).
    pub fn codec(&self) -> &StreamCodec<FriendlyByteBuf, V> {
        &self.codec
    }

    /// `ProtocolInfo.bundlerInfo()` — `@Nullable` in Java; `None` when the
    /// protocol does not bundle (everything except play/clientbound).
    pub fn bundler_info(&self) -> Option<&BundlerInfo> {
        self.bundler_info.as_ref()
    }
}

/// `net.minecraft.network.ProtocolInfo.Details` — the registration table a
/// template exposes: `(PacketType, networkId)` pairs in `addPacket` order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolDetails {
    id: ConnectionProtocol,
    flow: PacketFlow,
    packets: Vec<(PacketType, u32)>,
}

impl ProtocolDetails {
    /// Builds the details (`ProtocolInfoBuilder.buildDetails`).
    pub(crate) fn new(
        id: ConnectionProtocol,
        flow: PacketFlow,
        packets: Vec<(PacketType, u32)>,
    ) -> Self {
        ProtocolDetails { id, flow, packets }
    }

    /// `Details.id()`.
    pub const fn id(&self) -> ConnectionProtocol {
        self.id
    }

    /// `Details.flow()`.
    pub const fn flow(&self) -> PacketFlow {
        self.flow
    }

    /// `Details.listPackets(PacketVisitor)` — every registered `(PacketType,
    /// networkId)` in `addPacket` order; the network id is the `addPacket` index
    /// (the same order `IdDispatchCodec` assigns ids).
    pub fn list_packets(&self) -> &[(PacketType, u32)] {
        &self.packets
    }
}
