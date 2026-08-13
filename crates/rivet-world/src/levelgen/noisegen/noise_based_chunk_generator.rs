//! Port of `net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator` (26.2).
//!
//! The `ChunkGenerator` subclass that owns the per-world `NoiseGeneratorSettings`
//! holder, the memoized global `FluidPicker`, the single-column noise
//! interpolation (`getInterpolatedNoiseValue`/`iterateNoiseColumn`), the
//! debug-preliminary-surface-level overlay, and the three height/sea-level
//! accessors.
//!
//! Translation notes (the ownership-model seams):
//! - Java extends the abstract `ChunkGenerator` (biome source + codec dispatch),
//!   owned by `mc.world.level.chunk.generator`. The noisegen unit does not port
//!   that base; the methods that are pure overrides of it and have unported
//!   parameter types (`createBiomes`, `applyCarvers`, `fillFromNoise`,
//!   `buildSurface`, `spawnOriginalMobs`) are `STUB`-marked `*_stub` methods on
//!   the value shell — they keep Java's exact intent and defer the
//!   world-touching body to its owning unit. `addDebugScreenInfo` (router reads
//!   + string formatting) has fully ported parameter types, so it is ported in
//!     full. `CODEC` (the `BiomeSource` + `NoiseGeneratorSettings` record codec)
//!     defers with the `ChunkGenerator` unit (no `BiomeSource` ported here).
//! - Java memoizes the global fluid picker in `Suppliers.memoize`; the picker
//!   is a pure `Copy` struct here (the three `FluidStatus` values Java's
//!   closure captures), so it needs no memoization wrapper.
//! - `getInterpolatedNoiseValue`/`iterateNoiseColumn` are ported faithfully
//!   (the `BeardifierMarker.INSTANCE` beardifier, `Blender.empty()`); their
//!   `NoiseChunk` construction matches the wave's `NoiseChunk::new` seam.
//! - `settings.value()` needs the `NOISE_SETTINGS` `HolderLookup` (RivetTodo
//!   #126); every noisegen construction resolves the settings holder through
//!   `RandomState.create_from_provider`, so the generator's holder is a
//!   `Direct` value and `settings_value` reads it inline. A `Reference` holder
//!   (an un-threaded lookup) panics — the same "unbound value" contract as
//!   `Holder::value`.
//!
//! The world/level surfaces (`WorldGenRegion`, `StructureManager`,
//! `BiomeManager`/`BiomeSource`, `CarvingContext`/`CarvingMask`,
//! `NaturalSpawner`, `NoiseColumn`, `BiomeGenerationSettings`,
//! `ConfiguredWorldCarver`, `LevelChunkSection`, `ProtoChunk`, `Heightmap`)
//! are not ported; see the noisegen module doc.

