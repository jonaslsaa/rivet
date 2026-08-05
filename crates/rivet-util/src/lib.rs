//! `net.minecraft.util` port surface.
//!
//! `mth` is a full port of `net.minecraft.util.Mth` (arithmetic, trig tables,
//! lerp/clamp/floorDiv, packing helpers), golden-tested against Java. `random`
//! ports `net.minecraft.util.random` (Xoroshiro/Legacy RNG), also
//! parity-tested. `java_hash` ports the JDK string hashing used by
//! `ResourceLocation`.
//!
//! STUB(mc.nbt.io): `data_io` / `delegate_data_output` /
//! `fast_buffered_input_stream` / `util` are the byte-IO contract `NbtIo`
//! needs; only the surface `NbtIo` uses is provided.

pub mod data_io;
pub mod delegate_data_output;
pub mod fast_buffered_input_stream;
pub mod java_hash;
pub mod mth;
pub mod mth_atan_tables;
pub mod mth_sin_table;
pub mod mth_stubs;
pub mod random;
pub mod util;

pub use data_io::{DataInput, DataInputStream, DataOutput, DataOutputStream};
pub use delegate_data_output::DelegateDataOutput;
pub use fast_buffered_input_stream::FastBufferedInputStream;
// `BitRandomSource` is deliberately NOT re-exported at the root: it declares
// `next_int`/`next_long`/... (same names as `RandomSource`), so importing both
// makes every LCG call ambiguous (E0034). It lives at `rivet_util::random`.
pub use random::{PositionalRandomFactory, RandomSource};
// Module alias for the generated `mth_golden_tests.rs`, which references the
// RNG unit as `crate::random_source` (Java class `RandomSource`).
pub use random as random_source;
pub use util::log_and_pause_if_in_ide;

/// `Mth.floor(float v)` = `(int)Math.floor(v)`. Rust's `as` saturates and maps
/// NaN to 0 exactly like the Java float->int cast (PORTING.md).
pub fn floor(v: f32) -> i32 {
    v.floor() as i32
}

/// `Mth.floor(double v)` = `(int)Math.floor(v)`.
pub fn floor_d(v: f64) -> i32 {
    v.floor() as i32
}
