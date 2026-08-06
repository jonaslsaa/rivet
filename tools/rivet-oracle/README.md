# rivet-oracle — the M0 differential-test harness

Runs the real Java Paper server (the oracle), captures golden fixtures, and
verifies them. This is the harness's M0 foundation; the full differential
logic (azalea bots, worldgen chunk-hash diffs vs Rivet, packet round-trips)
builds on top of it later.

## M0 status: what works

- The Paper 26.2 server **boots headless** from the built paperclip bundler
  jar and reaches `Done (...)!` in ~5s.
- A fixed-seed superflat world is generated and a **deterministic golden
  fixture slice** is captured from the spawn region of all three dimensions.
- **Reproducibility is verified**: 432/432 chunk NBT payloads are
  byte-identical across two independent boots (seed 42, superflat).
- The Rust runner `cargo run -p rivet-oracle` verifies the fixtures against
  the manifest's SHA-256s and prints a summary.

## Directory layout

```
rivet-oracle/
  Cargo.toml            # rust crate (the runner)
  src/main.rs           # verifies fixtures + prints M0 summary
  scripts/extract_fixtures.py   # captures fixtures from a server run
  fixtures/             # golden fixtures (committed)
    manifest.json       # seed, server.properties, per-file SHA-256s
    server.properties   # exact config used
    level.dat           # world metadata (gzip-NBT)
    level.dat_old
    chunk/<dim>/0.0/<cx>.<cz>.nbt   # decompressed chunk NBT payloads
  work/                 # scratch space — gitignored, never commit
    run/                # a completed server run
    jars/               # copies of the built Paper jars
    logs/               # server stdout logs
```

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
are the right parity baseline. This was verified empirically (432/432 across
two boots).

Region chunk compression on this build: `compression=2` is **zlib-wrapped
deflate** (`zlib.decompress`, wbits=15) — not raw deflate — and `1` is gzip.
`extract_fixtures.py` handles this.

## Verify

```bash
cargo run -p rivet-oracle                # checks fixtures/ against manifest
cargo run -p rivet-oracle -- <dir>       # check a different fixtures dir
```

Prints `OK: all N captured files match manifest SHA-256s` and the M0 summary
(seed, level-type, per-dimension chunk counts). Exits nonzero on any hash or
size mismatch.

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
fixtures to pass. `work/verify/boot.log` and the kept fresh-extraction dir (in
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

`scripts/gate.sh` runs `verify --expect-fail` right after `verify` (behind the
same paperclip guard, accepting the extra boot) so a future change that breaks
the acceptance logic or the tamper is caught by the gate, not only by a manual
run.

## Conventions

- Never weaken fixtures to pass; regenerate them from a clean run instead.
- `work/` is scratch — never commit it.
- This crate is deliberately std-only-plus-{serde,serde_json,sha2}. Its deps
  live in this crate's `Cargo.toml` (not the shared `[workspace.dependencies]`),
  but like any workspace member the resolve still updates the shared
  `Cargo.lock` — expect that when adding deps here.