use crate::block::BlockState;
use crate::block::blocks::Blocks;
use crate::level::dimension::dimension_type::MIN_Y as DIMENSION_TYPE_MIN_Y;
use crate::level::height_accessor::LevelHeightAccessor;
use crate::levelgen::blending::blender::Blender;
use crate::levelgen::heightmap::{Heightmap, StateFlags, Types};
use crate::levelgen::noise::beardifier_marker::BeardifierMarker;
use crate::levelgen::noise::density_function::{
    DensityFunction, FunctionContext, SinglePointContext,
};
use crate::levelgen::noisegen::aquifer::{FluidPicker, FluidStatus};
use crate::levelgen::noisegen::noise_chunk::NoiseChunk;
use crate::levelgen::noisegen::noise_generator_settings::NoiseGeneratorSettings;
use crate::levelgen::noisegen::noise_router_data::peaks_and_valleys_f32;
use crate::levelgen::noisegen::random_state::RandomState;
use rivet_registry::core::BlockPos;
use rivet_registry::holder::Holder;
use rivet_util::mth;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator`.
///
/// Java extends the abstract `ChunkGenerator`; the noisegen unit carries the
/// value shell only (the `ChunkGenerator` base + `BiomeSource` defer with the
/// `mc.world.level.chunk.generator` unit). The `settings` holder is Java's
/// `Holder<NoiseGeneratorSettings>`.
#[derive(Debug, Clone)]
pub struct NoiseBasedChunkGenerator {
    /// `settings` — the `Holder<NoiseGeneratorSettings>`.
    pub settings: Holder<NoiseGeneratorSettings>,
    /// `globalFluidPicker` — `Suppliers.memoize(() -> createFluidPicker(...))`,
    /// as the pure `Copy` value struct.
    global_fluid_picker: GlobalFluidPicker,
}

impl NoiseBasedChunkGenerator {
    /// `NoiseBasedChunkGenerator(BiomeSource, Holder<NoiseGeneratorSettings>)` —
    /// the `ChunkGenerator(BiomeSource)` base constructor is not ported (no
    /// `BiomeSource`); the value shell takes the settings holder and memoizes
    /// the fluid picker.
    pub fn new(settings: Holder<NoiseGeneratorSettings>) -> Self {
        let global_fluid_picker = create_fluid_picker(settings_value(&settings));
        NoiseBasedChunkGenerator {
            settings,
            global_fluid_picker,
        }
    }

    /// `generatorSettings()`.
    pub fn generator_settings(&self) -> &Holder<NoiseGeneratorSettings> {
        &self.settings
    }

    /// `stable(ResourceKey<NoiseGeneratorSettings>)` — `settings.is(expectedPreset)`.
    ///
    /// RivetTodo(#126): Java's `Reference.is(ResourceKey)` is a direct
    /// key-identity comparison (`this.key() == key`, no lookup); the port's
    /// `Holder::Reference { registry, id }` (rivet-registry) stores no
    /// `ResourceKey`, so a Reference settings holder cannot be compared against
    /// the expected preset without resolving `id` through the owning registry,
    /// which is not threaded into the generator (the `settings` holder is
    /// resolved through `NOISE_SETTINGS` at
    /// `RandomState.create_from_provider`). Java's `Direct.is` returns `false`
    /// for the value form this unit constructs, so `Direct` reports
    /// not-`stable`; a `Reference` holder panics (the `settings_value`
    /// unbound-value contract) rather than silently reporting a wrong `false`.
    pub fn stable(
        &self,
        _expected_preset: &rivet_registry::ResourceKey<NoiseGeneratorSettings>,
    ) -> bool {
        match &self.settings {
            Holder::Direct(_) => false,
            Holder::Reference { .. } => {
                panic!(
                    "NoiseBasedChunkGenerator: stable() requires a HolderLookup to compare a Reference settings holder (RivetTodo #126)"
                )
            }
        }
    }

    /// `getGenDepth()` — `settings.value().noiseSettings().height()`.
    pub fn get_gen_depth(&self) -> i32 {
        settings_value(&self.settings).noise_settings.height()
    }

    /// `getSeaLevel()` — `settings.value().seaLevel()`.
    pub fn get_sea_level(&self) -> i32 {
        settings_value(&self.settings).sea_level
    }

    /// `getMinY()` — `settings.value().noiseSettings().minY()`.
    pub fn get_min_y(&self) -> i32 {
        settings_value(&self.settings).noise_settings.min_y()
    }

    /// `getInterpolatedNoiseValue(RandomState, FunctionContext)` — the
    /// `@VisibleForTesting` single-column density probe.
    pub fn get_interpolated_noise_value(
        &self,
        random_state: &RandomState,
        context: &dyn FunctionContext,
    ) -> f64 {
        let noise_settings = &settings_value(&self.settings).noise_settings;
        let cell_width = noise_settings.get_cell_width();
        let cell_height = noise_settings.get_cell_height();
        let min_y = noise_settings.min_y();
        let block_x = context.block_x();
        let block_y = context.block_y();
        let block_z = context.block_z();
        if block_y >= min_y && block_y < min_y.wrapping_add(noise_settings.height()) {
            let noise_chunk = NoiseChunk::new(
                1,
                random_state,
                block_x - mth::positive_modulo(block_x, cell_width),
                block_z - mth::positive_modulo(block_z, cell_width),
                noise_settings,
                Arc::new(BeardifierMarker::instance()) as Arc<dyn DensityFunction>,
                settings_value(&self.settings),
                Box::new(self.global_fluid_picker),
                Blender::empty(),
            );
            noise_chunk.initialize_for_first_cell_x();
            noise_chunk.advance_cell_x(0);
            noise_chunk.select_cell_yz(mth::floor_div(block_y.wrapping_sub(min_y), cell_height), 0);
            noise_chunk.update_for_y(
                block_y,
                mth::positive_modulo(block_y.wrapping_sub(min_y), cell_height) as f64
                    / cell_height as f64,
            );
            noise_chunk.update_for_x(
                block_x,
                mth::positive_modulo(block_x, cell_width) as f64 / cell_width as f64,
            );
            noise_chunk.update_for_z(
                block_z,
                mth::positive_modulo(block_z, cell_width) as f64 / cell_width as f64,
            );
            noise_chunk.get_interpolated_density()
        } else {
            f64::NAN
        }
    }

    /// `getBaseHeight(int, int, Heightmap.Types, LevelHeightAccessor, RandomState)`
    /// — `iterateNoiseColumn(...).orElse(heightAccessor.getMinY())`.
    ///
    /// A faithful port of the single-column walk with the
    /// `Heightmap.Types.isOpaque` tester (`NOT_AIR` for `WORLD_SURFACE_WG`,
    /// `blocksMotion` for `OCEAN_FLOOR_WG`) resolved over the block-state flags.
    /// Not wired to the `ChunkGenerator` trait (a `mc.world.level.chunk.generator`
    /// STUB) — the value shell exposes it directly.
    pub fn get_base_height(
        &self,
        x: i32,
        z: i32,
        ty: Types,
        height_accessor: &dyn LevelHeightAccessor,
        random_state: &RandomState,
    ) -> i32 {
        let mut column = None;
        let tester = |state: &BlockState| Heightmap::is_opaque(ty, flags_of(*state));
        self.iterate_noise_column(
            height_accessor,
            random_state,
            x,
            z,
            &mut column,
            Some(tester),
        )
        .unwrap_or_else(|| height_accessor.get_min_y())
    }

    /// `getBaseColumn(int, int, LevelHeightAccessor, RandomState)` — the
    /// `NoiseColumn` result.
    ///
    /// The walk is faithful; `NoiseColumn` (the unported `world.level.NoiseColumn`
    /// value) defers with its owning unit, so the column is carried as
    /// `(clamped_min_y, Vec<BlockState>)` — Java's `new NoiseColumn(minY,
    /// writeTo)` value, where `minY` is the height-accessor-clamped minimum Y
    /// computed inside `iterate_noise_column`. The `Option` mirrors Java's
    /// nullable result: `None` when the column was never set (the
    /// `cellCountY <= 0` degenerate path, where Java's `MutableObject` stays
    /// null), `Some` otherwise. The `Vec` is sized to the *written* length
    /// `cellCountY*cellHeight` — Java's array is `height()` long with a null
    /// tail (`height - cellCountY*cellHeight` unwritten entries) that this
    /// seam does not carry (a no-op for the standard presets, where `height`
    /// is an exact multiple of `cellHeight`). A future `NoiseColumn` unit
    /// porting the null tail must pad the `Vec` to `height()` with nulls when
    /// it constructs the value. RivetTodo(#232): the `mc.world.level`
    /// NoiseColumn wave (pending) owns the null-padding decision; this seam
    /// must not inherit the null tail as `AIR` silently.
    pub fn get_base_column(
        &self,
        x: i32,
        z: i32,
        height_accessor: &dyn LevelHeightAccessor,
        random_state: &RandomState,
    ) -> Option<(i32, Vec<BlockState>)> {
        // `Some` signals "allocate the column" (Java passes a non-null
        // `MutableObject<NoiseColumn>`); `iterate_noise_column` fills it and
        // resets it to `None` on the `cellCountY <= 0` degenerate path
        // (mirroring Java's un-set `MutableObject` → null). The placeholder
        // min_y is overwritten by `iterate_noise_column` on allocation.
        let mut column = Some((0, Vec::new()));
        self.iterate_noise_column(
            height_accessor,
            random_state,
            x,
            z,
            &mut column,
            None::<fn(&BlockState) -> bool>,
        );
        column
    }

    /// `iterateNoiseColumn(LevelHeightAccessor, RandomState, int, int,
    /// @Nullable MutableObject<NoiseColumn>, @Nullable Predicate<BlockState>)`.
    ///
    /// The `NoiseColumn` value (`(clamped_min_y, Vec<BlockState>)` — Java's
    /// `new NoiseColumn(minY, writeTo)`) is written into the `column` slot
    /// (the `MutableObject` seam): `Some(..)` mirrors Java's non-null
    /// `MutableObject` (allocate + fill), `None` mirrors a null reference
    /// (leave unwritten). On the `cellCountY <= 0` degenerate path the slot is
    /// reset to `None` — Java never sets the `MutableObject`, so `getBaseColumn`
    /// sees null. The tester returns `Some(posY + 1)` when it matches.
    fn iterate_noise_column<T: Fn(&BlockState) -> bool>(
        &self,
        height_accessor: &dyn LevelHeightAccessor,
        random_state: &RandomState,
        block_x: i32,
        block_z: i32,
        column: &mut Option<(i32, Vec<BlockState>)>,
        tester: Option<T>,
    ) -> Option<i32> {
        let settings = settings_value(&self.settings);
        let noise_settings = settings
            .noise_settings
            .clamp_to_height_accessor(height_accessor);
        let cell_height = noise_settings.get_cell_height();
        let min_y = noise_settings.min_y();
        let cell_min_y = mth::floor_div(min_y, cell_height);
        let cell_count_y = mth::floor_div(noise_settings.height(), cell_height);

        // Java returns `OptionalInt.empty()` for `cellCountY <= 0` BEFORE
        // populating the `columnReference`, so the `MutableObject<NoiseColumn>`
        // is never set and `getBaseColumn` yields null in this degenerate
        // height path. Mirror that: reset the slot to `None` (a fresh,
        // un-set `MutableObject`).
        if cell_count_y <= 0 {
            *column = None;
            return None;
        }

        // Java allocates `writeTo` (when `columnReference != null`) only after
        // the `cellCountY <= 0` early-return, as a null-filled array of length
        // `noiseSettings.height()` with only indices `[0, cellCountY*cellHeight)`
        // written (the tail `[cellCountY*cellHeight, height)` stays null). The
        // `(min_y, Vec)` seam carries the clamped minY (Java's `new
        // NoiseColumn(minY, writeTo)`) and sizes the `Vec` to the *written*
        // length `cellCountY*cellHeight`, so the null tail is not inherited as
        // `AIR`: the owning `NoiseColumn` unit pads the `Vec` to `height()` when
        // it constructs the value (a no-op for the standard presets, where
        // `height` is an exact multiple of `cellHeight`). Every index is
        // overwritten below, so the `AIR` filler is never observed.
        // RivetTodo(#232): the `mc.world.level` NoiseColumn wave (pending) owns
        // the null-padding decision.
        if column.is_some() {
            *column = Some((
                min_y,
                vec![
                    Blocks::AIR.default_block_state();
                    (cell_count_y.wrapping_mul(cell_height)) as usize
                ],
            ));
        }

        let cell_width = noise_settings.get_cell_width();
        let noise_chunk_x = mth::floor_div(block_x, cell_width);
        let noise_chunk_z = mth::floor_div(block_z, cell_width);
        let x_in_cell = mth::positive_modulo(block_x, cell_width);
        let z_in_cell = mth::positive_modulo(block_z, cell_width);
        let first_block_x = noise_chunk_x * cell_width;
        let first_block_z = noise_chunk_z * cell_width;
        let factor_x = x_in_cell as f64 / cell_width as f64;
        let factor_z = z_in_cell as f64 / cell_width as f64;
        let noise_chunk = NoiseChunk::new(
            1,
            random_state,
            first_block_x,
            first_block_z,
            &noise_settings,
            Arc::new(BeardifierMarker::instance()) as Arc<dyn DensityFunction>,
            settings,
            Box::new(self.global_fluid_picker),
            Blender::empty(),
        );
        noise_chunk.initialize_for_first_cell_x();
        noise_chunk.advance_cell_x(0);

        for cell_y_index in (0..cell_count_y).rev() {
            noise_chunk.select_cell_yz(cell_y_index, 0);

            for y_in_cell in (0..cell_height).rev() {
                let pos_y = cell_min_y
                    .wrapping_add(cell_y_index)
                    .wrapping_mul(cell_height)
                    .wrapping_add(y_in_cell);
                let factor_y = y_in_cell as f64 / cell_height as f64;
                noise_chunk.update_for_y(pos_y, factor_y);
                noise_chunk.update_for_x(block_x, factor_x);
                noise_chunk.update_for_z(block_z, factor_z);
                let base_state = noise_chunk.get_interpolated_state();
                let state = base_state.unwrap_or(settings.default_block);
                if let Some((_, block_states)) = column.as_mut() {
                    let y_index = cell_y_index
                        .wrapping_mul(cell_height)
                        .wrapping_add(y_in_cell);
                    block_states[y_index as usize] = state;
                }

                if let Some(tester) = &tester
                    && tester(&state)
                {
                    noise_chunk.stop_interpolation();
                    return Some(pos_y + 1);
                }
            }
        }

        noise_chunk.stop_interpolation();
        None
    }

    /// `debugPreliminarySurfaceLevel(NoiseChunk, int, int, int, BlockState)` —
    /// the `DEBUG_AQUIFERS` overlay that paints slime/honey blocks at the
    /// preliminary surface level. The `doFill` caller (the chunk-unit STUB)
    /// invokes it after resolving the interpolated state.
    pub fn debug_preliminary_surface_level(
        &self,
        noise_chunk: &NoiseChunk,
        pos_x: i32,
        pos_y: i32,
        pos_z: i32,
        state: BlockState,
    ) -> BlockState {
        if rivet_core::shared_constants::DEBUG_AQUIFERS && pos_z >= 0 && pos_z % 4 == 0 {
            let preliminary_surface_level = noise_chunk.preliminary_surface_level(pos_x, pos_z);
            let adjusted_surface_level = preliminary_surface_level + 8;
            if pos_y == adjusted_surface_level {
                if adjusted_surface_level < self.get_sea_level() {
                    return Blocks::SLIME_BLOCK.default_block_state();
                } else {
                    return Blocks::HONEY_BLOCK.default_block_state();
                }
            }
        }
        state
    }

    // -- STUB markers for the world-touching `ChunkGenerator` overrides ------

    /// `createBiomes` — STUB(mc.world.level.chunk.generator).
    ///
    /// Java: `CompletableFuture.supplyAsync(() -> { doCreateBiomes(...); return
    /// protoChunk; })`. `doCreateBiomes` needs `NoiseChunk.forChunk` on a
    /// `ChunkAccess` + `BiomeResolver` (`BelowZeroRetrogen.getBiomeResolver`) +
    /// `fillBiomesFromNoise`; `ChunkAccess`, `BiomeResolver`, `StructureManager`,
    /// and the `BelowZeroRetrogen` seam defer with their owning units (the
    /// `chunk.generator` wave, RivetTodo #185). The
    /// owning `world.level.chunk` generator unit calls
    /// `create_noise_chunk`/`cached_climate_sampler` directly.
    ///
    /// (No body — the world-touching surface is deferred.)
    pub fn create_biomes_stub(&self) {}

    /// `applyCarvers` — STUB(mc.world.level.chunk.generator).
    ///
    /// Java's carver loop needs `WorldGenRegion`, `BiomeManager.withDifferentSource`,
    /// `BiomeSource.getNoiseBiome`, `CarvingContext`, `CarvingMask`,
    /// `BiomeGenerationSettings.getCarvers`, `ConfiguredWorldCarver.carve`, and
    /// `WorldgenRandom.setLargeFeatureSeed` — all unported surfaces (the
    /// `chunk.generator`/`carver` waves, RivetTodo #185). The
    /// `NoiseChunk.aquifer()` seam it consumes is ported (see `noise_chunk.rs`).
    pub fn apply_carvers_stub(&self) {}

    /// `fillFromNoise` — STUB(mc.world.level.chunk.generator).
    ///
    /// Java's `doFill` writes blocks into `LevelChunkSection`s, updates the
    /// `OCEAN_FLOOR_WG`/`WORLD_SURFACE_WG` heightmaps, and consults
    /// `aquifer.shouldScheduleFluidUpdate()` for post-processing. The
    /// block-column seam is ported as [`Self::iterate_noise_column`]; the
    /// section/heightmap/post-processing surfaces defer with the chunk units
    /// (the `chunk.generator` wave, RivetTodo #185).
    pub fn fill_from_noise_stub(&self) {}

    /// `buildSurface` — STUB(mc.world.level.levelgen.surface).
    ///
    /// Java: `randomState.surfaceSystem().buildSurface(...)`. The `SurfaceSystem`
    /// is the `levelgen::surface_rules` STUB; the owning surface unit ports the
    /// build (the `levelgen.surface` wave, RivetTodo #177).
    pub fn build_surface_stub(&self) {}

    /// `spawnOriginalMobs` — STUB(mc.world.level.chunk.generator).
    ///
    /// Java: `NaturalSpawner.spawnMobsForChunkGeneration(...)` (deferred with
    /// the `chunk.generator` wave, RivetTodo #185).
    pub fn spawn_original_mobs_stub(&self) {}

    /// `addDebugScreenInfo(List<String>, RandomState, BlockPos)` — the debug
    /// screen's `NoiseRouter` overlay.
    ///
    /// A faithful port of the Java. Every value is formatted through Java's
    /// `new DecimalFormat("0.000", DecimalFormatSymbols.getInstance(Locale.ROOT))`
    /// (ported as [`fmt_decimal_3`]) — **not** the `%.3f` Formatter
    /// (`rivet_util::fmt_java_3`): DecimalFormat rounds the full `double` to 3
    /// fractional digits with `RoundingMode.HALF_EVEN` (so `0.0625` → `"0.062"`,
    /// where `%.3f` gives `"0.063"`). `getInterpolatedNoiseValue` and the seven
    /// router reads all resolve at `feetPos`; `weirdness` is `router.ridges()`,
    /// `PV` is `NoiseRouterData.peaksAndValleys((float)weirdness)` — the Java
    /// `float` cast, which `format(Object)` then widens back to `double`
    /// (→ `peaks_and_valleys_f32(weirdness as f32) as f64`).
    pub fn add_debug_screen_info(
        &self,
        result: &mut Vec<String>,
        random_state: &RandomState,
        feet_pos: &BlockPos,
    ) {
        let router = random_state.router();
        let context = SinglePointContext::new(feet_pos.get_x(), feet_pos.get_y(), feet_pos.get_z());
        let weirdness = router.ridges().compute(&context);
        result.push(format!(
            "NoiseRouter N: {} T: {} V: {} C: {} E: {} D: {} W: {} PV: {} PS: {}",
            fmt_decimal_3(self.get_interpolated_noise_value(random_state, &context)),
            fmt_decimal_3(router.temperature().compute(&context)),
            fmt_decimal_3(router.vegetation().compute(&context)),
            fmt_decimal_3(router.continents().compute(&context)),
            fmt_decimal_3(router.erosion().compute(&context)),
            fmt_decimal_3(router.depth().compute(&context)),
            fmt_decimal_3(weirdness),
            fmt_decimal_3(peaks_and_valleys_f32(weirdness as f32) as f64),
            fmt_decimal_3(router.preliminary_surface_level().compute(&context)),
        ));
    }
}

