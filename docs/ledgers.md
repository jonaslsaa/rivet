# Progress ledgers

Two committed TSV files answer the two questions the controller schedules by.
Both are written by tools, never by hand (ROADMAP.md 0.1).

| File | Question | Written by |
|---|---|---|
| `COVERAGE.tsv` | How much of Paper is ported? | `scripts/coverage_ledger.py` |
| `VERIFY.tsv` | How much of the port is checked against Paper? | `scripts/verify_ledger.py` |

`scripts/verify_debt.py` combines `VERIFY.tsv` with the tree into the
verification-debt table (VERIFICATION.md 4.1).

## COVERAGE.tsv

One row per Java class (per `.java` file) of the three Paper source roots that
`scripts/analyze_graph.py` scans, sorted by `java_path`.

- **bucket**: the ROADMAP 2.1 scope. A (vanilla runtime, DataConverter, Moonrise)
  and D (Paper server layer) are the translation target. B (datagen) is
  `n/a(datagen)`, C (DFU datafixers) is `deferred(dfu-audit)`, and E
  (CraftBukkit) and F (paper-api) are `adapter`. A B-bucket class on the
  Appendix A spine list (`PlacementUtils`, `Pools`, `FeatureUtils`) is a target.
- **hub**: `spine`, `adapter` or `dfu-audit` for the 200 Appendix A classes.
- **assigned_crate**: the D15 home, computed from the class graph. Paper
  server code and `net.minecraft.world.entity` go to `rivet-server`. A package of another
  crate moves to `rivet-server` when more than half of its classes reference a
  live type (world.entity, server, commands, ...). This moves block and item
  behaviour, block entities, menus, effects and damage, and keeps chunks,
  storage, levelgen, lighting and block-state tables in `rivet-world`.
  This column is a one-hop approximation and is not yet compile-sound. Taking
  the CRATES.md bottom-up order (core, util, serialization, nbt, text,
  brigadier, registry, protocol, world, server) and the script's own class
  references, 1,078 classes (3,818 references) in lower assigned crates still
  reference a class assigned to a higher one: world 607 classes, registry 170,
  protocol 128, util 119, text 37, core 7, nbt 7, brigadier 3. Those are the
  seams D15 resolves with dispatch/`LevelCtx` and the member-level split of
  mixed packages, which is a follow-up and not done here. The Batch 2 crate
  move must not consume `assigned_crate` as final.
- **rust_path**: the mirrored Rust file, or `-`. **other_paths** lists same-named
  files at non-mirrored paths (misplaced ports and duplicates) and, for a D15
  split class declared in `SPLITS`, its other half (`(pending)` while that file
  is not written yet).
- **method_total / method_covered**: distinct Java method names, and how many
  have a Rust `fn` of the same snake-case name in the home (or a getter
  without `get`, a pub field, an overload suffix, or a `// n/a: <reason>` note).
- **status**: `absent` (no mirrored file), `complete` (all methods covered, no
  `STUB`, `todo!()` or `RivetTodo`), otherwise `partial`.

`--show <class>` prints one row and the names of its missing methods.
`--summary` prints status counts and LOC per assigned crate. The heuristics
and their blind spots are listed in the script's header.

## VERIFY.tsv

One row per verification surface, with the VERIFICATION 3.1 columns plus
`fixtures` and `note`. Rows cover the `rivet-parity` checks, every oracle probe
script (fixture ownership from `tools/rivet-oracle/probes.tsv`), each recorded
oracle op (`op.<op>.recorded` offline, `op.<op>.live` against a Paper boot),
`rivet-oracle regen --check` (`regen.probes`), the oracle fixture kinds, the other committed goldens, the
`rivet-oracle verify` boots, the join capture, Anvil V1a, the codegen probes,
the scenario rows, `hash-diff` and `generated-full` (`INACTIVE`), and the Paper
JUnit census (`INACTIVE`, 0 of 9,653).

The structural columns (`id` through `tier`) are recomputed from the tree on
every run. The result columns (`status`, `matched`, `mismatched`,
`unavailable`, `last_verified_commit`, `note`) are carried over by row id. Only
two subcommands change them:

```sh
# L1 rows: results of the cargo tests in each row's consumer files.
python3 scripts/verify_ledger.py record-tests --commit <sha> <nextest log>...
# Any row: a tool or the gate records what it measured.
python3 scripts/verify_ledger.py record parity.nbt.decode --status PASS \
    --matched 432 --mismatched 0 --commit <sha>
```

`unavailable` counts the inputs of an op row that the Rust side declined
because it has no implementation or no data for what they exercise (an
unported recipe matcher, loot table, carver or structure, or a configured
feature outside the generated 170-entry closure, a data-scope gap rather than
an unported feature type). They are neither
matched nor mismatched, and an op row's `matched` must equal `corpus_n` minus
its excluded and its unavailable inputs. `-` means the row has no such count.

A new row starts as `UNVERIFIED` (or `INACTIVE` for a mechanism that never
runs). Nothing is seeded `PASS`. A `PASS` at L2 to L6 without a negative
control is rejected as malformed (VERIFICATION 2.4).

An L1 `PASS` means the cargo tests in the row's consumer files passed against
the committed Paper golden at `last_verified_commit`. It does not yet mean the
golden reproduces from Paper: that is `rivet-oracle regen --check`
(VERIFICATION item 2). Rows that need a Paper boot (parity, oracle verify,
capture, scenarios, codegen probes) stay `UNVERIFIED` until the gate records
them with `record`.

`rust_globs` lists the files a row verifies directly first (a production file
that reads the fixture, or the module that declares a test-only consumer),
then the files of the same crate that its tests import. The debt script
spends a row's capped credit in that order.

## Debt metric

```sh
python3 scripts/verify_debt.py                      # table + top-20 V0 files
python3 scripts/verify_debt.py --tsv batch-N.tsv    # keep a batch baseline
python3 scripts/verify_debt.py --baseline batch-N.tsv  # adds debt_delta
```

Each production line of `crates/*/src` (tests, test-only modules, generated
code and codegen-headed tables excluded) is V2 (a PASS row at L2 to L6 with a
negative control), V1 (a PASS L1 golden row, reported as `thin_loc`) or V0
(`debt_loc`). A row credits at most K x corpus_n LOC, with K = 50, so a
12-record fixture cannot verify a 5,000-line module.

Thresholds (VERIFICATION 4.2) are reported in the `flag` column, not enforced:

| Condition | Flag |
|---|---|
| subsystem `debt_ratio` > 0.30 and `debt_loc` > 10,000 | `pause`: next runs there are verification work |
| global `debt_ratio` > 0.35 | `global-pause` on the TOTAL line |
| subsystem `debt_delta` > 0 for two batches | read from `--baseline` runs |
| steady-state target | global `debt_ratio` <= 0.15 |

## Gate hook

`scripts/ledger_check.sh` checks that both ledgers regenerate byte-identically
from the tree and are well-formed, and runs the ledger tools' tests (about one
minute of CPU, offline). It is not wired into `scripts/gate.sh`: nothing calls
it yet. This is the one-line hook to add to the fast gate:

```sh
echo "==> ledger check (scripts/ledger_check.sh)"; scripts/ledger_check.sh
```

When it fails because a ledger is stale, regenerate and commit:
`python3 scripts/coverage_ledger.py && python3 scripts/verify_ledger.py`.
