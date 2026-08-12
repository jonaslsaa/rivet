//! Port of `net.minecraft.world.level.levelgen.Aquifer` (26.2).
//!
//! The fluid-aquifer filler: the `Aquifer` interface (`computeSubstance`/
//! `shouldScheduleFluidUpdate`), the `FluidPicker`/`FluidStatus` types, the
//! `NoiseBasedAquifer` 4-closest-cell pressure model, and the disabled
//! singleton.
//!
//! Translation notes:
//! - Java's `@Nullable BlockState` maps to `Option<BlockState>` (`None` = null
//!   = "no block here"). `Blocks.AIR` is never returned as `None` — a genuine
//!   air state is `Some(AIR)`; only "no change" is `None`.
//! - Java's `Aquifer` interface methods mutate instance state
//!   (`shouldScheduleFluidUpdate`, the two caches); the Rust `Visitor`/closure
//!   seams call through `&self`, so the mutable state lives in a
//!   `Mutex<AquiferState>` (single-threaded sync-tick model; the mutex is the
//!   interior-mutability seam, uncontended). `getAquiferStatus` takes the
//!   locked state as a parameter to avoid re-locking (no nested lock).
//! - The `noiseChunk` self-reference is broken with a
//!   `PreliminarySurfaceLevelFn` closure: the `NoiseChunk` passes the
//!   quantized `preliminarySurfaceLevel(sampleX, sampleZ)` lookup (its own
//!   `ColumnPos`-keyed cache included), so the aquifer needs no `&NoiseChunk`.
//!   `maxPreliminarySurfaceLevel` is computed from the same closure during
//!   construction.
//! - `-0.225F`/`0.9F` (the `OverworldBiomeBuilder.isDeepDarkRegion` leaf) are
//!   `float` literals meeting `double` comparisons — widened via `as f64`.
//! - `BlockPos.asLong(x, y, z)` is the instance `BlockPos::new(x,y,z).as_long()`;
//!   `BlockPos.getX(long)` etc. are `get_x_long`/`get_y_long`/`get_z_long`.
//! - `state.is(Blocks.LAVA)` — `state.block() == Blocks::LAVA.id()`.

use crate::block::blocks::Blocks;
use crate::level::dimension::dimension_type::WAY_BELOW_MIN_Y;
use crate::levelgen::noise::density_function::{
    DensityFunction, FunctionContext, SinglePointContext,
};
use crate::levelgen::noise::noise_router::NoiseRouter;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};
use rivet_util::mth;
use rivet_util::random::{PositionalRandomFactory, RandomSource};
use rivet_util::worldgen_random::{AlgorithmPositionalRandomFactory, AlgorithmRandomSource};
use std::sync::{Arc, Mutex};

/// `Aquifer.FLOWING_UPDATE_SIMULARITY` — `similarity(square(10), square(12))`.
const FLOWING_UPDATE_SIMULARITY: f64 = 1.0 - (144.0 - 100.0) / 25.0;

/// `SAMPLE_OFFSET_X` — `-5`.
const SAMPLE_OFFSET_X: i32 = -5;
/// `SAMPLE_OFFSET_Y` — `1`.
const SAMPLE_OFFSET_Y: i32 = 1;
/// `SAMPLE_OFFSET_Z` — `-5`.
const SAMPLE_OFFSET_Z: i32 = -5;
/// `MIN_CELL_SAMPLE_X` — `0`.
const MIN_CELL_SAMPLE_X: i32 = 0;
/// `MIN_CELL_SAMPLE_Y` — `-1`.
const MIN_CELL_SAMPLE_Y: i32 = -1;
/// `MIN_CELL_SAMPLE_Z` — `0`.
const MIN_CELL_SAMPLE_Z: i32 = 0;
/// `MAX_CELL_SAMPLE_X` — `1`. Java declares all three `MAX_CELL_SAMPLE_*`
/// constants but the sampling loop only uses the Y bound; kept for fidelity.
#[allow(dead_code)]
const MAX_CELL_SAMPLE_X: i32 = 1;
/// `MAX_CELL_SAMPLE_Y` — `1`.
const MAX_CELL_SAMPLE_Y: i32 = 1;
/// `MAX_CELL_SAMPLE_Z` — `1`. See `MAX_CELL_SAMPLE_X` (Java-faithful dead).
#[allow(dead_code)]
const MAX_CELL_SAMPLE_Z: i32 = 1;