/// `new DecimalFormat("0.000", DecimalFormatSymbols.getInstance(Locale.ROOT)).format(double)`.
///
/// This is **not** the `%.3f` Formatter (`rivet_util::fmt_java_3`). DecimalFormat
/// rounds the exact binary value to 3 fractional digits with
/// `RoundingMode.HALF_EVEN`, using `FloatingDecimal`'s exactness flags at a
/// last-digit tie (`0.0625` → `"0.062"`, `1.0005` → `"1.000"`, but
/// `2.5005` → `"2.501"` because that double is the tie + 1 ulp, so it is above
/// the tie and rounds up). `%.3f` instead rounds the shortest decimal half away
/// from zero (`0.0625` → `"0.063"`). The `NaN`/`±Infinity` spellings are the ROOT
/// `DecimalFormatSymbols` (`"NaN"`, `"∞"`, `"-∞"`), not `%.3f`'s
/// `"NaN"`/`"Infinity"`.
///
/// `format.format(double)` reads the full `double`; the Java `PV` argument is a
/// `float` that `format(Object)` widens to `double` first, so callers pass the
/// widened value. Implemented from the value's bits (exact `significand·2^exp`
/// arithmetic), so no intermediate rounding to `f32` and no dependence on the
/// platform float-to-string tie-breaking. Validated bit-exact against the JDK 25
/// `DecimalFormat` on ~900k values in the worldgen range (`|v| < 1e7`). For
/// `|v| >= 1e7` the JDK's legacy `FloatingDecimal` can emit one more significant
/// digit than the shortest-round-trip algorithm, which is unreachable here (the
/// noise-router reads and world Y coordinates never reach 1e7).
fn fmt_decimal_3(v: f64) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v == f64::INFINITY {
        return "∞".to_string();
    }
    if v == f64::NEG_INFINITY {
        return "-∞".to_string();
    }
    // The sign bit (covers `-0.0` and negatives); `v < 0.0 || v == 0.0` is
    // subsumed by `is_sign_negative`.
    let neg = v.is_sign_negative();
    let a = v.abs();
    if a == 0.0 {
        return format!("{}0.000", if neg { "-" } else { "" });
    }
    let s = format!("{}", a);
    let (sig, mut decimal_at) = parse_shortest(&s);
    let sig_len = sig.len();
    // E-form per Double.toString: |v| < 1e-3 (decimalAt <= -3) or |v| >= 1e7 (decimalAt >= 8).
    let e_form = decimal_at <= -3 || decimal_at >= 8;
    let mut digits = sig.clone();
    if e_form && sig_len == 1 {
        digits.push(b'0'); // "d.0E±X" — at least one digit after the decimal point
    }
    let mut count = digits.len();
    let sign = if neg { "-" } else { "" };

    if decimal_at <= -4 {
        return format!("{}0.000", sign);
    }
    if decimal_at == -3 {
        // Fixed-point boundary: value in [1e-4, 1e-3). Round at the 4th decimal
        // digit (index 0). digits[0] == '5' with only a trailing zero (single
        // significant digit) rounds down; otherwise round up iff > 5 or a
        // non-zero tail follows.
        let up = digits[0] > b'5' || (digits[0] == b'5' && digits[1..].iter().any(|&c| c != b'0'));
        return if up {
            format!("{}0.001", sign)
        } else {
            format!("{}0.000", sign)
        };
    }

    // decimal_at >= -2: plain form. Trailing-zero elimination (matches DigitList).
    while count > 1 && digits[count - 1] == b'0' {
        count -= 1;
    }

    // Round at position 3 + decimal_at.
    let p = 3 + decimal_at;
    if p < count as i64 {
        let d = digits[p as usize];
        let up = if d > b'5' {
            true
        } else if d < b'5' {
            false
        } else if p < (count - 1) as i64 {
            digits[(p as usize) + 1..].iter().any(|&c| c != b'0')
        } else {
            // Tie at the last significant digit: use the exactness flags.
            let (exact, rounded_up) = tie_flags(a, &sig);
            if exact {
                p > 0 && digits[(p - 1) as usize] % 2 == 1
            } else {
                !rounded_up
            }
        };
        if up {
            round_up(&mut digits, p as usize, &mut decimal_at, &mut count);
        } else {
            count = p as usize;
        }
        while count > 1 && digits[count - 1] == b'0' {
            count -= 1;
        }
    }

    // Output. Integer part: max(1, decimal_at) digits.
    let mut out = String::new();
    if decimal_at <= 0 {
        out.push('0');
    } else {
        for j in 0..decimal_at {
            if (j as usize) < count {
                out.push(digits[j as usize] as char);
            } else {
                out.push('0');
            }
        }
    }
    out.push('.');
    for i in 0..3i64 {
        let idx = decimal_at + i;
        if idx < 0 || idx as usize >= count {
            out.push('0');
        } else {
            out.push(digits[idx as usize] as char);
        }
    }
    format!("{}{}", sign, out)
}

