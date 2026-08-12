//! Port of `net.minecraft.world.level.levelgen.feature.stateproviders.
//! NoiseThresholdProvider` (class, 26.2).
//!
//! Java: a `NoiseBasedStateProvider` that branches on the noise value at a
//! position: below `threshold` a random `low_states` entry, otherwise a random
//! `high_states` entry with probability `highChance` else `defaultState`.
//! `type()` is `BlockStateProviderType.NOISE_THRESHOLD_PROVIDER`.
//!
//! `CODEC` is the 8-field record `noiseCodec(i).and(i.group(...))` — the
//! shared `"seed"`/`"noise"`/`"scale"` plus `"threshold"`
//! (`Codec.floatRange(-1.0F, 1.0F)`), `"high_chance"` (`Codec.floatRange(0.0F,
//! 1.0F)`), `"default_state"`, `"low_states"`, `"high_states"` (the last two
//! `ExtraCodecs.nonEmptyList(BlockState.CODEC.listOf())`). The 8-field record
//! exceeds the `record_builder` Group6 cap, so it is composed manually with the
//! local `map_encoder_fields8`/`map_decoder_ap8`/`ap8` helpers.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::stateproviders::block_state_provider::BlockStateProvider;
use crate::levelgen::feature::stateproviders::block_state_provider_type::{
    BlockStateProviderTypeId, BlockStateProviderTypes,
};
use crate::levelgen::feature::stateproviders::codec_helpers::{
    map_decoder_ap8, map_encoder_fields8,
};
use crate::levelgen::feature::stateproviders::noise_based_state_provider::{
    build_noise, get_noise_value,
};
use crate::levelgen::synth::normal_noise::{NoiseParameters, NormalNoise};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.stateproviders.NoiseThresholdProvider`.
#[derive(Debug, Clone)]
pub struct NoiseThresholdProvider {
    /// `this.seed`.
    seed: i64,
    /// `this.parameters`.
    parameters: NoiseParameters,
    /// `this.scale`.
    scale: f32,
    /// `this.threshold`.
    threshold: f32,
    /// `this.highChance`.
    high_chance: f32,
    /// `this.defaultState`.
    default_state: BlockState,
    /// `this.lowStates`.
    low_states: Vec<BlockState>,
    /// `this.highStates`.
    high_states: Vec<BlockState>,
    /// `this.noise` — the lazily-built `NormalNoise` (not part of the codec).
    noise: NormalNoise,
}

impl NoiseThresholdProvider {
    /// `NoiseThresholdProvider(long, NoiseParameters, float, float, float,
    /// BlockState, List, List)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        seed: i64,
        parameters: NoiseParameters,
        scale: f32,
        threshold: f32,
        high_chance: f32,
        default_state: BlockState,
        low_states: Vec<BlockState>,
        high_states: Vec<BlockState>,
    ) -> NoiseThresholdProvider {
        let noise = build_noise(seed, &parameters);
        NoiseThresholdProvider {
            seed,
            parameters,
            scale,
            threshold,
            high_chance,
            default_state,
            low_states,
            high_states,
            noise,
        }
    }
}

impl BlockStateProvider for NoiseThresholdProvider {
    fn get_state<R: RandomSource>(
        &self,
        _level: &dyn WorldGenLevel,
        random: &mut R,
        pos: &BlockPos,
    ) -> BlockState {
        let local_value = get_noise_value(&self.noise, pos, self.scale as f64);
        if local_value < self.threshold as f64 {
            rivet_util::util::get_random(&self.low_states, random)
        } else if random.next_float() < self.high_chance {
            rivet_util::util::get_random(&self.high_states, random)
        } else {
            self.default_state
        }
    }

