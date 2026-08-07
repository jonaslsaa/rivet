//! Port of `net.minecraft.world.level.levelgen.Heightmap` (MC 26.2) — the
//! client-heightmap slice.
//!
//! Java: `Heightmap.java` in `working/Paper`. The `Types` enum (ids, names,
//! `Usage.CLIENT`) lives in `rivet-protocol::protocol::game::heightmap_types`
//! (the wire-visible slice; world → protocol exists, protocol cannot depend on
//! world). This module ports the world-side value: the `SimpleBitStorage` of
//! `ceillog2(height + 1)`-bit entries and `primeHeightmaps`, which computes the
//! `long[]` a `LevelChunkPacketData` heightmap carries.
//!
//! Owned by the `mc.world.level.levelgen` manifest unit; ported ahead of that
//! unit because issue #100 needs only `primeHeightmaps` to produce the superflat
//! chunk's heightmap bytes.
//!
//! RivetTodo(#177): the `update`/`updateFromChunk` worldgen mutators and the
//! live `Heightmap` plumbing used during generation are not ported (owned by
//! the `mc.world.level.levelgen.noise` unit's wave-1 port).

use rivet_protocol::protocol::game::heightmap_types::HeightmapType;
use rivet_util::bit_storage::BitStorage;
use rivet_util::mth;
use rivet_util::simple_bit_storage::SimpleBitStorage;

/// `Heightmap` — `data` holds the height offset `height - minY` for each of the
/// 256 columns, stored at `heightBits = ceillog2(height + 1)` bits. The
/// `isOpaque` predicate and the `chunk` back-reference are resolved by the
/// caller passing an explicit topmost-opaque-y getter (OWNERSHIP.md — no stored
/// `&ChunkAccess`).
pub struct Heightmap {
    data: SimpleBitStorage,
}

impl Heightmap {
    /// `Heightmap(ChunkAccess, Types)` — `new SimpleBitStorage(ceillog2(
    /// chunk.getHeight() + 1), 256)`.
    pub fn new(height: i32) -> Self {
        let height_bits = mth::ceillog2(height + 1);
        Heightmap {
            data: SimpleBitStorage::new(height_bits, 256),
        }
    }

    /// `setHeight(x, z, height)` — `data.set(getIndex(x, z), height - minY)`.
    fn set_height(&mut self, x: i32, z: i32, height: i32, min_y: i32) {
        self.data.set(get_index(x, z), height - min_y);
    }

    /// `getRawData()`.
    pub fn get_raw_data(&self) -> &[i64] {
        self.data.get_raw()
    }

    /// `Heightmap.Types.isOpaque()` — the per-type block predicate, resolved
    /// over per-state flags because `rivet-registry`'s generated tables carry
    /// ids, not `BlockBehaviour` behavior flags.
    ///
    /// Only `WorldSurface`/`MotionBlocking`/`MotionBlockingNoLeaves` are sent
    /// to clients (the `Usage.CLIENT` set); the other three are worldgen/live
    /// types never emitted, but their predicates are ported for fidelity. The
    /// superflat chunk's single stone layer exercises only the "non-air,
    /// blocks-motion, no fluid, not leaves" path.
    pub fn is_opaque(
        heightmap_type: HeightmapType,
        state_is_air: bool,
        state_blocks_motion: bool,
        state_has_fluid: bool,
        state_is_leaves: bool,
    ) -> bool {
        match heightmap_type {
            HeightmapType::WorldSurfaceWg | HeightmapType::WorldSurface => !state_is_air,
            HeightmapType::OceanFloorWg | HeightmapType::OceanFloor => state_blocks_motion,
            HeightmapType::MotionBlocking => state_blocks_motion || state_has_fluid,
            HeightmapType::MotionBlockingNoLeaves => {
                (state_blocks_motion || state_has_fluid) && !state_is_leaves
            }
        }
    }
}

