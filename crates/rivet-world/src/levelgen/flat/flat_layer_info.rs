//! Port of `net.minecraft.world.level.levelgen.flat.FlatLayerInfo` (class, 26.2).
//!
//! Java: a two-field class (`Holder<Block> block, int height`) whose `CODEC`
//! is a `RecordCodecBuilder` over the required `"height"` field
//! (`Codec.intRange(0, DimensionType.Y_SIZE)` — the #388 `Y_SIZE` constant)
//! and the required `"block"` field (`BuiltInRegistries.BLOCK.holderByNameCodec()`,
//! the identifier↔block-reference codec). `getBlockState()` resolves the block
//! holder's default state, `heightLimited(maxHeight)` clamps the height, and
//! `toString()` renders `(height != 1 ? height + "*" : "") + block.getRegisteredName()`.
//!
//! The block is the id-handle placeholder [`BlockType`]; its holder is a
//! `Reference` whose element id is the block's registry id (== the generated
//! `BlockId`, OWNERSHIP's id model), so `getBlockState()` maps the reference's
//! id straight to [`BlockState::of`] and `getRegisteredName()` to the generated
//! `BlockId::name()` table — the id-model analogue of `block.value()
//! .defaultBlockState()` / `block.getRegisteredName()`. A `Direct` holder
//! carries the id-less [`BlockType`] placeholder (Java's `Direct` holds the
//! real `Block` value), so it cannot resolve a state — the port analogue of
//! Java's unbound `Reference`, which throws on `value()`.

use crate::level::dimension::Y_SIZE;
use rivet_registry::block_state::BlockState;
use rivet_registry::holder::{Holder, RegistryId};
use rivet_registry::registries::BlockType;
use rivet_registry::registry_file_codec::RegistryFixedCodec;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.flat.FlatLayerInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatLayerInfo {
    /// `this.block` — the layer's block holder.
    block: Holder<BlockType>,
    /// `this.height` — the layer thickness in `[0, DimensionType.Y_SIZE]`.
    height: i32,
}

impl FlatLayerInfo {
    /// `new FlatLayerInfo(int, Holder<Block>)` — the holder form of the codec's
    /// constructor.
    pub fn new(height: i32, block: Holder<BlockType>) -> Self {
        FlatLayerInfo { block, height }
    }

    /// `new FlatLayerInfo(int, Block)` — `block.builtInRegistryHolder()`.
    ///
    /// The caller supplies the block registry's real `RegistryId` (resolved from
    /// the same `RegistryAccess`/`BootstrapContext` the holder will serialize
    /// in): the `RegistryId` is a per-instance identity assigned by the global
    /// counter, so a hardcoded id would be silently rejected by the block
    /// fixed codec. The element id is the block's generated `BlockId`.
    pub fn from_block(height: i32, block: crate::block::Block, block_registry: RegistryId) -> Self {
        FlatLayerInfo::new(
            height,
            Holder::reference(block_registry, block.id().0 as u32),
        )
    }

    /// `getHeight()`.
    pub fn get_height(&self) -> i32 {
        self.height
    }

    /// `getBlockState()` — `this.block.value().defaultBlockState()`. The block
    /// holder's element id is the block's registry id, so the reference resolves
    /// to `BlockState::of` of that id.
    ///
    /// A `Direct` holder carries the id-less [`BlockType`] placeholder. Java's
    /// `Direct` holds the real `Block` value and resolves via `value()`; the
    /// placeholder has no id to resolve, so the panic is a deliberate deviation
    /// (Java would return the direct block's state). Unreachable from the fixed
    /// block-registry codec, which only produces `Reference`s.
    pub fn get_block_state(&self) -> BlockState {
        match &self.block {
            Holder::Reference { id, .. } => {
                crate::block::Block::new(rivet_registry::generated::blocks::BlockId(*id as u16))
                    .default_block_state()
            }
            Holder::Direct(_) => panic!("Direct block holder has no block id to resolve"),
        }
    }

    /// `heightLimited(int maxHeight)` — `height > maxHeight ? new
    /// FlatLayerInfo(maxHeight, block) : this`.
    pub fn height_limited(&self, max_height: i32) -> FlatLayerInfo {
        if self.height > max_height {
            FlatLayerInfo::new(max_height, self.block.clone())
        } else {
            self.clone()
        }
    }

    /// `toString()` — `(height != 1 ? height + "*" : "") +
    /// block.getRegisteredName()`. The name resolves the reference's element id
    /// through the generated block-name table (Java's `getRegisteredName` reads
    /// the reference's stored key — the id-model equivalent).
    pub fn to_string_flat(&self) -> String {
        let name = self.registered_name();
        if self.height != 1 {
            format!("{}*{}", self.height, name)
        } else {
            name.to_string()
        }
    }

    /// The block's registered name (Java `Holder.getRegisteredName()`).
    fn registered_name(&self) -> &'static str {
        match &self.block {
            Holder::Reference { id, .. } => {
                crate::block::Block::new(rivet_registry::generated::blocks::BlockId(*id as u16))
                    .name()
            }
            Holder::Direct(_) => "[unregistered]",
        }
    }
}

