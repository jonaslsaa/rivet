//! Port of `net.minecraft.world.level.levelgen.placement.CountOnEveryLayerPlacement`
//! (class, 26.2, `@Deprecated`).
//!
//! Java: a modifier holding an `IntProvider` `count` whose `getPositions`
//! scatters sample-draws per "layer" into the origin's chunk column, finding
//! each draw's on-ground Y via `findOnGroundYPosition`, and repeats the layer
//! scan until a layer finds nothing. Java's inner `for (int i = 0; i <
//! this.count.sample(random); i++)` re-evaluates `sample` on EVERY iteration,
//! so a non-constant `IntProvider` (e.g. `UniformInt`, bounded `[0, 256]`)
//! re-draws the bound — and consumes RNG state — each pass; the port mirrors
//! that exactly. Its `CODEC` is the `"count"` field (`IntProviders.codec(0,
//! 256)`), and its `type()` is `PlacementModifierType.COUNT_ON_EVERY_LAYER`.
//!
//! The `getHeight(Heightmap.Types.MOTION_BLOCKING, x, z)` and `getBlockState`
//! reads are the `#228`/`#399` world-access seams (see `placement_context.rs` /
//! `world_gen_level.rs`) — they fail explicitly rather than fabricating a
//! surface, exactly as the sibling placement-filter unit treats them.

use crate::levelgen::heightmap::Types;
use crate::levelgen::placement::placement_modifier_type::PlacementModifierTypes;
use crate::levelgen::placement::{PlacementContext, PlacementModifier};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::core::MutableBlockPos;
use rivet_registry::generated::blocks::BlockId;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use rivet_util::valueproviders::constant_int::ConstantInt;
use rivet_util::valueproviders::int_provider::IntProvider;
use rivet_util::valueproviders::int_provider::int_provider_codec_with_bounds;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.CountOnEveryLayerPlacement`.
#[derive(Debug, Clone, PartialEq)]
pub struct CountOnEveryLayerPlacement {
    /// `this.count` — the per-layer draw count, `[0, 256]`.
    count: IntProvider,
}

impl CountOnEveryLayerPlacement {
    /// `of(IntProvider)` — the public factory.
    pub fn of(count: IntProvider) -> Self {
        CountOnEveryLayerPlacement { count }
    }

    /// `of(int)` — `of(ConstantInt.of(count))`.
    pub fn of_int(count: i32) -> Self {
        CountOnEveryLayerPlacement::of(IntProvider::Constant(ConstantInt::of(count)))
    }
}

impl PlacementModifier for CountOnEveryLayerPlacement {
    fn get_positions<'a, R: RandomSource>(
        &'a self,
        context: &PlacementContext,
        random: &mut R,
        origin: &BlockPos,
    ) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
        // Eager shape is bounded: `count` is `IntProviders.codec(0, 256)` and
        // the layer loop terminates once a layer finds no on-ground position
        // (the column's air gaps are exhausted, bounded by the world height),
        // so the positions vector cannot grow without bound. The `Box<dyn
        // Iterator>` return keeps the uniform lazy `PlacementModifier` contract.
        let mut positions = Vec::new();
        let mut layer = 0i32;

        let mut found_any;
        loop {
            found_any = false;

            // `for (int i = 0; i < this.count.sample(random); i++)` — Java
            // re-evaluates `sample` (re-drawing the bound from the RNG) on
            // EVERY iteration, so the mirror samples fresh each pass.
            let mut i = 0i32;
            loop {
                if i >= self.count.sample(random) {
                    break;
                }
                // `random.nextInt(16) + origin.getX()` — Java int addition wraps.
                let x = random.next_int_bound(16).wrapping_add(origin.get_x());
                let z = random.next_int_bound(16).wrapping_add(origin.get_z());
                // `PlacementContext.getHeight(Heightmap.Types.MOTION_BLOCKING,
                // x, z)` — the `#228`-deferred height read fails explicitly.
                let start_y = context.get_height(Types::MotionBlocking, x, z);
                let y = find_on_ground_y_position(context, x, start_y, z, layer);
                if y != i32::MAX {
                    positions.push(BlockPos::new(x, y, z));
                    found_any = true;
                }
                // `i++` — Java int increment wraps.
                i = i.wrapping_add(1);
            }

            layer = layer.wrapping_add(1);
            if !found_any {
                break;
            }
        }

        Box::new(positions.into_iter())
    }

    fn type_id(
        &self,
    ) -> crate::levelgen::placement::placement_modifier_type::PlacementModifierTypeId {
        // `PlacementModifierType.COUNT_ON_EVERY_LAYER` is insertion index 8 in
        // `PlacementModifierType.java`'s registration order.
        PlacementModifierTypes::COUNT_ON_EVERY_LAYER
    }
}

