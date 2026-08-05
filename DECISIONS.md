# Rivet Decisions

Binding decisions for the port. Agents: do not relitigate these in PRs; propose changes as issues instead.

## D1 — Goal
Port the Minecraft server **and** Paper's layer on top of it to Rust, faithfully, and run existing Java Paper plugins through an embedded-JVM adapter. Fidelity target: Paper's observable behavior, module structure, algorithms, and constants — not its object graphs (see D4).

## D2 — Version pin
Minecraft/Paper **26.2** (the version in `working/Paper`). No version bumps until M4. The registry/data layer is codegen so future bumps are mechanical.

## D3 — Legal posture
Reference-based port of proprietary vanilla sources (the Pumpkin posture), **private repo** while this is the case. Mojang's MIT libraries (Brigadier, DataFixerUpper) and Paper's GPL/MIT code are ported directly. No Mojang-derived source (`working/`) is ever committed or pushed. Distribution/publication decisions are deferred and human-only.

## D4 — Memory architecture
No garbage collector to lean on, so: **index/arena storage, IDs over references** (see `OWNERSHIP.md`). No full ECS framework — feather died of architecture golf; we keep Paper's structure at module level with arenas underneath.

## D5 — Concurrency model
Single-threaded synchronous game tick owning all world state (matches vanilla's model and Bukkit's main-thread confinement — required for the plugin adapter). `tokio` for network IO at the edges, `rayon` for chunk generation/lighting worker pools; both communicate with the tick thread via channels, never locks on hot game state. Folia-style region threading is explicitly out of scope until after M4.

## D6 — Plugin story
1. **JVM adapter first**: `paper-api` jar unchanged; the implementation layer regenerated as Java shims calling a `rivet-ffi` C ABI (Java FFM/Panama on the Java side — not JNI C glue). Plugin code runs on a dedicated tick-synchronized thread. API-clean plugins only; NMS reflection is out of scope.
2. Native Rust plugin API second, WASM ABI later. Neither before M4.

## D7 — Pumpkin usage policy
`working/Pumpkin` is for **learning and unblocking only**: study its design decisions, consult it when stuck on a parity problem. **Never copy code from it while porting** — our source of truth is Paper's Java. (Rationale: fidelity to Paper is the differentiator; mixing in a second reimplementation's interpretations breaks the "faithful port" verification story.)

## D8 — Verification
The Java server is the oracle; parity is measured, not asserted (`WORKFLOWS.md` → PARITY scoreboard). No agent may weaken a test, fixture, or oracle criterion to go green. `todo!()` requires a `blocked` manifest/issue note.

## D9 — Process
Work is organized as GitHub **epics → agent-created sub-issues → PRs**, milestones M0–M4 (see `WORKFLOWS.md`). Reference docs (`PORTING.md`, `OWNERSHIP.md`) are updated via dedicated PRs, never silently mid-wave. Rust edition 2024, `rustfmt` defaults, clippy clean at wave gates.
