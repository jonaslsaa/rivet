//! `net.minecraft.world.entity` — the entity-identity slice (issue #178,
//! `mc.world.level.biome.core` unit prerequisite).
//!
//! `MobSpawnSettings` keyed its spawner map on `MobCategory` and its spawn-cost
//! map on `EntityType<?>` (both `net.minecraft.world.entity`). The `mc.world.
//! entity` unit is not ported (`rivet-entity` is an empty crate), so this
//! module carries the minimal faithful surface `MobSpawnSettings` needs:
//!
//! - `MobCategory` — the **full** `StringRepresentable` enum (eight values, the
//!   max/despawn/spawn-persist fields and the `CODEC`).
//! - `EntityType` — a minimal id-handle STUB (id + name + category) with the
//!   by-name `CODEC`; the full entity-type registry (`EntityTypes`, the
//!   builder, the `BuiltInRegistries.ENTITY_TYPE` by-name codec) defers with the
//!   entity unit.
//! - `EntityTypes` — the `PIG` constant the `SpawnerData` compact constructor
//!   falls back to for `MISC`-category types.
//!
//! The by-name `EntityType` codec resolves against the small `ENTITY_TYPES`
//! constant list; a name outside it errors honestly (the real registry is
//! generated data). RivetTodo(#178): the full `net.minecraft.world.entity`
//! value/registry surface lands with the entity unit.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable};
use rivet_util::string_representable::{self, EnumOrdinal, StringRepresentable};
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.world.entity.MobCategory` — the `StringRepresentable` spawn
/// category enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MobCategory {
    /// `MONSTER("monster", "MO", 70, false, false, 128)`.
    Monster,
    /// `CREATURE("creature", "C", 10, true, true, 128)`.
    Creature,
    /// `AMBIENT("ambient", "AM", 15, true, false, 128)`.
    Ambient,
    /// `AXOLOTLS("axolotls", "AX", 5, true, false, 128)`.
    Axolotls,
    /// `UNDERGROUND_WATER_CREATURE("underground_water_creature", "UWC", 5,
    /// true, false, 128)`.
    UndergroundWaterCreature,
    /// `WATER_CREATURE("water_creature", "WC", 5, true, false, 128)`.
    WaterCreature,
    /// `WATER_AMBIENT("water_ambient", "WA", 20, true, false, 64)`.
    WaterAmbient,
    /// `MISC("misc", "MI", -1, true, true, 128)`.
    Misc,
}

impl MobCategory {
    /// `MobCategory.CODEC` — the ops-generic enum codec.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<MobCategory, Ops>> {
        Arc::new(string_representable::from_enum(MOB_CATEGORY_VALUES))
    }

    /// `MobCategory.getName()`.
    pub fn name(self) -> &'static str {
        match self {
            MobCategory::Monster => "monster",
            MobCategory::Creature => "creature",
            MobCategory::Ambient => "ambient",
            MobCategory::Axolotls => "axolotls",
            MobCategory::UndergroundWaterCreature => "underground_water_creature",
            MobCategory::WaterCreature => "water_creature",
            MobCategory::WaterAmbient => "water_ambient",
            MobCategory::Misc => "misc",
        }
    }

    /// `MobCategory.getDebugAbbreviation()`.
    pub fn debug_abbreviation(self) -> &'static str {
        match self {
            MobCategory::Monster => "MO",
            MobCategory::Creature => "C",
            MobCategory::Ambient => "AM",
            MobCategory::Axolotls => "AX",
            MobCategory::UndergroundWaterCreature => "UWC",
            MobCategory::WaterCreature => "WC",
            MobCategory::WaterAmbient => "WA",
            MobCategory::Misc => "MI",
        }
    }

    /// `MobCategory.getMaxInstancesPerChunk()`.
    pub fn max_instances_per_chunk(self) -> i32 {
        match self {
            MobCategory::Monster => 70,
            MobCategory::Creature => 10,
            MobCategory::Ambient => 15,
            MobCategory::Axolotls => 5,
            MobCategory::UndergroundWaterCreature => 5,
            MobCategory::WaterCreature => 5,
            MobCategory::WaterAmbient => 20,
            MobCategory::Misc => -1,
        }
    }

    /// `MobCategory.isFriendly()`.
    pub fn is_friendly(self) -> bool {
        !matches!(self, MobCategory::Monster)
    }

    /// `MobCategory.isPersistent()`.
    pub fn is_persistent(self) -> bool {
        matches!(
            self,
            MobCategory::Creature | MobCategory::WaterCreature | MobCategory::Misc
        )
    }

    /// `MobCategory.getDespawnDistance()`.
    pub fn despawn_distance(self) -> i32 {
        match self {
            MobCategory::WaterAmbient => 64,
            _ => 128,
        }
    }

    /// `MobCategory.getNoDespawnDistance()` — the hardcoded `32`.
    pub fn no_despawn_distance(self) -> i32 {
        32
    }
}

