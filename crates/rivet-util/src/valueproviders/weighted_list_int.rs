//! Port of `net.minecraft.util.valueproviders.WeightedListInt` (class, 26.2).

use crate::RandomSource;
use crate::WeightedList;
use crate::valueproviders::int_provider::IntProvider;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.util.valueproviders.WeightedListInt` — a provider that holds a
/// `WeightedList<IntProvider>` and samples by selecting a weighted element
/// (`getRandomOrThrow`) and delegating to its `sample`. The provider's reported
/// `min`/`max` are the min/max across the distribution entries, computed at
/// construction.
///
/// Java is a plain class with no `equals`/`hashCode` override, so `==` is
/// reference identity; the derived `PartialEq` here is value equality. This is
/// the documented sealed-hierarchy divergence (see `IntProvider`'s enum doc): no
/// ported code path compares providers for identity, so it is not observable.
#[derive(Debug, Clone, PartialEq)]
pub struct WeightedListInt {
    /// `this.distribution` — the weighted provider list.
    distribution: WeightedList<IntProvider>,
    /// `this.minValue` — min of the entries' `minInclusive`.
    min_value: i32,
    /// `this.maxValue` — max of the entries' `maxInclusive`.
    max_value: i32,
}

impl WeightedListInt {
    /// `new WeightedListInt(WeightedList<IntProvider>)` — the public
    /// constructor, which folds the entries' min/max.
    ///
    /// ```java
    /// this.distribution = distribution;
    /// int min = Integer.MAX_VALUE;
    /// int max = Integer.MIN_VALUE;
    /// for (Weighted<IntProvider> value : distribution.unwrap()) {
    ///     int entryMin = value.value().minInclusive();
    ///     int entryMax = value.value().maxInclusive();
    ///     min = Math.min(min, entryMin);
    ///     max = Math.max(max, entryMax);
    /// }
    /// ```
    pub fn new(distribution: WeightedList<IntProvider>) -> WeightedListInt {
        let mut min_value = i32::MAX;
        let mut max_value = i32::MIN;
        for weighted in distribution.unwrap() {
            let entry_min = weighted.value().min_inclusive();
            let entry_max = weighted.value().max_inclusive();
            min_value = min_value.min(entry_min);
            max_value = max_value.max(entry_max);
        }
        WeightedListInt {
            distribution,
            min_value,
            max_value,
        }
    }

    /// `this.distribution` (Java private field, exposed for the codec getter).
    pub fn distribution(&self) -> &WeightedList<IntProvider> {
        &self.distribution
    }

    /// `WeightedListInt.sample(RandomSource)` —
    /// `this.distribution.getRandomOrThrow(random).sample(random)`.
    pub fn sample<R: RandomSource>(&self, random: &mut R) -> i32 {
        self.distribution.get_random_or_throw(random).sample(random)
    }

    /// `minInclusive()` — the folded `this.minValue`.
    pub fn min_inclusive(&self) -> i32 {
        self.min_value
    }

    /// `maxInclusive()` — the folded `this.maxValue`.
    pub fn max_inclusive(&self) -> i32 {
        self.max_value
    }
}

/// `WeightedListInt.CODEC` — a record codec over the `"distribution"` field
/// (`WeightedList.nonEmptyCodec(IntProvider.CODEC)`), as the ops-generic
/// `weighted_list_int_map_codec::<Ops>(top)` factory. `top` is the
/// `IntProvider.CODEC` `RecursiveSelf` from the dispatch graph, so nested
/// weighted lists round-trip through the single recursive codec.
pub fn weighted_list_int_map_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<IntProvider, Ops>>,
) -> Arc<dyn MapCodec<WeightedListInt, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|w: &WeightedListInt| w.distribution.clone()),
                // `WeightedList.nonEmptyCodec(IntProvider.CODEC).fieldOf(
                // "distribution")`.
                codec::field_of::<WeightedList<IntProvider>, Ops>(
                    crate::weighted::weighted_list_non_empty_codec::<IntProvider, Ops>(top),
                    "distribution".to_string(),
                ),
            ))
            .apply(instance, Arc::new(WeightedListInt::new))
    })
}
