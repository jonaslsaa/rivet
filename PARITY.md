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

### Plugin corpus (planned)

Per-plugin Tier 1 corpus rows belong here (`PLUGINS.md`). This file is rebuilt wholesale on every `rivet-parity --scoreboard` run, so the corpus table is emitted by the generator — never hand-edited into the file, which the next run would silently overwrite. **Planned stub**: no corpus rows exist yet and none are fabricated. Planned row schema (emitted by the generator; authoritative source: `plugin_corpus_section` in `tools/rivet-parity/src/main.rs` — distinct from the corpus manifest schema in `PLUGINS.md`):

| plugin | category | paper_baseline | rivet | status |
|---|---|---|---|---|
| `<pinned version + sha256>` | `<behavior category>` | `recorded` | `pending` | `VERIFIED` / `FAILED` / `UNVERIFIED` |

`paper_baseline`/`rivet` start as `recorded`/`pending` (or `n/a`); a row appears only once the sweep produced it. `status` follows the oracle exit-code contract (scripts/gate.sh): `VERIFIED` (0), `FAILED` (1), `UNVERIFIED` (3).
