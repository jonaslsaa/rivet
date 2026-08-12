//! Port of `net.minecraft.util.valueproviders.ConstantFloat` (record, 26.2).

use crate::RandomSource;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.util.valueproviders.ConstantFloat` — a provider always
/// returning `value`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConstantFloat {
    /// `this.value` — the constant.
    value: f32,
}

impl ConstantFloat {
    /// `ConstantFloat.of(float value)` — returns the `ZERO` singleton for 0.0F,
    /// exactly like Java's `of`.
    pub fn of(value: f32) -> ConstantFloat {
        if value == 0.0 {
            ConstantFloat::ZERO
        } else {
            ConstantFloat { value }
        }
    }

    /// `this.value` (Java record accessor).
    pub fn value(&self) -> f32 {
        self.value
    }

    /// `ConstantFloat.ZERO`.
    pub const ZERO: ConstantFloat = ConstantFloat { value: 0.0 };

    /// `ConstantFloat.sample(RandomSource)`.
    pub fn sample<R: RandomSource>(&self, _random: &mut R) -> f32 {
        self.value
    }

    /// `min()`.
    pub fn min(&self) -> f32 {
        self.value
    }

    /// `max()`.
    pub fn max(&self) -> f32 {
        self.value
    }
}

impl fmt::Display for ConstantFloat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `Float.toString(value)`.
        write!(
            f,
            "{}",
            crate::java_float_format::java_float_to_string(self.value)
        )
    }
}

/// `ConstantFloat.CODEC` — a record codec over the `"value"` field, as the
/// ops-generic `constant_float_map_codec::<Ops>()` factory.
pub fn constant_float_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ConstantFloat, Ops>>
{
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &ConstantFloat| c.value),
                codec::field_of::<f32, Ops>(codec::float_codec::<Ops>(), "value".to_string()),
            ))
            .apply(instance, Arc::new(ConstantFloat::of))
    })
}
