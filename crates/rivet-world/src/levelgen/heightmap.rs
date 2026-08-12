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
//! Issue #287 Part A adds the worldgen/live slice this module deferred: the
//! four `FINAL_HEIGHTMAPS`/`WORLDGEN_HEIGHTMAPS` predicates (`is_opaque`),
//! the per-block `update` mutator, and the on-demand `primeHeightmaps`/
//! `getHeight` compute on `ChunkAccess` (see `chunk::chunk_access`). Note
//! `updateFromChunk` does NOT exist in the pinned Paper 26.2 `Heightmap` —
//! only `update` — so the issue's mention of it is stale and only `update` is
//! ported.
//!
//! The `MOTION_BLOCKING_NO_LEAVES` exclusion is `state.is(TagKey)`
//! (`getBlock() instanceof LeavesBlock`), resolved by the caller through the
//! generated `BlockTags.LEAVES` (`BlockState::is_in_tag`) — `rivet-registry`'s
//! `blocks` feature is a production dependency of `rivet-world`, so the earlier
//! hand-maintained `LEAVES_BLOCKS` list is gone (it duplicated the generated
//! tag).

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
    Types::WorldSurface,
    Types::OceanFloor,
    Types::MotionBlocking,
    Types::MotionBlockingNoLeaves,
];

/// `ChunkStatus.WORLDGEN_HEIGHTMAPS` — the two `Usage.WORLDGEN` types a
/// `ProtoChunk` updates while its persisted status is below `CARVERS`
/// (`EnumSet.of(OCEAN_FLOOR_WG, WORLD_SURFACE_WG)`, iterated in declaration
/// order).
pub const WORLDGEN_HEIGHTMAPS: [Types; 2] = [Types::WorldSurfaceWg, Types::OceanFloorWg];

