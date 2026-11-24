# GOAL.md — read this first

You are working on **Rivet**: a faithful Rust port of the Paper Minecraft server (pinned to Minecraft 26.2), including a JVM adapter so existing Java Paper plugins run against the Rust server.

**Faithful** means: Paper's observable behavior, module structure, algorithms, names, and constants are preserved. The Java source in `working/Paper` is the source of truth. We measure correctness against the real Java server (differential testing), never by assertion.

## The docs, in reading order

1. `DECISIONS.md` — binding decisions (version pin, memory model, tick model, plugin story, crate topology, scope, gate tiers, test and verification policy). Don't relitigate in PRs.
2. `ROADMAP.md` — where the port stands, the phases P0–P4, the execution model and the next batches. Its §0 is binding over the rest of it.
3. `docs/VERIFICATION.md` — how ported code is checked against Paper: oracle ops, layers, gate tiers, the `VERIFY.tsv` ledger and the debt policy (D17–D19).
4. `PORTING.md` — the Java→Rust pattern map. Every translation follows it; every review checks against it.
5. `OWNERSHIP.md` — how the GC'd object graph maps to arenas/IDs, `LevelCtx`, dispatch, event hooks. Decided per subsystem here, never per-unit.
6. `CRATES.md` — workspace layout and which external libraries we use (and which are rejected, and why).
7. `WORKFLOWS.md` — how agent waves and reviews run. Where it conflicts with D15–D19 or `ROADMAP.md`, those win.
8. `docs/background.md` — background: why this plan, Bun's methodology, how Paper differs, what to adopt/avoid from Pumpkin.
9. `MANIFEST.tsv` — the work queue: units, dependencies, status (status is derived from the `COVERAGE.tsv` ledger once it exists, D15).

## Hard rules (excerpted; details in the docs)

- Source of truth is Paper's Java. Never copy from `working/Pumpkin` (consult only when stuck — D7).
- Nothing under `working/` is ever committed or pushed (D3).
- Wrapping arithmetic, exact RNG, exact `Mth` tables — parity is sacred (PORTING.md).
- Don't weaken tests/fixtures/oracle criteria to go green (D8). `todo!()` requires a `blocked` note.
- Only Java-sourced expectations earn verification credit; never hand-type a Paper-computed value (D18).
- One canonical home per type; never stand in a hub type (PORTING.md §Canonical homes). The ledger, not the agent, picks crate and module path (D15).
- No `git stash` / `git reset` / force-push. One PR per lane run (ROADMAP §3.4); every port PR updates the ledgers (D9, D19).
- Reference docs change via dedicated PRs only, never silently mid-task.

## Where we're going

`ROADMAP.md` is the plan: P0 truth (ledgers, baseline gate, crate topology) → P1 the live-game spine (the ~200 hub classes, complete and canonical) → P2 leaf fan-out by family → P3 the JVM plugin adapter → P4 terminal parity. `docs/VERIFICATION.md` is how each step is proven against Paper: generic oracle ops, arena tick traces, the `VERIFY.tsv` ledger and the debt thresholds that pause porting when verification falls behind. Progress is what the ledgers say (`COVERAGE.tsv`, `VERIFY.tsv`, the generated `PARITY.md`), not issue state.
