//! Native chest-minecart construction, persisted loading and full save.
//!
//! This is a typed constructor/load/save slice, not an arbitrary-NBT wrapper.
//! It implements the constructor chain through Entity, VehicleEntity,
//! AbstractMinecart, AbstractMinecartContainer and MinecartChest. Movement,
//! ticking, nonempty inventories, component names and passenger riding graphs
//! remain explicit capability boundaries. Server-side type dispatch and source
//! invalid-entity skipping live at the real owning-world load boundary.
//! World identity and entity-ID allocation belong to the actual owning world;
//! UUID generation uses the entity random source, never the structure RNG.

use rivet_nbt::{compound_tag::CompoundTag, double_tag::DoubleTag, list_tag::ListTag, tag::Tag};
#[path = "chest_minecart_data.rs"]
mod saved_data;
use rivet_registry::{
    Identifier,
    core::{BlockPos, Vec3},
};
use rivet_util::{
    mth::{Uuid, create_insecure_uuid},
    random_source::RandomSource,
};
pub use saved_data::{MinecartLoadError, NativeTriState};

/// Values supplied by the owning level, not inferred from a structure seed.
#[derive(Debug, Clone, Copy)]
pub struct EntityConstructionContext {
    pub entity_id: i32,
    pub world_uuid: Uuid,
    pub experimental_minecart_movement: bool,
}

/// Source-selected behavior constructor. Neither variant implies tick support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinecartMovement {
    Old,
    Experimental,
}

/// Constructor inventory state. Nonempty item construction/loading is outside
/// this slice; no arbitrary CompoundTag is accepted as a working ItemStack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshMinecartItemStack {
    Empty,
}

/// Genuine native, un-ticked chest minecart, fresh or loaded into typed state.
/// All 27 inventory slots are ItemStack.EMPTY in this executable slice; a
/// nonempty stored inventory refuses instead of retaining fake ItemStacks.
#[derive(Debug)]
pub struct NativeChestMinecart {
    entity_id: i32,
    uuid: Uuid,
    world_uuid: Uuid,
    position: Vec3,
    old_position: Vec3,
    movement: MinecartMovement,
    loot_table: Option<Identifier>,
    loot_seed: i64,
    inventory: [FreshMinecartItemStack; Self::CONTAINER_SIZE],
    generation: bool,
    removed: bool,
    saved: saved_data::MinecartSavedState,
}

impl NativeChestMinecart {
    pub const CONTAINER_SIZE: usize = 27;
    pub const WIDTH: f32 = 0.98;
    pub const HEIGHT: f32 = 0.7;
    pub const DISPLAY_OFFSET: i32 = 8;

    /// Entity construction consumes exactly two nextLong calls on the shared
    /// entity RNG. The vanilla MISC chest-minecart type is enabled in the
    /// ordinary world feature set and is allowed in peaceful difficulty.
    pub fn new<R: RandomSource>(context: EntityConstructionContext, entity_random: &mut R) -> Self {
        Self::from_identity(context, create_insecure_uuid(entity_random))
    }

    /// Owning worlds use this after generating the UUID, then allocating the
    /// entity ID: Java runs the UUID field initializer before getNextEntityId.
    pub fn from_identity(context: EntityConstructionContext, uuid: Uuid) -> Self {
        Self {
            entity_id: context.entity_id,
            uuid,
            world_uuid: context.world_uuid,
            position: Vec3::new(0.0, 0.0, 0.0),
            old_position: Vec3::new(0.0, 0.0, 0.0),
            movement: if context.experimental_minecart_movement {
                MinecartMovement::Experimental
            } else {
                MinecartMovement::Old
            },
            loot_table: None,
            loot_seed: 0,
            inventory: [FreshMinecartItemStack::Empty; Self::CONTAINER_SIZE],
            generation: false,
            removed: false,
            saved: saved_data::MinecartSavedState::default(),
        }
    }

