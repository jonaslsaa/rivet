# rivet-client

An isolated Azalea-based Minecraft 26.2 headless client for Rivet's
differential-test harness. It uses an offline account, emits JSON Lines on
stdout, and exits after spawning, disconnecting, failing to connect, or
reaching its timeout.
Every JSON record includes `"protocol":1`; consumers must reject unsupported
protocol versions rather than guessing event shapes.

Azalea requires nightly Rust, so this package is deliberately its own nested
workspace. Its dependency graph and toolchain do not enter Rivet's stable
workspace. The Azalea dependency is pinned to an exact 26.2-compatible Git
revision.

Run it against the local Paper fixture server:

```sh
tools/rivet-client/run.sh -- \
  --address 127.0.0.1:25599 \
  --username RivetProbe \
  --timeout-seconds 30
```

Exit codes are `0` after a successful spawn, `2` when the Minecraft login or
spawn phase times out, `1` after the actual Azalea connection attempt fails or disconnects before
spawning, and `64` for invalid CLI
arguments. Cargo writes build diagnostics to stderr; the client protocol on
stdout remains JSON Lines.

## Scenario runner (`run-scenario`)

`run-scenario` is the differential harness for the `join` scenario
(WORKFLOWS.md §headless-client-driver) over either server implementation —
Paper or Rivet (issue #155). It boots a server headlessly, joins it with the
Azalea client, captures a normalized observable transcript, and compares
transcripts with a field-level comparator.

```sh
tools/rivet-client/run-scenario.sh join            # Paper-vs-Paper self-check: 2 fresh Paper boots, must be identical
tools/rivet-client/run-scenario.sh join --runs 3  # more Paper boots
tools/rivet-client/run-scenario.sh move           # Paper-vs-Paper movement self-check: bounded walk, identical movement transcripts
tools/rivet-client/run-scenario.sh join --server rivet            # Rivet headless boot: pinned Azalea completes login/config/spawn, exact 117-chunk send-set
tools/rivet-client/run-scenario.sh join --server both --pairs paper:rivet  # Paper-vs-Rivet play scenario
tools/rivet-client/run-scenario.sh capture        # one boot; print the normalized transcript
```

Modes (`--server` selects which servers boot, `--pairs` selects the comparison):

| `--server` | `--pairs` | What runs |
|---|---|---|
| `paper` (default) | `paper:paper` (default) | `join` Paper-vs-Paper self-check: `--runs` Paper boots must produce identical transcripts, plus the tamper negative case. Every Paper boot verifies the materialized server jar carries the pinned Paper commit (exit 3 UNVERIFIED otherwise), so the self-check is always against the same reference the oracle gate uses. |
| `paper` | `paper:paper` | `move` Paper-vs-Paper movement self-check (issue #53): each boot drives the client's bounded forward walk (`move` mode) and `--runs` Paper boots must produce identical normalized movement transcripts (per-tick spawn-relative deltas, velocity, on-ground, teleport/keepalive echo relationships), plus the tamper negative case. This validates the movement harness against Paper; a Rivet-vs-Paper comparison is deferred until Rivet's movement listener lands (issue #158). |
| `rivet` | `paper:rivet` | `join` Rivet headless boot (issue #192): `--runs` rivet-servers, each must reach `RIVET_READY`, take the pinned Azalea client through offline login, configuration (registry sync), the play handoff, and spawn, receiving exactly the deterministic 117-chunk send-set, and shut down cleanly on SIGTERM. |
| `both` | `paper:rivet` | `join` Paper-vs-Rivet play scenario (issue #192, inverted by #159): Paper and Rivet boot on isolated ports, the client joins each, and both must reach spawn. Both servers boot the single-stone superflat fixture, so the spawn height `position.y` is compared; the transcripts must differ only on the excluded per-boot nondeterminism plus the one documented Rivet/Paper gap (the health component default) — any other divergence FAILS the run. A tamper negative that shifts Paper's `position.y` must be detected, proving the field is genuinely compared. `--runs` is rejected here (it always boots exactly one Paper + one Rivet). |

Rivet readiness is a machine-readable `RIVET_READY` marker on `rivet-server`
stdout (crates/rivet-server/src/main.rs); the harness waits for it as a hard
gate — a timeout or a missing `rivet-server` binary is UNVERIFIED (exit 3), not
a FAIL. Every boot gets its own isolated port (Paper's run-dir
`server.properties` is patched; Rivet gets `--port`), so concurrent servers can
never collide. Rivet boots do not need `server.properties` (the binary is
driven purely by `--host`/`--port`).

A Rivet run only passes if the client completed a genuine play session against
the Rivet port. Azalea fires `Event::Init` before any TCP connect, and
`connection_failed`/`timeout` fire without completing a session, so the
transcript alone cannot distinguish a live server from a dead or hung endpoint.
The harness requires two independent observables: the rivet log shows
`connection established` (a line only the real `rivet-server` binary emits on
TCP accept), and the client transcript passes `rivet_play_verdict` — outcome
`spawned`, lifecycle containing `login` and `spawn`, the pinned Azalea build
revision, exactly 117 chunks, and the deterministic superflat spawn y `-63.0`.
A stale pre-play Rivet build (which closes the client at the login boundary),
a fake/non-Rivet endpoint, or a Paper-like y=-60 spawn all fail the verdict.

The fallback `rivet-server` path is resolved inside the harness's workspace,
and a narrow freshness guard rejects it when it predates that workspace's
`rivet-server` entry point. Normal runs rebuild before executing; the PLAY
verdict and Rivet-only connection log remain the load-bearing stale/fake-server
checks. `RIVET_SERVER_BIN` is an explicit override and is not commit-bound.

The runner:

1. Boots a fresh world per run (fixed seed 42 / superflat from
   `rivet-oracle`'s fixtures, `online-mode=false`) and waits for the server's
   READY marker (`Done (...)!` for Paper, `RIVET_READY` for Rivet).
2. Runs the headless client in the requested mode (`join` waits for the stable
   `joined` record after the chunk stream quiesces; `move` drives the bounded
   forward walk and waits for the `moved` record). For Rivet the transcript must
   pass `rivet_play_verdict` — `connection_failed`/`timeout` and any
   `disconnected` pre-play outcome are rejected as "client never completed
   login/configuration into play".
3. SIGTERM-shuts the server down cleanly and preserves raw diagnostics under
   `work/scenario-join/`, `work/scenario-move/`, `work/scenario-rivet/`, or
   `work/scenario-both/` (`boot*.log`, `client*.stdout.jsonl`,
   `client*.stderr.log`, `transcript*.json`).
4. Diffs the normalized transcripts field-by-field; requires identical
   transcripts Paper-vs-Paper.
5. Runs a controlled negative case (tampers `position.y` for `join`, or a
   sampled position `walk.samples[60].dx` for `move`) proving the comparator
   detects a known difference, so the harness cannot pass vacuously.

Exit codes are machine-stable and consumed by gate.sh: `0` PASS, `1` FAIL
(scenario comparison failed, negative case failed, harness error), `3`
UNVERIFIED (missing prereq — paperclip jar / rivet-server binary — or a server
did not reach READY within its boot timeout), `64` invalid CLI arguments.

The normalized `join` and `move` transcript shapes are documented in
`src/bin/run-scenario/transcript.rs`. The comparator
(`src/bin/run-scenario/comparator.rs`) reports every differing field with its
path and both values, and only skips fields explicitly declared in the
transcript's `excluded` map. For `join`, the Paper server randomizes the player
spawn X/Z offset per boot, and the received chunk coordinate list is centered
on that randomized spawn chunk, so `position.x`, `position.z`, and `chunks` are
excluded with justification; `position.y` (superflat spawn height), the chunk
count (117), and all other observables are compared and are identical across
fresh boots. For `move`, the per-tick sampled walk is normalized to
spawn-relative `dx/dz` deltas (so it is identical across boots); the walk
geometry (`walk_ticks`, `movement_ticks`, `sampled_ticks`), the teleport ids
(deterministic per fresh boot), the echo-relationship flags, and the sampled
walk are compared. The keepalive ids (Paper's `Util.getMillis()` challengeId —
`System.nanoTime()/1e6`, monotonic milliseconds since JVM start) and the
`entity_position_sync` corrections (timing-dependent) are excluded with
justification. A `moved` record whose sampled walk shows no meaningful forward
progress is classified as `noop` and fails the run rather than passing a
vacuous Paper-vs-Paper comparison.
