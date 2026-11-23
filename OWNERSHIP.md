# OWNERSHIP.md — memory architecture

How Java's GC'd, cyclic object graph maps to owned Rust data. This is decided **here, per subsystem, never per unit**. Implementers follow it; reviewers audit against it. It changes only through dedicated PRs (D9); ROADMAP R5 is the signal that would reopen the entity model. Crate placement of every class is D15's ledger, not this file.

## The rule

**One owner, IDs everywhere else.** Game state has exactly one owner: `MinecraftServer` on the tick thread (D5). A Java reference to a game object (`entity.level`, `this.mob`, `damageSource.getEntity()`, `blockEntity.level`, `player.containerMenu`) becomes a **handle**, an ID or key resolved through the owner at each use. No Rust borrow of game state is held across a call that can reach game state. `Arc<RwLock<T>>` on game state is forbidden; the exceptions are listed at the end.

## Ownership tree

```
tick thread
├── MinecraftServer                                  all game state; one &mut at a time
│   ├── entity arena: SlotMap<EntityKey, Box<dyn EntityBehavior>>
│   ├── block-entity arena: SlotMap<BlockEntityKey, Box<dyn BlockEntityBehavior>>
│   ├── levels: ResourceKey<Level> → ServerLevel     Java LinkedHashMap order; LevelId = index
│   │   └── ServerLevel { level: Level, … }
│   │       ├── chunk source / ChunkMap: LevelChunk by value
│   │       │     sections (palettes), heightmaps, light, POI,
│   │       │     block_entities: BlockPos → BlockEntityKey      (index only)
│   │       ├── EntityLookup (Moonrise): by-int-id, by-uuid, section, tick-list, tracking indices of EntityKey
│   │       ├── block-entity ticker list
│   │       └── scheduled ticks, block events, raids, saved data, LevelData, …
│   ├── PlayerList: EntityKey ↔ ConnectionId ↔ UUID indices
│   └── server services: commands, recipes, advancements, scoreboard, …
└── PluginBridge                                     beside the server; no game state (§Events)

shared immutable: Arc<GameData> (registries), the 'static Blocks/Fluids/Items behaviour tables, config snapshots
off the tick thread: tokio connections and rayon chunk workers, talking to the tick thread over channels (D5)
```

The arenas are server-wide because a Java object's identity does not depend on its level: `ServerPlayer.teleport` moves the same object between levels, and references to it (`Mob.target`, a passenger list) stay valid. Levels own only indices.

- **`EntityKey`** is a `slotmap` generational key: the object identity of an entity, its arena key, and the handle every Rust reference to an entity holds. Java's `int id` (`Entity.getId()`) is *not* the identity: it is a field allocated by the one port of `ServerLevel.ENTITY_COUNTER`/`getNextEntityId()`, it can change (`Entity.setId`, called by `PaperPlayerConfigurationConnection.applyPendingEntityId`), and several objects can share one (`BaseSpawner.SET_DISPLAY_ENTITY_ID` sets every spawner display entity to -1). The `EntityLookup` by-int-id index maps it to a key for packets and `Level.getEntity(int)`; `setId` updates that index.
- **`BlockEntityKey`** is the same kind of key for block entities. Java has no block-entity id; the key is the object identity, so the block entity that replaces another at the same position is a different key, just as Java's `this` and `level.getBlockEntity(pos)` are then different objects.
- **`LevelId`** is a level's index in `MinecraftServer.levels`; `ResourceKey<Level>` stays the persistent identity. An entity's level is its `level: LevelId` field.

## Receivers and handles

Every translated method takes one of two forms.

