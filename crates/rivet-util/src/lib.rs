//! `net.minecraft.util` port surface.
//!
//! `mth` is a full port of `net.minecraft.util.Mth` (arithmetic, trig tables,
//! lerp/clamp/floorDiv, packing helpers), golden-tested against Java. `random`
//! ports `net.minecraft.util.random` (Xoroshiro/Legacy RNG), also
//! parity-tested. `worldgen_random` ports `net.minecraft.world.level.levelgen
//! .WorldgenRandom` (the worldgen seed decorator + `Algorithm`), on top of the
//! `random` unit. `java_hash` ports the JDK string hashing used by
//! `ResourceLocation`.
//!
//! `data_io` / `delegate_data_output` / `fast_buffered_input_stream` provide
//! the byte-IO contract `NbtIo` needs: big-endian primitives and in-repo
//! modified-UTF-8 strings (write + read sides, both OpenJDK-25-faithful — the
//! `cesu8` crate's Java-variant decoder rejects the overlong forms Java accepts
//! and has no byte-offset diagnostics), the write-delegate, and the 8KB
//! compressed-read buffer.
//!
//! `util` is a partial port of `net.minecraft.util.Util` for the registry-core
//! slice (issue #107 / #122); `string_representable` and `by_id_map` are full
//! ports of their `mc.util` classes. `hash_ops` is a full port of
//! `net.minecraft.util.HashOps` (the `DynamicOps<HashCode>` DFU serialization
//! adapter, issue #205). See each module's provenance header.

pub mod bit_storage;
pub mod bounded_float_function;
pub mod by_id_map;
pub mod cubic_spline;
pub mod data_io;
pub mod delegate_data_output;
pub mod fast_buffered_input_stream;
pub mod hash_ops;
pub mod java_float_format;
pub mod java_hash;
pub mod known_pack;
pub mod mth;
pub mod mth_atan_tables;
pub mod mth_sin_table;
pub mod mth_stubs;
pub mod problem_reporter;
pub mod random;
pub mod simple_bit_storage;
pub mod string_representable;
pub mod util;
pub mod worldgen_random;
pub mod zero_bit_storage;

pub use bit_storage::BitStorage;
pub use bounded_float_function::{
    BoundedFloat, BoundedFloatFunction, Comapped, Constant, Identity,
};
pub use by_id_map::{OutOfBoundsStrategy, continuous, sparse};
pub use cubic_spline::{
    Builder as CubicSplineBuilder, CubicSpline, Multipoint as CubicSplineMultipoint,
    Point as CubicSplinePoint, Sampler as CubicSplineSampler, fmt_f32_3 as fmt_java_3,
};
pub use data_io::{DataInput, DataInputStream, DataOutput, DataOutputStream};
pub use delegate_data_output::DelegateDataOutput;
pub use fast_buffered_input_stream::FastBufferedInputStream;
pub use hash_ops::{HashCode, HashFunction, HashOps, Hasher};
pub use known_pack::KnownPack;
pub use problem_reporter::{
    Collector, DiscardingReporter, FieldPathElement, IndexedFieldPathElement, IndexedPathElement,
    PathElement, Problem, ProblemReporter,
};
// `BitRandomSource` is deliberately NOT re-exported at the root: it declares
// `next_int`/`next_long`/... (same names as `RandomSource`), so importing both
// makes every LCG call ambiguous (E0034). It lives at `rivet_util::random`.
pub use random::{PositionalRandomFactory, RandomSource};
// Module alias for the generated `mth_golden_tests.rs`, which references the
// RNG unit as `crate::random_source` (Java class `RandomSource`).
pub use random as random_source;
pub use string_representable::StringRepresentable;
pub use util::{
    LazyValueMap, fixed_size, fixed_size_i32, fixed_size_i64, get_random, get_random_safe,
    log_and_pause_if_in_ide, shuffle, shuffled_copy,
};
pub use worldgen_random::{Algorithm, WorldgenRandom};

/// `Mth.floor(float v)` = `(int)Math.floor(v)`. Rust's `as` saturates and maps
/// NaN to 0 exactly like the Java float->int cast (PORTING.md).
pub fn floor(v: f32) -> i32 {
    v.floor() as i32
}

/// `Mth.floor(double v)` = `(int)Math.floor(v)`.
pub fn floor_d(v: f64) -> i32 {
    v.floor() as i32
}
