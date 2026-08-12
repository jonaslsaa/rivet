//! Port of `net.minecraft.world.level.levelgen.feature.configurations.MultifaceGrowthConfiguration`
//! (class, 26.2).
//!
//! Java: a seven-field value class (`Block placeBlock`, `int searchRange`,
//! `boolean canPlaceOnFloor/CanPlaceOnCeiling/CanPlaceOnWall`, `float
//! chanceOfSpreading`, `HolderSet<Block> canBePlacedOn`) whose `CODEC` is a
//! `RecordCodecBuilder` over the `"block"` field
//! (`BuiltInRegistries.BLOCK.byNameCodec().validate(MultifaceGrowthConfiguration::validateBlock)`
//! — required), the `"search_range"` field (`Codec.intRange(1, 64)
//! optionalFieldOf(name, 10)`), the `"can_place_on_floor"`/`"can_place_on_ceiling"`/
//! `"can_place_on_wall"` fields (`Codec.BOOL.optionalFieldOf(name, false)`),
//! the `"chance_of_spreading"` field (`Codec.floatRange(0.0F, 1.0F)
//! optionalFieldOf(name, 0.5F)`), and the required `"can_be_placed_on"` field
//! (`RegistryCodecs.homogeneousList(Registries.BLOCK)`). The constructor builds
//! the derived `validDirections` list (ceiling → `Direction.UP`, floor →
//! `Direction.DOWN`, wall → the four `Direction.Plane.HORIZONTAL` faces), and
//! `getShuffledDirections`/`getShuffledDirectionsExcept` shuffle it with
//! `Util.shuffledCopy`/`Util.toShuffledList`.
//!
//! The seven-field group exceeds the port's `record_builder` `Group6` cap, so
//! the record codec is hand-composed with `map_encoder`/`map_decoder` exactly
//! mirroring `Applicative.super.ap7` (`ap4(ap3(...curry3..., t1, t2, t3), t4,
//! t5, t6, t7)`). `validateBlock` checks `block instanceof
//! MultifaceSpreadeableBlock`; the marker class belongs to the block package and
//! is STUB'd here as the known multiface-spreadeable block set. DFU `Codec<T>`
//! is `Codec<E, Ops>` in the port, so the static Java constant is exposed as
//! the ops-generic `multiface_growth_configuration_codec::<Ops>()` factory.

use crate::block::Block;
use crate::chunk::registry_codecs::block_by_name_codec;
use rivet_registry::core::{Direction, Plane};
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::{self, DataResult};
use rivet_serialization::dynamic_ops::{DynamicOps, MapLike, RecordBuilder};
use rivet_serialization::functions::{Fn3, Fn4};
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::map_codec;
use rivet_serialization::map_decoder;
use rivet_serialization::map_encoder;
use rivet_util::RandomSource;
use rivet_util::util::shuffled_copy;
use std::sync::Arc;

/// `net.minecraft.world.level.block.MultifaceSpreadeableBlock` — the marker
/// class for blocks that grow across surfaces.
///
/// STUB(mc.world.level.levelgen.feature.configurations.wave2): the class is
/// owned by the block package (`MultifaceSpreadeableBlock`, `GlowLichenBlock`,
/// `SculkVeinBlock`) and defers with the block-port type hierarchy (RivetTodo
/// #228). `validateBlock` in this configuration checks
/// `instanceof MultifaceSpreadeableBlock`; the port models the marker as the
/// set of blocks whose concrete class extends it (`glow_lichen`, `sculk_vein`),
/// matched by their stable registry NAMES (name-based matching is robust to
/// registry reordering, unlike a raw id set).
fn is_multiface_spreadeable(block: Block) -> bool {
    matches!(
        block.name(),
        "minecraft:glow_lichen" | "minecraft:sculk_vein"
    )
}

/// `MultifaceGrowthConfiguration.validateBlock(Block)` — `block instanceof
/// MultifaceSpreadeableBlock ? success : error("Growth block should be a
/// multiface spreadeable block")`.
fn validate_block(block: &Block) -> DataResult<Block> {
    if is_multiface_spreadeable(*block) {
        DataResult::success(*block)
    } else {
        DataResult::error("Growth block should be a multiface spreadeable block")
    }
}