/// `SURFACE_SAMPLING_OFFSETS_IN_CHUNKS` — the 13 `(dx, dz)` chunk-section
/// offsets (a `const` because the arrays are compile-time constants).
const SURFACE_SAMPLING_OFFSETS_IN_CHUNKS: [[i32; 2]; 13] = [
    [0, 0],
    [-2, -1],
    [-1, -1],
    [0, -1],
    [1, -1],
    [-3, 0],
    [-2, 0],
    [-1, 0],
    [1, 0],
    [-2, 1],
    [-1, 1],
    [0, 1],
    [1, 1],
];

/// `Aquifer.FluidStatus` — the record `(int fluidLevel, BlockState fluidType)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidStatus {
    /// `fluidLevel`.
    pub fluid_level: i32,
    /// `fluidType`.
    pub fluid_type: BlockState,
}

impl FluidStatus {
    /// `FluidStatus.at(int blockY)` — `blockY < fluidLevel ? fluidType : AIR`.
    pub fn at(&self, block_y: i32) -> BlockState {
        if block_y < self.fluid_level {
            self.fluid_type
        } else {
            Blocks::AIR.default_block_state()
        }
    }
}

/// `Aquifer.FluidPicker` — `computeFluid(int blockX, int blockY, int blockZ)`.
pub trait FluidPicker: Send + Sync {
    /// `computeFluid`.
    fn compute_fluid(&self, block_x: i32, block_y: i32, block_z: i32) -> FluidStatus;
}

/// `Aquifer` — the fluid filler. `computeSubstance` returns `None` for Java's
/// `null` (no block) and `Some(state)` otherwise.
pub trait Aquifer: Send + Sync {
    /// `computeSubstance(FunctionContext, double density)`.
    fn compute_substance(&self, context: &dyn FunctionContext, density: f64) -> Option<BlockState>;
    /// `shouldScheduleFluidUpdate()`.
    fn should_schedule_fluid_update(&self) -> bool;
}

/// The `PreliminarySurfaceLevelFn` seam — the `NoiseChunk`'s quantized
/// `preliminarySurfaceLevel(sampleX, sampleZ)` lookup (with its cache).
pub type PreliminarySurfaceLevelFn = Arc<dyn Fn(i32, i32) -> i32 + Send + Sync>;

/// `Aquifer.create(...)` — the `NoiseBasedAquifer` constructor wrapper.
///
/// The `noiseChunk` self-reference is broken: `preliminary_surface_level` is
/// the chunk's quantized `preliminarySurfaceLevel` lookup (its `ColumnPos`
/// cache included), so the aquifer needs no `&NoiseChunk` handle.
#[allow(clippy::too_many_arguments)]
pub fn create(
    pos: &ChunkPos,
    router: &NoiseRouter,
    positional_random_factory: AlgorithmPositionalRandomFactory,
    min_block_y: i32,
    y_block_size: i32,
    fluid_rule: Box<dyn FluidPicker>,
    preliminary_surface_level: PreliminarySurfaceLevelFn,
) -> Arc<dyn Aquifer> {
    Arc::new(NoiseBasedAquifer::new(
        router,
        pos,
        positional_random_factory,
        min_block_y,
        y_block_size,
        fluid_rule,
        preliminary_surface_level,
    ))
}

/// `Aquifer.createDisabled(FluidPicker)` — `density > 0 ? null :
/// fluidRule.computeFluid(x, y, z).at(y)`.
pub fn create_disabled(fluid_rule: Box<dyn FluidPicker>) -> Arc<dyn Aquifer> {
    Arc::new(DisabledAquifer { fluid_rule })
}

/// The disabled aquifer — Java's anonymous `Aquifer` in `createDisabled`.
struct DisabledAquifer {
    fluid_rule: Box<dyn FluidPicker>,
}

impl Aquifer for DisabledAquifer {
    fn compute_substance(&self, context: &dyn FunctionContext, density: f64) -> Option<BlockState> {
        if density > 0.0 {
            None
        } else {
            Some(
                self.fluid_rule
                    .compute_fluid(context.block_x(), context.block_y(), context.block_z())
                    .at(context.block_y()),
            )
        }
    }

    fn should_schedule_fluid_update(&self) -> bool {
        false
    }
}

