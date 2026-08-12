//! Port of `net.minecraft.world.level.levelgen.feature.stateproviders.
//! NoiseProvider` (class, 26.2).
//!
//! Java: a `NoiseBasedStateProvider` that maps the noise value at a position to
//! an index into a `states` list: `placementValue = Mth.clamp((1.0 +
//! noiseValue) / 2.0, 0.0, 0.9999); states.get((int)(placementValue *
//! states.size()))`. `type()` is `BlockStateProviderType.NOISE_PROVIDER`.
//!
//! `CODEC` is `noiseProviderCodec(i).apply(i, NoiseProvider::new)` — the
//! shared `noiseCodec` (`"seed"`/`"noise"`/`"scale"`) plus the `"states"`
//! field, `ExtraCodecs.nonEmptyList(BlockState.CODEC.listOf())`. The 4-field
//! record fits the `record_builder` Group6 cap.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::stateproviders::block_state_provider::BlockStateProvider;
use crate::levelgen::feature::stateproviders::block_state_provider_type::{
    BlockStateProviderTypeId, BlockStateProviderTypes,
};
use crate::levelgen::feature::stateproviders::noise_based_state_provider::{
    build_noise, get_noise_value, positive_float,
};
use crate::levelgen::synth::normal_noise::{NoiseParameters, NormalNoise};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::mth::clamp_f64;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.stateproviders.NoiseProvider`.
#[derive(Debug, Clone)]
pub struct NoiseProvider {
    /// `this.seed`.
    seed: i64,
    /// `this.parameters`.
    parameters: NoiseParameters,
    /// `this.scale`.
    scale: f32,
    /// `this.states`.
    states: Vec<BlockState>,
    /// `this.noise` — the lazily-built `NormalNoise` (not part of the codec).
    noise: NormalNoise,
}

impl NoiseProvider {
    /// `NoiseProvider(long, NoiseParameters, float, List<BlockState>)` — the
    /// public constructor; builds the `NormalNoise` from the seed via the
    /// base.
    pub fn new(
        seed: i64,
        parameters: NoiseParameters,
        scale: f32,
        states: Vec<BlockState>,
    ) -> NoiseProvider {
        let noise = build_noise(seed, &parameters);
        NoiseProvider {
            seed,
            parameters,
            scale,
            states,
            noise,
        }
    }

    /// `this.states`.
    pub fn states(&self) -> &[BlockState] {
        &self.states
    }

    /// `getRandomState(List, BlockPos, double scale)` — the protected base
    /// method: `getRandomState(states, this.getNoiseValue(pos, scale))`.
    pub(crate) fn get_random_state(
        &self,
        states: &[BlockState],
        pos: &BlockPos,
        scale: f64,
    ) -> BlockState {
        get_random_state_at(&self.noise, states, pos, scale)
    }
}

/// `NoiseProvider.getRandomState(List, BlockPos, double)` — the shared
/// protected method, as a free function so `DualNoiseProvider` (a sibling, not
/// a subclass in the Rust port) reuses it: `getRandomState(states,
/// getNoiseValue(pos, scale))` on the given noise.
pub(crate) fn get_random_state_at(
    noise: &NormalNoise,
    states: &[BlockState],
    pos: &BlockPos,
    scale: f64,
) -> BlockState {
    let noise_value = get_noise_value(noise, pos, scale);
    get_random_state_from_value(states, noise_value)
}

/// `NoiseProvider.getRandomState(List, double noiseValue)` — the shared
/// protected method, as a free function: `placementValue = Mth.clamp((1.0 +
/// noiseValue) / 2.0, 0.0, 0.9999); states.get((int)(placementValue *
/// states.size()))`.
pub(crate) fn get_random_state_from_value(states: &[BlockState], noise_value: f64) -> BlockState {
    let placement_value = clamp_f64((1.0 + noise_value) / 2.0, 0.0, 0.9999);
    states[(placement_value * states.len() as f64) as usize]
}

impl BlockStateProvider for NoiseProvider {
    fn get_state<R: RandomSource>(
        &self,
        _level: &dyn WorldGenLevel,
        _random: &mut R,
        pos: &BlockPos,
    ) -> BlockState {
        self.get_random_state(&self.states, pos, self.scale as f64)
    }

    fn type_id(&self) -> BlockStateProviderTypeId {
        BlockStateProviderTypes::NOISE_PROVIDER
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `NoiseProvider.CODEC` — `noiseProviderCodec(i).apply(i, NoiseProvider::new)`
/// (the `"seed"`/`"noise"`/`"scale"`/`"states"` record), as the ops-generic
/// `noise_provider_map_codec::<Ops>()` factory.
pub fn noise_provider_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<NoiseProvider, Ops>>
{
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|p: &NoiseProvider| p.seed),
                // `Codec.LONG.fieldOf("seed")`.
                codec::field_of(codec::long_codec::<Ops>(), "seed".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &NoiseProvider| p.parameters.clone()),
                // `NormalNoise.NoiseParameters.DIRECT_CODEC.fieldOf("noise")`.
                codec::field_of(
                    crate::levelgen::synth::normal_noise::noise_parameters_direct_codec::<Ops>(),
                    "noise".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &NoiseProvider| p.scale),
                // `ExtraCodecs.POSITIVE_FLOAT.fieldOf("scale")`.
                codec::field_of(positive_float::<Ops>(), "scale".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &NoiseProvider| p.states.clone()),
                // `ExtraCodecs.nonEmptyList(BlockState.CODEC.listOf()).fieldOf(
                // "states")`.
                codec::field_of::<Vec<BlockState>, Ops>(
                    rivet_serialization::extra_codecs::non_empty_list::<BlockState, Ops>(
                        codec::list(rivet_registry::block_state_codec::block_state_codec::<Ops>()),
                    ),
                    "states".to_string(),
                ),
            ))
            .apply(instance, Arc::new(NoiseProvider::new))
    })
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

    fn stone() -> BlockState {
        BlockState::of(BlockId::from_id(1))
    }

    fn air() -> BlockState {
        BlockState::of(BlockId::from_id(0))
    }

    #[test]
    fn get_state_selects_by_noise_value() {
        // The noise is built deterministically from the seed. Whatever the
        // value, `getState` returns one of the two states.
        let p = NoiseProvider::new(42, parameters(), 1.0, vec![air(), stone()]);
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

    #[test]
    fn get_random_state_from_value_bounds() {
        // placementValue clamped to [0, 0.9999]; index 0 for noise -1, index
        // 1 for noise +1.
        assert_eq!(get_random_state_from_value(&[air(), stone()], -1.0), air());
        assert_eq!(get_random_state_from_value(&[air(), stone()], 1.0), stone());
        // noise values outside [-1, 1] clamp.
        assert_eq!(get_random_state_from_value(&[air(), stone()], -10.0), air());
        assert_eq!(
            get_random_state_from_value(&[air(), stone()], 10.0),
            stone()
        );
    }

    #[test]
    fn codec_round_trips_the_record() {
        let codec = rivet_serialization::map_codec::codec_of(noise_provider_map_codec::<JsonOps>());
        let input = json!({
            "seed": 42,
            "noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "scale": 1.0,
            "states": [{"Name": "minecraft:air"}, {"Name": "minecraft:stone"}]
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(decoded.type_id(), BlockStateProviderTypes::NOISE_PROVIDER);
        assert_eq!(decoded.seed, 42);
        assert_eq!(decoded.parameters, parameters());
        assert_eq!(decoded.scale, 1.0);
        assert_eq!(decoded.states, vec![air(), stone()]);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn codec_rejects_non_positive_scale() {
        let codec = rivet_serialization::map_codec::codec_of(noise_provider_map_codec::<JsonOps>());
        let input = json!({
            "seed": 42,
            "noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "scale": 0.0,
            "states": [{"Name": "minecraft:air"}]
        });
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        // The `scale` field errors last, but the record surfaces the whole
        // field-failure chain (`states`/`scale`).
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.ends_with("Value must be positive: 0.0"), "got: {msg}");
    }

    #[test]
    fn codec_rejects_empty_states() {
        let codec = rivet_serialization::map_codec::codec_of(noise_provider_map_codec::<JsonOps>());
        let input = json!({
            "seed": 42,
            "noise": {"firstOctave": 0, "amplitudes": [1.0]},
            "scale": 1.0,
            "states": []
        });
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "List must have contents");
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
