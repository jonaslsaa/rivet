//! Paper `net.minecraft.world.level.block.entity.BrewingStandBlockEntity`.
//!
//! Mirrors `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/block/entity/BrewingStandBlockEntity.java`.

use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};
use rivet_registry::core::Direction;

use crate::effect::{PotionBrewing, PotionContents, PotionId};
use crate::inventory::{
    ComponentMap, Container, ContainerAccessContext, ContainerData, ContainerHelper,
    InventoryItemStack, ItemId,
};

use super::fuel_values::inventory_crafting_remainder;
use super::nbt::{
    BaseContainerState, decode_component_tag, encode_component_tag, item_stack, load_all_items,
    save_all_items,
};
use super::worldly_container::WorldlyContainer;

/// `BrewingStandBlockEntity.INGREDIENT_SLOT = 3`.
pub const INGREDIENT_SLOT: usize = 3;
/// `BrewingStandBlockEntity.FUEL_SLOT = 4`.
pub const FUEL_SLOT: usize = 4;
/// Total slots in `BrewingStandBlockEntity` (`5`).
pub const BREWING_CONTAINER_SIZE: usize = 5;

/// `BrewingStandBlockEntity.SLOTS_FOR_UP = {3}`.
pub const SLOTS_FOR_UP: [usize; 1] = [INGREDIENT_SLOT];
/// `BrewingStandBlockEntity.SLOTS_FOR_DOWN = {0, 1, 2, 3}`.
pub const SLOTS_FOR_DOWN: [usize; 4] = [0, 1, 2, INGREDIENT_SLOT];
/// `BrewingStandBlockEntity.SLOTS_FOR_SIDES = {0, 1, 2, 4}`.
pub const SLOTS_FOR_SIDES: [usize; 4] = [0, 1, 2, FUEL_SLOT];

/// `BrewingStandBlockEntity.FUEL_USES = 20`.
pub const FUEL_USES: i32 = 20;
/// Default brew duration in ticks (`400`).
pub const DEFAULT_BREW_TIME: i32 = 400;

/// `BrewingStandBlockEntity.DATA_BREW_TIME = 0`.
pub const DATA_BREW_TIME: usize = 0;
/// `BrewingStandBlockEntity.DATA_FUEL_USES = 1`.
pub const DATA_FUEL_USES: usize = 1;
/// Paper `DATA_RECIPE_BREW_TIME = 2`.
pub const DATA_RECIPE_BREW_TIME: usize = 2;
/// `BrewingStandBlockEntity.NUM_DATA_VALUES = 3`.
pub const NUM_DATA_VALUES: usize = 3;

/// `DataComponents.POTION_CONTENTS` patch key on potion item stacks.
pub const POTION_COMPONENT_KEY: &str = "minecraft:potion_contents";

/// Observable outcomes of one `BrewingStandBlockEntity.serverTick(...)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BrewingTickResult {
    /// Whether brewing completed this tick.
    pub brewed: bool,
    /// Whether the 3 bottle-presence bits changed this tick.
    pub bottle_bits_changed: bool,
    /// Current bottle-presence bits for slots `0..3`.
    pub bottle_bits: [bool; 3],
    /// Any crafting remainders dropped into the world because the ingredient
    /// stack was not yet empty after consuming 1 item.
    pub dropped_remainders: Vec<InventoryItemStack>,
}

/// Paper `net.minecraft.world.level.block.entity.BrewingStandBlockEntity`.
#[derive(Debug, Clone)]
pub struct BrewingStandBlockEntity {
    pub base: BaseContainerState,
    items: [InventoryItemStack; BREWING_CONTAINER_SIZE],
    pub brew_time: i32,
    pub recipe_brew_time: i32,
    pub fuel: i32,
    /// Paper `lastPotionCount` (`null` until the first tick).
    last_potion_count: Option<[bool; 3]>,
    ingredient: ItemId,
    potion_brewing: PotionBrewing,
}

impl Default for BrewingStandBlockEntity {
    fn default() -> Self {
        Self::new()
    }
}