impl fmt::Display for FlatLayerInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_flat())
    }
}

/// `FlatLayerInfo.CODEC` — a record codec over the required `"height"` field
/// (int-range `[0, Y_SIZE]`) and the required `"block"` field (the block
/// registry's fixed reference codec — the id-model analogue of
/// `BuiltInRegistries.BLOCK.holderByNameCodec()`), as the ops-generic
/// `flat_layer_info_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     Codec.intRange(0, DimensionType.Y_SIZE).fieldOf("height"),
///     BuiltInRegistries.BLOCK.holderByNameCodec().fieldOf("block"))
///     .apply(i, FlatLayerInfo::new))
/// ```
pub fn flat_layer_info_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<FlatLayerInfo, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|l: &FlatLayerInfo| l.height),
                "height".to_string(),
                codec::int_range::<Ops>(0, Y_SIZE),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|l: &FlatLayerInfo| l.block.clone()),
                codec::field_of(
                    Arc::new(RegistryFixedCodec::create(
                        &rivet_registry::registries::BLOCK,
                    )),
                    "block".to_string(),
                ),
            ))
            .apply(
                instance,
                Arc::new(|height: i32, block: Holder<BlockType>| FlatLayerInfo::new(height, block)),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::Identifier;
    use rivet_registry::ResourceKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::root::AnyBox;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A block registry with `minecraft:air` (id 0), `minecraft:stone` (id 1),
    /// and `minecraft:dirt` (id 2), wrapped in a `RegistryAccess` under
    /// `Registries.BLOCK`. The air-first registration order makes each decoded
    /// element id coincide with its generated `BlockId` (air=0, stone=1), so the
    /// test registry is coherent with `get_block_state`/`registered_name`, which
    /// resolve element ids through the generated table.
    fn block_access() -> RegistryAccess {
        let mut builder = RegistryBuilder::new(&*rivet_registry::registries::BLOCK);
        for name in ["minecraft:air", "minecraft:stone", "minecraft:dirt"] {
            builder.register(
                &ResourceKey::create(&*rivet_registry::registries::BLOCK, Identifier::parse(name)),
                Arc::new(BlockType),
                RegistrationInfo::BUILT_IN,
            );
        }
        let registry = builder.freeze();
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("block")),
            Box::new(registry) as AnyBox,
        )])
    }

    /// The block registry's real `RegistryId` (assigned by the global counter) —
    /// the reference-id the hand-built holders carry, even in tests that never
    /// serialize (mirroring the `from_block` contract).
    fn block_registry_id(access: &RegistryAccess) -> RegistryId {
        RegistryAccess::lookup(access, &*rivet_registry::registries::BLOCK)
            .expect("block registry")
            .registry_id()
    }

    #[test]
    fn codec_round_trips_height_and_block_reference() {
        let access = block_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone());
        let codec = flat_layer_info_codec::<TestOps>();
        // Decode: `"block": "minecraft:stone"` resolves through the access to a
        // `Reference` carrying the *real* block-registry id (per-instance, from
        // the global counter) and stone's insertion index (1 — air is 0), which
        // coincides with stone's generated `BlockId` so `get_block_state`
        // resolves it coherently.
        let parsed = codec.parse(&ops, &json!({ "height": 2, "block": "minecraft:stone" }));
        let decoded = parsed.result().expect("decode should succeed");
        assert_eq!(decoded.get_height(), 2);
        assert_eq!(
            decoded.block,
            Holder::reference(block_registry_id(&access), 1)
        );
        // Encode round-trips the reference back to its identifier.
        let encoded = codec
            .encode_start(&ops, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({ "height": 2, "block": "minecraft:stone" }));
        // The decoded reference's element id (1) is stone's generated `BlockId`,
        // so `get_block_state` resolves it to the stone default state.
        assert_eq!(decoded.get_block_state().block().id() as u32, 1);
    }

    #[test]
    fn get_block_state_resolves_the_reference_to_a_generated_state() {
        // The reference element id is the vanilla block id: stone = 1 in the
        // generated block registry (not the test registry's insertion index).
        let registry_id = block_registry_id(&block_access());
        let stone = FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::STONE, registry_id);
        let state = stone.get_block_state();
        assert_eq!(state.block().id() as u32, 1);
    }

    #[test]
    fn height_limited_clamps_only_when_above_the_max() {
        let registry_id = block_registry_id(&block_access());
        let layer = FlatLayerInfo::from_block(5, crate::block::blocks::Blocks::STONE, registry_id);
        let clamped = layer.height_limited(3);
        assert_eq!(clamped.get_height(), 3);
        assert_eq!(clamped.block, layer.block);
        let kept = layer.height_limited(6);
        assert_eq!(kept.get_height(), 5);
    }

    #[test]
    fn to_string_matches_the_java_format() {
        let registry_id = block_registry_id(&block_access());
        assert_eq!(
            FlatLayerInfo::from_block(2, crate::block::blocks::Blocks::STONE, registry_id)
                .to_string(),
            "2*minecraft:stone"
        );
        assert_eq!(
            FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::BEDROCK, registry_id)
                .to_string(),
            "minecraft:bedrock"
        );
    }
}
