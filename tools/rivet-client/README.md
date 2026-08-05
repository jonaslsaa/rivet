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
spawn phase times out, `1` after a TCP preflight failure or disconnect before
spawning, and `64` for invalid CLI
arguments. Cargo writes build diagnostics to stderr; the client protocol on
stdout remains JSON Lines.
