//! Port of `net.minecraft.world.level.levelgen.NoiseChunk` (26.2).
//!
//! The per-chunk interpolation context: `NoiseChunk` implements
//! `FunctionContext` + `ContextProvider`, the `wrap` marker dispatch, the
//! `preliminarySurfaceLevel` `ColumnPos`-keyed cache, and the inner
//! `NoiseInterpolator`/`FlatCache`/`Cache2D`/`CacheOnce`/`CacheAllInCell`/
//! `BlendAlpha`/`BlendDensity`/`BlendOffset` density functions.
//!
//! Translation notes (the ownership-model seams):
//! - Java's inner classes are non-static and reference `NoiseChunk.this`; the
//!   port breaks the reference cycle by holding the mutable interpolation
//!   state in an `Arc<Mutex<InterpolationState>>` shared with the inner
//!   function structs. The `interpolators`/`cellCaches` lists live in that
//!   shared state so the `wrap` visitor (which constructs them during
//!   `router.mapAll`) registers them exactly like the Java inner-class
//!   constructors do. The per-instance mutable values (the interpolator's
//!   `noise000..value`, `Cache2D`/`CacheOnce` caches) live in their own
//!   `Mutex`es.
//! - Java's `context != NoiseChunk.this` reference-identity checks become the
//!   [`is_owning_chunk`] test: a downcast of the `FunctionContext` to
//!   `NoiseChunk` plus a shared-`InterpolationState` `Arc` identity compare.
//!   The `ContextProvider.forIndex` seam returns the owning chunk (`&self` /
//!   `&NoiseChunk.this`, exactly Java's `return this`) so every inner function
//!   reached through the per-index fill paths — `Ap2` Mul/Min/Max,
//!   `IntervalSelect`, `RangeChoice`, `BlendDensity` — evaluates with the chunk
//!   context and takes the interpolation/cache branch. A standalone
//!   `SinglePointContext` (outside the loop) never downcasts to the chunk, so
//!   the functions delegate to their noise filler exactly as Java delegates for
//!   a non-chunk context.
//! - `Beardifier.forStructuresInChunk` (the structure unit) defers: the
//!   `beardifier` field is the `BeardifierMarker` value shell (RivetTodo #177).
//! - `Aquifer.create`'s `noiseChunk` self-reference is broken with the
//!   `preliminary_surface_level` closure (see `aquifer.rs`).
//! - `MaterialRuleList` (`mc.world.level.levelgen.material`) is a STUB value
//!   struct here (the 2-line iteration; the owning material unit replaces it).

use crate::biome::{ParameterPoint, Sampler};
use crate::block::BlockState;
use crate::level::height_accessor::LevelHeightAccessor;
use crate::levelgen::blending::blender::{Blender, BlendingOutput};
use crate::levelgen::noise::beardifier_marker::BeardifierMarker;
use crate::levelgen::noise::density_function::{
    ContextProvider, DensityFunction, FunctionContext, IdentityKey, SinglePointContext, Visitor,
    map_all,
};
use crate::levelgen::noise::density_functions::{self as fns, HolderHolder, Marker, MarkerType};
use crate::levelgen::noise::noise_router::NoiseRouter;
use crate::levelgen::noisegen::aquifer::{self, Aquifer, FluidPicker, PreliminarySurfaceLevelFn};
use crate::levelgen::noisegen::column_pos::ColumnPos;
use crate::levelgen::noisegen::noise_generator_settings::NoiseGeneratorSettings;
use crate::levelgen::noisegen::ore_veinifier::create as create_ore_veinifier;
use crate::levelgen::noisegen::random_state::RandomState;
use rivet_registry::core::{ChunkPos, QuartPos, SectionPos};
use rivet_registry::holder::Holder;
use rivet_util::mth;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// `NoiseChunk.BlockStateFiller` — the `@FunctionalInterface` block-state
/// filler (`@Nullable BlockState calculate(FunctionContext)`).
pub trait BlockStateFiller: Send + Sync {
    /// `calculate(FunctionContext)` — `None` for Java's `null`.
    fn calculate(&self, context: &dyn FunctionContext) -> Option<BlockState>;
}

/// STUB(mc.world.level.levelgen.material) — `MaterialRuleList`, the
/// `NoiseChunk.BlockStateFiller` list. The owning material unit ports the real
/// class; the noisegen unit carries the 2-line iteration (the first non-`None`
/// filler wins).
pub struct MaterialRuleList {
    /// `rules` — the `NoiseChunk.BlockStateFiller[]`.
    pub rules: Vec<Arc<dyn BlockStateFiller>>,
}

impl MaterialRuleList {
    /// `MaterialRuleList(NoiseChunk.BlockStateFiller...)`.
    pub fn new(rules: Vec<Arc<dyn BlockStateFiller>>) -> Self {
        MaterialRuleList { rules }
    }
}

impl BlockStateFiller for MaterialRuleList {
    fn calculate(&self, context: &dyn FunctionContext) -> Option<BlockState> {
        for rule in &self.rules {
            if let Some(state) = rule.calculate(context) {
                return Some(state);
            }
        }
        None
    }
}

/// The mutable interpolation state shared between the `NoiseChunk`, its `wrap`
/// visitor, and the inner density-function structs (Java's `NoiseChunk.this`
/// mutable fields + the `interpolators`/`cellCaches` lists).
#[derive(Debug)]
pub struct InterpolationState {
    /// `interpolating`.
    interpolating: bool,
    /// `fillingCell`.
    filling_cell: bool,
    /// `cellStartBlockX`.
    cell_start_block_x: i32,
    /// `cellStartBlockY`.
    cell_start_block_y: i32,
    /// `cellStartBlockZ`.
    cell_start_block_z: i32,
    /// `inCellX`.
    in_cell_x: i32,
    /// `inCellY`.
    in_cell_y: i32,
    /// `inCellZ`.
    in_cell_z: i32,
    /// `interpolationCounter`.
    interpolation_counter: i64,
    /// `arrayInterpolationCounter`.
    array_interpolation_counter: i64,
    /// `arrayIndex`.
    array_index: usize,
    /// `lastBlendingDataPos`.
    last_blending_data_pos: i64,
    /// `lastBlendingOutput`.
    last_blending_output: BlendingOutput,
    /// `interpolators` — Java's `List<NoiseChunk.NoiseInterpolator>`.
    interpolators: Vec<Arc<NoiseInterpolator>>,
    /// `cellCaches` — Java's `List<NoiseChunk.CacheAllInCell>`.
    cell_caches: Vec<Arc<CacheAllInCell>>,
}

impl InterpolationState {
    fn new() -> Self {
        InterpolationState {
            interpolating: false,
            filling_cell: false,
            cell_start_block_x: 0,
            cell_start_block_y: 0,
            cell_start_block_z: 0,
            in_cell_x: 0,
            in_cell_y: 0,
            in_cell_z: 0,
            interpolation_counter: 0,
            array_interpolation_counter: 0,
            array_index: 0,
            last_blending_data_pos: ChunkPos::INVALID_CHUNK_POS,
            last_blending_output: BlendingOutput::new(1.0, 0.0),
            interpolators: Vec::new(),
            cell_caches: Vec::new(),
        }
    }

    /// `NoiseChunk.getOrComputeBlendingOutput(blockX, blockZ)` — the
    /// `lastBlendingDataPos`/`lastBlendingOutput` cache.
    fn get_or_compute_blending_output(
        &mut self,
        blender: &Blender,
        block_x: i32,
        block_z: i32,
    ) -> BlendingOutput {
        let pos_2d = ChunkPos::pack_coords(block_x, block_z);
        if self.last_blending_data_pos == pos_2d {
            return self.last_blending_output;
        }
        self.last_blending_data_pos = pos_2d;
        let output = blender.blend_offset_and_factor(block_x, block_z);
        self.last_blending_output = output;
        output
    }
}

/// `NoiseChunk` — the per-chunk interpolation context. The type implements
/// `FunctionContext` (the block coordinates resolve through the shared state)
/// and `ContextProvider`.
pub struct NoiseChunk {
    /// `cellCountXZ`.
    pub cell_count_xz: i32,
    /// `cellCountY`.
    pub cell_count_y: i32,
    /// `cellNoiseMinY`.
    pub cell_noise_min_y: i32,
    /// `firstCellX`.
    pub first_cell_x: i32,
    /// `firstCellZ`.
    pub first_cell_z: i32,
    /// `firstNoiseX`.
    pub first_noise_x: i32,
    /// `firstNoiseZ`.
    pub first_noise_z: i32,
    /// `noiseSizeXZ`.
    pub noise_size_xz: i32,
    /// `cellWidth`.
    pub cell_width: i32,
    /// `cellHeight`.
    pub cell_height: i32,
    /// `wrapped` — the `HashMap<DensityFunction, DensityFunction>` wrap cache.
    ///
    /// Shared (`Arc`) with the construction-time `wrap` visitor so
    /// `cachedClimateSampler` reuses the already-wrapped router fields — Java's
    /// single `this.wrapped` map used by both the constructor's `this::wrap`
    /// and `cachedClimateSampler`'s `this::wrap`.
    wrapped: Arc<Mutex<HashMap<IdentityKey, Arc<dyn DensityFunction>>>>,
    /// `blendAlpha` — the `@Nullable` blend-alpha flat cache (non-null iff the
    /// blender is non-empty).
    blend_alpha: Option<Arc<FlatCache>>,
    /// `blendOffset` — the `@Nullable` blend-offset flat cache.
    blend_offset: Option<Arc<FlatCache>>,
    /// `preliminarySurfaceLevelCache` — the `Long2IntMap` cache.
    preliminary_surface_level_cache: Mutex<HashMap<i64, i32>>,
    /// `aquifer`.
    aquifer: Arc<dyn Aquifer>,
    /// `preliminarySurfaceLevel` — the wrapped router function.
    preliminary_surface_level: Arc<dyn DensityFunction>,
    /// `fullNoiseDensity`.
    full_noise_density: Arc<dyn DensityFunction>,
    /// `blockStateRule` — the `MaterialRuleList` STUB.
    block_state_rule: Arc<dyn BlockStateFiller>,
    /// `blender`.
    blender: Blender,
    /// `beardifier` — the `BeardifierMarker` value shell (structure unit defers).
    beardifier: Arc<dyn DensityFunction>,
    /// The shared interpolation state (Java's `NoiseChunk.this` mutable fields).
    state: Arc<Mutex<InterpolationState>>,
}

