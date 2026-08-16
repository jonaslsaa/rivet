//! Port of `net.minecraft.world.level.levelgen.feature.configurations.SpeleothemConfiguration`
//! (record, 26.2).
//!
//! Java: a seven-field record `record SpeleothemConfiguration(BlockState
//! baseBlock, BlockState pointedBlock, HolderSet<Block> replaceableBlocks,
//! float chanceOfTallerGeneration, float chanceOfDirectionalSpread,
//! float chanceOfSpreadRadius2, float chanceOfSpreadRadius3)` whose `CODEC` is
//! a `RecordCodecBuilder` over the required `"base_block"`/`"pointed_block"`
//! (`BlockState.CODEC`), the required `"replaceable_blocks"`
//! (`RegistryCodecs.homogeneousList(Registries.BLOCK)` — a `HolderSetCodec`
//! over `RegistryFixedCodec(Registries.BLOCK)`, tag-key or element-list form),
//! and the four optional `"chance_of_*"` fields — `Codec.floatRange(0.0F, 1.0F)
//! .optionalFieldOf(name, default)` with defaults `0.2F`, `0.7F`, `0.5F`, `0.5F`.
//! DFU `Codec<T>` is `Codec<E, Ops>` in the port, so the static Java constant
//! is exposed as the ops-generic `speleothem_configuration_codec::<Ops>()`
//! factory.
//!
//! The seven-field group exceeds the port's `record_builder` `Group6` cap, so
//! the record codec is hand-composed with `map_encoder`/`map_decoder` exactly
//! mirroring `Applicative.super.ap7` (`ap4(ap3(...curry3..., t1, t2, t3), t4,
//! t5, t6, t7)`), the same shape `SculkPatchConfiguration` takes.
//!
//! ## The `optionalFieldOf(name, default)` fields
//!
//! Java's `Codec.floatRange(0.0F, 1.0F).optionalFieldOf(name, default)` is the
//! NON-lenient with-default form (verified from the pinned DFU 10.0.21
//! bytecode): `optionalField(name, codec, false).xmap(o -> o.orElse(default),
//! a -> Objects.equals(a, default) ? Optional.empty() : Optional.of(a))`. An
//! absent field decodes to `default`; a PRESENT-but-invalid value is a decode
//! error (the optional field is not lenient). The default value is OMITTED on
//! encode via `Float.equals` semantics.
//!
//! The fields use the serialization crate's shared
//! `rivet_serialization::codec::optional_field_of`, which omits on `PartialEq
//! ==` — for `f32` that treats `-0.0 == 0.0` as true and `NaN != NaN`, the
//! opposite of `Float.equals`. On encode the omission test runs first on the
//! raw config value (`xmap`'s `from` half drives `comap`, so `*a == default`
//! executes before the element codec); only a non-omitted value then reaches
//! `floatRange(0.0, 1.0)`'s range check, where `check_range_f32` rejects
//! `-0.0`, NaN, and out-of-range values under the `Float.compare` total order.
//! That ordering never diverges from Java: the defaults are positive and
//! nonzero, and no `f32` value is `PartialEq ==`-equal to `0.2`/`0.7`/`0.5`
//! while `Float.equals`-distinct (IEEE `==` differs from `Float.equals` only
//! for `-0.0`/`+0.0` and NaN, which a positive nonzero default is not), so the
//! omission decision agrees in both implementations; `-0.0` and NaN are then
//! range-rejected on both sides. (The struct's own [`PartialEq`] still
//! compares the float fields via `java_float_equals`, exactly `Float.equals`,
//! to match the record's `equals`.)
//!
//! ## Holder-set field
//!
//! `replaceable_blocks` is `HolderSet<Block>` where `Block` is the id-handle
//! placeholder [`BlockType`] — the same surface `MatchingBlocksPredicate` uses.
//! The field codec requires registry-aware ops (`Ops: RegistryOpsLookup`), so
//! the whole `CODEC` is ops-generic over that bound.

