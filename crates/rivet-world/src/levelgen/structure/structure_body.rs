//! Typed source configuration for every pinned structure family. Template pool
//! keys remain named references, not fabricated native template-pool holders.
use super::TerrainAdjustment;
use super::structure_settings::{StructureDataError, StructureSettings, decode};
use crate::levelgen::{
    heightmap::Types,
    heightproviders::height_provider::{HeightProvider, height_provider_codec},
};
use rivet_registry::{Identifier, RegistryAccess, registry_ops::RegistryOps};
use rivet_serialization::json_ops::JsonOps;
use rivet_util::weighted::Weighted;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum StructureBody {
    BuriedTreasure,
    DesertPyramid,
    EndCity,
    Fortress,
    Igloo,
    JungleTemple,
    WoodlandMansion,
    OceanMonument,
    Stronghold,
    SwampHut,
    Mineshaft {
        kind: MineshaftKind,
    },
    OceanRuin {
        temperature: OceanRuinTemperature,
        large_probability: f32,
        cluster_probability: f32,
    },
    Shipwreck {
        is_beached: bool,
    },
    NetherFossil {
        height: HeightProvider,
    },
    RuinedPortal {
        setups: Vec<PortalSetup>,
    },
    Jigsaw(Box<JigsawSettings>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MineshaftKind {
    Normal,
    Mesa,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OceanRuinTemperature {
    Warm,
    Cold,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortalPlacement {
    OnLandSurface,
    PartlyBuried,
    OnOceanFloor,
    InMountain,
    Underground,
    InNether,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PortalSetup {
    pub placement: PortalPlacement,
    pub air_pocket_probability: f32,
    pub mossiness: f32,
    pub overgrown: bool,
    pub vines: bool,
    pub can_be_cold: bool,
    pub replace_with_blackstone: bool,
    pub weight: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiquidSettings {
    ApplyWaterlogging,
    IgnoreWaterlogging,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaxDistance {
    pub horizontal: i32,
    pub vertical: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DimensionPadding {
    pub bottom: i32,
    pub top: i32,
}
#[derive(Clone, Debug, PartialEq)]
pub enum PoolAliasBinding {
    Direct {
        alias: Identifier,
        target: Identifier,
    },
    Random {
        alias: Identifier,
        targets: Vec<Weighted<Identifier>>,
    },
    RandomGroup {
        groups: Vec<Weighted<Vec<PoolAliasBinding>>>,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct JigsawSettings {
    pub start_pool: Identifier,
    pub start_jigsaw_name: Option<Identifier>,
    pub size: i32,
    pub start_height: HeightProvider,
    pub use_expansion_hack: bool,
    pub project_start_to_heightmap: Option<Types>,
    pub max_distance_from_center: MaxDistance,
    pub pool_aliases: Vec<PoolAliasBinding>,
    pub dimension_padding: DimensionPadding,
    pub liquid_settings: LiquidSettings,
}

impl StructureBody {
    pub fn type_id(&self) -> &'static str {
        match self {
            Self::BuriedTreasure => "minecraft:buried_treasure",
            Self::DesertPyramid => "minecraft:desert_pyramid",
            Self::EndCity => "minecraft:end_city",
            Self::Fortress => "minecraft:fortress",
            Self::Igloo => "minecraft:igloo",
            Self::JungleTemple => "minecraft:jungle_temple",
            Self::WoodlandMansion => "minecraft:woodland_mansion",
            Self::OceanMonument => "minecraft:ocean_monument",
            Self::Stronghold => "minecraft:stronghold",
            Self::SwampHut => "minecraft:swamp_hut",
            Self::Mineshaft { .. } => "minecraft:mineshaft",
            Self::OceanRuin { .. } => "minecraft:ocean_ruin",
            Self::Shipwreck { .. } => "minecraft:shipwreck",
            Self::NetherFossil { .. } => "minecraft:nether_fossil",
            Self::RuinedPortal { .. } => "minecraft:ruined_portal",
            Self::Jigsaw(_) => "minecraft:jigsaw",
        }
    }
    pub fn parse(
        value: &Value,
        settings: &StructureSettings,
        access: &RegistryAccess,
    ) -> Result<Self, StructureDataError> {
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone());
        Ok(match text(value, "type")? {
            "minecraft:buried_treasure" => Self::BuriedTreasure,
            "minecraft:desert_pyramid" => Self::DesertPyramid,
            "minecraft:end_city" => Self::EndCity,
            "minecraft:fortress" => Self::Fortress,
            "minecraft:igloo" => Self::Igloo,
            "minecraft:jungle_temple" => Self::JungleTemple,
            "minecraft:woodland_mansion" => Self::WoodlandMansion,
            "minecraft:ocean_monument" => Self::OceanMonument,
            "minecraft:stronghold" => Self::Stronghold,
            "minecraft:swamp_hut" => Self::SwampHut,
            "minecraft:mineshaft" => Self::Mineshaft {
                kind: match text(value, "mineshaft_type")? {
                    "normal" => MineshaftKind::Normal,
                    "mesa" => MineshaftKind::Mesa,
                    other => return error(format!("unknown mineshaft_type {other}")),
                },
            },
            "minecraft:ocean_ruin" => Self::OceanRuin {
                temperature: match text(value, "biome_temp")? {
                    "warm" => OceanRuinTemperature::Warm,
                    "cold" => OceanRuinTemperature::Cold,
                    other => return error(format!("unknown biome_temp {other}")),
                },
                large_probability: probability(value, "large_probability")?,
                cluster_probability: probability(value, "cluster_probability")?,
            },
            "minecraft:shipwreck" => Self::Shipwreck {
                is_beached: boolean(value, "is_beached")?,
            },
            "minecraft:nether_fossil" => Self::NetherFossil {
                height: decode(height_provider_codec(), &ops, &value["height"], "height")?,
            },
            "minecraft:ruined_portal" => {
                let setups = array(value, "setups")?
                    .iter()
                    .map(PortalSetup::parse)
                    .collect::<Result<Vec<_>, _>>()?;
                if setups.is_empty() {
                    return error("portal setups must not be empty");
                }
                Self::RuinedPortal { setups }
            }
            "minecraft:jigsaw" => {
                let max = if let Some(v) = value["max_distance_from_center"].as_i64() {
                    let v = ranged_i64(v, 1, 128, "max distance")?;
                    MaxDistance {
                        horizontal: v,
                        vertical: v,
                    }
                } else {
                    let v = &value["max_distance_from_center"];
                    MaxDistance {
                        horizontal: integer(v, "horizontal", 1, 128)?,
                        vertical: match v.get("vertical") {
                            Some(v) => ranged(v, 1, 4064, "vertical max distance")?,
                            None => 4064,
                        },
                    }
                };
                let edge = if settings.terrain_adaptation == TerrainAdjustment::None {
                    0
                } else {
                    12
                };
                if max.horizontal + edge > 128 {
                    return error(
                        "Horizontal structure size including terrain adaptation must not exceed 128",
                    );
                }
                let dimension_padding = match value.get("dimension_padding") {
                    None => DimensionPadding { bottom: 0, top: 0 },
                    Some(v) if v.is_number() => {
                        let n = ranged(v, 0, i32::MAX, "dimension padding")?;
                        DimensionPadding { bottom: n, top: n }
                    }
                    Some(v) if v.is_object() => DimensionPadding {
                        // Source fields are lenientOptionalFieldOf: invalid
                        // present values fall back to0, rather than error.
                        bottom: v
                            .get("bottom")
                            .and_then(Value::as_i64)
                            .and_then(|v| i32::try_from(v).ok())
                            .filter(|v| *v >= 0)
                            .unwrap_or(0),
                        top: v
                            .get("top")
                            .and_then(Value::as_i64)
                            .and_then(|v| i32::try_from(v).ok())
                            .filter(|v| *v >= 0)
                            .unwrap_or(0),
                    },
                    _ => return error("dimension padding must be an int or record"),
                };
                let aliases = match value.get("pool_aliases") {
                    None => Vec::new(),
                    Some(v) => v
                        .as_array()
                        .ok_or_else(|| StructureDataError("pool_aliases must be a list".into()))?
                        .iter()
                        .map(PoolAliasBinding::parse)
                        .collect::<Result<Vec<_>, _>>()?,
                };
                let project_start_to_heightmap = match value.get("project_start_to_heightmap") {
                    None => None,
                    Some(v) => Some(match v.as_str() {
                        Some("WORLD_SURFACE_WG") => Types::WorldSurfaceWg,
                        Some("WORLD_SURFACE") => Types::WorldSurface,
                        Some("OCEAN_FLOOR_WG") => Types::OceanFloorWg,
                        Some("OCEAN_FLOOR") => Types::OceanFloor,
                        Some("MOTION_BLOCKING") => Types::MotionBlocking,
                        Some("MOTION_BLOCKING_NO_LEAVES") => Types::MotionBlockingNoLeaves,
                        _ => return error("unknown project_start_to_heightmap"),
                    }),
                };
                let liquid_name = match value.get("liquid_settings") {
                    None => "apply_waterlogging",
                    Some(v) => v.as_str().ok_or_else(|| {
                        StructureDataError("liquid_settings must be a string".into())
                    })?,
                };
                let liquid_settings = match liquid_name {
                    "apply_waterlogging" => LiquidSettings::ApplyWaterlogging,
                    "ignore_waterlogging" => LiquidSettings::IgnoreWaterlogging,
                    other => return error(format!("unknown liquid_settings {other}")),
                };
                Self::Jigsaw(Box::new(JigsawSettings {
                    start_pool: id(value, "start_pool")?,
                    start_jigsaw_name: value
                        .get("start_jigsaw_name")
                        .map(|_| id(value, "start_jigsaw_name"))
                        .transpose()?,
                    size: integer(value, "size", 0, 20)?,
                    start_height: decode(
                        height_provider_codec(),
                        &ops,
                        &value["start_height"],
                        "start_height",
                    )?,
                    use_expansion_hack: boolean(value, "use_expansion_hack")?,
                    project_start_to_heightmap,
                    max_distance_from_center: max,
                    pool_aliases: aliases,
                    dimension_padding,
                    liquid_settings,
                }))
            }
            other => return error(format!("unknown structure type {other}")),
        })
    }
}

impl PortalSetup {
    fn parse(value: &Value) -> Result<Self, StructureDataError> {
        let placement = match text(value, "placement")? {
            "on_land_surface" => PortalPlacement::OnLandSurface,
            "partly_buried" => PortalPlacement::PartlyBuried,
            "on_ocean_floor" => PortalPlacement::OnOceanFloor,
            "in_mountain" => PortalPlacement::InMountain,
            "underground" => PortalPlacement::Underground,
            "in_nether" => PortalPlacement::InNether,
            other => return error(format!("unknown portal placement {other}")),
        };
        let weight = number(value, "weight")?;
        if weight <= 0.0 {
            return error("portal setup weight must be positive");
        }
        Ok(Self {
            placement,
            air_pocket_probability: probability(value, "air_pocket_probability")?,
            mossiness: probability(value, "mossiness")?,
            overgrown: boolean(value, "overgrown")?,
            vines: boolean(value, "vines")?,
            can_be_cold: boolean(value, "can_be_cold")?,
            replace_with_blackstone: boolean(value, "replace_with_blackstone")?,
            weight,
        })
    }
}
impl PoolAliasBinding {
    fn parse(value: &Value) -> Result<Self, StructureDataError> {
        Ok(match text(value, "type")? {
            "minecraft:direct" => Self::Direct {
                alias: id(value, "alias")?,
                target: id(value, "target")?,
            },
            "minecraft:random" => {
                let targets = weighted(array(value, "targets")?, |v| {
                    v.as_str().and_then(Identifier::try_parse).ok_or_else(|| {
                        StructureDataError("pool alias target must be an identifier".into())
                    })
                })?;
                Self::Random {
                    alias: id(value, "alias")?,
                    targets,
                }
            }
            "minecraft:random_group" => Self::RandomGroup {
                groups: weighted(array(value, "groups")?, |v| {
                    v.as_array()
                        .ok_or_else(|| StructureDataError("alias group must be a list".into()))?
                        .iter()
                        .map(Self::parse)
                        .collect()
                })?,
            },
            other => return error(format!("unknown pool alias type {other}")),
        })
    }
}
fn weighted<T>(
    values: &[Value],
    parse: impl Fn(&Value) -> Result<T, StructureDataError>,
) -> Result<Vec<Weighted<T>>, StructureDataError> {
    if values.is_empty() {
        return error("weighted alias list must not be empty");
    }
    values
        .iter()
        .map(|entry| {
            Ok(Weighted::new(
                parse(&entry["data"])?,
                integer(entry, "weight", 0, i32::MAX)?,
            ))
        })
        .collect()
}
fn error<T>(message: impl Into<String>) -> Result<T, StructureDataError> {
    Err(StructureDataError(message.into()))
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, StructureDataError> {
    value[key]
        .as_str()
        .ok_or_else(|| StructureDataError(format!("{key} must be a string")))
}
fn id(value: &Value, key: &str) -> Result<Identifier, StructureDataError> {
    Identifier::try_parse(text(value, key)?)
        .ok_or_else(|| StructureDataError(format!("invalid {key} identifier")))
}
fn boolean(value: &Value, key: &str) -> Result<bool, StructureDataError> {
    value[key]
        .as_bool()
        .ok_or_else(|| StructureDataError(format!("{key} must be a bool")))
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], StructureDataError> {
    value[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| StructureDataError(format!("{key} must be a list")))
}
fn number(value: &Value, key: &str) -> Result<f32, StructureDataError> {
    value[key]
        .as_f64()
        .map(|v| v as f32)
        .filter(|v| v.is_finite())
        .ok_or_else(|| StructureDataError(format!("{key} must be finite float")))
}
fn probability(value: &Value, key: &str) -> Result<f32, StructureDataError> {
    let v = number(value, key)?;
    if !(0.0..=1.0).contains(&v) {
        return error(format!("{key} outside [0,1]"));
    }
    Ok(v)
}
fn integer(value: &Value, key: &str, min: i32, max: i32) -> Result<i32, StructureDataError> {
    ranged(&value[key], min, max, key)
}
fn ranged(value: &Value, min: i32, max: i32, key: &str) -> Result<i32, StructureDataError> {
    ranged_i64(
        value
            .as_i64()
            .ok_or_else(|| StructureDataError(format!("{key} must be an int")))?,
        min,
        max,
        key,
    )
}
fn ranged_i64(value: i64, min: i32, max: i32, key: &str) -> Result<i32, StructureDataError> {
    i32::try_from(value)
        .ok()
        .filter(|v| *v >= min && *v <= max)
        .ok_or_else(|| StructureDataError(format!("{key} outside [{min},{max}]")))
}
