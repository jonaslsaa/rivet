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
- **`generate`** — reads that JSON and emits a sample registry table under
  `generated/` (a `BlockId` registry + a block-state property enum structure).

```
rivet-codegen extract [--bundler <path>] [--output <path>]
rivet-codegen generate [--input <path>]  [--output <dir>]
```

## How to run

Requirements: a JVM (`java` + `javac` on `PATH`, or `JAVA_HOME` set) and
`unzip`. The default bundler path is
`working/Paper/paper-server/build/libs/paper-bundler-26.2.local-SNAPSHOT.jar`
(the artifact of a Paper `build` — run `./gradlew :paper-server:build` in
`working/Paper` if it is missing). You cannot build that jar in this read-only
checkout; the tool needs a jar produced elsewhere.

```
cargo build --release
target/release/rivet-codegen extract          # -> data/block_states.json
target/release/rivet-codegen generate         # -> generated/{blocks.rs, block_properties.rs}
```

`extract` caches the unpacked classpath in `.cache/` (gitignored) so reruns are
fast. Pass `--bundler` to point at a different jar.

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

- `generated/blocks.rs` — `BlockId(pub u16)`, a `phf::Map<&'static str, u16>`
  (`BLOCK_BY_NAME`), an id-indexed `BLOCK_BY_ID` array, and lookup methods.
- `generated/block_properties.rs` — `BlockPropertyId`, an enum with one variant
  per distinct `(name, values)` property type, plus the value tables and a
  per-block `BLOCK_STATE_SHAPES` table (ordered property ids by block id).

The generator asserts block ids are contiguous `0..n` (true for vanilla 26.2).
These files are samples for `crates/rivet-registry`; wiring them into the crate
(adding the `phf` dependency, feature-gating) is a follow-up.