/// The mutable state the `NoiseBasedAquifer` caches (Java's `FluidStatus[]`
/// aquiferCache, `long[]` aquiferLocationCache, and the
/// `shouldScheduleFluidUpdate` flag).
struct AquiferState {
    aquifer_cache: Vec<Option<FluidStatus>>,
    aquifer_location_cache: Vec<i64>,
    should_schedule_fluid_update: bool,
}

/// `Aquifer.NoiseBasedAquifer` — the 4-closest-cell fluid model.
struct NoiseBasedAquifer {
    barrier_noise: Arc<dyn DensityFunction>,
    fluid_level_floodedness_noise: Arc<dyn DensityFunction>,
    fluid_level_spread_noise: Arc<dyn DensityFunction>,
    lava_noise: Arc<dyn DensityFunction>,
    erosion: Arc<dyn DensityFunction>,
    depth: Arc<dyn DensityFunction>,
    positional_random_factory: AlgorithmPositionalRandomFactory,
    state: Mutex<AquiferState>,
    global_fluid_picker: Box<dyn FluidPicker>,
    skip_sampling_above_y: i32,
    min_grid_x: i32,
    min_grid_y: i32,
    min_grid_z: i32,
    grid_size_x: i32,
    grid_size_z: i32,
    preliminary_surface_level: PreliminarySurfaceLevelFn,
}

impl NoiseBasedAquifer {
    /// The private `NoiseBasedAquifer(...)` constructor.
    #[allow(clippy::too_many_arguments)]
    fn new(
        router: &NoiseRouter,
        pos: &ChunkPos,
        positional_random_factory: AlgorithmPositionalRandomFactory,
        min_block_y: i32,
        y_block_size: i32,
        global_fluid_picker: Box<dyn FluidPicker>,
        preliminary_surface_level: PreliminarySurfaceLevelFn,
    ) -> Self {
        let barrier_noise = router.barrier_noise().clone();
        let fluid_level_floodedness_noise = router.fluid_level_floodedness_noise().clone();
        let fluid_level_spread_noise = router.fluid_level_spread_noise().clone();
        let lava_noise = router.lava_noise().clone();
        let erosion = router.erosion().clone();
        let depth = router.depth().clone();

        let min_grid_x = grid_x(pos.get_min_block_x().wrapping_add(SAMPLE_OFFSET_X));
        let max_grid_x =
            grid_x(pos.get_max_block_x().wrapping_add(SAMPLE_OFFSET_X)).wrapping_add(1);
        let grid_size_x = max_grid_x.wrapping_sub(min_grid_x).wrapping_add(1);
        let min_grid_y =
            grid_y(min_block_y.wrapping_add(SAMPLE_OFFSET_Y)).wrapping_add(MIN_CELL_SAMPLE_Y);
        let max_grid_y = grid_y(
            min_block_y
                .wrapping_add(y_block_size)
                .wrapping_add(SAMPLE_OFFSET_Y),
        )
        .wrapping_add(MAX_CELL_SAMPLE_Y);
        let grid_size_y = max_grid_y.wrapping_sub(min_grid_y).wrapping_add(1);
        let min_grid_z = grid_z(pos.get_min_block_z().wrapping_add(SAMPLE_OFFSET_Z));
        let max_grid_z =
            grid_z(pos.get_max_block_z().wrapping_add(SAMPLE_OFFSET_Z)).wrapping_add(1);
        let grid_size_z = max_grid_z.wrapping_sub(min_grid_z).wrapping_add(1);
        let total_grid_size = grid_size_x * grid_size_y * grid_size_z;
        let state = Mutex::new(AquiferState {
            aquifer_cache: vec![None; total_grid_size as usize],
            aquifer_location_cache: vec![i64::MAX; total_grid_size as usize],
            should_schedule_fluid_update: false,
        });

        // `maxAdjustedSurfaceLevel = adjustSurfaceLevel(noiseChunk.maxPreliminarySurfaceLevel(
        //   fromGridX(minGridX, 0), fromGridZ(minGridZ, 0), fromGridX(maxGridX, 9), fromGridZ(maxGridZ, 9)))`.
        let max_adjusted_surface_level = {
            let min_block_x = from_grid_x(min_grid_x, MIN_CELL_SAMPLE_X);
            let min_block_z = from_grid_z(min_grid_z, MIN_CELL_SAMPLE_Z);
            let max_block_x = from_grid_x(max_grid_x, 9);
            let max_block_z = from_grid_z(max_grid_z, 9);
            adjust_surface_level(max_preliminary_surface_level(
                &preliminary_surface_level,
                min_block_x,
                min_block_z,
                max_block_x,
                max_block_z,
            ))
        };
        // `gridY(maxAdjustedSurfaceLevel + 12) - -1`; `fromGridY(skipSamplingAboveGridY, 11) - 1`.
        let skip_sampling_above_grid_y =
            grid_y(max_adjusted_surface_level.wrapping_add(12)).wrapping_sub(-1);
        let skip_sampling_above_y = from_grid_y(skip_sampling_above_grid_y, 11).wrapping_sub(1);

        NoiseBasedAquifer {
            barrier_noise,
            fluid_level_floodedness_noise,
            fluid_level_spread_noise,
            lava_noise,
            erosion,
            depth,
            positional_random_factory,
            state,
            global_fluid_picker,
            skip_sampling_above_y,
            min_grid_x,
            min_grid_y,
            min_grid_z,
            grid_size_x,
            grid_size_z,
            preliminary_surface_level,
        }
    }