- **Handle form** `fn m(ctx: &mut LevelCtx, this: H, …)` is used when the receiver lives inside the state `ctx` owns (entities, block entities, levels, the server, components of those) and the Java body, or any override of it, reaches the level, the server or another game object. The body re-borrows the receiver through `ctx` for each group of statements that touches it.
- **Receiver form** `&self` / `&mut self` is used (a) for methods of those same objects whose Java body and overrides touch only the receiver's own fields and the arguments, with no `ctx` parameter, and (b) for objects outside `ctx`: the `'static` block, fluid and item singletons, and values not admitted to the game state (a block entity just built by `newBlockEntity`, `ProtoChunk` contents, locals). Methods in (b) take `ctx` as a parameter wherever Java passes a `Level`.

The compiler polices the split: a receiver-form method of an object inside `ctx` has no `ctx`, so it cannot call a handle-form method. When that bites, the method becomes handle form, decided once on the hub class (ROADMAP R6), never per call site.

**Why not take-tick-putback** (v1 of this file, and PR #767's `level_ctx.rs`): Java routinely reaches the running object through the level. `LivingEntity.hurtServer` resolves `source.getEntity()` (the mob that is mid-`doHurtTarget` in its own tick) and mutates it: `setLastHurtByMob`, then thorns through `EnchantmentHelper.doPostAttackEffects`. A hopper's `setChanged()` calls `Level.updateNeighbourForOutputSignal`, which reaches `ComparatorBlock`, which calls `HopperBlock.getAnalogOutputSignal`, which reads `level.getBlockEntity(pos)`: the hopper that is mid-tick. A receiver moved out of its slot is invisible on those paths, and safe Rust cannot lend `&mut self` back to the level. A handle is what Java's reference is, and re-resolving it on every use is exactly Java's aliasing semantics.

Rules:

1. **No borrow across a `ctx` call.** Read what the Java expression reads into locals, in Java evaluation order, then call, then re-borrow. Java re-reads fields after a call; so does the port.
2. **Dereferencing a handle is not a Java lookup.** `ctx.entity(key)` dereferences a Java reference: it finds any object in the arena, whatever its level, including one constructed but not yet added and one removed earlier in this tick. Java lookups (`Level.getEntity(int)`, `getEntities(except, aabb, predicate)`, `getBlockEntity(pos)` with its creation-type semantics) go through the ported indices with Java's semantics. Never answer one with the other.
3. **Construction and admission.** An entity is constructed into the arena, because Java's constructor takes the level and allocates the id, and Java calls level-touching methods (`snapTo`, `finalizeSpawn` in `NaturalSpawner`) before `addFreshEntity`. Admission (`addFreshEntity`, load, `addDuringTeleport`) inserts the key into a level's `EntityLookup` indices. A block entity is a value until `LevelChunk.setBlockEntity` admits it into the arena; Java calls only receiver-form methods (`loadWithComponents`, `setBlockState`) on it before that. Worldgen threads never touch an arena (§Chunks).
4. **Removal is immediate; freeing is explicit.** `Entity.setRemoved` updates every index in the same call (`levelCallback.onRemove`), as in Java, and Moonrise's iterator-safe tick list keeps the running iteration valid. At the end of each server tick, the arena frees every entry that is removed or was never admitted, unless it is **pinned**. A pin is Rivet's stand-in for a GC root and is taken only where Java stores a reference to such an object beyond the tick, with a comment citing the Java field: today `TrialSpawnerStateData.displayEntity` (a never-admitted display entity), `connection.savedPlayerForLegacyEvents` (the `ServerPlayer` that `PrepareSpawnTask` builds before admission), and the JVM adapter's pin for every entity a live `CraftEntity` wrapper holds (§JVM adapter boundary). A handle to a freed object dereferences to `None`, which ports Java's `isRemoved()` and `!isAlive()` checks. A Java site that reads other fields of a removed object through a stored, unpinned reference is a fidelity gap: mark it `RivetTodo(#issue)`; reviewers flag it.
5. **No invented queues.** Effects happen in Java order. Deferred structures exist only where Java has them: `Level.pendingBlockEntityTickers` while `tickingBlockEntities`, `EntityTickList`'s `IteratorSafeOrderedReferenceSet`, `ServerLevel.blockEvents`, `LevelTicks`, the `BlockableEventLoop` task queue, and Moonrise's `canRemoveEntity` guard (which drops the removal with a warning, as Paper does). A queue added to satisfy the borrow checker reorders observable effects and is a blocking review finding.
6. **Iterate as Java iterates.** Port the iterator semantics of the Java collection, and re-resolve the collection through `ctx` at each step when the loop body takes `ctx`. An index loop that re-reads `size()` stays an index loop; Moonrise's iterator-safe collections and fastutil's linked collections (whose iterators are *not* fail-fast: `GoalSelector.tick()` walks `availableGoals`, an `ObjectLinkedOpenHashSet`, while goal code can fire `EntityTargetLivingEntityEvent` whose listeners may edit the goals) are ported with their exact iteration behaviour. Iterating a snapshot is allowed only where Java itself iterates a copy (`new ArrayList<>(…)`, `List.copyOf`, `toArray`). Never take a collection or component out of its owner for a loop.
7. **Components.** Owned sub-objects without identity of their own (`GoalSelector` and its goals, `Brain` and its behaviors, `PathNavigation`, the move, look and jump controls, `FoodData`, `Inventory`, `AbstractContainerMenu`, `ServerGamePacketListenerImpl`) live inside their owner. Their Java back-reference (`this.mob`) becomes a field holding the owner's key. A component method that needs `ctx` is handle form with a component handle: the owner's key plus the component's stable key inside the owner (a goal's slot in the selector's ported linked set, the open menu's `containerId`), resolved through the owner on every access (`ctx.mob_mut(this.mob).goal_selector.goal_mut(this.slot)`).
8. **References typed by an interface** whose implementors are game objects become an enum of handles, one variant per implementor family, resolved through `ctx`. `Container` (block entities, container entities, the player inventory, compound containers) is the main case. Item stacks follow Java's own `SlotAccess`: see §Items.