/// `MobCategory.values()` — declaration order.
pub const MOB_CATEGORY_VALUES: &[MobCategory] = &[
    MobCategory::Monster,
    MobCategory::Creature,
    MobCategory::Ambient,
    MobCategory::Axolotls,
    MobCategory::UndergroundWaterCreature,
    MobCategory::WaterCreature,
    MobCategory::WaterAmbient,
    MobCategory::Misc,
];

impl StringRepresentable for MobCategory {
    fn get_serialized_name(&self) -> &str {
        self.name()
    }
}

impl EnumOrdinal for MobCategory {
    fn ordinal(&self) -> usize {
        match self {
            MobCategory::Monster => 0,
            MobCategory::Creature => 1,
            MobCategory::Ambient => 2,
            MobCategory::Axolotls => 3,
            MobCategory::UndergroundWaterCreature => 4,
            MobCategory::WaterCreature => 5,
            MobCategory::WaterAmbient => 6,
            MobCategory::Misc => 7,
        }
    }
}

impl fmt::Display for MobCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// `net.minecraft.world.entity.EntityType<?>` — the entity-type id-handle STUB.
///
/// The full `EntityType` (the builder, the `BuiltInRegistries.ENTITY_TYPE`
/// registry, the generated `EntityTypes` table) defers with the entity unit;
/// this slice carries only the id/name/category identity `MobSpawnSettings`
/// needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityType {
    /// The registry insertion id (the `BuiltInRegistries.ENTITY_TYPE` index).
    id: u16,
    /// The registry key (`minecraft:pig`).
    name: &'static str,
    /// `EntityType.category` — the `MobCategory`.
    category: MobCategory,
}

impl EntityType {
    /// Construct an id-handle.
    pub const fn new(id: u16, name: &'static str, category: MobCategory) -> Self {
        EntityType { id, name, category }
    }

    /// `EntityType.CODEC` — `BuiltInRegistries.ENTITY_TYPE.byNameCodec()`, as
    /// the ops-generic by-name codec. Resolves against the small `ENTITY_TYPES`
    /// constant list; an unknown name errors with Paper's exact
    /// `Registry.byNameCodec` diagnostic (`"Unknown registry key in " + key() +
    /// ": " + name`, `Registries.ENTITY_TYPE` = `minecraft:entity_type`).
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<EntityType, Ops>> {
        codec::comap_flat_map(
            codec::string_codec::<Ops>(),
            Arc::new(
                |name: &String| match entity_types().iter().find(|t| t.name == *name) {
                    Some(t) => DataResult::success(*t),
                    None => DataResult::error(format!(
                        "Unknown registry key in minecraft:entity_type: {}",
                        name
                    )),
                },
            ),
            Arc::new(|t: &EntityType| t.name.to_string()),
        )
    }

    /// `EntityType.getCategory()`.
    pub fn get_category(self) -> MobCategory {
        self.category
    }

    /// `EntityType.getKey(EntityType)` — the registry key name.
    pub fn get_key(self) -> &'static str {
        self.name
    }

    /// The registry id.
    pub const fn id(self) -> u16 {
        self.id
    }
}

