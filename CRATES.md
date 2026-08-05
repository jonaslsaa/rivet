# CRATES.md — workspace layout & library choices

## Workspace (bottom-up dependency order)

| Crate | Ports | Notes |
|---|---|---|
| `rivet-core` | `net.minecraft` root package | `CrashReport`, `ChatFormatting`, `SharedConstants`, `Util`, root exceptions. Bottom of the stack — keeps module-path mirroring clean instead of scattering root classes ad hoc. |
| `rivet-util` | `net.minecraft.util`, `Mth`, RNG | Java-parity layer: LCG/Xoroshiro128++ RNG, `nextGaussian` quirk, md5 `seedFromHashOf`, 65536-entry sin table, `java_string_hash`. Golden-tested against Java fixtures. |
| `rivet-serialization` | Mojang DataFixerUpper (MIT) | `Codec`/`DynamicOps`/`DataResult` shape preserved; serde only beneath external JSON. A leaf: DFU has no Minecraft deps. |
| `rivet-nbt` | `net.minecraft.nbt` | Own port (Java modified-UTF-8 via `cesu8`); SNBT; fuzz targets. **Depends on `rivet-serialization`** — faithful to Java, where `NbtOps implements DynamicOps<Tag>`. |
| `rivet-text` | Adventure (MIT) usage in Paper | Components, legacy `§` codes. |
| `rivet-brigadier` | Mojang Brigadier (MIT) | Direct port. |
| `rivet-registry` | `core.registries`, `src/generated` | **Generated** by `tools/rivet-codegen` from extracted vanilla data; committed; feature-gated; prefer compact tables over huge Rust source. |
| `rivet-protocol` | `net.minecraft.network` | Generated packet IDs, hand-ported bodies + derive macros; no serde. |
| `rivet-world` | `world.level`, chunks, worldgen, lighting | Single chunk pipeline design (DAG + tickets), rayon workers. |
| `rivet-entity` | `world.entity` hierarchy | `AnyEntity` enum + embedded base structs per OWNERSHIP.md. |
| `rivet-server` | `server.*`, Paper `src/main`, patches | The binary; tick loop, connections, events, Paper config. |
| `rivet-api` | `paper-api` shape | Native Rust API surface (M4). |
| `rivet-ffi` | — | C ABI facade for the JVM adapter; the only crate where `unsafe` is normal. |
| `tools/rivet-codegen` | — | Excluded from workspace; data extraction + code generation. In-repo, documented (unlike Pumpkin's out-of-tree extractor). |
| `tools/rivet-oracle` | — | Differential-test harness: runs vanilla/Paper jar, extracts golden fixtures, drives azalea bots, computes PARITY.md. |

Module paths inside crates mirror Java packages (PORTING.md). Crate boundaries exist for compile parallelism and dependency hygiene, not to re-architect.

## External libraries (decided)

**Core:** `tokio` + `tokio-util` (network/IO edges only — D5), `rayon` (worldgen/lighting pools), `crossfire` (mixed sync/async channels between rayon/tick/tokio — proven in Pumpkin's chunk scheduler), `crossbeam` (utilities; **not** per-field `AtomicCell` on game state).

**Data:** `serde` + `serde_json` (configs, datapack JSON — never packets or chunk hot paths), `toml` (config), `slotmap` (entity/holder arenas), `rustc-hash` (FxHashMap default), `indexmap` (order-observable maps), `smallvec`, `bitflags`, `phf` (generated static tables), `uuid`, `bytes`.

**Formats/compression:** `flate2` (+`async-compression` for the network stream), `cesu8`, `md5` (Java seed hashing, not security), `xxhash-rust` (fixture/chunk hashing).

**Crypto (protocol):** RustCrypto stable releases only — `aes`, `cfb8`, `rsa`, `sha1`, `sha2`; `rand` for non-gameplay randomness only (gameplay RNG is `rivet-util`).

**Errors/logging:** `thiserror` (per-crate error enums), `anyhow` (tools only), `tracing` + `tracing-subscriber`.

**Testing/tooling:** `cargo-nextest`, `criterion`, cargo-fuzz on all parsers, `azalea` (dev-dep of `tools/rivet-oracle` — bot driver, current with 26.2), `tempfile`.

**JVM adapter:** no `jni` crate — `rivet-ffi` exposes plain `extern "C"`; the Java side binds via FFM (Panama). Java shims live in `adapter/` (Gradle), not in the cargo workspace.

**Explicitly rejected:** ECS frameworks (bevy_ecs — D4), `dashmap` on tick-thread state, serde-based packet codecs, `async-trait`/boxed futures in game logic, pre-release crypto pins. Each rejection is a Pumpkin scar — see `LESSONS-PUMPKIN.md`.
