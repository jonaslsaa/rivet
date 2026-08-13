//! Port of `net.minecraft.world.level.levelgen.blending.BlendingData` (class,
//! 26.2) — the per-old-chunk height/biome/density grid behind `Blender`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! levelgen/blending/BlendingData.java`.
//!
//! This slice ports the independently compilable **value layer** the shared
//! Blender prerequisite and the `Packed` codec need:
//!
//! - the cell-layout constants and index math (`getInsideIndex`/`getOutsideIndex`/
//!   `getX`/`getZ`/`zeroIfNegative`);
//! - the private constructor + `LevelHeightAccessor`-derived geometry
//!   (`getMinY`/`getColumnMinY`/`getCellYIndex`/`cellCountPerColumn`/
//!   `quartCountPerColumn`);
//! - the value consumers `getHeight`/`getDensity`/`iterateBiomes`/
//!   `iterateHeights`/`iterateDensities`/`getAreaWithOldGeneration`;
//! - `unpack`/`pack` and the `Packed` record + `CODEC` (round-trips against the
//!   `blending_data` compound `serializable_chunk_data` carries; both agree on
//!   `CELL_COLUMN_COUNT == 16`).
//!
//! The chunk-reading half defers as `RivetTodo(#177)` owned by the blending
//! unit (it needs the `ChunkAccess`/`WorldGenRegion` surfaces):
//!
//! - `getOrUpdateBlendingData(WorldGenRegion, ...)` + `sideByGenerationAge`
//!   (needs `WorldGenRegion.getChunk` + `ChunkAccess` status/blending-data
//!   reads);
//! - `calculateData`/`addValuesForColumn`/`getHeightAtXZ`/`read1`/`read7`/
//!   `getDensityColumn`/`getBiomeColumn`/`isGround` (block-state, heightmap,
//!   fluid and collision-shape reads) and the private constants they own
//!   (`SOLID_DENSITY`, `AIR_DENSITY`, `CELLS_PER_SECTION_Y`,
//!   `SURFACE_BLOCKS`). Until that lands, `densities`/`biomes` columns are
//!   always `None`, which the value consumers faithfully report as
//!   `NO_VALUE`/absent (matching a freshly-unpacked Java instance).

use std::sync::Arc;

use rivet_registry::Holder;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::core::{QuartPos, SectionPos};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder;

use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};

/// `BLENDING_DENSITY_FACTOR` — the `* 0.1` applied to stored density-column
/// values (BlendingData.java line 36).
pub const BLENDING_DENSITY_FACTOR: f64 = 0.1;

/// `CELL_WIDTH` — 4 (protected; BlendingData.java line 38).
pub const CELL_WIDTH: i32 = 4;

/// `CELL_HEIGHT` — 8 (protected; BlendingData.java line 39).
pub const CELL_HEIGHT: i32 = 8;

/// `CELL_RATIO` — `CELL_HEIGHT / CELL_WIDTH` (protected; BlendingData.java
/// line 40).
pub const CELL_RATIO: i32 = 2;

/// `QUARTS_PER_SECTION` — `QuartPos.fromBlock(16)` = `16 >> 2`
/// (BlendingData.java line 43).
pub const QUARTS_PER_SECTION: i32 = 4;

/// `CELL_HORIZONTAL_MAX_INDEX_INSIDE` — `QUARTS_PER_SECTION - 1`.
pub const CELL_HORIZONTAL_MAX_INDEX_INSIDE: i32 = QUARTS_PER_SECTION - 1;

/// `CELL_HORIZONTAL_MAX_INDEX_OUTSIDE` — `QUARTS_PER_SECTION`.
pub const CELL_HORIZONTAL_MAX_INDEX_OUTSIDE: i32 = QUARTS_PER_SECTION;

/// `CELL_COLUMN_INSIDE_COUNT` — `2 * CELL_HORIZONTAL_MAX_INDEX_INSIDE + 1`.
pub const CELL_COLUMN_INSIDE_COUNT: i32 = 2 * CELL_HORIZONTAL_MAX_INDEX_INSIDE + 1;

/// `CELL_COLUMN_OUTSIDE_COUNT` — `2 * CELL_HORIZONTAL_MAX_INDEX_OUTSIDE + 1`.
pub const CELL_COLUMN_OUTSIDE_COUNT: i32 = 2 * CELL_HORIZONTAL_MAX_INDEX_OUTSIDE + 1;

/// `CELL_COLUMN_COUNT` — `CELL_COLUMN_INSIDE_COUNT + CELL_COLUMN_OUTSIDE_COUNT`
/// (16; the `heights` array length the `Packed` codec validates).
pub const CELL_COLUMN_COUNT: usize =
    (CELL_COLUMN_INSIDE_COUNT + CELL_COLUMN_OUTSIDE_COUNT) as usize;

