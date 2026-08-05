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

`run-scenario` is the Paper-vs-Paper differential harness for the `join`
scenario (WORKFLOWS.md §headless-client-driver). It boots a local Paper server
headlessly (the `verify` pattern from `rivet-oracle`), joins it with the Azalea
client, captures a normalized observable transcript, and compares transcripts
across Paper boots.

```sh
tools/rivet-client/run-scenario.sh join            # 2 fresh Paper boots, must be identical
tools/rivet-client/run-scenario.sh join --runs 3  # more boots
tools/rivet-client/run-scenario.sh capture        # one boot; print the normalized transcript
```

The runner:

1. Boots a fresh Paper world per run (fixed seed 42 / superflat from
   `rivet-oracle`'s fixtures, `online-mode=false`) and waits for `Done`.
2. Runs the headless client and waits for the stable `joined` record (chunk
   stream quiesced).
3. SIGTERM-shuts the server down cleanly and preserves raw diagnostics under
   `work/scenario-join/` (`boot*.log`, `client*.stdout.jsonl`,
   `client*.stderr.log`, `transcript*.json`).
4. Diffs the normalized transcripts field-by-field; requires identical
   transcripts Paper-vs-Paper.
5. Runs a controlled negative case (tampers `position.y`) proving the
   comparator detects a known difference, so the harness cannot pass vacuously.

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
