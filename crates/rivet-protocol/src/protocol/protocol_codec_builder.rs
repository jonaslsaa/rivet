//! Port of `net.minecraft.network.protocol.ProtocolCodecBuilder` (MC 26.2).
//!
//! Java: `ProtocolCodecBuilder.java` in `working/Paper` (vanilla 26.2). The
//! bridge between `ProtocolInfoBuilder`'s entries and the `IdDispatchCodec`:
//! it validates that every registered `PacketType` flows the same direction as
//! the protocol, then builds the varint-id dispatch codec (`Packet::type` is
//! the discriminator key).
//!
//! Java's `IdDispatchCodec.Builder` is consumed through this builder, so the
//! entries are collected here and handed to `IdDispatchCodec.builder(...)` at
//! `build()` — the flow check fires in `add` (Java's `ProtocolCodecBuilder.add`
//! timing), the duplicate-registration panic fires in `IdDispatchCodec.Builder.build`
//! (Java's `build()` timing). The buffer is `FriendlyByteBuf`, the only
//! registry-independent buffer this crate has (the same instantiation
//! `byte_buf_codecs` and `IdDispatchCodec` already use).

use crate::codec::{StreamCodec, builder as id_dispatch_builder};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::generated::protocol::PacketFlow;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `net.minecraft.network.protocol.ProtocolCodecBuilder<B, L>` with
/// `B = FriendlyByteBuf`, `L` erased to the packet value `V`.
pub struct ProtocolCodecBuilder<V: 'static> {
    flow: PacketFlow,
    entries: Vec<(PacketType, StreamCodec<FriendlyByteBuf, V>)>,
}

impl<V: 'static> ProtocolCodecBuilder<V> {
    /// `new ProtocolCodecBuilder<>(PacketFlow flow)`.
    pub fn new(flow: PacketFlow) -> Self {
        ProtocolCodecBuilder {
            flow,
            entries: Vec::new(),
        }
    }

    /// `add(PacketType<T> type, StreamCodec<? super B, T> serializer)`.
    ///
    /// Rejects a packet whose flow differs from the protocol's direction with
    /// Java's exact `IllegalArgumentException` message (the enum constant name
    /// is `flow.id()` uppercased: `SERVERBOUND`/`CLIENTBOUND`).
    pub fn add(
        &mut self,
        ty: PacketType,
        serializer: StreamCodec<FriendlyByteBuf, V>,
    ) -> &mut Self {
        if ty.flow() != self.flow {
            panic!(
                "Invalid packet flow for packet {ty}, expected {}",
                self.flow.id().to_uppercase()
            );
        }
        self.entries.push((ty, serializer));
        self
    }

    /// `build()` — the id-dispatch codec keyed on `Packet::type()`. The
    /// `IdDispatchCodec.Builder.build` duplicate check runs here, so a packet
    /// registered twice panics with Java's
    /// `IllegalStateException("Duplicate registration for type ...")`.
    pub fn build(self) -> StreamCodec<FriendlyByteBuf, V>
    where
        V: Packet,
    {
        let mut dispatch_builder = id_dispatch_builder(|packet: &V| packet.packet_type());
        for (ty, codec) in self.entries {
            dispatch_builder = dispatch_builder.add(ty, codec);
        }
        StreamCodec::new(dispatch_builder.build())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::unit;
    use std::panic::catch_unwind;

    #[derive(Debug, Clone, PartialEq)]
    struct Ping;

    impl Packet for Ping {
        fn packet_type(&self) -> PacketType {
            PacketType::serverbound("ping_request")
        }
    }

    impl std::fmt::Display for Ping {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.packet_type())
        }
    }

    fn panic_message<F: FnOnce() -> R, R>(f: F) -> String {
        let err = match catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(_) => panic!("expected the closure to panic"),
            Err(err) => err,
        };
        err.downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "non-string panic payload".to_string())
    }

    #[test]
    fn flow_mismatch_panics_with_java_message() {
        let mut builder = ProtocolCodecBuilder::<Ping>::new(PacketFlow::Serverbound);
        let msg = panic_message(|| {
            builder.add(PacketType::clientbound("status_response"), unit(Ping));
        });
        assert_eq!(
            msg,
            "Invalid packet flow for packet clientbound/minecraft:status_response, expected SERVERBOUND"
        );
    }
}