/// `NO_VALUE` — `Double.MAX_VALUE`, the "not set" sentinel.
pub const NO_VALUE: f64 = f64::MAX;

/// `net.minecraft.world.level.levelgen.blending.BlendingData`.
///
/// The per-old-chunk column grid. `heights` is always present (16 entries,
/// `NO_VALUE`-filled when absent from the packed form); `densities` and
/// `biomes` are per-column and stay `None` until the chunk-reading
/// `calculateData` half lands (RivetTodo #177).
#[derive(Debug, Clone)]
pub struct BlendingData {
    area_with_old_generation: SimpleLevelHeightAccessor,
    heights: Vec<f64>,
    /// Per-column biome lists; `None` until the deferred `calculateData` chunk
    /// reads land (RivetTodo #177). `pub(crate)` because `Blender` and the
    /// unit's tests build populated instances. Dead in the lib target until the
    /// chunk-reading `iterateBiomes` consumers land with `of(WorldGenRegion)`.
    #[allow(dead_code)]
    pub(crate) biomes: Vec<Option<Vec<Option<Holder<BiomeId>>>>>,
    /// Per-column density arrays; `None` until the deferred `calculateData`
    /// chunk reads land (RivetTodo #177). `pub(crate)` because `Blender` and
    /// the unit's tests build populated instances.
    pub(crate) densities: Vec<Option<Vec<f64>>>,
}

impl BlendingData {
    /// The private constructor (BlendingData.java lines 69-78): the heights
    /// array defaults to `NO_VALUE`-filled when absent, the density/biome
    /// columns start `null`, and `areaWithOldGeneration` is
    /// `LevelHeightAccessor.create(minY, height)` with `minY =
    /// SectionPos.sectionToBlockCoord(minSection)`, `height =
    /// sectionToBlockCoord(maxSection) - minY`.
    fn new(min_section: i32, max_section: i32, heights: Option<Vec<f64>>) -> Self {
        let heights = heights.unwrap_or_else(|| vec![NO_VALUE; CELL_COLUMN_COUNT]);
        let densities = vec![None; CELL_COLUMN_COUNT];
        let biomes = vec![None; CELL_COLUMN_COUNT];
        let min_y = SectionPos::section_to_block_coord(min_section);
        let height = SectionPos::section_to_block_coord(max_section).wrapping_sub(min_y);
        BlendingData {
            area_with_old_generation: create(min_y, height),
            heights,
            biomes,
            densities,
        }
    }

    /// `unpack(Packed)` — the `Packed` → `BlendingData` factory (BlendingData.java
    /// lines 80-82). `None` passes through.
    pub fn unpack(packed: Option<Packed>) -> Option<BlendingData> {
        packed
            .map(|packed| BlendingData::new(packed.min_section, packed.max_section, packed.heights))
    }

    /// `pack()` — `{minSection: getMinSectionY(), maxSection:
    /// getMaxSectionY() + 1, heights: present iff any entry != NO_VALUE}`
    /// (BlendingData.java lines 84-99).
    pub fn pack(&self) -> Packed {
        let has_height = self.heights.iter().any(|height| *height != NO_VALUE);
        Packed {
            min_section: self.area_with_old_generation.get_min_section_y(),
            max_section: self
                .area_with_old_generation
                .get_max_section_y()
                .wrapping_add(1),
            heights: if has_height {
                Some(self.heights.clone())
            } else {
                None
            },
        }
    }

    /// `getAreaWithOldGeneration()` (BlendingData.java lines 382-384).
    pub fn area_with_old_generation(&self) -> SimpleLevelHeightAccessor {
        self.area_with_old_generation
    }

    /// `getHeight(int cellX, int cellY, int cellZ)` (BlendingData.java lines
    /// 260-266) — reads the height cell, `NO_VALUE` for interior cells that
    /// were never sampled.
    pub(crate) fn get_height(&self, cell_x: i32, _cell_y: i32, cell_z: i32) -> f64 {
        if cell_x == CELL_HORIZONTAL_MAX_INDEX_OUTSIDE
            || cell_z == CELL_HORIZONTAL_MAX_INDEX_OUTSIDE
        {
            self.heights[get_outside_index(cell_x, cell_z) as usize]
        } else if cell_x != 0 && cell_z != 0 {
            NO_VALUE
        } else {
            self.heights[get_inside_index(cell_x, cell_z) as usize]
        }
    }

