# Rivet roadmap and execution plan

Written 2026-11-23 against `main` at `4fc14eeb`, before the open PRs #766–#775 are merged.
Audience: the controller session. Every number marked **(measured)** was computed on this host
today; everything marked **(estimate)** is a planning guess to be recalibrated after Batch 3.

Contents: 1 Assessment · 2 Scope and roadmap · 3 Execution model · 4 First three batches ·
5 Doc, issue and workflow hygiene · 6 Risks and decision rules · Appendix A hub list ·
Appendix B how the numbers were produced.

## 0. Controller reconciliation with `VERIFICATION.md` (binding over the sections below)

This roadmap and `docs/VERIFICATION.md` were written independently. Where they differ:

1. **Two ledgers, one tool.** `COVERAGE.tsv` (Java class → Rust home, method coverage, markers;
   this roadmap's ledger) answers "how much is ported". `VERIFY.tsv` (verification rows, rust globs,
   status, counts; VERIFICATION §3.1) answers "how much is checked against Paper". Both are written
   by tools, never by hand, and `PARITY.md` is rendered from `VERIFY.tsv`.
2. **Spine vs. debt pause.** The P1 spine proceeds despite the debt thresholds, because no gameplay
   verification (arena traces, bot rows) can run without it. But every spine cluster PR must ship
   with, or after, a Java-sourced row for its surface (VERIFICATION §3.3, test-first preferred), and
   the P2 leaf fan-out in any family is gated by that family's `debt_ratio` (VERIFICATION §4.2).
3. **Tickdiff vs. arena traces.** The gameplay oracle is VERIFICATION's L4 arena tick-trace harness
   (structure + seeded ticks, no console commands needed), started in Batch 1–2 against surfaces that
   already exist. The command-script variant (§2.4 / Batch 3D) becomes a later L4 input format, not a
   separate harness; the minimal command set stays in Batch 3 as product work.
4. **Gate tiers.** VERIFICATION §3.2 (T0 scoped / T1 merge with path-triggered oracle rows / T2 full
   per ≤5 merges / T3 nightly) supersedes §3.5 below. Recorded as one decision that amends D10.
