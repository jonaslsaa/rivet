//! Port of `net.minecraft.world.level.levelgen.feature.featuresize.FeatureSize`
//! (abstract class, 26.2) — the value-layer root of the feature-size package
//! (issue #391).
//!
//! Java `FeatureSize` is the abstract base of the tree-canopy size
//! configurations (`TwoLayersFeatureSize`/`ThreeLayersFeatureSize`); its
//! `CODEC` is the dispatch root that resolves the concrete variant by the
//! registered feature-size type:
//!
//! ```text
//! CODEC = BuiltInRegistries.FEATURE_SIZE_TYPE.byNameCodec()
//!            .dispatch(FeatureSize::type, FeatureSizeType::codec)
//! ```
//!
//! i.e. a `"type"` field naming the feature-size type via the by-name registry
//! codec, whose per-type `MapCodec` then applies to the whole map — exactly the
//! `Feature`/`BlockPredicate`/`BlockState` dispatch shape. The Rust port keeps
//! the same identity split as `BlockPredicate`: [`FeatureSize`] is the
//! object-safe behavior contract, its registry identity is the erased
//! [`FeatureSizeTypeId`] handle from `rivet-registry`, and the erased carrier
//! `Arc<dyn FeatureSize>` is what the dispatch codec (de)serializes.
//!
//! `FeatureSizeType.java` registers `TWO_LAYERS_FEATURE_SIZE` then
//! `THREE_LAYERS_FEATURE_SIZE` in that exact order (the generated id space, see
//! `rivet-registry::feature_size_type`), so the dispatch resolves both built-in
//! types — declaration-order codec dispatch with no fabricated fallback.
//!
//! ## Scope boundary
//!
//! Both vanilla feature-size types are ported in this wave, so the dispatch
//! table is total over the generated two-entry registry. A future third type
//! would fail the by-name lookup with Paper's `"Unknown registry key"` error.

use crate::levelgen::feature::featuresize::three_layers_feature_size;
use crate::levelgen::feature::featuresize::two_layers_feature_size;
use rivet_registry::feature_size_type::FeatureSizeTypeId;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `FeatureSize.MAX_WIDTH` — `16`.
pub const MAX_WIDTH: i32 = 16;

/// `net.minecraft.world.level.levelgen.feature.featuresize.FeatureSize` — the
/// behavior contract of a tree-canopy size configuration (Java's abstract
/// `getSizeAtHeight(int treeHeight, int yo)` + `type()`).
///
/// The erased carrier `Arc<dyn FeatureSize>` is what the dispatch codec
/// (de)serializes — the Rust analogue of Java's `Codec<FeatureSize>` value.
/// `Any` (supertrait) enables the dispatch codec's downcast of an erased value
/// back to its concrete type on encode, via the explicit [`FeatureSize::as_any`]
/// seam (the same pattern `BlockPredicate` uses).
pub trait FeatureSize: Any + Debug + Send + Sync + 'static {
    /// `FeatureSize.type()` — the registry-held `FeatureSizeType<?>` identity
    /// this size dispatches on (the key `FeatureSize.CODEC` uses).
    fn type_id(&self) -> Arc<FeatureSizeTypeId>;

    /// `FeatureSize.getSizeAtHeight(int treeHeight, int yo)` — the canopy size
    /// at relative height `yo` of a `treeHeight`-tall tree.
    fn get_size_at_height(&self, tree_height: i32, yo: i32) -> i32;

    /// `FeatureSize.minClippedHeight()` — the `OptionalInt` minimum clipped
    /// height (absent → `None`).
    fn min_clipped_height(&self) -> Option<i32>;

    /// `as_any` — the downcast seam (Java's erased `FeatureSize` cast) the
    /// dispatch codec uses on encode to recover the concrete variant type.
    fn as_any(&self) -> &dyn Any;
}

/// `FeatureSize.CODEC` — the dispatch codec, as the ops-generic
/// `feature_size_codec::<Ops>()` factory.
pub fn feature_size_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Arc<dyn FeatureSize>, Ops>>
{
    let dispatch =
        key_dispatch_codec::dispatch_map::<Arc<FeatureSizeTypeId>, Arc<dyn FeatureSize>, Ops>(
            "type",
            feature_size_type_by_name_codec::<Ops>(),
            Arc::new(|size: &Arc<dyn FeatureSize>| {
                DataResult::success(FeatureSize::type_id(&**size))
            }),
            Arc::new(codec_for_type),
        );
    map_codec::codec_of(dispatch)
}

