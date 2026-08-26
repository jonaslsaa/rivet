# rivet-oracle — the M0/M2 differential-test harness

Runs the real Java Paper server (the oracle), captures golden fixtures, and
verifies them. M0 is the harness's foundation: a fixed-seed superflat world
with a deterministic chunk-NBT fixture slice. M2 extends the same harness to
the normal-overworld generator: semantic worldgen samples (density / biome /
surface) plus a none-compression region chunk capture, per issue #51. The
source-disjoint generated normal-overworld FULL parity harness is also wired as
the Stage-B/G4 promotion row; the broader differential logic (worldgen chunk-hash
diffs vs Rivet, packet round-trips) builds on top of these fixtures later.

## Status: what works

- The Paper 26.2 server **boots headless** from the built paperclip bundler
  jar and reaches `Done (...)!` in ~5s.
- A fixed-seed superflat world is generated and a **deterministic golden
  fixture slice** is captured from the spawn region of all three dimensions.
- **Reproducibility is verified**: 432/432 chunk NBT payloads are
  byte-identical across two independent boots (seed 42, superflat); the M2
  normal-overworld none-compression region capture is 408/408 byte-identical
  across boots (region-file-compression=none per DECISIONS D13).
- **M2 worldgen semantic samples**: a Paper-side Java sampler
  (`src/java/WorldGenSampler.java`) boots the vanilla registries directly (no
  full server boot) and emits stable density/biome/surface samples for the
  normal-overworld generator; a companion script extracts Starlight light
  samples from the M0 FULL superflat chunks. Byte-identical across boots.
