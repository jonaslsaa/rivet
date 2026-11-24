# `rivet-api` — Native Rust Plugin Boundary

`rivet-api` provides Rivet's safe, synchronous, in-process native Rust plugin boundary (`rivet_api::native`). It is intentionally independent of `rivet-ffi`, the JVM Tier 1 adapter, and the `#251` JVM corpus gate.

## Modules

- `native::version` (`#506`): Semantic `ApiVersion` compatibility negotiation, named `Capability` / `CapabilitySet` grants, and explicit `LoadingMode` (`StaticInProcess`).
- `native::handle` (`#506`): Generational `Handle<M>` and `HandleTable<M, T>` with zero-generation rejection and ABA prevention.
- `native::lifecycle` (`#506`): `NativePlugin` trait (`on_load`, `on_enable`, `on_disable`), `NativePluginHost`, `PluginContext`, panic containment, and automatic per-plugin cleanup on disable/unload.
- `native::events` (`#507`): Deterministic `EventPriority` + FIFO `EventBus`, `Cancellable` propagation, `ignore_cancelled`, and per-listener panic isolation.
- `native::commands` (`#507`): `CommandRegistry` with aliases, permission checks, and automatic cleanup on plugin unload.
- `native::scheduler` (`#507`): Synchronous tick-thread `SyncScheduler` for delayed and repeating tasks with deterministic tick order and panic isolation.
- `native::facades` (`#508`): Bounded safe capability facades (`NativeFacadeView`, `SyntheticWorldState`) for world, entity, player, item, inventory, and menu snapshots and atomic/best-effort `TickCommandBatch` mutations.
- `native::smoke_plugin` (`#509`): Source-controlled `SmokePlugin` (lifecycle hooks, an event listener, a command with an alias, and delayed/repeating sync tasks, all through its `PluginContext`) and the Paper-independent `run_native_smoke_acceptance()` non-terminal acceptance runner. `PluginContext` has no facade seam, so the world/entity/player/item/inventory/menu exercise (`SmokePlugin::exercise_world_state`) is host-driven: the runner calls it on the tick thread on the plugin's behalf, using the capability set negotiated for the registered plugin (`NativePluginHost::granted_capabilities`).

## Capability & Version Negotiation

- Plugins declare `PluginDescriptor { target_api, required_capabilities, optional_capabilities, loading_mode }`.
- Pre-1.0 (`major == 0`): `major` and `minor` must match the host exactly and `plugin.patch <= host.patch`.
- Post-1.0 (`major >= 1`): `major` must match and `(plugin.minor, plugin.patch) <= (host.minor, host.patch)`.
- Registration fails closed with `NegotiationError::MissingCapabilities` or `NegotiationError::IncompatibleApiVersion` if any required capability or API version constraint is unsatisfied (also `InvalidPluginName`, `DuplicatePluginName`, and `UnsupportedLoadingMode`). The plugin is granted its required capabilities plus the optional ones the host offers; nothing else.

## Failure Behavior

- **Capability checks**: every `NativeFacadeView` read and write (`world_info`, `get_block`, `get_entity`, `execute`, `execute_batch`, ...) checks the tick thread and the specific capability on each call, failing with `FacadeError::NotOnTickThread` or `FacadeError::CapabilityDenied(cap)`. `PluginContext` event, command, and scheduler registration fails with `LifecycleError::CapabilityNotGranted(cap)` when the capability was not granted.
- **M3 providers are opt-in checks**: `Capability::is_m3_gated()` marks capabilities whose live wiring needs a reviewed M3 provider seam, but facade reads and writes do **not** consult providers. `SyntheticWorldState` is host-owned synthetic state and is not provider-gated. Provider verification happens only through an explicit `require_provider(name)` call (on `SyntheticWorldState` or `NativeFacadeView`), which fails closed with `FacadeError::ProviderUnverified` unless the host registered that provider via `register_verified_provider`. Code that wires live game state must call it before relying on a live seam.
- **Batches**: `BatchMode::Atomic` rolls back every command on the first failure; `BatchMode::ContinueOnError` records per-command failures without aborting later commands.

## Ownership, Safety, and Tick-Thread Rules

- **No raw pointers or references across the boundary**: plugins never receive arena pointers, retained caller buffers, Rust references into live game state, or foreign exceptions.
- **Value snapshots on reads**: world, entity, player, item, inventory, and menu reads return owned value snapshots (`WorldSnapshot`, `EntitySnapshot`, `PlayerSnapshot`, `ItemStackSnapshot`, `InventorySnapshot`, `MenuSnapshot`).
- **Tick-thread enforcement**: `SyntheticWorldState` records its creating thread as the authoritative tick thread. Views are built with `SyntheticWorldState::view(&granted)`, which does no check itself; every view call checks the current thread (`FacadeError::NotOnTickThread` otherwise), as does `require_provider`. All mutations execute synchronously via `execute` / `execute_batch` with typed `TickCommand`s.
- **Generational handles**: every `PluginHandle`, `ResourceHandle`, `EventListenerHandle`, `CommandHandle`, `TaskHandle`, `EntityHandle`, `PlayerHandle`, `InventoryHandle`, and `MenuHandle` embeds a non-zero generation counter. Accessing a despawned, unloaded, or freed handle returns `HandleError::Stale { .. }` (wrapped as `FacadeError::EntityStale` / `PlayerStale` / `InventoryStale` / `MenuStale` on facades).
- **Panic containment & cleanup**: panics inside plugin lifecycle hooks, event listeners, command executors, or scheduled tasks are caught at the host boundary and never corrupt host state. Disabling/unloading a plugin automatically frees its resources, unregisters its event listeners and commands, and cancels its scheduled tasks.

## Unsupported Surfaces (Intentional Non-Goals)

- Dynamic OS library loading (`dlopen` / `cdylib` Rust plugins) is not supported; native Rust plugins use `LoadingMode::StaticInProcess`.
- Async / off-thread mutation of game state is forbidden; background work must send typed messages back to the tick thread.
- Java Paper / Bukkit / Spigot plugin execution is handled separately by `rivet-ffi` and the JVM Tier 1 adapter (`#251`), not by `rivet-api::native`.

## Non-Terminal Acceptance Status

Passing `cargo test -p rivet-api --test native_smoke_acceptance` proves the **Paper-independent native Rust plugin acceptance gate (`#509`)**. It is explicitly **non-terminal** for M4 and is reported separately from `#251` (the M4 JVM Tier 1 Paper plugin corpus gate). Missing runtime/oracle prerequisites are reported fail-closed as `UNVERIFIED` by `scripts/gate.sh`.
