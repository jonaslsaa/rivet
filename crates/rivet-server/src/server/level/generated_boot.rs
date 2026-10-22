//! Saved world datapack/feature settings are NOT executable pack authority.
//!
//! Paper first reads LevelStorageSource.readDataConfig, then calls
//! MinecraftServer.configurePackRepository before constructing the world. Keep
//! those two stages distinct: successfully decoding a custom pack's name does
//! not establish that its resources, registries or feature requests are loaded.
//! No UUID generation, disk writes or normal-world boot are implemented here.

use rivet_nbt::{compound_tag::CompoundTag, nbt_ops::NbtOps, tag::Tag};
use rivet_world::{
    flag::FeatureFlagSet,
    level::world_data_configuration::{self, WorldDataConfiguration},
};

#[path = "builtin_pack_repository.rs"]
mod builtin_pack_repository;
pub use builtin_pack_repository::PinnedBuiltinPackAuthority;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BootSettingsError {
    #[error("world datapack authority is unavailable: {0}")]
    PackAuthorityUnavailable(String),
    #[error("world datapack discovery failed: {0}")]
    Discovery(String),
}

/// Actual resource/pack owner, responsible for repository discovery, selection,
/// required packs, feature gates and requested-feature joins. There is no
/// default implementation or vanilla-only fallback. In particular this cannot
/// be implemented by checking an allow-list of names while ignoring resources.
pub trait BootPackAuthority {
    /// The exact MinecraftServer.configurePackRepository input/output seam.
    /// Fresh loading uses init_mode=true; saved-world loading uses false.
    fn configure_pack_repository(
        &mut self,
        initial: &WorldDataConfiguration,
        init_mode: bool,
        safe_mode: bool,
    ) -> Result<WorldDataConfiguration, BootSettingsError>;
}

/// Decoded/selected initial settings, not yet backed by loaded datapacks.
#[derive(Debug, Clone)]
pub struct PendingBootSettings {
    initial: WorldDataConfiguration,
}

impl PendingBootSettings {
    /// LevelStorageSource.readDataConfig. Input is level.dat's `Data` compound,
    /// not its outer root. Reuse the actual lenient codec, preserving absent,
    /// explicit-empty and malformed-field behavior (including unknown flags).
    pub fn from_level_data(data: &CompoundTag) -> Self {
        let ops = NbtOps::instance();
        let initial = world_data_configuration::codec::<NbtOps>()
            .parse(&ops, &Tag::Compound(data.clone()))
            .result_or_partial(|message| {
                tracing::error!(%message, "failed to parse saved world data configuration");
            })
            .unwrap_or_else(WorldDataConfiguration::default_config);
        Self { initial }
    }

    /// A new world's explicitly chosen initial configuration. This does not
    /// silently choose defaults on behalf of the resource/boot owner.
    pub fn fresh(initial: WorldDataConfiguration) -> Self {
        Self { initial }
    }

    pub fn initial_configuration(&self) -> &WorldDataConfiguration {
        &self.initial
    }

    /// Only a real pack owner can turn initial settings into finalized flags.
    /// Errors retain no fabricated successful context; callers must stop boot.
    pub fn finalize(
        self,
        authority: &mut dyn BootPackAuthority,
        init_mode: bool,
        safe_mode: bool,
    ) -> Result<GeneratedBootSettings, BootSettingsError> {
        let configuration =
            authority.configure_pack_repository(&self.initial, init_mode, safe_mode)?;
        Ok(GeneratedBootSettings { configuration })
    }
}

/// Settings returned by the actual pack-selection authority. Private fields
/// prevent treating a successful saved-settings decode as successful pack load.
#[derive(Debug, Clone)]
pub struct GeneratedBootSettings {
    configuration: WorldDataConfiguration,
}

impl GeneratedBootSettings {
    pub fn data_configuration(&self) -> &WorldDataConfiguration {
        &self.configuration
    }

    pub fn enabled_features(&self) -> &FeatureFlagSet {
        self.configuration.enabled_features()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::tag_parser::parse_compound_fully;
    use rivet_world::flag::feature_flags;

    #[test]
    fn actual_paper_initial_settings_keep_feature_semantics_and_unresolved_pack_names() {
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/generated-boot-settings.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let input = parse_compound_fully(row["input"].as_str().unwrap()).unwrap();
            let pending = PendingBootSettings::from_level_data(&input);
            let config = pending.initial_configuration();
            let mut features: Vec<_> = feature_flags::REGISTRY
                .to_names(config.enabled_features())
                .into_iter()
                .map(|id| id.to_string())
                .collect();
            features.sort();
            assert_eq!(
                serde_json::to_value(features).unwrap(),
                row["enabled_features"],
                "{}",
                row["case"]
            );
            assert_eq!(
                serde_json::to_value(config.data_packs().get_enabled()).unwrap(),
                row["enabled_packs"],
                "{}",
                row["case"]
            );
            assert_eq!(
                serde_json::to_value(config.data_packs().get_disabled()).unwrap(),
                row["disabled_packs"],
                "{}",
                row["case"]
            );
        }
    }

    #[test]
    fn decoded_custom_pack_does_not_become_successful_generation_authority() {
        struct Unavailable;
        impl BootPackAuthority for Unavailable {
            fn configure_pack_repository(
                &mut self,
                initial: &WorldDataConfiguration,
                init_mode: bool,
                safe_mode: bool,
            ) -> Result<WorldDataConfiguration, BootSettingsError> {
                assert_eq!(initial.data_packs().get_enabled(), &["file/custom.zip"]);
                assert!(!init_mode);
                assert!(!safe_mode);
                Err(BootSettingsError::PackAuthorityUnavailable(
                    "repository/resource loading is not implemented".into(),
                ))
            }
        }
        let input = parse_compound_fully(
            "{DataPacks:{Enabled:[\"file/custom.zip\"],Disabled:[]},enabled_features:[]}",
        )
        .unwrap();
        let pending = PendingBootSettings::from_level_data(&input);
        assert_eq!(
            pending
                .finalize(&mut Unavailable, false, false)
                .unwrap_err(),
            BootSettingsError::PackAuthorityUnavailable(
                "repository/resource loading is not implemented".into()
            )
        );
    }
}
