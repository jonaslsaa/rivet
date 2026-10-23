//! BeehiveBlockEntity's stored occupants and persistent worldgen state.
//! Bee release, ticking and AI are deliberately outside this storage value.

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;
use rivet_registry::core::{BlockPos, block_pos_codec};
use rivet_registry::generated::registries::ENTITY_TYPE_BY_NAME;

#[derive(Debug, Clone, PartialEq)]
struct Occupant {
    entity_type: Identifier,
    entity_data: CompoundTag,
    ticks_in_hive: i32,
    min_ticks_in_hive: i32,
    // Paper BeeData: transient retry counter, reset from age on load. It must
    // never replace ticks_in_hive in the persistent Occupant codec.
    exit_tick_counter: i32,
}

fn codec_int(tag: &CompoundTag, name: &str) -> Option<i32> {
    // Codec.INT uses NbtOps' boxed Number.intValue, unlike NumericTag's
    // direct accessor (which floors floating-point tag values).
    tag.get(name)?.as_number().map(|number| number.int_value())
}

impl Occupant {
    fn load(value: &Tag) -> Option<Self> {
        let Tag::Compound(tag) = value else {
            return None;
        };
        let mut entity_data = tag.get_compound("entity_data")?.clone();
        let entity_type = Identifier::try_parse_result(entity_data.get_string("id")?).ok()??;
        if !ENTITY_TYPE_BY_NAME.contains_key(entity_type.to_string().as_str()) {
            return None;
        }
        entity_data.remove("id");
        let ticks_in_hive = codec_int(tag, "ticks_in_hive")?;
        Some(Self {
            entity_type,
            entity_data,
            ticks_in_hive,
            min_ticks_in_hive: codec_int(tag, "min_ticks_in_hive")?,
            exit_tick_counter: ticks_in_hive,
        })
    }

