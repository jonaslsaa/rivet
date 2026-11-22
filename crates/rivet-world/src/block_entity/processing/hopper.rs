//! Paper `net.minecraft.world.level.block.entity.HopperBlockEntity`.
//!
//! Mirrors `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/block/entity/HopperBlockEntity.java`.

use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::core::Direction;

use crate::inventory::{Container, ContainerAccessContext, ContainerHelper, InventoryItemStack};

use super::nbt::{BaseContainerState, load_all_items, save_all_items};
use super::worldly_container::WorldlyContainer;

/// `HopperBlockEntity.MOVE_ITEM_SPEED = 8`.
pub const MOVE_ITEM_SPEED: i32 = 8;
/// `HopperBlockEntity.HOPPER_CONTAINER_SIZE = 5`.
pub const HOPPER_CONTAINER_SIZE: usize = 5;
/// `HopperBlockEntity.DEFAULT_COOLDOWN = -1`.
pub const DEFAULT_COOLDOWN: i32 = -1;

/// Paper hopper inventory state codes.
pub const HOPPER_EMPTY: i32 = 0;
pub const HOPPER_IS_FULL: i32 = 1;
pub const HOPPER_HAS_ITEMS: i32 = 2;

/// Uniform view over either a plain `Container` or a sided `WorldlyContainer`
/// for hopper push/pull transfer operations.
pub enum HopperTarget<'a> {
    /// Ordinary unsided `Container` (e.g. chest, barrel, hopper, crafter).
    Plain(&'a mut dyn Container),
    /// Sided `WorldlyContainer` (e.g. furnace, brewing stand).
    Worldly(&'a mut dyn WorldlyContainer),
}

impl HopperTarget<'_> {
    fn container(&self) -> &dyn Container {
        match self {
            Self::Plain(c) => *c,
            Self::Worldly(w) => *w,
        }
    }

    fn container_mut(&mut self) -> &mut dyn Container {
        match self {
            Self::Plain(c) => *c,
            Self::Worldly(w) => *w,
        }
    }

    fn slots_for_face(&self, direction: Option<Direction>) -> Vec<usize> {
        match (self, direction) {
            (Self::Worldly(w), Some(dir)) => w.get_slots_for_face(dir).to_vec(),
            (target, _) => (0..target.container().get_container_size()).collect(),
        }
    }

    fn can_place_in_container(
        &self,
        stack: &InventoryItemStack,
        slot: usize,
        direction: Option<Direction>,
    ) -> bool {
        if !self.container().can_place_item(slot, stack) {
            return false;
        }
        match self {
            Self::Plain(_) => true,
            Self::Worldly(w) => w.can_place_item_through_face(slot, stack, direction),
        }
    }

    fn can_take_from_container(
        &self,
        stack: &InventoryItemStack,
        slot: usize,
        direction: Direction,
    ) -> bool {
        if !self.container().can_take_item(slot, stack) {
            return false;
        }
        match self {
            Self::Plain(_) => true,
            Self::Worldly(w) => w.can_take_item_through_face(slot, stack, direction),
        }
    }
}

/// Paper `net.minecraft.world.level.block.entity.HopperBlockEntity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HopperBlockEntity {
    pub base: BaseContainerState,
    items: [InventoryItemStack; HOPPER_CONTAINER_SIZE],
    pub cooldown_time: i32,
    pub ticked_game_time: i64,
    pub facing: Direction,
    /// `HopperBlock.ENABLED` (false when redstone-powered).
    pub enabled: bool,
}

impl Default for HopperBlockEntity {
    fn default() -> Self {
        Self::new(Direction::Down)
    }
}

impl HopperBlockEntity {
    /// Construct an empty 5-slot hopper pointing in `facing`.
    pub fn new(facing: Direction) -> Self {
        Self {
            base: BaseContainerState::new(),
            items: std::array::from_fn(|_| InventoryItemStack::empty()),
            cooldown_time: DEFAULT_COOLDOWN,
            ticked_game_time: 0,
            facing,
            enabled: true,
        }
    }

    /// `HopperBlockEntity.setCooldown(int)`.
    pub fn set_cooldown(&mut self, cooldown_time: i32) {
        self.cooldown_time = cooldown_time;
    }

    /// `HopperBlockEntity.isOnCooldown()`.
    pub const fn is_on_cooldown(&self) -> bool {
        self.cooldown_time > 0
    }

