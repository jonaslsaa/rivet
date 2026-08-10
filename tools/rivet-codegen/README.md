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
  `mth_golden_tests.rs`) from the real Paper `Mth` class. These files are
  `GENERATED — do not hand-edit`; this is the checked-in generator that makes
  regeneration possible. Idempotent: over the current committed files it
  reproduces them byte-for-byte (`git diff` stays clean).
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
  table (issue #154): size 32366, per-block contiguous ranges partitioning the
  id space, defaults in range, and the representative anchor ids. This is the
  live half of the fixture-pinned conformance test in `generate`'s
  `block_states.rs` tests.
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
target/release/rivet-codegen generate         # -> crates/rivet-registry/src/generated/{mod.rs, blocks.rs, block_properties.rs, block_behaviors.rs, block_states.rs, registries.rs, biomes.rs, tags.rs, synchronized.rs, registry_data.rs} + crates/rivet-protocol/src/generated/
target/release/rivet-codegen registries       # -> crates/rivet-registry/src/generated/registries.rs (report-driven half only)
target/release/rivet-codegen mth-gen          # -> crates/rivet-util/src/mth_{sin_table,atan_tables,golden_tests}.rs
target/release/rivet-codegen extract-biomes-tags  # -> data/biomes_tags.json + manifest
target/release/rivet-codegen probe-biomes-tags    # verify biome ids + tag network content against live Paper
target/release/rivet-codegen probe-block-states   # verify the emitted block-state global ids against live Paper
target/release/rivet-codegen extract-block-behaviors  # -> data/block_behaviors.json + manifest
target/release/rivet-codegen probe-block-behaviors    # verify the per-StateId behavior table against live Paper
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
(from `data/registry_data.json`, the canonical join capture), and the packet-ID
tables (from `data/reports/packets.json`, the `PacketReport` fixture).
Regenerate all of them with a single `generate` run; `--input`/`--output`
control the block half, `--packets`/`--packets-output` control the packet half.
(The `registries` subcommand emits just the report-driven registry tables.)

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

- `mod.rs` — declares the generated submodules: `biomes`, `block_behaviors`,
  `block_properties`, `block_states`, `blocks`, `registries`, `registry_data`,
  `synchronized`, `tags`.
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