    /// `getDensity(int cellX, int cellY, int cellZ)` (BlendingData.java lines
    /// 277-285) — the `cellY == getMinY()` floor cell is the constant
    /// `BLENDING_DENSITY_FACTOR`; interior cells that were never sampled report
    /// `NO_VALUE`.
    pub(crate) fn get_density(&self, cell_x: i32, cell_y: i32, cell_z: i32) -> f64 {
        if cell_y == self.get_min_y() {
            BLENDING_DENSITY_FACTOR
        } else if cell_x == CELL_HORIZONTAL_MAX_INDEX_OUTSIDE
            || cell_z == CELL_HORIZONTAL_MAX_INDEX_OUTSIDE
        {
            self.get_density_from_column(
                self.densities[get_outside_index(cell_x, cell_z) as usize].as_deref(),
                cell_y,
            )
        } else if cell_x != 0 && cell_z != 0 {
            NO_VALUE
        } else {
            self.get_density_from_column(
                self.densities[get_inside_index(cell_x, cell_z) as usize].as_deref(),
                cell_y,
            )
        }
    }

    /// `getDensity(double[] densityColumn, int cellY)` (BlendingData.java lines
    /// 268-275) — `null` column or out-of-range cell → `NO_VALUE`, else the
    /// stored density scaled by `BLENDING_DENSITY_FACTOR`.
    fn get_density_from_column(&self, density_column: Option<&[f64]>, cell_y: i32) -> f64 {
        match density_column {
            None => NO_VALUE,
            Some(density_column) => {
                let y_index = self.get_cell_y_index(cell_y);
                if y_index >= 0 && (y_index as usize) < density_column.len() {
                    density_column[y_index as usize] * BLENDING_DENSITY_FACTOR
                } else {
                    NO_VALUE
                }
            }
        }
    }

    /// `iterateBiomes(int minCellX, int quartY, int minCellZ, BiomeConsumer)`
    /// (BlendingData.java lines 287-301). Dead in the lib target until the
    /// chunk-reading `getBiomeColumn` consumers land (RivetTodo #177).
    #[allow(dead_code)]
    pub(crate) fn iterate_biomes(
        &self,
        min_cell_x: i32,
        quart_y: i32,
        min_cell_z: i32,
        mut consumer: impl FnMut(i32, i32, Holder<BiomeId>),
    ) {
        let from = QuartPos::from_block(self.area_with_old_generation.get_min_y());
        let to = QuartPos::from_block(self.area_with_old_generation.get_max_y());
        if quart_y >= from && quart_y <= to {
            let quart_index = quart_y.wrapping_sub(from);
            for (index, biome_cell) in self.biomes.iter().enumerate() {
                // `biome_cell` is `Vec<Optional<Holder>>`: flatten the outer
                // column and the inner `Optional` before visiting.
                if let Some(biome_cell) = biome_cell
                    && let Some(value) = biome_cell.get(quart_index as usize)
                    && let Some(value) = value
                {
                    // Java wraps the `minCellX + getX(i)` int sums.
                    consumer(
                        min_cell_x.wrapping_add(get_x(index as i32)),
                        min_cell_z.wrapping_add(get_z(index as i32)),
                        value.clone(),
                    );
                }
            }
        }
    }

    /// `iterateHeights(int minCellX, int minCellZ, HeightConsumer)`
    /// (BlendingData.java lines 303-310).
    pub(crate) fn iterate_heights(
        &self,
        min_cell_x: i32,
        min_cell_z: i32,
        mut consumer: impl FnMut(i32, i32, f64),
    ) {
        for (index, value) in self.heights.iter().enumerate() {
            if *value != NO_VALUE {
                // Java wraps the `minCellX + getX(i)` int sums.
                consumer(
                    min_cell_x.wrapping_add(get_x(index as i32)),
                    min_cell_z.wrapping_add(get_z(index as i32)),
                    *value,
                );
            }
        }
    }

    /// `iterateDensities(int minCellX, int minCellZ, int fromCellY, int
    /// toCellY, DensityConsumer)` (BlendingData.java lines 312-330).
    pub(crate) fn iterate_densities(
        &self,
        min_cell_x: i32,
        min_cell_z: i32,
        from_cell_y: i32,
        to_cell_y: i32,
        mut consumer: impl FnMut(i32, i32, i32, f64),
    ) {
        let min_cell_y = self.get_column_min_y();
        let min_y_index = 0.max(from_cell_y.wrapping_sub(min_cell_y));
        let max_y_index = self
            .cell_count_per_column()
            .min(to_cell_y.wrapping_sub(min_cell_y));
        for (index, density_column) in self.densities.iter().enumerate() {
            if let Some(density_column) = density_column {
                // Java wraps the `minCellX + getX(i)` int sums.
                let test_cell_x = min_cell_x.wrapping_add(get_x(index as i32));
                let test_cell_z = min_cell_z.wrapping_add(get_z(index as i32));
                for y_index in min_y_index..max_y_index {
                    consumer(
                        test_cell_x,
                        y_index.wrapping_add(min_cell_y),
                        test_cell_z,
                        density_column[y_index as usize] * BLENDING_DENSITY_FACTOR,
                    );
                }
            }
        }
    }

