//! Static block-light channel for generated `ChunkStatus::Light` chunks (MC 26.2, Paper).
//!
//! Ports the static chunk-generation path of Paper's
//! `ca.spottedleaf.moonrise.patches.starlight.light.BlockStarLightEngine`,
//! `StarLightEngine` (`light`, `handleEmptySectionChanges`, `propagateNeighbourLevels`,
//! `performLightIncrease`), and `StarLightInterface` (`getBlockLightValue`,
//! `blockReader.getDataLayerData`) over generic [`ChunkAccess`] values in `rivet-world`.
//!
//! Design properties:
//! - Operates on a bounded radius-2 (`5x5`) [`ChunkAccess`] workspace matching
//!   Paper's `StarLightEngine` 5×5 section cache and `ChunkStatus.LIGHT` neighbor
//!   requirements.
//! - Truthfully validates [`LightChannelCapability`] against a
//!   [`DimensionLightPolicy`] (e.g. normal Overworld requires both sky and block
//!   light) and refuses unsupported configurations with a typed error before
//!   mutating any chunk.
//! - Honestly refuses conditionally full-opaque blocks
//!   ([`StaticBlockLightError::UnsupportedConditionalOcclusion`]) rather than
//!   guessing face-shape occlusion (owned by issue #710).
//! - Stages all center and neighbor nibble updates in detached buffers and
//!   commits sky + block nibbles, emptiness maps, and `light_correct` atomically;
//!   any error or callback panic leaves all chunks completely untouched so
//!   retries are safe.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::hash::Hash;
use std::panic::{AssertUnwindSafe, catch_unwind};

use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};

use crate::chunk::chunk_access::ChunkAccess;
use crate::chunk::data_layer::DataLayer;
use crate::chunk::status::ChunkStatus;
use crate::level::height_accessor::LevelHeightAccessor;
use crate::lighting::swmr_nibble_array::SwmrNibbleArray;

/// Radius of the bounded generated-light neighbor workspace (`5x5` chunks).
///
/// Matches Paper's `StarLightEngine` 5×5 chunk cache (`dx in -2..=2, dz in -2..=2`).
pub const GENERATED_LIGHT_NEIGHBOR_RADIUS: i32 = 2;

/// Side length of the bounded generated-light chunk grid (`2 * RADIUS + 1 = 5`).
const GRID_SIZE: usize = (GENERATED_LIGHT_NEIGHBOR_RADIUS as usize) * 2 + 1;

/// Paper's `StarLightEngine.AxisDirection` propagation order:
/// `POSITIVE_X`, `NEGATIVE_X`, `POSITIVE_Z`, `NEGATIVE_Z`, `POSITIVE_Y`, `NEGATIVE_Y`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum AxisDirection {
    PositiveX = 0,
    NegativeX = 1,
    PositiveZ = 2,
    NegativeZ = 3,
    PositiveY = 4,
    NegativeY = 5,
}

const AXIS_DIRECTIONS: [AxisDirection; 6] = [
    AxisDirection::PositiveX,
    AxisDirection::NegativeX,
    AxisDirection::PositiveZ,
    AxisDirection::NegativeZ,
    AxisDirection::PositiveY,
    AxisDirection::NegativeY,
];

const ONLY_HORIZONTAL_DIRECTIONS: [AxisDirection; 4] = [
    AxisDirection::PositiveX,
    AxisDirection::NegativeX,
    AxisDirection::PositiveZ,
    AxisDirection::NegativeZ,
];

const ALL_DIRECTIONS_BITSET: u8 = 0b11_1111;

impl AxisDirection {
    #[inline]
    const fn offset(self) -> (i32, i32, i32) {
        match self {
            Self::PositiveX => (1, 0, 0),
            Self::NegativeX => (-1, 0, 0),
            Self::PositiveZ => (0, 0, 1),
            Self::NegativeZ => (0, 0, -1),
            Self::PositiveY => (0, 1, 0),
            Self::NegativeY => (0, -1, 0),
        }
    }

    #[inline]
    const fn opposite(self) -> Self {
        match self {
            Self::PositiveX => Self::NegativeX,
            Self::NegativeX => Self::PositiveX,
            Self::PositiveZ => Self::NegativeZ,
            Self::NegativeZ => Self::PositiveZ,
            Self::PositiveY => Self::NegativeY,
            Self::NegativeY => Self::PositiveY,
        }
    }

    #[inline]
    const fn bit(self) -> u8 {
        1u8 << (self as u8)
    }

    /// Paper's `AxisDirection.everythingButTheOppositeDirection`.
    #[inline]
    const fn everything_but_opposite(self) -> u8 {
        ALL_DIRECTIONS_BITSET ^ self.opposite().bit()
    }
}

/// Block-state optical properties required by Paper's `BlockStarLightEngine`.
///
/// Mirrors `PlatformHooks.getLightEmission`, `BlockState.getLightDampening`,
/// and `StarlightAbstractBlockState.starlight$isConditionallyFullOpaque`.
pub trait BlockLightProperties<B> {
    /// Light level emitted by `state` (`0..=15`; masked with `& 0xF` as in Paper).
    fn light_emission(&self, state: &B) -> i32;

    /// Light dampening (opacity) of `state` (`0..=15`; propagation always subtracts
    /// `max(1, light_dampening)` as in Paper's `performLightIncrease`).
    fn light_dampening(&self, state: &B) -> i32;

    /// Whether `state` requires directional face-shape occlusion checks
    /// (`starlight$isConditionallyFullOpaque`).
    fn is_conditionally_full_opaque(&self, state: &B) -> bool;
}

/// Enabled lighting channels for a level or lighting engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightChannelCapability {
    pub has_sky_light: bool,
    pub has_block_light: bool,
}

impl LightChannelCapability {
    /// Normal Overworld capability: both sky light and block light enabled.
    pub const fn normal_overworld() -> Self {
        Self {
            has_sky_light: true,
            has_block_light: true,
        }
    }

    /// Nether / End capability: block light only, no sky light.
    pub const fn block_light_only() -> Self {
        Self {
            has_sky_light: false,
            has_block_light: true,
        }
    }

    /// Validates that this capability truthfully satisfies `policy`.
    pub fn validate(self, policy: DimensionLightPolicy) -> Result<(), StaticBlockLightError> {
        let (require_sky_light, require_block_light) = policy.required_channels();
        if (require_sky_light && !self.has_sky_light)
            || (require_block_light && !self.has_block_light)
        {
            return Err(StaticBlockLightError::UnsupportedLightChannel {
                require_sky_light,
                require_block_light,
                has_sky_light: self.has_sky_light,
                has_block_light: self.has_block_light,
            });
        }
        Ok(())
    }
}

/// Dimension-level channel requirements for generated `ChunkStatus::Light`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DimensionLightPolicy {
    /// Normal Overworld: requires both sky light and block light.
    NormalOverworld,
    /// Dimensions without skylight (Nether, End): requires block light.
    BlockLightOnly,
    /// Custom channel requirement.
    Custom {
        require_sky_light: bool,
        require_block_light: bool,
    },
}

impl DimensionLightPolicy {
    #[inline]
    pub const fn required_channels(self) -> (bool, bool) {
        match self {
            Self::NormalOverworld => (true, true),
            Self::BlockLightOnly => (false, true),
            Self::Custom {
                require_sky_light,
                require_block_light,
            } => (require_sky_light, require_block_light),
        }
    }
}

/// Typed errors returned by the static block-light channel and atomic publisher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StaticBlockLightError {
    /// The configured light channels do not satisfy the dimension policy.
    UnsupportedLightChannel {
        require_sky_light: bool,
        require_block_light: bool,
        has_sky_light: bool,
        has_block_light: bool,
    },
    /// A chunk required within the bounded radius-2 workspace is missing.
    MissingNeighborChunk { pos: ChunkPos },
    /// A chunk's section count does not match the level height accessor.
    SectionCountMismatch {
        pos: ChunkPos,
        expected: usize,
        actual: usize,
    },
    /// Encountered a conditionally full-opaque block whose directional face-shape
    /// occlusion is not supported at this layer (issue #710).
    UnsupportedConditionalOcclusion { pos: BlockPos },
    /// Staged sky nibbles did not match `total_light_sections`.
    SkyNibbleCountMismatch { expected: usize, actual: usize },
    /// Staged sky emptiness map did not match `total_sections`.
    SkyEmptinessCountMismatch { expected: usize, actual: usize },
    /// Staged sky publication was omitted even though sky light is enabled.
    MissingSkyPublication,
    /// Block-light computation or property callback panicked.
    ComputationPanicked { message: String },
}