impl std::fmt::Debug for NoiseChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NoiseChunk")
    }
}

impl NoiseChunk {
    /// `NoiseChunk.forChunk(chunk, randomState, beardifier, settings,
    /// globalFluidPicker, blender)`.
    ///
    /// `beardifier` is the erased `BeardifierOrMarker` — the structure unit's
    /// real `Beardifier` defers (RivetTodo #177); callers pass the
    /// `BeardifierMarker` value shell.
    #[allow(clippy::too_many_arguments)]
    pub fn for_chunk(
        chunk: &dyn LevelHeightAccessor,
        pos: &ChunkPos,
        random_state: &RandomState,
        beardifier: Arc<dyn DensityFunction>,
        settings: &NoiseGeneratorSettings,
        global_fluid_picker: Box<dyn FluidPicker>,
        blender: Blender,
    ) -> NoiseChunk {
        let noise_settings = settings.noise_settings.clamp_to_height_accessor(chunk);
        let cell_count_xz = 16 / noise_settings.get_cell_width();
        NoiseChunk::new(
            cell_count_xz,
            random_state,
            pos.get_min_block_x(),
            pos.get_min_block_z(),
            &noise_settings,
            beardifier,
            settings,
            global_fluid_picker,
            blender,
        )
    }

    /// The full `NoiseChunk(...)` constructor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cell_count_xz: i32,
        random_state: &RandomState,
        chunk_min_block_x: i32,
        chunk_min_block_z: i32,
        noise_settings: &crate::levelgen::noise::noise_settings::NoiseSettings,
        beardifier: Arc<dyn DensityFunction>,
        settings: &NoiseGeneratorSettings,
        global_fluid_picker: Box<dyn FluidPicker>,
        blender: Blender,
    ) -> NoiseChunk {
        let cell_width = noise_settings.get_cell_width();
        let cell_height = noise_settings.get_cell_height();
        let cell_count_y = mth::floor_div(noise_settings.height(), cell_height);
        let cell_noise_min_y = mth::floor_div(noise_settings.min_y(), cell_height);
        let first_cell_x = mth::floor_div(chunk_min_block_x, cell_width);
        let first_cell_z = mth::floor_div(chunk_min_block_z, cell_width);
        let first_noise_x = QuartPos::from_block(chunk_min_block_x);
        let first_noise_z = QuartPos::from_block(chunk_min_block_z);
        let noise_size_xz = QuartPos::from_block(cell_count_xz * cell_width);
        let state = Arc::new(Mutex::new(InterpolationState::new()));
        // Java's single `this.wrapped` map, shared by the constructor's
        // `this::wrap` visitor AND the chunk's `wrap`/`cachedClimateSampler`.
        let wrapped = Arc::new(Mutex::new(HashMap::new()));

        // The `wrap` visitor used for the router wiring (Java's `this::wrap`).
        let wrap_visitor = NoiseChunkWrap {
            state: state.clone(),
            blender: blender.clone(),
            beardifier: beardifier.clone(),
            blend_alpha: None,
            blend_offset: None,
            cell_width,
            cell_height,
            cell_count_y,
            cell_count_xz,
            first_cell_z,
            first_noise_x,
            first_noise_z,
            noise_size_xz,
            wrapped: wrapped.clone(),
        };

        // The `blendAlpha`/`blendOffset` flat caches (Java's constructor block).
        let (blend_alpha, blend_offset) = if !blender.is_empty() {
            let blend_alpha = Arc::new(FlatCache::new(
                Arc::new(BlendAlpha::new(state.clone(), blender.clone())),
                false,
                noise_size_xz,
                first_noise_x,
                first_noise_z,
            ));
            let blend_offset = Arc::new(FlatCache::new(
                Arc::new(BlendOffset::new(state.clone(), blender.clone())),
                false,
                noise_size_xz,
                first_noise_x,
                first_noise_z,
            ));
            for x in 0..=noise_size_xz {
                let quart_x = first_noise_x + x;
                let block_x = QuartPos::to_block(quart_x);
                for z in 0..=noise_size_xz {
                    let quart_z = first_noise_z + z;
                    let block_z = QuartPos::to_block(quart_z);
                    let blending_output = blender.blend_offset_and_factor(block_x, block_z);
                    blend_alpha.values.lock().unwrap()[(x + z * blend_alpha.size_xz) as usize] =
                        blending_output.alpha();
                    blend_offset.values.lock().unwrap()[(x + z * blend_offset.size_xz) as usize] =
                        blending_output.blending_offset();
                }
            }
            (Some(blend_alpha), Some(blend_offset))
        } else {
            (None, None)
        };
        let wrap_visitor = wrap_visitor.with_blend(blend_alpha.clone(), blend_offset.clone());

        let router = random_state.router();
        let wrapped_router = router.map_all(&wrap_visitor);
        let preliminary_surface_level = wrapped_router.preliminary_surface_level().clone();

        let aquifer = if !settings.is_aquifers_enabled() {
            aquifer::create_disabled(global_fluid_picker)
        } else {
            let chunk_x = SectionPos::block_to_section_coord(chunk_min_block_x);
            let chunk_z = SectionPos::block_to_section_coord(chunk_min_block_z);
            let preliminary_fn: PreliminarySurfaceLevelFn = {
                let cache = Mutex::new(HashMap::<i64, i32>::new());
                let preliminary = preliminary_surface_level.clone();
                Arc::new(move |sample_x: i32, sample_z: i32| {
                    let quantized_x = QuartPos::to_block(QuartPos::from_block(sample_x));
                    let quantized_z = QuartPos::to_block(QuartPos::from_block(sample_z));
                    let key = ColumnPos::as_long(quantized_x, quantized_z);
                    let mut cache = cache.lock().unwrap();
                    let value = cache.entry(key).or_insert_with(|| {
                        let block_x = ColumnPos::get_x(key);
                        let block_z = ColumnPos::get_z(key);
                        mth::floor_d(
                            preliminary.compute(&SinglePointContext::new(block_x, 0, block_z)),
                        )
                    });
                    *value
                })
            };
            aquifer::create(
                &ChunkPos::new(chunk_x, chunk_z),
                &wrapped_router,
                random_state.aquifer_random(),
                noise_settings.min_y(),
                noise_settings.height(),
                global_fluid_picker,
                preliminary_fn,
            )
        };

        let mut builder: Vec<Arc<dyn BlockStateFiller>> = Vec::new();
        // `cacheAllInCell(add(finalDensity, beardifierMarker)).mapAll(this::wrap)`
        // — the same visitor wraps the full-noise expression after the router.
        // The `IdentityKey` wrap cache retains keys strongly (Java's single
        // `HashMap` shared by both `mapAll` calls), so no fresh-cache reset is
        // needed here.
        let full_noise_expr = fns::cache_all_in_cell(fns::add(
            wrapped_router.final_density().clone(),
            Arc::new(BeardifierMarker::instance()),
        ));
        let full_noise_value = map_all(&full_noise_expr, &wrap_visitor);
        let full_noise_density = full_noise_value.clone();
        {
            let full_noise_value = full_noise_value.clone();
            let aquifer = aquifer.clone();
            builder.push(Arc::new(BlockStateRuleFn {
                aquifer,
                full_noise_value,
            }));
        }
        if settings.ore_veins_enabled() {
            builder.push(create_ore_veinifier(
                wrapped_router.vein_toggle().clone(),
                wrapped_router.vein_ridged().clone(),
                wrapped_router.vein_gap().clone(),
                random_state.ore_random(),
            ));
        }
        let block_state_rule: Arc<dyn BlockStateFiller> = Arc::new(MaterialRuleList::new(builder));

        NoiseChunk {
            cell_count_xz,
            cell_count_y,
            cell_noise_min_y,
            first_cell_x,
            first_cell_z,
            first_noise_x,
            first_noise_z,
            noise_size_xz,
            cell_width,
            cell_height,
            wrapped: wrapped.clone(),
            blend_alpha,
            blend_offset,
            preliminary_surface_level_cache: Mutex::new(HashMap::new()),
            aquifer,
            preliminary_surface_level,
            full_noise_density,
            block_state_rule,
            blender,
            beardifier,
            state,
        }
    }

    /// `cachedClimateSampler(NoiseRouter noises, List<ParameterPoint> spawnTarget)`.
    pub fn cached_climate_sampler(
        &self,
        noises: &NoiseRouter,
        spawn_target: &[ParameterPoint],
    ) -> Sampler {
        let wrap = |f: &Arc<dyn DensityFunction>| self.wrap(f);
        Sampler {
            temperature: wrap(noises.temperature()),
            humidity: wrap(noises.vegetation()),
            continentalness: wrap(noises.continents()),
            erosion: wrap(noises.erosion()),
            depth: wrap(noises.depth()),
            weirdness: wrap(noises.ridges()),
            spawn_target: spawn_target.to_vec(),
        }
    }

    /// `getInterpolatedState()` — `blockStateRule.calculate(this)`.
    pub fn get_interpolated_state(&self) -> Option<BlockState> {
        self.block_state_rule.calculate(self)
    }

    /// `getInterpolatedDensity()` — `fullNoiseDensity.compute(this)`.
    pub fn get_interpolated_density(&self) -> f64 {
        self.full_noise_density.compute(self)
    }

    /// `maxPreliminarySurfaceLevel(minBlockX, minBlockZ, maxBlockX, maxBlockZ)`.
    pub fn max_preliminary_surface_level(
        &self,
        min_block_x: i32,
        min_block_z: i32,
        max_block_x: i32,
        max_block_z: i32,
    ) -> i32 {
        let mut max_y = i32::MIN;
        let mut block_z = min_block_z;
        while block_z <= max_block_z {
            let mut block_x = min_block_x;
            while block_x <= max_block_x {
                let surface_level = self.preliminary_surface_level(block_x, block_z);
                if surface_level > max_y {
                    max_y = surface_level;
                }
                block_x += 4;
            }
            block_z += 4;
        }
        max_y
    }

    /// `preliminarySurfaceLevel(int sampleX, int sampleZ)` — the quantized
    /// `ColumnPos`-keyed cache.
    pub fn preliminary_surface_level(&self, sample_x: i32, sample_z: i32) -> i32 {
        let quantized_x = QuartPos::to_block(QuartPos::from_block(sample_x));
        let quantized_z = QuartPos::to_block(QuartPos::from_block(sample_z));
        let key = ColumnPos::as_long(quantized_x, quantized_z);
        let mut cache = self.preliminary_surface_level_cache.lock().unwrap();
        let value = cache.entry(key).or_insert_with(|| {
            let block_x = ColumnPos::get_x(key);
            let block_z = ColumnPos::get_z(key);
            mth::floor_d(
                self.preliminary_surface_level
                    .compute(&SinglePointContext::new(block_x, 0, block_z)),
            )
        });
        *value
    }

    /// `fillSlice(boolean slice0, int cellX)`.
    fn fill_slice(&self, slice0: bool, cell_x: i32) {
        {
            let mut state = self.state.lock().unwrap();
            state.cell_start_block_x = cell_x * self.cell_width;
            state.in_cell_x = 0;
        }
        let provider = SliceFillingContextProvider {
            state: self.state.clone(),
            chunk: self,
            cell_noise_min_y: self.cell_noise_min_y,
            cell_height: self.cell_height,
            cell_count_y: self.cell_count_y,
        };
        let interpolators = self.state.lock().unwrap().interpolators.clone();
        for cell_z_index in 0..(self.cell_count_xz + 1) {
            let cell_z = self.first_cell_z + cell_z_index;
            {
                let mut state = self.state.lock().unwrap();
                state.cell_start_block_z = cell_z * self.cell_width;
                state.in_cell_z = 0;
                state.array_interpolation_counter += 1;
            }
            for interpolator in &interpolators {
                // Java: `(slice0 ? interpolator.slice0 : interpolator.slice1)[cellZIndex]`
                // — a single y-column `double[]` filled IN PLACE via
                // `noiseInterpolator.fillArray(slice, sliceFillingContextProvider)`.
                // The Rust port fills the shared column buffer in place (the
                // `NoiseInterpolator::fill_array` `!filling_cell` branch delegates
                // to `wrapped().fillArray`, exactly Java's `this.wrapped().fillArray`).
                interpolator.fill_slice_column(slice0, cell_z_index as usize, &provider);
            }
        }
        self.state.lock().unwrap().array_interpolation_counter += 1;
    }

    /// `initializeForFirstCellX()`.
    pub fn initialize_for_first_cell_x(&self) {
        {
            let mut state = self.state.lock().unwrap();
            if state.interpolating {
                panic!("Staring interpolation twice");
            }
            state.interpolating = true;
            state.interpolation_counter = 0;
        }
        self.fill_slice(true, self.first_cell_x);
    }

    /// `advanceCellX(int cellXIndex)`.
    pub fn advance_cell_x(&self, cell_x_index: i32) {
        self.fill_slice(false, self.first_cell_x + cell_x_index + 1);
        self.state.lock().unwrap().cell_start_block_x =
            (self.first_cell_x + cell_x_index) * self.cell_width;
    }

    /// `selectCellYZ(int cellYIndex, int cellZIndex)`.
    pub fn select_cell_yz(&self, cell_y_index: i32, cell_z_index: i32) {
        let interpolators = self.state.lock().unwrap().interpolators.clone();
        for i in &interpolators {
            i.select_cell_yz(cell_y_index, cell_z_index);
        }
        {
            let mut state = self.state.lock().unwrap();
            state.filling_cell = true;
            state.cell_start_block_y = (cell_y_index + self.cell_noise_min_y) * self.cell_height;
            state.cell_start_block_z = (self.first_cell_z + cell_z_index) * self.cell_width;
            state.array_interpolation_counter += 1;
        }
        let cell_caches = self.state.lock().unwrap().cell_caches.clone();
        for cell_cache in &cell_caches {
            let mut values = cell_cache.values.lock().unwrap();
            cell_cache.noise_filler.fill_array(&mut values, self);
        }
        {
            let mut state = self.state.lock().unwrap();
            state.array_interpolation_counter += 1;
            state.filling_cell = false;
        }
    }

    /// `updateForY(int posY, double factorY)`.
    pub fn update_for_y(&self, pos_y: i32, factor_y: f64) {
        {
            let mut state = self.state.lock().unwrap();
            state.in_cell_y = pos_y - state.cell_start_block_y;
        }
        let interpolators = self.state.lock().unwrap().interpolators.clone();
        for i in &interpolators {
            i.update_for_y(factor_y);
        }
    }

    /// `updateForX(int posX, double factorX)`.
    pub fn update_for_x(&self, pos_x: i32, factor_x: f64) {
        {
            let mut state = self.state.lock().unwrap();
            state.in_cell_x = pos_x - state.cell_start_block_x;
        }
        let interpolators = self.state.lock().unwrap().interpolators.clone();
        for i in &interpolators {
            i.update_for_x(factor_x);
        }
    }

    /// `updateForZ(int posZ, double factorZ)`.
    pub fn update_for_z(&self, pos_z: i32, factor_z: f64) {
        {
            let mut state = self.state.lock().unwrap();
            state.in_cell_z = pos_z - state.cell_start_block_z;
            state.interpolation_counter += 1;
        }
        let interpolators = self.state.lock().unwrap().interpolators.clone();
        for i in &interpolators {
            i.update_for_z(factor_z);
        }
    }

    /// `stopInterpolation()`.
    pub fn stop_interpolation(&self) {
        let mut state = self.state.lock().unwrap();
        if !state.interpolating {
            panic!("Staring interpolation twice");
        }
        state.interpolating = false;
    }

    /// `swapSlices()`.
    pub fn swap_slices(&self) {
        let interpolators = self.state.lock().unwrap().interpolators.clone();
        for i in &interpolators {
            i.swap_slices();
        }
    }

    /// `aquifer()`.
    pub fn aquifer(&self) -> Arc<dyn Aquifer> {
        self.aquifer.clone()
    }

    /// `cellWidth()`.
    pub fn cell_width(&self) -> i32 {
        self.cell_width
    }

    /// `cellHeight()`.
    pub fn cell_height(&self) -> i32 {
        self.cell_height
    }

    /// `getOrComputeBlendingOutput(int blockX, int blockZ)`.
    pub fn get_or_compute_blending_output(&self, block_x: i32, block_z: i32) -> BlendingOutput {
        self.state
            .lock()
            .unwrap()
            .get_or_compute_blending_output(&self.blender, block_x, block_z)
    }

    /// `wrap(DensityFunction)` — the `computeIfAbsent` wrap cache. Java's
    /// `HashMap` keys on object identity and retains the key; Rust uses the
    /// [`IdentityKey`] (address-hash + strong key retention).
    pub fn wrap(&self, function: &Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
        let key = IdentityKey::new(function.clone());
        {
            let wrapped = self.wrapped.lock().unwrap();
            if let Some(value) = wrapped.get(&key) {
                return value.clone();
            }
        }
        let value = self.wrap_new(function.as_ref());
        self.wrapped.lock().unwrap().insert(key, value.clone());
        value
    }

    /// `wrapNew(DensityFunction)` — the marker/holder/blend dispatch.
    fn wrap_new(&self, function: &dyn DensityFunction) -> Arc<dyn DensityFunction> {
        if let Some(marker) = function.as_any().downcast_ref::<Marker>() {
            let wrapped = marker.wrapped();
            let mut state = self.state.lock().unwrap();
            return match marker.marker_type() {
                MarkerType::Interpolated => {
                    let interp = Arc::new(NoiseInterpolator::new(
                        wrapped.clone(),
                        self.state.clone(),
                        self.cell_count_y,
                        self.cell_count_xz,
                        self.cell_width,
                        self.cell_height,
                        self.first_cell_z,
                    ));
                    state.interpolators.push(interp.clone());
                    // Return the *registered* instance: Java's `wrap` hands back
                    // the same inner-class object the constructor registered
                    // into `interpolators`/`cellCaches`, and the interpolation
                    // loop writes its `values` through those lists. A
                    // `clone_arc` copy would read the never-updated cloned
                    // `values`.
                    interp.clone()
                }
                MarkerType::FlatCache => Arc::new(FlatCache::new(
                    wrapped.clone(),
                    true,
                    self.noise_size_xz,
                    self.first_noise_x,
                    self.first_noise_z,
                )),
                MarkerType::Cache2D => Arc::new(Cache2D::new(wrapped.clone())),
                MarkerType::CacheOnce => {
                    Arc::new(CacheOnce::new(wrapped.clone(), self.state.clone()))
                }
                MarkerType::CacheAllInCell => {
                    let cache = Arc::new(CacheAllInCell::new(
                        wrapped.clone(),
                        self.state.clone(),
                        self.cell_width,
                        self.cell_height,
                    ));
                    state.cell_caches.push(cache.clone());
                    // Same identity requirement as `Interpolated`: the cell
                    // cache is filled through `selectCellYZ`'s `cellCaches`
                    // list, so the tree must read the registered instance.
                    cache.clone()
                }
                MarkerType::BlendDensity => {
                    if !self.blender.is_empty() {
                        Arc::new(BlendDensity::new(wrapped.clone(), self.blender.clone()))
                    } else {
                        wrapped.clone()
                    }
                }
            };
        }
        if let Some(holder) = function.as_any().downcast_ref::<HolderHolder>() {
            match holder.function() {
                Holder::Direct(value) => value.clone(),
                Holder::Reference { .. } => {
                    // The reference holders are resolved by the router wiring
                    // (`RandomState`'s flattener) before this point; a
                    // Reference here is Java's unbound-value panic.
                    panic!("Trying to access unbound value '{}'", render_unbound())
                }
            }
        } else if function
            .as_any()
            .downcast_ref::<fns::BlendAlpha>()
            .is_some()
        {
            match &self.blend_alpha {
                // Java: `function == BlendAlpha.INSTANCE && this.blendAlpha !=
                // null ? this.blendAlpha : function` — the *same* flat-cache
                // instance (a `clone_arc` copy would carry an empty `values`).
                Some(cache) => cache.clone(),
                None => function.clone_arc(),
            }
        } else if function
            .as_any()
            .downcast_ref::<fns::BlendOffset>()
            .is_some()
        {
            match &self.blend_offset {
                // Same identity requirement as `blend_alpha`.
                Some(cache) => cache.clone(),
                None => function.clone_arc(),
            }
        } else if function
            .as_any()
            .downcast_ref::<BeardifierMarker>()
            .is_some()
        {
            self.beardifier.clone()
        } else {
            function.clone_arc()
        }
    }

    /// `forIndex(int cellIndex)` — the `ContextProvider` entry that sets the
    /// in-cell coordinates from a flat cell index.
    pub fn for_index(&self, cell_index: usize) -> &Self {
        let cell_index = cell_index as i32;
        let z_in_cell = mth::positive_modulo(cell_index, self.cell_width);
        let xy_index = mth::floor_div(cell_index, self.cell_width);
        let x_in_cell = mth::positive_modulo(xy_index, self.cell_width);
        let y_in_cell = self.cell_height - 1 - mth::floor_div(xy_index, self.cell_width);
        let mut state = self.state.lock().unwrap();
        state.in_cell_x = x_in_cell;
        state.in_cell_y = y_in_cell;
        state.in_cell_z = z_in_cell;
        state.array_index = cell_index as usize;
        self
    }
}