impl BrewingStandBlockEntity {
    /// Construct an empty brewing stand backed by vanilla `PotionBrewing`.
    pub fn new() -> Self {
        Self::with_potion_brewing(PotionBrewing::vanilla())
    }

    /// Construct a brewing stand with an explicit `PotionBrewing` registry.
    pub fn with_potion_brewing(potion_brewing: PotionBrewing) -> Self {
        Self {
            base: BaseContainerState::new(),
            items: std::array::from_fn(|_| InventoryItemStack::empty()),
            brew_time: 0,
            recipe_brew_time: DEFAULT_BREW_TIME,
            fuel: 0,
            last_potion_count: None,
            ingredient: ItemId::AIR,
            potion_brewing,
        }
    }

    /// `PotionContents.createItemStack(item, potion)`: a single potion item
    /// whose `minecraft:potion_contents` is encoded with
    /// `PotionContents.FULL_CODEC` (`{potion:"minecraft:..."}`).
    pub fn make_potion_stack(container_name: &str, potion: PotionId) -> InventoryItemStack {
        let Some(item_id) = ItemId::from_name(container_name) else {
            return InventoryItemStack::empty();
        };
        let mut contents = CompoundTag::new();
        contents.put_string("potion", potion.def().identifier);
        let mut comps = ComponentMap::new();
        comps.insert(
            POTION_COMPONENT_KEY.to_string(),
            encode_component_tag(&Tag::Compound(contents)),
        );
        item_stack(item_id, 1).with_components(comps)
    }

    /// The `PotionContents.potion()` on a stack, decoded with
    /// `PotionContents.CODEC` (`FULL_CODEC` compound, or the bare `Potion.CODEC`
    /// id alternative).
    pub fn potion_of_stack(stack: &InventoryItemStack) -> Option<PotionId> {
        let value = stack.components().get(POTION_COMPONENT_KEY)?;
        match decode_component_tag(value) {
            Tag::Compound(contents) => PotionId::by_name(contents.get_string("potion")?),
            Tag::String(id) => PotionId::by_name(&id.value),
            _ => None,
        }
    }

    /// `BrewingStandBlockEntity.getPotionBits()`.
    pub fn get_potion_bits(&self) -> [bool; 3] {
        [
            !self.items[0].is_empty(),
            !self.items[1].is_empty(),
            !self.items[2].is_empty(),
        ]
    }

    /// `BrewingStandBlockEntity.isBrewable(PotionBrewing, NonNullList<ItemStack>)`.
    pub fn is_brewable(&self, potion_brewing: &PotionBrewing) -> bool {
        let ingredient = &self.items[INGREDIENT_SLOT];
        if ingredient.is_empty() {
            return false;
        }
        let Some(ing_name) = ingredient.item().name() else {
            return false;
        };
        if !potion_brewing.is_ingredient(ing_name) {
            return false;
        }
        for dest in 0..3 {
            if stack_has_mix(&self.items[dest], ing_name, potion_brewing) {
                return true;
            }
        }
        false
    }

    /// `BrewingStandBlockEntity.serverTick(Level, BlockPos, BlockState, BrewingStandBlockEntity)`.
    pub fn server_tick(&mut self, potion_brewing: &PotionBrewing) -> BrewingTickResult {
        let mut result = BrewingTickResult::default();

        // 1. Load fuel from slot 4 if empty and blaze_powder present.
        if self.fuel <= 0 && is_brewing_fuel(&self.items[FUEL_SLOT]) {
            self.fuel = FUEL_USES;
            self.items[FUEL_SLOT].shrink(1);
            self.set_changed();
        }

        // 2. Check brewability and active timer.
        let brewable = self.is_brewable(potion_brewing);
        let is_brewing = self.brew_time > 0;
        let ingredient_item = self.items[INGREDIENT_SLOT].item();

        if is_brewing {
            self.brew_time = self.brew_time.wrapping_sub(1);
            let is_done = self.brew_time == 0;
            if is_done && brewable {
                result.dropped_remainders = self.do_brew(potion_brewing);
                result.brewed = true;
            } else if !brewable || ingredient_item != self.ingredient {
                self.brew_time = 0;
            }
            self.set_changed();
        } else if brewable && self.fuel > 0 {
            self.fuel = self.fuel.wrapping_sub(1);
            self.brew_time = DEFAULT_BREW_TIME;
            self.recipe_brew_time = DEFAULT_BREW_TIME;
            self.ingredient = ingredient_item;
            self.set_changed();
        }

        // 3. Update bottle presence bits.
        let bits = self.get_potion_bits();
        if self.last_potion_count != Some(bits) {
            self.last_potion_count = Some(bits);
            result.bottle_bits_changed = true;
        }
        result.bottle_bits = bits;
        result
    }

