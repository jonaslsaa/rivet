# SerializableChunkData chunk-NBT on-disk format specification

Authoritative format spec for Rivet's port of Paper 26.2's chunk payload —
the compound NBT written into each region-file record by
`net.minecraft.world.level.chunk.storage.SerializableChunkData` (the
`chunk.storage` manifest units of issue #231). This is a **specification only**:
it pins what a byte-identical round-trip and a read-parity implementation must
honor; it does not implement storage code and it does not invent APIs.

**This document is the payload companion to the region-file container spec.**
The 4096-byte-sector framing, compression ids, external/oversized chunks,
header recalc, and IOWorker ordering live in
`docs/region-file-format-spec.md` (branch `docs/region-file-format-spec`,
commits `1d8a061..08e6584`); do not duplicate them here. §4 of that spec
defines the per-chunk stream (`length`, `compression_type`, codec-wrapped NBT)
that this document's CompoundTag is wrapped in.

**Sources of truth (read before changing this document):**

- `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/chunk/storage/SerializableChunkData.java` — the primary file (847 lines; all line refs below are to it)
- `.../net/minecraft/nbt/NbtIo.java` — root-tag framing (`writeUnnamedTagWithFallback`, `read`)
- `.../net/minecraft/nbt/NbtUtils.java` — `addCurrentDataVersion` (L522-527)
- `.../net/minecraft/world/level/chunk/status/ChunkStatus.java` — status ordering, chunk type, heightmap sets
- `.../net/minecraft/world/level/chunk/PalettedContainer.java`, `.../PalettedContainerRO.java`, `.../Strategy.java`, `.../Configuration.java`, `.../PalettedContainerFactory.java` — `block_states`/`biomes` codecs
- `.../net/minecraft/world/level/levelgen/Heightmap.java` — serialization keys
- `.../net/minecraft/world/level/levelgen/blending/BlendingData.java`, `.../net/minecraft/world/level/levelgen/BelowZeroRetrogen.java` — optional codecs
- `.../net/minecraft/world/level/chunk/UpgradeData.java` — `Indices`/`Sides`/neighbor ticks
- `.../net/minecraft/world/ticks/SavedTick.java`, `.../net/minecraft/world/ticks/LevelChunkTicks.java`, `.../net/minecraft/world/ticks/ProtoChunkTicks.java`, `.../net/minecraft/world/ticks/ScheduledTick.java` — tick codec and packing
- `.../net/minecraft/world/level/chunk/DataLayer.java` — 2048-byte light layer
- `.../ca/spottedleaf/moonrise/patches/starlight/util/SaveUtil.java`, `.../patches/starlight/storage/StarlightSectionData.java` — starlight light tags
- `.../ca/spottedleaf/moonrise/patches/chunk_system/scheduling/task/ChunkLoadTask.java`, `.../chunk_system/scheduling/NewChunkHolder.java` — read/write orchestration callers
- `.../net/minecraft/world/level/chunk/storage/SimpleRegionStorage.java` — misplaced-chunk guard, `upgradeChunkTag`
- `.../net/minecraft/world/level/LevelAccessor.java` — `getGameTime`

Conventions used throughout: **[Paper]** marks a Java/Paper fact; **[Rivet]**
marks a decision Rivet makes about its own implementation; **[Deferred]** marks
work explicitly deferred to the #231 wave (or later) and not part of this
spec's guarantees. All integers are Java `DataInput`/`DataOutput` big-endian.

---

## 1. Scope and relationship to the region-file spec

**[Paper]** A saved chunk is a single root `CompoundTag` produced by
`SerializableChunkData.write()` and stored as the codec-wrapped payload of one
region-file record (§4 of the region spec: 4-byte length + 1 compression byte +
payload). On read, `SerializableChunkData.parse()` (L141-264) rehydrates that
compound; `ChunkLoadTask.runOffMain` (L331-365) runs `upgradeChunkTag` (DFU) on
the raw compound **before** `parse`, then `chunkData.read(...)` builds the
`ProtoChunk`.

**[Rivet]** This document pins the *bytes inside the record*: root key order,
per-section tags, palette/data codecs, light tags, and the read-default
contract. The region spec (linked above) pins everything outside the payload.
The two specs together are the #231 acceptance contract.

**D12/D13 linkage:** DECISIONS.md **D12** (CompoundTag insertion order) makes
byte-identical chunk NBT reachable without a fastutil-hash-order port, and
**D13** pins the round-trip gate to `region-file-compression=none`. The root
key order in §2 is exactly the D12 put sequence from `write()` (L555-645);
the payload wraps into the D13-`none` record. The golden fixture evidence is
`committed_chunk_fixture_round_trips_byte_identical` in
`crates/rivet-nbt/src/tests/nbt_io.rs` (read→write of the M0 spawn chunk
`0.0/0.0.nbt` is byte-identical), with a negative control that reordered keys
change the bytes.

---

## 2. Root-level fields — write order, types, and presence (the D12 contract)

`write()` (L555-645) builds the root in this exact put order. Under D12 this
order is the byte-identity contract for any compound Rivet writes from
scratch; under D13 the gate wraps it in an uncompressed record. Keys marked
"always" are emitted on every save; everything else is conditional.

| # | key | NBT type | when written | source |
|---|---|---|---|---|
| 1 | `DataVersion` | int | always | `NbtUtils.addCurrentDataVersion` (L556); value = `SharedConstants` current version |
| 2 | `xPos` | int | always | L557 `chunkPos.x` |
| 3 | `yPos` | int | always | L558 `minSectionY` |
| 4 | `zPos` | int | always | L559 `chunkPos.z` |
| 5 | `LastUpdate` | long | always | L560 `level.getGameTime()` from `copyOf` |
| 6 | `InhabitedTime` | long | always | L561 `chunk.getInhabitedTime()` |
| 7 | `Status` | string | always | L562 `BuiltInRegistries.CHUNK_STATUS.getKey(...).toString()` — names `empty, structure_starts, structure_references, biomes, noise, surface, carvers, features, initialize_light, light, spawn, full` (ChunkStatus.java L21-32) |
| 8 | `blending_data` | compound | only if non-null | L563 `storeNullable`; `BlendingData.Packed.CODEC` (see §7) |
| 9 | `below_zero_retrogen` | compound | only if non-null | L564; `BelowZeroRetrogen.CODEC` (see §7) |
| 10 | `UpgradeData` | compound | only if `!upgradeData.isEmpty()` | L565-567; `UpgradeData.write()` (see §7) |
| 11 | `sections` | list of compounds | always | L569-608 (see §3) |
| 12 | `isLightOn` | boolean (`true`) | only if `this.lightCorrect` | L609-611 |
| 13 | `block_entities` | list of compounds | always (may be empty) | L613-615 |
| 14 | `entities` | list of compounds | **PROTOCHUNK only** | L616-619 (`chunkStatus.getChunkType() == PROTOCHUNK`) |
| 15 | `carving_mask` | long array | **PROTOCHUNK only**, only if non-null | L620-622 |
| 16 | `block_ticks` | codec list | always | `saveTicks` L647-650 (called L625) |
| 17 | `fluid_ticks` | codec list | always | L625 |
| 18 | `PostProcessing` | list of lists of shorts | always | L626, `packOffsets` L768-783 |
| 19 | `Heightmaps` | compound of long arrays | always | L627-629 |
| 20 | `structures` | compound | always | L630 (see §6) |
| 21 | `ChunkBukkitValues` | compound | only if PDC non-empty (CraftBukkit) | L631-634 |
| 22 | `isLightOn` → `false` + `starlight.light_version` → int 10 | (clobber) | only if `lightCorrect && !status.isBefore(LIGHT)` | L636-643 (starlight; see §5) |

**Status-dependent emissions:** `entities`/`carving_mask` only for
`ChunkType.PROTOCHUNK` (statuses `empty`..`spawn`; `full` is `LEVELCHUNK`);
`DataVersion`, coordinates, times, `Status`, `sections`, `block_entities`,
`block_ticks`, `fluid_ticks`, `PostProcessing`, `Heightmaps`, `structures` are
always present.

**[Rivet]** The implementer must reproduce this put sequence verbatim for the
byte-identity gate. Note two Paper asymmetries to preserve, not "fix":

- `isLightOn` is written `true` at position 12 (L609-611) and then **clobbered
  to `false`** at position 22 under starlight (L639). In an insertion-ordered
  map a re-put of an existing key updates the value **in place** and leaves the
  key at its original slot, so the on-disk compound contains `isLightOn` **once**,
  value `false`, positioned right after `sections`. Rivet's `CompoundTag.put`
  is `IndexMap::insert` (compound_tag.rs L102-103), which has exactly these
  semantics — re-putting must not move the key to the tail. This is the D12
  `put` behavior and is load-bearing for byte identity.
- `block_ticks`/`fluid_ticks` are stored via codec even when empty, so they are
  always present.

---

## 3. Section compounds (`sections` list)

### 3.1 Write (L573-606)

For each `SectionData` in order: build a section `CompoundTag`:

1. If `chunkSection != null`: `block_states` then `biomes` via codecs
   (`store`, L577-578). **The order inside a section is `block_states` first,
   then `biomes`.**
2. `BlockLight` byte array if non-null (L581-583); `SkyLight` byte array if
   non-null (L585-587). Each is exactly 2048 bytes (§5, `DataLayer.getData`).
3. Starlight state ints only when > 0 (L590-599): `starlight.blocklight_state`
   and `starlight.skylight_state` (see §5).
4. `Y` byte appended **last** (L603) — and the whole section tag is **skipped**
   if empty (L602-605). A section whose `LevelChunkSection` is null but that has
   light data (or starlight state) is still written.

### 3.2 Read / parse (L194-241)

- `Y` = byte, default 0 (L203).
- Section kept only if `minSectionY <= y <= maxSectionY`; otherwise
  `section = null` but `BlockLight`/`SkyLight` are **still** parsed (light-only
  sections allowed; L204-228).
- `block_states`: `PalettedContainer.codecRW(BlockState.CODEC, ...)`
  (L208-214); missing → `containerFactory::createForBlockStates` (fresh AIR
  container). Parse failure → `promotePartial` logs + `getOrThrow(...)`
  wraps as `ChunkReadException` (`RuntimeException` subclass via `NbtException`,
  L785-789).
- `biomes`: read codec is the **RW** `biomeContainerRWCodec`
  (`containerFactory.biomeContainerRWCodec()`, L196) — the write codec is the
  RO `biomeContainerCodec`; the asymmetry is deliberate (see §4).
- `BlockLight`/`SkyLight`: byte array → `new DataLayer(bytes)` (L227-228).
  `DataLayer` **enforces exactly 2048 bytes** or throws `IllegalArgumentException`
  (`DataLayer.java` L27-29) — a malformed light array is a hard failure, not a
  default.
- Starlight state tags read only when present, default 0 (L231-237).

### 3.3 Paper Anti-Xray (read-side only)

`parse` consults `serverLevel.chunkPacketBlockController.getPresetBlockStates(...)`
(L206); when a preset array is returned, the block-states read codec is replaced
by `PalettedContainer.codecRW(BlockState.CODEC, strategy, AIR, presetValues)`
(L207), and `unpack` can **resize** the palette to absorb the presets
(PalettedContainer.java L102-136, L305). **[Rivet]** the preset constructor was
ported with #216 (RivetTodo) and is exercised by the #216 tests; the spec pins
that read can re-size palettes — do not assume the disk palette survives read.

---

## 4. Palette / data encoding inside `block_states` and `biomes`

`PalettedContainer.codecRW`/`codecRO` (PalettedContainer.java L41-73) is a
record codec of two fields:

- `palette` — list, **required** (`fieldOf`), element codec
  `elementCodec.mapResult(ExtraCodecs.orElsePartial(defaultValue))`
  (unknown entries degrade to the default and log, rather than failing).
- `data` — long array, **lenient optional** (`lenientOptionalFieldOf`).

`PackedData = (paletteEntries: List<T>, storage: Optional<LongStream>, bitsPerEntry: int)`
(PalettedContainerRO.java L40).

### 4.1 `pack` (write) — PalettedContainer.java L348-371

- Re-encodes via a fresh `HashMapPalette` seeded with `currentStorage.getBits()`
  in **storage order** (indices ascending).
- `bitsOnDisc = strategy.getConfigurationForPaletteSize(paletteSize).bitsInStorage()`.
- If `bitsOnDisc != 0`: `data` = raw longs of a fresh `SimpleBitStorage` at
  `bitsOnDisc` bits (the packed `getRaw()` array). Else `data` is **omitted**
  (zero-bit storage).
- `bitsPerEntry = bitsOnDisc`.

### 4.2 `unpack` (read) — L305-345

- `bitsOnDisc = getConfigurationForPaletteSize(paletteEntries.size()).bitsInStorage()`.
- If `PackedData.bitsPerEntry != -1 && bitsOnDisc != bitsPerEntry` → error
  `"Invalid bit count, calculated ..., but container declared ..."`.
- Zero bits (`bitsInMemory == 0`): `ZeroBitStorage`, no `data` read.
- Else `data` is **required**; absent → error
  `"Missing values for non-zero storage"`.
- Re-encode when `alwaysRepack()` (Global configuration) or
  `bitsInMemory() != bitsOnDisc`: build a `HashMapPalette` over the old data,
  re-encode, build the target palette. `SimpleBitStorage.InitializationException`
  → error `"Failed to read PalettedContainer: ..."`.

### 4.3 Strategies (Strategy.java L33-63)

- **Block states** (`createForBlockStates`, bitsPerAxis 4, entryCount 4096):
  bits 0 → zero, 1-4 → linear-4bit (`FOUR_BITS_LINEAR`), 5-8 → hashmap
  (`FIVE..EIGHT_BITS_HASHMAP`), else Global.
- **Biomes** (`createForBiomes`, bitsPerAxis 2, entryCount 64): bits 0 → zero,
  1 → 1-bit linear, 2 → 2-bit linear, 3 → 3-bit linear, else Global.
- `Configuration.Global` has `alwaysRepack() == true` and `bitsInStorage` equal
  to the palette size bits; `Configuration.Simple` is `(factory, bits)` with
  `bitsInMemory == bitsInStorage == bits` (Configuration.java L14-46).
- `entryCount = 1 << (bitsPerAxis*3)`: 4096 block states, 64 biomes.

### 4.4 Element codecs and factory (PalettedContainerFactory.java L23-38)

- `BlockState.CODEC` = `codec(BuiltInRegistries.BLOCK.byNameCodec(), ...)`
  (block id + flattenable `Properties`). **[Rivet]** block-state encoding is
  covered by the #154/#216 codegen slice and is a dependency, not re-specified
  here.
- Factory defaults: `defaultBlockState = Blocks.AIR`, `defaultBiome = plains`.
- Write biome codec is **RO** (`biomeContainerCodec`); read is **RW**
  (`biomeContainerRWCodec`) — preserve the asymmetry (L196-197, L571).

**[Rivet]** `PalettedContainer`/`Strategy`/`Configuration`/`Palette` already
exist in `crates/rivet-world/src/chunk/` (`paletted_container.rs`,
`strategy.rs`, `palette.rs`, `configuration.rs`) and are the exact substrate
this spec's `block_states`/`biomes` tags serialize onto. The `#230` chunk-wire
closure worktree is the reference for the read path.

---

## 5. Lighting tags (`BlockLight`, `SkyLight`, starlight)

### 5.1 Vanilla data layers

`BlockLight` and `SkyLight` are `ByteArrayTag`s of exactly 2048 bytes
(`DataLayer.SIZE`, DataLayer.java L11). Read: `DataLayer` panics (thrown as
`IllegalArgumentException`) if the length differs (L27-29). The starlight light
**engine** (SWMRNibbleArray propagation, `StarLightEngine`, `WorldUtil` light
section bounds) is **[Deferred]** to #230/#231; the **tag schema** here is
current.

### 5.2 Starlight tags (Paper 26.2 + Moonrise)

Constants (`SaveUtil.java` L22-30):

- `STARLIGHT_LIGHT_VERSION = 10`; tag `starlight.light_version` (int).
- `starlight.blocklight_state` (int), `starlight.skylight_state` (int).

Read gating (parse L162): `lightCorrect = status.isOrAfter(LIGHT) &&
get("isLightOn") != null && getIntOr("starlight.light_version", -1) == 10`.
So `isLightOn` must be **present** (any value — starlight clobbers it to
`false`) and the light version must equal 10.

Per-section state tags are read only `contains(...)`, default 0 (L231-237);
`loadStarlightLightData` (L267-317) feeds `starlight$setBlockNibbles`/
`setSkyNibbles`, using state `>= 0` to build SWMRNibbleArrays (L290-304) and
light sections can extend beyond block-section range
(`WorldUtil.getMinLightSection`, L454-489).

Write: `write()` clobbers `isLightOn` → `false` and writes root
`starlight.light_version` when `lightCorrect && !status.isBefore(LIGHT)`
(L637-643); per-section states written only when > 0 (L593-599). The starlight
mixin `saveLightHook`/`saveLightHookReal` (SaveUtil.java L32-127) strips and
re-injects `BlockLight`/`SkyLight`/state tags, and `loadLightHookReal`
(SaveUtil.java L139-194) only reads light when `lit && status.isOrAfter(LIGHT)`,
defaulting state 0 when absent (L169/L182), and `setLightCorrect(lit)` last
(L193). SaveUtil L17: "keep in-sync with SerializableChunkDataMixin".

**[Rivet]** For the NBT round-trip, the schema is: root `starlight.light_version`
int 10 + clobbered `isLightOn` false + per-section state ints (omitted when 0)
+ optional 2048-byte `BlockLight`/`SkyLight`. The light **engine** and the
mixin injection are deferred. A chunk that carries starlight data must
round-trip these tags verbatim; a `lightCorrect` chunk with version != 10 reads
as `lightCorrect = false` (do not invent a fallback).

---

## 6. `structures`, `Heightmaps`, ticks, post-processing, carving mask

### 6.1 `structures` (L685-766)

Write `packStructureData` (L685-712): `starts` compound — one entry per
`StructureStart`, keyed by structure id, value `createTag(context, pos)`
(**always present**, possibly empty) — then `References` compound of long
arrays (only non-empty keys). Read `unpackStructureStart` (L714-739):
`StructureStart.loadStaticStart(context, startsTag.getCompoundOrEmpty(key), seed)`;
unknown ids are logged/discarded. `unpackStructureReferences` (L741-766):
references filtered by `refPos.getChessboardDistance(pos) > 8` (L753-761).

**[Deferred]** the per-structure `createTag`/`loadStaticStart` payload internals
and `StructurePieceSerializationContext` are deferred to the structure cluster;
the **container** schema (`starts`/`References`) is current.

### 6.2 `Heightmaps` (L166-171 read, L627-629 write)

Keys are `Heightmap.Types.getSerializationKey()` (Heightmap.java L144-172):
`WORLD_SURFACE_WG`, `WORLD_SURFACE`, `OCEAN_FLOOR_WG`, `OCEAN_FLOOR`,
`MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`. Values are long arrays
(`getRawData`, 256 entries).

- `WORLDGEN_HEIGHTMAPS = {OCEAN_FLOOR_WG, WORLD_SURFACE_WG}` for statuses
  `empty`..`surface`.
- `FINAL_HEIGHTMAPS = {OCEAN_FLOOR, WORLD_SURFACE, MOTION_BLOCKING,
  MOTION_BLOCKING_NO_LEAVES}` for `carvers`..`full` (ChunkStatus.java L17-20).

Write iterates **all** map entries regardless of status (L627-629); `copyOf`
only saves `persistedStatus.heightmapsAfter()` types (L514-518). Read only
accepts keys in `status.heightmapsAfter()` (L168); a missing entry adds the
type to `toPrime` and `Heightmap.primeHeightmaps` is called (L398-410).

### 6.3 `block_ticks` / `fluid_ticks` (SavedTick.java L26-42)

`SavedTick.codec` fields (order matters):

| key | type | source |
|---|---|---|
| `i` | id codec (`BuiltInRegistries.BLOCK/FLOOR.byNameCodec()`) | L35 |
| `x` | int | L29 |
| `y` | int | L29 |
| `z` | int | L29 |
| `t` | int (delay) | L37 |
| `p` | int (TickPriority; `TickPriority.CODEC = Codec.INT.xmap(...)`) | L38 |

Read filters to the current chunk via `SavedTick.filterTickListForChunk`
(L44-47, `ChunkPos.pack(pos) == posKey`). Packing
(`LevelChunkTicks.pack` L117-132, `ProtoChunkTicks.pack` L36) writes
`pendingTicks` first, then the scheduled `tickQueue` sorted by
`SUB_TICK_ORDERING` (`Comparator.comparingLong(ScheduledTick::subTickOrder)`),
each `toSavedTick(currentTick)` = `delay = triggerTick - currentTick`
(ScheduledTick.java L46). `unpack` (SavedTick.java L49-51) re-adds the
`currentTick` at load. **The `t`/`LastUpdate` values are tick-thread-time
derived** — a Rivet seam (see §8).

### 6.4 `PostProcessing` (L175-189 read, L626 + L768-783 write)

A `ListTag` with **one entry per section** (every section gets an entry);
each entry is a `ListTag` of shorts (`ShortTag.valueOf(...)`), or an **empty**
list when the section has none (L768-783). Read: null/empty → null entry
(L178-189). The index maps to the section index; the list length is the number
of sections, not the height range.

### 6.5 `carving_mask` (L165 read, L620-622 write, L436)

A long array (PROTOCHUNK only). `read` does `new CarvingMask(carvingMask,
chunk.getMinY())` (L436-437). Absent → null.

---

## 7. Optional / ancillary codecs

### 7.1 `blending_data` — `BlendingData.Packed.CODEC` (BlendingData.java L398-420)

Record `Packed(int minSection, int maxSection, Optional<double[]> heights)`:

- `min_section` int (`fieldOf`).
- `max_section` int (`fieldOf`).
- `heights` — double list (`Codec.DOUBLE.listOf().xmap(Doubles::toArray, ...)`),
  **lenient optional**. `pack()` stores the heights as a list only if any cell
  differs from `Double.MAX_VALUE` (L84-97); otherwise `heights` is absent.
  `unpack` fills missing cells with `Double.MAX_VALUE` (L69-70).

**[Deferred]** `BlendingData.unpack` application (density blending) is not part
of the NBT round-trip; only the codec shape is current. The tag is optional and
null in a fresh world.

### 7.2 `below_zero_retrogen` — `BelowZeroRetrogen.CODEC` (L35-43)

- `target_status` — a **non-empty** `ChunkStatus` string (`fieldOf`, with a
  `DataResult.error("target_status cannot be empty")` guard for `EMPTY`, L32).
- `missing_bedrock` — long-stream → BitSet, **lenient optional** (L38).

**[Deferred]** retrogen application deferred; only the codec round-trip is
current. Tag is optional and null in a fresh world.

### 7.3 `UpgradeData` (UpgradeData.java L228-259)

Written only when `!upgradeData.isEmpty()` (L565-567). `UpgradeData.write()`:

- `Indices` compound — keys `"0".."N"` (per upgrade index), value int array,
  only non-empty arrays written; the compound itself omitted if empty (L232-241).
- `Sides` byte — bitmask of `1 << Direction8.ordinal()` over `this.sides`,
  **always written** (L244-247; `sides` is an `EnumSet`, empty → 0).
- `neighbor_block_ticks` / `neighbor_fluid_ticks` — `SavedTick` codec lists,
  only if non-empty (L250-255).

**[Deferred]** the block-fixer upgrade application (`UpgradeData.MAP`, L109-226)
is deferred; only the `Indices`/`Sides`/neighbor-tick NBT round-trip is
current.

---

## 8. Read/write orchestration and the `getGameTime` dependency

- **Callers.** Read: `ChunkLoadTask.runOffMain` (L331-365) —
  `upgradeChunkTag` (DFU, SimpleRegionStorage.java L84-120: converts below
  `DataVersion` then `addDataVersion`) **before** `SerializableChunkData.parse`,
  then `.read(world, poiManager, chunkMap.storageInfo(), pos)`. Save:
  `NewChunkHolder.saveChunk` (L1706-1728): `copyOf` on the main thread, then
  `chunkData.write()` on the save executor; `PlatformHooks.chunkSyncSave`
  (PaperHooks.java L89) runs between them.
- **Misplaced-chunk guard.** `SimpleRegionStorage.write` throws
  `IllegalArgumentException` when `dataFixType == CHUNK` and
  `!pos.equals(getChunkCoordinate(nbt))` (L61-82). `read()` logs +
  `reportMisplacedChunk` (L320-323). `getChunkCoordinate` reads `xPos`/`zPos`
  from root, or from the `Level` sub-tag when `DataVersion < 2842` (L113-121);
  `getLastWorldSaveTime` reads `LastUpdate` likewise (L125-133) — used by region
  header recalc (region spec §8).
- **`getGameTime` dependency.** `copyOf` writes `LastUpdate = level.getGameTime()`
  (L536) and `ticksForSerialization = chunk.getTicksForSerialization(level.getGameTime())`
  (L521). `getGameTime()` is `LevelAccessor` default →
  `getLevelData().getGameTime()` (LevelAccessor.java L41-44); `ServerLevel`'s
  `getLevelData()` is the live `ServerLevelData.gameTime`. So `LastUpdate` and
  every `t` are **tick-thread-time derived** — a Rivet seam requirement: a
  server-level game time must exist before a chunk can be serialized. This is
  the #232 seam (Level-root value slice + GameTime), tracked in the dependency
  list below.

---

## 9. Malformed / missing-field read defaults (the robustness contract)

`parse()` and its helpers handle every field permissively except where noted.
Each default below is a load-bearing behavior for read parity:

| field | missing/empty → | source |
|---|---|---|
| `Status` (empty string) | `parse` returns **null**; caller drops the chunk (`ChunkLoadTask` L350-352) | L145-147 |
| `Status` (absent) | `ChunkStatus.EMPTY` | L160, L652-654 |
| `DataVersion` > current | `printStackTrace()` + `System.exit(1)` unless `-DPaper.ignoreWorldDataVersion` | L150-155 |
| `xPos`/`zPos` | 0 | L157 |
| `LastUpdate`/`InhabitedTime` | 0L | L158-159 |
| `blending_data`/`below_zero_retrogen` | null | L163-164 |
| `carving_mask` | null | L165 |
| missing heightmap | added to `toPrime` and primed | L398-410 |
| `UpgradeData` | `UpgradeData.EMPTY` | L161 |
| `block_ticks`/`fluid_ticks` | empty list | L172-173 |
| `block_states`/`biomes` absent | fresh container (AIR / plains) | L214, L221 |
| `block_states`/`biomes` wrong data | `promotePartial` logs then `ChunkReadException` | L212, L219 |
| `BlockLight`/`SkyLight` absent | null | L227-228 |
| `BlockLight`/`SkyLight` wrong length | hard `IllegalArgumentException` (DataLayer) | DataLayer.java L27-29 |
| `Y` absent | 0 | L203 |
| section y outside height range | `section = null` but light still parsed | L204-228 |
| non-compound element in `sections` | skipped (`getCompound(i)` empty) | L200-201 |
| `entities`/`block_entities` absent | empty list | L191-192 |
| `PostProcessing` absent | empty array (length 0) | L175 |
| `structures` absent | empty compound | L193 |
| `ChunkBukkitValues` absent | null | L262 |
| malformed `keepPacked` | `false` | L671 |

**[Rivet]** The **hard failures** (`DataLayer` length, `ChunkReadException`,
newer `DataVersion`) must not be silently weakened — they are part of the
robustness contract and are gate negatives.

---

## 10. NBT framing (the payload bytes inside the record)

The codec-wrapped NBT payload that region-file compression wraps (§4 of region
spec) is `NbtIo.write` = `writeUnnamedTagWithFallback`:

- Type byte `10` (CompoundTag) + `writeUTF("")` (2-byte length 0) + body
  (NbtIo.java L124-126, L162-167, L170-172).
- `StringFallbackDataOutput.writeUTF` catches `UTFDataFormatException` and
  writes `""` instead (L196-209) — a string longer than 65535 modified-UTF-8
  bytes degrades to empty rather than failing the whole write.

Read: `NbtIo.read` requires a Compound root else
`IOException("Root tag must be a named compound tag")` (L117-121).

**[Rivet]** The `readUTF`/`writeUTF` framing is the OpenJDK-faithful
modified-UTF-8 codec already ported in `rivet-util::data_io` (#265/#212); this
spec's payload rides on it. `NbtIo.read`/`write` are already proven
byte-identical in `rivet-nbt` (the D12 golden test, §1).

---

## 11. Staged implementation boundaries (what belongs where)

The NBT spec is the contract; the code lands across several in-flight units.
Dependency map (all OPEN as of this writing, verified 2026-08-09):

- **#230 (`chunk.wire` closure + `chunk.support`)** — `PalettedContainerRO`,
  `DataLayer`/`CarvingMask`/`BlockColumn`/`LightChunk`, the palette read path,
  and the starlight **read** of section light data. The section-tag shape in §3
  is its acceptance surface.
- **#232 (Level-root value slice + GameTime)** — the `getGameTime()`
  dependency (§8). Without it `LastUpdate`/`t` cannot be produced; the
  round-trip fixture must come from a world that has a game time.
- **#183-b / #183 (chunk.access SCC)** — `LevelChunkSection`, `ChunkAccess`,
  `ProtoChunk`/`LevelChunk`; the read path builds these from §3/§9.
- **#228 (worldgen-reachable block slice)** — `BlockState`/`Properties` codec
  used by §4.4.
- **#233 (deps wiring)** — workspace deps (indexmap for D12 order, bitflags,
  slotmap, etc.) that the chunk.storage implementation needs.
- **#231 (`chunk.storage` wave)** — `SerializableChunkData`, `RegionFile`
  (spec'd in the region-file doc), `SimpleRegionStorage`, the save pipeline.

**In this spec's guarantees** (implementable in the #231 wave once the above
are landed): the §2 put sequence, §3 section shape, §4 palette/data codecs,
§5 light tag schema, §6 container schemas (not structure internals), §7 codec
shapes (not application), §8 orchestration, §9 defaults, §10 framing.

**[Deferred]** starlight light engine + mixin wiring; entity/block-entity
parsing (`EntityType.loadEntitiesRecursive`, `BlockEntity.loadStatic`,
`postLoadChunk` L656-683) — the **verbatim CompoundTag round-trip** of
`entities`/`block_entities` (L191, L428-432, L616-619) is current, the
parse-to-objects step is not; `StructureStart` payload internals; `UpgradeData`
block fixers; `BlendingData`/retrogen application; deflate/lz4 write parity
(D13 → #231); Aikar oversized write (region spec §6.2).

---

## 12. Acceptance criteria

### 12.1 Deterministic round trip (byte identity)

1. **Golden NBT round trip:** a Paper 26.2 chunk NBT fixture (the M0 spawn
   chunk `0.0/0.0.nbt`) must survive `NbtIo.read` → `NbtIo.write`
   byte-identically. This is already the D12 golden test
   (`committed_chunk_fixture_round_trips_byte_identical`); it must continue to
   pass once the §2/§3/§4 write path is wired.
2. **Negative control:** reordering any §2 key (or a §3 section key) must
   change the emitted bytes (proves order-sensitivity, D12).
3. **Full payload round trip:** `parse(fixture) → write()` reproduces the
   fixture bytes for a chunk that exercises multiple sections, a non-trivial
   palette (`hashmap` and `linear`), `BlockLight`/`SkyLight`, starlight state
   ints, `Heightmaps`, `structures`/`References`, `block_ticks`/`fluid_ticks`,
   `PostProcessing`, `UpgradeData`, and `block_entities`/`entities`.

### 12.2 Real-world read-only-copy smoke (M2 world load)

4. **Read a launcher-created world:** copy a 26.2 world save (read-only, never
   write to the original — the launcher-created New World save noted in memory
   `local-new-world-save`) and load every region record through
   `parse` with the §9 defaults; every chunk either parses or drops exactly as
   Paper does (empty `Status` → null; newer `DataVersion` honored).
5. **No mutation on read:** reading a fixture never rewrites the fixture; a
   `parse → write` on a read-only copy must be byte-identical (D12) and must
   not alter `LastUpdate`/`InhabitedTime` (those come only from a `copyOf`
   save path with a real game time).

### 12.3 Oracle negatives (load-bearing, not incidental)

6. `DataLayer` length != 2048 → hard error (not a silent default).
7. `block_states`/`biomes` with wrong palette/data → `ChunkReadException`
   (not a silent AIR/plains default).
8. Empty `Status` → `parse` null → chunk dropped.
9. `DataVersion` newer than current → abort (unless the Paper flag is set).
10. Starlight version != 10 with `isLightOn` present → `lightCorrect = false`
    (no invented fallback).

These are the oracle negatives the #231 wave must carry; weakening any of them
is a regression against this spec.

---

## 13. Deferred / out-of-scope summary

- **[Deferred]** starlight light engine, `saveLightHook`/`loadLightHook` mixin
  wiring, `WorldUtil` light-section bounds → #230/#231.
- **[Deferred]** entity/block-entity content parsing (verbatim tag round-trip
  only) → entity cluster.
- **[Deferred]** `StructureStart`/`StructurePieceSerializationContext` payload
  internals (container schema only) → structure cluster.
- **[Deferred]** `UpgradeData.MAP` block fixers (NBT round-trip only).
- **[Deferred]** `BlendingData.unpack` / retrogen application (codec round-trip
  only).
- **[Deferred]** deflate/lz4 write parity (D13, region spec §5) → #231.
- **[Deferred]** Aikar oversized write path + meta-file handling (region spec
  §6.2) → #231.
- **[Out of scope]** `region-file-compression=deflate` byte identity (D13
  pins `none`); SectionStorage/POI/entity payload formats (same container,
  different payloads).
