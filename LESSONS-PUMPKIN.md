# Lessons from Pumpkin

Distilled from a deep read of `working/Pumpkin` (0.1.0-dev+26.2, ~310k hand-written LOC + 1.42M generated). Per D7: we learn from it and consult it when stuck — we never copy code from it.

## What Pumpkin got right (adopt)

- **Golden-file differential testing.** `assets/tests/` holds 34MB of vanilla-generated expected output (chunk dumps keyed by seed/coords, density-function samples asserted to full f64 precision, 10MB biome-source dumps). `pumpkin-world/src/generation/proto_chunk_test.rs` walks entire chunks asserting zero mismatches. This is our `verify-oracle` template — but they only built it for worldgen; we extend it to redstone, physics, loot, and packet bytes. Their own TODO ("create an extractor for them") marks the gap we close on day one: the fixture **extractor lives in-repo**.
- **The Java-parity utility layer.** `pumpkin-util/src/random/` (java.util.Random LCG, Xoroshiro128++, `nextGaussian` stored-second-value quirk, md5-based `seedFromHashOf`) and `pumpkin-util/src/math/` (65536-entry sin table with vanilla's exact `10430.378` constants, `java_string_hash` with `wrapping_mul(31)`). This is precisely the checklist for our `rivet-util`; theirs has 26 unit tests we can learn scenarios from.
- **Porting Mojang's Codec abstraction, not just the data.** `pumpkin-codecs/` is a DFU port (`DynamicOps`, `DataResult`, `Lifecycle`) — validates our `rivet-serialization` plan.
- **Hybrid protocol strategy**: generated packet *IDs* per protocol version + hand-written packet *bodies* with derive macros (`#[java_packet(...)]`, `derive(PacketRead/PacketWrite)`) for simple cases.
- **Committed generated data behind fine-grained cargo features** (`pumpkin-data`, ~50 features) — but their 22.9MB `block.rs` hurts build times; we should prefer compact tables/binary blobs where possible.
- **Lint posture tuned for faithful Java semantics**: `cast_possible_truncation/wrap`, `float_cmp` allowed; `todo!`/`print_stdout` denied. CI chain `fmt → cargo-machete → clippy → nextest` multi-arch; cargo-fuzz targets on every parser (NBT, packet codecs).
- **AI-agent PR etiquette** in their CONTRIBUTING.md (agents tag PRs, warning about duplicate agent PRs) — precedent for our process.

## What Pumpkin got wrong (avoid — these validate DECISIONS.md)

- **Entity storage: `ArcSwap<Vec<Arc<dyn EntityBase>>>` + per-field atomics** (~120-field Entity struct where every scalar is an `AtomicCell`). Consequences they document themselves: O(n) clone of the whole Vec on every spawn/despawn (`world/mod.rs` `.rcu()` sites), linear scans for every ID/UUID lookup, and an admitted ABA hazard: *"These should be atomic together… can cause ABA issues"* (`entity/player.rs:3619`). → D4: slotmap arenas, IDs, single-writer.
- **Fully async parallel tick**: one tokio task per entity/player/block-entity/random-tick per tick (`JoinSet`s in `World::tick`), 1,719 `Box::pin` allocations for CPU-bound game logic, and **16 explicit lock-ordering/deadlock-avoidance comments** scattered through the tree. Worse for us: it discards vanilla's deterministic tick *order*, which Paper behavior (redstone, collisions, AI targeting) observably depends on — parity becomes untestable. → D5: sync tick, tokio at the edges only. Their own `chunk_system/schedule.rs` (dedicated OS thread + `crossfire` channels) is the pattern that works — they just only used it for chunks.
- **serde on hot paths** — reverted for chunk I/O (commit `7350fba3`). Packets never used it. → serde for config/datapack JSON only.
- **Two coexisting chunk architectures** (old DashMap path + new DAG/slotmap `chunk_system/`) because the pipeline was designed twice. Design ours once.
- **No end-to-end harness at all** — no client library in the dep tree, no test that connects and plays; outside worldgen, their 482 parity TODOs are the *only* record of divergence. → azalea harness at M0, PARITY scoreboard.
- Pinned pre-release crypto crates (`rsa =0.10.0-rc.18` etc.) — a standing upgrade liability; we use stable RustCrypto.
- 1,431 `unwrap()`s in non-generated code; god-modules (5.6k-line `world/mod.rs`). Faithful porting of Paper's file structure naturally avoids the latter.

## Directly useful reference points when stuck (D7 consultation targets)

- RNG/parity edge cases: `pumpkin-util/src/random/*.rs` test vectors.
- Worldgen fixture format + harness shape: `pumpkin-world/src/generation/proto_chunk_test.rs`, `assets/tests/`.
- Multi-version packet body branching: `pumpkin-protocol/src/java/client/play/entity_velocity.rs`.
- Rayon↔tokio bridging rules: their CONTRIBUTING.md "Working with Tokio and Rayon" section; `crossfire` usage in `pumpkin-world/src/chunk_system/schedule.rs`.