    /// `getIndex(int gridX, int gridY, int gridZ)` — Java `int` arithmetic
    /// wraps; the Rust port returns the array index (a `usize`).
    fn get_index(&self, grid_x: i32, grid_y: i32, grid_z: i32) -> usize {
        let x = grid_x.wrapping_sub(self.min_grid_x);
        let y = grid_y.wrapping_sub(self.min_grid_y);
        let z = grid_z.wrapping_sub(self.min_grid_z);
        (y.wrapping_mul(self.grid_size_z).wrapping_add(z))
            .wrapping_mul(self.grid_size_x)
            .wrapping_add(x) as usize
    }

    /// `computeFluid(int x, int y, int z)` — the per-cell fluid status.
    fn compute_fluid(&self, x: i32, y: i32, z: i32) -> FluidStatus {
        let global_fluid = self.global_fluid_picker.compute_fluid(x, y, z);
        let mut lowest_preliminary_surface = i32::MAX;
        let top_of_aquifer_cell = y.wrapping_add(12);
        let bottom_of_aquifer_cell = y.wrapping_sub(12);
        let mut surface_at_center_is_under_global_fluid_level = false;

        for offset in SURFACE_SAMPLING_OFFSETS_IN_CHUNKS {
            let sample_x = x.wrapping_add(SectionPos::section_to_block_coord(offset[0]));
            let sample_z = z.wrapping_add(SectionPos::section_to_block_coord(offset[1]));
            let preliminary_surface_level = (self.preliminary_surface_level)(sample_x, sample_z);
            let adjusted_surface_level = adjust_surface_level(preliminary_surface_level);
            let start = offset[0] == 0 && offset[1] == 0;
            if start && bottom_of_aquifer_cell > adjusted_surface_level {
                return global_fluid;
            }

            let top_of_aquifer_cell_pokes_above_surface =
                top_of_aquifer_cell > adjusted_surface_level;
            if top_of_aquifer_cell_pokes_above_surface || start {
                let global_fluid_at_surface = self.global_fluid_picker.compute_fluid(
                    sample_x,
                    adjusted_surface_level,
                    sample_z,
                );
                if !global_fluid_at_surface.at(adjusted_surface_level).is_air() {
                    if start {
                        surface_at_center_is_under_global_fluid_level = true;
                    }
                    if top_of_aquifer_cell_pokes_above_surface {
                        return global_fluid_at_surface;
                    }
                }
            }

            lowest_preliminary_surface = lowest_preliminary_surface.min(preliminary_surface_level);
        }

        let fluid_surface_level = self.compute_surface_level(
            x,
            y,
            z,
            global_fluid,
            lowest_preliminary_surface,
            surface_at_center_is_under_global_fluid_level,
        );
        FluidStatus {
            fluid_level: fluid_surface_level,
            fluid_type: self.compute_fluid_type(x, y, z, global_fluid, fluid_surface_level),
        }
    }

