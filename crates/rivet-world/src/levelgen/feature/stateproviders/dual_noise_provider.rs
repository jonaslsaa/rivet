//! Port of `net.minecraft.world.level.levelgen.feature.stateproviders.
//! DualNoiseProvider` (class, 26.2).
//!
//! Java: a `NoiseProvider` that first samples a slow-noise-determined "local
//! variety" `localVariety` (`Mth.clampedMap(getSlowNoiseValue(pos), -1.0, 1.0,
//! variety.minInclusive, variety.maxInclusive + 1)`), builds a
//! `possibleStates` list of that size — each entry drawn via the SLOW noise
//! (`getRandomState(states, getSlowNoiseValue(pos.offset(i * 54545, 0, i *
//! 34234)))`) — then returns `getRandomState(possibleStates, pos, this.scale)`,
//! where the final selection indexes into `possibleStates` via the regular
//! `noise` (scaled by `this.scale`), not the slow noise. `type()` is
//! `BlockStateProviderType.DUAL_NOISE_PROVIDER`.
//!
//! `CODEC` is the 7-field record `i.group(variety, slow_noise, slow_scale)
//! .and(noiseProviderCodec(i)).apply(i, DualNoiseProvider::new)` — the three
//! extra fields (`variety` = `InclusiveRange.codec(Codec.INT, 1, 64)`,
//! `slow_noise` = `NormalNoise.NoiseParameters.DIRECT_CODEC`, `slow_scale` =
//! `ExtraCodecs.POSITIVE_FLOAT`) on top of `NoiseProvider`'s
//! `seed`/`noise`/`scale`/`states`. The 7-field record exceeds the
//! `record_builder` Group6 cap, so it is composed manually with the local
//! `map_encoder_fields7`/`map_decoder_ap7`/`ap7` helpers.
//!
//! The Rust port does not model the Java inheritance (`DualNoiseProvider extends
//! NoiseProvider`): it reuses the shared free helpers `get_random_state_at` /
//! `get_random_state_from_value` instead.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::stateproviders::block_state_provider::BlockStateProvider;
use crate::levelgen::feature::stateproviders::block_state_provider_type::{
    BlockStateProviderTypeId, BlockStateProviderTypes,
};
use crate::levelgen::feature::stateproviders::codec_helpers::{
    map_decoder_ap7, map_encoder_fields7,
};
use crate::levelgen::feature::stateproviders::noise_based_state_provider::{
    build_noise, positive_float,
};
use crate::levelgen::feature::stateproviders::noise_provider::{
    get_random_state_at, get_random_state_from_value,
};
use crate::levelgen::synth::normal_noise::{NoiseParameters, NormalNoise};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::RandomSource;
use rivet_util::inclusive_range::{InclusiveRange, inclusive_range_codec};
use rivet_util::mth::clamped_map;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.stateproviders.DualNoiseProvider`.
#[derive(Debug, Clone)]
pub struct DualNoiseProvider {
    /// `this.variety`.
    variety: InclusiveRange,
    /// `this.slowNoiseParameters`.
    slow_noise_parameters: NoiseParameters,
    /// `this.slowScale`.
    slow_scale: f32,
    /// `this.seed` (from the `NoiseProvider` base).
    seed: i64,
    /// `this.parameters` (from the `NoiseProvider` base).
    parameters: NoiseParameters,
    /// `this.scale` (from the `NoiseProvider` base).
    scale: f32,
    /// `this.states` (from the `NoiseProvider` base).
    states: Vec<BlockState>,
    /// `this.noise` — the regular `NormalNoise` from the base constructor.
    noise: NormalNoise,
    /// `this.slowNoise` — `NormalNoise.create(new WorldgenRandom(new
    /// LegacyRandomSource(seed)), slowNoiseParameters)`.
    slow_noise: NormalNoise,
}

impl DualNoiseProvider {
    /// `DualNoiseProvider(InclusiveRange, NoiseParameters, float, long,
    /// NoiseParameters, float, List<BlockState>)` — the codec constructor,
    /// `super(seed, parameters, scale, states)` then the slow noise.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        variety: InclusiveRange,
        slow_noise_parameters: NoiseParameters,
        slow_scale: f32,
        seed: i64,
        parameters: NoiseParameters,
        scale: f32,
        states: Vec<BlockState>,
    ) -> DualNoiseProvider {
        let noise = build_noise(seed, &parameters);
        let slow_noise = build_noise(seed, &slow_noise_parameters);
        DualNoiseProvider {
            variety,
            slow_noise_parameters,
            slow_scale,
            seed,
            parameters,
            scale,
            states,
            noise,
            slow_noise,
        }
    }
}