/// `primeHeightmaps(ChunkAccess, Set<Types>)` — for each column, walk the
/// section stack from the highest filled section down to min Y; the first
/// block that satisfies a type's `isOpaque` predicate sets that heightmap's
/// height to `y + 1`. A column with no opaque block leaves the entry 0 (Java
/// `setHeight` never runs), which decodes as `minY`.
///
/// The Rust port resolves `chunk.getBlockState`/`isOpaque` via a per-column
/// `topmost_opaque` closure returning the topmost opaque y per type, so
/// `Heightmap` stays a pure value. The superflat chunk exercises the
/// single-block path: stone at y = -64 gives height -63 (`-64 + 1`).
pub fn prime_heightmaps(
    height: i32,
    min_y: i32,
    topmost_opaque: impl Fn(HeightmapType, i32, i32) -> Option<i32>,
) -> Vec<(HeightmapType, Vec<i64>)> {
    // The three `Usage.CLIENT` types, in enum id order (1, 4, 5) — the
    // `EnumMap` iteration order the `LevelChunkPacketData` heightmap map is
    // written in.
    const CLIENT_TYPES: [HeightmapType; 3] = [
        HeightmapType::WorldSurface,
        HeightmapType::MotionBlocking,
        HeightmapType::MotionBlockingNoLeaves,
    ];
    let mut heightmaps: Vec<Heightmap> = CLIENT_TYPES
        .iter()
        .map(|_| Heightmap::new(height))
        .collect();
    for z in 0..16 {
        for x in 0..16 {
            for (i, ty) in CLIENT_TYPES.iter().enumerate() {
                let height = topmost_opaque(*ty, x, z).map_or(min_y, |y| y + 1);
                heightmaps[i].set_height(x, z, height, min_y);
            }
        }
    }
    heightmaps
        .into_iter()
        .zip(CLIENT_TYPES)
        .map(|(hm, ty)| (ty, hm.get_raw_data().to_vec()))
        .collect()
}

/// `Heightmap.getIndex(x, z)` — `x + z * 16`.
fn get_index(x: i32, z: i32) -> usize {
    (x + z * 16) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_single_stone_layer_packs_all_ones() {
        // height 384, min_y -64: height_bits = ceillog2(385) = 9, so each of
        // the 256 columns' offsets (0..384) needs 9 bits -> 7 entries per long,
        // ceil(256/7) = 37 longs. Every column has stone at y = -64, so the
        // stored offset is -64 + 1 - (-64) = 1.
        let heightmaps = prime_heightmaps(384, -64, |_ty, _x, _z| Some(-64));
        assert_eq!(heightmaps.len(), 3);
        assert_eq!(heightmaps[0].0, HeightmapType::WorldSurface);
        assert_eq!(heightmaps[1].0, HeightmapType::MotionBlocking);
        assert_eq!(heightmaps[2].0, HeightmapType::MotionBlockingNoLeaves);
        let expected: Vec<i64> = {
            // 36 cells each holding 7 packed `1` entries, low bit first:
            // 1 | 1<<9 | ... | 1<<54 = 0x0040201008040201 (matches the
            // committed fixture's first 36 longs).
            let mut v = vec![0x0040_2010_0804_0201i64; 36];
            // The 37th cell holds the remaining 4 entries (256 - 36*7), packed
            // at 9 bits each: 1 | 1<<9 | 1<<18 | 1<<27 = 0x0000000008040201.
            v.push(0x0000_0000_0804_0201i64);
            v
        };
        for (_, raw) in &heightmaps {
            assert_eq!(raw, &expected);
        }
    }

    #[test]
    fn air_column_stays_at_min_y() {
        // A column with no opaque block: Java leaves the entry at 0 (min_y).
        let heightmaps = prime_heightmaps(384, -64, |_ty, _x, _z| None);
        assert_eq!(heightmaps[0].1.iter().filter(|&&v| v != 0).count(), 0);
    }
}
