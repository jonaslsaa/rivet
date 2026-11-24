# L4 arena tick traces

The gameplay oracle of docs/VERIFICATION.md (section 2.2 L4, item 7). An
arena is a small box of blocks plus an optional script. Paper places it in a
real, freshly booted dedicated server and records the box every tick; Rivet
replays the same arena on its live level runtime and must produce the same
bytes.

| Path | What |
|---|---|
| `<name>.json` | the arena (input; its SHA-256 is pinned in every trace header) |
| `arenas.tsv` | the declared corpus: one row per arena with the Rust files and Java classes it verifies (read by `scripts/verify_ledger.py`) |
| `paper/<name>.trace.jsonl`, `paper/manifest.json` | the twin-admitted Paper recordings (record mode) |
| `exclusions.tsv` | NBT keys both emitters drop; only keys that differ between Paper twin boots |
| `seed-sites.tsv` | the seed-injection sites (VERIFICATION 2.5) |
| `server.properties` | the Paper arena boot (superflat, seed 42, never pauses) |

Code: `src/java/arena/ArenaTraceProbe.java` (Paper plugin),
`scripts/arena_trace_paper.sh` (one fresh Paper boot),
`src/bin/rivet_arena_trace/` (CLI, comparator and the Rivet runner
`runner.rs`, over rivet-server's public level API; nothing of the harness is
compiled into the server library).

## Commands

```sh
cargo run -p rivet-oracle --bin rivet-arena-trace -- check            # offline: Rivet vs committed Paper traces
cargo run -p rivet-oracle --bin rivet-arena-trace -- check --ledger   # ... and write the arena.* rows of VERIFY.tsv
cargo run -p rivet-oracle --bin rivet-arena-trace -- record [--arena N]  # two fresh Paper boots, admit, write paper/
cargo run -p rivet-oracle --bin rivet-arena-trace -- live [--ledger]  # two fresh Paper boots, then Rivet vs those
cargo run -p rivet-oracle --bin rivet-arena-trace -- compare A B      # first divergence between two trace files
cargo run -p rivet-oracle --bin rivet-arena-trace -- rivet --arena N  # print Rivet's trace
```

Paper boots need the materialized runtime (`RIVET_PAPER_RUNTIME_JAR`,
`RIVET_PAPER_LIBRARIES`, default `tools/rivet-oracle/work/run`) and Temurin
25 (`RIVET_JAVA_HOME`/`JAVA_HOME`, else `PATH`); a missing jar, library
directory, `java`/`javac`/`jar`, or a Java other than 25 exits 2 (UNVERIFIED). Boots, Rivet traces and logs
go to `work/arena-trace/` (`RIVET_ARENA_WORK`), never `/tmp`. A full twin
record of all arenas takes about four minutes (the arenas run at 20 TPS).

`check` and `live` exit 0 when the harness holds (closure, admission, the
tamper negative on every trace) and no arena the ledger records as `PASS`
has regressed. A diverging arena is a `FAIL` result with a named first
divergence, not a harness failure. `scripts/gate.sh` runs `check` only when
`RIVET_ARENA_TRACE=1` (opt-in).

## Determinism contract

- **Boot.** Fresh run dir per boot; `paper-global.yml` pins one chunk worker
  and one I/O thread; `paper-world-defaults.yml` caps every spawn limit at 0.
  The probe sets `randomTickSpeed 0`, `advance_time`, `advance_weather` and
  `spawn_mobs` false, and clears weather. No player ever joins.
- **Timeline.** At the heartbeat where the arena's chunks (the box grown by
  one block, force-loaded) are ticking: reseed, place quietly (flags
  `16|32|256|512`: no neighbour or shape updates, no `onPlace`, no drops),
  load the block-entity NBT, run script ops of tick 0, snapshot `t=0`. Each
  later heartbeat runs after exactly one more `ServerLevel.tick`: script ops
  of tick `t`, then snapshot `t`. The Rivet runner uses the same order.
