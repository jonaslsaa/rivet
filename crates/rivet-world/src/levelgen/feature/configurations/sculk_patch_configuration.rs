//! Port of `net.minecraft.world.level.levelgen.feature.configurations.SculkPatchConfiguration`
//! (record, 26.2): a seven-field record whose `CODEC` is a `RecordCodecBuilder`
//! over the required `"charge_count"` (`Codec.intRange(1, 32)`),
//! `"amount_per_charge"` (`Codec.intRange(1, 500)`), `"spread_attempts"`
//! (`Codec.intRange(1, 64)`), `"growth_rounds"`/`"spread_rounds"`
//! (`Codec.intRange(0, 8)` each), `"extra_rare_growths"` (`IntProviders.CODEC`),
//! and `"catalyst_chance"` (`Codec.floatRange(0.0F, 1.0F)`) fields. DFU
//! `Codec<T>` is `Codec<E, Ops>` here, so the static Java constant is the
//! ops-generic `sculk_patch_configuration_codec::<Ops>()` factory.
//!
//! The seven-field group exceeds the port's `record_builder` `Group6` cap, so
//! the record codec is hand-composed with `map_encoder`/`map_decoder` exactly
//! mirroring `Applicative.super.ap7` (`ap4(ap3(...curry3..., t1, t2, t3), t4,
//! t5, t6, t7)`). Equality mirrors `Float.compare`: every NaN payload
//! canonicalizes to one value and `-0.0` is distinct from `0.0` (see
//! [`PartialEq`]).

