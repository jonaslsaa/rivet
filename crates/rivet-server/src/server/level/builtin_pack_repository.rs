//! Source pack selection over the complete pinned builtin catalog. Registry
//! content is the server's embedded vanilla corpus and synchronized payloads;
//! resource-bearing overrides absent from that corpus refuse, rather than
//! being treated as loaded merely because a pack ID is recognized.
use super::{BootPackAuthority, BootSettingsError};
use rivet_registry::Identifier;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::json_ops::JsonOps;
use rivet_world::{
    flag::{FeatureFlagSet, feature_flags},
    level::{data_pack_config::DataPackConfig, world_data_configuration::WorldDataConfiguration},
};
use std::{path::Path, sync::LazyLock};

struct BuiltinPack {
    id: String,
    required: bool,
    fixed: bool,
    top: bool,
    auto: bool,
    feature_source: bool,
    features: FeatureFlagSet,
    resources: Vec<String>,
    synchronized_payloads: serde_json::Map<String, serde_json::Value>,
}
static CATALOG: LazyLock<Vec<BuiltinPack>> = LazyLock::new(|| {
    let catalog: serde_json::Value = serde_json::from_str(
        include_str!("../../../../../tools/rivet-oracle/fixtures/builtin-pack-repository.jsonl")
            .lines()
            .next()
            .expect("pinned pack catalog"),
    )
    .expect("pinned pack catalog JSON");
    catalog["packs"]
        .as_array()
        .expect("pinned packs")
        .iter()
        .map(|pack| BuiltinPack {
            id: pack["id"].as_str().unwrap().into(),
            required: pack["required"].as_bool().unwrap(),
            fixed: pack["fixed"].as_bool().unwrap(),
            top: pack["position"] == "TOP",
            auto: pack["auto"].as_bool().unwrap(),
            feature_source: pack["feature_source"].as_bool().unwrap(),
            features: feature_flags::codec()
                .parse(&JsonOps::INSTANCE, &pack["features"])
                .result()
                .expect("pinned feature flags")
                .clone(),
            resources: pack
                .get("resources")
                .and_then(|r| r.as_array())
                .map(|r| r.iter().map(|s| s.as_str().unwrap().into()).collect())
                .unwrap_or_default(),
            synchronized_payloads: pack
                .get("synchronized_payloads")
                .and_then(|value| value.as_object())
                .cloned()
                .unwrap_or_default(),
        })
        .collect()
});

/// Concrete baseline resource authority, not a configurable allow-list. Custom
/// pack discovery is refused because its resource/feature metadata cannot be
/// asserted from the compiled builtin corpus. No filesystem writes occur.
pub struct PinnedBuiltinPackAuthority;
impl PinnedBuiltinPackAuthority {
    /// An explicitly builtin-only source (for a fresh world's empty pack
    /// repository, or a caller with no filesystem repository at all).
    pub fn builtin_only() -> Self {
        Self
    }

    /// FolderRepositorySource would discover directory packs and .zip files.
    /// Until their real resource loaders exist, do not ignore installed custom
    /// packs and then claim builtin registry data describes the actual world.
    pub fn for_world_root(root: &Path) -> Result<Self, BootSettingsError> {
        let directory = root.join("datapacks");
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Self),
            Err(error) => {
                return Err(BootSettingsError::Discovery(format!(
                    "{}: {error}",
                    directory.display()
                )));
            }
        };
        for entry in entries {
            let entry = entry.map_err(|e| BootSettingsError::Discovery(e.to_string()))?;
            let path = entry.path();
            if entry
                .file_type()
                .map_err(|e| BootSettingsError::Discovery(e.to_string()))?
                .is_symlink()
            {
                return Err(BootSettingsError::PackAuthorityUnavailable(format!(
                    "custom pack symlink validation is unavailable: {}",
                    path.display()
                )));
            }
            let is_zip = path.extension().is_some_and(|ext| ext == "zip");
            if is_zip || path.join("pack.mcmeta").is_file() {
                return Err(BootSettingsError::PackAuthorityUnavailable(format!(
                    "custom pack resource discovery/loading is unavailable: {}",
                    path.display()
                )));
            }
        }
        Ok(Self)
    }
}