/// `net.minecraft.world.level.levelgen.feature.configurations.MultifaceGrowthConfiguration`.
#[derive(Debug, Clone)]
pub struct MultifaceGrowthConfiguration {
    /// `placeBlock` — the block to grow.
    pub place_block: Block,
    /// `searchRange` — `[1, 64]`, default 10.
    pub search_range: i32,
    /// `canPlaceOnFloor` — whether the block may grow on floors.
    pub can_place_on_floor: bool,
    /// `canPlaceOnCeiling` — whether the block may grow on ceilings.
    pub can_place_on_ceiling: bool,
    /// `canPlaceOnWall` — whether the block may grow on walls.
    pub can_place_on_wall: bool,
    /// `chanceOfSpreading` — `[0.0F, 1.0F]`, default 0.5F.
    pub chance_of_spreading: f32,
    /// `canBePlacedOn` — the `HolderSet<Block>` blocks the growth may attach to.
    pub can_be_placed_on: HolderSet<BlockType>,
    /// `validDirections` — the derived direction list (the `ObjectArrayList<Direction>`
    /// built in the constructor).
    valid_directions: Vec<Direction>,
}

impl PartialEq for MultifaceGrowthConfiguration {
    fn eq(&self, other: &Self) -> bool {
        fn canonical_bits(value: f32) -> u32 {
            if value.is_nan() {
                f32::NAN.to_bits()
            } else {
                value.to_bits()
            }
        }
        self.place_block == other.place_block
            && self.search_range == other.search_range
            && self.can_place_on_floor == other.can_place_on_floor
            && self.can_place_on_ceiling == other.can_place_on_ceiling
            && self.can_place_on_wall == other.can_place_on_wall
            && canonical_bits(self.chance_of_spreading) == canonical_bits(other.chance_of_spreading)
            && self.can_be_placed_on == other.can_be_placed_on
    }
}

impl Eq for MultifaceGrowthConfiguration {}

impl MultifaceGrowthConfiguration {
    /// `new MultifaceGrowthConfiguration(Block, int, boolean, boolean,
    /// boolean, float, HolderSet<Block>)` — the constructor (the codec's
    /// `apply` function); derives `validDirections`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        place_block: Block,
        search_range: i32,
        can_place_on_floor: bool,
        can_place_on_ceiling: bool,
        can_place_on_wall: bool,
        chance_of_spreading: f32,
        can_be_placed_on: HolderSet<BlockType>,
    ) -> Self {
        let mut valid_directions = Vec::with_capacity(6);
        if can_place_on_ceiling {
            valid_directions.push(Direction::Up);
        }
        if can_place_on_floor {
            valid_directions.push(Direction::Down);
        }
        if can_place_on_wall {
            valid_directions.extend_from_slice(Plane::Horizontal.faces());
        }
        MultifaceGrowthConfiguration {
            place_block,
            search_range,
            can_place_on_floor,
            can_place_on_ceiling,
            can_place_on_wall,
            chance_of_spreading,
            can_be_placed_on,
            valid_directions,
        }
    }

    /// `getShuffledDirectionsExcept(RandomSource, Direction)` —
    /// `Util.toShuffledList(this.validDirections.stream().filter(direction ->
    /// direction != excludeDirection), random)`.
    pub fn get_shuffled_directions_except(
        &self,
        random: &mut impl RandomSource,
        exclude_direction: Direction,
    ) -> Vec<Direction> {
        let filtered: Vec<Direction> = self
            .valid_directions
            .iter()
            .copied()
            .filter(|direction| *direction != exclude_direction)
            .collect();
        shuffled_copy(&filtered, random)
    }

    /// `getShuffledDirections(RandomSource)` — `Util.shuffledCopy(this.validDirections, random)`.
    pub fn get_shuffled_directions(&self, random: &mut impl RandomSource) -> Vec<Direction> {
        shuffled_copy(&self.valid_directions, random)
    }

    /// `placeBlock()`.
    pub fn place_block(&self) -> Block {
        self.place_block
    }

    /// `searchRange()`.
    pub fn search_range(&self) -> i32 {
        self.search_range
    }

    /// `canPlaceOnFloor()`.
    pub fn can_place_on_floor(&self) -> bool {
        self.can_place_on_floor
    }

    /// `canPlaceOnCeiling()`.
    pub fn can_place_on_ceiling(&self) -> bool {
        self.can_place_on_ceiling
    }

    /// `canPlaceOnWall()`.
    pub fn can_place_on_wall(&self) -> bool {
        self.can_place_on_wall
    }

    /// `chanceOfSpreading()`.
    pub fn chance_of_spreading(&self) -> f32 {
        self.chance_of_spreading
    }

    /// `canBePlacedOn()`.
    pub fn can_be_placed_on(&self) -> &HolderSet<BlockType> {
        &self.can_be_placed_on
    }
}