    /// `HopperBlockEntity.isOnCustomCooldown()`.
    pub const fn is_on_custom_cooldown(&self) -> bool {
        self.cooldown_time > MOVE_ITEM_SPEED
    }

    /// `HopperBlockEntity.inventoryFull()`.
    pub fn inventory_full(&self) -> bool {
        for stack in &self.items {
            if stack.is_empty() || stack.get_count() != stack.get_max_stack_size() {
                return false;
            }
        }
        true
    }

    /// `HopperBlockEntity.canMergeItems(ItemStack, ItemStack)`.
    pub fn can_merge_items(a: &InventoryItemStack, b: &InventoryItemStack) -> bool {
        a.get_count() < a.get_max_stack_size()
            && InventoryItemStack::is_same_item_same_components(a, b)
    }

    /// `HopperBlockEntity.addItem(@Nullable Container, Container, ItemStack, @Nullable Direction)`.
    ///
    /// Inserts `stack` into `dest` scanning `getSlotsForFace(direction)` for
    /// `WorldlyContainer` or `0..size` for plain `Container`. Returns any
    /// uninserted remainder.
    pub fn add_item(
        dest: &mut HopperTarget<'_>,
        mut stack: InventoryItemStack,
        direction: Option<Direction>,
    ) -> InventoryItemStack {
        let slots = dest.slots_for_face(direction);
        for slot in slots {
            if stack.is_empty() {
                break;
            }
            stack = Self::try_move_in_item(dest, stack, slot, direction);
        }
        stack
    }

    /// `HopperBlockEntity.tryMoveInItem(@Nullable Container, Container, ItemStack, int, @Nullable Direction)`.
    fn try_move_in_item(
        dest: &mut HopperTarget<'_>,
        mut stack: InventoryItemStack,
        slot: usize,
        direction: Option<Direction>,
    ) -> InventoryItemStack {
        if !dest.can_place_in_container(&stack, slot, direction) {
            return stack;
        }

        let current = dest.container().get_item(slot).clone();
        let mut moved = false;

        if current.is_empty() {
            let max_size = dest.container().get_max_stack_size_for_item(&stack);
            if stack.get_count() > max_size {
                let split = stack.split(max_size);
                dest.container_mut().set_item(slot, split);
            } else {
                let to_place = std::mem::take(&mut stack);
                dest.container_mut().set_item(slot, to_place);
            }
            moved = true;
        } else if Self::can_merge_items(&current, &stack) {
            let max_size = dest
                .container()
                .get_max_stack_size_for_item(&current)
                .min(current.get_max_stack_size());
            let space = max_size.wrapping_sub(current.get_count());
            let transfer = stack.get_count().min(space);
            if transfer > 0 {
                let mut merged = current;
                merged.grow(transfer);
                stack.shrink(transfer);
                dest.container_mut().set_item(slot, merged);
                moved = true;
            }
        }

        if moved {
            dest.container_mut().set_changed();
        }
        stack
    }

    /// `HopperBlockEntity.ejectItems(...)` — pushes up to `hopper_amount`
    /// (default `1`) from the first eligible hopper slot into `dest` through
    /// `self.facing.opposite()`.
    pub fn eject_items(&mut self, dest: &mut HopperTarget<'_>, hopper_amount: i32) -> bool {
        if self.is_empty() || hopper_amount <= 0 {
            return false;
        }
        let insert_face = self.facing.get_opposite();
        if is_full_container(dest, insert_face) {
            return false;
        }

        for slot in 0..HOPPER_CONTAINER_SIZE {
            if self.items[slot].is_empty() {
                continue;
            }
            let original = self.items[slot].clone();
            let extracted = self.remove_item(slot, hopper_amount);
            let remainder = Self::add_item(dest, extracted, Some(insert_face));
            if remainder.is_empty() {
                dest.container_mut().set_changed();
                return true;
            }
            // Restore unmoved items cleanly so item count is strictly conserved.
            let moved_count = original
                .get_count()
                .wrapping_sub(self.items[slot].get_count())
                .wrapping_sub(remainder.get_count());
            if moved_count > 0 {
                // Partial move: restore only the unmoved remainder back to the slot.
                let mut restored = original;
                restored.set_count(
                    self.items[slot]
                        .get_count()
                        .wrapping_add(remainder.get_count()),
                );
                self.items[slot] = restored;
                self.set_changed();
                dest.container_mut().set_changed();
                return true;
            }
            self.items[slot] = original;
        }
        false
    }