    /// `cellCountPerColumn()` — `getSectionsCount() * 2` (BlendingData.java
    /// lines 332-334), the density-column length.
    fn cell_count_per_column(&self) -> i32 {
        self.area_with_old_generation
            .get_sections_count()
            .wrapping_mul(2)
    }

    /// `quartCountPerColumn()` — `QuartPos.fromSection(getSectionsCount())`
    /// (BlendingData.java lines 336-338), the biome-column length. Dead in the
    /// lib target until the chunk-reading `getBiomeColumn` lands (RivetTodo
    /// #177).
    #[allow(dead_code)]
    fn quart_count_per_column(&self) -> i32 {
        QuartPos::from_section(self.area_with_old_generation.get_sections_count())
    }

    /// `getColumnMinY()` — `getMinY() + 1` (BlendingData.java lines 340-342).
    fn get_column_min_y(&self) -> i32 {
        self.get_min_y().wrapping_add(1)
    }

    /// `getMinY()` — `getMinSectionY() * 2` (BlendingData.java lines 344-346),
    /// the cell-y of the world floor.
    fn get_min_y(&self) -> i32 {
        self.area_with_old_generation
            .get_min_section_y()
            .wrapping_mul(2)
    }

    /// `getCellYIndex(int cellY)` — `cellY - getColumnMinY()` (BlendingData.java
    /// lines 348-350), the cell-y as a density-column index.
    fn get_cell_y_index(&self, cell_y: i32) -> i32 {
        cell_y.wrapping_sub(self.get_column_min_y())
    }
}

/// `BlendingData.Packed(int minSection, int maxSection, Optional<double[]>
/// heights)` record (BlendingData.java lines 398-415) — the serialized form of
/// a `BlendingData`.
///
/// `max_section` is exclusive (`getMaxSectionY() + 1`); `heights` is present
/// iff any stored height differs from `NO_VALUE`.
///
/// Equality deviation from Java: the record's generated `equals` compares the
/// `Optional<double[]>` heights field via `Objects.equals`, which for arrays
/// is reference identity — two `Packed` holding distinct-but-equal-content
/// arrays compare unequal in Java. The port deliberately derives value
/// equality (content comparison), the practically useful semantic and the one
/// the round-trip test relies on; the deviation is unobservable in serialized
/// NBT bytes. Any future caller must not rely on `Packed`'s `PartialEq`
/// matching Java's reference-identity record `equals`.
///
/// `RivetTodo(#177)`: a byte-for-byte parity audit of `Packed.equals` must
/// treat this content-equality `PartialEq` as a deliberate, documented
/// deviation rather than a porting error.
#[derive(Debug, Clone, PartialEq)]
pub struct Packed {
    min_section: i32,
    max_section: i32,
    heights: Option<Vec<f64>>,
}

impl Packed {
    /// The record constructor.
    pub fn new(min_section: i32, max_section: i32, heights: Option<Vec<f64>>) -> Self {
        Packed {
            min_section,
            max_section,
            heights,
        }
    }

    /// `minSection()` (record accessor).
    pub fn min_section(&self) -> i32 {
        self.min_section
    }

    /// `maxSection()` (record accessor).
    pub fn max_section(&self) -> i32 {
        self.max_section
    }

    /// `heights()` (record accessor).
    pub fn heights(&self) -> Option<&Vec<f64>> {
        self.heights.as_ref()
    }

    /// `Packed.CODEC` — the `{min_section, max_section, heights}` record codec
    /// (BlendingData.java lines 400-408), as the ops-generic
    /// `packed_codec::<Ops>()` factory.
    ///
    /// `heights` is the lenient-optional double-array field: absent or
    /// malformed on decode → `None`, omitted on encode when `None`. The
    /// `Codec.DOUBLE.listOf().xmap(Doubles::toArray, Doubles::asList)` shape
    /// reduces to `list(double_codec)` because the port's `Vec<f64>` is both
    /// the array and its list representation. `.validate(validateArraySize)`
    /// enforces the 16-entry length on both directions (Java's
    /// `Codec.validate` is a `flatXmap`).
    pub fn packed_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Packed, Ops>> {
        let base = record_builder::create(|instance| {
            instance
                .group(record_builder::RecordCodecBuilder::of_named(
                    Arc::new(|packed: &Packed| packed.min_section),
                    "min_section".to_string(),
                    codec::int_codec::<Ops>(),
                ))
                .and(record_builder::RecordCodecBuilder::of_named(
                    Arc::new(|packed: &Packed| packed.max_section),
                    "max_section".to_string(),
                    codec::int_codec::<Ops>(),
                ))
                .and(record_builder::RecordCodecBuilder::of(
                    Arc::new(|packed: &Packed| packed.heights.clone()),
                    codec::optional_field(
                        "heights".to_string(),
                        codec::list(codec::double_codec::<Ops>()),
                        true,
                    ),
                ))
                .apply(
                    instance,
                    Arc::new(
                        |min_section: i32, max_section: i32, heights: Option<Vec<f64>>| {
                            Packed::new(min_section, max_section, heights)
                        },
                    ),
                )
        });
        codec::validate(base, Arc::new(validate_array_size))
    }
}

