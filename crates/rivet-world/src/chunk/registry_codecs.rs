//! Block/fluid by-name codecs (#370) — the `BuiltInRegistries.BLOCK/FLUID
//! .byNameCodec()` surface the stored-tick codecs decode through.
//!
//! Paper's `byNameCodec()` is
//! `Identifier.CODEC.comapFlatMap(name -> this.get(name).map(DataResult::success)
//! .orElseGet(() -> DataResult.error(() -> "Unknown registry key in " + key +
//! ": " + name)), holder -> holder.key().identifier())`, wrapped in
//! `ExtraCodecs.overrideLifecycle`. The generated tables make every lookup a
//! pure name→id resolution, so this port builds the same decode shape directly
//! over the `Identifier` codec (the lifecycle override is a registry-instance
//! concern the id-handles don't carry).
//!
//! Encode (`flatComapMap`) is the registry key: `BlockId.name()` is the
//! canonical `"namespace:path"`. The blocks registry is **not** defaulted, so
//! `BLOCK`'s unknown-name error is a hard `DataResult::error`; the fluids
//! registry **is** defaulted (`minecraft:empty`), but `byNameCodec()` still
//! errors on an unknown name — the defaulted `getValue(Identifier)` fallback is
//! only reached through other surfaces, never through the codec. So both
//! by-name codecs share the same strict unknown-id error.
//!
//! `UpgradeData` wraps these with `.orElse(Blocks.AIR)` /
//! `.orElse(Fluids.EMPTY)`; the top-level `SerializableChunkData` codecs do
//! not.

use rivet_registry::Identifier;
use rivet_registry::fluid_id::FluidId;
use rivet_registry::generated::blocks::BlockId;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use std::sync::Arc;

use crate::block::Block;

/// `BuiltInRegistries.BLOCK.byNameCodec()` — the strict block id-handle codec.
///
/// Unknown names (a valid identifier that is not a registered block) error
/// with Paper's exact message `Unknown registry key in
/// ResourceKey[minecraft:root / minecraft:block]: <id>`. A malformed
/// identifier errors through `Identifier::read` before the registry lookup,
/// exactly like Paper's `comapFlatMap` chain.
pub fn block_by_name_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Block, Ops>> {
    codec::comap_flat_map(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|identifier: &Identifier| {
            match BlockId::from_name(&identifier.to_string()).map(Block::new) {
                Some(block) => DataResult::success(block),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:block]: {identifier}"
                )),
            }
        }),
        Arc::new(|block: &Block| Identifier::parse(block.name())),
    )
}

/// `BuiltInRegistries.FLUID.byNameCodec()` — the strict fluid id-handle codec.
///
/// The fluid registry is defaulted, but the by-name codec still errors on an
/// unknown name (Java's `byNameCodec` uses `get(Identifier)`, not
/// `getValue(Identifier)`), so the message mirrors the block one with the
/// `minecraft:fluid` registry key.
pub fn fluid_by_name_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<FluidId, Ops>> {
    codec::comap_flat_map(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(
            |identifier: &Identifier| match FluidId::from_name(&identifier.to_string()) {
                Some(fluid) => DataResult::success(fluid),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:fluid]: {identifier}"
                )),
            },
        ),
        Arc::new(|fluid: &FluidId| Identifier::parse(fluid.name())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn block_by_name_roundtrips_and_errors() {
        let codec = block_by_name_codec::<JsonOps>();
        let stone = Block::from_name("minecraft:stone").unwrap();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &stone)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(encoded, json!("minecraft:stone"));
        assert_eq!(
            codec
                .parse(&JsonOps::INSTANCE, &json!("minecraft:stone"))
                .result(),
            Some(&stone)
        );
        // Unqualified identifiers normalize through Identifier before lookup.
        assert_eq!(
            codec.parse(&JsonOps::INSTANCE, &json!("stone")).result(),
            Some(&stone)
        );
        // Unknown block errors with Paper's exact message.
        let err = codec.parse(&JsonOps::INSTANCE, &json!("minecraft:not_a_block"));
        assert!(err.is_error());
        assert!(err.error_ref().unwrap().message().contains(
            "Unknown registry key in ResourceKey[minecraft:root / minecraft:block]: minecraft:not_a_block"
        ));
        // Malformed identifiers error through Identifier.read.
        assert!(
            codec
                .parse(&JsonOps::INSTANCE, &json!("not valid"))
                .is_error()
        );
    }

    #[test]
    fn fluid_by_name_roundtrips_and_errors() {
        let codec = fluid_by_name_codec::<JsonOps>();
        let water = FluidId::from_name("minecraft:water").unwrap();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &water)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(encoded, json!("minecraft:water"));
        assert_eq!(
            codec
                .parse(&JsonOps::INSTANCE, &json!("minecraft:water"))
                .result(),
            Some(&water)
        );
        // Unknown fluid errors with the fluid registry key (still strict — the
        // defaulted fallback is not the byNameCodec surface).
        let err = codec.parse(&JsonOps::INSTANCE, &json!("minecraft:not_a_fluid"));
        assert!(err.is_error());
        assert!(err.error_ref().unwrap().message().contains(
            "Unknown registry key in ResourceKey[minecraft:root / minecraft:fluid]: minecraft:not_a_fluid"
        ));
    }

    /// `UpgradeData`'s `.orElse(Blocks.AIR)` wrapper: unknown names decode to
    /// air (a valid tick) — the exact Java asymmetry this slice preserves.
    #[test]
    fn block_by_name_or_else_air_recovers_unknown() {
        let strict = block_by_name_codec::<JsonOps>();
        let codec = codec::or_else_value(strict, Block::from_name("minecraft:air").unwrap());
        let decoded = codec.parse(&JsonOps::INSTANCE, &json!("minecraft:not_a_block"));
        assert_eq!(
            decoded.result(),
            Some(&Block::from_name("minecraft:air").unwrap())
        );
    }
}
