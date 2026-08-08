# Paper region file (.mca) on-disk format specification

Authoritative format spec for Rivet's port of Paper 26.2's region-file layer
(`net.minecraft.world.level.chunk.storage`, the `chunk.storage` manifest units of issue #231).
This is a **specification only**: it pins what a byte-identical round-trip and read parity must
implement, it does not implement storage code and it does not invent APIs.

**Sources of truth (read before changing this document):**

- `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/chunk/storage/RegionFile.java`
- `.../storage/RegionFileVersion.java`
- `.../storage/RegionBitmap.java`
- `.../storage/RegionFileStorage.java`
- `.../storage/IOWorker.java`
- `.../storage/SimpleRegionStorage.java`
- `.../storage/SectionStorage.java`
- `.../storage/RegionStorageInfo.java`
- `.../storage/SerializableChunkData.java` (chunk coordinate / save-time helpers only)
- `.../net/minecraft/nbt/NbtIo.java` (root-tag framing)
- `.../net/minecraft/world/level/ChunkPos.java` (region math)

Conventions used throughout: **[Paper]** marks a Java/Paper fact; **[Rivet]** marks a decision Rivet
makes about its own implementation; **[Deferred]** marks work explicitly deferred to the #231 wave
(or later) and not part of this spec's guarantees. Any field marked *big-endian* is stored as Java
`DataInput`/`DataOutput` big-endian integers. All byte lengths are in bytes unless stated.

---

## 1. File naming and coordinates

**[Paper]** Region files live one per region: `r.<regionX>.<regionZ>.mca` under the storage folder
(which also holds oversized/external chunk files, §6). A chunk at absolute column `(x, z)` belongs to
region `(x >> 5, z >> 5)`; its slot inside the region file is `localX = x & 31`, `localZ = z & 31`.
There is exactly one region file per 32×32-chunk region.

**[Rivet]** Keep the same filename derivation and slot math verbatim — region-file names are the
collaboration boundary with the on-disk world format and with Paper.

---

## 2. Header: 8192 bytes, sectors 0 and 1

**[Paper]** The file begins with a fixed 8192-byte header occupying sectors 0 and 1. It is two
arrays of 1024 big-endian 4-byte ints:

| Byte offset | Content |
| --- | --- |
| `0x0000` | **Locations** — 1024 entries, index `i = localX + localZ * 32` |
| `0x1000` | **Timestamps** — 1024 entries, same indexing |

- **Location entry (4 bytes):** `sector_offset << 8 | sector_count`, where `sector_offset` is the
  start sector (≥ 2) of the chunk's data and `sector_count` is its length in 4096-byte sectors.
  The low byte (`offset & 0xFF`) is the sector count; `sector_count == 0` **and** `sector_offset == 0`
  means "chunk not present". A location of `0` is the canonical empty marker.
- **Timestamp entry (4 bytes):** Unix epoch **seconds** (`epochMillis / 1000`, truncated), written on
  every write/clear; not otherwise read or validated by the storage layer. On header recalc the
  timestamp is rewritten for present chunks and zeroed for absent ones (§8).
- The sector count occupies the low 8 bits and the sector offset the next 24; on read
  `getSectorNumber = (offset >> 8) & 0xFFFFFF` and `getNumSectors = offset & 0xFF`, so a present
  chunk always decodes to a count in `1..=255` (count 0 is only valid when the whole location is 0,
  the absent marker). A count of exactly `255` is the *maxed* sentinel (§4). The sector-offset field
  is masked to 24 bits on read (`offset >> 8 & 16777215`), so any sector number needing more than
  24 bits is truncated to its low 24 bits.

**[Rivet]** Pack and unpack exactly as above; no reinterpretation of the packed int is allowed. The
byte-identity gate (D13) treats the 8192-byte header as opaque bytes produced by these rules — do not
reorder, reformat, or add metadata.

---

## 3. Sector allocation model (`RegionBitmap`)

**[Paper]** After the 2-sector header, the file is a sequence of 4096-byte sectors. Sector allocation
is tracked in-memory by a `RegionBitmap` (a `java.util.BitSet`) that is loaded from the header at
open time and mutated as chunks are written/cleared:

- `force(start, size)` — mark `[start, start+size)` used (unchecked).
- `free(start, size)` — clear `[start, start+size)`.
- `tryAllocate(start, size)` — allocate exactly `[start, start+size)` **only if** that run contains no
  used bit; fails (returns false) if any used sector intersects.
- `allocate(size)` — first-fit: starting from sector 0 each call, find the first free run of at least
  `size` clear bits and claim its first `size` bits (`current` is a per-call local, not a persistent
  cursor).

At open time the header's locations are replayed into the bitmap (a corrupt/overlapping header may
trigger recalc, §8). `usedSectors.force(0, 2)` reserves the header. **Nothing in the format itself
records free space** — allocation is derived state, which is why corrupt headers can only be repaired
by re-deriving sector usage from chunk payloads (§8).

