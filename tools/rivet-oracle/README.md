# rivet-oracle — the M0/M2 differential-test harness

Runs the real Java Paper server (the oracle), captures golden fixtures, and
verifies them. M0 is the harness's foundation: a fixed-seed superflat world
with a deterministic chunk-NBT fixture slice. M2 extends the same harness to
the normal-overworld generator: semantic worldgen samples (density / biome /
surface) plus a none-compression region chunk capture, per issue #51. The full
differential logic (worldgen chunk-hash diffs vs Rivet, packet round-trips)
builds on top of these fixtures later.

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
- The Rust runner `cargo run -p rivet-oracle` verifies every committed
  fixture kind against its manifest's SHA-256s and prints a summary.

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
  work/                 # scratch space — gitignored, never commit
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
(`fixtures/regions/overworld-normal/`), and the text component-JSON corpus
(`fixtures/text/`, issue #98) — and verifies each against its own
manifest. Prints `OK: all N captured files match manifest SHA-256s` and a
summary per kind (seed, level-type, region-file-compression, per-dimension
chunk counts). Exits nonzero on any hash or size mismatch, or if any kind
fails.

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

## Regenerate: `regenerate`

Full regeneration of every fixture kind (boots Paper where a boot is required):

```bash
cargo run -p rivet-oracle -- regenerate            # all kinds
cargo run -p rivet-oracle -- regenerate --m0       # M0 superflat slice only
cargo run -p rivet-oracle -- regenerate --m2       # M2 region payloads only
cargo run -p rivet-oracle -- regenerate --samples  # worldgen samples only
cargo run -p rivet-oracle -- regenerate --text     # text corpus only (Paper oracle op)
```

The `text/` corpus (issue #98) records the exact component JSON a chat/title/
player-info/scoreboard packet carries, Paper's accept/reject verdict in the
Bootstrap-only oracle context, and Paper's canonical `ComponentSerialization.
CODEC` decode->re-encode under non-compressed `JsonOps` (stored as verbatim JSON
strings so the byte identity is preserved). The four Paper-accepted
click/hover entries use exactly Paper 26.2's codec field names (ShowText
`value`, OpenUrl `url`, RunCommand `command`, CopyToClipboard `value`) and none
needs registry/Holder context, so the only reason Rivet rejects them is the
unported `ClickEvent`/`HoverEvent` STUB codec (epic #12) — never a malformed
field or registry/Holder context; the four `malformed-*-wrong-key` negatives
carry the same content with a wrong field name and Paper rejects them, pinning
the field names as load-bearing. `regenerate --text` boots the Paper oracle
(like `--m0`/`--m2`) and is deterministic: two independent boots must produce
byte-identical `golden.json`. The `text_manifest_regeneration_is_byte_identical`
unit test enforces that regeneration is git-clean.

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
issue #266). `--to` requires exactly one of `--m0`/`--m2`: bare
`regenerate --to /dir` and multi-kind `--m0 --m2 --to /dir` are refused (they
would share one destination across kinds and M2's twin-boot replaces the whole
directory, silently discarding M0's output), and the derived kinds `--samples`
and `--text` are refused with `--to` (worldgen samples regenerate the committed
`fixtures/worldgen` tree; the text corpus regenerates the committed
`fixtures/text` tree via the Paper reference oracle).

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
- `hash-paper` — rebuilds `fixtures/chunk-hash/paper/manifest.json` from the
  committed M2 region payloads. Must be byte-identical (git-clean). The manifest
  **stamps `status` from each payload's root `Status` string** — never assumed —
  so the committed capture honestly reports 2 genuine FULL chunks
  (the_nether/0.0 + the_end/0.0; overworld has 0). Its working seed (42) is not
  a corpus seed, so its sweep coverage is honestly 0/N (see below). Exit 0.
- `hash-rivet <dir>` — reads a Rivet region tree (`chunk/<dim>/<region>/<cx>.<cz>.nbt`).
  There is no Rivet FULL serialization yet, so it exits **3 UNVERIFIED**, never
  green (Rivet chunk serialization is #231/#15; #51 must capture status-FULL
  regions).
- `hash-diff <paper> <rivet>` — compares Paper vs Rivet manifests. Refuses
  differing provenance (seed/algorithm/paper/concurrency) AND refuses a
  Paper-vs-Paper self-diff (both args the same tree — canonicalized, so a
  symlink alias is caught too): a self-comparison can never imply Paper ==
  Rivet parity, so it is UNVERIFIED (3), never a PASS. Only FULL entries are
  compared; a Paper-only or Rivet-only FULL chunk, a raw-digest difference, or a
  missing required corpus coordinate are each real divergence — never a vacuous
  green. Exit 0 = PASS, 1 = FAIL (names each chunk), 3 = UNVERIFIED, 64 = usage.
- `hash-diff --expect-fail <paper> <rivet> [kind]` — negative control: corrupt a
  copy of the **Rivet** baseline and require the tampered chunk named. `kind` is
  `block`/`light`/`heightmap`/`nbt-order`/`all` (runs every class, so a future
  mutation the comparator silently ignores is caught). Order-only `nbt-order`
  tampering is flagged as triage (canonical-identical) but still fails — order
  divergence is divergence. The corrupted copy keeps the original manifest's
  seed so the tamper is the only divergence.

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
