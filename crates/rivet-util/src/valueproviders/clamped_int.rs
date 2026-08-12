//! Port of `net.minecraft.util.valueproviders.ClampedInt` (record, 26.2).

use crate::RandomSource;
use crate::mth;
use crate::valueproviders::int_provider::IntProvider;
use rivet_serialization::codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.util.valueproviders.ClampedInt` — samples `source`, clamped to
/// `[min_inclusive, max_inclusive]`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClampedInt {
    /// `this.source` — the wrapped provider (boxed: `IntProvider` embeds a
    /// `ClampedInt`, so the Java heap reference becomes `Box` to break the
    /// recursive type).
    source: Box<IntProvider>,
    /// `this.minInclusive`.
    min_inclusive: i32,
    /// `this.maxInclusive`.
    max_inclusive: i32,
}

impl ClampedInt {
    /// `ClampedInt.of(IntProvider source, int minInclusive, int maxInclusive)`.
    pub fn of(source: IntProvider, min_inclusive: i32, max_inclusive: i32) -> ClampedInt {
        ClampedInt {
            source: Box::new(source),
            min_inclusive,
            max_inclusive,
        }
    }

    /// `source()` (Java record accessor).
    pub fn source(&self) -> &IntProvider {
        &self.source
    }

    /// Clone the boxed source (the record codec's `forGetter` needs an owned
    /// value; Java's record accessor returns the same reference).
    pub(crate) fn source_cloned(&self) -> IntProvider {
        (*self.source).clone()
    }

    /// `minInclusive()` (Java record accessor).
    pub fn min_inclusive(&self) -> i32 {
        self.min_inclusive
    }

    /// `maxInclusive()` (Java record accessor).
    pub fn max_inclusive(&self) -> i32 {
        self.max_inclusive
    }

    /// `ClampedInt.sample(RandomSource)` — `Mth.clamp(source.sample(random),
    /// minInclusive, maxInclusive)`.
    pub fn sample<R: RandomSource>(&self, random: &mut R) -> i32 {
        mth::clamp(
            self.source.sample(random),
            self.min_inclusive,
            self.max_inclusive,
        )
    }

    /// `ClampedInt.minInclusive()` override — `Math.max(minInclusive,
    /// source.minInclusive())`.
    pub fn effective_min_inclusive(&self) -> i32 {
        self.min_inclusive.max(self.source.min_inclusive())
    }

    /// `ClampedInt.maxInclusive()` override — `Math.min(maxInclusive,
    /// source.maxInclusive())`.
    pub fn effective_max_inclusive(&self) -> i32 {
        self.max_inclusive.min(self.source.max_inclusive())
    }
}

/// `ClampedInt.CODEC` — a record codec over `"source"` (recursive), the two
/// inclusive bound fields, validated, as the ops-generic
/// `clamped_int_map_codec::<Ops>(top)` factory. `top` is the
/// `IntProvider.CODEC` `RecursiveSelf` from the dispatch graph, so a `clamped`
/// source resolves through the single recursive codec.
pub fn clamped_int_map_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn rivet_serialization::Codec<IntProvider, Ops>>,
) -> Arc<dyn MapCodec<ClampedInt, Ops>> {
    let inner = record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &ClampedInt| c.source_cloned()),
                codec::field_of::<IntProvider, Ops>(top, "source".to_string()),
            ))
            // `forGetter(ClampedInt::minInclusive)` / `forGetter(ClampedInt::maxInclusive)`
            // are method references to the record's OVERRIDDEN accessors, so on
            // encode Java serializes the EFFECTIVE bounds
            // (`Math.max(min, source.minInclusive())` / `Math.min(max,
            // source.maxInclusive())`), not the raw clamp bounds.
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &ClampedInt| c.effective_min_inclusive()),
                codec::field_of::<i32, Ops>(codec::int_codec::<Ops>(), "min_inclusive".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &ClampedInt| c.effective_max_inclusive()),
                codec::field_of::<i32, Ops>(codec::int_codec::<Ops>(), "max_inclusive".to_string()),
            ))
            .apply(
                instance,
                Arc::new(|source: IntProvider, min: i32, max: i32| {
                    ClampedInt::of(source, min, max)
                }),
            )
    });
    map_codec::validate(
        inner,
        Arc::new(|c: &ClampedInt| {
            // Java's `.validate` lambda resolves `u.maxInclusive`/`u.minInclusive`
            // to the record's OVERRIDDEN accessors (the effective bounds), not the
            // raw clamp fields — so a source range narrower than the clamp range
            // errors even when the raw bounds look valid.
            if c.effective_max_inclusive() < c.effective_min_inclusive() {
                DataResult::error(format!(
                    "Max must be at least min, min_inclusive: {}, max_inclusive: {}",
                    c.effective_min_inclusive(),
                    c.effective_max_inclusive()
                ))
            } else {
                DataResult::success(c.clone())
            }
        }),
    )
}
