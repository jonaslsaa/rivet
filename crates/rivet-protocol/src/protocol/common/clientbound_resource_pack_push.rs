//! STUB — `net.minecraft.network.protocol.common.ClientboundResourcePackPushPacket`.
//!
//! Java: `ClientboundResourcePackPushPacket.java` in `working/Paper`. Fields:
//! UUID, `stringUtf8()` url (unbounded, default `MAX_STRING_LENGTH` 32767),
//! `stringUtf8(40)` hash, required bool, `Optional<Component>` prompt. The
//! constructor asserts `hash.length() <= 40`.
//!
//! BLOCKED on two deps:
//! - the `Optional<Component>` prompt needs the `Component` **stream codec**
//!   (epic #12/#98; see `clientbound_disconnect`);
//! - the server-link `Component` path also pulls the trusted JSON parser.
//!
//! The url/hash/required part is portable today; the prompt field pins the
//! whole body until the component codec lands. Discriminator:
//! `packet_types::clientbound_resource_pack_push`.
