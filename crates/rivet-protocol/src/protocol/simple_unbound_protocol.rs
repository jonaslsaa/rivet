//! Port of `net.minecraft.network.protocol.SimpleUnboundProtocol` (MC 26.2).
//!
//! Java: `SimpleUnboundProtocol.java` in `working/Paper` (vanilla 26.2). The
//! non-context protocol template: `serverboundProtocol`/`clientboundProtocol`
//! produce one, and `bind(Function<ByteBuf, B>)` yields the bound
//! [`crate::protocol_info::ProtocolInfo`].
//!
//! Java's `bind` takes the `Function<ByteBuf, B> contextWrapper` because the
//! packet codecs are `StreamCodec<? super B, P>` and `B` is only known at bind.
//! This port fixes `B = FriendlyByteBuf` (the only buffer the codec family has),
//! so the codec is already built when the template is created and `bind` has no
//! wrapper to apply — the wrapper is absorbed. Binding is then a cheap `Clone`
//! (the codec is `Arc`ed), matching Java's "bind once per connection" usage.

use crate::protocol_info::{ProtocolDetails, ProtocolInfo};

/// `net.minecraft.network.protocol.SimpleUnboundProtocol<T, B>` with `B =
/// FriendlyByteBuf`, `T` erased to the packet value `V`.
#[derive(Clone)]
pub struct SimpleUnboundProtocol<V: 'static> {
    protocol_info: ProtocolInfo<V>,
    details: ProtocolDetails,
}

impl<V: 'static> SimpleUnboundProtocol<V> {
    /// Wraps the bound protocol plus its registration details.
    pub(crate) fn new(protocol_info: ProtocolInfo<V>, details: ProtocolDetails) -> Self {
        SimpleUnboundProtocol {
            protocol_info,
            details,
        }
    }

    /// `bind(Function<ByteBuf, B> contextWrapper)` — the bound [`ProtocolInfo`].
    /// The context wrapper is absorbed (see the module doc).
    pub fn bind(&self) -> ProtocolInfo<V> {
        self.protocol_info.clone()
    }

    /// `ProtocolInfo.DetailsProvider.details()` — the `(PacketType, networkId)`
    /// registration table in `addPacket` order.
    pub fn details(&self) -> &ProtocolDetails {
        &self.details
    }
}
