//! Port of `net.minecraft.network.protocol.configuration` (issue #109) — the
//! configuration-phase packet bodies.
//!
//! Java: `net/minecraft/network/protocol/configuration/`. `packet_types` holds
//! the `ConfigurationPacketTypes` discriminators; each body is a value type +
//! `stream_codec()` + `Packet` impl, registered in `ConfigurationProtocols`
//! order (the generated table pins the vanilla ids).
//!
//! The M1 offline join path's registry-sync content lives here: the
//! `SelectKnownPacks` handshake pair, `ClientboundRegistryDataPacket` (with its
//! `RegistrySynchronization.PackedRegistryEntry` value type), and the
//! terminal `finish_configuration` pair. `ClientboundUpdateTagsPacket` and
//! `ClientboundUpdateEnabledFeaturesPacket` are the trailing registry-sync
//! packets; `update_tags`' `TagNetworkSerialization.NetworkPayload` wire shape
//! lives in `crate::protocol::common::tag_network_payload`.
//!
//! `accept_code_of_conduct` is ported (issue #236): the fieldless
//! `ServerboundAcceptCodeOfConductPacket` decodes via `StreamCodec.unit`, and
//! the configuration listener's `handleAcceptCodeOfConduct` always closes on the
//! mismatch — no CoC task is ever queued (`MinecraftServer.getCodeOfConducts()`
//! is `Map.of()`).
//!
//! Deferred, not stubbed (not on the M1 offline join path): the
//! `reset_chat`/`code_of_conduct` bodies stay deferred with the CoC task that
//! would send them.
//!
//! `handle()` stays deferred with the listener hierarchy (M1.1/#148), like every
//! other body module.

pub mod clientbound_finish_configuration;
pub mod clientbound_registry_data;
pub mod clientbound_select_known_packs;
pub mod clientbound_update_enabled_features;
pub mod packed_registry_entry;
pub mod packet_types;
pub mod serverbound_accept_code_of_conduct;
pub mod serverbound_finish_configuration;
pub mod serverbound_select_known_packs;