    /// `computeSurfaceLevel(...)`.
    fn compute_surface_level(
        &self,
        x: i32,
        y: i32,
        z: i32,
        global_fluid: FluidStatus,
        lowest_preliminary_surface: i32,
        surface_at_center_is_under_global_fluid_level: bool,
    ) -> i32 {
        let context = SinglePointContext::new(x, y, z);
        let (partially_floodedness, fully_floodidness) =
            if is_deep_dark_region(&self.erosion, &self.depth, &context) {
                (-1.0, -1.0)
            } else {
                let distance_below_surface =
                    lowest_preliminary_surface.wrapping_add(8).wrapping_sub(y);
                let floodedness_max_depth = 64;
                let floodedness_factor = if surface_at_center_is_under_global_fluid_level {
                    mth::clamped_map(
                        distance_below_surface as f64,
                        0.0,
                        floodedness_max_depth as f64,
                        1.0,
                        0.0,
                    )
                } else {
                    0.0
                };
                let floodedness_noise_value = mth::clamp_f64(
                    self.fluid_level_floodedness_noise.compute(&context),
                    -1.0,
                    1.0,
                );
                let fully_flooded_threshold = mth::map(floodedness_factor, 1.0, 0.0, -0.3, 0.8);
                let partially_flooded_threshold = mth::map(floodedness_factor, 1.0, 0.0, -0.8, 0.4);
                (
                    floodedness_noise_value - partially_flooded_threshold,
                    floodedness_noise_value - fully_flooded_threshold,
                )
            };

        if fully_floodidness > 0.0 {
            global_fluid.fluid_level
        } else if partially_floodedness > 0.0 {
            self.compute_randomized_fluid_surface_level(x, y, z, lowest_preliminary_surface)
        } else {
            WAY_BELOW_MIN_Y
        }
    }

    /// `computeRandomizedFluidSurfaceLevel(int x, int y, int z, int lowestPreliminarySurface)`.
    fn compute_randomized_fluid_surface_level(
        &self,
        x: i32,
        y: i32,
        z: i32,
        lowest_preliminary_surface: i32,
    ) -> i32 {
        let fluid_cell_width = 16;
        let fluid_cell_height = 40;
        let fluid_level_cell_x = mth::floor_div(x, fluid_cell_width);
        let fluid_level_cell_y = mth::floor_div(y, fluid_cell_height);
        let fluid_level_cell_z = mth::floor_div(z, fluid_cell_width);
        let fluid_cell_middle_y = fluid_level_cell_y * 40 + 20;
        let max_spread = 10;
        let fluid_level_spread = self
            .fluid_level_spread_noise
            .compute(&SinglePointContext::new(
                fluid_level_cell_x,
                fluid_level_cell_y,
                fluid_level_cell_z,
            ))
            * max_spread as f64;
        let fluid_level_spread_quantized = mth::quantize(fluid_level_spread, 3);
        let target_fluid_surface_level =
            fluid_cell_middle_y.wrapping_add(fluid_level_spread_quantized);
        lowest_preliminary_surface.min(target_fluid_surface_level)
    }

    /// `computeFluidType(int x, int y, int z, FluidStatus globalFluid, int fluidSurfaceLevel)`.
    fn compute_fluid_type(
        &self,
        x: i32,
        y: i32,
        z: i32,
        global_fluid: FluidStatus,
        fluid_surface_level: i32,
    ) -> BlockState {
        let mut fluid_type = global_fluid.fluid_type;
        if fluid_surface_level <= -10
            && fluid_surface_level != WAY_BELOW_MIN_Y
            && global_fluid.fluid_type.block() != Blocks::LAVA.id()
        {
            let fluid_type_cell_width = 64;
            let fluid_type_cell_height = 40;
            let fluid_type_cell_x = mth::floor_div(x, fluid_type_cell_width);
            let fluid_type_cell_y = mth::floor_div(y, fluid_type_cell_height);
            let fluid_type_cell_z = mth::floor_div(z, fluid_type_cell_width);
            let lava_noise_value = self.lava_noise.compute(&SinglePointContext::new(
                fluid_type_cell_x,
                fluid_type_cell_y,
                fluid_type_cell_z,
            ));
            if lava_noise_value.abs() > 0.3 {
                fluid_type = Blocks::LAVA.default_block_state();
            }
        }
        fluid_type
    }

    /// `getAquiferStatus(int index)` — resolves (and caches) the cell fluid
    /// status. Takes the locked `AquiferState` to avoid re-locking.
    fn get_aquifer_status(&self, state: &mut AquiferState, index: usize) -> FluidStatus {
        if let Some(old_status) = state.aquifer_cache[index] {
            return old_status;
        }
        let location = state.aquifer_location_cache[index];
        let status = self.compute_fluid(
            BlockPos::get_x_long(location),
            BlockPos::get_y_long(location),
            BlockPos::get_z_long(location),
        );
        state.aquifer_cache[index] = Some(status);
        status
    }

