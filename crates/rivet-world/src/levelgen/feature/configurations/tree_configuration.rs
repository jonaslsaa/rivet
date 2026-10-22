//! TreeConfiguration's nine fields and executable trunk/foliage/size codec.
//! Source: pinned Paper 26.2 TreeConfiguration.java.
//! Root/decorator descriptors retain known unsupported type identities so the
//! eager registry closure can be loaded. Selecting such a tree refuses before
//! writes/RNG; encoding those incomplete descriptors also refuses explicitly.

use super::FeatureConfiguration;
use crate::levelgen::feature::featuresize::{FeatureSize, TwoLayersFeatureSize};
use crate::levelgen::feature::foliageplacers::blob_foliage_placer::BlobFoliagePlacer;
use crate::levelgen::feature::foliageplacers::foliage_placer::ErasedFoliagePlacer;
use crate::levelgen::feature::stateproviders::block_state_provider::ErasedBlockStateProvider;
use crate::levelgen::feature::trunkplacers::straight_trunk_placer::StraightTrunkPlacer;
use crate::levelgen::feature::trunkplacers::trunk_placer::ErasedTrunkPlacer;
use std::sync::Arc;

use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::{
    codec::{self, Codec},
    data_result::DataResult,
    dynamic_ops::DynamicOps,
    map_codec,
    record_builder::{self, RecordCodecBuilder},
};

/// Retain known unsupported decorators for eager registry resolution. Their
/// behavior and serialization remain explicitly unavailable.
pub(crate) fn unavailable_decorator_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<UnavailableTreeDecorator, Ops>> {
    codec::flat_xmap(
        map_codec::codec_of(codec::field_of(
            codec::string_codec::<Ops>(),
            "type".to_string(),
        )),
        Arc::new(|name: &String| {
            let path = name.strip_prefix("minecraft:").unwrap_or(name);
            if !matches!(
                path,
                "trunk_vine"
                    | "leave_vine"
                    | "pale_moss"
                    | "creaking_heart"
                    | "cocoa"
                    | "beehive"
                    | "alter_ground"
                    | "attached_to_leaves"
                    | "place_on_ground"
                    | "attached_to_logs"
            ) || (name.contains(':') && !name.starts_with("minecraft:"))
            {
                return DataResult::error(format!("unknown tree decorator type {name}"));
            }
            DataResult::success(UnavailableTreeDecorator {
                type_name: format!("minecraft:{path}"),
            })
        }),
        Arc::new(|value: &UnavailableTreeDecorator| {
            DataResult::error(format!(
                "tree decorator {} is not implemented",
                value.type_name
            ))
        }),
    )
}
fn unavailable_root_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<UnavailableRootPlacer, Ops>>
{
    codec::flat_xmap(
        map_codec::codec_of(codec::field_of(
            codec::string_codec::<Ops>(),
            "type".to_string(),
        )),
        Arc::new(|name: &String| {
            if name != "minecraft:mangrove_root_placer" && name != "mangrove_root_placer" {
                return DataResult::error(format!("unknown root placer type {name}"));
            }
            DataResult::success(UnavailableRootPlacer {
                type_name: "minecraft:mangrove_root_placer".to_string(),
            })
        }),
        Arc::new(|value: &UnavailableRootPlacer| {
            DataResult::error(format!(
                "tree root placer {} is not implemented",
                value.type_name
            ))
        }),
    )
}

