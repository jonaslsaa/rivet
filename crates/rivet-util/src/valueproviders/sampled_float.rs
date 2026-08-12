//! Port of `net.minecraft.util.valueproviders.SampledFloat` (interface, 26.2).
//!
//! Java's `SampledFloat` is the base interface `FloatProvider` extends, with
//! one extra implementor in this package: `MultipliedFloats`. The Rust port
//! collapses the closed set to a single enum (the same closed-sum shape the
//! dispatch roots take): a `SampledFloat` is either a [`FloatProvider`] or a
//! [`MultipliedFloats`]. `sample` is generic over the `RandomSource` (which is
//! `Sized`, so the interface cannot be object-safe), matching the dispatch-root
//! convention in `HeightProvider`.

use crate::RandomSource;
use crate::valueproviders::float_provider::FloatProvider;
use crate::valueproviders::multiplied_floats::MultipliedFloats;
use std::fmt;

/// `net.minecraft.util.valueproviders.SampledFloat` — the closed sum over the
/// two implementors in this package.
///
/// The derived `PartialEq` is value equality, whereas Java's `MultipliedFloats`
/// is a plain class with no `equals` override (reference identity) — the same
/// documented sealed-hierarchy divergence `IntProvider`'s enum doc covers. No
/// ported code path compares samplers for identity, so it is not observable.
#[derive(Debug, Clone, PartialEq)]
pub enum SampledFloat {
    /// A `FloatProvider` (a `FloatProvider` IS-A `SampledFloat`).
    Float(FloatProvider),
    /// `MultipliedFloats`.
    MultipliedFloats(MultipliedFloats),
}

impl SampledFloat {
    /// `SampledFloat.sample(RandomSource)` — dispatch to the concrete
    /// implementor, preserving Java float arithmetic.
    pub fn sample<R: RandomSource>(&self, random: &mut R) -> f32 {
        match self {
            SampledFloat::Float(f) => f.sample(random),
            SampledFloat::MultipliedFloats(m) => m.sample(random),
        }
    }
}

impl fmt::Display for SampledFloat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SampledFloat::Float(p) => write!(f, "{p}"),
            SampledFloat::MultipliedFloats(m) => write!(f, "{m}"),
        }
    }
}