/// `validateArraySize(Packed)` — `DataResult.error("heights has to be of
/// length 16")` when a present heights array isn't `CELL_COLUMN_COUNT` long
/// (BlendingData.java lines 410-414).
fn validate_array_size(packed: &Packed) -> DataResult<Packed> {
    if let Some(heights) = &packed.heights
        && heights.len() != CELL_COLUMN_COUNT
    {
        return DataResult::error(format!("heights has to be of length {}", CELL_COLUMN_COUNT));
    }
    DataResult::success(packed.clone())
}

/// `getInsideIndex(int x, int z)` (BlendingData.java lines 352-354).
fn get_inside_index(x: i32, z: i32) -> i32 {
    CELL_HORIZONTAL_MAX_INDEX_INSIDE
        .wrapping_sub(x)
        .wrapping_add(z)
}

/// `getOutsideIndex(int x, int z)` (BlendingData.java lines 356-358).
fn get_outside_index(x: i32, z: i32) -> i32 {
    CELL_COLUMN_INSIDE_COUNT
        .wrapping_add(x)
        .wrapping_add(CELL_HORIZONTAL_MAX_INDEX_OUTSIDE)
        .wrapping_sub(z)
}

/// `getX(int index)` (BlendingData.java lines 360-366).
fn get_x(index: i32) -> i32 {
    if index < CELL_COLUMN_INSIDE_COUNT {
        return zero_if_negative(CELL_HORIZONTAL_MAX_INDEX_INSIDE.wrapping_sub(index));
    }
    let offset_index = index.wrapping_sub(CELL_COLUMN_INSIDE_COUNT);
    CELL_HORIZONTAL_MAX_INDEX_OUTSIDE.wrapping_sub(zero_if_negative(
        CELL_HORIZONTAL_MAX_INDEX_OUTSIDE.wrapping_sub(offset_index),
    ))
}

/// `getZ(int index)` (BlendingData.java lines 368-375).
fn get_z(index: i32) -> i32 {
    if index < CELL_COLUMN_INSIDE_COUNT {
        return zero_if_negative(index.wrapping_sub(CELL_HORIZONTAL_MAX_INDEX_INSIDE));
    }
    let offset_index = index.wrapping_sub(CELL_COLUMN_INSIDE_COUNT);
    CELL_HORIZONTAL_MAX_INDEX_OUTSIDE.wrapping_sub(zero_if_negative(
        offset_index.wrapping_sub(CELL_HORIZONTAL_MAX_INDEX_OUTSIDE),
    ))
}

