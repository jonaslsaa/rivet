//! Minimal holder-reference shape required by the #124 registry internals.
//!
//! This is NOT the full `Holder<T>` port — that is **#126 (holder codecs)**
//! (`Holder<T>` = `Direct(T)` | `Reference { registry: RegistryId, id: u32 }`,
//! value/codec surface, `HolderSet<T>`). The #124 SCC only needs the two Copy
//! ID types below so `Registry<T>`/`RegistryBuilder<T>` can talk about holders
//! without depending on #126:
//!
//! - `RegistryId` — per-instance registry identity, distinct from the
//!   `ResourceKey<Registry<T>>` key (one key, many instances per world).
//!   Holder serialization-owner checks compare `RegistryId`.
//! - `HolderId` — a placeholder for the eventual `Holder.Reference { registry,
//!   id }`; Copy, 4 bytes (the 8-byte figure is the future `Reference {
//!   registry: RegistryId, id: u32 }` pair).
//!
//! #126 replaces `HolderId` with the real `Holder<T>` enum; it must NOT edit
//! `registry.rs`/`builder.rs` (ownership B) to do so — only this module and
//! `holder_set.rs`.
//!
//! Boundary note (OWNERSHIP.md §Registries): `RegistryId`'s stream codec (and
//! every `StreamCodec` for Identifier/ResourceKey/TagKey/Holder/HolderSet)
//! lives in `rivet-protocol`, never `rivet-registry`.

/// `RegistryId` — per-instance registry identity (a per-instance u32), distinct
/// from the `ResourceKey<Registry<T>>` key. OWNERSHIP.md §Registries. A
/// `RegistryBuilder` assigns one at construction (see `builder.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegistryId(pub u32);

/// The minimal registry-held holder reference — Copy, 4 bytes, resolved through
/// the owning registry. #126 widens this to the real `Holder<T>` enum
/// (`Direct(T)` | `Reference { registry, id }`); the SCC's `Registry`/`Builder`
/// return this type so the id space (element id == holder id == network id ==
/// insertion index) is already the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct HolderId(pub u32);