/// `findOnGroundYPosition(PlacementContext, xStart, yStart, zStart,
/// layerToPlaceOn)` — walk down the column from `yStart` to `minY + 1`,
/// counting air-or-fluid gaps above a non-bedrock solid block; return the Y of
/// the block above the `layerToPlaceOn`-th such gap, or `Integer.MAX_VALUE`.
fn find_on_ground_y_position(
    context: &PlacementContext,
    x_start: i32,
    y_start: i32,
    z_start: i32,
    layer_to_place_on: i32,
) -> i32 {
    let mut current_pos = MutableBlockPos::new(x_start, y_start, z_start);
    let mut current_layer = 0i32;
    // `context.getBlockState(currentPos)` — `PlacementContext.getBlockState`
    // delegates to `this.level.getBlockState(pos)`; the `#399` block-state seam
    // fails explicitly unless the level provides it.
    let mut current_block = context
        .get_level()
        .get_block_state(&current_pos.immutable());

    // `for (int y = yStart; y >= context.getMinY() + 1; y--)`.
    let mut y = y_start;
    let min_y_plus_one = context.get_min_y().wrapping_add(1);
    while y >= min_y_plus_one {
        // `currentPos.setY(y - 1)`.
        current_pos.set_y(y.wrapping_sub(1));
        let below_block = context
            .get_level()
            .get_block_state(&current_pos.immutable());
        if !is_empty(&below_block) && is_empty(&current_block) && below_block.block() != BEDROCK {
            if current_layer == layer_to_place_on {
                // `currentPos.getY() + 1` — after `setY(y - 1)`, this is `y`.
                return current_pos.get_y().wrapping_add(1);
            }
            current_layer = current_layer.wrapping_add(1);
        }
        current_block = below_block;
        y = y.wrapping_sub(1);
    }

    i32::MAX
}

/// `Blocks.BEDROCK`, `Blocks.WATER`, `Blocks.LAVA` — `minecraft:bedrock` (34),
/// `minecraft:water` (35), `minecraft:lava` (36) as generated registry ids. The
/// generated registry exposes no compile-time per-block id constant, so the ids
/// are inlined here and pinned to the registry by the
/// `*_matches_the_generated_registry` tests — a future regeneration that
/// renumbers a block fails loudly in those tests rather than silently changing
/// behavior.
const BEDROCK: BlockId = BlockId::from_id(34);
const WATER: BlockId = BlockId::from_id(35);
const LAVA: BlockId = BlockId::from_id(36);

/// `isEmpty(BlockState)` — `blockState.isAir() || blockState.is(Blocks.WATER)
/// || blockState.is(Blocks.LAVA)`.
fn is_empty(block_state: &BlockState) -> bool {
    block_state.is_air() || block_state.block() == WATER || block_state.block() == LAVA
}

/// `CountOnEveryLayerPlacement.CODEC` — `IntProviders.codec(0, 256).fieldOf(
/// "count")`, as the ops-generic
/// `count_on_every_layer_placement_map_codec::<Ops>()` factory.
pub fn count_on_every_layer_placement_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<CountOnEveryLayerPlacement, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|c: &CountOnEveryLayerPlacement| c.count.clone()),
                "count".to_string(),
                int_provider_codec_with_bounds::<Ops>(0, 256),
            ))
            .apply(
                instance,
                Arc::new(|count: IntProvider| CountOnEveryLayerPlacement::of(count)),
            )
    })
}