/// `zeroIfNegative(int value)` — `value & ~(value >> 31)` (BlendingData.java
/// lines 378-380). Java `>>` is arithmetic on `int`, so `value >> 31` is
/// `-1`/`0` for negative/non-negative; `value & ~(sign)` yields `0` when
/// negative, `value` otherwise.
fn zero_if_negative(value: i32) -> i32 {
    value & !(value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::nbt_ops::NbtOps;
    use rivet_nbt::tag::Tag;

    fn overworld_packed(heights: Option<Vec<f64>>) -> Packed {
        Packed::new(-4, 20, heights)
    }

    #[test]
    fn cell_geometry_constants() {
        assert_eq!(QUARTS_PER_SECTION, 4);
        assert_eq!(CELL_HORIZONTAL_MAX_INDEX_INSIDE, 3);
        assert_eq!(CELL_HORIZONTAL_MAX_INDEX_OUTSIDE, 4);
        assert_eq!(CELL_COLUMN_INSIDE_COUNT, 7);
        assert_eq!(CELL_COLUMN_OUTSIDE_COUNT, 9);
        assert_eq!(CELL_COLUMN_COUNT, 16);
        assert_eq!(CELL_WIDTH, 4);
        assert_eq!(CELL_HEIGHT, 8);
        assert_eq!(CELL_RATIO, 2);
    }

    #[test]
    fn inside_outside_index_math() {
        // `getInsideIndex(x, z) = 3 - x + z` (BlendingData.java line 353).
        assert_eq!(get_inside_index(0, 0), 3);
        assert_eq!(get_inside_index(1, 0), 2);
        assert_eq!(get_inside_index(0, 1), 4);
        assert_eq!(get_inside_index(3, 3), 3);
        // `getOutsideIndex(x, z) = 7 + x + 4 - z` (BlendingData.java line 357).
        assert_eq!(get_outside_index(0, 0), 11);
        assert_eq!(get_outside_index(4, 0), 15);
        assert_eq!(get_outside_index(0, 4), 7);
        assert_eq!(get_outside_index(4, 4), 11);
    }

    #[test]
    fn x_z_index_math() {
        // `getX`/`getZ` map column index → cell (x, z). Inside indices 0..7
        // are the 3x3 (minus centre) interior; outside indices 7..16 the ring.
        // Sample a few that pin the formula:
        // index 0: x = zeroIfNegative(3 - 0) = 3, z = zeroIfNegative(0 - 3) = 0
        assert_eq!(get_x(0), 3);
        assert_eq!(get_z(0), 0);
        // index 3 (inside 0,0): x = zeroIfNegative(3-3) = 0, z = zeroIfNegative(3-3) = 0
        assert_eq!(get_x(3), 0);
        assert_eq!(get_z(3), 0);
        // index 6: x = zeroIfNegative(3-6) = 0, z = zeroIfNegative(6-3) = 3
        assert_eq!(get_x(6), 0);
        assert_eq!(get_z(6), 3);
        // index 7 (first outside): offset = 0; x = 4 - zeroIfNegative(4-0) = 0,
        // z = 4 - zeroIfNegative(0-4) = 4
        assert_eq!(get_x(7), 0);
        assert_eq!(get_z(7), 4);
        // index 15: offset = 8; x = 4 - zeroIfNegative(4-8) = 4, z = 4 - zeroIfNegative(8-4) = 0
        assert_eq!(get_x(15), 4);
        assert_eq!(get_z(15), 0);
    }

    #[test]
    fn zero_if_negative_math() {
        assert_eq!(zero_if_negative(0), 0);
        assert_eq!(zero_if_negative(7), 7);
        assert_eq!(zero_if_negative(-1), 0);
        assert_eq!(zero_if_negative(-100), 0);
        assert_eq!(zero_if_negative(i32::MIN), 0);
        assert_eq!(zero_if_negative(i32::MAX), i32::MAX);
    }

    /// Overworld geometry (`min_section = -4`, `max_section = 20` from
    /// `Packed`, so min y = -64, height = 384): the cell-y of the floor is
    /// `-4 * 2 = -8`, column min `-7`, 24 sections → 48 density cells and
    /// `QuartPos.fromSection(24) = 24 * 4 = 96` quart cells per column.
    #[test]
    fn overworld_vertical_geometry() {
        let data = BlendingData::unpack(Some(overworld_packed(None))).unwrap();
        assert_eq!(data.area_with_old_generation.get_min_y(), -64);
        assert_eq!(data.area_with_old_generation.get_height(), 384);
        assert_eq!(data.area_with_old_generation.get_max_y(), 319);
        assert_eq!(data.get_min_y(), -8);
        assert_eq!(data.get_column_min_y(), -7);
        assert_eq!(data.cell_count_per_column(), 48);
        assert_eq!(data.quart_count_per_column(), 96);
        assert_eq!(data.get_cell_y_index(-8), -1);
        assert_eq!(data.get_cell_y_index(-7), 0);
        assert_eq!(data.get_cell_y_index(39), 46);
    }

    /// `unpack`/`pack` round-trip: a heights-bearing packed form survives, and
    /// an all-`NO_VALUE` heights array packs as absent (BlendingData.java lines
    /// 84-99).
    #[test]
    fn pack_unpack_round_trip() {
        let heights: Vec<f64> = (0..16).map(|i| i as f64 * 10.0).collect();
        let packed = overworld_packed(Some(heights.clone()));
        let data = BlendingData::unpack(Some(packed.clone())).unwrap();
        assert_eq!(data.pack(), packed);
        assert_eq!(data.pack().heights(), Some(&heights));

        // Absent heights unpack to an all-NO_VALUE array, and pack omits them.
        let data = BlendingData::unpack(Some(overworld_packed(None))).unwrap();
        assert_eq!(data.pack().heights(), None);
        assert_eq!(data.pack().min_section(), -4);
        assert_eq!(data.pack().max_section(), 20);

        assert!(BlendingData::unpack(None).is_none());
    }

    /// `getHeight` reads the stored cell: inside origin → `heights[3]`, outside
    /// ring → `heights[getOutsideIndex]`, unsampled interior → `NO_VALUE`
    /// (BlendingData.java lines 260-266).
    #[test]
    fn get_height_reads_cells() {
        let mut heights = vec![NO_VALUE; CELL_COLUMN_COUNT];
        heights[3] = 120.0; // inside (0, 0) — getInsideIndex(0, 0) = 3
        heights[15] = 88.0; // outside (4, 0) — getOutsideIndex(4, 0) = 7 + 4 + 4 - 0 = 15
        let data = BlendingData::unpack(Some(overworld_packed(Some(heights)))).unwrap();
        assert_eq!(data.get_height(0, 0, 0), 120.0);
        assert_eq!(data.get_height(4, 0, 0), 88.0);
        assert_eq!(data.get_height(1, 0, 1), NO_VALUE);
        assert_eq!(data.get_height(3, 0, 3), NO_VALUE);
    }

    /// `getDensity`: the world-floor cell (`cellY == getMinY()`) is the
    /// constant `BLENDING_DENSITY_FACTOR`; an absent column or out-of-range cell
    /// is `NO_VALUE`; a stored value is scaled by `BLENDING_DENSITY_FACTOR`
    /// (BlendingData.java lines 268-285).
    #[test]
    fn get_density_reads_cells() {
        let data = BlendingData::unpack(Some(overworld_packed(None))).unwrap();
        assert_eq!(data.get_density(0, -8, 0), BLENDING_DENSITY_FACTOR);
        assert_eq!(data.get_density(0, 0, 0), NO_VALUE);
        assert_eq!(data.get_density(1, 0, 1), NO_VALUE);

        // A stored column: inside origin column 0.5 at cell y 5 (index 12).
        let mut data = BlendingData::unpack(Some(overworld_packed(None))).unwrap();
        data.densities[3] = Some(vec![0.0; 48]);
        data.densities[3].as_mut().unwrap()[12] = 0.5;
        assert_eq!(data.get_density(0, 5, 0), 0.5 * BLENDING_DENSITY_FACTOR);
        // The column holds 48 cells (indices 0..48 = cell ys -7..40 inclusive):
        // cell y 39 (index 46) and cell y 40 (index 47) are in range; cell y 41
        // (index 48) is out.
        assert_eq!(data.get_density(0, 39, 0), 0.0);
        assert_eq!(data.get_density(0, 40, 0), 0.0);
        assert_eq!(data.get_density(0, 41, 0), NO_VALUE);
    }

    /// `iterateHeights` visits only non-`NO_VALUE` cells at their cell
    /// coordinates (BlendingData.java lines 303-310).
    #[test]
    fn iterate_heights_visits_set_cells() {
        let mut heights = vec![NO_VALUE; CELL_COLUMN_COUNT];
        heights[3] = 120.0; // inside cell (0, 0) — getX/Z(3) = (0, 0)
        heights[15] = 88.0; // outside cell (4, 0) — getX/Z(15) = (4, 0)
        let data = BlendingData::unpack(Some(overworld_packed(Some(heights)))).unwrap();
        let mut visited = Vec::new();
        data.iterate_heights(4, 8, |x, z, h| visited.push((x, z, h)));
        assert_eq!(visited, vec![(4, 8, 120.0), (8, 8, 88.0)]);
    }

    /// `iterateDensities` visits the present columns' cells in the
    /// `[fromCellY, toCellY)` band, scaling by `BLENDING_DENSITY_FACTOR`
    /// (BlendingData.java lines 312-330).
    #[test]
    fn iterate_densities_visits_columns_in_band() {
        let mut data = BlendingData::unpack(Some(overworld_packed(None))).unwrap();
        data.densities[3] = Some(vec![2.0; 48]); // inside cell (0, 0)
        data.densities[15] = Some(vec![4.0; 48]); // outside cell (4, 0)
        let mut visited = Vec::new();
        // minCellY = -7; band [4, 7) → y indices 11..14 → cell ys 4,5,6. Column
        // 3 → cell (0, 0) → (minCellX + 0, minCellZ + 0); column 15 → cell
        // (4, 0) → (minCellX + 4, minCellZ + 0).
        data.iterate_densities(4, 8, 4, 7, |x, y, z, d| visited.push((x, y, z, d)));
        let expected: Vec<(i32, i32, i32, f64)> = (11..14)
            .map(|i| (4, i + (-7), 8, 2.0 * BLENDING_DENSITY_FACTOR))
            .chain((11..14).map(|i| (8, i + (-7), 8, 4.0 * BLENDING_DENSITY_FACTOR)))
            .collect();
        assert_eq!(visited.len(), 6);
        for (i, e) in expected.iter().enumerate() {
            assert_eq!(visited[i].0, e.0);
            assert_eq!(visited[i].1, e.1);
            assert_eq!(visited[i].2, e.2);
            assert!((visited[i].3 - e.3).abs() < 1e-12);
        }
    }

    /// `iterateBiomes` visits the present columns' cells at the requested quart
    /// band (BlendingData.java lines 287-301).
    #[test]
    fn iterate_biomes_visits_columns() {
        let mut data = BlendingData::unpack(Some(overworld_packed(None))).unwrap();
        let biome = Holder::direct(BiomeId(7));
        data.biomes[3] = Some(vec![None, Some(biome.clone()), None]);
        let mut visited = Vec::new();
        data.iterate_biomes(4, -15, 8, |x, z, b| visited.push((x, z, b)));
        // quart -15 is index 1 of the column (from block min y -64 → quart -16).
        assert_eq!(visited.len(), 1);
        assert_eq!(visited[0], (4, 8, biome));
    }

    /// The `Packed` codec round-trips the overworld shape through NBT ops:
    /// `{min_section: -4, max_section: 20, heights: [16 doubles]}`.
    #[test]
    fn packed_codec_round_trips_through_nbt() {
        let ops = NbtOps::instance();
        let codec = Packed::packed_codec::<NbtOps>();
        let heights: Vec<f64> = (0..16).map(|i| (i * 10) as f64).collect();
        let packed = overworld_packed(Some(heights));

        let encoded = codec
            .encode_start(&ops, &packed)
            .result()
            .expect("encode")
            .clone();
        let Tag::Compound(compound) = &encoded else {
            panic!("expected a compound tag");
        };
        assert_eq!(compound.get_int("min_section").unwrap(), -4);
        assert_eq!(compound.get_int("max_section").unwrap(), 20);
        let heights_tag = compound.get("heights").expect("heights present");
        let Tag::List(list) = heights_tag else {
            panic!("expected a list tag");
        };
        assert_eq!(list.list.len(), 16);

        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded, packed);
    }

    /// An absent `heights` field decodes to `None` and encodes back with the
    /// field omitted (the lenient-optional-field behavior).
    #[test]
    fn packed_codec_omits_absent_heights() {
        let ops = NbtOps::instance();
        let codec = Packed::packed_codec::<NbtOps>();
        let packed = overworld_packed(None);

        let encoded = codec
            .encode_start(&ops, &packed)
            .result()
            .expect("encode")
            .clone();
        let Tag::Compound(compound) = &encoded else {
            panic!("expected a compound tag");
        };
        assert!(compound.get("heights").is_none());
        assert_eq!(compound.get_int("min_section").unwrap(), -4);
        assert_eq!(compound.get_int("max_section").unwrap(), 20);

        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded, packed);
        assert!(decoded.heights().is_none());
    }

    /// `validateArraySize` rejects a present heights array that isn't 16 long
    /// with the Java error message (BlendingData.java lines 410-414).
    #[test]
    fn packed_codec_rejects_wrong_heights_length() {
        let ops = NbtOps::instance();
        let codec = Packed::packed_codec::<NbtOps>();

        for len in [0usize, 1, 15, 17, 100] {
            let packed = overworld_packed(Some(vec![1.0; len]));
            let result = codec.encode_start(&ops, &packed);
            let error = result.error_ref().expect("validate on encode");
            assert!(
                error.message().contains("heights has to be of length 16"),
                "unexpected error for len {len}: {}",
                error.message()
            );

            // Build the same malformed tag directly and check the decode path.
            let mut compound = rivet_nbt::compound_tag::CompoundTag::new();
            compound.put_int("min_section", -4);
            compound.put_int("max_section", 20);
            compound.put(
                "heights".into(),
                Tag::List(rivet_nbt::list_tag::ListTag::with_list(
                    (0..len)
                        .map(|_| Tag::Double(rivet_nbt::double_tag::DoubleTag::value_of(1.0)))
                        .collect(),
                )),
            );
            let result = codec.parse(&ops, &Tag::Compound(compound));
            let error = result.error_ref().expect("validate on decode");
            assert!(
                error.message().contains("heights has to be of length 16"),
                "unexpected error for len {len}: {}",
                error.message()
            );
        }
    }

    /// A malformed `heights` value (a string) is lenient-absorbed to `None`,
    /// matching the `lenientOptionalFieldOf` decode contract.
    #[test]
    fn packed_codec_leniently_absorbs_malformed_heights() {
        let ops = NbtOps::instance();
        let codec = Packed::packed_codec::<NbtOps>();
        let mut compound = rivet_nbt::compound_tag::CompoundTag::new();
        compound.put_int("min_section", -4);
        compound.put_int("max_section", 20);
        compound.put(
            "heights".into(),
            Tag::String(rivet_nbt::string_tag::StringTag::value_of("nope".into())),
        );
        let decoded = codec
            .parse(&ops, &Tag::Compound(compound))
            .result()
            .expect("lenient decode succeeds")
            .clone();
        assert_eq!(decoded, overworld_packed(None));
    }
}
