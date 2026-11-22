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
- `native::smoke_plugin` (`#509`): Source-controlled `SmokePlugin` and Paper-independent `run_native_smoke_acceptance()` non-terminal acceptance runner.

## Capability & Version Negotiation

- Plugins declare `PluginDescriptor { target_api, required_capabilities, optional_capabilities, loading_mode }`.
- Pre-1.0 (`major == 0`): `major` and `minor` must match the host exactly and `plugin.patch <= host.patch`.
- Post-1.0 (`major >= 1`): `major` must match and `(plugin.minor, plugin.patch) <= (host.minor, host.patch)`.
- Registration fails closed (`NegotiationError::MissingRequiredCapabilities` or `IncompatibleApiVersion`) if any required capability or API version constraint is unsatisfied.
- Capabilities marked `Capability::is_m3_gated()` require reviewed M3 providers; if a provider is not wired or is marked unverified, facade calls fail closed with `FacadeError::ProviderUnverified` rather than silently succeeding or skipping.

## Ownership, Safety, and Tick-Thread Rules

- **No raw pointers or references across the boundary**: plugins never receive arena pointers, retained caller buffers, Rust references into live game state, or foreign exceptions.
- **Value snapshots on reads**: world, entity, player, item, inventory, and menu reads return owned value snapshots (`WorldSnapshot`, `EntitySnapshot`, `PlayerSnapshot`, `ItemStackSnapshot`, `InventorySnapshot`, `MenuSnapshot`).
- **Tick-thread enforcement**: `NativeFacadeView::new` checks that the caller is on the authoritative tick thread (`FacadeError::NotOnTickThread` otherwise). All mutations execute synchronously via `TickCommandBatch` (`BatchMode::Atomic` rolls back cleanly on any command failure).
- **Generational handles**: every `PluginHandle`, `ResourceHandle`, `EventListenerHandle`, `CommandHandle`, `TaskHandle`, `EntityHandle`, `PlayerHandle`, `InventoryHandle`, and `MenuHandle` embeds a non-zero generation counter. Accessing a despawned, unloaded, or freed handle returns `HandleError::StaleGeneration` / `FacadeError::*Stale`.
- **Panic containment & cleanup**: panics inside plugin lifecycle hooks, event listeners, command executors, or scheduled tasks are caught at the host boundary and never corrupt host state. Disabling/unloading a plugin automatically frees its resources, unregisters its event listeners and commands, and cancels its scheduled tasks.

## Unsupported Surfaces (Intentional Non-Goals)

- Dynamic OS library loading (`dlopen` / `cdylib` Rust plugins) is not supported; native Rust plugins use `LoadingMode::StaticInProcess`.
- Async / off-thread mutation of game state is forbidden; background work must send typed messages back to the tick thread.
- Java Paper / Bukkit / Spigot plugin execution is handled separately by `rivet-ffi` and the JVM Tier 1 adapter (`#251`), not by `rivet-api::native`.

## Non-Terminal Acceptance Status

Passing `cargo test -p rivet-api --test native_smoke_acceptance` proves the **Paper-independent native Rust plugin acceptance gate (`#509`)**. It is explicitly **non-terminal** for M4 and is reported separately from `#251` (the M4 JVM Tier 1 Paper plugin corpus gate). Missing runtime/oracle prerequisites are reported fail-closed as `UNVERIFIED` by `scripts/gate.sh`.
