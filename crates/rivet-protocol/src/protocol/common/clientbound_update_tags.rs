//! Port of `net.minecraft.network.protocol.common.ClientboundUpdateTagsPacket`
//! (issue #109).
//!
//! Java: `ClientboundUpdateTagsPacket.java` in `working/Paper`. A `Map<ResourceKey
//! <? extends Registry<?>>, TagNetworkSerialization.NetworkPayload>` — the full
//! tag network serialization. Sent at the end of the configuration registry
//! sync (`SynchronizeRegistriesTask` -> `handleResponse`).
//!
//! The wire codec is `input.readMap(FriendlyByteBuf::readRegistryKey,
//! TagNetworkSerialization.NetworkPayload::read)` / `writeMap(this.tags,
//! FriendlyByteBuf::writeResourceKey, (buffer, value) -> value.write(buffer))`:
//! a varint map count, then per entry the **raw registry-key identifier
//! string** (`readRegistryKey` -> `Identifier.parse`, panicking on a hostile id
//! exactly like Java) and the [`tag_network_payload::NetworkPayload`] wire
//! shape (a varint tag count, then per tag an identifier string + an id list).
//!
//! The outer map decode also goes through `Maps::newHashMapWithExpectedSize`
//! (the raw `readMap`), so a negative registry count panics with guava's
//! `"expectedSize cannot be negative but was: {n}"`, exactly like the inner
//! `NetworkPayload` map. A hostile registry-key identifier panics on
//! `Identifier.parse` (`IdentifierException`).
//!
//! Map iteration order on the wire is not contractually stable (Java `HashMap`),
//! so tests compare decoded content, never byte identity, for multi-registry
//! maps (capture-semantics rule). The registry-key type is the erased
//! `ResourceKey<Registry<()>>`, shared with `ClientboundRegistryDataPacket`.

use crate::friendly_byte_buf::{FriendlyByteBuf, MAX_STRING_LENGTH};
use crate::protocol::common::packet_types::clientbound_update_tags;
use crate::protocol::common::tag_network_payload::NetworkPayload;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use rivet_registry::{Identifier, Registry, ResourceKey};

use std::collections::HashMap;

/// `net.minecraft.network.protocol.common.ClientboundUpdateTagsPacket` — the
/// `Map<ResourceKey<? extends Registry<?>>, NetworkPayload> tags` value.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientboundUpdateTagsPacket {
    tags: HashMap<ResourceKey<Registry<()>>, NetworkPayload>,
}

impl ClientboundUpdateTagsPacket {
    /// `new ClientboundUpdateTagsPacket(Map<ResourceKey<? extends Registry<?>>,
    /// NetworkPayload> tags)`.
    pub fn new(tags: HashMap<ResourceKey<Registry<()>>, NetworkPayload>) -> Self {
        ClientboundUpdateTagsPacket { tags }
    }

    /// `ClientboundUpdateTagsPacket.getTags()`.
    pub fn tags(&self) -> &HashMap<ResourceKey<Registry<()>>, NetworkPayload> {
        &self.tags
    }

    /// `ClientboundUpdateTagsPacket.STREAM_CODEC` — the Java private decode-ctor
    /// + `write` composed as `Packet.codec(write, new)`.
    pub fn stream_codec() -> crate::codec::StreamCodec<FriendlyByteBuf, ClientboundUpdateTagsPacket>
    {
        crate::codec::codec(
            |value: &ClientboundUpdateTagsPacket, output: &mut FriendlyByteBuf| {
                // `writeMap(tags, writeResourceKey, (buffer, v) -> v.write(buffer))`.
                output.write_var_int(value.tags.len() as i32);
                for (key, payload) in &value.tags {
                    // `writeResourceKey` -> `writeIdentifier` (raw utf string).
                    output.write_utf(&key.identifier().to_string());
                    payload.write(output);
                }
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                // `readMap(readRegistryKey, NetworkPayload::read)` —
                // `Maps.newHashMapWithExpectedSize`, negative count -> guava's
                // checkNonnegative.
                let count = input.read_var_int();
                if count < 0 {
                    panic!("expectedSize cannot be negative but was: {count}");
                }
                let mut tags = HashMap::with_capacity(count as usize);
                for _ in 0..count {
                    // `readRegistryKey` -> raw identifier -> `createRegistryKey`.
                    let id = Identifier::parse(&input.read_utf_max(MAX_STRING_LENGTH));
                    let key = ResourceKey::create_registry_key(id);
                    let payload = NetworkPayload::read(input);
                    tags.insert(key, payload);
                }
                Ok(ClientboundUpdateTagsPacket { tags })
            },
        )
    }
}

impl Packet for ClientboundUpdateTagsPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_update_tags()
    }
}
