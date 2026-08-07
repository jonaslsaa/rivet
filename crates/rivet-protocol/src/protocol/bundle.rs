//! Port of the `net.minecraft.network.protocol` bundle trio (MC 26.2):
//! `BundlePacket`, `BundleDelimiterPacket`, `BundlerInfo`.
//!
//! Java: the three files in `working/Paper`. A play-state connection can wrap a
//! run of packets in a bundle: `BundlePacket` carries the sub-packets in memory,
//! `BundleDelimiterPacket` marks the wire boundary (the only serialized half),
//! and `BundlerInfo` knows how to expand a bundle into delimiter-delimited
//! packets (unbundling) and re-assemble one (bundling).
//!
//! This slice ports the registration-relevant surface only:
//!   - the two marker traits `BundlePacket`/`BundleDelimiterPacket` — the
//!     `subPackets()`/`handle()` bodies belong to the play packet-body units
//!     (M1.1, #148), so the traits are markers for now;
//!   - `BundlerInfo` as a value `(bundlePacketType, delimiterPacketType)` that
//!     `ProtocolInfo.bundlerInfo()` returns. The wire machinery that uses it —
//!     `PacketBundlePacker`/`PacketBundleUnpacker` and `BundlerInfo.createForPacket`'s
//!     `unbundlePacket`/`startPacketBundling`/`Bundler` — needs actual bundle
//!     packet values (the `subPackets()`/constructor surface above), so it is
//!     deferred with them. Registering `withBundlePacket` is still faithful: it
//!     puts the delimiter at network id 0 (Paper's play/clientbound `bundle_delimiter`),
//!     which the join path depends on.
//!
//! RivetTodo(#148): the bundle-packet bodies and bundling/unbundling machinery
//! (M1.1 play bodies) are not ported; the marker traits and registration value
//! are what the join path needs today.

use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `net.minecraft.network.protocol.BundlePacket<T>` — an in-memory container of
/// sub-packets.
///
/// `subPackets()` is deferred with the play body units (M1.1); the marker keeps
/// the type identity so `BundlerInfo`/`withBundlePacket` can be typed against it
/// later.
pub trait BundlePacket: Packet {}

/// `net.minecraft.network.protocol.BundleDelimiterPacket<T>` — the wire
/// boundary of a bundle.
///
/// Java's `handle()` throws `AssertionError("This packet should be handled by
/// pipeline")`; `handle()` is deferred with the listener surface (see
/// [`Packet`]), so the marker carries the type identity for now.
pub trait BundleDelimiterPacket: Packet {}

/// `net.minecraft.network.protocol.BundlerInfo` — the bundle metadata a
/// [`crate::protocol_info::ProtocolInfo`] carries.
///
/// `BUNDLE_SIZE_LIMIT` is Java's constant; the bundling/unbundling *behavior*
/// (its `unbundlePacket`/`startPacketBundling`/`Bundler` members and
/// `createForPacket` factory) is deferred with `PacketBundlePacker`/
/// `PacketBundleUnpacker` and the bundle-packet bodies, as documented above.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BundlerInfo {
    /// The bundle container packet's type (`GamePacketTypes.CLIENTBOUND_BUNDLE`,
    /// `minecraft:bundle`) — used to recognize a bundle packet during
    /// unbundling. Never serialized itself, so it is absent from the generated
    /// packet-id tables.
    bundle_packet_type: PacketType,
    /// The delimiter packet's type (`minecraft:bundle_delimiter`) — the packet
    /// `withBundlePacket` registers (unit codec) at network id 0.
    delimiter_packet_type: PacketType,
}

impl BundlerInfo {
    /// `BundlerInfo.BUNDLE_SIZE_LIMIT` — the maximum sub-packets in one bundle.
    pub const BUNDLE_SIZE_LIMIT: usize = 4096;

    /// Builds the value `ProtocolInfoBuilder.withBundlePacket` stores.
    pub(crate) fn new(bundle_packet_type: PacketType, delimiter_packet_type: PacketType) -> Self {
        BundlerInfo {
            bundle_packet_type,
            delimiter_packet_type,
        }
    }

    /// The bundle container packet's type (`minecraft:bundle` for play/clientbound).
    pub fn bundle_packet_type(&self) -> &PacketType {
        &self.bundle_packet_type
    }

    /// The delimiter packet's type (`minecraft:bundle_delimiter`).
    pub fn delimiter_packet_type(&self) -> &PacketType {
        &self.delimiter_packet_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Delimiter;

    impl Packet for Delimiter {
        fn packet_type(&self) -> PacketType {
            PacketType::clientbound("bundle_delimiter")
        }
    }

    impl BundleDelimiterPacket for Delimiter {}

    #[test]
    fn delimiter_marker_is_implementable() {
        // Constructs the marker so `impl BundleDelimiterPacket` is exercised;
        // the type identity is what `withBundlePacket`/`BundlerInfo` key on.
        let delimiter = Delimiter;
        assert_eq!(
            delimiter.packet_type(),
            PacketType::clientbound("bundle_delimiter")
        );
    }

    #[test]
    fn size_limit_is_java_constant() {
        assert_eq!(BundlerInfo::BUNDLE_SIZE_LIMIT, 4096);
    }

    #[test]
    fn carries_both_packet_types() {
        let info = BundlerInfo::new(
            PacketType::clientbound("bundle"),
            PacketType::clientbound("bundle_delimiter"),
        );
        assert_eq!(
            *info.bundle_packet_type(),
            PacketType::clientbound("bundle")
        );
        assert_eq!(
            *info.delimiter_packet_type(),
            PacketType::clientbound("bundle_delimiter")
        );
    }
}
