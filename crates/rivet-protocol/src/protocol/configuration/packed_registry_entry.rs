//! Port of `net.minecraft.core.RegistrySynchronization.PackedRegistryEntry`
//! (issue #109) — the per-element record of the configuration registry sync.
//!
//! Java: `RegistrySynchronization.java` (nested record) in `working/Paper`. A
//! `(Identifier id, Optional<Tag> data)` pair. The `data` `Optional<Tag>` is
//! `Optional.empty()` when the client already has the element's contents (the
//! server matched it against the client's `KnownPack`s via
//! `RegistrySynchronization.packRegistry`); otherwise the element codec's NBT
//! encoding. The value type is protocol-shared, so it lives here (not in its
//! package-mirror server crate) exactly like `CommonPlayerSpawnInfo` (#108).
//!
//! The wire codec is `Identifier.STREAM_CODEC` + `ByteBufCodecs.TAG.apply
//! (ByteBufCodecs::optional)` — `TAG.apply(...)` dispatches the
//! `CodecOperation`, so this is `ByteBufCodecs.optional(TAG)`: a boolean
//! presence byte (`0x01`/`0x00`), then the `TAG` codec. It is **not** the
//! `optionalTagCodec` null-tag form (that one reads the NBT type byte directly,
//! no boolean, and maps an `EndTag` to `empty()`) — the real server writes the
//! boolean form (the pinned capture fixture is `... 01 0a 09 00 0b ...` =
//! present, then compound). `ByteBufCodecs.optional` is the Rust
//! [`crate::codec::byte_buf_codecs::optional`] combinator over
//! [`crate::codec::byte_buf_codecs::tag_codec`].

use crate::codec::byte_buf_codecs::{optional, tag_codec};
use crate::codec::{StreamCodec, composite_2};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::stream_codecs::identifier_codec;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;

/// `RegistrySynchronization.PackedRegistryEntry` — `(Identifier id, Optional<Tag> data)`.
#[derive(Clone, Debug, PartialEq)]
pub struct PackedRegistryEntry {
    id: Identifier,
    /// `data` — Java `Optional<Tag>`; `None` when the element's contents were
    /// skipped (the client already has them via a matched `KnownPack`).
    data: Option<Tag>,
}

impl PackedRegistryEntry {
    /// `new PackedRegistryEntry(Identifier id, Optional<Tag> data)`.
    pub fn new(id: Identifier, data: Option<Tag>) -> Self {
        PackedRegistryEntry { id, data }
    }

    /// `PackedRegistryEntry.id()`.
    pub fn id(&self) -> &Identifier {
        &self.id
    }

    /// `PackedRegistryEntry.data()`.
    pub fn data(&self) -> Option<&Tag> {
        self.data.as_ref()
    }

    /// `PackedRegistryEntry.STREAM_CODEC` — `Identifier.STREAM_CODEC` then
    /// `ByteBufCodecs.TAG.apply(ByteBufCodecs::optional)`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, PackedRegistryEntry> {
        composite_2(
            identifier_codec(),
            |e: &PackedRegistryEntry| e.id.clone(),
            optional(tag_codec(
                rivet_nbt::nbt_accounter::NbtAccounter::default_quota,
            )),
            |e: &PackedRegistryEntry| e.data.clone(),
            PackedRegistryEntry::new,
        )
    }
}