fn pack(id: &str) -> Option<&'static BuiltinPack> {
    CATALOG.iter().find(|p| p.id == id)
}
fn add(selected: &mut Vec<String>, id: &str) {
    if !selected.iter().any(|value| value == id) {
        selected.push(id.into());
    }
}
fn requested(selected: &[String]) -> FeatureFlagSet {
    selected
        .iter()
        .filter_map(|id| pack(id))
        .fold(FeatureFlagSet::of(), |flags, pack| {
            flags.join(&pack.features)
        })
}
fn rebuild_selected(selected: &mut Vec<String>) {
    selected.retain(|id| pack(id).is_some());
    for required in CATALOG.iter().filter(|pack| pack.required) {
        if selected.contains(&required.id) {
            continue;
        }
        let index = if required.top {
            selected
                .iter()
                .rposition(|id| {
                    let p = pack(id).unwrap();
                    !p.fixed || p.top != required.top
                })
                .map_or(0, |index| index + 1)
        } else {
            selected
                .iter()
                .position(|id| {
                    let p = pack(id).unwrap();
                    !p.fixed || p.top != required.top
                })
                .unwrap_or(selected.len())
        };
        selected.insert(index, required.id.clone());
    }
}

/// MinecraftServer.configurePackRepository + PackRepository.rebuildSelected.
/// Input pack IDs are settings, not evidence of custom resource availability.
fn select_configuration(
    initial: &WorldDataConfiguration,
    init_mode: bool,
    safe_mode: bool,
) -> WorldDataConfiguration {
    let forced = if init_mode {
        FeatureFlagSet::of()
    } else {
        initial.enabled_features().clone()
    };
    let allowed = if init_mode {
        feature_flags::REGISTRY.all_flags().clone()
    } else {
        initial.enabled_features().clone()
    };
    let mut selected = Vec::new();
    if safe_mode {
        selected.push("vanilla".into());
    } else {
        for id in initial.data_packs().get_enabled() {
            if pack(id).is_some() {
                add(&mut selected, id);
            }
        }
        for available in CATALOG.iter() {
            if initial.data_packs().get_disabled().contains(&available.id) {
                continue;
            }
            let was_selected = selected.contains(&available.id);
            if !was_selected && available.auto && available.features.is_subset_of(&allowed) {
                add(&mut selected, &available.id);
            }
            if was_selected && !available.features.is_subset_of(&allowed) {
                selected.retain(|id| *id != available.id);
            }
        }
        if selected.is_empty() {
            selected.push("vanilla".into());
        }
    }
    rebuild_selected(&mut selected);
    let mut missing = forced.subtract(&requested(&selected));
    if !missing.is_empty() {
        for available in CATALOG.iter() {
            if missing.is_empty() {
                break;
            }
            if available.feature_source
                && !available.features.is_empty()
                && available.features.intersects(&missing)
                && available.features.is_subset_of(&forced)
            {
                assert!(
                    !selected.contains(&available.id),
                    "source forced an already enabled feature pack"
                );
                selected.push(available.id.clone());
                missing = missing.subtract(&available.features);
            }
        }
        rebuild_selected(&mut selected);
    }
    let disabled = if safe_mode {
        Vec::new()
    } else {
        CATALOG
            .iter()
            .filter(|pack| !selected.contains(&pack.id))
            .map(|pack| pack.id.clone())
            .collect()
    };
    let features = requested(&selected).join(&forced);
    WorldDataConfiguration::new(DataPackConfig::new(selected, disabled), features)
}

