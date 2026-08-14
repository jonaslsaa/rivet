//! Port of `net.minecraft.world.level.levelgen.GeodeBlockSettings` (record,
//! 26.2) — the `mc.world.level.levelgen.settings` unit.
//!
//! The eight-field geode block-settings record: five `BlockStateProvider`s
//! (filling / inner / alternate-inner / middle / outer layer), the
//! `List<BlockState>` `inner_placements` (non-empty), and the two
//! `HolderSet<Block>`s `cannot_replace`/`invalid_blocks`
//! (`RegistryCodecs.homogeneousList(Registries.BLOCK)`).
//!
//! The `toPlace`-style providers are held as the erased
//! `Arc<dyn ErasedBlockStateProvider>` carrier (they are behavior, not values),
//! so the record is `Clone`+`Debug` only — the same shape
//! `SimpleBlockConfiguration`/`BlockBlobConfiguration` take. The block holder
//! sets use the block registry's `RegistryFixedCodec` + `HolderSetCodec` (the
//! `replaceable_blocks` pattern in `SpeleothemClusterConfiguration`).
//!
//! The eight-field group exceeds the `record_builder` `Group6` cap, so the
//! record codec is hand-composed with `map_encoder`/`map_decoder` mirroring
//! `Applicative.super.ap8`, flattened to the port's `ap4` helpers as
//! `ap4(ap4(map(curry4, func), t1..t4), t5..t8)` (the speleothem pattern).

use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_codec,
};
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
use rivet_serialization::extra_codecs;
use rivet_serialization::functions::Fn4;
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::{MapCodecDecoderHalf, MapCodecEncoderHalf};
use rivet_serialization::map_decoder::{self, MapDecoder};
use rivet_serialization::map_encoder::{self, MapEncoder};
use std::sync::Arc;

/// The erased provider type the five `BlockStateProvider` fields hold.
type Provider = Arc<dyn ErasedBlockStateProvider>;

/// `net.minecraft.world.level.levelgen.GeodeBlockSettings`.
#[derive(Debug, Clone)]
pub struct GeodeBlockSettings {
    /// `fillingProvider`.
    pub filling_provider: Provider,
    /// `innerLayerProvider`.
    pub inner_layer_provider: Provider,
    /// `alternateInnerLayerProvider`.
    pub alternate_inner_layer_provider: Provider,
    /// `middleLayerProvider`.
    pub middle_layer_provider: Provider,
    /// `outerLayerProvider`.
    pub outer_layer_provider: Provider,
    /// `innerPlacements` — the blocks placed inside the geode.
    pub inner_placements: Vec<BlockState>,
    /// `cannotReplace` — the blocks the geode cannot replace.
    pub cannot_replace: HolderSet<BlockType>,
    /// `invalidBlocks` — the blocks invalid for geode placement.
    pub invalid_blocks: HolderSet<BlockType>,
}

impl GeodeBlockSettings {
    /// The record constructor (the codec's `apply` function).
    #[allow(clippy::too_many_arguments)] // Java's 8-field record constructor.
    pub fn new(
        filling_provider: Provider,
        inner_layer_provider: Provider,
        alternate_inner_layer_provider: Provider,
        middle_layer_provider: Provider,
        outer_layer_provider: Provider,
        inner_placements: Vec<BlockState>,
        cannot_replace: HolderSet<BlockType>,
        invalid_blocks: HolderSet<BlockType>,
    ) -> Self {
        GeodeBlockSettings {
            filling_provider,
            inner_layer_provider,
            alternate_inner_layer_provider,
            middle_layer_provider,
            outer_layer_provider,
            inner_placements,
            cannot_replace,
            invalid_blocks,
        }
    }
}

/// `RegistryCodecs.homogeneousList(Registries.BLOCK)` — a `HolderSetCodec` over
/// the block registry whose element codec is a `RegistryFixedCodec` (tag-key or
/// element-list form, `alwaysUseList=false`). The concrete codec is not
/// `Send + Sync`, so the `Arc` is held by the ops-parameterized codec and never
/// crosses threads (the speleothem `replaceable_blocks_field_codec` pattern).
fn block_holder_set_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>(
    name: &'static str,
) -> Arc<dyn rivet_serialization::map_codec::MapCodec<HolderSet<BlockType>, Ops>> {
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
    codec::field_of(holder_set, name.to_string())
}

