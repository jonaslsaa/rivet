//! STUB(mc.network.protocol.common) — `ClientboundDisconnectPacket` body not
//! ported: `Component` has a JSON codec (`rivet-text`) but **no stream codec**
//! yet — `TRUSTED_CONTEXT_FREE` is `ComponentSerialization`'s stream codec, the
//! wire half of #89, and it blocks this whole body.
//!
//! Java: `ClientboundDisconnectPacket.java` in `working/Paper`. A single
//! `Component` (the kick reason) over `Component.TRUSTED_CONTEXT_FREE`.
//!
//! The discriminator value exists (`packet_types::clientbound_disconnect`); only
//! the body is deferred. This is a join-path kick packet (config+play
//! `disconnect0`), so it matters for M1.
