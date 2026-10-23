//! BaseSpawner's persistent and transient server-tick state.
#[path = "spawner_tick.rs"]
pub mod ticking;
use super::NativeBlockEntityError;
use rivet_nbt::{
    compound_tag::CompoundTag, float_tag::FloatTag, int_tag::IntTag, list_tag::ListTag, tag::Tag,
};
use rivet_registry::{Identifier, core::BlockPos};
use rivet_serialization::float_format::java_float_equals;

#[derive(Debug, Clone, PartialEq)]
pub struct SpawnerData {
    delay: i32,
    min_delay: i32,
    max_delay: i32,
    spawn_count: i32,
    max_nearby: i32,
    required_range: i32,
    spawn_range: i32,
    next_spawn: Option<CompoundTag>,
    potentials: Vec<(i32, CompoundTag)>,
    public_bukkit_values: CompoundTag,
    tick_delay: i32,
}
impl SpawnerData {
    pub fn spawn_count(&self) -> i32 {
        self.spawn_count
    }
    pub fn min_spawn_delay(&self) -> i32 {
        self.min_delay
    }
    /// BaseSpawner.setEntityId changes only the chosen SpawnData and clears
    /// potentials. It must not serialize/reload unrelated numeric state.
    /// The owning world supplies the genuine prior weighted-selection draw.
    pub fn set_entity_id(
        &mut self,
        id: &Identifier,
        potential_roll: Option<i32>,
    ) -> Result<(), NativeBlockEntityError> {
        if self.next_spawn.is_none() {
            let total: i64 = self
                .potentials
                .iter()
                .map(|(weight, _)| i64::from(*weight))
                .sum();
            let selected = if total > 0 {
                let mut roll = potential_roll
                    .filter(|roll| *roll >= 0 && i64::from(*roll) < total)
                    .ok_or_else(|| {
                        NativeBlockEntityError::Unsupported(
                            "setEntityId requires actual weighted selection draw".into(),
                        )
                    })?;
                self.potentials.iter().find_map(|(weight, data)| {
                    if roll < *weight {
                        Some(data.clone())
                    } else {
                        roll -= *weight;
                        None
                    }
                })
            } else {
                None
            };
            self.next_spawn = Some(selected.unwrap_or_else(|| {
                let mut data = CompoundTag::new();
                data.put("entity".into(), Tag::Compound(CompoundTag::new()));
                data
            }));
        }
        self.next_spawn
            .as_mut()
            .expect("chosen SpawnData")
            .get_compound_or_empty_mut("entity")
            .put_string("id", &id.to_string());
        self.potentials.clear();
        Ok(())
    }
    pub fn fresh() -> Self {
        Self {
            delay: 20,
            min_delay: 200,
            max_delay: 800,
            spawn_count: 4,
            max_nearby: 6,
            required_range: 16,
            spawn_range: 4,
            next_spawn: None,
            potentials: Vec::new(),
            public_bukkit_values: CompoundTag::new(),
            tick_delay: 0,
        }
    }
    pub fn load(pos: BlockPos, input: &CompoundTag) -> Result<Self, NativeBlockEntityError> {
        let tag = normalized_data(pos, input)?;
        let potentials = tag
            .get_list("SpawnPotentials")
            .expect("normalized list")
            .list
            .iter()
            .map(|entry| {
                let Tag::Compound(entry) = entry else {
                    unreachable!()
                };
                (
                    entry.get_int("weight").unwrap(),
                    entry.get_compound("data").unwrap().clone(),
                )
            })
            .collect();
        Ok(Self {
            delay: input.get_int_or("Paper.Delay", i32::from(input.get_short_or("Delay", 20))),
            min_delay: input.get_int_or(
                "Paper.MinSpawnDelay",
                input.get_int_or("MinSpawnDelay", 200),
            ),
            max_delay: input.get_int_or(
                "Paper.MaxSpawnDelay",
                input.get_int_or("MaxSpawnDelay", 800),
            ),
            spawn_count: input.get_int_or("SpawnCount", 4),
            max_nearby: input.get_int_or("MaxNearbyEntities", 6),
            required_range: input.get_int_or("RequiredPlayerRange", 16),
            spawn_range: input.get_int_or("SpawnRange", 4),
            next_spawn: tag.get_compound("SpawnData").cloned(),
            potentials,
            public_bukkit_values: tag
                .get_compound("PublicBukkitValues")
                .cloned()
                .unwrap_or_default(),
            tick_delay: 0,
        })
    }
    pub fn save(&self, pos: BlockPos) -> CompoundTag {
        let mut tag = super::base_tag(pos, "minecraft:mob_spawner");
        if !self.public_bukkit_values.tags.is_empty() {
            tag.put(
                "PublicBukkitValues".into(),
                Tag::Compound(self.public_bukkit_values.clone()),
            );
        }
        for (key, value) in [
            ("Delay", self.delay),
            ("MinSpawnDelay", self.min_delay),
            ("MaxSpawnDelay", self.max_delay),
        ] {
            tag.put_short(key, value.min(i32::from(i16::MAX)) as i16);
        }
        if self.delay > i32::from(i16::MAX) {
            tag.put_int("Paper.Delay", self.delay);
        }
        if self.min_delay > i32::from(i16::MAX) || self.max_delay > i32::from(i16::MAX) {
            tag.put_int("Paper.MinSpawnDelay", self.min_delay);
            tag.put_int("Paper.MaxSpawnDelay", self.max_delay);
        }
        for (key, value) in [
            ("SpawnCount", self.spawn_count),
            ("MaxNearbyEntities", self.max_nearby),
            ("RequiredPlayerRange", self.required_range),
            ("SpawnRange", self.spawn_range),
        ] {
            tag.put_short(key, value as i16);
        }
        if let Some(data) = &self.next_spawn {
            tag.put("SpawnData".into(), Tag::Compound(data.clone()));
        }
        let list = self
            .potentials
            .iter()
            .map(|(weight, data)| {
                let mut entry = CompoundTag::new();
                entry.put_int("weight", *weight);
                entry.put("data".into(), Tag::Compound(data.clone()));
                Tag::Compound(entry)
            })
            .collect();
        tag.put(
            "SpawnPotentials".into(),
            Tag::List(ListTag::with_list(list)),
        );
        tag
    }
}
fn normalized_data(
    pos: BlockPos,
    input: &CompoundTag,
) -> Result<CompoundTag, NativeBlockEntityError> {
    let mut result = super::base_tag(pos, "minecraft:mob_spawner");
    super::common_data(&mut result, input)?;
    let delay = input.get_int_or("Paper.Delay", i32::from(input.get_short_or("Delay", 20)));
    let min = input.get_int_or(
        "Paper.MinSpawnDelay",
        input.get_int_or("MinSpawnDelay", 200),
    );
    let max = input.get_int_or(
        "Paper.MaxSpawnDelay",
        input.get_int_or("MaxSpawnDelay", 800),
    );
    if delay > i32::from(i16::MAX) {
        result.put_int("Paper.Delay", delay);
    }
    result.put_short("Delay", delay.min(i32::from(i16::MAX)) as i16);
    if min > i32::from(i16::MAX) || max > i32::from(i16::MAX) {
        result.put_int("Paper.MinSpawnDelay", min);
        result.put_int("Paper.MaxSpawnDelay", max);
    }
    result.put_short("MinSpawnDelay", min.min(i32::from(i16::MAX)) as i16);
    result.put_short("MaxSpawnDelay", max.min(i32::from(i16::MAX)) as i16);
    for (key, default) in [
        ("SpawnCount", 4),
        ("MaxNearbyEntities", 6),
        ("RequiredPlayerRange", 16),
        ("SpawnRange", 4),
    ] {
        result.put_short(key, input.get_int_or(key, default) as i16);
    }
    let next = spawn_data(input);
    if let Some(data) = &next {
        result.put("SpawnData".into(), Tag::Compound(data.clone()));
    }
    let mut potentials = ListTag::new();
    let mut sum = 0i64;
    if let Some(list) = input.get_list("SpawnPotentials") {
        for entry in &list.list {
            let Tag::Compound(record) = entry else {
                continue;
            };
            let Some(weight) = record
                .get("weight")
                .and_then(Tag::as_number)
                .map(|n| n.int_value())
                .filter(|weight| *weight >= 0)
            else {
                continue;
            };
            let Some(data) = record.get_compound("data").and_then(decode_spawn_data) else {
                continue;
            };
            sum += i64::from(weight);
            if sum > i64::from(i32::MAX) {
                return Err(NativeBlockEntityError::Unsupported(
                    "spawner sum of weights must be <= 2147483647".into(),
                ));
            }
            let mut normalized = CompoundTag::new();
            normalized.put_int("weight", weight);
            normalized.put("data".into(), Tag::Compound(data));
            potentials.list.push(Tag::Compound(normalized));
        }
    } else {
        let mut data = next.unwrap_or_else(|| {
            let mut data = CompoundTag::new();
            data.put("entity".into(), Tag::Compound(CompoundTag::new()));
            data
        });
        // SpawnData's constructor normalizes absent/malformed entity ids.
        data = decode_spawn_data(&data).expect("default SpawnData entity compound");
        let mut entry = CompoundTag::new();
        entry.put_int("weight", 1);
        entry.put("data".into(), Tag::Compound(data));
        potentials.list.push(Tag::Compound(entry));
    }
    result.put("SpawnPotentials".into(), Tag::List(potentials));
    Ok(result)
}

