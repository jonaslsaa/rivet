# Rivet Decisions

Binding decisions for the port. Agents: do not relitigate these in PRs; propose changes as issues instead.

## D1 — Goal
Port the Minecraft server **and** Paper's layer on top of it to Rust, faithfully, and run existing Java Paper plugins through an embedded-JVM adapter. Fidelity target: Paper's observable behavior, module structure, algorithms, and constants — not its object graphs (see D4).

## D2 — Version pin
Minecraft/Paper **26.2** (the version in `working/Paper`). No version bumps until M4. The registry/data layer is codegen so future bumps are mechanical.

## D3 — Legal posture

Human decision (2026-08-06): Rivet is published under GPL-3.0-or-later (see `LICENSE`). The license covers Rivet's own contributions only and grants no rights to Mojang-owned material; Mojang's MIT libraries (Brigadier, DataFixerUpper) and Paper's GPL/MIT code are ported directly. No Mojang-derived source (`working/`) is ever committed or pushed.

## D4 — Memory architecture
No garbage collector to lean on, so: **index/arena storage, IDs over references** (see `OWNERSHIP.md`). No full ECS framework — feather died of architecture golf; we keep Paper's structure at module level with arenas underneath.

## D5 — Concurrency model
Single-threaded synchronous game tick owning all world state (matches vanilla's model and Bukkit's main-thread confinement — required for the plugin adapter). `tokio` for network IO at the edges, `rayon` for chunk generation/lighting worker pools; both communicate with the tick thread via channels, never locks on hot game state. Folia-style region threading is explicitly out of scope until after M4.

## D6 — Plugin story
1. **JVM adapter first**: `paper-api` jar unchanged; the implementation layer regenerated as Java shims calling a `rivet-ffi` C ABI (Java FFM/Panama on the Java side — not JNI C glue). Plugin code runs on the tick thread itself, as Bukkit plugins run on Paper's server main thread (D20). API-clean plugins only; NMS reflection is out of scope.
2. Native Rust plugin API second, WASM ABI later. Neither before M4.

*Item 1 amended 2026-11-24 by D20; previously "Plugin code runs on a dedicated tick-synchronized thread."*

## D7 — Pumpkin usage policy
`working/Pumpkin` is for **learning and unblocking only**: study its design decisions, consult it when stuck on a parity problem. **Never copy code from it while porting** — our source of truth is Paper's Java. (Rationale: fidelity to Paper is the differentiator; mixing in a second reimplementation's interpretations breaks the "faithful port" verification story.)

## D8 — Verification
The Java server is the oracle; parity is measured, not asserted (the `VERIFY.tsv` ledger and the generated `PARITY.md` scoreboard, D19; what a test must be to count, D18). No agent may weaken a test, fixture, or oracle criterion to go green. `todo!()` requires a `blocked` manifest/issue note.

## D9 — Process
Work follows the phases and batches of `ROADMAP.md` (P0–P4; its §0 is binding over the rest of it). Progress is measured only by the tool-written ledgers, `COVERAGE.tsv` (D15) and `VERIFY.tsv` (D19). GitHub issues are for family trackers, defects and parity mismatches, and decisions (ROADMAP §5.4). Leaf work is never issue-scoped: spine clusters and infrastructure run as `worktree-task`, leaf units as `translate-wave` over ledger units (ROADMAP §3.1). Reference docs (`DECISIONS.md`, `PORTING.md`, `OWNERSHIP.md`) are updated via dedicated PRs, never silently mid-wave. Rust edition 2024, `rustfmt` defaults, clippy `-Dwarnings` clean in every gate tier (D17).
*Amended 2026-11-23 by D15, D17 and D19; previously "GitHub epics → agent-created sub-issues → PRs, milestones M0–M4 (see `WORKFLOWS.md`)". Where `WORKFLOWS.md` still describes the old process, D15–D19 and `ROADMAP.md` win until that file is rewritten.*

## D10 — No hosted CI
No GitHub Actions (or any hosted CI). The merge gate is `scripts/gate.sh` (fmt → clippy `-Dwarnings` → tests → oracle steps: `rivet-oracle verify` and the byte-for-byte `rivet-parity` diff vs the Paper Java oracle), run locally by the controller in the tiers of D17: T1 before merging any PR, T2 at every batch boundary and before any milestone or phase closes. Oracle verification is never silently skipped: missing prerequisites make the gate exit nonzero with an UNVERIFIED status and a per-item fix list; `--require-oracle` turns any missing oracle prerequisite into a hard failure. A red gate blocks the merge; enforcement is by process (PR checklist + controller), not by GitHub.
*Amended 2026-11-23 by D17: "before merging any PR and at the end of every wave" became the T0–T3 tiers. No hosted CI, no silent skip, and the blocking rule are unchanged.*