    pub fn entity_id(&self) -> i32 {
        self.entity_id
    }
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }
    pub fn set_uuid(&mut self, uuid: Uuid) {
        self.uuid = uuid;
    }
    pub fn is_removed(&self) -> bool {
        self.removed
    }
    pub fn world_uuid(&self) -> Uuid {
        self.world_uuid
    }
    pub fn position(&self) -> Vec3 {
        self.position
    }
    pub fn old_position(&self) -> Vec3 {
        self.old_position
    }
    pub fn delta_movement(&self) -> Vec3 {
        self.saved.motion
    }
    pub fn rotation(&self) -> [f32; 2] {
        self.saved.rotation
    }
    pub fn old_rotation(&self) -> [f32; 2] {
        self.saved.old_rotation
    }
    pub fn movement(&self) -> MinecartMovement {
        self.movement
    }
    pub fn loot_table(&self) -> Option<&Identifier> {
        self.loot_table.as_ref()
    }
    pub fn loot_seed(&self) -> i64 {
        self.loot_seed
    }
    pub fn inventory(&self) -> &[FreshMinecartItemStack] {
        &self.inventory
    }
    pub fn is_alive(&self) -> bool {
        !self.removed
    }
    pub fn generation(&self) -> bool {
        self.generation
    }
    pub fn set_generation(&mut self, generation: bool) {
        self.generation = generation;
    }
    /// Entity.discard's relevant lifecycle transition; discarded entities do
    /// not save. No gameplay death/event/ticking behavior is implied.
    pub fn discard(&mut self) {
        self.removed = true;
    }

    /// AbstractMinecart.setInitialPos invokes Entity.setPos (which refuses
    /// non-finite coordinates), then assigns xo/yo/zo even after that refusal.
    pub fn set_initial_pos(&mut self, position: Vec3) {
        if position.x.is_finite() && position.y.is_finite() && position.z.is_finite() {
            self.position = position;
        }
        self.old_position = position;
    }

    pub fn block_position(&self) -> BlockPos {
        BlockPos::new(
            rivet_util::mth::floor_d(self.position.x),
            rivet_util::mth::floor_d(self.position.y),
            rivet_util::mth::floor_d(self.position.z),
        )
    }

    /// AbstractMinecartContainer.setLootTable: assignment only; no RNG, table
    /// evaluation or inventory fill. The caller draws the seed only after the
    /// real constructor has succeeded, as MineShaftCorridor.createChest does.
    pub fn set_loot_table(&mut self, table: Identifier, seed: i64) {
        self.loot_table = Some(table);
        self.loot_seed = seed;
    }

    /// EntityDimensions.makeBoundingBox uses float width/height before double
    /// promotion. This does not normalize or impose a world-coordinate limit.
    pub fn bounding_box(&self) -> [f64; 6] {
        let half_width = f64::from(Self::WIDTH / 2.0);
        let p = self.position;
        [
            p.x - half_width,
            p.y,
            p.z - half_width,
            p.x + half_width,
            p.y + f64::from(Self::HEIGHT),
            p.z + half_width,
        ]
    }

    /// Full save as invoked by ProtoChunk.addEntity. Fresh constructors do not
    /// invent Paper.SpawnReason/Origin; actual loading initializes its source
    /// DEFAULT/SPAWNER reason and preserves supported typed saved state.
    pub fn save(&self) -> Option<CompoundTag> {
        if self.removed || !self.saved.persist {
            return None;
        }
        let mut tag = CompoundTag::new();
        tag.put_string("id", "minecraft:chest_minecart");
        tag.put(
            "Pos".into(),
            doubles([self.position.x, self.position.y, self.position.z]),
        );
        tag.put_int_array(
            "UUID",
            vec![
                (self.uuid.most >> 32) as i32,
                self.uuid.most as i32,
                (self.uuid.least >> 32) as i32,
                self.uuid.least as i32,
            ],
        );
        tag.put_long("WorldUUIDLeast", self.world_uuid.least);
        tag.put_long("WorldUUIDMost", self.world_uuid.most);
        tag.put_int("Bukkit.updateLevel", 2);
        if let Some(table) = &self.loot_table {
            tag.put_string("LootTable", &table.to_string());
            if self.loot_seed != 0 {
                tag.put_long("LootTableSeed", self.loot_seed);
            }
        }
        tag.put("Items".into(), Tag::List(ListTag::new()));
        self.saved.write(&mut tag);
        Some(tag)
    }
}

#[cfg(test)]
#[path = "chest_minecart_fixture.rs"]
mod fixture;

fn doubles(values: [f64; 3]) -> Tag {
    Tag::List(ListTag::with_list(
        values
            .into_iter()
            .map(|v| Tag::Double(DoubleTag::value_of(v)))
            .collect(),
    ))
}
