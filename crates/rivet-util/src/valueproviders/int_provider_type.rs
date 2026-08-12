//! Port of the `IntProvider` type registry — the `minecraft:int_provider_type`
//! provider-type registry (26.2).
//!
//! Java's `IntProviders.bootstrap` registers the seven concrete `IntProvider`
//! `MapCodec`s into `BuiltInRegistries.INT_PROVIDER_TYPE` in this exact order;
//! `IntProviders.CODEC` dispatches on the by-name registry codec. The Rust port
//! replaces the registry with a closed string-based namespaced lookup (see
//! `valueproviders`' module doc) that reproduces Paper's registry contents and
//! declaration order: element id == insertion index, keyed by the
//! `minecraft:`-namespaced name.

/// The `MapCodec<? extends IntProvider>` registry element identity — the
/// per-type `u32` id (insertion index in the provider-type registry) plus its
/// registry-key location, mirroring `HeightProviderTypeId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntProviderTypeId {
    /// The per-type `u32` identity (insertion index in the provider-type registry).
    pub id: u32,
    /// The registry-key location of the type's registration
    /// (`register("constant", …)` → `minecraft:constant`).
    pub location: &'static str,
}

impl IntProviderTypeId {
    /// `new IntProviderTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> IntProviderTypeId {
        IntProviderTypeId { id, location }
    }
}

/// The seven `IntProviderType` constants — `IntProviders.bootstrap`'s exact
/// registration order (the `BuiltInRegistries.INT_PROVIDER_TYPE` insertion
/// order, so element ids 0..=6).
pub struct IntProviderTypes;
impl IntProviderTypes {
    /// `register("constant", ConstantInt.CODEC)`.
    pub const CONSTANT: IntProviderTypeId = IntProviderTypeId::new(0, "minecraft:constant");
    /// `register("uniform", UniformInt.CODEC)`.
    pub const UNIFORM: IntProviderTypeId = IntProviderTypeId::new(1, "minecraft:uniform");
    /// `register("biased_to_bottom", BiasedToBottomInt.CODEC)`.
    pub const BIASED_TO_BOTTOM: IntProviderTypeId =
        IntProviderTypeId::new(2, "minecraft:biased_to_bottom");
    /// `register("clamped", ClampedInt.CODEC)`.
    pub const CLAMPED: IntProviderTypeId = IntProviderTypeId::new(3, "minecraft:clamped");
    /// `register("weighted_list", WeightedListInt.CODEC)`.
    pub const WEIGHTED_LIST: IntProviderTypeId =
        IntProviderTypeId::new(4, "minecraft:weighted_list");
    /// `register("clamped_normal", ClampedNormalInt.CODEC)`.
    pub const CLAMPED_NORMAL: IntProviderTypeId =
        IntProviderTypeId::new(5, "minecraft:clamped_normal");
    /// `register("trapezoid", TrapezoidInt.CODEC)`.
    pub const TRAPEZOID: IntProviderTypeId = IntProviderTypeId::new(6, "minecraft:trapezoid");
}

/// `BuiltInRegistries.INT_PROVIDER_TYPE.get(Identifier)` — resolve a
/// registry-key location to its type id. All seven Paper entries are
/// registered (matching Java's `registerSimple`-populated registry), so every
/// known location resolves.
pub fn int_provider_type_by_name(name: &str) -> Option<IntProviderTypeId> {
    match name {
        "minecraft:constant" => Some(IntProviderTypes::CONSTANT),
        "minecraft:uniform" => Some(IntProviderTypes::UNIFORM),
        "minecraft:biased_to_bottom" => Some(IntProviderTypes::BIASED_TO_BOTTOM),
        "minecraft:clamped" => Some(IntProviderTypes::CLAMPED),
        "minecraft:weighted_list" => Some(IntProviderTypes::WEIGHTED_LIST),
        "minecraft:clamped_normal" => Some(IntProviderTypes::CLAMPED_NORMAL),
        "minecraft:trapezoid" => Some(IntProviderTypes::TRAPEZOID),
        _ => None,
    }
}
