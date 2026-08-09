//! Port of `net.minecraft.world.level.levelgen.placement.PlacementModifierType`
//! (interface, 26.2).
//!
//! Java is the interface every placement modifier's `type()` returns; its
//! fifteen constants are `register(...)` calls into
//! `BuiltInRegistries.PLACEMENT_MODIFIER_TYPE`, each holding the modifier's
//! `MapCodec`. The Rust port mirrors `Feature`'s identity split: the modifier's
//! type identity is the opaque `PlacementModifierTypeId` handle (the registry
//! element identity), and `placement_modifier_type(id)` resolves it to the
//! type behavior. The registration table is generated content — the `#181`
//! hub (same codegen as `Feature.register`) — so this core unit does NOT
//! hand-port the fifteen constants, and the per-type `MapCodec` is deferred
//! with the codec surface (`#126`). Until the generated table lands the lookup
//! panics unconditionally (the pre-wire stand-in); once wired, an unknown type
//! id throws `IllegalStateException` like Java's `Registry.getValueOrThrow`
//! (which throws only when the key is genuinely missing).

use std::fmt::Debug;

/// The `PlacementModifierType<P>` registry element identity — the per-type
/// `u32` id (element id == holder id == insertion index) plus its registry-key
/// location, mirroring `FeatureId`. Identity-semantic (not `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlacementModifierTypeId {
    /// The per-type `u32` identity (insertion index in the modifier-type registry).
    pub id: u32,
    /// The registry-key location of the type's registration (`register("count",
    /// …)` → `minecraft:count`).
    pub location: &'static str,
}

impl PlacementModifierTypeId {
    /// `new PlacementModifierTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> PlacementModifierTypeId {
        PlacementModifierTypeId { id, location }
    }
}

/// `net.minecraft.world.level.levelgen.placement.PlacementModifierType<P>` —
/// the object-safe carrier of a modifier type's identity.
///
/// `P` is erased in Rust (like the `Feature` half of `ConfiguredFeature`); the
/// per-type `MapCodec<P>` (`PlacementModifierType.codec()`) lands with the
/// codec surface (`#126`) and the `#181` generated table. Until then this is a
/// marker: concrete modifier structs report their `PlacementModifierTypeId`
/// from `PlacementModifier::type_id`, and the registry holds the uniform
/// behavior-bearing reference.
pub trait PlacementModifierType: Debug + Send + Sync + 'static {}

/// Resolve a `PlacementModifierTypeId` to its type behavior.
///
/// STUB(mc.world.level.levelgen.placement.core) — the generated
/// `BuiltInRegistries.PLACEMENT_MODIFIER_TYPE` table (emitted by `rivet-codegen`
/// per the `#181` manifest note). Panics unconditionally until the table is
/// wired; once wired, an unresolvable id throws `IllegalStateException` like
/// Java's `Registry.getValueOrThrow` (which throws only when the key is
/// genuinely missing).
pub fn placement_modifier_type(
    _id: &PlacementModifierTypeId,
) -> &'static dyn PlacementModifierType {
    // The generated table is not wired yet — this unconditional panic is the
    // pre-wire stand-in for the generated dispatch's unknown-id `getValueOrThrow`.
    panic!("Trying to access placement modifier type with no registered behavior (#181 codegen)")
}
