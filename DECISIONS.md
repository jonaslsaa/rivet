# Rivet Decisions

Binding decisions for the port. Agents: do not relitigate these in PRs; propose changes as issues instead.

## D1 — Goal
Port the Minecraft server **and** Paper's layer on top of it to Rust, faithfully, and run existing Java Paper plugins through an embedded-JVM adapter. Fidelity target: Paper's observable behavior, module structure, algorithms, and constants — not its object graphs (see D4).

## D2 — Version pin
Minecraft/Paper **26.2** (the version in `working/Paper`). No version bumps until M4. The registry/data layer is codegen so future bumps are mechanical.

## D3 — Legal posture

Human decision (2026-08-06): Rivet is published under GPL-3.0-or-later (see `LICENSE`). The license covers Rivet's own contributions only and grants no rights to Mojang-owned material; Mojang's MIT libraries (Brigadier, DataFixerUpper) and Paper's GPL/MIT code are ported directly. No Mojang-derived source (`working/`) is ever committed or pushed.

## D4 — Memory architecture
No garbage collector to lean on, so: **index/arena storage, IDs over references** (see `OWNERSHIP.md`). No full ECS framework — feather died of architecture golf; we keep Paper's structure at module level with arenas underneath.

## D5 — Concurrency model
Single-threaded synchronous game tick owning all world state (matches vanilla's model and Bukkit's main-thread confinement — required for the plugin adapter). `tokio` for network IO at the edges, `rayon` for chunk generation/lighting worker pools; both communicate with the tick thread via channels, never locks on hot game state. Folia-style region threading is explicitly out of scope until after M4.

## D6 — Plugin story
1. **JVM adapter first**: `paper-api` jar unchanged; the implementation layer regenerated as Java shims calling a `rivet-ffi` C ABI (Java FFM/Panama on the Java side — not JNI C glue). Plugin code runs on a dedicated tick-synchronized thread. API-clean plugins only; NMS reflection is out of scope.
2. Native Rust plugin API second, WASM ABI later. Neither before M4.

## D7 — Pumpkin usage policy
`working/Pumpkin` is for **learning and unblocking only**: study its design decisions, consult it when stuck on a parity problem. **Never copy code from it while porting** — our source of truth is Paper's Java. (Rationale: fidelity to Paper is the differentiator; mixing in a second reimplementation's interpretations breaks the "faithful port" verification story.)

## D8 — Verification
The Java server is the oracle; parity is measured, not asserted (`WORKFLOWS.md` → PARITY scoreboard). No agent may weaken a test, fixture, or oracle criterion to go green. `todo!()` requires a `blocked` manifest/issue note.

## D9 — Process
Work is organized as GitHub **epics → agent-created sub-issues → PRs**, milestones M0–M4 (see `WORKFLOWS.md`). Reference docs (`PORTING.md`, `OWNERSHIP.md`) are updated via dedicated PRs, never silently mid-wave. Rust edition 2024, `rustfmt` defaults, clippy clean at wave gates.

## D10 — No hosted CI
No GitHub Actions (or any hosted CI). The merge gate is `scripts/gate.sh` (fmt → clippy `-Dwarnings` → tests → oracle steps: `rivet-oracle verify` and the byte-for-byte `rivet-parity` diff vs the Paper Java oracle), run locally by the controller before merging any PR and at the end of every wave. Oracle verification is never silently skipped: missing prerequisites make the gate exit nonzero with an UNVERIFIED status and a per-item fix list; `--require-oracle` turns any missing oracle prerequisite into a hard failure. A red gate blocks the merge; enforcement is by process (PR checklist + controller), not by GitHub.

