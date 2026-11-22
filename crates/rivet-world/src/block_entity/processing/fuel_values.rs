//! Paper `net.minecraft.world.level.block.entity.FuelValues`.
//!
//! Mirrors `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/block/entity/FuelValues.java`.
//! Uses `IndexMap` to preserve Java's `Object2IntLinkedOpenHashMap` / `SequencedSet<Item>`
//! insertion order.

use indexmap::IndexMap;

use crate::inventory::{InventoryItemStack, ItemId};

/// Standard burn time unit (`AbstractFurnaceBlockEntity.BURN_TIME_STANDARD = 200`).
pub const BURN_TIME_STANDARD: i32 = 200;

/// Paper `net.minecraft.world.level.block.entity.FuelValues`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FuelValues {
    values: IndexMap<ItemId, i32>,
}

impl FuelValues {
    /// Empty fuel table.
    pub fn empty() -> Self {
        Self {
            values: IndexMap::new(),
        }
    }

    /// Construct a builder.
    pub fn builder() -> FuelValuesBuilder {
        FuelValuesBuilder {
            values: IndexMap::new(),
        }
    }

    /// `FuelValues.vanillaBurnTimes(HolderLookup.Provider, FeatureFlagSet, int)`
    /// with the standard `BURN_TIME_STANDARD = 200` base smelt time.
    pub fn vanilla_burn_times() -> Self {
        Self::vanilla_burn_times_with_smelt_time(BURN_TIME_STANDARD)
    }

    /// `FuelValues.vanillaBurnTimes(..., int smeltTime)`.
    ///
    /// Exact Paper 26.2 fuel durations parameterized by `smelt_time` (default 200).
    pub fn vanilla_burn_times_with_smelt_time(smelt_time: i32) -> Self {
        let mut b = Self::builder();

        // Exact Paper FuelValues.vanillaBurnTimes order & multipliers:
        b.add_name("minecraft:lava_bucket", smelt_time.wrapping_mul(100));
        b.add_name("minecraft:coal_block", smelt_time.wrapping_mul(80));
        b.add_name("minecraft:blaze_rod", smelt_time.wrapping_mul(12));
        b.add_name("minecraft:coal", smelt_time.wrapping_mul(8));
        b.add_name("minecraft:charcoal", smelt_time.wrapping_mul(8));

        // Logs & planks (smelt_time * 3 / 2 = 300)
        let wood_burn = smelt_time.wrapping_mul(3) / 2;
        for name in [
            "minecraft:oak_log",
            "minecraft:spruce_log",
            "minecraft:birch_log",
            "minecraft:jungle_log",
            "minecraft:acacia_log",
            "minecraft:dark_oak_log",
            "minecraft:mangrove_log",
            "minecraft:cherry_log",
            "minecraft:pale_oak_log",
            "minecraft:oak_planks",
            "minecraft:spruce_planks",
            "minecraft:birch_planks",
            "minecraft:jungle_planks",
            "minecraft:acacia_planks",
            "minecraft:dark_oak_planks",
            "minecraft:mangrove_planks",
            "minecraft:cherry_planks",
            "minecraft:pale_oak_planks",
            "minecraft:bamboo_planks",
            "minecraft:bamboo_mosaic",
            "minecraft:bamboo_block",
            "minecraft:stripped_bamboo_block",
            "minecraft:crafting_table",
            "minecraft:chest",
            "minecraft:trapped_chest",
            "minecraft:barrel",
            "minecraft:bookshelf",
            "minecraft:chiseled_bookshelf",
            "minecraft:lectern",
            "minecraft:jukebox",
            "minecraft:note_block",
            "minecraft:composter",
            "minecraft:loom",
            "minecraft:cartography_table",
            "minecraft:fletching_table",
            "minecraft:smithing_table",
            "minecraft:daylight_detector",
            "minecraft:crossbow",
            "minecraft:bow",
            "minecraft:fishing_rod",
            "minecraft:ladder",
            "minecraft:mangrove_roots",
            "minecraft:oak_fence",
            "minecraft:oak_fence_gate",
            "minecraft:oak_stairs",
            "minecraft:oak_trapdoor",
            "minecraft:oak_pressure_plate",
        ] {
            b.add_name(name, wood_burn);
        }

        // Boats (smelt_time * 6 = 1200)
        let boat_burn = smelt_time.wrapping_mul(6);
        for name in [
            "minecraft:oak_boat",
            "minecraft:spruce_boat",
            "minecraft:birch_boat",
            "minecraft:jungle_boat",
            "minecraft:acacia_boat",
            "minecraft:dark_oak_boat",
            "minecraft:mangrove_boat",
            "minecraft:cherry_boat",
            "minecraft:pale_oak_boat",
            "minecraft:bamboo_raft",
        ] {
            b.add_name(name, boat_burn);
        }

        // Dried kelp block (1 + smelt_time * 20 = 4001)
        b.add_name(
            "minecraft:dried_kelp_block",
            1_i32.wrapping_add(smelt_time.wrapping_mul(20)),
        );

        // Wooden doors, signs, tools (smelt_time = 200)
        for name in [
            "minecraft:oak_door",
            "minecraft:oak_sign",
            "minecraft:oak_hanging_sign",
            "minecraft:wooden_pickaxe",
            "minecraft:wooden_shovel",
            "minecraft:wooden_hoe",
            "minecraft:wooden_axe",
            "minecraft:wooden_sword",
        ] {
            b.add_name(name, smelt_time);
        }

        // Slabs (smelt_time * 3 / 4 = 150)
        let slab_burn = smelt_time.wrapping_mul(3) / 4;
        for name in [
            "minecraft:oak_slab",
            "minecraft:spruce_slab",
            "minecraft:birch_slab",
            "minecraft:bamboo_slab",
            "minecraft:bamboo_mosaic_slab",
        ] {
            b.add_name(name, slab_burn);
        }

        // Sticks, saplings, bowls, buttons, wool, azalea, leaf_litter (smelt_time / 2 = 100)
        let half_burn = smelt_time / 2;
        for name in [
            "minecraft:stick",
            "minecraft:bowl",
            "minecraft:oak_sapling",
            "minecraft:spruce_sapling",
            "minecraft:birch_sapling",
            "minecraft:oak_button",
            "minecraft:white_wool",
            "minecraft:azalea",
            "minecraft:flowering_azalea",
            "minecraft:leaf_litter",
        ] {
            b.add_name(name, half_burn);
        }

        // Carpet (1 + smelt_time / 3 = 67)
        b.add_name("minecraft:white_carpet", 1_i32.wrapping_add(smelt_time / 3));

        // Bamboo & scaffolding (smelt_time / 4 = 50)
        let quarter_burn = smelt_time / 4;
        for name in ["minecraft:bamboo", "minecraft:scaffolding"] {
            b.add_name(name, quarter_burn);
        }

        b.build()
    }

