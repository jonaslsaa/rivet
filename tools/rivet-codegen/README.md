# rivet-codegen

Vanilla data extraction + registry codegen for Rivet (epic #8).

Per `PORTING.md`: registries/data-driven content are **generated, not
hand-ported**. This tool reads the real Paper 26.2 data and emits Rust source
for `crates/rivet-registry` and `crates/rivet-protocol`. It is excluded from
the cargo workspace (`Cargo.toml` `exclude`), so it is free to shell out to the
JVM and to use `anyhow`/`serde`.

## What it does

- **`extract`** — pulls the block registry + block-state properties out of the
  Paper 26.2 bundler jar and writes `data/block_states.json`.
- **`generate`** — reads that JSON and emits the registry tables directly into
  `crates/rivet-registry/src/generated/` (a `BlockId` registry + a block-state
  property enum structure), consumes `data/reports/registries.json` to emit the
  static-builtin registry tables into the same `src/generated/` (see
  `registries` below), consumes `data/reports/blocks.json` to emit the
  block-state global-id table (see `block_states.rs`, issue #154), consumes
  `data/block_behaviors.json` to emit the per-`StateId` behavior table (see
  `block_behaviors.rs`, issue #228), and consumes
  `data/reports/packets.json` to emit the packet-ID tables into
  `crates/rivet-protocol/src/generated/`.
- **`registries`** — reads the pinned vanilla `data/reports/registries.json`
  (`RegistryDumpReport`) and emits the id-indexed static-builtin tables for the
  ordered registries M1 touches into
  `crates/rivet-registry/src/generated/registries.rs`. This is the same
  report-driven half `generate` runs; the standalone subcommand regenerates just
  those tables.
- **`mth-gen`** — regenerates the Mth tables + golden tests
  (`crates/rivet-util/src/mth_sin_table.rs`, `mth_atan_tables.rs`,
  `mth_cos_tab_{x86_64,aarch64}.rs`, `mth_golden_tests.rs`) from the real Paper
  `Mth` class. These files are `GENERATED — do not hand-edit`; this is the
  checked-in generator that makes regeneration possible. Idempotent per arch:
  over the current committed files it reproduces them byte-for-byte on the
  matching host (`git diff` stays clean). `COS_TAB` is architecture-dependent
  (D14) and only the **host arch's** module is written: on x86_64 (the primary
  release-gate target) this writes `mth_cos_tab_x86_64.rs`; the committed
  `mth_cos_tab_aarch64.rs` is provenance-checked — generated and verified on
  native aarch64, never substituted or silently overwritten by an x86_64 run
  (an aarch64 host regenerates it idempotently). The golden expectation values
  are NaN-canonicalized via Java `floatToIntBits`/`doubleToLongBits`
  (`bitexact_f32`/`bitexact_f64` in the test), because hardware-created NaN
  sign/payload is architecture-undefined; every non-NaN value stays bit-exact.
- **`extract-biomes-tags`** — compiles and runs `java/BiomeTagExtractor.java`
  against the real Paper jar, reproducing `WorldLoader.load`
  (vanilla pack -> STATIC layer -> `TagLoader.loadTagsForExistingRegistries`
  -> `RegistryDataLoader.load(WORLDGEN_REGISTRIES)` ->
  `serializeTagsToNetwork`), and writes the deterministic biome id table + tag
  network-serialization content to `data/biomes_tags.json` (issue #49). The
  helper writes a `probe` object with the live-load counts into the fixture
  because `Bootstrap.wrapStreams()` redirects `System.out` into the logger.
- **`probe-biomes-tags`** — re-runs `BiomeTagExtractor` against the real Paper
  jar and requires byte-identity with the committed `data/biomes_tags.json`
  plus the anchor counts (66 biomes / 15 tag-carrying registries / 697 tags).
  This is the live half of the fixture-pinned conformance test in `generate`'s
  `biomes_tags.rs` tests.
- **`probe-block-states`** — compiles and runs `java/GlobalPaletteProbe.java`
  against the real Paper jar and cross-checks the emitted block-state global-id
  table (issue #154): a canonical digest over all 32,366 ids, block names,
  default markers, and serialized properties must match committed
  `data/reports/blocks.json`; size/range/default invariants and representative
  anchor ids remain additional diagnostics. This is the live half of the
  fixture-pinned conformance test in `generate`'s `block_states.rs` tests.
- **`extract-block-behaviors`** — compiles and runs `java/BlockBehaviourProbe.java`
  against the real Paper jar, boots `Bootstrap` + the `Blocks` static init
  (which `initCache`s every state against `EmptyBlockGetter`), evaluates all
  32,366 states' worldgen/heightmap/lighting accessors, and writes the
  run-length-encoded per-`StateId` behavior table to `data/block_behaviors.json`
  (+ provenance manifest), issue #228.
- **`probe-block-behaviors`** — re-runs `BlockBehaviourProbe` against the real
  Paper jar and requires byte-identity with the committed `data/block_behaviors.json`,
  that every probe anchor key (state_count, run_count, and the representative
  air/stone/water/lava/oak_leaves/glass/torch words) is present, and that
  state_count is pinned to 32366. The anchor *values* are pinned independently
  by the `rivet-registry` `BlockState` behavior-decode tests
  (`behavior_queries_match_probe_anchors` /
  `behavior_word_fields_match_paper_semantics`). This is the live half of the
  fixture-pinned conformance tests in `generate`'s `block_behaviors.rs` tests.
- **`extract-worldgen`** — compiles and runs `java/WorldgenDataExtractor.java`
  against the real Paper jar, reproducing `WorldLoader.load` (vanilla pack ->
  STATIC layer -> `TagLoader.loadTagsForExistingRegistries` ->
  `RegistryDataLoader.load(WORLDGEN_REGISTRIES)`), and writes the deterministic
  worldgen noise registry, per-biome climate configuration, and multi-noise
  biome-source preset parameter points to `data/worldgen.json` (issue #354).
  The three surfaces are datapack-loaded or hardcoded in Paper:
  `minecraft:worldgen/noise` (`NormalNoise.NoiseParameters`, dense `0..n`),
  `minecraft:worldgen/biome` (`Biome.ClimateSettings` per biome), and
  `MultiNoiseBiomeSourceParameterList.knownPresets()` (overworld +
  `OverworldBiomeBuilder` / nether inline list). Parameter spans are the
  quantized longs (`Climate.quantizeCoord`, `(long)(coord * 10000.0F)`) exactly
  as stored in the runtime `Climate.ParameterPoint`, so the generated Rust table
  reconstructs the exact values with no float round-trip.
- **`probe-worldgen`** — re-runs `WorldgenDataExtractor` against the real Paper
  jar and requires byte-identity with the committed `data/worldgen.json` plus
  the anchor counts (63 noises / 66 biome climates / 2 presets, nether 5 points,
  overworld 7594 points). This is the live half of the fixture-pinned
  conformance test in `generate`'s `worldgen.rs` tests.
- **`extract-feature-data`** — compiles and runs
  `java/WorldgenFeatureDataExtractor.java` against the real Paper jar, sampling
  the seed-42 biome source over the committed FEATURES grid's decoration
  context (chunks 1..6, full Y range) to materialize the reachable biome set,
  each biome's full `BiomeGenerationSettings` (carvers + per-step placed-feature
  lists), and the transitive placed/configured feature closure as
  `RegistryOps`-encoded JSON into `data/feature_data.json` (+ provenance
  manifest), the seed-42 FEATURES checkpoint data foundation. Self-validates
  the fresh capture against the same contract the probe enforces.
- **`probe-feature-data`** — re-runs `WorldgenFeatureDataExtractor` against the
  real Paper jar and requires byte-identity with the committed
  `data/feature_data.json` plus the anchor counts (5 reachable biomes / 72
  placed / 70 configured), the pinned Paper provenance, and non-vacuity (the
  deep `lush_caves` biome AND a surface biome must be reachable). This is the
  live half of the fixture-pinned conformance gate for the FEATURES checkpoint.
- **`reports`** — runs the vanilla `net.minecraft.data.Main --reports` datagen
  against the materialized Paper 26.2 server jar and pins the canonical
  `packets.json` / `registries.json` / `blocks.json` reports (with provenance)
  under `data/reports/`.

```
rivet-codegen extract    [--bundler <path>] [--output <path>]
rivet-codegen generate   [--input <path>]  [--output <dir>]
                         [--packets <path>] [--packets-output <dir>]
rivet-codegen registries [--input <path>]  [--output <dir>]
rivet-codegen mth-gen    [--bundler <path>] [--output <dir>]
rivet-codegen extract-biomes-tags [--bundler <path>] [--output <path>]
rivet-codegen probe-biomes-tags  [--bundler <path>]
rivet-codegen probe-block-states [--bundler <path>]
rivet-codegen extract-block-behaviors [--bundler <path>] [--output <path>]
rivet-codegen probe-block-behaviors  [--bundler <path>]
rivet-codegen extract-worldgen [--bundler <path>] [--output <path>]
rivet-codegen probe-worldgen  [--bundler <path>]
rivet-codegen extract-feature-data [--bundler <path>] [--output <path>]
rivet-codegen probe-feature-data  [--bundler <path>]
rivet-codegen reports    [--jar <path>] [--output <dir>] [--verify]
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
target/release/rivet-codegen generate         # -> crates/rivet-registry/src/generated/{mod.rs, blocks.rs, block_properties.rs, block_behaviors.rs, block_states.rs, registries.rs, biomes.rs, tags.rs, synchronized.rs, registry_data.rs, worldgen.rs} + crates/rivet-protocol/src/generated/
target/release/rivet-codegen registries       # -> crates/rivet-registry/src/generated/registries.rs (report-driven half only)
target/release/rivet-codegen mth-gen          # -> crates/rivet-util/src/mth_{sin_table,atan_tables,golden_tests}.rs
target/release/rivet-codegen extract-biomes-tags  # -> data/biomes_tags.json + manifest
target/release/rivet-codegen probe-biomes-tags    # verify biome ids + tag network content against live Paper
target/release/rivet-codegen probe-block-states   # verify the emitted block-state global ids against live Paper
target/release/rivet-codegen extract-block-behaviors  # -> data/block_behaviors.json + manifest
target/release/rivet-codegen probe-block-behaviors    # verify the per-StateId behavior table against live Paper
target/release/rivet-codegen extract-worldgen         # -> data/worldgen.json + manifest
target/release/rivet-codegen probe-worldgen           # verify the worldgen noise/biome/preset data against live Paper
target/release/rivet-codegen extract-feature-data     # -> data/feature_data.json + manifest
target/release/rivet-codegen probe-feature-data       # verify the seed-42 feature data against live Paper
target/release/rivet-codegen reports          # -> data/reports/{packets,registries,blocks}.json + manifest.json
```

`generate` emits the block registry (from `data/block_states.json`), the
static-builtin registry tables (from `data/reports/registries.json`, the
`RegistryDumpReport` fixture), the block-state global-id table (from
`data/reports/blocks.json`, the `BlockListReport` fixture), the biome id +
tag network tables (from `data/biomes_tags.json`), the per-`StateId` behavior
table (from `data/block_behaviors.json`, the `BlockBehaviourProbe` fixture),
the synchronized configuration-registry element tables (from
`data/synchronized_registries.json`), the pre-baked registry NBT payloads
(from `data/registry_data.json`, the canonical join capture), the worldgen
noise/biome-climate/preset tables (from `data/worldgen.json`, the
`WorldgenDataExtractor` fixture), and the packet-ID tables (from
`data/reports/packets.json`, the `PacketReport` fixture).
Regenerate all of them with a single `generate` run; `--input`/`--output`
control the block half, `--packets`/`--packets-output` control the packet half.
(The `registries` subcommand emits just the report-driven registry tables.)

`extract` and `mth-gen` cache the unpacked classpath in `.cache/` (gitignored)
so reruns are fast. Pass `--bundler` to point at a different jar.

## How the Mth tables + golden tests are generated (`mth-gen`)

`crates/rivet-util/src/mth_sin_table.rs` / `mth_atan_tables.rs` /
`mth_cos_tab_{x86_64,aarch64}.rs` / `mth_golden_tests.rs` are marked
`GENERATED — do not hand-edit` but had no checked-in generator (regeneration
was impossible). `mth-gen` is that generator. The Mth tables and every golden
expected value are computed by the **real compiled Paper `Mth` class** —
nothing is hand-copied:

1. unpack the bundler classpath (same `extract` machinery);
2. compile `src/java/MthGen.java` against it. `MthGen.java` is itself generated
   by `scripts/gen_mth_gen.py` from `data/mth_vectors.tsv` (the `(lhs => rhs)`
   pairs extracted from the committed golden file) — it calls the exact Java
   overload each Rust fn mirrors and prints each result as the committed Rust
   literal. Float/double results go through `Float.floatToIntBits`/
   `Double.doubleToLongBits` (not the `Raw` variants) so NaN sign/payload —
   which is hardware-architecture-undefined (D14) — is canonicalized, while
   every non-NaN value is bit-exact and identical to the Raw forms;
3. `MthGen` prints the `SIN`/`ASIN_TAB`/`COS_TAB` arrays and the 1156 vectors;
4. `mth_gen.rs` substitutes those values into `data/mth_golden_skeleton.rs`
   (the committed golden file with every expected `rhs` replaced by a `@@N@@`
   placeholder) and emits the table files, then pipes everything through
   `rustfmt` — the committed files are rustfmt-idempotent, so the output
   matches them byte-for-byte. The golden compares float/double bits via the
   `bitexact_f32`/`bitexact_f64` helpers, which canonicalize NaNs to the
   canonical bit-pattern exactly as `floatToIntBits`/`doubleToLongBits` do.

Regeneration is **idempotent per arch**: `git diff` stays clean when run over
the current committed files on the matching host. `COS_TAB` is
architecture-dependent (D14) — Paper builds it at class-init with
`java.lang.Math.cos`/`asin`, whose HotSpot intrinsics differ by up to 1 ULP
between x86_64 and aarch64. `mth-gen` therefore writes only the **host arch's**
`COS_TAB` module:
  - on **x86_64** (primary release-gate target) it writes
    `mth_cos_tab_x86_64.rs` and leaves `mth_cos_tab_aarch64.rs` untouched —
    the committed aarch64 variant is provenance-checked (generated + verified
    on native aarch64 Paper) and must never be substituted or silently
    overwritten;
  - on **aarch64** it regenerates `mth_cos_tab_aarch64.rs` idempotently from
    native Paper.
`ASIN_TAB` is identical on both supported arches and lives in
`mth_atan_tables.rs`, which re-exports the correctly arch-selected `COS_TAB`
via `cfg(target_arch)`.

### Arch-selected golden expected values (D14)

Because `atan2` reads `COS_TAB` and that table differs by 1 ULP between x86_64
and aarch64, the four golden rows that happen to read the differing `COS_TAB[181]`
slot (`atan2(1,1)`, `atan2(-1,1)`, `atan2(0.5,0.5)`, `atan2(-0.5,0.5)`) have
**arch-selected expected values**: their `rhs` is a `#[cfg(target_arch)]` block
with an `aarch64` provenance-committed literal, an `x86_64` branch holding the
live oracle value, and a `compile_error!` fall-through so unsupported
architectures fail closed. Every other golden `rhs` stays a single plain
literal (bit-exact on both arches).

The skeleton keeps that block with an `@@N@@` placeholder in the `x86_64` branch;
`mth-gen` fills it with the live x86_64 oracle value. The `aarch64` literal is
provenance-checked at generation time: `mth-gen` computes the four `atan2`
results with *faithful atan2 arithmetic* over the **committed aarch64 `COS_TAB`**
(+ arch-independent `ASIN_TAB`) and hard-fails if the rendered golden's aarch64
literal disagrees — so a stale aarch64 table or literal is caught instead of
silently propagating.

Regenerate after bumping the Paper version or correcting a golden expectation;
a value that diverges from Java shows up as a `git diff` hunk to review.

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
repo-relative (machine-independent); the SHA records the historical capture's
physical identity, while live rebuilds are authorized by the pinned semantic
contract and mandatory complete no-drift comparison below.

### Why the jar SHA is advisory, not authoritative (issue #670)

`gate.sh` could not complete on a clean machine because the live-probe
provenance boundary required the materialized Paper server jar's raw SHA-256 to
equal the historical fixture SHA exactly. But Paper jar bytes are **not
reproducible**: clean builds of the exact pinned Paper commit
`0a993450f129c4942c2a9ed45ba047412b4667cf` produced several different jar
hashes (`e94fba6b…`, `88ccec84…`), differing only in `javac`-generated synthetic
local-variable debug names in one `EntityCommand.class`; executable bytecode and
the Paper manifest commit are unchanged, and the historical `e1a027e9…` capture
bytes are not archived.

So the physical jar SHA is treated as **recorded/advisory cross-build
provenance** for the live boundary, not a permanent identity. The durable,
deterministic contract that actually authorizes a fresh build is:

- the live jar's `Git-Commit` is a validated 7–40 hex prefix of the pinned Paper commit;
- a clean exact `working/Paper` checkout (full HEAD == the pin, not dirty);
- the MC/protocol/world versions read from the jar's `version.json`;
- the deterministic live probe/datagen output being byte-identical to the
  committed fixtures (including the twin-run no-drift gate).

Committed manifests are still validated strictly (a committed capture may only
name one of the pinned capture SHAs + the pinned commit + versions), so a
committed manifest can never self-assert an arbitrary source; only a *freshly
built* pinned-commit jar may proceed — on its deterministic semantic identity —
to the live no-drift comparison that authorizes it. A jar SHA that differs from
the committed capture prints a clear `note:` as an advisory, and the semantic
comparison still runs to completion.

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

- `mod.rs` — declares the generated submodules: `biomes`, `block_behaviors`,
  `block_properties`, `block_states`, `blocks`, `registries`, `registry_data`,
  `synchronized`, `tags`, `worldgen`.
- `blocks.rs` — `BlockId(pub u16)`, a `phf::Map<&'static str, u16>`
  (`BLOCK_BY_NAME`), an id-indexed `BLOCK_BY_ID` array, and lookup methods.
- `block_properties.rs` — `BlockPropertyId`, an enum with one variant per
  distinct `(name, values)` property type, plus the value tables and a per-block
  `BLOCK_STATE_SHAPES` table (ordered property ids by block id).
- `block_states.rs` — the dense global block-state ids in Paper's
  `Block.BLOCK_STATE_REGISTRY` order (`StateId(pub u16)`, `BLOCK_STATE_COUNT` =
  32366, `GLOBAL_PALETTE_BITS`, `BLOCK_STATE_BASES`, and
  `shape_of`/`state_id`/`block_of`), the wire global-palette index (issue #154).
- `block_behaviors.rs` — the run-length-encoded per-`StateId` behavior table
  (`BLOCK_BEHAVIOR_RUNS`, the flag/shift/mask constants, and `behavior_of`),
  issue #228.
- `registries.rs` — the report-driven static-builtin tables (see below).
- `registry_data.rs` — the pre-baked registry NBT payloads (`SYNCHRONIZED_NBT`,
  from `data/registry_data.json`, the canonical join capture).
- `synchronized.rs` — the synchronized configuration-registry element tables
  (`SYNCHRONIZED_REGISTRIES`, from `data/synchronized_registries.json`).
- `biomes.rs` — the biome id/name table (`BIOME_BY_NAME`/`BIOME_BY_ID`, dense
  `0..n`, `BIOME_COUNT`), the element table a `PalettedContainer<Holder<Biome>>`
  global palette indexes into (issue #49).
- `tags.rs` — the tag network-serialization content: every registry on the
  `ClientboundUpdateTagsPacket` wire, each mapped to tag-location -> element
  names in the tag file's value order (issue #49). For the 7 datapack
  registries the report cannot cover it also emits the element table; the other
  8 resolve through the existing `blocks.rs`/`registries.rs`/`biomes.rs`
  surfaces. See "Biome + tag tables" below.
- `worldgen.rs` — the worldgen noise registry, per-biome climate
  configuration, and multi-noise biome-source preset parameter points
  (`NOISE_BY_NAME`/`NOISE_BY_ID`/`NOISE_AMPLITUDES`, `BIOME_CLIMATE`/
  `BIOME_CLIMATE_BY_ID`, and `NETHER_BIOME_SOURCE_PARAMETER_POINTS`/
  `OVERWORLD_BIOME_SOURCE_PARAMETER_POINTS`, from `data/worldgen.json`, the
  `WorldgenDataExtractor` fixture, issue #354). Parameter spans are the
  quantized longs (`Climate.quantizeCoord`) exactly as stored in the runtime
  `Climate.ParameterPoint`.

The generator asserts block ids are contiguous `0..n` (true for vanilla 26.2).
After regenerating, run `cargo fmt -p rivet-registry` (the phf macro output is
not format-clean as emitted) and `cargo check -p rivet-registry --features blocks`.
The codegen test `generated_output_matches_committed` enforces the golden
no-drift invariant: it regenerates to a temp dir, rustfmts the temp copy, and
asserts byte-equality with the committed `src/generated/` — without touching
repository source.

## Static-builtin registry tables (`registries`)

Both `generate` and the standalone `registries` subcommand consume the pinned
`data/reports/registries.json` (the vanilla `RegistryDumpReport`, issue #124
phase F) and emit `crates/rivet-registry/src/generated/registries.rs`, committed
and gated behind the crate's `"blocks"` cargo feature. The same validated
surface also emits the dependency-free `generated/block_entity_types.rs`,
which is available without that feature for loaded chunks and packet codecs.

The report covers only `BuiltInRegistries.REGISTRY` — the 95 static registries,
each element mapped to its `protocol_id` (the `MappedRegistry.byId` insertion
index). The generator emits tables for the **minimal** subset whose element ids
are on current wire or loaded-chunk paths: `minecraft:item`,
`minecraft:entity_type`, `minecraft:block_entity_type`,
`minecraft:data_component_type` (ItemStack / `ClientboundAddEntity` /
`DataComponentPatch`), and `minecraft:fluid`, `minecraft:game_event`,
`minecraft:potion`, `minecraft:point_of_interest_type` (static registries whose
vanilla tags ride the config-sync `UpdateTags` payload). Datapack-loaded
registries (`dimension_type`, `biome`, `worldgen/*`, …) are not in the report
and are not emitted. `minecraft:block` is on the wire but is already covered by
the extract-driven `blocks.rs`; a drift test asserts the two captures agree.

Each surface is a dense `0..n` bijection — element id == holder id == network id
== insertion index (OWNERSHIP.md §Registries):

- `{PREFIX}_BY_NAME` — a `phf::Map<&'static str, u16>` (name -> id), mirroring
  `BLOCK_BY_NAME`.
- `{PREFIX}_BY_ID` — an id-indexed `&[&str]` (id == index), mirroring
  `BLOCK_BY_ID`. The block-entity surface additionally emits a dependency-free
  id-indexed table and exact name lookup used by `BlockEntityType`.
- `{PREFIX}_DEFAULT` — the `DefaultedRegistry` fold (`&str` name) for the four
  defaulted surfaces (item/entity_type/fluid/game_event); `Option<&str> = None`
  for the plain registries.

The JSON keys are alphabetically sorted by GsonHelper's stable writer, so the
generator never trusts key order — it re-orders entries by `protocol_id` to
recover Java registration order (deterministic, byte-idempotent output). The
`protocol_id` u16 space is validated strictly: generation fails on sparse or
non-contiguous ids, duplicate ids, duplicate names, non-integer/negative/
overflowing ids, a `default` naming no element, unexpected registry/entry
fields, malformed `Identifier` names, and Rust-identifier collisions among
element names. The fixture is linked to `data/reports/manifest.json` by sha256 —
a stale fixture aborts generation.

`crates/rivet-registry/src/static_builtin_tests.rs` (feature-gated `blocks` +
`cfg(test)`, outside `src/generated/` so it does not collide with the golden
drift test) asserts every emitted `*_BY_NAME`/`*_BY_ID` pair is a dense
bijection and that the `DefaultedRegistry` folds line up with the tables.

## Biome + tag tables (`biomes.rs`, `tags.rs`)

`generate` consumes `data/biomes_tags.json` (issue #49) — produced by
`extract-biomes-tags` from a live Paper load — and emits
`crates/rivet-registry/src/generated/biomes.rs` + `tags.rs`, committed and gated
behind the crate's `"blocks"` cargo feature.

The biome registry is **datapack-loaded** (not in `BuiltInRegistries`), so its
ids are assigned at runtime by `ResourceManagerRegistryLoadTask` from a
`TreeMap<Identifier, Resource>` sorted by `Identifier` compareTo (path first,
then namespace): id 0 = `minecraft:badlands`, 66 biomes, alphabetical. The tag
content is exactly what `TagNetworkSerialization.serializeTagsToNetwork` emits
for the `ClientboundUpdateTagsPacket` — every `networkSafeRegistries` registry
(WORLDGEN networkable + STATIC) that carries at least one bound tag, mapped to
tag-location -> element ids in the tag JSON file's value order.

- `biomes.rs` — `BIOME_BY_NAME` (phf, name -> id) / `BIOME_BY_ID`
  (id-indexed `&[&str]`) / `BIOME_COUNT` = 66.
- `tags.rs` — `TAG_REGISTRIES` (the 15 tag-carrying registry keys in
  deterministic order) + per-registry `{PREFIX}_TAG_BY_NAME` phf maps
  (tag-location -> element names in the tag file's value order). For the 7
  datapack registries the report cannot cover (`enchantment`, `dialog`,
  `painting_variant`, `timeline`, `instrument`, `banner_pattern`, `damage_type`)
  `tags.rs` also emits the element table; the other 8 (`block`, `item`,
  `entity_type`, `fluid`, `game_event`, `potion`, `point_of_interest_type`,
  `worldgen/biome`) resolve their element names through the existing
  `blocks.rs`/`registries.rs`/`biomes.rs` surfaces.

The validator is strict: every fixture top-level/registry/probe field is
allow-listed; element tables must be dense `0..n` bijections (sparse ids,
duplicate names/ids, non-integer/overflowing ids, malformed identifiers all
fail); every tag element must resolve against its registry's element table; the
15-registry set and each registry size plus the 697-tag total must match the
live-Paper anchors; the runtime element tables for the report-backed surfaces
are cross-checked against `data/reports/registries.json`; the `probe` counts
must be internally consistent and match the anchors; and the fixture must match
the sha256 in `data/biomes_tags.manifest.json` (pinned provenance). Regeneration
is byte-idempotent.

`crates/rivet-registry/src/biomes_tags_tests.rs` (feature-gated `blocks` +
`cfg(test)`, outside `src/generated/`) exercises the generated tables: the
biome table round-trips, tag elements resolve against the shared tables, the
tag-registry surface is complete, and the `is_overworld`/`is_nether` biome tags
(superflat presets) exist. The live `probe-biomes-tags` is the counterpart gate
that re-derives the fixture from a fresh Paper load.

## Feature data (`data/feature_data.json`)

`extract-feature-data` produces the seed-42 FEATURES checkpoint data foundation
from a live Paper load (the same `RegistryDataLoader` sequence as the worldgen
extractor, plus the dimension layer so the composite access binds block tags).
It materializes:

- `reachable_biomes` — the biome set that can drive FEATURES placement into the
  committed grid {(3,3),(4,3),(3,4),(4,4)}. Each committed chunk's FEATURES pass
  reads the biome map of its 3x3 neighborhood, and the writers (radius 1) are
  chunks 2..5, so the biome read set is chunks 1..6. The biome source is sampled
  at every quart position and every Y quart (-64..319 blocks) because the depth
  parameter varies by Y (both surface biomes and the deep `lush_caves` biome
  appear). Emitted id-sorted. For seed 42 this is exactly 5 biomes: beach,
  dark_forest, lush_caves, ocean, river.
- `biomes` — per-biome `BiomeGenerationSettings`: the dense registry `id`, the
  carver identity names, and the per-step placed-feature name lists. Step `i` is
  `GenerationStep.Decoration.values()[i]` (raw_generation .. top_layer_
  modification); holder-set order within a step is the builder's fixed order
  (part of the decoration semantics — the validator pins the per-step counts).
- `placed_features` / `configured_features` — the transitive closure of
  referenced registry entries, keyed by name with the full-registry dense `id`
  and the full `RegistryOps`-encoded JSON (the exact datapack JSON shape: holder
  references are bare strings, inline values are nested). The closure starts
  from the biomes' direct placed features and grows to fixpoint over the
  configured features' encoded JSON: any bare string that names a placed or
  configured registry entry is a holder reference (block-state `Name` values are
  object fields, never bare refs; registry membership disambiguates).

The fixture is self-validating: `extract-feature-data` runs the same contract
the probe enforces (structure, order, closure, provenance sha256). The validator
in `src/feature_data.rs` pins the reachable-biome id-sorted order, the per-biome
step counts, the dense-id uniqueness, the transitive closure (every referenced
feature present, every present feature reachable), and the manifest provenance.

## Packet-ID tables (`rivet-protocol`)

`generate` consumes the pinned `data/reports/packets.json` (issue #50) and emits
`crates/rivet-protocol/src/generated/`, committed and gated behind the crate's
`"packets"` cargo feature:

- `mod.rs` — declares the two generated submodules.
- `protocol.rs` — `ConnectionProtocol` and `PacketFlow` enums mirroring the
  vanilla enums (declaration order, string `id()`/`from_id()`, `PacketFlow`
  `opposite()`), with `ALL` arrays.
- `packets.rs` — a module per connection state (`handshake`, `play`, `status`,
  `login`, `configuration`) and flow (`serverbound`/`clientbound`). Each module
  holds a `#[repr(u32)] PacketType` enum whose discriminant is the vanilla
  `protocol_id` (the `addPacket` index in the corresponding `*Protocols.TEMPLATE`),
  a `PACKET_BY_ID` name table (id == index), a `phf` `PACKET_BY_NAME` map, and
  `PacketType::id()/name()/from_id()/from_name()` plus an `ALL` array.

The ids are **not re-extracted or hand-authored**: they come straight from the
`PacketReport` fixture (`protocol_id` values), sorted within each flow by
`protocol_id` to recover the exact `addPacket` registration order. The generator
validates the fixture strictly (duplicate packet names/ids, non-contiguous ids,
unknown states/flows, malformed entries all fail) and links each run to the
provenance in `data/reports/manifest.json` by sha256 — a stale fixture aborts
generation. After regenerating, run `cargo fmt -p rivet-protocol` and
`cargo check -p rivet-protocol --features packets`. The codegen test
`generated_packets_match_committed` enforces the same golden no-drift invariant
as the block tables.
