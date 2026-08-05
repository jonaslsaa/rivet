# Rivet: Porting Paper to Rust — Research & Plan

Research date: 2026-08-05. Source tree analyzed: `working/Paper` (Minecraft 26.2, apiVersion 26.2).

## 1. What Paper actually is (measured)

| Source root | Files | LOC | What it is | License |
|---|---|---|---|---|
| `paper-server/src/minecraft` | 5,324 | ~630k | Decompiled, remapped, patched **Mojang vanilla server** | Proprietary (Mojang EULA) |
| `paper-api` | 1,844 | ~192k | Bukkit/Paper plugin API | GPL-3 / MIT |
| `paper-server/src/main` | 1,154 | ~124k | CraftBukkit/Paper implementation glue (Craft* classes, event wiring) | GPL-3 |
| `paper-server/src/generated` | 190 | ~10k | Generated registry/data holders | — |
| **Total** | **~8,500** | **~956k** | | |

Plus 973 patch files under `paper-server/patches` (Paper's per-file diffs against vanilla sources — this is Paper's actual behavioral value-add over vanilla).

**Key fact: 66% of the code is Mojang's decompiled server, not Paper's.** "Porting Paper" is really three distinct ports: (1) the vanilla Minecraft server, (2) Paper/CraftBukkit's implementation layer on top of it, (3) the Paper plugin API.

## 2. How Bun did it (bun.com/blog/bun-in-rust)

Bun ported ~1,448 Zig files (+1M lines of Rust) in 11 days for ~$165k of API cost, using ~64 concurrent Claude instances via Claude Code dynamic workflows. The transferable methodology:

1. **Faithful mechanical port, idiomatic refactor later.** Preserve structure so existing tests verify behavior.
2. **Serialize shared knowledge before fan-out.** 3 hours writing `PORTING.md` (Zig→Rust pattern map) and `LIFETIMES.tsv` (per-struct lifetime analysis) so parallel agents stay aligned without synchronizing.
3. **Adversarial review with split contexts.** Per unit: 1 implementer (sees original code + guides), 2 reviewers (see *only the diff*, instructed to find bugs), 1 fixer applies feedback. Diff-only reviewers caught bugs the implementer's context biased it away from (e.g. `debug_assert!` semantics, `unwrap_or` eagerness).
4. **Trial run on 3 files first**; fix the *process* (workflow instructions), not the output, when agents misbehave — e.g. explicitly forbidding `git stash`/`git reset`.
5. **Worktree isolation**: 4 worktrees × 16 agents, parallel `cargo check`.
6. **Compiler errors as a work queue**, burned down crate-by-crate.
7. **Merge gate: 100% of the existing 60k language-independent tests pass on all 6 platforms. No test deletion allowed.**

Result: 19 regressions across +1M lines, mostly cross-language semantic differences.

## 3. Why Paper ≠ Bun — the five hard differences

### 3.1 Legal workarounds

Prior-art postures:
- **Pumpkin** (pumpkinmc.org): reimplements vanilla behavior in Rust, *referencing* decompiled code for parity.
- Mojang's **Brigadier** (commands) and **DataFixerUpper** (Codec/serialization framework) are MIT on GitHub — legally portable, and both are load-bearing throughout the codebase.

We don't want these workarounds, we'll directly look at the minecraft server code to make this. (this is fine as this will be just be the reference base so we can optimize from there, will not be published online.)

### 3.2 Semantic gap: GC → ownership
Zig→Rust is manual-memory→manual-memory; a mechanical port works. Java→Rust crosses a garbage-collection boundary:
- The core object graph is **cyclic**: `Entity ↔ Level ↔ Chunk ↔ BlockEntity`, everything holds back-references.
- Deep inheritance (the `Entity` hierarchy alone is hundreds of classes), reflection, dynamic dispatch, `synchronized`, statics.
- A Bun-style file-by-file mechanical translation produces `Rc<RefCell<>>` soup or code that never compiles.

Fix: decide the **memory architecture per subsystem up front** and encode it in the porting guide. The proven shape for this domain is index/arena-based storage (slotmap/generational indices, ECS-lite — azalea uses Bevy ECS; Pumpkin uses its own structures). Fidelity to Paper should be at the level of **module structure, names, algorithms, and constants** — not object graphs. This replaces Bun's `LIFETIMES.tsv` with an `OWNERSHIP.md` mapping every major Java class cluster to its Rust storage strategy.