/// The `wrap` visitor — the constructor's `router.mapAll(this::wrap)` seam.
/// Captures the construction-time context so the inner functions can be
/// created and registered (Java's inner-class constructors register into
/// `NoiseChunk.this.interpolators`/`cellCaches` via the shared state).
struct NoiseChunkWrap {
    state: Arc<Mutex<InterpolationState>>,
    blender: Blender,
    beardifier: Arc<dyn DensityFunction>,
    blend_alpha: Option<Arc<FlatCache>>,
    blend_offset: Option<Arc<FlatCache>>,
    cell_width: i32,
    cell_height: i32,
    cell_count_y: i32,
    cell_count_xz: i32,
    first_cell_z: i32,
    first_noise_x: i32,
    first_noise_z: i32,
    noise_size_xz: i32,
    /// `wrapped` — the `HashMap<DensityFunction, DensityFunction>` wrap cache
    /// (Java's `this.wrapped` `computeIfAbsent` used by `this::wrap`). Shared
    /// with the `NoiseChunk` so `cachedClimateSampler` reuses the
    /// construction-time wraps (Java's single `this.wrapped` map).
    wrapped: Arc<Mutex<HashMap<IdentityKey, Arc<dyn DensityFunction>>>>,
}

#[allow(clippy::too_many_arguments)]
impl NoiseChunkWrap {
    fn with_blend(
        self,
        blend_alpha: Option<Arc<FlatCache>>,
        blend_offset: Option<Arc<FlatCache>>,
    ) -> Self {
        NoiseChunkWrap {
            blend_alpha,
            blend_offset,
            ..self
        }
    }

