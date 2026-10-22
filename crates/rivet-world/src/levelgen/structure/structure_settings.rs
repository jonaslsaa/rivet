//! Structure settings resolve biome holders against the actual shared registry.
use super::{StructureError, TerrainAdjustment};
use crate::{
    biome::mob_spawn_settings::SpawnerData,
    entity::MobCategory,
    levelgen::generation_step::{Decoration, decoration_codec},
};
use rivet_registry::{
    RegistryAccess,
    biome_id::BiomeId,
    holder::Holder,
    holder_set::HolderSet,
    registries::BIOME,
    registry_file_codec::{HolderSetCodec, RegistryFixedCodec},
    registry_ops::RegistryOps,
};
use rivet_serialization::{codec::Codec, json_ops::JsonOps};
use rivet_util::weighted::{WeightedList, weighted_list_codec_map};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

pub(super) type JsonRegistryOps = RegistryOps<Value, JsonOps>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnBoundingBox {
    Piece,
    Full,
}
#[derive(Clone, Debug, PartialEq)]
pub struct StructureSpawnOverride {
    pub bounding_box: SpawnBoundingBox,
    pub spawns: WeightedList<SpawnerData>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct StructureSettings {
    pub biomes: HolderSet<BiomeId>,
    pub spawn_overrides: HashMap<MobCategory, StructureSpawnOverride>,
    pub step: Decoration,
    pub terrain_adaptation: TerrainAdjustment,
}

pub(super) fn decode<T: Clone + std::fmt::Debug + 'static>(
    codec: Arc<dyn Codec<T, JsonRegistryOps>>,
    ops: &JsonRegistryOps,
    value: &Value,
    field: &str,
) -> Result<T, StructureDataError> {
    let result = codec.parse(ops, value);
    result
        .result()
        .cloned()
        .ok_or_else(|| StructureDataError(format!("{field}: {result:?}")))
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct StructureDataError(pub String);

impl StructureSettings {
    pub fn parse(value: &Value, access: &RegistryAccess) -> Result<Self, StructureDataError> {
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone());
        #[allow(clippy::arc_with_non_send_sync)]
        let element: Arc<dyn Codec<Holder<BiomeId>, JsonRegistryOps>> =
            Arc::new(RegistryFixedCodec::create(&BIOME));
        #[allow(clippy::arc_with_non_send_sync)]
        let biomes: Arc<dyn Codec<HolderSet<BiomeId>, JsonRegistryOps>> =
            Arc::new(HolderSetCodec::create(&BIOME, element, false));
        let biomes = decode(biomes, &ops, &value["biomes"], "biomes")?;
        let step = decode(Arc::new(decoration_codec()), &ops, &value["step"], "step")?;
        let terrain_name = match value.get("terrain_adaptation") {
            None => "none",
            Some(v) => v
                .as_str()
                .ok_or_else(|| StructureDataError("terrain_adaptation must be a string".into()))?,
        };
        let terrain_adaptation = match terrain_name {
            "none" => TerrainAdjustment::None,
            "bury" => TerrainAdjustment::Bury,
            "beard_thin" => TerrainAdjustment::BeardThin,
            "beard_box" => TerrainAdjustment::BeardBox,
            "encapsulate" => TerrainAdjustment::Encapsulate,
            other => {
                return Err(StructureDataError(format!(
                    "unknown terrain_adaptation {other}"
                )));
            }
        };
        let overrides = value["spawn_overrides"]
            .as_object()
            .ok_or_else(|| StructureDataError("spawn_overrides must be an object".into()))?;
        let mut spawn_overrides = HashMap::new();
        for (category, data) in overrides {
            let category = decode(
                MobCategory::codec(),
                &ops,
                &Value::String(category.clone()),
                "spawn category",
            )?;
            let bounding_box = match data["bounding_box"].as_str() {
                Some("piece") => SpawnBoundingBox::Piece,
                Some("full") => SpawnBoundingBox::Full,
                _ => return Err(StructureDataError("invalid spawn bounding_box".into())),
            };
            let spawns = decode(
                weighted_list_codec_map(SpawnerData::map_codec_of()),
                &ops,
                &data["spawns"],
                "spawn override",
            )?;
            spawn_overrides.insert(
                category,
                StructureSpawnOverride {
                    bounding_box,
                    spawns,
                },
            );
        }
        Ok(Self {
            biomes,
            spawn_overrides,
            step,
            terrain_adaptation,
        })
    }
}

impl From<StructureError> for StructureDataError {
    fn from(error: StructureError) -> Self {
        Self(error.to_string())
    }
}
