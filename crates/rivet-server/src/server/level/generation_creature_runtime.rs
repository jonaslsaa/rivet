//! Explicit owning-world construction authority for generation creatures.
//! This shares the actual server UUID stream/ID sequence with players and
//! structure entities. An unported enabled family never becomes null or NBT.
use super::entity_construction::EntityConstructionRuntime;
use rivet_nbt::{nbt_ops::NbtOps, string_tag::StringTag, tag::Tag};
use rivet_registry::{RegistryAccess, holder::RegistryId, registries::BIOME};
use rivet_util::mth::Uuid;
use rivet_world::{
    entity::{
        EntityType, MobCategory,
        farm_variants::{FarmFamily, FarmVariantRegistries},
        generation_creature::{
            NativeEntityIdentity, NativeFarmAnimal, NativeGenerationCreature, NativeSheep,
        },
    },
    flag::{feature_flag_set::FeatureFlagSet, feature_flags},
};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GenerationCreatureError {
    #[error("generation creature constructor unavailable for {0}")]
    UnsupportedEntity(String),
    #[error("generation creature authority unavailable: {0}")]
    AuthorityUnavailable(String),
    #[error("unknown generation entity type {0}")]
    UnknownEntity(String),
    #[error("entity type {0} is not a generation CREATURE")]
    UnsupportedCategory(String),
}
pub trait GenerationCreatureFactory: Send + Sync {
    fn world_uuid(&self) -> Uuid;
    fn require_authority(&self) -> Result<(), GenerationCreatureError>;
    fn create(
        &self,
        entity_type: &str,
        registry_access: &RegistryAccess,
    ) -> Result<Option<NativeGenerationCreature>, GenerationCreatureError>;
}
pub struct WorldGenerationCreatureFactory {
    runtime: EntityConstructionRuntime,
    world_uuid: Uuid,
    enabled: FeatureFlagSet,
    biome_registry: Option<RegistryId>,
    farm_variants: Result<Arc<FarmVariantRegistries>, GenerationCreatureError>,
    is_in_use: Arc<dyn Fn(i32) -> bool + Send + Sync>,
}
impl WorldGenerationCreatureFactory {
    pub fn new(
        runtime: EntityConstructionRuntime,
        world_uuid: Uuid,
        enabled: FeatureFlagSet,
        registry_access: &RegistryAccess,
        is_in_use: Arc<dyn Fn(i32) -> bool + Send + Sync>,
    ) -> Self {
        let biome_registry = registry_access
            .lookup(&BIOME)
            .map(|registry| registry.registry_id());
        let farm_variants = if biome_registry.is_none() {
            Err(GenerationCreatureError::AuthorityUnavailable(
                "destination world has no BIOME registry".into(),
            ))
        } else {
            FarmVariantRegistries::builtin(registry_access)
                .map(Arc::new)
                .map_err(GenerationCreatureError::AuthorityUnavailable)
        };
        Self {
            runtime,
            world_uuid,
            enabled,
            biome_registry,
            farm_variants,
            is_in_use,
        }
    }
}
impl GenerationCreatureFactory for WorldGenerationCreatureFactory {
    fn world_uuid(&self) -> Uuid {
        self.world_uuid
    }
    fn require_authority(&self) -> Result<(), GenerationCreatureError> {
        self.runtime
            .with_entity_random(|_| ())
            .map_err(|error| GenerationCreatureError::AuthorityUnavailable(error.to_string()))
    }
    fn create(
        &self,
        entity_type: &str,
        registry_access: &RegistryAccess,
    ) -> Result<Option<NativeGenerationCreature>, GenerationCreatureError> {
        let registered = EntityType::codec::<NbtOps>()
            .parse(
                &NbtOps,
                &Tag::String(StringTag::value_of(entity_type.to_owned())),
            )
            .result_or_partial_silent()
            .ok_or_else(|| GenerationCreatureError::UnknownEntity(entity_type.into()))?;
        let key = registered.get_key().to_owned();
        if registered.get_category() != MobCategory::Creature {
            return Err(GenerationCreatureError::UnsupportedCategory(key));
        }
        // All generated CREATURE entries require VANILLA and are allowed in
        // peaceful. Perform this real enabled-feature gate BEFORE subtype
        // dispatch, including a valid but not yet implemented creature family.
        if !feature_flags::vanilla_set().is_subset_of(&self.enabled) {
            return Ok(None);
        }
        let family = match key.as_str() {
            "minecraft:sheep" => None,
            "minecraft:cow" => Some(FarmFamily::Cow),
            "minecraft:pig" => Some(FarmFamily::Pig),
            "minecraft:chicken" => Some(FarmFamily::Chicken),
            _ => return Err(GenerationCreatureError::UnsupportedEntity(key)),
        };
        let family_registry = family
            .map(|family| {
                if registry_access
                    .lookup(&BIOME)
                    .map(|registry| registry.registry_id())
                    != self.biome_registry
                {
                    return Err(GenerationCreatureError::AuthorityUnavailable(
                        "farm constructor received a foreign world BIOME registry".into(),
                    ));
                }
                self.farm_variants
                    .as_ref()
                    .map(|registries| registries.family(family))
                    .map_err(Clone::clone)
            })
            .transpose()?;
        let (uuid, entity_id) = self
            .runtime
            .allocate(|id| (self.is_in_use)(id))
            .map_err(|error| GenerationCreatureError::AuthorityUnavailable(error.to_string()))?;
        let identity = NativeEntityIdentity {
            entity_id,
            uuid,
            world_uuid: self.world_uuid,
        };
        self.runtime
            .with_entity_random(|random| match family_registry {
                Some(registry) => NativeGenerationCreature::Farm(NativeFarmAnimal::from_identity(
                    identity, registry, random,
                )),
                None => {
                    NativeGenerationCreature::Sheep(NativeSheep::from_identity(identity, random))
                }
            })
            .map(Some)
            .map_err(|error| GenerationCreatureError::AuthorityUnavailable(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::level::entity_id_allocator::EntityIdAllocator;
    use rivet_util::RandomSource;
    fn uuid() -> Uuid {
        Uuid {
            most: 0x1234_5678_9abc_def0,
            least: 0xfedc_ba98_7654_3210_u64 as i64,
        }
    }
    #[test]
    fn sheep_uses_actual_shared_identity_stream_then_living_constructor_draw() {
        let mut players = EntityIdAllocator::new();
        for _ in 0..100 {
            players.next_id();
        }
        let runtime = EntityConstructionRuntime::with_seed(players.sequence(), 42);
        let factory = WorldGenerationCreatureFactory::new(
            runtime.clone(),
            uuid(),
            feature_flags::vanilla_set(),
            &RegistryAccess::empty(),
            Arc::new(|_| false),
        );
        let sheep = factory
            .create("minecraft:sheep", &RegistryAccess::empty())
            .unwrap()
            .unwrap();
        let row: serde_json::Value = serde_json::from_str(
            include_str!("../../../../../tools/rivet-oracle/fixtures/generation-sheep.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            sheep.save().unwrap(),
            rivet_nbt::tag_parser::parse_compound_fully(row["saved"].as_str().unwrap()).unwrap()
        );
        assert_eq!(
            runtime.with_entity_random(|r| r.next_long()).unwrap(),
            row["entity_rng_next"].as_i64().unwrap()
        );
        assert_eq!(players.next_id(), 102);
    }
    #[test]
    fn actual_feature_gate_precedes_unsupported_dispatch_without_constructor_draws() {
        let players = EntityIdAllocator::new();
        let runtime = EntityConstructionRuntime::with_seed(players.sequence(), 42);
        let disabled = WorldGenerationCreatureFactory::new(
            runtime.clone(),
            uuid(),
            FeatureFlagSet::of(),
            &RegistryAccess::empty(),
            Arc::new(|_| false),
        );
        for name in ["minecraft:sheep", "minecraft:cow"] {
            assert!(
                disabled
                    .create(name, &RegistryAccess::empty())
                    .unwrap()
                    .is_none()
            );
        }
        // This authority is deliberately scoped to NaturalSpawner's CREATURE
        // pool; other categories cannot inherit its vanilla/peaceful gate.
        assert!(matches!(
            disabled.create("minecraft:zombie", &RegistryAccess::empty()),
            Err(GenerationCreatureError::UnsupportedCategory(_))
        ));
        assert_eq!(
            runtime.with_entity_random(|r| r.next_long()).unwrap(),
            -5025562857975149833
        );
        assert_eq!(players.sequence().next_id(|_| false), 1);
        let enabled = WorldGenerationCreatureFactory::new(
            runtime,
            uuid(),
            feature_flags::vanilla_set(),
            &RegistryAccess::empty(),
            Arc::new(|_| false),
        );
        assert!(matches!(
            enabled.create("minecraft:horse", &RegistryAccess::empty()),
            Err(GenerationCreatureError::UnsupportedEntity(_))
        ));
    }

    #[test]
    fn all_native_farm_constructors_use_world_identity_and_actual_entity_stream() {
        let access = rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries();
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/generation-farm-animals.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            if row["finalize"] != false {
                continue;
            }
            let mut players = EntityIdAllocator::new();
            for _ in 0..100 {
                players.next_id();
            }
            let runtime = EntityConstructionRuntime::with_seed(players.sequence(), 42);
            let factory = WorldGenerationCreatureFactory::new(
                runtime.clone(),
                uuid(),
                feature_flags::vanilla_set(),
                &access,
                Arc::new(|_| false),
            );
            let entity = factory
                .create(
                    &format!("minecraft:{}", row["family"].as_str().unwrap()),
                    &access,
                )
                .unwrap()
                .unwrap();
            assert_eq!(
                entity.save().unwrap(),
                rivet_nbt::tag_parser::parse_compound_fully(row["saved"].as_str().unwrap())
                    .unwrap(),
                "{line}"
            );
            assert_eq!(
                runtime
                    .with_entity_random(|random| random.next_long())
                    .unwrap(),
                row["entity_rng_next"].as_i64().unwrap()
            );
            assert_eq!(players.next_id(), 102);
        }
    }

    #[test]
    fn foreign_world_registry_refuses_before_identity_or_constructor_draws() {
        use rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries;
        let owner = build_worldgen_registries();
        let foreign = build_worldgen_registries();
        assert_ne!(
            owner.lookup_or_throw(&BIOME).registry_id(),
            foreign.lookup_or_throw(&BIOME).registry_id()
        );
        let players = EntityIdAllocator::new();
        let runtime = EntityConstructionRuntime::with_seed(players.sequence(), 42);
        let factory = WorldGenerationCreatureFactory::new(
            runtime.clone(),
            uuid(),
            feature_flags::vanilla_set(),
            &owner,
            Arc::new(|_| false),
        );
        for name in ["minecraft:cow", "minecraft:pig", "minecraft:chicken"] {
            assert!(
                matches!(factory.create(name,&foreign),Err(GenerationCreatureError::AuthorityUnavailable(message))if message.contains("foreign world"))
            );
        }
        assert_eq!(players.sequence().next_id(|_| false), 1);
        assert_eq!(
            runtime
                .with_entity_random(|random| random.next_long())
                .unwrap(),
            -5025562857975149833
        );
        assert!(factory.create("minecraft:cow", &owner).unwrap().is_some());
    }
}
