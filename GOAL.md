# GOAL.md — read this first

You are working on **Rivet**: a faithful Rust port of the Paper Minecraft server (pinned to Minecraft 26.2), including a JVM adapter so existing Java Paper plugins run against the Rust server.

**Faithful** means: Paper's observable behavior, module structure, algorithms, names, and constants are preserved. The Java source in `working/Paper` is the source of truth. We measure correctness against the real Java server (differential testing), never by assertion.

## The docs, in reading order

1. `DECISIONS.md` — binding decisions (version pin, memory model, tick model, plugin story). Don't relitigate in PRs.
2. `PORTING.md` — the Java→Rust pattern map. Every translation follows it; every review checks against it.
3. `OWNERSHIP.md` — how the GC'd object graph maps to arenas/IDs. Decided per subsystem here, never per-unit.
4. `CRATES.md` — workspace layout and which external libraries we use (and which are rejected, and why).
5. `WORKFLOWS.md` — how agent waves, reviews, and verification gates work; test-reuse strategy.
6. `RESEARCH.md` — background: why this plan, Bun's methodology, how Paper differs.
7. `LESSONS-PUMPKIN.md` — what to adopt/avoid from the Pumpkin project; consultation targets when stuck.
8. `MANIFEST.tsv` — the work queue: units, dependencies, status.

## Hard rules (excerpted; details in the docs)

- Source of truth is Paper's Java. Never copy from `working/Pumpkin` (consult only when stuck — D7).
- Nothing under `working/` is ever committed or pushed (D3).
- Wrapping arithmetic, exact RNG, exact `Mth` tables — parity is sacred (PORTING.md).
- Don't weaken tests/fixtures/oracle criteria to go green (D8). `todo!()` requires a `blocked` note.
- No `git stash` / `git reset` / force-push. Small PRs against the epic's issue; link the manifest unit.
- Reference docs change via dedicated PRs only, never silently mid-task.

## Where we're going

M0 oracle harness → M1 client joins an empty world → M2 worldgen parity → M3 survival gameplay → M4 Paper API + JVM plugin adapter. Milestones and epics live on GitHub; agents decompose epics into sub-issues and PRs.
