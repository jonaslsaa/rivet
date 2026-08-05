# PORTING.md — Java → Rust pattern map

The canonical translation guide. Every implementer and reviewer prompt includes this file. When a bug class recurs, the fix lands *here* first (via a dedicated PR), then affected units are re-run.

## Module & naming conventions

- Java package → Rust module path: `net.minecraft.world.entity.monster` → `rivet_entity::monster`. Crate boundaries per `CRATES.md`/workspace.
- One Java class → one Rust module file of the same name in snake_case (`ServerPlayer.java` → `server_player.rs`), unless the class is trivial (<50 lines) and package-private — then fold into the parent module.
- Keep Java names translated only by case convention (`getBlockState` → `block_state` for getters, `setBlockState` → `set_block_state`). Do not "improve" names — greppability against the Java source is a feature.
- Method overloading: suffix by discriminating arg (`teleport_to`, `teleport_to_with_rotation`). Constructors: `new`, then `new_with_*` / `from_*`.

## Types

| Java | Rust | Notes |
|---|---|---|
| `int`, `long` | `i32`, `i64` | **All arithmetic wrapping** unless proven range-safe: `wrapping_add/mul/...` or `Wrapping<T>`. Overflow panics in debug are ports bugs. |
| `>>`, `>>>` | `>>` on `iN` / on `uN` (cast) | Java `>>>` = logical shift: `((x as u32) >> n) as i32`. |
| `float`, `double` | `f32`, `f64` | Keep precision level exactly — do not widen f32 math to f64. `Math.floor(d)` then `(int)` cast: Java float→int casts **saturate**; Rust `as` also saturates (post-1.45) — OK, but keep NaN→0 semantics in mind. |
| `boolean`, `byte`, `short`, `char` | `bool`, `i8`, `i16`, see below | Java `byte` is signed. |
| `String`, `char` | `String`/`&str`, but | Java is UTF-16: `length()`/`charAt`/`substring` indices are UTF-16 code units. Where indices are observable (chat, NBT string limits, command parsing) count UTF-16 units, not bytes. |
| `T[]`, `List<T>` | `[T; N]`/`Vec<T>` | |
| `Optional<T>` / nullable | `Option<T>` | Every dropped Java null-check is a review finding. |
| `Map`/`Set` | `HashMap`/`HashSet` (hashbrown via std) | **Iteration order differs from Java.** If order is observable (serialization, ticking, packets), flag it: use `IndexMap`, a sort, or document why order can't matter. |
| `UUID` | `uuid::Uuid` | Java `UUID(mostSig, leastSig)` ordering. |

## Structure

- **Class hierarchy → struct embedding + trait.** Superclass fields become an embedded struct field named after the parent: `struct Zombie { monster: Monster, ... }` (chain continues downward). Overridable behavior becomes a trait per hierarchy root (`trait EntityBehavior`) with default methods for base implementations; `super.foo()` → explicit call to the parent's fn. Storage/dispatch is defined in `OWNERSHIP.md` — do not choose per-unit.
- Interfaces → traits. Abstract classes → embedded struct + trait with required methods.
- Inner/anonymous classes → nested structs or closures; static nested → sibling module.
- `static final` constants → `const`/`static`; static mutable state / static init blocks → `LazyLock` (and note it — static init order dependencies are a drift risk).
- `synchronized`/`volatile`: game-state code is tick-thread-confined (D5) — usually drop with a `// tick-thread` note; genuinely cross-thread state (network, chunk IO) → explicit `Mutex`/`RwLock`/atomics. Never silently drop.
- Checked exceptions → `Result<T, E>` with `thiserror` enums per crate. `IllegalStateException`-style unchecked → `panic!` only where vanilla crashing is the actual observable behavior; otherwise `Result`.
- `equals`/`hashCode` → `PartialEq`/`Hash` derives when value-semantic; identity-semantic Java classes (`==` on references) compare by arena ID, never derive blindly.
- Java streams / iterators → iterator chains; beware eager vs lazy: `unwrap_or(f())` evaluates eagerly — use `unwrap_or_else`.
- `instanceof` chains → trait downcast (`as_any`) or enum match per `OWNERSHIP.md` strategy for that hierarchy.

## Minecraft-specific

- **RNG parity is sacred.** `LegacyRandomSource` (java.util.Random LCG), `XoroshiroRandomSource`, and `Mth.sin`'s 65536-entry lookup table are ported bit-exactly in `rivet-util` with golden tests against Java-generated fixtures. Never substitute `rand` or `f32::sin` in gameplay/worldgen code.
- Hashing used in game logic (e.g. `String.hashCode`, position hashes) is ported exactly — Java's `hashCode` algorithms live in `rivet-util::java_hash`.
- Codec/DataFixerUpper: `rivet-serialization` ports the DFU Codec API shape (MIT). External formats (JSON datapacks, configs) may use `serde` underneath, but Codec semantics (error accumulation, partial results) are preserved.
- NBT: `rivet-nbt`, ported types + SNBT; golden round-trip tests against real region files.
- Text components: port Paper's Adventure usage (Adventure is MIT) as `rivet-text`.
- Registries/data-driven content: **generated, not hand-ported** — `rivet-registry` codegen from vanilla data extraction. If you're hand-typing block properties, stop.

## Forbidden

- `todo!()`/`unimplemented!()` without a `blocked` note in the manifest/issue.
- Inventing APIs not present in the Java source; "improving" logic during translation (file an issue instead).
- Copying code from `working/Pumpkin` (D7).
- Weakening/deleting tests or fixtures to pass gates (D8).
- `git stash`, `git reset`, force-push, editing reference docs mid-wave.
- `unsafe` outside `rivet-ffi` without a justifying comment and reviewer sign-off.

## Review drift checklist

Reviewers hunt these specifically: wrapping arithmetic omissions · `>>>` mishandling · HashMap order observability · UTF-16 index drift · f32/f64 widening · `Mth` table vs libm calls · dropped null-checks · eager `unwrap_or` · identity-vs-value equality · dropped `synchronized` without note · side effects in `debug_assert!` · `as` casts changing overflow/NaN semantics · off-by-one from Java's inclusive/exclusive idioms.
