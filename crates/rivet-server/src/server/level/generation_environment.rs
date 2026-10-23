//! The generation-facing slice of ServerLevel's real dimension and SavedData.
//! GameRuleMap is decoded/reconciled with GameRules' constructor semantics;
//! this slice does not pretend to implement the general live GameRules owner.
use super::generated_chunk_owner::GeneratedChunkOwner;
use rivet_nbt::{nbt_accounter::NbtAccounter, nbt_io, nbt_ops::NbtOps, tag::Tag};
use rivet_registry::{Identifier, generated::registry_data::SYNCHRONIZED_NBT};
use rivet_serialization::{Dynamic, codec::Codec};
use rivet_util::DataInputStream;
use rivet_world::{
    flag::feature_flag_set::FeatureFlagSet,
    level::{
        WorldBorder, WorldBorderSettings,
        gamerules::{game_rule::GameRuleValue, game_rule_map::GameRuleMap, game_rules},
    },
};
use std::{path::Path, sync::Arc};

#[derive(Debug, thiserror::Error)]
pub enum GenerationEnvironmentError {
    #[error("dimension type {0} is not in the pinned runtime registry")]
    UnknownDimensionType(Identifier),
    #[error("pinned dimension type {0} has invalid generation fields")]
    InvalidDimensionType(Identifier),
    #[error("SavedData {kind} requires unavailable data fixing from {version} to {expected}")]
    DataFixUnavailable {
        kind: &'static str,
        version: i32,
        expected: i32,
    },
    #[error("dimension height does not match the canonical generation owner")]
    DimensionHeightMismatch,
    #[error("generation environment is already installed")]
    AlreadyInstalled,
}

#[derive(Debug)]
pub struct GenerationEnvironment {
    dimension_type: Identifier,
    has_skylight: bool,
    has_ceiling: bool,
    min_y: i32,
    height: i32,
    spawn_mobs: bool,
    advance_time: bool,
    water_source_conversion: bool,
    lava_source_conversion: bool,
    border: WorldBorder,
}

#[derive(Debug)]
pub struct GenerationEnvironmentInstallError {
    pub error: GenerationEnvironmentError,
    pub environment: GenerationEnvironment,
}

impl GenerationEnvironment {
    /// SavedDataStorage.computeIfAbsent uses constructors for absent or failed
    /// reads, including logged IO/codec failures. Unsupported data fixing is a
    /// missing capability, not a simulated parse failure and never defaults.
    pub fn load(
        dimension_path: &Path,
        dimension_type: &Identifier,
        enabled_features: &FeatureFlagSet,
        game_time: i64,
    ) -> Result<Self, GenerationEnvironmentError> {
        let (has_skylight, has_ceiling, min_y, height) = dimension_fields(dimension_type)?;
        let mut rules = read_saved_data(dimension_path, "game_rules")?
            .and_then(|data| {
                decode_saved(
                    data,
                    game_rules::game_rule_map_codec::<NbtOps>(),
                    "game_rules",
                )
            })
            .unwrap_or_else(GameRuleMap::of);
        // GameRules(enabledFeatures, map), in built-in declaration order.
        let registry = game_rules::built_in_registry();
        for id in 0..registry.size() {
            let rule = registry.by_id_arc(id).expect("built-in game-rule id");
            if rule.is_enabled(enabled_features) {
                if !rules.has(rule) {
                    rules.reset(rule);
                }
            } else if rules.has(rule) {
                rules.remove(rule);
            }
        }
        let mut border = read_saved_data(dimension_path, "world_border")?
            .and_then(|data| decode_saved(data, WorldBorder::codec::<NbtOps>(), "world_border"))
            .unwrap_or_else(WorldBorder::default);
        border.apply_initial_settings(game_time);
        Ok(Self {
            dimension_type: dimension_type.clone(),
            has_skylight,
            has_ceiling,
            min_y,
            height,
            spawn_mobs: boolean_rule(&rules, "spawn_mobs"),
            advance_time: boolean_rule(&rules, "advance_time"),
            water_source_conversion: boolean_rule(&rules, "water_source_conversion"),
            lava_source_conversion: boolean_rule(&rules, "lava_source_conversion"),
            border,
        })
    }

    pub fn has_skylight(&self) -> bool {
        self.has_skylight
    }
    pub fn has_ceiling(&self) -> bool {
        self.has_ceiling
    }
    pub fn height_range(&self) -> (i32, i32) {
        (self.min_y, self.height)
    }
    pub fn spawn_mobs(&self) -> bool {
        self.spawn_mobs
    }
    pub fn advance_time(&self) -> bool {
        self.advance_time
    }
    pub fn dimension_type(&self) -> &Identifier {
        &self.dimension_type
    }
    pub fn water_source_conversion(&self) -> bool {
        self.water_source_conversion
    }
    pub fn lava_source_conversion(&self) -> bool {
        self.lava_source_conversion
    }
    /// Propagate actual rule changes after updating the authoritative rule owner.
    pub fn set_fluid_source_conversion(&mut self, water: bool, lava: bool) {
        self.water_source_conversion = water;
        self.lava_source_conversion = lava;
    }
    pub fn world_border(&self) -> &WorldBorder {
        &self.border
    }
    pub fn world_border_mut(&mut self) -> &mut WorldBorder {
        &mut self.border
    }