## D11 — JVM-adapter FFI design: GO (in-process FFM)
De-risking spike for epic #14 / sub-issue #81 (the `spikes/ffi-latency/` crate + Java FFM benchmark) concluded **GO** on the in-process Java FFM/Panama bridge design (D6). The C-ABI spike measures: scalar downcall, per-call handle-table re-resolution, batched event publish, a Rust→Java→Rust callback round-trip, and back-to-back callback storms. Reproduce with `spikes/ffi-latency/run.sh`, which builds the cdylib, compiles the Java harness, runs the correctness assertions (including exception containment), benchmarks, and writes the machine-readable report to `spikes/ffi-latency/results/benchmark.json`. That report is a **regenerable artifact**: it is gitignored and never committed; any claim in this decision is backed by the recorded run in issue #81, not by a checked-in result file.

Recorded numbers (release build, Temurin 25.0.2 arm64; representative measured ranges across recorded runs, bracketing the exact run in issue #81):
- per-event callback cadence during a 100k storm ~100-120 ns → **~41k-50k events/tick fit the 5 ms dispatch budget** (10% of a 50 ms tick), vs a realistic assumption of ~10k plugin events/tick → **GO**.
- single callback round-trip (Rust→Java→Rust) ~215-250 ns; bulk state mutation via `rfv_apply_events` is far cheaper per event (~0.5-0.56 ns/event at n=4096) and should stay on the batched path.
- the worst-case single 100k-event storm (~10-12.1 ms) does NOT fit the 5 ms budget — recorded honestly; the go/no-go is driven by realistic per-tick volume, not the synthetic storm.
- exception containment is proven: the Java upcall target catches every plugin `Throwable` and returns an explicit ABI-safe status; Rust surfaces it as an error result (`ERR_CALLBACK`), so a foreign exception can never unwind through Rust.

Decisions locked: `rivet-ffi` C ABI with marshal-only u64 IDs (never pointers across the boundary); batched event publishing for bulk state, per-plugin handler dispatch on the callback path; plugin code on a dedicated tick-synchronized thread (D5).

## D12 — CompoundTag is insertion-ordered (NBT key order for byte-identical chunk NBT)
Locked 2026-08-07 for issue #226 (M2 gate preflight). Java's `CompoundTag` wraps an `Object2ObjectOpenHashMap<String, Tag>` (fastutil) that iterates in fastutil hash order; reproducing that byte-for-byte in Rust would require a Java-identical hasher. Rivet instead stores tags in an insertion-ordered `indexmap::IndexMap`:
- the reader (`NbtIo.read`/`load`) inserts keys in on-disk order, so any compound **read** from binary NBT re-emits its exact on-disk field order — a Paper 26.2 chunk fixture round-trips byte-for-byte (golden test `committed_chunk_fixture_round_trips_byte_identical`);
- hand-built compounds (SNBT → binary) emit Rust's put sequence, which differs from fastutil hash order — the documented `compound_key_order` divergence in PARITY.md (soft, `diverged`, never `mismatched`);
- deterministic across processes (no randomized seed).

This makes byte-identical `SerializableChunkData` NBT possible without a fastutil hasher port. The `compound_key_order` oracle divergence is accepted and counted under `diverged`.

## D13 — Region compression: byte-identity gate runs at `region-file-compression=none`
Locked 2026-08-07 for issue #226 (M2 gate preflight). Java `Deflater` output is not `flate2`-reproducible in general, and `RegionFileVersion.DEFAULT = VERSION_DEFLATE` is volatile-selected, so deflate is not a byte-identity mode. The M2 byte-identity round-trip therefore pins **both sides** to `region-file-compression=none` (fixtures/`server.properties` + manifest now record `none`). Read support is mode-separate: `NbtIo.read_compressed` (gzip only, mirroring Java's `NbtIo.readCompressed`) is proven against Paper's own gzip `level.dat` fixture (`reads_real_paper_gzip_level_dat_fixture`); deflate/lz4 reads live in the region-file layer, which is not yet ported. Deflate/lz4 **write** parity is deferred to the chunk.storage wave (issue #231, `RivetTodo` in `nbt_io.rs`); chunk payloads are captured as decompressed NBT so the `none` pin is byte-consistent with existing fixtures. The region-file on-disk format this pins (header, sector layout, codec ids, oversized/`.mcc` stubs, corruption handling) is specified in `docs/region-file-format-spec.md`.
