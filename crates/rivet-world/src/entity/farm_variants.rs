//! Complete pinned farm appearance/sound registries, in actual runtime ID
//! order. This is the vanilla authority, not support for custom datapacks.
//! JSON includes all assets/sounds; spawn selectors resolve genuine BIOME
//! holders/tags, never a hard-coded temperature substitution.
use rivet_registry::{
    Identifier, RegistrationInfo, RegistryAccess, RegistryBuilder, ResourceKey,
    biome_id::BiomeId,
    holder::{Holder, HolderId},
    holder_set::HolderSet,
    registries::BIOME,
    registry::Registry,
    registry_file_codec::{HolderSetCodec, RegistryFixedCodec},
    registry_ops::RegistryOps,
};
use rivet_serialization::{codec::Codec, decoder::Decoder, json_ops::JsonOps};
use rivet_util::RandomSource;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FarmFamily {
    Cow,
    Pig,
    Chicken,
}
impl FarmFamily {
    pub fn name(self) -> &'static str {
        match self {
            Self::Cow => "cow",
            Self::Pig => "pig",
            Self::Chicken => "chicken",
        }
    }
}
#[derive(Clone, Debug)]
pub struct FarmSelector {
    pub priority: i32,
    pub biomes: Option<HolderSet<BiomeId>>,
}
#[derive(Clone, Debug)]
pub struct FarmVariant {
    pub data: Value,
    pub selectors: Vec<FarmSelector>,
}
#[derive(Clone, Debug)]
pub struct FarmSoundVariant {
    pub data: Value,
}
#[derive(Debug)]
pub struct FarmFamilyRegistry {
    pub family: FarmFamily,
    variants: Registry<FarmVariant>,
    sounds: Registry<FarmSoundVariant>,
}
impl FarmFamilyRegistry {
    pub fn default_variant(&self) -> HolderId {
        self.variants
            .get_optional(&Identifier::with_default_namespace("temperate"))
            .map(|v| HolderId(self.variants.get_id(v) as u32))
            .or_else(|| self.variants.get_any())
            .expect("nonempty variant registry")
    }
    pub fn default_sound(&self) -> HolderId {
        self.sounds
            .get_optional(&Identifier::with_default_namespace("classic"))
            .map(|v| HolderId(self.sounds.get_id(v) as u32))
            .or_else(|| self.sounds.get_any())
            .expect("nonempty sound registry")
    }
    pub fn variant_name(&self, id: HolderId) -> Identifier {
        self.variants
            .get_key(self.variants.by_id_arc(id.0 as i32).unwrap())
            .unwrap()
    }
    pub fn sound_name(&self, id: HolderId) -> Identifier {
        self.sounds
            .get_key(self.sounds.by_id_arc(id.0 as i32).unwrap())
            .unwrap()
    }
    pub fn pick_variant(
        &self,
        biome: &Holder<BiomeId>,
        random: &mut impl RandomSource,
    ) -> Option<HolderId> {
        let mut entries = Vec::new();
        for id in 0..self.variants.size() {
            let variant = self.variants.by_id_arc(id).unwrap();
            for selector in &variant.selectors {
                entries.push((HolderId(id as u32), selector));
            }
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.1.priority));
        let mut highest = i32::MIN;
        entries.retain(|(_, selector)| {
            if selector.priority < highest {
                return false;
            }
            if selector
                .biomes
                .as_ref()
                .is_none_or(|set| set.contains(biome))
            {
                highest = selector.priority;
                true
            } else {
                false
            }
        });
        if entries.is_empty() {
            None
        } else {
            Some(entries[random.next_int_bound(entries.len() as i32) as usize].0)
        }
    }
    pub fn pick_sound(&self, random: &mut impl RandomSource) -> HolderId {
        self.sounds
            .get_random(random)
            .expect("nonempty sound registry")
    }
}
#[derive(Debug)]
pub struct FarmVariantRegistries {
    cow: Arc<FarmFamilyRegistry>,
    pig: Arc<FarmFamilyRegistry>,
    chicken: Arc<FarmFamilyRegistry>,
}
impl FarmVariantRegistries {
    pub fn family(&self, family: FarmFamily) -> Arc<FarmFamilyRegistry> {
        Arc::clone(match family {
            FarmFamily::Cow => &self.cow,
            FarmFamily::Pig => &self.pig,
            FarmFamily::Chicken => &self.chicken,
        })
    }
    pub fn builtin(access: &RegistryAccess) -> Result<Self, String> {
        let rows: Vec<Value> = include_str!("farm-variant-registries.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        let build = |family: FarmFamily| -> Result<Arc<FarmFamilyRegistry>, String> {
            let appearance_key = ResourceKey::create_registry_key(
                Identifier::with_default_namespace(&format!("{}_variant", family.name())),
            );
            let sound_key = ResourceKey::create_registry_key(Identifier::with_default_namespace(
                &format!("{}_sound_variant", family.name()),
            ));
            let row = |kind: &str| {
                rows.iter()
                    .find(|r| r["registry"] == format!("minecraft:{}{kind}", family.name()))
                    .and_then(|r| r["entries"].as_array())
                    .ok_or_else(|| "captured registry missing".to_string())
            };
            let mut appearances = RegistryBuilder::new(&appearance_key);
            let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone());
            #[allow(clippy::arc_with_non_send_sync)]
            let biome_codec: Arc<dyn Codec<Holder<BiomeId>, _>> =
                Arc::new(RegistryFixedCodec::create(&BIOME));
            let biome_sets = HolderSetCodec::create(&BIOME, biome_codec, false);
            for entry in row("_variant")? {
                let data = entry["value"].clone();
                let mut selectors = Vec::new();
                for selector in data["spawn_conditions"]
                    .as_array()
                    .ok_or("missing spawn selectors")?
                {
                    let priority = selector["priority"]
                        .as_i64()
                        .and_then(|v| i32::try_from(v).ok())
                        .ok_or("invalid selector priority")?;
                    let biomes = if let Some(condition) = selector.get("condition") {
                        if condition["type"] != "minecraft:biome" {
                            return Err("unsupported farm spawn condition".into());
                        }
                        Some(
                            biome_sets
                                .parse(&ops, &condition["biomes"])
                                .result()
                                .cloned()
                                .ok_or("invalid biome holder set")?,
                        )
                    } else {
                        None
                    };
                    selectors.push(FarmSelector { priority, biomes });
                }
                let key =
                    Identifier::try_parse(entry["key"].as_str().ok_or("invalid appearance key")?)
                        .ok_or("invalid appearance identifier")?;
                appearances.register(
                    &ResourceKey::create(&appearance_key, key),
                    Arc::new(FarmVariant { data, selectors }),
                    RegistrationInfo::BUILT_IN,
                );
            }
            let mut sounds = RegistryBuilder::new(&sound_key);
            for entry in row("_sound_variant")? {
                let key = Identifier::try_parse(entry["key"].as_str().ok_or("invalid sound key")?)
                    .ok_or("invalid sound identifier")?;
                sounds.register(
                    &ResourceKey::create(&sound_key, key),
                    Arc::new(FarmSoundVariant {
                        data: entry["value"].clone(),
                    }),
                    RegistrationInfo::BUILT_IN,
                );
            }
            Ok(Arc::new(FarmFamilyRegistry {
                family,
                variants: appearances.freeze(),
                sounds: sounds.freeze(),
            }))
        };
        Ok(Self {
            cow: build(FarmFamily::Cow)?,
            pig: build(FarmFamily::Pig)?,
            chicken: build(FarmFamily::Chicken)?,
        })
    }
}
