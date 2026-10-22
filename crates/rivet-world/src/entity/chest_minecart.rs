//! Fresh `EntityTypes.CHEST_MINECART.create(level, CHUNK_GENERATION)` state.
//!
//! This is a native constructor/save slice, not an arbitrary-NBT entity loader.
//! It implements the constructor chain through Entity, VehicleEntity,
//! AbstractMinecart, AbstractMinecartContainer and MinecartChest. Movement,
//! ticking, inventory interactions and recursive entity loading are not exposed.
//! World identity and entity-ID allocation belong to the actual owning world;
//! UUID generation uses the entity random source, never the structure RNG.

use rivet_nbt::{
    compound_tag::CompoundTag, double_tag::DoubleTag, float_tag::FloatTag, list_tag::ListTag,
    tag::Tag,
};
use rivet_registry::{
    Identifier,
    core::{BlockPos, Vec3},
};
use rivet_util::{
    mth::{Uuid, create_insecure_uuid},
    random_source::RandomSource,
};

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

/// Genuine newly constructed, un-ticked chest minecart. All 27 inventory slots
/// are initialized to ItemStack.EMPTY; loot assignment does not unpack them.
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
        }
    }

    pub fn entity_id(&self) -> i32 {
        self.entity_id
    }
    pub fn uuid(&self) -> Uuid {
        self.uuid
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

    /// Full save of this fresh nonpassenger, persisting entity, as invoked by
    /// ProtoChunk.addEntity. No Paper.SpawnReason/Origin is invented: direct
    /// EntityType.create does not initialize either. Empty lootable metadata is
    /// discarded by PaperLootableInventoryData; Items is still emitted empty.
    pub fn save(&self) -> Option<CompoundTag> {
        if self.removed {
            return None;
        }
        let mut tag = CompoundTag::new();
        tag.put_string("id", "minecraft:chest_minecart");
        tag.put(
            "Pos".into(),
            doubles([self.position.x, self.position.y, self.position.z]),
        );
        tag.put("Motion".into(), doubles([0.0; 3]));
        tag.put(
            "Rotation".into(),
            Tag::List(ListTag::with_list(vec![
                Tag::Float(FloatTag::value_of(0.0)),
                Tag::Float(FloatTag::value_of(0.0)),
            ])),
        );
        tag.put_double("fall_distance", 0.0);
        tag.put_short("Fire", 0);
        tag.put_short("Air", 300);
        tag.put_boolean("OnGround", false);
        tag.put_boolean("Invulnerable", false);
        tag.put_int("PortalCooldown", 0);
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
        tag.put_int("Spigot.ticksLived", 0);
        tag.put_boolean("FlippedRotation", false);
        // Source writes firstTick under this name; a fresh entity writes true.
        tag.put_boolean("HasTicked", true);
        if let Some(table) = &self.loot_table {
            tag.put_string("LootTable", &table.to_string());
            if self.loot_seed != 0 {
                tag.put_long("LootTableSeed", self.loot_seed);
            }
        }
        tag.put("Items".into(), Tag::List(ListTag::new()));
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
