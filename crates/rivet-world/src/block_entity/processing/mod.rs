//! Container, furnace, and processing block entities (issue #464).
//!
//! Faithful Rust port of Paper 26.2 container and processing block-entity
//! families:
//! - [`WorldlyContainer`] (`net.minecraft.world.WorldlyContainer`)
//! - [`BaseContainerState`] / [`LockCode`] (`BaseContainerBlockEntity`)
//! - [`RandomizableContainer`] (`RandomizableContainer.unpackLootTable`)
//! - [`FuelValues`] (`FuelValues`)
//! - [`ChestBlockEntity`] (`ChestBlockEntity`)
//! - [`BarrelBlockEntity`] (`BarrelBlockEntity`)
//! - [`AbstractFurnaceBlockEntity`] / [`FurnaceKind`] (`AbstractFurnaceBlockEntity`,
//!   `FurnaceBlockEntity`, `BlastFurnaceBlockEntity`, `SmokerBlockEntity`)
//! - [`BrewingStandBlockEntity`] (`BrewingStandBlockEntity`)
//! - [`HopperBlockEntity`] / [`HopperTarget`] (`HopperBlockEntity`)
//! - [`CrafterBlockEntity`] (`CrafterBlockEntity`)

pub mod barrel;
pub mod brewing_stand;
pub mod chest;
pub mod crafter;
pub mod fuel_values;
pub mod furnace;
pub mod hopper;
pub mod nbt;
pub mod randomizable;
pub mod worldly_container;

#[cfg(test)]
mod tests;

pub use barrel::{BARREL_CONTAINER_SIZE, BarrelBlockEntity};
pub use brewing_stand::{
    BREWING_CONTAINER_SIZE, BrewingStandBlockEntity, BrewingTickResult, DATA_BREW_TIME,
    DATA_FUEL_USES, DATA_RECIPE_BREW_TIME, DEFAULT_BREW_TIME, FUEL_SLOT, FUEL_USES,
    INGREDIENT_SLOT, NUM_DATA_VALUES as BREWING_NUM_DATA_VALUES, POTION_COMPONENT_KEY,
    SLOTS_FOR_DOWN as BREWING_SLOTS_FOR_DOWN, SLOTS_FOR_SIDES as BREWING_SLOTS_FOR_SIDES,
    SLOTS_FOR_UP as BREWING_SLOTS_FOR_UP,
};
pub use chest::{CHEST_CONTAINER_SIZE, ChestBlockEntity, EVENT_SET_OPEN_COUNT};
pub use crafter::{
    CONTAINER_HEIGHT as CRAFTER_CONTAINER_HEIGHT, CONTAINER_SIZE as CRAFTER_CONTAINER_SIZE,
    CONTAINER_WIDTH as CRAFTER_CONTAINER_WIDTH, CrafterBlockEntity,
    DATA_TRIGGERED as CRAFTER_DATA_TRIGGERED, DEFAULT_CRAFTING_TICKS_REMAINING, DEFAULT_TRIGGERED,
    NUM_DATA as CRAFTER_NUM_DATA, SLOT_DISABLED, SLOT_ENABLED,
};
pub use fuel_values::{
    BURN_TIME_STANDARD, FuelValues, FuelValuesBuilder, inventory_crafting_remainder,
};
pub use furnace::{
    AbstractFurnaceBlockEntity, BURN_COOL_SPEED, DATA_COOKING_PROGRESS, DATA_COOKING_TOTAL_TIME,
    DATA_LIT_DURATION, DATA_LIT_TIME, FURNACE_CONTAINER_SIZE, FurnaceKind, FurnaceTickResult,
    NUM_DATA_VALUES as FURNACE_NUM_DATA_VALUES, SLOT_FUEL, SLOT_INPUT, SLOT_RESULT,
    SLOTS_FOR_DOWN as FURNACE_SLOTS_FOR_DOWN, SLOTS_FOR_SIDES as FURNACE_SLOTS_FOR_SIDES,
    SLOTS_FOR_UP as FURNACE_SLOTS_FOR_UP, new_blast_furnace, new_furnace, new_smoker,
};
pub use hopper::{
    DEFAULT_COOLDOWN, HOPPER_CONTAINER_SIZE, HOPPER_EMPTY, HOPPER_HAS_ITEMS, HOPPER_IS_FULL,
    HopperBlockEntity, HopperTarget, MOVE_ITEM_SPEED,
};
pub use nbt::{BaseContainerState, LockCode, PaperLootableData, load_all_items, save_all_items};
pub use randomizable::{LootUnpackContext, RandomizableContainer};
pub use worldly_container::WorldlyContainer;
