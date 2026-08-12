//! The `BuiltInRegistries.DENSITY_FUNCTION_TYPE` identity (issue #177).
//!
//! Java's `DensityFunctions.CODEC` dispatches on
//! `BuiltInRegistries.DENSITY_FUNCTION_TYPE.byNameCodec()` — the registry whose
//! elements are the per-type `MapCodec<? extends DensityFunction>`. Rust cannot
//! use a `MapCodec` (a trait object) as a registry key, so this module mirrors
//! the `BlockPredicateTypeId` identity split: the type identity is the opaque
//! [`DensityFunctionTypeId`] handle (element id == insertion index ==
//! `bootstrap` registration order), and the per-type `MapCodec`s are resolved
//! by the `#177` dispatch table in `density_functions`, not stored on the id.
//!
//! The ids reproduce Paper's exact `bootstrap` registration order (see
//! `density_functions::bootstrap`): the 6 marker types, then the 7 mapped
//! types, then the 4 two-argument types, interleaved with the standalone
//! registrations exactly as the Java `register(...)` calls sequence them.

use std::fmt::Debug;

/// The `DensityFunctionType<P>` registry element identity — the per-type `u32`
/// id (element id == holder id == insertion index) plus its registry-key
/// location, mirroring `BlockPredicateTypeId`/`FeatureId`. Identity-semantic
/// (not `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DensityFunctionTypeId {
    /// The per-type `u32` identity (insertion index in the
    /// `DENSITY_FUNCTION_TYPE` registry, Paper's `bootstrap` order).
    pub id: u32,
    /// The registry-key location of the type's registration
    /// (`register("clamp", …)` → `minecraft:clamp`).
    pub location: &'static str,
}

impl DensityFunctionTypeId {
    /// `new DensityFunctionTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> DensityFunctionTypeId {
        DensityFunctionTypeId { id, location }
    }
}

/// The `BuiltInRegistries.DENSITY_FUNCTION_TYPE` constants — Paper's exact
/// `DensityFunctions.bootstrap` registration order (element ids 0..=33).
///
/// The ids mirror `bootstrap`'s sequence: the four leading registrations
/// (`blend_alpha`, `blend_offset`, `beardifier`, `old_blended_noise`), then the
/// six `Marker.Type` values in enum order, then `noise`/`end_islands`/
/// `shifted_noise`/`range_choice`/`interval_select`/`shift_a`/`shift_b`/`shift`/
/// `clamp`, then the seven `Mapped.Type` values, then the four
/// `TwoArgumentSimpleFunction.Type` values, then `spline`/`constant`/
/// `y_clamped_gradient`/`find_top_surface`.
pub struct DensityFunctionTypes;
impl DensityFunctionTypes {
    /// `register("blend_alpha", BlendAlpha.CODEC)`.
    pub const BLEND_ALPHA: DensityFunctionTypeId =
        DensityFunctionTypeId::new(0, "minecraft:blend_alpha");
    /// `register("blend_offset", BlendOffset.CODEC)`.
    pub const BLEND_OFFSET: DensityFunctionTypeId =
        DensityFunctionTypeId::new(1, "minecraft:blend_offset");
    /// `register("beardifier", BeardifierMarker.CODEC)`.
    pub const BEARDIFIER: DensityFunctionTypeId =
        DensityFunctionTypeId::new(2, "minecraft:beardifier");
    /// `register("old_blended_noise", BlendedNoise.CODEC)`.
    pub const OLD_BLENDED_NOISE: DensityFunctionTypeId =
        DensityFunctionTypeId::new(3, "minecraft:old_blended_noise");

