# FFI latency spike (epic #14, sub-issue #81)

Smallest reproducible Java 22+ FFM <-> Rust cdylib round-trip benchmark, aligned
with Rivet's eventual tick-synchronized in-process JVM plugin adapter
(OWNERSHIP.md "JVM adapter boundary"; WORKFLOWS.md "JVM plugin adapter track").

This crate is intentionally **outside** the cargo workspace and outside
`crates/` — it de-risks the FFI boundary only; no production adapter
architecture is built here.

## Layout

```
src/lib.rs            # Rust cdylib: minimal C ABI (init/tick/publish/handle-lookup/callback)
java/rivet/ffi/RivetFfi.java   # FFM (Panama) bindings, upcall stub
java/rivet/ffi/Benchmark.java  # correctness assertions + benchmark + JSON report
run.sh                # build + compile + run (the exact commands are below)
results/              # machine-readable JSON output (regenerable, gitignored)
```

## C ABI (`#[repr(C)]`-safe, fixed-width ints only)

| fn | signature | purpose |
|---|---|---|
| `rfv_api_version` | `() -> u32` | ABI version check |
| `rfv_create_world` / `rfv_destroy_world` | `() -> u64` / `(u64) -> i32` | handle-table lifecycle |
| `rfv_spawn_entity` / `rfv_free_entity` | `(u64) -> u64` / `(u64,u64) -> i32` | generational `EntityId` (gen<<32\|index) |
| `rfv_get_entity_x` / `rfv_set_entity_x` | `(u64,u64) -> i32` / `(u64,u64,i32) -> i32` | re-resolved per-call handle lookup |
| `rfv_tick` | `(u64) -> u64` | bare scalar downcall |
| `rfv_apply_events` | `(u64, *const Event, usize) -> i64` | batched mutation |
| `rfv_register_callback` | `(u64, u64, u64) -> i32` | install FFM upcall stub |
| `rfv_dispatch_callback` | `(u64,u64,i32,i64) -> i32` | Rust -> Java -> Rust round-trip |
| `rfv_event_storm` | `(u64,u64,usize) -> u64` | N synchronous Rust->Java callbacks |

`Event` struct layout: `u64 entity, i32 event_id, [4 pad], i64 payload` (24 bytes).
IDs are marshaled, never pointers into Rust arenas (OWNERSHIP §JVM-adapter).

### Callback status contract (exception containment)

The Java upcall target (`RivetFfi.onCallback`) is an `extern` boundary guard: it
catches **every** plugin `Throwable` and returns an explicit status code, so a
foreign exception can never unwind through Rust. The C ABI mirrors three statuses
in both `lib.rs` and `RivetFfi.java`:

| status | value | meaning |
|---|---|---|
| `OK` | 0 | callback dispatched cleanly |
| `ERR_NO_CALLBACK` | -1 | no callback registered on the world |
| `ERR_CALLBACK` | -2 | the Java callback threw; exception contained on the Java side |

`rfv_dispatch_callback` returns one of these. `rfv_event_storm` aborts the loop
at the first event whose callback returns nonzero and returns the count actually
dispatched (0 on the first throw), instead of unwinding through Rust.

## Exact commands

```bash
# 1. Build the Rust cdylib (release)
cargo build --release --manifest-path Cargo.toml

# 2. Compile the Java benchmark (JDK 22+ with FFM/Panama)
mkdir -p java/out
javac --release 22 -d java/out $(find java -name '*.java')

# 3. Run (the dylib path is resolved at runtime, never hard-coded)
OUT=results/benchmark.json java \
  -Dffi.lib="$PWD/target/release/libffi_latency_spike.dylib" \
  -Dout="$OUT" --enable-native-access=ALL-UNNAMED \
  -cp java/out rivet.ffi.Benchmark

# or, all of the above:
./run.sh
```

Requires `$HOME/.cargo/bin` on PATH for cargo (this repo pins rust 1.97.1 via
`rust-toolchain.toml`). JDK tested: Temurin 25.0.2 (arm64); the FFM API used is
stable since JDK 22.

## What it measures

- **startup**: process init ns + cold first scalar call ns (JVM/JIT startup,
  distinct from steady state)
- **scalar_call**: `rfv_tick` downcall, steady-state percentiles
- **handle_lookup**: `rfv_get_entity_x` with per-call re-resolution
- **batched_publish**: `rfv_apply_events` for batch sizes 1..4096, per-event cost
- **callback_roundtrip**: `rfv_dispatch_callback` with a callback registered on
  the world — Rust->Java->Rust (the Java callback calls back into Rust via
  `rfv_tick`). Fails loudly (rc != 0) if no callback is registered.
- **event_storm**: `rfv_event_storm` for 1k/10k/100k; per-event latency from
  callback arrival times; events/sec
- **verdict**: per-tick capacity vs realistic plugin volume (see below)

Correctness assertions (ABI version, handle set/get, stale-generational-handle
rejection, batch apply counts, callback R->J->R tick, **deliberately throwing
callback containment**) run **before** any timing and abort the run on failure.
The throwing-callback case installs a sink that raises a `RuntimeException`,
asserts `dispatchCallback` returns `ERR_CALLBACK`, asserts the storm aborts at
event 0, and asserts the world remains fully functional afterwards.

## Per-tick budget (go/no-go criterion)

Minecraft ticks at 20 TPS -> 50 ms/tick. The adapter's synchronous event
dispatch is budgeted at **10% of the tick (5 ms)**. The verdict weighs two
observations:
- `verdict.fits_100k_storm_in_budget` — whether a single worst-case 100k-event
  storm's total dispatch time stays under the 5 ms budget (reported honestly;
  this strict synthetic storm does NOT fit).
- `verdict.max_events_per_tick_in_budget` — the per-event callback cadence
  (from the storm) extended to the 5 ms budget, compared against a realistic
  plugin volume of ~10k events/tick (`verdict.fits_realistic_volume`).

`go_no_go` is `GO` when the realistic volume fits: the storm cadence
(~100-120 ns/event) leaves ~41k-50k events/tick of headroom, an order of
magnitude above sane plugin volume. The same numbers give the headroom for
steady-state per-event latency against the remaining 45 ms of game logic.
