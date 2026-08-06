# Rivet Agent Orchestration Design

How we run the port with Claude Code workflows. Companion to `RESEARCH.md`.

## Design principles

1. **Durable state lives in git, not in workflows.** Workflows crash, sessions end, resume caches are session-scoped. The single source of truth is `MANIFEST.tsv` (the work queue) plus committed code. Every workflow invocation is a stateless, idempotent executor over the manifest: it reads unit status, skips anything already `done`, and the orchestrator script (never the agents) writes status updates back.
2. **Disjoint-file assignment beats worktrees.** Bun used 4 worktrees × 16 agents because their agents shared files. We can do better: the scaffold phase pre-creates every crate, `mod` declaration, and stub file *serially*, so translation agents each own a disjoint set of files in the main tree. No worktrees, no merge stage, no conflicts. Worktree isolation is reserved for the rare agent that must touch shared files (`Cargo.toml`, `lib.rs`) — or better, the orchestrator batches those edits itself between waves.
3. **Translation does not gate on compilation.** Bun's insight: translate everything first, then burn down compiler errors as a separate crate-by-crate phase. Requiring `cargo check` green per-unit serializes the whole wave on the slowest unit and causes agents to "fix" errors by deleting code.
4. **Reviewers never see the implementer's context.** Diff + Java original + drift checklist only. Schema-forced verdicts.
5. **Fix the process, not the output.** When the same bug class appears in ≥2 units, the fix is an edit to `PORTING.md`/the wave prompt, then re-run — not hand-patching. The main session (campaign controller) owns this.
6. **The oracle gates promotion, agents gate merges.** Unit-level: adversarial review. Wave-level: `cargo check` clean + unit tests. Milestone-level: differential oracle (worldgen hashes, packet conformance, azalea bot scenarios) before a milestone branch promotes to `main`. No agent may weaken a test or the oracle to go green — enforced in every prompt and checked by reviewers.

## The manifest

`MANIFEST.tsv`, one row per **unit** (a class cluster that translates together — not per Java file; cyclic Java clusters must move as one unit):

```
id	java_package	java_paths	source_root	files	loc	crate	wave	cycle	needs_split	deps	status	attempts	notes
mc.nbt	net.minecraft.nbt	net/minecraft/nbt/Tag.java,...	minecraft	27	3109	rivet-nbt	3	nbt		com.mojang.serialization,net.minecraft,net.minecraft.util	done	1	
mc.nbt.utils	net.minecraft.nbt	net/minecraft/nbt/NbtUtils.java	minecraft	1	560	rivet-nbt	5		mc.nbt.snbt,mc.nbt.text,net.minecraft.nbt,...	pending	0	
```

