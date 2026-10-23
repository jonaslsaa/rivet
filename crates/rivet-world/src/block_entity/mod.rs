//! Chunk-owned native block entities. Pending serialized tags remain a
//! separate lazy-load authority. These values implement persistence and
//! identity/state transitions, not server ticking, inventories or entity AI.
pub mod beehive_data;
pub mod creaking_heart_data;
pub mod spawner_data;

use beehive_data::BeehiveData;
pub use creaking_heart_data::HeartData;
use rivet_nbt::{compound_tag::CompoundTag, list_tag::ListTag, tag::Tag};
use rivet_registry::{Identifier, block_state::BlockState, core::BlockPos};
use spawner_data::SpawnerData;
use std::sync::{Arc, Weak};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NativeBlockEntityError {
    #[error("native block entity capability unavailable: {0}")]
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChestData {
    loot: Option<(Identifier, i64)>,
    public_bukkit_values: CompoundTag,
}
impl ChestData {
    pub fn load(input: &CompoundTag) -> Result<Self, NativeBlockEntityError> {
        let mut common = CompoundTag::new();
        common_data(&mut common, input)?;
        if input.get_list("Items").is_some_and(|v| !v.list.is_empty()) {
            return Err(NativeBlockEntityError::Unsupported(
                "nonempty chest item stack codec".into(),
            ));
        }
        for key in ["CustomName", "lock", "Paper.LootableData"] {
            if input.get(key).is_some() {
                return Err(NativeBlockEntityError::Unsupported(format!(
                    "chest {key} codec"
                )));
            }
        }
        let loot = input
            .get_string("LootTable")
            .and_then(|id| Identifier::try_parse_result(id).ok().flatten())
            .map(|id| (id, input.get_long_or("LootTableSeed", 0)));
        Ok(Self {
            loot,
            public_bukkit_values: common
                .get_compound("PublicBukkitValues")
                .cloned()
                .unwrap_or_default(),
        })
    }
    pub fn save(&self, pos: BlockPos) -> CompoundTag {
        let mut tag = base_tag(pos, "minecraft:chest");
        tag.put("Items".into(), Tag::List(ListTag::new()));
        if !self.public_bukkit_values.tags.is_empty() {
            tag.put(
                "PublicBukkitValues".into(),
                Tag::Compound(self.public_bukkit_values.clone()),
            );
        }
        if let Some((id, seed)) = &self.loot {
            tag.put_string("LootTable", &id.to_string());
            if *seed != 0 {
                tag.put_long("LootTableSeed", *seed);
            }
        }
        tag
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NativeBlockEntityData {
    Chest(ChestData),
    Spawner(SpawnerData),
    Beehive(BeehiveData),
    CreakingHeart(HeartData),
}

#[derive(Debug, Clone)]
pub struct NativeBlockEntity {
    position: BlockPos,
    state: BlockState,
    data: NativeBlockEntityData,
    removed: bool,
    identity: Arc<()>,
    level_associated: bool,
}
impl PartialEq for NativeBlockEntity {
    fn eq(&self, other: &Self) -> bool {
        self.position == other.position
            && self.state == other.state
            && self.data == other.data
            && self.removed == other.removed
    }
}
impl NativeBlockEntity {
    pub fn fresh(position: BlockPos, state: BlockState) -> Result<Self, NativeBlockEntityError> {
        let empty = CompoundTag::new();
        let data = match state.block().name() {
            "minecraft:chest" => NativeBlockEntityData::Chest(ChestData::load(&empty)?),
            "minecraft:spawner" => NativeBlockEntityData::Spawner(SpawnerData::fresh()),
            "minecraft:beehive" | "minecraft:bee_nest" => {
                NativeBlockEntityData::Beehive(BeehiveData::load(&empty))
            }
            "minecraft:creaking_heart" => {
                NativeBlockEntityData::CreakingHeart(HeartData::load(&empty)?)
            }
            name => {
                return Err(NativeBlockEntityError::Unsupported(format!(
                    "factory for {name}"
                )));
            }
        };
        Ok(Self {
            position,
            state,
            data,
            removed: false,
            identity: Arc::new(()),
            level_associated: false,
        })
    }
    /// loadStatic resolves the entity id independently, then validates its
    /// allowed block state. Unknown/mismatched types return no live entity.
    pub fn load(
        position: BlockPos,
        state: BlockState,
        input: &CompoundTag,
    ) -> Result<Option<Self>, NativeBlockEntityError> {
        let Some(id) = input
            .get_string("id")
            .and_then(|id| Identifier::try_parse_result(id).ok().flatten())
        else {
            return Ok(None);
        };
        if !rivet_registry::generated::registries::BLOCK_ENTITY_TYPE_BY_NAME
            .contains_key(id.to_string().as_str())
        {
            return Ok(None);
        }
        let data = match id.to_string().as_str() {
            "minecraft:chest" if state.block().name() == "minecraft:chest" => {
                NativeBlockEntityData::Chest(ChestData::load(input)?)
            }
            "minecraft:mob_spawner" if state.block().name() == "minecraft:spawner" => {
                NativeBlockEntityData::Spawner(SpawnerData::load(position, input)?)
            }
            "minecraft:beehive"
                if matches!(
                    state.block().name(),
                    "minecraft:beehive" | "minecraft:bee_nest"
                ) =>
            {
                let mut common = CompoundTag::new();
                common_data(&mut common, input)?;
                NativeBlockEntityData::Beehive(BeehiveData::load(input))
            }
            "minecraft:creaking_heart" if state.block().name() == "minecraft:creaking_heart" => {
                NativeBlockEntityData::CreakingHeart(HeartData::load(input)?)
            }
            "minecraft:chest"
            | "minecraft:mob_spawner"
            | "minecraft:beehive"
            | "minecraft:creaking_heart" => return Ok(None),
            other => {
                return Err(NativeBlockEntityError::Unsupported(format!(
                    "loadStatic for {other}"
                )));
            }
        };
        Ok(Some(Self {
            position,
            state,
            data,
            removed: false,
            identity: Arc::new(()),
            level_associated: false,
        }))
    }
    /// Stable identity for a borrowed live ticker, not a second payload owner.
    /// Payload equality and persistent NBT deliberately exclude this identity.
    pub fn ticker_identity(&self) -> Weak<()> {
        Arc::downgrade(&self.identity)
    }
    pub fn has_level(&self) -> bool {
        self.level_associated
    }
    pub fn associate_level(&mut self) {
        self.level_associated = true;
    }
    pub fn position(&self) -> BlockPos {
        self.position
    }
    pub fn state(&self) -> BlockState {
        self.state
    }
    pub fn data(&self) -> &NativeBlockEntityData {
        &self.data
    }
    pub fn data_mut(&mut self) -> &mut NativeBlockEntityData {
        &mut self.data
    }
    pub fn is_removed(&self) -> bool {
        self.removed
    }
    pub fn set_removed(&mut self) {
        self.removed = true;
    }
    pub fn clear_removed(&mut self) {
        self.removed = false;
    }
    pub fn set_block_state(&mut self, state: BlockState) -> bool {
        let valid = match &self.data {
            NativeBlockEntityData::Chest(_) => state.block().name() == "minecraft:chest",
            NativeBlockEntityData::Spawner(_) => state.block().name() == "minecraft:spawner",
            NativeBlockEntityData::Beehive(_) => matches!(
                state.block().name(),
                "minecraft:beehive" | "minecraft:bee_nest"
            ),
            NativeBlockEntityData::CreakingHeart(_) => {
                state.block().name() == "minecraft:creaking_heart"
            }
        };
        if !valid {
            return false;
        }
        self.state = state;
        true
    }
    pub fn save_with_full_metadata(&self) -> CompoundTag {
        match &self.data {
            NativeBlockEntityData::Chest(data) => data.save(self.position),
            NativeBlockEntityData::Spawner(data) => data.save(self.position),
            NativeBlockEntityData::Beehive(data) => data.save_with_full_metadata(&self.position),
            NativeBlockEntityData::CreakingHeart(data) => data.save(self.position),
        }
    }
}

pub(crate) fn base_tag(pos: BlockPos, id: &str) -> CompoundTag {
    let mut tag = CompoundTag::new();
    tag.put_string("id", id);
    tag.put_int("x", pos.get_x());
    tag.put_int("y", pos.get_y());
    tag.put_int("z", pos.get_z());
    tag.put("components".into(), Tag::Compound(CompoundTag::new()));
    tag
}
pub(crate) fn common_data(
    target: &mut CompoundTag,
    input: &CompoundTag,
) -> Result<(), NativeBlockEntityError> {
    if input
        .get_compound("components")
        .is_some_and(|v| !v.tags.is_empty())
    {
        return Err(NativeBlockEntityError::Unsupported(
            "nonempty DataComponentMap codec".into(),
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
