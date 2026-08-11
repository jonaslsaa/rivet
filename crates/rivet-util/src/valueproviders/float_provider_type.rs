//! Port of the `FloatProvider` type registry — the `minecraft:float_provider_type`
//! provider-type registry (26.2).
//!
//! Java's `FloatProviders.bootstrap` registers the four concrete
//! `FloatProvider` `MapCodec`s into `BuiltInRegistries.FLOAT_PROVIDER_TYPE` in
//! this exact order; `FloatProviders.CODEC` dispatches on the by-name registry
//! codec. The Rust port replaces the registry with a closed string-based
//! namespaced lookup (see `valueproviders`' module doc) that reproduces Paper's
//! registry contents and declaration order: element id == insertion index,
//! keyed by the `minecraft:`-namespaced name.

/// The `MapCodec<? extends FloatProvider>` registry element identity — the
/// per-type `u32` id (insertion index in the provider-type registry) plus its
/// registry-key location, mirroring `IntProviderTypeId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FloatProviderTypeId {
    /// The per-type `u32` identity (insertion index in the provider-type registry).
    pub id: u32,
    /// The registry-key location of the type's registration
    /// (`register("constant", …)` → `minecraft:constant`).
    pub location: &'static str,
}

impl FloatProviderTypeId {
    /// `new FloatProviderTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> FloatProviderTypeId {
        FloatProviderTypeId { id, location }
    }
}

/// The four `FloatProviderType` constants — `FloatProviders.bootstrap`'s exact
/// registration order (the `BuiltInRegistries.FLOAT_PROVIDER_TYPE` insertion
/// order, so element ids 0..=3).
pub struct FloatProviderTypes;
impl FloatProviderTypes {
    /// `register("constant", ConstantFloat.CODEC)`.
    pub const CONSTANT: FloatProviderTypeId = FloatProviderTypeId::new(0, "minecraft:constant");
    /// `register("uniform", UniformFloat.CODEC)`.
    pub const UNIFORM: FloatProviderTypeId = FloatProviderTypeId::new(1, "minecraft:uniform");
    /// `register("clamped_normal", ClampedNormalFloat.CODEC)`.
    pub const CLAMPED_NORMAL: FloatProviderTypeId =
        FloatProviderTypeId::new(2, "minecraft:clamped_normal");
    /// `register("trapezoid", TrapezoidFloat.CODEC)`.
    pub const TRAPEZOID: FloatProviderTypeId = FloatProviderTypeId::new(3, "minecraft:trapezoid");
}

/// `BuiltInRegistries.FLOAT_PROVIDER_TYPE.get(Identifier)` — resolve a
/// registry-key location to its type id. All four Paper entries are registered
/// (matching Java's `registerSimple`-populated registry), so every known
/// location resolves.
pub fn float_provider_type_by_name(name: &str) -> Option<FloatProviderTypeId> {
    match name {
        "minecraft:constant" => Some(FloatProviderTypes::CONSTANT),
        "minecraft:uniform" => Some(FloatProviderTypes::UNIFORM),
        "minecraft:clamped_normal" => Some(FloatProviderTypes::CLAMPED_NORMAL),
        "minecraft:trapezoid" => Some(FloatProviderTypes::TRAPEZOID),
        _ => None,
    }
}
