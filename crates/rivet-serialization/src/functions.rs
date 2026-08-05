//! Named aliases for the `Arc<dyn Fn>` function types threaded through the
//! codec combinators. Java's `Function`/`BiFunction`/`Function3`/`Function4`
//! and `DataResult`-returning variants map to `Arc<dyn Fn>` here (see `lib.rs`:
//! codecs compose via `Arc<dyn Codec<..>>`, so multi-argument functions are
//! `Arc<dyn Fn>` to allow currying); the aliases keep the repeated signatures
//! readable.

use crate::data_result::DataResult;
use std::sync::Arc;

/// `Function<? super A, ? extends B>` — a plain by-reference mapping.
pub type Fn1<A, B> = Arc<dyn Fn(&A) -> B>;

/// `BiFunction<? super A, ? super B, ? extends C>`.
pub type Fn2<A, B, C> = Arc<dyn Fn(&A, &B) -> C>;

/// 3-argument by-reference function (DFU `Function3`).
pub type Fn3<A, B, C, R> = Arc<dyn Fn(&A, &B, &C) -> R>;

/// 4-argument by-reference function (DFU `Function4`).
pub type Fn4<A, B, C, D, R> = Arc<dyn Fn(&A, &B, &C, &D) -> R>;

/// `Function<? super A, ? extends DataResult<? extends B>>` — a decoding step
/// (`flatMap`/`flatXmap`/`validate`).
pub type DecoderFn<A, B> = Arc<dyn Fn(&A) -> DataResult<B>>;