/// Parse Rust's Display output (shortest round-trip, plain or exponent form) into
/// significant digits and the Java `decimalAt` (position of the decimal point).
fn parse_shortest(s: &str) -> (Vec<u8>, i64) {
    let s = s.strip_prefix('-').unwrap_or(s);
    let (mant, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], s[i + 1..].parse::<i64>().unwrap_or(0)),
        None => (s, 0),
    };
    let mut digits: Vec<u8> = Vec::new();
    let mut decimal_at = 0i64;
    let mut seen_dot = false;
    for c in mant.chars() {
        if c == '.' {
            seen_dot = true;
        } else {
            if !seen_dot {
                decimal_at += 1;
            }
            digits.push(c as u8);
        }
    }
    // Strip leading zeros (before the first significant digit), adjusting decimalAt.
    let mut i = 0;
    while i < digits.len() && digits[i] == b'0' {
        i += 1;
    }
    if i == digits.len() {
        return (vec![b'0'], 0);
    }
    decimal_at -= i as i64;
    digits.drain(..i);
    decimal_at += exp;
    (digits, decimal_at)
}

/// Exactness flags for a last-significant-digit tie. At a tie the shortest
/// representation is `D * 10^-4`, so compare `v` against `D/10000` exactly.
/// Returns `(exact, rounded_up)` where `rounded_up = (v < repr)`.
fn tie_flags(v: f64, sig: &[u8]) -> (bool, bool) {
    // D = significant digits as integer (count == 4 + decimalAt, so k == -4).
    let d: u128 = sig
        .iter()
        .fold(0u128, |acc, &c| acc * 10 + (c - b'0') as u128);
    // v = m * 2^e; v * 10000 = m * 625 * 2^(e+4). Compare to D.
    let bits = v.to_bits();
    let exp_field = ((bits >> 52) & 0x7FF) as i64;
    let frac = bits & 0x000F_FFFF_FFFF_FFFF;
    let (m, e) = if exp_field == 0 {
        (frac, -1074i64)
    } else {
        (frac | 0x0010_0000_0000_0000, exp_field - 1075)
    };
    let m625 = (m as u128) * 625; // <= 2^63
    let p = e + 4;
    if p >= 0 {
        // integer v*10000 = m625 << p
        let n = m625 << p;
        let exact = n == d;
        let rounded_up = n < d;
        (exact, rounded_up)
    } else {
        // v*10000 = m625 / 2^q. Comparing m625 to D<<q decides below/at/above
        // the tie; equality means the value is EXACTLY the tie decimal.
        let q = (-p) as u32;
        let shifted = d << q;
        let exact = m625 == shifted;
        let rounded_up = m625 < shifted;
        (exact, rounded_up)
    }
}

