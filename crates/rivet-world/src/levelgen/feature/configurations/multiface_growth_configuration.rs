//! Port of `net.minecraft.world.level.levelgen.feature.configurations.MultifaceGrowthConfiguration`
//! (class, 26.2).
//!
//! Java: a seven-field class (`Block placeBlock, int searchRange, boolean
//! canPlaceOnFloor, boolean canPlaceOnCeiling, boolean canPlaceOnWall, float
//! chanceOfSpreading, HolderSet<Block> canBePlacedOn`) whose `CODEC` is a
//! `RecordCodecBuilder` over the required `"block"` field
//! (`BuiltInRegistries.BLOCK.byNameCodec().validate(validateBlock)` — the
//! by-name block codec validated to multiface-spreadeable blocks), the
//! `"search_range"` field (`Codec.intRange(1, 64).optionalFieldOf(..., 10)`),
//! the `"can_place_on_floor"`/`"can_place_on_ceiling"`/`"can_place_on_wall"`
//! fields (`Codec.BOOL.optionalFieldOf(..., false)` each), the
//! `"chance_of_spreading"` field (`Codec.floatRange(0.0F, 1.0F).optionalFieldOf(..., 0.5F)`),
//! and the required `"can_be_placed_on"` field (`RegistryCodecs.homogeneousList(
//! Registries.BLOCK)`). The constructor derives `validDirections` (ceiling →
//! `UP`, floor → `DOWN`, wall → the horizontal plane faces in
//! `Direction.Plane.HORIZONTAL.forEach` order `NORTH, EAST, SOUTH, WEST`),
//! exposed through `getShuffledDirectionsExcept(random, exclude)` (filter then
//! `Util.toShuffledList`) and `getShuffledDirections(random)` (`Util.shuffledCopy`).
//!
//! The seven-field group exceeds the port's `record_builder` `Group6` cap, so
//! the record codec is hand-composed with `map_encoder`/`map_decoder` exactly
//! mirroring `Applicative.super.ap7` (`ap4(ap3(...curry3..., t1, t2, t3), t4,
//! t5, t6, t7)`), following `sculk_patch_configuration.rs`. Equality mirrors
//! `Float.compare` for `chanceOfSpreading` (canonical NaN, `-0.0` distinct
//! from `0.0`).
//!
//! ## `placeBlock` carrier and the `validateBlock` check
//!
//! Java's `placeBlock` is a `Block` — the id-handle [`BlockId`] here (the
//! `BlockType` placeholder has no by-name codec). [`block_by_name_codec`] is
//! the `BuiltInRegistries.BLOCK.byNameCodec()` surface: a namespaced-name
//! string codec with the vanilla `Unknown registry key ... minecraft:block`
//! error, wrapped in `.validate(MultifaceGrowthConfiguration::validateBlock)`
//! — `block instanceof MultifaceSpreadeableBlock ? success : error("Growth
//! block should be a multiface spreadeable block")`. The
//! `MultifaceSpreadeableBlock` abstract class (block behavior, owned by the
//! `net.minecraft.world.level.block` unit) is not ported; the validation is
//! narrowed to its concrete subclasses in Paper, matching Java's `instanceof`
//! exactly: `minecraft:glow_lichen` (`GlowLichenBlock`) and
//! `minecraft:sculk_vein` (`SculkVeinBlock`) are the only two blocks that are
//! `MultifaceSpreadeableBlock` in Paper (see the [validate checker]). The
//! `canBePlacedOn` half is the value-semantic `HolderSet<BlockType>`.

