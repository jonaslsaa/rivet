# OWNERSHIP.md — memory architecture

How Java's GC'd, cyclic object graph maps to owned Rust data. This is decided **here, per subsystem, never per-unit**. Implementers follow it; reviewer lens 2 audits against it. v1 — sections marked *(refine)* get detail PRs before their subsystem's first wave.

## The rule

**One owner, IDs everywhere else.** Java back-references (`entity.level`, `blockEntity.level`, `player.connection`) become either (a) an ID/key resolved through the owner, or (b) a `&`/`&mut` parameter passed down the call stack. Storing `Arc<RwLock<T>>` to game state is a design smell — game state has exactly one owner: the tick thread (D5).

## Ownership tree

```
Server
├── ServerConnection (tokio side: accept loop, per-conn tasks)
│     └── channels ⇄ tick thread (packet in/out queues per player)
├── Registries / GameData (immutable after startup, Arc, shared freely)
└── Vec<Level>                        (the worlds)
      ├── ChunkMap: HashMap<ChunkPos, Chunk>
      │     ├── sections: palettes + block states
      │     ├── block_entities: HashMap<BlockPos, BlockEntity>
      │     └── heightmaps, light, poi
      ├── EntityStorage: SlotMap<EntityId, EntityRecord> + spatial + uuid indices
      ├── PlayerList indices (EntityId ↔ ConnectionId ↔ Uuid)
      ├── scheduled ticks, events, raids, …
      └── LevelData (world settings, time, weather)
```

- `EntityId` = slotmap generational key (Java object identity). Vanilla's `int` network entity-id is a separate field mapped through an index.
- Cross-world references (portals, teleports) go through `Server` by `(DimensionId, EntityId)`.

## Entity hierarchy *(the hard one)*

- Struct embedding per PORTING.md: `Zombie { monster: Monster { mob: Mob { living: LivingEntity { entity: Entity }}}}`. Field access is `self.monster.mob.living.entity.pos` — verbose, faithful, greppable; accessor helpers (`fn entity(&self) -> &Entity`) generated per level.
- Storage: `enum AnyEntity` over concrete leaf types (~100 variants, generated), stored by value in the slotmap. Enum dispatch, not `Box<dyn>`: match generates the vtable, downcasts (`instanceof Zombie`) become patterns, and memory stays contiguous.
- Behavior: one trait per abstract level (`EntityBehavior`, `LivingEntityBehavior`, `MobBehavior`…) implemented by leaf types; default impls carry the Java base-class bodies; `super.tick()` = explicit `living_entity::tick(self, ctx)` call.

### The reentrancy problem (Java: `entity.tick()` touches `level.getEntity(other)`)
Rust can't hold `&mut entity` (inside the arena) and `&mut level` (owning the arena) at once. Pattern: **take-tick-putback** — the tick loop removes the entity value from the slotmap slot, ticks it with `(&mut entity, ctx: &mut LevelCtx)`, reinserts. `LevelCtx` exposes the rest of the level (chunks, other entities, RNG, events) without the ticked entity. Access to *self through the level* during own tick (rare in vanilla) resolves by ID lookup returning `None` — matches Java semantics closely enough; deviations get documented per call site. *(refine: exact LevelCtx API before the entity wave)*

## Chunks & blocks
Chunks owned by `ChunkMap` by value. Block state = palette index into generated global state table (`rivet-registry`), copy `u32`-ish IDs, no references. BlockEntities live in their chunk; ticking uses the same take-tick-putback pattern with a `BlockEntityCtx`. Chunk gen/lighting runs on `rayon` on detached `ProtoChunk` values, results merged into `ChunkMap` on the tick thread via channel.