/// `FeatureSizeType::codec` — resolve a `FeatureSizeTypeId` to its
/// `MapCodec<Arc<dyn FeatureSize>>` (the dispatch's `codec` function).
fn codec_for_type<Ops: DynamicOps + 'static>(
    k: &Arc<FeatureSizeTypeId>,
) -> DataResult<Arc<dyn MapCodec<Arc<dyn FeatureSize>, Ops>>> {
    match k.name() {
        "minecraft:two_layers_feature_size" => {
            DataResult::success(erase_map_codec(two_layers_feature_size::map_codec::<Ops>()))
        }
        "minecraft:three_layers_feature_size" => DataResult::success(erase_map_codec(
            three_layers_feature_size::map_codec::<Ops>(),
        )),
        other => DataResult::error(format!("Feature size type '{}' is not ported", other)),
    }
}

/// `BuiltInRegistries.FEATURE_SIZE_TYPE.byNameCodec()` over the erased
/// `Arc<FeatureSizeTypeId>` identity.
pub fn feature_size_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<Arc<FeatureSizeTypeId>, Ops>> {
    let access = FeatureSizeTypeId::built_in_registry_access();
    let registry: &rivet_registry::registry::Registry<FeatureSizeTypeId> = access
        .lookup(&rivet_registry::registries::FEATURE_SIZE_TYPE)
        .expect("built-in feature-size registry is present");
    registry.by_name_codec::<Ops>()
}

/// Lift a concrete variant's `MapCodec<C>` to `MapCodec<Arc<dyn FeatureSize>>`
/// — Java's `MapCodec<? extends FeatureSize>` variance, via xmap (the same lift
/// `BlockPredicate`'s dispatch performs onto its erased carrier).
fn erase_map_codec<C, Ops>(
    inner: Arc<dyn MapCodec<C, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn FeatureSize>, Ops>>
where
    C: FeatureSize + Clone + 'static,
    Ops: DynamicOps + 'static,
{
    map_codec::xmap(
        inner,
        Arc::new(|c: &C| -> Arc<dyn FeatureSize> { Arc::new(c.clone()) }),
        Arc::new(downcast_erased::<C>),
    )
}

/// The encode-side `from` of the erase lift: downcast the erased value to its
/// concrete variant (safe — the dispatch guarantees the value's type).
fn downcast_erased<C: FeatureSize + Clone + 'static>(size: &Arc<dyn FeatureSize>) -> C {
    size.as_any()
        .downcast_ref::<C>()
        .expect("feature size codec applied to a value of a different type")
        .clone()
}

/// `FeatureSize.minClippedHeightCodec()` — the `min_clipped_height` optional
/// field shared by both concrete variants.
///
/// Java (FeatureSize.java:13-19):
/// ```java
/// return Codec.intRange(0, 80).optionalFieldOf("min_clipped_height")
///     .xmap(o -> o.map(OptionalInt::of).orElse(OptionalInt.empty()),
///           o -> o.isPresent() ? Optional.of(o.getAsInt()) : Optional.empty())
///     .forGetter(f -> f.minClippedHeight);
/// ```
/// The `optionalFieldOf(String)` form is the non-lenient optional: absent →
/// `None`, present-but-invalid → error.
pub(crate) fn min_clipped_height_codec<
    S: FeatureSize + Clone + 'static,
    Ops: DynamicOps + 'static,
