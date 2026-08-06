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
tools/rivet-client/run-scenario.sh join --server rivet            # Rivet headless boot + pre-play transcript
tools/rivet-client/run-scenario.sh join --server both --pairs paper:rivet  # Paper-vs-Rivet pre-play scenario
tools/rivet-client/run-scenario.sh capture        # one boot; print the normalized transcript
```

Modes (`--server` selects which servers boot, `--pairs` selects the comparison):

| `--server` | `--pairs` | What runs |
|---|---|---|
| `paper` (default) | `paper:paper` (default) | Paper-vs-Paper self-check: `--runs` Paper boots must produce identical transcripts, plus the tamper negative case. Behavior unchanged from before #155. |
| `rivet` | `paper:rivet` | Rivet headless boot: `--runs` rivet-servers, each must reach `RIVET_READY`, accept the client at the pre-play boundary, and shut down cleanly on SIGTERM. Reports the pre-play limitation honestly (issue #96). |
| `both` | `paper:rivet` | Paper-vs-Rivet pre-play scenario: Paper and Rivet boot on isolated ports, the client joins each, and the harness reports the controlled pre-play transcript divergence. `--runs` is rejected here (it always boots exactly one Paper + one Rivet). |

Rivet readiness is a machine-readable `RIVET_READY` marker on `rivet-server`
stdout (crates/rivet-server/src/main.rs); the harness waits for it as a hard
gate — a timeout or a missing `rivet-server` binary is UNVERIFIED (exit 3), not
a FAIL. Every boot gets its own isolated port (Paper's run-dir
`server.properties` is patched; Rivet gets `--port`), so concurrent servers can
never collide. Rivet boots do not need `server.properties` (the binary is
driven purely by `--host`/`--port`).

A Rivet run only passes if the client actually reached the Rivet port. Azalea
fires `Event::Init` before any TCP connect, and `connection_failed`/`timeout`
fire without completing a session, so the transcript alone cannot distinguish a
live pre-play exchange from a dead or hung endpoint. The harness requires both
independent observables: the client transcript outcome is `disconnected`, and
the rivet log shows `connection established` followed by the login listener's
`login state not implemented yet` rejection (issue #96) — lines only the real
`rivet-server` emits for a genuine pre-play exchange.

The runner:

1. Boots a fresh world per run (fixed seed 42 / superflat from
   `rivet-oracle`'s fixtures, `online-mode=false`) and waits for the server's
   READY marker (`Done (...)!` for Paper, `RIVET_READY` for Rivet).
2. Runs the headless client and waits for the stable `joined` record (chunk
   stream quiesced) — or, for Rivet, records the honest pre-play outcome
   (`disconnected`, never `spawned`; `connection_failed`/`timeout` are rejected
   as "client never completed a session").
3. SIGTERM-shuts the server down cleanly and preserves raw diagnostics under
   `work/scenario-join/`, `work/scenario-rivet/`, or `work/scenario-both/`
   (`boot*.log`, `client*.stdout.jsonl`, `client*.stderr.log`,
   `transcript*.json`).
4. Diffs the normalized transcripts field-by-field; requires identical
   transcripts Paper-vs-Paper.
5. Runs a controlled negative case (tampers `position.y`) proving the
   comparator detects a known difference, so the harness cannot pass vacuously.

Exit codes are machine-stable and consumed by gate.sh: `0` PASS, `1` FAIL
(scenario comparison failed, negative case failed, harness error), `3`
UNVERIFIED (missing prereq — paperclip jar / rivet-server binary — or a server
did not reach READY within its boot timeout), `64` invalid CLI arguments.

The normalized `join` transcript shape is documented in
`src/bin/run-scenario/transcript.rs`. The comparator
(`src/bin/run-scenario/comparator.rs`) reports every differing field with its
path and both values, and only skips fields explicitly declared in the
transcript's `excluded` map. The Paper server randomizes the player spawn X/Z
offset per boot, and the received chunk coordinate list is centered on that
randomized spawn chunk, so `position.x`, `position.z`, and `chunks` are
excluded with justification; `position.y` (superflat spawn height), the chunk
count (117), and all other observables are compared and are identical across
fresh boots.
