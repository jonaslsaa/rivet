//! `net.minecraft.world.flag` — feature flags (issue #387, the
//! `LevelSettings`/`WorldDataConfiguration` prerequisite slice for #323).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/flag/`.
//! The `mc.world.flag` manifest unit ports these six files; this slice ports
//! the whole value layer (universe identity, set subset/join/subtract
//! semantics, declaration-ordering registry, the `FeatureFlags` statics, and
//! the registry codec with its exact unknown-flag partial-error behavior).
//! `FeatureElement` is deferred with a `RivetTodo(#418)` marker: its
//! `FILTERED_REGISTRIES` set references `Registries.ITEM`/`BLOCK`/
//! `ENTITY_TYPE`/`GAME_RULE`/`MENU`/`POTION`/`MOB_EFFECT`, none of which have
//! generated registry keys in `rivet-registry` yet.
//!
//! Value types are `Clone`/`Eq` handles (see each module): a `FeatureFlag` is
//! an opaque `(universe, mask)` pair (the universe owns a `String`, so nothing
//! is `Copy`) — the Rust analogue of Java's object identity — so
//! `FeatureFlagSet` equality is structural (universe value + mask), exactly
//! Java's `equals`.
//!
//! Placement: the whole unit is owned by `rivet-world` (the `mc.world.flag`
//! manifest unit maps there). `FeatureFlags`' statics are `LazyLock`s — the
//! registry owns an `Identifier` (a `String`) and an `Arc`, so Java's `static
//! final` constants become lazily-initialized statics (same as
//! `rivet-registry::registries`).

pub mod feature_flag;
pub mod feature_flag_registry;
pub mod feature_flag_set;
pub mod feature_flag_universe;
pub mod feature_flags;

pub use feature_flag::FeatureFlag;
pub use feature_flag_registry::{Builder, FeatureFlagRegistry};
pub use feature_flag_set::FeatureFlagSet;
pub use feature_flag_universe::FeatureFlagUniverse;
pub use feature_flags::{
    MINECART_IMPROVEMENTS, REDSTONE_EXPERIMENTS, REGISTRY, TRADE_REBALANCE, VANILLA, codec,
    default_flags, is_experimental, print_missing_flags, print_missing_flags_in, vanilla_set,
};
