//! Port of `net.minecraft.network.protocol.game.ClientboundBundlePacket` (issue
//! #87) — the play `bundle` container marker (never serialized directly).
//!
//! Java: `ClientboundBundlePacket.java` in `working/Paper` — `BundlePacket<
//! ClientGamePacketListener>` holding an `Iterable<Packet>` of sub-packets.
//! The container is expanded/assembled by the bundling machinery, so it has no
//! `STREAM_CODEC` and never appears in the id table (its type is what
//! `withBundlePacket`/`BundlerInfo` key on). The `subPackets()` surface and the
//! packer/unpacker are deferred with the bundle bodies (RivetTodo #148); this
//! slice ports the concrete play type identity.

use crate::protocol::bundle::BundlePacket;
use crate::protocol::game::packet_types::clientbound_bundle;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ClientboundBundlePacket` — the play bundle container marker.
///
/// RivetTodo(#148): the `subPackets()` container surface is deferred with the
/// bundle bodies; the marker keeps the type identity for registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientboundBundlePacket;

impl Packet for ClientboundBundlePacket {
    fn packet_type(&self) -> PacketType {
        clientbound_bundle()
    }
}

impl BundlePacket for ClientboundBundlePacket {}
