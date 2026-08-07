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
cargo run -p rivet-capture -- verify --mutate KIND # detector discrimination (see below)
cargo run -p rivet-capture -- audit --runs 3       # report raw-field variance across boots
```

The rivet-client binary must be built first (nightly workspace):

```sh
cd tools/rivet-client && cargo build --locked
# or set RIVET_CLIENT_BIN to an existing rivet-client binary
```

`verify --mutate KIND` accepts one of `reorder`, `delete`, `insert`, `field`,
`canon`, `relabel`, `burst`, `entity-id`, or `set-time-absent`.

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

## Independent detectors (#195)

`normalize` groups the raw capture by `(state, direction, id)` and rewrites the
per-boot-randomized fields, so a **reordering** of required packets, a
**cross-direction causality** violation, or **content corruption that keeps the
canonical form self-consistent** all still byte-match the fixture. To close that
hole, `verify` runs a set of read-only detectors on the raw capture (and, for
the canonical form, on the normalized one) before the byte-compare. They are
deliberately implemented only against `frame.rs` leaf primitives + generated
registry tables — they never call the normalizer — so a normalizer that
silently drops, duplicates, or alters content is caught here:

- `ordering.rs` — replays the proxy state machine: within-direction emit order,
  response chains, the non-decreasing handshake → login → configuration → play
  rank, the deterministic play burst total order (the join's fixed send order,
  which the `(state, dir, id)` grouping would otherwise erase), and the
  configuration send order. Catches reorderings the `(state, dir, id)` grouping
  erases.
- `relationships.rs` — id/content-matched cross-direction causality: teleport →
  ack (every `accept_teleportation` id echoes an issued `player_position` id, in
  order), keepalive request → echo (every serverbound body equals a prior
  clientbound body), spawn consistency (movement y, `set_default_spawn_position`
  block pos, chunk-cache center), and entity id agreement across every
  entity-id packet (login, entity_event, set_entity_data, update_attributes,
  set_entity_motion, the move_entity_* trio, rotate_head, add_entity).
- `semantic.rs` — content shape: superflat chunk structure + state-id validity,
  the 11×11-minus-corners chunk grid, registry/tag id-range + coverage, and
  `set_time` structural validity (including failing when the canonical capture
  drops the packet entirely).
- `preservation` — raw↔canonical content agreement (chunk block histogram,
  tag-id multiset, registry entry-name set), so a self-consistent normalizer
  that changes content trips a detector even though the fixture still
  round-trips.

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

## Detector discrimination: `verify --mutate KIND`

`--expect-fail` proves the byte-compare catches a content change, but it cannot
prove the detectors catch reorderings, deletions, insertions, field edits, or a
corrupt canonical form. `verify --mutate KIND` closes that gap: it boots a fresh
Paper, runs every detector on the clean capture (which must pass), applies a
controlled mutation to the raw (or canonical) capture, re-runs the detectors,
and **requires the expected `Failure` kind(s) to fire and name the defect**. Each
kind is a deterministic transform whose required detector(s) are observed on the
real raw capture:

| `--mutate` | transform | required detector(s) |
|---|---|---|
| `reorder` | swaps two adjacent distinct packets | `ordering` |
| `delete` | drops the `accept_teleportation` | `teleport-ack` |
| `insert` | duplicates a `level_chunk_with_light` | `chunk` |
| `field` | rewrites the `accept_teleportation` teleport id | `teleport-ack` |
| `canon` | truncates the canonical `set_time` body | `set_time` |
| `relabel` | flips a chunk's direction to serverbound | `ordering` / `chunk` |
| `burst` | swaps two mid-burst packets | `ordering` |
| `entity-id` | corrupts the `update_attributes` entity id | `entity-id` |
| `set-time-absent` | drops every canonical `set_time` | `set_time` |

A clean run on a mutated capture (false negative) exits nonzero — the detectors
must never be vacuous.

## Gate integration

`scripts/gate.sh` runs this harness as a required oracle stage after
`rivet-oracle verify` (guarded by the same paperclip jar prerequisite). A
nonzero exit — boot/extract failure, pin mismatch, or a tamper not detected and
named — aborts the gate under `set -e` exactly like any other oracle step. After
`verify` and `verify --expect-fail` it runs `verify --mutate` for all nine kinds,
so every detector must stay discriminating on every gate.

## Conventions

- Never weaken fixtures to pass; regenerate them from a clean run
  (`rivet-capture fixture`) against the pinned Paper.
- `work/` is scratch — never commit it.
- The proxy is byte-transparent: it never re-encodes a forwarded packet, only
  parses a copy for recording.