## LevelCtx

`LevelCtx` replaces every back-reference Java code follows to world state: `this.level()`, `level.getServer()`, `this.server`, `level.getCraftServer()`. It is one concrete struct, not a trait, at `rivet_server::server::level::level_ctx`; it has no Java counterpart, and its module doc says so. `ServerCtx` is the same without a current level, for code that belongs to no level: `MinecraftServer`, `PlayerList`, commands, the connection layer.

```rust
pub struct ServerCtx<'t> {
    pub server: &'t mut MinecraftServer,
    pub bridge: &'t PluginBridge,
}

pub struct LevelCtx<'t> {
    pub server: &'t mut MinecraftServer,
    pub bridge: &'t PluginBridge,
    /// Java `this.level()` for the code running now.
    pub level: LevelId,
}

impl ServerCtx<'_> {
    pub fn level_ctx(&mut self, level: LevelId) -> LevelCtx<'_>;
    // handle dereference (rule 2); the same methods exist on LevelCtx
    pub fn entity(&self, key: EntityKey) -> Option<&dyn EntityBehavior>;
    pub fn entity_mut(&mut self, key: EntityKey) -> Option<&mut dyn EntityBehavior>;
    pub fn entity_dispatch(&self, key: EntityKey) -> Option<&'static EntityDispatchRow>;
    pub fn block_entity(&self, id: BlockEntityKey) -> Option<&dyn BlockEntityBehavior>;
    pub fn block_entity_mut(&mut self, id: BlockEntityKey) -> Option<&mut dyn BlockEntityBehavior>;
    pub fn block_entity_dispatch(&self, id: BlockEntityKey) -> Option<&'static BlockEntityDispatchRow>;
    pub fn pin(&mut self, key: EntityKey);   // rule 4, Java-cited sites only
    pub fn unpin(&mut self, key: EntityKey);
    pub fn call_event<E: Event>(&mut self, event: &mut E);
}

impl LevelCtx<'_> {
    pub fn level(&self) -> &ServerLevel;
    pub fn level_mut(&mut self) -> &mut ServerLevel;
    pub fn server_ctx(&mut self) -> ServerCtx<'_>;              // Java level.getServer()
    pub fn in_level(&mut self, level: LevelId) -> LevelCtx<'_>; // Java server.getLevel(key)
    // plus the ServerCtx dereference, pin and call_event methods
}
```

- Typed accessors per hierarchy class (`ctx.living_mut(this)`, `ctx.mob(this)`, `ctx.block_entity_as::<HopperBlockEntity>(id)`) are generated by the family scaffold. Accessors used on `this` panic if the object is gone, because dispatch only reaches live objects and rule 4 keeps removed ones until the tick ends; accessors used on stored references return `Option`.
- The tick loop owns `MinecraftServer` and `PluginBridge` side by side and builds one `ServerCtx` per tick. `MinecraftServer.tickChildren` builds a `LevelCtx` per level for `ServerLevel::tick(ctx)`; packet handling builds one for the player's current level (its `level` field). Rayon and tokio threads never see a `ctx`.
- A handle-form method whose receiver changes level mid-body (a player in `ServerPlayer.teleport`; Java keeps the same object, and `this.level()` changes) continues with `ctx.in_level(new_level)`. Its key does not change.
- `ServerLevel` and `Level` methods follow the same two forms: those that touch only the level's own fields are `&self`/`&mut self`; those that reach entities, block behaviour, the server or events take `ctx`. The entity-returning members of the reader interfaces (`EntityGetter.getEntities`, `CollisionGetter.getEntityCollisions`, `LevelAccessor.getServer`) are therefore live-crate members (D15 split).