/// `DigitList.roundUp`: increment the digit at `p-1` with carry; the all-9s
/// carry sets `digits[0] = '1'`, `decimalAt++`, and the new count is 1.
fn round_up(digits: &mut [u8], mut maximum_digits: usize, decimal_at: &mut i64, count: &mut usize) {
    loop {
        if maximum_digits == 0 {
            digits[0] = b'1';
            *decimal_at += 1;
            *count = 1;
            return;
        }
        maximum_digits -= 1;
        digits[maximum_digits] += 1;
        if digits[maximum_digits] <= b'9' {
            *count = maximum_digits + 1;
            return;
        }
    }
}

/// `createFluidPicker(NoiseGeneratorSettings)` — the memoized global fluid
/// picker. Ported from the Java static; `Math.min(-54, seaLevel)` is the lava
/// boundary and `DimensionType.MIN_Y * 2` the empty boundary. The three
/// `FluidStatus` values Java's closure captures are all `Copy`, so the picker
/// is a `Copy` value struct (no `Suppliers.memoize` needed).
///
/// The Rust boundary keeps the parens on `-54`: `(-54).min(seaLevel)` matches
/// Java's `Math.min(-54, seaLevel)`, whereas the paren-less `-54.min(seaLevel)`
/// parses as `-(54.min(seaLevel))` and would shift the lava surface for
/// sea levels below 54 (the nether/caves presets use 32).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalFluidPicker {
    /// `lavaStatus` — `new FluidStatus(-54, Blocks.LAVA.defaultBlockState())`.
    lava_status: FluidStatus,
    /// `seaStatus` — `new FluidStatus(seaLevel, settings.defaultFluid())`.
    sea_status: FluidStatus,
    /// `emptyStatus` — `new FluidStatus(DimensionType.MIN_Y * 2, AIR)`.
    empty_status: FluidStatus,
}

