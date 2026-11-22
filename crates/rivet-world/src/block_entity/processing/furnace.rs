//! Paper `AbstractFurnaceBlockEntity`, `FurnaceBlockEntity`,
//! `BlastFurnaceBlockEntity`, and `SmokerBlockEntity`.
//!
//! Mirrors Paper 26.2:
//! - `net.minecraft.world.level.block.entity.AbstractFurnaceBlockEntity`
//! - `net.minecraft.world.level.block.entity.FurnaceBlockEntity`
//! - `net.minecraft.world.level.block.entity.BlastFurnaceBlockEntity`
//! - `net.minecraft.world.level.block.entity.SmokerBlockEntity`

use std::collections::BTreeMap;

use indexmap::IndexMap;
use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};
use rivet_registry::{Identifier, core::Direction};

use crate::crafting::{
    CookingKind, CookingRecipe, ItemId as CraftingItemId, ItemStack as CraftingItemStack, Recipe,
    RecipeKey, RecipeManager, SingleRecipeInput,
};
use crate::inventory::{
    ComponentMap, Container, ContainerAccessContext, ContainerData, ContainerHelper,
    InventoryItemStack, ItemId,
};

use super::fuel_values::{BURN_TIME_STANDARD, FuelValues, inventory_crafting_remainder};
use super::nbt::{BaseContainerState, load_all_items, save_all_items};
use super::worldly_container::WorldlyContainer;

/// `AbstractFurnaceBlockEntity.SLOT_INPUT = 0`.
pub const SLOT_INPUT: usize = 0;
/// `AbstractFurnaceBlockEntity.SLOT_FUEL = 1`.
pub const SLOT_FUEL: usize = 1;
/// `AbstractFurnaceBlockEntity.SLOT_RESULT = 2`.
pub const SLOT_RESULT: usize = 2;
/// Total slots in any furnace block entity (`3`).
pub const FURNACE_CONTAINER_SIZE: usize = 3;

/// `AbstractFurnaceBlockEntity.SLOTS_FOR_UP = {0}`.
pub const SLOTS_FOR_UP: [usize; 1] = [SLOT_INPUT];
/// `AbstractFurnaceBlockEntity.SLOTS_FOR_DOWN = {2, 1}`.
pub const SLOTS_FOR_DOWN: [usize; 2] = [SLOT_RESULT, SLOT_FUEL];
/// `AbstractFurnaceBlockEntity.SLOTS_FOR_SIDES = {1}`.
pub const SLOTS_FOR_SIDES: [usize; 1] = [SLOT_FUEL];

/// `AbstractFurnaceBlockEntity.DATA_LIT_TIME = 0`.
pub const DATA_LIT_TIME: usize = 0;
/// `AbstractFurnaceBlockEntity.DATA_LIT_DURATION = 1`.
pub const DATA_LIT_DURATION: usize = 1;
/// `AbstractFurnaceBlockEntity.DATA_COOKING_PROGRESS = 2`.
pub const DATA_COOKING_PROGRESS: usize = 2;
/// `AbstractFurnaceBlockEntity.DATA_COOKING_TOTAL_TIME = 3`.
pub const DATA_COOKING_TOTAL_TIME: usize = 3;
/// `AbstractFurnaceBlockEntity.NUM_DATA_VALUES = 4`.
pub const NUM_DATA_VALUES: usize = 4;

/// `AbstractFurnaceBlockEntity.BURN_COOL_SPEED = 2`.
pub const BURN_COOL_SPEED: i32 = 2;

/// Concrete Paper furnace subtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FurnaceKind {
    /// `FurnaceBlockEntity` (`RecipeType.SMELTING`, full burn duration).
    Furnace,
    /// `BlastFurnaceBlockEntity` (`RecipeType.BLASTING`, halved burn duration).
    BlastFurnace,
    /// `SmokerBlockEntity` (`RecipeType.SMOKING`, halved burn duration).
    Smoker,
}

impl FurnaceKind {
    /// Cooking recipe kind handled by this furnace subtype.
    pub const fn cooking_kind(self) -> CookingKind {
        match self {
            Self::Furnace => CookingKind::Smelting,
            Self::BlastFurnace => CookingKind::Blasting,
            Self::Smoker => CookingKind::Smoking,
        }
    }

    /// Paper `BlastFurnaceBlockEntity.getBurnDuration` / `SmokerBlockEntity.getBurnDuration`
    /// halve the standard burn duration (`super.getBurnDuration(...) / 2`).
    pub const fn scales_burn_duration_half(self) -> bool {
        matches!(self, Self::BlastFurnace | Self::Smoker)
    }
}