use crate::levelgen::feature::configurations::FeatureConfiguration;
use rivet_registry::core::Direction;
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::holder::Holder;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::{self, DataResult};
use rivet_serialization::dynamic_ops::{DynamicOps, MapLike, RecordBuilder};
use rivet_serialization::functions::{Fn3, Fn4};
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::map_codec;
use rivet_serialization::map_decoder::{self, MapDecoder};
use rivet_serialization::map_encoder::{self, MapEncoder};
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.MultifaceGrowthConfiguration`.
#[derive(Debug, Clone)]
pub struct MultifaceGrowthConfiguration {
    /// `placeBlock` — the multiface-spreadeable block to place (id-handle).
    pub place_block: BlockId,
    /// `searchRange` — `[1, 64]`.
    pub search_range: i32,
    /// `canPlaceOnFloor`.
    pub can_place_on_floor: bool,
    /// `canPlaceOnCeiling`.
    pub can_place_on_ceiling: bool,
    /// `canPlaceOnWall`.
    pub can_place_on_wall: bool,
    /// `chanceOfSpreading` — `[0.0F, 1.0F]`.
    pub chance_of_spreading: f32,
    /// `canBePlacedOn` — the blocks the multiface may be placed on.
    pub can_be_placed_on: HolderSet<BlockType>,
    /// `validDirections` — derived in the constructor: ceiling → `UP`, floor →
    /// `DOWN`, wall → the horizontal plane faces (`NORTH, EAST, SOUTH, WEST`).
    valid_directions: Vec<Direction>,
}

impl PartialEq for MultifaceGrowthConfiguration {
    fn eq(&self, other: &Self) -> bool {
        // `Float.compare`: NaN payloads canonicalize, signed zero keeps its sign.
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
            // `validDirections` is a pure function of the three booleans, so it
            // is redundant in equality — but derived equality would include it;
            // it is always equal when the booleans are.
            && self.valid_directions == other.valid_directions
    }
}

impl Eq for MultifaceGrowthConfiguration {}

impl MultifaceGrowthConfiguration {
    /// `new MultifaceGrowthConfiguration(Block, int, boolean, boolean, boolean,
    /// float, HolderSet<Block>)` — the constructor (the codec's `apply`
    /// function); derives `validDirections` exactly like Java.
    pub fn new(
        place_block: BlockId,
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
            // `Direction.Plane.HORIZONTAL.forEach` — `NORTH, EAST, SOUTH, WEST`.
            valid_directions.extend_from_slice(&[
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West,
            ]);
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
    /// `Util.toShuffledList(validDirections.stream().filter(direction !=
    /// excludeDirection), random)`.
    pub fn get_shuffled_directions_except(
        &self,
        random: &mut impl RandomSource,
        exclude_direction: Direction,
    ) -> Vec<Direction> {
        let filtered: Vec<Direction> = self
            .valid_directions
            .iter()
            .copied()
            .filter(|d| *d != exclude_direction)
            .collect();
        rivet_util::util::shuffled_copy(&filtered, random)
    }

    /// `getShuffledDirections(RandomSource)` — `Util.shuffledCopy(validDirections,
    /// random)`.
    pub fn get_shuffled_directions(&self, random: &mut impl RandomSource) -> Vec<Direction> {
        rivet_util::util::shuffled_copy(&self.valid_directions, random)
    }
}

/// `BuiltInRegistries.BLOCK.byNameCodec().validate(validateBlock)` — the
/// `"block"` field element codec: a namespaced-name string codec over the
/// generated block ids, validated to multiface-spreadeable blocks.
///
/// Java (`MultifaceGrowthConfiguration.java`):
/// ```java
/// BuiltInRegistries.BLOCK.byNameCodec().validate(MultifaceGrowthConfiguration::validateBlock)
/// ```
/// where `validateBlock` is `block instanceof MultifaceSpreadeableBlock ?
/// success : error("Growth block should be a multiface spreadeable block")`.
/// The abstract class is not ported, so [`validate_block`] narrows the check to
/// Paper's concrete multiface-spreadeable blocks (see the module doc).
fn block_by_name_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BlockId, Ops>> {
    let by_name = codec::comap_flat_map::<rivet_registry::Identifier, BlockId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(
            |name: &rivet_registry::Identifier| match BlockId::from_name(&name.to_string()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:block]: {}",
                    name
                )),
            },
        ),
        Arc::new(|id: &BlockId| rivet_registry::Identifier::parse(id.name())),
    );
    codec::validate(by_name, Arc::new(validate_block))
}

/// `MultifaceGrowthConfiguration.validateBlock(Block)` — `block instanceof
/// MultifaceSpreadeableBlock ? success : error("Growth block should be a
/// multiface spreadeable block")`.
///
/// Paper's `MultifaceSpreadeableBlock` abstract class (block behavior) is not
/// ported; its concrete subclasses are `GlowLichenBlock` and
/// `SculkVeinBlock`, so the `instanceof` check is narrowed to those two
/// generated block ids (`minecraft:glow_lichen`, `minecraft:sculk_vein`).
fn validate_block(block: &BlockId) -> DataResult<BlockId> {
    if is_multiface_spreadeable(*block) {
        DataResult::success(*block)
    } else {
        DataResult::error("Growth block should be a multiface spreadeable block")
    }
}

/// Whether the block is a `MultifaceSpreadeableBlock` — the id allowlist
/// mirroring Paper's concrete subclasses (`GlowLichenBlock`,
/// `SculkVeinBlock`).
fn is_multiface_spreadeable(block: BlockId) -> bool {
    matches!(
        block.name(),
        "minecraft:glow_lichen" | "minecraft:sculk_vein"
    )
}

