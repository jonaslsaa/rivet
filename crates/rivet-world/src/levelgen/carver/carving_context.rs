//! Port of `net.minecraft.world.level.levelgen.carver.CarvingContext` (class,
//! 26.2) — the `WorldGenerationContext` subclass the carvers resolve heights
//! and lava levels against.
//!
//! Java: `CarvingContext extends WorldGenerationContext` and adds the
//! `registryAccess`, `noiseChunk`, `randomState` and `surfaceRule` fields, the
//! `@Deprecated topMaterial(...)` helper and the `registryAccess()`/
//! `randomState()` accessors. The `super(generator, heightAccessor, level)`
//! call folds into the embedded [`WorldGenerationContext`] (Java's Paper
//! `level()` accessor is omitted like the base's — RivetTodo(#232), the
//! `Level` type is the world unit's and no carver reads it).
//!
//! What the carvers actually consume of `CarvingContext`:
//! - `getMinGenY()`/`getGenDepth()` (the `WorldGenerationContext` minY/height
//!   window, via the embedded base) — `CaveWorldCarver`'s `configuration.y
//!   .sample(random, context)`, `NetherWorldCarver.carveBlock`'s
//!   `context.getMinGenY() + 31`, `CanyonWorldCarver`'s `yIndex` math and
//!   `initWidthFactors` depth.
//! - `topMaterial(biomeGetter, chunk, pos, underFluid)` — `WorldCarver.
//!   carveBlock`'s grass-block replacement under a carved surface.
//! - `randomState()` — the public accessor (no concrete carver reads it
//!   today; it is part of the class surface).
//!
//! The `registryAccess`/`noiseChunk`/`surfaceRule` fields exist in Java only
//! to feed `topMaterial`'s `randomState.surfaceSystem().topMaterial(...)`
//! call. The port keeps those fields out of the struct and instead exposes
//! `topMaterial` as a **typed closure seam** — the caller binds a closure that
//! captures its `ruleSource`/`noiseChunk`/`biomeGetter` plus the shared
//! [`SurfaceSystem`](crate::levelgen::surface_rules::SurfaceSystem), and an
//! unbound seam returns `None`, Java's `Optional.empty()` when the surface
//! system yields no replacement. The closure rides the `CarvingContext`'s
//! lifetime (`TopMaterialFn<'a>`) so it can capture the borrowed
//! `RandomState`; the surface unit's
//! [`bind_carver_top_material`](crate::levelgen::surface_rules::bind_carver_top_material)
//! constructs it once the surface system is available (see `surface_rules.rs`).

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::height_accessor::LevelHeightAccessor;
use crate::levelgen::noisegen::random_state::RandomState;
use crate::levelgen::world_generation_context::WorldGenerationContext;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use std::sync::Arc;

/// `CarvingContext.topMaterial`'s surface-system call — the typed seam for the
/// `@Deprecated` `randomState.surfaceSystem().topMaterial(this.surfaceRule,
/// this, biomeGetter, chunk, this.noiseChunk, pos, underFluid)`.
///
/// The closure returns the surface replacement for a block position (Java's
/// `Optional<BlockState>`); `None` means "no replacement" (`Optional.empty()`).
/// The `'a` is the `RandomState` borrow the closure captures (the
/// `SurfaceSystem` probe's `Context` carries it); the surface unit's
/// [`SurfaceSystem::top_material`](crate::levelgen::surface_rules::SurfaceSystem::top_material)
/// is the bound implementation. The seam is unbound (`None`) until bound.
pub type TopMaterialFn<'a> = dyn Fn(&BlockPos, bool) -> Option<BlockState> + Send + Sync + 'a;

/// `net.minecraft.world.level.levelgen.carver.CarvingContext`.
pub struct CarvingContext<'a> {
    /// The embedded `WorldGenerationContext` base (Java's `super`).
    world: WorldGenerationContext,
    /// `randomState` — `randomState()`.
    random_state: &'a RandomState<'a>,
    /// `topMaterial` seam — `None` when the surface system is not bound.
    top_material: Option<Arc<TopMaterialFn<'a>>>,
}

impl<'a> CarvingContext<'a> {
    /// `new CarvingContext(NoiseBasedChunkGenerator generator, RegistryAccess,
    /// LevelHeightAccessor heightAccessor, NoiseChunk, RandomState,
    /// SurfaceRules.RuleSource, Level)` — the constructor. The Rust
    /// `generator` is `&dyn ChunkGenerator` (the `WorldGenerationContext::new`
    /// surface); `registryAccess`/`noiseChunk`/`surfaceRule` are consumed by
    /// the `top_material` seam and not stored.
    pub fn new(
        generator: &dyn ChunkGenerator,
        height_accessor: &dyn LevelHeightAccessor,
        random_state: &'a RandomState<'a>,
    ) -> Self {
        CarvingContext {
            world: WorldGenerationContext::new(generator, height_accessor),
            random_state,
            top_material: None,
        }
    }

    /// `getMinGenY()` — delegated to the embedded base.
    pub fn get_min_gen_y(&self) -> i32 {
        self.world.get_min_gen_y()
    }

    /// `getGenDepth()` — delegated to the embedded base.
    pub fn get_gen_depth(&self) -> i32 {
        self.world.get_gen_depth()
    }

    /// The embedded `WorldGenerationContext` base — the `VerticalAnchor.
    /// resolveY`/`HeightProvider.sample` receiver (Java's `this` after the
    /// `super(generator, heightAccessor)` call).
    pub fn world_context(&self) -> &WorldGenerationContext {
        &self.world
    }

    /// `randomState()`. Returns the full `'a` borrow (not shortened to the
    /// `&self` borrow) so a bound seam closure can capture it — the reference
    /// is a `Copy` field whose validity is independent of the `&self` borrow.
    pub fn random_state(&self) -> &'a RandomState<'a> {
        self.random_state
    }

    /// Bind the `topMaterial` surface seam. Java's `topMaterial` is always
    /// available (it delegates to `surfaceSystem`), so binding is a one-way
    /// set; the surface unit's `bind_carver_top_material` is the caller.
    pub fn set_top_material(&mut self, top_material: Arc<TopMaterialFn<'a>>) {
        self.top_material = Some(top_material);
    }

    /// `topMaterial(Function<BlockPos, Holder<Biome>>, ChunkAccess, BlockPos,
    /// boolean)` — the `@Deprecated` grass-replacement helper. The
    /// `biomeGetter`/`chunk` are consumed by the bound closure; an unbound
    /// seam returns `None` (Java's `Optional.empty()` — no replacement).
    pub fn top_material(&self, pos: &BlockPos, under_fluid: bool) -> Option<BlockState> {
        self.top_material.as_ref().and_then(|f| f(pos, under_fluid))
    }
}