/// Observable outcomes of one `AbstractFurnaceBlockEntity.serverTick(...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FurnaceTickResult {
    /// Whether the block's `LIT` state changed this tick.
    pub lit_changed: bool,
    /// Current `isLit()` state after the tick.
    pub is_lit: bool,
    /// Whether an item finished smelting this tick.
    pub burned_item: bool,
    /// Whether the block entity was marked changed (`setChanged`) this tick.
    pub dirty: bool,
}

/// Paper `net.minecraft.world.level.block.entity.AbstractFurnaceBlockEntity`.
#[derive(Debug, Clone, PartialEq)]
pub struct AbstractFurnaceBlockEntity {
    pub base: BaseContainerState,
    kind: FurnaceKind,
    items: [InventoryItemStack; FURNACE_CONTAINER_SIZE],
    pub lit_time_remaining: i32,
    pub lit_total_time: i32,
    pub cooking_timer: i32,
    pub cooking_total_time: i32,
    /// Paper's `Paper.CookSpeedMultiplier` (default `1.0`).
    pub cook_speed_multiplier: f64,
    /// `Reference2IntOpenHashMap<ResourceKey<Recipe<?>>>` (insertion-ordered).
    recipes_used: IndexMap<Identifier, i32>,
    last_recipe: Option<RecipeKey>,
    fuel_values: FuelValues,
}

impl AbstractFurnaceBlockEntity {
    /// Construct a new furnace block entity of `kind` with vanilla fuel values.
    pub fn new(kind: FurnaceKind) -> Self {
        Self::with_fuel_values(kind, FuelValues::vanilla_burn_times())
    }

    /// Construct a new furnace block entity with explicit `FuelValues`.
    pub fn with_fuel_values(kind: FurnaceKind, fuel_values: FuelValues) -> Self {
        Self {
            base: BaseContainerState::new(),
            kind,
            items: std::array::from_fn(|_| InventoryItemStack::empty()),
            lit_time_remaining: 0,
            lit_total_time: 0,
            cooking_timer: 0,
            cooking_total_time: 0,
            cook_speed_multiplier: 1.0,
            recipes_used: IndexMap::new(),
            last_recipe: None,
            fuel_values,
        }
    }

    /// Furnace subtype (`Furnace`, `BlastFurnace`, or `Smoker`).
    pub const fn kind(&self) -> FurnaceKind {
        self.kind
    }

    /// `AbstractFurnaceBlockEntity.isLit()`.
    pub const fn is_lit(&self) -> bool {
        self.lit_time_remaining > 0
    }

    /// `AbstractFurnaceBlockEntity.getBurnDuration(FuelValues, ItemStack)`.
    pub fn get_burn_duration(&self, fuel_values: &FuelValues, stack: &InventoryItemStack) -> i32 {
        let base = fuel_values.burn_duration(stack);
        if self.kind.scales_burn_duration_half() {
            base / 2
        } else {
            base
        }
    }

    /// Insertion-ordered map of recipe ID -> completed cook count.
    pub fn recipes_used(&self) -> &IndexMap<Identifier, i32> {
        &self.recipes_used
    }

    /// `AbstractFurnaceBlockEntity.setRecipeUsed(RecipeHolder)`.
    pub fn set_recipe_used(&mut self, recipe_id: Identifier) {
        let count = self.recipes_used.entry(recipe_id).or_insert(0);
        *count = count.wrapping_add(1);
    }

    /// `AbstractFurnaceBlockEntity.setItem` with `RecipeManager` context so
    /// replacing slot 0 with a different item recalculates `cooking_total_time`
    /// and resets `cooking_timer = 0` (exact Paper semantics).
    pub fn set_item_with_recipes(
        &mut self,
        slot: usize,
        mut stack: InventoryItemStack,
        recipes: &RecipeManager,
    ) {
        if slot >= FURNACE_CONTAINER_SIZE {
            return;
        }
        let same = !stack.is_empty()
            && InventoryItemStack::is_same_item_same_components(&self.items[slot], &stack);
        let max_size = self.get_max_stack_size_for_item(&stack);
        stack.limit_size(max_size);
        self.items[slot] = stack;
        if slot == SLOT_INPUT && !same {
            self.cooking_total_time = self.compute_total_cook_time(recipes);
            self.cooking_timer = 0;
            self.set_changed();
        }
    }

    /// Compute `getTotalCookTime` for the current input item.
    pub fn compute_total_cook_time(&mut self, recipes: &RecipeManager) -> i32 {
        let cook_time = self
            .lookup_cooking_recipe(recipes)
            .map(|(_, r)| r.cooking_time)
            .unwrap_or(BURN_TIME_STANDARD);
        let scaled = (cook_time as f64 / self.cook_speed_multiplier).ceil() as i32;
        scaled.max(1)
    }