/// `GeodeBlockSettings.CODEC` — the ops-generic
/// `geode_block_settings_codec::<Ops>()` factory (record codec over the eight
/// required fields). See the module doc for the flattened `ap8`
/// decomposition.
pub fn geode_block_settings_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<GeodeBlockSettings, Ops>> {
    let filling_provider_field = codec::field_of(
        block_state_provider_codec::<Ops>(),
        "filling_provider".to_string(),
    );
    let inner_layer_provider_field = codec::field_of(
        block_state_provider_codec::<Ops>(),
        "inner_layer_provider".to_string(),
    );
    let alternate_inner_layer_provider_field = codec::field_of(
        block_state_provider_codec::<Ops>(),
        "alternate_inner_layer_provider".to_string(),
    );
    let middle_layer_provider_field = codec::field_of(
        block_state_provider_codec::<Ops>(),
        "middle_layer_provider".to_string(),
    );
    let outer_layer_provider_field = codec::field_of(
        block_state_provider_codec::<Ops>(),
        "outer_layer_provider".to_string(),
    );
    // `ExtraCodecs.nonEmptyList(BlockState.CODEC.listOf())`.
    let inner_placements_field = codec::field_of(
        extra_codecs::non_empty_list(codec::list(block_state_codec::<Ops>())),
        "inner_placements".to_string(),
    );
    let cannot_replace_field = block_holder_set_field_codec::<Ops>("cannot_replace");
    let invalid_blocks_field = block_holder_set_field_codec::<Ops>("invalid_blocks");

    let f1 = Arc::new(MapCodecEncoderHalf(filling_provider_field.clone()));
    let d1 = Arc::new(MapCodecDecoderHalf(filling_provider_field));
    let f2 = Arc::new(MapCodecEncoderHalf(inner_layer_provider_field.clone()));
    let d2 = Arc::new(MapCodecDecoderHalf(inner_layer_provider_field));
    let f3 = Arc::new(MapCodecEncoderHalf(
        alternate_inner_layer_provider_field.clone(),
    ));
    let d3 = Arc::new(MapCodecDecoderHalf(alternate_inner_layer_provider_field));
    let f4 = Arc::new(MapCodecEncoderHalf(middle_layer_provider_field.clone()));
    let d4 = Arc::new(MapCodecDecoderHalf(middle_layer_provider_field));
    let f5 = Arc::new(MapCodecEncoderHalf(outer_layer_provider_field.clone()));
    let d5 = Arc::new(MapCodecDecoderHalf(outer_layer_provider_field));
    let f6 = Arc::new(MapCodecEncoderHalf(inner_placements_field.clone()));
    let d6 = Arc::new(MapCodecDecoderHalf(inner_placements_field));
    let f7 = Arc::new(MapCodecEncoderHalf(cannot_replace_field.clone()));
    let d7 = Arc::new(MapCodecDecoderHalf(cannot_replace_field));
    let f8 = Arc::new(MapCodecEncoderHalf(invalid_blocks_field.clone()));
    let d8 = Arc::new(MapCodecDecoderHalf(invalid_blocks_field));

    // The encoder writes the fields in group declaration order (like
    // `record_builder::build`'s `BuiltEncoder`).
    let encode = map_encoder::of(
        Arc::new(
            move |c: &GeodeBlockSettings,
                  ops: &Ops,
                  prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                f1.encode(&c.filling_provider, ops, prefix);
                f2.encode(&c.inner_layer_provider, ops, prefix);
                f3.encode(&c.alternate_inner_layer_provider, ops, prefix);
                f4.encode(&c.middle_layer_provider, ops, prefix);
                f5.encode(&c.outer_layer_provider, ops, prefix);
                f6.encode(&c.inner_placements, ops, prefix);
                f7.encode(&c.cannot_replace, ops, prefix);
                f8.encode(&c.invalid_blocks, ops, prefix);
            },
        ),
        Arc::new(|_ops: &Ops| -> Vec<Ops::Output> { Vec::new() }),
    );

    // The decoder mirrors `Applicative.super.ap8` flattened to `ap4(ap4(...))`:
    // the leading quadruple forms a `Fn4` for `(t5..t8)`.
    #[allow(clippy::type_complexity)]
    let decode = map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let fr: DataResult<Fn4<Provider, Provider, Provider, Provider, Fn4_5_8>> =
                DataResult::success_with_lifecycle(
                    Arc::new(
                        move |a: &Provider, b: &Provider, c: &Provider, d: &Provider| {
                            let a = a.clone();
                            let b = b.clone();
                            let c = c.clone();
                            let d = d.clone();
                            let inner: Fn4_5_8 = Arc::new(
                                move |e: &Provider,
                                      f: &Vec<BlockState>,
                                      g: &HolderSet<BlockType>,
                                      h: &HolderSet<BlockType>| {
                                    GeodeBlockSettings::new(
                                        a.clone(),
                                        b.clone(),
                                        c.clone(),
                                        d.clone(),
                                        e.clone(),
                                        f.clone(),
                                        g.clone(),
                                        h.clone(),
                                    )
                                },
                            );
                            inner
                        },
                    ),
                    Lifecycle::experimental(),
                );
            let step1 = data_result::ap4(
                fr,
                d1.decode(ops, input),
                d2.decode(ops, input),
                d3.decode(ops, input),
                d4.decode(ops, input),
            );
            data_result::ap4(
                step1,
                d5.decode(ops, input),
                d6.decode(ops, input),
                d7.decode(ops, input),
                d8.decode(ops, input),
            )
        }),
        Arc::new(move |ops: &Ops| -> Vec<Ops::Output> {
            vec![
                ops.create_string("filling_provider".to_string()),
                ops.create_string("inner_layer_provider".to_string()),
                ops.create_string("alternate_inner_layer_provider".to_string()),
                ops.create_string("middle_layer_provider".to_string()),
                ops.create_string("outer_layer_provider".to_string()),
                ops.create_string("inner_placements".to_string()),
                ops.create_string("cannot_replace".to_string()),
                ops.create_string("invalid_blocks".to_string()),
            ]
        }),
    );

    map_codec::codec_of(map_codec::of(
        encode,
        decode.clone(),
        format!("RecordCodec[{:?}]", decode),
    ))
}

