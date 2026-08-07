//! STUB(mc.network.protocol.common) — `ClientboundUpdateTagsPacket` body not
//! ported: the registry-wired `TagNetworkSerialization.NetworkPayload` +
//! registry-key codecs defer with the holder codecs (#126; configuration-phase
//! registry sync is #109).
//!
//! Java: `ClientboundUpdateTagsPacket.java` in `working/Paper`. A `Map<ResourceKey<? extends Registry<?>>, TagNetworkSerialization.NetworkPayload>`
//! — the tag network serialization needs the registry-wired
//! `TagNetworkSerialization.NetworkPayload` codec. Sent at the end of the
//! configuration registry sync (`SynchronizeRegistriesTask` ->
//! `handleResponse`).
//!
//! Discriminator: `packet_types::clientbound_update_tags`.