    // The six `Marker.Type` values (enum declaration order).
    /// `register("interpolated", Marker.Type.Interpolated.codec)`.
    pub const INTERPOLATED: DensityFunctionTypeId =
        DensityFunctionTypeId::new(4, "minecraft:interpolated");
    /// `register("flat_cache", Marker.Type.FlatCache.codec)`.
    pub const FLAT_CACHE: DensityFunctionTypeId =
        DensityFunctionTypeId::new(5, "minecraft:flat_cache");
    /// `register("cache_2d", Marker.Type.Cache2D.codec)`.
    pub const CACHE_2D: DensityFunctionTypeId = DensityFunctionTypeId::new(6, "minecraft:cache_2d");
    /// `register("cache_once", Marker.Type.CacheOnce.codec)`.
    pub const CACHE_ONCE: DensityFunctionTypeId =
        DensityFunctionTypeId::new(7, "minecraft:cache_once");
    /// `register("cache_all_in_cell", Marker.Type.CacheAllInCell.codec)`.
    pub const CACHE_ALL_IN_CELL: DensityFunctionTypeId =
        DensityFunctionTypeId::new(8, "minecraft:cache_all_in_cell");
    /// `register("blend_density", Marker.Type.BlendDensity.codec)`.
    pub const BLEND_DENSITY: DensityFunctionTypeId =
        DensityFunctionTypeId::new(9, "minecraft:blend_density");

    /// `register("noise", Noise.CODEC)`.
    pub const NOISE: DensityFunctionTypeId = DensityFunctionTypeId::new(10, "minecraft:noise");
    /// `register("end_islands", EndIslandDensityFunction.CODEC)`.
    pub const END_ISLANDS: DensityFunctionTypeId =
        DensityFunctionTypeId::new(11, "minecraft:end_islands");
    /// `register("shifted_noise", ShiftedNoise.CODEC)`.
    pub const SHIFTED_NOISE: DensityFunctionTypeId =
        DensityFunctionTypeId::new(12, "minecraft:shifted_noise");
    /// `register("range_choice", RangeChoice.CODEC)`.
    pub const RANGE_CHOICE: DensityFunctionTypeId =
        DensityFunctionTypeId::new(13, "minecraft:range_choice");
    /// `register("interval_select", IntervalSelect.CODEC)`.
    pub const INTERVAL_SELECT: DensityFunctionTypeId =
        DensityFunctionTypeId::new(14, "minecraft:interval_select");
    /// `register("shift_a", ShiftA.CODEC)`.
    pub const SHIFT_A: DensityFunctionTypeId = DensityFunctionTypeId::new(15, "minecraft:shift_a");
    /// `register("shift_b", ShiftB.CODEC)`.
    pub const SHIFT_B: DensityFunctionTypeId = DensityFunctionTypeId::new(16, "minecraft:shift_b");
    /// `register("shift", Shift.CODEC)`.
    pub const SHIFT: DensityFunctionTypeId = DensityFunctionTypeId::new(17, "minecraft:shift");
    /// `register("clamp", Clamp.CODEC)`.
    pub const CLAMP: DensityFunctionTypeId = DensityFunctionTypeId::new(18, "minecraft:clamp");

    // The seven `Mapped.Type` values (enum declaration order).
    /// `register("abs", Mapped.Type.ABS.codec)`.
    pub const ABS: DensityFunctionTypeId = DensityFunctionTypeId::new(19, "minecraft:abs");
    /// `register("square", Mapped.Type.SQUARE.codec)`.
    pub const SQUARE: DensityFunctionTypeId = DensityFunctionTypeId::new(20, "minecraft:square");
    /// `register("cube", Mapped.Type.CUBE.codec)`.
    pub const CUBE: DensityFunctionTypeId = DensityFunctionTypeId::new(21, "minecraft:cube");
    /// `register("half_negative", Mapped.Type.HALF_NEGATIVE.codec)`.
    pub const HALF_NEGATIVE: DensityFunctionTypeId =
        DensityFunctionTypeId::new(22, "minecraft:half_negative");
    /// `register("quarter_negative", Mapped.Type.QUARTER_NEGATIVE.codec)`.
    pub const QUARTER_NEGATIVE: DensityFunctionTypeId =
        DensityFunctionTypeId::new(23, "minecraft:quarter_negative");
    /// `register("invert", Mapped.Type.INVERT.codec)`.
    pub const INVERT: DensityFunctionTypeId = DensityFunctionTypeId::new(24, "minecraft:invert");
    /// `register("squeeze", Mapped.Type.SQUEEZE.codec)`.
    pub const SQUEEZE: DensityFunctionTypeId = DensityFunctionTypeId::new(25, "minecraft:squeeze");

