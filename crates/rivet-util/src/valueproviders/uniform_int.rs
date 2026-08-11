//! Port of `net.minecraft.util.valueproviders.UniformInt` (record, 26.2).

use crate::RandomSource;
use crate::mth::random_between_inclusive;
use rivet_serialization::codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.util.valueproviders.UniformInt` — uniform over
/// `[min_inclusive, max_inclusive]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UniformInt {
    /// `this.minInclusive`.
    min_inclusive: i32,
    /// `this.maxInclusive`.
    max_inclusive: i32,
}

impl UniformInt {
    /// `UniformInt.of(int minInclusive, int maxInclusive)`.
    pub const fn of(min_inclusive: i32, max_inclusive: i32) -> UniformInt {
        UniformInt {
            min_inclusive,
            max_inclusive,
        }
    }

    /// `minInclusive()` (Java record accessor).
    pub fn min_inclusive(&self) -> i32 {
        self.min_inclusive
    }

    /// `maxInclusive()` (Java record accessor).
    pub fn max_inclusive(&self) -> i32 {
        self.max_inclusive
    }

    /// `UniformInt.sample(RandomSource)` — `Mth.randomBetweenInclusive(random,
    /// minInclusive, maxInclusive)`. Java int arithmetic wraps.
    pub fn sample<R: RandomSource>(&self, random: &mut R) -> i32 {
        random_between_inclusive(random, self.min_inclusive, self.max_inclusive)
    }
}

impl fmt::Display for UniformInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `"[" + minInclusive + "-" + maxInclusive + "]"`.
        write!(f, "[{}-{}]", self.min_inclusive, self.max_inclusive)
    }
}

/// `UniformInt.CODEC` — a record codec over the two inclusive bound fields,
/// validated, as the ops-generic `uniform_int_map_codec::<Ops>()` factory.
pub fn uniform_int_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<UniformInt, Ops>> {
    let inner = record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|u: &UniformInt| u.min_inclusive),
                codec::field_of::<i32, Ops>(codec::int_codec::<Ops>(), "min_inclusive".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|u: &UniformInt| u.max_inclusive),
                codec::field_of::<i32, Ops>(codec::int_codec::<Ops>(), "max_inclusive".to_string()),
            ))
            .apply(instance, Arc::new(UniformInt::of))
    });
    map_codec::validate(
        inner,
        Arc::new(|u: &UniformInt| {
            if u.max_inclusive < u.min_inclusive {
                DataResult::error(format!(
                    "Max must be at least min, min_inclusive: {}, max_inclusive: {}",
                    u.min_inclusive, u.max_inclusive
                ))
            } else {
                DataResult::success(*u)
            }
        }),
    )
}