    fn wrap_new(&self, function: &dyn DensityFunction) -> Arc<dyn DensityFunction> {
        if let Some(marker) = function.as_any().downcast_ref::<Marker>() {
            let wrapped = marker.wrapped();
            let mut state = self.state.lock().unwrap();
            return match marker.marker_type() {
                MarkerType::Interpolated => {
                    let interp = Arc::new(NoiseInterpolator::new(
                        wrapped.clone(),
                        self.state.clone(),
                        self.cell_count_y,
                        self.cell_count_xz,
                        self.cell_width,
                        self.cell_height,
                        self.first_cell_z,
                    ));
                    state.interpolators.push(interp.clone());
                    // Return the *registered* instance: Java's `wrap` hands back
                    // the same inner-class object the constructor registered
                    // into `interpolators`/`cellCaches`, and the interpolation
                    // loop writes its `values` through those lists. A
                    // `clone_arc` copy would read the never-updated cloned
                    // `values`.
                    interp.clone()
                }
                MarkerType::FlatCache => Arc::new(FlatCache::new(
                    wrapped.clone(),
                    true,
                    self.noise_size_xz,
                    self.first_noise_x,
                    self.first_noise_z,
                )),
                MarkerType::Cache2D => Arc::new(Cache2D::new(wrapped.clone())),
                MarkerType::CacheOnce => {
                    Arc::new(CacheOnce::new(wrapped.clone(), self.state.clone()))
                }
                MarkerType::CacheAllInCell => {
                    let cache = Arc::new(CacheAllInCell::new(
                        wrapped.clone(),
                        self.state.clone(),
                        self.cell_width,
                        self.cell_height,
                    ));
                    state.cell_caches.push(cache.clone());
                    // Same identity requirement as `Interpolated`: the cell
                    // cache is filled through `selectCellYZ`'s `cellCaches`
                    // list, so the tree must read the registered instance.
                    cache.clone()
                }
                MarkerType::BlendDensity => {
                    if !self.blender.is_empty() {
                        Arc::new(BlendDensity::new(wrapped.clone(), self.blender.clone()))
                    } else {
                        wrapped.clone()
                    }
                }
            };
        }
        if let Some(holder) = function.as_any().downcast_ref::<HolderHolder>() {
            match holder.function() {
                Holder::Direct(value) => value.clone(),
                Holder::Reference { .. } => {
                    panic!("Trying to access unbound value '{}'", render_unbound())
                }
            }
        } else if function
            .as_any()
            .downcast_ref::<BeardifierMarker>()
            .is_some()
        {
            self.beardifier.clone()
        } else if function
            .as_any()
            .downcast_ref::<fns::BlendAlpha>()
            .is_some()
        {
            match &self.blend_alpha {
                // Java: `function == BlendAlpha.INSTANCE && this.blendAlpha !=
                // null ? this.blendAlpha : function` — the *same* flat-cache
                // instance (a `clone_arc` copy would carry an empty `values`).
                Some(cache) => cache.clone(),
                None => function.clone_arc(),
            }
        } else if function
            .as_any()
            .downcast_ref::<fns::BlendOffset>()
            .is_some()
        {
            match &self.blend_offset {
                // Same identity requirement as `blend_alpha`.
                Some(cache) => cache.clone(),
                None => function.clone_arc(),
            }
        } else {
            function.clone_arc()
        }
    }
}

impl Visitor for NoiseChunkWrap {
    fn apply(&self, input: &Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
        let key = IdentityKey::new(input.clone());
        {
            let wrapped = self.wrapped.lock().unwrap();
            if let Some(value) = wrapped.get(&key) {
                return value.clone();
            }
        }
        let value = self.wrap_new(input.as_ref());
        self.wrapped.lock().unwrap().insert(key, value.clone());
        value
    }
}

/// `render_unbound()` — the `Holder.Reference.value()` panic render (the
/// holder model's unbound state; the full `render_holder` is in
/// `rivet-registry`).
fn render_unbound() -> String {
    "unbound holder reference".to_string()
}

impl FunctionContext for NoiseChunk {
    fn block_x(&self) -> i32 {
        let state = self.state.lock().unwrap();
        state.cell_start_block_x + state.in_cell_x
    }
    fn block_y(&self) -> i32 {
        let state = self.state.lock().unwrap();
        state.cell_start_block_y + state.in_cell_y
    }
    fn block_z(&self) -> i32 {
        let state = self.state.lock().unwrap();
        state.cell_start_block_z + state.in_cell_z
    }
}

impl ContextProvider for NoiseChunk {
    fn for_index(&self, index: usize) -> &dyn FunctionContext {
        // Java's `forIndex` mutates the in-cell coordinates and returns `this`
        // — the owning chunk — so every inner function reached through the
        // per-index fill paths evaluates with `context == NoiseChunk.this` and
        // takes the interpolation/cache branch (`is_owning_chunk` matches).
        // The inherent `NoiseChunk::for_index` sets the shared in-cell
        // coordinates exactly like Java (`floorMod`/`floorDiv` against the
        // cell geometry) and returns `&self`, which coerces to the trait
        // object. The fully-qualified call is deliberate: the inherent method
        // has the same name, so a bare `self.for_index(index)` would already
        // resolve to it, but spelling the dispatch out removes any ambiguity
        // with this trait method.
        NoiseChunk::for_index(self, index)
    }

    fn fill_all_directly(&self, output: &mut [f64], function: &dyn DensityFunction) {
        // Each in-cell coordinate is set under a short lock and the guard is
        // dropped before `function.compute`: every inner density function
        // re-locks the shared `InterpolationState` (the `CacheAllInCell`/
        // `CacheOnce`/`NoiseInterpolator` read paths and the `FunctionContext`
        // accessors), so holding the guard across the compute would deadlock on
        // the non-reentrant `Mutex`. Java's `context != NoiseChunk.this`
        // identity check needs no such lock (single-threaded sync-tick model;
        // the short locks are uncontended).
        self.state.lock().unwrap().array_index = 0;
        for y_in_cell in (0..self.cell_height).rev() {
            self.state.lock().unwrap().in_cell_y = y_in_cell;
            for x_in_cell in 0..self.cell_width {
                self.state.lock().unwrap().in_cell_x = x_in_cell;
                for z_in_cell in 0..self.cell_width {
                    self.state.lock().unwrap().in_cell_z = z_in_cell;
                    let index = self.state.lock().unwrap().array_index;
                    output[index] = function.compute(self);
                    self.state.lock().unwrap().array_index = index + 1;
                }
            }
        }
    }
}

