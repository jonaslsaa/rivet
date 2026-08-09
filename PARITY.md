# PARITY scoreboard

Differential parity vs the pinned Paper Java oracle. Refreshed by the `rivet-parity` tool (`cargo run -p rivet-parity -- --scoreboard`). _Run date: 2026-08-09_

| crate/check | inputs | matched | diverged | mismatched | date |
|---|---|---|---|---|---|
| rivet-nbt:idem | 432 | 432 | 0 | 0 | 2026-08-09 |
| rivet-nbt:nbt.decode | 432 | 432 | 0 | 0 | 2026-08-09 |
| rivet-nbt:nbt.encode | 467 | 30 | 437 | 0 | 2026-08-09 |
| rivet-nbt:snbt.parse | 602 | 602 | 0 | 0 | 2026-08-09 |

### M1 scenario gate (join/move/dwell)

The M1 terminal acceptance (issues #157/#160: keepalive survival + terminal M1 gate) adds three live-server scenario rows that this fixture-diff tool does not measure: they are exercised by `scripts/gate.sh` via `run-scenario` (exit 0 PASS / 1 FAIL / 3 UNVERIFIED), never by `rivet-parity`. They are listed here so the DoD's PARITY.md rows are present and explicit: two Paper-vs-Rivet differentials (`join --server both` and `move --server both`) plus the Rivet-only `dwell --server rivet` wall-clock keepalive-survival row.

| scenario | servers | comparison | gate.sh row |
|---|---|---|---|
| `join --server both` | Paper + Rivet | Paper-vs-Rivet play transcript | `run-scenario.sh join --server both` |
| `move --server both` | Paper + Rivet | Paper-vs-Rivet authoritative movement transcript | `run-scenario.sh move --server both` |
| `dwell --server rivet` | Rivet only | Rivet-only wall-clock keepalive survival past the 30 s kick limit (no Paper comparison) | `run-scenario.sh dwell --server rivet` |

### Divergences

`compound_key_order` is the documented insertion-order divergence (DECISIONS.md D12): Rust's `CompoundTag` is insertion-ordered, so hand-built compounds emit Rust's put sequence while Java emits fastutil hash order; read-back fixtures round-trip byte-for-byte. All such checks remain `ok` and are counted under `diverged`, never under `mismatched`.