    // The four `TwoArgumentSimpleFunction.Type` values (enum declaration order).
    /// `register("add", TwoArgumentSimpleFunction.Type.ADD.codec)`.
    pub const ADD: DensityFunctionTypeId = DensityFunctionTypeId::new(26, "minecraft:add");
    /// `register("mul", TwoArgumentSimpleFunction.Type.MUL.codec)`.
    pub const MUL: DensityFunctionTypeId = DensityFunctionTypeId::new(27, "minecraft:mul");
    /// `register("min", TwoArgumentSimpleFunction.Type.MIN.codec)`.
    pub const MIN: DensityFunctionTypeId = DensityFunctionTypeId::new(28, "minecraft:min");
    /// `register("max", TwoArgumentSimpleFunction.Type.MAX.codec)`.
    pub const MAX: DensityFunctionTypeId = DensityFunctionTypeId::new(29, "minecraft:max");

    /// `register("spline", Spline.CODEC)`.
    pub const SPLINE: DensityFunctionTypeId = DensityFunctionTypeId::new(30, "minecraft:spline");
    /// `register("constant", Constant.CODEC)`.
    pub const CONSTANT: DensityFunctionTypeId =
        DensityFunctionTypeId::new(31, "minecraft:constant");
    /// `register("y_clamped_gradient", YClampedGradient.CODEC)`.
    pub const Y_CLAMPED_GRADIENT: DensityFunctionTypeId =
        DensityFunctionTypeId::new(32, "minecraft:y_clamped_gradient");
    /// `register("find_top_surface", FindTopSurface.CODEC)`.
    pub const FIND_TOP_SURFACE: DensityFunctionTypeId =
        DensityFunctionTypeId::new(33, "minecraft:find_top_surface");
}