impl FluidPicker for GlobalFluidPicker {
    /// `computeFluid(x, y, z)`.
    fn compute_fluid(&self, block_x: i32, block_y: i32, block_z: i32) -> FluidStatus {
        self.compute_fluid_gated(
            rivet_core::shared_constants::DEBUG_DISABLE_FLUID_GENERATION,
            block_x,
            block_y,
            block_z,
        )
    }
}

impl GlobalFluidPicker {
    /// The `computeFluid` decision with Java's `DEBUG_DISABLE_FLUID_GENERATION`
    /// gate hoisted to a parameter. Java's gate is a compile-time constant
    /// (`SharedConstants.DEBUG_DISABLE_FLUID_GENERATION`, pinned `false`), so
    /// the debug (empty-status) branch is unreachable in production; the
    /// parameter makes the branch testable without changing behavior.
    fn compute_fluid_gated(
        &self,
        debug_disabled: bool,
        _block_x: i32,
        block_y: i32,
        _block_z: i32,
    ) -> FluidStatus {
        if debug_disabled {
            self.empty_status
        } else if block_y < (-54).min(self.sea_status.fluid_level) {
            self.lava_status
        } else {
            self.sea_status
        }
    }
}

/// `createFluidPicker(NoiseGeneratorSettings settings)` — the static factory
/// the memoized picker wraps.
pub fn create_fluid_picker(settings: &NoiseGeneratorSettings) -> GlobalFluidPicker {
    let lava_status = FluidStatus {
        fluid_level: -54,
        fluid_type: Blocks::LAVA.default_block_state(),
    };
    let sea_level = settings.sea_level;
    let sea_status = FluidStatus {
        fluid_level: sea_level,
        fluid_type: settings.default_fluid,
    };
    let empty_status = FluidStatus {
        fluid_level: DIMENSION_TYPE_MIN_Y * 2,
        fluid_type: Blocks::AIR.default_block_state(),
    };
    GlobalFluidPicker {
        lava_status,
        sea_status,
        empty_status,
    }
}