    fn lookup_cooking_recipe<'a>(
        &mut self,
        recipes: &'a RecipeManager,
    ) -> Option<(Identifier, &'a CookingRecipe)> {
        let ingredient = &self.items[SLOT_INPUT];
        if ingredient.is_empty() {
            return None;
        }
        let crafting_stack = inventory_to_crafting_stack(ingredient)?;
        let input = SingleRecipeInput::new(crafting_stack);
        let holder = recipes.get_single_recipe_for_with_hint(
            self.kind.cooking_kind().recipe_type(),
            &input,
            self.last_recipe.as_ref(),
        )?;
        self.last_recipe = Some(holder.id.clone());
        match &holder.value {
            Recipe::Cooking(cooking) => Some((holder.id.identifier().clone(), cooking)),
            _ => None,
        }
    }

    /// `AbstractFurnaceBlockEntity.canBurn(NonNullList, int, ItemStack)`.
    pub fn can_burn(&self, burn_result: &InventoryItemStack) -> bool {
        if burn_result.is_empty() {
            return false;
        }
        let result_slot = &self.items[SLOT_RESULT];
        if result_slot.is_empty() {
            return true;
        }
        if !InventoryItemStack::is_same_item_same_components(result_slot, burn_result) {
            return false;
        }
        let result_count = result_slot
            .get_count()
            .wrapping_add(burn_result.get_count());
        let max_result_count = self
            .get_max_stack_size()
            .min(burn_result.get_max_stack_size());
        result_count <= max_result_count
    }

    /// `AbstractFurnaceBlockEntity.serverTick(ServerLevel, BlockPos, BlockState, AbstractFurnaceBlockEntity)`.
    pub fn server_tick(
        &mut self,
        recipes: &RecipeManager,
        fuel_values: &FuelValues,
    ) -> FurnaceTickResult {
        let was_lit;
        let mut is_lit;
        if self.lit_time_remaining > 0 {
            was_lit = true;
            self.lit_time_remaining = self.lit_time_remaining.wrapping_sub(1);
            is_lit = self.lit_time_remaining > 0;
        } else {
            was_lit = false;
            is_lit = false;
        }

        let mut dirty = false;
        let mut burned_item = false;

        let has_ingredient = !self.items[SLOT_INPUT].is_empty();
        let has_fuel = !self.items[SLOT_FUEL].is_empty();

        if is_lit || (has_fuel && has_ingredient) {
            let max_stack_size = self.get_max_stack_size();
            let matched = if has_ingredient {
                self.lookup_cooking_recipe(recipes).and_then(|(id, r)| {
                    let result = crafting_to_inventory_stack(&r.result.create())?;
                    Some((id, r.cooking_time, result))
                })
            } else {
                None
            };

            let burn_result = matched.as_ref().map(|(_, _, res)| res);
            let can_cook = burn_result.is_some_and(|res| self.can_burn(res));

            if !is_lit && can_cook {
                let burn_duration = self.get_burn_duration(fuel_values, &self.items[SLOT_FUEL]);
                self.lit_time_remaining = burn_duration;
                self.lit_total_time = burn_duration;
                if self.is_lit() {
                    is_lit = true;
                    dirty = true;
                    if has_fuel {
                        let fuel_item = self.items[SLOT_FUEL].item();
                        self.items[SLOT_FUEL].shrink(1);
                        if self.items[SLOT_FUEL].is_empty() {
                            self.items[SLOT_FUEL] = inventory_crafting_remainder(fuel_item)
                                .unwrap_or_else(InventoryItemStack::empty);
                        }
                    }
                }
            }

            if is_lit && can_cook {
                if let Some((recipe_id, recipe_cook_time, result_stack)) = matched {
                    if self.cooking_total_time <= 0 {
                        let scaled =
                            (recipe_cook_time as f64 / self.cook_speed_multiplier).ceil() as i32;
                        self.cooking_total_time = scaled.max(1);
                    }
                    self.cooking_timer = self.cooking_timer.wrapping_add(1);
                    if self.cooking_timer == self.cooking_total_time {
                        self.cooking_timer = 0;
                        let scaled =
                            (recipe_cook_time as f64 / self.cook_speed_multiplier).ceil() as i32;
                        self.cooking_total_time = scaled.max(1);
                        if self.burn_item(max_stack_size, &result_stack) {
                            self.set_recipe_used(recipe_id);
                            burned_item = true;
                            dirty = true;
                        }
                    }
                }
            } else {
                self.cooking_timer = 0;
            }
        } else if !is_lit && self.cooking_timer > 0 {
            self.cooking_timer = self
                .cooking_timer
                .wrapping_sub(BURN_COOL_SPEED)
                .clamp(0, self.cooking_total_time.max(0));
        }

        let lit_changed = was_lit != is_lit;
        if lit_changed {
            dirty = true;
        }
        if dirty {
            self.set_changed();
        }

        FurnaceTickResult {
            lit_changed,
            is_lit,
            burned_item,
            dirty,
        }
    }

    fn burn_item(&mut self, max_stack_size: i32, burn_result: &InventoryItemStack) -> bool {
        if !self.can_burn(burn_result) {
            return false;
        }
        let max_allowed = max_stack_size.min(burn_result.get_max_stack_size());
        if self.items[SLOT_RESULT].is_empty() {
            let mut copy = burn_result.clone();
            copy.limit_size(max_allowed);
            self.items[SLOT_RESULT] = copy;
        } else {
            self.items[SLOT_RESULT].grow(burn_result.get_count());
        }

        // Paper special-case: wet_sponge in input turns empty bucket in fuel slot into water_bucket.
        if self.items[SLOT_INPUT].item().name() == Some("minecraft:wet_sponge")
            && !self.items[SLOT_FUEL].is_empty()
            && self.items[SLOT_FUEL].item().name() == Some("minecraft:bucket")
            && let Some(water_bucket) = ItemId::from_name("minecraft:water_bucket")
        {
            self.items[SLOT_FUEL] = InventoryItemStack::with_max_stack_size(water_bucket, 1, 1);
        }

        self.items[SLOT_INPUT].shrink(1);
        true
    }

    /// `AbstractFurnaceBlockEntity.saveAdditional(ValueOutput)`.
    pub fn save_additional(&self, output: &mut CompoundTag) {
        self.base.save_base(output);
        output.put_short("cooking_time_spent", self.cooking_timer as i16);
        output.put_short("cooking_total_time", self.cooking_total_time as i16);
        output.put_short("lit_time_remaining", self.lit_time_remaining as i16);
        output.put_short("lit_total_time", self.lit_total_time as i16);
        output.put_double("Paper.CookSpeedMultiplier", self.cook_speed_multiplier);
        save_all_items(output, &self.items, true);
        if !self.recipes_used.is_empty() {
            let mut recipes_tag = CompoundTag::new();
            for (id, &count) in &self.recipes_used {
                recipes_tag.put_int(&id.to_string(), count);
            }
            output.put("RecipesUsed".into(), Tag::Compound(recipes_tag));
        }
    }

    /// `AbstractFurnaceBlockEntity.loadAdditional(ValueInput)`.
    pub fn load_additional(&mut self, input: &CompoundTag) {
        self.base.load_base(input);
        load_all_items(input, &mut self.items);
        self.cooking_timer = i32::from(input.get_short_or("cooking_time_spent", 0));
        self.cooking_total_time = i32::from(input.get_short_or("cooking_total_time", 0));
        self.lit_time_remaining = i32::from(input.get_short_or("lit_time_remaining", 0));
        self.lit_total_time = i32::from(input.get_short_or("lit_total_time", 0));
        self.cook_speed_multiplier = input.get_double_or("Paper.CookSpeedMultiplier", 1.0);
        self.recipes_used.clear();
        if let Some(recipes_tag) = input.get_compound("RecipesUsed") {
            for (k, v) in &recipes_tag.tags {
                if let Tag::Int(count) = v
                    && let Ok(Some(id)) = Identifier::try_parse_result(k)
                {
                    self.recipes_used.insert(id, count.value);
                }
            }
        }
    }
}