impl BlockStateProvider for DualNoiseProvider {
    fn get_state<R: RandomSource>(
        &self,
        _level: &dyn WorldGenLevel,
        _random: &mut R,
        pos: &BlockPos,
    ) -> BlockState {
        // `double varietyNoise = this.getSlowNoiseValue(pos); int localVariety =
        // (int)Mth.clampedMap(varietyNoise, -1.0, 1.0, this.variety.minInclusive
        // .intValue(), this.variety.maxInclusive() + 1);` — the `+ 1` is
        // Java-int wrapping arithmetic (`Integer` unboxed to `int`, then
        // widened to `double` for `clampedMap`), Java-int `(int)` cast.
        let variety_noise = self.get_slow_noise_value(pos);
        let local_variety = clamped_map(
            variety_noise,
            -1.0,
            1.0,
            self.variety.min_inclusive as f64,
            self.variety.max_inclusive.wrapping_add(1) as f64,
        ) as i32;

        // `List<BlockState> possibleStates = Lists.newArrayListWithCapacity(
        // localVariety); for (int i = 0; i < localVariety; i++) {
        // possibleStates.add(this.getRandomState(this.states,
        // this.getSlowNoiseValue(pos.offset(i * 54545, 0, i * 34234)))); }` —
        // Java `pos.offset(x, y, z)` with `i * 54545` / `i * 34234` wrapping
        // `i32` multiply. The codec caps `variety` to [1, 64], so `localVariety`
        // is always >= 1 on the decode path. The `.max(0)` on the capacity does
        // NOT guard the public-constructor path (e.g. `InclusiveRange::new(-5,
        // 10)`): it only avoids the negative-capacity process abort, deferring
        // the crash — with `localVariety <= 0` the loop is empty and the final
        // `get_random_state_from_value` indexes the empty `possible_states`,
        // panicking with index-out-of-bounds. Java fails on this path too, but
        // earlier and with a clear message: Guava's `newArrayListWithCapacity(-5)`
        // throws `IllegalArgumentException`.
        let mut possible_states: Vec<BlockState> =
            Vec::with_capacity(local_variety.max(0) as usize);
        for i in 0..local_variety {
            let offset_pos = pos.offset(i.wrapping_mul(54545), 0, i.wrapping_mul(34234));
            possible_states.push(get_random_state_from_value(
                &self.states,
                self.get_slow_noise_value(&offset_pos),
            ));
        }

        // `return this.getRandomState(possibleStates, pos, this.scale);`
        get_random_state_at(&self.noise, &possible_states, pos, self.scale as f64)
    }

