//! STUB(mc.network.protocol.common) — `ClientboundServerLinksPacket` body not
//! ported.
//!
//! Java: `ClientboundServerLinksPacket.java` in `working/Paper`. A
//! `ServerLinks` value (`Set<ServerLinks.Entry>` of `Type` + `KnownPack` or
//! `URI`).
//!
//! BLOCKED: `ServerLinks` is a server-level value type not yet ported.
//! RivetTodo(#207): the `ServerLinks` value type + its `STREAM_CODEC` (needs
//! the `KnownPack`/`URI` handling in the registry/links units). Sent only when
//! server links are non-empty (default: none on join). Discriminator:
//! `packet_types::clientbound_server_links`.
