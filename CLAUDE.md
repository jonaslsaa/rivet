# Rivet

Faithful Rust port of the Paper Minecraft server (pinned MC 26.2), plus a JVM adapter for Java Paper plugins. **Read `GOAL.md` first** — it indexes all design docs (`DECISIONS.md`, `PORTING.md`, `OWNERSHIP.md`, `CRATES.md`, `WORKFLOWS.md`) and the hard rules.

## Layout

- `crates/` — cargo workspace (`rivet-*`); module paths mirror Java packages.
- `tools/rivet-oracle` — differential-test harness; `tools/rivet-codegen` — data extraction/codegen (excluded from workspace).
- `working/` — Paper (source of truth) and Pumpkin (reference only, never copy). **Never committed; never push anything from it.**
- `MANIFEST.tsv` — the work queue (regenerate: `python3 scripts/analyze_graph.py`).

## Commands

- `cargo check --workspace` / `cargo test --workspace` (nextest preferred when available)
- `scripts/gate.sh` — the merge gate (fmt, clippy -Dwarnings, tests). No hosted CI: run this before merging any PR.
- Java oracle lives in `working/Paper` (gradle).

## Non-negotiables

- Translation fidelity per `PORTING.md`: wrapping arithmetic, exact RNG/`Mth` tables, no "improvements" during porting.
- Memory model per `OWNERSHIP.md`: arenas + IDs, sync tick thread; no `Arc<RwLock>` game state.
- Never weaken tests/fixtures to pass. No `git stash`/`git reset`/force-push.