/// The shared `sliceFillingContextProvider` — sets the chunk's cell-start
/// block-y and fills via `fillAllDirectly` on the owning chunk (Java's
/// anonymous `ContextProvider` returns `NoiseChunk.this` from `forIndex` and
/// computes with it in `fillAllDirectly`).
#[derive(Debug)]
struct SliceFillingContextProvider<'a> {
    state: Arc<Mutex<InterpolationState>>,
    /// The owning chunk — `NoiseChunk.this`, so inner functions reached
    /// through the per-index fill paths take the interpolation branch.
    chunk: &'a NoiseChunk,
    cell_noise_min_y: i32,
    cell_height: i32,
    cell_count_y: i32,
}

impl ContextProvider for SliceFillingContextProvider<'_> {
    fn for_index(&self, index: usize) -> &dyn FunctionContext {
        let mut state = self.state.lock().unwrap();
        state.cell_start_block_y = (index as i32 + self.cell_noise_min_y) * self.cell_height;
        state.interpolation_counter += 1;
        state.in_cell_y = 0;
        state.array_index = index;
        // Java's `forIndex` returns `NoiseChunk.this` — the owning chunk, so
        // every inner function reached through the per-index fill takes the
        // interpolation/cache branch. The lock guard is dropped at the end of
        // this method (before the caller's `compute`), so the returned
        // `&NoiseChunk` borrow is deadlock-free.
        self.chunk
    }

    fn fill_all_directly(&self, output: &mut [f64], function: &dyn DensityFunction) {
        for cell_y_index in 0..(self.cell_count_y + 1) {
            // The cell-start-y/counter/array-index are set under a short lock
            // and the guard is dropped before `compute` (same deadlock
            // consideration as `NoiseChunk::fill_all_directly`).
            {
                let mut state = self.state.lock().unwrap();
                state.cell_start_block_y =
                    (cell_y_index + self.cell_noise_min_y) * self.cell_height;
                state.interpolation_counter += 1;
                state.in_cell_y = 0;
                state.array_index = cell_y_index as usize;
            }
            output[cell_y_index as usize] = function.compute(self.chunk);
        }
    }
}

/// The `blockStateRule` closure — `context -> this.aquifer.computeSubstance(context,
/// fullNoiseValue.compute(context))`.
struct BlockStateRuleFn {
    aquifer: Arc<dyn Aquifer>,
    full_noise_value: Arc<dyn DensityFunction>,
}

impl BlockStateFiller for BlockStateRuleFn {
    fn calculate(&self, context: &dyn FunctionContext) -> Option<BlockState> {
        self.aquifer
            .compute_substance(context, self.full_noise_value.compute(context))
    }
}

/// `NoiseChunk.BlendAlpha` — the blend-alpha inner function.
#[derive(Debug)]
pub struct BlendAlpha {
    state: Arc<Mutex<InterpolationState>>,
    blender: Blender,
}

impl BlendAlpha {
    fn new(state: Arc<Mutex<InterpolationState>>, blender: Blender) -> Self {
        BlendAlpha { state, blender }
    }
}

impl DensityFunction for BlendAlpha {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        self.state
            .lock()
            .unwrap()
            .get_or_compute_blending_output(&self.blender, context.block_x(), context.block_z())
            .alpha()
    }
    fn min_value(&self) -> f64 {
        0.0
    }
    fn max_value(&self) -> f64 {
        1.0
    }
    fn type_id(&self) -> crate::levelgen::noise::density_function_type::DensityFunctionTypeId {
        crate::levelgen::noise::density_function_type::DensityFunctionTypes::BLEND_ALPHA
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(BlendAlpha::new(self.state.clone(), self.blender.clone()))
    }
}

/// `NoiseChunk.BlendOffset` — the blend-offset inner function.
#[derive(Debug)]
pub struct BlendOffset {
    state: Arc<Mutex<InterpolationState>>,
    blender: Blender,
}

impl BlendOffset {
    fn new(state: Arc<Mutex<InterpolationState>>, blender: Blender) -> Self {
        BlendOffset { state, blender }
    }
}

impl DensityFunction for BlendOffset {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        self.state
            .lock()
            .unwrap()
            .get_or_compute_blending_output(&self.blender, context.block_x(), context.block_z())
            .blending_offset()
    }
    fn min_value(&self) -> f64 {
        f64::NEG_INFINITY
    }
    fn max_value(&self) -> f64 {
        f64::INFINITY
    }
    fn type_id(&self) -> crate::levelgen::noise::density_function_type::DensityFunctionTypeId {
        crate::levelgen::noise::density_function_type::DensityFunctionTypes::BLEND_OFFSET
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(BlendOffset::new(self.state.clone(), self.blender.clone()))
    }
}

/// `NoiseChunk.BlendDensity` — `blender.blendDensity(context, input.compute(context))`.
#[derive(Debug)]
pub struct BlendDensity {
    input: Arc<dyn DensityFunction>,
    blender: Blender,
}

impl BlendDensity {
    fn new(input: Arc<dyn DensityFunction>, blender: Blender) -> Self {
        BlendDensity { input, blender }
    }
}

impl DensityFunction for BlendDensity {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        self.blender
            .blend_density(context, self.input.compute(context))
    }
    fn fill_array(&self, output: &mut [f64], context_provider: &dyn ContextProvider) {
        self.input.fill_array(output, context_provider);
        for (i, slot) in output.iter_mut().enumerate() {
            let context = context_provider.for_index(i);
            *slot = self.blender.blend_density(context, *slot);
        }
    }
    fn min_value(&self) -> f64 {
        f64::NEG_INFINITY
    }
    fn max_value(&self) -> f64 {
        f64::INFINITY
    }
    fn type_id(&self) -> crate::levelgen::noise::density_function_type::DensityFunctionTypeId {
        crate::levelgen::noise::density_function_type::DensityFunctionTypes::BLEND_DENSITY
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(BlendDensity::new(self.input.clone(), self.blender.clone()))
    }
}

/// `NoiseChunk.Cache2D` — the 2D position cache (static inner class).
#[derive(Debug)]
pub struct Cache2D {
    function: Arc<dyn DensityFunction>,
    cache: Mutex<(i64, f64)>,
}

impl Cache2D {
    fn new(function: Arc<dyn DensityFunction>) -> Self {
        Cache2D {
            function,
            cache: Mutex::new((ChunkPos::INVALID_CHUNK_POS, 0.0)),
        }
    }
}

impl DensityFunction for Cache2D {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        let block_x = context.block_x();
        let block_z = context.block_z();
        let pos_2d = ChunkPos::pack_coords(block_x, block_z);
        let mut cache = self.cache.lock().unwrap();
        if cache.0 == pos_2d {
            return cache.1;
        }
        cache.0 = pos_2d;
        let value = self.function.compute(context);
        cache.1 = value;
        value
    }
    fn fill_array(&self, output: &mut [f64], context_provider: &dyn ContextProvider) {
        self.function.fill_array(output, context_provider);
    }
    fn min_value(&self) -> f64 {
        self.function.min_value()
    }
    fn max_value(&self) -> f64 {
        self.function.max_value()
    }
    fn type_id(&self) -> crate::levelgen::noise::density_function_type::DensityFunctionTypeId {
        crate::levelgen::noise::density_function_type::DensityFunctionTypes::CACHE_2D
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(Cache2D::new(self.function.clone()))
    }
}

/// `NoiseChunk.CacheAllInCell` — the per-cell cache.
#[derive(Debug)]
pub struct CacheAllInCell {
    noise_filler: Arc<dyn DensityFunction>,
    values: Arc<Mutex<Vec<f64>>>,
    state: Arc<Mutex<InterpolationState>>,
    cell_width: i32,
    cell_height: i32,
}

/// Java's `context != NoiseChunk.this` reference-identity check: downcasts the
/// `FunctionContext` to `NoiseChunk` and compares the shared
/// `InterpolationState` `Arc` identity. Any other context — a standalone
/// `SinglePointContext`, or a *different* `NoiseChunk` — never matches, exactly
/// like Java's outer-class identity comparison. The owning chunk's own
/// `forIndex`/`fillAllDirectly` seam returns `&self`, so the in-loop fills hit
/// this test with a matching state. Lock-free (only pointer compares).
fn is_owning_chunk(state: &Arc<Mutex<InterpolationState>>, context: &dyn FunctionContext) -> bool {
    match (context as &dyn Any).downcast_ref::<NoiseChunk>() {
        Some(chunk) => Arc::ptr_eq(&chunk.state, state),
        None => false,
    }
}

impl CacheAllInCell {
    fn new(
        noise_filler: Arc<dyn DensityFunction>,
        state: Arc<Mutex<InterpolationState>>,
        cell_width: i32,
        cell_height: i32,
    ) -> Self {
        let values = Arc::new(Mutex::new(vec![
            0.0;
            (cell_width * cell_width * cell_height)
                as usize
        ]));
        CacheAllInCell {
            noise_filler,
            values,
            state,
            cell_width,
            cell_height,
        }
    }
}

impl DensityFunction for CacheAllInCell {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        if !is_owning_chunk(&self.state, context) {
            return self.noise_filler.compute(context);
        }
        let state = self.state.lock().unwrap();
        if !state.interpolating {
            // Java: `throw new IllegalStateException("Trying to sample
            // interpolator outside the interpolation loop")`.
            panic!("Trying to sample interpolator outside the interpolation loop");
        }
        let x = state.in_cell_x;
        let y = state.in_cell_y;
        let z = state.in_cell_z;
        if x >= 0
            && y >= 0
            && z >= 0
            && x < self.cell_width
            && y < self.cell_height
            && z < self.cell_width
        {
            let values = self.values.lock().unwrap();
            values[(((self.cell_height - 1 - y) * self.cell_width + x) * self.cell_width + z)
                as usize]
        } else {
            drop(state);
            self.noise_filler.compute(context)
        }
    }
    fn fill_array(&self, output: &mut [f64], context_provider: &dyn ContextProvider) {
        context_provider.fill_all_directly(output, self);
    }
    fn min_value(&self) -> f64 {
        self.noise_filler.min_value()
    }
    fn max_value(&self) -> f64 {
        self.noise_filler.max_value()
    }
    fn type_id(&self) -> crate::levelgen::noise::density_function_type::DensityFunctionTypeId {
        crate::levelgen::noise::density_function_type::DensityFunctionTypes::CACHE_ALL_IN_CELL
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(CacheAllInCell::new(
            self.noise_filler.clone(),
            self.state.clone(),
            self.cell_width,
            self.cell_height,
        ))
    }
}

