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
Chunks owned by `ChunkMap` by value. Block state = palette index into generated global state table (`rivet-registry`), copy `u32`-ish IDs, no references. BlockEntities live in their chunk; ticking uses the same take-tick-putback pattern with a `BlockEntityCtx`. Chunk gen/lighting runs on `rayon` on detached `ProtoChunk` values, results merged into `ChunkMap` on the tick thread via channel. *(refine: light engine threading before world wave)*

## Network
Per-connection tokio task owns the socket, encryption, and framing; decoded packets flow to the tick thread over bounded channels keyed by `ConnectionId`; outbound is the reverse. Handshake/status/login handled entirely on the tokio side; play-state packets are game state and cross to the tick thread. Packets are plain owned structs — no lifetimes in packet types (accept the copies; optimize later with `bytes::Bytes` for blobs).

## Registries, tags, recipes
Loaded/generated at startup, frozen, `Arc<GameData>` shared everywhere including worker pools. Interior mutability forbidden after freeze.

## Events (Bukkit/Paper layer)
Events dispatch synchronously on the tick thread at the same call sites as Paper's patches. Handlers (Rust or JVM-bridged) receive `&mut` access via the same ctx objects — no event queue, ordering matches Bukkit.

## JVM adapter boundary
JVM plugin thread and tick thread alternate via a rendezvous (tick thread parks while plugin callbacks run — Bukkit semantics, zero data races by construction). All FFI calls marshal IDs (`EntityId`, `BlockPos`, …), never pointers into arenas; lookups re-resolve per call. *(refine: rivet-ffi handle table design at M1)*

## Allowed shared-state exceptions
`Arc`: registries/GameData, config snapshots, the connection registry. `Mutex`: cross-thread queues only. Anything else needs an OWNERSHIP.md amendment PR first.
