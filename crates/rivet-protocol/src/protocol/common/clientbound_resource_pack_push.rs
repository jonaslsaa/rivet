//! STUB(mc.network.protocol.common) — `ClientboundResourcePackPushPacket` body
//! not ported: the `Optional<Component>` prompt needs the `Component` **stream
//! codec** (the wire half of #89, see `clientbound_disconnect`), and the
//! server-link `Component` path also pulls the trusted JSON parser.
//!
//! Java: `ClientboundResourcePackPushPacket.java` in `working/Paper`. Fields:
//! UUID, `stringUtf8()` url (unbounded, default `MAX_STRING_LENGTH` 32767),
//! `stringUtf8(40)` hash, required bool, `Optional<Component>` prompt. The
//! constructor asserts `hash.length() <= 40`.
//!
//! The url/hash/required part is portable today; the prompt field pins the
//! whole body until the component codec lands. Discriminator:
//! `packet_types::clientbound_resource_pack_push`.
