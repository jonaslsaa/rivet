//! STUB(mc.network.protocol.common) — `ClientboundDisconnectPacket` body not
//! ported.
//!
//! Java: `ClientboundDisconnectPacket.java` in `working/Paper`. A single
//! `Component` (the kick reason) over `ComponentSerialization
//! .TRUSTED_CONTEXT_FREE_STREAM_CODEC`, which is now ported as
//! [`crate::codec::byte_buf_codecs::trusted_component`] (issue #207) — so this
//! body is portable but not yet ported.
//!
//! The discriminator value exists (`packet_types::clientbound_disconnect`); only
//! the body is deferred. This is a join-path kick packet (config+play
//! `disconnect0`), so it matters for M1.