/// The per-state `BlockBehaviour` flags `Types.isOpaque()` needs. The caller
/// resolves them from the merged generated behavior table (`rivet-registry`'s
/// `BlockState` at the world sites; the superflat/server sites supply their
/// own flag predicates), keeping `Heightmap` a pure value (OWNERSHIP.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateFlags {
    /// `state.isAir()`.
    pub is_air: bool,
    /// `state.blocksMotion()`.
    pub blocks_motion: bool,
    /// `!state.getFluidState().isEmpty()` — true when the state carries a
    /// non-empty fluid state (the `MotionBlocking` predicate's disjunct).
    pub has_fluid: bool,
    /// `state.getBlock() instanceof LeavesBlock` — i.e. the caller's
    /// `BlockState::is_in_tag("minecraft:leaves")` (the generated
    /// `BlockTags.LEAVES`).
    pub is_leaves: bool,
}

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

    /// The exact inverse of [`serialization_key`](Self::serialization_key).
    ///
    /// `SerializableChunkData.parse` only accepts the six canonical,
    /// case-sensitive `Heightmap.Types.getSerializationKey()` names. Unknown,
    /// differently-cased, or otherwise malformed keys are not aliases.
    pub fn from_serialization_key(key: &str) -> Option<Types> {
        match key {
            "WORLD_SURFACE_WG" => Some(Types::WorldSurfaceWg),
            "WORLD_SURFACE" => Some(Types::WorldSurface),
            "OCEAN_FLOOR_WG" => Some(Types::OceanFloorWg),
            "OCEAN_FLOOR" => Some(Types::OceanFloor),
            "MOTION_BLOCKING" => Some(Types::MotionBlocking),
            "MOTION_BLOCKING_NO_LEAVES" => Some(Types::MotionBlockingNoLeaves),
            _ => None,
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
    pub(crate) fn set_height(&mut self, x: i32, z: i32, height: i32, min_y: i32) {
        self.data.set(get_index(x, z), height - min_y);
    }

    /// `getFirstAvailable(x, z)` — `data.get(getIndex(x, z)) + chunk.getMinY()`
    /// (the insertion slot one above the column's topmost opaque block; a
    /// never-set entry stores 0, so it reads `minY`).
    pub fn first_available_at(&self, x: i32, z: i32, min_y: i32) -> i32 {
        self.data.get(get_index(x, z)) + min_y
    }

    /// `Heightmap.update(localX, localY, localZ, BlockState)` — the per-block
    /// worldgen/live mutator. `localY` is the absolute block Y (Java passes
    /// `ProtoChunk`'s `y`, not `sectionRelative(y)`, to the heightmap update).
    ///
    /// `placed` is the placed state's behavior flags; `flags_at(abs_y)`
    /// resolves the flags of the state at `(localX, abs_y, localZ)` for the
    /// downward re-scan Java reads through `chunk.getBlockState`. Resolving
    /// both through closures keeps `Heightmap` a pure value (OWNERSHIP.md).
    #[allow(clippy::too_many_arguments)] // Java's `update` has 4 parameters; the port adds the flags resolver.
    pub fn update(
        &mut self,
        local_x: i32,
        local_y: i32,
        local_z: i32,
        ty: Types,
        placed: StateFlags,
        min_y: i32,
        flags_at: impl Fn(i32) -> StateFlags,
    ) -> bool {
        let first_available = self.first_available_at(local_x, local_z, min_y);
        if local_y <= first_available - 2 {
            return false;
        }
        if Self::is_opaque(ty, placed) {
            if local_y >= first_available {
                self.set_height(local_x, local_z, local_y + 1, min_y);
                return true;
            }
        } else if first_available - 1 == local_y {
            for y in (min_y..=local_y - 1).rev() {
                if Self::is_opaque(ty, flags_at(y)) {
                    self.set_height(local_x, local_z, y + 1, min_y);
                    return true;
                }
            }
            self.set_height(local_x, local_z, min_y, min_y);
            return true;
        }
        false
    }

    /// `getRawData()`.
    pub fn get_raw_data(&self) -> &[i64] {
        self.data.get_raw()
    }

    /// `setRawData(ChunkAccess, Types, long[])` — copies the packed storage
    /// in place when the length matches. Java logs a warning and re-primes on
    /// a mismatch; the re-prime walks the chunk's blocks, which this pure
    /// value has no handle to, so the port ignores a mismatched array (a
    /// documented no-op). The re-prime path is [`ChunkAccess::prime_heightmaps`]
    /// (issue #287), which no `setHeightmap` caller here needs — they all
    /// supply a matching-length storage.
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
    /// ids, not `BlockBehaviour` behavior flags:
    /// - `WORLD_SURFACE_WG`/`WORLD_SURFACE` — `NOT_AIR` (`!state.isAir()`);
    /// - `OCEAN_FLOOR_WG`/`OCEAN_FLOOR` — `MATERIAL_MOTION_BLOCKING`
    ///   (`state.blocksMotion()`);
    /// - `MOTION_BLOCKING` — `blocksMotion || !fluidState.isEmpty()`;
    /// - `MOTION_BLOCKING_NO_LEAVES` — the same, and not a `LeavesBlock` (the
    ///   caller's `is_leaves` flag, resolved via the generated
    ///   `BlockTags.LEAVES`).
    ///
    /// Only `WorldSurface`/`MotionBlocking`/`MotionBlockingNoLeaves` are sent
    /// to clients (the `Usage.CLIENT` set); the other three are worldgen/live
    /// types never emitted, but their predicates are ported for fidelity. The
    /// superflat chunk's single stone layer exercises only the "non-air,
    /// blocks-motion, no fluid, not leaves" path.
    pub fn is_opaque(ty: Types, flags: StateFlags) -> bool {
        match ty {
            Types::WorldSurfaceWg | Types::WorldSurface => !flags.is_air,
            Types::OceanFloorWg | Types::OceanFloor => flags.blocks_motion,
            Types::MotionBlocking => flags.blocks_motion || flags.has_fluid,
            Types::MotionBlockingNoLeaves => {
                (flags.blocks_motion || flags.has_fluid) && !flags.is_leaves
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

    /// The superflat air (id 0) / stone (id 1) flags, driven by value.
    fn state_flags(value: u8) -> StateFlags {
        StateFlags {
            is_air: value == 0,
            blocks_motion: value != 0,
            has_fluid: false,
            is_leaves: false,
        }
    }

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

        for ty in Types::all() {
            assert_eq!(
                Types::from_serialization_key(ty.serialization_key()),
                Some(ty)
            );
        }
        assert_eq!(Types::from_serialization_key("world_surface"), None);
        assert_eq!(Types::from_serialization_key("WORLD_SURFACE_"), None);
        assert_eq!(Types::from_serialization_key("UNKNOWN"), None);
    }

    #[test]
    fn final_heightmaps_are_the_four_full_status_types() {
        // `ChunkStatus.FULL.heightmapsAfter()` — the types `LevelChunk`'s
        // constructor creates unprimed entries for.
        assert_eq!(
            FINAL_HEIGHTMAPS,
            [
                Types::WorldSurface,
                Types::OceanFloor,
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
    fn is_opaque_implements_the_four_predicates() {
        // The four `Heightmap.Types.isOpaque` predicates, exercised over the
        // four flag combinations the state flags can hold:
        // air (id 0) and stone (id 1) exercise NOT_AIR + blocksMotion; a
        // water-like flag set exercises the fluid disjunct; a leaf-like flag
        // set exercises the MOTION_BLOCKING_NO_LEAVES exclusion.
        let air = state_flags(0);
        let stone = state_flags(1);
        let water = StateFlags {
            is_air: false,
            blocks_motion: false,
            has_fluid: true,
            is_leaves: false,
        };
        let leaves = StateFlags {
            is_air: false,
            blocks_motion: true,
            has_fluid: false,
            is_leaves: true,
        };
        // WORLD_SURFACE: NOT_AIR.
        for ty in [Types::WorldSurfaceWg, Types::WorldSurface] {
            assert!(!Heightmap::is_opaque(ty, air));
            assert!(Heightmap::is_opaque(ty, stone));
        }
        // OCEAN_FLOOR: blocksMotion.
        for ty in [Types::OceanFloorWg, Types::OceanFloor] {
            assert!(!Heightmap::is_opaque(ty, air));
            assert!(Heightmap::is_opaque(ty, stone));
            assert!(
                !Heightmap::is_opaque(ty, water),
                "water does not block motion"
            );
        }
        // MOTION_BLOCKING: blocksMotion || hasFluid.
        assert!(!Heightmap::is_opaque(Types::MotionBlocking, air));
        assert!(Heightmap::is_opaque(Types::MotionBlocking, stone));
        assert!(Heightmap::is_opaque(Types::MotionBlocking, water));
        // MOTION_BLOCKING_NO_LEAVES: the same, but not a leaf.
        assert!(!Heightmap::is_opaque(Types::MotionBlockingNoLeaves, air));
        assert!(Heightmap::is_opaque(Types::MotionBlockingNoLeaves, stone));
        assert!(Heightmap::is_opaque(Types::MotionBlockingNoLeaves, water));
        assert!(
            !Heightmap::is_opaque(Types::MotionBlockingNoLeaves, leaves),
            "leaves block motion but are excluded"
        );
    }

    #[test]
    fn update_places_and_lowers_like_java() {
        // A fresh WorldSurface heightmap: every stored entry is 0, so
        // `first_available` is `min_y` (-64) everywhere (Java: never-set ->
        // `min_y`). Local coords are absolute Y for the update (Java passes
        // ProtoChunk's `y`).
        let mut hm = Heightmap::new(384);
        let min_y = -64;
        // Place stone (id 1) at y=-64: localY >= firstAvailable (-64 >= -64),
        // opaque -> set height -64 + 1 = -63 (stored 1). getHeight is
        // `firstAvailable - 1`, the topmost opaque block's Y: -64.
        assert!(hm.update(
            0,
            -64,
            0,
            Types::WorldSurface,
            state_flags(1),
            min_y,
            |_| { state_flags(0) }
        ));
        assert_eq!(hm.get_height_at(0, 0, min_y), -64);
        // Now first_available is -63. Placing a second stone at y=-64 is
        // opaque but one below the top (-64 < -63), so the opaque branch's
        // `localY >= firstAvailable` check fails and the method falls through
        // to Java's trailing `return false`.
        assert!(!hm.update(
            0,
            -64,
            0,
            Types::WorldSurface,
            state_flags(1),
            min_y,
            |_| { state_flags(0) }
        ));
        // Raising the column: place stone at y=-63 (>= -63). The stored height
        // becomes -62, i.e. the topmost opaque is now at -63 -> getHeight -63.
        assert!(hm.update(
            0,
            -63,
            0,
            Types::WorldSurface,
            state_flags(1),
            min_y,
            |_| { state_flags(0) }
        ));
        assert_eq!(hm.get_height_at(0, 0, min_y), -63);
    }

    #[test]
    fn update_removal_rescans_downward_for_the_next_opaque() {
        let mut hm = Heightmap::new(384);
        let min_y = -64;
        // Two stacked stones: place at -64 then -63 -> topmost -63.
        assert!(hm.update(
            0,
            -64,
            0,
            Types::WorldSurface,
            state_flags(1),
            min_y,
            |_| { state_flags(0) }
        ));
        assert!(hm.update(
            0,
            -63,
            0,
            Types::WorldSurface,
            state_flags(1),
            min_y,
            |_| { state_flags(0) }
        ));
        assert_eq!(hm.get_height_at(0, 0, min_y), -63);
        // Remove the top stone (place air at -63): firstAvailable - 1 == -63,
        // so Java walks down from -62. The flags_at closure resolves the real
        // below state (stone at -64) -> new topmost -64 -> getHeight -64.
        assert!(
            hm.update(0, -63, 0, Types::WorldSurface, state_flags(0), min_y, |y| {
                state_flags(if y == -64 { 1 } else { 0 })
            })
        );
        assert_eq!(hm.get_height_at(0, 0, min_y), -64);
    }

    #[test]
    fn update_removal_with_no_opaque_below_sets_min_y() {
        let mut hm = Heightmap::new(384);
        let min_y = -64;
        // A single stone at -64: stored height -63, getHeight -64.
        assert!(hm.update(
            0,
            -64,
            0,
            Types::WorldSurface,
            state_flags(1),
            min_y,
            |_| { state_flags(0) }
        ));
        assert_eq!(hm.get_height_at(0, 0, min_y), -64);
        // Remove it: the downward scan finds nothing -> `setHeight(minY)`,
        // which stores 0; getHeight is then `minY - 1` (an empty column).
        assert!(hm.update(
            0,
            -64,
            0,
            Types::WorldSurface,
            state_flags(0),
            min_y,
            |_| { state_flags(0) }
        ));
        assert_eq!(hm.get_height_at(0, 0, min_y), -65);
        // A removal not at the top edge is a no-op: with the column empty,
        // `firstAvailable` is `minY`, so air at -63 is neither the opaque
        // raise branch nor `firstAvailable - 1 == localY` (-65 == -63 is
        // false) — Java falls through to `return false` and the column stays
        // empty.
        assert!(!hm.update(
            0,
            -63,
            0,
            Types::WorldSurface,
            state_flags(0),
            min_y,
            |_| { state_flags(0) }
        ));
        assert_eq!(hm.get_height_at(0, 0, min_y), -65);
    }

    #[test]
    fn worldgen_heightmaps_is_the_two_worldgen_types() {
        // `ChunkStatus.WORLDGEN_HEIGHTMAPS` — `EnumSet.of(OCEAN_FLOOR_WG,
        // WORLD_SURFACE_WG)`, the two `Usage.WORLDGEN` types a `ProtoChunk`
        // updates before it reaches `CARVERS`.
        assert_eq!(
            WORLDGEN_HEIGHTMAPS,
            [Types::WorldSurfaceWg, Types::OceanFloorWg]
        );
        for ty in WORLDGEN_HEIGHTMAPS {
            assert!(!ty.send_to_client(), "worldgen types are never sent");
            assert!(!ty.keep_after_worldgen(), "worldgen types are dropped");
        }
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
        // Java warns and re-primes on a length mismatch; the pure value ignores
        // the data (the re-prime lives on `ChunkAccess::prime_heightmaps`). A
        // shorter array must not clobber or panic.
        let mut hm = Heightmap::new(384);
        hm.set_raw_data(&[1, 2, 3]);
        assert!(hm.get_raw_data().iter().all(|&v| v == 0));
    }
}
