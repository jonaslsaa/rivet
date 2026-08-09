//! `net.minecraft.world.level.chunk.storage` — the region-file chunk-IO layer
//! (issue #231, the `chunk.storage` manifest units).
//!
//! Phase 0/A foundation — the smallest coherent slice justified by the two
//! committed specs (`docs/region-file-format-spec.md`,
//! `docs/serializable-chunk-data-spec.md`) and pinned Paper 26.2 sources: the
//! two pure, IO-free primitives the later file-backed wave builds on.
//!
//! - `region_bitmap` — `RegionBitmap` (§3 of the region-file spec): the exact
//!   `java.util.BitSet`-equivalent for per-file sector allocation.
//! - `region_file_version` — `RegionFileVersion` (§5): the codec registry by
//!   id (1 gzip / 2 deflate / 3 none / 4 lz4 / 127 custom) plus the
//!   `server.properties` `region-file-compression` selection surface.
//!
//! Neither primitive touches disk or game state: allocation is per-`RegionFile`
//! derived state, and the codec registry/selection is a frozen value (see the
//! storage-worker amendment in `OWNERSHIP.md`).

// RivetTodo(#231): the file-backed `RegionFile` (8192-byte header replay,
// `getChunkDataInputStream`'s §7 corruption checks, the `ChunkBuffer`/`write`/
// `clear`/`padToFullSector` path, `.mcc` externals, both `recalculateHeader`
// tiers), `RegionFileStorage`'s LRU + negative caches, `IOWorker`'s
// `PendingStore` coalescing, and `SimpleRegionStorage`'s coordinate guard all
// land with the region read/write wave. Codec coverage per DECISIONS.md D13:
// gzip/none write and gzip/deflate read are wired on `flate2` in
// `region_file_version`; lz4 **read** (the lz4-java "LZ4 Block" format:
// `lz4_flex` + `xxhash-rust` per CRATES.md) and deflate/lz4 **write** (Java
// `Deflater` is not `flate2`-reproducible; lz4 compressor not ported) stay
// deferred, and the id-127 custom read path lands with the file-backed read.

pub mod region_bitmap;
pub mod region_file_version;

pub use region_bitmap::RegionBitmap;
pub use region_file_version::RegionFileVersion;
