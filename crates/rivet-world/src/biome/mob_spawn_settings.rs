//! `net.minecraft.world.level.biome.MobSpawnSettings` — the biome's mob spawn
//! configuration (issue #178, `mc.world.level.biome.core` unit).
//!
//! Faithful port of the 26.2 `MobSpawnSettings.java` value surface: the
//! per-`MobCategory` `WeightedList<SpawnerData>` spawners, the
//! `EntityType -> MobSpawnCost` map, `creatureGenerationProbability`, the
//! `CODEC`, the `Builder` (with the Paper `MobListBuilder`/
//! `WeightedSpawnerDataList` O(1)-contains subclass), the `SpawnerData` and
//! `MobSpawnCost` records, and `EMPTY`.
//!
//! ## Fidelity notes
//!
//! - **Spawner map order.** Java's `Builder` holds an `EnumMap<MobCategory,
//!   Builder>` (enum declaration order) and `ImmutableMap.copyOf`s it at
//!   `build()`; the CODEC's `simple_map` decodes into a `HashMap`. The port
//!   mirrors the field as an `IndexMap` (insertion order) and converts the
//!   decode-time `HashMap` into it (hash iteration order, exactly what Java's
//!   `ImmutableMap.copyOf(HashMap)` would preserve).
//! - **`SpawnerData` compact constructor.** `type.getCategory() == MISC`
//!   replaces the type with `EntityTypes.PIG`. The record `toString` is
//!   `EntityType.getKey(type) + "*(" + minCount + "-" + maxCount + ")"`.
//! - **Validation.** `SpawnerData.CODEC` validates `minCount <= maxCount`
//!   (decode AND encode, via `MapCodec.validate`) with the exact message
//!   `"minCount needs to be smaller or equal to maxCount"`. `minCount`/`maxCount`
//!   use `ExtraCodecs.POSITIVE_INT`.
//! - **`spawn_costs` key codec.** `BuiltInRegistries.ENTITY_TYPE.byNameCodec()`
//!   is the entity-type STUB's by-name codec (see [`crate::entity`]); a name
//!   outside the STUB list errors honestly.
//! - **The `MobSpawnCost` map.** The `getMobSpawnCost(EntityType)` lookup and
//!   the spawn-cost `CODEC` are fully ported; the `EntityType` key is the STUB
//!   id-handle.

use crate::entity::{EntityType, MOB_CATEGORY_VALUES, MobCategory, entity_type_keys};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::extra_codecs::positive_int;
use rivet_util::string_representable;
use rivet_util::weighted::{Weighted, WeightedList, weighted_list_codec_map};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// `MobSpawnSettings.DEFAULT_CREATURE_SPAWN_PROBABILITY` — `0.1F`.
const DEFAULT_CREATURE_SPAWN_PROBABILITY: f32 = 0.1;

/// `MobSpawnSettings.EMPTY_MOB_LIST` — `WeightedList.of()`.
pub static EMPTY_MOB_LIST: std::sync::LazyLock<WeightedList<SpawnerData>> =
    std::sync::LazyLock::new(WeightedList::of);

/// `net.minecraft.world.level.biome.MobSpawnSettings`.
#[derive(Debug, Clone, PartialEq)]
pub struct MobSpawnSettings {
    /// `this.creatureGenerationProbability`.
    creature_generation_probability: f32,
    /// `this.spawners` — the per-category weighted spawner lists.
    spawners: indexmap::IndexMap<MobCategory, WeightedSpawnerDataList<SpawnerData>>,
    /// `this.mobSpawnCosts` — the per-entity spawn costs.
    mob_spawn_costs: indexmap::IndexMap<EntityType, MobSpawnCost>,
}

impl MobSpawnSettings {
    /// `MobSpawnSettings.EMPTY` — Java's static field; a fresh empty value here
    /// because the `IndexMap` fields are not `const`-constructible.
    pub fn empty() -> MobSpawnSettings {
        MobSpawnSettings {
            creature_generation_probability: DEFAULT_CREATURE_SPAWN_PROBABILITY,
            spawners: indexmap::IndexMap::new(),
            mob_spawn_costs: indexmap::IndexMap::new(),
        }
    }