Statuses: `pending → translated → reviewed → fixed → done`, plus `blocked(reason)` for human triage. Built by `scripts/analyze_graph.py` (run with `--split-nbt` to fold in the net.minecraft.nbt class-cluster split, `--split-network` to split the mc.network package into mc.network.buf / mc.network.framing / residual mc.network, and `--split-game` to split mc.network.protocol.game into the join-critical mc.network.protocol.game.join / .chunk / .serverbound units plus a residual — the M1 protocol wave's units); rerunning it is idempotent and preserves each unit's `status`/`attempts`/`notes` by id (notes may span several roots: a package's files can live under multiple source roots, and `source_root` is then the comma-joined list of those roots — each `java_paths` entry is relative to the root it was found under). `deps` are comma-joined java packages, or unit ids (`mc.nbt.snbt`) where a unit must name a sibling unit that shares its `java_package`. The wave-picker selects only units whose `deps` are `done`, preferring low-wave cycle-free units.

## Parallelization: tracks, waves, and the big cycle

**Measured fact (analyze_graph.py): 535 of 697 package units form one giant dependency cycle** —
entity↔level↔server back-references make most of Minecraft a single SCC at package level. Implications:
- Topological ordering alone can schedule only ~160 units. The rest rely on the **stub-first strategy**: scaffold + `// STUB(unit-id)` declarations let any unit translate against not-yet-ported neighbors; semantic convergence is enforced by burndown, review, and the oracle — not by ordering.
- Epic #9 (class-cluster SCC refinement) is load-bearing, not cleanup: it right-sizes units inside the blob.
- `MANIFEST.tsv` columns `wave` (topo depth), `cycle` (SCC id), `needs_split` encode this; the wave-picker prefers low-wave cycle-free units and otherwise schedules cluster-sized bites of the big SCC.

**Concurrent tracks** (independent agent sessions/workflows, from day one):
- **A — oracle/infra**: epics #1–3 (harness, azalea driver, gate/fuzz tooling).
- **B — foundation crates**: epics #4–8 — util, nbt, serialization, brigadier, registry-codegen are mutually independent; up to five parallel sessions.
- **C — manifest refinement**: epic #9.
- **D — adapter de-risk**: epic #14 (FFM spike) as soon as the M1 skeleton exists.
The serial spine is protocol → join → world → entity → gameplay; tracks A/C/D and `rivet-api` ride alongside. Within any epic, `translate-wave` pipelines units with disjoint file ownership; across crates, waves run concurrently without conflict.

### Parallelism mechanics: when worktrees, when not

Three tiers, isolation matched to conflict risk:
1. **Within a wave (same session)**: no worktrees. The scaffold pre-creates crates/mod trees/stubs so workflow agents own disjoint files in one checkout — nothing to merge, and one shared cargo `target/` keeps incremental builds warm. This is deliberate (Bun needed 4 worktrees because their agents shared files; we design the sharing away).
2. **Across epics/tracks (multiple Claude sessions)**: one session per epic, each in **its own git worktree + branch** (`claude --worktree`, background agents with worktree isolation, or `git worktree add`). Sessions never share a checkout — cross-track conflicts are resolved at PR merge, gated by `scripts/gate.sh` run on the merged result (D10 — no hosted CI). This is the intended way to scale beyond one session: 3–5 concurrent epic sessions, each producing PRs against `main`.
3. **Shared-file hotspots** (`Cargo.toml`, `lib.rs` mod lists, MANIFEST.tsv): only the wave controller edits these, serially, between waves. If two tracks both need a workspace change, it goes in a tiny standalone PR first.
Caveats: each worktree gets its own cargo `target/` (disk + cold builds — do not share one via symlink; concurrent cargo runs fight over the lock; consider sccache if this hurts). Keep MANIFEST.tsv updates append/status-only so PR merges of it stay trivial.

## GitHub process wiring

Milestones M0–M4, epics (label `epic`) per track live on github.com/jonaslsaa/rivet. Agents decompose epics into sub-issues using the **port-unit issue template**, work them via small PRs (PR template carries the fidelity checklist), and update `MANIFEST.tsv` in the same PR. **No hosted CI (D10)**: the controller runs `scripts/gate.sh` (fmt → clippy -Dwarnings → tests → oracle verify → rivet-parity → Paper-vs-Paper scenario → machete) on every PR before merging — a red gate blocks the merge. The oracle steps never silently skip: missing prerequisites make the gate exit nonzero with an UNVERIFIED status (distinct exit code 3) and a per-item fix list; `--require-oracle` turns any missing oracle prerequisite into a hard failure (exit 1). `blocked` label = controller triage; `regression`/`parity` labels come from `verify-oracle` runs.

## Workflow catalog (`.claude/workflows/`)

`translate-wave`, `check-burndown`, and `review-pr` exist as executable scripts in `.claude/workflows/` — invoke them by name with `args` built from MANIFEST.tsv rows (the controller selects units and passes them in; scripts never read the manifest themselves). The rest below are designs to be scripted when their phase begins.

| Workflow | Shape | Purpose |
|---|---|---|
| `analyze-graph` | fan-out readers → merge | Build/refresh MANIFEST.tsv from Java sources |
| `scaffold` | serial | Crate skeletons, mod trees, stub files, Cargo.toml — run once per crate wave, before fan-out |
| `translate-wave` | pipeline per unit | The core loop (below) |
| `check-burndown` | loop until clean | Compiler-error work queue, partitioned by module |
| `review-pr` | single max-effort agent | Pre-merge "nuke": whole-PR cross-unit review, then `gate.sh` |
| `verify-oracle` | parallel scenarios | Differential tests vs vanilla/Paper; failures → new manifest rows |
| `shim-gen` | pipeline per API class | JVM adapter codegen (below) |
| `doc-drift` | small panel | After each wave: mine review findings for systemic patterns → proposed PORTING.md edits |

## The core loop: `translate-wave`

Args: `{ waveId, units: [...manifest rows...] }` (topo-ready units only, ~10–30 per wave; concurrency caps at 16 anyway).

Per unit (see `.claude/workflows/translate-wave.js` for the real script):

1. **Implement** — full context: Java source, PORTING.md, OWNERSHIP.md section, stub conventions.
2. **Review→fix convergence loop** ("golden loop"), max 3 rounds:
   - Round 1: **two** fresh diff-only reviewers with different lenses (semantic drift / ownership+API) — first contact has the highest yield.
   - Rounds 2–3: **one** fresh full-lens reviewer per round, blind to prior findings, reviewing the post-fix state.
   - Zero findings → converged. Findings → fixer applies them, next round re-verifies.
   - After round 3: minor-only findings get one blind fix and pass as `converged: with-unverified-minor-fixes`; anything critical/major → `blocked` for controller triage. The loop's verdict comes from agents that never met the implementer — the implementer never judges its own convergence.
3. **Per-PR "thermo-nuclear" review** (`review-pr`, `effort: max`) runs once per PR after burndown, before `gate.sh` — it hunts what per-unit reviews structurally can't see: cross-unit stub conflicts, aggregate OWNERSHIP.md violations, API-surface incoherence, process violations (weakened tests, smuggled doc edits).

Notes:
- **pipeline(), not phase barriers** — unit A runs its review loop while unit B is still translating.
- Reviewers are **fresh instances every round** and see only code + Java originals, never the implementer's reasoning or previous findings (anchoring kills adversarial yield).
- Review catches drift early; the oracle is the truth. "Zero findings" never substitutes for parity fixtures — the merge gate stays `review-pr` + `gate.sh` + oracle scoreboard.
- Agents return structured reports; the **main session** updates MANIFEST.tsv and commits per wave. Agents never `git commit`, never `git stash/reset` (forbidden in every prompt — Bun learned this the hard way).
- Implementer prompt hard rules: no TODO-and-skip, no inventing APIs, wrapping arithmetic everywhere Java arithmetic exists, `blocked` verdict with reason if the unit can't translate faithfully.

### The Java→Rust semantic-drift checklist (reviewer lens 1)

The bug classes a Java→Rust port breeds, kept in `PORTING.md` and pasted into every review prompt:

- **Integer overflow**: Java wraps silently; Rust panics in debug. Minecraft's RNG, worldgen, and hashing *depend* on wrapping. Every arithmetic op on Java `int`/`long` ports as `wrapping_*` / `Wrapping<i32>` unless proven otherwise. This is the #1 worldgen-parity killer.
- `>>` vs `>>>`: Java has both arithmetic and logical shifts; map to the right one on the right signedness.
- **HashMap/HashSet iteration order** assumptions; Java `HashMap` order ≠ `std::collections::HashMap` order — flag any iteration whose order is observable.
- **UTF-16 vs UTF-8**: `String.length()`/`charAt` semantics, `char` arithmetic.
- **Float parity**: MC uses lookup-table `sin`/`cos` (`Mth.sin`) — port the table, don't call `f32::sin`. `Math.floor(double)→int` casts saturate differently than Rust `as`.
- `null` → `Option` elisions: any Java null-check dropped in translation.
- `equals`/`hashCode` identity vs value semantics; `==` on boxed types.
- Dropped `synchronized`/`volatile`; static-init order dependencies.
- Eager vs lazy: `unwrap_or(expensive())` where Java had short-circuit; side effects inside `debug_assert!`.

## `check-burndown`

After each wave: run `cargo check` (orchestrator or a single agent), partition errors by module, spawn one fixer per partition, loop until clean or dry.

```js
let rounds = 0
while (rounds++ < 8) {
  const check = await agent('Run cargo check --message-format=json for the workspace; return errors grouped by module.', { schema: ERRORS })
  if (!check.groups.length) break
  await parallel(check.groups.map(g => () =>
    agent(burndownPrompt(g), { label: `burn:${g.module}`, effort: 'low' })))
  log(`round ${rounds}: ${check.groups.length} modules with errors`)
}
```

Fixer rule: errors are fixed by *correcting the translation*, never by deleting functionality or `todo!()` — a `todo!()` requires a `blocked` manifest note.

## `verify-oracle`

Runs against a milestone branch, scenarios in parallel: worldgen chunk-hash diff over N seeds vs vanilla, packet round-trips vs recorded sessions, azalea bot scripts (join, move, dig, place, combat) executed against both servers with compared outcomes. Output: structured failure list → converted to manifest rows tagged `regression`. Milestones promote only on green.

### Headless client driver

Use Azalea pinned to an exact Minecraft 26.2-compatible release or Git revision as the headless client for behavioral scenarios. Keep it in an isolated tool package/process with its own pinned nightly Rust toolchain: Rivet's production workspace remains on its pinned stable toolchain, and Azalea's Bevy/ECS dependency graph must not enter any server crate. `rivet-oracle` starts the driver and consumes a versioned, machine-readable JSON transcript.

Each scenario is run with identical seed, configuration, world state, and offline bot identity against Paper and Rivet. Compare normalized observable outcomes—login state, position, inventory, health, chunks, server corrections, and relevant packets—rather than treating a successful connection as sufficient. Preserve raw logs or packet captures for diagnosing differences, but exclude nondeterministic values from the parity comparison only when the exclusion is explicit and justified.

Before Rivet implements enough of the protocol to accept a client, validate the harness by running scenarios Paper-versus-Paper and requiring identical normalized transcripts. Also include a controlled negative case to prove that the comparator detects a known difference. Start with a single join scenario, then add movement, digging, placement, inventory, and combat as the corresponding server functionality lands. The harness uses `online-mode=false`; no Microsoft account is required.

## Test reuse: what we can port, what we must record, what we must build

Measured in the tree: 186 JUnit files (~21k LOC) in `paper-server/src/test` + `paper-api/src/test`, and the vanilla **GameTest framework** (47 files under `net/minecraft/gametest`).

1. **The Java server is the executable oracle** — the replacement for Bun's 60k tests. Never hand-write expected values; generate them from the Java side and check them in as golden fixtures: worldgen chunk hashes per seed, packet captures per scenario, region/NBT files, registry dumps, loot-table rolls with fixed seeds.
2. **Port the GameTest runner early, reuse vanilla's test content.** Modern Minecraft defines test instances as *data* (structures + test-environment definitions in data packs) executed by the framework. Port the runner (small — 47 files) and the vanilla test content runs against Rivet unchanged. This is the closest thing to a language-independent behavioral suite that exists for Minecraft, and it's the highest-leverage single test investment.
3. **Port Paper's JUnit tests with their units.** They're mostly registry/API-consistency checks (MaterialTest, BlockDataTest, …) — mechanical to translate, and they pin the API surface. Policy: when a unit's Java class has tests, the same wave translates the tests; the implementer's report lists them and reviewers check the tests weren't weakened in translation.
4. **Tests-with-translation policy for untested code** (most of the 630k): the implementer writes round-trip/property tests for anything with a serialization or math surface (NBT, packets, RNG, Mth) against the golden fixtures from #1. Behavior-heavy code gets its coverage from GameTests and bot scenarios instead — don't ask agents to invent unit tests for AI/gameplay logic; invented tests just enshrine the translation's own bugs.

**The on-track signal is a parity scoreboard, not vibes**: a checked-in `PARITY.md` updated by `verify-oracle` after each milestone run — % chunks hash-identical over N seeds, % recorded packets round-tripping, GameTests passing/total, Paper JUnit ports green, corpus plugins booting. Wave-level health is cheaper and continuous: `cargo check` clean, per-wave test pass rate, and reviewer-findings-per-unit trending *down* (if it isn't, stop scaling and fix PORTING.md). Any scoreboard number that goes down blocks the next wave — regressions are cheapest the wave they appear.

## The JVM plugin adapter track (`shim-gen`)

Goal: real Paper plugins (jars) run against the Rust server. Architecture:

- **Keep `paper-api` as-is** (plugins compile against it unchanged). Reimplement the *implementation* layer (`CraftServer`, `CraftWorld`, `CraftPlayer`, event dispatch, scheduler) as Java shims that call into Rust over **JNI / Java 22+ FFM (Panama)**.
- JVM runs **in-process**, plugin code confined to a dedicated "main thread" that is tick-synchronized with the Rust tick loop (Bukkit's API is main-thread-confined anyway — this maps cleanly).
- Rust side exposes a stable C ABI facade (`rivet-ffi` crate); events flow Rust → JVM dispatch → mutations back through the facade. Batch per-tick to amortize FFI cost; benchmark event-storm latency *early* (M1), because if this is too slow the whole adapter needs a redesign.
- **Honest limit**: plugins that reflect into NMS/CraftBukkit internals (many do) cannot work; only API-clean plugins are in scope. Track a compatibility corpus (LuckPerms, EssentialsX-core-commands, Vault, PlaceholderAPI, WorldGuard as stretch) and make "corpus plugin boots and passes its smoke script" the oracle for this track.

`shim-gen` is embarrassingly parallel and highly mechanical — ideal agent work:

pipeline over paper-api classes: **generate** (Java shim + Rust FFI stub from the API signatures) → **compile-gate** (javac + cargo check per class, this track *does* gate per-unit since units are tiny) → **review** (one diff reviewer, `effort: low`) — then integration-test waves that boot the corpus plugins.

This track only needs the Rust core's internal API, so it starts at M1 against the minimal server, de-risking the FFI design years before M4.

## Campaign control

- **The main session is the controller, not a workflow.** Per cycle: pick ready units from MANIFEST (topo order) → `translate-wave` → `check-burndown` → read journals → `doc-drift` → update docs/manifest → commit → next wave. Chained single-phase workflows with judgment between them beat one mega-workflow.
- **Long autonomous runs**: `/loop` (dynamic self-pacing) with the cycle above as the loop body; wakeups land as workflow notifications arrive. Token budget directives (`+Nk`) with the loop-until-budget guard for bounded overnight runs.
- **Config prerequisites**: raise the workflow size guideline (default is <15 agents) via "Dynamic workflow size" in /config; pre-allow the build commands (cargo, javac, gradle) to avoid permission stalls mid-wave.
- **Escalation**: `blocked` units and 3×-failed units stop being retried and surface in the wave report for human/controller decision. Systemic findings (same bug class twice) trigger `doc-drift`, and affected `done` units get re-queued for a targeted re-review.
- **Model/effort tiering**: implementers inherit session model; reviewers `effort: high`; mechanical work (stubs, burndown, shim codegen) `effort: low`.

## Pilot before scale (non-negotiable)

Exactly like Bun's 3-file trial: pick 3 units from `rivet-nbt` (leaf crate, oracle-testable via NBT round-trip against real region files), run the full `translate-wave` → `check-burndown` → review cycle, then spend a session tuning prompts, schemas, and PORTING.md from what went wrong. Only then scale to full waves.