    /// The caller first updates its authoritative game rule, then propagates
    /// that actual value here. There is deliberately no default rule setter.
    pub fn set_spawn_mobs(&mut self, spawn_mobs: bool) {
        self.spawn_mobs = spawn_mobs;
    }

    /// ServerLevel ticks the real border once per world tick before capturing
    /// bounds for the synchronous SPAWN workspace.
    pub fn tick(&mut self) {
        self.border.tick();
    }

    pub fn bind_owner(
        &self,
        owner: &mut GeneratedChunkOwner,
    ) -> Result<(), GenerationEnvironmentError> {
        let generator = owner.generator().generator();
        if generator.get_min_y() != self.min_y || generator.get_gen_depth() != self.height {
            return Err(GenerationEnvironmentError::DimensionHeightMismatch);
        }
        // ThreadedLevelLightEngine always enables block light; sky light is
        // the actual DimensionType.hasSkyLight() value.
        owner.set_light_channels(self.has_skylight, true);
        owner.set_spawn_settings(
            WorldBorderSettings::from_world_border(&self.border),
            self.spawn_mobs(),
        );
        Ok(())
    }
}

fn boolean_rule(rules: &GameRuleMap, name: &str) -> bool {
    let rule = game_rules::built_in_registry()
        .get_optional(&Identifier::with_default_namespace(name))
        .expect("pinned game-rule registration");
    let Some(GameRuleValue::Bool(value)) = rules.get(rule) else {
        unreachable!("enabled boolean game rule reconciled at initialization");
    };
    value
}

fn dimension_fields(
    key: &Identifier,
) -> Result<(bool, bool, i32, i32), GenerationEnvironmentError> {
    let elements = SYNCHRONIZED_NBT
        .iter()
        .find(|(name, _)| *name == "minecraft:dimension_type")
        .expect("pinned dimension registry")
        .1;
    let payload = elements
        .iter()
        .find(|(name, _)| Identifier::parse(name) == *key)
        .ok_or_else(|| GenerationEnvironmentError::UnknownDimensionType(key.clone()))?
        .1;
    let mut input = DataInputStream::new(payload);
    let mut accounter = NbtAccounter::unlimited_heap();
    let data = nbt_io::read_any_tag(&mut input, &mut accounter)
        .map_err(|_| GenerationEnvironmentError::InvalidDimensionType(key.clone()))?;
    let Tag::Compound(data) = data else {
        return Err(GenerationEnvironmentError::InvalidDimensionType(
            key.clone(),
        ));
    };
    match (
        data.get_boolean("has_skylight"),
        data.get_boolean("has_ceiling"),
        data.get_int("min_y"),
        data.get_int("height"),
    ) {
        (Some(sky), Some(ceiling), Some(min_y), Some(height)) => Ok((sky, ceiling, min_y, height)),
        _ => Err(GenerationEnvironmentError::InvalidDimensionType(
            key.clone(),
        )),
    }
}

fn read_saved_data(
    path: &Path,
    kind: &'static str,
) -> Result<Option<Tag>, GenerationEnvironmentError> {
    let path = path.join("data/minecraft").join(format!("{kind}.dat"));
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            tracing::error!(%error, path = %path.display(), "Error loading saved data");
            return Ok(None);
        }
    };
    let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut accounter = NbtAccounter::unlimited_heap();
        if bytes.starts_with(&[0x1f, 0x8b]) {
            nbt_io::read_compressed(&bytes[..], &mut accounter)
        } else {
            nbt_io::read(&mut DataInputStream::new(&bytes[..]), &mut accounter)
        }
    }));
    let root = match decoded {
        Ok(Ok(root)) => root,
        _ => {
            tracing::error!(path = %path.display(), "Error loading saved data NBT");
            return Ok(None);
        }
    };
    let expected = super::region_backed::EXPECTED_DATA_VERSION;
    let version = root.get_int("DataVersion").unwrap_or(1343);
    // DataFixerUpper.update returns input unchanged when version >= target.
    // Only genuinely older data requires the unavailable migration body.
    if version < expected {
        return Err(GenerationEnvironmentError::DataFixUnavailable {
            kind,
            version,
            expected,
        });
    }
    // parse(null) fails and computeIfAbsent then uses the constructor.
    Ok(Some(
        root.get("data")
            .cloned()
            .unwrap_or(Tag::End(rivet_nbt::end_tag::EndTag)),
    ))
}