pub fn tree_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<TreeConfiguration, Ops>> {
    use crate::levelgen::feature::featuresize::feature_size_codec;
    use crate::levelgen::feature::foliageplacers::foliage_placer::foliage_placer_codec;
    use crate::levelgen::feature::stateproviders::block_state_provider::block_state_provider_codec;
    use crate::levelgen::feature::trunkplacers::trunk_placer::trunk_placer_codec;
    record_builder::create(|i| {
        let placers = i
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &TreeConfiguration| c.trunk_provider.clone()),
                codec::field_of(
                    block_state_provider_codec::<Ops>(),
                    "trunk_provider".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &TreeConfiguration| c.trunk_placer.clone()),
                codec::field_of(trunk_placer_codec::<Ops>(), "trunk_placer".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &TreeConfiguration| c.foliage_provider.clone()),
                codec::field_of(
                    block_state_provider_codec::<Ops>(),
                    "foliage_provider".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &TreeConfiguration| c.foliage_placer.clone()),
                codec::field_of(foliage_placer_codec::<Ops>(), "foliage_placer".to_string()),
            ))
            .apply(i, Arc::new(|a, b, c, d| (a, b, c, d)));
        let settings = i
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &TreeConfiguration| c.root_placer.clone()),
                codec::optional_field(
                    "root_placer".to_string(),
                    unavailable_root_codec::<Ops>(),
                    false,
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &TreeConfiguration| c.minimum_size.clone()),
                codec::field_of(feature_size_codec::<Ops>(), "minimum_size".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &TreeConfiguration| c.decorators.clone()),
                codec::field_of(
                    codec::list(unavailable_decorator_codec::<Ops>()),
                    "decorators".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &TreeConfiguration| c.ignore_vines),
                map_codec::or_else_value(
                    codec::field_of(codec::bool_codec::<Ops>(), "ignore_vines".to_string()),
                    false,
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &TreeConfiguration| c.below_trunk_provider.clone()),
                codec::field_of(
                    block_state_provider_codec::<Ops>(),
                    "below_trunk_provider".to_string(),
                ),
            ))
            .apply(i, Arc::new(|a, b, c, d, e| (a, b, c, d, e)));
        i.group(placers).and(settings).apply(i, Arc::new(
            |(trunk_provider, trunk_placer, foliage_provider, foliage_placer),
             (root_placer, minimum_size, decorators, ignore_vines, below_trunk_provider)| TreeConfiguration {
                trunk_provider, trunk_placer, foliage_provider, foliage_placer,
                root_placer, minimum_size, decorators, ignore_vines, below_trunk_provider,
            }
        ))
    })
}

/// `net.minecraft.world.level.levelgen.feature.configurations.TreeConfiguration`
/// — executable tree parameters, with explicit root/decorator boundaries.
#[derive(Debug, Clone)]
pub struct TreeConfiguration {
    /// `this.foliageProvider` — the foliage (leaves) block state provider.
    pub foliage_provider: Arc<dyn ErasedBlockStateProvider>,
    /// `this.trunkProvider` — the trunk (log) block state provider.
    pub trunk_provider: Arc<dyn ErasedBlockStateProvider>,
    /// `this.belowTrunkProvider` — the optional below-trunk block state
    /// provider (`getOptionalState`); `TrunkPlacer.placeBelowTrunkBlock`
    /// places its state only when the provider yields one.
    pub below_trunk_provider: Arc<dyn ErasedBlockStateProvider>,
    pub trunk_placer: Arc<dyn ErasedTrunkPlacer>,
    pub foliage_placer: Arc<dyn ErasedFoliagePlacer>,
    pub root_placer: Option<UnavailableRootPlacer>,
    pub minimum_size: Arc<dyn FeatureSize>,
    pub decorators: Vec<UnavailableTreeDecorator>,
    pub ignore_vines: bool,
}

/// Explicit capability boundary, not a no-op root implementation.
#[derive(Debug, Clone)]
pub struct UnavailableRootPlacer {
    pub type_name: String,
}

/// Explicit capability boundary, not a no-op decorator implementation.
#[derive(Debug, Clone)]
pub struct UnavailableTreeDecorator {
    pub type_name: String,
}

impl FeatureConfiguration for TreeConfiguration {}

impl TreeConfiguration {
    /// A test-only config whose providers are single `SimpleStateProvider`s of
    /// the default state (used by the foliage/trunk placer codec/geometry
    /// tests; production construction lands with the owning unit).
    pub fn stub() -> TreeConfiguration {
        let state = rivet_registry::block_state::BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_id(1),
        );
        let provider = crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider::new(state);
        let erased: Arc<dyn ErasedBlockStateProvider> = Arc::new(provider);
        TreeConfiguration {
            foliage_provider: erased.clone(),
            trunk_provider: erased.clone(),
            below_trunk_provider: erased,
            trunk_placer: Arc::new(StraightTrunkPlacer::new(4, 2, 0)),
            foliage_placer: Arc::new(BlobFoliagePlacer::new(
                rivet_util::valueproviders::int_provider::IntProvider::Constant(
                    rivet_util::valueproviders::constant_int::ConstantInt::of(2),
                ),
                rivet_util::valueproviders::int_provider::IntProvider::Constant(
                    rivet_util::valueproviders::constant_int::ConstantInt::of(0),
                ),
                3,
            )),
            root_placer: None,
            minimum_size: Arc::new(TwoLayersFeatureSize::new(1, 0, 1)),
            decorators: Vec::new(),
            ignore_vines: false,
        }
    }

    pub(crate) fn require_supported_extensions(&self) {
        if let Some(root) = &self.root_placer {
            panic!("tree root placer {} is not implemented", root.type_name);
        }
        if let Some(decorator) = self.decorators.first() {
            panic!("tree decorator {} is not implemented", decorator.type_name);
        }
    }
}