impl fmt::Display for EntityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// `EntityType.byNameCodec`'s `Keyable` — the known entity-type names (the
/// `BuiltInRegistries.ENTITY_TYPE` key set, STUB-limited).
#[derive(Debug, Clone, Copy)]
pub struct EntityTypeKeys;

impl<Ops: DynamicOps + 'static> Keyable<Ops> for EntityTypeKeys {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        entity_types()
            .iter()
            .map(|t| ops.create_string(t.name.to_string()))
            .collect()
    }
}

/// The `BuiltInRegistries.ENTITY_TYPE` key set, as a `Keyable`.
pub fn entity_type_keys<Ops: DynamicOps + 'static>() -> Arc<dyn Keyable<Ops>> {
    Arc::new(EntityTypeKeys)
}

/// `net.minecraft.world.entity.EntityTypes` — the named entity-type constants.
/// Only `PIG` exists in this slice (the `SpawnerData` MISC fallback).
pub struct EntityTypes;

impl EntityTypes {
    /// `EntityTypes.PIG` — `EntityType.Builder.of(Pig::new, MobCategory.
    /// CREATURE)`; the `MISC`-category fallback in `SpawnerData`'s compact
    /// constructor. Id `100` is PIG's `BuiltInRegistries.ENTITY_TYPE` index in
    /// 26.2 (see `rivet-registry`'s generated `ENTITY_TYPE_BY_NAME`).
    pub const PIG: EntityType = EntityType::new(100, "minecraft:pig", MobCategory::Creature);
}

/// The known entity types — the STUB registry (only `PIG`).
pub const fn entity_types() -> &'static [EntityType] {
    &[EntityTypes::PIG]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn mob_category_serde_names_and_fields() {
        assert_eq!(MobCategory::Monster.name(), "monster");
        assert_eq!(MobCategory::Monster.debug_abbreviation(), "MO");
        assert_eq!(MobCategory::Monster.max_instances_per_chunk(), 70);
        assert!(!MobCategory::Monster.is_friendly());
        assert!(!MobCategory::Monster.is_persistent());
        assert_eq!(MobCategory::WaterAmbient.despawn_distance(), 64);
        assert_eq!(MobCategory::Creature.despawn_distance(), 128);
        assert_eq!(MobCategory::Monster.no_despawn_distance(), 32);
        assert_eq!(MobCategory::Misc.max_instances_per_chunk(), -1);
    }

    #[test]
    fn mob_category_codec_round_trips() {
        let codec = MobCategory::codec::<JsonOps>();
        for (name, category) in [
            ("monster", MobCategory::Monster),
            ("creature", MobCategory::Creature),
            (
                "underground_water_creature",
                MobCategory::UndergroundWaterCreature,
            ),
            ("misc", MobCategory::Misc),
        ] {
            let decoded = codec
                .parse(&JsonOps::INSTANCE, &json!(name))
                .result()
                .cloned()
                .expect("decode");
            assert_eq!(decoded, category);
            let encoded = codec
                .encode_start(&JsonOps::INSTANCE, &category)
                .result()
                .expect("encode")
                .clone();
            assert_eq!(encoded, json!(name));
        }
    }

    #[test]
    fn entity_type_by_name_codec_round_trips_pig() {
        let codec = EntityType::codec::<JsonOps>();
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &json!("minecraft:pig"))
            .result()
            .cloned()
            .expect("decode");
        assert_eq!(decoded, EntityTypes::PIG);
        assert_eq!(decoded.get_category(), MobCategory::Creature);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &EntityTypes::PIG)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(encoded, json!("minecraft:pig"));
    }

    #[test]
    fn entity_type_unknown_name_errors() {
        let codec = EntityType::codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!("minecraft:zombie"));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(
            msg,
            "Unknown registry key in minecraft:entity_type: minecraft:zombie"
        );
    }

    #[test]
    fn entity_type_keys_are_the_stub_names() {
        assert_eq!(entity_types(), &[EntityTypes::PIG]);
    }
}