### 3.3 No test suite → build the oracle first
Bun's superpower was 60k+ language-independent tests as the merge gate. Paper has comparatively minimal tests, and they're JVM-coupled. **The verification harness must be built before porting begins**, using vanilla/Paper as a reference oracle:
- **World-gen differential testing**: same seed → chunk-by-chunk block hashes must match vanilla. (Deterministic, brutal, wonderful.)
- **Protocol conformance**: packet round-trip tests against recorded vanilla sessions; drive the server with [azalea](https://github.com/azalea-rs/azalea) bots (Rust, supports 26.2).
- **Behavioral tests**: GameTest-style in-world scenarios (redstone, mob AI, item mechanics) run against both servers, outputs compared.
- **Real-client smoke tests** at milestones.

### 3.4 The plugin ecosystem is the point of Paper — and it doesn't port
Paper's value over vanilla is Bukkit/Paper plugins: JVM jars. A Rust server cannot load them. Options:
1. **Native Rust plugin API mirroring paper-api's shape** (events, scheduler, registries) — port the API design, not binary compat. Recommended start.
2. **WASM plugin ABI** — language-agnostic, sandboxed; a plausible long-term differentiator.
3. Embedded JVM (JNI/GraalVM) for real plugin compat — heroic, entangles you with the JVM you left; not worth it initially.

### 3.5 Moving target
Minecraft ships several updates per year and Paper tracks them. **Pin to 26.2** for the entire port; design the registry/data layer as codegen from Mojang's official data generators (`src/generated` shows what's extractable) so version bumps are mechanical later.

## 4. Prior art (be honest about this)

- **[Pumpkin](https://github.com/Pumpkin-MC/Pumpkin)** — most active Rust server; vanilla parity goal, Java+Bedrock protocol, world loading/chunks/lighting working; early but real. If the goal is merely "a Rust Minecraft server exists," contributing there beats starting over.
- **[FerrumC](https://github.com/ferrumc-rs/ferrumc)** — another Rust reimplementation, smaller.
- **Valence** — server *framework* (bring-your-own game logic), not vanilla-parity.
- **feather** — dead; cautionary tale (ECS purity over shipping).
- **[azalea](https://github.com/azalea-rs/azalea)** — Rust *client* library, current with 26.2. Reuse for the test harness regardless of everything else; its protocol crates may be reusable server-side.
- Reusable crates: `simdnbt`/`fastnbt`/`valence_nbt` (NBT), `azalea-protocol` (packets), `serde` (much of what Codec does), `tokio`, `rayon`.

Rivet's differentiation vs Pumpkin: **Paper-fidelity** — port Paper's API surface and its 973 patches of behavioral fixes, not just vanilla parity.

## 5. Proposed plan

### Phase 0 — Decisions & serialized docs (human + agent, sequential)
Deliverables, in repo, before any fan-out:
- `DECISIONS.md`: legal posture, version pin (26.2), plugin story, async model (tokio for IO + sync tick loop; consider Folia-style region threading later, not now).
- `PORTING.md`: Java→Rust pattern map (inheritance→trait+enum strategies, statics, `synchronized`, ticking, events, registries, Codec→serde-or-ported-DFU, NBT, error handling, `null`→`Option` conventions).
- `OWNERSHIP.md`: the Entity/Level/Chunk/BlockEntity graph → arena/index storage design per subsystem. This is the doc that makes or breaks the port.
- `CRATES.md`: workspace DAG mirroring Paper's module structure bottom-up: `rivet-nbt`, `rivet-serialization` (DFU/Codec), `rivet-brigadier`, `rivet-protocol`, `rivet-registry` (codegen), `rivet-world`, `rivet-entity`, `rivet-server`, `rivet-api` (paper-api port).

### Phase 1 — Oracle harness (before porting)
Vanilla-server runner + azalea bot driver + world-gen hash differ + packet snapshot recorder/replayer. Define per-milestone green criteria. This is the merge gate for everything after.

### Phase 2 — Leaf crates, bottom-up
NBT → Brigadier + DFU (MIT, safe to port directly) → protocol → registry codegen. Each unit runs the Bun loop: 1 implementer / 2 diff-only adversarial reviewers / 1 fixer, verified by round-trip tests against recorded vanilla data. **Trial run on 3 files first**, tune the workflow, then scale.

### Phase 3 — World, then entities, then game logic
Chunk storage/lighting/worldgen (gated on seed-hash parity) → entity system on the OWNERSHIP.md architecture → block/item/mob behavior. Worktree-isolated agent pools, `cargo check` errors as the work queue, crate-by-crate burn-down.

### Phase 4 — The Paper layer
Port `paper-server/src/main` semantics + the 973 patches as a behavior layer; expose `rivet-api` mirroring paper-api's shape in idiomatic Rust; config (paper-world-defaults etc.).

### Milestones (each ends with a real client connecting)
- **M0**: harness green against vanilla itself (sanity).
- **M1**: client joins an empty superflat, sees chunks, walks around.
- **M2**: world-gen hash parity on N seeds; persistence round-trips vanilla worlds.
- **M3**: survival loop — mobs, combat, inventory, crafting, redstone basics.
- **M4**: Paper API + patch-set behavior parity; plugin story demo.

### Agent-ops rules (adapted from Bun)
- Serialized reference docs are the coordination mechanism; agents read, never edit them mid-run.
- Reviewers see diffs only, never the implementer's context.
- Forbid `git stash`/`git reset`/force-push in workflow instructions from day one.
- Fix the workflow prompt when agents misbehave, don't hand-patch output.
- No weakening the oracle to make it pass; merge gate = oracle green.
- Worktree isolation per agent pool; bounded parallel `cargo check`.
