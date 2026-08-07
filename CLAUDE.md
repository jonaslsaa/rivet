# Rivet

Faithful Rust port of the Paper Minecraft server (pinned MC 26.2), plus a JVM adapter for Java Paper plugins. **Read `GOAL.md` first** — it indexes all design docs (`DECISIONS.md`, `PORTING.md`, `OWNERSHIP.md`, `CRATES.md`, `WORKFLOWS.md`) and the hard rules.

## General
- Use your best judgement.
- Do not preserve backward compatibility. Remove obsolete paths instead of adding compatibility layers, fallbacks, or migrations. We have no users.
- Choose the simplest implementation that fully meets the current requirements. Avoid speculative abstractions, configuration, and indirection.
- Grow the system in layers. Start from the smallest version that works end to end, and add each new capability on top of a product that already works. Never trade a working product for unfinished complexity.
- Keep components modular and concerns clearly separated.
- Prefer established, well-maintained libraries when they reduce overall complexity or improve reliability. Do not reimplement common functionality without a clear reason.
- Lean on the dependencies already in the project before writing your own implementation or adding packages. Do not assume a library lacks a capability without checking its documentation and types.
- Make architectural decisions for the long term. Do not accept a stopgap that only works for now and is meant to be replaced later.

## Layout

- `crates/` — cargo workspace (`rivet-*`); module paths mirror Java packages.
- `tools/rivet-oracle` — differential-test harness; `tools/rivet-codegen` — data extraction/codegen (excluded from workspace).
- `working/` — Paper (source of truth) and Pumpkin (reference only, never copy). **Never committed; never push anything from it.**
- `MANIFEST.tsv` — the work queue (regenerate: `python3 scripts/analyze_graph.py --split-nbt --split-network --split-game --split-world --split-server`).

## Commands

- `cargo check --workspace` / `cargo test --workspace` (nextest preferred when available)
- `scripts/gate.sh` — the merge gate (fmt, clippy -Dwarnings, tests, then the oracle steps: `rivet-oracle verify` + byte-for-byte `rivet-parity` vs Paper). No hosted CI: run this before merging any PR. Oracle steps never silently skip — missing prereqs exit nonzero with UNVERIFIED; `--require-oracle` hard-fails instead.
- Java oracle lives in `working/Paper` (gradle).

## Non-negotiables

- Translation fidelity per `PORTING.md`: wrapping arithmetic, exact RNG/`Mth` tables, no "improvements" during porting.
- Memory model per `OWNERSHIP.md`: arenas + IDs, sync tick thread; no `Arc<RwLock>` game state.
- Never weaken tests/fixtures to pass. No `git stash`/`git reset`/force-push.
