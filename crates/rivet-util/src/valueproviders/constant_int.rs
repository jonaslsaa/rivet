//! Port of `net.minecraft.util.valueproviders.ConstantInt` (record, 26.2).

use crate::RandomSource;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.util.valueproviders.ConstantInt` — a provider always
/// returning `value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConstantInt {
    /// `this.value` — the constant.
    value: i32,
}

impl ConstantInt {
    /// `ConstantInt.of(int value)` — returns the `ZERO` singleton for 0,
    /// exactly like Java's `of`.
    pub fn of(value: i32) -> ConstantInt {
        if value == 0 {
            ConstantInt::ZERO
        } else {
            ConstantInt { value }
        }
    }

    /// `this.value` (Java record accessor).
    pub fn value(&self) -> i32 {
        self.value
    }

    /// `ConstantInt.ZERO`.
    pub const ZERO: ConstantInt = ConstantInt { value: 0 };

    /// `ConstantInt.sample(RandomSource)`.
    pub fn sample<R: RandomSource>(&self, _random: &mut R) -> i32 {
        self.value
    }

    /// `minInclusive()`.
    pub fn min_inclusive(&self) -> i32 {
        self.value
    }

    /// `maxInclusive()`.
    pub fn max_inclusive(&self) -> i32 {
        self.value
    }
}

impl fmt::Display for ConstantInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `Integer.toString(value)`.
        write!(f, "{}", self.value)
    }
}

/// `ConstantInt.CODEC` — a record codec over the `"value"` field, as the
/// ops-generic `constant_int_map_codec::<Ops>()` factory.
pub fn constant_int_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ConstantInt, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &ConstantInt| c.value),
                codec::field_of::<i32, Ops>(codec::int_codec::<Ops>(), "value".to_string()),
            ))
            .apply(instance, Arc::new(ConstantInt::of))
    })
}