    /// `BrewingStandBlockEntity.doBrew(Level, BlockPos, NonNullList<ItemStack>)`.
    fn do_brew(&mut self, potion_brewing: &PotionBrewing) -> Vec<InventoryItemStack> {
        let mut dropped = Vec::new();
        let ingredient_item = self.items[INGREDIENT_SLOT].item();
        let Some(ing_name) = ingredient_item.name() else {
            return dropped;
        };

        for dest in 0..3 {
            if let Some(mixed) = mix_stack(&self.items[dest], ing_name, potion_brewing) {
                self.items[dest] = mixed;
            }
        }

        self.items[INGREDIENT_SLOT].shrink(1);
        // `ingredient.getItem().getCraftingRemainder()` after the shrink: an
        // emptied stack reports `Items.AIR`, which has no remainder.
        if let Some(remainder) = inventory_crafting_remainder(self.items[INGREDIENT_SLOT].item()) {
            if self.items[INGREDIENT_SLOT].is_empty() {
                self.items[INGREDIENT_SLOT] = remainder;
            } else {
                dropped.push(remainder);
            }
        }

        dropped
    }

    /// `BrewingStandBlockEntity.saveAdditional(ValueOutput)`.
    pub fn save_additional(&self, output: &mut CompoundTag) {
        self.base.save_base(output);
        output.put_short("BrewTime", self.brew_time as i16);
        save_all_items(output, &self.items, true);
        output.put_byte("Fuel", self.fuel as i8);
    }

    /// `BrewingStandBlockEntity.loadAdditional(ValueInput)`.
    pub fn load_additional(&mut self, input: &CompoundTag) {
        self.base.load_base(input);
        load_all_items(input, &mut self.items);
        self.brew_time = i32::from(input.get_short_or("BrewTime", 0));
        if self.brew_time > 0 {
            self.ingredient = self.items[INGREDIENT_SLOT].item();
        }
        self.fuel = i32::from(input.get_byte_or("Fuel", 0));
    }
}

impl Container for BrewingStandBlockEntity {
    fn get_container_size(&self) -> usize {
        BREWING_CONTAINER_SIZE
    }

    fn is_empty(&self) -> bool {
        self.items.iter().all(InventoryItemStack::is_empty)
    }

    fn get_item(&self, slot: usize) -> &InventoryItemStack {
        self.items.get(slot).unwrap_or_else(|| &EMPTY_STACK)
    }

    fn remove_item(&mut self, slot: usize, count: i32) -> InventoryItemStack {
        let result = ContainerHelper::remove_item(&mut self.items, slot, count);
        if !result.is_empty() {
            self.set_changed();
        }
        result
    }

    fn remove_item_no_update(&mut self, slot: usize) -> InventoryItemStack {
        ContainerHelper::take_item(&mut self.items, slot)
    }

    fn set_item(&mut self, slot: usize, mut stack: InventoryItemStack) {
        if slot >= BREWING_CONTAINER_SIZE {
            return;
        }
        let max_size = self.get_max_stack_size_for_item(&stack);
        stack.limit_size(max_size);
        self.items[slot] = stack;
        self.set_changed();
    }

    fn set_changed(&mut self) {
        self.base.set_changed();
    }

    fn still_valid(&self, ctx: &ContainerAccessContext) -> bool {
        !ctx.is_removed && ctx.can_interact
    }

