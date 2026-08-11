# Chunk pipeline (ticket, holder, status-DAG, send/IO ordering) specification

Authoritative design spec for Rivet's port of Paper 26.2's Moonrise chunk-pipeline
— the `net.minecraft.server.level` pipeline units of issue #185 (`ChunkMap`,
`DistanceManager`, `ChunkHolder`/`GenerationChunkHolder`, `ServerChunkCache`,
`ChunkLevel`/`FullChunkStatus`, `Ticket`/`TicketType`, the `ChunkTrackingView`
value, the tracker family), the `net.minecraft.world.level.chunk.status` pyramid
units, and the `net.minecraft.world.level.chunk.generator` units.
This is a **specification only**: it pins the scheduling/ownership invariants and
the port's staged scope; it does not implement the pipeline and it does not invent
APIs. Where Paper's behavior is load-bearing we reproduce it; where Moonrise's
*mechanism* (executor internals) is not, we replace it with the D5 threading model.

**Sources of truth (read before changing this document):**

- `working/Paper/paper-server/src/minecraft/java/net/minecraft/server/level/ChunkLevel.java` — level↔status mappings, the ticket-level constants
- `.../server/level/FullChunkStatus.java` — the four-state status ladder (enum ordinal order: `INACCESSIBLE, FULL, BLOCK_TICKING, ENTITY_TICKING`)
- `.../server/level/Ticket.java`, `.../server/level/TicketType.java` — ticket value model, flags, built-in types
- `.../server/level/DistanceManager.java` — ticket-priority graph, spawn tracker
- `.../server/level/ChunkMap.java` — the pipeline hub; `FORCED_TICKET_LEVEL = byStatus(ENTITY_TICKING) = 31`
- `.../server/level/ChunkHolder.java`, `.../GenerationChunkHolder.java`, `.../ChunkGenerationTask.java`
- `.../server/level/ServerChunkCache.java` — the `ChunkSource` facade + main-thread executor
- `.../world/level/chunk/status/ChunkStatus.java` — the 12 statuses, index/parent/heightmaps-after, `ChunkSystemChunkStatus` flags
- `.../world/level/chunk/status/ChunkStep.java`, `.../status/ChunkDependencies.java`, `.../status/ChunkPyramid.java`, `.../status/ChunkStatusTasks.java` — the generation/loading DAGs and task bodies
- `.../world/level/chunk/ChunkGenerator.java`, `.../ChunkGenerators.java`, `.../ChunkGeneratorStructureState.java`
- `.../ca/spottedleaf/moonrise/patches/chunk_system/scheduling/ChunkHolderManager.java` — ticket→holder management, `MAX_TICKET_LEVEL = 44`, `UNLOAD_COOLDOWN = 100` ticks
- `.../scheduling/NewChunkHolder.java` — the `processTicketLevelUpdate` cancellation + 3-stage unload, `NEIGHBOUR_RADIUS = 2`
- `.../scheduling/ChunkTaskScheduler.java` — status config tables (write radius, empty-load, parallel-capable), access-radius table, executor set
- `.../scheduling/task/ChunkUpgradeGenericStatusTask.java`, `ChunkLoadTask.java`, `ChunkFullTask.java`, `ChunkLightTask.java` — per-status task dispatch
- `.../player/RegionizedPlayerChunkLoader.java` — ticket stages, send ordering, per-player rate limiting
- `.../io/MoonriseRegionFileIO.java` — save-coalescing write ordering (also §11 of `docs/region-file-format-spec.md`)
- `docs/region-file-format-spec.md`, `docs/serializable-chunk-data-spec.md` — the storage payloads the pipeline writes/reads (issues #231)

Conventions used throughout: **[Paper]** marks a Java/Paper fact; **[Rivet]** marks
a decision Rivet makes about its own implementation; **[Deferred]** marks work
explicitly deferred to a later wave and not part of this spec's guarantees.

---

## 1. Scope: what #185 ports, what it defers

Issue #185 is the **pipeline spine**: tickets + holder lifecycle + the status DAG +
send/IO ordering, with the Moonrise *scheduler internals* deferred. The manifest
units in scope:

| Manifest unit | Files | LOC |
| --- | --- | --- |
| `mc.server.level.pipeline.level` | `ChunkLevel`, `ChunkResult`, `FullChunkStatus` | 182 |
| `mc.server.level.pipeline.ticket` | `Ticket`, `TicketType` | 268 |
| `mc.server.level.pipeline.tracker` | `ChunkTracker`, `LoadingChunkTracker`, `SectionTracker`, `SimulationChunkTracker` | 235 |
| `mc.server.level.pipeline.view` | `ChunkTrackingView` | 120 |
| `mc.server.level.pipeline.task` | `ChunkTaskDispatcher`, `ChunkTaskPriorityQueue`, `ThrottlingChunkTaskDispatcher` | 244 |
| `mc.server.level.pipeline.holder` | `ChunkHolder`, `GenerationChunkHolder`, `GeneratingChunkMap`, `ChunkGenerationTask` | 744 |
| `mc.server.level.pipeline.distance` | `DistanceManager` | 322 |
| `mc.server.level.pipeline.chunkmap` | `ChunkMap` | 1438 |
| `mc.server.level.pipeline.servercache` | `ServerChunkCache` | 840 |
| `mc.server.level.pipeline.region` | `WorldGenRegion` | 584 |
| `mc.server.level.pipeline.light` | `ThreadedLevelLightEngine` (#184 seam) | 261 |
| `mc.world.level.chunk.status` | `ChunkStatus`/`ChunkStep`/`ChunkDependencies`/`ChunkPyramid`/`ChunkStatusTask(s)`/`ChunkType`/`WorldGenContext` | 847 |
| `mc.world.level.chunk.generator` | `ChunkGenerator`/`ChunkGenerators`/`ChunkGeneratorStructureState` | 1053 |
| `ca.spottedleaf.moonrise.patches.chunk_system.ticket` | `ChunkSystemTicket`/`ChunkSystemTicketStorage`/`ChunkSystemTicketType` | 65 |

**[Rivet]** This is the minimal spine that turns a `ChunkPos` request into a
loaded/generated chunk with correct ordering. Two large Moonrise clusters are
explicitly **out of scope** for #185 (see §9):

- **[Deferred]** `ca.spottedleaf.moonrise.patches.chunk_system.scheduling`
  (6060 LOC: `ChunkHolderManager`, `ChunkTaskScheduler`, `NewChunkHolder`,
  `PriorityHolder`, `ThreadedTicketLevelPropagator`) — the ticket-propagation
  engine, executor plumbing, and holder-queue internals. `#185` absorbs the
  *invariants* (this spec) and RivetTodos the mechanism.
- **[Deferred]** `ca.spottedleaf.moonrise.patches.chunk_system.scheduling.task`
  (1816 LOC: `ChunkFullTask`, `ChunkLightTask`, `ChunkLoadTask`,
  `ChunkProgressionTask`, `ChunkUpgradeGenericStatusTask`,
  `GenericDataLoadTask`) — the concrete task classes. The per-status *task
  bodies* are in `ChunkStatusTasks` (§3); the progression-task framework is
  part of the deferred scheduler surface.

The existing Rivet slices are the fixed anchors this spec builds on:

- `crates/rivet-world/src/chunk/chunk_access.rs` + `chunk_source.rs` — the
  `ChunkSource` facade with a slice-local `ChunkStatus { Empty, Full }`
  (RivetTodo #185; `getChunkForLighting` = `EMPTY` is the #184 light seam).
  The 12-status DAG below replaces that 2-value enum at the pipeline wave.
- `crates/rivet-server/src/server/level/chunk_map.rs` — the M1 `ChunkMap`
  (single spawn chunk + deterministic superflat content). It is not the pipeline
  hub yet; it holds a `RivetTodo(#185)` for the `DistanceManager`/ticket/light
  hub wiring.
- `crates/rivet-server/src/server/level/player_chunk_loader.rs` — the direct-send
  half of `RegionizedPlayerChunkLoader` (M1 superflat). The send *order* it emits
  (deterministic X-major raster) is the parity-canonical order; §4 pins it.
  The tickets/queues/rate-limiters of that class are the #185 scope.

---

## 2. Ticket levels and the holder lifecycle

### 2.1 The level ladder

**[Paper]** `ChunkLevel` maps a **ticket level** (smaller = more loaded) to a
generation target and a full status. Constants:

- `ENTITY_TICKING_LEVEL = 31`, `BLOCK_TICKING_LEVEL = 32`, `FULL_CHUNK_LEVEL = 33`
- `RADIUS_AROUND_FULL_CHUNK = 11` (from the FULL generation step's accumulated radius, §3)
- `MAX_LEVEL = 33 + 11 = 44`

The **generation status** a ticket level demands is
`generationStatus(level) = getStatusAroundFullChunk(level - 33)`:

| Level | distance to full | generation status demanded |
| --- | --- | --- |
| 33 | 0 | `FULL` |
| 34 | 1 | `INITIALIZE_LIGHT` |
| 35 | 2 | `CARVERS` |
| 36 | 3 | `BIOMES` |
| 37–44 | 4–11 | `STRUCTURE_STARTS` |

`FullChunkStatus` (enum ordinal order `INACCESSIBLE=0, FULL=1, BLOCK_TICKING=2,
ENTITY_TICKING=3`) derives from the level: `level <= 31` → `ENTITY_TICKING`,
`<= 32` → `BLOCK_TICKING`, `<= 33` → `FULL`, else `INACCESSIBLE`.
`byStatus(FullChunkStatus)`: `INACCESSIBLE → 44`, `FULL → 33`,
`BLOCK_TICKING → 32`, `ENTITY_TICKING → 31`. `isEntityTicking = level <= 31`,
`isBlockTicking = level <= 32`, `isLoaded = level <= 44`.
`ChunkHolderManager` aliases: `ENTITY_TICKING_TICKET_LEVEL = 31`,
`BLOCK_TICKING_TICKET_LEVEL = 32`, `FULL_LOADED_TICKET_LEVEL = 33`,
`MAX_TICKET_LEVEL = 44` (inclusive; a chunk with no tickets sits at `45`).
`ChunkMap.FORCED_TICKET_LEVEL = byStatus(ENTITY_TICKING) = 31`.

**[Rivet]** Port the constants and both mappings verbatim (they are parity facts
consumed by ticket math and by the #175 hash gate's status tagging).

### 2.2 Ticket values

**[Paper]** A `Ticket<T>` is `(type, level, key)` ordered by level then type then key.
`TicketType<T>` carries a comparator, a timeout in ticks (`<= 0` = no
timeout), and flags. Flags: `FLAG_PERSIST=1`, `FLAG_LOADING=2`,
`FLAG_SIMULATION=4`, `FLAG_KEEP_DIMENSION_ACTIVE=8`,
`FLAG_CAN_EXPIRE_IF_UNLOADED=16`. Built-in types (in registration order):

| Type | timeout (ticks) | flags |
| --- | --- | --- |
| `PLAYER_SPAWN` | 20 | LOADING |
| `SPAWN_SEARCH` | 1 | LOADING |
| `DRAGON` | none | LOADING\|SIMULATION |
| `PLAYER_LOADING` | none | LOADING |
| `PLAYER_SIMULATION` | none | SIMULATION\|KEEP_DIMENSION_ACTIVE |
| `FORCED` | none | PERSIST\|LOADING\|SIMULATION\|KEEP_DIMENSION_ACTIVE |
| `PORTAL` | 300 | PERSIST\|LOADING\|SIMULATION\|KEEP_DIMENSION_ACTIVE |
| `ENDER_PEARL` | 40 | LOADING\|SIMULATION\|KEEP_DIMENSION_ACTIVE |
| `UNKNOWN` | 1 | CAN_EXPIRE_IF_UNLOADED\|LOADING |
| `PLUGIN` | 600 | LOADING\|SIMULATION |
| `POST_TELEPORT` | 5 | LOADING\|SIMULATION |
| `PLUGIN_TICKET` | none | LOADING\|SIMULATION |
| `FUTURE_AWAIT` | none | LOADING\|SIMULATION |
| `CHUNK_LOAD` | none | LOADING |

Note on `PLUGIN`: it is *registered* with the constructor base `NO_TIMEOUT`,
but `TicketType.timeout()` overrides it to `PLUGIN_TYPE_TIMEOUT = 600` (the
chunk-gc config default), so `hasTimeout()` is true and plugin tickets expire
after 600 ticks. Port the `timeout()` override, not the base value.

Moonrise player tickets (`RegionizedPlayerChunkLoader`):
`PLAYER_TICKET` (flags LOADING\|SIMULATION\|KEEP_DIMENSION_ACTIVE, no timeout)
and `PLAYER_TICKET_DELAYED` (same flags, with a short timeout
`setTimeout(max(1, ticks))` that gives a grace window before a freshly-dropped
position re-spins a load). The moonrise ticket storage/types
(`chunk_system.ticket` unit) sit under these.

### 2.3 Holder lifecycle

**[Paper]** A `NewChunkHolder` owns one column's mutable chunk state and is kept
in the chunk map while its ticket level is `<= MAX_TICKET_LEVEL` (44). The
per-column `processTicketLevelUpdate` loop:

1. Computes the new ticket level (min over all tickets + the tracker state).
2. If the level **downgrades** (higher number) while a generation is requested
   and the chunk is not yet full: **cancel** — either all tasks (if now
   `newLevel > 44`, unloaded) or clamp the requested status to the new
   `generationStatus(newLevel)` and cancel the in-flight generation task if the
   already-reached status is at/after the new cap (NewChunkHolder
   `processTicketLevelUpdate`, the "cancellations from downgrading ticket level"
   block).
3. Else it schedules/keeps the generation task for `generationStatus(newLevel)`.

**Unload** is a deliberate 3-stage handoff that does not hold the scheduling lock
during I/O:

- **Stage 1** (holds the scheduling lock): null out the chunk/entity/poi state,
  clear the completion array, and *capture* the values to be written; create the
  `UnloadTask` save tasks.
- **Stage 2** (releases the lock): perform the saves — chunk data via
  `saveChunk(...)`/`MoonriseRegionFileIO.scheduleSave`, entity data via
  `saveEntities`, poi via `savePOI` when dirty — and unload the entity/poi
  slices.
- **Stage 3** (relock): `unloadStage3()` drops the save-task references and
  **re-checks**: if anything was reloaded mid-unload (`entityChunk`/`poiChunk`/
  `currentChunk` present again) or `isSafeToUnload()` is no longer `null`, the
  unload **aborts** and the holder is retained; otherwise the holder is dropped
  from the map.

`isSafeToUnload` returns `null` (safe) only when all of: ticket level `> 44`; no
neighbour is using the chunk for generation (`neighboursGenerating`),
`neighboursWaitingForUs` empty; `FullChunkStatus.INACCESSIBLE`; no generation
task; no requested generation; no pending entity/poi load; no pending
entity/poi/chunk serialization. Light tasks do not need a check (they hold a
ticket). Neighbour checks use `NEIGHBOUR_RADIUS = 2` (the 5×5 square around the
column). When stage 1 finds nothing to save it removes the holder immediately;
when stage 3 aborts, `ChunkHolderManager` adds an `UNLOAD_COOLDOWN` ticket
(timeout `5L * 20L = 100` ticks) at `MAX_TICKET_LEVEL` so the next unload retry
is not immediately next tick. Only a successful stage 3 calls
`removeChunkHolder`; the final `releaseChunkData` is the drop from the map.

**[Rivet]** Port the 3-stage unload shape and the cancellation rule verbatim —
they are the backpressure/cancellation contract (§6). The lock in Rivet is not
Java's `ReentrantAreaLock`; the D5 model (§5) gives the single tick thread the
same exclusive ownership, so Stage 1/3 run on the tick thread and Stage 2 on the
storage/IO worker set. `UNLOAD_COOLDOWN` is a plain `100`-tick ticket at
`MAX_TICKET_LEVEL`, added only when a stage-3 unload aborts; it is precisely the
retry-spacing cooldown, not a save/drop mechanism.

---

## 3. The ChunkStatus generation/loading DAG and radii

### 3.1 Statuses

**[Paper]** `ChunkStatus` is an ordered list (index = position), each with a
parent, a heightmap policy (WORLDGEN up to `SURFACE`, FINAL from `CARVERS` — the
boundary is *at* `CARVERS`), and a chunk type (`PROTOCHUNK` for `EMPTY`..`SPAWN`,
`LEVELCHUNK` only at `FULL` — `SPAWN` is still a `PROTOCHUNK`):
`EMPTY, STRUCTURE_STARTS, STRUCTURE_REFERENCES, BIOMES, NOISE, SURFACE, CARVERS,
FEATURES, INITIALIZE_LIGHT, LIGHT, SPAWN, FULL`. Index 0 = `EMPTY`, 11 = `FULL`.

### 3.2 Generation pyramid

**[Paper]** `ChunkPyramid.GENERATION_PYRAMID` — the steps and their **direct**
requirements by radius (from the builder calls; the parent status is always an
implicit dependency at radius 0). The `NOISE`/`SURFACE`/`CARVERS`/`FEATURES`
steps also set a builder-level `blockStateWriteRadius` (0/0/0/1) that feeds the
task bodies; the *dispatch* write radius used for parallel exclusion is the
separate `ChunkSystemChunkStatus` config in §3.4.

| Status | direct `addRequirement` | task |
| --- | --- | --- |
| `EMPTY` | — | pass-through |
| `STRUCTURE_STARTS` | — | `generateStructureStarts` |
| `STRUCTURE_REFERENCES` | `STRUCTURE_STARTS` r8 | `generateStructureReferences` |
| `BIOMES` | `STRUCTURE_STARTS` r8 | `generateBiomes` |
| `NOISE` | `STRUCTURE_STARTS` r8, `BIOMES` r1 | `generateNoise` |
| `SURFACE` | `STRUCTURE_STARTS` r8, `BIOMES` r1 | `generateSurface` |
| `CARVERS` | `STRUCTURE_STARTS` r8 | `generateCarvers` |
| `FEATURES` | `STRUCTURE_STARTS` r8, `CARVERS` r1 | `generateFeatures` |
| `INITIALIZE_LIGHT` | — | `initializeLight` |
| `LIGHT` | `INITIALIZE_LIGHT` r1 | `light` |
| `SPAWN` | `BIOMES` r1 | `generateSpawn` |
| `FULL` | — | `full` (main thread) |

`ChunkStep.Builder.addRequirement` merges radii by `ChunkStatus.max` (later
status wins) and `buildAccumulatedDependencies` folds the parent's accumulated
dependencies through `radiusOfParent + parentDeps` with the same max-merge.
`ChunkDependencies` derives, per dependency status, the radius at which that
status is required (the `radiusByDependency` table). The **FULL step's
accumulated** dependencies are the DAG that matters — 12 entries, radius 11:

| distance | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| status | `SPAWN` | `INITIALIZE_LIGHT` | `CARVERS` | `BIOMES` | `STRUCTURE_STARTS` | ··· (STRUCTURE_STARTS ×8) |

This is the source of `RADIUS_AROUND_FULL_CHUNK = 11` and `MAX_LEVEL = 44`.
`ChunkStep.byRadius` precomputes, per step, the minimum status that must be
generated at each neighbour distance (a prefix-filled table from the accumulated
deps); `moonrise$getRequiredStatusAtRadius(d)` reads it.

**[Rivet]** The accumulated-dependency tables (per step, per radius) are exact
parity data: port `ChunkPyramid` + `ChunkStep.Builder` accumulation as a
deterministic build (they are pure functions of the builder calls above) rather
than hand-flattening the tables. The `byRadius` table is derived the same way.

### 3.3 Loading pyramid

**[Paper]** `ChunkPyramid.LOADING_PYRAMID` is the all-zero-radius DAG
(`EMPTY → … → FULL`, each step a radius-0 parent dependency) with the task
bodies `loadStructureStarts`, `initializeLight`, `light` (starlight needs no
neighbours), `full`. Loading does not require far neighbours; the access-radius
tables below come out of this too.

### 3.4 Parallelism and write radii

**[Paper]** `ChunkTaskScheduler`'s static config marks each status:

- **Write radius** (block-state writes into neighbours): `FEATURES = 1`,
  `LIGHT = 2`, all others 0. A task with a nonzero write radius is scheduled on
  the radius-aware queue so two tasks whose write areas overlap are never run
  concurrently (§9 defers the mechanism; the invariant is §7).
- **Empty-load flags** (`emptyLoadStatus`): `EMPTY, STRUCTURE_REFERENCES,
  BIOMES, NOISE, SURFACE, CARVERS, FEATURES, SPAWN` — statuses a freshly-loaded
  (never-before-seen) chunk may be considered to have reached without work.
- **Parallel-capable**: `EMPTY, STRUCTURE_STARTS, STRUCTURE_REFERENCES, BIOMES,
  NOISE, SURFACE, CARVERS, INITIALIZE_LIGHT`. `FEATURES`/`LIGHT`/`SPAWN`/`FULL`
  are not: FEATURES writes neighbours; LIGHT is the starlight hook; SPAWN, though
  it writes only its own chunk, is kept non-parallel because a neighbour FEATURES
  chunk that was unloaded but fails to reload could write into it, and SPAWN
  reads its own blocks (per the source comment); FULL is the main-thread
  promotion.

**[Rivet]** These three tables are exact parity data. The Rivet realization:
parallel-capable statuses run on the worldgen `rayon` pool on detached
`ProtoChunk` values; the radius-aware write-radius exclusion and main-thread
`FULL` promotion are the D5 tick thread (§5). `INITIALIZE_LIGHT`'s parallel
safety holds because it writes only within the chunk; the #184 starlight seam
owns the light computation units.

### 3.5 Access radii

**[Paper]** `getAccessRadius(toStatus)` is computed from the accumulated deps:
`max over distances d of (d + getAccessRadius(requiredStatusAtRadius d))`,
combined as `max(LOADING, GENERATION)` per status:

| Status | `EMPTY` | `SS` | `SR` | `BIOMES` | `NOISE` | `SURFACE` | `CARVERS` | `FEATURES` | `INIT_LIGHT` | `LIGHT` | `SPAWN` | `FULL` |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Access radius | 0 | 0 | 8 | 8 | 9 | 9 | 9 | 10 | 10 | 11 | 11 | 11 |

`MAX_ACCESS_RADIUS = 11`. For a full status:
`getAccessRadius(full) = (ordinal - 1) + 11`, giving `INACCESSIBLE = 10`,
`FULL = 11`, `BLOCK_TICKING = 12`, `ENTITY_TICKING = 13`.

**[Rivet]** Port `getAccessRadius0` as written (the recursive max over the
`byRadius` table); it is a pure function of §3.2/§3.3. The access radius is the
owner-boundary number used in §7: it is the maximum neighbour distance a chunk
operation can read or write, and therefore the minimum distance at which two
columns are independent.

---

## 4. Chunk send ordering

**[Paper]** `RegionizedPlayerChunkLoader` drives per-player chunk load/tick/send
through **six ticket stages** per view position:

| Stage | `CHUNK_TICKET_STAGE_*` | ticket level applied |
| --- | --- | --- |
| NONE | 0 | `MAX_TICKET_LEVEL + 1` (45) |
| LOADING | 1 | `LOADED_TICKET_LEVEL` = `getTicketLevel(EMPTY)` = 44 |
| LOADED | 2 | 44 |
| GENERATING | 3 | `GENERATED_TICKET_LEVEL` = 33 |
| GENERATED | 4 | 33 |
| TICK | 5 | `TICK_TICKET_LEVEL` = 31 |

The ticket levels come from the constants `GENERATED_TICKET_LEVEL =
FULL_LOADED_TICKET_LEVEL (33)`, `LOADED_TICKET_LEVEL = getTicketLevel(EMPTY)
(44)`, `TICK_TICKET_LEVEL = ENTITY_TICKING_TICKET_LEVEL (31)`. Stage *entry*
adds only `PLAYER_TICKET` at the stage level: the LOADED/GENERATING/TICK
promotions are `TicketOperation.addOp`/`addAndRemove` calls carrying
`PLAYER_TICKET` alone. `PLAYER_TICKET_DELAYED` is the demotion/exit ticket —
the area-map remove callbacks (`loadTicketCleanup`, `tickMap`) replace the
departing `PLAYER_TICKET` with `PLAYER_TICKET_DELAYED` at the same level, whose
short timeout is the grace window that stops a rapid demote-re-add cycle from
re-spinning a load. The
loader holds three `StaggeredRateLimiter`s (send, load-ticket, generate-ticket),
each ticked with a config rate clamped to `[1, MAX_RATE]` (`MAX_RATE = 10_000`; a
config of `≤ 0` or `> MAX_RATE` becomes `MAX_RATE`), with an
`INITIAL_ALLOCATION_FACTOR = 0` (cold start). Two of the three carry a separate
**concurrency cap** (`getMaxChunkLoads`/`getMaxChunkGenerates`):
`max(5, radiusChunks / 5)` where `radiusChunks = (2·loadViewDistance + 1)²`,
minus the currently-queued count — at most 1/5th of the load-view square may be
loading/generating concurrently; the send limiter has no such cap.

View distances: `loadViewDistance = max(tickViewDistance + 1, override)`;
`sendViewDistance = loadViewDistance - 1`. Positions enter the send queue when
`wantChunkSent` — Chebyshev distance `max(|dx|,|dz|) <= lastSendDistance + 1`
(one square beyond the send view distance) and within the send-view
`ChunkTrackingView` region (`wantChunkLoaded` at radius `lastSendDistance`,
neighbour-buffer 2) — and the chunk is GENERATED; positions independently enter
the tick queue when inside the tick view distance (`wantChunkTicked`,
`max(|dx|,|dz|) <= lastTickDistance`).
The GENERATED→TICK promotion (which replaces the
`GENERATED`-level ticket with the `TICK_TICKET_LEVEL` = 31 ticket, keeping the
chunk at the generated level for anyone else) is gated only on the chunk's
neighbours being generated/ticking within `FULL_LOADED − ENTITY_TICKING = 2`
(`areNeighboursGenerated`, the 5×5 square) — **not** on the chunk having been
sent. The client is given a grace via the per-position send that runs in the same
tick loop, but Paper does not sequence TICK after send. When a chunk becomes
unloaded, the loader demotes the position back down the stages.

**[Rivet]** The send *order* Rivet emits is the **deterministic coordinate raster**
already adopted by the #192/#159 join scenario and recorded in
`crates/rivet-server/src/server/level/player_chunk_loader.rs`: an X-major /
Z-minor sweep of the square view (corners skipped), because Paper's raw receive
order (a squared-distance heap whose equal-distance tie-break depends on chunk
load timing) is not stable across boots. `rivet-capture`'s `ordering.rs` therefore
excludes chunk order from the parity contract and `canonicalize` sorts chunk
packets by coordinate. **Chunk send order is deliberately not Paper-faithful
wire order** — it is the canonicalized order the fixtures byte-match. What is
Paper-faithful and pinned here: the six ticket stages, the load/send/tick
view-distance relationships, the independent send-vs-tick enqueueing and the
neighbour-generated gate on TICK promotion (§4 above), and the per-player rate
limits (bounded burst, not exact tick counts — rate limiting is backpressure,
not parity).

The current `player_chunk_loader.rs` resolves every view position directly from
the deterministic superflat content; the #185 wave replaces that direct
resolution with the pipeline (tickets → holder → status) while keeping the same
raster order.

---

## 5. Tick-thread ownership boundaries (D5)

**[Rivet]** One tick thread owns all game state. The pipeline lives under that
owner, not on the tokio side and not in shared locks:

- **Owned by the tick thread:** the `ChunkMap` holder table, `DistanceManager`
  ticket graph, ticket propagators, per-holder `requestedGenStatus`/completion
  array, the player loader queues, and the send path (packets cross to the
  connection's bounded outbound channel per OWNERSHIP §Network). `ChunkFullTask`
  (promotion to `FULL`) and the `FULL`-status game tick run on the tick thread —
  this mirrors Paper's `mainThreadExecutor` + `ChunkStatusTasks.full`.
- **Detached worldgen** (rayon per DECISIONS D5, crossfire per CRATES.md): the
  parallel-capable statuses run on detached `ProtoChunk` values; results are
  merged back into the holder's completion array on the tick thread, exactly the
  OWNERSHIP §Chunks&blocks sentence ("chunk gen/lighting runs on `rayon` on
  detached `ProtoChunk` values, results merged into `ChunkMap` on the tick
  thread via channel"). No `Arc<RwLock>` game state (§OWNERSHIP the rule).
- **Storage/IO workers:** region-file reads, `SerializableChunkData` parse,
  compression, and writes run off the tick thread (§8). The single-threaded
  save-order discipline comes from the storage spec (§11 of `region-file-format-spec.md`).
- **The #184 light seam** (`ThreadedLevelLightEngine` → starlight provider) is
  the boundary where light computation crosses to the starlight compute units.
  The seam is on main: `LevelLightEngine` (`rivet-world`) is the facade that
  owns the world's vertical extent and an `Option<Box<dyn StarLightProvider>>`
  (the `starlight$getLightEngine()` surface), and `rivet-server` supplies the
  concrete provider — currently `StubStarLightProvider`, a no-op stand-in for
  the real `StarLightInterface` propagation engines (still deferred with the
  Starlight unit, `RivetTodo(#184)`). Until a `LIGHT`-status wave plugs in, the
  pipeline's light seam is the `EMPTY` stub (§1).

Java's `ReentrantAreaLock`/`schedulingLockArea` (region locks) are the *mechanism*
Paper uses to serialize stage-1/3 of unload and task scheduling across its worker
threads. The lock grid's cell size is `1 << lockShift`, where
`lockShift = max(moonrise$getRegionChunkShift(), SECTION_SHIFT)`; `SECTION_SHIFT
= 6`, and in this pinned build `moonrise$getRegionChunkShift()` delegates to
Folia's placeholder `TickRegions.getRegionChunkShift()`, which also returns
`SECTION_SHIFT` — so `lockShift = 6` and the cells are 64×64 chunks (a Folia
deployment may configure a different region-chunk shift). Under D5 the single
tick thread makes those locks degenerate (no contention), so Rivet does not port
the lock grid — the **invariant** it enforces (a column's schedule/unload
decisions are atomic with respect to its neighbour-read/write set) is preserved
by tick-thread ownership.

---

## 6. Cancellation and backpressure

**[Paper]** Three backpressure surfaces, all carried into the port:

1. **Ticket downgrade cancels generation** (§2.3): a holder whose ticket level
   rises clamps or cancels its generation task. This is what stops work for
   chunks that leave the load radius — the load/send limiters (below) never see
   them.
2. **Rate limiters** (`RegionizedPlayerChunkLoader`): send, load-ticket and
   generate-ticket allocations are bounded per player per tick by the config
   rates (§4), and the load/generate concurrency caps (`max(5, radiusChunks/5)`)
   bound in-flight work. These are wall-clock/progress caps, not determinism
   inputs.
3. **IO coalescing + no-write-after-shutdown** (§8): saves queue per region and
   coalesce, so unload of a chunk that just saved does not queue a redundant
   write; the pipeline must not enqueue work after the IO set is shutting down.

**[Rivet]** Cancellation is deterministic: when the tick thread downgrades a
holder it either drops the detached generation future (parallel statuses) or
removes the pending task from the per-thread queue. Because worldgen runs on
detached values, cancel never races with an in-progress block write — the merge
into the completion array is the only mutation of shared state and it happens on
the tick thread. The 3-stage unload's final re-check (§2.3) is the guard against
reload-during-unload.

---

## 7. Deterministic-parallelism invariants

The #175/#54 hash gates require: **two boots with the same seed produce
byte-identical world content and identical xxh3-64 chunk hashes** (see
`DECISIONS.md` D12/D13, `#54` seed-hash gate), *while* worldgen runs in
parallel. The pipeline must preserve the following invariants:

1. **Same status, same bytes.** A chunk's content at a given `ChunkStatus` is a
   deterministic function of its position, the world seed, and the statuses of
   the chunks in its dependency window. The parallelism *schedule* never changes
   what bytes a status produces.
2. **Neighbour-read safety.** A status running in parallel may only read
   neighbours at statuses that are guaranteed immutable for the duration (the
   `parallelCapable` set + the requirement that `STRUCTURE_REFERENCES` reads only
   already-created starts; `FEATURES` writes neighbours so it is excluded). The
   access radius (§3.5) is the distance at which columns are independent — two
   columns farther apart than the access radius of their target status cannot
   touch each other's mutable state, so they are free to run in any order.
3. **Write-radius exclusion.** Statuses with a nonzero write radius (`FEATURES`,
   `LIGHT`) never run concurrently with another task whose write area overlaps
   theirs. Under D5 the tick thread guarantees this for the merge; the detached
   worldgen pool must not run two such tasks whose radius-`writeRadius`
   neighbourhoods intersect.
4. **Merge is the only shared mutation.** Detached worldgen produces a complete
   `ProtoChunk` value; the tick thread installs it into the holder's completion
   array. No worker ever mutates the shared chunk map.
5. **Stable iteration.** Any pass over holders/players/chunks that can affect
   bytes (save scheduling, send, the #175 hashing pass) iterates in a
   deterministic order (coordinate order, not hash-map order). The M1
   `chunk_map.rs` already iterates deterministically; the pipeline preserves
   that.
6. **No RNG/clock in the schedule.** Rate limiting, view movement, and ticket
   expiry use wall-clock ticks, but never influence the generated content or the
   save *bytes* — only whether/when work happens.

**[Rivet]** The gate evidence is: (a) `scripts/gate.sh` runs `rivet-oracle
verify` (Paper twin-boot differential) and `rivet-parity` byte-for-byte vs Paper;
(b) the `#54` xxh3-64 seed-hash gate; (c) determinism-under-parallelism — two
runs of the same seed with different rayon thread counts produce identical
hashes. The pipeline wave must not land a change that breaks any of these.

---

## 8. Storage-worker / write ordering

**[Paper]** `MoonriseRegionFileIO` serializes chunk writes per region:

- One **logical writer per region file** (a `RegionFile` handle is
  `synchronized`; the queue only adds ordering, not locking).
- **Per-chunk store coalescing**: later/latest store wins; a superseded
  intermediate store may never reach disk, and the final disk record is the last
  store. Writes are not FIFO.
- **Read-during-write**: a concurrent read of a chunk with an in-progress write
  is served the pending in-memory value, not disk.
- **Flush**: `flushRegionsOnSave` (`RegionFile.flush()` after each `finishWrite`)
  when enabled; otherwise fsync at close/eviction.
- `SerializableChunkData.write()` is the payload writer; the container framing is
  §2/§6 of `docs/region-file-format-spec.md`, the payload NBT is
  `docs/serializable-chunk-data-spec.md`.

**[Rivet]** The unload path (§2.3 stage 2) and the periodic save both call into
this single write path. Rivet ports the ordering invariants (one writer per
region, per-chunk coalescing, read-serves-pending-write, no writes after
shutdown) — these are already the pinned storage-spec invariants (§11 of
`region-file-format-spec.md`). The executor mechanics (Java's
`Concurrent`/`Priority` threading, moonrise's `AreaDependentQueue`) are an
implementation surface; Rivet's storage worker set realizes the same ordering.
Because stage-2 unload runs off the tick thread, the save queue must accept a
`(chunkX, chunkZ, CompoundTag)` unit and complete a callback; the tick thread
only waits on that callback where the #175 hash pass needs the byte identity.

---

## 9. Excluded / deferred Moonrise internals

**[Deferred]** These are **not** part of #185's spine and are marked
`RivetTodo(#185)` in the corresponding modules when a port touches their seam:

- **`ChunkHolderManager` (in the deferred `scheduling` cluster):** the
  `ThreadedTicketLevelPropagator` fixed-point propagation (level updates fan out
  over neighbour holders), `PriorityHolder`, and the `unloadQueue` batching. The
  *invariant* — a ticket at `level L` forces all holders within the propagation
  radius to level `L` too — is what the port must reproduce; the fixed-point
  engine is deferred.
- **`ChunkTaskScheduler` executor plumbing:** `PrioritisedTaskQueue`,
  `AreaDependentQueue`, the `BalancedPrioritisedThreadPool` executor groups
  (main-thread, parallel-gen, radius-aware, load, io, compression, save). Their
  **priorities and ordering heuristics** are deferred. The *dispatch rule* is
  pinned in §3.4 (the `ChunkUpgradeGenericStatusTask` split: parallel-capable →
  the parallel-gen pool; otherwise the radius-aware queue keyed by `writeRadius`
  — a negative write radius is an error). Two status bodies override the queue's
  thread: `ChunkStatusTasks.full` schedules on `context.mainThreadExecutor()`
  (the `ServerChunkCache.MainThreadExecutor`, the main-thread promotion) and the
  light tasks route to the starlight queue; `generateSpawn` runs inline on the
  radius-aware task.
- **`NewChunkHolder`'s neighbour-blocking machinery** (`neighboursBlockingGenTask`
  / `neighboursWaitingForUs`, `addGenerationBlockingNeighbour`): the concrete
  reference-counted graph the scheduler maintains. The *safety* it provides
  (never generate a status whose neighbours are not at the required status) is a
  hard invariant (§7 invariants 1–2); the holder-level bookkeeping is deferred.
- **`ChunkProgressionTask` and the concrete task classes** (the `scheduling.task`
  cluster): the completion-future framework, `CancellableChunkTask`, and the
  load-task split (`ChunkDataLoadTask` → `loadExecutor` off-main + main callback
  is the *shape*; the class bodies are deferred).
- **`ChunkLoadCounter`, `PriorityHolder`, `ServerChunkCache`'s
  `MainThreadExecutor` batching** — same deferral.
- **Folia-style region threading** (multiple parallel "regions" ticking
  independently) — out of scope until M4; D5 is single-threaded.

The `mc.server.level.pipeline.*` M2 stubs (residual `ServerLevel`/`ServerPlayer`
back-references absorbed as stubs in `MANIFEST.tsv`) are port-order
conveniences, not design decisions; the spec above is the design they serve.

---

## 10. Staged implementation slices

The spine lands in dependency order (each slice builds a working product; no
slice ships speculative API):

1. **Value layer** — `mc.server.level.pipeline.level` (`ChunkLevel`,
   `FullChunkStatus`, `ChunkResult`) + `mc.world.level.chunk.status`
   (`ChunkStatus` 12-value enum, `ChunkStep`/`ChunkDependencies`/`ChunkPyramid`
   builders, `ChunkType`, `ChunkStatusTask(s)` bodies as pure functions) +
   `mc.world.level.chunk.generator`. Replaces the slice-local 2-value
   `ChunkStatus` in `chunk_access.rs`/`chunk_source.rs`. Pure values, no server
   state. Gate: unit tests of the accumulated-dependency tables (§3.2) and the
   access-radius tables (§3.5) byte-match the Java build.
2. **Ticket + tracker value layer** — `mc.server.level.pipeline.ticket`
   (`Ticket`/`TicketType`, flags, timeouts) + `pipeline.tracker` (the
   ticket-level propagation graphs `ChunkTracker`/`LoadingChunkTracker`/
   `SimulationChunkTracker` + `SectionTracker`) + `pipeline.view`
   (`ChunkTrackingView`). Pure value/graph types. Gate: ticket-ordering tests,
   tracker fixed-point tests against the Java graphs.
3. **Holder + distance spine** — `pipeline.holder` (`ChunkHolder`/
   `GenerationChunkHolder`/`ChunkGenerationTask`) + `pipeline.distance`
   (`DistanceManager`) + the D5 scheduler realization (the §3.4 dispatch rule on
   the tick thread + detached worldgen pool). This is where the ticket→holder
   lifecycle (§2.3) and cancellation (§6) land. Gate: holder lifecycle tests
   (downgrade cancels, 3-stage unload, `UNLOAD_COOLDOWN`), determinism-under-
   parallelism (same seed, two thread counts → identical hashes).
4. **ChunkMap + ServerChunkCache hub** — `pipeline.chunkmap` + `pipeline.
   servercache` + `pipeline.region` (`WorldGenRegion`), wiring the existing M1
   `chunk_map.rs` and the #184 light seam. Gate: the superflat join scenario
   still byte-matches; `#54` hash gate green.
5. **Send ordering** — the `RegionizedPlayerChunkLoader` ticket stages + rate
   limiters replace the direct-resolution path in `player_chunk_loader.rs`,
   keeping the deterministic raster (§4). Gate: join capture still byte-matches;
   move scenario determinism (#192/#159).

After each slice the marker audit (`scripts/check_markers.py`) stays green; the
`RivetTodo(#185)` markers in `chunk_access.rs`, `chunk_map.rs`,
`player_chunk_loader.rs` are removed slice-by-slice as the corresponding seam is
replaced, not all at once.

---

## 11. Reconciliations with landed work

- **`chunk_access`/`chunk_source` slice-local `ChunkStatus{Empty,Full}`** — a
  placeholder for §3's 12-value enum; slice 1 replaces it. `getChunkForLighting`
  resolves `EMPTY` today and is the #184 seam.
- **`docs/region-file-format-spec.md` §11** already pins the Moonrise IO ordering
  (one writer/region, coalescing, read-serves-pending-write, no writes after
  shutdown). §8 above *consumes* those invariants; no duplication.
- **`docs/serializable-chunk-data-spec.md`** pins the payload NBT the pipeline's
  unload path writes. §8 points at it; the pipeline does not re-specify payload
  bytes.
- **#231 storage foundation** (RegionFile/`RegionBitmap`/compression in
  `rivet-world`) is the storage worker's substrate; the pipeline's save path is
  the caller.
- **#184 lighting seam** — `INITIALIZE_LIGHT`/`LIGHT` statuses and the
  `pipeline.light` unit are where the seam plugs in; `LIGHT` is not parallel
  (§3.4) because the starlight compute units own it. The seam itself is on main
  (#309): `LevelLightEngine` in `rivet-world` owns `Box<dyn StarLightProvider>`
  (the `starlight$getLightEngine()` surface), and `rivet-server` provides the
  concrete impl — `StubStarLightProvider` until the real `StarLightInterface`
  propagation engines land (deferred with the Starlight unit, `RivetTodo(#184)`).
- **#175/#54 hash parity** — the determinism invariants in §7 are the pipeline's
  contract with the hash gate; a pipeline change that breaks two-run-identical
  hashes is a release blocker.
- **OWNERSHIP §Chunks & blocks** — amended to link this spec; the existing
  sentence ("chunk gen/lighting runs on `rayon` on detached `ProtoChunk` values,
  results merged into `ChunkMap` on the tick thread via channel") is the 
  ownership rule §5 realizes; `*(refine: light engine threading before world
  wave)*` is now resolved by this spec + the #184 seam.
