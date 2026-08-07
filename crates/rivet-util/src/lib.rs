//! `net.minecraft.util` port surface.
//!
//! `mth` is a full port of `net.minecraft.util.Mth` (arithmetic, trig tables,
//! lerp/clamp/floorDiv, packing helpers), golden-tested against Java. `random`
//! ports `net.minecraft.util.random` (Xoroshiro/Legacy RNG), also
//! parity-tested. `java_hash` ports the JDK string hashing used by
//! `ResourceLocation`.
//!
//! RivetTodo(#209): `data_io` / `delegate_data_output` /
//! `fast_buffered_input_stream` are the byte-IO contract `NbtIo` needs; only
//! the surface `NbtIo` uses is provided.
//!
//! `util` is a partial port of `net.minecraft.util.Util` for the registry-core
//! slice (issue #107 / #122); `string_representable` and `by_id_map` are full
//! ports of their `mc.util` classes. See each module's provenance header.

pub mod bit_storage;
pub mod by_id_map;
pub mod data_io;
pub mod delegate_data_output;
pub mod fast_buffered_input_stream;
pub mod java_float_format;
pub mod java_hash;
pub mod known_pack;
pub mod mth;
pub mod mth_atan_tables;
pub mod mth_sin_table;
pub mod mth_stubs;
pub mod random;
pub mod simple_bit_storage;
pub mod string_representable;
pub mod util;
pub mod zero_bit_storage;

pub use bit_storage::BitStorage;
pub use by_id_map::{OutOfBoundsStrategy, continuous, sparse};
pub use data_io::{DataInput, DataInputStream, DataOutput, DataOutputStream};
pub use delegate_data_output::DelegateDataOutput;
pub use fast_buffered_input_stream::FastBufferedInputStream;
pub use known_pack::KnownPack;
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

/// `Mth.floor(float v)` = `(int)Math.floor(v)`. Rust's `as` saturates and maps
/// NaN to 0 exactly like the Java float->int cast (PORTING.md).
pub fn floor(v: f32) -> i32 {
    v.floor() as i32
}

/// `Mth.floor(double v)` = `(int)Math.floor(v)`.
pub fn floor_d(v: f64) -> i32 {
    v.floor() as i32
}