/// `BuiltInRegistries.DENSITY_FUNCTION_TYPE.get(Identifier)` — resolve a
/// registry-key location to its type id. All thirty-four Paper entries are
/// registered (matching the `registerSimple`-populated registry), so every
/// known location resolves.
pub fn density_function_type_by_name(name: &str) -> Option<DensityFunctionTypeId> {
    match name {
        "minecraft:blend_alpha" => Some(DensityFunctionTypes::BLEND_ALPHA),
        "minecraft:blend_offset" => Some(DensityFunctionTypes::BLEND_OFFSET),
        "minecraft:beardifier" => Some(DensityFunctionTypes::BEARDIFIER),
        "minecraft:old_blended_noise" => Some(DensityFunctionTypes::OLD_BLENDED_NOISE),
        "minecraft:interpolated" => Some(DensityFunctionTypes::INTERPOLATED),
        "minecraft:flat_cache" => Some(DensityFunctionTypes::FLAT_CACHE),
        "minecraft:cache_2d" => Some(DensityFunctionTypes::CACHE_2D),
        "minecraft:cache_once" => Some(DensityFunctionTypes::CACHE_ONCE),
        "minecraft:cache_all_in_cell" => Some(DensityFunctionTypes::CACHE_ALL_IN_CELL),
        "minecraft:blend_density" => Some(DensityFunctionTypes::BLEND_DENSITY),
        "minecraft:noise" => Some(DensityFunctionTypes::NOISE),
        "minecraft:end_islands" => Some(DensityFunctionTypes::END_ISLANDS),
        "minecraft:shifted_noise" => Some(DensityFunctionTypes::SHIFTED_NOISE),
        "minecraft:range_choice" => Some(DensityFunctionTypes::RANGE_CHOICE),
        "minecraft:interval_select" => Some(DensityFunctionTypes::INTERVAL_SELECT),
        "minecraft:shift_a" => Some(DensityFunctionTypes::SHIFT_A),
        "minecraft:shift_b" => Some(DensityFunctionTypes::SHIFT_B),
        "minecraft:shift" => Some(DensityFunctionTypes::SHIFT),
        "minecraft:clamp" => Some(DensityFunctionTypes::CLAMP),
        "minecraft:abs" => Some(DensityFunctionTypes::ABS),
        "minecraft:square" => Some(DensityFunctionTypes::SQUARE),
        "minecraft:cube" => Some(DensityFunctionTypes::CUBE),
        "minecraft:half_negative" => Some(DensityFunctionTypes::HALF_NEGATIVE),
        "minecraft:quarter_negative" => Some(DensityFunctionTypes::QUARTER_NEGATIVE),
        "minecraft:invert" => Some(DensityFunctionTypes::INVERT),
        "minecraft:squeeze" => Some(DensityFunctionTypes::SQUEEZE),
        "minecraft:add" => Some(DensityFunctionTypes::ADD),
        "minecraft:mul" => Some(DensityFunctionTypes::MUL),
        "minecraft:min" => Some(DensityFunctionTypes::MIN),
        "minecraft:max" => Some(DensityFunctionTypes::MAX),
        "minecraft:spline" => Some(DensityFunctionTypes::SPLINE),
        "minecraft:constant" => Some(DensityFunctionTypes::CONSTANT),
        "minecraft:y_clamped_gradient" => Some(DensityFunctionTypes::Y_CLAMPED_GRADIENT),
        "minecraft:find_top_surface" => Some(DensityFunctionTypes::FIND_TOP_SURFACE),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_bootstrap_order_and_ids() {
        // Element ids equal the insertion index in `DensityFunctions.bootstrap`
        // (Paper's exact registration order).
        assert_eq!(DensityFunctionTypes::BLEND_ALPHA.id, 0);
        assert_eq!(DensityFunctionTypes::BLEND_OFFSET.id, 1);
        assert_eq!(DensityFunctionTypes::BEARDIFIER.id, 2);
        assert_eq!(DensityFunctionTypes::OLD_BLENDED_NOISE.id, 3);
        assert_eq!(DensityFunctionTypes::INTERPOLATED.id, 4);
        assert_eq!(DensityFunctionTypes::FLAT_CACHE.id, 5);
        assert_eq!(DensityFunctionTypes::CACHE_2D.id, 6);
        assert_eq!(DensityFunctionTypes::CACHE_ONCE.id, 7);
        assert_eq!(DensityFunctionTypes::CACHE_ALL_IN_CELL.id, 8);
        assert_eq!(DensityFunctionTypes::BLEND_DENSITY.id, 9);
        assert_eq!(DensityFunctionTypes::NOISE.id, 10);
        assert_eq!(DensityFunctionTypes::END_ISLANDS.id, 11);
        assert_eq!(DensityFunctionTypes::SHIFTED_NOISE.id, 12);
        assert_eq!(DensityFunctionTypes::RANGE_CHOICE.id, 13);
        assert_eq!(DensityFunctionTypes::INTERVAL_SELECT.id, 14);
        assert_eq!(DensityFunctionTypes::SHIFT_A.id, 15);
        assert_eq!(DensityFunctionTypes::SHIFT_B.id, 16);
        assert_eq!(DensityFunctionTypes::SHIFT.id, 17);
        assert_eq!(DensityFunctionTypes::CLAMP.id, 18);
        assert_eq!(DensityFunctionTypes::ABS.id, 19);
        assert_eq!(DensityFunctionTypes::SQUARE.id, 20);
        assert_eq!(DensityFunctionTypes::CUBE.id, 21);
        assert_eq!(DensityFunctionTypes::HALF_NEGATIVE.id, 22);
        assert_eq!(DensityFunctionTypes::QUARTER_NEGATIVE.id, 23);
        assert_eq!(DensityFunctionTypes::INVERT.id, 24);
        assert_eq!(DensityFunctionTypes::SQUEEZE.id, 25);
        assert_eq!(DensityFunctionTypes::ADD.id, 26);
        assert_eq!(DensityFunctionTypes::MUL.id, 27);
        assert_eq!(DensityFunctionTypes::MIN.id, 28);
        assert_eq!(DensityFunctionTypes::MAX.id, 29);
        assert_eq!(DensityFunctionTypes::SPLINE.id, 30);
        assert_eq!(DensityFunctionTypes::CONSTANT.id, 31);
        assert_eq!(DensityFunctionTypes::Y_CLAMPED_GRADIENT.id, 32);
        assert_eq!(DensityFunctionTypes::FIND_TOP_SURFACE.id, 33);
    }

    #[test]
    fn paper_registry_key_locations() {
        assert_eq!(DensityFunctionTypes::CLAMP.location, "minecraft:clamp");
        assert_eq!(
            DensityFunctionTypes::BLEND_DENSITY.location,
            "minecraft:blend_density"
        );
        assert_eq!(
            DensityFunctionTypes::Y_CLAMPED_GRADIENT.location,
            "minecraft:y_clamped_gradient"
        );
        assert_eq!(
            DensityFunctionTypes::OLD_BLENDED_NOISE.location,
            "minecraft:old_blended_noise"
        );
        assert_eq!(
            DensityFunctionTypes::FIND_TOP_SURFACE.location,
            "minecraft:find_top_surface"
        );
    }

    #[test]
    fn by_name_resolves_every_registered_type() {
        let all = [
            DensityFunctionTypes::BLEND_ALPHA,
            DensityFunctionTypes::BLEND_OFFSET,
            DensityFunctionTypes::BEARDIFIER,
            DensityFunctionTypes::OLD_BLENDED_NOISE,
            DensityFunctionTypes::INTERPOLATED,
            DensityFunctionTypes::FLAT_CACHE,
            DensityFunctionTypes::CACHE_2D,
            DensityFunctionTypes::CACHE_ONCE,
            DensityFunctionTypes::CACHE_ALL_IN_CELL,
            DensityFunctionTypes::BLEND_DENSITY,
            DensityFunctionTypes::NOISE,
            DensityFunctionTypes::END_ISLANDS,
            DensityFunctionTypes::SHIFTED_NOISE,
            DensityFunctionTypes::RANGE_CHOICE,
            DensityFunctionTypes::INTERVAL_SELECT,
            DensityFunctionTypes::SHIFT_A,
            DensityFunctionTypes::SHIFT_B,
            DensityFunctionTypes::SHIFT,
            DensityFunctionTypes::CLAMP,
            DensityFunctionTypes::ABS,
            DensityFunctionTypes::SQUARE,
            DensityFunctionTypes::CUBE,
            DensityFunctionTypes::HALF_NEGATIVE,
            DensityFunctionTypes::QUARTER_NEGATIVE,
            DensityFunctionTypes::INVERT,
            DensityFunctionTypes::SQUEEZE,
            DensityFunctionTypes::ADD,
            DensityFunctionTypes::MUL,
            DensityFunctionTypes::MIN,
            DensityFunctionTypes::MAX,
            DensityFunctionTypes::SPLINE,
            DensityFunctionTypes::CONSTANT,
            DensityFunctionTypes::Y_CLAMPED_GRADIENT,
            DensityFunctionTypes::FIND_TOP_SURFACE,
        ];
        for id in all {
            assert_eq!(density_function_type_by_name(id.location), Some(id));
        }
    }

    #[test]
    fn by_name_unknown_location_is_none() {
        assert_eq!(density_function_type_by_name("minecraft:nope"), None);
        assert_eq!(density_function_type_by_name("clamp"), None);
    }
}