>() -> rivet_serialization::record_builder::RecordCodecBuilder<S, Ops, Option<i32>> {
    rivet_serialization::record_builder::RecordCodecBuilder::of(
        Arc::new(|s: &S| s.min_clipped_height()),
        codec::optional_field(
            "min_clipped_height".to_string(),
            codec::int_range::<Ops>(0, 80),
            false,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn two_layers(limit: i32, lower_size: i32, upper_size: i32) -> Arc<dyn FeatureSize> {
        Arc::new(two_layers_feature_size::TwoLayersFeatureSize::new(
            limit, lower_size, upper_size,
        ))
    }

    fn three_layers(
        limit: i32,
        upper_limit: i32,
        lower_size: i32,
        middle_size: i32,
        upper_size: i32,
        min_clipped_height: Option<i32>,
    ) -> Arc<dyn FeatureSize> {
        Arc::new(three_layers_feature_size::ThreeLayersFeatureSize::new(
            limit,
            upper_limit,
            lower_size,
            middle_size,
            upper_size,
            min_clipped_height,
        ))
    }

    fn round_trip(size: Arc<dyn FeatureSize>) -> Arc<dyn FeatureSize> {
        let codec = feature_size_codec::<JsonOps>();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &size)
            .result()
            .expect("encode should succeed")
            .clone();
        codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone()
    }

    #[test]
    fn two_layers_dispatch_round_trip() {
        let size = two_layers(4, 1, 2);
        let codec = feature_size_codec::<JsonOps>();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &size)
            .result()
            .expect("encode should succeed")
            .clone();
        // Element-first encode order: the value fields write first, then the
        // "type" key (Java `KeyDispatchCodec.encode`). Map equality is
        // order-insensitive, so pin the actual key sequence.
        assert_eq!(
            encoded,
            json!({
                "limit": 4,
                "lower_size": 1,
                "upper_size": 2,
                "type": "minecraft:two_layers_feature_size",
            })
        );
        assert_eq!(
            encoded
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["limit", "lower_size", "upper_size", "type"]
        );
        let decoded = round_trip(size);
        assert_eq!(
            FeatureSize::type_id(&*decoded).name(),
            "minecraft:two_layers_feature_size"
        );
    }

    #[test]
    fn two_layers_defaults_decode() {
        // All five fields are optional with defaults (limit=1, lower_size=0,
        // upper_size=1); min_clipped_height is the non-lenient optional.
        let codec = feature_size_codec::<JsonOps>();
        let decoded = codec
            .parse(
                &JsonOps::INSTANCE,
                &json!({"type": "minecraft:two_layers_feature_size"}),
            )
            .result()
            .expect("decode should succeed")
            .clone();
        let two = downcast_erased::<two_layers_feature_size::TwoLayersFeatureSize>(&decoded);
        assert_eq!(two.limit, 1);
        assert_eq!(two.lower_size, 0);
        assert_eq!(two.upper_size, 1);
        assert_eq!(two.min_clipped_height, None);
    }

    #[test]
    fn two_layers_min_clipped_height_round_trips() {
        let size: Arc<dyn FeatureSize> = Arc::new(
            two_layers_feature_size::TwoLayersFeatureSize::new_with_min_clipped_height(
                4,
                1,
                2,
                Some(8),
            ),
        );
        let codec = feature_size_codec::<JsonOps>();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &size)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "limit": 4,
                "lower_size": 1,
                "upper_size": 2,
                "min_clipped_height": 8,
                "type": "minecraft:two_layers_feature_size",
            })
        );
        assert_eq!(
            encoded
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                "limit",
                "lower_size",
                "upper_size",
                "min_clipped_height",
                "type"
            ]
        );
        let decoded = round_trip(size);
        let two = downcast_erased::<two_layers_feature_size::TwoLayersFeatureSize>(&decoded);
        assert_eq!(two.min_clipped_height, Some(8));
    }

    #[test]
    fn three_layers_dispatch_round_trip() {
        let size = three_layers(4, 2, 1, 2, 3, None);
        let codec = feature_size_codec::<JsonOps>();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &size)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "limit": 4,
                "upper_limit": 2,
                "lower_size": 1,
                "middle_size": 2,
                "upper_size": 3,
                "type": "minecraft:three_layers_feature_size",
            })
        );
        assert_eq!(
            encoded
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                "limit",
                "upper_limit",
                "lower_size",
                "middle_size",
                "upper_size",
                "type"
            ]
        );
        let decoded = round_trip(size);
        assert_eq!(
            FeatureSize::type_id(&*decoded).name(),
            "minecraft:three_layers_feature_size"
        );
    }

    #[test]
    fn three_layers_defaults_decode() {
        let codec = feature_size_codec::<JsonOps>();
        let decoded = codec
            .parse(
                &JsonOps::INSTANCE,
                &json!({"type": "minecraft:three_layers_feature_size"}),
            )
            .result()
            .expect("decode should succeed")
            .clone();
        let three = downcast_erased::<three_layers_feature_size::ThreeLayersFeatureSize>(&decoded);
        assert_eq!(three.limit, 1);
        assert_eq!(three.upper_limit, 1);
        assert_eq!(three.lower_size, 0);
        assert_eq!(three.middle_size, 1);
        assert_eq!(three.upper_size, 1);
        assert_eq!(three.min_clipped_height, None);
    }

    #[test]
    fn two_layers_get_size_at_height_boundaries() {
        // `yo < limit ? lowerSize : upperSize`.
        let size = two_layers(4, 1, 2);
        assert_eq!(FeatureSize::get_size_at_height(&*size, 10, 0), 1);
        assert_eq!(FeatureSize::get_size_at_height(&*size, 10, 3), 1);
        assert_eq!(FeatureSize::get_size_at_height(&*size, 10, 4), 2);
        assert_eq!(FeatureSize::get_size_at_height(&*size, 10, 9), 2);
        // limit == 0: every yo is >= limit.
        let zero = two_layers(0, 5, 6);
        assert_eq!(FeatureSize::get_size_at_height(&*zero, 1, 0), 6);
    }

    #[test]
    fn three_layers_get_size_at_height_boundaries() {
        // `yo < limit ? lowerSize : (yo >= treeHeight - upperLimit ? upperSize
        // : middleSize)`.
        let size = three_layers(4, 2, 1, 2, 3, None);
        assert_eq!(FeatureSize::get_size_at_height(&*size, 10, 0), 1);
        assert_eq!(FeatureSize::get_size_at_height(&*size, 10, 3), 1);
        // treeHeight - upperLimit = 8: yo >= 8 → upperSize.
        assert_eq!(FeatureSize::get_size_at_height(&*size, 10, 4), 2);
        assert_eq!(FeatureSize::get_size_at_height(&*size, 10, 7), 2);
        assert_eq!(FeatureSize::get_size_at_height(&*size, 10, 8), 3);
        assert_eq!(FeatureSize::get_size_at_height(&*size, 10, 9), 3);
    }

    #[test]
    fn dispatch_rejects_missing_type() {
        let codec = feature_size_codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!({"limit": 4}));
        assert!(result.is_error());
    }

    #[test]
    fn dispatch_rejects_unknown_type() {
        let codec = feature_size_codec::<JsonOps>();
        let result = codec.parse(
            &JsonOps::INSTANCE,
            &json!({"type": "minecraft:one_layer_feature_size", "limit": 4}),
        );
        assert!(result.is_error());
    }

    #[test]
    fn defaulted_fields_fall_back_on_out_of_range_present_values() {
        // The five defaulted fields are `optionalFieldOf(name, default)` =
        // lenient optional with default: a present-but-invalid value silently
        // falls back to the default (DFU's `optionalField(name, codec, true)`).
        let codec = feature_size_codec::<JsonOps>();
        // two_layers limit is [0, 81]; 82 is out of range → decodes to default 1.
        let decoded = codec
            .parse(
                &JsonOps::INSTANCE,
                &json!({"type": "minecraft:two_layers_feature_size", "limit": 82}),
            )
            .result()
            .expect("lenient optional swallows the out-of-range value")
            .clone();
        let two = downcast_erased::<two_layers_feature_size::TwoLayersFeatureSize>(&decoded);
        assert_eq!(two.limit, 1);
        // three_layers upper_size is [0, 16]; -1 is out of range → default 1.
        let decoded = codec
            .parse(
                &JsonOps::INSTANCE,
                &json!({"type": "minecraft:three_layers_feature_size", "upper_size": -1}),
            )
            .result()
            .expect("lenient optional swallows the out-of-range value")
            .clone();
        let three = downcast_erased::<three_layers_feature_size::ThreeLayersFeatureSize>(&decoded);
        assert_eq!(three.upper_size, 1);
    }

    #[test]
    fn non_lenient_min_clipped_height_rejects_out_of_range() {
        // min_clipped_height is the non-lenient optional (`optionalFieldOf(String)`
        // without a default): a present-but-invalid value is an error, not a
        // fallback.
        let codec = feature_size_codec::<JsonOps>();
        assert!(
            codec
                .parse(
                    &JsonOps::INSTANCE,
                    &json!({
                        "type": "minecraft:two_layers_feature_size",
                        "min_clipped_height": 81,
                    })
                )
                .is_error()
        );
    }

    #[test]
    fn dispatch_rejects_out_of_range_on_encode() {
        let codec = feature_size_codec::<JsonOps>();
        let bad = two_layers(82, 0, 1);
        assert!(
            codec
                .encode_start(&JsonOps::INSTANCE, &bad)
                .result()
                .is_none()
        );
    }
}
