//! Paper `net.minecraft.world.level.block.entity.HopperBlockEntity`.
//!
//! Mirrors `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/block/entity/HopperBlockEntity.java`.

use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::core::Direction;

use crate::inventory::{Container, ContainerAccessContext, ContainerHelper, InventoryItemStack};

use super::furnace::AbstractFurnaceBlockEntity;
use super::nbt::{BaseContainerState, load_all_items, save_all_items};
use super::randomizable::RandomizableContainer;
use super::worldly_container::WorldlyContainer;
use crate::crafting::RecipeManager;

/// `HopperBlockEntity.MOVE_ITEM_SPEED = 8`.
pub const MOVE_ITEM_SPEED: i32 = 8;
/// `HopperBlockEntity.HOPPER_CONTAINER_SIZE = 5`.
pub const HOPPER_CONTAINER_SIZE: usize = 5;
/// `HopperBlockEntity.DEFAULT_COOLDOWN = -1`.
pub const DEFAULT_COOLDOWN: i32 = -1;

/// Paper `HopperBlockEntity.HOPPER_EMPTY` (`getFullState`, Perf: Optimize Hoppers).
pub const HOPPER_EMPTY: i32 = 0;
/// Paper `HopperBlockEntity.HOPPER_HAS_ITEMS`.
pub const HOPPER_HAS_ITEMS: i32 = 1;
/// Paper `HopperBlockEntity.HOPPER_IS_FULL`.
pub const HOPPER_IS_FULL: i32 = 2;

