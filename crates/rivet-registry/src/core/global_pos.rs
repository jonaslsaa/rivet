//! `net.minecraft.core.GlobalPos` — a `(ResourceKey<Level>, BlockPos)` record.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/core/GlobalPos.java`.
//! Ported as a record-like value type: `of`, the `dimension`/`pos` record
//! accessors, `toString` (`dimension + " " + pos`) and `isCloseEnough` mirror
//! the Java record; `PartialEq`/`Eq`/`Hash`/`Clone` are derived over the two
//! components (Java record value semantics — `ResourceKey` carries value
//! semantics per the #107 binding, see `resource_key.rs`).
//!
//! The `dimension` type is `ResourceKey<Level>` using the world-unit
//! placeholder `crate::registries::Level` — the same type `Registries::DIMENSION`
//! and `Registries::level_stem_to_level` use, so a `GlobalPos` can be built
//! against a registry key directly.
//!
//! `MAP_CODEC`/`CODEC` landed here (the `Level.RESOURCE_KEY_CODEC` the Java
//! `GlobalPos` uses is `ResourceKey.codec(Registries.DIMENSION)`, ported in
//! `resource_key.rs`; the map-codec surface is rivet-serialization).
//! RivetTodo(#126): `STREAM_CODEC` (`ResourceKey.streamCodec(
//! Registries.DIMENSION)` + `BlockPos.STREAM_CODEC`) still defers with the
//! protocol codec surface (#126).

use super::block_pos::{BlockPos, block_pos_codec};
use crate::ResourceKey;
use crate::registries::{DIMENSION, Level};
use crate::resource_key::resource_key_codec;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{MapCodec, codec_of};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.core.GlobalPos` — a `(ResourceKey<Level>, BlockPos)` record.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GlobalPos {
    dimension: ResourceKey<Level>,
    pos: BlockPos,
}

impl GlobalPos {
    /// `GlobalPos.of(ResourceKey<Level>, BlockPos)`.
    pub fn of(dimension: ResourceKey<Level>, pos: BlockPos) -> Self {
        Self { dimension, pos }
    }

    /// `GlobalPos.dimension()`.
    pub fn dimension(&self) -> &ResourceKey<Level> {
        &self.dimension
    }

    /// `GlobalPos.pos()`.
    pub fn pos(&self) -> BlockPos {
        self.pos
    }

    /// `GlobalPos.isCloseEnough(dimension, pos, maxDistance)` — the dimensions
    /// match and `pos.distChessboard(pos) <= maxDistance`.
    pub fn is_close_enough(
        &self,
        dimension: &ResourceKey<Level>,
        pos: &BlockPos,
        max_distance: i32,
    ) -> bool {
        self.dimension == *dimension && self.pos.dist_chessboard(pos) <= max_distance
    }
}

impl std::fmt::Display for GlobalPos {
    /// `GlobalPos.toString()` — `dimension + " " + pos`. `ResourceKey`'s
    /// `Display` is `"ResourceKey[registry / identifier]"` and `BlockPos`'s is
    /// `"BlockPos{x=…, y=…, z=…}"`, matching Java's string concatenation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.dimension, self.pos)
    }
}

/// `GlobalPos.MAP_CODEC` — `RecordCodecBuilder.mapCodec(i -> i.group(
/// Level.RESOURCE_KEY_CODEC.fieldOf("dimension").forGetter(...),
/// BlockPos.CODEC.fieldOf("pos").forGetter(...)).apply(i, GlobalPos::of))`.
///
/// The `dimension` field codec is `ResourceKey.codec(Registries.DIMENSION)` —
/// the `Level.RESOURCE_KEY_CODEC` the Java `GlobalPos` uses (Paper wires it to
/// the `Registries.DIMENSION` registry key). Exposed as the ops-generic
/// `global_pos_map_codec::<Ops>()` factory (Java's `static final` constant).
pub fn global_pos_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<GlobalPos, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|g: &GlobalPos| g.dimension.clone()),
                "dimension".to_string(),
                resource_key_codec::<Level, Ops>(&DIMENSION),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|g: &GlobalPos| g.pos),
                "pos".to_string(),
                block_pos_codec::<Ops>(),
            ))
            .apply(instance, Arc::new(GlobalPos::of))
    })
}

/// `GlobalPos.CODEC` — `MAP_CODEC.codec()`.
pub fn global_pos_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<GlobalPos, Ops>> {
    codec_of(global_pos_map_codec::<Ops>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Identifier;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn overworld() -> ResourceKey<Level> {
        ResourceKey::create(&DIMENSION, Identifier::with_default_namespace("overworld"))
    }

    #[test]
    fn global_pos_map_codec_round_trips() {
        let ops = JsonOps::INSTANCE;
        let codec = global_pos_codec::<JsonOps>();
        let gp = GlobalPos::of(overworld(), BlockPos::new(1, -60, 3));
        // Encode to the Java wire shape: {"dimension":"minecraft:overworld","pos":[1,-60,3]}.
        let encoded = codec.encode_start(&ops, &gp).get_or_throw("encode").clone();
        assert_eq!(
            encoded,
            json!({"dimension": "minecraft:overworld", "pos": [1, -60, 3]})
        );
        // Decode the same shape back.
        let input = json!({"dimension": "minecraft:overworld", "pos": [1, -60, 3]});
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(decoded.0, gp);
    }

    #[test]
    fn global_pos_codec_rejects_wrong_pos_size() {
        let ops = JsonOps::INSTANCE;
        let codec = global_pos_codec::<JsonOps>();
        // `Util.fixedSize(input, 3)` errors on a 2-element int array.
        let input = json!({"dimension": "minecraft:overworld", "pos": [1, 2]});
        assert!(
            codec.decode(&ops, &input).result().is_none(),
            "a 2-element pos must fail the fixed-size check"
        );
    }
}