- **Seeds.** `mix(seed, site, ordinal) = mixStafford13(seed + site *
  0x9E3779B97F4A7C15 + ordinal)` at the sites in `seed-sites.tsv`. An arena
  must not exercise an RNG that is not listed there (for example random ticks:
  `simpleRandom` only draws when `randomTickSpeed > 0`).
  VERIFICATION 2.5 also names `level.getThreadSafeRandom()`; Paper 26.2's
  `Level`/`ServerLevel` have no such source, so there is no site for it. On
  the Rust side the reseed is a call on the arena's own level
  (`random_mut().set_seed`) inside the runner, never a global.
- **Entity RNG (site 3).** VERIFICATION 2.5 asks for each entity's `random`
  reseeded at spawn. Paper 26.2 cannot do that: every `Entity.random` is the
  one static `Entity.SHARED_RANDOM`, whose `setSeed` honours only the first
  call per JVM. So each arena installs a fresh seeded source as
  `Entity.SHARED_RANDOM` before placement; every entity constructed while it
  runs shares it, in Paper's own draw order, independent of which arenas ran
  before in the batch. The probe fails if an entity near the box holds any
  other source, or if anything calls `setSeed` on it. A Rivet entity runtime
  must model the same single shared source per arena.
- **Seeded RNG coverage today.** Site 1 is exercised by
  `fire_tick_delay_seeded`: every `FireBlock` reschedule draws
  `level.random.nextInt(10)`, so its scheduled-tick delays are the seeded
  stream and twin admission holds only because the seed takes effect (an
  unseeded level RNG is time-seeded). Site 3 is exercised only by
  `sand_fall` (the falling block's construction-time UUID draw, which the
  trace renumbers). `spawner_idle_seeded` draws nothing: with no player in
  range `BaseSpawner.serverTick` returns before any draw. A spawner that
  spawns needs an arena player, which neither side has yet, so VERIFICATION
  item 7's "spawner with seeded RNG" remains open.
- **Block-entity NBT.** Paper places the block (fresh entity) and then loads
  the arena NBT into it; the Rust runner puts the header's full post-load NBT
  pending before the same quiet placement, which creates the live entity from
  it. Both start tick 0 from the same entity state.
- **Admission.** Two fresh Paper boots must produce byte-identical traces
  before a recording is committed or a live comparison counts.
- **Batch order.** One boot runs the requested arenas in `arenas.tsv` order
  (alphabetical). Each arena reseeds sites 1 and 3 at its own placement, so
  its trace does not depend on the arenas before it.
- **Opt-in.** `opt_in` in an arena turns a frozen mechanic back on. None is
  implemented yet; both sides refuse an arena that sets one.

## Trace schema `rivet.arena-trace/1`

JSON lines. Key order is fixed as listed; a member whose value is null is
written as `null`, never omitted. Positions are relative to the arena origin
`[dx,dy,dz]`; block states are global state ids (`Block.getId`); doubles and
floats are IEEE-754 bit patterns as `0x`-prefixed hex (`doubleToLongBits` /
`floatToIntBits`, NaN canonicalized, D14), including inside NBT: before
printing SNBT both emitters replace every `FloatTag` with the string
`"#f:0x<8 hex>"` and every `DoubleTag` with `"#d:0x<16 hex>"`, and prefix one
more `#` to any real string that starts with `#` (an injective mapping), so
parity never rests on decimal float formatting. The header's
`block_entities` SNBT is the exception: it is the raw input the Rivet runner
loads, not a compared record.

1. `{"kind":"header","schema","arena","arena_sha256","origin","size","ticks","seed","opt_in","placements","block_entities","script"}`
   — the arena resolved by Paper: `placements` `[dx,dy,dz,state]` in placement
   order, `block_entities` `[dx,dy,dz,snbt]` (full NBT after loading),
   `script` ops with state ids. The Rivet runner reads its input from here.
