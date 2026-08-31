# Independent Paper normal-overworld FULL evidence

This directory is a Paper-only evidence producer. It is intentionally separate
from the generated-full parity harness and does not import Rivet code, fixtures,
commits, or hashes. A successful bundle is genuine evidence from the pinned
Paper server; it is not a parity result.

## Contract

- Paper source revision: `0a993450f129c4942c2a9ed45ba047412b4667cf`.
- Java: explicit Temurin 25 through `JAVA_HOME`.
- Route: `minecraft:overworld`, `minecraft:normal`,
  `generate-structures=true`.
- Seeds, in unsigned 64-bit corpus notation:
  `5207638315753790570`, `12807505919197044144` (signed Java long
  `-5639238154512507472`), `5246862266665176429`, and
  `3423572188437197996`.
- Targets, in deterministic order: `(0,0)`, `(15,15)`, `(31,31)`,
  `(-1,-1)`, `(-16,-16)`, `(-31,-31)`, `(-1,0)`, `(0,-1)`.
- The Paper scheduler support closure is the sorted target expansion by radius
  11. Forced tickets are exactly `minecraft:forced`, level `33`, with
  `ticks_left=Long.MIN_VALUE`; the immutable injected NBT list follows the
  frozen closure order. Paper's persisted ticket map may reorder that list, so
  post-exit validation checks the exact set and the injected/post-exit hashes
  separately.
- Each seed gets three fresh isolated two-boot roots. Boot one creates the
  normal world. The driver removes all target/support region, POI, entity,
  `.mcc`, and ticket data before injecting tickets and starting boot two.
- The probe freezes random ticks, daylight, weather, and mob spawning on the
  Paper server's main thread. Its output records the server side and main
  thread explicitly. Paper is stopped with the console `stop` command only
  after the probe proves the full closure is loaded and every target is
  `FULL` plus lit. Extraction reads the world only after process exit and zero
  exit status, and rejects chunk data outside the exact closure.
- Raw decompressed chunk NBT is saved and hashed without rewriting. The capture
  command runs the same fail-closed validator before returning success. The
  validator rejects symlinks, hardlinks, non-regular files, path escapes, and
  oversized individual or aggregate evidence payloads. A separate canonical
  semantic hash sorts compound keys and removes only the root
  `InhabitedTime` and `LastUpdate` fields, which are documented save-clock
  fields. The raw bytes remain authoritative. Paper's Starlight save hook
  intentionally writes `isLightOn=false`; persisted lit evidence is the exact
  `starlight.light_version=10` marker together with `minecraft:full` status,
  matching Paper's `SerializableChunkData` load rule.
- Paper 26.2 stores the SavedData source at
  `world/dimensions/minecraft/overworld/data/minecraft/world_gen_settings.dat`.
  The driver preserves those exact bytes at the contract capture path
  `world/data/minecraft/worldgen_settings.dat` and records both source and copy
  hashes. The validator checks that the route is normal noise, not flat, and
  that the seed/structures/DataVersion are exact.
- Paper rewrites `server.properties` and expands both YAML files with defaults
  on first boot. The validator binds the source properties, both Paper YAML
  fixtures, and the probe source/plugin descriptor to immutable digests, then
  independently recompiles the pinned probe with Temurin 25 and compares the
  archived class bytes. It checks runtime property values and effective YAML
  paths while preserving exact fixture copies as provenance, without mistaking
  Paper's generated defaults for a configuration mismatch.

## Runtime prerequisites

The driver refuses a missing or dirty Paper source, a source revision other than
 the pin, a non-Temurin JDK, and any pre-existing output bundle. It builds Paper
from source with Gradle and uses only the freshly built Paperclip jar; it never
falls back to a stale jar, global Paper runtime, or shared world/cache.

The source must be available as a canonical, read-only clean checkout at
`working/Paper` in this worktree (or pass `--paper-source` with any checkout
path; the basename does not need to be `Paper`). The checkout must resolve to
the pinned revision above. The source build may write only its ignored Gradle
build outputs. Capture/runtime output is restricted to
`/home/jonas/Rivet/working/output/paper-normal-full/`.

## Exact command

From the repository root, after ensuring the Temurin 25 JDK and read-only Paper
source are available:

```bash
export JAVA_HOME="$HOME/.local/share/jdk25"
export PATH="$JAVA_HOME/bin:$HOME/.cargo/bin:$PATH"
python3 tools/paper-normal-full-evidence/capture.py \
  --paper-source "$PWD/working/Paper" \
  --output /home/jonas/Rivet/working/output/paper-normal-full/bundle
```

The production command is intentionally expensive: 12 isolated Paper roots,
2451 closure chunks per root, two graceful boots per root, and fresh Paper
build/materialization. Do not run it as a focused test. A failed run leaves its
private diagnostic root but no `bundle.json`; remove that output directory
before retrying.

Validate an existing bundle with:

```bash
python3 tools/paper-normal-full-evidence/validate.py \
  /home/jonas/Rivet/working/output/paper-normal-full/bundle
```

Exit status `0` is `VERIFIED`, `1` is `FAILED` evidence, and `3` is
`UNVERIFIED` because no evidence/prerequisite is available.

## Focused checks

No production capture is needed for implementation checks:

```bash
python3 -m py_compile tools/paper-normal-full-evidence/nbt.py \
  tools/paper-normal-full-evidence/capture.py \
  tools/paper-normal-full-evidence/validate.py
python3 tools/paper-normal-full-evidence/tests/test_evidence.py
```

The tests exercise strict NBT full-consumption parsing, signed seed handling,
closure order, exact-closure enforcement, tri-state validation, copied-root
rejection, malformed/trailing payloads, provenance tamper discrimination,
link/non-regular rejection, bounded payloads, and non-FULL/light/heightmap
rejection. They do not create a Paper world and do not touch the production
output directory.