/// The nested `Fn4` for `(t5..t8)` in the flattened `ap8` decomposition:
/// `outer_layer_provider` (Provider), `inner_placements` (`Vec<BlockState>`),
/// `cannot_replace` (`HolderSet<BlockType>`), `invalid_blocks`
/// (`HolderSet<BlockType>`) — returning the final `GeodeBlockSettings`.
type Fn4_5_8 =
    Fn4<Provider, Vec<BlockState>, HolderSet<BlockType>, HolderSet<BlockType>, GeodeBlockSettings>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::stateproviders::block_state_provider::ErasedBlockStateProvider;
    use crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::{Identifier, RegistryAccess, RegistryBuilder};
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;
    use std::sync::Arc;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn empty_ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    fn provider(state: BlockState) -> Provider {
        let simple = SimpleStateProvider::new(state);
        let erased: Arc<dyn ErasedBlockStateProvider> = Arc::new(simple);
        erased
    }

    /// A block registry with `minecraft:stone` registered at element id 0, so a
    /// reference holder round-trips through the `cannot_replace` field codec.
    fn block_ops() -> TestOps {
        let key = rivet_registry::registries::BLOCK.clone();
        let mut builder = RegistryBuilder::<BlockType>::new(&key);
        builder.register(
            &rivet_registry::ResourceKey::create(&key, Identifier::with_default_namespace("stone")),
            Arc::new(BlockType),
            rivet_registry::RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        let access = RegistryAccess::from_single_registry(key, registry);
        RegistryOps::create_from_access(&JsonOps::INSTANCE, access)
    }

    #[test]
    fn codec_round_trips_all_eight_fields() {
        let codec = geode_block_settings_codec::<TestOps>();
        let stone = crate::block::blocks::Blocks::STONE.default_block_state();
        let air = crate::block::blocks::Blocks::AIR.default_block_state();
        let settings = GeodeBlockSettings::new(
            provider(air),
            provider(stone),
            provider(air),
            provider(stone),
            provider(air),
            vec![stone],
            HolderSet::empty(),
            HolderSet::empty(),
        );
        let encoded = codec
            .encode_start(&empty_ops(), &settings)
            .result()
            .expect("encode should succeed")
            .clone();
        // The five providers are `simple_state_provider` (`"state"`-only), and
        // the empty holder sets encode as `[]` (no registry getter needed).
        let json = encoded.as_object().expect("record object");
        assert!(json.contains_key("filling_provider"));
        assert!(json.contains_key("inner_layer_provider"));
        assert!(json.contains_key("alternate_inner_layer_provider"));
        assert!(json.contains_key("middle_layer_provider"));
        assert!(json.contains_key("outer_layer_provider"));
        assert_eq!(
            json["inner_placements"],
            json!([{"Name": "minecraft:stone"}])
        );
        assert_eq!(json["cannot_replace"], json!([]));
        assert_eq!(json["invalid_blocks"], json!([]));

        let decoded = codec
            .parse(&empty_ops(), &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.inner_placements, vec![stone]);
        assert!(decoded.cannot_replace.iter().next().is_none());
        assert!(decoded.invalid_blocks.iter().next().is_none());
        // Providers are behavior (no `PartialEq`) — the codec round-trips them
        // structurally; the other fields pin the decode.

        // The flattened-ap8 decode lifecycle is Experimental — matching Java's
        // `RecordCodecBuilder.create` over eight unstamped fields (verified
        // against the pinned DFU 10.0.21 jar: a plain `create(...).apply(i,
        // Foo::new)` decodes Experimental, unlike the `world_options` fields,
        // which are individually `.stable()`-stamped and decode Stable). The
        // seed here is deliberately not `stable()`: `.add()` combining the
        // unstamped field decodes dominates anyway, so a stable seed would be a
        // no-op (and a fidelity regression against the Java result).
        let result = codec.parse(&empty_ops(), &encoded);
        assert_eq!(
            result.lifecycle(),
            rivet_serialization::lifecycle::Lifecycle::experimental()
        );
    }

    #[test]
    fn codec_rejects_empty_inner_placements() {
        let codec = geode_block_settings_codec::<TestOps>();
        // `nonEmptyList` errors on `[]`.
        let bad = json!({
            "filling_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:air"}},
            "inner_layer_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:air"}},
            "alternate_inner_layer_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:air"}},
            "middle_layer_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:air"}},
            "outer_layer_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:air"}},
            "inner_placements": [],
            "cannot_replace": [],
            "invalid_blocks": []
        });
        assert!(codec.parse(&empty_ops(), &bad).result().is_none());
    }

    #[test]
    fn holder_sets_resolve_through_the_block_registry() {
        let codec = geode_block_settings_codec::<TestOps>();
        let ops = block_ops();
        let stone_owner = ops
            .getter::<BlockType>(&rivet_registry::registries::BLOCK)
            .expect("block getter")
            .registry()
            .expect("block registry")
            .registry_id();
        let settings = GeodeBlockSettings::new(
            provider(crate::block::blocks::Blocks::AIR.default_block_state()),
            provider(crate::block::blocks::Blocks::AIR.default_block_state()),
            provider(crate::block::blocks::Blocks::AIR.default_block_state()),
            provider(crate::block::blocks::Blocks::AIR.default_block_state()),
            provider(crate::block::blocks::Blocks::AIR.default_block_state()),
            vec![crate::block::blocks::Blocks::STONE.default_block_state()],
            HolderSet::direct(vec![Holder::reference(stone_owner, 0)]),
            HolderSet::empty(),
        );
        let encoded = codec
            .encode_start(&ops, &settings)
            .result()
            .expect("encode")
            .clone();
        // A reference holder in a single-element set encodes compactly as the
        // element itself (HolderSetCodec `alwaysUseList=false`).
        assert_eq!(encoded["cannot_replace"], json!("minecraft:stone"));
    }
}