**[Rivet]** Port `RegionBitmap` as an exact `BitSet`-equivalent (paper must remain byte-identical:
allocation order affects sector placement on-disk). Keep first-fit allocation and the exact
force/free/tryAllocate/allocate semantics.

---

## 4. Chunk data: per-chunk stream, 4096-byte sectors

**[Paper]** A chunk's on-disk record is a **stream** laid out inside `sector_count * 4096` bytes:

| Field | Size | Meaning |
| --- | --- | --- |
| `length` | 4, big-endian | **Payload length field** = `stream_length + 1` (§ below) |
| `compression_type` | 1 | Codec id (§5); high bit `0x80` = external-stream flag (§6) |
| `payload` | `stream_length` bytes | Codec-wrapped NBT: [1 byte type + 2-byte UTF name + NBT body] |

Two lengths appear and must not be confused:

- **`stream_length`** = the payload byte count, i.e. the compressed NBT bytes between the
  compression byte and the end of the record (`stream_length = payload_byte_count`).
- **`length`** (the 4-byte field) = `stream_length + 1`, i.e. it counts the compression byte too.
  On write `ChunkBuffer.close()` computes the field as `buffer_count - 5 + 1` where `buffer_count`
  is the total record length (5-byte prefix `[0,0,0,0, compression]` + compressed payload); since
  `buffer_count = 5 + payload_byte_count`, the field is `payload_byte_count + 1 = stream_length + 1`.

**Minimum `length` is 1** (a record of exactly the compression byte → `stream_length == 0`).
A `length == 0` is never produced on the happy path; the reader treats `length == 0` as "allocated
but stream missing" (corruption, §8).

**Sector sizing:** `sectors = ceil(total_bytes / 4096)` where `total_bytes = 5 + stream_length`.
`sizeToSectors` is `(size + 4096 - 1) / 4096`. A record stays internal when `sectors <= 255`, i.e.
`stream_length <= 255 * 4096 - 5`; at exactly `sectors == 255` the header stores count 255 (the
maxed sentinel). A record needing `sectors >= 256` is external (§6).

**Maxed-count sentinel (Spigot legacy):** if a header location's sector count is `255`, the count is
"maxed out": the reader reads the *actual* 4-byte length field from the **chunk's own first sector**
(`sectorNumber * 4096`) and recomputes `sectors = (length + 4) / 4096 + 1`. Paper's own writer can emit count 255 (a chunk whose record
lands exactly in 255 sectors is written internally, since the oversized redirect only triggers at
`sizeToSectors >= 256`), so this path also covers legitimately-written 255-sector records, not just
third-party files. Note the sentinel recompute is `floor((length+4)/4096) + 1`, which is
`sizeToSectors` except when the record is an exact multiple of 4096 — a Paper edge case the port
should reproduce rather than "fix".

**[Rivet]** Replicate `sizeToSectors`, the two distinct length meanings, and the 255 sentinel exactly.
Do **not** "simplify" the length field to exclude the compression byte — the byte-identity gate
depends on it.

---

## 5. Compression (codec) ids — `RegionFileVersion`

**[Paper]** `RegionFileVersion` registers codecs by id; the id is written as the `compression_type`
byte of every chunk stream. The **writer** uses one codec for the whole region file — the
*selected* version, `RegionFileVersion.getSelected()` — except the id `127` custom which can never be
selected.

| id | name | paper on read | paper on write | `fromId` |
| --- | --- | --- | --- | --- |
| `1` | gzip | `GZIPInputStream` | `GZIPOutputStream` | yes |
| `2` | deflate | `InflaterInputStream` | `DeflaterOutputStream` | yes |
| `3` | none | identity | identity | yes |
| `4` | lz4 | `LZ4BlockInputStream` | `LZ4BlockOutputStream` | yes |
| `127` | custom | unwrap path reads a modified-UTF-8 string id, logs, returns null (never crashes) | never writable (output wrapper throws `UnsupportedOperationException`) | yes |

