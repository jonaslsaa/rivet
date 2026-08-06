# PARITY scoreboard

Differential parity vs the pinned Paper Java oracle. Refreshed by the `rivet-parity` tool (`cargo run -p rivet-parity -- --scoreboard`). _Run date: 2026-08-06_

| crate/check | inputs | matched | diverged | mismatched | date |
|---|---|---|---|---|---|
| rivet-nbt:idem | 432 | 432 | 0 | 0 | 2026-08-06 |
| rivet-nbt:nbt.decode | 432 | 432 | 0 | 0 | 2026-08-06 |
| rivet-nbt:nbt.encode | 467 | 30 | 437 | 0 | 2026-08-06 |
| rivet-nbt:snbt.parse | 602 | 602 | 0 | 0 | 2026-08-06 |

### Divergences

`compound_key_order` is the documented HashMap-iteration-order divergence; all such checks remain `ok` and are counted under `diverged`, never under `mismatched`.
