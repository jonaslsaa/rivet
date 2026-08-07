//! Port of `net.minecraft.network.protocol.game.ClientboundBundleDelimiterPacket`
//! (issue #87) — the play `bundle_delimiter` marker (network id 0).
//!
//! Java: `ClientboundBundleDelimiterPacket.java` in `working/Paper` — extends
//! `BundleDelimiterPacket<ClientGamePacketListener>`, a marker whose `handle()`
//! is `AssertionError("This packet should be handled by pipeline")`. It carries
//! no body; `GameProtocols.withBundlePacket` registers it (unit codec) at the
//! head of the play/clientbound table, which is why `bundle_delimiter` owns
//! network id 0. The bundling machinery (`PacketBundleUnpacker`/`Packer`,
//! `BundlerInfo.createForPacket`) is deferred with the bundle bodies
//! (RivetTodo #148); this slice ports the concrete play type identity the
//! join-path registration needs.

use crate::protocol::bundle::BundleDelimiterPacket;
use crate::protocol::game::packet_types::clientbound_bundle_delimiter;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ClientboundBundleDelimiterPacket` — the concrete play delimiter marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientboundBundleDelimiterPacket;

impl Packet for ClientboundBundleDelimiterPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_bundle_delimiter()
    }
}

impl BundleDelimiterPacket for ClientboundBundleDelimiterPacket {}
