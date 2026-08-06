//! STUB — `net.minecraft.network.protocol.common.ClientboundUpdateTagsPacket`.
//!
//! Java: `ClientboundUpdateTagsPacket.java` in `working/Paper`. A `Map<ResourceKey<? extends Registry<?>>, TagNetworkSerialization.NetworkPayload>`
//! — the tag network serialization needs the registry-wired
//! `TagNetworkSerialization.NetworkPayload` codec (deferred with #126/#109).
//! Sent at the end of the configuration registry sync (`SynchronizeRegistriesTask`
//! -> `handleResponse`).
//!
//! BLOCKED: `TagNetworkSerialization.NetworkPayload` + registry-key codecs.
//! Discriminator: `packet_types::clientbound_update_tags`.
