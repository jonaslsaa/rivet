# rivet-capture — join-path packet-capture harness (#153)

Captures the canonical protocol-776 offline-mode Paper join path (handshake →
login → configuration → play → spawn chunk send → movement sample) as a
deterministic, byte-identical fixture. This is the byte-identity source for the
M1.1 DoD: every join-path packet body in the ported `rivet-protocol` must
round-trip against these captures (consumed by #86/#87/#90/#94/#97 and the M1.3
chunk byte-compare).

## How it works

The harness:

1. Boots a fresh Java Paper server headlessly (the `verify` pattern from
   `tools/rivet-oracle`: paperclip bundler jar, seed 42 / superflat /
   `online-mode=false`). The capture uses its own port (25600, distinct from the
   oracle's 25599 so the two never collide) and installs
   `fixtures/paper-world-defaults.yml` (all natural-spawn categories capped at 0)
   so the superflat world holds exactly one entity — the joining player. MC 26.2
   removed the vanilla `spawn-monsters` server.properties key; without this Paper
   config, slimes spawn in slime chunks and their RNG-driven hopping makes the
   join capture non-deterministic no matter what field normalization you apply.
2. Joins it with the Azalea headless client (`tools/rivet-client`) **through a
   byte-transparent TCP proxy**. The proxy forwards the exact raw bytes in both
   directions and frames every packet at the wire boundary, so the capture is
   `(state, direction, packet id, body bytes)` with no re-encoding.
3. Shuts the server down cleanly (SIGTERM + `All dimensions are saved`).
4. **Normalizes** the raw capture into a deterministic canonical form — rewriting
   only the fields the server randomizes per boot, each with a documented
   justification (see `src/normalize.rs` and the fixture manifest `note`s).
5. Compares the normalized capture **byte-for-byte** against the committed
   fixture (`fixtures/join/`).

## Commands

```sh
cargo run -p rivet-capture -- capture              # boot+join once, print the packet summary
cargo run -p rivet-capture -- capture --runs 3     # 3 boots; require identical normalized captures
cargo run -p rivet-capture -- fixture              # boot+join once and (re)write fixtures/join/
cargo run -p rivet-capture -- verify               # boot+join, diff against the committed fixture
cargo run -p rivet-capture -- verify --expect-fail # negative control (see below)
```

The rivet-client binary must be built first (nightly workspace):

```sh
cd tools/rivet-client && cargo build --locked
# or set RIVET_CLIENT_BIN to an existing rivet-client binary
```

## Fixture layout

```
rivet-capture/
  src/            # proxy, framer, normalizer, fixture diff
  fixtures/join/
    manifest.json  # provenance (Paper commit, bot identity, config, azalea
                   # revision) + one `captured` entry per packet: identity,
                   # SHA-256, byte length, normalization note.
    capture.jsonl  # one JSON object per canonical packet: hex body bytes.
  work/           # scratch space — gitignored, never commit
```

## Determinism contract

The `verify` command enforces the fixture's pinned Paper commit
(`fixtures/join/manifest.json` `paper: 26.2-DEV-main@0a99345`) against the
`Git-Commit` attribute of the server jar the paperclip actually materialized and
booted — a stale or unverifiable Paper fails loudly, never silently (mirrors
`rivet-oracle verify`).

`capture --runs N` additionally proves Paper-vs-Paper determinism: every boot's
normalized capture must be byte-identical to every other. The fields the
normalizer rewrites (spawn X/Z, entity ids, keepalive ids, `set_time`, chunk
coordinates) are exactly the ones the server randomizes per boot; everything
else is compared and must match.

## Negative control: `verify --expect-fail`

Tamper detection exists as Rust unit tests on the pure diff function, but
`verify --expect-fail` closes the end-to-end hole: it copies the committed
fixture to a scratch dir, **corrupts one packet body and that packet's recorded
SHA-256** (so the copy is internally consistent — a plausible but wrong
baseline), boots a fresh Paper, captures, and requires the divergence to be
detected **and** name the tampered packet. The tamper deliberately lands in a
packet the normalizer does NOT rewrite, proving the byte-compare catches drift
in a genuinely compared field. A clean diff (false negative) or a divergence
naming a different packet both fail with distinct nonzero exits. The committed
fixtures are never touched.

## Gate integration

`scripts/gate.sh` runs this harness as a required oracle stage after
`rivet-oracle verify` (guarded by the same paperclip jar prerequisite). A
nonzero exit — boot/extract failure, pin mismatch, or a tamper not detected and
named — aborts the gate under `set -e` exactly like any other oracle step.

## Conventions

- Never weaken fixtures to pass; regenerate them from a clean run
  (`rivet-capture fixture`) against the pinned Paper.
- `work/` is scratch — never commit it.
- The proxy is byte-transparent: it never re-encodes a forwarded packet, only
  parses a copy for recording.