/// `CountOnEveryLayerPlacement.CODEC` as a `Codec` (`MapCodec.codec()`), the
/// shape the `#181` generated dispatch's registration table consumes.
#[allow(dead_code)]
pub fn count_on_every_layer_placement_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<CountOnEveryLayerPlacement, Ops>> {
    map_codec::codec_of(count_on_every_layer_placement_map_codec::<Ops>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use crate::levelgen::placement::PlacementModifier;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::json;

    /// A minimal `WorldGenLevel` double over the overworld window.
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

        fn get_block_state(&self, _pos: &BlockPos) -> BlockState {
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }

    struct NoopGenerator;

    impl crate::chunk::ChunkGenerator for NoopGenerator {
        fn create_biomes(&self) {}
        fn apply_carvers(&self) {}
        fn build_surface(&self) {}
        fn spawn_original_mobs(&self) {}
        fn fill_from_noise(&self) {}
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            384
        }
    }

    #[test]
    fn layer_scan_fails_explicitly_at_the_world_access_seam() {
        // `getPositions` reads the MOTION_BLOCKING heightmap through
        // `PlacementContext.getHeight` (the `#228` seam) on the first sample —
        // it fails explicitly rather than fabricating a surface, exactly as the
        // sibling placement-filter unit documents.
        let modifier = CountOnEveryLayerPlacement::of_int(1);
        let mut level = TestLevel(create(-64, 384));
        let generator = NoopGenerator;
        let context = PlacementContext::new(&mut level, &generator, None);
        let mut random = LegacyRandomSource::new(0);
        let origin = BlockPos::new(0, 0, 0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            modifier.get_positions(&context, &mut random, &origin)
        }));
        assert!(
            result.is_err(),
            "getHeight is not implemented (RivetTodo #228)"
        );
    }

    #[test]
    fn empty_checks_air_water_and_lava() {
        // `isEmpty(BlockState)` — `isAir() || is(Blocks.WATER) || is(Blocks.LAVA)`.
        let air = BlockState::of(BlockId::from_name("minecraft:air").unwrap());
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let water = BlockState::of(WATER);
        let lava = BlockState::of(LAVA);
        let bedrock = BlockState::of(BEDROCK);
        assert!(is_empty(&air));
        assert!(is_empty(&water));
        assert!(is_empty(&lava));
        assert!(!is_empty(&stone));
        assert!(!is_empty(&bedrock));
    }

    #[test]
    fn bedrock_matches_the_generated_registry() {
        // Pins the inlined `BEDROCK` id to the generated `minecraft:block`
        // registry, so a future regeneration that renumbers bedrock breaks
        // loudly instead of silently changing `findOnGroundYPosition`.
        assert_eq!(
            BEDROCK,
            BlockId::from_name("minecraft:bedrock").expect("bedrock in the registry")
        );
    }

    #[test]
    fn water_matches_the_generated_registry() {
        assert_eq!(
            WATER,
            BlockId::from_name("minecraft:water").expect("water in the registry")
        );
    }

    #[test]
    fn lava_matches_the_generated_registry() {
        assert_eq!(
            LAVA,
            BlockId::from_name("minecraft:lava").expect("lava in the registry")
        );
    }

    #[test]
    fn count_on_every_layer_type_identity_is_reported() {
        // `PlacementModifierType.COUNT_ON_EVERY_LAYER` is insertion index 8.
        let modifier = CountOnEveryLayerPlacement::of_int(1);
        assert_eq!(
            modifier.type_id(),
            PlacementModifierTypes::COUNT_ON_EVERY_LAYER
        );
    }

    #[test]
    fn codec_round_trips_the_count() {
        let ops = JsonOps::INSTANCE;
        let codec = count_on_every_layer_placement_codec::<JsonOps>();
        let modifier = CountOnEveryLayerPlacement::of_int(4);
        let encoded = codec
            .encode_start(&ops, &modifier)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"count": 4}));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, modifier);
    }

    #[test]
    fn codec_rejects_a_count_out_of_bounds() {
        // `IntProviders.codec(0, 256)` — a constant 300 is too high.
        let ops = JsonOps::INSTANCE;
        let codec = count_on_every_layer_placement_codec::<JsonOps>();
        let result = codec.parse(&ops, &json!({"count": 300}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.contains("Value provider too high: 256"), "got: {msg}");
    }
}