5. **Batch 1 lanes, in launch order:** 1C decisions+docs (no cargo) → 1A ledgers, debt script,
   `gate.sh --fast` + timings, workflow-script fixes (§5.3) → 1B canonical `world.phys` → generic
   oracle-op framework (VERIFICATION #3) → probe regen check (VERIFICATION #2) → DataConverter
   calibration wave (1D) → protocol residual (1E). At most 3 lanes plus the gate lane.
6. **Plugin threading (D20, 2026-11-24).** P3's adapter runs plugin code on the tick thread through
   FFM upcalls, with no separate plugin thread; it amends D6 and D11, which this plan cites.

---

## 1. Where the port stands

### 1.1 The numbers

| Fact | Value | Source |
|---|---|---|
| Java source of truth | 7,876 classes, 944k LOC (minecraft 629k, paper-server 124k, paper-api 191k) | measured |
| Manifest status | 842 units; 122 `done` (38k LOC, 4%); 720 `pending` (908k LOC) | measured |
| Rust on main, non-generated | 544k LOC: about 283k production (plus a 65.5k-line sin table), 196k tests | measured |
| Rust on main, generated | 239k LOC (registry 140k, nbt fixtures 82k, server 15k) | measured |
| Java classes with a same-named Rust file | 784 of 7,876 (10%); 144.5k of 944k LOC (15%); minecraft root 120k of 629k (19%) | measured, upper bound: many are partial slices |
| Tests | 6,509 tests in 72 binaries | measured |
| Deferral markers | 255 `RivetTodo`, 51 `STUB`, 1 `todo!()`; 79 of the 255 `RivetTodo` (31%) point at issues that are no longer open | measured |
| Git sprawl | 16 worktrees, 50 local branches, 86 remote branches (8 are ancestors of main; squash-merged ones are not detected) | measured |

The manifest says 4% done; file-name matching says at most 15–19%. Neither is trustworthy,
which is itself the first finding: **there is no reliable progress measure today.**

### 1.2 What works end to end

Per the gate rows and the code (I did not run the oracle gate; see 1.5):

- Offline login, configuration, play join, keepalive survival, authoritative movement, kick.
- Loaded world: read-only Anvil load, official-client walking, chunk-view recentering.
- Generated world: `rivet-server --seed N` boots, generates and serves chunks; world time,
  block events and neighbour updates, fluids, incremental lighting, some generated creatures.
- Foundation crates with real Paper parity evidence: nbt, serialization, brigadier, text,
  util (RNG, Mth), registry codegen, protocol framing and join packets, chunk storage,
  most of overworld levelgen, Starlight.

What does not exist: digging, placing, inventories, visible entities, mobs that tick, combat,
commands, any plugin. A player can join and walk; nothing else is a game yet.

### 1.3 The real critical path

It is the **live-game spine**, and it is missing:

- No `LivingEntity`, `Mob`, `Player`, `Item` or `BlockBehaviour` type exists on main. The only
  `Entity` struct is `rivet-world/src/level/border/entity_stub.rs`.
- `rivet-server` does not depend on `rivet-entity` and never references it (0 files).
  `rivet-entity` does not depend on `rivet-world`, so entity code cannot see `BlockState` or `Level`.
- `rivet-api` and `rivet-ffi` depend on no Rivet crate at all.
- Eight gameplay modules in `rivet-world` (`loot`, `enchantment`, `crafting`, `trading`, `scores`,
  `effect`, `inventory`, `item`) have zero references from any other crate and zero references
  to each other.
- Stand-in types fill the gaps: 4 definitions of `Vec3`, 4 of `BlockPos`, 4 of `Aabb`, 2 of
  `EntityId`, 2 of `ItemStack` (plus a third `LootItemStack`).

The class-level dependency graph explains why this matters **(measured, Appendix B)**:

- One strongly connected component holds 4,303 classes and 598k LOC. Cutting at class level
  instead of package level does not dissolve it.
- Removing the 200 highest in-degree classes (120k LOC, Appendix A) collapses it: the largest
  remaining component is 42 classes, with 173 small clusters.
- So the port has exactly one hard part: about 200 hub classes. Everything else is leaf work
  that becomes mechanical once those hubs exist as real, complete, canonical Rust types.
- 69 of the 200 have a same-named Rust file today (44k LOC of Java), mostly the value and
  worldgen hubs. The 131 missing ones are the game: `LivingEntity` 4,915 LOC, `ServerPlayer`
  3,141, `MinecraftServer` 3,188, `Mob` 1,707, `BlockBehaviour` 1,455, `PlayerList` 1,303, `Items`,
  `EntityType`, `Block`, `VoxelShape`, `AABB`, `Brain`, `BlockEntity`, and so on.

### 1.4 Where effort was misallocated

1. **Leaves before spine.** On 2026-11-21/22, 32 PRs added about 87k lines with almost no
   deletions (10 more, about 34k lines, are open). Each is a "foundation" for one issue, built
   against private stand-ins because the hubs did not exist. They are shallow and off-pattern:
   - PR #767 ports `Entity.java` (5,462 LOC) as a 678-line `lifecycle/entity.rs`.
   - `world/level/storage/loot` is 129 Java files; Rust has 13 files at `rivet-world/src/loot`
     (wrong module path) with a self-contained `LootItemStack`.
   - `world/scores` is 1,473 Java LOC and became 6,052 Rust LOC; `world/effect` 1,451 became 4,705.
     Small packages got heavy invented unit tests while `Entity` sits at about 12%.
2. **Native plugin API before a game.** `rivet-api` has 6.3k LOC of facades (#506–#508, #762,
   open #775) over nothing.
3. **Process weight on the M2 terminal gate.** `gate.sh` is 1,363 lines with about 4,300 lines of
   bash tests for the gate itself. Epic bodies (#16, #175, #339) are 90-plus lines of layered
   "status refresh" and "review correction" text. M2 opened in August and is still open.
4. **The manifest was abandoned as the tracker** when work moved to issue-scoped PRs. 87
   `needs_split` units (516k LOC) were never split; #65 has been open since August.
5. **Merging without the gate.** #763 exists only to fix fmt and clippy on main, and
   `cargo check --workspace --locked` fails on `4fc14eeb` because `Cargo.lock` is missing
   `rivet-entity`'s new dependencies (`rivet-core`, `rivet-registry`, `thiserror`) **(measured)**.

### 1.5 What I did not verify

- I did not run `scripts/gate.sh`. The oracle rows, the M2 terminal state after the October
  PRs, and full-gate wall time are unknown. `PARITY.md` is dated 2026-08-09.
- Test-run wall time is unknown: my timing run was cut off (see 3.3).
- The same-name coverage figure counts a partial file as covered. A method-level ledger
  (Batch 1A) replaces it.
- The handoff archives named in issue #657 (`/tmp/rivet-session-handoff-20260816/…`) no longer
  exist: `/tmp` is a tmpfs and the host has rebooted.

---

## 2. Scope and roadmap

### 2.1 What "all of Paper ported" means

The 908k "pending" figure mixes four different kinds of work. Fix the denominator first.

| Bucket | Java LOC | Treatment |
|---|---|---|
| A. Minecraft runtime (`net.minecraft` minus B and C, plus DataConverter 31.6k and Moonrise 22.6k) | about 558k | Translate to Rust, file for file. |
| B. Datagen (`net.minecraft.data.*`) | 36k | Do not translate. The generated JSON is already in `working/Paper/.../resources/data/minecraft`; `rivet-codegen` extracts it. Port only classes a runtime call site needs (keys such as `BuiltInLootTables`, `PlacementUtils`). |
| C. DFU datafixers (`net.minecraft.util.datafix`) | 35k | Paper routes world data through DataConverter (12 `MCDataConverter.convertTag` call sites versus 7 `DataFixTypes` update sites). Port DataConverter; port a DFU fix only when an audited live call site needs it. |
| D. Paper server layer (`io.papermc.paper`, `com.destroystokyo`, Moonrise in `src/main`) | 48k | Translate the behaviour (config, patches, chunk system, event call sites). |
| E. CraftBukkit (`org.bukkit.craftbukkit`) | 76k | Not a translation. These are Java wrappers that implement `org.bukkit` interfaces over NMS handles. Under D6 they become generated Java shims over `rivet-ffi`. The Rust side is a facade plus event hooks. |
| F. `paper-api` | 191k | Unchanged jar (D6). Zero translation. |

**Translation target: about 606k Java LOC (A + D), of which at most about 121k has any Rust
today. Adapter target: the E + F surface, reached by shim generation.**

Record this as a decision (D16 below) and re-label the manifest: B and C units become
`n/a(datagen)` and `deferred(dfu-audit)`; E and F units move to an `adapter` track that the
wave picker never selects.

### 2.2 Phases

Sizes are Java LOC from the manifest's pending rows, so they are upper bounds. "Ledger" is
the file-and-method coverage tool built in Batch 1A. "Tickdiff" is the command-script
differential oracle built in Batches 2–3 (section 2.4).

| Phase | Content | Size | Exit criterion (all checkable) |
|---|---|---|---|
| **P0 Truth** | Merge open PRs; lockfile; baseline gate; ledger; crate topology; prune | none | `cargo check --workspace --locked` exits 0. One full `gate.sh --require-oracle` run on main recorded with per-step timings, every non-green row filed as an issue. `COVERAGE.tsv` committed and checked by the gate. D15–D18 merged. At most 3 worktrees and 15 remote branches outside active lanes. |
| **P1 Spine** | The hub classes of Appendix A, complete, canonical, wired into the live tick | 187 classes, 102k LOC (excludes 12 CraftBukkit hubs and `DataFixers`) | Ledger: every spine class at 100% method coverage (a method is covered by a Rust fn or an explicit `n/a` note). Single-definition check passes with an empty allow-list. `rivet-entity` no longer exists as an island. All existing scenario rows still green. New Paper-vs-Rivet rows green: dig and place (#55), a summoned pig visible to the client and walking, and at least 10 tickdiff scripts. |
| **P2 Fan-out** | Leaf families, in the order below | about 430k LOC remaining (estimate: 606k target, minus the spine, minus leaf code that already has Rust) | Per family: ledger shows every file complete and zero `STUB(<family unit>)`; at least one tickdiff script per 20 classes, all passing Paper-vs-Paper and Paper-vs-Rivet; ported Paper JUnit tests green where they exist. |
| **P3 Adapter** | `rivet-ffi`, shim generation, Java compatibility kernel, corpus | E + F surface | Tier-1 corpus plugins boot and pass their smoke scripts on Rivet with the same transcript as on Paper (#251). |
| **P4 Terminal parity** | Worldgen seed corpus, soak, marker burn-down | none | #175 Stage B: 8,192 chunks byte-identical. Three bots for 30 minutes on Paper and Rivet with no transcript divergence. `todo!()` and `STUB` counts are 0; every `RivetTodo` points at an open issue. |

P2 family order. The rule is "what makes the running server more of a game next", then
"what has an oracle". Rows are manifest pending LOC and still include each family's own hub
classes (except F3), so they sum to more than the P2 figure above:

| # | Family | Manifest groups | Pending LOC |
|---|---|---|---|
| F1 | Blocks, block entities, fluids, ticks, redstone | `mc.world.level.block*`, `block.entity` | 67k |
| F2 | Items, inventory, crafting, loot, enchantments, trading | `mc.world.item*`, `mc.world.inventory`, `storage.loot` | 50k |
| F3 | Entities and AI: animals, monsters, projectiles, vehicles, villagers, bosses | `mc.world.entity.*`, `entity.ai` | 78k after hubs |
| F4 | Level services: spawning, raids, portals, game events, POI, saved data, borders | `mc.world.level.*` other, `mc.world` other | 48k |
| F5 | Protocol residual and network | `mc.network.*` | 23k |
| F6 | Commands and arguments | `mc.server.commands`, `mc.commands` | 28k |
| F7 | Server management: players, bans, dedicated server, rcon, jsonrpc, packs, advancements, stats | `mc.server` other, `mc.server.level`, `mc.server.network`, `mc.advancements` | 52k |
| F8 | Worldgen completion and structures; nether and end | `mc.levelgen.*` | 28k |
| F9 | Persistence and upgrades: DataConverter, level storage | `spottedleaf.dataconverter` | 32k |
| F10 | Paper layer: Moonrise chunk system, Paper config, event call sites | `spottedleaf.moonrise`, `io.papermc.paper` server | 69k |
| F11 | Residual util, core, misc, GameTest framework | `mc.util` other, `mc.core*`, `mc other`, `mc.gametest` | 51k |

F5, F9 and the below-the-cycle parts of F11 do not depend on the spine and can run from
Batch 1. P3 starts with a single lane once P1 exits (events and players exist), with one
goal first: a trivial plugin boots and receives `PlayerJoinEvent`.

**Calendar (estimate):** P0 2–3 days; P1 about 2 weeks; P2 4–6 weeks; P3 overlapping, 3–4
weeks; P4 2–3 weeks. Roughly 10–14 weeks of continuous controller operation. Replace this
with measured LOC per day after Batch 3.

### 2.3 Cutting the giant cycle into schedulable work

The manifest's `wave`/`cycle` columns cannot schedule this (567 pending units share cycle 27).
Use the hub cut instead:

1. **Spine units** are the Appendix A classes, grouped into about 25 clusters of up to 6k LOC
   (section 4). They are translated in full, at the mirrored path, in the crate D15 assigns.
   Calls into leaves that do not exist yet go to minimal declarations marked
   `// STUB(<owning unit id>)`; `check_markers.py` already polices those.
2. **Leaf units** are everything else, regenerated at class-cluster level: at most 15 files and
   about 2.5k LOC each, never splitting one of the 173 small clusters. This finishes #65.
3. After the cut the leaf graph is a DAG but a deep one (maximum depth 49; 300k of 478k LOC
   sits at depth 12 or deeper). Strict "dependencies done first" would serialise it. So each
   family starts with a **scaffold PR**: every file, `mod` line, struct and function signature
   for the whole family, bodies stubbed. After that, leaf units own disjoint files, compile
   against real signatures, and can run in any order.
4. A leaf unit is ready when its hub dependencies are complete in the ledger and its family
   scaffold is merged. Nothing else gates it.

### 2.4 The oracle that has to exist: tickdiff

About 400k LOC of behaviour has no executable oracle today. Paper bundles almost no
GameTest content (#483 notes this), so the GameTest runner is not a free suite. Build one
generic differential instead of one harness per issue:

- A scenario is a command script fed to the server console: `/tick freeze`, `/setblock`,
  `/fill`, `/summon`, `/give`, `/tick step N`, `/data get`, `/save-all flush`.
- Run it on Paper and on Rivet from the same seed and world, then diff the saved region,
  entity and POI NBT with the existing chunk-NBT comparator.
- A script counts only after it passes Paper-vs-Paper twice.

This needs real product features on the Rivet side (console input, `Commands`,
`CommandSourceStack`, about ten commands, entity and chunk save), which is why a minimal
command set is in Batch 3 rather than in F6. Azalea client scenarios stay for what only a
client can observe (packets, visibility, interaction).

---

## 3. Execution model

### 3.1 Unit of work: both, with a hard rule for which

| Work | Vehicle | Why |
|---|---|---|
| Spine clusters, crate moves, reconciling the November islands, harness and codegen | `worktree-task` (issue-scoped) | Cross-cutting, needs design judgement, touches shared files. |
| Leaf units after a scaffold; below-the-cycle units (DataConverter, protocol bodies, util) | `translate-wave` (manifest units) | Mechanical, disjoint files, and the wave prompt enforces file-for-file mirroring, which issue-scoped PRs did not. |
| Family scaffolds | new `scaffold` workflow (5.3) | Serial, one agent plus one reviewer. |

Never issue-scope leaf work again: that is what produced partial slices with stand-ins.
Every PR, of either kind, changes the ledger, and the ledger is the only progress tracker.

### 3.2 Batch sizes and concurrency

- **Lanes:** 3 implementation workflows at once, each in its own worktree and branch, plus one
  controller-owned gate lane that is not a workflow. Go to 4 when the 15-minute load average
  stays under 8 and the merge queue is under an hour deep. Drop to 2 if available memory falls
  under 4 GB or the train gate goes red twice in a day.
- **Workflow size (under 10 agents):**
  - `translate-wave`, lean rigor: 3 units, each implementer, one combined reviewer, one fixer,
    at most 2 rounds.
  - `translate-wave`, full rigor: 2 units, two-lens first round, at most 3 rounds.
  - `worktree-task`: `maxRounds: 3`.
  - Ask the owner to raise the guideline to about 16. The build cost supports it (3.3) and it
    doubles units per wave.
- **Rigor tier:** full for anything that carries RNG, arithmetic, tick order or physics
  (worldgen, AI, movement, loot, spawning, redstone). Lean for declarative code (packets,
  component types, converters, block property declarations, command plumbing).
- **Unit size:** at most 15 files and about 2.5k Java LOC. A class over 2,000 LOC is its own
  unit, split into a directory module (5.2).

### 3.3 What the host can carry

Measured on a clean export of `4fc14eeb`, cold, while the controller ran 2–3 other cargo jobs:

| Step | Wall time | Notes |
|---|---|---|
| `cargo check --workspace` | 45 s | 3.3 cores average, 0.9 GB peak RSS, 0.7 GB target |
| `cargo clippy --workspace --all-targets -Dwarnings` | 77 s | clean on main |
| `cargo nextest run --workspace --no-run` | 157 s | target grows to **9.4 GB** |
| test run | not measured | see below |

Consequences:

- **Checks are cheap; agents may run `cargo check -p <crate>` freely** without the build lock.
  Set `CARGO_BUILD_JOBS=4` in lane prompts so three lanes do not thrash 12 cores.
- **Test builds are the expensive step.** Agents run filtered tests only
  (`cargo nextest run -p <crate> -E 'test(<module>)'`). Full test builds happen in the gate
  lane, one at a time, under `scripts/with-build-lock.sh`.
- **Never build under `/tmp`.** It is a 9.6 GB RAM-backed tmpfs. My timing run put a target
  directory there, filled it at 9.4 GB and broke the test run; I deleted that directory and
  the volume is back to 2% used. Any controller command that failed with "no space left on
  device" between about 22:50 and 22:55 CET on 2026-11-23 should be retried. Keep targets and worktrees under
  `/home` (772 GB free).
- Budget 10 GB per worktree that has built tests. Prune a worktree as soon as its PR merges.

### 3.4 PR sizing and merge cadence

- One PR per lane run: one spine cluster, or one wave of 2–6 leaf units from one family.
  Roughly 3–10k Java LOC. Never mix spine and leaf work, and never mix families.
- Scaffold PRs and `mod.rs`/`Cargo.toml`/generated-dispatch changes merge alone and first, so
  leaf PRs never conflict on shared files.
- No rebasing of pushed branches (it needs a force-push). Bring a lane up to date by merging
  `origin/main` into it; squash-merge at the end. `release-prep` rebases, so use it only on
  branches that have not been pushed, or change it to merge.
- Push every lane branch after every unit and keep a draft PR open as the durable record.
  Workflow journals do not survive the session; git does.

### 3.5 Running the gate without making it the bottleneck

Three tiers. This amends D10 (see D17).

1. **Fast gate, every PR, before merge.** Workspace fmt, clippy, nextest, marker audit, ledger
   check, single-definition check. No oracle, no scenarios. Run it in one persistent warm
   worktree (`.claude/worktrees/gate`) on the PR's merge ref (`git fetch origin pull/N/merge`),
   so builds are incremental. Expect 2–5 minutes warm **(estimate from the cold figures above)**.
2. **Train gate, on main.** Full `gate.sh --require-oracle` after every 3–6 merges or every 3
   hours, whichever comes first, in the same worktree. If it is red, bisect the train (at
   most three more runs), revert the offender with a revert commit, and re-queue that PR.
3. **Full gate before merge** only for PRs that touch what the oracle rows exercise today:
   `rivet-nbt`, `rivet-protocol`, `rivet-serialization`, chunk storage, levelgen, lighting,
   login and join, the chunk pipeline, and `tools/`. A PR that adds `Cow` cannot change those
   rows; a PR that edits `noise_chunk.rs` can.

Measure the full gate's wall time in P0. If it exceeds 30 minutes, split the oracle steps
into two halves that alternate between trains.

### 3.6 Model and effort tiering

| Role | Model and effort |
|---|---|
| Spine implementer (Entity, LivingEntity, Player, ServerPlayer, Level, ServerLevel, MinecraftServer) | Opus, xhigh |
| Other spine implementers; full-rigor leaf implementers | Opus, high |
| Lean-rigor leaf implementers; fixers | Opus, medium |
| Reviewers (always fresh, never shown implementer reasoning) | Opus, high |
| Scaffold, file moves, import rewrites, fmt and clippy burn-down | Opus low, or Sonnet |
| `review-pr` | max for spine PRs and once per train over the train's merged range; high for leaf PRs |
| Controller | reads structured reports and ledger deltas, not diffs |

### 3.7 When a unit blocks

- **Missing dependency that is a hub:** stop the unit and move that hub to the front of the
  spine queue. Do not stand it in.
- **Missing dependency that is a leaf:** declare a `STUB(<unit>)` and continue.
- **Does not converge in the round limit:** split the unit to single classes and retry once
  with a fresh implementer. On a second failure, mark it `blocked(reason)` in the manifest,
  file one issue, and move on. Never run the same prompt a third time.
- **Parity mismatch not localised after two fix rounds:** write a small Java probe in
  `tools/rivet-reference-oracle` that dumps the intermediate values and bisect. Pumpkin is
  for reading only (D7).
- **Same finding class in two units:** stop, patch `PORTING.md` and the wave prompt in a
  dedicated PR, then resume.

### 3.8 The controller loop

1. Train gate red? Fix or revert before anything else.
2. Any spine class incomplete? Fill up to 3 lanes with spine clusters.
3. Otherwise take the next family in order: scaffold first, then ready leaf units, lowest
   graph depth first.
4. Keep the oracle within reach: no family may be more than two waves ahead of its tickdiff
   coverage. If it is, the next free lane writes scripts, not code.
5. Every 10 merged PRs: read the findings-per-unit trend, wave wall time and ledger delta;
   adjust lanes, rigor and unit size; update the calendar.

---

## 4. The first three batches

Launch these after #766–#775 are merged. Treat those ten PRs as additive islands: merge
what passes the fast gate, and do not spend review effort perfecting them. Reconciliation
happens in the spine tasks below. #767's `level_ctx.rs` is the one that matters: it seeds
the entity spine.

### Batch 1 — truth, canonical types, and two waves that need no spine

| Lane | Vehicle | Work | Why now |
|---|---|---|---|
| **1A** | `worktree-task` | (a) Regenerate `Cargo.lock`; add `--locked` to every cargo call in `gate.sh`. (b) `scripts/coverage_ledger.py` writing `COVERAGE.tsv`: per Java class, the assigned crate, mirrored Rust path, method coverage, `STUB` and `RivetTodo` counts, status; manifest `status` becomes derived from it. (c) Finish #65: class-cluster units inside the cycle, using the Appendix A hub cut. (d) `gate.sh --fast` plus a per-step timing summary. (e) A single-definition check with an allow-list of today's duplicates that may only shrink. | Nothing else can be measured or scheduled without it. No cargo contention. |
| **1B** | `worktree-task` | Canonical value hubs: complete `net.minecraft.world.phys` (`Vec3`, `AABB`, `HitResult`, `BlockHitResult`, `EntityHitResult`, `Vec2`) and `world.phys.shapes` (`VoxelShape`, `Shapes`, `CollisionContext`) in one home. Delete the duplicates in `rivet-util/src/mth_stubs.rs`, `rivet-entity/src/projectile/phys.rs`, `rivet-entity/src/ai/goal/context.rs`, `rivet-entity/src/storage/entity_record.rs` and `rivet-api/src/native/facades/world.rs`. Closes #206. | Every later unit needs these, and the duplicates multiply with each PR. |
| **1C** | `worktree-task`, max effort, docs only | Write and merge D15–D18 and the missing `OWNERSHIP.md` sections: block behaviour dispatch, item behaviour dispatch, block-entity dispatch, the concrete `LevelCtx` API (start from #767), and the event-hook interface for Paper call sites. | The spine cannot start until these are decided once. |
| **1D** | `translate-wave`, lean | DataConverter, first wave: `ca.spottedleaf.dataconverter.minecraft.walkers.{generic,block_name,item_name,itemstack,tile_entity,game_event}`, then `.minecraft.converters.{advancements,attributes,blockname,entity,itemname,leveldat,options,poi,recipe,stats,tileentity}`, `.minecraft.datatypes`, `.minecraft.hooks`. All are ready in `wave_picker.py` today. | Independent of the spine, 31.6k LOC, byte-diffable against Paper. It is the calibration wave for the pipeline, which has not run in months: record agents, wall time, findings and tokens per unit. |
| **1E** | `translate-wave`, lean (start when a lane frees) | Protocol residual: the `net.minecraft.network.protocol.game` packet bodies still missing (Java has 194 files there; `rivet-protocol/src/game` has 140), taken from manifest `mc.network.protocol.game` after the 1A split, then `mc.network.syncher` (#222), #357, #358, #385. | Every spine cluster needs these packets; `rivet-protocol` is disjoint from the other lanes; `rivet-decode` fixtures are the oracle. |

Batch 1 is done when 1A–1C are merged and the baseline train gate has been run once.

### Batch 2 — the crate move, then the first four spine clusters

**2.0 (serial, about an hour, before anything else):** execute D15. Move `rivet-entity` into
the live-game crate at the mirrored module path, fix imports, delete the crate. Fast gate,
merge alone.

| Lane | Cluster (Java classes, LOC) | Reconciles |
|---|---|---|
| **2A Entity core** | `Entity` (5,462), `EntityType` (739), `EntityTypes` (1,205), `EntityReference`, `EntityDimensions`, `Pose`, `SynchedEntityData` with `EntityDataSerializers` (230) | #767 `lifecycle/entity.rs`, `rivet-entity/src/storage`, the open tracker PR #768 |
| **2B Living** | `LivingEntity` (4,915), `world.entity.ai.attributes`, `DamageSource` wiring, `MobEffects` (136) with `MobEffectInstance` | `rivet-entity/src/damage`, `survival`, `rivet-world/src/effect` |
| **2C Block spine** | `BlockBehaviour` (1,455), `Block` (716), live `BlockState` methods, `FluidState` (156), `Fluids`, `BlockEntity` (393), `BlockEntityType`, the `Blocks` registration table | `rivet-world/src/block`, `rivet-server/src/server/level/level_block_*`, `fluid_runtime.rs` |
| **2D Item spine** | `Item` (773), `Items` (2,149, generated: finish the `items-recipes-codegen` worktree, #186), `ItemStack` to completion (1,373), `DataComponents` (439) | #760, #774, `rivet-world/src/crafting/item.rs`, `LootItemStack` |

Run three of the four at once; 2D can wait for a lane. Each cluster translates the whole
class, deletes the stand-ins it replaces, and keeps every existing scenario row green.

### Batch 3 — players, the level, mobs, and the oracle

| Lane | Work | Notes |
|---|---|---|
| **3A Mob chain** | `Mob` (1,707), `PathfinderMob`, `AgeableMob`, `Animal`, `Monster`, `Brain` (499), `BehaviorBuilder`, `MemoryModuleType`, `SensorType`, `PathNavigation` (486) | Reconciles #752 and #770. Includes the generated `AnyEntity` dispatch (#487). |
| **3B Player and connection** | `Player` (2,250), `ServerPlayer` (3,141), `ServerGamePacketListenerImpl` with `net.minecraft.server.network`, `AbstractContainerMenu` and `MenuType`, `Inventory` | Reconciles #751 and #766. Exit row: dig and place (#55). |
| **3C Level and server tick** | `Level` (2,205), `ServerLevel` (2,997), `LevelChunk` to completion, `MinecraftServer` (3,188), `PlayerList` (1,303), `ItemEntity`, `Explosion`, `NaturalSpawner` | Folds the ad-hoc `level_*_runtime.rs` files in `rivet-server/src/server/level` into the mirrored classes. Highest regression risk: full gate before merge. |
| **3D Tickdiff v0** | `Commands`, `CommandSourceStack`, console input, and `TickCommand`, `SetBlockCommand`, `FillCommand`, `SummonCommand`, `GiveCommand`, `KillCommand`, `DataCommands` (get), `SaveAllCommand`, `TimeCommand`, `GameRuleCommand`; the harness in `tools/rivet-oracle`; the first 10 scripts | Without it, P1 has no exit criterion and P2 has no oracle. |
| **3E Scaffold F1** | The `net.minecraft.world.level.block` family skeleton (about 436 classes) | Lets block leaf waves start the moment 2C and 3C land. |

Remaining spine clusters after Batch 3 (queue them as lanes free): `LootContext`,
`LootTable` and entry containers; `Enchantment` and `EnchantmentHelper`; the structure
hubs (`Structure`, `StructureTemplatePool`, `StructurePoolElement`, `StructurePieceType`);
`Raid` and `Raider`; `Villager` and `AbstractVillager`; `AbstractHorse`, `AbstractBoat`,
`AbstractMinecart`; `EnderDragon`; `EntitySelector` and its parser; `PlayerAdvancements` and
`CriteriaTriggers`; `DedicatedServer` and `LevelStorageSource`; `SoundEvents` (generated);
Moonrise `NewChunkHolder` and `ChunkTaskScheduler`.

---

## 5. Hygiene

### 5.1 Decisions to record in `DECISIONS.md`

- **D15 Crate topology.** One crate must contain `ServerLevel`, `Entity`, block and item
  behaviour, menus, commands and the player. Rust's acyclic crate graph forces this unless
  trait seams are invented, which PORTING forbids. `ServerLevel` and the live runtime already
  sit in `rivet-server`, so the smallest move is to fold `rivet-entity` (16k LOC) into
  `rivet-server` at `world::entity`. `rivet-world` keeps what compiles without live types:
  chunks, storage, levelgen, lighting, block state tables, the reader interfaces, item value
  types. The ledger's `crate` column assigns every class, computed from the class graph, so no
  agent ever chooses. Packages that need live types move up as whole packages.
- **D16 Scope.** The table in 2.1.
- **D17 Gate tiers.** Section 3.5; amends D10.
- **D18 Tests.** Only three kinds: ported Paper JUnit tests, tests backed by a Java-generated
  fixture, and tickdiff or client scenarios. No invented behavioural unit tests.
  `WORKFLOWS.md` already says this; the November PRs ignored it and tests are now 41% of
  hand-written Rust.

### 5.2 Docs

| File | Action |
|---|---|
| `GOAL.md` | Keep. Point "where we're going" at this roadmap. |
| `DECISIONS.md` | Keep. Add D15–D18; amend D9 and D10. |
| `PORTING.md` | Keep. Add: classes over 2,000 LOC become a directory module with `impl` blocks split by Java source order; defining any Appendix A type outside its canonical home is forbidden; the D18 test rule. |
| `OWNERSHIP.md` | Add the 1C sections; remove the "(refine)" placeholders. |
| `CRATES.md` | Rewrite the table for D15. Cut the three-paragraph `pcre2` justification to two lines. |
| `WORKFLOWS.md` | Rewrite to about 80 lines describing section 3. Delete the Bun narrative, the pilot section, and catalog entries that were never scripted (`analyze-graph`, `verify-oracle`, `shim-gen`, `doc-drift`). |
| `RESEARCH.md`, `LESSONS-PUMPKIN.md` | Merge into one `docs/background.md`. |
| `SPEC-264-surrogate-string-boundary.md` | Decide #264, put the five-line rule in `PORTING.md`, delete the file. |
| `PARITY.md` | Generated by the train gate only; never hand-edited. Add the worldgen chunk-match count and the tickdiff count as rows. |
| `docs/g4-generated-full-parent-promotion.md` | Fold into `docs/chunk-pipeline-spec.md`. |
| `MANIFEST.tsv` | Generated. `status` derived from the ledger; add the `adapter`, `n/a(datagen)` and `deferred(dfu-audit)` labels. |
| `ROADMAP.md` (new, repo root) | This document, kept current by the controller. |

### 5.3 Workflow scripts

1. **`translate-wave.js`**
   - Take a `worktree` argument and pin it in every prompt, as `worktree-task` does. Today it
     assumes the session's working directory, so two waves cannot run at once.
   - Pass and print each unit's `java_paths`. The prompt currently says "read every Java file
     of the package", which is wrong for every split unit that shares a package.
   - Add a `rigor` argument (lean or full) and lower the round cap from 6 to 3.
   - Gate convergence on `cargo check -p <crate>` passing. The "translate first, burn down
     later" rule was written for slow builds; a cold workspace check is 45 seconds.
   - Commit per unit on the lane branch.
   - Inject the canonical-type list and the D18 test rule.
2. **`scaffold.js` (new)**: one agent creates a family's files, `mod` lines and signatures from
   the Java; one reviewer checks that the surface matches.
3. **`worktree-task.js`**: default `maxRounds` 3; add a spine task template (whole class,
   mirrored path, canonical types only, stand-ins deleted, `STUB` at leaf boundaries, ledger
   delta in the report).
4. **`review-pr.js`**: effort as an argument; feed it the ledger delta.
5. **`check-burndown.js`**: add `worktree`; expect to need it rarely.
6. **`gate.sh`**: `--fast`, `--locked`, the ledger and single-definition steps, a timing
   summary per step.
7. **`wave_picker.py`**: select by ledger (hub dependencies complete and scaffold merged),
   not by `wave`/`cycle`.
8. **`release-prep.js`**: merge `origin/main` instead of rebasing.

### 5.4 Issues, branches and worktrees

- **Tracker model.** GitHub issues are for three things only: one tracking issue per roadmap
  family (body generated from the ledger), defects and parity mismatches, and decisions.
- **Close** the M3 planning envelopes (#404–#482, #487, #511) and the M4 envelopes
  (#489–#509) with a pointer to their family tracker. Before closing any issue, grep for
  `RivetTodo(#N)` and retarget those markers to the family tracker in one hygiene PR. Do the
  same for the 79 markers that already point at closed issues (#399 alone has 32).
- **Collapse** epics #14–#28 and #250–#255 into one epic per phase.
- **Rewrite** #16, #175, #339 and #54 as 20-line scoreboards: current count, target, the
  command that produces it.
- **Close** #657 (its archives are gone) and #65 when 1A lands.
- **Keep** real defects and scenario work as they are: #55, #56, #328, #532, #698–#701,
  #709–#712, #88, #99.
- **Freeze** `rivet-api` (#505–#509) until P1 exits.
- **Prune** with `scripts/prune-worktrees.sh`. Check each of the 10 non-PR worktrees: harvest
  `items-recipes-codegen` (feeds 2D) and `manifest-scc-right-sizing` (feeds 1A) first, prune the
  ones whose work is merged. Then delete merged remote branches, and the preservation-only
  branches from the August handoff after confirming each is merged or obsolete.

---

## 6. Risks and decision rules

| # | Risk | Signal | Rule |
|---|---|---|---|
| R1 | The live-game crate outgrows the compiler. At today's ratio (about 2.5 Rust production lines per Java line) it reaches roughly 1M lines. | Incremental `cargo check -p rivet-server` over 90 s; cold test build over 12 minutes; a worktree target over 40 GB. | Stop fan-out for one batch. Move unit tests out of the crate into per-family test binaries, then split leaf families into crates that register behaviour through dispatch tables. Design for this in 1C so it is a move, not a redesign. |
| R2 | Translation quality drifts at scale. | Round-one critical plus major findings above 3 per unit for two waves running; more than 25% of a wave blocked; train gate red twice in 24 hours. | Halt new waves. Mine the findings, patch `PORTING.md` and the prompts, re-run the affected units. Halve the lane count until one clean train passes. |
| R3 | Code outruns verification again. | A family more than two waves ahead of its tickdiff scripts; no working tickdiff harness at the end of Batch 3. | Free lanes write scripts, not code. If the harness is late, put two lanes on it and pause spine clusters. |
| R4 | Tickdiff is nondeterministic. | A script fails Paper-vs-Paper. More than 30% of scripts flaky. | A script counts only after two Paper-vs-Paper passes. If flakiness is widespread, restrict scripts to a pre-generated void world with `/tick freeze` and stepping, mob spawning off, and normalise only fields listed in a committed allow-list. |
| R5 | The ownership model cannot express Java's reentrancy (entities mutating other entities, self-removal, chained explosions). | More than 10 per-call-site workarounds in clusters 2A and 2B. | Stop and amend `OWNERSHIP.md` once (for example disjoint arena borrows or a deferred mutation queue) before any entity leaf work starts. Never decide per unit. |
| R6 | Hub signatures change after leaves depend on them. | A spine class edited by a leaf PR. | After P1, hub signatures change only in a dedicated PR that goes through a full gate. Java-mirrored signatures make this rare. |
| R7 | Host limits. | `/tmp` over 50%; available memory under 4 GB; `/home` under 100 GB. | Nothing builds under `/tmp`. One full test build at a time. Prune merged worktrees immediately. |
| R8 | Scope disputes about "all of Paper". | A reachable call site into datagen or DFU code; an owner request to translate CraftBukkit. | The D16 audit decides: port exactly what the call site needs. Anything larger is an owner decision; ask once and record it. |
| R9 | Session loss. | Workflow state is per session. | Lane branches pushed after every unit, a draft PR per lane, the ledger committed. Resuming means reading open draft PRs and `COVERAGE.tsv`, nothing else. |
| R10 | Binary milestone gates stall the project, as M2 has since August. | A phase waiting on 100% of an earlier phase's parity. | Scoreboards are monotonic: a number may not go down, and no phase waits for another's number to reach 100%. Only P4 requires the totals. |
| R11 | The estimate is wrong. | After Batch 3, measured Java LOC merged per day under 8k. | Re-plan: raise the agent guideline, cut rigor to lean for more families, or narrow P2 to the overworld survival subset and report the new date to the owner. |

**When to stop scaling:** any of R1, R2 or R7 firing. **When to re-plan:** R5 or R11 firing, the
D15 move turning out to cost more than two days, or the baseline gate in P0 showing that
rows believed green are not.

---

## Appendix A — the 200 hub classes

`*` means no same-named Rust file on main at `4fc14eeb`. Numbers are Java LOC. Classes
without `*` may still be partial. CraftBukkit hubs and `DataFixers` belong to the adapter
track and the DFU audit, not to P1.

- **world.entity:** Entity (5462), LivingEntity* (4915), Mob* (1707), EntityTypes* (1205), EntityType* (739), ExperienceOrb* (494), Leashable* (469), AgeableMob* (338), LightningBolt* (286), EntityReference* (156), PathfinderMob* (80), ConversionParams* (22)
- **world.entity.player:** Player (2250)
- **world.entity, species and AI:** AbstractHorse* (1082), EnderDragon* (1014), Villager* (997), PiglinAi* (886), Raid* (882), AbstractBoat* (852), AbstractArrow (830), AbstractMinecart* (700), Turtle* (659), Warden* (657), Zombie* (641), Creaking* (611), ItemEntity* (591), Raider* (576), Projectile (504), Brain* (499), PathNavigation* (486), BehaviorBuilder* (483), AbstractVillager* (358), IronGolem* (328), Animal* (317), Breeze* (307), ProjectileUtil (281), PrimedTnt* (247), BehaviorUtils* (180), MemoryModuleType* (180), Monster* (155), HurtByTargetGoal (116), RandomStroll* (83), EnderDragonPhase* (66), AbstractDragonPhaseInstance* (65), SensorType* (61), WaterAvoidingRandomStrollGoal* (30)
- **server:** MinecraftServer* (3188), ServerPlayer* (3141), ServerLevel (2997), PlayerList* (1303), DedicatedServer* (1049), WorldGenRegion (584), PlayerAdvancements* (361), GenerationChunkHolder* (152)
- **world.level:** Level (2205), NaturalSpawner* (696), BlockGetter (342), ChunkPos (249), LevelReader (238), StructureManager (196), LevelAccessor (107), Explosion* (58), ScheduledTickAccess* (34), WorldGenLevel (16), GameRules (242)
- **world.level.block:** Blocks (6018), BlockBehaviour* (1455), Block* (716), BeehiveBlockEntity* (492), BlockEntity* (393), ChestBlockEntity* (236), BlockEntityTypes (235), DispenserBlock* (203), BlockStateProperties (162), FluidState* (156), BlockState (30), Fluids* (25)
- **world.level.chunk:** LevelChunk (1055), ChunkGenerator (736), ChunkAccess (593), ProtoChunk (350), ImposterProtoChunk (330), ChunkStep (231)
- **world.item and inventory:** Items* (2149), ItemStack (1373), AbstractContainerMenu (981), Item (773), Enchantment* (729), EnchantmentHelper* (646), DataComponents* (439), BlockItemStateProperties* (78), MenuType* (67), EnchantmentEntityEffect* (52), VanillaEnchantmentProviders* (36), EnchantmentValueEffect* (27)
- **world.phys:** VoxelShape* (968), AABB* (489), Vec3 (341), CollisionContext* (78)
- **loot, damage, effects:** LootTable* (270), DamageSource (215), LootContext* (207), LootPoolSingletonContainer* (164), BuiltInLootTables* (149), MobEffects* (136), LootItemFunctions* (89), LootPoolEntryContainer* (75)
- **levelgen:** StructureTemplate (947), StructurePiece (637), Aquifer (517), Biome (420), Blender (405), Structure* (328), Feature* (224), FoliagePlacer (220), Heightmap (199), StructureTemplateManager (175), StructureTemplatePool* (149), TreeConfiguration (142), RandomState (139), StructurePoolElement* (137), PlacementUtils* (117), StructurePieceType* (115), TreeDecorator* (91), PlacedFeature (73), CarvingContext (54), PlacementContext (53), StructureType* (46), Pools* (41), FeatureUtils* (40), PlacementModifierType (31), TreeDecoratorType* (31), PlacementModifier (17), StructurePieceSerializationContext* (16)
- **storage:** MapItemSavedData* (763), LevelStorageSource* (740), ValueInput (65)
- **core, util, resources:** Util (1314), BlockPos (789), Mth (779), Direction (682), BuiltInRegistries* (474), Registries (343), RegistryDataLoader (334), SectionPos (297), Identifier (282), Holder (278), Registry (181), RandomSource* (80), ResourceKey (78)
- **network:** FriendlyByteBuf (1700), ByteBufCodecs (794), CompoundTag (570), ComponentSerialization (262), ClientGamePacketListener* (260), Component (239), EntityDataSerializers* (230), GamePacketTypes* (219), ClientboundAddEntityPacket (178), ServerGamePacketListener* (128), Packet (65), PacketListener (41), CommonPacketTypes* (36), RegistryFriendlyByteBuf (31), ClientCommonPacketListener* (31), ConfigurationPacketTypes* (26), ClientConfigurationPacketListener* (23)
- **commands, advancements, sounds, gametest:** SoundEvents* (2060), Commands (728), CommandSourceStack* (655), EntitySelectorParser* (595), GameTestInfo* (365), EntitySelector* (292), EntityPredicate* (274), EntityArgument* (221), ArgumentUtils* (141), GameTestInstance (109), CriteriaTriggers* (75), SoundEvent* (46)
- **Moonrise and Paper:** NewChunkHolder* (1975), ChunkTaskScheduler* (1055), MCUtil* (318), PlatformHooks* (126), Conversions* (102), PaperRegistrySets* (95), ChunkSystemServerLevel* (70), ChunkData* (21)
- **Adapter track, not P1:** CraftPlayer* (3391), CraftServer* (2964), CraftMetaItem* (2646), CraftWorld* (2052), CraftEntity* (1360), CraftHumanEntity* (917), CraftItemStack* (798), CraftBlock* (703), CraftMagicNumbers* (609), CraftInventory* (580), CraftRegistry* (283), SerializableMeta* (181)
- **DFU audit, not P1:** DataFixers* (1687)

## Appendix B — how the numbers were produced

- **Class graph.** Every `.java` file under the three roots in `scripts/analyze_graph.py`
  (`ROOTS`) is a node. Edges are resolved `import` lines (longest matching prefix, so nested
  classes and static imports land on the outer class), wildcard imports matched by simple
  name, and same-package references by simple-name token after stripping comments. Tarjan
  SCC over that graph gives 4,303 / 708 / 285 as the three largest components.
- **Hub cut.** Repeatedly take the largest component, remove its 10 highest in-degree
  classes, recompute. Largest component after 25 / 50 / 100 / 150 / 200 removals:
  3,877 / 2,695 / 1,218 / 519 / 42 classes. The list is greedy, not minimal; the ledger
  work in 1A should make it reproducible inside `analyze_graph.py` and may trim it.
- **Depth.** Longest dependency chain over the condensed graph of the component minus hubs.
- **Coverage.** A Java class counts as present when a `.rs` file with its snake-case name
  exists anywhere under `crates/`. This over-counts partial ports and misses classes folded
  into a parent module.
- **Build timings.** `git archive HEAD` exported to a scratch directory, then `cargo check`,
  `cargo clippy --all-targets`, `cargo nextest run --no-run` in that order, so clippy reused
  the check's dependency artifacts. The host was shared with the controller's own builds.
- **Not measured:** full gate wall time, test run time, oracle row status, memory during a
  full test link.