/// Decode the stored `SpawnData` compound through the partial shape of
/// `SpawnData.CODEC`. The required `entity` field must be a compound; malformed
/// optional fields are dropped from the retained partial value, unknown
/// top-level fields are dropped on re-encode, and valid equipment map entries
/// survive malformed siblings. The record constructor also canonicalizes a
/// valid id and removes an absent or malformed one.
fn decode_spawn_data(spawn_data: &CompoundTag) -> Option<CompoundTag> {
    let entity = spawn_data.get_compound("entity")?.clone();
    let mut decoded = CompoundTag::new();
    decoded.put("entity".to_string(), Tag::Compound(entity));
    if let Some(value) = spawn_data.get("custom_spawn_rules")
        && let Tag::Compound(rules) = value
        && let Ok(normalized) = normalize_custom_spawn_rules(rules)
    {
        decoded.put("custom_spawn_rules".to_string(), Tag::Compound(normalized));
    }
    if let Some(value) = spawn_data.get("equipment")
        && let Tag::Compound(equipment) = value
        && let Some(normalized) = normalize_equipment(equipment)
    {
        decoded.put("equipment".to_string(), Tag::Compound(normalized));
    }
    let Some(Tag::Compound(entity)) = decoded.tags.get_mut("entity") else {
        unreachable!("entity was cloned above")
    };
    match entity.get_string("id") {
        Some(id) => match Identifier::try_parse_result(id) {
            Ok(Some(identifier)) => entity.put_string("id", &identifier.to_string()),
            Ok(None) | Err(_) => {
                entity.remove("id");
            }
        },
        None => {
            entity.remove("id");
        }
    }
    Some(decoded)
}