    /// `new MobSpawnSettings(float, Map<MobCategory, WeightedList<SpawnerData>>,
    /// Map<EntityType<?>, MobSpawnCost>)` — the private constructor (Java
    /// `ImmutableMap.copyOf`s both maps).
    fn new(
        creature_generation_probability: f32,
        spawners: indexmap::IndexMap<MobCategory, WeightedSpawnerDataList<SpawnerData>>,
        mob_spawn_costs: indexmap::IndexMap<EntityType, MobSpawnCost>,
    ) -> Self {
        MobSpawnSettings {
            creature_generation_probability,
            spawners,
            mob_spawn_costs,
        }
    }

    /// `MobSpawnSettings.CODEC` — the ops-generic `MapCodec`.
    pub fn map_codec_of<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<MobSpawnSettings, Ops>> {
        let spawner_list = codec::promote_partial(
            weighted_list_codec_map(SpawnerData::map_codec_of::<Ops>()),
            Arc::new(|_| {}),
        );
        let spawners_map: Arc<dyn MapCodec<HashMap<MobCategory, WeightedList<SpawnerData>>, Ops>> =
            codec::simple_map(
                MobCategory::codec::<Ops>(),
                spawner_list,
                Arc::new(string_representable::keys(MOB_CATEGORY_VALUES)),
            );
        let spawn_costs_map: Arc<dyn MapCodec<HashMap<EntityType, MobSpawnCost>, Ops>> =
            codec::simple_map(
                EntityType::codec::<Ops>(),
                MobSpawnCost::codec::<Ops>(),
                entity_type_keys::<Ops>(),
            );

        record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|m: &MobSpawnSettings| m.creature_generation_probability),
                    codec::optional_field_of(
                        "creature_spawn_probability",
                        codec::float_range::<Ops>(0.0, 0.9999999),
                        DEFAULT_CREATURE_SPAWN_PROBABILITY,
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|m: &MobSpawnSettings| {
                        m.spawners
                            .iter()
                            .map(|(k, v)| (*k, (**v).clone()))
                            .collect::<HashMap<_, _>>()
                    }),
                    codec::field_of(
                        map_codec::codec_of(spawners_map),
                        "spawners".to_string(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|m: &MobSpawnSettings| {
                        m.mob_spawn_costs
                            .iter()
                            .map(|(k, v)| (*k, *v))
                            .collect::<HashMap<_, _>>()
                    }),
                    codec::field_of(
                        map_codec::codec_of(spawn_costs_map),
                        "spawn_costs".to_string(),
                    ),
                ))
                .apply(
                    instance,
                    Arc::new(
                        |creature_generation_probability: f32,
                         spawners: HashMap<MobCategory, WeightedList<SpawnerData>>,
                         mob_spawn_costs: HashMap<EntityType, MobSpawnCost>| {
                            let spawners = spawners
                                .into_iter()
                                .map(|(k, v)| (k, WeightedSpawnerDataList::from_weighted_list(v)))
                                .collect();
                            let mob_spawn_costs = mob_spawn_costs.into_iter().collect();
                            MobSpawnSettings::new(
                                creature_generation_probability,
                                spawners,
                                mob_spawn_costs,
                            )
                        },
                    ),
                )
        })
    }

    /// `MobSpawnSettings.getMobs(MobCategory)` — `getOrDefault(category,
    /// EMPTY_MOB_LIST)`.
    pub fn get_mobs(&self, category: MobCategory) -> &WeightedList<SpawnerData> {
        match self.spawners.get(&category) {
            Some(list) => list,
            None => &EMPTY_MOB_LIST,
        }
    }

    /// `MobSpawnSettings.getMobSpawnCost(EntityType<?>)` — `@Nullable`.
    pub fn get_mob_spawn_cost(&self, entity_type: &EntityType) -> Option<&MobSpawnCost> {
        self.mob_spawn_costs.get(entity_type)
    }

    /// `MobSpawnSettings.getCreatureProbability()`.
    pub fn get_creature_probability(&self) -> f32 {
        self.creature_generation_probability
    }

    /// `MobSpawnSettings.spawners` — the internal map (test/access surface).
    pub fn spawners(
        &self,
    ) -> &indexmap::IndexMap<MobCategory, WeightedSpawnerDataList<SpawnerData>> {
        &self.spawners
    }
}

