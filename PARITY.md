# PARITY scoreboard

Differential parity vs the pinned Paper Java oracle. Refreshed by the `rivet-parity` tool (`cargo run -p rivet-parity -- --scoreboard`). _Run date: 2026-08-07_

| crate/check | inputs | matched | diverged | mismatched | date |
|---|---|---|---|---|---|
| rivet-nbt:idem | 432 | 432 | 0 | 0 | 2026-08-07 |
| rivet-nbt:nbt.decode | 432 | 432 | 0 | 0 | 2026-08-07 |
| rivet-nbt:nbt.encode | 467 | 30 | 437 | 0 | 2026-08-07 |
| rivet-nbt:snbt.parse | 602 | 602 | 0 | 0 | 2026-08-07 |

### Divergences

`compound_key_order` is the documented insertion-order divergence (DECISIONS.md D12): Rust's `CompoundTag` is insertion-ordered, so hand-built compounds emit Rust's put sequence while Java emits fastutil hash order; read-back fixtures round-trip byte-for-byte. All such checks remain `ok` and are counted under `diverged`, never under `mismatched`.
