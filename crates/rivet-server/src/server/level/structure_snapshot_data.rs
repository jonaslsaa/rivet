//! Source CraftBlockStates fresh chest/spawner snapshots and supported payload
//! loading. Nonempty item/component codecs and other BE families refuse
//! explicitly; they are not passed through as if raw NBT were a live entity.
use super::{
    structure_snapshot_blocks::BLOCKS_WITH_ENTITIES, structure_transformer::CraftBlockSnapshot,
};
use rivet_nbt::{compound_tag::CompoundTag, list_tag::ListTag, tag::Tag};
use rivet_registry::{
    Identifier, block_state::BlockState, core::BlockPos, generated::blocks::BlockId,
};
use rivet_world::levelgen::structure::StructureError;

fn unavailable(message: impl Into<String>) -> StructureError {
    StructureError::PlacementUnavailable(message.into())
}
pub(super) fn has_block_entity(state: BlockState) -> bool {
    BLOCKS_WITH_ENTITIES.contains(&state.block().name())
}
fn base(pos: BlockPos, id: &str) -> CompoundTag {
    let mut tag = CompoundTag::new();
    tag.put_string("id", id);
    tag.put_int("x", pos.get_x());
    tag.put_int("y", pos.get_y());
    tag.put_int("z", pos.get_z());
    tag.put("components".into(), Tag::Compound(CompoundTag::new()));
    tag
}
pub fn fresh_snapshot(
    pos: BlockPos,
    state: BlockState,
) -> Result<CraftBlockSnapshot, StructureError> {
    let data = match state.block().name() {
        "minecraft:chest" => {
            let mut tag = base(pos, "minecraft:chest");
            tag.put("Items".into(), Tag::List(ListTag::new()));
            Some(tag)
        }
        "minecraft:spawner" => {
            let mut tag = base(pos, "minecraft:mob_spawner");
            for (name, value) in [
                ("Delay", 20),
                ("MinSpawnDelay", 200),
                ("MaxSpawnDelay", 800),
                ("SpawnCount", 4),
                ("MaxNearbyEntities", 6),
                ("RequiredPlayerRange", 16),
                ("SpawnRange", 4),
            ] {
                tag.put_short(name, value);
            }
            tag.put("SpawnPotentials".into(), Tag::List(ListTag::new()));
            Some(tag)
        }
        _ if has_block_entity(state) => {
            return Err(unavailable(format!(
                "Craft snapshot factory for {} is unavailable",
                state.block().name()
            )));
        }
        _ => None,
    };
    Ok(CraftBlockSnapshot {
        position: pos,
        state,
        block_entity: data,
    })
}
pub fn loot_chest_snapshot(
    pos: BlockPos,
    state: BlockState,
    loot: &str,
    seed: i64,
) -> Result<CraftBlockSnapshot, StructureError> {
    let mut snapshot = fresh_snapshot(pos, state)?;
    if state.block().name() != "minecraft:chest" {
        return Err(unavailable("CraftChest requires chest state"));
    }
    let key = Identifier::try_parse_result(loot)
        .ok()
        .flatten()
        .ok_or_else(|| unavailable(format!("invalid loot table key {loot}")))?;
    let data = snapshot.block_entity.as_mut().expect("fresh chest entity");
    data.put_string("LootTable", &key.to_string());
    if seed != 0 {
        data.put_long("LootTableSeed", seed);
    }
    Ok(snapshot)
}
pub fn cave_spider_spawner_snapshot(pos: BlockPos) -> Result<CraftBlockSnapshot, StructureError> {
    let state =
        BlockState::of(BlockId::from_name("minecraft:spawner").expect("registered spawner"));
    let mut snapshot = fresh_snapshot(pos, state)?;
    // Fresh Craft snapshot has no SpawnData and an EMPTY weighted list. The
    // source getRandom returns Optional.empty without drawing world RNG.
    let mut entity = CompoundTag::new();
    entity.put_string("id", "minecraft:cave_spider");
    let mut spawn = CompoundTag::new();
    spawn.put("entity".into(), Tag::Compound(entity));
    snapshot
        .block_entity
        .as_mut()
        .expect("fresh spawner entity")
        .put("SpawnData".into(), Tag::Compound(spawn));
    Ok(snapshot)
}
pub(super) fn common_data(
    target: &mut CompoundTag,
    input: &CompoundTag,
) -> Result<(), StructureError> {
    if input
        .get_compound("components")
        .is_some_and(|v| !v.tags.is_empty())
    {
        return Err(unavailable(
            "nonempty block-entity DataComponentMap codec is unavailable",
        ));
    }
    if let Some(values) = input
        .get_compound("PublicBukkitValues")
        .filter(|v| !v.tags.is_empty())
    {
        target.put("PublicBukkitValues".into(), Tag::Compound(values.clone()));
    }
    Ok(())
}
pub(super) fn chest_data(
    pos: BlockPos,
    input: &CompoundTag,
) -> Result<CompoundTag, StructureError> {
    if input.get_list("Items").is_some_and(|v| !v.list.is_empty()) {
        return Err(unavailable(
            "nonempty CraftChest item stack codec is unavailable",
        ));
    }
    for field in ["CustomName", "lock", "Paper.LootableData"] {
        if input.get(field).is_some() {
            return Err(unavailable(format!(
                "CraftChest {field} codec is unavailable"
            )));
        }
    }
    let state = BlockState::of(BlockId::from_name("minecraft:chest").expect("registered chest"));
    let mut result = fresh_snapshot(pos, state)?
        .block_entity
        .expect("fresh chest");
    common_data(&mut result, input)?;
    if let Some(key) = input
        .get_string("LootTable")
        .and_then(|id| Identifier::try_parse_result(id).ok().flatten())
    {
        result.put_string("LootTable", &key.to_string());
        let seed = input.get_long_or("LootTableSeed", 0);
        if seed != 0 {
            result.put_long("LootTableSeed", seed);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::tag_parser::parse_compound_fully;
    #[test]
    fn fresh_craft_snapshots_match_actual_paper_factories_and_setters() {
        let p = BlockPos::new(8, 64, 8);
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/structure-snapshots.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let name = row["name"].as_str().unwrap();
            if name == "block_entity_types" {
                let actual: Vec<&str> = row["types"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                assert_eq!(BLOCKS_WITH_ENTITIES, actual);
                continue;
            }
            let state = BlockState::new(rivet_registry::generated::block_states::StateId(
                row["state"].as_u64().unwrap() as u16,
            ));
            if let Some(input) = row["input"].as_str() {
                let data = parse_compound_fully(input).unwrap();
                let loaded = if name.starts_with("load_chest") {
                    chest_data(p, &data).unwrap()
                } else {
                    super::super::world_gen_region::structure_spawner_data(p, &data).unwrap()
                };
                assert_eq!(
                    loaded,
                    parse_compound_fully(row["nbt"].as_str().unwrap()).unwrap(),
                    "{name}"
                );
                continue;
            }
            let snapshot = match name {
                "fresh_chest" | "fresh_spawner" => fresh_snapshot(p, state).unwrap(),
                "loot_chest" => {
                    loot_chest_snapshot(p, state, "minecraft:chests/buried_treasure", 42).unwrap()
                }
                "cave_spider_spawner" => cave_spider_spawner_snapshot(p).unwrap(),
                _ => panic!("unknown snapshot {name}"),
            };
            assert_eq!(snapshot.state, state);
            assert_eq!(
                snapshot.block_entity.unwrap(),
                parse_compound_fully(row["nbt"].as_str().unwrap()).unwrap(),
                "{name}"
            );
        }
    }
}