use crate::levelgen::feature::configurations::FeatureConfiguration;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::{self, DataResult};
use rivet_serialization::dynamic_ops::{DynamicOps, MapLike, RecordBuilder};
use rivet_serialization::functions::{Fn3, Fn4};
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::map_codec;
use rivet_serialization::map_decoder;
use rivet_serialization::map_encoder;
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.SculkPatchConfiguration`.
#[derive(Debug, Clone)]
pub struct SculkPatchConfiguration {
    /// `chargeCount` — `[1, 32]`.
    pub charge_count: i32,
    /// `amountPerCharge` — `[1, 500]`.
    pub amount_per_charge: i32,
    /// `spreadAttempts` — `[1, 64]`.
    pub spread_attempts: i32,
    /// `growthRounds` — `[0, 8]`.
    pub growth_rounds: i32,
    /// `spreadRounds` — `[0, 8]`.
    pub spread_rounds: i32,
    /// `extraRareGrowths` — an `IntProvider`.
    pub extra_rare_growths: IntProvider,
    /// `catalystChance` — `[0.0F, 1.0F]`.
    pub catalyst_chance: f32,
}

impl PartialEq for SculkPatchConfiguration {
    fn eq(&self, other: &Self) -> bool {
        // `Float.compare`: NaN payloads canonicalize, signed zero keeps its sign.
        fn canonical_bits(value: f32) -> u32 {
            if value.is_nan() {
                f32::NAN.to_bits()
            } else {
                value.to_bits()
            }
        }
        self.charge_count == other.charge_count
            && self.amount_per_charge == other.amount_per_charge
            && self.spread_attempts == other.spread_attempts
            && self.growth_rounds == other.growth_rounds
            && self.spread_rounds == other.spread_rounds
            && self.extra_rare_growths == other.extra_rare_growths
            && canonical_bits(self.catalyst_chance) == canonical_bits(other.catalyst_chance)
    }
}

impl Eq for SculkPatchConfiguration {}

impl SculkPatchConfiguration {
    /// The record constructor (the codec's `apply` function).
    pub fn new(
        charge_count: i32,
        amount_per_charge: i32,
        spread_attempts: i32,
        growth_rounds: i32,
        spread_rounds: i32,
        extra_rare_growths: IntProvider,
        catalyst_chance: f32,
    ) -> Self {
        SculkPatchConfiguration {
            charge_count,
            amount_per_charge,
            spread_attempts,
            growth_rounds,
            spread_rounds,
            extra_rare_growths,
            catalyst_chance,
        }
    }

    /// `chargeCount()`.
    pub fn charge_count(&self) -> i32 {
        self.charge_count
    }

    /// `amountPerCharge()`.
    pub fn amount_per_charge(&self) -> i32 {
        self.amount_per_charge
    }

    /// `spreadAttempts()`.
    pub fn spread_attempts(&self) -> i32 {
        self.spread_attempts
    }

    /// `growthRounds()`.
    pub fn growth_rounds(&self) -> i32 {
        self.growth_rounds
    }

    /// `spreadRounds()`.
    pub fn spread_rounds(&self) -> i32 {
        self.spread_rounds
    }

    /// `extraRareGrowths()`.
    pub fn extra_rare_growths(&self) -> &IntProvider {
        &self.extra_rare_growths
    }

    /// `catalystChance()`.
    pub fn catalyst_chance(&self) -> f32 {
        self.catalyst_chance
    }
}

/// `SculkPatchConfiguration.CODEC` — the ops-generic
/// `sculk_patch_configuration_codec::<Ops>()` factory (record codec over the
/// seven required fields). The seven-field group exceeds the port's
/// `record_builder` `Group6` cap, so the decode side is hand-composed with the
/// `Applicative.super.ap7` decomposition `ap4(ap3(map(Function7.curry3, func),
/// t1, t2, t3), t4, t5, t6, t7)`: the leading three field results assemble a
/// `Fn3` returning the trailing `Fn4`, which the outer `ap4` applies to the
/// last four field results.
pub fn sculk_patch_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<SculkPatchConfiguration, Ops>> {
    let charge_count_codec = codec::int_range::<Ops>(1, 32);
    let amount_per_charge_codec = codec::int_range::<Ops>(1, 500);
    let spread_attempts_codec = codec::int_range::<Ops>(1, 64);
    let growth_rounds_codec = codec::int_range::<Ops>(0, 8);
    let spread_rounds_codec = codec::int_range::<Ops>(0, 8);
    let extra_rare_growths_codec = int_provider_codec::<Ops>();
    let catalyst_chance_codec = codec::float_range::<Ops>(0.0, 1.0);

    let charge_count_encoder = map_encoder::field_encoder(
        "charge_count".to_string(),
        codec::encoder_of_codec(charge_count_codec.clone()),
    );
    let charge_count_decoder = map_decoder::field_decoder(
        "charge_count".to_string(),
        codec::decoder_of_codec(charge_count_codec),
    );
    let amount_per_charge_encoder = map_encoder::field_encoder(
        "amount_per_charge".to_string(),
        codec::encoder_of_codec(amount_per_charge_codec.clone()),
    );
    let amount_per_charge_decoder = map_decoder::field_decoder(
        "amount_per_charge".to_string(),
        codec::decoder_of_codec(amount_per_charge_codec),
    );
    let spread_attempts_encoder = map_encoder::field_encoder(
        "spread_attempts".to_string(),
        codec::encoder_of_codec(spread_attempts_codec.clone()),
    );
    let spread_attempts_decoder = map_decoder::field_decoder(
        "spread_attempts".to_string(),
        codec::decoder_of_codec(spread_attempts_codec),
    );
    let growth_rounds_encoder = map_encoder::field_encoder(
        "growth_rounds".to_string(),
        codec::encoder_of_codec(growth_rounds_codec.clone()),
    );
    let growth_rounds_decoder = map_decoder::field_decoder(
        "growth_rounds".to_string(),
        codec::decoder_of_codec(growth_rounds_codec),
    );
    let spread_rounds_encoder = map_encoder::field_encoder(
        "spread_rounds".to_string(),
        codec::encoder_of_codec(spread_rounds_codec.clone()),
    );
    let spread_rounds_decoder = map_decoder::field_decoder(
        "spread_rounds".to_string(),
        codec::decoder_of_codec(spread_rounds_codec),
    );
    let extra_rare_growths_encoder = map_encoder::field_encoder(
        "extra_rare_growths".to_string(),
        codec::encoder_of_codec(extra_rare_growths_codec.clone()),
    );
    let extra_rare_growths_decoder = map_decoder::field_decoder(
        "extra_rare_growths".to_string(),
        codec::decoder_of_codec(extra_rare_growths_codec),
    );
    let catalyst_chance_encoder = map_encoder::field_encoder(
        "catalyst_chance".to_string(),
        codec::encoder_of_codec(catalyst_chance_codec.clone()),
    );
    let catalyst_chance_decoder = map_decoder::field_decoder(
        "catalyst_chance".to_string(),
        codec::decoder_of_codec(catalyst_chance_codec),
    );

    // Like `record_builder::build`'s `BuiltEncoder`, the encoder supplies no
    // keys and writes the fields in group declaration order.
    let encode = map_encoder::of(
        Arc::new(
            move |c: &SculkPatchConfiguration,
                  ops: &Ops,
                  prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                charge_count_encoder.encode(&c.charge_count, ops, prefix);
                amount_per_charge_encoder.encode(&c.amount_per_charge, ops, prefix);
                spread_attempts_encoder.encode(&c.spread_attempts, ops, prefix);
                growth_rounds_encoder.encode(&c.growth_rounds, ops, prefix);
                spread_rounds_encoder.encode(&c.spread_rounds, ops, prefix);
                extra_rare_growths_encoder.encode(&c.extra_rare_growths, ops, prefix);
                catalyst_chance_encoder.encode(&c.catalyst_chance, ops, prefix);
            },
        ),
        Arc::new(|_ops: &Ops| -> Vec<Ops::Output> { Vec::new() }),
    );

    // The decoder mirrors `Applicative.super.ap7`: the leading triple forms a
    // `Fn3` returning the trailing `Fn4`, which `ap4` applies.
    #[allow(clippy::type_complexity)]
    let decode = map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let fr: DataResult<
                Fn3<i32, i32, i32, Fn4<i32, i32, IntProvider, f32, SculkPatchConfiguration>>,
            > = DataResult::success_with_lifecycle(
                Arc::new(move |c1: &i32, c2: &i32, c3: &i32| {
                    let c1 = *c1;
                    let c2 = *c2;
                    let c3 = *c3;
                    let inner: Fn4<i32, i32, IntProvider, f32, SculkPatchConfiguration> =
                        Arc::new(move |g1: &i32, g2: &i32, g3: &IntProvider, g4: &f32| {
                            SculkPatchConfiguration::new(c1, c2, c3, *g1, *g2, g3.clone(), *g4)
                        });
                    inner
                }),
                Lifecycle::experimental(),
            );
            let step1 = data_result::ap3(
                fr,
                charge_count_decoder.decode(ops, input),
                amount_per_charge_decoder.decode(ops, input),
                spread_attempts_decoder.decode(ops, input),
            );
            data_result::ap4(
                step1,
                growth_rounds_decoder.decode(ops, input),
                spread_rounds_decoder.decode(ops, input),
                extra_rare_growths_decoder.decode(ops, input),
                catalyst_chance_decoder.decode(ops, input),
            )
        }),
        Arc::new(move |ops: &Ops| -> Vec<Ops::Output> {
            vec![
                ops.create_string("charge_count".to_string()),
                ops.create_string("amount_per_charge".to_string()),
                ops.create_string("spread_attempts".to_string()),
                ops.create_string("growth_rounds".to_string()),
                ops.create_string("spread_rounds".to_string()),
                ops.create_string("extra_rare_growths".to_string()),
                ops.create_string("catalyst_chance".to_string()),
            ]
        }),
    );

    map_codec::codec_of(map_codec::of(
        encode,
        decode.clone(),
        format!("RecordCodec[{:?}]", decode),
    ))
}