**What PR #767's `level_ctx.rs` becomes.** Kept: the Paper-ordered passenger traversal (`tickNonPassenger`/`tickPassenger` with Paper's activation-range `isActive` branch), the preconditions and ordering of `startRiding`, `stopRiding` and `ejectPassengers`, the Moonrise `canRemoveEntity` guard, and int ids starting at 1. Moved: riding and removal become handle-form `Entity` methods in `world::entity::entity`, where Java declares them; the traversal becomes `ServerLevel::tick_non_passenger`/`tick_passenger` in `server::level::server_level`. Deleted: `LifecycleEntity` (replaced by §Class hierarchies), the `IndexMap<i32, Option<_>>` slots keyed by int id with `taken_ids` (take-tick-putback), `pending_removals`/`flush_pending_removals` (Java removes immediately), the `cancel_mount`/`cancel_dismount` flags (`EntityMountEvent`/`EntityDismountEvent` through `call_event`), the `events: Vec<LifecycleEvent>` log (test instrumentation with no Java counterpart; tests observe state, D18), and the separate `NetworkIdAllocator`.

## Class hierarchies: the dispatch pattern

This applies to every Java class hierarchy with virtual methods: entities, blocks, fluids, items, block entities, menus, goals and behaviors. Java dispatches on the object's runtime class; the port keeps one Rust construct per Java construct.