fn decode_saved<T: 'static>(
    data: Tag,
    codec: Arc<dyn Codec<T, NbtOps>>,
    kind: &'static str,
) -> Option<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ops = NbtOps::instance();
        Dynamic::new(&ops, data)
            .decode(&ops, &*codec)
            .result_or_partial(|message| {
                tracing::error!(kind, %message, "Failed to parse saved data");
            })
            .map(|(value, _)| value)
    })) {
        Ok(value) => value,
        Err(_) => {
            tracing::error!(kind, "Runtime failure decoding saved data");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_world::flag::feature_flags::vanilla_set;

    fn write_saved(path: &Path, kind: &str, data: CompoundTag, version: i32, gzip: bool) {
        let directory = path.join("data/minecraft");
        std::fs::create_dir_all(&directory).unwrap();
        let mut root = CompoundTag::new();
        root.put_int("DataVersion", version);
        root.put("data".into(), Tag::Compound(data));
        let file = std::fs::File::create(directory.join(format!("{kind}.dat"))).unwrap();
        if gzip {
            nbt_io::write_compressed(&root, file).unwrap();
        } else {
            nbt_io::write(&root, &mut rivet_util::DataOutputStream::new(file)).unwrap();
        }
    }

    #[test]
    fn dimension_channels_come_from_actual_pinned_registry() {
        assert_eq!(
            dimension_fields(&Identifier::parse("minecraft:overworld")).unwrap(),
            (true, false, -64, 384)
        );
        assert_eq!(
            dimension_fields(&Identifier::parse("minecraft:the_nether")).unwrap(),
            (false, true, 0, 256)
        );
        assert!(matches!(
            dimension_fields(&Identifier::parse("example:missing")),
            Err(GenerationEnvironmentError::UnknownDimensionType(_))
        ));
    }

    #[test]
    fn saved_rules_keep_partial_values_and_reconcile_missing_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let mut rules = CompoundTag::new();
        rules.put_boolean("minecraft:spawn_mobs", false);
        rules.put_boolean("minecraft:advance_time", false);
        rules.put_int("minecraft:not_a_rule", 1);
        write_saved(
            dir.path(),
            "game_rules",
            rules,
            super::super::region_backed::EXPECTED_DATA_VERSION,
            false,
        );
        let env = GenerationEnvironment::load(
            dir.path(),
            &Identifier::parse("minecraft:overworld"),
            &vanilla_set(),
            42,
        )
        .unwrap();
        assert!(
            !env.spawn_mobs(),
            "resultOrPartial retains the valid rule beside an invalid key"
        );
        assert!(
            !env.advance_time(),
            "saved clock rule is not replaced by its default"
        );
        assert_eq!(
            WorldBorderSettings::from_world_border(env.world_border()),
            WorldBorderSettings::default_settings().current_bounds_snapshot()
        );
        write_saved(
            dir.path(),
            "game_rules",
            CompoundTag::new(),
            super::super::region_backed::EXPECTED_DATA_VERSION - 1,
            true,
        );
        assert!(matches!(
            GenerationEnvironment::load(
                dir.path(),
                &Identifier::parse("minecraft:overworld"),
                &vanilla_set(),
                42
            ),
            Err(GenerationEnvironmentError::DataFixUnavailable {
                kind: "game_rules",
                ..
            })
        ));
    }

    #[test]
    fn saved_border_is_applied_at_world_time_and_remains_a_live_moving_border() {
        let dir = tempfile::tempdir().unwrap();
        let mut border = CompoundTag::new();
        border.put_double("center_x", 100.0);
        border.put_double("center_z", -40.0);
        border.put_double("damage_per_block", 0.2);
        border.put_double("safe_zone", 5.0);
        border.put_int("warning_blocks", 5);
        border.put_int("warning_time", 300);
        border.put_double("size", 100.0);
        border.put_long("lerp_time", 4);
        border.put_double("lerp_target", 20.0);
        write_saved(
            dir.path(),
            "world_border",
            border,
            super::super::region_backed::EXPECTED_DATA_VERSION,
            true,
        );
        let mut env = GenerationEnvironment::load(
            dir.path(),
            &Identifier::parse("minecraft:overworld"),
            &vanilla_set(),
            12_345,
        )
        .unwrap();
        assert!(
            env.spawn_mobs(),
            "only missing gamerule data legitimately uses enabled defaults"
        );
        assert_eq!(env.world_border().get_center_x(), 100.0);
        assert_eq!(env.world_border().get_size(), 100.0);
        env.tick();
        assert_eq!(env.world_border().get_size(), 80.0);
        // A known current-version bad codec payload follows the real
        // computeIfAbsent constructor path rather than masquerading as an
        // unavailable DataFix implementation.
        write_saved(
            dir.path(),
            "world_border",
            CompoundTag::new(),
            super::super::region_backed::EXPECTED_DATA_VERSION,
            true,
        );
        let fallback = GenerationEnvironment::load(
            dir.path(),
            &Identifier::parse("minecraft:overworld"),
            &vanilla_set(),
            12_345,
        )
        .unwrap();
        assert_eq!(
            fallback.world_border().get_size(),
            WorldBorderSettings::default_settings().size()
        );
    }
}
