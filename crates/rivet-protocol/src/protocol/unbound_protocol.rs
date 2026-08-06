//! Port of `net.minecraft.network.protocol.UnboundProtocol` (MC 26.2).
//!
//! Java: `UnboundProtocol.java` in `working/Paper` (vanilla 26.2). The
//! context protocol template (`contextServerboundProtocol`/`contextClientboundProtocol`),
//! used by the play protocols whose packet codecs depend on a per-connection
//! context (`GameProtocols.Context`, `RegistryFriendlyByteBuf`).
//!
//! The context-dependent half — `CodecModifier` and the 3-arg
//! `ProtocolInfoBuilder.addPacket(type, codec, modifier)` that consumes it — is
//! deferred with the registry-wired codecs (#126, #109): a modifier adapts a
//! codec at bind time and needs `RegistryFriendlyByteBuf`. Until then the
//! packet codecs registered through a context builder are context-independent,
//! so the dispatch codec is built when the template is created and `bind`
//! ignores the context (documented: this changes when modifiers land).

use crate::protocol_info::{ProtocolDetails, ProtocolInfo};
use std::marker::PhantomData;

/// `net.minecraft.network.protocol.UnboundProtocol<T, B, C>` with
/// `B = FriendlyByteBuf`, `T` erased to the packet value `V`, and the context
/// type `C`.
#[derive(Clone)]
pub struct UnboundProtocol<V: 'static, C> {
    protocol_info: ProtocolInfo<V>,
    details: ProtocolDetails,
    /// `C` appears only here (and in [`UnboundProtocol::bind`]); keeping it as a
    /// `fn(C)` phantom avoids a spurious `C: Clone` bound on the derived `Clone`.
    _context: PhantomData<fn(C)>,
}

impl<V: 'static, C> UnboundProtocol<V, C> {
    /// Wraps the bound protocol plus its registration details.
    pub(crate) fn new(protocol_info: ProtocolInfo<V>, details: ProtocolDetails) -> Self {
        UnboundProtocol {
            protocol_info,
            details,
            _context: PhantomData,
        }
    }

    /// `bind(Function<ByteBuf, B> contextWrapper, C context)` — the bound
    /// [`ProtocolInfo`]. The context wrapper is absorbed (as in
    /// [`crate::protocol::SimpleUnboundProtocol`]); the context is unused until
    /// `CodecModifier` lands (see the module doc).
    pub fn bind(&self, _context: C) -> ProtocolInfo<V> {
        self.protocol_info.clone()
    }

    /// `ProtocolInfo.DetailsProvider.details()` — the `(PacketType, networkId)`
    /// registration table in `addPacket` order.
    pub fn details(&self) -> &ProtocolDetails {
        &self.details
    }
}
