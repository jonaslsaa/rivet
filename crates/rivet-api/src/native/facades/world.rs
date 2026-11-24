//! World and block capability facade types and tick-thread commands (#508).
//!
//! Mirrors the observable world/chunk/block boundary of Paper's `org.bukkit.World`
//! and `net.minecraft.world.level.Level` (Paper 26.2) via opaque IDs and
//! value-copy snapshots.

use std::collections::BTreeMap;
use std::fmt;

/// Opaque identifier for a loaded world/dimension.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct WorldId(u32);

impl WorldId {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl fmt::Display for WorldId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "world#{}", self.0)
    }
}

/// Block coordinates using signed 32-bit Java `int` semantics (`BlockPos`).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Converts block coordinates to chunk coordinates using Java arithmetic
    /// right shift (`blockCoord >> 4`, matching `SectionPos.blockToSectionCoord`).
    #[must_use]
    pub const fn chunk_pos(self) -> ChunkPos {
        ChunkPos {
            x: self.x >> 4,
            z: self.z >> 4,
        }
    }
}

/// Chunk coordinates using signed 32-bit Java `int` semantics (`ChunkPos`).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl ChunkPos {
    #[must_use]
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

/// Value-copy snapshot of a block state at a world coordinate.
///
/// Uses deterministic `BTreeMap` ordering for block state properties.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct BlockStateSnapshot {
    pub block_id: String,
    pub light_emission: u8,
    pub properties: BTreeMap<String, String>,
}

impl BlockStateSnapshot {
    #[must_use]
    pub fn air() -> Self {
        Self {
            block_id: "minecraft:air".to_owned(),
            light_emission: 0,
            properties: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn simple(block_id: impl Into<String>) -> Self {
        Self {
            block_id: block_id.into(),
            light_emission: 0,
            properties: BTreeMap::new(),
        }
    }
}

/// Value-copy snapshot of a loaded world's observable metadata.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct WorldSnapshot {
    pub id: WorldId,
    pub name: String,
    pub dimension_key: String,
    pub loaded_chunks: usize,
    pub game_time: i64,
    pub day_time: i64,
    pub min_y: i32,
    pub height: i32,
}

impl WorldSnapshot {
    #[must_use]
    pub const fn max_y_exclusive(&self) -> i32 {
        self.min_y.wrapping_add(self.height)
    }

    #[must_use]
    pub const fn contains_y(&self, y: i32) -> bool {
        y >= self.min_y && y < self.max_y_exclusive()
    }
}

/// Typed tick-thread mutation commands targeting world state.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum WorldCommand {
    /// Sets the block state at `pos` in `world`. Fails with `ChunkUnloaded`
    /// if `pos.chunk_pos()` is not loaded, or `OutOfWorldBounds` if `pos.y`
    /// is outside `[min_y, min_y + height)`.
    SetBlock {
        world: WorldId,
        pos: BlockPos,
        state: BlockStateSnapshot,
    },
    /// Sets the world's `day_time` (signed Java `long`).
    SetDayTime { world: WorldId, day_time: i64 },
}