    /// `calculatePressure(FunctionContext, MutableDouble, FluidStatus, FluidStatus)`.
    fn calculate_pressure(
        &self,
        context: &dyn FunctionContext,
        barrier_noise_value: &mut f64,
        status_closest_1: FluidStatus,
        status_closest_2: FluidStatus,
    ) -> f64 {
        let pos_y = context.block_y();
        let type1 = status_closest_1.at(pos_y);
        let type2 = status_closest_2.at(pos_y);
        if (type1.block() != Blocks::LAVA.id() || type2.block() != Blocks::WATER.id())
            && (type1.block() != Blocks::WATER.id() || type2.block() != Blocks::LAVA.id())
        {
            let fluid_y_diff = (status_closest_1.fluid_level - status_closest_2.fluid_level).abs();
            if fluid_y_diff == 0 {
                return 0.0;
            }

            let average_fluid_y =
                0.5 * (status_closest_1.fluid_level as f64 + status_closest_2.fluid_level as f64);
            let how_far_above_average_fluid_point = pos_y as f64 + 0.5 - average_fluid_y;
            let base_value = fluid_y_diff as f64 / 2.0;
            let distance_from_barrier_edge_towards_middle =
                base_value - how_far_above_average_fluid_point.abs();
            let gradient = if how_far_above_average_fluid_point > 0.0 {
                let center_point = 0.0 + distance_from_barrier_edge_towards_middle;
                if center_point > 0.0 {
                    center_point / 1.5
                } else {
                    center_point / 2.5
                }
            } else {
                let center_point = 3.0 + distance_from_barrier_edge_towards_middle;
                if center_point > 0.0 {
                    center_point / 3.0
                } else {
                    center_point / 10.0
                }
            };

            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            // Java's exact `!(x < -2) && !(x > 2)`: `>=`/`<=` would flip a NaN gradient into the `0.0` branch (PORTING.md fidelity).
            let noise_value = if !(gradient < -2.0) && !(gradient > 2.0) {
                let current_noise_value = *barrier_noise_value;
                if current_noise_value.is_nan() {
                    let barrier_noise = self.barrier_noise.compute(context);
                    *barrier_noise_value = barrier_noise;
                    barrier_noise
                } else {
                    current_noise_value
                }
            } else {
                0.0
            };

            2.0 * (noise_value + gradient)
        } else {
            2.0
        }
    }
}

