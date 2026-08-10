//! `net.minecraft.world.level.chunk.storage` — the region-file chunk-IO layer
//! (issue #231, the `chunk.storage` manifest units).
//!
//! The file-backed container layer on the `RegionBitmap` + `RegionFileVersion`
//! foundation: `RegionFile` sector/header management, the read/write/clear
//! path, `ChunkBuffer` semantics, `padToFullSector`, external `.mcc` payloads,
//! exact compression dispatch, corruption/failure handling, and both header
//! recalc tiers — all ported from Paper 26.2's `RegionFile.java` and pinned by
//! `docs/region-file-format-spec.md`.
//!
//! - `region_bitmap` — `RegionBitmap` (§3 of the region-file spec): the exact
//!   `java.util.BitSet`-equivalent for per-file sector allocation.
//! - `region_file_version` — `RegionFileVersion` (§5): the codec registry by
//!   id (1 gzip / 2 deflate / 3 none / 4 lz4 / 127 custom) plus the
//!   `server.properties` `region-file-compression` selection surface.
//! - `region_file` — `RegionFile` + `ChunkBuffer` (§4, §6, §7, §8, §9, §10):
//!   the file-backed container. Also carries the local
//!   `get_chunk_coordinate`/`get_last_world_save_time` helpers (the recalc
//!   slot matching needs them; `SerializableChunkData` is a later wave) and
//!   `get_region_file_coordinates` (the `r.<x>.<z>.mca` parser `RegionFileStorage`
//!   also uses).
//! - `region_storage_info` — `RegionStorageInfo`, flattened to a plain
//!   `is_chunk_data: bool` per the storage-worker amendment in `OWNERSHIP.md`.
//!
//! Neither the bitmap nor the codec registry touches game state: allocation is
//! per-`RegionFile` derived state, and the codec registry/selection is a frozen
//! value. `RegionFile` is owned by the region's single IO worker behind a
//! `Mutex<RegionFile>` (OWNERSHIP.md) — never `Arc<RwLock>` game state.

// RivetTodo(#231): `RegionFileStorage` (the LRU `Long2ObjectLinkedOpenHashMap`
// region cache + `nonExistingRegionFiles` negative cache; `getRegionFile`,
// `read`/`write`/`flush`/`close` — the `RegionFileSizeException` it throws is
// already ported in `region_file.rs`) is deferred with the rest of the storage
// wave. Its thin read-side surface is implementable
// without IOWorker/async, but `read`'s coordinate guard
// (`SerializableChunkData.getChunkCoordinate` vs the requested pos) and the
// Aikar `readOversizedChunk` merge duplicate the helpers already ported here,
// so it lands together with the chunk-payload wave. `IOWorker`'s `PendingStore`
// coalescing, the moonrise `RegionDataController` interfaces
// (`moonrise$startWrite`/`moonrise$finishWrite`/`moonrise$readData`/
// `moonrise$finishRead`), and `SimpleRegionStorage`'s coordinate guard also
// land with that wave. The legacy Aikar oversized subsystem (§6.2) — per-chunk
// `*.oversized_<x>_<z>.nbt` files, the `.oversized.nbt` meta, `isOversized`/
// `setOversized`, and the recalc branches that read them — is deferred here:
// nothing creates those files anymore, so a modern-world round-trip never
// touches them. Codec coverage per DECISIONS.md D13: gzip/none write and
// gzip/deflate read are wired on `flate2`; lz4 **read** (the lz4-java "LZ4
// Block" format: `lz4_flex` + `xxhash-rust` per CRATES.md) and deflate/lz4
// **write** (Java `Deflater` is not `flate2`-reproducible) stay deferred.

pub mod region_bitmap;
pub mod region_file;
pub mod region_file_version;
pub mod region_storage_info;
pub mod serializable_chunk_data;

pub use region_bitmap::RegionBitmap;
pub use region_file::{
    ChunkBuffer, RegionFile, RegionFileSizeException, get_chunk_coordinate,
    get_last_world_save_time, get_region_file_coordinates,
};
pub use region_file_version::RegionFileVersion;
pub use region_storage_info::RegionStorageInfo;