/// `RegistryCodecs.homogeneousList(Registries.BLOCK)` — the `"can_be_placed_on"`
/// field codec.
fn can_be_placed_on_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn map_codec::MapCodec<HolderSet<BlockType>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<rivet_registry::holder::Holder<BlockType>, Ops>> = Arc::new(
        rivet_registry::registry_file_codec::RegistryFixedCodec::create(
            &rivet_registry::registries::BLOCK,
        ),
    );
    #[allow(clippy::arc_with_non_send_sync)]
    let holder_set: Arc<dyn Codec<HolderSet<BlockType>, Ops>> =
        Arc::new(rivet_registry::registry_file_codec::HolderSetCodec::create(
            &rivet_registry::registries::BLOCK,
            element,
            false,
        ));
    codec::field_of(holder_set, "can_be_placed_on".to_string())
}

/// `MultifaceGrowthConfiguration.CODEC` — the ops-generic
/// `multiface_growth_configuration_codec::<Ops>()` factory (record codec over
/// the seven fields). The decode side mirrors `Applicative.super.ap7`:
/// `ap4(ap3(map(Function7.curry3, func), t1, t2, t3), t4, t5, t6, t7)`.
#[allow(clippy::type_complexity)]
pub fn multiface_growth_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<MultifaceGrowthConfiguration, Ops>> {
    let block_codec = codec::field_of(
        codec::validate(
            block_by_name_codec::<Ops>(),
            Arc::new(|b: &Block| validate_block(b)),
        ),
        "block".to_string(),
    );
    let search_range_codec =
        codec::optional_field_of::<i32, Ops>("search_range", codec::int_range::<Ops>(1, 64), 10);
    let can_place_on_floor_codec = codec::optional_field_of::<bool, Ops>(
        "can_place_on_floor",
        codec::bool_codec::<Ops>(),
        false,
    );
    let can_place_on_ceiling_codec = codec::optional_field_of::<bool, Ops>(
        "can_place_on_ceiling",
        codec::bool_codec::<Ops>(),
        false,
    );
    let can_place_on_wall_codec = codec::optional_field_of::<bool, Ops>(
        "can_place_on_wall",
        codec::bool_codec::<Ops>(),
        false,
    );
    let chance_of_spreading_codec = codec::optional_field_of::<f32, Ops>(
        "chance_of_spreading",
        codec::float_range::<Ops>(0.0, 1.0),
        0.5,
    );
    let can_be_placed_on_codec = can_be_placed_on_field_codec::<Ops>();

    // The encode closure moves the field codecs (each `MapCodec` `Arc` is used
    // by both the encode and decode closures, so the encode side takes clones).
    let block_codec_enc = block_codec.clone();
    let search_range_codec_enc = search_range_codec.clone();
    let can_place_on_floor_codec_enc = can_place_on_floor_codec.clone();
    let can_place_on_ceiling_codec_enc = can_place_on_ceiling_codec.clone();
    let can_place_on_wall_codec_enc = can_place_on_wall_codec.clone();
    let chance_of_spreading_codec_enc = chance_of_spreading_codec.clone();
    let can_be_placed_on_codec_enc = can_be_placed_on_codec.clone();
    let encode = map_encoder::of(
        Arc::new(
            move |c: &MultifaceGrowthConfiguration,
                  ops: &Ops,
                  prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                block_codec_enc.encode(&c.place_block, ops, prefix);
                search_range_codec_enc.encode(&c.search_range, ops, prefix);
                can_place_on_floor_codec_enc.encode(&c.can_place_on_floor, ops, prefix);
                can_place_on_ceiling_codec_enc.encode(&c.can_place_on_ceiling, ops, prefix);
                can_place_on_wall_codec_enc.encode(&c.can_place_on_wall, ops, prefix);
                chance_of_spreading_codec_enc.encode(&c.chance_of_spreading, ops, prefix);
                can_be_placed_on_codec_enc.encode(&c.can_be_placed_on, ops, prefix);
            },
        ),
        // Mirror the decode keys: Java's RecordCodecBuilder encoder (`Keyable`)
        // exposes all seven field keys, which is what a compressed-map
        // `KeyCompressor` is built from.
        Arc::new(move |ops: &Ops| -> Vec<Ops::Output> {
            vec![
                ops.create_string("block".to_string()),
                ops.create_string("search_range".to_string()),
                ops.create_string("can_place_on_floor".to_string()),
                ops.create_string("can_place_on_ceiling".to_string()),
                ops.create_string("can_place_on_wall".to_string()),
                ops.create_string("chance_of_spreading".to_string()),
                ops.create_string("can_be_placed_on".to_string()),
            ]
        }),
    );

    // `Applicative.super.ap7`: `ap4(ap3(curry3, t1, t2, t3), t4, t5, t6, t7)`.
    let decode = map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let fr: DataResult<
                Fn3<
                    Block,
                    i32,
                    bool,
                    Fn4<bool, bool, f32, HolderSet<BlockType>, MultifaceGrowthConfiguration>,
                >,
            > = DataResult::success_with_lifecycle(
                Arc::new(move |b: &Block, sr: &i32, floor: &bool| {
                    let b = *b;
                    let sr = *sr;
                    let floor = *floor;
                    let inner: Fn4<
                        bool,
                        bool,
                        f32,
                        HolderSet<BlockType>,
                        MultifaceGrowthConfiguration,
                    > = Arc::new(
                        move |ceiling: &bool,
                              wall: &bool,
                              chance: &f32,
                              placed_on: &HolderSet<BlockType>| {
                            MultifaceGrowthConfiguration::new(
                                b,
                                sr,
                                floor,
                                *ceiling,
                                *wall,
                                *chance,
                                placed_on.clone(),
                            )
                        },
                    );
                    inner
                }),
                Lifecycle::experimental(),
            );
            let step1 = data_result::ap3(
                fr,
                block_codec.decode(ops, input),
                search_range_codec.decode(ops, input),
                can_place_on_floor_codec.decode(ops, input),
            );
            data_result::ap4(
                step1,
                can_place_on_ceiling_codec.decode(ops, input),
                can_place_on_wall_codec.decode(ops, input),
                chance_of_spreading_codec.decode(ops, input),
                can_be_placed_on_codec.decode(ops, input),
            )
        }),
        Arc::new(move |ops: &Ops| -> Vec<Ops::Output> {
            vec![
                ops.create_string("block".to_string()),
                ops.create_string("search_range".to_string()),
                ops.create_string("can_place_on_floor".to_string()),
                ops.create_string("can_place_on_ceiling".to_string()),
                ops.create_string("can_place_on_wall".to_string()),
                ops.create_string("chance_of_spreading".to_string()),
                ops.create_string("can_be_placed_on".to_string()),
            ]
        }),
    );

    map_codec::codec_of(map_codec::of(
        encode,
        decode.clone(),
        format!("RecordCodec[{:?}]", decode),
    ))
}

