//! Vanilla data registries for Rivet.
//!
//! Two ownership layers:
//!
//! - **Generated data** (`generated/`) — **generated, not hand-typed**
//!   (PORTING.md). Run `tools/rivet-codegen generate` and commit the output.
//!   The tables are compile-time (`phf` maps and `&'static` arrays) and gated
//!   behind the `blocks` cargo feature. The `generated/mod.rs` wiring is owned
//!   by the codegen tool. Hand-written tests that read the generated tables
//!   live OUTSIDE `generated/` (the codegen golden drift test asserts that dir
//!   contains exactly the generated files) — see `block_id_tests` (ownership C).
//! - **#124 registry SCC** (M1-A) — the strongly-connected `net.minecraft.core`/
//!   `net.minecraft.resources`/`net.minecraft.tags` key + registry types. Owned
//!   by the #124 units per the ownership map in `Cargo.toml`. Each module is a
//!   full Java-faithful port of its `mc.*` package with its own tests; #126
//!   (holder codecs) is the only deferred surface and is called out per site.
//!
//! `core` (issue #125) ports the registry-independent position/value
//! primitives of `net.minecraft.core`: `Vec3i`/`BlockPos`/`SectionPos`/
//! `ChunkPos` plus the tightly coupled `Position`, `Direction`, `AxisCycle`,
//! `Cursor3D` and `Rotation` value types. These are pure value types, resolved
//! by ID per OWNERSHIP.md; their `StreamCodec` impls live in `rivet-protocol`,
//! not here. `GlobalPos` is deferred (needs `ResourceKey` from the #124
//! registry SCC) and is not declared in this crate.

/// Compile-time block registry + block-state tables.
///
/// Gated behind the `blocks` feature; empty when the feature is off.
/// Submodule wiring lives in the generated `generated/mod.rs` (codegen-owned).
#[cfg(feature = "blocks")]
pub mod generated;

// ---------------------------------------------------------------------------
// Ownership A — resources / keys (`net.minecraft.resources`, `net.minecraft.tags`,
// `net.minecraft.core.registries.Registries`, `net.minecraft.IdentifierException`)
// ---------------------------------------------------------------------------

/// `net.minecraft.resources.Identifier` (MC 26.2 `ResourceLocation`).
pub mod identifier;
/// `net.minecraft.IdentifierException` — lives in `rivet-core` with the other
/// `net.minecraft` root-package classes; re-exported here for the registry units.
pub mod identifier_exception;
/// `net.minecraft.core.RegistrationInfo` (known-pack info + lifecycle).
pub mod registration_info;
/// `net.minecraft.core.registries.Registries` — const registry keys.
pub mod registries;
/// `net.minecraft.resources.ResourceKey<T>`.
pub mod resource_key;
/// `net.minecraft.tags.TagKey<T>`.
pub mod tag_key;

// ---------------------------------------------------------------------------
// Ownership B — registry lifecycle (`net.minecraft.core`)
// ---------------------------------------------------------------------------

/// `RegistryBuilder<T>` + `freeze()` (pre-freeze phase of `MappedRegistry`).
pub mod builder;
/// Minimal Copy holder-reference shape (`RegistryId`, `HolderId`) that the
/// registry internals need. The full `Holder<T>`/`HolderSet<T>` is #126.
pub mod holder;
/// `net.minecraft.core.IdMap<T>` — the id <-> value contract.
pub mod id_map;
/// `net.minecraft.core.Registry<T>` — the frozen registry surface.
pub mod registry;

// ---------------------------------------------------------------------------
// Ownership C — access / provider (erased boundaries, ROOT, GameData)
// ---------------------------------------------------------------------------

/// `net.minecraft.core.RegistryAccess` (heterogeneous registry sets) +
/// `LayeredRegistryAccess`.
pub mod access;
/// `GameData` — owns the provider; explicit STATIC → WORLDGEN → DIMENSIONS →
/// RELOADABLE layer order.
pub mod game_data;
/// The ROOT `WritableRegistry<AnyRegistry>` (`BuiltInRegistries.REGISTRY`).
pub mod root;

/// Generated-block-table integration tests (ownership C). The file sits
/// OUTSIDE the codegen-owned `generated/` dir (the golden drift test asserts
/// `src/generated/` contains exactly the generated files), so this module is
/// declared here via `#[path]` and only exists under the `blocks` feature +
/// `cfg(test)`.
#[cfg(all(feature = "blocks", test))]
#[path = "block_id_tests.rs"]
pub mod block_id_tests;

// ---------------------------------------------------------------------------
// Ownership D — serialization context (`net.minecraft.resources`)
// ---------------------------------------------------------------------------

/// `RegistryOps<T>` + the `DelegatingOps<T>` pieces #124 needs. The
/// `RegistryFileCodec`/`HolderSetCodec` codecs and all protocol `StreamCodec`s
/// are #126 (holder codecs), not here.
pub mod registry_ops;

// ---------------------------------------------------------------------------
// Ownership E — position/value SCC (`net.minecraft.core`, issue #125)
// ---------------------------------------------------------------------------

/// Registry-independent position/value primitives of `net.minecraft.core`
/// (issue #125) — see the crate doc for the ownership split.
pub mod core;

pub use access::RegistryAccess;
pub use builder::RegistryBuilder;
pub use holder::RegistryId;
pub use id_map::IdMap;
pub use identifier::{Identifier, IdentifierParseError};
pub use registration_info::RegistrationInfo;
pub use registry::Registry;
pub use resource_key::ResourceKey;
pub use root::AnyRegistry;
pub use tag_key::TagKey;