    fn can_place_item(&self, slot: usize, stack: &InventoryItemStack) -> bool {
        if slot == INGREDIENT_SLOT {
            return stack
                .item()
                .name()
                .is_some_and(|name| self.potion_brewing.is_ingredient(name));
        }
        if slot == FUEL_SLOT {
            return is_brewing_fuel(stack);
        }
        if slot < 3 {
            let valid_bottle = matches!(
                stack.item().name(),
                Some(
                    "minecraft:potion"
                        | "minecraft:splash_potion"
                        | "minecraft:lingering_potion"
                        | "minecraft:glass_bottle"
                )
            );
            return valid_bottle && self.items[slot].is_empty();
        }
        false
    }

    fn clear_content(&mut self) {
        for slot in &mut self.items {
            *slot = InventoryItemStack::empty();
        }
    }
}

impl WorldlyContainer for BrewingStandBlockEntity {
    fn get_slots_for_face(&self, direction: Direction) -> &[usize] {
        if direction == Direction::Up {
            &SLOTS_FOR_UP
        } else if direction == Direction::Down {
            &SLOTS_FOR_DOWN
        } else {
            &SLOTS_FOR_SIDES
        }
    }

    fn can_place_item_through_face(
        &self,
        slot: usize,
        stack: &InventoryItemStack,
        _direction: Option<Direction>,
    ) -> bool {
        self.can_place_item(slot, stack)
    }

    fn can_take_item_through_face(
        &self,
        slot: usize,
        stack: &InventoryItemStack,
        _direction: Direction,
    ) -> bool {
        slot != INGREDIENT_SLOT || stack.item().name() == Some("minecraft:glass_bottle")
    }
}

impl ContainerData for BrewingStandBlockEntity {
    fn get(&self, index: usize) -> i32 {
        match index {
            DATA_BREW_TIME => self.brew_time,
            DATA_FUEL_USES => self.fuel,
            DATA_RECIPE_BREW_TIME => self.recipe_brew_time,
            _ => 0,
        }
    }

    fn set(&mut self, index: usize, value: i32) {
        match index {
            DATA_BREW_TIME => self.brew_time = value,
            DATA_FUEL_USES => self.fuel = value,
            DATA_RECIPE_BREW_TIME => self.recipe_brew_time = value,
            _ => {}
        }
    }

    fn get_count(&self) -> usize {
        NUM_DATA_VALUES
    }
}

fn is_brewing_fuel(stack: &InventoryItemStack) -> bool {
    !stack.is_empty() && stack.item().name() == Some("minecraft:blaze_powder")
}

fn canonical_potion_container(name: &str) -> Option<&'static str> {
    match name {
        "minecraft:potion" => Some("minecraft:potion"),
        "minecraft:splash_potion" => Some("minecraft:splash_potion"),
        "minecraft:lingering_potion" => Some("minecraft:lingering_potion"),
        _ => None,
    }
}

fn stack_has_mix(
    stack: &InventoryItemStack,
    ingredient_name: &str,
    potion_brewing: &PotionBrewing,
) -> bool {
    if stack.is_empty() {
        return false;
    }
    let Some(container) = stack.item().name().and_then(canonical_potion_container) else {
        return false;
    };
    let Some(potion) = BrewingStandBlockEntity::potion_of_stack(stack) else {
        return false;
    };
    potion_brewing.has_mix(
        container,
        &PotionContents::from_potion(potion),
        ingredient_name,
    )
}

fn mix_stack(
    stack: &InventoryItemStack,
    ingredient_name: &str,
    potion_brewing: &PotionBrewing,
) -> Option<InventoryItemStack> {
    if stack.is_empty() {
        return None;
    }
    let container = stack.item().name().and_then(canonical_potion_container)?;
    let potion = BrewingStandBlockEntity::potion_of_stack(stack)?;
    let (new_container, new_contents) = potion_brewing.mix(
        ingredient_name,
        container,
        &PotionContents::from_potion(potion),
    )?;
    let new_potion = new_contents.potion?;
    Some(BrewingStandBlockEntity::make_potion_stack(
        new_container,
        new_potion,
    ))
}

static EMPTY_STACK: std::sync::LazyLock<InventoryItemStack> =
    std::sync::LazyLock::new(InventoryItemStack::empty);