impl Aquifer for NoiseBasedAquifer {
    fn compute_substance(&self, context: &dyn FunctionContext, density: f64) -> Option<BlockState> {
        if density > 0.0 {
            self.state.lock().unwrap().should_schedule_fluid_update = false;
            return None;
        }

        let pos_x = context.block_x();
        let pos_y = context.block_y();
        let pos_z = context.block_z();
        let global_fluid = self.global_fluid_picker.compute_fluid(pos_x, pos_y, pos_z);
        if pos_y > self.skip_sampling_above_y {
            self.state.lock().unwrap().should_schedule_fluid_update = false;
            return Some(global_fluid.at(pos_y));
        }

        if global_fluid.at(pos_y).block() == Blocks::LAVA.id() {
            self.state.lock().unwrap().should_schedule_fluid_update = false;
            return Some(
                if rivet_core::shared_constants::DEBUG_DISABLE_FLUID_GENERATION {
                    Blocks::AIR.default_block_state()
                } else {
                    Blocks::LAVA.default_block_state()
                },
            );
        }

        let x_anchor = grid_x(pos_x.wrapping_add(SAMPLE_OFFSET_X));
        let y_anchor = grid_y(pos_y.wrapping_add(SAMPLE_OFFSET_Y));
        let z_anchor = grid_z(pos_z.wrapping_add(SAMPLE_OFFSET_Z));
        let mut distance_sqr1 = i32::MAX;
        let mut distance_sqr2 = i32::MAX;
        let mut distance_sqr3 = i32::MAX;
        let mut distance_sqr4 = i32::MAX;
        let mut closest_index1 = 0usize;
        let mut closest_index2 = 0usize;
        let mut closest_index3 = 0usize;
        let mut closest_index4 = 0usize;

        {
            let mut state = self.state.lock().unwrap();
            for x1 in 0..=1 {
                for y1 in -1..=1 {
                    for z1 in 0..=1 {
                        let spaced_grid_x = x_anchor.wrapping_add(x1);
                        let spaced_grid_y = y_anchor.wrapping_add(y1);
                        let spaced_grid_z = z_anchor.wrapping_add(z1);
                        let index = self.get_index(spaced_grid_x, spaced_grid_y, spaced_grid_z);
                        let existing_location = state.aquifer_location_cache[index];
                        let location = if existing_location != i64::MAX {
                            existing_location
                        } else {
                            let mut random: AlgorithmRandomSource = self
                                .positional_random_factory
                                .at(spaced_grid_x, spaced_grid_y, spaced_grid_z);
                            let location = BlockPos::new(
                                from_grid_x(spaced_grid_x, random.next_int_bound(10)),
                                from_grid_y(spaced_grid_y, random.next_int_bound(9)),
                                from_grid_z(spaced_grid_z, random.next_int_bound(10)),
                            )
                            .as_long();
                            state.aquifer_location_cache[index] = location;
                            location
                        };

                        let dx = BlockPos::get_x_long(location).wrapping_sub(pos_x);
                        let dy = BlockPos::get_y_long(location).wrapping_sub(pos_y);
                        let dz = BlockPos::get_z_long(location).wrapping_sub(pos_z);
                        let new_distance = dx
                            .wrapping_mul(dx)
                            .wrapping_add(dy.wrapping_mul(dy))
                            .wrapping_add(dz.wrapping_mul(dz));
                        if distance_sqr1 >= new_distance {
                            closest_index4 = closest_index3;
                            closest_index3 = closest_index2;
                            closest_index2 = closest_index1;
                            closest_index1 = index;
                            distance_sqr4 = distance_sqr3;
                            distance_sqr3 = distance_sqr2;
                            distance_sqr2 = distance_sqr1;
                            distance_sqr1 = new_distance;
                        } else if distance_sqr2 >= new_distance {
                            closest_index4 = closest_index3;
                            closest_index3 = closest_index2;
                            closest_index2 = index;
                            distance_sqr4 = distance_sqr3;
                            distance_sqr3 = distance_sqr2;
                            distance_sqr2 = new_distance;
                        } else if distance_sqr3 >= new_distance {
                            closest_index4 = closest_index3;
                            closest_index3 = index;
                            distance_sqr4 = distance_sqr3;
                            distance_sqr3 = new_distance;
                        } else if distance_sqr4 >= new_distance {
                            closest_index4 = index;
                            distance_sqr4 = new_distance;
                        }
                    }
                }
            }

            let closest_status_1 = self.get_aquifer_status(&mut state, closest_index1);
            let similarity12 = similarity(distance_sqr1, distance_sqr2);
            let fluid_state = closest_status_1.at(pos_y);
            let actual_fluid_state = if rivet_core::shared_constants::DEBUG_DISABLE_FLUID_GENERATION
            {
                Blocks::AIR.default_block_state()
            } else {
                fluid_state
            };
            if similarity12 <= 0.0 {
                if similarity12 >= FLOWING_UPDATE_SIMULARITY {
                    let closest_status_2 = self.get_aquifer_status(&mut state, closest_index2);
                    state.should_schedule_fluid_update = closest_status_1 != closest_status_2;
                } else {
                    state.should_schedule_fluid_update = false;
                }
                return Some(actual_fluid_state);
            }

            if fluid_state.block() == Blocks::WATER.id()
                && self
                    .global_fluid_picker
                    .compute_fluid(pos_x, pos_y - 1, pos_z)
                    .at(pos_y - 1)
                    .block()
                    == Blocks::LAVA.id()
            {
                state.should_schedule_fluid_update = true;
                return Some(actual_fluid_state);
            }

            let mut barrier_noise_value = f64::NAN;
            let closest_status_2 = self.get_aquifer_status(&mut state, closest_index2);
            let barrier12 = similarity12
                * self.calculate_pressure(
                    context,
                    &mut barrier_noise_value,
                    closest_status_1,
                    closest_status_2,
                );
            if density + barrier12 > 0.0 {
                state.should_schedule_fluid_update = false;
                return None;
            }

            let closest_status_3 = self.get_aquifer_status(&mut state, closest_index3);
            let similarity13 = similarity(distance_sqr1, distance_sqr3);
            if similarity13 > 0.0 {
                let barrier13 = similarity12
                    * similarity13
                    * self.calculate_pressure(
                        context,
                        &mut barrier_noise_value,
                        closest_status_1,
                        closest_status_3,
                    );
                if density + barrier13 > 0.0 {
                    state.should_schedule_fluid_update = false;
                    return None;
                }
            }

            let similarity23 = similarity(distance_sqr2, distance_sqr3);
            if similarity23 > 0.0 {
                let barrier23 = similarity12
                    * similarity23
                    * self.calculate_pressure(
                        context,
                        &mut barrier_noise_value,
                        closest_status_2,
                        closest_status_3,
                    );
                if density + barrier23 > 0.0 {
                    state.should_schedule_fluid_update = false;
                    return None;
                }
            }

            let may_flow12 = closest_status_1 != closest_status_2;
            let may_flow23 =
                similarity23 >= FLOWING_UPDATE_SIMULARITY && closest_status_2 != closest_status_3;
            let may_flow13 =
                similarity13 >= FLOWING_UPDATE_SIMULARITY && closest_status_1 != closest_status_3;
            if !may_flow12 && !may_flow23 && !may_flow13 {
                state.should_schedule_fluid_update = similarity13 >= FLOWING_UPDATE_SIMULARITY
                    && similarity(distance_sqr1, distance_sqr4) >= FLOWING_UPDATE_SIMULARITY
                    && closest_status_1 != self.get_aquifer_status(&mut state, closest_index4);
            } else {
                state.should_schedule_fluid_update = true;
            }
            Some(actual_fluid_state)
        }
    }