/// `MobSpawnSettings.Builder`.
#[derive(Debug, Clone)]
pub struct MobSpawnSettingsBuilder {
    /// `Builder.spawners` — the per-category builders (Java `EnumMap`).
    spawners: indexmap::IndexMap<MobCategory, MobListBuilder<SpawnerData>>,
    /// `Builder.mobSpawnCosts` — Java `Maps.newLinkedHashMap` (insertion
    /// order).
    mob_spawn_costs: indexmap::IndexMap<EntityType, MobSpawnCost>,
    /// `Builder.creatureGenerationProbability` — defaults to `0.1F`.
    creature_generation_probability: f32,
}

impl Default for MobSpawnSettingsBuilder {
    fn default() -> Self {
        let mut spawners = indexmap::IndexMap::new();
        for category in MOB_CATEGORY_VALUES {
            spawners.insert(*category, MobListBuilder::default());
        }
        MobSpawnSettingsBuilder {
            spawners,
            mob_spawn_costs: indexmap::IndexMap::new(),
            creature_generation_probability: DEFAULT_CREATURE_SPAWN_PROBABILITY,
        }
    }
}

impl MobSpawnSettingsBuilder {
    /// `new Builder()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `Builder.addSpawn(MobCategory, int weight, SpawnerData)`.
    pub fn add_spawn(
        mut self,
        category: MobCategory,
        weight: i32,
        spawner_data: SpawnerData,
    ) -> Self {
        self.spawners
            .get_mut(&category)
            .expect("spawners initialized for every MobCategory")
            .add_weighted(spawner_data, weight);
        self
    }

    /// `Builder.addMobCharge(EntityType<?>, double charge, double energyBudget)`.
    pub fn add_mob_charge(
        mut self,
        entity_type: EntityType,
        charge: f64,
        energy_budget: f64,
    ) -> Self {
        self.mob_spawn_costs
            .insert(entity_type, MobSpawnCost::new(energy_budget, charge));
        self
    }

    /// `Builder.creatureGenerationProbability(float)`.
    pub fn creature_generation_probability(mut self, creature_generation_probability: f32) -> Self {
        self.creature_generation_probability = creature_generation_probability;
        self
    }

    /// `Builder.build()`.
    pub fn build(self) -> MobSpawnSettings {
        let spawners = self
            .spawners
            .into_iter()
            .map(|(k, v)| (k, v.build()))
            .collect();
        MobSpawnSettings::new(
            self.creature_generation_probability,
            spawners,
            self.mob_spawn_costs,
        )
    }
}

/// `MobSpawnSettings.Builder.MobListBuilder<E>` — the Paper perf subclass of
/// `WeightedList.Builder<E>` whose `build()` returns a `WeightedSpawnerDataList`.
#[derive(Debug, Clone)]
pub struct MobListBuilder<E> {
    /// The accumulated `Weighted<E>` entries.
    result: Vec<Weighted<E>>,
}

impl<E> Default for MobListBuilder<E> {
    fn default() -> Self {
        MobListBuilder { result: Vec::new() }
    }
}

impl<E> MobListBuilder<E> {
    /// `WeightedList.Builder.add(E item, int weight)`.
    pub fn add_weighted(&mut self, item: E, weight: i32) {
        self.result.push(Weighted::new(item, weight));
    }

    /// `WeightedList.Builder.build()` — the Paper override returning the
    /// O(1)-contains `WeightedSpawnerDataList`.
    pub fn build(&self) -> WeightedSpawnerDataList<E>
    where
        E: Clone + Eq + std::hash::Hash,
    {
        WeightedSpawnerDataList::new(self.result.clone())
    }
}

