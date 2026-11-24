//! Port of `net.minecraft.world.item` (`ItemStack`, `ItemStackTemplate`,
//! `ItemInstance`, `Item` identity/registry seam) and
//! `net.minecraft.core.component` (`DataComponentType`, `DataComponentMap`,
//! `DataComponentPatch`, `PatchedDataComponentMap`).
//!
//! All vanilla item and data component identities are backed by the generated
//! registry tables in `rivet_registry::generated::registries`.

pub mod component_map;
pub mod component_patch;
pub mod component_type;
pub mod components;
pub mod item_id;
pub mod item_stack;
pub mod recursive_components;
#[cfg(test)]
mod tests;

pub use component_map::{
    DataComponentGetter, DataComponentHolder, DataComponentMap, DataComponentMapBuilder,
    data_component_map_codec,
};
pub use component_patch::{
    DataComponentPatch, DataComponentPatchBuilder, PatchKey, PatchedDataComponentMap,
    REMOVED_PREFIX, SplitResult, data_component_patch_codec,
};
pub use component_type::{
    DataComponentTypeId, DataComponentValue, Rarity, TypedDataComponent, decode_component_value,
    encode_component_value,
};
pub use item_id::{
    ABSOLUTE_MAX_STACK_SIZE, ABSOLUTE_MAX_SWING_DURATION, DEFAULT_MAX_STACK_SIZE, ItemHolder,
    ItemId, ItemLike, ItemProperties, MAX_BAR_WIDTH, common_item_components,
    default_components_for, item_codec,
};
pub use item_stack::{
    ItemInstance, ItemStack, ItemStackTypeAndComponentsKey, item_stack_codec,
    item_stack_optional_codec, validate_components,
};
pub use recursive_components::{
    BUNDLE_MAX_ITEMS, BundleContents, CHARGED_PROJECTILES_MAX, CONTAINER_MAX_SIZE,
    ChargedProjectiles, Fraction, ItemContainerContents, ItemStackTemplate, bundle_contents_codec,
    charged_projectiles_codec, item_container_contents_codec, item_stack_template_codec,
};

use rivet_serialization::{Codec, DataResult, Decoder, DynamicOps, Encoder};
use std::sync::Arc;

pub(crate) fn data_result_into<T>(res: DataResult<T>) -> Result<T, String> {
    if let Some(err) = res.error_ref() {
        return Err(err.message().to_string());
    }
    match res.result_or_partial_silent() {
        Some(v) => Ok(v),
        None => Err("unknown DataResult error".to_string()),
    }
}

pub(crate) fn codec_from_fns<A, Ops, E, D>(
    name: &'static str,
    encode: E,
    decode: D,
) -> Arc<dyn Codec<A, Ops>>
where
    A: 'static,
    Ops: DynamicOps + 'static,
    E: Fn(&A, &Ops) -> DataResult<Ops::Output> + Send + Sync + 'static,
    D: Fn(&Ops, &Ops::Output) -> DataResult<A> + Send + Sync + 'static,
{
    struct FnCodec<A, Ops, E, D> {
        name: &'static str,
        encode: E,
        decode: D,
        _marker: std::marker::PhantomData<fn(A, Ops)>,
    }

    impl<A, Ops, E, D> std::fmt::Debug for FnCodec<A, Ops, E, D> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.name)
        }
    }

    impl<A, Ops, E, D> Encoder<A, Ops> for FnCodec<A, Ops, E, D>
    where
        A: 'static,
        Ops: DynamicOps + 'static,
        E: Fn(&A, &Ops) -> DataResult<Ops::Output> + Send + Sync + 'static,
        D: Fn(&Ops, &Ops::Output) -> DataResult<A> + Send + Sync + 'static,
    {
        fn encode(&self, input: &A, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
            (self.encode)(input, ops).flat_map(|v| ops.merge_to_primitive(prefix, v))
        }
    }

    impl<A, Ops, E, D> Decoder<A, Ops> for FnCodec<A, Ops, E, D>
    where
        A: 'static,
        Ops: DynamicOps + 'static,
        E: Fn(&A, &Ops) -> DataResult<Ops::Output> + Send + Sync + 'static,
        D: Fn(&Ops, &Ops::Output) -> DataResult<A> + Send + Sync + 'static,
    {
        fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(A, Ops::Output)> {
            (self.decode)(ops, input).map_owned(|v| (v, ops.empty()))
        }
    }

    impl<A, Ops, E, D> Codec<A, Ops> for FnCodec<A, Ops, E, D>
    where
        A: 'static,
        Ops: DynamicOps + 'static,
        E: Fn(&A, &Ops) -> DataResult<Ops::Output> + Send + Sync + 'static,
        D: Fn(&Ops, &Ops::Output) -> DataResult<A> + Send + Sync + 'static,
    {
    }

    Arc::new(FnCodec {
        name,
        encode,
        decode,
        _marker: std::marker::PhantomData,
    })
}