fn normalize_custom_spawn_rules(rules: &CompoundTag) -> Result<CompoundTag, ()> {
    let mut normalized = CompoundTag::new();
    for field in ["block_light_limit", "sky_light_limit"] {
        if let Some(value) = rules.get(field)
            && let Some(value) = normalize_light_limit(value)?
        {
            normalized.put(field.to_string(), value);
        }
    }
    Ok(normalized)
}

fn normalize_light_limit(value: &Tag) -> Result<Option<Tag>, ()> {
    let bounds = if let Some(value) = value.as_int() {
        Some((value, value))
    } else if let Tag::List(list) = value {
        if list.list.len() == 2 {
            list.list[0].as_int().zip(list.list[1].as_int())
        } else {
            None
        }
    } else if let Tag::Compound(compound) = value {
        compound
            .get("min_inclusive")
            .and_then(Tag::as_int)
            .zip(compound.get("max_inclusive").and_then(Tag::as_int))
    } else {
        None
    };
    let Some((min, max)) = bounds else {
        return Ok(None);
    };
    if !(0..=15).contains(&min) || !(0..=15).contains(&max) || min > max {
        return Err(());
    }
    if min == 0 && max == 15 {
        return Ok(None);
    }
    if min == max {
        Ok(Some(Tag::Int(IntTag::value_of(min))))
    } else {
        let mut range = CompoundTag::new();
        range.put_int("min_inclusive", min);
        range.put_int("max_inclusive", max);
        Ok(Some(Tag::Compound(range)))
    }
}

fn normalize_equipment(equipment: &CompoundTag) -> Option<CompoundTag> {
    let loot_table = equipment.get_string("loot_table")?;
    let identifier = Identifier::try_parse_result(loot_table).ok()??;
    let slot_drop_chances = match equipment.get("slot_drop_chances") {
        Some(value) => normalize_slot_drop_chances(value).ok()?,
        None => None,
    };
    let mut normalized = CompoundTag::new();
    normalized.put_string("loot_table", &identifier.to_string());
    if let Some(slot_drop_chances) = slot_drop_chances {
        normalized.put("slot_drop_chances".to_string(), slot_drop_chances);
    }
    Some(normalized)
}

fn normalize_slot_drop_chances(value: &Tag) -> Result<Option<Tag>, ()> {
    if let Some(chance) = value.as_float() {
        return Ok(Some(Tag::Float(FloatTag::value_of(chance))));
    }
    let Tag::Compound(chances) = value else {
        return Err(());
    };
    const SLOTS: [&str; 8] = [
        "mainhand", "offhand", "feet", "legs", "chest", "head", "body", "saddle",
    ];
    let mut normalized = CompoundTag::new();
    let mut values = Vec::with_capacity(chances.size());
    for (slot, chance) in chances.entry_set() {
        if !SLOTS.contains(&slot.as_str()) {
            continue;
        }
        let Some(chance) = chance.as_float() else {
            continue;
        };
        values.push(chance);
        normalized.put(slot.clone(), Tag::Float(FloatTag::value_of(chance)));
    }
    let Some(first) = values.first().copied() else {
        return Ok(None);
    };
    if values.len() == SLOTS.len() && values.iter().all(|value| java_float_equals(*value, first)) {
        Ok(Some(Tag::Float(FloatTag::value_of(first))))
    } else {
        Ok(Some(Tag::Compound(normalized)))
    }
}

fn spawn_data(tag: &CompoundTag) -> Option<CompoundTag> {
    tag.get_compound("SpawnData").and_then(decode_spawn_data)
}