impl crate::levelgen::feature::configurations::FeatureConfiguration
    for MultifaceGrowthConfiguration
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::Identifier;
    use rivet_registry::ResourceKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::holder::Holder;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::root::AnyBox;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A block registry with `glow_lichen` and `stone`, wrapped in a
    /// `RegistryAccess` under `Registries.BLOCK`.
    fn block_access() -> RegistryAccess {
        let mut builder = RegistryBuilder::new(&*rivet_registry::registries::BLOCK);
        builder.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:glow_lichen"),
            ),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:stone"),
            ),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("block")),
            Box::new(registry) as AnyBox,
        )])
    }

    fn ops(access: &RegistryAccess) -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone())
    }

    fn config(access: &RegistryAccess) -> MultifaceGrowthConfiguration {
        let registry = rivet_registry::access::RegistryAccess::lookup(
            access,
            &*rivet_registry::registries::BLOCK,
        )
        .expect("block registry");
        MultifaceGrowthConfiguration::new(
            Block::from_name("minecraft:glow_lichen").unwrap(),
            10,
            true,
            false,
            true,
            0.5,
            HolderSet::direct(vec![Holder::reference(registry.registry_id(), 0)]),
        )
    }

    #[test]
    fn constructor_derives_valid_directions() {
        // ceiling → UP, floor → DOWN, wall → HORIZONTAL (N, E, S, W).
        let floor = MultifaceGrowthConfiguration::new(
            Block::from_name("minecraft:glow_lichen").unwrap(),
            10,
            true,
            false,
            false,
            0.5,
            HolderSet::direct(vec![]),
        );
        let floor_dirs = floor.get_shuffled_directions(&mut LegacyRandomSource::new(0));
        assert_eq!(floor_dirs, vec![Direction::Down]);

        let ceiling = MultifaceGrowthConfiguration::new(
            Block::from_name("minecraft:glow_lichen").unwrap(),
            10,
            false,
            true,
            false,
            0.5,
            HolderSet::direct(vec![]),
        );
        let ceiling_dirs = ceiling.get_shuffled_directions(&mut LegacyRandomSource::new(0));
        assert_eq!(ceiling_dirs, vec![Direction::Up]);

        let wall = MultifaceGrowthConfiguration::new(
            Block::from_name("minecraft:glow_lichen").unwrap(),
            10,
            false,
            false,
            true,
            0.5,
            HolderSet::direct(vec![]),
        );
        // `getShuffledDirections` shuffles (`Util.shuffledCopy`), so assert the
        // derived set, not the shuffled order.
        let wall_dirs = wall.get_shuffled_directions(&mut LegacyRandomSource::new(0));
        assert_eq!(wall_dirs.len(), 4);
        for direction in Plane::Horizontal.faces() {
            assert!(wall_dirs.contains(direction));
        }

        let all = MultifaceGrowthConfiguration::new(
            Block::from_name("minecraft:glow_lichen").unwrap(),
            10,
            true,
            true,
            true,
            0.5,
            HolderSet::direct(vec![]),
        );
        let all_dirs = all.get_shuffled_directions(&mut LegacyRandomSource::new(0));
        assert_eq!(all_dirs.len(), 6);
        let mut expected: Vec<Direction> = vec![Direction::Up, Direction::Down];
        expected.extend_from_slice(Plane::Horizontal.faces());
        for direction in &expected {
            assert!(all_dirs.contains(direction));
        }
    }

    #[test]
    fn get_shuffled_directions_except_filters() {
        let all = MultifaceGrowthConfiguration::new(
            Block::from_name("minecraft:glow_lichen").unwrap(),
            10,
            true,
            true,
            true,
            0.5,
            HolderSet::direct(vec![]),
        );
        let dirs =
            all.get_shuffled_directions_except(&mut LegacyRandomSource::new(7), Direction::Up);
        assert!(!dirs.contains(&Direction::Up));
        assert_eq!(dirs.len(), 5);
    }

    #[test]
    fn validate_block_accepts_only_multiface_blocks() {
        assert!(validate_block(&Block::from_name("minecraft:glow_lichen").unwrap()).is_success());
        assert!(validate_block(&Block::from_name("minecraft:sculk_vein").unwrap()).is_success());
        assert!(validate_block(&Block::from_name("minecraft:stone").unwrap()).is_error());
    }

    #[test]
    fn codec_round_trip() {
        let access = block_access();
        let codec = multiface_growth_configuration_codec::<TestOps>();
        let ops = ops(&access);
        let config = config(&access);
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "block": "minecraft:glow_lichen",
                "can_place_on_floor": true,
                "can_place_on_wall": true,
                // A one-element homogeneous list compacts to a bare value
                // (`alwaysUseList=false`).
                "can_be_placed_on": "minecraft:glow_lichen",
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_defaults_absent_optional_fields() {
        let access = block_access();
        let codec = multiface_growth_configuration_codec::<TestOps>();
        let ops = ops(&access);
        let decoded = codec
            .parse(
                &ops,
                &json!({"block": "minecraft:glow_lichen", "can_be_placed_on": []}),
            )
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.search_range(), 10);
        assert_eq!(decoded.chance_of_spreading(), 0.5);
        assert!(!decoded.can_place_on_floor());
        assert!(!decoded.can_place_on_ceiling());
        assert!(!decoded.can_place_on_wall());
    }

    #[test]
    fn codec_requires_block_and_can_be_placed_on() {
        let access = block_access();
        let codec = multiface_growth_configuration_codec::<TestOps>();
        let ops = ops(&access);
        assert!(codec.parse(&ops, &json!({})).is_error());
        assert!(
            codec
                .parse(&ops, &json!({"block": "minecraft:glow_lichen"}))
                .is_error()
        );
    }

    #[test]
    fn codec_rejects_non_multiface_block() {
        let access = block_access();
        let codec = multiface_growth_configuration_codec::<TestOps>();
        let ops = ops(&access);
        let result = codec.parse(
            &ops,
            &json!({"block": "minecraft:stone", "can_be_placed_on": []}),
        );
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Growth block should be a multiface spreadeable block"),
            "got: {msg}"
        );
    }

    #[test]
    fn codec_round_trip_with_defaulted_optionals() {
        // Paper's default-shaped config: only `block` and `can_be_placed_on`
        // are non-default; the optional fields default (10 / false / false /
        // false / 0.5) and are omitted on encode.
        let access = block_access();
        let codec = multiface_growth_configuration_codec::<TestOps>();
        let ops = ops(&access);
        let config = MultifaceGrowthConfiguration::new(
            Block::from_name("minecraft:glow_lichen").unwrap(),
            10,
            false,
            false,
            false,
            0.5,
            HolderSet::direct(vec![Holder::reference(
                rivet_registry::access::RegistryAccess::lookup(
                    &access,
                    &*rivet_registry::registries::BLOCK,
                )
                .expect("block registry")
                .registry_id(),
                0,
            )]),
        );
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"block": "minecraft:glow_lichen", "can_be_placed_on": "minecraft:glow_lichen"})
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_rejects_out_of_range_search_and_chance() {
        let access = block_access();
        let codec = multiface_growth_configuration_codec::<TestOps>();
        let ops = ops(&access);
        // search_range above [1, 64].
        let high = json!({
            "block": "minecraft:glow_lichen",
            "search_range": 65,
            "can_be_placed_on": [],
        });
        let result = codec.parse(&ops, &high);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "Value 65 outside of range [1:64]");
        // chance_of_spreading above [0.0, 1.0].
        let chance = json!({
            "block": "minecraft:glow_lichen",
            "chance_of_spreading": 1.5,
            "can_be_placed_on": [],
        });
        assert!(codec.parse(&ops, &chance).is_error());
    }

    #[test]
    fn codec_rejects_present_malformed_optional() {
        // NON-lenient optional: a present-but-wrong-typed `"search_range"` is a
        // decode error (not silently defaulted to 10).
        let access = block_access();
        let codec = multiface_growth_configuration_codec::<TestOps>();
        let ops = ops(&access);
        let result = codec.parse(
            &ops,
            &json!({"block": "minecraft:glow_lichen", "search_range": "many", "can_be_placed_on": []}),
        );
        assert!(result.is_error());
    }

    #[test]
    fn value_equality_semantics() {
        // Float.compare canonicalizes NaN payloads.
        let nan_a = f32::from_bits(0x7fc0_0001);
        let nan_b = f32::from_bits(0x7fc0_0002);
        let base = |chance: f32| -> MultifaceGrowthConfiguration {
            MultifaceGrowthConfiguration::new(
                Block::from_name("minecraft:glow_lichen").unwrap(),
                10,
                true,
                true,
                true,
                chance,
                HolderSet::direct(vec![]),
            )
        };
        assert_eq!(base(nan_a), base(nan_b));
        // `Float.compare(-0.0F, 0.0F) != 0`.
        assert_ne!(base(-0.0), base(0.0));
    }
}