    /// `FuelValues.isFuel(ItemStack)`.
    pub fn is_fuel(&self, stack: &InventoryItemStack) -> bool {
        !stack.is_empty() && self.values.contains_key(&stack.item())
    }

    /// `FuelValues.burnDuration(ItemStack)`.
    pub fn burn_duration(&self, stack: &InventoryItemStack) -> i32 {
        if stack.is_empty() {
            0
        } else {
            self.values.get(&stack.item()).copied().unwrap_or(0)
        }
    }

    /// `FuelValues.fuelItems()` — preserves exact insertion order.
    pub fn fuel_items(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.values.keys().copied()
    }
}

/// `FuelValues.Builder`.
#[derive(Debug, Default)]
pub struct FuelValuesBuilder {
    values: IndexMap<ItemId, i32>,
}

impl FuelValuesBuilder {
    /// Register an item with a burn duration in ticks.
    pub fn add(&mut self, item: ItemId, burn_time: i32) -> &mut Self {
        if !item.is_air() && burn_time > 0 {
            self.values.insert(item, burn_time);
        }
        self
    }

    /// Register an item by namespaced name if present in the generated registry.
    pub fn add_name(&mut self, name: &str, burn_time: i32) -> &mut Self {
        if let Some(item) = ItemId::from_name(name) {
            self.add(item, burn_time);
        }
        self
    }

    /// Remove an item from the fuel map.
    pub fn remove(&mut self, item: ItemId) -> &mut Self {
        self.values.shift_remove(&item);
        self
    }

    /// Build the immutable `FuelValues`.
    pub fn build(self) -> FuelValues {
        FuelValues {
            values: self.values,
        }
    }
}

/// Paper `Item.getCraftingRemainder()` seam for inventory items used in
/// furnaces (e.g. `lava_bucket -> bucket`) and brewing stands (e.g.
/// `dragon_breath -> glass_bottle`).
pub fn inventory_crafting_remainder(item: ItemId) -> Option<InventoryItemStack> {
    let remainder_name = match item.name()? {
        "minecraft:lava_bucket"
        | "minecraft:water_bucket"
        | "minecraft:milk_bucket"
        | "minecraft:powder_snow_bucket" => "minecraft:bucket",
        "minecraft:dragon_breath" | "minecraft:honey_bottle" => "minecraft:glass_bottle",
        _ => return None,
    };
    let rem_id = ItemId::from_name(remainder_name)?;
    let max_stack = if remainder_name == "minecraft:bucket" {
        16
    } else {
        64
    };
    Some(InventoryItemStack::with_max_stack_size(
        rem_id, 1, max_stack,
    ))
}
