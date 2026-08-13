//! Port of `net.minecraft.world.level.levelgen.heightproviders.ConstantHeight`
//! (class, 26.2).
//!
//! Java: a single-field value class wrapping a `VerticalAnchor` whose `sample`
//! resolves the anchor and whose `type()` is `HeightProviderType.CONSTANT`.
//! `ZERO` is the `VerticalAnchor.absolute(0)` singleton, `of` is the public
//! constructor, `getValue` exposes the anchor. `toString` delegates to the
//! anchor's.
//!
//! ## Codec — two shapes
//!
//! `ConstantHeight.CODEC` is `VerticalAnchor.CODEC.fieldOf("value").xmap(
//! ConstantHeight::new, ConstantHeight::getValue)` — the record form used by the
//! `"constant"` dispatch branch. The top-level `HeightProvider.CODEC` ALSO
//! encodes a `ConstantHeight` through its Left branch (a bare `VerticalAnchor`,
//! see `height_provider`) — Java's `CODEC.xmap` special-cases CONSTANT to emit
//! the anchor directly, so the two shapes never collide on the round trip.

use crate::levelgen::vertical_anchor::VerticalAnchor;
use crate::levelgen::world_generation_context::WorldGenerationContext;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::RandomSource;
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.heightproviders.ConstantHeight`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConstantHeight {
    /// `this.value` — the fixed `VerticalAnchor`.
    value: VerticalAnchor,
}

impl ConstantHeight {
    /// `ConstantHeight.ZERO` — `new ConstantHeight(VerticalAnchor.absolute(0))`.
    pub const ZERO: ConstantHeight = ConstantHeight::of(VerticalAnchor::absolute(0));

    /// `ConstantHeight.of(VerticalAnchor)` — the public factory.
    pub const fn of(value: VerticalAnchor) -> ConstantHeight {
        ConstantHeight { value }
    }

    /// `ConstantHeight.getValue()`.
    pub const fn get_value(&self) -> VerticalAnchor {
        self.value
    }

    /// `ConstantHeight.sample(RandomSource, WorldGenerationContext)` —
    /// `this.value.resolveY(context)`.
    pub fn sample<R: RandomSource>(
        &self,
        _random: &mut R,
        context: &WorldGenerationContext,
    ) -> i32 {
        self.value.resolve_y(context)
    }
}

impl fmt::Display for ConstantHeight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `toString()` — `this.value.toString()`.
        write!(f, "{}", self.value)
    }
}

/// `ConstantHeight.CODEC` — `VerticalAnchor.CODEC.fieldOf("value").xmap(
/// ConstantHeight::new, ConstantHeight::getValue)`, as the ops-generic
/// `constant_height_map_codec::<Ops>()` factory (the record form used by the
/// `"constant"` dispatch branch).
pub fn constant_height_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<ConstantHeight, Ops>> {
    // `VerticalAnchor.CODEC.fieldOf("value")`.
    let field = codec::field_of::<VerticalAnchor, Ops>(
        crate::levelgen::vertical_anchor::vertical_anchor_codec::<Ops>(),
        "value".into(),
    );
    // `.xmap(ConstantHeight::new, ConstantHeight::getValue)`.
    map_codec::xmap(
        field,
        Arc::new(|v: &VerticalAnchor| ConstantHeight::of(*v)),
        Arc::new(|c: &ConstantHeight| c.get_value()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::chunk_generator::ChunkGenerator;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use rivet_registry::core::BlockPos;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// A `ChunkGenerator` double exposing a fixed worldgen window.
    struct TestGenerator {
        min_y: i32,
        depth: i32,
    }
    impl ChunkGenerator for TestGenerator {
        fn create_biomes(&self) {}
        fn apply_carvers(&self) {}
        fn build_surface(&self) {}
        fn spawn_original_mobs(&self) {}
        fn fill_from_noise(&self) {}
        fn get_min_y(&self) -> i32 {
            self.min_y
        }
        fn get_gen_depth(&self) -> i32 {
            self.depth
        }
    }

    /// A `WorldGenLevel` double over a fixed window.
    struct TestLevel(SimpleLevelHeightAccessor);
    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            self.0.get_height()
        }
        fn get_min_y(&self) -> i32 {
            self.0.get_min_y()
        }
    }
    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> rivet_registry::block_state::BlockState {
            // RivetTodo(#399): never read here — sampling only touches the
            // height window.
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }

    fn context(min_y: i32, height: i32, gen_depth: i32) -> WorldGenerationContext {
        let level = TestLevel(create(min_y, height));
        let generator = TestGenerator {
            min_y,
            depth: gen_depth,
        };
        WorldGenerationContext::new(&generator, &level)
    }

    #[test]
    fn zero_is_absolute_zero() {
        assert_eq!(
            ConstantHeight::ZERO.get_value(),
            VerticalAnchor::absolute(0)
        );
    }

    #[test]
    fn sample_resolves_the_anchor() {
        let ctx = context(-64, 384, 384);
        let mut random = rivet_util::random::LegacyRandomSource::new(1);
        let absolute = ConstantHeight::of(VerticalAnchor::absolute(42));
        assert_eq!(absolute.sample(&mut random, &ctx), 42);
        let above = ConstantHeight::of(VerticalAnchor::above_bottom(16));
        assert_eq!(above.sample(&mut random, &ctx), -48);
        let below = ConstantHeight::of(VerticalAnchor::below_top(16));
        assert_eq!(below.sample(&mut random, &ctx), 303);
    }

    #[test]
    fn display_delegates_to_anchor() {
        assert_eq!(
            ConstantHeight::of(VerticalAnchor::absolute(3)).to_string(),
            "3 absolute"
        );
    }

    #[test]
    fn record_codec_round_trips_the_value_field() {
        // The `ConstantHeight.CODEC` record form: `{"value": {...anchor...}}`.
        let codec = map_codec::codec_of(constant_height_map_codec::<JsonOps>());
        let constant = ConstantHeight::of(VerticalAnchor::absolute(5));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &constant)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"value": {"absolute": 5}}));
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &encoded);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(*decoded, constant);
    }
}