/// `MobSpawnSettings.Builder.WeightedSpawnerDataList<E>` — the Paper perf
/// `WeightedList<E>` subclass with an O(1) `HashSet` `contains`.
#[derive(Debug, Clone)]
pub struct WeightedSpawnerDataList<E> {
    /// The underlying `WeightedList<E>`.
    inner: WeightedList<E>,
    /// The O(1) membership set.
    spawner_data_set: HashSet<E>,
}

impl<E: Clone + Eq + std::hash::Hash> WeightedSpawnerDataList<E> {
    /// `new WeightedSpawnerDataList(List<Weighted<E>> items)` — `super(items)`
    /// then fills the set.
    pub fn new(items: Vec<Weighted<E>>) -> Self {
        let inner = WeightedList::new(&items);
        let spawner_data_set = items.iter().map(|w| w.value().clone()).collect();
        WeightedSpawnerDataList {
            inner,
            spawner_data_set,
        }
    }

    /// Rebuild from an existing `WeightedList` (the CODEC decode path).
    pub fn from_weighted_list(list: WeightedList<E>) -> Self {
        let spawner_data_set = list.unwrap().iter().map(|w| w.value().clone()).collect();
        WeightedSpawnerDataList {
            inner: list,
            spawner_data_set,
        }
    }

    /// `WeightedList.contains(E)` — the Paper O(1) override.
    pub fn contains(&self, element: &E) -> bool {
        self.spawner_data_set.contains(element)
    }
}

impl<E> Deref for WeightedSpawnerDataList<E> {
    type Target = WeightedList<E>;
    fn deref(&self) -> &WeightedList<E> {
        &self.inner
    }
}

impl<E: PartialEq> PartialEq for WeightedSpawnerDataList<E> {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl<E: Eq> Eq for WeightedSpawnerDataList<E> {}

/// `MobSpawnSettings.MobSpawnCost` — the record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobSpawnCost {
    /// `MobSpawnCost.energyBudget`.
    pub energy_budget: f64,
    /// `MobSpawnCost.charge`.
    pub charge: f64,
}

impl MobSpawnCost {
    /// `new MobSpawnCost(double energyBudget, double charge)`.
    pub const fn new(energy_budget: f64, charge: f64) -> Self {
        MobSpawnCost {
            energy_budget,
            charge,
        }
    }

    /// `MobSpawnCost.CODEC`.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<MobSpawnCost, Ops>> {
        record_builder::create(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|c: &MobSpawnCost| c.energy_budget),
                    codec::field_of(codec::double_codec::<Ops>(), "energy_budget".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|c: &MobSpawnCost| c.charge),
                    codec::field_of(codec::double_codec::<Ops>(), "charge".to_string()),
                ))
                .apply(
                    instance,
                    Arc::new(|energy_budget: f64, charge: f64| MobSpawnCost {
                        energy_budget,
                        charge,
                    }),
                )
        })
    }
}

/// `MobSpawnSettings.SpawnerData` — the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpawnerData {
    /// `SpawnerData.type`.
    pub entity_type: EntityType,
    /// `SpawnerData.minCount`.
    pub min_count: i32,
    /// `SpawnerData.maxCount`.
    pub max_count: i32,
}

impl SpawnerData {
    /// `new SpawnerData(EntityType<?>, int minCount, int maxCount)` — the
    /// compact constructor: a `MISC`-category type is replaced by
    /// `EntityTypes.PIG`.
    pub fn new(entity_type: EntityType, min_count: i32, max_count: i32) -> Self {
        let entity_type = if entity_type.get_category() == MobCategory::Misc {
            crate::entity::EntityTypes::PIG
        } else {
            entity_type
        };
        SpawnerData {
            entity_type,
            min_count,
            max_count,
        }
    }

