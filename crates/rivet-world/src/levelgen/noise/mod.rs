//! `net.minecraft.world.level.levelgen` — the density-function/noise-router
//! value slice (issue #177, `mc.world.level.levelgen.noise` unit).
//!
//! This slice ports the coherent Paper 26.2 density-function/noise-router
//! value layer from the six `levelgen` classes:
//!
//! - [`Density`] — the three surface/density sentinel constants.
//! - [`DensityFunction`] — the behavior contract (`compute`/`fillArray`/
//!   `mapChildren`/`minValue`/`maxValue`/`codec`), its `CODEC`
//!   (`RegistryFileCodec` over `Registries.DENSITY_FUNCTION`), the
//!   `ContextProvider`/`FunctionContext`/`NoiseHolder`/`SimpleFunction`/
//!   `SinglePointContext`/`Visitor` nested types, and the default combinator
//!   methods (`clamp`/`abs`/`square`/`cube`/`halfNegative`/`quarterNegative`/
//!   `invert`/`squeeze`).
//! - [`DensityFunctions`] — the dispatch hub: the `DIRECT_CODEC` (either
//!   constant or type-dispatched), the `bootstrap` registration order of the
//!   density-function types, and the concrete value functions (`Constant`,
//!   `Marker`, `Mapped`, `TwoArgumentSimpleFunction`/`Ap2`/`MulOrAdd`,
//!   `Clamp`, `RangeChoice`, `IntervalSelect`, `Shift`/`ShiftA`/`ShiftB`/
//!   `ShiftNoise`, `ShiftedNoise`, `Noise`, `Spline`, `YClampedGradient`,
//!   `FindTopSurface`, `EndIslandDensityFunction`, `BlendAlpha`, `BlendOffset`,
//!   `HolderHolder`, `BeardifierMarker` + the `BeardifierOrMarker` codec shell)
//!   plus the `Type` enum identity (`DensityFunctionsType`).
//! - [`NoiseRouter`] — the 15-field record + `CODEC` + `mapAll`.
//! - [`NoiseSettings`] — the minY/height/size record + `CODEC` + `guardY` +
//!   the five dimension constants + cell helpers.
//! - [`Noises`] — the `ResourceKey<NoiseParameters>` constants and
//!   `instantiate`.
//!
//! ## Reused layers
//!
//! The `Heightmap`, `VerticalAnchor`, `WorldGenerationContext`, `random`,
//! `synth`, `registry`, `holder`, `spline` (`rivet-util::cubic_spline`), and
//! `codec` (`rivet-serialization`) layers were ported by their owning units and
//! are NOT re-ported here. `QuartPos` (the `NoiseSettings.getCellHeight`/
//! `getCellWidth` helper) is a pure value leaf added to
//! `rivet-registry::core` (this unit's minimal prerequisite).
//!
//! ## Minimal registry additions (this unit)
//!
//! `Registries.NOISE` / `DENSITY_FUNCTION` / `DENSITY_FUNCTION_TYPE` — the
//! typed registry keys — are declared in [`registry_keys`]. Their element
//! types are `rivet-world` types (`NoiseParameters` value lives in `synth`,
//! the `DENSITY_FUNCTION` element is the erased `Arc<dyn DensityFunction>`
//! carrier, the `DENSITY_FUNCTION_TYPE` element is the [`DensityFunctionTypeId`]
//! identity), so they cannot be declared in `rivet-registry::registries` (a
//! Cargo cycle); the placeholder `STUB`s that previously lived there were
//! removed when this unit landed. `DensityFunctionType` identity/dispatch
//! lives here in `density_function_type`.
//!
//! ## Deferred seams (sparse issue-linked markers)
//!
//! - `BeardifierMarker` is ported as the value shell (`compute`/`fillArray`/
//!   `min`/`max` return the zero/empty surface Java's `DensityFunctions`
//!   declares, and `BeardifierOrMarker` carries its unit codec), but the real
//!   `Beardifier` structure (the `BEARD_KERNEL` contributions, `Rigid`,
//!   `forStructuresInChunk`) defers to the `structure` unit —
//!   `RivetTodo(#177)`, see `beardifier_marker`.
//! - `Column` (the `levelgen` class) is a separate manifest unit whose
//!   `LevelSimulatedReader` dependency is not ported; the `FindTopSurface`
//!   search loop and `NoiseRouter` do not reference it, so nothing here defers
//!   on it. The full 12-file manifest unit stays incomplete until the
//!   `mc.world.level.levelgen.noise` manifest row is split accordingly.
//! - The `EndIslandDensityFunction` Paper `NoiseCache` is ported faithfully
//!   (the 8192-entry chunk-key cache) and pins Paper's default
//!   `configFixMC159283()` = `true` (the long-sqrt path); the configurable
//!   disable path is deferred with `PlatformHooks` (RivetTodo #177).

pub mod beardifier_marker;
pub mod density;
pub mod density_function;
pub mod density_function_type;
pub mod density_functions;
pub mod noise_router;
pub mod noise_settings;
pub mod noises;
pub mod registry_keys;

pub use beardifier_marker::BeardifierMarker;
pub use density::Density;
pub use density_function::{
    ContextProvider, DensityFunction, FunctionContext, NoiseHolder, SinglePointContext, Visitor,
};
pub use density_function_type::DensityFunctionTypeId;
pub use noise_router::NoiseRouter;
pub use noise_settings::NoiseSettings;
pub use noises::Noises;