/// `NoiseChunk.CacheOnce` — the per-counter cache.
#[derive(Debug)]
pub struct CacheOnce {
    function: Arc<dyn DensityFunction>,
    state: Arc<Mutex<InterpolationState>>,
    cache: Mutex<OnceCacheValues>,
}

/// The `CacheOnce` mutable cache values.
#[derive(Debug)]
struct OnceCacheValues {
    last_counter: i64,
    last_array_counter: i64,
    last_value: f64,
    last_array: Option<Vec<f64>>,
}

impl CacheOnce {
    fn new(function: Arc<dyn DensityFunction>, state: Arc<Mutex<InterpolationState>>) -> Self {
        CacheOnce {
            function,
            state,
            cache: Mutex::new(OnceCacheValues {
                last_counter: -1,
                last_array_counter: -1,
                last_value: 0.0,
                last_array: None,
            }),
        }
    }
}

impl DensityFunction for CacheOnce {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        if !is_owning_chunk(&self.state, context) {
            return self.function.compute(context);
        }
        let state = self.state.lock().unwrap();
        let mut cache = self.cache.lock().unwrap();
        if let Some(last_array) = &cache.last_array
            && cache.last_array_counter == state.array_interpolation_counter
        {
            return last_array[state.array_index];
        }
        if cache.last_counter == state.interpolation_counter {
            return cache.last_value;
        }
        cache.last_counter = state.interpolation_counter;
        drop(state);
        let value = self.function.compute(context);
        cache.last_value = value;
        value
    }
    fn fill_array(&self, output: &mut [f64], context_provider: &dyn ContextProvider) {
        let state = self.state.lock().unwrap();
        let mut cache = self.cache.lock().unwrap();
        if let Some(last_array) = &cache.last_array
            && cache.last_array_counter == state.array_interpolation_counter
        {
            output.copy_from_slice(&last_array[..output.len()]);
        } else {
            drop(state);
            self.function.fill_array(output, context_provider);
            let state = self.state.lock().unwrap();
            if let Some(last_array) = &mut cache.last_array
                && last_array.len() == output.len()
            {
                last_array.copy_from_slice(output);
            } else {
                cache.last_array = Some(output.to_vec());
            }
            cache.last_array_counter = state.array_interpolation_counter;
        }
    }
    fn min_value(&self) -> f64 {
        self.function.min_value()
    }
    fn max_value(&self) -> f64 {
        self.function.max_value()
    }
    fn type_id(&self) -> crate::levelgen::noise::density_function_type::DensityFunctionTypeId {
        crate::levelgen::noise::density_function_type::DensityFunctionTypes::CACHE_ONCE
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(CacheOnce::new(self.function.clone(), self.state.clone()))
    }
}

/// `NoiseChunk.FlatCache` — the flat XZ cache.
#[derive(Debug)]
pub struct FlatCache {
    noise_filler: Arc<dyn DensityFunction>,
    values: Arc<Mutex<Vec<f64>>>,
    size_xz: i32,
    first_noise_x: i32,
    first_noise_z: i32,
}

impl FlatCache {
    fn new(
        noise_filler: Arc<dyn DensityFunction>,
        fill: bool,
        noise_size_xz: i32,
        first_noise_x: i32,
        first_noise_z: i32,
    ) -> Self {
        let size_xz = noise_size_xz + 1;
        let mut data = vec![0.0; (size_xz * size_xz) as usize];
        if fill {
            for x in 0..=noise_size_xz {
                let quart_x = first_noise_x + x;
                let block_x = QuartPos::to_block(quart_x);
                for z in 0..=noise_size_xz {
                    let quart_z = first_noise_z + z;
                    let block_z = QuartPos::to_block(quart_z);
                    data[(x + z * size_xz) as usize] =
                        noise_filler.compute(&SinglePointContext::new(block_x, 0, block_z));
                }
            }
        }
        FlatCache {
            noise_filler,
            values: Arc::new(Mutex::new(data)),
            size_xz,
            first_noise_x,
            first_noise_z,
        }
    }
}

impl DensityFunction for FlatCache {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        let quart_x = QuartPos::from_block(context.block_x());
        let quart_z = QuartPos::from_block(context.block_z());
        let x = quart_x - self.first_noise_x;
        let z = quart_z - self.first_noise_z;
        if x >= 0 && z >= 0 && x < self.size_xz && z < self.size_xz {
            self.values.lock().unwrap()[(x + z * self.size_xz) as usize]
        } else {
            self.noise_filler.compute(context)
        }
    }
    fn fill_array(&self, output: &mut [f64], context_provider: &dyn ContextProvider) {
        context_provider.fill_all_directly(output, self);
    }
    fn min_value(&self) -> f64 {
        self.noise_filler.min_value()
    }
    fn max_value(&self) -> f64 {
        self.noise_filler.max_value()
    }
    fn type_id(&self) -> crate::levelgen::noise::density_function_type::DensityFunctionTypeId {
        crate::levelgen::noise::density_function_type::DensityFunctionTypes::FLAT_CACHE
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(FlatCache::new(
            self.noise_filler.clone(),
            false,
            self.size_xz - 1,
            self.first_noise_x,
            self.first_noise_z,
        ))
    }
}

/// The `NoiseInterpolator` mutable per-instance values.
#[derive(Debug)]
struct InterpolatorValues {
    noise000: f64,
    noise001: f64,
    noise100: f64,
    noise101: f64,
    noise010: f64,
    noise011: f64,
    noise110: f64,
    noise111: f64,
    value_xz00: f64,
    value_xz10: f64,
    value_xz01: f64,
    value_xz11: f64,
    value_z0: f64,
    value_z1: f64,
    value: f64,
}

impl InterpolatorValues {
    fn new() -> Self {
        InterpolatorValues {
            noise000: 0.0,
            noise001: 0.0,
            noise100: 0.0,
            noise101: 0.0,
            noise010: 0.0,
            noise011: 0.0,
            noise110: 0.0,
            noise111: 0.0,
            value_xz00: 0.0,
            value_xz10: 0.0,
            value_xz01: 0.0,
            value_xz11: 0.0,
            value_z0: 0.0,
            value_z1: 0.0,
            value: 0.0,
        }
    }
}

/// `NoiseChunk.NoiseInterpolator` — the per-function 3D interpolation.
#[derive(Debug)]
pub struct NoiseInterpolator {
    slice0: Arc<Mutex<Vec<Vec<f64>>>>,
    slice1: Arc<Mutex<Vec<Vec<f64>>>>,
    noise_filler: Arc<dyn DensityFunction>,
    state: Arc<Mutex<InterpolationState>>,
    cell_width: i32,
    cell_height: i32,
    first_cell_z: i32,
    /// The corner/lerp values the interpolation loop writes and the tree
    /// reads. Shared (`Arc`) so a `clone_arc` (the `SimpleFunction::mapChildren`
    /// identity and the wrap-visitor default) aliases the SAME instance — Java
    /// returns the identical inner-class object; a per-clone `values` would let
    /// the tree read the never-updated copy.
    values: Arc<Mutex<InterpolatorValues>>,
}

impl NoiseInterpolator {
    /// The private `NoiseInterpolator(DensityFunction noiseFiller)` — allocates
    /// the two slices (Java's constructor registers into
    /// `NoiseChunk.this.interpolators`; the `wrap` visitor does that).
    #[allow(clippy::too_many_arguments)]
    fn new(
        noise_filler: Arc<dyn DensityFunction>,
        state: Arc<Mutex<InterpolationState>>,
        cell_count_y: i32,
        cell_count_xz: i32,
        cell_width: i32,
        cell_height: i32,
        first_cell_z: i32,
    ) -> Self {
        NoiseInterpolator {
            slice0: Arc::new(Mutex::new(allocate_slice(cell_count_y, cell_count_xz))),
            slice1: Arc::new(Mutex::new(allocate_slice(cell_count_y, cell_count_xz))),
            noise_filler,
            state,
            cell_width,
            cell_height,
            first_cell_z,
            values: Arc::new(Mutex::new(InterpolatorValues::new())),
        }
    }

    /// `fillArray(double[] slice, ContextProvider provider)` — via the slice
    /// provider's `fillAllDirectly` (the interpolation loop). `slice0`/`slice1`
    /// are `Arc<Mutex<Vec<Vec<f64>>>>`, so the `slice0`/`slice1` clone below is
    /// an Arc clone aliasing the SAME shared buffer: the column is filled IN
    /// PLACE through the shared `Arc<Mutex>`, exactly Java's `fillSlice` handing
    /// the interpolator a live column from its own arrays (the
    /// `interpolation_loop_reads_filled_slices` test reads the filled slices,
    /// and `selectCellYZ` reads this same buffer).
    fn fill_slice_column(&self, slice0: bool, cell_z_index: usize, provider: &dyn ContextProvider) {
        // Clone the Arc so the `&mut column` borrow no longer ties to `self`
        // (the `fill_array` call below takes `&self`).
        let buffer = if slice0 {
            self.slice0.clone()
        } else {
            self.slice1.clone()
        };
        let column = &mut buffer.lock().unwrap()[cell_z_index];
        // `NoiseInterpolator::fill_array` delegates to `wrapped().fillArray`
        // when `!filling_cell` (Java's `this.wrapped().fillArray`), so the
        // slice is filled with the wrapped leaf density, not the interpolated
        // value.
        self.fill_array(column, provider);
    }

