//! Port of `net.minecraft.network.protocol.configuration.ClientboundRegistryDataPacket`
//! (issue #109).
//!
//! Java: `ClientboundRegistryDataPacket.java` in `working/Paper`. One registry's
//! packed contents in the configuration sync: `(ResourceKey<? extends Registry<?>>
//! registry, List<PackedRegistryEntry> entries)`. Sent ~30 times per join
//! (`RegistryDataLoader.SYNCHRONIZED_REGISTRIES`, one packet per registry).

use crate::codec::byte_buf_codecs::list;
use crate::codec::{CodecOperation, StreamCodec, apply, composite_2};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::configuration::packed_registry_entry::PackedRegistryEntry;
use crate::protocol::configuration::packet_types::clientbound_registry_data;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::protocol::stream_codecs::registry_key_codec;
use rivet_registry::{Registry, ResourceKey};

/// `net.minecraft.network.protocol.configuration.ClientboundRegistryDataPacket` —
/// the record `(ResourceKey<? extends Registry<?>> registry, List<PackedRegistryEntry> entries)`.
///
/// The erased registry key is `ResourceKey<Registry<()>>` (rustc's stand-in for
/// Java's unbounded-wildcard `ResourceKey<? extends Registry<?>>`), matching the
/// `update_tags` key type so both packets can share one erased-key map contract.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientboundRegistryDataPacket {
    registry: ResourceKey<Registry<()>>,
    entries: Vec<PackedRegistryEntry>,
}

impl ClientboundRegistryDataPacket {
    /// `new ClientboundRegistryDataPacket(ResourceKey<? extends Registry<?>>, List<PackedRegistryEntry>)`.
    pub fn new(registry: ResourceKey<Registry<()>>, entries: Vec<PackedRegistryEntry>) -> Self {
        ClientboundRegistryDataPacket { registry, entries }
    }

    /// `ClientboundRegistryDataPacket.registry()`.
    pub fn registry(&self) -> &ResourceKey<Registry<()>> {
        &self.registry
    }

    /// `ClientboundRegistryDataPacket.entries()`.
    pub fn entries(&self) -> &[PackedRegistryEntry] {
        &self.entries
    }

    /// `ClientboundRegistryDataPacket.STREAM_CODEC` — the erased registry key
    /// (identifier string), then `PackedRegistryEntry.STREAM_CODEC.apply(ByteBufCodecs.list())`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundRegistryDataPacket> {
        let list: CodecOperation<FriendlyByteBuf, PackedRegistryEntry, Vec<PackedRegistryEntry>> =
            list();
        composite_2(
            registry_key_codec(),
            |p: &ClientboundRegistryDataPacket| p.registry.clone(),
            apply(PackedRegistryEntry::stream_codec(), list),
            |p: &ClientboundRegistryDataPacket| p.entries.clone(),
            ClientboundRegistryDataPacket::new,
        )
    }
}

impl Packet for ClientboundRegistryDataPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_registry_data()
    }
}