fn validate_embedded_resources(
    configuration: &WorldDataConfiguration,
) -> Result<(), BootSettingsError> {
    if !configuration
        .data_packs()
        .get_enabled()
        .iter()
        .any(|id| id == "vanilla")
    {
        return Err(BootSettingsError::PackAuthorityUnavailable(
            "selected resources omit the compiled vanilla baseline".into(),
        ));
    }
    for id in configuration.data_packs().get_enabled() {
        for resource in &pack(id).expect("selected builtin").resources {
            // The Paper overlay's real chat_type/raw payload is already owned
            // by the synchronized registry corpus. Resolve resource identity
            // generically, not by blessing the whole pack's name.
            let synchronized = resource.split_once(':').and_then(|(namespace, path)| {
                let (registry, name) = path.split_once('/')?;
                let name = name.strip_suffix(".json")?;
                let key = Identifier::with_default_namespace(registry).to_string();
                let element = format!("{namespace}:{name}");
                rivet_registry::generated::registry_data::SYNCHRONIZED_NBT
                    .iter()
                    .find(|(id, _)| *id == key)
                    .and_then(|(_, values)| values.iter().find(|(id, _)| *id == element))
            });
            let matches_payload = synchronized.is_some_and(|(_, bytes)| {
                let Some(payload) = pack(id).unwrap().synchronized_payloads.get(resource) else {
                    return false;
                };
                let mut input = rivet_util::DataInputStream::new(*bytes);
                let Ok(tag) = rivet_nbt::nbt_io::read_any_tag(
                    &mut input,
                    &mut rivet_nbt::nbt_accounter::NbtAccounter::unlimited_heap(),
                ) else {
                    return false;
                };
                rivet_nbt::nbt_ops::NbtOps::instance().convert_to(&JsonOps::INSTANCE, &tag)
                    == *payload
            });
            if !matches_payload {
                return Err(BootSettingsError::PackAuthorityUnavailable(format!(
                    "pack {id} resource {resource} is not loaded by the embedded registry/resource authority"
                )));
            }
        }
    }
    Ok(())
}
impl BootPackAuthority for PinnedBuiltinPackAuthority {
    fn configure_pack_repository(
        &mut self,
        initial: &WorldDataConfiguration,
        init_mode: bool,
        safe_mode: bool,
    ) -> Result<WorldDataConfiguration, BootSettingsError> {
        if !safe_mode
            && let Some(id) = initial
                .data_packs()
                .get_enabled()
                .iter()
                .find(|id| pack(id).is_none())
        {
            return Err(BootSettingsError::PackAuthorityUnavailable(format!(
                "custom pack {id} has no loaded resource authority"
            )));
        }
        let configuration = select_configuration(initial, init_mode, safe_mode);
        validate_embedded_resources(&configuration)?;
        Ok(configuration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::level::generated_boot::PendingBootSettings;
    use rivet_nbt::tag_parser::parse_compound_fully;
    #[test]
    fn actual_paper_builtin_selection_order_flags_and_required_packs() {
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/builtin-pack-repository.jsonl")
                .lines()
                .skip(1)
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let pending = PendingBootSettings::from_level_data(
                &parse_compound_fully(row["input"].as_str().unwrap()).unwrap(),
            );
            let result = select_configuration(
                pending.initial_configuration(),
                row["init_mode"].as_bool().unwrap(),
                row["safe_mode"].as_bool().unwrap(),
            );
            let mut flags: Vec<_> = feature_flags::REGISTRY
                .to_names(result.enabled_features())
                .into_iter()
                .map(|id| id.to_string())
                .collect();
            flags.sort();
            assert_eq!(
                serde_json::to_value(flags).unwrap(),
                row["features"],
                "{line}"
            );
            assert_eq!(
                serde_json::to_value(result.data_packs().get_enabled()).unwrap(),
                row["enabled"],
                "{line}"
            );
            assert_eq!(
                serde_json::to_value(result.data_packs().get_disabled()).unwrap(),
                row["disabled"],
                "{line}"
            );
            // Actual selected overlays, not a recognized-ID shortcut, decide
            // whether this server can own the resulting resource authority.
            if result
                .data_packs()
                .get_enabled()
                .iter()
                .any(|id| id == "trade_rebalance")
                || !result
                    .data_packs()
                    .get_enabled()
                    .iter()
                    .any(|id| id == "vanilla")
            {
                assert!(validate_embedded_resources(&result).is_err());
            } else {
                validate_embedded_resources(&result).unwrap();
            }
        }
    }
    #[test]
    fn installed_custom_pack_and_unloaded_trade_overrides_refuse() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("datapacks/custom")).unwrap();
        std::fs::write(root.path().join("datapacks/custom/pack.mcmeta"), "{}").unwrap();
        assert!(PinnedBuiltinPackAuthority::for_world_root(root.path()).is_err());
        let pending = PendingBootSettings::from_level_data(
            &parse_compound_fully("{enabled_features:[\"vanilla\",\"trade_rebalance\"]}").unwrap(),
        );
        assert!(
            pending
                .finalize(
                    &mut PinnedBuiltinPackAuthority::builtin_only(),
                    false,
                    false
                )
                .is_err()
        );
    }
}
