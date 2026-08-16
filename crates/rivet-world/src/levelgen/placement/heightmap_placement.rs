//! Port of `net.minecraft.world.level.levelgen.placement.HeightmapPlacement`
//! (class, 26.2).
//!
//! Java: a modifier holding a `Heightmap.Types` whose `getPositions` relocates
//! the origin to the heightmap's height at its X/Z, dropping the position when
//! that height is at or below the world min Y — `height > context.getMinY() ?
//! Stream.of(new BlockPos(x, height, z)) : Stream.of()`. Its `CODEC` is the
//! `"heightmap"` field (`Heightmap.Types.CODEC`), and its `type()` is
//! `PlacementModifierType.HEIGHTMAP`.
//!
//! The `getHeight` read is the `#228`-deferred `PlacementContext.getHeight`
//! (which panics explicitly — see `placement_context.rs`); `Heightmap.Types.
//! CODEC` is reused from the heightmap module's proactive port.

use crate::levelgen::heightmap::Types;
use crate::levelgen::placement::placement_modifier_type::PlacementModifierTypes;
use crate::levelgen::placement::{PlacementContext, PlacementModifier};
use rivet_registry::core::BlockPos;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.HeightmapPlacement`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeightmapPlacement {
    /// `this.heightmap` — the `Heightmap.Types` height read per origin.
    heightmap: Types,
}

impl HeightmapPlacement {
    /// `onHeightmap(Heightmap.Types)` — the public factory.
    pub fn on_heightmap(heightmap: Types) -> Self {
        HeightmapPlacement { heightmap }
    }
}

impl PlacementModifier for HeightmapPlacement {
    fn get_positions<R: RandomSource>(
        &self,
        context: &PlacementContext,
        _random: &mut R,
        origin: &BlockPos,
    ) -> Vec<BlockPos> {
        let x = origin.get_x();
        let z = origin.get_z();
        let height = context.get_height(self.heightmap, x, z);
        if height > context.get_min_y() {
            vec![BlockPos::new(x, height, z)]
        } else {
            Vec::new()
        }
    }

    fn type_id(
        &self,
    ) -> crate::levelgen::placement::placement_modifier_type::PlacementModifierTypeId {
        // `PlacementModifierType.HEIGHTMAP` is insertion index 10 in
        // `PlacementModifierType.java`'s registration order.
        PlacementModifierTypes::HEIGHTMAP
    }
}

/// `HeightmapPlacement.CODEC` — `Heightmap.Types.CODEC.fieldOf("heightmap")`,
/// as the ops-generic `heightmap_placement_map_codec::<Ops>()` factory.
pub fn heightmap_placement_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<HeightmapPlacement, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|c: &HeightmapPlacement| c.heightmap),
                "heightmap".to_string(),
                Arc::new(crate::levelgen::heightmap::types_codec::<Ops>()),
            ))
            .apply(
                instance,
                Arc::new(|heightmap: Types| HeightmapPlacement::on_heightmap(heightmap)),
            )
    })
}

/// `HeightmapPlacement.CODEC` as a `Codec` (`MapCodec.codec()`), the shape the
/// `#181` generated dispatch's registration table consumes.
#[allow(dead_code)]
pub fn heightmap_placement_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<HeightmapPlacement, Ops>> {
    map_codec::codec_of(heightmap_placement_map_codec::<Ops>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn type_identity_is_reported() {
        // `PlacementModifierType.HEIGHTMAP` is insertion index 10.
        let modifier = HeightmapPlacement::on_heightmap(Types::MotionBlocking);
        assert_eq!(modifier.type_id(), PlacementModifierTypes::HEIGHTMAP);
    }

    #[test]
    fn codec_round_trips_the_heightmap_type() {
        let ops = JsonOps::INSTANCE;
        let codec = heightmap_placement_codec::<JsonOps>();
        let modifier = HeightmapPlacement::on_heightmap(Types::MotionBlocking);
        let encoded = codec
            .encode_start(&ops, &modifier)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"heightmap": "MOTION_BLOCKING"}));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, modifier);
    }

    #[test]
    fn codec_rejects_an_unknown_heightmap_type() {
        let ops = JsonOps::INSTANCE;
        let codec = heightmap_placement_codec::<JsonOps>();
        let result = codec.parse(&ops, &json!({"heightmap": "NOT_A_HEIGHTMAP"}));
        assert!(result.is_error());
    }

    #[test]
    fn codec_missing_heightmap_field_errors() {
        let ops = JsonOps::INSTANCE;
        let codec = heightmap_placement_codec::<JsonOps>();
        let result = codec.parse(&ops, &json!({}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.contains("No key heightmap"), "got: {msg}");
    }
}