impl fmt::Display for StaticBlockLightError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedLightChannel {
                require_sky_light,
                require_block_light,
                has_sky_light,
                has_block_light,
            } => write!(
                f,
                "unsupported light channels: required (sky={require_sky_light}, block={require_block_light}), available (sky={has_sky_light}, block={has_block_light})"
            ),
            Self::MissingNeighborChunk { pos } => {
                write!(f, "missing neighbor chunk at ({}, {})", pos.x(), pos.z())
            }
            Self::SectionCountMismatch {
                pos,
                expected,
                actual,
            } => write!(
                f,
                "chunk ({}, {}) has {actual} sections, expected {expected}",
                pos.x(),
                pos.z()
            ),
            Self::UnsupportedConditionalOcclusion { pos } => write!(
                f,
                "conditional face-shape occlusion is not supported at ({}, {}, {})",
                pos.get_x(),
                pos.get_y(),
                pos.get_z()
            ),
            Self::SkyNibbleCountMismatch { expected, actual } => write!(
                f,
                "staged sky nibble count {actual} does not match expected {expected}"
            ),
            Self::SkyEmptinessCountMismatch { expected, actual } => write!(
                f,
                "staged sky emptiness map length {actual} does not match expected {expected}"
            ),
            Self::MissingSkyPublication => {
                write!(
                    f,
                    "sky light is enabled but no staged sky publication was provided"
                )
            }
            Self::ComputationPanicked { message } => {
                write!(f, "static block-light computation panicked: {message}")
            }
        }
    }
}

impl std::error::Error for StaticBlockLightError {}

/// Detached sky-light artifacts to commit atomically alongside block light.
#[derive(Clone, Debug)]
pub struct StagedSkyLightPublication {
    pub center_nibbles: Vec<SwmrNibbleArray>,
    pub center_emptiness_map: Vec<bool>,
    pub neighbor_nibbles: Vec<(ChunkPos, Vec<SwmrNibbleArray>)>,
}

/// Detached block-light computation result ready for atomic publication.
#[derive(Clone, Debug)]
pub struct StagedBlockLightPublication {
    pub center: ChunkPos,
    pub center_nibbles: Vec<SwmrNibbleArray>,
    pub center_emptiness_map: Vec<bool>,
    pub neighbor_nibbles: Vec<(ChunkPos, Vec<SwmrNibbleArray>)>,
}

/// Queue entry for Paper's `StarLightEngine.performLightIncrease` (block light).
#[derive(Clone, Copy, Debug)]
struct IncreaseEntry {
    x: i32,
    y: i32,
    z: i32,
    level: i32,
    directions: u8,
}

/// Paper-faithful static block-light engine (`BlockStarLightEngine.light` /
/// `lightChunk`) operating over generic [`ChunkAccess`] workspaces.
#[derive(Clone, Copy, Debug)]
pub struct StaticBlockLightEngine {
    min_section: i32,
    max_section: i32,
    min_light_section: i32,
    max_light_section: i32,
    total_sections: usize,
    total_light_sections: usize,
}

impl StaticBlockLightEngine {
    /// Creates a static block-light engine bound to the vertical extent of `height`.
    pub fn new(height: &dyn LevelHeightAccessor) -> Self {
        let min_section = height.get_min_section_y();
        let total_sections = height.get_sections_count().max(0) as usize;
        let max_section = min_section
            .wrapping_add(total_sections as i32)
            .wrapping_sub(1);
        let min_light_section = min_section.wrapping_sub(1);
        let max_light_section = max_section.wrapping_add(1);
        let total_light_sections = total_sections + 2;
        Self {
            min_section,
            max_section,
            min_light_section,
            max_light_section,
            total_sections,
            total_light_sections,
        }
    }

    pub const fn min_section(&self) -> i32 {
        self.min_section
    }

    pub const fn max_section(&self) -> i32 {
        self.max_section
    }

    pub const fn min_light_section(&self) -> i32 {
        self.min_light_section
    }

    pub const fn max_light_section(&self) -> i32 {
        self.max_light_section
    }

    pub const fn total_sections(&self) -> usize {
        self.total_sections
    }

    pub const fn total_light_sections(&self) -> usize {
        self.total_light_sections
    }