    /// `selectCellYZ(int cellYIndex, int cellZIndex)` — reads the two slices.
    fn select_cell_yz(&self, cell_y_index: i32, cell_z_index: i32) {
        let cell_z_index = cell_z_index as usize;
        let cell_y_index = cell_y_index as usize;
        let slice0 = self.slice0.lock().unwrap();
        let slice1 = self.slice1.lock().unwrap();
        let mut values = self.values.lock().unwrap();
        values.noise000 = slice0[cell_z_index][cell_y_index];
        values.noise001 = slice0[cell_z_index + 1][cell_y_index];
        values.noise100 = slice1[cell_z_index][cell_y_index];
        values.noise101 = slice1[cell_z_index + 1][cell_y_index];
        values.noise010 = slice0[cell_z_index][cell_y_index + 1];
        values.noise011 = slice0[cell_z_index + 1][cell_y_index + 1];
        values.noise110 = slice1[cell_z_index][cell_y_index + 1];
        values.noise111 = slice1[cell_z_index + 1][cell_y_index + 1];
    }

    /// `updateForY(double factorY)`.
    fn update_for_y(&self, factor_y: f64) {
        let mut values = self.values.lock().unwrap();
        values.value_xz00 = mth::lerp(factor_y, values.noise000, values.noise010);
        values.value_xz10 = mth::lerp(factor_y, values.noise100, values.noise110);
        values.value_xz01 = mth::lerp(factor_y, values.noise001, values.noise011);
        values.value_xz11 = mth::lerp(factor_y, values.noise101, values.noise111);
    }

    /// `updateForX(double factorX)`.
    fn update_for_x(&self, factor_x: f64) {
        let mut values = self.values.lock().unwrap();
        values.value_z0 = mth::lerp(factor_x, values.value_xz00, values.value_xz10);
        values.value_z1 = mth::lerp(factor_x, values.value_xz01, values.value_xz11);
    }

    /// `updateForZ(double factorZ)`.
    fn update_for_z(&self, factor_z: f64) {
        let mut values = self.values.lock().unwrap();
        values.value = mth::lerp(factor_z, values.value_z0, values.value_z1);
    }

    /// `swapSlices()`.
    fn swap_slices(&self) {
        let mut slice0 = self.slice0.lock().unwrap();
        let mut slice1 = self.slice1.lock().unwrap();
        std::mem::swap(&mut *slice0, &mut *slice1);
    }
}

/// `allocateSlice(int cellCountY, int cellCountZ)` — `new double[sizeZ][sizeY]`.
fn allocate_slice(cell_count_y: i32, cell_count_z: i32) -> Vec<Vec<f64>> {
    let size_z = cell_count_z + 1;
    let size_y = cell_count_y + 1;
    vec![vec![0.0; size_y as usize]; size_z as usize]
}