- **Generated-world seed-42 ground-truth handoff** (`generated-expected 42`,
  PR #563): the oracle boots a fresh seed-42 normal-overworld world, force-
  generates a spawn-area grid to `minecraft:full` (the issue #51 forced-ticket
  mechanism, from a blank chunk state so it is byte-deterministic), and commits
  the per-chunk `surface`/`bedrock`/`below_feet` sample contract the generated
  acceptance compares against. Twin-boot byte-identity verified.
- **Seed-42 FEATURES oracle checkpoint** (`features 42`, PR #631): the per-chunk
  `surface`/`bedrock`/`below_feet` fingerprint for the committed 2×2 grid
  {(3,3),(4,3),(3,4),(4,4)}, captured by booting a fresh seed-42 normal-
  overworld world and force-generating generated-expected's {-6..=6} forced grid
  to level 33 (`minecraft:full`). FEATURES is the last block-mutating status, so
  a FULL serialization's block data IS the FEATURES-decoration output; the
  verifier cross-checks each committed chunk against the generated-expected
  golden at the same coordinates. A features-only `feature_observations` layer
  additionally pins the positional magma_block (UnderwaterMagmaFeature, #644)
  and glow_lichen (MultifaceGrowthFeature, #645) occurrences, so those leaves
  are non-vacuously covered. This remains feature-leaf evidence, not a claim of
  full block-volume or FULL parity. Twin-boot byte-identity verified.
- **Seed-42 LIGHT-stage oracle checkpoint** (`light 42`, PR #175/#184): the
  per-section Starlight sky nibbles + derived sky-emptiness map for the
  committed 3×3 interior {19..21}² of a self-contained forced 5×5 grid
  {18..22}², captured by booting a fresh seed-42 normal-overworld world and
  force-generating the grid to level 33 (`minecraft:full`). FULL serialization
  carries the Starlight-computed light arrays, so the persisted light data IS
  the LIGHT-stage output; the rivet-server engine differential re-lights the
  interior through the real engine and matches it byte-exact. Twin-boot
  byte-identity verified.
- **Generated normal-overworld FULL parity** (`verify-generated-full`, Stage-B/G4,
  issue #175): a source-disjoint four-seed/four-region contract verifies Paper
  and Rivet overworld FULL payloads byte-for-byte, with strict provenance,
  closure, and named block/light/heightmap/NBT-order/key/
  LastUpdate tamper controls. The promotion row is opt-in via
  `RIVET_GENERATED_FULL=1` until the genuine Paper/Rivet capture is present;
  wholly absent evidence is UNVERIFIED, while malformed or linked evidence is
  FAIL. Stable evidence opening is deliberately Linux-only (`openat2` with
  `RESOLVE_NO_SYMLINKS` and `/proc/self/fd`); non-Linux platforms fail
  explicitly instead of taking an insecure pathname-reopen fallback. Linux
  x86_64 is the primary tested target. Replay records label commit-derived
  hashes as `paper-revision-identity-sha256` and
  `rivet-revision-identity-sha256`; these are revision identities, not
  source-content snapshots. Paper clean shutdown requires the post-READY
  `All dimensions are saved` marker and exit 0 or conventional SIGTERM exit
  143. The dedicated Rivet producer's exit 4 is BLOCKED/UNVERIFIED until its
  real FULL pipeline exists.
- The Rust runner `cargo run -p rivet-oracle` verifies every committed
  fixture kind against its manifest's SHA-256s and prints a summary.
- **Storage-only #231 V1a is green**: `anvil-roundtrip-v1a` writes all 432
  committed M0 CompoundTag payloads through a fresh `RegionFileStorage` with
  compression `none`, closes and recreates read-only storage, compares exact
  source/saved/reloaded payload bytes, proves the saved region tree is not
  mutated by reload, and runs strict named length/codec/header/overlap/
  truncation negatives. This is V1a storage evidence only; it does not claim
  V1b whole-region parity, `SerializableChunkData` reconstruction, or generated
  FULL parity.

## Directory layout

```
rivet-oracle/
  Cargo.toml            # rust crate (the runner)
  src/main.rs           # verify / sample / regenerate / gate commands
  src/java/WorldGenSampler.java    # Paper-side M2 worldgen sampler (semantic)
  scripts/extract_fixtures.py      # captures chunk-NBT fixtures from a run
  scripts/run_worldgen_sampler.sh  # compiles+runs WorldGenSampler on the runtime
  scripts/extract_light_samples.py # Starlight light samples from M0 chunks
  fixtures/             # golden fixtures (committed), one manifest per kind
    manifest.json       # M0 superflat slice: seed, config, per-file SHA-256s
    server.properties   # exact M0 config (superflat, seed 42)
    paper-global.yml    # pinned Paper global config (chunk-system 1/1, #266)
    paper-world-defaults.yml    # pinned Paper world-defaults (spawn-limits 0, #266)
    level.dat           # world metadata (gzip-NBT)
    level.dat_old
    chunk/<dim>/0.0/<cx>.<cz>.nbt   # decompressed chunk NBT payloads
    server-normal.properties        # exact M2 config (normal overworld, seed 42)
    worldgen/           # M2 semantic worldgen samples (kind: worldgen-samples)
      manifest.json     # hashes of samples.json + light.json
      samples.json      # 25 density + 22 biome + 16 surface entries
      light.json        # Starlight skylight/blocklight nibbles (M0 FULL chunks)
    regions/overworld-normal/       # M2 normal-overworld region payloads
      manifest.json     # 408 chunk NBT payloads, region-file-compression=none
      chunk/<dim>/0.0/<cx>.<cz>.nbt # decompressed normal-overworld chunk NBT
    chunk-hash/         # #54 xxh3_64 seed-hash gate (see 'Chunk-hash engine')
      corpus.json       # deterministic seed/coordinate corpus (single source of truth)
      paper/manifest.json  # Paper xxh3_64 digest table over the M2 region payloads
    text/               # chat/title/player-info/scoreboard component-JSON corpus
      manifest.json     # hashes of corpus.json + golden.json (kind: text)
      corpus.json       # 62 exact component-JSON inputs (issue #98)
      golden.json       # Paper's verdict + canonical decode->re-encode per input
    spline/             # CubicSpline/BoundedFloatFunction value-leaf goldens
      manifest.json     # hash of spline-goldens.json (kind: spline, issue #372)
      spline-goldens.json  # Paper's exact min/max/sample outputs as hex-float (plus parity strings)
    biome-temperature/  # Biome.getTemperature/FROZEN value-leaf goldens
      manifest.json     # hash of biome-temperature.json (kind: biome-temperature)
      biome-temperature.json  # Paper's bit-exact getTemperature/coldEnoughToSnow samples + raw noise
    generated-expected/ # seed-42 generated-world ground-truth handoff (PR #563)
      manifest.json     # hash of generated-expected.json (kind: generated-expected)
      generated-expected.json  # 81 FULL spawn-grid chunks' surface/bedrock/below_feet
    features/           # seed-42 FEATURES oracle checkpoint (PR #631)
      manifest.json     # hash of features.json (kind: features)
      features.json     # 4 committed chunks' surface/bedrock/below_feet
    light/              # seed-42 LIGHT-stage oracle checkpoint (PR #175/#184)
      manifest.json     # hashes of light.json + all 25 forced chunk NBTs (kind: light)
      light.json        # 9 committed chunks' sky nibbles + emptiness map + light_correct
      chunks/<x>.<z>.nbt # 25 forced chunk NBTs the rivet-server differential rebuilds
    generated-full/     # Stage-B/G4 normal-overworld FULL contract inputs
      contract.json     # strict four-seed/four-region schema and artifact paths
      server-normal-full.properties # canonical Paper producer template
  work/                 # scratch space — gitignored, never commit
    generated-full/replay-<nonce>/ # verifier-owned Paper/Rivet roots and lifecycle record
    run/                # a completed server run (materialized runtime)
    jars/               # copies of the built Paper jars
    logs/               # server stdout logs
```

Every fixture kind has its own `manifest.json`, so kinds grow independently
without a format migration.

## Boot procedure (exact commands that worked)

Java 25 (Temurin) and the already-built Paper jar. The **paperclip bundler**
jar (`paper-paperclip-26.2.local-SNAPSHOT.jar`) is the thing you run — it
applies embedded patches and assembles the runtime classpath/libraries. The
raw `paper-server-26.2.local-SNAPSHOT.jar` is the patched server but is not
standalone (no bundled libraries), so boot through paperclip.

Setup a scratch run dir:

```bash
mkdir -p work/run
cp working/Paper/paper-server/build/libs/paper-paperclip-26.2.local-SNAPSHOT.jar work/jars/
# write work/run/eula.txt with:  eula=true
# write work/run/server.properties (see fixtures/server.properties for the
#   exact M0 config: level-seed=42, level-type=minecraft:flat,
#   online-mode=false, generate-structures=false, server-port=25599,
#   view-distance=4, simulation-distance=4, enable-status=false)
# write work/run/config/paper-global.yml from fixtures/paper-global.yml
#   (chunk-system 1/1 — the #266 concurrency pin; see the section below)
# write work/run/config/paper-world-defaults.yml from
#   fixtures/paper-world-defaults.yml (spawn-limits 0 — no entity spawns into
#   the capture window; MC 26.2 removed the vanilla spawn-* server.properties
#   keys, so this is the effective suppression, see the section below)
```

Boot (from inside the run dir; server creates world/ subdir):

```bash
cd work/run
java -Xms512M -Xmx2G -jar ../jars/paper-paperclip-26.2.local-SNAPSHOT.jar nogui
# console reads stdin; in a harness, redirect stdin from /dev/null and tee
# stdout to a log. Wait for:  Done (...s)! For help, type "help"
```

Shut down cleanly: send SIGTERM to the `java` PID. Paper's shutdown hook runs
`Saving worlds` / `All chunks are saved` and exits 143. (A stdin-closed
process may hang at the console reader after the tick loop stops — SIGTERM
still triggers the clean save; SIGKILL only as a last resort.)

Paperclip materializes `libraries/`, `versions/`, `cache/` (~160MB) inside the
run dir on first boot — that's why `work/` is gitignored.

## Chunk-generation concurrency pin (issue #266)

A byte-identity oracle "from seed alone" is not well-posed while Paper's
Moonrise chunk system runs adjacent chunks' FEATURES passes concurrently: the
normal-overworld Nether vegetation placement races across chunk borders, so the
same seed + config can produce alternate block states depending on scheduling
and core count. Every oracle boot is therefore **pinned to one worker thread
and one I/O thread**, which serializes worldgen so the same seed + config yields
the same chunks every boot.

The pin has three layers, each verified at run time:

1. **Config.** `fixtures/paper-global.yml` sets
   `chunk-system.io-threads: 1` / `chunk-system.worker-threads: 1`. `prepare_run_dir`
   copies it to the run dir's `config/paper-global.yml` before every boot and
   errors if it is missing. (Paper rewrites the file in the run dir with its
   full defaults on first boot, so the committed config stays minimal and
   version-robust.)
2. **Log pin.** `boot_and_shutdown` parses the boot log's
   `[MoonriseCommon] Paper is using N worker threads, M I/O threads` line and
   refuses to continue unless exactly `1 worker threads, 1 I/O threads` was
   observed. A missing line (Paper renamed it, or it never appears) also fails
   loudly — the pin can never silently lapse.
3. **Provenance.** M2 region manifests record the observed concurrency as
   `chunk-concurrency: {io-threads, worker-threads}` and `verify` refuses any
   region capture whose provenance is missing or not 1/1. `verify --m2` also
   checks the baseline's provenance against the freshly booted log each run,
   so a committed manifest that drifts from what a run actually did is caught.

   Which manifests are region captures is decided by the explicit `kind` field
   (`kind: "m2"`), stamped by `regenerate` — never inferred from Paper's
   level-type/compression strings, so a future change to how Paper spells
   `level-type` cannot silently drop the provenance requirement. The old
   pre-`kind` committed manifests are handled by a strict, named fallback
   (none-compression `minecraft\:normal` with chunks) that is never a silent
   skip: a kind-less manifest of that shape is still a region capture and still
   requires the pinned provenance.

Every boot also runs with **entity spawning suppressed**: `fixtures/paper-world-defaults.yml`
sets every `entities.spawning.spawn-limits.*` category to 0, so no mob can
spawn into the save window and serialize into the captured chunks' `Entities`
tags (unrelated nondeterminism no normalization can remove). MC 26.2 removed
the vanilla `spawn-monsters`/`spawn-animals`/`spawn-npcs` server.properties
keys — `DedicatedServerProperties` reads none of them — so the world-defaults
spawn-limits are the effective mechanism (the same one `rivet-capture` uses).

## Fixture capture

```bash
python3 scripts/extract_fixtures.py work/run/world fixtures
```

Writes the spawn-region chunk NBT + `level.dat`(+_old) + `server.properties`
+ `manifest.json` into `fixtures/`. The manifest records the seed, the exact
server.properties, and a SHA-256 per captured file.

## Determinism note (important)

The raw region files (`.mca`) are **not** byte-stable across boots: the
offset/timestamp tables and sector padding vary (timestamps are wall-clock).
Only the **decompressed chunk NBT** is deterministic for a fixed seed +
generator settings. That is exactly what we capture, so the fixture SHA-256s
are the right parity baseline. This was verified empirically (432/432 M0
superflat and 408/408 M2 normal-overworld chunks across two boots each). The
worldgen semantic samples are emitted by the Java sampler directly and are
byte-identical across boots for a fixed seed + generator settings.

Region chunk compression on this build: `compression=2` is **zlib-wrapped
deflate** (`zlib.decompress`, wbits=15) — not raw deflate — and `1` is gzip.
`extract_fixtures.py` handles this.

## Verify

```bash
cargo run -p rivet-oracle                # checks ALL committed fixture kinds
cargo run -p rivet-oracle -- <dir>       # check a specific fixtures dir
```

The no-arg form discovers every `manifest.json` under `fixtures/` — the M0
superflat slice (`fixtures/`), the worldgen semantic samples
(`fixtures/worldgen/`), the normal-overworld region payloads
(`fixtures/regions/overworld-normal/`), the text component-JSON corpus
(`fixtures/text/`, issue #98), the script-driven value-leaf goldens
(`fixtures/spline/` #372, `fixtures/seq/`, `fixtures/biome-temperature/`,
`fixtures/dataconverter/` #535, `fixtures/data-worldgen/`), the composed-noise
goldens (`fixtures/composed-noise/`, issue #177), the post-surface column
goldens (`fixtures/surface-column/`, issue #179), and the generated-world
ground-truth handoff (`fixtures/generated-expected/`, PR #563) — and verifies
each against its own manifest. Prints `OK: all N captured files match
manifest SHA-256s` and a summary per kind (seed, level-type,
region-file-compression, per-dimension chunk counts). Exits nonzero on any
hash or size mismatch, or if any kind fails.

## One-command M0 sanity gate: `verify`

The whole boot → extract → pin-check → diff → verdict loop is a single
command:

```bash
cargo run -p rivet-oracle -- verify
# or against a custom baseline fixtures dir:
cargo run -p rivet-oracle -- verify <fixtures-dir>
```

It does, in order:

1. **Boot a fresh Paper run** in `work/verify/run`. The paperclip jar is
   resolved from `RIVET_ORACLE_JAR` if set, else `work/jars/`, else copied
   from `working/Paper/paper-server/build/libs/`. `server.properties` is copied
   from `fixtures/server.properties` (the exact M0 config: seed 42, superflat,
   `online-mode=false`, `generate-structures=false`, port 25599,
   view/simulation-distance 4), guaranteeing config parity by construction.
2. **Wait for `Done (...)!`** in the log (timeout 180s — covers the paperclip
   first-boot materialization of ~160MB libraries), then **SIGTERM** and wait
   for the clean save (`All dimensions are saved` must appear in the post-Done
   log tail; SIGKILL on timeout). Re-running reuses the materialized
   `libraries/`/`versions/`/`cache/` and wipes everything else, so the world is
   always fresh and a re-run boots in ~10s instead of ~30s.
3. **Extract the deterministic slice** by calling
   `scripts/extract_fixtures.py` (kept as the subprocess; it's small,
   already-tested, and needs no Rust port).
4. **Enforce the pinned Paper commit.** `verify` never passes silently against
   a stale or unverifiable Paper: the manifest's `paper` provenance
   (`26.2-DEV-main@0a99345`) pins the commit the golden baseline was captured
   against, and the gate compares it to the `Git-Commit` attribute of the
   server jar the paperclip **actually materialized and booted**
   (`work/verify/run/versions/26.2/paper-26.2.jar`). The pin is read from what
   actually ran — never from a proxy build (a co-located/`working/Paper`
   `paper-server-*.jar` can sit at a different commit than the resolved
   paperclip; a stale `work/jars/` sibling shadows the source build silently).
   A **mismatch** fails with the expected and actual commits and points at the
   regeneration path; an **unavailable** pin (no manifest pin, or the booted
   jar carries no readable Git-Commit) also fails loudly — the gate never
   passes when the pin cannot be confirmed.
5. **Diff chunk-NBT SHA-256s** against the baseline manifest. Only the
   chunk-NBT layer is compared — level.dat / server.properties contain
   wall-clock timestamps and are expected to differ.
6. **Verdict**: `PASS: 432/432 chunk NBT payloads are byte-identical to the
   committed golden baseline (seed 42 / minecraft:flat) — green against vanilla
   itself.` or a FAIL report (per-chunk expected/actual hashes, missing/extra
   chunks). Nonzero exit on any failure.

A fresh-boot worldgen diff is a real result — investigate, never fudge
fixtures to pass. `work/verify/boot-gate.log` and the kept fresh-extraction dir (in
the system temp dir, printed on FAIL) are the diagnostic artifacts.

## Negative control: `verify --expect-fail`

Tamper detection used to exist only as Rust unit tests on the pure diff
function. `verify --expect-fail` closes that hole: it proves the *full*
boot → extract → diff pipeline is not vacuously green by diffing a fresh boot
against a **deliberately corrupted** baseline and requiring the divergence to
be detected *and named*.

```bash
cargo run -p rivet-oracle -- verify --expect-fail
# or against a custom baseline fixtures dir:
cargo run -p rivet-oracle -- verify --expect-fail <fixtures-dir>
```

It runs the same pipeline as `verify` (including the pin check), but instead
of diffing against the committed fixtures it:

1. Copies the baseline fixtures to a scratch dir in the system temp and
   **corrupts one known chunk payload** (flips a byte) *and* that chunk's
   recorded SHA-256 in the copy's manifest, so the copy is internally
   consistent — a plausible but wrong baseline. The committed fixtures are
   never touched.
2. Boots a fresh Paper run, extracts the deterministic slice, and diffs it
   against the corrupted copy.
3. Passes (exit 0) **only** when the tampered chunk is the one named in the
   mismatch list. It rejects a clean diff (false negative — the pipeline
   missed the tamper) and any divergence that names a *different* chunk
   (detected for the wrong reason), both with distinct nonzero exits and the
   kept fresh-extraction dir for inspection.

The negative control reuses the exact `verify` boot and extract steps, so it
cannot pass if `verify` itself is broken: a boot/extract failure, a pin
mismatch, or an unverifiable pin all exit nonzero before the diff runs.

`scripts/gate.sh` runs `verify --expect-fail` right after `verify` as a
required oracle stage (whenever the verify prereqs are present, accepting the
extra boot) so a future change that breaks the acceptance logic or the tamper
is caught by the gate, not only by a manual run. A nonzero exit — boot/extract
failure, pin mismatch, or a tamper not detected and named — aborts the gate
under `set -e` exactly like any other oracle step.

## M2 region gate: `verify --m2`

The M2 gate proves two fresh boots of the **normal-overworld** none-compression
region capture match byte-for-byte (the foundation for the density/biome/surface
waves). It runs the same boot → extract → pin-check → diff pipeline as `verify`,
but with the normal-overworld config (`fixtures/server-normal.properties`:
`level-type=minecraft:normal`, `region-file-compression=none` per DECISIONS D13,
seed 42, view/simulation-distance 2, port 25599) and diffed against
`fixtures/regions/overworld-normal`. Entity spawning is suppressed for the
boot via `fixtures/paper-world-defaults.yml` (spawn-limits 0, per #266) — the
vanilla `spawn-monsters/animals/npcs` keys were removed in MC 26.2 and would be
a no-op.

```bash
cargo run -p rivet-oracle -- verify --m2
cargo run -p rivet-oracle -- verify --m2 --expect-fail   # negative control
```

`--expect-fail` corrupts a copy of the region baseline and requires the tampered
chunk to be detected and named — the same vacuous-green guard as the M0 control.
A custom baseline dir can be passed as the final argument.

Both `verify --m2` modes additionally enforce the #266 concurrency pin on the
baseline: the manifest must carry `chunk-concurrency: {io-threads: 1,
worker-threads: 1}`, the freshly booted log must report exactly 1/1 threads,
and the two must agree. Any drift — a manifest without provenance, provenance
other than 1/1, or a run that actually used a different thread count — fails
loudly before the diff runs.

## FULL region gate: `verify --full` (issue #51)

The FULL gate proves the **superflat status-FULL region capture** is genuine
and reproducible: a fresh superflat Paper boot (issue #51,
`fixtures/server-full.properties` — `level-type=minecraft:flat`,
`region-file-compression=none` per DECISIONS D13, corpus seed 0) is diffed
against the committed `fixtures/regions/superflat-full` baseline. The fixtures
are produced by a twin-boot under the #266 1/1 concurrency pin (a `regenerate
--full` requires the two fresh extractions to be byte-identical before it
commits anything).

```bash
cargo run -p rivet-oracle -- verify --full
cargo run -p rivet-oracle -- verify --full --expect-fail   # negative control
```

The capture is **corpus-forced**: between boot 1 (world create) and boot 2
(capture), level-33 `minecraft:forced` tickets for every corpus coordinate are
injected into each dimension's `chunk_tickets.dat` (see
`full_forced_extraction`), so boot 2 loads "8 persistent chunks" per dimension
and finishes every corpus coordinate to `minecraft:full`. The extraction spans
the four regions around the origin (`r.0.0`, `r.-1.-1`, `r.-1.0`, `r.0.-1`) so
positive, negative, and the x/z=31 region seams all land in captured region
files. The save-clock `LastUpdate` long is normalized to 0 (a boot-timing
artifact, not worldgen content) so the two boots are byte-identical.

The FULL baseline therefore carries 8 status-FULL chunks per dimension
(overworld, the_nether, the_end) plus the non-FULL neighbours, and coverage
against the #54 corpus reports 8 present / 24 missing / 0 extra: all 8 corpus
coordinates of the recorded seed reach FULL (seed 0's row fully owned), and the
24 missing are the other three corpus seeds' rows, which are unreachable from a
single-seed manifest — never a false green. `--expect-fail` corrupts a copy of
the baseline and requires the tampered chunk to be detected and named, guarding
against a vacuously green boot→extract→diff chain.

## M2 worldgen semantic samples: `sample`

The `worldgen/` fixtures are regenerated without a full server boot — the
Paper-side sampler (`scripts/run_worldgen_sampler.sh`) boots the vanilla
registries against the materialized runtime
(`work/run/versions/26.2/paper-26.2.jar` + library jars) and emits
`samples.json`; `scripts/extract_light_samples.py` re-extracts the Starlight
light samples from the M0 FULL superflat chunks; the manifest is re-hashed.

```bash
cargo run -p rivet-oracle -- sample
```

The manifest is serialized in committed field order, so regeneration is
byte-identical (git-clean) for unchanged samples — verified by a unit test.

## Post-surface column oracle (issue #179)

`fixtures/surface-column/surface-columns.json` is the independent Paper 26.2
post-surface column golden that the surface checkpoint of a chunk-production
port must reproduce. It is produced by `SurfaceColumnProbe.java`, which boots
only the vanilla registries (no server boot) and then drives the REAL overworld
generator pipeline on REAL `ProtoChunk`s at seed 42:
`createBiomes` -> `fillFromNoise` -> `buildSurface`. The corpus is the #175
chunk-coordinate matrix (8 chunks: positive/negative/region-seam), and every
column records:

- pre/post block states at every 4th Y (block registry key + raw state id) for
  the chunk's own block-origin corner column (`(0,0)` in chunk-local
  coordinates, i.e. the block at `min-block-x`, y, `min-block-z`), so the exact
  post-surface block id per sampled Y is pinned at that column;
- pre/post `WORLD_SURFACE_WG` + `OCEAN_FLOOR_WG` heights for all 256 columns;

So a green pins the exact post-surface block id down the corner column of every
chunk, and pins the surface/floor height across all 256 columns of every chunk.
It does NOT pin the sub-surface block id at the other 255 columns — a surface
port whose cave/biome-driven subsurface differs off the corner column (while
matching top heights and the corner column) would still pass this golden. That
bound is intentional for the #179 SURFACE checkpoint; the exact per-column block
id coverage is a follow-up (e.g. a full-column sample matrix).

- the surface biome the surface pass read at the top of the column (captured
  and hash-pinned with the rest of the fixture; verification does not
  semantically assert the id);
- `any-surface-changed` / `any-height-changed` flags plus the pre-surface
  snapshot, so a no-op capture is detectable: the pre snapshot is taken on an
  all-air chunk with unprimed heightmaps (`-65` = `MIN_Y-1`), so a probe that
  recorded the chunk before any generation (or a rules set that never applied)
  would emit all-false deltas and be rejected by verification. Separately,
  verification requires the corpus to contain at least one surface-rule block
  (grass_block/dirt/sand/gravel/sulfur/...), which the fill pass cannot emit —
  so a probe that ran `fillFromNoise` but dropped the `buildSurface` call
  (post = plain fill output) is rejected as fill-only, not relabeled
  "post-surface".

One load-bearing substitution is documented in the fixture metadata
(`flat-bedrock-substitution`): Paper injects
`paper:optionally_flat_bedrock_condition_source` at the top of the overworld
surface sequence, and that class derefs `context.level()` for
`generateFlatBedrock`. The probe drives surface with a Level-free
`WorldGenerationContext`, so it ships a shadow of that condition source under
the same FQN and codec id with the DEFAULT config (`generateFlatBedrock=false`),
exact for these default-overworld columns. The shadow's class is placed FIRST
on the classpath, so `Bootstrap.bootStrap()` — which registers the FQN with a
class literal — loads the shadow instead of the jar's class; the runner
preserves that ordering (shadow classes before the server jar) so the
substitution is effective during the probe run. The fixture is pinned to this
substitution and to the `26.2-DEV-main@0a99345` Paper commit.

Another load-bearing property is that the seed-42 corpus is **structure-free**.
The probe pre-sets each `NoiseChunk` with `Beardifier.EMPTY` (mirroring the
composed-noise oracle) instead of Paper's real
`Beardifier.forStructuresInChunk(...)`. That is exact only because no
beard-affecting structure start (village, pillager outpost, ancient city, trail
ruins, trial chambers, stronghold) comes within beard reach of any corpus chunk.
This was verified against the pinned Paper 0a99345 runtime (real
`ChunkGeneratorStructureState` + `StructurePlacement.isStructureChunk` + the
`Structure.isValidBiome` gate at the real start position): zero beard-affecting
starts within 6 chunks of any corpus chunk, and no corpus chunk is a placement
chunk. The Rust regression test
`committed_surface_column_is_structure_free` replays the placement predicate
(including Paper's power-of-two `nextInt(bound)` shortcut, which ancient
cities' limit of 16 needs) over the committed coordinates and encodes the four
verified in-reach placement chunks — (9,11) village + ancient city near (15,15),
(36,25) village near (31,31), (-25,-27) trail ruins near (-31,-31) — each
biome-rejected at its true start position, so a future regeneration that stops
being structure-free fails loudly.

Verify / regenerate:

```bash
cargo run -p rivet-oracle -- surface-column            # verify golden + non-vacuity
cargo run -p rivet-oracle -- surface-column --tamper   # negative control (must fail)
cargo run -p rivet-oracle -- surface-column --sample   # regenerate from pinned Paper
scripts/run_surface_column_probe.sh <out-dir>          # raw probe into an out-dir
```

Regeneration requires the materialized pinned runtime (or
`RIVET_PAPER_RUNTIME_JAR` / `RIVET_PAPER_LIBRARIES`). Before running, the runner
authenticates the runtime jar's `Git-Commit` manifest attribute against the
pinned `26.2-DEV-main@0a99345` commit — the same source of truth `verify`'s pin
check reads — so a jar at a different (or unverifiable) commit can never
relabel fixtures with provenance it does not have. A missing runtime or an
unconfirmed pin exits **3 UNVERIFIED**; only a genuine probe failure exits 1.
`verify` (and the no-arg `cargo run -p rivet-oracle`) gates on this golden
exactly like the composed-noise fixtures; a missing fixture tree exits nonzero
with `UNVERIFIED`.

## Generated-world ground-truth handoff: `generated-expected`

The generated-world acceptance (PR #563) compares the seed-42 content a
`rivet-server --seed 42` serves against a Paper-captured reference — never
against nothing, and never against a superflat fallback. `generated-expected`
builds that reference.

```bash
cargo run -p rivet-oracle -- generated-expected 42             # verify the committed handoff
cargo run -p rivet-oracle -- generated-expected 42 --to out.json  # capture a fresh handoff
cargo run -p rivet-oracle -- generated-expected 42 --tamper    # negative control
cargo run -p rivet-oracle -- regenerate --generated-expected   # twin-boot regenerate the fixture
```

The capture boots the pinned Paper runtime on a fresh seed-42 normal-overworld
world, discards boot1's partial spawn-area chunks (regenerating those is not
byte-deterministic), injects level-33 forced tickets for a -6..6 spawn grid
(the issue #51 mechanism), and boot2 finishes them to `minecraft:full`. The
committed handoff is the -4..4 interior subset — every committed chunk's
neighbors are forced FULL too, keeping border-tree placement deterministic. The
capture runs in its own dedicated scratch run dir (a self-contained Paper
runtime, never symlinked from the shared oracle run dirs) and binds
`server-port=0` (an OS-assigned free port) so it never collides with a
concurrent release gate booting on the shared 25599 or any other fixed port. It
also enforces provenance: the booted server jar's `Git-Commit` must match the
pinned `0a99345` before any content is written, so a wrong-commit jar can never
be stamped with the pinned provenance. The
`regenerate --generated-expected` path requires two independent captures to be
byte-identical AND contract-valid before committing anything — two equally-wrong
captures are refused, never committed.

Verify mode is pinned to seed 42: `generated-expected 999` is a usage error,
never a silent verify of the wrong reference (the `--to` capture path accepts
any seed whose spawn grid passes the anti-superflat sample contract, e.g. seed
42 — an arbitrary seed whose spawn grid is uniform, such as an all-ocean one,
is refused like any superflat echo).

The golden is seed-self-describing (PR #595): `generated-expected.json` is a
`GoldenWorld` — the per-chunk `WorldManifest` (`format`/`overworld_region`/
`chunks`) flattened together with the `seed` field it was actually captured
under. `--to <seed>` writes the real seed into the golden, so a wrong-seed
capture carries its true seed. The committed verification requires the golden
seed to be exactly the pinned `42` (a wrong-seed capture is drift, refused even
if the manifest hash is freshly rebuilt around it), and `regenerate
--generated-expected` reads the seed back OUT of the golden rather than
stamping the constant — the manifest's `seed` always describes the bytes it
hashes. The seed field sits inside the hashed bytes, so it is bound both
structurally and by the manifest SHA-256. The shared loaded-world
`WorldManifest` schema is unchanged — only the generated-expected golden wraps
it.

The per-chunk contract is exactly what the acceptance compares: 16×16
`surface`/`bedrock`/`below_feet` arrays indexed row-major `z*16+x`, sampled at
the chunk center offset (8,8), with the surface being the highest non-air
block, `bedrock` the block at `y=-60`, and `below_feet` at `y=-61`, up to
`WORLD_CEILING_Y=320`. The verify path pins the provenance (Paper 0a99345,
seed 42), the manifest SHA-256s, the forced-grid shape, and the anti-superflat
contract: a superflat echo (all-air bedrock plane at -60, one repeated surface
pattern, a tiny distinct-block set) is refused loudly, never handed off as
ground truth. A missing runtime or a missing fixture tree is typed UNVERIFIED
(exit 3), never a fabricated green.

## Generated normal-overworld FULL parity: `verify-generated-full`

The Stage-B/G4 row is deliberately source-disjoint from the loaded-world,
superflat-FULL, and generated-expected fixtures. It compares four pinned seeds
across the eight origin-adjacent/seam coordinates in the overworld only. The
contract and canonical Paper producer template live under
`fixtures/generated-full/`; the verifier allocates a fresh nonce-scoped replay
root under `work/generated-full/` for every run. It does not accept committed,
caller-selected, or producer-selected Paper/Rivet evidence roots.

```bash
cargo run -p rivet-oracle -- verify-generated-full
cargo run -p rivet-oracle -- verify-generated-full --refresh-determinism
RIVET_GENERATED_FULL=1 scripts/gate.sh --full
```

The verifier derives every digest from payload bytes read through stable opened
file descriptors. Contract, producer executable/config, and nested payload
metadata are checked on those same descriptors; symlink and hardlink aliases,
nonregular files, malformed metadata, and self-diff aliases are hard failures.
A wholly absent prerequisite remains **3 UNVERIFIED**, while any partial
launched handoff, stale provenance, and parity mismatch are **1 FAIL**. Stable
evidence opening is deliberately Linux-only (`openat2` with
`RESOLVE_NO_SYMLINKS` and `/proc/self/fd`); non-Linux platforms fail explicitly
instead of taking an insecure pathname-reopen fallback. Linux x86_64 is the
primary tested target.

The dedicated `rivet-generated-full` producer is wired as the only Rivet side
of this lifecycle. In the current checkout the real
`OverworldGenerator -> FULL/light -> SerializableChunkData` production API is
not exposed as one callable path: Level/registry bootstrap, chunk-source
closure, light-engine completion, and the FULL `SerializableChunkData` snapshot
writer still need to be connected. It therefore exits with an explicit
`BLOCKED` result and never creates output; no synthetic payload is accepted as
parity evidence. Until that API exists and fresh evidence is produced, the
promotion row remains blocked rather than claiming green.

## Seed-42 FEATURES checkpoint: `features`

The seed-42 FEATURES oracle checkpoint (PR #631) is the oracle ground-truth
BEFORE any Rivet FEATURES implementation: a focused Paper 26.2 capture of a
deterministic seed-42 overworld chunk set forced through the FEATURES
decoration status, plus an exact Rivet-side verifier. It reuses the
generated-expected two-boot capture machinery (level-33 forced tickets,
`clear_region_files`, the provenance pin) but records stage-specific truth for
the decoration step itself, so a future Rivet FEATURES port is checked against
Paper ground truth rather than against nothing.

```bash
cargo run -p rivet-oracle -- features 42               # verify the committed checkpoint
cargo run -p rivet-oracle -- features 42 --to out.json # capture a fresh checkpoint
cargo run -p rivet-oracle -- features 42 --tamper      # negative control (must fail)
cargo run -p rivet-oracle -- regenerate --features     # twin-boot regenerate the fixture
```

The Moonrise forced-ticket path can only serialize FULL: a level-34 ticket
(`ChunkLevel.byStatus(FEATURES)`) is `INACCESSIBLE` to `fullStatus`, so the
checkpoint captures at level 33, which serializes as `minecraft:full`. That is
faithful to the FEATURES step because FEATURES is the last block-mutating
status: the InitializeLight/Light steps only compute light arrays and never
touch block data, so a FULL serialization's block states ARE the
FEATURES-decoration output.

The committed golden is the 2×2 grid {(3,3),(4,3),(3,4),(4,4)} around the
tree-bearing chunk (4,4), a strict interior subset of generated-expected's
committed {-4..=4} grid. The FORCED grid is generated-expected's {-6..=6}²
regime: the FEATURES step is declared with `blockStateWriteRadius(1)`, so a
chunk's decoration writes one chunk into each neighbor, and only the same
forced-grid context makes the committed chunks byte-identical to the canonical
golden — which the verifier enforces by cross-checking every committed chunk
against generated-expected at the same coordinates (an absent or divergent
golden is damage, never a silent skip).

The per-chunk contract reuses the loaded-world fingerprint: 16×16 row-major
`z*16+x` `surface`/`bedrock`/`below_feet` arrays, the sorted distinct block
set, distinct-state-id count, and section count, plus the chunk status and
capability flags (a FULL chunk folds no `status:` flag). Non-vacuity: chunk
(4,4) must carry tree blocks in its surface (a pre-features carvers capture has
none), the union distinct set must clear the pre-features floor, and the
bedrock plane must be depth-sampled. The tri-state contract mirrors
generated-expected: a wholly absent fixture tree is UNVERIFIED (exit 3), a
partial/corrupt tree hard-fails (exit 1), never a silent green. The tamper
negative control proves the manifest SHA-256 gate is not vacuous. `verify` (and
the no-arg `cargo run -p rivet-oracle`) gates on this golden exactly like the
other load-bearing kinds.

The checkpoint also pins the leaf features it covers, via a features-only
observation layer (`feature_observations`) kept out of the shared
`WorldManifest`: the positional `{block, index (z*16+x), y}` occurrences of
`magma_block` (UnderwaterMagmaFeature, PR #644) and `glow_lichen`
(MultifaceGrowthFeature/`glow_lichen`, PR #645). `surface`/`bedrock`/
`below_feet` do not locate these — magma sits on the ocean floor below the
surface water and glow_lichen attaches in the water column/caves — so the
golden records them directly. The verifier requires a `magma_block` in a
submerged column (`surface[index] == water`), the pinned UnderwaterMagma
ocean-floor signature, and at least one `glow_lichen`. The observations are
feature-leaf evidence only; they do not claim full feature dispatch or
FULL/generated-world parity. Tamper negatives remove each set (and relocate
magma off the ocean floor) and must fail verification, so the #644/#645
coverage is non-vacuous.

## Seed-42 LIGHT-stage checkpoint: `light`

The seed-42 LIGHT oracle checkpoint is the oracle ground-truth BEFORE any Rivet
LIGHT-status wiring: a focused Paper 26.2 capture of a deterministic seed-42
overworld chunk grid forced through real Starlight sky lighting, plus an exact
Rivet-side verifier and the rivet-server engine differential. It reuses the
generated-expected/features two-boot capture machinery (level-33 forced tickets,
`clear_region_files`, the `0a99345` provenance pin) but records stage-specific
truth for the LIGHT step itself — the per-section sky light nibbles, the derived
sky-emptiness map, and `light_correct` — so a future Rivet LIGHT-status port
(the merged `SkyStarLightEngine` / `SkyLightProvider` path) is checked against
Paper ground truth rather than against nothing.

```bash
cargo run -p rivet-oracle -- light 42               # verify the committed checkpoint
cargo run -p rivet-oracle -- light 42 --to out.json # capture a fresh checkpoint
cargo run -p rivet-oracle -- light 42 --tamper      # negative control (must fail)
cargo run -p rivet-oracle -- regenerate --light     # twin-boot regenerate the fixture
```

The Moonrise forced-ticket path can only serialize FULL: a level-35 ticket
(`ChunkLevel.byStatus(LIGHT)`) is `INACCESSIBLE` to `fullStatus`, so the
checkpoint captures at level 33, which serializes as `minecraft:full`. That is
faithful to the LIGHT step because FULL serialization carries the
Starlight-computed light arrays: `ChunkLightTask`'s fresh-chunk branch runs
`lightChunk -> StarLightInterface.lightChunk -> SkyStarLightEngine.light`
(`lightChunk(lightAccess, chunk, true)`), and the resulting nibbles are what
`SaveUtil` persists (`isLightOn` + `starlight.light_version` 10).

The committed golden is the 3×3 interior {19..21}² of a self-contained forced
5×5 grid {18..22}², far from seed-42's spawn-area chunks (chunk (-2,0)). Every
committed chunk's full 1-radius block context and 2-radius emptiness context
lies inside the forced grid, so Paper's light for the interior is computed over
exactly the set this checkpoint commits. The raw NBT of all 25 forced chunks is
committed under `chunks/` so the rivet-server engine differential rebuilds the
exact context Paper lit in (every chunk reconstructed into the server's
`StateId` space with its persisted light installed) and re-lights the interior
via `SkyLightProvider::relight_chunk_with` — the per-neighbour no-edge-checks
path from Paper's `relightChunks`, whose neighbour-light pull
(`propagateNeighbourLevels`) reproduces the fixture's east-neighbour water
dampening at the boundary columns. The published sky nibbles and emptiness map
must then match the fixture truth byte-exact.

The per-chunk contract: `status` exactly `minecraft:full`, `light_correct`
true, light sections `-5..=20` (26 sections), per-section `sky_nibbles` as the
`to_vanilla_nibble` byte views (`None` for a null section), and the derived
`sky_emptiness` map covering all 24 world sections. Non-vacuity: at least one
committed chunk must carry a non-null sky nibble with real terrain shadowing
(not uniformly 0 or uniformly 0xFF), and a non-uniform emptiness map — a
superflat or all-zeros echo is refused loudly. The tri-state contract mirrors
features: a wholly absent fixture tree is UNVERIFIED (exit 3), a partial/corrupt
tree hard-fails (exit 1), never a silent green. The tamper negative control
proves the manifest SHA-256 gate is not vacuous. `verify` (and the no-arg
`cargo run -p rivet-oracle`) gates on this golden exactly like the other
load-bearing kinds.

## Regenerate: `regenerate`

Full regeneration of every fixture kind (boots Paper where a boot is required):

```bash
cargo run -p rivet-oracle -- regenerate            # all kinds
cargo run -p rivet-oracle -- regenerate --m0       # M0 superflat slice only
cargo run -p rivet-oracle -- regenerate --m2       # M2 region payloads only
cargo run -p rivet-oracle -- regenerate --samples  # worldgen samples only
cargo run -p rivet-oracle -- regenerate --text     # text corpus only (Paper oracle op)
cargo run -p rivet-oracle -- regenerate --composed-noise   # composed-noise goldens only
cargo run -p rivet-oracle -- regenerate --surface-column   # post-surface column goldens only
cargo run -p rivet-oracle -- regenerate --generated-expected  # generated-expected handoff only
cargo run -p rivet-oracle -- regenerate --features         # seed-42 FEATURES checkpoint only
```

The script-driven value-leaf goldens are regenerated outside `regenerate`:
`spline/` (issue #372) via `scripts/run_spline_probe.sh`, `seq/` via
`scripts/run_seq_probe.sh`, `biome-temperature/` via
`scripts/run_biome_temperature_probe.sh`, `dataconverter/` (issue #535) via
`scripts/run_dataconverter_probe.sh`, and `data-worldgen/` via
`scripts/run_data_worldgen_probe.sh` (see each fixture's manifest note).

The `surface-rule-data/` goldens (the SurfaceRuleData surface trees under
`RuleSource.CODEC`/`ConditionSource.CODEC`, issue #179) are likewise
script-driven: `scripts/run_surface_rule_data_probe.sh` (see the fixture
manifest note).

The `text/` corpus (issue #98) records the exact component JSON a chat/title/
player-info/scoreboard packet carries, Paper's accept/reject verdict in the
Bootstrap-only oracle context, and Paper's canonical `ComponentSerialization.
CODEC` decode->re-encode under non-compressed `JsonOps` (stored as verbatim JSON
strings so the byte identity is preserved). The four Paper-accepted
click/hover entries use exactly Paper 26.2's codec field names (ShowText
`value`, OpenUrl `url`, RunCommand `command`, CopyToClipboard `value`) and none
needs registry/Holder context; the four `malformed-*-wrong-key` negatives
carry the same content with a wrong field name and Paper rejects them, pinning
the field names as load-bearing. `regenerate --text` boots the Paper oracle
(like `--m0`/`--m2`). The `text_manifest_regeneration_is_byte_identical` unit
test proves only the *manifest writer's* determinism: re-running the manifest
hashing over the committed `corpus.json` + `golden.json` reproduces the
committed `manifest.json` byte-for-byte (git-clean), and the regenerated
manifest verifies its own files. It does not run a second Paper boot, so it is
not a twin-boot determinism proof of `golden.json` (that is the M2 `regenerate
--m2` procedure below, which actually performs two independent boots).

M2 region regeneration (the worldgen nondeterminism case, #266) is a
**twin-boot**: `regenerate --m2` performs two independent fresh Paper boots
(`boot-m2a.log` / `boot-m2b.log`) and requires the two extractions to be
byte-identical **before anything is committed**. On a mismatch it refuses to
write the fixtures and leaves both trees (in the system temp dir, paths printed)
for investigation — it never commits, excludes, or normalizes chunks to force
a pass. On a match it records the boot log's observed `chunk-concurrency` into
the region manifest (replacing any previous value) and commits the fixtures.

`regenerate --m0` stamps the M0 superflat manifest with `kind: "m0"` and the
M2 twin-boot stamps `kind: "m2"` (+ the observed concurrency). Regenerating
into a scratch destination (`--to <dir>`) validates the produced tree before it
is committed anywhere: `cargo run -p rivet-oracle -- regenerate --m0 --to /tmp/x`
must produce a manifest that verifies clean and requires no M2 chunk-concurrency
provenance (proving the regenerated M0 is not misclassified as a region capture,
issue #266). `--to` requires exactly one of `--m0`/`--m2`/`--full`: bare
`regenerate --to /dir` and multi-kind `--m0 --m2 --to /dir` are refused (they
would share one destination across kinds and the M2/FULL twin-boots replace the
whole directory, silently discarding the other kind's output), and the derived
kinds `--samples` and `--text` are refused with `--to` (worldgen samples
regenerate the committed `fixtures/worldgen` tree; the text corpus regenerates
the committed `fixtures/text` tree via the Paper reference oracle).

The gate's hash verification is the safety net against a bad regeneration.
Never hand-edit fixtures; regenerate from a clean run instead. Every boot in the
pipeline is pinned to `chunk-system` 1/1 and runs with entity spawning
suppressed (spawn-limits 0) per the sections above.

## Chunk-hash engine (issue #54)

The xxh3_64 seed-hash gate compares Paper's chunk digests against Rivet's once
Rivet can serialize FULL chunks. It deliberately never boots Paper — the digests
come from the committed M2 region payloads via the rivet-nbt codec.

- `hash-self-check` — verifies the `xxh3_64` implementation against pinned
  known-answer vectors (anchor `xxh3_64(b"") = 2d06800538d394c2`). A wrong
  variant or an endianness slip fails loudly instead of silently corrupting every
  digest. Exit 0 = pass, 1 = fail.
- `hash-paper [dir]` — rebuilds `fixtures/chunk-hash/paper/manifest.json` from
  the committed M2 region payloads. Must be byte-identical (git-clean). The
  manifest **stamps `status` from each payload's root `Status` string** — never
  assumed — so the committed M2 capture honestly reports 2 genuine FULL chunks
  (the_nether/0.0 + the_end/0.0; overworld has 0). Its working seed (42) is not
  a corpus seed, so its sweep coverage is honestly 0/N (see below). The single
  `dir` argument overrides both source and destination (one tree): point it at a
  scratch copy of the corpus-forced superflat-full capture (#51) to report that
  tree's 8 FULL chunks per dimension without touching committed fixtures. Exit 0;
  a missing/empty payload source is **UNVERIFIED (3)** — never a fabricated
  zero-chunk manifest that could make a later diff vacuously green.
- `hash-rivet <dir>` — reads a Rivet region tree (`chunk/<dim>/<region>/<cx>.<cz>.nbt`).
  A wholly absent tree, missing `chunk/`, or a present tree that has not yet
  reached FULL is **3 UNVERIFIED**. Present non-directories, nonregular entries,
  malformed manifests/payloads, partial trees, and invalid NBT are **1 FAIL**;
  no malformed existing evidence is downgraded to a missing prerequisite.
- `hash-diff <paper> <rivet>` — compares Paper vs Rivet manifests. Refuses
  differing provenance (seed/algorithm/paper/concurrency) AND refuses a
  Paper-vs-Paper self-diff (both args the same tree — canonicalized, so a
  symlink alias is caught too): a self-comparison can never imply Paper ==
  Rivet parity, so it is UNVERIFIED (3), never a PASS. Missing Rivet evidence,
  self-diff/provenance/coverage prerequisites remain UNVERIFIED; existing
  malformed manifests or corrupted raw trees are FAIL (1). Only FULL entries
  are compared; a Paper-only or Rivet-only FULL chunk or a raw-digest
  difference is real divergence — never a vacuous green. Exit 0 = PASS,
  1 = FAIL (names each chunk), 3 = UNVERIFIED, 64 = usage.
- `hash-diff --expect-fail <paper> <rivet> [kind]` — negative control: corrupt a
  copy of the **Rivet** baseline and require the tampered chunk named **and only
  it** — a FAIL for any other reason (a different chunk, a provenance mismatch,
  an unrelated divergence) is rejected as a wrong-reason pass. `kind` is
  `block`/`light`/`heightmap`/`nbt-order`/`nbt-key`/`all` (runs every class, so a
  future mutation the comparator silently ignores is caught). Order-only
  `nbt-order` tampering is flagged as triage (canonical-identical to the
  original) but still fails — order divergence is divergence; `nbt-key` inserts
  a root NBT key Paper's writer never emits, a real content change whose
  canonical digest differs from the original (unlike `nbt-order`). The corrupted
  copy keeps the original manifest's seed so the tamper is the only divergence.

The corpus (`corpus.json`) is the single source of truth for which seeds and
coordinates a green sweep must cover; coverage is always reported against it,
never assumed. Coverage is seed-aware: a manifest records one world seed, and
only a corpus seed satisfies sweep cells — a capture under the committed working
seed (42, not a corpus seed) honestly reports 0/N sweep coverage. Live
FULL-chunk generation is blocked (#51/#231/#15), so the gate always runs
`hash-self-check` + `hash-paper` and, with `RIVET_HASH_DIR` unset (the default),
skips the Paper-vs-Rivet comparison with an explicit NOTICE (never a self-diff,
never a claim of parity it does not have). Setting `RIVET_HASH_DIR` to a Rivet
region tree opts into the strict check: the comparison and the tamper negatives
then run for real, and any UNVERIFIED (incomplete corpus coverage, or a self-diff
if it aliases the paper tree) or FAILED divergence is gate-fatal.

## Conventions

- Never weaken fixtures to pass; regenerate them from a clean run instead.
- `work/` is scratch — never commit it.
- This crate is deliberately std-only-plus-{serde,serde_json,sha2}. Its deps
  live in this crate's `Cargo.toml` (not the shared `[workspace.dependencies]`),
  but like any workspace member the resolve still updates the shared
  `Cargo.lock` — expect that when adding deps here. Exception: `xxhash-rust`
  (the #54 engine's only third-party crypto, xxh3 feature only) is declared at
  workspace scope so every member sees one identical digest family and the
  feature set cannot drift per-crate.