    /// Computes detached static block-light nibbles and emptiness map for `center`
    /// across its bounded radius-2 (`5x5`) neighborhood without mutating any chunk.
    ///
    /// Matches Paper's `StarLightEngine.light` + `BlockStarLightEngine.lightChunk`:
    /// 1. Validates all 25 chunks in `center ± 2` are present with matching section counts.
    /// 2. Computes `center_emptiness_map` (`section.has_only_air()`).
    /// 3. Initializes block nibbles for `center` and clones existing nibbles for
    ///    usable neighbors (`persisted_status >= ChunkStatus::Light && is_light_correct()`),
    ///    applying Paper's `handleEmptySectionChanges(unlit = true)` 3×3×3 init/de-init rules.
    /// 4. Scans `center`'s non-empty sections in Paper's `x | (z << 4) | (y << 8)` order
    ///    (`BlockStarLightEngine.getSources`) and seeds emissive sources.
    /// 5. Pulls boundary light from already-lit radius-1 neighbors (`propagateNeighbourLevels`).
    /// 6. Runs Paper's `performLightIncrease` BFS across the 5×5 workspace.
    pub fn compute_chunk<B, Bi, S, P>(
        &self,
        center: ChunkPos,
        chunks: &HashMap<ChunkPos, ChunkAccess<B, Bi, S>>,
        props: &P,
    ) -> Result<StagedBlockLightPublication, StaticBlockLightError>
    where
        B: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
        Bi: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
        S: Eq + Hash,
        P: BlockLightProperties<B>,
    {
        // Validate complete bounded radius-2 workspace before doing any work.
        for dz in -GENERATED_LIGHT_NEIGHBOR_RADIUS..=GENERATED_LIGHT_NEIGHBOR_RADIUS {
            for dx in -GENERATED_LIGHT_NEIGHBOR_RADIUS..=GENERATED_LIGHT_NEIGHBOR_RADIUS {
                let pos = ChunkPos::new(center.x().wrapping_add(dx), center.z().wrapping_add(dz));
                let chunk = chunks
                    .get(&pos)
                    .ok_or(StaticBlockLightError::MissingNeighborChunk { pos })?;
                let actual = chunk.get_sections().len();
                if actual != self.total_sections {
                    return Err(StaticBlockLightError::SectionCountMismatch {
                        pos,
                        expected: self.total_sections,
                        actual,
                    });
                }
            }
        }

        // Catch any panic inside BlockLightProperties callbacks so callers receive
        // a typed error and no chunk state is ever partially mutated.
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.compute_chunk_inner(center, chunks, props)
        }));

        match result {
            Ok(inner) => inner,
            Err(payload) => Err(StaticBlockLightError::ComputationPanicked {
                message: panic_message(payload),
            }),
        }
    }

    fn compute_chunk_inner<B, Bi, S, P>(
        &self,
        center: ChunkPos,
        chunks: &HashMap<ChunkPos, ChunkAccess<B, Bi, S>>,
        props: &P,
    ) -> Result<StagedBlockLightPublication, StaticBlockLightError>
    where
        B: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
        Bi: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
        S: Eq + Hash,
        P: BlockLightProperties<B>,
    {
        let center_chunk = &chunks[&center];

        // Local 5x5 caches mirroring StarLightEngine's chunk/nibble/emptiness caches.
        let mut nibbles_grid: Vec<Option<Vec<SwmrNibbleArray>>> = vec![None; GRID_SIZE * GRID_SIZE];
        let mut emptiness_grid: Vec<Option<Vec<bool>>> = vec![None; GRID_SIZE * GRID_SIZE];
        let mut neighbor_dirty = [false; GRID_SIZE * GRID_SIZE];

        // Populate caches: center is freshly initialized; neighbors are usable when
        // persisted_status >= LIGHT && is_light_correct (Paper's BlockStarLightEngine.canUseChunk).
        for dz in -GENERATED_LIGHT_NEIGHBOR_RADIUS..=GENERATED_LIGHT_NEIGHBOR_RADIUS {
            for dx in -GENERATED_LIGHT_NEIGHBOR_RADIUS..=GENERATED_LIGHT_NEIGHBOR_RADIUS {
                let idx = grid_index(dx, dz);
                let pos = ChunkPos::new(center.x().wrapping_add(dx), center.z().wrapping_add(dz));
                let chunk = &chunks[&pos];
                if dx == 0 && dz == 0 {
                    nibbles_grid[idx] = Some(filled_empty_light(self.total_light_sections));
                    emptiness_grid[idx] = Some(
                        chunk
                            .get_sections()
                            .iter()
                            .map(|section| section.has_only_air())
                            .collect(),
                    );
                } else if can_use_neighbor_chunk(chunk) {
                    let existing = chunk.block_nibbles();
                    let cloned = if existing.len() == self.total_light_sections {
                        existing.to_vec()
                    } else {
                        filled_empty_light(self.total_light_sections)
                    };
                    nibbles_grid[idx] = Some(cloned);
                    emptiness_grid[idx] = chunk.block_emptiness_map().map(|m| m.to_vec());
                }
            }
        }

        // Paper's `handleEmptySectionChanges(unlit = true)` for center chunk:
        // 1. For every non-empty section in center, init nibbles in its 3x3x3 neighborhood.
        let center_emptiness = emptiness_grid[grid_index(0, 0)]
            .clone()
            .expect("center emptiness map initialized");
        for section_idx in (0..self.total_sections).rev() {
            if center_emptiness[section_idx] {
                continue;
            }
            let section_y = self.min_section.wrapping_add(section_idx as i32);
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let gidx = grid_index(dx, dz);
                    if let Some(nibbles) = nibbles_grid[gidx].as_mut() {
                        for dy in (-1..=1).rev() {
                            let target_y = section_y.wrapping_add(dy);
                            if target_y >= self.min_light_section
                                && target_y <= self.max_light_section
                            {
                                let nidx = (target_y.wrapping_sub(self.min_light_section)) as usize;
                                nibbles[nidx].set_non_null();
                                if dx != 0 || dz != 0 {
                                    neighbor_dirty[gidx] = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. Lazy-init and de-init pass over dx, dz in -1..=1 (Paper lines 21200-21245).
        for dz in -1..=1 {
            for dx in -1..=1 {
                let gidx = grid_index(dx, dz);
                if nibbles_grid[gidx].is_none() {
                    continue;
                }
                let mut neighbours_loaded = true;
                'loaded: for dz2 in -1..=1 {
                    for dx2 in -1..=1 {
                        if emptiness_grid[grid_index(dx + dx2, dz + dz2)].is_none() {
                            neighbours_loaded = false;
                            break 'loaded;
                        }
                    }
                }

                for section_y in (self.min_light_section..=self.max_light_section).rev() {
                    let mut all_empty = true;
                    'empty: for dy2 in -1..=1 {
                        let y = section_y.wrapping_add(dy2);
                        if y < self.min_section || y > self.max_section {
                            continue;
                        }
                        let sec_idx = (y.wrapping_sub(self.min_section)) as usize;
                        for dz2 in -1..=1 {
                            for dx2 in -1..=1 {
                                let nx = dx + dx2;
                                let nz = dz + dz2;
                                if let Some(map) = emptiness_grid[grid_index(nx, nz)].as_ref() {
                                    if !map[sec_idx] {
                                        all_empty = false;
                                        break 'empty;
                                    }
                                } else if nx.abs() <= 1 && nz.abs() <= 1 {
                                    // Paper's `StarLightEngine.handleEmptySectionChanges` falls back
                                    // to `getChunkSection(nx, y, nz).hasOnlyAir()` when the cached
                                    // emptiness map is null (e.g. a neighbor loaded from disk).
                                    let npos = ChunkPos::new(
                                        center.x().wrapping_add(nx),
                                        center.z().wrapping_add(nz),
                                    );
                                    if let Some(nchunk) = chunks.get(&npos)
                                        && can_use_neighbor_chunk(nchunk)
                                        && !nchunk.get_sections()[sec_idx].has_only_air()
                                    {
                                        all_empty = false;
                                        break 'empty;
                                    }
                                }
                            }
                        }
                    }

                    let nidx = (section_y.wrapping_sub(self.min_light_section)) as usize;
                    let nibble = &mut nibbles_grid[gidx].as_mut().unwrap()[nidx];
                    if all_empty && neighbours_loaded {
                        // BlockStarLightEngine.setNibbleNull uses setHidden().
                        if !nibble.is_null_nibble_updating() {
                            nibble.set_hidden();
                            if dx != 0 || dz != 0 {
                                neighbor_dirty[gidx] = true;
                            }
                        }
                    } else if !all_empty {
                        nibble.set_non_null();
                        if dx != 0 || dz != 0 {
                            neighbor_dirty[gidx] = true;
                        }
                    }
                }
            }
        }

        // Paper's `BlockStarLightEngine.getSources`:
        // Iterate section_y from min_section..=max_section, skip has_only_air(),
        // iterate index = 0..4096 in x | (z << 4) | (y << 8) order.
        let mut queue: VecDeque<IncreaseEntry> = VecDeque::new();
        let center_off_x = center.x().wrapping_shl(4);
        let center_off_z = center.z().wrapping_shl(4);

        for section_idx in 0..self.total_sections {
            let section = &center_chunk.get_sections()[section_idx];
            if section.has_only_air() {
                continue;
            }
            let section_y = self.min_section.wrapping_add(section_idx as i32);
            let off_y = section_y.wrapping_shl(4);

            for index in 0..4096usize {
                let lx = (index & 15) as i32;
                let lz = ((index >> 4) & 15) as i32;
                let ly = (index >> 8) as i32;
                let state = section.get_block_state(lx, ly, lz);
                let emitted = props.light_emission(&state) & 0xF;
                if emitted == 0 {
                    continue;
                }

                let wx = center_off_x | lx;
                let wy = off_y | ly;
                let wz = center_off_z | lz;

                if props.is_conditionally_full_opaque(&state) {
                    return Err(StaticBlockLightError::UnsupportedConditionalOcclusion {
                        pos: BlockPos::new(wx, wy, wz),
                    });
                }

                let current = self.get_updating_level(center, &nibbles_grid, wx, wy, wz);
                if emitted <= current {
                    continue;
                }

                self.set_updating_level(
                    center,
                    &mut nibbles_grid,
                    &mut neighbor_dirty,
                    wx,
                    wy,
                    wz,
                    emitted,
                );
                queue.push_back(IncreaseEntry {
                    x: wx,
                    y: wy,
                    z: wz,
                    level: emitted,
                    directions: ALL_DIRECTIONS_BITSET,
                });
            }
        }

        // Paper's `StarLightEngine.propagateNeighbourLevels`:
        // Pull boundary light from already-lit radius-1 neighbors into the center chunk.
        for section_y in (self.min_light_section..=self.max_light_section).rev() {
            let nidx = (section_y.wrapping_sub(self.min_light_section)) as usize;
            let center_nibble_present = nibbles_grid[grid_index(0, 0)]
                .as_ref()
                .is_some_and(|n| !n[nidx].is_null_nibble_updating());
            if !center_nibble_present {
                continue;
            }

            for direction in ONLY_HORIZONTAL_DIRECTIONS {
                let (ndx, _, ndz) = direction.offset();
                let Some(neighbour_nibbles) = nibbles_grid[grid_index(ndx, ndz)].as_ref() else {
                    continue;
                };
                let neighbour_nibble = &neighbour_nibbles[nidx];
                if !neighbour_nibble.is_initialised_updating() {
                    continue;
                }

                let (inc_x, inc_z, start_x, start_z) = if ndx != 0 {
                    let sx = if ndx < 0 {
                        center_off_x.wrapping_sub(1)
                    } else {
                        center_off_x.wrapping_add(16)
                    };
                    (0, 1, sx, center_off_z)
                } else {
                    let sz = if ndz < 0 {
                        center_off_z.wrapping_sub(1)
                    } else {
                        center_off_z.wrapping_add(16)
                    };
                    (1, 0, center_off_x, sz)
                };

                let propagate_dir_bit = direction.opposite().bit();
                let base_y = section_y.wrapping_shl(4);
                for dy in 0..16 {
                    let curr_y = base_y | dy;
                    let mut curr_x = start_x;
                    let mut curr_z = start_z;
                    for _ in 0..16 {
                        let local_idx =
                            ((curr_x & 15) | ((curr_z & 15) << 4) | ((curr_y & 15) << 8)) as usize;
                        let level = neighbour_nibble.get_updating_index(local_idx);
                        if level > 1 {
                            if let Some(state) =
                                self.get_block_state(center, chunks, curr_x, curr_y, curr_z)
                                && props.is_conditionally_full_opaque(&state)
                            {
                                return Err(
                                    StaticBlockLightError::UnsupportedConditionalOcclusion {
                                        pos: BlockPos::new(curr_x, curr_y, curr_z),
                                    },
                                );
                            }
                            queue.push_back(IncreaseEntry {
                                x: curr_x,
                                y: curr_y,
                                z: curr_z,
                                level,
                                directions: propagate_dir_bit,
                            });
                        }
                        curr_x = curr_x.wrapping_add(inc_x);
                        curr_z = curr_z.wrapping_add(inc_z);
                    }
                }
            }
        }

        // Paper's `StarLightEngine.performLightIncrease` (block light).
        while let Some(entry) = queue.pop_front() {
            for dir in AXIS_DIRECTIONS {
                if (entry.directions & dir.bit()) == 0 {
                    continue;
                }
                let (ox, oy, oz) = dir.offset();
                let nx = entry.x.wrapping_add(ox);
                let ny = entry.y.wrapping_add(oy);
                let nz = entry.z.wrapping_add(oz);

                let ncx = nx >> 4;
                let ncy = ny >> 4;
                let ncz = nz >> 4;
                let dx = ncx.wrapping_sub(center.x());
                let dz = ncz.wrapping_sub(center.z());
                if dx.abs() > GENERATED_LIGHT_NEIGHBOR_RADIUS
                    || dz.abs() > GENERATED_LIGHT_NEIGHBOR_RADIUS
                    || ncy < self.min_light_section
                    || ncy > self.max_light_section
                {
                    continue;
                }

                let gidx = grid_index(dx, dz);
                let nidx = (ncy.wrapping_sub(self.min_light_section)) as usize;
                let Some(nibbles) = nibbles_grid[gidx].as_mut() else {
                    continue;
                };
                if nibbles[nidx].is_null_nibble_updating() {
                    continue;
                }

                let local_idx = ((nx & 15) | ((nz & 15) << 4) | ((ny & 15) << 8)) as usize;
                let current_level = nibbles[nidx].get_updating_index(local_idx);
                if current_level >= entry.level.wrapping_sub(1) {
                    continue;
                }

                let opacity = if ncy >= self.min_section && ncy <= self.max_section {
                    let Some(state) = self.get_block_state(center, chunks, nx, ny, nz) else {
                        continue;
                    };
                    if props.is_conditionally_full_opaque(&state) {
                        return Err(StaticBlockLightError::UnsupportedConditionalOcclusion {
                            pos: BlockPos::new(nx, ny, nz),
                        });
                    }
                    props.light_dampening(&state)
                } else {
                    // Padding sections above max_section / below min_section are empty air.
                    0
                };

                let target_level = entry.level.wrapping_sub(opacity.max(1));
                if target_level <= current_level {
                    continue;
                }

                nibbles[nidx].set_index(local_idx, target_level);
                if dx != 0 || dz != 0 {
                    neighbor_dirty[gidx] = true;
                }

                if target_level > 1 {
                    queue.push_back(IncreaseEntry {
                        x: nx,
                        y: ny,
                        z: nz,
                        level: target_level,
                        directions: dir.everything_but_opposite(),
                    });
                }
            }
        }

        let center_nibbles = nibbles_grid[grid_index(0, 0)]
            .take()
            .expect("center nibbles present");

        let mut neighbor_nibbles = Vec::new();
        for dz in -GENERATED_LIGHT_NEIGHBOR_RADIUS..=GENERATED_LIGHT_NEIGHBOR_RADIUS {
            for dx in -GENERATED_LIGHT_NEIGHBOR_RADIUS..=GENERATED_LIGHT_NEIGHBOR_RADIUS {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let gidx = grid_index(dx, dz);
                if neighbor_dirty[gidx]
                    && let Some(nibbles) = nibbles_grid[gidx].take()
                {
                    let pos =
                        ChunkPos::new(center.x().wrapping_add(dx), center.z().wrapping_add(dz));
                    neighbor_nibbles.push((pos, nibbles));
                }
            }
        }

        Ok(StagedBlockLightPublication {
            center,
            center_nibbles,
            center_emptiness_map: center_emptiness,
            neighbor_nibbles,
        })
    }

    fn get_block_state<B, Bi, S>(
        &self,
        center: ChunkPos,
        chunks: &HashMap<ChunkPos, ChunkAccess<B, Bi, S>>,
        wx: i32,
        wy: i32,
        wz: i32,
    ) -> Option<B>
    where
        B: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
        Bi: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
        S: Eq + Hash,
    {
        let cy = wy >> 4;
        if cy < self.min_section || cy > self.max_section {
            return None;
        }
        let pos = ChunkPos::new(wx >> 4, wz >> 4);
        if (pos.x().wrapping_sub(center.x())).abs() > GENERATED_LIGHT_NEIGHBOR_RADIUS
            || (pos.z().wrapping_sub(center.z())).abs() > GENERATED_LIGHT_NEIGHBOR_RADIUS
        {
            return None;
        }
        let chunk = chunks.get(&pos)?;
        let sec_idx = (cy.wrapping_sub(self.min_section)) as usize;
        let section = chunk.get_sections().get(sec_idx)?;
        Some(section.get_block_state(wx & 15, wy & 15, wz & 15))
    }

    fn get_updating_level(
        &self,
        center: ChunkPos,
        nibbles_grid: &[Option<Vec<SwmrNibbleArray>>],
        wx: i32,
        wy: i32,
        wz: i32,
    ) -> i32 {
        let cy = wy >> 4;
        if cy < self.min_light_section || cy > self.max_light_section {
            return 0;
        }
        let dx = (wx >> 4).wrapping_sub(center.x());
        let dz = (wz >> 4).wrapping_sub(center.z());
        if dx.abs() > GENERATED_LIGHT_NEIGHBOR_RADIUS || dz.abs() > GENERATED_LIGHT_NEIGHBOR_RADIUS
        {
            return 0;
        }
        let Some(nibbles) = nibbles_grid[grid_index(dx, dz)].as_ref() else {
            return 0;
        };
        let nidx = (cy.wrapping_sub(self.min_light_section)) as usize;
        let local_idx = ((wx & 15) | ((wz & 15) << 4) | ((wy & 15) << 8)) as usize;
        nibbles[nidx].get_updating_index(local_idx)
    }

    #[allow(clippy::too_many_arguments)]
    fn set_updating_level(
        &self,
        center: ChunkPos,
        nibbles_grid: &mut [Option<Vec<SwmrNibbleArray>>],
        neighbor_dirty: &mut [bool; GRID_SIZE * GRID_SIZE],
        wx: i32,
        wy: i32,
        wz: i32,
        level: i32,
    ) {
        let cy = wy >> 4;
        if cy < self.min_light_section || cy > self.max_light_section {
            return;
        }
        let dx = (wx >> 4).wrapping_sub(center.x());
        let dz = (wz >> 4).wrapping_sub(center.z());
        if dx.abs() > GENERATED_LIGHT_NEIGHBOR_RADIUS || dz.abs() > GENERATED_LIGHT_NEIGHBOR_RADIUS
        {
            return;
        }
        let gidx = grid_index(dx, dz);
        let Some(nibbles) = nibbles_grid[gidx].as_mut() else {
            return;
        };
        let nidx = (cy.wrapping_sub(self.min_light_section)) as usize;
        let local_idx = ((wx & 15) | ((wz & 15) << 4) | ((wy & 15) << 8)) as usize;
        nibbles[nidx].set_index(local_idx, level);
        if dx != 0 || dz != 0 {
            neighbor_dirty[gidx] = true;
        }
    }
}

/// Computes static block light and atomically publishes sky + block nibbles,
/// emptiness maps, and `light_correct = true` across the bounded workspace.
///
/// If channel capability validation, workspace validation, block-light computation,
/// sky publication validation, or a `BlockLightProperties` callback fails or panics,
/// **no** chunk in `chunks` is mutated.
pub fn publish_generated_light_atomic<B, Bi, S, P>(
    height: &dyn LevelHeightAccessor,
    capability: LightChannelCapability,
    policy: DimensionLightPolicy,
    center: ChunkPos,
    chunks: &mut HashMap<ChunkPos, ChunkAccess<B, Bi, S>>,
    props: &P,
    staged_sky: Option<StagedSkyLightPublication>,
) -> Result<(), StaticBlockLightError>
where
    B: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
    Bi: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
    S: Eq + Hash,
    P: BlockLightProperties<B>,
{
    capability.validate(policy)?;

    let engine = StaticBlockLightEngine::new(height);

    // Validate sky publication upfront before mutating anything.
    let mut sky_to_commit = if capability.has_sky_light {
        let sky = staged_sky.ok_or(StaticBlockLightError::MissingSkyPublication)?;
        if sky.center_nibbles.len() != engine.total_light_sections() {
            return Err(StaticBlockLightError::SkyNibbleCountMismatch {
                expected: engine.total_light_sections(),
                actual: sky.center_nibbles.len(),
            });
        }
        if sky.center_emptiness_map.len() != engine.total_sections() {
            return Err(StaticBlockLightError::SkyEmptinessCountMismatch {
                expected: engine.total_sections(),
                actual: sky.center_emptiness_map.len(),
            });
        }
        for (pos, nibbles) in &sky.neighbor_nibbles {
            if !chunks.contains_key(pos) {
                return Err(StaticBlockLightError::MissingNeighborChunk { pos: *pos });
            }
            if nibbles.len() != engine.total_light_sections() {
                return Err(StaticBlockLightError::SkyNibbleCountMismatch {
                    expected: engine.total_light_sections(),
                    actual: nibbles.len(),
                });
            }
        }
        Some(sky)
    } else {
        None
    };

    // Compute detached block-light publication (pure read of `chunks`).
    let mut block_to_commit = if capability.has_block_light {
        Some(engine.compute_chunk(center, chunks, props)?)
    } else {
        // Still verify center exists even if block light is disabled under Custom policy.
        if !chunks.contains_key(&center) {
            return Err(StaticBlockLightError::MissingNeighborChunk { pos: center });
        }
        None
    };

    // --- Infallible atomic commit phase ---
    if let Some(sky) = sky_to_commit.as_mut() {
        for nibble in &mut sky.center_nibbles {
            nibble.update_visible();
        }
        for (_, nibbles) in &mut sky.neighbor_nibbles {
            for nibble in nibbles {
                nibble.update_visible();
            }
        }
    }

    if let Some(block) = block_to_commit.as_mut() {
        for nibble in &mut block.center_nibbles {
            nibble.update_visible();
        }
        for (_, nibbles) in &mut block.neighbor_nibbles {
            for nibble in nibbles {
                nibble.update_visible();
            }
        }
    }

    if let Some(sky) = sky_to_commit {
        for (pos, nibbles) in sky.neighbor_nibbles {
            if let Some(chunk) = chunks.get_mut(&pos) {
                chunk.set_sky_nibbles(nibbles);
                chunk.mark_unsaved();
            }
        }
        let center_chunk = chunks.get_mut(&center).expect("validated center exists");
        center_chunk.set_sky_nibbles(sky.center_nibbles);
        center_chunk.set_sky_emptiness_map(Some(sky.center_emptiness_map));
    }

    if let Some(block) = block_to_commit {
        for (pos, nibbles) in block.neighbor_nibbles {
            if let Some(chunk) = chunks.get_mut(&pos) {
                chunk.set_block_nibbles(nibbles);
                chunk.mark_unsaved();
            }
        }
        let center_chunk = chunks.get_mut(&center).expect("validated center exists");
        center_chunk.set_block_nibbles(block.center_nibbles);
        center_chunk.set_block_emptiness_map(Some(block.center_emptiness_map));
    }

    let center_chunk = chunks.get_mut(&center).expect("validated center exists");
    if !center_chunk
        .get_persisted_status()
        .is_or_after(ChunkStatus::Light)
    {
        center_chunk.set_persisted_status(ChunkStatus::Light);
    }
    center_chunk.set_light_correct(true);

    Ok(())
}

/// Paper's `StarLightInterface.getBlockLightValue(BlockPos, ChunkAccess)`.
pub fn get_block_light_value<B, Bi, S>(
    height: &dyn LevelHeightAccessor,
    has_block_light: bool,
    pos: BlockPos,
    chunk: Option<&ChunkAccess<B, Bi, S>>,
) -> i32
where
    B: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
    Bi: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
    S: Eq + Hash,
{
    if !has_block_light {
        return 0;
    }
    let y = pos.get_y();
    let cy = y >> 4;
    let min_light_section = height.get_min_section_y().wrapping_sub(1);
    let max_light_section = height.get_max_section_y().wrapping_add(1);
    if cy < min_light_section || cy > max_light_section {
        return 0;
    }
    let Some(chunk) = chunk else {
        return 0;
    };
    let nidx = (cy.wrapping_sub(min_light_section)) as usize;
    chunk
        .block_nibbles()
        .get(nidx)
        .map_or(0, |nibble| nibble.get_visible(pos.get_x(), y, pos.get_z()))
}

/// Paper's `StarLightInterface.blockReader.getDataLayerData(SectionPos)`.
pub fn get_block_data_layer_data<B, Bi, S>(
    height: &dyn LevelHeightAccessor,
    has_block_light: bool,
    pos: SectionPos,
    chunk: Option<&ChunkAccess<B, Bi, S>>,
) -> Option<DataLayer>
where
    B: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
    Bi: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
    S: Eq + Hash,
{
    if !has_block_light {
        return None;
    }
    let chunk = chunk?;
    let min_light_section = height.get_min_section_y().wrapping_sub(1);
    let max_light_section = height.get_max_section_y().wrapping_add(1);
    let sy = pos.get_y();
    if sy < min_light_section || sy > max_light_section {
        return None;
    }
    let nidx = (sy.wrapping_sub(min_light_section)) as usize;
    chunk.block_nibbles().get(nidx)?.to_vanilla_nibble()
}

/// Paper's `StarLightEngine.getFilledEmptyLight(int totalLightSections)`.
pub fn filled_empty_light(total_light_sections: usize) -> Vec<SwmrNibbleArray> {
    (0..total_light_sections)
        .map(|_| SwmrNibbleArray::new_with_bytes_and_null(None, true))
        .collect()
}

#[inline]
fn grid_index(dx: i32, dz: i32) -> usize {
    let gx = (dx + GENERATED_LIGHT_NEIGHBOR_RADIUS) as usize;
    let gz = (dz + GENERATED_LIGHT_NEIGHBOR_RADIUS) as usize;
    gz * GRID_SIZE + gx
}

/// Paper's `BlockStarLightEngine.canUseChunk` (server side).
#[inline]
fn can_use_neighbor_chunk<B, Bi, S>(chunk: &ChunkAccess<B, Bi, S>) -> bool
where
    B: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
    Bi: Clone + PartialEq + Send + Sync + fmt::Debug + 'static,
    S: Eq + Hash,
{
    chunk.get_persisted_status().is_or_after(ChunkStatus::Light) && chunk.is_light_correct()
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::palette::GlobalIdMap;
    use crate::chunk::paletted_container_factory::PalettedContainerFactory;
    use crate::chunk::strategy::Strategy;
    use crate::chunk::upgrade_data::UpgradeData;
    use crate::level::height_accessor::create as create_accessor;
    use crate::levelgen::heightmap::StateFlags;

    // Test block IDs:
    const AIR: u8 = 0;
    const STONE: u8 = 1; // dampening 15, emission 0
    const TORCH: u8 = 2; // dampening 0, emission 14
    const GLOWSTONE: u8 = 3; // dampening 15, emission 15
    const REDSTONE_TORCH: u8 = 4; // dampening 0, emission 7
    const WATER: u8 = 5; // dampening 1, emission 0
    const TINTED_FILTER: u8 = 6; // dampening 3, emission 0
    const CONDITIONAL_SLAB: u8 = 7; // conditionally full-opaque (issue #710 boundary)
    const PANIC_BLOCK: u8 = 8; // triggers panic in TestProps

    #[derive(Clone, Copy)]
    struct TestGlobalMap;
    impl GlobalIdMap<u8> for TestGlobalMap {
        fn get_id(&self, value: &u8) -> i32 {
            *value as i32
        }
        fn by_id_or_throw(&self, id: i32) -> u8 {
            id as u8
        }
        fn size(&self) -> i32 {
            256
        }
        fn by_id(&self, id: i32) -> Option<u8> {
            Some(id as u8)
        }
        fn clone_box(&self) -> Box<dyn GlobalIdMap<u8> + Send + Sync> {
            Box::new(*self)
        }
    }

    struct TestProps;
    impl BlockLightProperties<u8> for TestProps {
        fn light_emission(&self, state: &u8) -> i32 {
            match *state {
                TORCH => 14,
                GLOWSTONE => 15,
                REDSTONE_TORCH => 7,
                PANIC_BLOCK => panic!("simulated property callback panic"),
                _ => 0,
            }
        }

        fn light_dampening(&self, state: &u8) -> i32 {
            match *state {
                STONE | GLOWSTONE => 15,
                WATER => 1,
                TINTED_FILTER => 3,
                PANIC_BLOCK => panic!("simulated dampening callback panic"),
                _ => 0,
            }
        }

        fn is_conditionally_full_opaque(&self, state: &u8) -> bool {
            *state == CONDITIONAL_SLAB
        }
    }

    fn test_flags(s: &u8) -> StateFlags {
        StateFlags {
            is_air: *s == AIR,
            blocks_motion: *s != AIR,
            has_fluid: *s == WATER,
            is_leaves: false,
        }
    }

    fn make_empty_chunk(
        pos: ChunkPos,
        min_y: i32,
        height: i32,
    ) -> ChunkAccess<u8, u8, &'static str> {
        let accessor = create_accessor(min_y, height);
        let sections_count = accessor.get_sections_count() as usize;
        let factory = PalettedContainerFactory::new(
            Strategy::create_for_block_states(Box::new(TestGlobalMap)),
            AIR,
            Strategy::create_for_biomes(Box::new(TestGlobalMap)),
            0u8,
        );
        let mut chunk = ChunkAccess::new(
            pos,
            UpgradeData::empty(sections_count),
            accessor,
            &factory,
            0,
            None,
            &test_flags,
        );
        chunk.set_persisted_status(ChunkStatus::InitializeLight);
        chunk
    }

    fn set_block(
        chunk: &mut ChunkAccess<u8, u8, &'static str>,
        min_section: i32,
        lx: i32,
        wy: i32,
        lz: i32,
        block: u8,
    ) {
        let sec_idx = ((wy >> 4) - min_section) as usize;
        let ly = wy & 15;
        let section = chunk.get_section_mut(sec_idx);
        section.set_block_state(
            lx,
            ly,
            lz,
            block,
            &|s: &u8| *s == AIR,
            &|_| false,
            &|s: &u8| *s != WATER,
            &|_| false,
            &|_| false,
        );
    }

    fn make_workspace(
        center: ChunkPos,
        min_y: i32,
        height: i32,
    ) -> HashMap<ChunkPos, ChunkAccess<u8, u8, &'static str>> {
        let mut chunks = HashMap::new();
        for dz in -GENERATED_LIGHT_NEIGHBOR_RADIUS..=GENERATED_LIGHT_NEIGHBOR_RADIUS {
            for dx in -GENERATED_LIGHT_NEIGHBOR_RADIUS..=GENERATED_LIGHT_NEIGHBOR_RADIUS {
                let pos = ChunkPos::new(center.x() + dx, center.z() + dz);
                chunks.insert(pos, make_empty_chunk(pos, min_y, height));
            }
        }
        chunks
    }

    fn make_staged_sky(height: &dyn LevelHeightAccessor, level: i32) -> StagedSkyLightPublication {
        let total_sections = height.get_sections_count() as usize;
        let total_light_sections = total_sections + 2;
        let mut nibbles = filled_empty_light(total_light_sections);
        for nibble in &mut nibbles {
            nibble.set_non_null();
            if level == 15 {
                nibble.set_full();
            } else if level > 0 {
                nibble.set(0, 0, 0, level);
            }
        }
        StagedSkyLightPublication {
            center_nibbles: nibbles,
            center_emptiness_map: vec![true; total_sections],
            neighbor_nibbles: Vec::new(),
        }
    }

    #[test]
    fn emissive_source_initialization_and_manhattan_decay() {
        let height = create_accessor(0, 64); // 4 sections (y = 0..64)
        let center = ChunkPos::ZERO;
        let mut chunks = make_workspace(center, 0, 64);
        set_block(chunks.get_mut(&center).unwrap(), 0, 8, 20, 8, TORCH);

        publish_generated_light_atomic(
            &height,
            LightChannelCapability::block_light_only(),
            DimensionLightPolicy::BlockLightOnly,
            center,
            &mut chunks,
            &TestProps,
            None,
        )
        .expect("block-light generation succeeds");

        let chunk = &chunks[&center];
        assert!(chunk.is_light_correct());
        assert_eq!(
            get_block_light_value(&height, true, BlockPos::new(8, 20, 8), Some(chunk)),
            14
        );
        // Exact Manhattan decay in air: 14 - (|dx| + |dy| + |dz|).
        assert_eq!(
            get_block_light_value(&height, true, BlockPos::new(9, 20, 8), Some(chunk)),
            13
        );
        assert_eq!(
            get_block_light_value(&height, true, BlockPos::new(8, 23, 8), Some(chunk)),
            11
        );
        assert_eq!(
            get_block_light_value(&height, true, BlockPos::new(11, 22, 10), Some(chunk)),
            14 - (3 + 2 + 2)
        );
        // Out of range (distance 14) is 0.
        assert_eq!(
            get_block_light_value(&height, true, BlockPos::new(8, 34, 8), Some(chunk)),
            0
        );
        // Reading with has_block_light = false always returns 0 / None.
        assert_eq!(
            get_block_light_value(&height, false, BlockPos::new(8, 20, 8), Some(chunk)),
            0
        );
        assert!(
            get_block_data_layer_data(&height, true, SectionPos::of(0, 1, 0), Some(chunk))
                .is_some()
        );
        assert!(
            get_block_data_layer_data(&height, false, SectionPos::of(0, 1, 0), Some(chunk))
                .is_none()
        );
    }

    #[test]
    fn overlapping_sources_keep_brighter_level() {
        let height = create_accessor(0, 64);
        let center = ChunkPos::ZERO;
        let mut chunks = make_workspace(center, 0, 64);
        // Glowstone (15) at (6, 20, 8) and redstone torch (7) at (8, 20, 8).
        // Glowstone arrives at (8, 20, 8) with level 13 > 7, so brighter wins.
        set_block(chunks.get_mut(&center).unwrap(), 0, 6, 20, 8, GLOWSTONE);
        set_block(
            chunks.get_mut(&center).unwrap(),
            0,
            8,
            20,
            8,
            REDSTONE_TORCH,
        );

        publish_generated_light_atomic(
            &height,
            LightChannelCapability::block_light_only(),
            DimensionLightPolicy::BlockLightOnly,
            center,
            &mut chunks,
            &TestProps,
            None,
        )
        .unwrap();

        let chunk = &chunks[&center];
        assert_eq!(
            get_block_light_value(&height, true, BlockPos::new(6, 20, 8), Some(chunk)),
            15
        );
        assert_eq!(
            get_block_light_value(&height, true, BlockPos::new(8, 20, 8), Some(chunk)),
            13
        );
        // Also verify dimmer source placed earlier in scan order is overwritten by brighter.
        assert_eq!(
            get_block_light_value(&height, true, BlockPos::new(9, 20, 8), Some(chunk)),
            12
        );
    }

    #[test]
    fn boundary_and_bounded_workspace_propagation_flows_both_ways() {
        let height = create_accessor(0, 64);
        let center = ChunkPos::ZERO;
        let east = ChunkPos::new(1, 0);
        let west = ChunkPos::new(-1, 0);
        let mut chunks = make_workspace(center, 0, 64);

        // First light `east` (with a torch right on its western border x=16) so it is
        // already LIGHT + light_correct when `center` is generated.
        set_block(chunks.get_mut(&east).unwrap(), 0, 0, 20, 8, TORCH);
        let mut east_workspace = make_workspace(east, 0, 64);
        set_block(east_workspace.get_mut(&east).unwrap(), 0, 0, 20, 8, TORCH);
        publish_generated_light_atomic(
            &height,
            LightChannelCapability::block_light_only(),
            DimensionLightPolicy::BlockLightOnly,
            east,
            &mut east_workspace,
            &TestProps,
            None,
        )
        .unwrap();
        chunks.insert(east, east_workspace.remove(&east).unwrap());

        // Also mark `west` as already LIGHT + light_correct (empty dark neighbor)
        // so `center`'s western-edge torch propagates into `west`.
        let west_chunk = chunks.get_mut(&west).unwrap();
        west_chunk.set_persisted_status(ChunkStatus::Light);
        west_chunk.set_block_nibbles(filled_empty_light(6));
        west_chunk.set_block_emptiness_map(Some(vec![true; 4]));
        west_chunk.set_light_correct(true);

        // Place a torch on `center`'s western border (x=0, y=20, z=8).
        set_block(chunks.get_mut(&center).unwrap(), 0, 0, 20, 8, TORCH);

        publish_generated_light_atomic(
            &height,
            LightChannelCapability::block_light_only(),
            DimensionLightPolicy::BlockLightOnly,
            center,
            &mut chunks,
            &TestProps,
            None,
        )
        .unwrap();

        // 1. Center -> West neighbor propagation across chunk border (not center-only!).
        assert_eq!(
            get_block_light_value(
                &height,
                true,
                BlockPos::new(0, 20, 8),
                Some(&chunks[&center])
            ),
            14
        );
        assert_eq!(
            get_block_light_value(
                &height,
                true,
                BlockPos::new(-1, 20, 8),
                Some(&chunks[&west])
            ),
            13
        );
        assert_eq!(
            get_block_light_value(
                &height,
                true,
                BlockPos::new(-5, 20, 8),
                Some(&chunks[&west])
            ),
            9
        );

        // 2. East neighbor -> Center propagation via `propagateNeighbourLevels`.
        assert_eq!(
            get_block_light_value(
                &height,
                true,
                BlockPos::new(16, 20, 8),
                Some(&chunks[&east])
            ),
            14
        );
        assert_eq!(
            get_block_light_value(
                &height,
                true,
                BlockPos::new(15, 20, 8),
                Some(&chunks[&center])
            ),
            13
        );
        assert_eq!(
            get_block_light_value(
                &height,
                true,
                BlockPos::new(12, 20, 8),
                Some(&chunks[&center])
            ),
            10
        );
    }

    #[test]
    fn zero_emission_controls_leave_chunk_dark_with_accurate_emptiness_map() {
        let height = create_accessor(0, 64);
        let center = ChunkPos::ZERO;
        let mut chunks = make_workspace(center, 0, 64);
        // Non-emissive stone in section 1 only.
        set_block(chunks.get_mut(&center).unwrap(), 0, 4, 20, 4, STONE);

        publish_generated_light_atomic(
            &height,
            LightChannelCapability::block_light_only(),
            DimensionLightPolicy::BlockLightOnly,
            center,
            &mut chunks,
            &TestProps,
            None,
        )
        .unwrap();

        let chunk = &chunks[&center];
        assert!(chunk.is_light_correct());
        assert_eq!(
            chunk.block_emptiness_map(),
            Some(&[true, false, true, true][..])
        );
        for y in 0..64 {
            assert_eq!(
                get_block_light_value(&height, true, BlockPos::new(4, y, 4), Some(chunk)),
                0
            );
        }
        // Section 1 has non-air, so nibbles for sections 0, 1, 2 (indices 1, 2, 3) are
        // uninitialised (non-null, all zero), while section 4 padding (index 5) stays null.
        assert!(chunk.block_nibbles()[2].is_uninitialised_visible());
        assert!(chunk.block_nibbles()[5].is_null_nibble_visible());
    }

    #[test]
    fn opaque_and_attenuating_blocks_respect_paper_dampening() {
        let height = create_accessor(0, 64);
        let center = ChunkPos::ZERO;
        let mut chunks = make_workspace(center, 0, 64);
        let c = chunks.get_mut(&center).unwrap();
        set_block(c, 0, 8, 20, 8, TORCH); // 14
        // Wall of stone completely enclosing (9, 20, 8) on the +X side via a 3x3 wall at x=9.
        for dy in -1..=1 {
            for dz in -1..=1 {
                set_block(c, 0, 9, 20 + dy, 8 + dz, STONE);
            }
        }
        // Filter block (dampening = 3) at (7, 20, 8).
        set_block(c, 0, 7, 20, 8, TINTED_FILTER);

        publish_generated_light_atomic(
            &height,
            LightChannelCapability::block_light_only(),
            DimensionLightPolicy::BlockLightOnly,
            center,
            &mut chunks,
            &TestProps,
            None,
        )
        .unwrap();

        let chunk = &chunks[&center];
        // Stone absorbs all light (dampening 15 -> 14 - 15 < 0).
        assert_eq!(
            get_block_light_value(&height, true, BlockPos::new(9, 20, 8), Some(chunk)),
            0
        );
        // Tinted filter attenuates by 3 directly from torch (14 - 3 = 11),
        // unless light wraps around through adjacent air:
        // (8,20,8)=14 -> (8,20,9)=13 -> (7,20,9)=12 -> (7,20,8)=12-3=9 < 11, so 11 wins.
        assert_eq!(
            get_block_light_value(&height, true, BlockPos::new(7, 20, 8), Some(chunk)),
            11
        );
    }

    #[test]
    fn missing_capability_returns_typed_refusal_without_mutation() {
        let height = create_accessor(0, 64);
        let center = ChunkPos::ZERO;
        let mut chunks = make_workspace(center, 0, 64);
        set_block(chunks.get_mut(&center).unwrap(), 0, 8, 20, 8, TORCH);

        let err = publish_generated_light_atomic(
            &height,
            LightChannelCapability {
                has_sky_light: true,
                has_block_light: false,
            },
            DimensionLightPolicy::NormalOverworld,
            center,
            &mut chunks,
            &TestProps,
            Some(make_staged_sky(&height, 15)),
        )
        .expect_err("normal Overworld must refuse has_block_light=false");

        assert_eq!(
            err,
            StaticBlockLightError::UnsupportedLightChannel {
                require_sky_light: true,
                require_block_light: true,
                has_sky_light: true,
                has_block_light: false,
            }
        );
        // Zero mutation on refusal.
        let chunk = &chunks[&center];
        assert!(!chunk.is_light_correct());
        assert!(
            chunk
                .block_nibbles()
                .iter()
                .all(|n| n.is_null_nibble_visible())
        );
        assert!(
            chunk
                .sky_nibbles()
                .iter()
                .all(|n| n.is_null_nibble_visible())
        );
        assert_eq!(chunk.block_emptiness_map(), None);
        assert_eq!(chunk.sky_emptiness_map(), None);
    }

    #[test]
    fn conditional_face_occlusion_returns_typed_refusal_without_mutation() {
        let height = create_accessor(0, 64);
        let center = ChunkPos::ZERO;
        let mut chunks = make_workspace(center, 0, 64);
        let c = chunks.get_mut(&center).unwrap();
        set_block(c, 0, 8, 20, 8, TORCH);
        set_block(c, 0, 9, 20, 8, CONDITIONAL_SLAB);

        let err = publish_generated_light_atomic(
            &height,
            LightChannelCapability::normal_overworld(),
            DimensionLightPolicy::NormalOverworld,
            center,
            &mut chunks,
            &TestProps,
            Some(make_staged_sky(&height, 15)),
        )
        .expect_err("conditional face-shape occlusion must be refused honestly");

        assert_eq!(
            err,
            StaticBlockLightError::UnsupportedConditionalOcclusion {
                pos: BlockPos::new(9, 20, 8),
            }
        );
        // Neither sky nor block nor light_correct was mutated.
        let chunk = &chunks[&center];
        assert!(!chunk.is_light_correct());
        assert!(
            chunk
                .block_nibbles()
                .iter()
                .all(|n| n.is_null_nibble_visible())
        );
        assert!(
            chunk
                .sky_nibbles()
                .iter()
                .all(|n| n.is_null_nibble_visible())
        );
    }

    #[test]
    fn atomic_sky_and_block_publication_commits_together_or_not_at_all() {
        let height = create_accessor(0, 64);
        let center = ChunkPos::ZERO;
        let mut chunks = make_workspace(center, 0, 64);
        set_block(chunks.get_mut(&center).unwrap(), 0, 8, 20, 8, TORCH);

        // 1. Invalid sky publication (wrong nibble count) aborts without committing block light.
        let bad_sky = StagedSkyLightPublication {
            center_nibbles: filled_empty_light(2),
            center_emptiness_map: vec![true; 4],
            neighbor_nibbles: Vec::new(),
        };
        let err = publish_generated_light_atomic(
            &height,
            LightChannelCapability::normal_overworld(),
            DimensionLightPolicy::NormalOverworld,
            center,
            &mut chunks,
            &TestProps,
            Some(bad_sky),
        )
        .expect_err("mismatched sky nibbles must abort");
        assert_eq!(
            err,
            StaticBlockLightError::SkyNibbleCountMismatch {
                expected: 6,
                actual: 2,
            }
        );
        assert!(!chunks[&center].is_light_correct());
        assert!(
            chunks[&center]
                .block_nibbles()
                .iter()
                .all(|n| n.is_null_nibble_visible())
        );
        assert!(
            chunks[&center]
                .sky_nibbles()
                .iter()
                .all(|n| n.is_null_nibble_visible())
        );

        // 2. Valid retry commits sky + block + emptiness maps + light_correct together.
        publish_generated_light_atomic(
            &height,
            LightChannelCapability::normal_overworld(),
            DimensionLightPolicy::NormalOverworld,
            center,
            &mut chunks,
            &TestProps,
            Some(make_staged_sky(&height, 15)),
        )
        .expect("retry with valid sky publication succeeds");

        let chunk = &chunks[&center];
        assert!(chunk.is_light_correct());
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert_eq!(chunk.sky_nibbles().len(), 6);
        assert_eq!(chunk.block_nibbles().len(), 6);
        assert_eq!(chunk.sky_emptiness_map(), Some(&[true; 4][..]));
        assert_eq!(
            chunk.block_emptiness_map(),
            Some(&[true, false, true, true][..])
        );
        assert_eq!(
            get_block_light_value(&height, true, BlockPos::new(8, 20, 8), Some(chunk)),
            14
        );
        assert_eq!(chunk.sky_nibbles()[2].get_visible(8, 20, 8), 15);
    }

    #[test]
    fn panic_and_missing_neighbor_preserve_all_chunks_for_safe_retry() {
        let height = create_accessor(0, 64);
        let center = ChunkPos::ZERO;
        let west = ChunkPos::new(-1, 0);
        let mut chunks = make_workspace(center, 0, 64);

        // Pre-populate west as already lit so we can prove even neighbor chunks are untouched.
        {
            let w = chunks.get_mut(&west).unwrap();
            w.set_persisted_status(ChunkStatus::Light);
            w.set_block_nibbles(filled_empty_light(6));
            w.set_block_emptiness_map(Some(vec![true; 4]));
            w.set_light_correct(true);
        }

        // Case A: Panic inside BlockLightProperties callback.
        set_block(chunks.get_mut(&center).unwrap(), 0, 0, 20, 8, TORCH);
        set_block(chunks.get_mut(&center).unwrap(), 0, 1, 20, 8, PANIC_BLOCK);

        let err = publish_generated_light_atomic(
            &height,
            LightChannelCapability::block_light_only(),
            DimensionLightPolicy::BlockLightOnly,
            center,
            &mut chunks,
            &TestProps,
            None,
        )
        .expect_err("panicking callback must return typed ComputationPanicked");
        assert!(matches!(
            err,
            StaticBlockLightError::ComputationPanicked { .. }
        ));
        assert!(!chunks[&center].is_light_correct());
        assert!(
            chunks[&center]
                .block_nibbles()
                .iter()
                .all(|n| n.is_null_nibble_visible())
        );
        assert_eq!(
            get_block_light_value(
                &height,
                true,
                BlockPos::new(-1, 20, 8),
                Some(&chunks[&west])
            ),
            0
        );

        // Case B: Missing neighbor in radius-2 workspace.
        set_block(chunks.get_mut(&center).unwrap(), 0, 1, 20, 8, AIR);
        let removed_pos = ChunkPos::new(2, 2);
        let removed_chunk = chunks.remove(&removed_pos).unwrap();
        let err = publish_generated_light_atomic(
            &height,
            LightChannelCapability::block_light_only(),
            DimensionLightPolicy::BlockLightOnly,
            center,
            &mut chunks,
            &TestProps,
            None,
        )
        .expect_err("missing radius-2 neighbor must fail");
        assert_eq!(
            err,
            StaticBlockLightError::MissingNeighborChunk { pos: removed_pos }
        );
        assert!(!chunks[&center].is_light_correct());

        // Case C: Restore missing neighbor and retry -> succeeds cleanly.
        chunks.insert(removed_pos, removed_chunk);
        publish_generated_light_atomic(
            &height,
            LightChannelCapability::block_light_only(),
            DimensionLightPolicy::BlockLightOnly,
            center,
            &mut chunks,
            &TestProps,
            None,
        )
        .expect("retry after restoring workspace succeeds");
        assert!(chunks[&center].is_light_correct());
        assert_eq!(
            get_block_light_value(
                &height,
                true,
                BlockPos::new(0, 20, 8),
                Some(&chunks[&center])
            ),
            14
        );
        assert_eq!(
            get_block_light_value(
                &height,
                true,
                BlockPos::new(-1, 20, 8),
                Some(&chunks[&west])
            ),
            13
        );
    }

    #[test]
    fn neighbor_light_updates_mark_neighbors_unsaved() {
        let height = create_accessor(0, 64);
        let center = ChunkPos::new(0, 0);
        let west = ChunkPos::new(-1, 0);
        let east = ChunkPos::new(1, 0);
        let far = ChunkPos::new(2, 2);
        let mut chunks = make_workspace(center, 0, 64);

        // Place a torch on the west border of center so block light spills into west.
        set_block(chunks.get_mut(&center).unwrap(), 0, 0, 20, 8, TORCH);

        // Prepare west, east, and far as previously lit & saved (clean) neighbors.
        for pos in [west, east, far] {
            let chunk = chunks.get_mut(&pos).unwrap();
            chunk.set_persisted_status(ChunkStatus::Light);
            chunk.set_light_correct(true);
            chunk.set_block_nibbles(filled_empty_light(6));
            chunk.set_block_emptiness_map(Some(vec![true; 4]));
            chunk.try_mark_saved();
            assert!(!chunk.is_unsaved());
        }

        // Also stage a sky nibble update targeting east to verify sky neighbor marking.
        let mut east_sky = filled_empty_light(6);
        east_sky[2].set_non_null();
        east_sky[2].set(0, 4, 8, 15);
        let staged_sky = StagedSkyLightPublication {
            center_nibbles: filled_empty_light(6),
            center_emptiness_map: vec![true; 4],
            neighbor_nibbles: vec![(east, east_sky)],
        };

        publish_generated_light_atomic(
            &height,
            LightChannelCapability::normal_overworld(),
            DimensionLightPolicy::NormalOverworld,
            center,
            &mut chunks,
            &TestProps,
            Some(staged_sky),
        )
        .expect("publication succeeds");

        assert!(chunks[&center].is_unsaved());
        assert!(
            chunks[&west].is_unsaved(),
            "neighbor receiving block-light update must be marked unsaved"
        );
        assert!(
            chunks[&east].is_unsaved(),
            "neighbor receiving sky-light update must be marked unsaved"
        );
        assert!(
            !chunks[&far].is_unsaved(),
            "untouched radius-2 neighbor must remain clean"
        );
    }

    #[test]
    fn loaded_neighbor_without_emptiness_map_propagates_light_into_all_air_center() {
        let height = create_accessor(0, 64);
        let center = ChunkPos::ZERO;
        let west = ChunkPos::new(-1, 0);
        let mut chunks = make_workspace(center, 0, 64);

        // Simulate a chunk loaded from disk: already lit with a torch on its east border
        // and populated block_nibbles, but no cached block_emptiness_map (None).
        {
            let w = chunks.get_mut(&west).unwrap();
            set_block(w, 0, 15, 20, 8, TORCH);
            let mut nibbles = filled_empty_light(6);
            // Section y=1 corresponds to nibble index 2 (min_light_section = -1).
            nibbles[2].set_non_null();
            nibbles[2].set(15, 4, 8, 14);
            nibbles[2].update_visible();
            w.set_block_nibbles(nibbles);
            w.set_block_emptiness_map(None);
            w.set_persisted_status(ChunkStatus::Light);
            w.set_light_correct(true);
        }

        // Center chunk is completely air.
        publish_generated_light_atomic(
            &height,
            LightChannelCapability::block_light_only(),
            DimensionLightPolicy::BlockLightOnly,
            center,
            &mut chunks,
            &TestProps,
            None,
        )
        .expect("publication succeeds");

        assert!(chunks[&center].is_light_correct());
        assert_eq!(
            get_block_light_value(
                &height,
                true,
                BlockPos::new(0, 20, 8),
                Some(&chunks[&center])
            ),
            13,
            "incoming light from loaded neighbor without emptiness map must reach center border"
        );
        assert_eq!(
            get_block_light_value(
                &height,
                true,
                BlockPos::new(1, 20, 8),
                Some(&chunks[&center])
            ),
            12
        );
    }
}