impl Container for AbstractFurnaceBlockEntity {
    fn get_container_size(&self) -> usize {
        FURNACE_CONTAINER_SIZE
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
        if slot >= FURNACE_CONTAINER_SIZE {
            return;
        }
        let same = !stack.is_empty()
            && InventoryItemStack::is_same_item_same_components(&self.items[slot], &stack);
        let max_size = self.get_max_stack_size_for_item(&stack);
        stack.limit_size(max_size);
        self.items[slot] = stack;
        if slot == SLOT_INPUT && !same {
            self.cooking_timer = 0;
            self.cooking_total_time = self.kind.cooking_kind().default_cooking_time();
            self.set_changed();
        }
    }

    fn set_changed(&mut self) {
        self.base.set_changed();
    }

    fn still_valid(&self, _ctx: &ContainerAccessContext) -> bool {
        true
    }

    fn can_place_item(&self, slot: usize, stack: &InventoryItemStack) -> bool {
        if slot == SLOT_RESULT {
            return false;
        }
        if slot != SLOT_FUEL {
            return true;
        }
        let fuel_slot = &self.items[SLOT_FUEL];
        let is_bucket = stack.item().name() == Some("minecraft:bucket");
        let fuel_slot_is_bucket = fuel_slot.item().name() == Some("minecraft:bucket");
        self.fuel_values.is_fuel(stack) || (is_bucket && !fuel_slot_is_bucket)
    }