impl DensityFunction for NoiseInterpolator {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        if !is_owning_chunk(&self.state, context) {
            return self.noise_filler.compute(context);
        }
        let state = self.state.lock().unwrap();
        if !state.interpolating {
            // Java: `throw new IllegalStateException("Trying to sample
            // interpolator outside the interpolation loop")`.
            panic!("Trying to sample interpolator outside the interpolation loop");
        }
        let values = self.values.lock().unwrap();
        if state.filling_cell {
            mth::lerp3(
                state.in_cell_x as f64 / self.cell_width as f64,
                state.in_cell_y as f64 / self.cell_height as f64,
                state.in_cell_z as f64 / self.cell_width as f64,
                values.noise000,
                values.noise100,
                values.noise010,
                values.noise110,
                values.noise001,
                values.noise101,
                values.noise011,
                values.noise111,
            )
        } else {
            values.value
        }
    }
    fn fill_array(&self, output: &mut [f64], context_provider: &dyn ContextProvider) {
        let state = self.state.lock().unwrap();
        if state.filling_cell {
            drop(state);
            context_provider.fill_all_directly(output, self);
        } else {
            drop(state);
            self.noise_filler.fill_array(output, context_provider);
        }
    }
    fn min_value(&self) -> f64 {
        self.noise_filler.min_value()
    }
    fn max_value(&self) -> f64 {
        self.noise_filler.max_value()
    }
    fn type_id(&self) -> crate::levelgen::noise::density_function_type::DensityFunctionTypeId {
        crate::levelgen::noise::density_function_type::DensityFunctionTypes::INTERPOLATED
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(NoiseInterpolator {
            slice0: self.slice0.clone(),
            slice1: self.slice1.clone(),
            noise_filler: self.noise_filler.clone(),
            state: self.state.clone(),
            cell_width: self.cell_width,
            cell_height: self.cell_height,
            first_cell_z: self.first_cell_z,
            // Share `values`: the wrap visitor's default (`SimpleFunction`'s
            // `mapChildren` identity) and the second `map_all` both `clone_arc`
            // the already-wrapped interpolator, and Java returns the same
            // object — the clone must read what the interpolation loop writes.
            values: self.values.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::levelgen::blending::blender::Blender;
    use crate::levelgen::noise::beardifier_marker::BeardifierMarker;
    use crate::levelgen::noise::density_functions::{self as fns, MappedType};
    use crate::levelgen::noise::noise_router::NoiseRouter;
    use crate::levelgen::noise::noise_settings::OVERWORLD_NOISE_SETTINGS;
    use crate::levelgen::noisegen::noise_based_chunk_generator::create_fluid_picker;
    use crate::levelgen::noisegen::noise_generator_settings::NoiseGeneratorSettings;
    use crate::levelgen::noisegen::noise_router_data::DensityFunctionValue;
    use crate::levelgen::surface_rules::surface_rule_air;
    use crate::levelgen::synth::normal_noise::NoiseParameters;
    use rivet_registry::RegistryBuilder;
    use rivet_registry::registry::Registry;

    /// A settings record whose router's `finalDensity` is a non-trivial
    /// interpolated function — `interpolated(cacheOnce(constant(3.5)))`. The
    /// aquifer/ore-vein fields are disabled so `NoiseChunk::new` uses the
    /// disabled aquifer; the remaining router fields are zero constants.
    fn test_settings() -> NoiseGeneratorSettings {
        test_settings_with_final_density(fns::interpolated(fns::cache_once(fns::constant(3.5))))
    }

    /// The `test_settings` router with an arbitrary `finalDensity`.
    fn test_settings_with_final_density(final_density: Arc<dyn DensityFunction>) -> NoiseGeneratorSettings {
        let z = fns::zero();
        let router = NoiseRouter::new(
            z.clone(),
            z.clone(),
            z.clone(),
            z.clone(),
            z.clone(),
            z.clone(),
            z.clone(),
            z.clone(),
            z.clone(),
            z.clone(),
            fns::constant(100.0),
            final_density,
            z.clone(),
            z.clone(),
            z,
        );

        NoiseGeneratorSettings::new(
            OVERWORLD_NOISE_SETTINGS,
            Blocks::STONE.default_block_state(),
            Blocks::WATER.default_block_state(),
            router,
            surface_rule_air(),
            Vec::new(),
            63,
            true,
            false,
            false,
            false,
        )
    }

    /// Empty registries — the test router has no registry-resolved nodes
    /// (`HolderHolder`/`NoiseHolder`), so `RandomState::create` never touches
    /// them.
    fn empty_registries() -> (Registry<NoiseParameters>, Registry<DensityFunctionValue>) {
        let noise_key = &crate::levelgen::noise::registry_keys::NOISE;
        let noise_registry: Registry<NoiseParameters> = RegistryBuilder::new(noise_key).freeze();
        let df_key = &crate::levelgen::noise::registry_keys::DENSITY_FUNCTION;
        let df_registry: Registry<DensityFunctionValue> = RegistryBuilder::new(df_key).freeze();
        (noise_registry, df_registry)
    }

    #[test]
    fn interpolation_loop_reads_filled_slices() {
        let settings = test_settings();
        let (noise_registry, df_registry) = empty_registries();
        let state = RandomState::create(&settings, &noise_registry, &df_registry, 1234);

        let chunk = NoiseChunk::new(
            4, // cellCountXZ — overworld cellWidth 4
            &state,
            0, // chunkMinBlockX
            0, // chunkMinBlockZ
            &OVERWORLD_NOISE_SETTINGS,
            Arc::new(BeardifierMarker::instance()) as Arc<dyn DensityFunction>,
            &settings,
            Box::new(create_fluid_picker(&settings)),
            Blender::empty(),
        );

        // The full per-chunk interpolation loop (Java's `iterateNoiseColumn`
        // walk): two slices are filled with the interpolated leaf density
        // (`fill_slice`), then a cell is selected and the in-cell factors
        // advanced before sampling the final density.
        chunk.initialize_for_first_cell_x();
        chunk.advance_cell_x(0);
        chunk.select_cell_yz(40, 0);
        chunk.update_for_y(260, 0.5);
        chunk.update_for_x(2, 0.5);
        chunk.update_for_z(2, 0.5);

        // `fullNoiseDensity = cacheAllInCell(add(finalDensity, beardifier))` —
        // the beardifier marker is 0.0, so the sampled density is the constant
        // 3.5. The slices are filled in place during the interpolation walk, so
        // the sampled cell reads 3.5.
        let density = chunk.get_interpolated_density();
        assert!(
            (density - 3.5).abs() < 1e-9,
            "expected the interpolated constant 3.5, got {density}"
        );
        chunk.stop_interpolation();
    }

    /// Builds a chunk and walks it into the same cell state as
    /// `interpolation_loop_reads_filled_slices` (cell start X = 0, Y = 256,
    /// Z = 0 with the overworld cellWidth 4 / cellHeight 8 geometry).
    fn interpolating_chunk() -> NoiseChunk {
        let settings = test_settings();
        let (noise_registry, df_registry) = empty_registries();
        let state = RandomState::create(&settings, &noise_registry, &df_registry, 1234);
        let chunk = NoiseChunk::new(
            4,
            &state,
            0,
            0,
            &OVERWORLD_NOISE_SETTINGS,
            Arc::new(BeardifierMarker::instance()) as Arc<dyn DensityFunction>,
            &settings,
            Box::new(create_fluid_picker(&settings)),
            Blender::empty(),
        );
        chunk.initialize_for_first_cell_x();
        chunk.advance_cell_x(0);
        chunk.select_cell_yz(40, 0);
        chunk
    }

    /// The `ContextProvider.forIndex` seam — the formerly recursive entry that
    /// stacks overflows on the `BlendDensity`/`Ap2` Mul/Min/Max fill paths when
    /// it calls itself. The inherent `NoiseChunk::for_index` shadows it (the
    /// trait impl now calls the inherent method explicitly), so each index must
    /// produce Java's exact `floorMod`/`floorDiv` in-cell coordinates against
    /// the cell starts. Also exercises the hostile negative-index wrap path
    /// (`usize::MAX` → `-1` as `i32`).
    #[test]
    fn for_index_matches_java_floor_mod_div_coordinates() {
        let chunk = interpolating_chunk();
        // After initialize/advance/select: cellStartBlockX = 0, Y = 256, Z = 0.
        // `forIndex(i)`: zInCell = floorMod(i, 4), xyIndex = floorDiv(i, 4),
        // xInCell = floorMod(xyIndex, 4), yInCell = 7 - floorDiv(xyIndex, 4).
        for (index, (block_x, block_y, block_z)) in [
            (0usize, (0, 263, 0)),
            (1, (0, 263, 1)),
            (3, (0, 263, 3)),
            (4, (1, 263, 0)),
            (7, (1, 263, 3)),
            (15, (3, 263, 3)),
            (16, (0, 262, 0)),
            (31, (3, 262, 3)),
            (32, (0, 261, 0)),
            (63, (3, 260, 3)),
        ] {
            let point = ContextProvider::for_index(&chunk, index);
            assert_eq!(
                (point.block_x(), point.block_y(), point.block_z()),
                (block_x, block_y, block_z),
                "for_index({index}) block coords must match Java floorMod/floorDiv"
            );
        }
        // Negative wrap: cellIndex `-1` (usize::MAX) → zInCell 3, xyIndex -1,
        // xInCell 3, yInCell 8 — Java's wrapping `Math.floorMod`/`Math.floorDiv`.
        let point = ContextProvider::for_index(&chunk, usize::MAX);
        assert_eq!(
            (point.block_x(), point.block_y(), point.block_z()),
            (3, 264, 3)
        );
        chunk.stop_interpolation();
    }

    /// `Ap2.fill_array` on the `Mul` branch computes `v1 * argument2.compute
    /// (provider.forIndex(i))` per slot — the path that recursed (stack
    /// overflow) when the provider was a `NoiseChunk`. The per-slot result must
    /// equal the direct per-index `compute` at the same provider coordinates.
    #[test]
    fn ap2_mul_fill_array_uses_chunk_provider() {
        let chunk = interpolating_chunk();
        // Both arguments non-constant so `two_argument_create` builds `Ap2`
        // (not the constant-folded `MulOrAdd`).
        let f = fns::mul(
            fns::y_clamped_gradient(0, 8, 0.0, 1.0),
            fns::y_clamped_gradient(0, 8, 1.0, 2.0),
        );
        assert!(f.as_any().downcast_ref::<fns::Ap2>().is_some());
        let mut out = [0.0; 64];
        f.fill_array(&mut out, &chunk);
        for (i, slot) in out.iter().enumerate() {
            let point = ContextProvider::for_index(&chunk, i);
            let expected = f.compute(point);
            assert!(
                (slot - expected).abs() < 1e-9,
                "mul fill_array slot {i} = {}, expected per-index compute {expected}",
                slot
            );
        }
        chunk.stop_interpolation();
    }

    /// Same hostile exercise for the `Ap2` `Min` branch (`provider.forIndex(i)`
    /// per slot), whose per-slot result must match the direct per-index compute.
    #[test]
    fn ap2_min_fill_array_uses_chunk_provider() {
        let chunk = interpolating_chunk();
        // Overlapping bounds (both `[0, 2]`) so `two_argument_create` does not
        // hit its non-overlapping-inputs `LOGGER.warn` debug branch.
        let f = fns::min(
            fns::y_clamped_gradient(0, 8, 0.0, 2.0),
            fns::y_clamped_gradient(0, 8, 0.0, 2.0),
        );
        assert!(f.as_any().downcast_ref::<fns::Ap2>().is_some());
        let mut out = [0.0; 64];
        f.fill_array(&mut out, &chunk);
        for (i, slot) in out.iter().enumerate() {
            let point = ContextProvider::for_index(&chunk, i);
            let expected = f.compute(point);
            assert!(
                (slot - expected).abs() < 1e-9,
                "min fill_array slot {i} = {}, expected per-index compute {expected}",
                slot
            );
        }
        chunk.stop_interpolation();
    }

    /// Same hostile exercise for the `Ap2` `Max` branch.
    #[test]
    fn ap2_max_fill_array_uses_chunk_provider() {
        let chunk = interpolating_chunk();
        let f = fns::max(
            fns::y_clamped_gradient(0, 8, 0.0, 1.0),
            fns::y_clamped_gradient(0, 8, 1.0, 2.0),
        );
        assert!(f.as_any().downcast_ref::<fns::Ap2>().is_some());
        let mut out = [0.0; 64];
        f.fill_array(&mut out, &chunk);
        for (i, slot) in out.iter().enumerate() {
            let point = ContextProvider::for_index(&chunk, i);
            let expected = f.compute(point);
            assert!(
                (slot - expected).abs() < 1e-9,
                "max fill_array slot {i} = {}, expected per-index compute {expected}",
                slot
            );
        }
        chunk.stop_interpolation();
    }

    /// `NoiseChunk.BlendDensity.fillArray` blends each filled slot against
    /// `provider.forIndex(i)` — the other formerly recursive path. With the
    /// empty blender (the only constructible value) `blendDensity` is the
    /// identity, so the output must equal the input's per-index fill.
    #[test]
    fn blend_density_fill_array_uses_chunk_provider() {
        let chunk = interpolating_chunk();
        let input = fns::y_clamped_gradient(0, 8, 0.0, 1.0);
        let blend = BlendDensity::new(input.clone(), Blender::empty());
        let mut out = [0.0; 64];
        blend.fill_array(&mut out, &chunk);
        for (i, slot) in out.iter().enumerate() {
            let point = ContextProvider::for_index(&chunk, i);
            let expected = input.compute(point);
            assert!(
                (slot - expected).abs() < 1e-9,
                "blend_density fill_array slot {i} = {}, expected per-index compute {expected}",
                slot
            );
        }
        chunk.stop_interpolation();
    }

    /// The critical per-index seam: during `selectCellYZ` the registered
    /// `CacheAllInCell` fill runs its wrapped function's `fillArray` (the 128
    /// `cellWidth*cellWidth*cellHeight` in-cell samples), whose per-index loop
    /// samples the noodle through `argument2.compute(provider.forIndex(i))`.
    /// Java's `forIndex` returns `NoiseChunk.this`, so the wrapped
    /// `NoiseInterpolator` takes the interpolation branch and produces the
    /// trilinear-lerped `interpolated` value. The old port returned a
    /// `SinglePointContext`, delegating to the raw `square` noise — so at a
    /// mid-cell in-cell y the cell read the raw `(y/8)^2` instead of the lerp3
    /// `y/8`. This asserts the cell cache holds the interpolated values.
    #[test]
    fn ap2_min_per_index_argument2_takes_interpolation_branch() {
        // finalDensity = min(constant(4.0), interpolated(square(y_gradient))),
        // with the y-gradient spanning exactly the cell's y-range
        // `[256, 264)` (`cellNoiseMinY = -8`, `cellHeight = 8`,
        // `selectCellYZ(40, 0)` → `cellStartBlockY = 256`). The interpolator's
        // y-corners are then `square(0.0) = 0.0` (bottom) and `square(1.0) =
        // 1.0` (top) — a pure y-lerp, so the in-cell sample at in-cell y
        // reads `y/8`. The raw `square` noise at that block is `(y/8)^2`, so
        // the two branches diverge at any interior y. The `min` is on the
        // interpolated arm everywhere (both span `[0, 1] < 4.0`), keeping the
        // assertion exact.
        let y_grad = fns::y_clamped_gradient(256, 264, 0.0, 1.0);
        let noodle = fns::interpolated(fns::mapped(&*y_grad, MappedType::Square));
        let settings = test_settings_with_final_density(fns::min(fns::constant(4.0), noodle));
        let (noise_registry, df_registry) = empty_registries();
        let state = RandomState::create(&settings, &noise_registry, &df_registry, 1234);
        let chunk = NoiseChunk::new(
            4,
            &state,
            0,
            0,
            &OVERWORLD_NOISE_SETTINGS,
            Arc::new(BeardifierMarker::instance()) as Arc<dyn DensityFunction>,
            &settings,
            Box::new(create_fluid_picker(&settings)),
            Blender::empty(),
        );
        chunk.initialize_for_first_cell_x();
        chunk.advance_cell_x(0);
        chunk.select_cell_yz(40, 0);

        // Read the one registered `CacheAllInCell` (the `fullNoiseDensity`
        // wrapper) and verify its in-cell values are the lerp3 `y/8` — not the
        // raw `(y/8)^2` the pre-fix `forIndex` snapshot produced.
        let values = chunk
            .state
            .lock()
            .unwrap()
            .cell_caches[0]
            .values
            .lock()
            .unwrap()
            .clone();
        let cell_width = chunk.cell_width as usize; // 4
        let cell_height = chunk.cell_height as usize; // 8
        for y in 0..cell_height {
            for x in 0..cell_width {
                for z in 0..cell_width {
                    let index = ((cell_height - 1 - y) * cell_width + x) * cell_width + z;
                    let expected = y as f64 / cell_height as f64;
                    assert!(
                        (values[index] - expected).abs() < 1e-9,
                        "cell ({x}, {y}, {z}) value {} = expected lerp3 {expected}, not the raw (y/8)^2 {}",
                        values[index],
                        (y as f64 / cell_height as f64).powi(2)
                    );
                }
            }
        }
        chunk.stop_interpolation();
    }
}
