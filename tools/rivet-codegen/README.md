# rivet-codegen

Vanilla data extraction + registry codegen for Rivet (epic #8).

Per `PORTING.md`: registries/data-driven content are **generated, not
hand-ported**. This tool reads the real Paper 26.2 data and emits Rust source
for `crates/rivet-registry`. It is excluded from the cargo workspace
(`Cargo.toml` `exclude`), so it is free to shell out to the JVM and to use
`anyhow`/`serde`.

## What it does

- **`extract`** — pulls the block registry + block-state properties out of the
  Paper 26.2 bundler jar and writes `data/block_states.json`.
- **`generate`** — reads that JSON and emits the registry tables directly into
  `crates/rivet-registry/src/generated/` (a `BlockId` registry + a block-state
  property enum structure).
- **`mth-gen`** — regenerates the Mth tables + golden tests
  (`crates/rivet-util/src/mth_sin_table.rs`, `mth_atan_tables.rs`,
  `mth_golden_tests.rs`) from the real Paper `Mth` class. These files are
  `GENERATED — do not hand-edit`; this is the checked-in generator that makes
  regeneration possible. Idempotent: over the current committed files it
  reproduces them byte-for-byte (`git diff` stays clean).
- **`reports`** — runs the vanilla `net.minecraft.data.Main --reports` datagen
  against the materialized Paper 26.2 server jar and pins the canonical
  `packets.json` / `registries.json` / `blocks.json` reports (with provenance)
  under `data/reports/`.

```
rivet-codegen extract  [--bundler <path>] [--output <path>]
rivet-codegen generate [--input <path>]  [--output <dir>]
rivet-codegen mth-gen  [--bundler <path>] [--output <dir>]
rivet-codegen reports  [--jar <path>] [--output <dir>] [--verify]
```

## How to run

Requirements: a JVM (`java` + `javac` on `PATH`, or `JAVA_HOME` set), `unzip`,
and `rustfmt` (for `mth-gen`). The default bundler path is
`working/Paper/paper-server/build/libs/paper-bundler-26.2.local-SNAPSHOT.jar`
(the artifact of a Paper `build` — run `./gradlew :paper-server:build` in
`working/Paper` if it is missing). You cannot build that jar in this read-only
checkout; the tool needs a jar produced elsewhere.

```
cargo build --release
target/release/rivet-codegen extract          # -> data/block_states.json
target/release/rivet-codegen generate         # -> crates/rivet-registry/src/generated/{mod.rs, blocks.rs, block_properties.rs}
target/release/rivet-codegen mth-gen          # -> crates/rivet-util/src/mth_{sin_table,atan_tables,golden_tests}.rs
target/release/rivet-codegen reports          # -> data/reports/{packets,registries,blocks}.json + manifest.json
```

`extract` and `mth-gen` cache the unpacked classpath in `.cache/` (gitignored)
so reruns are fast. Pass `--bundler` to point at a different jar.

## How the Mth tables + golden tests are generated (`mth-gen`)

`crates/rivet-util/src/mth_sin_table.rs` / `mth_atan_tables.rs` /
`mth_golden_tests.rs` are marked `GENERATED — do not hand-edit` but had no
checked-in generator (regeneration was impossible). `mth-gen` is that
generator. The Mth tables and every golden expected value are computed by the
**real compiled Paper `Mth` class** — nothing is hand-copied:

1. unpack the bundler classpath (same `extract` machinery);
2. compile `src/java/MthGen.java` against it. `MthGen.java` is itself generated
   by `scripts/gen_mth_gen.py` from `data/mth_vectors.tsv` (the `(lhs => rhs)`
   pairs extracted from the committed golden file) — it calls the exact Java
   overload each Rust fn mirrors and prints each result as the committed Rust
   literal;
3. `MthGen` prints the `SIN`/`ASIN_TAB`/`COS_TAB` arrays and the 1156 vectors;
4. `mth_gen.rs` substitutes those values into `data/mth_golden_skeleton.rs`
   (the committed golden file with every expected `rhs` replaced by a `@@N@@`
   placeholder) and emits the two table files, then pipes everything through
   `rustfmt` — the committed files are rustfmt-idempotent, so the output
   matches them byte-for-byte.

Regeneration is **idempotent**: `git diff` stays clean when run over the
current committed files. Regenerate after bumping the Paper version or
correcting a golden expectation; a value that diverges from Java shows up as a
`git diff` hunk to review.

### Authoring a change to the golden file

1. Edit `crates/rivet-util/src/mth_golden_tests.rs` directly (add/change an
   `assert_eq!(lhs, rhs)`), or fix a wrong committed `rhs`.
2. Re-run `scripts/mth_skeletonize.py` to rebuild `data/mth_golden_skeleton.rs`
   and `scripts/mth_vectors.py` to rebuild `data/mth_vectors.tsv`.
3. Re-run `scripts/gen_mth_gen.py` to rebuild `src/java/MthGen.java`, then
   `rivet-codegen mth-gen` to regenerate. The regenerated file must match your
   edit (a mismatch means the Java oracle computes something different).

## The `reports` subcommand

`reports` runs the **real vanilla** `net.minecraft.data.Main --reports` datagen
against the materialized Paper 26.2 server jar and copies the three canonical
report artifacts byte-for-byte into `data/reports/`:

- `packets.json` — every protocol/flow packet name -> `protocol_id`, in the
  exact `addPacket` registration order of the `*Protocols.TEMPLATE` definitions
  (`PacketReport`). This is the canonical enumeration `IdDispatchCodec` assigns
  ids from, so it is the oracle for `rivet-protocol`'s packet-ID tables.
- `registries.json` — every `BuiltInRegistries` registry with numeric protocol
  ids (`RegistryDumpReport`; `Bootstrap.bootStrap()` runs in `DataGenerator`
  static init, so this is a fully-populated real-server dump).
- `blocks.json` — per-block ordered state properties, all state ids, the default
  marker, and the `BlockTypes.CODEC` definition (`BlockListReport`).

No extraction logic is invented — these are the same generators vanilla's own
`Main --reports` ships, so the fixtures can never drift from upstream's canonical
data. The datagen output is deterministic (verified byte-identical across
independent runs), so the committed files are the no-drift baseline.

Source pinning is recorded in `data/reports/manifest.json`: the source jar's
sha256, the Paper git commit it was built from, and the MC/protocol/world
versions read straight out of the jar's `version.json`. The jar path is stored
repo-relative (machine-independent); the jar identity is the sha256.

The source jar is the oracle's materialization at
`tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar` (gitignored; absent
from committed checkouts). Resolve it with `--jar <path>`, or
`RIVET_CODEGEN_JAR=/path/to/paper-26.2.jar` (mirrors the oracle's
`RIVET_ORACLE_JAR`), or boot the oracle once (`cargo run -p rivet-oracle --
verify`) to materialize it at the default location.

`reports --verify` is the no-drift gate: it runs the datagen **twice** fresh
(proving cross-run determinism), requires both runs byte-identical to the
committed fixtures, and re-checks the committed files against the manifest's
recorded hashes. A changed source jar whose reports are still byte-identical
prints a provenance note rather than failing — the fixtures are canonical, but
`reports` should be re-run to refresh the manifest.

## Data source decision

The block registry is **not stored as JSON anywhere** in the jar or datagen
resources — it exists only as compiled Java (a static registry built in
`BuiltInRegistries` / `Blocks`). So `extract` runs a small Java helper
(`src/java/BlockDataExtractor.java`) inside the full server classpath:

1. unpacks the bundler jar's `META-INF/versions/<mc>/paper-<mc>.jar` + every
   `META-INF/libraries/**/*.jar` into `.cache/`;
2. compiles the helper against that classpath;
3. calls `Bootstrap.bootStrap()`, then iterates `BuiltInRegistries.BLOCK`
   and writes the registry to JSON.

This reads the *actual* compiled Paper server — nothing is hardcoded. Block
names/ids come from `BuiltInRegistries.BLOCK`; property values come from
`Property.getName(value)`, which for `EnumProperty` is the SNBT/blockstate
serialized name (`getSerializedName()`), not the enum name.

## JSON shape (`data/block_states.json`)

```jsonc
{
  "minecraft_version": "26.2",
  "blocks": [
    { "id": 0, "name": "minecraft:air", "properties": [] },
    {
      "id": 199,
      "name": "minecraft:creaking_heart",
      "properties": [
        { "name": "axis", "values": ["x", "y", "z"] },
        { "name": "creaking_heart_state", "values": ["uprooted", "dormant", "awake"] },
        { "name": "natural", "values": ["true", "false"] }
      ]
    }
  ]
}
```

`id` is the numeric vanilla block id; properties preserve the declaration
order and `values` preserve `getPossibleValues()` order — that ordering defines
the block-state index layout and must not be sorted during codegen.

## Generated output

Wired into `crates/rivet-registry/src/generated/`, committed, and gated behind
the crate's `"blocks"` cargo feature:

- `mod.rs` — declares the two generated submodules.
- `blocks.rs` — `BlockId(pub u16)`, a `phf::Map<&'static str, u16>`
  (`BLOCK_BY_NAME`), an id-indexed `BLOCK_BY_ID` array, and lookup methods.
- `block_properties.rs` — `BlockPropertyId`, an enum with one variant per
  distinct `(name, values)` property type, plus the value tables and a per-block
  `BLOCK_STATE_SHAPES` table (ordered property ids by block id).

The generator asserts block ids are contiguous `0..n` (true for vanilla 26.2).
After regenerating, run `cargo fmt -p rivet-registry` (the phf macro output is
not format-clean as emitted) and `cargo check -p rivet-registry --features blocks`.
The codegen test `generated_output_matches_committed` enforces the golden
no-drift invariant: it regenerates to a temp dir, rustfmts the temp copy, and
asserts byte-equality with the committed `src/generated/` — without touching
repository source.
