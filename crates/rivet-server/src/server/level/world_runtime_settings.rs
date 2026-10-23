//! Runtime configuration consumed by native chunk activation. Paper world
//! overrides inherit world defaults; Spigot uses its separate typed-getter
//! fallback rules. This is a projection, not a claim that every Paper setting
//! or historical configuration migration has been implemented.
use super::entity_post_load::{DuplicateUuidMode, DuplicateUuidSettings};
use super::entity_tracking::EntityTrackingConfig;
use rivet_registry::Identifier;
use rivet_util::mth::Uuid;
use serde_yaml_ng::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeSettingsError {
    #[error("runtime configuration IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("runtime configuration YAML: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),
    #[error("invalid configuration value at {0}")]
    Value(String),
    #[error("Paper configuration version {0} requires historical transformations")]
    HistoricalVersion(i32),
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeSettingsInstallError {
    #[error(transparent)]
    Settings(#[from] RuntimeSettingsError),
    #[error(transparent)]
    Level(#[from] super::server_level::LevelEntityError),
    #[error("runtime settings require the generated level's retained storage and registry owner")]
    MissingWorldOwner,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorldRuntimeSettings {
    pub duplicate_uuid: DuplicateUuidSettings,
    pub block_entity_ticking: bool,
    pub mob_spawner_tick_rate: i32,
    pub cooldown_failed_beehive_releases: bool,
    pub nerf_spawner_mobs: bool,
    pub animal_tracking_range: i32,
    pub other_tracking_range: i32,
    pub animal_tracking_range_y: Option<f64>,
    pub other_tracking_range_y: Option<f64>,
    pub entity_save_limits: HashMap<Identifier, i32>,
}

impl WorldRuntimeSettings {
    /// Read-only configuration stage. Missing files mean fresh source defaults;
    /// permissions, invalid YAML and unsupported historical versions are errors.
    pub fn load(
        defaults: &Path,
        world: &Path,
        spigot: &Path,
        world_key: &str,
    ) -> Result<Self, RuntimeSettingsError> {
        fn read(path: &Path) -> Result<String, std::io::Error> {
            match std::fs::read_to_string(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
                result => result,
            }
        }
        Self::from_yaml(&read(defaults)?, &read(world)?, &read(spigot)?, world_key)
    }

    pub fn from_yaml(
        defaults: &str,
        world: &str,
        spigot: &str,
        world_key: &str,
    ) -> Result<Self, RuntimeSettingsError> {
        let defaults = parse(defaults)?;
        let mut world = parse(world)?;
        for node in [&defaults, &world] {
            let version = paper_int(at(node, &["_version"]), 31, "_version")?;
            if version < 31 {
                return Err(RuntimeSettingsError::HistoricalVersion(version));
            }
            if version > 31 {
                tracing::error!(
                    version,
                    "loading newer Paper world configuration than supported"
                );
            }
        }
        inherit(&mut world, &defaults);
        let spigot = parse(spigot)?;
        let setting = |path: &[&str]| at(&world, path);
        let mode_path = ["entities", "spawning", "duplicate-uuid", "mode"];
        let duplicate_mode = match setting(&mode_path) {
            Value::Null => DuplicateUuidMode::SafeRegen,
            Value::String(mode) => match mode.to_ascii_uppercase().as_str() {
                "SAFE_REGEN" => DuplicateUuidMode::SafeRegen,
                "DELETE" => DuplicateUuidMode::Delete,
                "WARN" => DuplicateUuidMode::Warn,
                "NOTHING" => DuplicateUuidMode::Nothing,
                _ => return Err(RuntimeSettingsError::Value(mode_path.join("."))),
            },
            _ => return Err(RuntimeSettingsError::Value(mode_path.join("."))),
        };
        let int = |path: &[&str], fallback| paper_int(setting(path), fallback, &path.join("."));
        let boolean =
            |path: &[&str], fallback| paper_bool(setting(path), fallback, &path.join("."));
        let y_enabled = boolean(&["entities", "tracking-range-y", "enabled"], false)?;
        let y_range = |category| -> Result<Option<f64>, RuntimeSettingsError> {
            if !y_enabled {
                return Ok(None);
            }
            let path = ["entities", "tracking-range-y", category];
            let value = setting(&path);
            let range = match value {
                Value::Null => -1,
                Value::String(value) if value.eq_ignore_ascii_case("default") => -1,
                Value::Number(_) => number_int(value).expect("numeric YAML value"),
                _ => paper_int(value, -1, &path.join("."))?,
            };
            Ok((range != -1).then_some(f64::from(range)))
        };
        // Bukkit's getInt accepts Number (including fractional Number.intValue),
        // but not a numeric string. Invalid world values fall back to defaults.
        let spigot_int = |path: &[&str], fallback| {
            let default = spigot_value(&spigot, "default", path);
            let selected = spigot_value(&spigot, world_key, path);
            number_int(selected)
                .or_else(|| number_int(default))
                .unwrap_or(fallback)
        };
        let nerf = spigot_value(&spigot, world_key, &["nerf-spawner-mobs"])
            .as_bool()
            .or_else(|| spigot_value(&spigot, "default", &["nerf-spawner-mobs"]).as_bool())
            .unwrap_or(false);
        Ok(Self {
            duplicate_uuid: DuplicateUuidSettings {
                mode: duplicate_mode,
                safe_regen_delete_range: int(
                    &[
                        "entities",
                        "spawning",
                        "duplicate-uuid",
                        "safe-regen-delete-range",
                    ],
                    32,
                )?,
            },
            block_entity_ticking: boolean(
                &["unsupported-settings", "ticking", "block-entities"],
                true,
            )?,
            mob_spawner_tick_rate: int(&["tick-rates", "mob-spawner"], 1)?,
            cooldown_failed_beehive_releases: boolean(
                &["entities", "behavior", "cooldown-failed-beehive-releases"],
                true,
            )?,
            nerf_spawner_mobs: nerf,
            animal_tracking_range: spigot_int(&["entity-tracking-range", "animals"], 96),
            other_tracking_range: spigot_int(&["entity-tracking-range", "other"], 64),
            animal_tracking_range_y: y_range("animal")?,
            other_tracking_range_y: y_range("other")?,
            entity_save_limits: entity_save_limits(setting(&[
                "chunks",
                "entity-per-chunk-save-limit",
            ]))?,
        })
    }

    pub fn tracking_config(&self, world_uuid: Uuid) -> EntityTrackingConfig {
        EntityTrackingConfig {
            world_uuid,
            animal_range_blocks: self.animal_tracking_range,
            other_range_blocks: self.other_tracking_range,
            animal_range_y: self.animal_tracking_range_y,
            other_range_y: self.other_tracking_range_y,
        }
    }
}

fn parse(input: &str) -> Result<Value, RuntimeSettingsError> {
    let mut value: Value = serde_yaml_ng::from_str(input)?;
    value.apply_merge()?;
    Ok(value)
}

fn entity_save_limits(value: &Value) -> Result<HashMap<Identifier, i32>, RuntimeSettingsError> {
    if value.is_null() {
        return Ok([
            "experience_orb",
            "snowball",
            "ender_pearl",
            "arrow",
            "fireball",
            "small_fireball",
        ]
        .into_iter()
        .map(|key| (Identifier::parse(key), -1))
        .collect());
    }
    let entries = value
        .as_mapping()
        .ok_or_else(|| RuntimeSettingsError::Value("chunks.entity-per-chunk-save-limit".into()))?;
    let mut sorted = std::collections::BTreeMap::new();
    for (key, value) in entries {
        let key = key
            .as_str()
            .ok_or_else(|| RuntimeSettingsError::Value("entity save-limit key".into()))?;
        sorted.insert(key, value);
    }
    let mut limits = HashMap::new();
    for (key, value) in sorted {
        let id = Identifier::try_parse(key)
            .filter(|id| {
                rivet_registry::generated::registries::ENTITY_TYPE_BY_NAME
                    .contains_key(id.to_string().as_str())
            })
            .ok_or_else(|| RuntimeSettingsError::Value(format!("entity save-limit key {key}")))?;
        if value.is_null() {
            return Err(RuntimeSettingsError::Value(format!(
                "entity save-limit {key}"
            )));
        }
        limits.insert(
            id,
            paper_int(value, -1, &format!("entity save-limit {key}"))?,
        );
    }
    Ok(limits)
}
fn at<'a>(mut node: &'a Value, path: &[&str]) -> &'a Value {
    for part in path {
        node = &node[*part];
    }
    node
}
fn spigot_value<'a>(root: &'a Value, world: &str, path: &[&str]) -> &'a Value {
    at(&root["world-settings"][world], path)
}
fn inherit(world: &mut Value, defaults: &Value) {
    if world.is_null() {
        *world = defaults.clone();
        return;
    }
    if let (Value::Mapping(world), Value::Mapping(defaults)) = (world, defaults) {
        for (key, value) in defaults {
            inherit(world.entry(key.clone()).or_insert(Value::Null), value);
        }
    }
}
fn number_int(value: &Value) -> Option<i32> {
    if let Some(integer) = value.as_i64() {
        return Some(integer as i32);
    }
    if let Some(integer) = value.as_u64() {
        return Some(integer as i32);
    }
    value.as_f64().map(|number| number as i32)
}
fn paper_int(value: &Value, fallback: i32, path: &str) -> Result<i32, RuntimeSettingsError> {
    let parsed = match value {
        Value::Null => Some(fallback),
        Value::String(value) => value.parse::<i32>().ok(),
        Value::Number(value) => value
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .or_else(|| {
                let number = value.as_f64()?;
                (number >= i32::MIN as f64 && number <= i32::MAX as f64 && number.fract() == 0.)
                    .then_some(number as i32)
            }),
        _ => None,
    };
    parsed.ok_or_else(|| RuntimeSettingsError::Value(path.into()))
}
fn paper_bool(value: &Value, fallback: bool, path: &str) -> Result<bool, RuntimeSettingsError> {
    match value {
        Value::Null => Ok(fallback),
        Value::Bool(value) => Ok(*value),
        Value::String(value) if value.eq_ignore_ascii_case("true") => Ok(true),
        Value::String(value) if value.eq_ignore_ascii_case("false") => Ok(false),
        _ => Err(RuntimeSettingsError::Value(path.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(unix)]
    #[test]
    fn file_settings_install_on_the_retained_generated_world() {
        use crate::server::level::{
            MissingChunkPolicy, OverworldGenerator, ServerLevelConfig,
            level_storage_access::LevelStorageAccess, server_level::ServerLevel,
        };
        use std::sync::Arc;
        let root = tempfile::tempdir().unwrap();
        let storage = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
        let generator = Arc::new(OverworldGenerator::new(42));
        let config = ServerLevelConfig {
            is_flat: false,
            sea_level: generator.generator().get_sea_level(),
            missing_chunk_policy: MissingChunkPolicy::RequireLoaded,
            ..ServerLevelConfig::default()
        };
        let world_dir = storage.dimension_path(&crate::server::level::overworld_dimension());
        std::fs::create_dir_all(&world_dir).unwrap();
        let mut level =
            ServerLevel::new_generated_with_storage(config, generator, storage).unwrap();
        let defaults = root.path().join("paper-world-defaults.yml");
        let spigot = root.path().join("spigot.yml");
        std::fs::write(&defaults, "tick-rates: {mob-spawner: 4}\n").unwrap();
        std::fs::write(
            world_dir.join("paper-world.yml"),
            "entities: {spawning: {duplicate-uuid: {mode: warn}}}\n",
        )
        .unwrap();
        level
            .load_runtime_settings_from_files(&defaults, &spigot)
            .unwrap();
        let installed = level.runtime_settings().unwrap().clone();
        assert_eq!(installed.mob_spawner_tick_rate, 4);
        assert_eq!(installed.duplicate_uuid.mode, DuplicateUuidMode::Warn);
        assert!(level.entity_tracking().is_some());
        // A parse failure must not replace the configuration or retained tracker.
        let tracker = std::ptr::from_ref(level.entity_tracking().unwrap());
        std::fs::write(&defaults, "tick-rates: {mob-spawner: invalid}\n").unwrap();
        assert!(
            level
                .load_runtime_settings_from_files(&defaults, &spigot)
                .is_err()
        );
        assert_eq!(level.runtime_settings(), Some(&installed));
        assert_eq!(
            std::ptr::from_ref(level.entity_tracking().unwrap()),
            tracker
        );
    }
    #[test]
    fn actual_paper_layered_runtime_settings_and_spigot_typed_fallbacks() {
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/world-runtime-settings.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some(key) = row["entity_key"].as_str() {
                let yaml = format!("chunks: {{entity-per-chunk-save-limit: {{'{key}': 3}}}}");
                let result = WorldRuntimeSettings::from_yaml("", &yaml, "", "minecraft:overworld");
                if row.get("error").is_some() {
                    assert!(result.is_err());
                } else {
                    assert_eq!(
                        result
                            .unwrap()
                            .entity_save_limits
                            .get(&Identifier::parse(row["canonical"].as_str().unwrap())),
                        Some(&3)
                    );
                }
                continue;
            }
            if let Some(spigot) = row["spigot"].as_str() {
                let value =
                    WorldRuntimeSettings::from_yaml("", "", spigot, "minecraft:overworld").unwrap();
                assert_eq!(
                    json!([
                        value.animal_tracking_range,
                        value.other_tracking_range,
                        value.nerf_spawner_mobs
                    ]),
                    json!([row["animal"], row["other"], row["nerf"]])
                );
                continue;
            }
            let result = WorldRuntimeSettings::from_yaml(
                row["defaults"].as_str().unwrap(),
                row["world"].as_str().unwrap(),
                "",
                "minecraft:overworld",
            );
            if row.get("error").is_some() {
                assert!(result.is_err(), "{row}");
                continue;
            }
            let value = result.unwrap();
            let mode = match value.duplicate_uuid.mode {
                DuplicateUuidMode::SafeRegen => "SAFE_REGEN",
                DuplicateUuidMode::Delete => "DELETE",
                DuplicateUuidMode::Warn => "WARN",
                DuplicateUuidMode::Nothing => "NOTHING",
            };
            assert_eq!(
                json!({"spawner_rate":value.mob_spawner_tick_rate,"block_entities":value.block_entity_ticking,"bee_cooldown":value.cooldown_failed_beehive_releases,"duplicate_mode":mode,"duplicate_range":value.duplicate_uuid.safe_regen_delete_range,"animal_y":value.animal_tracking_range_y.unwrap_or(-1.) as i32,"other_y":value.other_tracking_range_y.unwrap_or(-1.) as i32}),
                row["result"],
                "{row}"
            );
        }
    }
}