## D11 — JVM-adapter FFI design: GO (in-process FFM)
De-risking spike for epic #14 / sub-issue #81 (the `spikes/ffi-latency/` crate + Java FFM benchmark) concluded **GO** on the in-process Java FFM/Panama bridge design (D6). The C-ABI spike measures: scalar downcall, per-call handle-table re-resolution, batched event publish, a Rust→Java→Rust callback round-trip, and back-to-back callback storms. Reproduce with `spikes/ffi-latency/run.sh`, which builds the cdylib, compiles the Java harness, runs the correctness assertions (including exception containment), benchmarks, and writes the machine-readable report to `spikes/ffi-latency/results/benchmark.json`. That report is a **regenerable artifact**: it is gitignored and never committed; any claim in this decision is backed by the recorded run in issue #81, not by a checked-in result file.

Recorded numbers (release build, Temurin 25.0.2 arm64; representative measured ranges across recorded runs, bracketing the exact run in issue #81):
- per-event callback cadence during a 100k storm ~100-120 ns → **~41k-50k events/tick fit the 5 ms dispatch budget** (10% of a 50 ms tick), vs a realistic assumption of ~10k plugin events/tick → **GO**.
- single callback round-trip (Rust→Java→Rust) ~215-250 ns; bulk state mutation via `rfv_apply_events` is far cheaper per event (~0.5-0.56 ns/event at n=4096) and should stay on the batched path.
- the worst-case single 100k-event storm (~10-12.1 ms) does NOT fit the 5 ms budget — recorded honestly; the go/no-go is driven by realistic per-tick volume, not the synthetic storm.
- exception containment is proven: the Java upcall target catches every plugin `Throwable` and returns an explicit ABI-safe status; Rust surfaces it as an error result (`ERR_CALLBACK`), so a foreign exception can never unwind through Rust.

Decisions locked: `rivet-ffi` C ABI with marshal-only u64 IDs (never pointers across the boundary); batched event publishing for bulk state, per-plugin handler dispatch on the callback path; plugin code on the tick thread, called through FFM upcalls (D5, D20).
*Amended 2026-11-24 by D20; previously "plugin code on a dedicated tick-synchronized thread (D5)". The measurements above are unchanged: the Rust→Java→Rust round trip is exactly D20's call path.*

## D12 — CompoundTag is insertion-ordered (NBT key order for byte-identical chunk NBT)
Locked 2026-08-07 for issue #226 (M2 gate preflight). Java's `CompoundTag` wraps an `Object2ObjectOpenHashMap<String, Tag>` (fastutil) that iterates in fastutil hash order; reproducing that byte-for-byte in Rust would require a Java-identical hasher. Rivet instead stores tags in an insertion-ordered `indexmap::IndexMap`:
- the reader (`NbtIo.read`/`load`) inserts keys in on-disk order, so any compound **read** from binary NBT re-emits its exact on-disk field order — a Paper 26.2 chunk fixture round-trips byte-for-byte (golden test `committed_chunk_fixture_round_trips_byte_identical`);
- hand-built compounds (SNBT → binary) emit Rust's put sequence, which differs from fastutil hash order — the documented `compound_key_order` divergence in PARITY.md (soft, `diverged`, never `mismatched`);
- deterministic across processes (no randomized seed).

This makes byte-identical `SerializableChunkData` NBT possible without a fastutil hasher port. The `compound_key_order` oracle divergence is accepted and counted under `diverged`.

## D13 — Region compression: byte-identity gate runs at `region-file-compression=none`
Locked 2026-08-07 for issue #226 (M2 gate preflight). Java `Deflater` output is not `flate2`-reproducible in general, and `RegionFileVersion.DEFAULT = VERSION_DEFLATE` is volatile-selected, so deflate is not a byte-identity mode. The M2 byte-identity round-trip therefore pins **both sides** to `region-file-compression=none` (fixtures/`server.properties` + manifest now record `none`). Read support is mode-separate: `NbtIo.read_compressed` (gzip only, mirroring Java's `NbtIo.readCompressed`) is proven against Paper's own gzip `level.dat` fixture (`reads_real_paper_gzip_level_dat_fixture`); deflate/lz4 reads live in the region-file layer, which is not yet ported. Deflate/lz4 **write** parity is deferred to the chunk.storage wave (issue #231, `RivetTodo` in `nbt_io.rs`); chunk payloads are captured as decompressed NBT so the `none` pin is byte-consistent with existing fixtures. The region-file on-disk format this pins (header, sector layout, codec ids, oversized/`.mcc` stubs, corruption handling) is specified in `docs/region-file-format-spec.md`.

## D14 — Mth transcendentals are native-platform parity, not single-binary parity
Locked 2026-08-19 for issue #668. Paper 26.2 builds `Mth`'s `COS_TAB`/`ASIN_TAB` at class-init time with `java.lang.Math.cos`/`Math.asin` (`Mth.java` static init: `COS_TAB[ind] = Math.cos(Math.asin(ind/256.0))`). The HotSpot intrinsics used while constructing `COS_TAB` differ by up to **1 ULP across supported architectures** (x86_64 vs aarch64), so there is no single bit-exact `COS_TAB` Paper produces on every machine. Rivet therefore carries **independently generated, compile-time selected** x86_64 and aarch64 `COS_TAB` variants and matches native Paper **per supported platform** — not a single cross-platform golden constant. `ASIN_TAB` is shared because it is bit-identical on both supported architectures.
- **x86_64 Linux with Temurin 25 is the primary development/release-gate target**; the release gate and byte-for-byte `rivet-parity` diff run against it (D10).
- **aarch64 remains supported** and must have its **own verified generated `COS_TAB`**, generated and verified against native aarch64 Paper (or the same JVM math library) — the x86_64 `COS_TAB` must not be substituted.
- **Hardware-created NaN sign is undefined** (architecture picks the payload/sign bits). Golden and parity comparisons therefore **canonicalize NaNs** — a NaN bit-pattern is compared as "is NaN", not bit-for-bit — while all **non-NaN values remain bit-exact**. Canonicalization is done the **same way Java's oracle does**: the `MthGen` oracle and the golden `bitexact_f32`/`bitexact_f64` helpers use `Float.floatToIntBits`/`Double.doubleToLongBits` (canonical NaN `0x7fc00000`/`0x7ff8000000000000`). Unlike the `*Raw*` variants, these methods canonicalize NaN payload/sign bits; both forms preserve every non-NaN bit exactly.
- **Extent of the divergence**: exactly **11 of the 257 `COS_TAB` entries differ by 1 ULP** between the committed x86_64 and aarch64 tables (`COS_TAB[181]` is one of them). `ASIN_TAB` is bit-identical on both. Because `atan2` reads `COS_TAB`, **exactly four golden rows are arch-selected**: `atan2(1,1)`, `atan2(-1,1)`, `atan2(0.5,0.5)`, `atan2(-0.5,0.5)` — each reads `COS_TAB[181]` and carries a `#[cfg(target_arch)]`-selected expected value. Every other golden `rhs` is a single plain literal, bit-exact on both arches.
- **`Math.min`/`Math.max` signed-zero semantics**: `Mth.min`/`Mth.max` (all four f32/f64 overloads) mirror Java's `Math.min`/`Math.max`, which return the **first operand** for a same-sign equal pair — e.g. `Math.max(-0.0,-0.0)` is `-0.0`, `Math.min(0.0,0.0)` is `+0.0` — and only force `+0.0`/`-0.0` when a pair of zeroes differ in sign. The `rivet-util` ports use explicit sign-bit checks, never Rust's `f32::min`, `f32::max`, `f64::min`, or `f64::max`, so the result does not depend on optimization level.
- **Forbidden**: substituting `StrictMath` (whose results differ from HotSpot's `Math.cos`) for table generation, and **hand-editing** a generated table. Tables must be regenerated on each target platform via the codegen path, never hand-edited (D8 and the `rivet-codegen` documentation).
- **Generator provenance is fail-closed** (D14/D8): `rivet-codegen mth-gen` writes **only the host arch's** `COS_TAB` module — x86_64 (primary release-gate target) writes `mth_cos_tab_x86_64.rs` and leaves `mth_cos_tab_aarch64.rs` untouched; the committed `aarch64` module is provenance-checked (generated + verified on native aarch64 Paper) and must never be substituted or silently overwritten by an x86_64 run. The renderer uses `compile_error!` for any `target_arch` that is neither x86_64 nor aarch64, and re-checks the four aarch64 expected bits against faithful atan2 arithmetic over the committed aarch64 table so a stale/mismatched literal fails at generation time.

This refines the "exact `Mth` tables" fidelity language: the requirement is per-platform bit-exact match to native Paper (11 `COS_TAB` entries, 1 ULP each), with NaN width-for-width canonicalization via the same `floatToIntBits`/`doubleToLongBits` semantics Java's oracle uses — not a single architecture-independent run-to-run table.

## D15 — Crate topology: one live-game crate; the ledger assigns every class
Locked 2026-11-23 (ROADMAP §5.1). Paper's runtime is one class-level strongly connected component (4,303 classes, ROADMAP §1.3): `ServerLevel`, `Entity` and its hierarchy, block, item and block-entity behaviour, menus, commands, the player and its connection, and the Paper event call sites reference each other directly. Rust's crate graph is acyclic, and breaking the cycle with traits where Java has concrete classes is forbidden (PORTING.md), so all of them live in **one crate, `rivet-server`**, the live-game crate.
- `rivet-entity` folds into `rivet-server` at the mirrored module path and the crate is deleted (ROADMAP Batch 2.0). In the live-game crate a module path is the Java package with `net.minecraft.` dropped (`net.minecraft.world.entity.monster` → `rivet_server::world::entity::monster`, `net.minecraft.server.level` → `rivet_server::server::level`); any other package drops only its first segment (`org.bukkit.craftbukkit.event` → `rivet_server::bukkit::craftbukkit::event`, `io.papermc.paper` → `rivet_server::papermc::paper`, `ca.spottedleaf.moonrise` → `rivet_server::spottedleaf::moonrise`).
- The crates below keep exactly what compiles without live types. The expected outcome (ROADMAP §5.1): `rivet-world` keeps chunks and `ProtoChunk`, chunk storage, levelgen, lighting, the entity-free members of the reader interfaces (`BlockGetter`, `LevelReader`, `LevelAccessor`, `WorldGenLevel`, …) and the block-entity and item *value* types; `rivet-registry` keeps the block-state tables; today's gameplay islands in `rivet-world` (`loot`, `enchantment`, `crafting`, `trading`, `scores`, `effect`, `inventory`) move up. The ledger's computation, not this list, is binding.
- **The ledger assigns; agents never choose.** `COVERAGE.tsv`, written by `scripts/coverage_ledger.py` (ROADMAP Batch 1A), carries a computed crate and module path for every Java class and interface, derived from the class graph. The live types are the seed set: `Level`, `ServerLevel`, `MinecraftServer`, `Entity` and its subclasses, `Player`, the block, fluid, item and block-entity behaviour classes (`BlockBehaviour` and `Block` subclasses, `Fluid`, `Item`, the behaviour members of `BlockEntity`), `AbstractContainerMenu`, `Commands` and `CommandSourceStack`, and the Paper/CraftBukkit event hooks. A member needs a live type when its signature or body references one, directly or through another member that does (transitively). A package goes to the live-game crate whole when a class needs a live type in a member it cannot shed by the split rule below. A wrong assignment is fixed in the tool, never overridden in a PR.
- **Split classes** are the one exception to whole-package assignment. A class or interface needed below the live-game crate (by worldgen, storage or lighting) that also has members needing live types is split by member: its struct, fields and live-type-free members stay below at the mirrored path; members needing live types go to the live-game crate at the same mirrored path: a class's non-virtual ones as an extension trait `<JavaClass>Live` with exactly one `impl`, its virtual ones through the dispatch pattern of OWNERSHIP.md §Class hierarchies, and an interface's as a subtrait `<Interface>Live: <Interface>` that every implementor implements with its own Java body (OWNERSHIP.md §Level interfaces). Examples: `BlockState` (tables in `rivet-registry`, `BlockStateBaseLive` above), `ItemStack` (`ItemStackLive`), `BlockEntity`, and the reader interfaces, whose entity and server members (`EntityGetter.getEntities`, `CollisionGetter.getEntityCollisions`, `LevelAccessor.getServer`, `ServerLevelAccessor.getLevel`, the `Entity`-taking `playSound`/`gameEvent`) go to `LevelAccessorLive` and its siblings, implemented by `LevelCtx`, `WorldGenRegion` and every other Java implementor. A split member is never dropped: worldgen code that calls one (structure pieces that add entities, such as `OceanMonumentPieces`) goes up with it or calls it through the live-game half. The ledger lists every split with both homes; no other class straddles crates.
- No other crate is carved out of the live-game component. If compile cost forces a split (ROADMAP R1), whole leaf families move to a higher crate and register into the `'static` behaviour tables and dispatch rows OWNERSHIP.md defines; that is a move, not a redesign.

## D16 — Scope: what "all of Paper ported" means
Locked 2026-11-23 (ROADMAP §2.1). The denominator, measured on Paper `0a99345`:

| Bucket | Java LOC | Treatment |
|---|---|---|
| A. Minecraft runtime (`net.minecraft` minus B and C, plus DataConverter and Moonrise) | ≈558k | Translate, file for file. |
| B. Datagen (`net.minecraft.data.*`) | 36k | Not translated: `rivet-codegen` extracts the generated data. Port only classes an audited runtime call site needs (keys such as `BuiltInLootTables`, `PlacementUtils`). |
| C. DFU datafixers (`net.minecraft.util.datafix`) | 35k | Not translated: Paper routes world data through DataConverter (bucket A). Port a DFU fix only when an audited live call site needs it. |
| D. Paper server layer (`io.papermc.paper`, `com.destroystokyo`, Moonrise in `src/main`) | 48k | Translate the behaviour: config, patches, chunk system, event call sites. |
| E. CraftBukkit (`org.bukkit.craftbukkit`) | 76k | Adapter track, not translated: generated Java shims over `rivet-ffi` (D6). The Rust side is a facade plus event hooks: the game-state logic of the CraftBukkit helpers that translated patch sites call (`CraftEventFactory`, block-state snapshot placement) is ported with the call site; Bukkit wrapper objects are not (OWNERSHIP.md §Events). |
| F. `paper-api` | 191k | Unchanged jar (D6). Not translated, except the state and read-back logic of the event classes that translated call sites fire (constructor defaults, derived getters such as `PlayerInteractEvent.isCancelled()`), ported with the call site (OWNERSHIP.md §Events). |

D16 refines the ROADMAP §2.1 table on rows E and F, where the ROADMAP says "not a translation" and "zero translation": the CraftBukkit helper logic and the `paper-api` event-class state that translated call sites depend on are ported with those call sites, because they carry game behaviour even when no plugin listens. `handleBlockGrowEvent` places its block-state snapshot only when the event is not cancelled, and `PlayerInteractEvent.isCancelled()` is `useInteractedBlock() == DENY`, whose constructor sets `DENY` when there is no clicked block. Leaving them out would change vanilla behaviour on a server with no plugins. Where the two documents differ on this, D16 governs.

The translation target is A + D, about 606k Java LOC; the adapter target is the E + F surface, reached by shim generation. The ledger and manifest label B units `n/a(datagen)`, C units `deferred(dfu-audit)`, and E and F units `adapter`; the wave picker never selects them. The E and F members ported as hooks are ledger rows of the unit that owns the calling site, labelled `hook` and counted toward that unit's method coverage; the rest of their classes stays `adapter`. An audit means the PR names the live call site, ports exactly the classes that call reaches, and records them in the ledger. Anything larger, or any request to translate CraftBukkit, is an owner decision, asked once and recorded here (ROADMAP R8).

## D17 — Gate tiers (amends D10)
Locked 2026-11-23 (docs/VERIFICATION.md §3.2; supersedes ROADMAP §3.5 per its §0.4). `scripts/gate.sh` runs in four tiers. Tiering changes *when* a row runs, never *whether*:

| Tier | When | Contents |
|---|---|---|
| T0 scoped | Every PR, in its worktree, by the workflow, at the exact head | fmt, clippy `-Dwarnings` and nextest for the touched crates (this runs every L1 golden), marker audit, `verify-ledger --check` on the diff, the `COVERAGE.tsv` check, the single-definition check (PORTING.md §Canonical homes) |
| T1 merge | Every merge, serialized under the build lock | T0 workspace-wide, codegen drift, ledger monotonicity (D19), the oracle rows whose `rust_globs` intersect the diff (path-triggered), and the always-on cheap rows (`rivet-parity`, live oracle ops on one JVM boot) |
| T2 full | Every batch (at most 5 merges or 24 hours, whichever comes first) and before any milestone, phase or epic closes | Everything, with `--require-oracle`: all Paper boots, scenarios, arenas, Stage A hash-diff, harness self-checks |
| T3 nightly | Once a day, unattended | T2, `regen --check` of every probe and op, Stage B/C seeds, time-boxed differential fuzzing, rayon 1-thread vs default determinism |

- Unchanged from D10: no hosted CI; the controller runs the gate locally; oracle steps never silently skip; a red tier blocks.
- No row is ever dropped from T2.
- A red T2 or T3 on `main` freezes merges of port PRs. The only allowed work is the fix or a revert; the batch size bounds the bisect.
- The controller may require T2 before merging a specific PR whose regression risk warrants it (for example ROADMAP Batch 3C).
- Until `gate.sh` implements the tiers and `VERIFY.tsv` exists (ROADMAP Batch 1A, VERIFICATION §5 item 1), the full `gate.sh` stays the per-merge gate, as D10 originally said.

## D18 — Tests: only Java-sourced expectations earn verification credit
Locked 2026-11-23 (ROADMAP §5.1; docs/VERIFICATION.md §2.2, §3.3). A test counts toward verification only if it is one of:
1. a ported Paper JUnit test for a class with a direct Rust twin, translated with that class and never weakened (Paper's registry and CraftBukkit consistency suites run unchanged against the JVM adapter in P3 instead);
2. a test whose expected values come from a Java-generated fixture with provenance (Paper commit, producing op or probe, SHA-256 in a manifest, reproducible by `regen`), or are derived from one;
3. a differential row at layers L2–L6: an oracle op, a world-state diff, an arena tick trace, a client scenario, or a GameTest run on both servers.

- Hand-written tests are allowed for ownership invariants, error paths and Rust-only seams. They earn zero credit, and none may assert by hand a value or ordering that Paper computes: no invented behavioural unit tests.
- No golden master of Rivet's own output. A Rivet-vs-Rivet round trip or a snapshot of Rivet output earns nothing.
- Fixtures, goldens, exclusion tables, comparators and normalizers change only in verification PRs, and a fixture change must be reproducible by `regen`. A port PR that touches them is rejected.
- A test that can pass vacuously (empty corpus, a filter that drops every row, an early return when a file is missing) is a defect: rows assert closure counts.
- This supersedes `WORKFLOWS.md` "Test reuse" items 2 and 4 (vanilla ships no GameTest corpus, and round-trip tests earn no credit) until that file is rewritten.

## D19 — Verification ledger, scoreboard and debt policy
Locked 2026-11-23 (docs/VERIFICATION.md §3–§4; ROADMAP §0.1–§0.2).
- **Two ledgers, written by tools, never by hand.** `COVERAGE.tsv` (D15: Java class → Rust home, method coverage, markers) answers "how much is ported". `VERIFY.tsv` answers "how much is checked against Paper", one row per verification surface: `id`, `layer`, `rust_globs`, `java_classes`, `mechanism`, `corpus_n`, `negative`, `tier`, `status`, `matched`, `mismatched`, `last_verified_commit`. `status` is `PASS`, `FAIL`, `UNVERIFIED` or `INACTIVE`, and only the tool writes status and counts. A row whose tamper negative is not detected, or that executed zero inputs, is `UNVERIFIED`. A row's Paper-vs-Rivet comparison counts only after Paper-vs-Paper twin runs are byte-identical after normalization; until then the row is `UNVERIFIED` (or `INACTIVE` when it cannot run yet).
- **`PARITY.md` is rendered from `VERIFY.tsv`** (`rivet-parity --scoreboard`) and never hand-edited.
- **Monotone scoreboard.** A row never disappears; its declared inputs (`corpus_n`) and `matched` never decrease; `status` never leaves `PASS`; `mismatched` never increases; a subsystem's verified-LOC share never drops by more than the PR's own registered debt. Adding a `FAIL` or `UNVERIFIED` row is always allowed. The only exit from a violation is a dedicated PR that changes nothing else and is approved by the owner; agents cannot approve it. A known divergence is a `FAIL` row with an issue, not an exclusion; an accepted divergence needs its own decision here (D12 precedent).
- **Coverage declaration in every PR.** Each new or materially changed `crates/**/src/**.rs` file is (a) matched by a Java-sourced row that is `PASS` after the PR, (b) given its row in the same PR, (c) covered by a row an earlier verification PR added as `UNVERIFIED`, or (d) registered as debt with an owning issue, which is legal only while its subsystem is under threshold.
- **Test-first is preferred.** A verification PR lands the op or arena and its recorded goldens while the Rust side is a `STUB`, so the row is `UNVERIFIED`; the port PR flips it to `PASS`. The expectation then predates the implementation.
- **Debt.** `scripts/verify_debt.py` (offline, deterministic) classes the non-test LOC of every source file as V2 (a `PASS` live differential row with a negative control), V1 (a `PASS` recorded golden reproducible by `regen`) or V0 (no Java-sourced row). A row credits at most K × `corpus_n` LOC, with K = 50; the excess counts as V0. Thresholds (VERIFICATION §4.2):

| Condition | Action |
|---|---|
| Subsystem `debt_ratio > 0.30` and `debt_loc > 10k` | No new port waves in that subsystem; its next workflow runs are verification items. |
| Subsystem `debt_delta > 0` for 2 consecutive batches | The same. |
| Global `debt_ratio > 0.35` | Global pause on translate-waves; the next cycle works the top of the V0 list. |
| A subsystem with no rows | Its first wave is preceded by its first op or arena. No exceptions. |
| Steady-state target | Global `debt_ratio <= 0.15`, and every subsystem has at least one V2 row. |

- **Hard stops** (no new translate-wave anywhere): T2 or T3 red on `main`; any scoreboard row regressed; `regen --check` drift; the oracle unbootable for more than one batch. **Subsystem stops:** debt over threshold, or the subsystem's milestone-terminal row still `INACTIVE`/`UNVERIFIED` while units downstream of it are being ported (VERIFICATION §3.5).
- **Spine exception** (ROADMAP §0.2). The P1 spine proceeds despite the debt thresholds, because no gameplay verification can run without it. Every spine cluster PR ships with, or after, a Java-sourced row for its surface, and P2 leaf fan-out in a family is gated by that family's `debt_ratio`.

## D20 — Plugin code runs on the tick thread (amends D6 and D11)
Locked 2026-11-24 by the controller. Bukkit plugins run on Paper's server main thread, and Rivet's tick thread is that thread (D5), so plugin code runs on it.
- **One thread, nested calls.** The JVM is created in-process (D11). Rust calls Java listener, scheduler (`mainThreadHeartbeat`), command and plugin lifecycle code through FFM upcalls made from the tick thread itself, and the Java shims' downcalls into `rivet-ffi` run on that same thread, nested inside the upcall, against the `ctx` the upcalling frame lends (OWNERSHIP.md §JVM adapter boundary). D11's measured Rust→Java→Rust round trip (~215-250 ns) is this path. There is no plugin thread, no rendezvous and no handoff of a `ctx` between threads.
- **`rivet-ffi` is confined to the tick thread.** The runtime belongs to the thread that called `rivet_ffi_v1_init`, the tick thread; a call from any other thread returns `WrongThread` (#772). Where `rivet-ffi`'s doc comments say "dedicated tick-synchronized thread", that thread is the tick thread.
- **Other JVM threads never touch game state.** Bukkit async tasks (`runTaskAsynchronously`, `runTaskTimerAsynchronously`), threads a plugin starts and asynchronous events (`Event.isAsynchronous()`) run on JVM threads other than the tick thread. Their API calls that reach game state are rejected or marshalled onto the tick thread exactly as Paper does off its main thread: rejected where Paper rejects (`AsyncCatcher.catchOp` throws `IllegalStateException("Asynchronous <reason>!")`; `PaperEventManager.callEvent` throws for a synchronous event fired off the main thread while the server is not stopping), marshalled where Paper marshals (`MCUtil.ensureMain` queues on `MinecraftServer.processQueue`, and its `Supplier` form blocks the caller until the tick thread has run it; `BukkitScheduler.runTask` runs at the next `mainThreadHeartbeat`). A call that Paper leaves unguarded off the main thread is a data race in Paper, not behaviour; the shim rejects it as `catchOp` does, and a corpus plugin that depends on one is a parity mismatch under D19. Bukkit bookkeeping that Paper keeps thread-safe in Java (the scheduler's task queues, `HandlerList` registration) stays in the Java shims and needs no game state.
- **Consequence.** All game code runs on the one tick thread. Per-server mutable state of the behaviour singletons needs no atomics, no `Arc` and no thread handoff (OWNERSHIP.md §Mutable singleton state).
- This supersedes `WORKFLOWS.md`'s "dedicated main thread that is tick-synchronized with the Rust tick loop".