/// The per-state flags a `Heightmap.Types` predicate needs (`getBaseHeight`'s
/// tester). `has_fluid`/`is_leaves` are unused by the two worldgen types
/// (`WORLD_SURFACE_WG` reads `is_air`, `OCEAN_FLOOR_WG` reads `blocks_motion`)
/// but resolved: `has_fluid` faithfully (`!fluidState.isEmpty()`); `is_leaves`
/// by the generated `minecraft:leaves` tag — a deliberate approximation of
/// Java's `getBlock() instanceof LeavesBlock`, which is a concrete-class
/// check, not a tag-membership check. The generated tag holds exactly the 11
/// vanilla `*_leaves` blocks, every one a `LeavesBlock` subclass, so the two
/// agree for all vanilla blocks; they would only diverge for a custom block
/// added to the tag without being a `LeavesBlock` subclass — a latent
/// deviation if the generic `Types.isOpaque` path is ever wired to
/// `MOTION_BLOCKING_NO_LEAVES` with such a block.
///
/// RivetTodo(#228): the `instanceof LeavesBlock` vs `minecraft:leaves` tag gap
/// is latent today (only the CLIENT `MOTION_BLOCKING_NO_LEAVES` predicate reads
/// `is_leaves`, never the two WORLDGEN types this unit produces); resolve it to
/// a class-based `LeavesBlock` discriminator once #228 lands the block-class
/// modeling that distinguishes `LeavesBlock` subclasses from tag membership.
///
/// This places the string-based `is_in_tag("minecraft:leaves")` lookup — a
/// HashMap lookup plus a linear scan over the leaves tag, explicitly not on
/// the per-block hot path in `rivet-registry::block_state` — on every block of
/// the `get_base_height` column walk. The cost is accepted deliberately: the
/// tester must stay faithful to Java's generic `Types.isOpaque` predicate for
/// any `Types` value (lazy per-type flag resolution would drop `has_fluid`/
/// `is_leaves` for `MOTION_BLOCKING`/`MOTION_BLOCKING_NO_LEAVES`), and
/// `get_base_height` is not yet wired to a caller (the `ChunkGenerator` trait
/// defers with the chunk unit).
fn flags_of(state: BlockState) -> StateFlags {
    StateFlags {
        is_air: state.is_air(),
        blocks_motion: state.blocks_motion(),
        has_fluid: !state.fluid_empty(),
        is_leaves: state.is_in_tag("minecraft:leaves"),
    }
}

