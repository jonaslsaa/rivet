//! STUB — `net.minecraft.network.protocol.common.ClientboundDisconnectPacket`.
//!
//! Java: `ClientboundDisconnectPacket.java` in `working/Paper`. A single
//! `Component` (the kick reason) over `Component.TRUSTED_CONTEXT_FREE`.
//!
//! BLOCKED: `Component` has a JSON codec (`rivet-text`) but **no stream codec**
//! yet — `TRUSTED_CONTEXT_FREE` is `ComponentSerialization`'s stream codec
//! (epic #12/#98 deferred the wire half). Ported when the component stream
//! codec lands.
//!
//! The discriminator value exists (`packet_types::clientbound_disconnect`); only
//! the body is deferred. This is a join-path kick packet (config+play
//! `disconnect0`), so it matters for M1.
