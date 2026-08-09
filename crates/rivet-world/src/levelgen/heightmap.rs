//! Port of `net.minecraft.world.level.levelgen.Heightmap` (MC 26.2) — the
//! world-side heightmap value.
//!
//! Java: `Heightmap.java` in `working/Paper`. This module ports two things.
//!
//! First the world-side `Types` enum — the storage key `ChunkAccess` uses
//! (`Maps.newEnumMap(Heightmap.Types.class)`). The protocol
//! `HeightmapType` (in `rivet-protocol`, the wire-visible slice; world →
//! protocol exists, protocol cannot depend on world) is the *id* order; the
//! world `Types` enum here is Java's *ordinal* (declaration) order — the
//! `EnumMap` iteration order `SerializableChunkData` serializes heightmaps in.
//! In 26.2 the two orders coincide (the enum is declared in ascending id
//! order: `WORLD_SURFACE_WG=0, WORLD_SURFACE=1, OCEAN_FLOOR_WG=2, OCEAN_FLOOR=3,
//! MOTION_BLOCKING=4, MOTION_BLOCKING_NO_LEAVES=5`), so `to_wire_id()` is the
//! identity here — but the chunk storage is keyed on the world enum (never on
//! the protocol enum's variant index), so a future reordering of either cannot
//! silently change the serialized order.
//!
//! Second the `SimpleBitStorage` of `ceillog2(height + 1)`-bit entries and
//! `primeHeightmaps`, which computes the `long[]` a `LevelChunkPacketData`
//! heightmap carries.
//!
//! Owned by the `mc.world.level.levelgen` manifest unit; ported ahead of that
//! unit because issue #100 needs only `primeHeightmaps` to produce the superflat
//! chunk's heightmap bytes, and issue #183's `ChunkAccess` needs the `Types`
//! storage key (like #100 pulled `LevelChunkSection` ahead).
//!
//! RivetTodo(#177): the `update`/`updateFromChunk` worldgen mutators and the
//! live `Heightmap` plumbing used during generation are not ported (owned by
//! the `mc.world.level.levelgen.noise` unit's wave-1 port).

use rivet_protocol::protocol::game::heightmap_types::HeightmapType;
use rivet_util::bit_storage::BitStorage;
use rivet_util::mth;
use rivet_util::simple_bit_storage::SimpleBitStorage;

/// `Heightmap.Types` — the six world heightmap types, in Java's ordinal
/// (declaration) order: the `EnumMap` key order `ChunkAccess.getHeightmaps()`
/// iterates and `SerializableChunkData` serializes in. `to_wire_id()` is the
/// enum `id` field (the wire form, shared with the protocol `HeightmapType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Types {
    /// `WORLD_SURFACE_WG` (id 0, `Usage.WORLDGEN`).
    WorldSurfaceWg,
    /// `WORLD_SURFACE` (id 1, `Usage.CLIENT`).
    WorldSurface,
    /// `OCEAN_FLOOR_WG` (id 2, `Usage.WORLDGEN`).
    OceanFloorWg,
    /// `OCEAN_FLOOR` (id 3, `Usage.LIVE_WORLD`).
    OceanFloor,
    /// `MOTION_BLOCKING` (id 4, `Usage.CLIENT`).
    MotionBlocking,
    /// `MOTION_BLOCKING_NO_LEAVES` (id 5, `Usage.CLIENT`).
    MotionBlockingNoLeaves,
}

/// `ChunkStatus.FULL.heightmapsAfter()` — the four `FINAL_HEIGHTMAPS` in
/// declaration order; `LevelChunk`'s constructor creates (unprimed) entries
/// for exactly these.
pub const FINAL_HEIGHTMAPS: [Types; 4] = [
    Types::OceanFloor,
    Types::WorldSurface,
    Types::MotionBlocking,
    Types::MotionBlockingNoLeaves,
];

impl Types {
    /// The enum `id` (the wire form), `Heightmap.Types` `id` field.
    pub const fn to_wire_id(self) -> i32 {
        match self {
            Types::WorldSurfaceWg => 0,
            Types::WorldSurface => 1,
            Types::OceanFloorWg => 2,
            Types::OceanFloor => 3,
            Types::MotionBlocking => 4,
            Types::MotionBlockingNoLeaves => 5,
        }
    }