- **Data.** Each Java class is a struct of its own fields, with its parent embedded as a field named after the parent (`struct Husk { zombie: Zombie }`, PORTING.md).
- **Value trait `<Class>Behavior`** for the root and for every class that has subclasses; its supertrait is the parent's trait, and the root's supertrait is `Any`. It is object safe and holds: generated accessors to the embedded struct at each level (`entity()`, `living_mut()`), generated `as_<x>()`/`as_<x>_mut()` accessors for every class with subclasses and every Java interface in the hierarchy, and the receiver-form virtual methods first declared at that class. Java `instanceof X` is `as_x().is_some()` for those; for a class without subclasses it is an `Any` downcast.
- **Dispatch trait `<Class>Dispatch`**, for hierarchies whose objects live inside `ctx` (entities, block entities), holds the handle-form virtual methods first declared at that class: `fn tick(&self, ctx: &mut LevelCtx, this: EntityKey)`. `&self` is a per-class `'static` marker, not the object. Dispatch objects are found through a generated table indexed by the `EntityType` or `BlockEntityType` registry id (each Java type's factory fixes the runtime class), with one `Option<&'static dyn XDispatch>` per hierarchy level in each row.
- **Bodies.** The Java body of a method declared or overridden in class X is a free function in X's module: `pub fn tick(ctx: &mut LevelCtx, this: EntityKey)` in handle form, `pub fn random_tick(this: &dyn CropBlockBehavior, …)` in receiver form. Inside a body, a virtual call on `this` always goes through dynamic dispatch (`ctx.entity_dispatch(this)`, `this.get_max_age()`), because that is what Java does when a subclass overrides it; there are no static shortcuts. `super.m()` is a direct call to the parent class's free function.
- **Routing is generated.** For every concrete class, the family scaffold (ROADMAP §2.3) generates the trait impls from the Java class graph: the accessors, the `as_*` methods, and, for each virtual method, the call to the free function of the nearest class that declares it in Java's method resolution order. A missed inherited override silently runs the base body, so routing is never written by hand: it is regenerated from the Java class graph whenever a class in the family changes. Hand-written code is the free functions: the Java bodies.
- **Interfaces** with methods (`EntityBlock`, `Container`, `SimpleWaterloggedBlock`, `BonemealableBlock`, `Leashable`) are traits; the root value trait's `as_<iface>()` returns `Option<&dyn Iface>`.
- **R1 readiness.** All dispatch goes through `'static` trait objects and type-indexed tables, so a leaf family can move to a higher crate and register its rows at bootstrap without changing a caller.
- There is no `AnyEntity` enum: ROADMAP Batch 3A's "generated `AnyEntity` dispatch (#487)" is this generated routing and dispatch table, and `CRATES.md`'s `rivet-entity` row is superseded by it.

## Entities

Java: `Entity` → `LivingEntity` → `Mob` → … (158 entity types in `EntityTypes`). `ServerLevel` ticks them from `entityTickList`, an `EntityTickList` over Moonrise's `IteratorSafeOrderedReferenceSet`, whose iterator never visits entries added during the iteration.

- **Storage.** The server-wide arena maps `EntityKey → Box<dyn EntityBehavior>`; each level's Moonrise `EntityLookup` holds only keys. Boxed, because concrete classes range from `ItemEntity` to `ServerPlayer` (inventories, menus, advancements): an enum would size every slot for the largest. The arena's internal layout is an implementation choice; the `LevelCtx` API is the contract.
- **Traits.** `EntityBehavior`/`EntityDispatch`, `LivingEntityBehavior`/`LivingEntityDispatch`, `MobBehavior`/`MobDispatch`, and so on down every class with subclasses.
- **Tick.** `ServerLevel::tick` walks the ported `EntityTickList` with Java's checks in Java's order (`isRemoved`, `isEntityFrozen`, `checkDespawn`, the vehicle and `hasPassenger` test, `stopRiding`), then `tick_non_passenger(ctx, key)`: `setOldPosAndRot`, `tickCount++`, `totalEntityAge++`, the activation-range `checkIfActive`, then `tick` and `postTick` or `inactiveTick` through the dispatch table, then each passenger through `tick_passenger` (`ServerLevel.tickNonPassenger`/`tickPassenger`).
- **Removal.** `entity::set_removed(ctx, this, reason, cause)` follows `Entity.setRemoved`: the Moonrise guard, the `EntityRemoveEvent` hook, `removalReason`, `stopRiding` when `shouldDestroy`, passengers' `stopRiding` unless `UNLOADED_TO_CHUNK`, `levelCallback.onRemove`, `onRemoval`, then Paper's `retireScheduler()` for a non-player not already removed and not leaving by `CHANGED_DIMENSION` (a plugin-bridge hook: it retires the Folia entity scheduler). All immediate (rule 4).
- **Players.** `ServerPlayer` is an entity like any other: arena entry plus `PlayerList` indices; its connection listener, inventory and open menu are components. Changing dimension moves only index entries from one level's `EntityLookup` to the other's and updates the entity's `level` field; the key and the arena entry stay, because Java keeps the same object.
- **Loading.** Java `EntityType.loadEntityRecursive`/`create` constructs into the arena, runs the receiver-form `load(ValueInput)`, then admits (rule 3).

## Blocks and fluids

Java: `BlockBehaviour` (abstract) → `Block` → about 295 subclasses. `Blocks` constructs one instance per block, in registration order, during static initialization; every field is final. `BlockBehaviour.BlockStateBase` delegates as `this.getBlock().m(this.asState(), …)`, and its `initCache` fills per-state caches by calling those methods once.

- **Identity.** The `Block` handle (`rivet-world`) and `BlockState` (`rivet-registry`) stay the only identities. Java `==` on blocks and states is handle equality.
- **Instances.** `'static` immutable singletons in one table, built in Java order by the translated `Blocks` registration code, a `LazyLock` as Java's static initializer is. `Blocks` is split (D15): the named `Block` handles stay in `rivet-world`, the registration and table sit in the live-game crate. Lookup is the extension method `block.behavior() -> &'static dyn BlockBehavior` (trait `BlockLive`), because Java's `state.getBlock()` *is* the behaviour object. A startup check asserts table index == generated registry id.
- **Data.** The struct `BlockBehaviour` holds Java `BlockBehaviour`'s instance fields (`properties`, `hasCollision`, `explosionResistance`, `soundType`, `friction`, `drops`, …). Java `Block`'s own fields (`stateDefinition`, `defaultBlockState`, the registry holder, `item`) are served by the generated tables behind the handle, so the embedding chain starts at `BlockBehaviour`: `struct CropBlock { vegetation_block: VegetationBlock }`, `struct VegetationBlock { block: BlockBehaviour }`. A plain Java `Block` instance is a `BlockBehaviour` value.
- **Dispatch.** One root value trait, `BlockBehavior` (named for `Block`, per the `<Class>Behavior` rule), carries the virtual surface Java splits across `BlockBehaviour` and `Block`; `BlockBehaviour` is abstract with `Block` as its only subclass, so they are one dispatch level. Bodies sit in `block_behaviour.rs` or `block.rs`, wherever Java declares them. Everything is receiver form, because `&self` is a static singleton and never inside `ctx`. Java `Level`/`ServerLevel` parameters become `&mut LevelCtx`; reader-interface parameters (`BlockGetter`, `LevelReader`, `CollisionGetter`) become `&dyn` reader traits, which worldgen satisfies with `WorldGenRegion` and the live game with `ctx.level()`.
- **Live `BlockState` methods** (`tick`, `randomTick`, `useItemOn`, `entityInside`, `handleNeighborChanged`, …) are the `BlockStateBaseLive` extension trait (D15 split), implemented for `BlockState` in the live-game crate, each delegating as Java does: `self.block().behavior().tick(*self, ctx, pos, random)`. The cached `BlockStateBase` fields stay in the generated tables (`block_behaviors.rs`, dumped from live Paper). The ported virtuals that Java's `initCache` calls must reproduce those tables, checked by a fixture-backed test over all block states (D18 kind 2).
- **Fluids** (`Fluid` → `FlowingFluid` → `WaterFluid`/`LavaFluid`) follow the same pattern, with `FluidState` as the value type, a `Fluids` table and `fluid.behavior()`.

## Block entities

Java: `BlockEntity` (abstract) → about 54 subclasses. `BlockEntityType` holds a factory and the valid blocks. `EntityBlock.newBlockEntity(pos, state)` constructs; `EntityBlock.getTicker(level, state, type)` returns a `BlockEntityTicker<T>`, in practice a static method (`HopperBlockEntity::pushItemsTick`, `AbstractFurnaceBlockEntity::serverTick`) wrapped by `BaseEntityBlock.createTickerHelper`. `LevelChunk` keeps `pos → BlockEntity` and registers each ticker, wrapped in a `RebindableTickingBlockEntityWrapper`, into `Level.blockEntityTickers`; `Level.tickBlockEntities` walks that list by index and removes dead wrappers (Paper's `removeAll` fix).

- **Values.** A struct per class embeds `BlockEntity`. The value trait `BlockEntityBehavior` (load, save, components, `as_container()`, `as_randomizable_container()`, …) and the structs sit below the live-game crate (D15 split), because `ProtoChunk` holds block entities (Java `ChunkAccess.blockEntities`) and worldgen uses them (`RandomizableContainer.setBlockEntityLootTable`; `MonsterRoomFeature` configuring its `SpawnerBlockEntity`).
- **Arena.** The server-wide `SlotMap<BlockEntityKey, Box<dyn BlockEntityBehavior>>` (§Ownership tree); `LevelChunk` maps `BlockPos → BlockEntityKey` and keeps the Java `HashMap` save order the chunk layer already reproduces. Promoting a `ProtoChunk` moves its values into the arena.
- **Handle-form virtuals** (the ones that reach the level: `setChanged`, `preRemoveSideEffects`, `setRemoved` overrides) go through `BlockEntityDispatch`, found in the `BlockEntityType`-indexed table.
- **Tickers.** `type BlockEntityTicker = fn(&mut LevelCtx, BlockPos, BlockState, BlockEntityKey)`. Java's static ticker methods are handle-form free functions with that signature; the block's `get_ticker` returns `Option<BlockEntityTicker>` through the `createTickerHelper` port. The level's ticker list holds `(BlockEntityKey, BlockEntityTicker)` wrappers with Java's rebinding semantics: the existing `LevelBlockEntityRuntime`, holding `BlockEntityKey` instead of a `Weak<()>` identity.
- This replaces v1's "block entities live in their chunk and tick by take-tick-putback through a `BlockEntityCtx`": the hopper-and-comparator path above needs the ticking block entity reachable through the level.

## Items

Java: `Item` (concrete) → about 70 subclasses. `Items` registers one instance per item, every field final. `ItemStack` is a mutable object referenced from slots: `ItemStack.use` calls `this.getItem().use(level, player, hand)`, and item code re-reads `player.getItemInHand(hand)` and mutates that stack in place (`itemStack.consume(1, player)`).

- **Instances.** `'static` immutable singletons in one table, built in registration order (generated per #186 or translated), looked up by the extension method `item.behavior() -> &'static dyn ItemBehavior` on the `ItemId` handle (trait `ItemLive`, D15 split). The struct `Item` holds only the fields of Java `Item` that the generated tables do not serve (default components come from the generated table); subclasses embed `item: Item`, and `BlockItem` holds a `Block` handle.
- **Dispatch.** The root value trait `ItemBehavior` carries Java `Item`'s virtual methods, all receiver form: `ctx` where Java passes a `Level`, the player or entity as an `EntityKey`, the stack as a `StackRef`.

**Stacks.** `ItemStack` is a value type in `rivet-world` (count, `ItemId`, component patch); its live methods (`use`, `useOn`, `hurtAndBreak`, `inventoryTick`, `finishUsingItem`, …) are the `ItemStackLive` extension trait (D15 split) and take the stack as a handle:

```rust
pub enum StackRef<'a> {
    /// Java's `SlotAccess` / `Container` + index: a stack inside game state.
    Slot(SlotRef),
    /// A stack outside game state (a fresh result, an event record); disjoint from ctx.
    Local(&'a mut ItemStack),
}
```

`SlotRef` names an owner handle plus a slot (an entity's equipment or inventory slot, a block entity's container slot, a menu slot, an item entity's stack) and is resolved through `ctx` at each access, as Java's code re-reads the shared object. Where Java keeps mutating a stack after its slot was overwritten (the stack is now detached), the port moves the stack out into a `Local` at the point Java detaches it.

## Chunks
Chunks owned by `ChunkMap` by value. Block state = palette index into generated global state table (`rivet-registry`), copy `u32`-ish IDs, no references. Block entities are indexed by the chunk and owned by the server-wide block-entity arena (§Block entities). Chunk gen/lighting runs on `rayon` on detached `ProtoChunk` values, results merged into `ChunkMap` on the tick thread via channel; worker threads never touch an entity or block-entity arena or a `LevelCtx`.

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

## Events and the plugin bridge

Java: Paper's patches fire Bukkit and Paper events at fixed points in NMS code, either through CraftBukkit helpers (about 470 `CraftEventFactory` calls in about 250 files, measured on `0a99345`; the most frequent are `callEntityChangeBlockEvent`, `callBinaryRedstoneChange`, `handleBlockGrowEvent` and `handleBlockSpreadEvent`) or by building the event inline and calling `event.callEvent()`/`PluginManager.callEvent` (about 420 sites). `PaperEventManager.callEvent` dispatches synchronously, on the main thread, to the event's `HandlerList`, and the call site then reads the event back (`isCancelled()`, modified fields). Some helpers carry game logic: `handleBlockGrowEvent` places its `CraftBlockState` snapshot (`snapshot.place(flags)`) when the event is not cancelled. A few sites skip work when no listener is registered (`getHandlerList().getRegisteredListeners().length == 0`, in hoppers and signs).

1. **Call sites are translated in place**, in Java order relative to the surrounding game-state changes.
2. **`CraftEventFactory` helpers** reached from translated code are ported, with their Java names, to `rivet_server::bukkit::craftbukkit::event::craft_event_factory` (the D15 path of `org.bukkit.craftbukkit.event.CraftEventFactory`); the block-state snapshot logic they use (`CraftBlockStates.getBlockState`, `CraftBlockState.place`, including its block-entity copy) goes to `rivet_server::bukkit::craftbukkit::block`. Their game-state logic is ported exactly (D16, bucket E); the Bukkit wrapper objects (`CraftBlock.at`, `getBukkitEntity()`, `asBlockData()`) are replaced by handles. Inline event sites stay inline.
3. **Event records.** Each Bukkit or Paper event class that translated code fires gets a Rust struct with the Java class name, in the live-game crate at the D15 path of its Java class (`org.bukkit.event.block.BlockGrowEvent` → `rivet_server::bukkit::event::block::BlockGrowEvent`; Paper's `io.papermc.paper.event.…` → `rivet_server::papermc::paper::event::…`). It is a port of the event class's state and of every member the call site reads back, with Java's constructor defaults and derived getters, because those carry behaviour even with no listener: `PlayerInteractEvent.isCancelled()` is `useInteractedBlock() == DENY`, its constructor sets `useClickedBlock` to `DENY` when there is no clicked block, and `setCancelled` rewrites both results; `EntityDamageEvent.getFinalDamage()` sums the modifiers. Values hold handles and owned values (`EntityKey`, `(LevelId, BlockPos)`, `BlockState`); a stack that Bukkit exposes as a live mirror of a slot (`CraftItemStack.asCraftMirror`) is a `SlotRef`, not a copy, so a plugin edit reaches the slot as in Paper. `trait Event { const TYPE: EventType; }`, with `EventType` a generated enum, is the marshalling key, not a gameplay seam.
4. **`LevelCtx::call_event(&mut self, event: &mut E)`** is `PaperEventManager.callEvent`. If the bridge reports no listener for `E::TYPE`, it returns: building the Bukkit wrappers has no game-state effect, so skipping them is unobservable. Otherwise `PluginBridge::dispatch` lends the whole `LevelCtx` to the plugin runtime until the last handler returns, and the call site then reads the record back. Paper's explicit no-listener checks become `ctx.bridge.has_listeners(EventType::X)` at the same sites.
5. **Plugins touch game state only inside a lent `ctx`**: during an event dispatch, during the scheduler heartbeat (`MinecraftServer.tickServer` → `mainThreadHeartbeat`), and during plugin lifecycle calls. Every plugin-side operation resolves ids through that `ctx` and may fire nested events, as Bukkit's reentrant `callEvent` allows. No game state sits behind `Arc`, a lock or a queue.
6. **`PluginBridge`** is owned by the tick loop beside `MinecraftServer` (Java's `CraftServer` is adapter code, D16) and lent as `&PluginBridge` in every `ctx`. It holds the listener registry (listener presence per `EventType`, native handlers), the JVM adapter handle and no game state; that is what makes lending it shared next to `&mut MinecraftServer` sound. The registry sits in a tick-thread-only `RefCell` and is borrowed only to copy the handler list before invoking it, as Bukkit's `HandlerList` bakes its array, so a handler may register or unregister listeners mid-dispatch and the change applies from the next `callEvent`, as in Bukkit.
7. **Asynchronous events** (`Event.isAsynchronous()`: `AsyncPlayerPreLoginEvent`, `AsyncChatEvent`, …) are dispatched from the thread Paper uses, with owned data only and no `ctx`; the tick thread consumes their results where Paper does.
8. **Bukkit state that Paper persists in NMS saves** (`PersistentDataContainer` → `PublicBukkitValues`) is a field of the Rust entity or block entity, not adapter state.
9. **Before P3** the bridge has no plugin runtime and `has_listeners` is always false; every hook still runs its game logic.

## JVM adapter boundary

The JVM plugin thread and the tick thread alternate through a rendezvous: the tick thread parks while plugin code runs (D11), so exactly one thread touches game state at any time. Handing the lent `ctx` to the plugin thread for the duration of a rendezvous is the one place a tick-thread borrow crosses threads; it is `unsafe`, confined to `rivet-ffi`, and justified by that park/unpark protocol.

FFI handles for game objects are the arena keys themselves, marshalled as `u64` words (D11): an entity is its `EntityKey` and a block entity its `BlockEntityKey` (`KeyData::as_ffi`: slot and generation, the same `(slot, generation)` shape `rivet-ffi`'s `GenerationalTable` already uses); a world is its `LevelId`; a block is two words, `LevelId` and `BlockPos.asLong()`; a stack is the words of its `SlotRef`. For game objects the arenas are the handle table, and D11's per-call re-resolution is an arena lookup through the lent `ctx`; `rivet-ffi`'s own `GenerationalTable` keeps the handles that are not game objects (runtimes, callback tokens) and, until the live game exists, the entity handles of its ABI tests. Pointers into arenas never cross the boundary.

Bukkit lets a plugin keep an `Entity` and read it after the entity died, because the `CraftEntity` retains the NMS object. The adapter therefore pins (rule 4) every entity that a live `CraftEntity` wrapper references and unpins it when the wrapper is collected (a `java.lang.ref.Cleaner` posting to the tick thread's cross-thread queue), so a dead entity's last state stays readable exactly as long as Paper would keep it.

## Allowed shared-state exceptions
`Arc`: registries/GameData, config snapshots, the connection registry. `'static` immutable: the `Blocks`, `Fluids` and `Items` behaviour tables and the generated dispatch tables. `Mutex`: cross-thread queues only. `Cell`/`RefCell`: tick-thread-only bookkeeping that is not game state (the plugin bridge's listener registry). Anything else needs an OWNERSHIP.md amendment PR first.
