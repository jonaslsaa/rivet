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
tools/rivet-client/run-scenario.sh move --server both --pairs paper:rivet  # Paper-vs-Rivet movement differential (issue #53)
tools/rivet-client/run-scenario.sh join --server rivet            # Rivet headless boot: pinned Azalea completes login/config/spawn, exact 117-chunk send-set
tools/rivet-client/run-scenario.sh join --server both --pairs paper:rivet  # Paper-vs-Rivet play scenario
tools/rivet-client/run-scenario.sh dwell --server rivet           # wall-clock keepalive survival past the 30 s kick limit (issues #157/#160)
tools/rivet-client/run-scenario.sh load-world                     # copy the local 26.2 save, prove immutability, probe the future #339 launch seam
tools/rivet-client/run-scenario.sh capture        # one boot; print the normalized transcript
```

`load-world` is the independent #316 harness slice. `RIVET_WORLD_SRC` may
override the default launcher save at
`~/Library/Application Support/minecraft/saves/New World`. The runner refuses
symlinks, copies the source beneath a fresh unpredictable private (`0700`)
directory using descriptor-relative no-follow operations, verifies the copy
byte-for-byte, and retains storage, private-parent, and world directory
descriptors through server shutdown. Cleanup first proves the visible private
parent still has the created device/inode; a missing, symlinked, or substituted
entry is leaked rather than recursively deleted. On Linux, Rivet receives the
inherited `/proc/self/fd/<n>` identity, so replacing the visible pathname cannot
redirect the verified root. macOS does not provide a traversable directory-fd
pathname;
there the server receives the unpredictable private path, which closes the old
deterministic-path window but does not defend against a malicious same-user
process that discovers and races that path. Source traversal and copying open
each entry relative to a retained directory with `O_NOFOLLOW` (and Linux
`openat2` beneath/no-symlink/no-magic-link/no-cross-mount restrictions), so a
symlink replacement is never followed. The before/after fingerprints are two
non-atomic snapshots: they detect differences visible at those snapshot times,
not every transient modify-and-restore event or every intermediate state of a
concurrently changing tree. A same-uid process can still rename or mutate
entries between descriptor-relative operations; the harness fails closed when
an operation or identity check observes the race, but portable Unix APIs do not
provide an atomic external-tree snapshot or unlink-by-fd. Until #339 provides
the world-path/loading capability and official-client acceptance, this command
exits `3` UNVERIFIED; it never turns an accepted argument into a fake PASS.
Because the probe starts no client, explicit `--username` and
`--timeout-seconds` options are rejected instead of being silently ignored.

Modes (`--server` selects which servers boot, `--pairs` selects the comparison):

| `--server` | `--pairs` | What runs |
|---|---|---|
| `paper` (default) | `paper:paper` (default) | `join` Paper-vs-Paper self-check: `--runs` Paper boots must produce identical transcripts, plus the tamper negative case. Paper-vs-Paper modes compare a build against itself, so they boot whatever Paper build the paperclip produces and do not require the oracle pin. |
| `paper` | `paper:paper` | `move` Paper-vs-Paper movement self-check (issue #53): each boot drives the client's bounded forward walk (`move` mode) and `--runs` Paper boots must produce identical normalized movement transcripts (per-tick spawn-relative deltas, velocity, on-ground, teleport/keepalive echo relationships), plus the tamper negative case. This validates the movement harness against Paper. |
| `rivet` | `paper:rivet` | `join` Rivet headless boot (issue #192): `--runs` rivet-servers, each must reach `RIVET_READY`, take the pinned Azalea client through offline login, configuration (registry sync), the play handoff, and spawn, receiving exactly the deterministic 117-chunk send-set, and shut down cleanly on SIGTERM. |
| `rivet` | (rejected) | `dwell` wall-clock keepalive-survival gate (issues #157/#160: keepalive survival + terminal M1 gate): one Rivet boot, the pinned Azalea client spawns into PLAY and stays connected for `--dwell-seconds` (default 41) of wall-clock time while azalea auto-echoes every live keepalive challenge. The run passes only if the client survived past the server's 30 s keepalive kick limit, proven four ways: the rivet log's `connection established` line (only the real rivet-server emits it), the rivet log containing no `read timeout` kick (the server never disconnected the client for failing its keepalive), the client transcript passing `rivet_dwell_verdict` (outcome `dwelled`, lifecycle containing login and spawn, the pinned Azalea revision, `connected_wall_seconds` beyond the kick limit, >= 30 challenges, a 1:1 challenge->echo pairing, and a challenge span across the window), and a controlled negative that tampers `connected_wall_seconds` to 0 and requires the real verdict path to refuse PASS — so wall-clock survival cannot pass vacuously. `dwell` has no comparison concept, so `--runs` and `--pairs` are both rejected (exit 64) rather than silently ignored — any explicit `--runs`, even `--runs 1` (equal to the implicit default), is rejected just like `--pairs`, because the one Rivet boot is the whole run and a caller-supplied value that changes nothing must not pass silently. `--dwell-seconds` is likewise dwell-only: an explicit value on `join`/`move`/`capture` is a silent no-op and is rejected (exit 64). The window must be at least 35 s (the 30 s kick limit plus the ~1.2 s first-challenge offset and margin — a 31 s window would span only ~29.8 s of challenges and fail the verdict), and `--timeout-seconds` must exceed it by more than 6 s (the client's 1 s keepalive settle loop plus pre-spawn login/configuration time), so a too-tight timeout cannot cut the client off before it emits the `dwell` record. |
| `both` | `paper:rivet` | `join` Paper-vs-Rivet play scenario (issue #192, inverted by #159): Paper and Rivet boot on isolated ports, the client joins each, and both must reach spawn. This is the load-bearing provenance path: after the Paper boot, the materialized server jar is verified to carry the pinned oracle commit (exit 3 UNVERIFIED otherwise), so the differential is always against the same reference the oracle gate uses. Both servers boot the single-stone superflat fixture, so the spawn height `position.y` is compared; the transcripts must differ only on the excluded per-boot nondeterminism plus the one documented Rivet/Paper gap (the health component default) — any other divergence FAILS the run. A controlled negative then tampers the compared `position.y` on the Paper reference (offset above both spawn heights) and requires the real comparator/divergence path to report the tampered value and refuse PASS, so the acceptance cannot pass vacuously or by silently excluding position.y. `--runs` is rejected here (it always boots exactly one Paper + one Rivet). |
| `both` | `paper:rivet` | `move` Paper-vs-Rivet authoritative movement differential (issue #53): Paper and Rivet boot on isolated ports with the single-stone superflat fixture (so Paper's spawn y aligns with Rivet's -63.0), each drives the client's bounded forward walk, and both must produce the `moved` record. The normalized movement transcripts are compared field-by-field with no documented gap — the sampled walk geometry, velocity, teleport ack echo, and the final sent position `walk.last_sent` are deterministic and Paper-vs-Rivet equal — so any compared-field divergence FAILS the run. Paper provenance is verified from the materialized jar (same as `join`). The Rivet connection is proven two ways: the rivet log's `connection established` line, and the `RIVET_TRACE_MOVEMENT=1` authoritative movement trace parsed from the same log — teleport ack accepted at spawn, accepted-move counter matching the record trail, and final authoritative position matching the client's `last_sent` modulo in-flight frames. The trace's authoritative position is absolute world coordinates while `last_sent` is spawn-relative, so the runner adds the client's carried full-precision `walk.spawn_origin` (excluded from parity) back to `last_sent` before the cross-check, and fails loudly if it is missing — the comparison never assumes the player spawned at (0, 0) — so the compared evidence is Rivet's server-side movement, not a client-side artifact. A controlled negative then tampers the compared `walk.last_sent.x` on the Paper reference (a +1.0 offset that cannot collide with Rivet's recorded value) and requires the real comparator/divergence path to report the tampered leaf and refuse PASS, so the acceptance cannot pass vacuously or by silently excluding `walk.last_sent`. `--runs` is rejected here (it always boots exactly one Paper + one Rivet). |

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
   forward walk and waits for the `moved` record; `dwell` spawns into PLAY and
   waits for the `dwelled` record after `--dwell-seconds` of wall clock). For
   Rivet the transcript must pass the corresponding verdict
   (`rivet_play_verdict` for join/move, `rivet_dwell_verdict` for dwell) —
   `connection_failed`/`timeout` and any `disconnected` pre-play outcome are
   rejected as "client never completed login/configuration into play".
3. SIGTERM-shuts the server down cleanly and preserves raw diagnostics under
   `work/scenario-join/`, `work/scenario-move/`, `work/scenario-rivet/`,
   `work/scenario-both/`, or `work/scenario-dwell/` (`boot*.log`,
   `client*.stdout.jsonl`, `client*.stderr.log`, `transcript*.json`).
4. Diffs the normalized transcripts field-by-field; requires identical
   transcripts Paper-vs-Paper.
5. Runs a controlled negative case proving the acceptance path is non-vacuous:
   tampers `position.y` for `join`, a sampled position `walk.samples[60].dx` for
   `move`, or the compared survival scalar `connected_wall_seconds` (to 0, where
   the dwell verdict must refuse PASS) for `dwell`.

Exit codes are machine-stable and consumed by gate.sh: `0` PASS, `1` FAIL
(scenario comparison failed, negative case failed, harness error), `3`
UNVERIFIED (missing prereq — paperclip jar / rivet-server binary — or a server
did not reach READY within its boot timeout), `64` invalid CLI arguments.

The normalized `join`, `move`, and `dwell` transcript shapes are documented in
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
