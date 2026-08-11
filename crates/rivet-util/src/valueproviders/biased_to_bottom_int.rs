//! Port of `net.minecraft.util.valueproviders.BiasedToBottomInt` (record, 26.2).

use crate::RandomSource;
use rivet_serialization::codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.util.valueproviders.BiasedToBottomInt` — biased toward the
/// bottom of `[min_inclusive, max_inclusive]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BiasedToBottomInt {
    /// `this.minInclusive`.
    min_inclusive: i32,
    /// `this.maxInclusive`.
    max_inclusive: i32,
}

impl BiasedToBottomInt {
    /// `BiasedToBottomInt.of(int minInclusive, int maxInclusive)`.
    pub const fn of(min_inclusive: i32, max_inclusive: i32) -> BiasedToBottomInt {
        BiasedToBottomInt {
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

    /// `BiasedToBottomInt.sample(RandomSource)`.
    ///
    /// ```java
    /// return this.minInclusive + random.nextInt(random.nextInt(this.maxInclusive - this.minInclusive + 1) + 1);
    /// ```
    ///
    /// The two `nextInt` calls and the Java int arithmetic (the span
    /// `max - min + 1` and the final `min + …`) wrap.
    pub fn sample<R: RandomSource>(&self, random: &mut R) -> i32 {
        let span = self
            .max_inclusive
            .wrapping_sub(self.min_inclusive)
            .wrapping_add(1);
        let inner = random.next_int_bound(span);
        let outer = random.next_int_bound(inner.wrapping_add(1));
        self.min_inclusive.wrapping_add(outer)
    }
}

impl fmt::Display for BiasedToBottomInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `"[" + minInclusive + "-" + maxInclusive + "]"`.
        write!(f, "[{}-{}]", self.min_inclusive, self.max_inclusive)
    }
}

/// `BiasedToBottomInt.CODEC` — a record codec over the two inclusive bound
/// fields, validated, as the ops-generic `biased_to_bottom_int_map_codec::<Ops>()`
/// factory.
pub fn biased_to_bottom_int_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<BiasedToBottomInt, Ops>> {
    let inner = record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|b: &BiasedToBottomInt| b.min_inclusive),
                codec::field_of::<i32, Ops>(codec::int_codec::<Ops>(), "min_inclusive".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|b: &BiasedToBottomInt| b.max_inclusive),
                codec::field_of::<i32, Ops>(codec::int_codec::<Ops>(), "max_inclusive".to_string()),
            ))
            .apply(instance, Arc::new(BiasedToBottomInt::of))
    });
    map_codec::validate(
        inner,
        Arc::new(|b: &BiasedToBottomInt| {
            if b.max_inclusive < b.min_inclusive {
                DataResult::error(format!(
                    "Max must be at least min, min_inclusive: {}, max_inclusive: {}",
                    b.min_inclusive, b.max_inclusive
                ))
            } else {
                DataResult::success(*b)
            }
        }),
    )
}
