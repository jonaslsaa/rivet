//! Retained StarLightInterface block/section tasks over canonical live chunks.
//! Only light caches are copied. Blocks and chunk lifecycle retain one owner.
use std::collections::{HashMap, VecDeque};

use rivet_registry::core::{BlockPos, ChunkPos, JavaBlockPosSet, SectionPos};
use rivet_world::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor};
use rivet_world::lighting::swmr_nibble_array::SwmrNibbleArray;

use super::star_light_engine::{ChunkAccessor, NeighborLightUpdate, SkyStarLightEngine};
use super::star_light_provider_impl::LightChunk;
use crate::server::level::chunk_map::ChunkMap;

#[derive(Debug, thiserror::Error)]
pub enum LiveLightError {
    #[error("the world has no installed incremental light owner")]
    NotInstalled,
    #[error("incremental light center {0:?} is not LIGHT and light-correct")]
    NotReady(ChunkPos),
    #[error("incremental light position ordering unavailable: {0}")]
    PositionOrder(#[from] rivet_registry::core::JavaBlockPosSetError),
    #[error("incremental light propagation refused: {0}")]
    Propagation(String),
}

#[derive(Default)]
pub(crate) struct LightChanges {
    order: VecDeque<ChunkPos>,
    tasks: HashMap<ChunkPos, LightTask>,
}
#[derive(Default)]
pub(crate) struct LightTask {
    positions: Vec<BlockPos>,
    sections: HashMap<i32, bool>,
}
impl LightChanges {
    fn task(&mut self, pos: ChunkPos) -> &mut LightTask {
        self.tasks.entry(pos).or_insert_with(|| {
            self.order.push_back(pos);
            LightTask::default()
        })
    }
    pub(crate) fn block_change(&mut self, pos: BlockPos) {
        self.task(ChunkPos::new(pos.get_x() >> 4, pos.get_z() >> 4))
            .positions
            .push(pos);
    }
    pub(crate) fn section_change(&mut self, pos: SectionPos, empty: bool) {
        self.task(ChunkPos::new(pos.x(), pos.z()))
            .sections
            .insert(pos.y(), empty);
    }
    pub(crate) fn front(&self) -> Option<(ChunkPos, &LightTask)> {
        self.order.front().map(|pos| (*pos, &self.tasks[pos]))
    }
    pub(crate) fn complete(&mut self, pos: ChunkPos) {
        assert_eq!(self.order.pop_front(), Some(pos));
        self.tasks.remove(&pos);
    }
}

/// Reusable engine pair, with work retained through missing readiness or a
/// propagation refusal. No light output is committed until both channels pass.
pub struct LiveLightRuntime {
    sky: Option<SkyStarLightEngine>,
    block: Option<SkyStarLightEngine>,
    min_section: i32,
    max_section: i32,
    changes: LightChanges,
    pending_notifications: Vec<ChunkPos>,
}
impl LiveLightRuntime {
    pub fn new(
        height: SimpleLevelHeightAccessor,
        has_sky_light: bool,
        has_block_light: bool,
    ) -> Self {
        Self {
            sky: has_sky_light.then(|| SkyStarLightEngine::new(&height)),
            block: has_block_light.then(|| SkyStarLightEngine::new_block(&height)),
            min_section: height.get_min_section_y(),
            max_section: height.get_max_section_y(),
            changes: LightChanges::default(),
            pending_notifications: Vec::new(),
        }
    }
    pub fn block_change(&mut self, pos: BlockPos) {
        if (self.min_section..=self.max_section).contains(&(pos.get_y() >> 4)) {
            self.changes.block_change(pos);
        }
    }
    pub fn section_change(&mut self, pos: SectionPos, empty: bool) {
        assert!(
            (self.min_section..=self.max_section).contains(&pos.y()),
            "light section outside world"
        );
        self.changes.section_change(pos, empty);
    }
    pub fn has_pending_updates(&self) -> bool {
        self.changes.front().is_some()
    }

