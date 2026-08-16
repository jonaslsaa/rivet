# CRATES.md — workspace layout & library choices

## Workspace (bottom-up dependency order)

| Crate | Ports | Notes |
|---|---|---|
| `rivet-core` | `net.minecraft` root package | `CrashReport`, `ChatFormatting`, `SharedConstants`, `Util`, root exceptions. Bottom of the stack — keeps module-path mirroring clean instead of scattering root classes ad hoc. |
| `rivet-util` | `net.minecraft.util`, `Mth`, RNG | Java-parity layer: LCG/Xoroshiro128++ RNG, `nextGaussian` quirk, md5 `seedFromHashOf`, 65536-entry sin table, `java_string_hash`, `CubicSpline`/`BoundedFloatFunction` (worldgen value layer, issue #372). Golden-tested against Java fixtures. |
| `rivet-serialization` | Mojang DataFixerUpper (MIT) | `Codec`/`DynamicOps`/`DataResult` shape preserved; serde only beneath external JSON. A leaf: DFU has no Minecraft deps. |
| `rivet-nbt` | `net.minecraft.nbt` | Own port (Java modified UTF-8 via `rivet-util::data_io`); SNBT; fuzz targets. **Depends on `rivet-serialization`** — faithful to Java, where `NbtOps implements DynamicOps<Tag>`. |
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
| `tools/rivet-harness-common` | — | Shared E2E harness primitives (child-process boot lifecycle, held-port reservations, strict JSONL transcripts, exit contract, named negative controls, and attested Cargo artifact provenance); depended on by `rivet-capture` and `rivet-client`. |
| `tools/rivet-oracle` | — | Differential-test harness: runs vanilla/Paper jar, extracts golden fixtures, drives azalea bots, computes PARITY.md. |

Module paths inside crates mirror Java packages (PORTING.md). Crate boundaries exist for compile parallelism and dependency hygiene, not to re-architect.

## External libraries (decided)

**Core:** `tokio` + `tokio-util` (network/IO edges only — D5), `rayon` (worldgen/lighting pools), `crossfire` (mixed sync/async channels between rayon/tick/tokio — proven in Pumpkin's chunk scheduler), `crossbeam` (utilities; **not** per-field `AtomicCell` on game state).

**Data:** `serde` + `serde_json` (configs, datapack JSON — never packets or chunk hot paths), `toml` (config), `slotmap` (entity/holder arenas), `rustc-hash` (FxHashMap default), `indexmap` (order-observable maps), `smallvec`, `bitflags`, `phf` (generated static tables), `uuid`, `bytes`.

**Path-validation regex (issue #340):** `pcre2` is approved for `PathAllowList`. Paper passes `regex:` rules to `FileSystem.getPathMatcher`, whose OpenJDK contract defines them as `java.util.regex.Pattern`; valid rules can therefore require lookaround and numeric or named backreferences. The accepted language is a deliberately Java-compatible subset, not PCRE2's full language: a compatibility validator must reject every construct whose Java meaning has not been established before PCRE2 compiles it. Paper compiles the complete rule list before matching and replaces it with an always-false matcher on any compilation error, so one invalid or unsupported rule must disable the whole list; retaining the other rules would broaden access and is forbidden.

`pcre2-sys` is an accepted native build surface. It uses a system `libpcre2-8` reported by `pkg-config` when available and permitted; otherwise (including static builds) it compiles its vendored PCRE2 C sources and links them statically, so supported builds must provide a compatible C toolchain for that fallback (including MinGW-w64 or equivalent for Windows GNU). Its documented tested platforms—Windows, Linux, and macOS—are Rivet's support expectation for this dependency; another target needs build and matcher-parity evidence before being declared supported. JIT is neither enabled nor required: the Rust wrapper defaults to interpreted matching, and lack of PCRE2 JIT support must not by itself exclude a target.

The `regex` crate and its `regex-automata` engines deliberately omit lookaround and backreferences, and Rust `std` has no regex engine; those choices cannot accept the required Java rules without rejecting valid configuration or implementing another engine. `fancy-regex` provides those constructs, but does not provide a Java `Pattern` contract and executes them through its own backtracking VM while delegating other subexpressions to `regex`; it would still require the same compatibility boundary while putting two matching engines on this authorization path, increasing the parity and security review surface. These alternatives are rejected for `PathAllowList`.

**Formats/compression:** `flate2` (+`async-compression` for the network stream), `md5` (Java seed hashing, not security), `xxhash-rust` (fixture/chunk hashing), `crc32c` (RFC 3720 CRC-32C for `HashOps`'s `DynamicOps<HashCode>` serialization adapter, issue #205 — the byte-identical checksum behind Guava's `Hashing.crc32c()`). Region-file codecs (issue #231, the `world.level.chunk.storage` slice): gzip/deflate **read** reuse `flate2` (gzip already proven via `NbtIo.read_compressed`); lz4 read (and later write) will add `lz4_flex` — pure-Rust LZ4 block codec, MIT, no C toolchain — for the lz4-java "LZ4 Block" format Paper writes, with the per-block xxHash32 checksum (seed `0x9747b28c`) served by `xxhash-rust`. The workspace currently enables only its `xxh3` feature (the #54 chunk-hash engine), so the lz4 wave must add the `xxh32` feature before the checksum can be served. `lz4-sys`/C bindings are rejected. Deflate **write** parity stays deferred under D13 (Java `Deflater` is not `flate2`-reproducible in general; the byte-identity gate pins `region-file-compression=none`). Both directions of modified UTF-8 (`DataOutputStream.writeUTF` / `DataInputStream.readUTF`) are direct in-repo ports of the OpenJDK 25 codec in `rivet-util::data_io` — the `cesu8` crate's Java-variant decoder diverges from `DataInputStream` on `C1 80`, overlong 3-byte forms, and error messages, and its Java-variant encoder would be a second source of truth for the write side (issue #265).

**Crypto (protocol):** RustCrypto stable releases only — `aes`, `cfb8`, `rsa`, `sha1`, `sha2`; `rand` for non-gameplay randomness only (gameplay RNG is `rivet-util`).

**Errors/logging:** `thiserror` (per-crate error enums), `anyhow` (tools only), `tracing` + `tracing-subscriber`.

**Testing/tooling:** `cargo-nextest`, `criterion`, cargo-fuzz on all parsers, `azalea` (dev-dep of `tools/rivet-oracle` — bot driver, current with 26.2), `tempfile`, `chrono` (date stamps in `tools/rivet-parity`'s `--scoreboard`).

**JVM adapter:** no `jni` crate — `rivet-ffi` exposes plain `extern "C"`; the Java side binds via FFM (Panama). Java shims live in `adapter/` (Gradle), not in the cargo workspace.

**Explicitly rejected:** ECS frameworks (bevy_ecs — D4), `dashmap` on tick-thread state, serde-based packet codecs, `async-trait`/boxed futures in game logic, pre-release crypto pins. Each rejection is a Pumpkin scar — see `LESSONS-PUMPKIN.md`.
