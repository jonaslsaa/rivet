//! Paper `net.minecraft.world.level.block.entity.FuelValues`.
//!
//! Mirrors `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/block/entity/FuelValues.java`.
//! Uses `IndexMap` to preserve Java's `Object2IntLinkedOpenHashMap` / `SequencedSet<Item>`
//! insertion order.

use indexmap::IndexMap;
use rivet_registry::generated::tags::ITEM_TAG_BY_NAME;

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

    /// `FuelValues.vanillaBurnTimes(..., int baseUnit)`.
    ///
    /// Exact Paper 26.2 fuel durations parameterized by `base_unit` (default 200),
    /// matching `FuelValues.java` lines 42–110 in order including tag expansion
    /// and `minecraft:non_flammable_wood` removal.
    pub fn vanilla_burn_times_with_smelt_time(base_unit: i32) -> Self {
        let mut b = Self::builder();

        b.add_name("minecraft:lava_bucket", base_unit.wrapping_mul(100))
            .add_name("minecraft:coal_block", base_unit.wrapping_mul(8).wrapping_mul(10))
            .add_name("minecraft:blaze_rod", base_unit.wrapping_mul(12))
            .add_name("minecraft:coal", base_unit.wrapping_mul(8))
            .add_name("minecraft:charcoal", base_unit.wrapping_mul(8))
            .add_tag("minecraft:logs", base_unit.wrapping_mul(3) / 2)
            .add_tag("minecraft:bamboo_blocks", base_unit.wrapping_mul(3) / 2)
            .add_tag("minecraft:planks", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:bamboo_mosaic", base_unit.wrapping_mul(3) / 2)
            .add_tag("minecraft:wooden_stairs", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:bamboo_mosaic_stairs", base_unit.wrapping_mul(3) / 2)
            .add_tag("minecraft:wooden_slabs", base_unit.wrapping_mul(3) / 4)
            .add_name("minecraft:bamboo_mosaic_slab", base_unit.wrapping_mul(3) / 4)
            .add_tag("minecraft:wooden_trapdoors", base_unit.wrapping_mul(3) / 2)
            .add_tag("minecraft:wooden_pressure_plates", base_unit.wrapping_mul(3) / 2)
            .add_tag("minecraft:wooden_shelves", base_unit.wrapping_mul(3) / 2)
            .add_tag("minecraft:wooden_fences", base_unit.wrapping_mul(3) / 2)
            .add_tag("minecraft:fence_gates", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:note_block", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:bookshelf", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:chiseled_bookshelf", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:lectern", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:jukebox", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:chest", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:trapped_chest", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:crafting_table", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:daylight_detector", base_unit.wrapping_mul(3) / 2)
            .add_tag("minecraft:banners", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:bow", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:fishing_rod", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:ladder", base_unit.wrapping_mul(3) / 2)
            .add_tag("minecraft:signs", base_unit)
            .add_tag("minecraft:hanging_signs", base_unit.wrapping_mul(4))
            .add_name("minecraft:wooden_shovel", base_unit)
            .add_name("minecraft:wooden_sword", base_unit)
            .add_name("minecraft:wooden_spear", base_unit)
            .add_name("minecraft:wooden_hoe", base_unit)
            .add_name("minecraft:wooden_axe", base_unit)
            .add_name("minecraft:wooden_pickaxe", base_unit)
            .add_tag("minecraft:wooden_doors", base_unit)
            .add_tag("minecraft:boats", base_unit.wrapping_mul(6))
            .add_tag("minecraft:wool", base_unit / 2)
            .add_tag("minecraft:wooden_buttons", base_unit / 2)
            .add_name("minecraft:stick", base_unit / 2)
            .add_tag("minecraft:saplings", base_unit / 2)
            .add_name("minecraft:bowl", base_unit / 2)
            .add_tag("minecraft:wool_carpets", 1_i32.wrapping_add(base_unit / 3))
            .add_name(
                "minecraft:dried_kelp_block",
                1_i32.wrapping_add(base_unit.wrapping_mul(20)),
            )
            .add_name("minecraft:crossbow", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:bamboo", base_unit / 4)
            .add_name("minecraft:dead_bush", base_unit / 2)
            .add_name("minecraft:short_dry_grass", base_unit / 2)
            .add_name("minecraft:tall_dry_grass", base_unit / 2)
            .add_name("minecraft:scaffolding", base_unit / 4)
            .add_name("minecraft:loom", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:barrel", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:cartography_table", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:fletching_table", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:smithing_table", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:composter", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:azalea", base_unit / 2)
            .add_name("minecraft:flowering_azalea", base_unit / 2)
            .add_name("minecraft:mangrove_roots", base_unit.wrapping_mul(3) / 2)
            .add_name("minecraft:leaf_litter", base_unit / 2)
            .remove_tag("minecraft:non_flammable_wood");

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

    /// Register every item in a vanilla item tag (`FuelValues.Builder.add(TagKey<Item>, int)`).
    pub fn add_tag(&mut self, tag: &str, burn_time: i32) -> &mut Self {
        if let Some(items) = ITEM_TAG_BY_NAME.get(tag) {
            for &name in *items {
                self.add_name(name, burn_time);
            }
        }
        self
    }

    /// Remove an item from the fuel map.
    pub fn remove(&mut self, item: ItemId) -> &mut Self {
        self.values.shift_remove(&item);
        self
    }

    /// Remove every item in a vanilla item tag (`FuelValues.Builder.remove(TagKey<Item>)`).
    pub fn remove_tag(&mut self, tag: &str) -> &mut Self {
        if let Some(items) = ITEM_TAG_BY_NAME.get(tag) {
            for &name in *items {
                if let Some(item) = ItemId::from_name(name) {
                    self.remove(item);
                }
            }
        }
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