    /// `SpawnerData.CODEC` — the ops-generic `MapCodec` (with the
    /// `minCount <= maxCount` validation).
    pub fn map_codec_of<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<SpawnerData, Ops>> {
        let base = record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|d: &SpawnerData| d.entity_type),
                    codec::field_of(EntityType::codec::<Ops>(), "type".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|d: &SpawnerData| d.min_count),
                    codec::field_of(positive_int::<Ops>(), "minCount".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|d: &SpawnerData| d.max_count),
                    codec::field_of(positive_int::<Ops>(), "maxCount".to_string()),
                ))
                .apply(
                    instance,
                    Arc::new(|entity_type: EntityType, min_count: i32, max_count: i32| {
                        SpawnerData::new(entity_type, min_count, max_count)
                    }),
                )
        });
        rivet_serialization::map_codec::validate(
            base,
            Arc::new(|d: &SpawnerData| {
                if d.min_count > d.max_count {
                    DataResult::error(
                        "minCount needs to be smaller or equal to maxCount".to_string(),
                    )
                } else {
                    DataResult::success(*d)
                }
            }),
        )
    }
}

impl fmt::Display for SpawnerData {
    /// `SpawnerData.toString()` — `EntityType.getKey(type) + "*(" + minCount +
    /// "-" + maxCount + ")"`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}*({}-{})",
            self.entity_type.get_key(),
            self.min_count,
            self.max_count
        )
    }
}