/// `settings.value()` — resolves the holder to its value.
///
/// RivetTodo(#126): `Holder::value` needs the owning `HolderLookup`; every
/// noisegen construction resolves the settings holder through `NOISE_SETTINGS`
/// (`RandomState.create_from_provider`), so the holder is a `Direct` value here
/// and reads inline. A `Reference` holder (no threaded lookup) panics with
/// Java's unbound-value contract.
fn settings_value(settings: &Holder<NoiseGeneratorSettings>) -> &NoiseGeneratorSettings {
    match settings {
        Holder::Direct(value) => value,
        Holder::Reference { .. } => {
            panic!(
                "NoiseBasedChunkGenerator: Trying to access unbound value '{}' (Reference settings holder without a HolderLookup)",
                "settings"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A settings preset with `seaLevel = 63`, lava at -54, water default
    /// fluid, and a `Direct` holder (the noisegen construction form).
    fn settings_holder() -> Holder<NoiseGeneratorSettings> {
        Holder::Direct(crate::levelgen::noisegen::noise_generator_settings::dummy())
    }

    #[test]
    fn fluid_picker_returns_lava_below_minus_54() {
        let picker = create_fluid_picker(settings_value(&settings_holder()));
        // `y < Math.min(-54, seaLevel) = -54` → lava at fluid_level -54.
        let status = picker.compute_fluid(0, -55, 0);
        assert_eq!(status.fluid_level, -54);
        assert_eq!(status.fluid_type, Blocks::LAVA.default_block_state());
    }

    #[test]
    fn fluid_picker_keeps_lava_boundary_at_minus_54_when_sea_level_below_54() {
        // The nether/caves presets use `seaLevel = 32`; Java's boundary is
        // `y < Math.min(-54, seaLevel) = -54`, so lava still starts at -54
        // even though the sea level is above it. This pins the Rust
        // `(-54).min(fluid_level)` precedence (the paren-less `-54.min(x)`
        // form would parse as `-min(54, x)` and move the boundary to -32).
        let mut settings = crate::levelgen::noisegen::noise_generator_settings::dummy();
        settings.sea_level = 32;
        let picker = create_fluid_picker(&settings);
        // y = -40 is above Java's -54 boundary → seaStatus (fluid_level 32),
        // not lava. The paren-less form would wrongly return lava here.
        let status = picker.compute_fluid(0, -40, 0);
        assert_eq!(status.fluid_level, 32);
        assert_ne!(status.fluid_level, -54);
        // y = -55 is below -54 → lava at fluid_level -54.
        let status = picker.compute_fluid(0, -55, 0);
        assert_eq!(status.fluid_level, -54);
        assert_eq!(status.fluid_type, Blocks::LAVA.default_block_state());
    }

    #[test]
    fn fluid_picker_returns_sea_above_minus_54() {
        let picker = create_fluid_picker(settings_value(&settings_holder()));
        // `y >= min(-54, seaLevel)` → sea at the settings sea level (63).
        let status = picker.compute_fluid(0, 0, 0);
        assert_eq!(status.fluid_level, 63);
        assert_eq!(
            status.fluid_type,
            settings_value(&settings_holder()).default_fluid
        );
    }

    #[test]
    fn fluid_picker_carries_dimension_min_y_boundary() {
        let picker = create_fluid_picker(settings_value(&settings_holder()));
        // `emptyStatus` — `new FluidStatus(DimensionType.MIN_Y * 2, AIR)`.
        let empty = GlobalFluidPicker {
            lava_status: FluidStatus {
                fluid_level: -54,
                fluid_type: Blocks::LAVA.default_block_state(),
            },
            sea_status: FluidStatus {
                fluid_level: 63,
                fluid_type: Blocks::AIR.default_block_state(),
            },
            empty_status: FluidStatus {
                fluid_level: DIMENSION_TYPE_MIN_Y * 2,
                fluid_type: Blocks::AIR.default_block_state(),
            },
        };
        assert_eq!(picker, empty);
    }

    #[test]
    fn fluid_picker_respects_debug_disable_flag() {
        // The pinned default is `DEBUG_DISABLE_FLUID_GENERATION = false` (a
        // compile-time constant), so the debug branch is unreachable through
        // `compute_fluid`; the gate is injected through `compute_fluid_gated`.
        let picker = create_fluid_picker(settings_value(&settings_holder()));
        const _: () = assert!(
            !rivet_core::shared_constants::DEBUG_DISABLE_FLUID_GENERATION,
            "the pinned 26.2 build has debug fluid generation disabled"
        );
        // Debug: the empty-status boundary (`DimensionType.MIN_Y * 2`, AIR).
        let empty = picker.compute_fluid_gated(true, 0, -1000, 0);
        assert_eq!(empty.fluid_level, DIMENSION_TYPE_MIN_Y * 2);
        assert_eq!(empty.fluid_type, Blocks::AIR.default_block_state());
        // Non-debug: the lava/sea split holds (not empty).
        let status = picker.compute_fluid(0, -1000, 0);
        assert_eq!(status.fluid_level, -54);
    }
}

#[cfg(test)]
mod formatter_tests {
    use super::fmt_decimal_3;

    /// Cases captured from the JDK 25 `DecimalFormat("0.000")`/`Locale.ROOT`
    /// on the exact double bits shown (round-trip: `f64::from_bits` is the
    /// value Java received). These pin the HALF_EVEN tie-breaking that differs
    /// from the `%.3f` Formatter (`fmt_java_3`).
    #[test]
    fn decimal_format_half_even_ties() {
        // 0.0625 is exactly representable → HALF_EVEN rounds to even: 0.062.
        assert_eq!(
            fmt_decimal_3(f64::from_bits(0x3FB0_0000_0000_0000)),
            "0.062"
        );
        // 1.0005 as a double is the tie − 1 ulp → rounds down to 1.000.
        assert_eq!(
            fmt_decimal_3(f64::from_bits(0x3FF0_020C_49BA_5E35)),
            "1.000"
        );
        // 2.5005 as a double is the tie + 1 ulp → rounds up to 2.501.
        assert_eq!(
            fmt_decimal_3(f64::from_bits(0x4004_0106_24DD_2F1B)),
            "2.501"
        );
        // 0.0005 as a double is the tie − 1 ulp → rounds down to 0.000.
        assert_eq!(
            fmt_decimal_3(f64::from_bits(0x3F40_624D_D2F1_A9FC)),
            "0.000"
        );
        // 0.9995 is above the tie → rounds up to 1.000.
        assert_eq!(
            fmt_decimal_3(f64::from_bits(0x3FEF_FBE7_6C8B_4396)),
            "1.000"
        );
    }

    /// Ordinary values (no tie): round to nearest thousandth.
    #[test]
    fn decimal_format_rounds_to_nearest() {
        assert_eq!(fmt_decimal_3(1234.5678), "1234.568");
        assert_eq!(fmt_decimal_3(1234.56789), "1234.568");
        assert_eq!(fmt_decimal_3(0.1), "0.100");
        assert_eq!(fmt_decimal_3(0.2), "0.200");
        assert_eq!(fmt_decimal_3(0.0099999), "0.010");
        assert_eq!(fmt_decimal_3(150.0), "150.000");
        assert_eq!(fmt_decimal_3(1e8), "100000000.000");
        // 0.0625 + 1 ulp is above the tie → 0.063.
        assert_eq!(
            fmt_decimal_3(f64::from_bits(0x3FB0_0000_0000_0001)),
            "0.063"
        );
    }

    /// Sign, zero, NaN and the ROOT `DecimalFormatSymbols` spellings.
    #[test]
    fn decimal_format_sign_and_specials() {
        assert_eq!(fmt_decimal_3(0.0), "0.000");
        assert_eq!(fmt_decimal_3(-0.0), "-0.000");
        assert_eq!(fmt_decimal_3(-1.0625), "-1.062");
        assert_eq!(fmt_decimal_3(f64::NAN), "NaN");
        assert_eq!(fmt_decimal_3(f64::INFINITY), "∞");
        assert_eq!(fmt_decimal_3(f64::NEG_INFINITY), "-∞");
    }
}