use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_codec::block_state_codec;
use rivet_registry::holder::Holder;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_registry::registry_file_codec::{HolderSetCodec, RegistryFixedCodec};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::{self, DataResult};
use rivet_serialization::dynamic_ops::{DynamicOps, MapLike, RecordBuilder};
use rivet_serialization::float_format::java_float_equals;
use rivet_serialization::functions::{Fn3, Fn4};
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::{MapCodecDecoderHalf, MapCodecEncoderHalf};
use rivet_serialization::map_decoder::{self, MapDecoder};
use rivet_serialization::map_encoder::{self, MapEncoder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.SpeleothemConfiguration`.
#[derive(Debug, Clone)]
pub struct SpeleothemConfiguration {
    /// `baseBlock` — the block speleothems grow from/on.
    pub base_block: BlockState,
    /// `pointedBlock` — the speleothem block (e.g. pointed dripstone).
    pub pointed_block: BlockState,
    /// `replaceableBlocks` — the blocks the speleothem may replace.
    pub replaceable_blocks: HolderSet<BlockType>,
    /// `chanceOfTallerGeneration` — `[0.0F, 1.0F]`, default `0.2F`.
    pub chance_of_taller_generation: f32,
    /// `chanceOfDirectionalSpread` — `[0.0F, 1.0F]`, default `0.7F`.
    pub chance_of_directional_spread: f32,
    /// `chanceOfSpreadRadius2` — `[0.0F, 1.0F]`, default `0.5F`.
    pub chance_of_spread_radius2: f32,
    /// `chanceOfSpreadRadius3` — `[0.0F, 1.0F]`, default `0.5F`.
    pub chance_of_spread_radius3: f32,
}

impl PartialEq for SpeleothemConfiguration {
    fn eq(&self, other: &Self) -> bool {
        // `Objects.equals` on the record's Float fields compares via
        // `Float.equals`, whose contract is `Float.compare` semantics: NaN
        // payloads canonicalize to one value and `-0.0` is distinct from
        // `+0.0` (see `java.lang.Float.compare`).
        self.base_block == other.base_block
            && self.pointed_block == other.pointed_block
            && self.replaceable_blocks == other.replaceable_blocks
            && java_float_equals(
                self.chance_of_taller_generation,
                other.chance_of_taller_generation,
            )
            && java_float_equals(
                self.chance_of_directional_spread,
                other.chance_of_directional_spread,
            )
            && java_float_equals(
                self.chance_of_spread_radius2,
                other.chance_of_spread_radius2,
            )
            && java_float_equals(
                self.chance_of_spread_radius3,
                other.chance_of_spread_radius3,
            )
    }
}

impl Eq for SpeleothemConfiguration {}

impl SpeleothemConfiguration {
    /// `new SpeleothemConfiguration(BlockState, BlockState, HolderSet<Block>,
    /// float, float, float, float)` — the record constructor (the codec's
    /// `apply` function).
    pub fn new(
        base_block: BlockState,
        pointed_block: BlockState,
        replaceable_blocks: HolderSet<BlockType>,
        chance_of_taller_generation: f32,
        chance_of_directional_spread: f32,
        chance_of_spread_radius2: f32,
        chance_of_spread_radius3: f32,
    ) -> Self {
        SpeleothemConfiguration {
            base_block,
            pointed_block,
            replaceable_blocks,
            chance_of_taller_generation,
            chance_of_directional_spread,
            chance_of_spread_radius2,
            chance_of_spread_radius3,
        }
    }

    /// `baseBlock()`.
    pub fn base_block(&self) -> BlockState {
        self.base_block
    }

    /// `pointedBlock()`.
    pub fn pointed_block(&self) -> BlockState {
        self.pointed_block
    }

    /// `replaceableBlocks()`.
    pub fn replaceable_blocks(&self) -> &HolderSet<BlockType> {
        &self.replaceable_blocks
    }

    /// `chanceOfTallerGeneration()`.
    pub fn chance_of_taller_generation(&self) -> f32 {
        self.chance_of_taller_generation
    }

    /// `chanceOfDirectionalSpread()`.
    pub fn chance_of_directional_spread(&self) -> f32 {
        self.chance_of_directional_spread
    }

    /// `chanceOfSpreadRadius2()`.
    pub fn chance_of_spread_radius2(&self) -> f32 {
        self.chance_of_spread_radius2
    }

    /// `chanceOfSpreadRadius3()`.
    pub fn chance_of_spread_radius3(&self) -> f32 {
        self.chance_of_spread_radius3
    }
}

/// `RegistryCodecs.homogeneousList(Registries.BLOCK)` — the
/// `"replaceable_blocks"` field codec: a `HolderSetCodec` over the block
/// registry whose element codec is a `RegistryFixedCodec` (tag key
/// `#minecraft:...` or element-list form, `alwaysUseList=false`).
///
/// The concrete codec is not `Send + Sync` (its `RegistryOps` carries the
/// single-threaded `HolderLookupAdapter`, `RefCell` memo — OWNERSHIP's single
/// sync tick); the `Arc` is held by the ops-parameterized configuration codec
/// and never crosses threads.
fn replaceable_blocks_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn rivet_serialization::map_codec::MapCodec<HolderSet<BlockType>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<Holder<BlockType>, Ops>> = Arc::new(RegistryFixedCodec::create(
        &rivet_registry::registries::BLOCK,
    ));
    #[allow(clippy::arc_with_non_send_sync)]
    let holder_set: Arc<dyn Codec<HolderSet<BlockType>, Ops>> = Arc::new(HolderSetCodec::create(
        &rivet_registry::registries::BLOCK,
        element,
        false,
    ));
    codec::field_of(holder_set, "replaceable_blocks".to_string())
}