// Re-export the builder alias for ergonomics (Java's `MobSpawnSettings.Builder`).
pub type Builder = MobSpawnSettingsBuilder;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::EntityTypes;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn pig_spawner() -> SpawnerData {
        SpawnerData::new(EntityTypes::PIG, 1, 4)
    }

    #[test]
    fn empty_is_empty() {
        let empty = MobSpawnSettings::empty();
        assert_eq!(empty.get_creature_probability(), 0.1);
        assert!(empty.get_mobs(MobCategory::Creature).is_empty());
        assert_eq!(empty.get_mob_spawn_cost(&EntityTypes::PIG), None);
        assert!(empty.spawners().is_empty());
    }

    #[test]
    fn spawner_data_compact_ctor_replaces_misc_with_pig() {
        // A MISC-category type would need a MISC entity; the STUB has only PIG
        // (CREATURE), so construct a MISC entity directly to pin the swap.
        let misc_type = EntityType::new(0, "minecraft:item", MobCategory::Misc);
        let data = SpawnerData::new(misc_type, 1, 1);
        assert_eq!(data.entity_type, EntityTypes::PIG);
        // A non-MISC type passes through.
        let data = SpawnerData::new(EntityTypes::PIG, 1, 2);
        assert_eq!(data.entity_type, EntityTypes::PIG);
    }

    #[test]
    fn spawner_data_to_string() {
        let data = SpawnerData::new(EntityTypes::PIG, 1, 4);
        assert_eq!(data.to_string(), "minecraft:pig*(1-4)");
    }

    #[test]
    fn builder_accumulates_and_builds() {
        let settings = MobSpawnSettingsBuilder::new()
            .creature_generation_probability(0.3)
            .add_spawn(MobCategory::Creature, 10, pig_spawner())
            .add_mob_charge(EntityTypes::PIG, 0.7, 0.15)
            .build();
        assert_eq!(settings.get_creature_probability(), 0.3);
        let mobs = settings.get_mobs(MobCategory::Creature);
        assert_eq!(mobs.unwrap(), vec![Weighted::new(pig_spawner(), 10)]);
        assert!(settings.get_mobs(MobCategory::Monster).is_empty());
        let cost = settings
            .get_mob_spawn_cost(&EntityTypes::PIG)
            .expect("cost");
        assert_eq!(cost.energy_budget, 0.15);
        assert_eq!(cost.charge, 0.7);
    }

    #[test]
    fn weighted_spawner_data_list_o1_contains() {
        let list = WeightedSpawnerDataList::new(vec![
            Weighted::new(pig_spawner(), 10),
            Weighted::new(SpawnerData::new(EntityTypes::PIG, 1, 1), 5),
        ]);
        assert!(list.contains(&pig_spawner()));
        assert!(!list.contains(&SpawnerData::new(EntityTypes::PIG, 9, 9)));
        assert!(!list.is_empty());
    }

    #[test]
    fn spawner_data_codec_round_trips() {
        let codec = map_codec::codec_of(SpawnerData::map_codec_of::<JsonOps>());
        let input = json!({"type": "minecraft:pig", "minCount": 1, "maxCount": 4});
        let decoded = *codec
            .parse(&JsonOps::INSTANCE, &input)
            .result()
            .expect("decode");
        assert_eq!(decoded, pig_spawner());
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &decoded)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn spawner_data_codec_rejects_min_gt_max_with_exact_message() {
        let codec = map_codec::codec_of(SpawnerData::map_codec_of::<JsonOps>());
        let input = json!({"type": "minecraft:pig", "minCount": 5, "maxCount": 2});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "minCount needs to be smaller or equal to maxCount");
    }

    #[test]
    fn spawner_data_codec_rejects_zero_counts() {
        let codec = map_codec::codec_of(SpawnerData::map_codec_of::<JsonOps>());
        let result = codec.parse(
            &JsonOps::INSTANCE,
            &json!({"type": "minecraft:pig", "minCount": 0, "maxCount": 4}),
        );
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "Value must be positive: 0");
    }

    #[test]
    fn codec_round_trips_empty_settings() {
        let codec = map_codec::codec_of(MobSpawnSettings::map_codec_of::<JsonOps>());
        let settings = MobSpawnSettings::empty();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &settings)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(encoded, json!({"spawners": {}, "spawn_costs": {}}));
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded, settings);
    }

    #[test]
    fn codec_round_trips_populated_settings() {
        let codec = map_codec::codec_of(MobSpawnSettings::map_codec_of::<JsonOps>());
        let settings = MobSpawnSettingsBuilder::new()
            .creature_generation_probability(0.2)
            .add_spawn(MobCategory::Creature, 10, pig_spawner())
            .add_mob_charge(EntityTypes::PIG, 0.7, 0.15)
            .build();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &settings)
            .result()
            .expect("encode")
            .clone();
        // The Builder's `EnumMap` seeds every MobCategory, so all eight
        // categories encode (the empty ones as `[]`) — Java-faithful.
        let obj = encoded.as_object().expect("object");
        assert_eq!(obj["creature_spawn_probability"], json!(0.2));
        assert_eq!(
            obj["spawn_costs"],
            json!({"minecraft:pig": {"energy_budget": 0.15, "charge": 0.7}})
        );
        let spawners = obj["spawners"].as_object().expect("spawners");
        assert_eq!(
            spawners["creature"],
            json!([{"type": "minecraft:pig", "minCount": 1, "maxCount": 4, "weight": 10}])
        );
        for category in crate::entity::MOB_CATEGORY_VALUES {
            if *category != MobCategory::Creature {
                assert_eq!(spawners[category.name()], json!([]), "{}", category.name());
            }
        }
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded, settings);
    }

    #[test]
    fn codec_unknown_spawner_category_errors() {
        let codec = map_codec::codec_of(MobSpawnSettings::map_codec_of::<JsonOps>());
        let result = codec.parse(
            &JsonOps::INSTANCE,
            &json!({"spawners": {"not_a_category": []}, "spawn_costs": {}}),
        );
        assert!(result.is_error());
    }

    #[test]
    fn weighted_list_builder_alias_is_used_by_the_builder() {
        // The shared `WeightedListBuilder` (here `E: Default`) and the
        // `MobListBuilder` accumulate the same `Weighted<E>` shape.
        let mut inner = rivet_util::weighted::WeightedListBuilder::default();
        inner.add_weighted(7, 3);
        assert_eq!(inner.build().unwrap(), vec![Weighted::new(7, 3)]);
        let mut list = MobListBuilder::default();
        list.add_weighted(pig_spawner(), 3);
        let built = list.build();
        assert_eq!(built.unwrap(), vec![Weighted::new(pig_spawner(), 3)]);
    }
}
