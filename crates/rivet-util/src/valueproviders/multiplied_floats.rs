//! Port of `net.minecraft.util.valueproviders.MultipliedFloats` (class, 26.2).

use crate::RandomSource;
use crate::valueproviders::sampled_float::SampledFloat;
use std::fmt;

/// `net.minecraft.util.valueproviders.MultipliedFloats` — a `SampledFloat`
/// that multiplies the samples of its components, in order.
///
/// Java is a plain class with no `equals`/`hashCode` override, so `==` is
/// reference identity; the derived `PartialEq` here is value equality (the
/// documented sealed-hierarchy divergence — see `SampledFloat`'s enum doc).
#[derive(Debug, Clone, PartialEq)]
pub struct MultipliedFloats {
    /// `this.values` — the component samplers.
    values: Vec<SampledFloat>,
}

impl MultipliedFloats {
    /// `new MultipliedFloats(SampledFloat... values)`.
    pub fn new(values: Vec<SampledFloat>) -> MultipliedFloats {
        MultipliedFloats { values }
    }

    /// `this.values`.
    pub fn values(&self) -> &[SampledFloat] {
        &self.values
    }

    /// `MultipliedFloats.sample(RandomSource)`.
    ///
    /// ```java
    /// float result = 1.0F;
    /// for (SampledFloat value : this.values) {
    ///     result *= value.sample(random);
    /// }
    /// return result;
    /// ```
    ///
    /// The multiplication is in Java's iteration order (component draw order is
    /// observable), all f32.
    pub fn sample<R: RandomSource>(&self, random: &mut R) -> f32 {
        let mut result = 1.0;
        for value in &self.values {
            result *= value.sample(random);
        }
        result
    }
}

impl fmt::Display for MultipliedFloats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `"MultipliedFloats" + Arrays.toString(values)`
        // (the JDK `Arrays.toString` element format, comma-space separated in
        // brackets).
        write!(
            f,
            "MultipliedFloats[{}]",
            self.values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}
