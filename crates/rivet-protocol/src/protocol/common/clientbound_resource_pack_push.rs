//! STUB(mc.network.protocol.common) — `ClientboundResourcePackPushPacket` body
//! not ported.
//!
//! Java: `ClientboundResourcePackPushPacket.java` in `working/Paper`. Fields:
//! UUID, `stringUtf8()` url (unbounded, default `MAX_STRING_LENGTH` 32767),
//! `stringUtf8(40)` hash, required bool, `Optional<Component>` prompt over
//! `ComponentSerialization.TRUSTED_CONTEXT_FREE_STREAM_CODEC.apply(
//! ByteBufCodecs::optional)` — now ported as
//! [`crate::chat::trusted_optional_component`] (issue #89/#207) — so this
//! body is portable but not yet ported. The constructor asserts
//! `hash.length() <= 40`.
//!
//! The url/hash/required part is portable; the prompt field is the only
//! `Component`-codec dependency. Discriminator:
//! `packet_types::clientbound_resource_pack_push`.