/// `RegistryCodecs.homogeneousList(Registries.BLOCK)` — the `"can_be_placed_on"`
/// field codec: a `HolderSetCodec` over the block registry, whose element codec
/// is a `RegistryFixedCodec`. The concrete codec is not `Send + Sync` (its
/// `RegistryOps` carries the single-threaded `HolderLookupAdapter` `RefCell`
/// memo), so the `Arc` is held by the ops-parameterized codec and never crosses
/// threads.
fn blocks_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn rivet_serialization::map_codec::MapCodec<HolderSet<BlockType>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<Holder<BlockType>, Ops>> = Arc::new(
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
/// `multiface_growth_configuration_codec::<Ops>()` factory (a record codec over
/// the seven fields). The seven-field group exceeds `record_builder`'s `Group6`
/// cap, so the codec is hand-composed with `map_encoder`/`map_decoder` and the
/// decode side mirrors `Applicative.super.ap7`:
/// `ap4(ap3(map(Function7.curry3, func), t1, t2, t3), t4, t5, t6, t7)`.
///
/// The five optional-with-default fields (`search_range`, the three booleans,
/// `chance_of_spreading`) are `optional_field_of` `MapCodec`s, whose own encode
/// already omits a value equal to the default and whose decode applies the
/// default when absent (and errors on a present-but-malformed value). Each
/// field `MapCodec` is wrapped in its encoder/decoder half for the composed
/// encode/decode, so all seven fields decode through the applicative chain —
/// faithful to Java's all-fields-decoded error accumulation.
pub fn multiface_growth_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<MultifaceGrowthConfiguration, Ops>> {
    use map_codec::{MapCodecDecoderHalf, MapCodecEncoderHalf};

    let block_field = codec::field_of(block_by_name_codec::<Ops>(), "block".to_string());
    let search_range_field =
        codec::optional_field_of::<i32, Ops>("search_range", codec::int_range::<Ops>(1, 64), 10);
    let can_place_on_floor_field = codec::optional_field_of::<bool, Ops>(
        "can_place_on_floor",
        codec::bool_codec::<Ops>(),
        false,
    );
    let can_place_on_ceiling_field = codec::optional_field_of::<bool, Ops>(
        "can_place_on_ceiling",
        codec::bool_codec::<Ops>(),
        false,
    );
    let can_place_on_wall_field = codec::optional_field_of::<bool, Ops>(
        "can_place_on_wall",
        codec::bool_codec::<Ops>(),
        false,
    );
    let chance_of_spreading_field = codec::optional_field_of::<f32, Ops>(
        "chance_of_spreading",
        codec::float_range::<Ops>(0.0, 1.0),
        0.5,
    );
    let can_be_placed_on_field = blocks_field_codec::<Ops>();

    let block_encoder = MapCodecEncoderHalf(block_field.clone());
    let block_decoder = MapCodecDecoderHalf(block_field);
    let search_range_encoder = MapCodecEncoderHalf(search_range_field.clone());
    let search_range_decoder = MapCodecDecoderHalf(search_range_field);
    let can_place_on_floor_encoder = MapCodecEncoderHalf(can_place_on_floor_field.clone());
    let can_place_on_floor_decoder = MapCodecDecoderHalf(can_place_on_floor_field);
    let can_place_on_ceiling_encoder = MapCodecEncoderHalf(can_place_on_ceiling_field.clone());
    let can_place_on_ceiling_decoder = MapCodecDecoderHalf(can_place_on_ceiling_field);
    let can_place_on_wall_encoder = MapCodecEncoderHalf(can_place_on_wall_field.clone());
    let can_place_on_wall_decoder = MapCodecDecoderHalf(can_place_on_wall_field);
    let chance_of_spreading_encoder = MapCodecEncoderHalf(chance_of_spreading_field.clone());
    let chance_of_spreading_decoder = MapCodecDecoderHalf(chance_of_spreading_field);
    let can_be_placed_on_encoder = MapCodecEncoderHalf(can_be_placed_on_field.clone());
    let can_be_placed_on_decoder = MapCodecDecoderHalf(can_be_placed_on_field);

    // Like `record_builder::build`'s `BuiltEncoder`, the encoder supplies no
    // keys and writes the fields in group declaration order.
    let encode = map_encoder::of(
        Arc::new(
            move |c: &MultifaceGrowthConfiguration,
                  ops: &Ops,
                  prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                block_encoder.encode(&c.place_block, ops, prefix);
                search_range_encoder.encode(&c.search_range, ops, prefix);
                can_place_on_floor_encoder.encode(&c.can_place_on_floor, ops, prefix);
                can_place_on_ceiling_encoder.encode(&c.can_place_on_ceiling, ops, prefix);
                can_place_on_wall_encoder.encode(&c.can_place_on_wall, ops, prefix);
                chance_of_spreading_encoder.encode(&c.chance_of_spreading, ops, prefix);
                can_be_placed_on_encoder.encode(&c.can_be_placed_on, ops, prefix);
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
                Fn3<
                    BlockId,
                    i32,
                    bool,
                    Fn4<bool, bool, f32, HolderSet<BlockType>, MultifaceGrowthConfiguration>,
                >,
            > = DataResult::success_with_lifecycle(
                Arc::new(move |b: &BlockId, s: &i32, f: &bool| {
                    let b = *b;
                    let s = *s;
                    let f = *f;
                    let inner: Fn4<
                        bool,
                        bool,
                        f32,
                        HolderSet<BlockType>,
                        MultifaceGrowthConfiguration,
                    > = Arc::new(
                        move |cf: &bool, cw: &bool, cs: &f32, cb: &HolderSet<BlockType>| {
                            MultifaceGrowthConfiguration::new(b, s, f, *cf, *cw, *cs, cb.clone())
                        },
                    );
                    inner
                }),
                Lifecycle::experimental(),
            );
            let step1 = data_result::ap3(
                fr,
                block_decoder.decode(ops, input),
                search_range_decoder.decode(ops, input),
                can_place_on_floor_decoder.decode(ops, input),
            );
            data_result::ap4(
                step1,
                can_place_on_ceiling_decoder.decode(ops, input),
                can_place_on_wall_decoder.decode(ops, input),
                chance_of_spreading_decoder.decode(ops, input),
                can_be_placed_on_decoder.decode(ops, input),
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

impl FeatureConfiguration for MultifaceGrowthConfiguration {}

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
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A block registry with `glow_lichen` (id 0), wrapped in a `RegistryAccess`
    /// under `Registries.BLOCK` — the `can_be_placed_on` holder-set field
    /// resolves its reference elements through it.
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
        let registry = builder.freeze();
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("block")),
            Box::new(registry) as AnyBox,
        )])
    }

    fn ops(access: RegistryAccess) -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, access)
    }

    fn glow_lichen() -> BlockId {
        BlockId::from_name("minecraft:glow_lichen").unwrap()
    }

    /// A one-element direct holder set over the test registry, resolved through
    /// the SAME access the ops use.
    fn replaceable(access: &RegistryAccess) -> HolderSet<BlockType> {
        let registry = RegistryAccess::lookup(access, &*rivet_registry::registries::BLOCK)
            .expect("block registry");
        HolderSet::direct(vec![Holder::reference(registry.registry_id(), 0)])
    }

    fn sample_config(access: &RegistryAccess) -> MultifaceGrowthConfiguration {
        MultifaceGrowthConfiguration::new(
            glow_lichen(),
            10,
            true,
            true,
            true,
            0.5,
            replaceable(access),
        )
    }

    #[test]
    fn valid_directions_follow_the_constructor_derivation() {
        let access = block_access();
        // All three booleans: UP, DOWN, then the horizontal faces.
        let all = MultifaceGrowthConfiguration::new(
            glow_lichen(),
            10,
            true,
            true,
            true,
            0.5,
            replaceable(&access),
        );
        assert_eq!(
            all.valid_directions,
            vec![
                Direction::Up,
                Direction::Down,
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West,
            ]
        );
        // Ceiling only.
        let ceiling = MultifaceGrowthConfiguration::new(
            glow_lichen(),
            10,
            false,
            true,
            false,
            0.5,
            replaceable(&access),
        );
        assert_eq!(ceiling.valid_directions, vec![Direction::Up]);
        // Wall only — the horizontal plane faces, no verticals.
        let wall = MultifaceGrowthConfiguration::new(
            glow_lichen(),
            10,
            false,
            false,
            true,
            0.5,
            replaceable(&access),
        );
        assert_eq!(
            wall.valid_directions,
            vec![
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West,
            ]
        );
    }

    #[test]
    fn shuffled_directions_preserve_the_valid_set() {
        use rivet_util::random::LegacyRandomSource;
        let access = block_access();
        let config = sample_config(&access);
        // Deterministic seed — the shuffle permutes the six valid directions
        // but never drops or duplicates one.
        let mut random = LegacyRandomSource::new(1234);
        let shuffled = config.get_shuffled_directions(&mut random);
        assert_eq!(shuffled.len(), 6);
        let mut sorted = shuffled.clone();
        sorted.sort_by_key(|d| *d as i32);
        assert_eq!(
            sorted,
            vec![
                Direction::Down,
                Direction::Up,
                Direction::North,
                Direction::South,
                Direction::West,
                Direction::East,
            ]
        );
        // Excluding a direction filters it before shuffling.
        let mut random = LegacyRandomSource::new(1234);
        let without_up = config.get_shuffled_directions_except(&mut random, Direction::Up);
        assert_eq!(without_up.len(), 5);
        assert!(!without_up.contains(&Direction::Up));
    }

    #[test]
    fn codec_round_trip_with_all_optionals_explicit() {
        let access = block_access();
        let config = sample_config(&access);
        let codec = multiface_growth_configuration_codec::<TestOps>();
        let ops = ops(access);
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
                "can_place_on_ceiling": true,
                "can_place_on_wall": true,
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
    fn codec_round_trip_with_defaulted_optionals() {
        // Paper's default-shaped config: only `block` and `can_be_placed_on`
        // are non-default; the optional fields default (10 / false / false /
        // false / 0.5) and are omitted on encode.
        let access = block_access();
        let config = MultifaceGrowthConfiguration::new(
            glow_lichen(),
            10,
            false,
            false,
            false,
            0.5,
            replaceable(&access),
        );
        let codec = multiface_growth_configuration_codec::<TestOps>();
        let ops = ops(access);
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
    fn codec_requires_block_and_can_be_placed_on() {
        let codec = multiface_growth_configuration_codec::<TestOps>();
        let ops = ops(block_access());
        // `fieldOf("block")` is required.
        let no_block = json!({"can_be_placed_on": []});
        let result = codec.parse(&ops, &no_block);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key block"), "got: {msg}");
        // `fieldOf("can_be_placed_on")` is required.
        let no_blocks = json!({"block": "minecraft:glow_lichen"});
        assert!(codec.parse(&ops, &no_blocks).is_error());
    }

    #[test]
    fn codec_rejects_unknown_block() {
        let codec = multiface_growth_configuration_codec::<TestOps>();
        let ops = ops(block_access());
        let result = codec.parse(
            &ops,
            &json!({"block": "minecraft:not_a_block", "can_be_placed_on": []}),
        );
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:block]"),
            "got: {msg}"
        );
    }

    #[test]
    fn codec_rejects_known_non_multiface_block() {
        // `validateBlock` — a KNOWN block that is not `MultifaceSpreadeableBlock`
        // (only `GlowLichenBlock`/`SculkVeinBlock` are) errors with the exact
        // Java message.
        let codec = multiface_growth_configuration_codec::<TestOps>();
        let ops = ops(block_access());
        let result = codec.parse(
            &ops,
            &json!({"block": "minecraft:stone", "can_be_placed_on": []}),
        );
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "Growth block should be a multiface spreadeable block");
    }

    #[test]
    fn codec_rejects_out_of_range_search_and_chance() {
        let codec = multiface_growth_configuration_codec::<TestOps>();
        let ops = ops(block_access());
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
    fn value_equality_semantics() {
        let access = block_access();
        let config = sample_config(&access);
        assert_eq!(
            config,
            MultifaceGrowthConfiguration::new(
                glow_lichen(),
                10,
                true,
                true,
                true,
                0.5,
                replaceable(&access)
            )
        );
        // Float.compare canonicalizes NaN payloads.
        let nan_a = f32::from_bits(0x7fc0_0001);
        let nan_b = f32::from_bits(0x7fc0_0002);
        assert_eq!(
            MultifaceGrowthConfiguration::new(
                glow_lichen(),
                10,
                true,
                true,
                true,
                nan_a,
                replaceable(&access)
            ),
            MultifaceGrowthConfiguration::new(
                glow_lichen(),
                10,
                true,
                true,
                true,
                nan_b,
                replaceable(&access)
            )
        );
        // `Float.compare(-0.0F, 0.0F) != 0`.
        assert_ne!(
            MultifaceGrowthConfiguration::new(
                glow_lichen(),
                10,
                true,
                true,
                true,
                -0.0,
                replaceable(&access)
            ),
            MultifaceGrowthConfiguration::new(
                glow_lichen(),
                10,
                true,
                true,
                true,
                0.0,
                replaceable(&access)
            )
        );
    }
}