    fn type_id(&self) -> BlockStateProviderTypeId {
        BlockStateProviderTypes::DUAL_NOISE_PROVIDER
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl DualNoiseProvider {
    /// `getSlowNoiseValue(BlockPos)` — `this.slowNoise.getValue(pos.getX() *
    /// this.slowScale, pos.getY() * this.slowScale, pos.getZ() * this.slowScale)`.
    ///
    /// Java `pos.getX() * this.slowScale` is `int * float`: binary numeric
    /// promotion computes the product in 32-bit `float`, and the `float` result
    /// is then widened to `double` for `getValue(double, double, double)`. The
    /// Rust port mirrors that by rounding the product in `f32` before the `f64`
    /// widen — the `as f64` product would keep 53-bit mantissa precision and
    /// skip Java's intermediate 24-bit rounding.
    fn get_slow_noise_value(&self, pos: &BlockPos) -> f64 {
        self.slow_noise.get_value(
            (pos.get_x() as f32 * self.slow_scale) as f64,
            (pos.get_y() as f32 * self.slow_scale) as f64,
            (pos.get_z() as f32 * self.slow_scale) as f64,
        )
    }
}

/// `DualNoiseProvider.CODEC` — the 7-field record
/// (`i.group(variety, slow_noise, slow_scale).and(noiseProviderCodec(i))`), as
/// the ops-generic `dual_noise_provider_map_codec::<Ops>()` factory.
pub fn dual_noise_provider_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<DualNoiseProvider, Ops>> {
    let variety = codec::field_of(
        inclusive_range_codec::<Ops>(codec::int_codec::<Ops>(), 1, 64),
        "variety".to_string(),
    );
    let slow_noise = codec::field_of(
        crate::levelgen::synth::normal_noise::noise_parameters_direct_codec::<Ops>(),
        "slow_noise".to_string(),
    );
    let slow_scale = codec::field_of(positive_float::<Ops>(), "slow_scale".to_string());
    // The `noiseProviderCodec` fields: `seed`/`noise`/`scale`/`states`.
    let seed = codec::field_of(codec::long_codec::<Ops>(), "seed".to_string());
    let parameters = codec::field_of(
        crate::levelgen::synth::normal_noise::noise_parameters_direct_codec::<Ops>(),
        "noise".to_string(),
    );
    let scale = codec::field_of(positive_float::<Ops>(), "scale".to_string());
    let states = codec::field_of::<Vec<BlockState>, Ops>(
        rivet_serialization::extra_codecs::non_empty_list::<BlockState, Ops>(codec::list(
            rivet_registry::block_state_codec::block_state_codec::<Ops>(),
        )),
        "states".to_string(),
    );

    map_codec::of(
        map_encoder_fields7(
            variety.clone(),
            slow_noise.clone(),
            slow_scale.clone(),
            seed.clone(),
            parameters.clone(),
            scale.clone(),
            states.clone(),
            Arc::new(|p: &DualNoiseProvider| p.variety),
            Arc::new(|p: &DualNoiseProvider| p.slow_noise_parameters.clone()),
            Arc::new(|p: &DualNoiseProvider| p.slow_scale),
            Arc::new(|p: &DualNoiseProvider| p.seed),
            Arc::new(|p: &DualNoiseProvider| p.parameters.clone()),
            Arc::new(|p: &DualNoiseProvider| p.scale),
            Arc::new(|p: &DualNoiseProvider| p.states.clone()),
        ),
        map_decoder_ap7(
            variety,
            slow_noise,
            slow_scale,
            seed,
            parameters,
            scale,
            states,
            Arc::new(
                |variety: &InclusiveRange,
                 slow_noise_parameters: &NoiseParameters,
                 slow_scale: &f32,
                 seed: &i64,
                 parameters: &NoiseParameters,
                 scale: &f32,
                 states: &Vec<BlockState>| {
                    DualNoiseProvider::new(
                        *variety,
                        slow_noise_parameters.clone(),
                        *slow_scale,
                        *seed,
                        parameters.clone(),
                        *scale,
                        states.clone(),
                    )
                },
            ),
        ),
        "DualNoiseProvider".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn parameters() -> NoiseParameters {
        NoiseParameters::new(0, vec![1.0])
    }

    fn air() -> BlockState {
        BlockState::of(BlockId::from_id(0))
    }

    fn stone() -> BlockState {
        BlockState::of(BlockId::from_id(1))
    }

    #[test]
    fn codec_round_trips_the_record() {
        let codec =
            rivet_serialization::map_codec::codec_of(dual_noise_provider_map_codec::<JsonOps>());
        let input = json!({
            "variety": [1, 2],
            "slow_noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "slow_scale": 0.5,
            "seed": 7,
            "noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "scale": 1.0,
            "states": [{"Name": "minecraft:air"}, {"Name": "minecraft:stone"}]
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            decoded.type_id(),
            BlockStateProviderTypes::DUAL_NOISE_PROVIDER
        );
        assert_eq!(decoded.variety, InclusiveRange::new(1, 2));
        assert_eq!(decoded.slow_scale, 0.5);
        assert_eq!(decoded.seed, 7);
        assert_eq!(decoded.states, vec![air(), stone()]);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        // `variety` re-encodes as `[min, max]` (the `intervalCodec` array form).
        assert_eq!(encoded["variety"], json!([1, 2]));
        assert_eq!(encoded, input);
    }

    #[test]
    fn codec_rejects_variety_out_of_bounds() {
        let codec =
            rivet_serialization::map_codec::codec_of(dual_noise_provider_map_codec::<JsonOps>());
        let input = json!({
            "variety": [1, 65],
            "slow_noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "slow_scale": 0.5,
            "seed": 7,
            "noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "scale": 1.0,
            "states": [{"Name": "minecraft:air"}]
        });
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        // The `variety` field errors last; the record surfaces the whole
        // field-failure chain.
        assert!(
            msg.ends_with("Range limit too high, expected at most 64 [1-65]"),
            "got: {msg}"
        );
    }

    #[test]
    fn codec_rejects_inverted_variety() {
        let codec =
            rivet_serialization::map_codec::codec_of(dual_noise_provider_map_codec::<JsonOps>());
        let input = json!({
            "variety": [4, 2],
            "slow_noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "slow_scale": 0.5,
            "seed": 7,
            "noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "scale": 1.0,
            "states": [{"Name": "minecraft:air"}]
        });
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        // The `variety` field errors last; the record surfaces the whole
        // field-failure chain.
        assert!(
            msg.ends_with(
                "Failed to parse either. First: Not a number: [4,2]; Second: Failed to parse either. First: min_inclusive must be less than or equal to max_inclusive; Second: Not a JSON object: [4,2]"
            ),
            "got: {msg}"
        );
    }

    #[test]
    fn codec_rejects_two_fields_in_reverse_order() {
        // Java's `ap7` chain (`ap4(ap3(map(Function7::curry3, func), t1, t2,
        // t3), t4, t5, t6, t7)` through `Error.ap`) accumulates failing-field
        // messages in REVERSE field order: the last field's message leads, the
        // first field's trails. Failing `states` (last) and `variety` (first)
        // pins the exact concatenation — a regression that swapped the ap3/ap4
        // groups or used forward accumulation would flip this.
        let codec =
            rivet_serialization::map_codec::codec_of(dual_noise_provider_map_codec::<JsonOps>());
        let input = json!({
            "variety": [1, 65],
            "slow_noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "slow_scale": 0.5,
            "seed": 7,
            "noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "scale": 1.0,
            "states": []
        });
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(
            msg, "List must have contents; Range limit too high, expected at most 64 [1-65]",
            "field failures must accumulate in reverse field order (states first, variety last)"
        );
    }

    #[test]
    fn slow_noise_feeds_the_f32_rounded_product() {
        // Java `pos.getX() * slowScale` (int * float) computes the product in
        // 32-bit float before widening to double for `getValue`. At x = 34234
        // with slow_scale = 0.1 the f32 product (3423.400146484375) differs
        // from the f64 product (3423.4) — a parity-relevant coordinate. The
        // provider must feed the f32-rounded product, not the f64 one.
        let p = DualNoiseProvider::new(
            InclusiveRange::new(1, 64),
            parameters(),
            0.1,
            42,
            parameters(),
            1.0,
            vec![air(), stone()],
        );
        let pos = BlockPos::new(34234, 0, 0);
        let actual = p.get_slow_noise_value(&pos);
        let f32_path = p
            .slow_noise
            .get_value((34234_f32 * 0.1_f32) as f64, 0.0, 0.0);
        let f64_path = p.slow_noise.get_value(34234_f64 * 0.1_f64, 0.0, 0.0);
        assert_ne!(
            f32_path.to_bits(),
            f64_path.to_bits(),
            "the chosen coordinate must genuinely diverge f32 vs f64"
        );
        assert_eq!(
            actual.to_bits(),
            f32_path.to_bits(),
            "slow noise must use the f32-rounded product"
        );
    }

    #[test]
    fn get_state_returns_one_of_the_possible_states() {
        // Whatever the noise values, the result is drawn from `states` (the
        // slow-noise offsets make the per-entry `getRandomState` hit `states`
        // too), or is a member of `states` through the final selection.
        let p = DualNoiseProvider::new(
            InclusiveRange::new(1, 64),
            parameters(),
            0.5,
            42,
            parameters(),
            1.0,
            vec![air(), stone()],
        );
        let mut random = rivet_util::random::LegacyRandomSource::new(1);
        for pos in [
            BlockPos::new(0, 0, 0),
            BlockPos::new(1, 2, 3),
            BlockPos::new(-5, 10, 7),
        ] {
            let state = p.get_state(&TestLevel, &mut random, &pos);
            assert!(
                state == air() || state == stone(),
                "unexpected state {state:?}"
            );
        }
    }

    struct TestLevel;

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> BlockState {
            // RivetTodo(#399): never read here.
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }
}