    pub fn run_updates(&mut self, chunks: &mut ChunkMap) -> Result<Vec<ChunkPos>, LiveLightError> {
        while let Some((pos, task)) = self.changes.front() {
            let center = chunks
                .get_chunk(pos)
                .ok_or(LiveLightError::NotReady(pos))?
                .generation_base();
            let mut access = CanonicalAccess(chunks);
            let updates = compute_changes(
                self.sky.as_mut(),
                self.block.as_mut(),
                &mut access,
                center,
                task,
                self.min_section,
                self.max_section,
            )?;
            for (sky, channel) in [(true, updates.sky), (false, updates.block)] {
                for ((x, z), update) in channel {
                    let key = ChunkPos::new(x, z);
                    let chunk = chunks
                        .get_chunk_mut(key)
                        .expect("cached canonical chunk remains owned");
                    if publish_update(chunk.generation_base_mut(), update, sky) {
                        chunk.refresh_light_data();
                        if !self.pending_notifications.contains(&key) {
                            self.pending_notifications.push(key);
                        }
                    }
                }
            }
            self.changes.complete(pos);
        }
        Ok(std::mem::take(&mut self.pending_notifications))
    }
}

struct CanonicalAccess<'a>(&'a ChunkMap);
impl ChunkAccessor for CanonicalAccess<'_> {
    fn get_chunk_for_lighting(&mut self, x: i32, z: i32) -> Option<&LightChunk> {
        self.0
            .get_chunk(ChunkPos::new(x, z))
            .map(|chunk| chunk.generation_base())
    }
}

pub(crate) struct ComputedChanges {
    pub(crate) sky: HashMap<(i32, i32), NeighborLightUpdate>,
    pub(crate) block: HashMap<(i32, i32), NeighborLightUpdate>,
}
pub(crate) fn compute_changes(
    sky: Option<&mut SkyStarLightEngine>,
    block: Option<&mut SkyStarLightEngine>,
    access: &mut dyn ChunkAccessor,
    center: &LightChunk,
    task: &LightTask,
    min_section: i32,
    max_section: i32,
) -> Result<ComputedChanges, LiveLightError> {
    if !center.is_light_correct()
        || !center
            .get_persisted_status()
            .is_or_after(rivet_world::chunk::status::ChunkStatus::Light)
    {
        return Err(LiveLightError::NotReady(center.get_pos()));
    }
    let positions = JavaBlockPosSet::try_from_insertions(task.positions.iter().copied())?;
    let positions: Vec<_> = positions.iter().copied().collect();
    let mut sections = vec![None; (max_section - min_section + 1) as usize];
    for (&y, &empty) in &task.sections {
        sections[(y - min_section) as usize] = Some(empty);
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut result = ComputedChanges {
            sky: HashMap::new(),
            block: HashMap::new(),
        };
        if let Some(engine) = sky {
            engine.blocks_changed(access, center, &positions, &sections);
            result.sky = engine.take_pending_neighbor_updates();
        }
        if let Some(engine) = block {
            engine.blocks_changed(access, center, &positions, &sections);
            result.block = engine.take_pending_neighbor_updates();
        }
        result
    }));
    result.map_err(|payload| {
        LiveLightError::Propagation(
            payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&str>()
                        .map(|value| (*value).to_owned())
                })
                .unwrap_or_else(|| "light engine panicked".into()),
        )
    })
}

fn visible_equal(a: &SwmrNibbleArray, b: &SwmrNibbleArray) -> bool {
    a.is_null_nibble_visible() == b.is_null_nibble_visible()
        && a.is_uninitialised_visible() == b.is_uninitialised_visible()
        && a.is_hidden_visible() == b.is_hidden_visible()
        && (0..4096).all(|index| a.get_visible_index(index) == b.get_visible_index(index))
}
pub(crate) fn publish_update(
    chunk: &mut LightChunk,
    update: NeighborLightUpdate,
    sky: bool,
) -> bool {
    let notified = !update.notifications.is_empty();
    let mut changed = false;
    let mut nibbles = if sky {
        chunk.sky_nibbles()
    } else {
        chunk.block_nibbles()
    }
    .to_vec();
    for (index, nibble) in update.nibbles {
        changed |= !visible_equal(&nibbles[index], &nibble);
        nibbles[index] = nibble;
    }
    if sky {
        chunk.set_sky_nibbles(nibbles);
    } else {
        chunk.set_block_nibbles(nibbles);
    }
    let map_changed = update.emptiness_map.as_deref()
        != if sky {
            chunk.sky_emptiness_map()
        } else {
            chunk.block_emptiness_map()
        };
    if let Some(map) = update.emptiness_map {
        if sky {
            chunk.set_sky_emptiness_map(Some(map));
        } else {
            chunk.set_block_emptiness_map(Some(map));
        }
    }
    if changed || map_changed || notified {
        chunk.mark_unsaved();
    }
    // Keep packet-cache refresh for actual content changes, and never lose a
    // real source dirty notification just because the final bytes are equal.
    changed || notified
}