    fn type_id(&self) -> BlockStateProviderTypeId {
        BlockStateProviderTypes::NOISE_THRESHOLD_PROVIDER
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `NoiseThresholdProvider.CODEC` — the 8-field record
/// (`noiseCodec(...).and(...).apply(...)`), as the ops-generic
/// `noise_threshold_provider_map_codec::<Ops>()` factory.
pub fn noise_threshold_provider_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<NoiseThresholdProvider, Ops>> {
    let seed = codec::field_of(codec::long_codec::<Ops>(), "seed".to_string());
    let parameters = codec::field_of(
        crate::levelgen::synth::normal_noise::noise_parameters_direct_codec::<Ops>(),
        "noise".to_string(),
    );
    let scale = codec::field_of(
        crate::levelgen::feature::stateproviders::noise_based_state_provider::positive_float::<Ops>(
        ),
        "scale".to_string(),
    );
    let threshold = codec::field_of(
        codec::float_range::<Ops>(-1.0, 1.0),
        "threshold".to_string(),
    );
    let high_chance = codec::field_of(
        codec::float_range::<Ops>(0.0, 1.0),
        "high_chance".to_string(),
    );
    let default_state = codec::field_of(
        rivet_registry::block_state_codec::block_state_codec::<Ops>(),
        "default_state".to_string(),
    );
    let non_empty_states = |name: &str| {
        codec::field_of::<Vec<BlockState>, Ops>(
            rivet_serialization::extra_codecs::non_empty_list::<BlockState, Ops>(codec::list(
                rivet_registry::block_state_codec::block_state_codec::<Ops>(),
            )),
            name.to_string(),
        )
    };
    let low_states = non_empty_states("low_states");
    let high_states = non_empty_states("high_states");

    map_codec::of(
        map_encoder_fields8(
            seed.clone(),
            parameters.clone(),
            scale.clone(),
            threshold.clone(),
            high_chance.clone(),
            default_state.clone(),
            low_states.clone(),
            high_states.clone(),
            Arc::new(|p: &NoiseThresholdProvider| p.seed),
            Arc::new(|p: &NoiseThresholdProvider| p.parameters.clone()),
            Arc::new(|p: &NoiseThresholdProvider| p.scale),
            Arc::new(|p: &NoiseThresholdProvider| p.threshold),
            Arc::new(|p: &NoiseThresholdProvider| p.high_chance),
            Arc::new(|p: &NoiseThresholdProvider| p.default_state),
            Arc::new(|p: &NoiseThresholdProvider| p.low_states.clone()),
            Arc::new(|p: &NoiseThresholdProvider| p.high_states.clone()),
        ),
        map_decoder_ap8(
            seed,
            parameters,
            scale,
            threshold,
            high_chance,
            default_state,
            low_states,
            high_states,
            Arc::new(
                |seed: &i64,
                 parameters: &NoiseParameters,
                 scale: &f32,
                 threshold: &f32,
                 high_chance: &f32,
                 default_state: &BlockState,
                 low_states: &Vec<BlockState>,
                 high_states: &Vec<BlockState>| {
                    NoiseThresholdProvider::new(
                        *seed,
                        parameters.clone(),
                        *scale,
                        *threshold,
                        *high_chance,
                        *default_state,
                        low_states.clone(),
                        high_states.clone(),
                    )
                },
            ),
        ),
        "NoiseThresholdProvider".to_string(),
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

    fn state(id: u16) -> BlockState {
        BlockState::of(BlockId::from_id(id))
    }

    #[test]
    fn codec_round_trips_the_record() {
        let codec = map_codec::codec_of(noise_threshold_provider_map_codec::<JsonOps>());
        let input = json!({
            "seed": 7,
            "noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "scale": 2.0,
            "threshold": 0.5,
            "high_chance": 0.25,
            "default_state": {"Name": "minecraft:air"},
            "low_states": [{"Name": "minecraft:stone"}, {"Name": "minecraft:dirt"}],
            "high_states": [{"Name": "minecraft:oak_planks"}]
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            decoded.type_id(),
            BlockStateProviderTypes::NOISE_THRESHOLD_PROVIDER
        );
        assert_eq!(decoded.seed, 7);
        assert_eq!(decoded.threshold, 0.5);
        assert_eq!(decoded.high_chance, 0.25);
        assert_eq!(decoded.default_state, state(0));
        assert_eq!(decoded.low_states, vec![state(1), state(9)]);
        assert_eq!(decoded.high_states, vec![state(13)]);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn codec_rejects_threshold_out_of_range() {
        let codec = map_codec::codec_of(noise_threshold_provider_map_codec::<JsonOps>());
        let input = json!({
            "seed": 7,
            "noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "scale": 2.0,
            "threshold": 1.5,
            "high_chance": 0.25,
            "default_state": {"Name": "minecraft:air"},
            "low_states": [{"Name": "minecraft:stone"}],
            "high_states": [{"Name": "minecraft:oak_planks"}]
        });
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        // The `threshold` field errors last, but the record surfaces the whole
        // field-failure chain. `threshold` is `Codec.floatRange` (DFU), not
        // `ExtraCodecs.floatRange`: the message is "Value X outside of range
        // [min:max]" and the check uses Float.compare total order.
        assert!(
            msg.ends_with("Value 1.5 outside of range [-1.0:1.0]"),
            "got: {msg}"
        );
    }

    #[test]
    fn codec_rejects_threshold_below_min() {
        // A negative value below `-1.0` must be rejected: the `Float.compare`
        // total order orders the negative reals IEEE-wise (`-5.0 < -1.0`), so
        // the signed-intBits fallback in a naive total-order port would
        // mis-order them. This pins the ordered `<`/`>` first.
        let codec = map_codec::codec_of(noise_threshold_provider_map_codec::<JsonOps>());
        let input = json!({
            "seed": 7,
            "noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "scale": 2.0,
            "threshold": -5.0,
            "high_chance": 0.25,
            "default_state": {"Name": "minecraft:air"},
            "low_states": [{"Name": "minecraft:stone"}],
            "high_states": [{"Name": "minecraft:oak_planks"}]
        });
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.ends_with("Value -5.0 outside of range [-1.0:1.0]"),
            "got: {msg}"
        );
    }

    #[test]
    fn codec_rejects_negative_zero_high_chance() {
        // `high_chance` is `Codec.floatRange(0.0F, 1.0F)` (DFU), whose check
        // uses Float.compare total order: `Float.compare(-0.0, 0.0) = -1`, so a
        // `-0.0` value fails the `0.0` lower bound. Rust `PartialOrd` would
        // accept `-0.0 >= 0.0`, so this pins the total-order comparison.
        let codec = map_codec::codec_of(noise_threshold_provider_map_codec::<JsonOps>());
        let input = json!({
            "seed": 7,
            "noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "scale": 2.0,
            "threshold": 0.0,
            "high_chance": -0.0,
            "default_state": {"Name": "minecraft:air"},
            "low_states": [{"Name": "minecraft:stone"}],
            "high_states": [{"Name": "minecraft:oak_planks"}]
        });
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        // The `high_chance` field errors last; the record surfaces the whole
        // field-failure chain.
        assert!(
            msg.ends_with("Value -0.0 outside of range [0.0:1.0]"),
            "got: {msg}"
        );
    }

    #[test]
    fn codec_rejects_empty_low_states() {
        let codec = map_codec::codec_of(noise_threshold_provider_map_codec::<JsonOps>());
        let input = json!({
            "seed": 7,
            "noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "scale": 2.0,
            "threshold": 0.0,
            "high_chance": 0.25,
            "default_state": {"Name": "minecraft:air"},
            "low_states": [],
            "high_states": [{"Name": "minecraft:oak_planks"}]
        });
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        // The `low_states` field errors last; the record surfaces the whole
        // field-failure chain.
        assert!(msg.ends_with("List must have contents"), "got: {msg}");
    }

    #[test]
    fn get_state_branches_on_threshold() {
        // `default_state` is returned when the noise is >= threshold and the
        // random draw >= highChance. With threshold 1.0 the noise (bounded
        // near [-1, 1] for these coords) is below it → a low_states member.
        let p = NoiseThresholdProvider::new(
            42,
            parameters(),
            1.0,
            1.0,
            0.0,
            state(0),
            vec![state(1)],
            vec![state(13)],
        );
        let mut random = rivet_util::random::LegacyRandomSource::new(1);
        let got = p.get_state(&TestLevel, &mut random, &BlockPos::new(0, 0, 0));
        assert!(got == state(1));
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
