//! Port of `net.minecraft.util.valueproviders.TrapezoidFloat` (record, 26.2).

use crate::RandomSource;
use rivet_serialization::codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.util.valueproviders.TrapezoidFloat` — a trapezoid
/// distribution over `[min, max]` with a `plateau` span of equally likely
/// values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrapezoidFloat {
    /// `this.min`.
    min: f32,
    /// `this.max`.
    max: f32,
    /// `this.plateau` — the plateau span.
    plateau: f32,
}

impl TrapezoidFloat {
    /// `TrapezoidFloat.of(float min, float max, float plateau)`.
    pub const fn of(min: f32, max: f32, plateau: f32) -> TrapezoidFloat {
        TrapezoidFloat { min, max, plateau }
    }

    /// `min()` (Java record accessor).
    pub fn min(&self) -> f32 {
        self.min
    }

    /// `max()` (Java record accessor).
    pub fn max(&self) -> f32 {
        self.max
    }

    /// `plateau()` (Java record accessor).
    pub fn plateau(&self) -> f32 {
        self.plateau
    }

    /// `TrapezoidFloat.sample(RandomSource)`.
    ///
    /// ```java
    /// float range = this.max - this.min;
    /// float plateauStart = (range - this.plateau) / 2.0F;
    /// float plateauEnd = range - plateauStart;
    /// return this.min + random.nextFloat() * plateauEnd + random.nextFloat() * plateauStart;
    /// ```
    ///
    /// Two `nextFloat` draws, in Java order; all float arithmetic is f32.
    pub fn sample<R: RandomSource>(&self, random: &mut R) -> f32 {
        let range = self.max - self.min;
        let plateau_start = (range - self.plateau) / 2.0;
        let plateau_end = range - plateau_start;
        self.min + random.next_float() * plateau_end + random.next_float() * plateau_start
    }
}

impl fmt::Display for TrapezoidFloat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `"trapezoid(" + plateau + ") in [" + min + "-" + max +
        // "]"` with Java float stringification.
        write!(
            f,
            "trapezoid({}) in [{}-{}]",
            crate::java_float_format::java_float_to_string(self.plateau),
            crate::java_float_format::java_float_to_string(self.min),
            crate::java_float_format::java_float_to_string(self.max)
        )
    }
}

/// `TrapezoidFloat.CODEC` — a record codec over `"min"`/`"max"`/`"plateau"`,
/// validated, as the ops-generic `trapezoid_float_map_codec::<Ops>()` factory.
pub fn trapezoid_float_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<TrapezoidFloat, Ops>> {
    let inner = record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|t: &TrapezoidFloat| t.min),
                codec::field_of::<f32, Ops>(codec::float_codec::<Ops>(), "min".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &TrapezoidFloat| t.max),
                codec::field_of::<f32, Ops>(codec::float_codec::<Ops>(), "max".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &TrapezoidFloat| t.plateau),
                codec::field_of::<f32, Ops>(codec::float_codec::<Ops>(), "plateau".to_string()),
            ))
            .apply(
                instance,
                Arc::new(|min: f32, max: f32, plateau: f32| TrapezoidFloat::of(min, max, plateau)),
            )
    });
    map_codec::validate(
        inner,
        Arc::new(|t: &TrapezoidFloat| {
            if t.max < t.min {
                DataResult::error(format!(
                    "Max must be larger than min: [{}, {}]",
                    crate::java_float_format::java_float_to_string(t.min),
                    crate::java_float_format::java_float_to_string(t.max)
                ))
            } else if t.plateau > t.max - t.min {
                DataResult::error(format!(
                    "Plateau can at most be the full span: [{}, {}]",
                    crate::java_float_format::java_float_to_string(t.min),
                    crate::java_float_format::java_float_to_string(t.max)
                ))
            } else {
                DataResult::success(*t)
            }
        }),
    )
}