/// `SpeleothemConfiguration.CODEC` — the ops-generic
/// `speleothem_configuration_codec::<Ops>()` factory (record codec over the
/// seven fields: three required, four optional-with-default). The seven-field
/// group exceeds the port's `record_builder` `Group6` cap, so the decode side
/// is hand-composed with the `Applicative.super.ap7` decomposition
/// `ap4(ap3(map(Function7.curry3, func), t1, t2, t3), t4, t5, t6, t7)`: the
/// leading triple's results assemble a `Fn3` returning the trailing `Fn4`,
/// which the outer `ap4` applies to the last four field results (the same
/// shape `SculkPatchConfiguration` takes).
///
/// Each field is a full `MapCodec` (`codec::field_of(...)` for the required
/// fields, `codec::optional_field_of` for the chance fields); the
/// encoder/decoder halves are the `MapCodecEncoderHalf`/`MapCodecDecoderHalf`
/// adapters, so the field codec's own `encode`/`decode` (which knows the key
/// and — for the optionals — the default/omission logic) drives both directions.
pub fn speleothem_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<SpeleothemConfiguration, Ops>> {
    let base_block_field = codec::field_of(block_state_codec::<Ops>(), "base_block".to_string());
    let pointed_block_field =
        codec::field_of(block_state_codec::<Ops>(), "pointed_block".to_string());
    let replaceable_blocks_field = replaceable_blocks_field_codec::<Ops>();
    let chance_of_taller_generation_field = codec::optional_field_of::<f32, Ops>(
        "chance_of_taller_generation",
        codec::float_range::<Ops>(0.0, 1.0),
        0.2,
    );
    let chance_of_directional_spread_field = codec::optional_field_of::<f32, Ops>(
        "chance_of_directional_spread",
        codec::float_range::<Ops>(0.0, 1.0),
        0.7,
    );
    let chance_of_spread_radius2_field = codec::optional_field_of::<f32, Ops>(
        "chance_of_spread_radius2",
        codec::float_range::<Ops>(0.0, 1.0),
        0.5,
    );
    let chance_of_spread_radius3_field = codec::optional_field_of::<f32, Ops>(
        "chance_of_spread_radius3",
        codec::float_range::<Ops>(0.0, 1.0),
        0.5,
    );

    let base_block_encoder = Arc::new(MapCodecEncoderHalf(base_block_field.clone()));
    let base_block_decoder = Arc::new(MapCodecDecoderHalf(base_block_field));
    let pointed_block_encoder = Arc::new(MapCodecEncoderHalf(pointed_block_field.clone()));
    let pointed_block_decoder = Arc::new(MapCodecDecoderHalf(pointed_block_field));
    let replaceable_blocks_encoder =
        Arc::new(MapCodecEncoderHalf(replaceable_blocks_field.clone()));
    let replaceable_blocks_decoder = Arc::new(MapCodecDecoderHalf(replaceable_blocks_field));
    let chance_of_taller_generation_encoder = Arc::new(MapCodecEncoderHalf(
        chance_of_taller_generation_field.clone(),
    ));
    let chance_of_taller_generation_decoder =
        Arc::new(MapCodecDecoderHalf(chance_of_taller_generation_field));
    let chance_of_directional_spread_encoder = Arc::new(MapCodecEncoderHalf(
        chance_of_directional_spread_field.clone(),
    ));
    let chance_of_directional_spread_decoder =
        Arc::new(MapCodecDecoderHalf(chance_of_directional_spread_field));
    let chance_of_spread_radius2_encoder =
        Arc::new(MapCodecEncoderHalf(chance_of_spread_radius2_field.clone()));
    let chance_of_spread_radius2_decoder =
        Arc::new(MapCodecDecoderHalf(chance_of_spread_radius2_field));
    let chance_of_spread_radius3_encoder =
        Arc::new(MapCodecEncoderHalf(chance_of_spread_radius3_field.clone()));
    let chance_of_spread_radius3_decoder =
        Arc::new(MapCodecDecoderHalf(chance_of_spread_radius3_field));

    // Like `record_builder::build`'s `BuiltEncoder`, the encoder supplies no
    // keys and writes the fields in group declaration order.
    let encode = map_encoder::of(
        Arc::new(
            move |c: &SpeleothemConfiguration,
                  ops: &Ops,
                  prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                base_block_encoder.encode(&c.base_block, ops, prefix);
                pointed_block_encoder.encode(&c.pointed_block, ops, prefix);
                replaceable_blocks_encoder.encode(&c.replaceable_blocks, ops, prefix);
                chance_of_taller_generation_encoder.encode(
                    &c.chance_of_taller_generation,
                    ops,
                    prefix,
                );
                chance_of_directional_spread_encoder.encode(
                    &c.chance_of_directional_spread,
                    ops,
                    prefix,
                );
                chance_of_spread_radius2_encoder.encode(&c.chance_of_spread_radius2, ops, prefix);
                chance_of_spread_radius3_encoder.encode(&c.chance_of_spread_radius3, ops, prefix);
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
                    BlockState,
                    BlockState,
                    HolderSet<BlockType>,
                    Fn4<f32, f32, f32, f32, SpeleothemConfiguration>,
                >,
            > = DataResult::success_with_lifecycle(
                Arc::new(
                    move |c1: &BlockState, c2: &BlockState, c3: &HolderSet<BlockType>| {
                        let c1 = *c1;
                        let c2 = *c2;
                        let c3 = c3.clone();
                        let inner: Fn4<f32, f32, f32, f32, SpeleothemConfiguration> =
                            Arc::new(move |g1: &f32, g2: &f32, g3: &f32, g4: &f32| {
                                SpeleothemConfiguration::new(c1, c2, c3.clone(), *g1, *g2, *g3, *g4)
                            });
                        inner
                    },
                ),
                Lifecycle::experimental(),
            );
            let step1 = data_result::ap3(
                fr,
                base_block_decoder.decode(ops, input),
                pointed_block_decoder.decode(ops, input),
                replaceable_blocks_decoder.decode(ops, input),
            );
            data_result::ap4(
                step1,
                chance_of_taller_generation_decoder.decode(ops, input),
                chance_of_directional_spread_decoder.decode(ops, input),
                chance_of_spread_radius2_decoder.decode(ops, input),
                chance_of_spread_radius3_decoder.decode(ops, input),
            )
        }),
        Arc::new(move |ops: &Ops| -> Vec<Ops::Output> {
            vec![
                ops.create_string("base_block".to_string()),
                ops.create_string("pointed_block".to_string()),
                ops.create_string("replaceable_blocks".to_string()),
                ops.create_string("chance_of_taller_generation".to_string()),
                ops.create_string("chance_of_directional_spread".to_string()),
                ops.create_string("chance_of_spread_radius2".to_string()),
                ops.create_string("chance_of_spread_radius3".to_string()),
            ]
        }),
    );

    map_codec::codec_of(map_codec::of(
        encode,
        decode.clone(),
        format!("RecordCodec[{:?}]", decode),
    ))
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for SpeleothemConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::Identifier;
    use rivet_registry::ResourceKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::holder::Holder;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::root::AnyBox;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A block registry with `stone` (id 0) and `oak_log` (id 1), wrapped in a
    /// `RegistryAccess` under `Registries.BLOCK` — the holder-set field's
    /// element codec resolves the block through it.
    fn block_access() -> RegistryAccess {
        let mut builder = RegistryBuilder::new(&*rivet_registry::registries::BLOCK);
        builder.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:stone"),
            ),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:oak_log"),
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

    /// A holder set over the two-block test registry, resolved through the SAME
    /// access the ops use. Each `RegistryBuilder::freeze()` allocates a fresh
    /// `RegistryId` from the global counter, so the set's reference holders
    /// must carry the registry id the ops' provider reads — one access for both.
    fn two_block_set(access: &RegistryAccess) -> HolderSet<BlockType> {
        let registry = RegistryAccess::lookup(access, &*rivet_registry::registries::BLOCK)
            .expect("block registry");
        HolderSet::direct(vec![
            Holder::reference(registry.registry_id(), 0),
            Holder::reference(registry.registry_id(), 1),
        ])
    }

    fn sample_config(replaceable: HolderSet<BlockType>) -> SpeleothemConfiguration {
        SpeleothemConfiguration::new(
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
            BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap()),
            replaceable,
            0.2,
            0.7,
            0.5,
            0.5,
        )
    }

    #[test]
    fn codec_round_trip() {
        // One access builds BOTH the set's reference holders and the ops: each
        // `freeze()` allocates a fresh `RegistryId` from the global counter, so
        // the holders must carry the same registry id the ops' provider reads.
        let access = block_access();
        let config = sample_config(two_block_set(&access));
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = speleothem_configuration_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        // All four chance fields equal their defaults, so they are omitted on
        // encode (the `Objects.equals(a, default)` half of `optionalFieldOf`).
        // `BlockState.CODEC` (`StateHolder.codec`) writes the full `"Properties"`
        // map for a non-singleton state definition — including defaults — so
        // `pointed_dripstone`'s default state encodes with all three properties.
        assert_eq!(
            encoded,
            json!({
                "base_block": {"Name": "minecraft:stone"},
                "pointed_block": {
                    "Name": "minecraft:pointed_dripstone",
                    "Properties": {
                        "waterlogged": "false",
                        "vertical_direction": "up",
                        "thickness": "tip",
                    },
                },
                "replaceable_blocks": ["minecraft:stone", "minecraft:oak_log"],
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
    fn codec_round_trip_with_explicit_chances() {
        let access = block_access();
        let config = SpeleothemConfiguration::new(
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
            BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap()),
            two_block_set(&access),
            0.1,
            0.3,
            0.6,
            0.9,
        );
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = speleothem_configuration_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "base_block": {"Name": "minecraft:stone"},
                "pointed_block": {
                    "Name": "minecraft:pointed_dripstone",
                    "Properties": {
                        "waterlogged": "false",
                        "vertical_direction": "up",
                        "thickness": "tip",
                    },
                },
                "replaceable_blocks": ["minecraft:stone", "minecraft:oak_log"],
                "chance_of_taller_generation": 0.1,
                "chance_of_directional_spread": 0.3,
                "chance_of_spread_radius2": 0.6,
                "chance_of_spread_radius3": 0.9,
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
    fn codec_decodes_absent_chances_to_defaults() {
        // A fixture with only the three required fields decodes to the defaults
        // `0.2`, `0.7`, `0.5`, `0.5`.
        let access = block_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = speleothem_configuration_codec::<TestOps>();
        let decoded = codec
            .parse(
                &ops,
                &json!({
                    "base_block": {"Name": "minecraft:stone"},
                    "pointed_block": {"Name": "minecraft:pointed_dripstone"},
                    "replaceable_blocks": ["minecraft:stone", "minecraft:oak_log"],
                }),
            )
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.chance_of_taller_generation(), 0.2);
        assert_eq!(decoded.chance_of_directional_spread(), 0.7);
        assert_eq!(decoded.chance_of_spread_radius2(), 0.5);
        assert_eq!(decoded.chance_of_spread_radius3(), 0.5);
    }

    #[test]
    fn codec_requires_the_required_fields() {
        let access = block_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = speleothem_configuration_codec::<TestOps>();
        // A record with no required fields at all.
        assert!(codec.parse(&ops, &json!({})).is_error());
        // Two of the three required fields — `replaceable_blocks` missing.
        let missing_blocks = json!({
            "base_block": {"Name": "minecraft:stone"},
            "pointed_block": {"Name": "minecraft:pointed_dripstone"},
        });
        assert!(codec.parse(&ops, &missing_blocks).is_error());
        // `base_block` present but wrong-typed.
        let bad_base = json!({
            "base_block": "not-a-state",
            "pointed_block": {"Name": "minecraft:pointed_dripstone"},
            "replaceable_blocks": ["minecraft:stone"],
        });
        assert!(codec.parse(&ops, &bad_base).is_error());
    }

    #[test]
    fn codec_rejects_out_of_range_chance_when_present() {
        // The optional chance fields are NON-lenient (`optionalFieldOf(name,
        // default)` is `optionalField(name, codec, false)`): a PRESENT but
        // out-of-range value is a decode error, NOT a fallback to the default.
        let access = block_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = speleothem_configuration_codec::<TestOps>();
        let too_high = json!({
            "base_block": {"Name": "minecraft:stone"},
            "pointed_block": {"Name": "minecraft:pointed_dripstone"},
            "replaceable_blocks": ["minecraft:stone"],
            "chance_of_taller_generation": 1.5,
        });
        assert!(codec.parse(&ops, &too_high).is_error());
        // `chance_of_spread_radius3` = -0.0 is below [0.0, 1.0]: `Float.compare`
        // places -0.0 before +0.0 (Paper's `checkRange` rejects it) even though
        // IEEE `-0.0 >= 0.0` is true.
        let negative_zero = json!({
            "base_block": {"Name": "minecraft:stone"},
            "pointed_block": {"Name": "minecraft:pointed_dripstone"},
            "replaceable_blocks": ["minecraft:stone"],
            "chance_of_spread_radius3": -0.0,
        });
        assert!(codec.parse(&ops, &negative_zero).is_error());
    }

    #[test]
    fn codec_rejects_out_of_range_chance_on_encode() {
        let access = block_access();
        let replaceable = two_block_set(&access);
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = speleothem_configuration_codec::<TestOps>();
        // `chance_of_taller_generation` above [0.0, 1.0].
        let too_high = SpeleothemConfiguration::new(
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
            BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap()),
            replaceable.clone(),
            1.1,
            0.7,
            0.5,
            0.5,
        );
        assert!(codec.encode_start(&ops, &too_high).result().is_none());
        // `chance_of_directional_spread` = -0.0 is below [0.0, 1.0].
        let negative_zero = SpeleothemConfiguration::new(
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
            BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap()),
            replaceable,
            0.2,
            -0.0,
            0.5,
            0.5,
        );
        assert!(codec.encode_start(&ops, &negative_zero).result().is_none());
    }

    #[test]
    fn accessors_return_the_fields() {
        let access = block_access();
        let config = sample_config(two_block_set(&access));
        assert_eq!(
            config.base_block(),
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap())
        );
        assert_eq!(
            config.pointed_block(),
            BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap())
        );
        assert_eq!(config.replaceable_blocks().size(), 2);
        assert_eq!(config.chance_of_taller_generation(), 0.2);
        assert_eq!(config.chance_of_directional_spread(), 0.7);
        assert_eq!(config.chance_of_spread_radius2(), 0.5);
        assert_eq!(config.chance_of_spread_radius3(), 0.5);
    }

    #[test]
    fn value_equality_semantics() {
        let access = block_access();
        let replaceable = two_block_set(&access);
        let config = sample_config(replaceable.clone());
        assert_eq!(
            config,
            SpeleothemConfiguration::new(
                BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
                BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap()),
                replaceable.clone(),
                0.2,
                0.7,
                0.5,
                0.5
            )
        );
        assert_ne!(
            config,
            SpeleothemConfiguration::new(
                BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
                BlockState::of(BlockId::from_name("minecraft:oak_log").unwrap()),
                replaceable.clone(),
                0.2,
                0.7,
                0.5,
                0.5
            )
        );
        // `Float.equals` canonicalizes every NaN payload: two distinct payloads
        // compare equal (IEEE `==` rejects).
        let nan_a = f32::from_bits(0x7fc0_0001);
        let nan_b = f32::from_bits(0x7fc0_0002);
        assert!(nan_a.is_nan() && nan_b.is_nan());
        assert_ne!(nan_a, nan_b);
        assert_eq!(
            SpeleothemConfiguration::new(
                BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
                BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap()),
                replaceable.clone(),
                nan_a,
                0.7,
                0.5,
                0.5
            ),
            SpeleothemConfiguration::new(
                BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
                BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap()),
                replaceable,
                nan_b,
                0.7,
                0.5,
                0.5
            )
        );
        // `Objects.equals(-0.0F, 0.0F)` is false — signed zero is distinct.
        let access = block_access();
        let replaceable = two_block_set(&access);
        assert_ne!(
            SpeleothemConfiguration::new(
                BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
                BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap()),
                replaceable.clone(),
                -0.0,
                0.7,
                0.5,
                0.5
            ),
            SpeleothemConfiguration::new(
                BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
                BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap()),
                replaceable,
                0.0,
                0.7,
                0.5,
                0.5
            )
        );
    }
}