    /// `HopperBlockEntity.suckInItems(...)` — pulls up to `hopper_amount`
    /// (default `1`) from `source` through `Direction::Down` into this hopper.
    pub fn suck_in_items(&mut self, source: &mut HopperTarget<'_>, hopper_amount: i32) -> bool {
        if self.inventory_full() || hopper_amount <= 0 {
            return false;
        }
        let slots = source.slots_for_face(Some(Direction::Down));
        for slot in slots {
            let stack = source.container().get_item(slot).clone();
            if stack.is_empty() {
                continue;
            }
            if !source.can_take_from_container(&stack, slot, Direction::Down) {
                continue;
            }
            let original = stack;
            let taken = source.container_mut().remove_item(slot, hopper_amount);
            if taken.is_empty() {
                continue;
            }
            let mut self_target = HopperTarget::Plain(self);
            let remainder = Self::add_item(&mut self_target, taken, None);
            if remainder.is_empty() {
                source.container_mut().set_changed();
                return true;
            }
            let remaining_in_source = source.container().get_item(slot).get_count();
            let moved_count = original
                .get_count()
                .wrapping_sub(remaining_in_source)
                .wrapping_sub(remainder.get_count());
            if moved_count > 0 {
                let mut restored = original;
                restored.set_count(remaining_in_source.wrapping_add(remainder.get_count()));
                source.container_mut().set_item(slot, restored);
                source.container_mut().set_changed();
                return true;
            }
            source.container_mut().set_item(slot, original);
        }
        false
    }

    /// `HopperBlockEntity.pushItemsTick(...)` — exact Paper tick sequence:
    /// 1. Decrement `cooldown_time` and record `ticked_game_time`.
    /// 2. If not on cooldown and `enabled`, clamp `cooldown_time = 0` and
    ///    attempt eject BEFORE pull (`tryMoveItems`).
    /// 3. On any movement, set `cooldown_time = MOVE_ITEM_SPEED` and call
    ///    `set_changed()`.
    pub fn push_items_tick(
        &mut self,
        game_time: i64,
        above: Option<&mut HopperTarget<'_>>,
        facing_dest: Option<&mut HopperTarget<'_>>,
    ) -> bool {
        self.cooldown_time = self.cooldown_time.wrapping_sub(1);
        self.ticked_game_time = game_time;
        if self.is_on_cooldown() {
            return false;
        }
        self.set_cooldown(0);
        if !self.enabled {
            return false;
        }

        let mut moved = false;
        if !self.is_empty()
            && let Some(dest) = facing_dest
        {
            moved = self.eject_items(dest, 1);
        }
        if !self.inventory_full()
            && let Some(src) = above
        {
            moved |= self.suck_in_items(src, 1);
        }

        if moved {
            self.set_cooldown(MOVE_ITEM_SPEED);
            self.set_changed();
        }
        moved
    }

    /// `HopperBlockEntity.saveAdditional(ValueOutput)`.
    pub fn save_additional(&self, output: &mut CompoundTag) {
        self.base.save_base(output);
        if !self.base.try_save_loot_table(output) {
            save_all_items(output, &self.items, true);
        }
        output.put_int("TransferCooldown", self.cooldown_time);
    }

    /// `HopperBlockEntity.loadAdditional(ValueInput)`.
    pub fn load_additional(&mut self, input: &CompoundTag) {
        self.base.load_base(input);
        self.items = std::array::from_fn(|_| InventoryItemStack::empty());
        if !self.base.try_load_loot_table(input) {
            load_all_items(input, &mut self.items);
        }
        self.cooldown_time = input.get_int_or("TransferCooldown", DEFAULT_COOLDOWN);
    }
}

fn is_full_container(target: &HopperTarget<'_>, direction: Direction) -> bool {
    let slots = target.slots_for_face(Some(direction));
    for slot in slots {
        let stack = target.container().get_item(slot);
        if stack.get_count() < stack.get_max_stack_size() {
            return false;
        }
    }
    true
}

impl Container for HopperBlockEntity {
    fn get_container_size(&self) -> usize {
        HOPPER_CONTAINER_SIZE
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
        if slot >= HOPPER_CONTAINER_SIZE {
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

    fn still_valid(&self, _ctx: &ContainerAccessContext) -> bool {
        true
    }

    fn clear_content(&mut self) {
        for slot in &mut self.items {
            *slot = InventoryItemStack::empty();
        }
    }
}

static EMPTY_STACK: std::sync::LazyLock<InventoryItemStack> =
    std::sync::LazyLock::new(InventoryItemStack::empty);