    /// `Heightmap.Types.BY_ID` — `ByIdMap.continuous(t -> t.id, values(),
    /// OutOfBoundsStrategy.ZERO)`: an out-of-range wire id falls back to id 0
    /// (`WORLD_SURFACE_WG`).
    pub fn from_wire_id(id: i32) -> Types {
        match id {
            0 => Types::WorldSurfaceWg,
            1 => Types::WorldSurface,
            2 => Types::OceanFloorWg,
            3 => Types::OceanFloor,
            4 => Types::MotionBlocking,
            5 => Types::MotionBlockingNoLeaves,
            _ => Types::WorldSurfaceWg,
        }
    }

    /// `Heightmap.Types.getSerializationKey()` — the canonical name.
    pub const fn serialization_key(self) -> &'static str {
        match self {
            Types::WorldSurfaceWg => "WORLD_SURFACE_WG",
            Types::WorldSurface => "WORLD_SURFACE",
            Types::OceanFloorWg => "OCEAN_FLOOR_WG",
            Types::OceanFloor => "OCEAN_FLOOR",
            Types::MotionBlocking => "MOTION_BLOCKING",
            Types::MotionBlockingNoLeaves => "MOTION_BLOCKING_NO_LEAVES",
        }
    }

    /// `Heightmap.Types.sendToClient()` — `usage == CLIENT`.
    pub const fn send_to_client(self) -> bool {
        matches!(
            self,
            Types::WorldSurface | Types::MotionBlocking | Types::MotionBlockingNoLeaves
        )
    }

    /// `Heightmap.Types.keepAfterWorldgen()` — `usage != WORLDGEN`.
    pub const fn keep_after_worldgen(self) -> bool {
        !matches!(self, Types::WorldSurfaceWg | Types::OceanFloorWg)
    }

    /// The protocol enum → world enum conversion (identity in 26.2, see the
    /// module doc on the shared id order).
    pub const fn from_protocol(ty: HeightmapType) -> Types {
        match ty {
            HeightmapType::WorldSurfaceWg => Types::WorldSurfaceWg,
            HeightmapType::WorldSurface => Types::WorldSurface,
            HeightmapType::OceanFloorWg => Types::OceanFloorWg,
            HeightmapType::OceanFloor => Types::OceanFloor,
            HeightmapType::MotionBlocking => Types::MotionBlocking,
            HeightmapType::MotionBlockingNoLeaves => Types::MotionBlockingNoLeaves,
        }
    }

    /// The world enum ↔ protocol enum conversion (identity in 26.2, see the
    /// module doc on the shared id order).
    pub const fn as_protocol(self) -> HeightmapType {
        match self {
            Types::WorldSurfaceWg => HeightmapType::WorldSurfaceWg,
            Types::WorldSurface => HeightmapType::WorldSurface,
            Types::OceanFloorWg => HeightmapType::OceanFloorWg,
            Types::OceanFloor => HeightmapType::OceanFloor,
            Types::MotionBlocking => HeightmapType::MotionBlocking,
            Types::MotionBlockingNoLeaves => HeightmapType::MotionBlockingNoLeaves,
        }
    }

    /// The `Types` enum in ordinal order (the storage key order).
    pub const fn all() -> [Types; 6] {
        [
            Types::WorldSurfaceWg,
            Types::WorldSurface,
            Types::OceanFloorWg,
            Types::OceanFloor,
            Types::MotionBlocking,
            Types::MotionBlockingNoLeaves,
        ]
    }

    /// Whether `ChunkStatus.FULL.heightmapsAfter()` contains this type.
    pub const fn in_final_heightmaps(self) -> bool {
        matches!(
            self,
            Types::OceanFloor
                | Types::WorldSurface
                | Types::MotionBlocking
                | Types::MotionBlockingNoLeaves
        )
    }
}

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

    /// `setRawData(ChunkAccess, Types, long[])` — copies the packed storage
    /// in place when the length matches; Java logs a warning and re-primes on
    /// a mismatch. The port cannot re-prime without the per-state behavior
    /// predicates (#287), so a mismatched array is ignored (a documented
    /// no-op) — the callers in this slice always supply a matching-length
    /// storage.
    ///
    /// RivetTodo(#287): the re-prime fallback on a length mismatch needs the
    /// `Heightmap.update`/`updateFromChunk` compute, deferred with the
    /// worldgen heightmap unit.
    pub fn set_raw_data(&mut self, data: &[i64]) {
        if self.data.get_raw().len() == data.len() {
            self.data.get_raw_mut().copy_from_slice(data);
        }
        // Length mismatch: Java warns and primes; the port ignores the data.
    }

    /// `ChunkAccess.getHeight(Types, x, z)` — `getFirstAvailable(x, z) - 1`
    /// where `getFirstAvailable(index) = data.get(index) + chunk.getMinY()`.
    /// A never-set entry is 0, so the height is `minY - 1` (Java behavior for
    /// an unprimed heightmap).
    pub fn get_height_at(&self, x: i32, z: i32, min_y: i32) -> i32 {
        self.data.get(get_index(x, z)) + min_y - 1
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

    #[test]
    fn types_ordinal_order_matches_java_declaration() {
        // The world `Types` enum is Java's ordinal (declaration) order — the
        // `EnumMap` key order `ChunkAccess.getHeightmaps()` iterates and
        // `SerializableChunkData` serializes in. In 26.2 it equals the wire id
        // order, but storage must be keyed on this enum, never the protocol
        // enum's variant index.
        assert_eq!(
            Types::all(),
            [
                Types::WorldSurfaceWg,
                Types::WorldSurface,
                Types::OceanFloorWg,
                Types::OceanFloor,
                Types::MotionBlocking,
                Types::MotionBlockingNoLeaves,
            ]
        );
        for (ordinal, ty) in Types::all().iter().enumerate() {
            assert_eq!(*ty as usize, ordinal, "ordinal {ordinal}");
            assert_eq!(ty.to_wire_id(), ordinal as i32, "wire id {ordinal}");
        }
        // `from_wire_id` round-trips and falls back to id 0 out of range
        // (`ByIdMap.continuous(..., OutOfBoundsStrategy.ZERO)`).
        for ty in Types::all() {
            assert_eq!(Types::from_wire_id(ty.to_wire_id()), ty);
        }
        assert_eq!(Types::from_wire_id(99), Types::WorldSurfaceWg);
        assert_eq!(Types::from_wire_id(-1), Types::WorldSurfaceWg);
    }

    #[test]
    fn final_heightmaps_are_the_four_full_status_types() {
        // `ChunkStatus.FULL.heightmapsAfter()` — the types `LevelChunk`'s
        // constructor creates unprimed entries for.
        assert_eq!(
            FINAL_HEIGHTMAPS,
            [
                Types::OceanFloor,
                Types::WorldSurface,
                Types::MotionBlocking,
                Types::MotionBlockingNoLeaves,
            ]
        );
        for ty in Types::all() {
            assert_eq!(ty.in_final_heightmaps(), FINAL_HEIGHTMAPS.contains(&ty));
        }
        // `sendToClient` is the `Usage.CLIENT` set (ids 1/4/5).
        let client: Vec<Types> = Types::all()
            .into_iter()
            .filter(|t| t.send_to_client())
            .collect();
        assert_eq!(
            client,
            vec![
                Types::WorldSurface,
                Types::MotionBlocking,
                Types::MotionBlockingNoLeaves
            ]
        );
    }

    #[test]
    fn set_raw_data_and_get_height_at_round_trip() {
        // A primed flat-world heightmap: stored offset 1 at every column
        // (stone at y=-64 -> stored `-63 + 64`), so `get_height` returns -64.
        let mut hm = Heightmap::new(384);
        let raw = prime_heightmaps(384, -64, |_ty, _x, _z| Some(-64))[0]
            .1
            .clone();
        hm.set_raw_data(&raw);
        assert_eq!(hm.get_height_at(0, 0, -64), -64);
        assert_eq!(hm.get_height_at(15, 15, -64), -64);
        // Unprimed (never set): entries stay 0 -> `minY - 1`.
        let unprimed = Heightmap::new(384);
        assert_eq!(unprimed.get_height_at(3, 7, -64), -65);
    }

    #[test]
    fn set_raw_data_ignores_mismatched_length() {
        // Java warns and re-primes on a length mismatch; the port ignores the
        // data (see the `RivetTodo(#287)` on `set_raw_data`). A shorter array
        // must not clobber or panic.
        let mut hm = Heightmap::new(384);
        hm.set_raw_data(&[1, 2, 3]);
        assert!(hm.get_raw_data().iter().all(|&v| v == 0));
    }
}