/// Uniform view over a plain `Container`, a sided `WorldlyContainer`, or a
/// receiving `HopperBlockEntity` for hopper push/pull transfer operations.
pub enum HopperTarget<'a> {
    /// Ordinary unsided `Container` (e.g. chest, barrel, crafter).
    Plain(&'a mut dyn Container),
    /// Sided `WorldlyContainer` (e.g. furnace, brewing stand).
    Worldly(&'a mut dyn WorldlyContainer),
    /// Level-bound furnace: `setItem` applies Paper's `this.level instanceof
    /// ServerLevel` branch (`getTotalCookTime`, `cookingTimer = 0`), so the level's
    /// `RecipeManager` travels with the target.
    Furnace(&'a mut AbstractFurnaceBlockEntity, &'a RecipeManager),
    /// Another `HopperBlockEntity` (so empty-hopper transfer cooldown is applied).
    Hopper(&'a mut HopperBlockEntity),
}

impl HopperTarget<'_> {
    fn container(&self) -> &dyn Container {
        match self {
            Self::Plain(c) => *c,
            Self::Worldly(w) => *w,
            Self::Furnace(f, _) => &**f,
            Self::Hopper(h) => *h,
        }
    }

    fn container_mut(&mut self) -> &mut dyn Container {
        match self {
            Self::Plain(c) => *c,
            Self::Worldly(w) => *w,
            Self::Furnace(f, _) => &mut **f,
            Self::Hopper(h) => *h,
        }
    }

    /// `Container.setItem` through the target (level-bound for furnaces).
    fn set_item(&mut self, slot: usize, stack: InventoryItemStack) {
        match self {
            Self::Furnace(f, recipes) => f.set_item_with_recipes(slot, stack, recipes),
            other => other.container_mut().set_item(slot, stack),
        }
    }

    fn slots_for_face(&self, direction: Option<Direction>) -> Vec<usize> {
        match (self, direction) {
            (Self::Furnace(f, _), Some(dir)) => f.get_slots_for_face(dir).to_vec(),
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
            Self::Plain(_) | Self::Hopper(_) => true,
            Self::Worldly(w) => w.can_place_item_through_face(slot, stack, direction),
            Self::Furnace(f, _) => f.can_place_item_through_face(slot, stack, direction),
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
            Self::Plain(_) | Self::Hopper(_) => true,
            Self::Worldly(w) => w.can_take_item_through_face(slot, stack, direction),
            Self::Furnace(f, _) => f.can_take_item_through_face(slot, stack, direction),
        }
    }
}

/// Paper `net.minecraft.world.level.block.entity.HopperBlockEntity`.
#[derive(Debug, Clone, PartialEq)]
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
        stack: InventoryItemStack,
        direction: Option<Direction>,
    ) -> InventoryItemStack {
        Self::add_item_with_source_tick(dest, stack, direction, None)
    }

    fn add_item_with_source_tick(
        dest: &mut HopperTarget<'_>,
        mut stack: InventoryItemStack,
        direction: Option<Direction>,
        from_ticked_game_time: Option<i64>,
    ) -> InventoryItemStack {
        let slots = dest.slots_for_face(direction);
        for slot in slots {
            if stack.is_empty() {
                break;
            }
            stack = Self::try_move_in_item(dest, stack, slot, direction, from_ticked_game_time);
        }
        stack
    }

    /// `HopperBlockEntity.tryMoveInItem(@Nullable Container, Container, ItemStack, int, @Nullable Direction)`.
    fn try_move_in_item(
        dest: &mut HopperTarget<'_>,
        mut stack: InventoryItemStack,
        slot: usize,
        direction: Option<Direction>,
        from_ticked_game_time: Option<i64>,
    ) -> InventoryItemStack {
        if !dest.can_place_in_container(&stack, slot, direction) {
            return stack;
        }

        let was_empty = dest.container().is_empty();
        let current = dest.container().get_item(slot).clone();
        let mut moved = false;

        if current.is_empty() {
            let max_size = dest.container().get_max_stack_size_for_item(&stack);
            if stack.get_count() > max_size {
                let split = stack.split(max_size);
                dest.set_item(slot, split);
            } else {
                let to_place = std::mem::take(&mut stack);
                dest.set_item(slot, to_place);
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
                dest.set_item(slot, merged);
                moved = true;
            }
        }

        if moved {
            if was_empty
                && let HopperTarget::Hopper(dest_hopper) = dest
                && !dest_hopper.is_on_custom_cooldown()
            {
                let skip_tick_count = match from_ticked_game_time {
                    Some(from_time) if dest_hopper.ticked_game_time >= from_time => 1,
                    _ => 0,
                };
                dest_hopper.set_cooldown(MOVE_ITEM_SPEED.wrapping_sub(skip_tick_count));
            }
            dest.container_mut().set_changed();
        }
        stack
    }

    /// `HopperBlockEntity.ejectItems(...)` / `hopperPush(...)` — pushes up to
    /// `hopper_amount` (default `1`) from the first eligible hopper slot into
    /// `dest` through `self.facing.opposite()`.
    ///
    /// With Paper's default `hopper.cooldown-when-full: true`, a push that finds
    /// items but moves none sets the cooldown to `MOVE_ITEM_SPEED`
    /// (`spigotConfig.hopperTransfer`).
    pub fn eject_items(&mut self, dest: &mut HopperTarget<'_>, hopper_amount: i32) -> bool {
        if self.is_empty() || hopper_amount <= 0 {
            return false;
        }
        let insert_face = self.facing.get_opposite();
        if is_full_container(dest, insert_face) {
            return false;
        }

        let mut found_item = false;
        for slot in 0..HOPPER_CONTAINER_SIZE {
            if self.items[slot].is_empty() {
                continue;
            }
            found_item = true;
            // `hopperPush`: the stack is trimmed in place to the moved count, pushed,
            // and rebuilt as `orig - moved + remaining` (no `setChanged` on the hopper).
            let original = self.items[slot].clone();
            let original_count = original.get_count();
            let moved_count = hopper_amount.min(original_count);
            let mut moved_item = original.clone();
            moved_item.set_count(moved_count);
            let remainder = Self::add_item_with_source_tick(
                dest,
                moved_item,
                Some(insert_face),
                Some(self.ticked_game_time),
            );
            let remaining_count = remainder.get_count();
            if remaining_count != moved_count {
                let mut rebuilt = original;
                rebuilt.set_count(
                    original_count
                        .wrapping_sub(moved_count)
                        .wrapping_add(remaining_count),
                );
                self.set_item(slot, rebuilt);
                dest.container_mut().set_changed();
                return true;
            }
        }
        if found_item {
            // `hopper.cooldownWhenFull` — inventory was full.
            self.set_cooldown(MOVE_ITEM_SPEED);
        }
        false
    }

    /// `HopperBlockEntity.suckInItems(...)` / `hopperPull(...)` — pulls up to
    /// `hopper_amount` (default `1`) from `source` through `Direction::Down`
    /// into this hopper.
    ///
    /// Each attempted slot marks the source changed; with Paper's default
    /// `hopper.cooldown-when-full: true`, each failed slot transfer applies the
    /// `MOVE_ITEM_SPEED` cooldown before the next slot is tried.
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
            // "original logic always marks source inv as changed even if no move happens."
            source.container_mut().set_changed();
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
                source.set_item(slot, restored);
                source.container_mut().set_changed();
                return true;
            }
            source.set_item(slot, original);
            // `hopper.cooldownWhenFull` — `applyCooldown(hopper)`.
            self.set_cooldown(MOVE_ITEM_SPEED);
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
        // `trySaveLootTable` returns `false` for Paper block entities (they carry
        // `lootableData`), so `Items` is always written.
        self.base.save_loot_table(output);
        save_all_items(output, &self.items, true);
        output.put_int("TransferCooldown", self.cooldown_time);
    }

    /// `HopperBlockEntity.loadAdditional(ValueInput)`.
    pub fn load_additional(&mut self, input: &CompoundTag) {
        self.base.load_base(input);
        self.items = std::array::from_fn(|_| InventoryItemStack::empty());
        // `tryLoadLootTable` returns `false` for Paper block entities (they carry
        // `lootableData`), so `Items` is always read.
        self.base.load_loot_table(input);
        load_all_items(input, &mut self.items);
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
        ContainerHelper::remove_item(&mut self.items, slot, count)
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
    }

    fn set_changed(&mut self) {
        self.base.set_changed();
    }

    fn still_valid(&self, ctx: &ContainerAccessContext) -> bool {
        !ctx.is_removed && ctx.can_interact
    }

    fn clear_content(&mut self) {
        for slot in &mut self.items {
            *slot = InventoryItemStack::empty();
        }
    }
}

impl RandomizableContainer for HopperBlockEntity {
    fn container_state(&self) -> &BaseContainerState {
        &self.base
    }

    fn container_state_mut(&mut self) -> &mut BaseContainerState {
        &mut self.base
    }
}

static EMPTY_STACK: std::sync::LazyLock<InventoryItemStack> =
    std::sync::LazyLock::new(InventoryItemStack::empty);