impl FeatureConfiguration for SculkPatchConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;

    fn sample_config() -> SculkPatchConfiguration {
        SculkPatchConfiguration::new(
            5,
            10,
            3,
            2,
            1,
            IntProvider::Constant(ConstantInt::of(0)),
            0.5,
        )
    }

    #[test]
    fn codec_round_trip() {
        let codec = sculk_patch_configuration_codec::<JsonOps>();
        let config = sample_config();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "charge_count": 5,
                "amount_per_charge": 10,
                "spread_attempts": 3,
                "growth_rounds": 2,
                "spread_rounds": 1,
                "extra_rare_growths": 0,
                "catalyst_chance": 0.5,
            })
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_round_trip_with_uniform_extra_rare_growths() {
        // The `IntProviders.CODEC` dispatch encodes a `UniformInt` provider as
        // the typed object (constant providers are the bare-int form).
        let codec = sculk_patch_configuration_codec::<JsonOps>();
        let config = SculkPatchConfiguration::new(
            32,
            500,
            64,
            8,
            8,
            IntProvider::Uniform(UniformInt::of(1, 4)),
            1.0,
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "charge_count": 32,
                "amount_per_charge": 500,
                "spread_attempts": 64,
                "growth_rounds": 8,
                "spread_rounds": 8,
                "extra_rare_growths": {
                    "type": "minecraft:uniform",
                    "min_inclusive": 1,
                    "max_inclusive": 4,
                },
                "catalyst_chance": 1.0,
            })
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn accessors_return_the_fields() {
        let config = sample_config();
        assert_eq!(config.charge_count(), 5);
        assert_eq!(config.amount_per_charge(), 10);
        assert_eq!(config.spread_attempts(), 3);
        assert_eq!(config.growth_rounds(), 2);
        assert_eq!(config.spread_rounds(), 1);
        assert_eq!(
            *config.extra_rare_growths(),
            IntProvider::Constant(ConstantInt::of(0))
        );
        assert_eq!(config.catalyst_chance(), 0.5);
    }

    #[test]
    fn codec_encodes_bounds() {
        // All five intRange windows and the floatRange are inclusive on both
        // ends; encode validates the same way decode does.
        let codec = sculk_patch_configuration_codec::<JsonOps>();
        let at_min = SculkPatchConfiguration::new(
            1,
            1,
            1,
            0,
            0,
            IntProvider::Constant(ConstantInt::of(1)),
            0.0,
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &at_min)
                .result()
                .is_some()
        );
        let at_max = SculkPatchConfiguration::new(
            32,
            500,
            64,
            8,
            8,
            IntProvider::Constant(ConstantInt::of(32)),
            1.0,
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &at_max)
                .result()
                .is_some()
        );
    }

    #[test]
    fn codec_rejects_out_of_range_on_decode() {
        let codec = sculk_patch_configuration_codec::<JsonOps>();
        // charge_count above [1, 32] — the DFU-exact message.
        let high = json!({
            "charge_count": 33,
            "amount_per_charge": 10,
            "spread_attempts": 3,
            "growth_rounds": 2,
            "spread_rounds": 1,
            "extra_rare_growths": 0,
            "catalyst_chance": 0.5,
        });
        let result = codec.parse(&JsonOps::INSTANCE, &high);
        assert!(result.is_error());
        let err = result.error_ref().expect("error should be present");
        assert_eq!(err.message(), "Value 33 outside of range [1:32]");
        // amount_per_charge below [1, 500].
        let low = json!({
            "charge_count": 5,
            "amount_per_charge": 0,
            "spread_attempts": 3,
            "growth_rounds": 2,
            "spread_rounds": 1,
            "extra_rare_growths": 0,
            "catalyst_chance": 0.5,
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &low).is_error());
        // spread_attempts above [1, 64].
        let spread = json!({
            "charge_count": 5,
            "amount_per_charge": 10,
            "spread_attempts": 65,
            "growth_rounds": 2,
            "spread_rounds": 1,
            "extra_rare_growths": 0,
            "catalyst_chance": 0.5,
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &spread).is_error());
        // growth_rounds above [0, 8].
        let growth = json!({
            "charge_count": 5,
            "amount_per_charge": 10,
            "spread_attempts": 3,
            "growth_rounds": 9,
            "spread_rounds": 1,
            "extra_rare_growths": 0,
            "catalyst_chance": 0.5,
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &growth).is_error());
        // spread_rounds above [0, 8].
        let spread_rounds = json!({
            "charge_count": 5,
            "amount_per_charge": 10,
            "spread_attempts": 3,
            "growth_rounds": 2,
            "spread_rounds": 9,
            "extra_rare_growths": 0,
            "catalyst_chance": 0.5,
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &spread_rounds).is_error());
        // catalyst_chance above [0.0, 1.0] — the out-of-range message matches
        // Paper exactly: `check_range_f32` renders bounds via Java's
        // `Float.toString` (PR #557), pinning the message here.
        let chance = json!({
            "charge_count": 5,
            "amount_per_charge": 10,
            "spread_attempts": 3,
            "growth_rounds": 2,
            "spread_rounds": 1,
            "extra_rare_growths": 0,
            "catalyst_chance": 1.5,
        });
        let result = codec.parse(&JsonOps::INSTANCE, &chance);
        let err = result.error_ref().map(|e| e.message().to_string());
        assert!(
            err.as_deref()
                .unwrap_or_default()
                .contains("Value 1.5 outside of range [0.0:1.0]"),
            "catalyst_chance above range should surface Paper's exact message, got: {err:?}"
        );
        // catalyst_chance = -0.0 is below [0.0, 1.0]: `Float.compare` places
        // -0.0 before +0.0 (Paper's `checkRange` rejects it) even though IEEE
        // `-0.0 >= 0.0` is true. `codec::float_range` implements this via
        // `java_float_compare` (via PR #557).
        let negative_zero = json!({
            "charge_count": 5,
            "amount_per_charge": 10,
            "spread_attempts": 3,
            "growth_rounds": 2,
            "spread_rounds": 1,
            "extra_rare_growths": 0,
            "catalyst_chance": -0.0,
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &negative_zero).is_error());
    }

    #[test]
    fn codec_rejects_out_of_range_on_encode() {
        let codec = sculk_patch_configuration_codec::<JsonOps>();
        // charge_count above [1, 32].
        let too_high = SculkPatchConfiguration::new(
            33,
            10,
            3,
            2,
            1,
            IntProvider::Constant(ConstantInt::of(0)),
            0.5,
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &too_high)
                .result()
                .is_none()
        );
        // catalyst_chance above [0.0, 1.0].
        let chance = SculkPatchConfiguration::new(
            5,
            10,
            3,
            2,
            1,
            IntProvider::Constant(ConstantInt::of(0)),
            1.1,
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &chance)
                .result()
                .is_none()
        );
        // catalyst_chance = -0.0 is below [0.0, 1.0] (see the decode test).
        let negative_zero = SculkPatchConfiguration::new(
            5,
            10,
            3,
            2,
            1,
            IntProvider::Constant(ConstantInt::of(0)),
            -0.0,
        );
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &negative_zero)
                .result()
                .is_none()
        );
    }

    #[test]
    fn codec_requires_all_fields() {
        let codec = sculk_patch_configuration_codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!({})).is_error());
        // Six of seven fields — `extra_rare_growths` missing.
        let missing = json!({
            "charge_count": 5,
            "amount_per_charge": 10,
            "spread_attempts": 3,
            "growth_rounds": 2,
            "spread_rounds": 1,
            "catalyst_chance": 0.5,
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &missing).is_error());
    }

    #[test]
    fn value_equality_semantics() {
        let config = sample_config();
        assert_eq!(
            config,
            SculkPatchConfiguration::new(
                5,
                10,
                3,
                2,
                1,
                IntProvider::Constant(ConstantInt::of(0)),
                0.5
            )
        );
        assert_ne!(
            config,
            SculkPatchConfiguration::new(
                6,
                10,
                3,
                2,
                1,
                IntProvider::Constant(ConstantInt::of(0)),
                0.5
            )
        );
        assert_ne!(
            config,
            SculkPatchConfiguration::new(
                5,
                10,
                3,
                2,
                1,
                IntProvider::Uniform(UniformInt::of(1, 4)),
                0.5
            )
        );
        // `Float.compare` canonicalizes every NaN payload: two distinct
        // payloads compare equal (IEEE `==` rejects).
        let nan_a = f32::from_bits(0x7fc0_0001);
        let nan_b = f32::from_bits(0x7fc0_0002);
        assert!(nan_a.is_nan() && nan_b.is_nan());
        assert_ne!(nan_a, nan_b);
        assert_eq!(
            SculkPatchConfiguration::new(
                5,
                10,
                3,
                2,
                1,
                IntProvider::Constant(ConstantInt::of(0)),
                nan_a
            ),
            SculkPatchConfiguration::new(
                5,
                10,
                3,
                2,
                1,
                IntProvider::Constant(ConstantInt::of(0)),
                nan_b
            )
        );
        // `Float.compare(-0.0F, 0.0F) != 0` — signed zero is distinct.
        assert_ne!(
            SculkPatchConfiguration::new(
                5,
                10,
                3,
                2,
                1,
                IntProvider::Constant(ConstantInt::of(0)),
                -0.0
            ),
            SculkPatchConfiguration::new(
                5,
                10,
                3,
                2,
                1,
                IntProvider::Constant(ConstantInt::of(0)),
                0.0
            )
        );
    }
}