The chunk *pipeline* — ticket levels and holder lifecycle, the `ChunkStatus` generation DAG and radii, chunk send ordering, storage-worker/write ordering, cancellation/backpressure, and the determinism-under-parallelism invariants — is specified in `docs/chunk-pipeline-spec.md` (issue #185). Tick-thread ownership (§5 there) is the realization of this section's rule; the Moonrise scheduler internals (executors, propagation engine) are deliberately deferred to that issue.

`ChunkPos`/`SectionPos` live in `rivet-registry::core` as pure value types, resolved by ID — `ChunkPyramid.MAX_CHUNK_COORDINATE_VALUE` moves to a `const` there so `ChunkPos` stays value-only. (Java puts `ChunkPos` in `world.level`; the module mirror is a convenience and cycle-breaking justifies the one-line move.)

## Chunk storage workers (region files) — storage-worker amendment

Issue #231 amendment (the `world.level.chunk.storage` slice). Region-file IO is **not game state**: `RegionFile`/`RegionFileStorage`/`IOWorker` are owned handles on the chunk-IO side (rayon/tokio worker pools), never stored inside `ChunkMap`. The tick thread hands **owned `CompoundTag` values** across the channel boundary — no `Arc<RwLock>` on chunk data anywhere on this path.

- Per-`RegionFile` mutual exclusion (Java `synchronized` on `write`/`getChunkDataInputStream`) maps to a `Mutex<RegionFile>` held by the region's single IO task, or a region-keyed single-writer queue (`chunkX >> 5, chunkZ >> 5`). This cross-thread IO lock is explicitly inside the "cross-thread queues only" exception below — a worker/queue mutex, never a lock on game state.
- Chunk ownership is unchanged: the chunk is owned by `ChunkMap` by value; `SerializableChunkData` builds/reads a plain value `CompoundTag` on the worker side. Starlight light arrays survive load→save as opaque bytes in the compound — no engine, no shared state.
- Value types: `ChunkPos` (region math `& ~31`, `getRegionLocalX/Z`, `pack`) is a `Copy` value type; `RegionStorageInfo` is a `Clone` value type (it owns `String`/`ResourceKey` fields, so it cannot be `Copy`). Java's `info.dfuType()[0] = dataFixType` mutable-array hack becomes a plain `is_chunk_data: bool` field — do not reproduce a shared-mutable array.
- Codec selection is a frozen value: the `RegionFileVersion` chosen by `configure` is shared freely like `GameData`; the gzip/deflate/lz4 stream wrappers are pure functions over `Read`/`Write`.
- `RegionBitmap` is an owned `BitSet`-equivalent inside each `RegionFile` — sector allocation is per-file derived state, never global.

## Network
Per-connection tokio task owns the socket, encryption, and framing; decoded packets flow to the tick thread over bounded channels keyed by `ConnectionId`; outbound is the reverse. Handshake/status/login handled entirely on the tokio side; play-state packets are game state and cross to the tick thread. Packets are plain owned structs — no lifetimes in packet types (accept the copies; optimize later with `bytes::Bytes` for blobs).

## Registries, tags, recipes

Loaded/generated at startup, frozen, `Arc<GameData>` shared everywhere including
worker pools. Interior mutability forbidden after freeze.

Registry model (decided #107, implemented in rivet-registry):

- **One concrete `Registry<T>`** (no trait): owns `Vec<T>` by insertion order
  (append-only; **element id == holder id == network id == insertion index**),
  plus `by_location`/`by_key` (`FxHashMap<_, u32>`). `DefaultedRegistry` is
  `Option<u32> default_id` with its asymmetric fallbacks preserved. Frozen
  registries are immutable value tables.
- **Builder → freeze:** mutable `RegistryBuilder<T>` (pre-freeze `register`,
  `get_or_create_holder`, `create_intrusive_holder`, `bind_tags`) is consumed by
  `freeze()` → `Registry<T>`; `freeze()` panics with sorted unbound keys like
  `MappedRegistry.freeze()`. The `frozen` boolean + `validateWrite()` are
  compile-time (phase types), not runtime checks.
- **`Holder<T>` is an ID, not a value:** `Direct(T)` (unregistered, decode-only)
  or `Reference{ registry: RegistryId, id: u32 }` (Copy, 8 bytes). All
  `holder.value()/is(tag)/key()/tags()` resolve through the owning
  `&Registry<T>` / `&HolderLookup<T>` — OWNERSHIP's back-reference rule. No
  stored `Arc`/`&Registry` in game state (FFI marshal IDs).
- **Registry identity is `RegistryId` (a per-instance u32), distinct from the
  `ResourceKey<Registry<T>>` key** — one key can have many instances (per-world
  registries); holder serialization-owner checks compare `RegistryId`.
- **`ResourceKey<T>`/`TagKey<T>` are value types** (`PhantomData<fn() -> T>`,
  no `T` bound on Eq/Hash). Java's weak interning makes its `==` equivalent to
  value equality; Rust derives it. No interning, no pointer comparisons.
- **Heterogeneous registry sets (`RegistryAccess`, the ROOT registry) use
  `trait AnyRegistry: Any` + `Box<dyn AnyRegistry>`**, downcast at those two
  erased boundaries only.
- **Reload = rebuild + swap:** datapack reload builds a fresh `Registry`/
  `GameData` (via `RegistrySetBuilder.buildPatch`) and atomically replaces the
  old. No in-place tag rebind on frozen registries; code holding a `&Registry`
  across a reload sees the old table (document per site).
- **Protocol codecs live in `rivet-protocol`, never `rivet-registry`:**
  `StreamCodec` impls for Identifier/ResourceKey/TagKey/Holder/HolderSet live in
  `rivet-protocol`. The pure value types (BlockPos/Vec3i/SectionPos/UUIDUtil/
  Direction/GlobalPos/BlockBox/Rotations) stay in `rivet-registry::core`, with
  only their `StreamCodec` impls crossing to `rivet-protocol`. The network-sync
  `RegistrySynchronization` and `ClientAsset` (both `net.minecraft.network.codec`-
  dependent) move to `rivet-protocol` outright. Dependency direction:
  `rivet-protocol → rivet-registry`; `rivet-registry` never depends on
  `rivet-protocol`.
- `GameData` owns the provider; `Level` may hold a per-dimension provider
  (layer order STATIC → WORLDGEN → DIMENSIONS → RELOADABLE is observable —
  keep an explicit ordered vec).

## Events (Bukkit/Paper layer)
Events dispatch synchronously on the tick thread at the same call sites as Paper's patches. Handlers (Rust or JVM-bridged) receive `&mut` access via the same ctx objects — no event queue, ordering matches Bukkit.

## JVM adapter boundary
JVM plugin thread and tick thread alternate via a rendezvous (tick thread parks while plugin callbacks run — Bukkit semantics, zero data races by construction). All FFI calls marshal IDs (`EntityId`, `BlockPos`, …), never pointers into arenas; lookups re-resolve per call. *(refine: rivet-ffi handle table design at M1)*

## Allowed shared-state exceptions
`Arc`: registries/GameData, config snapshots, the connection registry. `Mutex`: cross-thread queues only. Anything else needs an OWNERSHIP.md amendment PR first.
