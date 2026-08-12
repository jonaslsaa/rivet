//! Port of `net.minecraft.util.valueproviders.TrapezoidInt` (record, 26.2).

use crate::RandomSource;
use crate::mth::random_between_inclusive;
use rivet_serialization::codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.util.valueproviders.TrapezoidInt` — a trapezoid (or triangle)
/// distribution over `[min_inclusive, max_inclusive]` with a `plateau` span of
/// equally likely values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrapezoidInt {
    /// `this.minInclusive`.
    min_inclusive: i32,
    /// `this.maxInclusive`.
    max_inclusive: i32,
    /// `this.plateau` — the plateau span.
    plateau: i32,
}

impl TrapezoidInt {
    /// `TrapezoidInt.of(int min, int max, int plateau)`.
    pub const fn of(min_inclusive: i32, max_inclusive: i32, plateau: i32) -> TrapezoidInt {
        TrapezoidInt {
            min_inclusive,
            max_inclusive,
            plateau,
        }
    }

    /// `TrapezoidInt.triangle(int range)` — `of(-range, range, 0)`.
    pub fn triangle(range: i32) -> crate::valueproviders::int_provider::IntProvider {
        crate::valueproviders::int_provider::IntProvider::Trapezoid(TrapezoidInt::of(
            -range, range, 0,
        ))
    }

    /// `minInclusive()` (Java record accessor).
    pub fn min_inclusive(&self) -> i32 {
        self.min_inclusive
    }

    /// `maxInclusive()` (Java record accessor).
    pub fn max_inclusive(&self) -> i32 {
        self.max_inclusive
    }

    /// `plateau()` (Java record accessor).
    pub fn plateau(&self) -> i32 {
        self.plateau
    }

    /// `TrapezoidInt.sample(RandomSource)`.
    ///
    /// ```java
    /// if (this.plateau == 0 && this.maxInclusive == -this.minInclusive) {
    ///     return random.nextInt(this.maxInclusive + 1) - random.nextInt(this.maxInclusive + 1);
    /// }
    ///
    /// int range = this.maxInclusive - this.minInclusive;
    /// if (this.plateau == range) {
    ///     return Mth.randomBetweenInclusive(random, this.minInclusive, this.maxInclusive);
    /// }
    ///
    /// int plateauStart = (range - this.plateau) / 2;
    /// int plateauEnd = range - plateauStart;
    /// return this.minInclusive + Mth.randomBetweenInclusive(random, 0, plateauEnd)
    ///     + Mth.randomBetweenInclusive(random, 0, plateauStart);
    /// ```
    ///
    /// Java int arithmetic wraps (negation, `+1`, the span and the final
    /// additions).
    pub fn sample<R: RandomSource>(&self, random: &mut R) -> i32 {
        if self.plateau == 0 && self.max_inclusive == self.min_inclusive.wrapping_neg() {
            let bound = self.max_inclusive.wrapping_add(1);
            return random
                .next_int_bound(bound)
                .wrapping_sub(random.next_int_bound(bound));
        }

        let range = self.max_inclusive.wrapping_sub(self.min_inclusive);
        if self.plateau == range {
            return random_between_inclusive(random, self.min_inclusive, self.max_inclusive);
        }

        let plateau_start = (range.wrapping_sub(self.plateau)) / 2;
        let plateau_end = range.wrapping_sub(plateau_start);
        self.min_inclusive
            .wrapping_add(random_between_inclusive(random, 0, plateau_end))
            .wrapping_add(random_between_inclusive(random, 0, plateau_start))
    }
}

impl fmt::Display for TrapezoidInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `"trapezoid(" + plateau + ") in [" + minInclusive + "-"
        // + maxInclusive + "]"`.
        write!(
            f,
            "trapezoid({}) in [{}-{}]",
            self.plateau, self.min_inclusive, self.max_inclusive
        )
    }
}

/// `TrapezoidInt.CODEC` — a record codec over `"min"`/`"max"`/`"plateau"`,
/// validated, as the ops-generic `trapezoid_int_map_codec::<Ops>()` factory.
pub fn trapezoid_int_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<TrapezoidInt, Ops>>
{
    let inner = record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|t: &TrapezoidInt| t.min_inclusive),
                codec::field_of::<i32, Ops>(codec::int_codec::<Ops>(), "min".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &TrapezoidInt| t.max_inclusive),
                codec::field_of::<i32, Ops>(codec::int_codec::<Ops>(), "max".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &TrapezoidInt| t.plateau),
                codec::field_of::<i32, Ops>(codec::int_codec::<Ops>(), "plateau".to_string()),
            ))
            .apply(
                instance,
                Arc::new(|min: i32, max: i32, plateau: i32| TrapezoidInt::of(min, max, plateau)),
            )
    });
    map_codec::validate(
        inner,
        Arc::new(|t: &TrapezoidInt| {
            if t.max_inclusive < t.min_inclusive {
                DataResult::error(format!(
                    "Max must be larger than min: [{}, {}]",
                    t.min_inclusive, t.max_inclusive
                ))
            } else if t.plateau > t.max_inclusive.wrapping_sub(t.min_inclusive) {
                DataResult::error(format!(
                    "Plateau can at most be the full span: [{}, {}]",
                    t.min_inclusive, t.max_inclusive
                ))
            } else {
                DataResult::success(*t)
            }
        }),
    )
}