    fn should_schedule_fluid_update(&self) -> bool {
        self.state.lock().unwrap().should_schedule_fluid_update
    }
}

/// `similarity(int distanceSqr1, int distanceSqr2)` — `1.0 - (d2 - d1) / 25.0`.
fn similarity(distance_sqr1: i32, distance_sqr2: i32) -> f64 {
    let threshold = 25.0;
    1.0 - (distance_sqr2 - distance_sqr1) as f64 / threshold
}

/// `gridX(int blockCoord)` — `blockCoord >> 4`.
fn grid_x(block_coord: i32) -> i32 {
    block_coord >> 4
}

/// `fromGridX(int gridCoord, int blockOffset)` — `(gridCoord << 4) + blockOffset`.
fn from_grid_x(grid_coord: i32, block_offset: i32) -> i32 {
    (grid_coord << 4) + block_offset
}

/// `gridY(int blockCoord)` — `Math.floorDiv(blockCoord, 12)`.
fn grid_y(block_coord: i32) -> i32 {
    mth::floor_div(block_coord, 12)
}

/// `fromGridY(int gridCoord, int blockOffset)` — `gridCoord * 12 + blockOffset`.
fn from_grid_y(grid_coord: i32, block_offset: i32) -> i32 {
    grid_coord * 12 + block_offset
}

/// `gridZ(int blockCoord)` — `blockCoord >> 4`.
fn grid_z(block_coord: i32) -> i32 {
    block_coord >> 4
}

/// `fromGridZ(int gridCoord, int blockOffset)` — `(gridCoord << 4) + blockOffset`.
fn from_grid_z(grid_coord: i32, block_offset: i32) -> i32 {
    (grid_coord << 4) + block_offset
}

/// `adjustSurfaceLevel(int preliminarySurfaceLevel)` — `preliminarySurfaceLevel + 8`.
fn adjust_surface_level(preliminary_surface_level: i32) -> i32 {
    preliminary_surface_level.wrapping_add(8)
}

/// `OverworldBiomeBuilder.isDeepDarkRegion(erosion, depth, context)` — the
/// inlined biome-unit leaf this SCC reads.
fn is_deep_dark_region(
    erosion: &Arc<dyn DensityFunction>,
    depth: &Arc<dyn DensityFunction>,
    context: &dyn FunctionContext,
) -> bool {
    erosion.compute(context) < (-0.225f32) as f64 && depth.compute(context) > (0.9f32) as f64
}

/// `NoiseChunk.maxPreliminarySurfaceLevel(minX, minZ, maxX, maxZ)` — the
/// step-4 max scan over the preliminary surface function.
fn max_preliminary_surface_level(
    f: &PreliminarySurfaceLevelFn,
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
            let surface_level = f(block_x, block_z);
            if surface_level > max_y {
                max_y = surface_level;
            }
            block_x += 4;
        }
        block_z += 4;
    }
    max_y
}