- **Selection:** `RegionFileVersion.DEFAULT = VERSION_DEFLATE`; `configure(optionName)` switches the
  selected version from `server.properties` `region-file-compression`. [Rivet] under D13 the M2
  byte-identity gate pins `region-file-compression=none`, so Rivet's writer is only required to emit
  id `3` for the round-trip gate; the other ids are read-side-only requirements (below).
- **Invalid ids:** `RegionFileVersion.fromId` returns null for any unregistered id; the reader then
  treats the stream as corrupt (§8). [Rivet] the read path must therefore accept `1..=4, 127` and
  reject everything else exactly the way `fromId == null` does.

**[Rivet] read-side codec coverage (D13 split):**

- gzip (`1`) — proven elsewhere in the codebase (`NbtIo.read_compressed` on Paper's `level.dat`
  fixture); region-layer gzip read rides on it.
- none (`3`) — identity; the round-trip gate.
- deflate (`2`) and lz4 (`4`) — **read** support must exist for `read-all-codecs` parity (the "read
  support is mode-separate" sentence in D13); **write** parity for deflate/lz4 is **[Deferred]** to
  the #231 wave because Java `Deflater` output is not `flate2`-reproducible in general and lz4 write
  is not ported. [Rivet] do not emit `2`/`4` before that deferral is resolved.
- `127` custom — read path must match Paper: read a modified-UTF-8 string, log "Unrecognized custom
  compression {id}" when it parses as an identifier or "Invalid custom compression id {id}" when it
  does not, and return null in both cases (never crash); it is never written.

---

## 6. External (oversized) chunks

**[Paper]** Two mechanisms exist for chunks that do not fit in 255 sectors; both are **write**
redirects (a reader never splits a chunk itself):

### 6.1 Modern (Mojang-style) external file — `.mcc`

If `sizeToSectors(dataSize) >= 256` (`EXTERNAL_CHUNK_THRESHOLD = 256`, i.e. the count would not fit
in the header low byte), the chunk is written to a separate file and the region file gets a **stub**
record instead:

- External filename: `c.<chunkX>.<chunkZ>.mcc` in the external-file directory (the storage folder).
- **Write sequence** (`RegionFile.write`, oversized branch):
  1. Allocate **1 sector** (`usedSectors.allocate(1)`).
  2. Write the **codec-wrapped payload only** to `c.<x>.<z>.mcc` via a temp file (`tmp...` in the
     same dir) that is then atomically moved over the target. The external file holds exactly the
     bytes after the 5-byte prefix — i.e. the compressed NBT payload with **no length field and no
     compression byte** (the codec id lives in the region-file stub, and the reader wraps the
     external file with that codec).
  3. Write a 5-byte **stub** into the allocated sector:
     `length = 1` (`putInt(1)`) then `compression_type = version.getId() | 0x80`.
  4. Update the header location to `sector_offset << 8 | 1` and timestamp; write header; then the
     external file move completes (commit ordering §10).
- On the **read** path the stub is detected via `compression_type & 0x80`; `stream_length` is then
  `length - 1`. A valid external stub has `length == 1` (`stream_length == 0`); a `length != 1`
  logs "has both internal and external streams", triggers header recalc + retry (§8), and if recalc
  does not resolve it, **still falls through to read the external file** — it is a warning, not a
  hard corruption. The real codec id is `compression_type & ~0x80`; the payload is read from the
  external file and unwrapped with that codec.
- The 5-byte stub is thus exactly `[00 00 00 01][id | 0x80]`.

**Size guard (Paper only):** the in-memory buffer is capped at `MAX_CHUNK_SIZE = 500 * 1024 * 1024`
bytes; exceeding it throws `RegionFileSizeException`, which the caller converts to a **delete**
(`region.clear(pos)`, §9) — a too-large chunk is dropped, never partially written.

**[Rivet]** The `.mcc` stub + external-file mechanism is part of the format and must round-trip. The
`RegionFileSizeException` delete path is Paper behavior — replicate it (drop the chunk) rather than
"fix" it by writing a partial or oversized internal record.

### 6.2 Legacy Aikar-style oversized — `*.oversized_<x>_<z>.nbt` + `.oversized.nbt` meta

`RegionFile` also carries **Aikar-style** oversized chunk support: a per-chunk file
`<regionfile-base>_oversized_<x>_<z>.nbt` — i.e. the `.mca` filename minus its extension plus an
underscore, e.g. `r.0.0_oversized_3_5.nbt` — (deflate-compressed NBT read via `InflaterInputStream`)
and a 1024-byte meta file `<regionfile-base>.oversized.nbt` (one flag byte per slot).
`setOversized(x, z, bool)`
writes/removes the meta file and the per-chunk file; the flag means "when loading this chunk, also
read `<regionfile-base>_oversized_<x>_<z>.nbt` and merge its `Entities` / `TileEntities` lists into
the region-file record's lists". The region file still holds a real (possibly partial) chunk compound;
the oversized file supplements it.

**[Paper]** This is **legacy-write path**: `RegionFileStorage.write` (the moonrise-rewritten path
used for chunks) explicitly calls `region.setOversized(..., false)` and the comment says "We don't do
this anymore, mojang stores differently, but clear old meta flag if it exists". The only active
producers of Aikar files are old saves / the header-recalc path, which re-derives the meta flags
from existing `*.oversized.nbt` files (§8).

**[Rivet]** **[Deferred]** — implement the Aikar-side read (so old worlds don't lose chunk data) in
the #231 wave; the write path (`setOversized` writing/clearing) is not needed for a byte-identical
round-trip at `none` compression and is deferred with it. The `.mcc` mechanism (§6.1) is the one the
round-trip gate exercises.

---

## 7. Read path — `getChunkDataInputStream`

**[Paper]** Sequence for `ChunkPos pos`:

1. `offset = offsets[localX + localZ*32]`; `offset == 0` → chunk absent, return null.
2. `sectorNumber = (offset >> 8) & 0xFFFFFF`; `numSectors = offset & 0xFF`; if `numSectors == 255`,
   read the 4-byte length from the chunk's own first sector (`sectorNumber * 4096`) and set
   `numSectors = (length + 4)/4096 + 1` (§4).
3. Read `numSectors * 4096` bytes at `sectorNumber * 4096`.
4. Corruption checks (any failure → §8; the "has both" case is a warning that still reads the
   external file if recalc does not fix it):
   - `< 5` bytes available → truncated chunk header.
   - `length == 0` → allocated but stream missing.
   - `compression_type & 0x80` (external): `stream_length = length - 1`; `stream_length != 0`
     ("has both internal and external streams") → warn + recalc + fall through to external read;
     invalid codec id after masking → invalid stream version.
   - else: `stream_length = length - 1`; `stream_length > remaining` → truncated stream;
     `stream_length < 0` (i.e. `length == 0` was handled above, so this is a negative `length`) →
     declared size negative.
5. Unwrap with codec `compression_type` (§5) and hand a `DataInputStream` of the payload to the NBT
   reader.

`RegionFileStorage.read` then verifies the parsed root is a chunk of the *requested* coordinate via
`SerializableChunkData.getChunkCoordinate` (which reads `xPos`/`zPos`; for DataVersion < 2842 it
reads them from the `Level` sub-tag); a mismatch triggers header recalc and a retry (§8). This
coordinate check only runs for `CHUNK`-type storages (`isChunkData`).

**[Rivet]** Port the checks in the same order with the same thresholds; they are part of the format's
robustness contract, not incidental logging. `length == 0` must be surfaced as corruption, not
treated as "empty chunk".

---

## 8. Corruption / error boundaries and header recalc

**[Paper]** Two tiers:

**Tier 1 — per-read soft failure.** Any anomaly in §7 returns null (chunk treated as absent) after
logging; on `CHUNK`-type region files (`canRecalcHeader == true`) it first attempts
`recalculateHeader()` and recursively retries after each successful recalc. In practice a successful
recalc relinks the slot to valid data or clears it, terminating the retry. Non-CHUNK files (e.g.
POI/entities with `DataFixTypes`
other than `CHUNK`) do not recalc — the read simply returns null; the slot is left untouched on the
per-read path. (The "log and delete the slot" behavior for non-CHUNK files exists only in the
constructor's header-replay loop, below, when an *open-time* header entry is invalid — not on read.)

**Tier 2 — full header recalculation (`recalculateHeader()`).** Triggered on open when the header
replay fails (invalid sector < 2, `numSectors <= 0`, out-of-bounds, or overlapping allocations) and
by the soft-failure path above. Steps:

1. Back up the file (`<file>.<random>.backup`), then **scan every sector** from sector 2 up to
   `min(roundToSectors(fileSize), 0x7FFFFF)` — the constant is `Integer.MAX_VALUE >>> 8`, i.e. it is
   *not* the 24-bit sector mask — looking for valid chunk streams (4-byte length via `getLength` +
   `attemptRead`); after each successful read the scan jumps ahead by that chunk's sector span, and
   after a failed read it advances by exactly one sector.
2. `attemptRead` validates `0 <= length` and that `sector*4096 + 4 + length <= fileLength`, reads
   exactly `length` bytes at `sector*4096 + 4` (a short read is a failure), unwraps the codec, and
   parses NBT; any failure returns null. A compression byte with the external bit set returns the
   `OVERSIZED_COMPOUND` marker and is skipped for local data.
3. Chunks are matched to slots by `SerializableChunkData.getChunkCoordinate`; newer
   `LastUpdate` wins (strictly-greater keeps the incumbent, so equal timestamps are overwritten by
   whichever is scanned later); records carry `rawLength` (the length field value + 4) and
   `sectorOffset` so the new header can reference the existing sectors in place.
4. **External (`.mcc`) and Aikar oversized are decided separately:** each `c.<x>.<z>.mcc` file within
   the region's bounds is read by trying every registered codec; the slot is marked oversized if that
   compound's `LastUpdate` is newer than the locally-found compound (ties prefer the local record).
   Aikar `*.oversized_<x>_<z>.nbt` files (read via `InflaterInputStream`) mark the slot when their
   `LastUpdate` **equals** the local compound's — "best we got for an id".
5. New locations are computed with a **fresh bitmap** (`force(0,2)` then re-allocating each slot),
   so overlapping/duplicate data gets only one owner; oversized stubs are re-emitted into newly
   allocated single sectors, each stub carrying the codec id that was detected for its `.mcc` file.
6. Timestamps are rewritten (present → now, absent → 0) — "simply destroy the timestamp header".
7. The header is flushed and `force(true)`-ed.

After recalc the in-memory `usedSectors` is `copyFrom` the fresh bitmap.

**[Rivet]** Port both tiers. The recalc must reproduce `roundToSectors`, the `LastUpdate`-based
newest-wins tie-break, the `getChunkCoordinate` slot matching, and the "fresh bitmap" re-allocation
— this is a data-recovery path where subtle divergence silently mis-links chunks. `roundToSectors`:
`sectors = bytes >>> 12; rem = bytes & 4095; sectors + (rem != 0 ? 1 : 0)`.

---

## 9. Chunk deletion (`clear`), existence checks

**[Paper]**

- `clear(pos)`: if `offset != 0`, zero the location entry, write `now` to the timestamp entry,
  rewrite the header, delete `c.<x>.<z>.mcc` if present, and free the old sectors in the bitmap.
  The freed sectors are not zeroed and the file is not truncated.
- `hasChunk(pos)` / `doesChunkExist(pos)`: location nonzero (and for existence, the stream header
  validates as a real stream of a registered codec, with `streamLength` within the sector span; for
  external chunks the `.mcc` file must exist).

**[Rivet]** Port `clear` with the same header-then-free order; do not zero freed sectors and do not
shrink the file — Paper does neither, and truncation would break sector-number stability.

---

## 10. Write path and flush/close

**[Paper]**

- **`getChunkDataOutputStream(pos)`** returns `new DataOutputStream(version.wrap(new ChunkBuffer(pos)))`.
  `ChunkBuffer` (a `ByteArrayOutputStream` with 5-byte prefix) accumulates the payload; its
  `close()` patches the 4-byte length, then (unless write-on-close is disabled by the moonrise
  layer) calls `RegionFile.write`.
- **`RegionFile.write(pos, data)`** (§4, §6.1): allocate sectors (oversized → 1 + external file +
  stub, else `ceil/4096`), write data, patch location+timestamp, `writeHeader()`, run the commit op,
  then free the old sectors if any. Note the order: header is written *before* the external-file
  move and before freeing old sectors; the free of the previous location happens **last**.
- **`flush()`** = `file.force(true)` (FileChannel fsync). **`close()`** = `padToFullSector()`
  (write one padding byte at `paddedSize - 1` if the file is not a multiple of 4096, extending it to
  a full sector) in a `finally`, then `force(true)`, then `file.close()`.
- **Header write (`writeHeader`)** rewrites all 8192 bytes (`header.position(0)` then
  `file.write(header, 0)`); the in-memory header buffer is kept in sync by every mutation.

**Commit ordering for the byte-identity gate:** on a *new* write of an existing chunk the old
sectors are freed only after the new record + header are durable in the buffer. On the oversized
branch the external file's temp→target move is the commit op and runs after the header write.
A crash can leave a *stale* location pointing at old freed data (Paper accepts this); the gate's
fixtures are written on a quiescent server, so ordering must still match Paper's for byte identity.

**[Rivet]** Port `ChunkBuffer` sizing (initial 8096-byte buffer) and the length patch at close, the
write-order steps, and pad-to-sector on close exactly. `flush`/`force` semantics carry over to
Rivet's `File`/fsync handling. The moonrise "write on close" flag is a control-flow seam, not a
format concern; keep an equivalent toggle so the write can be split from serialization.

---

## 11. IOWorker ordering relevant to the region-file layer

**[Paper]** `IOWorker` is the async façade over `RegionFileStorage`; it is a
`PriorityConsecutiveExecutor` with three priorities: `FOREGROUND`, `BACKGROUND`, `SHUTDOWN`. The
region-file layer only ever sees work serialized through this executor:

- **Pending-write coalescing:** `store(pos, data)` records the latest `CompoundTag` per `ChunkPos`
  in `pendingWrites` (a `LinkedHashMap`) and returns the same shared future on re-store; that future
  completes later when the `BACKGROUND` runnable performs `storePendingChunk` → `storage.write`.
  A `loadAsync` / `scanChunk` for a pos with a pending write serves the in-memory copy, not the disk.
- **Write serialization:** all region-file access is synchronized on the single `RegionFile`
  (`RegionFile.write` and `getChunkDataInputStream` are `synchronized`); the executor provides
  ordering so a chunk's write is not interleaved with its own read, and `synchronize(flush)` drains
  pending writes (`allOf` the store futures) and then `storage.flush()`.
- **Shutdown:** `close()` sets `shutdownRequested`, schedules a `SHUTDOWN`-priority barrier, closes
  the executor, then closes storage. No new work is accepted after the barrier.

**[Rivet]** The IOWorker ordering that matters to the region-file layer is: (a) one logical writer per
region file (mutual exclusion over a `RegionFile` handle), (b) pending-write coalescing so the last
`store` for a chunk is what lands on disk, (c) drain-then-flush on `synchronize`, (d) no writes after
shutdown. Replicate these invariants; the executor mechanics (Java `Concurrent`/`Priority`
threading) are an implementation surface, not part of the on-disk format, and Rivet's own threading
model (D5) governs their port.

---

## 12. Relationship to the rest of #231 scope

`RegionFile` is the byte container; `SerializableChunkData` is the chunk payload written into it
(the write path is `SerializableChunkData.write()`, not legacy NBT — issue #231). This spec pins the
container only. The chunk NBT content (key order, codecs, `DataVersion`) is governed by D12/D13 and
the NBT wave, not by this document. `RegionFileStorage` caching (LRU `Long2ObjectLinkedOpenHashMap`,
`MAX_CACHE_SIZE = 256` as the default when global config is unavailable, overridable via Paper's
`misc.regionFileCacheSize`; `nonExistingRegionFiles` negative-cache of size `1024 * 4 = 4096`) is a
read-side performance/behavior surface; it affects observable file-open/close patterns but not bytes
on disk.

**[Rivet]** Keep the region-cache eviction order (`getAndMoveToFirst`/`removeLast`, close-on-evict)
faithful if byte-identity fixtures ever assert on file-handle lifecycles; otherwise it is an
implementation detail.

---

## 13. Deferred / out-of-scope summary

- **[Deferred]** deflate (`2`) and lz4 (`4`) **write** parity (Java `Deflater` non-reproducible; lz4
  write not ported) → #231 wave; read support still required.
- **[Deferred]** Aikar-style oversized write path and meta-file handling (§6.2) — legacy-only; read
  path (to not lose old data) in the #231 wave.
- **[Deferred]** DataFixer upgrade of chunk tags (SimpleRegionStorage.upgradeChunkTag) — NBT/datafix
  wave, not region-file format.
- **[Out of scope]** `region-file-compression=deflate` byte identity: D13 pins the gate to `none`;
  deflate byte identity is a known non-goal until proven otherwise.
- **[Out of scope]** SectionStorage content codecs, POI/entity storage formats (same container,
  different payloads).
