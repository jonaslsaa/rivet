//! Structural placement codecs. Named exclusion references are resolved and
//! validated against the completed structure-set registry by its bootstrap.
use super::*;
use rivet_registry::{
    Identifier,
    biome_id::BiomeId,
    core::{BlockPos, block_pos_codec},
    holder::Holder,
    holder_set::HolderSet,
    identifier::identifier_codec,
    registry_file_codec::{HolderSetCodec, RegistryFixedCodec},
    registry_ops::RegistryOpsLookup,
};
use rivet_serialization::{
    codec::{self, Codec},
    data_result::DataResult,
    dynamic_ops::DynamicOps,
    key_dispatch_codec,
    map_codec::{self, MapCodec},
    record_builder::{self, RecordCodecBuilder},
};
use std::sync::Arc;

fn default_field<T: Clone + PartialEq + Send + Sync + 'static, Ops: DynamicOps + 'static>(
    name: &str,
    codec: Arc<dyn Codec<T, Ops>>,
    default: T,
) -> Arc<dyn MapCodec<T, Ops>> {
    let decode = default.clone();
    map_codec::xmap(
        codec::optional_field(name.to_string(), codec, false),
        Arc::new(move |v: &Option<T>| v.clone().unwrap_or_else(|| decode.clone())),
        Arc::new(move |v: &T| if *v == default { None } else { Some(v.clone()) }),
    )
}
fn frequency_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<FrequencyReductionMethod, Ops>> {
    codec::comap_flat_map(
        codec::string_codec(),
        Arc::new(|name: &String| {
            FrequencyReductionMethod::from_name(name).map_or_else(
                || DataResult::error(format!("unknown frequency reduction method {name}")),
                DataResult::success,
            )
        }),
        Arc::new(|method: &FrequencyReductionMethod| method.name().to_string()),
    )
}
fn spread_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<RandomSpreadType, Ops>> {
    codec::comap_flat_map(
        codec::string_codec(),
        Arc::new(|name: &String| match name.as_str() {
            "linear" => DataResult::success(RandomSpreadType::Linear),
            "triangular" => DataResult::success(RandomSpreadType::Triangular),
            _ => DataResult::error(format!("unknown random spread type {name}")),
        }),
        Arc::new(|kind: &RandomSpreadType| {
            match kind {
                RandomSpreadType::Linear => "linear",
                RandomSpreadType::Triangular => "triangular",
            }
            .to_string()
        }),
    )
}
fn exclusion_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ExclusionZone, Ops>> {
    record_builder::create(|i| {
        i.group(RecordCodecBuilder::of(
            Arc::new(|p: &ExclusionZone| p.other_set.clone()),
            codec::field_of(identifier_codec(), "other_set".into()),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &ExclusionZone| p.chunk_count),
            codec::field_of(codec::int_range(1, 16), "chunk_count".into()),
        ))
        .apply(
            i,
            Arc::new(|other_set: Identifier, chunk_count| ExclusionZone {
                other_set,
                chunk_count,
            }),
        )
    })
}
fn settings_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<StructurePlacementSettings, Ops>>
{
    let offset = codec::validate(
        block_pos_codec(),
        Arc::new(|pos: &BlockPos| {
            if [pos.get_x(), pos.get_y(), pos.get_z()]
                .iter()
                .all(|v| v.wrapping_abs() < 16)
            {
                DataResult::success(*pos)
            } else {
                DataResult::error(format!(
                    "Position out of range, expected at most 16: {pos:?}"
                ))
            }
        }),
    );
    record_builder::map_codec(|i| {
        i.group(RecordCodecBuilder::of(
            Arc::new(|p: &StructurePlacementSettings| p.locate_offset),
            default_field("locate_offset", offset, BlockPos::ZERO),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &StructurePlacementSettings| p.frequency_reduction_method),
            default_field(
                "frequency_reduction_method",
                frequency_codec(),
                FrequencyReductionMethod::Default,
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &StructurePlacementSettings| p.frequency),
            codec::optional_field_of("frequency", codec::float_range(0.0, 1.0), 1.0),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &StructurePlacementSettings| p.salt),
            codec::field_of(codec::int_range(0, i32::MAX), "salt".into()),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &StructurePlacementSettings| p.exclusion_zone.clone()),
            codec::optional_field("exclusion_zone".into(), exclusion_codec(), false),
        ))
        .apply(
            i,
            Arc::new(
                |locate_offset, frequency_reduction_method, frequency, salt, exclusion_zone| {
                    StructurePlacementSettings {
                        locate_offset,
                        frequency_reduction_method,
                        frequency,
                        salt,
                        exclusion_zone,
                    }
                },
            ),
        )
    })
}
fn random_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<StructurePlacement, Ops>> {
    let inner = record_builder::map_codec(|i| {
        i.group(RecordCodecBuilder::of(
            Arc::new(|p: &RandomSpreadPlacement| p.settings.clone()),
            settings_codec(),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &RandomSpreadPlacement| p.spacing),
            codec::field_of(codec::int_range(0, 4096), "spacing".into()),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &RandomSpreadPlacement| p.separation),
            codec::field_of(codec::int_range(0, 4096), "separation".into()),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &RandomSpreadPlacement| p.spread_type),
            default_field("spread_type", spread_codec(), RandomSpreadType::Linear),
        ))
        .apply(
            i,
            Arc::new(
                |settings, spacing, separation, spread_type| RandomSpreadPlacement {
                    settings,
                    spacing,
                    separation,
                    spread_type,
                },
            ),
        )
    });
    // MapCodec.validate has the same bidirectional validation surface as Codec.
    let validated = map_codec::validate(
        inner,
        Arc::new(|p: &RandomSpreadPlacement| {
            if p.spacing <= p.separation {
                DataResult::error("Spacing has to be larger than separation".to_string())
            } else {
                DataResult::success(p.clone())
            }
        }),
    );
    map_codec::xmap(
        validated,
        Arc::new(|p: &RandomSpreadPlacement| StructurePlacement::RandomSpread(p.clone())),
        Arc::new(|p: &StructurePlacement| match p {
            StructurePlacement::RandomSpread(p) => p.clone(),
            _ => unreachable!("random spread codec dispatch"),
        }),
    )
}
fn rings_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<StructurePlacement, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<Holder<BiomeId>, Ops>> = Arc::new(RegistryFixedCodec::create(
        &rivet_registry::registries::BIOME,
    ));
    #[allow(clippy::arc_with_non_send_sync)]
    let biomes: Arc<dyn Codec<HolderSet<BiomeId>, Ops>> = Arc::new(HolderSetCodec::create(
        &rivet_registry::registries::BIOME,
        element,
        false,
    ));
    let inner = record_builder::map_codec(|i| {
        i.group(RecordCodecBuilder::of(
            Arc::new(|p: &ConcentricRingsPlacement| p.settings.clone()),
            settings_codec(),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &ConcentricRingsPlacement| p.distance),
            codec::field_of(codec::int_range(0, 1023), "distance".into()),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &ConcentricRingsPlacement| p.spread),
            codec::field_of(codec::int_range(0, 1023), "spread".into()),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &ConcentricRingsPlacement| p.count),
            codec::field_of(codec::int_range(1, 4095), "count".into()),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &ConcentricRingsPlacement| p.preferred_biomes.clone()),
            codec::field_of(biomes, "preferred_biomes".into()),
        ))
        .apply(
            i,
            Arc::new(|settings, distance, spread, count, preferred_biomes| {
                ConcentricRingsPlacement {
                    settings,
                    distance,
                    spread,
                    count,
                    preferred_biomes,
                }
            }),
        )
    });
    map_codec::xmap(
        inner,
        Arc::new(|p: &ConcentricRingsPlacement| StructurePlacement::ConcentricRings(p.clone())),
        Arc::new(|p: &StructurePlacement| match p {
            StructurePlacement::ConcentricRings(p) => p.clone(),
            _ => unreachable!("rings codec dispatch"),
        }),
    )
}
pub fn structure_placement_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<StructurePlacement, Ops>> {
    map_codec::codec_of(key_dispatch_codec::dispatch_map(
        "type",
        identifier_codec(),
        Arc::new(|p: &StructurePlacement| {
            DataResult::success(Identifier::parse(match p {
                StructurePlacement::RandomSpread(_) => "minecraft:random_spread",
                StructurePlacement::ConcentricRings(_) => "minecraft:concentric_rings",
            }))
        }),
        Arc::new(|name: &Identifier| match name.to_string().as_str() {
            "minecraft:random_spread" => DataResult::success(random_codec()),
            "minecraft:concentric_rings" => DataResult::success(rings_codec()),
            _ => DataResult::error(format!("unknown structure placement {name}")),
        }),
    ))
}
