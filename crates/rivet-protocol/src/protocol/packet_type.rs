//! Port of `net.minecraft.network.protocol.PacketType` (MC 26.2).
//!
//! Java: `PacketType.java` in `working/Paper` (vanilla 26.2). A `PacketType<T>`
//! is a value `(PacketFlow flow, Identifier id)` — the discriminator that
//! `Packet.type()` returns and that `IdDispatchCodec` keys its id table on.
//!
//! The type parameter `T extends Packet<?>` is erased in Java (a `PacketType<?>`
//! is used wherever the packet subtype is unknown, e.g. `ProtocolInfo.Details`
//! listing). Rust mirrors that with a non-generic value type — no phantom, no
//! `T` bound — so `Packet::packet_type()` is object-safe and a single
//! `PacketType` value can be shared across protocols (Java's
//! `CommonPacketTypes.CLIENTBOUND_KEEP_ALIVE` is the same value in
//! `ConfigurationProtocols` and `GameProtocols`; the *network id* is
//! protocol-local, assigned by `ProtocolInfoBuilder`).
//!
//! `toString()` is `flow.id() + "/" + id` (e.g. `serverbound/minecraft:intention`),
//! and it is the id-dispatch error text (`"Sending unknown packet '...'"`,
//! `"Duplicate registration for type ..."`) and the `ProtocolCodecBuilder` flow
//! panic, so `Display` carries Java's exact string.

use crate::generated::protocol::PacketFlow;
use rivet_registry::Identifier;
use std::fmt;

/// `net.minecraft.network.protocol.PacketType<T>` — the packet discriminator.
///
/// `flow` is the direction the packet travels; `id` is the canonical
/// `minecraft:...` identifier. Equality/hash cover both fields (Java record
/// semantics).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct PacketType {
    flow: PacketFlow,
    id: Identifier,
}

impl PacketType {
    /// `new PacketType<>(PacketFlow flow, Identifier id)` — the record
    /// constructor.
    pub const fn new(flow: PacketFlow, id: Identifier) -> Self {
        PacketType { flow, id }
    }

    /// `PacketType.flow()`.
    pub const fn flow(&self) -> PacketFlow {
        self.flow
    }

    /// `PacketType.id()`.
    pub const fn id(&self) -> &Identifier {
        &self.id
    }

    /// `createServerbound("intention")` — `new PacketType<>(PacketFlow.SERVERBOUND,
    /// Identifier.withDefaultNamespace(id))`, the helper every
    /// `*PacketTypes.SERVERBOUND_*` static uses.
    pub fn serverbound(id: &str) -> Self {
        PacketType::new(
            PacketFlow::Serverbound,
            Identifier::with_default_namespace(id),
        )
    }

    /// `createClientbound(...)` — the clientbound sibling of
    /// [`PacketType::serverbound`].
    pub fn clientbound(id: &str) -> Self {
        PacketType::new(
            PacketFlow::Clientbound,
            Identifier::with_default_namespace(id),
        )
    }
}

/// Java `toString()` — `this.flow.id() + "/" + this.id`.
impl fmt::Display for PacketType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.flow.id(), self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serverbound_clientbound_set_flow_and_default_namespace() {
        let ty = PacketType::serverbound("intention");
        assert_eq!(ty.flow(), PacketFlow::Serverbound);
        assert_eq!(ty.id().to_string(), "minecraft:intention");
        assert_eq!(
            PacketType::clientbound("keep_alive").id().to_string(),
            "minecraft:keep_alive"
        );
    }

    #[test]
    fn value_equality_and_hash() {
        let a = PacketType::serverbound("intention");
        let b = PacketType::serverbound("intention");
        assert_eq!(a, b);
        assert_ne!(a, PacketType::serverbound("key"));
        assert_ne!(a, PacketType::clientbound("intention"));
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }

    #[test]
    fn display_is_flow_slash_id() {
        assert_eq!(
            PacketType::serverbound("intention").to_string(),
            "serverbound/minecraft:intention"
        );
        assert_eq!(
            PacketType::clientbound("status_response").to_string(),
            "clientbound/minecraft:status_response"
        );
    }
}