    fn clear_content(&mut self) {
        for slot in &mut self.items {
            *slot = InventoryItemStack::empty();
        }
    }
}

impl WorldlyContainer for AbstractFurnaceBlockEntity {
    fn get_slots_for_face(&self, direction: Direction) -> &[usize] {
        if direction == Direction::Down {
            &SLOTS_FOR_DOWN
        } else if direction == Direction::Up {
            &SLOTS_FOR_UP
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
        direction: Direction,
    ) -> bool {
        if direction != Direction::Down || slot != SLOT_FUEL {
            return true;
        }
        matches!(
            stack.item().name(),
            Some("minecraft:water_bucket" | "minecraft:bucket")
        )
    }
}

impl ContainerData for AbstractFurnaceBlockEntity {
    fn get(&self, index: usize) -> i32 {
        match index {
            DATA_LIT_TIME => self.lit_time_remaining,
            DATA_LIT_DURATION => self.lit_total_time,
            DATA_COOKING_PROGRESS => self.cooking_timer,
            DATA_COOKING_TOTAL_TIME => self.cooking_total_time,
            _ => 0,
        }
    }

    fn set(&mut self, index: usize, value: i32) {
        match index {
            DATA_LIT_TIME => self.lit_time_remaining = value,
            DATA_LIT_DURATION => self.lit_total_time = value,
            DATA_COOKING_PROGRESS => self.cooking_timer = value,
            DATA_COOKING_TOTAL_TIME => self.cooking_total_time = value,
            _ => {}
        }
    }

    fn get_count(&self) -> usize {
        NUM_DATA_VALUES
    }
}

/// Paper `net.minecraft.world.level.block.entity.FurnaceBlockEntity`.
pub fn new_furnace() -> AbstractFurnaceBlockEntity {
    AbstractFurnaceBlockEntity::new(FurnaceKind::Furnace)
}

/// Paper `net.minecraft.world.level.block.entity.BlastFurnaceBlockEntity`.
pub fn new_blast_furnace() -> AbstractFurnaceBlockEntity {
    AbstractFurnaceBlockEntity::new(FurnaceKind::BlastFurnace)
}

/// Paper `net.minecraft.world.level.block.entity.SmokerBlockEntity`.
pub fn new_smoker() -> AbstractFurnaceBlockEntity {
    AbstractFurnaceBlockEntity::new(FurnaceKind::Smoker)
}

fn inventory_to_crafting_stack(stack: &InventoryItemStack) -> Option<CraftingItemStack> {
    if stack.is_empty() {
        return Some(CraftingItemStack::empty());
    }
    let name = stack.item().name()?;
    let id = CraftingItemId::from_name(name)?;
    if stack.components().is_empty() {
        Some(CraftingItemStack::new(id, stack.get_count()))
    } else {
        let components: BTreeMap<Identifier, String> = stack
            .components()
            .iter()
            .filter_map(|(k, v)| Identifier::try_parse(k).map(|id| (id, v.clone())))
            .collect();
        Some(CraftingItemStack::with_components(
            id,
            stack.get_count(),
            components,
        ))
    }
}

fn crafting_to_inventory_stack(stack: &CraftingItemStack) -> Option<InventoryItemStack> {
    if stack.is_empty() {
        return Some(InventoryItemStack::empty());
    }
    let name = stack.item().name();
    let id = ItemId::from_name(name)?;
    let mut max_stack_size = stack.item().default_max_stack_size().max(1);
    let mut components = ComponentMap::new();
    for (k, v) in stack.components() {
        components.insert(k.to_string(), v.clone());
    }
    if let Some(override_max) = components
        .get("minecraft:max_stack_size")
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|&n| n > 0)
    {
        max_stack_size = override_max;
    }
    let mut inv = InventoryItemStack::with_max_stack_size(id, stack.count(), max_stack_size);
    if !components.is_empty() {
        inv = inv.with_components(components);
    }
    Some(inv)
}

static EMPTY_STACK: std::sync::LazyLock<InventoryItemStack> =
    std::sync::LazyLock::new(InventoryItemStack::empty);