    fn save(&self) -> Tag {
        let mut entity_data = self.entity_data.clone();
        entity_data.put_string("id", &self.entity_type.to_string());
        let mut tag = CompoundTag::new();
        tag.put("entity_data".to_string(), Tag::Compound(entity_data));
        tag.put_int("ticks_in_hive", self.ticks_in_hive);
        tag.put_int("min_ticks_in_hive", self.min_ticks_in_hive);
        Tag::Compound(tag)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BeehiveData {
    occupants: Vec<Occupant>,
    flower_pos: Option<BlockPos>,
    max_bees: i32,
    public_bukkit_values: CompoundTag,
    components: CompoundTag,
}

impl BeehiveData {
    pub fn load(tag: &CompoundTag) -> Self {
        let components = tag.get_compound("components").cloned().unwrap_or_default();
        assert!(
            components.tags.is_empty(),
            "BeehiveBlockEntity nonempty components codec is not implemented"
        );
        // TagValueInput.read takes the partial valid list on codec error.
        // Each Occupant is all-required; malformed records have no partial.
        let occupants = tag
            .get_list("bees")
            .map(|list| list.list.iter().filter_map(Occupant::load).collect())
            .unwrap_or_default();
        let flower_pos = tag.get("flower_pos").and_then(|value| {
            block_pos_codec::<NbtOps>()
                .parse(&NbtOps, value)
                .result_or_partial_silent()
        });
        Self {
            occupants,
            flower_pos,
            max_bees: tag.get_int_or("Bukkit.MaxEntities", 3),
            public_bukkit_values: tag
                .get_compound("PublicBukkitValues")
                .cloned()
                .unwrap_or_default(),
            components,
        }
    }

    /// Occupant.create: a typed BEE with no entity payload and minTicks600.
    /// storeBee appends without enforcing maxBees (as in Paper's source).
    pub fn store_worldgen_bee(&mut self, ticks_in_hive: i32) {
        self.occupants.push(Occupant {
            entity_type: Identifier::with_default_namespace("bee"),
            entity_data: CompoundTag::new(),
            ticks_in_hive,
            min_ticks_in_hive: 600,
            exit_tick_counter: ticks_in_hive,
        });
    }

    /// Actual BeeData.tick/Iterator.remove order. Release owns the real world
    /// behavior (environment gates, entity creation/admission and events), not
    /// an assumed success. A capability error leaves the already-aged prefix
    /// intact, with no claimed transaction or completed server tick.
    pub fn tick_occupants<E>(
        &mut self,
        cooldown_failed_releases: bool,
        mut release: impl FnMut(&CompoundTag, bool, Option<BlockPos>) -> Result<bool, E>,
    ) -> Result<bool, E> {
        let mut changed = false;
        let mut index = 0;
        while index < self.occupants.len() {
            let occupant = &mut self.occupants[index];
            occupant.ticks_in_hive = occupant.ticks_in_hive.wrapping_add(1);
            let attempt = occupant.exit_tick_counter > occupant.min_ticks_in_hive;
            occupant.exit_tick_counter = occupant.exit_tick_counter.wrapping_add(1);
            if attempt {
                let Tag::Compound(saved) = occupant.save() else {
                    unreachable!()
                };
                let nectar = occupant.entity_data.get_boolean_or("HasNectar", false);
                if release(&saved, nectar, self.flower_pos)? {
                    self.occupants.remove(index);
                    changed = true;
                    continue;
                } else if cooldown_failed_releases {
                    occupant.exit_tick_counter = occupant.min_ticks_in_hive / 2;
                }
            }
            index += 1;
        }
        Ok(changed)
    }

    pub fn has_occupants(&self) -> bool {
        !self.occupants.is_empty()
    }

    pub fn save_with_full_metadata(&self, pos: &BlockPos) -> CompoundTag {
        let mut tag = CompoundTag::new();
        if !self.public_bukkit_values.tags.is_empty() {
            tag.put(
                "PublicBukkitValues".to_string(),
                Tag::Compound(self.public_bukkit_values.clone()),
            );
        }
        tag.put(
            "bees".to_string(),
            Tag::List(ListTag::with_list(
                self.occupants.iter().map(Occupant::save).collect(),
            )),
        );
        if let Some(flower) = self.flower_pos {
            let encoded = block_pos_codec::<NbtOps>()
                .encode_start(&NbtOps, &flower)
                .result()
                .expect("valid flower position")
                .clone();
            tag.put("flower_pos".to_string(), encoded);
        }
        tag.put_int("Bukkit.MaxEntities", self.max_bees);
        tag.put(
            "components".to_string(),
            Tag::Compound(self.components.clone()),
        );
        tag.put_string("id", "minecraft:beehive");
        tag.put_int("x", pos.get_x());
        tag.put_int("y", pos.get_y());
        tag.put_int("z", pos.get_z());
        tag
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::tag_parser::parse_compound_fully;

    #[test]
    fn live_paper_occupant_aging_and_retry_counter_are_distinct() {
        let pos = BlockPos::new(3, 64, -8);
        let mut cases = 0;
        for line in
            include_str!("../../../../tools/rivet-oracle/fixtures/block-entity-ticker.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            if row["kind"] != "hive" {
                continue;
            }
            let input = parse_compound_fully(row["input_snbt"].as_str().unwrap()).unwrap();
            let mut hive = BeehiveData::load(&input);
            for _ in 0..row["ticks"].as_u64().unwrap() {
                assert!(
                    !hive
                        .tick_occupants(row["cooldown"].as_bool().unwrap(), |_, nectar, _| {
                            assert!(nectar);
                            Ok::<_, ()>(false)
                        })
                        .unwrap()
                );
            }
            assert_eq!(
                hive.occupants[0].exit_tick_counter,
                row["exit_counter"].as_i64().unwrap() as i32,
                "{row}"
            );
            assert_eq!(
                hive.save_with_full_metadata(&pos),
                parse_compound_fully(row["output_snbt"].as_str().unwrap()).unwrap(),
                "{row}"
            );
            cases += 1;
        }
        assert_eq!(cases, 30);
    }

    #[test]
    fn load_store_and_full_save_match_live_paper_hive_storage() {
        let pos = BlockPos::new(-17, 70, 31);
        for line in
            include_str!("../../../../tools/rivet-oracle/fixtures/hive-storage.jsonl").lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let input = row["input_snbt"]
                .as_str()
                .map(|text| parse_compound_fully(text).unwrap())
                .unwrap_or_default();
            // Nonempty DataComponentMap payloads are not translated yet.
            // Refuse rather than preserve invalid component data as valid.
            if input
                .get_compound("components")
                .is_some_and(|components| !components.tags.is_empty())
            {
                assert!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| BeehiveData::load(
                        &input
                    )))
                    .is_err(),
                    "unsupported components must not silently pass through"
                );
                continue;
            }
            let mut hive = BeehiveData::load(&input);
            for ticks in row["append"].as_array().unwrap() {
                hive.store_worldgen_bee(ticks.as_i64().unwrap() as i32);
            }
            let expected = parse_compound_fully(row["output_snbt"].as_str().unwrap()).unwrap();
            assert_eq!(
                hive.save_with_full_metadata(&pos),
                expected,
                "{}",
                row["name"]
            );
        }
    }
}