2. `ticks + 1` records `{"kind":"tick","t","blocks","block_ticks","fluid_ticks","block_events","block_entities","entities"}`:
   - `blocks`: `[dx,dy,dz,state]` for every position whose state changed since
     the previous record (record 0: against an all-air box), y, z, x order.
   - `block_ticks` / `fluid_ticks`: every scheduled tick in the box,
     `[dx,dy,dz,type,delay,priority]` with `delay = triggerTick - gameTime`,
     in `ScheduledTick.DRAIN_ORDER` (the absolute sub-tick counter is implied
     by the order, never printed).
   - `block_events`: `[dx,dy,dz,block,b0,b1,added]` for every
     `ServerLevel.blockEvent` queued in the box since the previous record, in
     queue order (Paper side: a recording subclass of the private queue).
   - `block_entities`: `[dx,dy,dz,snbt|null]` for every block entity whose
     `saveWithFullMetadata` SNBT (sorted keys, floats normalized) changed,
     appeared or vanished.
   - `entities`: every entity in the box, `{"n","type","pos","delta","health","nbt"}`;
     `n` is the first-appearance number within the arena, `UUID` is removed
     from `nbt` in favour of `n`, `health` is `null` for non-living entities.
     The Rivet runner binds no entity owner (admission refuses with
     `MissingOwner`); if the level ever holds an entity the runner ends with
     an `error` line, since no Rust row emitter exists yet.
   - Rivet's `block_events` is empty because `ServerLevel` has no block-event
     queue at all; the port that adds `blockEvents` makes the runner read it.
3. `{"kind":"end","ticks"}` — closure: the run reached its declared length.

A Rivet run that hits a missing or refusing authority (an `Unavailable`
error, a block tick without a Rust body in `ServerLevel::tick_block_live`,
an entity it cannot render, a panic in an unported branch) ends
with `{"kind":"error","t","phase","message"}` instead of the record for that
tick. The comparator canonicalizes both lines through one JSON writer and
compares bytes; the first divergence is reported as tick, field path and both
values.

## Adding an arena (for a spine PR)

The arena lands in a verification PR, before or with the port it verifies
(VERIFICATION 3.3): its expectation must predate the implementation.

1. Write `<name>.json`: `name`, `description`, `origin` in an unused chunk
   (current arenas sit at `x = 64 + 32 * i, z = 64`, y 0), `size`, `ticks`,
   `seed`, `fill` (`{"from","to"?,"state"}`, block-state syntax as in
   commands, relative coordinates, applied in order), optional
   `block_entities` (`{"pos","snbt"}`) and `script` (`{"tick","op":"set","pos","state","flags"}`,
   `{"tick","op":"schedule_block","pos","block","delay","priority"?}`,
   `{"tick","op":"schedule_fluid","pos","fluid","delay","priority"?}`).
   Enclose it: nothing may flow or move out of the box. Start RNG-free.
2. Add its row to `arenas.tsv` (the Rust files and Java classes it verifies).
3. `rivet-arena-trace record --arena <name>` (two fresh boots; a probe change
   forces re-recording every arena). If the twins differ, find the source:
   seed it through a new row of `seed-sites.tsv` on both sides, or, only for a
   value Paper itself does not reproduce, add an `exclusions.tsv` row with the
   twin-run evidence. Never in the same PR as the port it would excuse.
4. `python3 scripts/verify_ledger.py` (new `arena.<name>` row), then
   `rivet-arena-trace check --arena <name>` and record the honest result with
   `check --ledger` after committing. A `FAIL` with a named divergence is a
   valid row; the port PR flips it to `PASS`.
5. If the Rust side needs a phase the runner does not drive yet, add the
   production entry point to `ServerLevel` (as `tick_block_live` /
   `run_block_ticks_live` are) and call it from `runner.rs`; the runner never
   dispatches game logic itself. A newly ported scheduled-tick body joins
   `ServerLevel::tick_block_live`.
