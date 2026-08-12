//! Named aliases for the `Arc<dyn Fn + Send + Sync>` function types threaded through the
//! codec combinators. Java's `Function`/`BiFunction`/`Function3`/`Function4`
//! and `DataResult`-returning variants map to `Arc<dyn Fn + Send + Sync>` here (see `lib.rs`:
//! codecs compose via `Arc<dyn Codec<..>>`, so multi-argument functions are
//! `Arc<dyn Fn + Send + Sync>` to allow currying); the aliases keep the repeated signatures
//! readable.
//!
//! Every alias is `Send + Sync`: codecs are `static final` values in Java,
//! shared across netty threads, and the packet `StreamCodec`s built from them
//! must themselves be `Send + Sync` (see `crates/rivet-protocol`'s
//! `StreamCodecDyn`).

use crate::data_result::DataResult;
use std::sync::Arc;

/// `Function<? super A, ? extends B>` — a plain by-reference mapping.
pub type Fn1<A, B> = Arc<dyn Fn(&A) -> B + Send + Sync>;

/// `BiFunction<? super A, ? super B, ? extends C>`.
pub type Fn2<A, B, C> = Arc<dyn Fn(&A, &B) -> C + Send + Sync>;

/// 3-argument by-reference function (DFU `Function3`).
pub type Fn3<A, B, C, R> = Arc<dyn Fn(&A, &B, &C) -> R + Send + Sync>;

/// 4-argument by-reference function (DFU `Function4`).
pub type Fn4<A, B, C, D, R> = Arc<dyn Fn(&A, &B, &C, &D) -> R + Send + Sync>;

/// 5-argument by-reference function (DFU `Function5`).
pub type Fn5<A, B, C, D, E, R> = Arc<dyn Fn(&A, &B, &C, &D, &E) -> R + Send + Sync>;

/// 6-argument by-reference function (DFU `Function6`).
pub type Fn6<A, B, C, D, E, F, R> = Arc<dyn Fn(&A, &B, &C, &D, &E, &F) -> R + Send + Sync>;

/// `Function<? super A, ? extends DataResult<? extends B>>` — a decoding step
/// (`flatMap`/`flatXmap`/`validate`).
pub type DecoderFn<A, B> = Arc<dyn Fn(&A) -> DataResult<B> + Send + Sync>;
