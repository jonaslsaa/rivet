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
//!   full Java-faithful port of its `mc.*` package with its own tests.
//! - **#126 holder codecs** (M1-A, sub-issue of #10) — the `Holder<T>`/
//!   `HolderSet<T>`/`HolderLookup`/`HolderGetter`/`HolderOwner` value surface
//!   (`holder.rs`/`holder_set.rs`/`holder_lookup.rs`), the `RegistryOps` codec
//!   context (`registry_ops.rs`), and `RegistryFileCodec`/`RegistryFixedCodec`/
//!   `HolderSetCodec` (`registry_file_codec.rs`). Protocol `StreamCodec`s
//!   (Identifier/ResourceKey/TagKey/Holder/HolderSet/GlobalPos) live in
//!   `rivet-protocol` — `rivet-registry` never depends on `rivet-protocol`
//!   (OWNERSHIP.md §Registries).
//!
//! `core` (issue #125) ports the registry-independent position/value
//! primitives of `net.minecraft.core`: `Vec3i`/`BlockPos`/`SectionPos`/
//! `ChunkPos`/`GlobalPos` plus the tightly coupled `Position`, `Direction`,
//! `AxisCycle`, `Cursor3D` and `Rotation` value types. These are pure value
//! types, resolved by ID per OWNERSHIP.md; their `StreamCodec` impls live in
//! `rivet-protocol`, not here. `GlobalPos`'s `ResourceKey<Level>` component
//! uses the world-unit `Level` placeholder from `registries`; its `StreamCodec`
//! is #126 in `rivet-protocol`.
//!
//! `core` (issue #198) also carries the authlib login-profile value types
//! (`GameProfile`/`Property`/`PropertyMap`, an ordered multimap whose `values()`
//! is key-grouped — `PropertyMap::new` re-groups like guava's
//! `ImmutableMultimap.copyOf`) and `UUIDUtil.createOfflinePlayerUUID` (the
//! offline-login v3 name UUID). These are authlib/`net.minecraft.core` value
//! types the login codec needs; their `StreamCodec` impls
//! (`ByteBufCodecs.GAME_PROFILE`/`GAME_PROFILE_PROPERTIES`) live in
//! `rivet-protocol` per the same ownership rule.

/// Compile-time generated registry tables.
///
/// The block-entity identity table is unconditional because protocol codecs
/// already need it. Larger block/state/static-builtin tables remain gated
/// behind `blocks`. Wiring lives in generated `generated/mod.rs`.
pub mod generated;

/// The pure fluid id-handle (`FluidId`) over the generated `minecraft:fluid`
/// tables (issue #370), mirroring `BlockId`'s ownership. Gated behind
/// `blocks` like the fluid tables it reads.
#[cfg(feature = "blocks")]
pub mod fluid_id;

/// Hand-written `BlockState` value type over the generated global-id + behavior
/// tables (issue #228). The "pure table ops, no world types" surface the
/// worldgen/heightmap/lighting work consumes; gated behind `blocks` like the
/// tables it decodes.
#[cfg(feature = "blocks")]
pub mod block_state;

/// `MapColor` + `Brightness` — the material color surface lighting/heightmap
/// code reads off a `BlockState` (issue #228). Table-driven over the 62
/// generated constants; see the module doc for the Paper 26.2 grounding.
#[cfg(feature = "blocks")]
pub mod map_color;

/// `Property` + `PropertyValue` + `PropertyKind` — the typed block-property
/// surface (`BooleanProperty`/`IntegerProperty`/`EnumProperty` collapsed into
/// one id-keyed `Property`), table-driven over the generated property tables
/// (issue #228).
#[cfg(feature = "blocks")]
pub mod block_state_property;

/// `StateDefinition` — a block's name-sorted property map, derived from the
/// generated shape tables (issue #228). `NbtUtils.readBlockState` resolves
/// properties through this.
#[cfg(feature = "blocks")]
pub mod state_definition;

/// The typed `block.state.properties` leaf value classes and the
/// `BlockStateProperties` constant facade (issue #228) — the worldgen/lighting
/// surface that sets property values on states by their value-class enum
/// (`state.set_value(SlabBlock.TYPE, SlabType.DOUBLE)`).
#[cfg(feature = "blocks")]
pub mod block_state_properties;

// ---------------------------------------------------------------------------
// Ownership A — resources / keys (`net.minecraft.resources`, `net.minecraft.tags`,
// `net.minecraft.core.registries.Registries`, `net.minecraft.IdentifierException`)
// ---------------------------------------------------------------------------

/// The generated-identity surface of
/// `net.minecraft.world.level.block.entity.BlockEntityType` (#341).
pub mod block_entity_type;
/// The generated-identity surface of
/// `net.minecraft.world.level.levelgen.feature.featuresize.FeatureSizeType`
/// (#394).
pub mod feature_size_type;
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
/// `Holder<T>` + `RegistryId`/`HolderId` (`net.minecraft.core`, #126).
pub mod holder;
/// `HolderOwner`/`HolderGetter`/`HolderLookup`/`RegistryLookup`/`Provider` +
/// the codec views `RegistryOwner`/`RegistryGetter` (#126).
pub mod holder_lookup;
/// `HolderSet<T>` (`net.minecraft.core.HolderSet`, #126).
pub mod holder_set;
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

/// Generated static-builtin-table integration tests (issue #124 phase F). The
/// file sits OUTSIDE the codegen-owned `generated/` dir (the golden drift test
/// asserts `src/generated/` contains exactly the generated files); same pattern
/// as `block_id_tests`.
#[cfg(all(feature = "blocks", test))]
#[path = "static_builtin_tests.rs"]
pub mod static_builtin_tests;

/// Generated block-state global-id table integration tests (issue #154). Same
/// pattern: outside `src/generated/`, only under the `blocks` feature + test.
#[cfg(all(feature = "blocks", test))]
#[path = "block_state_tests.rs"]
pub mod block_state_tests;

/// Generated biome id table + tag network-content integration tests (issue #49).
/// Same pattern: outside `src/generated/`, only under the `blocks` feature + test.
#[cfg(all(feature = "blocks", test))]
#[path = "biomes_tags_tests.rs"]
pub mod biomes_tags_tests;

// ---------------------------------------------------------------------------
// Ownership D — serialization context (`net.minecraft.resources`)
// ---------------------------------------------------------------------------

/// `RegistryFileCodec`/`RegistryFixedCodec`/`HolderSetCodec` (#126 holder
/// codecs, `net.minecraft.resources`).
pub mod registry_file_codec;
/// `RegistryOps<T>` + the `DelegatingOps<T>` pieces #124 needs.
pub mod registry_ops;

// ---------------------------------------------------------------------------
// Ownership E — position/value SCC (`net.minecraft.core`, issue #125)
// ---------------------------------------------------------------------------

/// Registry-independent position/value primitives of `net.minecraft.core`
/// (issue #125) — see the crate doc for the ownership split.
pub mod core;

pub use access::RegistryAccess;
pub use builder::RegistryBuilder;
pub use holder::{Holder, HolderId, HolderKind, RegistryId};
pub use holder_lookup::{
    HolderGetter, HolderLookup, HolderLookupProvider, HolderOwner, RegistryGetter, RegistryLookup,
    RegistryOwner,
};
pub use holder_set::HolderSet;
pub use id_map::IdMap;
pub use identifier::{Identifier, IdentifierParseError};
pub use registration_info::RegistrationInfo;
pub use registry::Registry;
pub use resource_key::ResourceKey;
pub use root::AnyRegistry;
pub use tag_key::TagKey;
