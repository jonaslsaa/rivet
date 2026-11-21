//! Paper `net.minecraft.world.inventory.AbstractContainerMenu`.

use indexmap::{IndexMap, IndexSet};
use rivet_util::mth;

use super::click_action::ClickAction;
use super::container::{Container, ContainerAccessContext};
use super::container_data::ContainerData;
use super::container_input::ContainerInput;
use super::container_listener::{ListenerId, MenuContainerListener};
use super::container_synchronizer::ContainerSynchronizer;
use super::data_slot::DataSlot;
use super::item_stack::{HashedStack, InventoryItemStack};
use super::player_inventory::{INVENTORY_SIZE, PlayerInventory, SLOT_OFFHAND};
use super::remote_slot::RemoteSlot;
use super::slot::Slot;

/// `AbstractContainerMenu.SLOT_CLICKED_OUTSIDE`.
pub const SLOT_CLICKED_OUTSIDE: i32 = -999;

/// `AbstractContainerMenu.QUICKCRAFT_TYPE_CHARITABLE` (left-drag even split).
pub const QUICKCRAFT_TYPE_CHARITABLE: i32 = 0;

/// `AbstractContainerMenu.QUICKCRAFT_TYPE_GREEDY` (right-drag place 1 per slot).
pub const QUICKCRAFT_TYPE_GREEDY: i32 = 1;

/// `AbstractContainerMenu.QUICKCRAFT_TYPE_CLONE` (middle-drag creative full stack).
pub const QUICKCRAFT_TYPE_CLONE: i32 = 2;

/// `AbstractContainerMenu.QUICKCRAFT_HEADER_START`.
pub const QUICKCRAFT_HEADER_START: i32 = 0;

/// `AbstractContainerMenu.QUICKCRAFT_HEADER_CONTINUE`.
pub const QUICKCRAFT_HEADER_CONTINUE: i32 = 1;

/// `AbstractContainerMenu.QUICKCRAFT_HEADER_END`.
pub const QUICKCRAFT_HEADER_END: i32 = 2;

/// Maximum state id mask (`32767`, Paper `incrementStateId`).
pub const STATE_ID_MASK: i32 = 32767;

/// Outcome of a single [`AbstractContainerMenu::clicked`] transaction.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ClickOutcome {
    /// Items dropped into the world by the transaction (e.g. outside click or
    /// `ContainerInput::Throw`), guaranteeing item conservation.
    pub dropped_items: Vec<InventoryItemStack>,
    /// Whether the transaction was rejected (e.g. invalid slot, invalid drag
    /// header, survival clone attempt, or denied pickup/place).
    pub rejected: bool,
}

/// Paper `net.minecraft.world.inventory.AbstractContainerMenu`.
pub struct AbstractContainerMenu {
    pub container_id: i32,
    pub slots: Vec<Slot>,
    pub last_slots: Vec<InventoryItemStack>,
    pub remote_slots: Vec<RemoteSlot>,
    pub data_slots: Vec<DataSlot>,
    pub remote_data_slots: Vec<i32>,
    carried: InventoryItemStack,
    remote_carried: RemoteSlot,
    state_id: i32,
    pub quickcraft_type: i32,
    pub quickcraft_status: i32,
    pub quickcraft_slots: IndexSet<usize>,
    suppress_remote_updates: bool,
    listeners: IndexMap<ListenerId, Box<dyn MenuContainerListener>>,
    synchronizer: Option<Box<dyn ContainerSynchronizer>>,
}

impl AbstractContainerMenu {
    /// Construct a new empty menu with the given `container_id`.
    pub fn new(container_id: i32) -> Self {
        Self {
            container_id,
            slots: Vec::new(),
            last_slots: Vec::new(),
            remote_slots: Vec::new(),
            data_slots: Vec::new(),
            remote_data_slots: Vec::new(),
            carried: InventoryItemStack::empty(),
            remote_carried: RemoteSlot::Placeholder,
            state_id: 0,
            quickcraft_type: -1,
            quickcraft_status: 0,
            quickcraft_slots: IndexSet::new(),
            suppress_remote_updates: false,
            listeners: IndexMap::new(),
            synchronizer: None,
        }
    }

    /// `AbstractContainerMenu.addSlot(Slot)`.
    pub fn add_slot(&mut self, mut slot: Slot) -> usize {
        let index = self.slots.len();
        slot.index = index;
        self.slots.push(slot);
        self.last_slots.push(InventoryItemStack::empty());
        let remote = if self.synchronizer.is_some() {
            RemoteSlot::synchronized()
        } else {
            RemoteSlot::Placeholder
        };
        self.remote_slots.push(remote);
        index
    }

    /// `AbstractContainerMenu.addDataSlot(DataSlot)`.
    pub fn add_data_slot(&mut self, data_slot: DataSlot) -> usize {
        let index = self.data_slots.len();
        self.data_slots.push(data_slot);
        self.remote_data_slots.push(0);
        index
    }

    /// `AbstractContainerMenu.addDataSlots(ContainerData)`.
    pub fn add_data_slots(&mut self, data_id: usize, count: usize) {
        for index in 0..count {
            self.add_data_slot(DataSlot::for_container(data_id, index));
        }
    }

    /// `AbstractContainerMenu.addSlotListener(ContainerListener)` —
    /// deduplicates by [`ListenerId`] and immediately broadcasts current state.
    pub fn add_slot_listener(
        &mut self,
        id: ListenerId,
        listener: Box<dyn MenuContainerListener>,
        containers: &[&dyn Container],
        data_sources: &[&dyn ContainerData],
    ) {
        if self.listeners.contains_key(&id) {
            return;
        }
        self.listeners.insert(id, listener);
        self.broadcast_changes(containers, data_sources);
    }

    /// `AbstractContainerMenu.removeSlotListener(ContainerListener)`.
    pub fn remove_slot_listener(&mut self, id: ListenerId) -> bool {
        self.listeners.shift_remove(&id).is_some()
    }

    /// `AbstractContainerMenu.setSynchronizer(ContainerSynchronizer)`.
    pub fn set_synchronizer(
        &mut self,
        synchronizer: Box<dyn ContainerSynchronizer>,
        containers: &[&dyn Container],
        data_sources: &[&dyn ContainerData],
    ) {
        self.synchronizer = Some(synchronizer);
        self.remote_slots = vec![RemoteSlot::synchronized(); self.slots.len()];
        self.remote_carried = RemoteSlot::synchronized();
        self.send_all_data_to_remote(containers, data_sources);
    }

    /// `AbstractContainerMenu.sendAllDataToRemote()`.
    pub fn send_all_data_to_remote(
        &mut self,
        containers: &[&dyn Container],
        data_sources: &[&dyn ContainerData],
    ) {
        let mut items = Vec::with_capacity(self.slots.len());
        for (i, slot) in self.slots.iter().enumerate() {
            let stack = self.read_slot_item(slot, containers);
            self.remote_slots[i].force(&stack);
            items.push(stack);
        }
        self.remote_carried.force(&self.carried);

        let mut initial_data = Vec::with_capacity(self.data_slots.len());
        for (i, data_slot) in self.data_slots.iter().enumerate() {
            let value = data_slot.get(data_sources);
            self.remote_data_slots[i] = value;
            initial_data.push(value);
        }

        let state_id = self.increment_state_id();
        if let Some(sync) = self.synchronizer.as_mut() {
            sync.send_initial_data(
                self.container_id,
                &items,
                &self.carried,
                &initial_data,
                state_id,
            );
        }
    }

    /// `AbstractContainerMenu.broadcastChanges()` — preserves Paper's exact
    /// notification order:
    /// 1. Slots `0..slots.len()` (listeners, then remote sync).
    /// 2. Carried stack remote sync.
    /// 3. Data slots `0..data_slots.len()` (listeners, then remote sync).
    pub fn broadcast_changes(
        &mut self,
        containers: &[&dyn Container],
        data_sources: &[&dyn ContainerData],
    ) {
        for i in 0..self.slots.len() {
            let current = self.read_slot_item(&self.slots[i], containers);
            self.trigger_slot_listeners(i, &current);
            self.synchronize_slot_to_remote(i, &current);
        }

        self.synchronize_carried_to_remote();

        for i in 0..self.data_slots.len() {
            let current_value = self.data_slots[i].get(data_sources);
            if self.data_slots[i].check_and_clear_update_flag(current_value) {
                self.update_data_slot_listeners(i, current_value);
            }
            self.synchronize_data_slot_to_remote(i, current_value);
        }
    }

    /// `AbstractContainerMenu.broadcastFullState()`.
    pub fn broadcast_full_state(
        &mut self,
        containers: &[&dyn Container],
        data_sources: &[&dyn ContainerData],
    ) {
        for i in 0..self.slots.len() {
            let current = self.read_slot_item(&self.slots[i], containers);
            self.trigger_slot_listeners(i, &current);
        }
        for i in 0..self.data_slots.len() {
            let current_value = self.data_slots[i].get(data_sources);
            if self.data_slots[i].check_and_clear_update_flag(current_value) {
                self.update_data_slot_listeners(i, current_value);
            }
        }
        self.send_all_data_to_remote(containers, data_sources);
    }

    /// `AbstractContainerMenu.setRemoteSlot(int, HashedStack)`.
    pub fn set_remote_slot(&mut self, slot: usize, hash: HashedStack) {
        if let Some(remote) = self.remote_slots.get_mut(slot) {
            remote.receive(hash);
        }
    }

    /// `AbstractContainerMenu.setRemoteCarried(HashedStack)`.
    pub fn set_remote_carried(&mut self, hash: HashedStack) {
        self.remote_carried.receive(hash);
    }

    /// `AbstractContainerMenu.suppressRemoteUpdates()`.
    pub fn suppress_remote_updates(&mut self) {
        self.suppress_remote_updates = true;
    }

    /// `AbstractContainerMenu.resumeRemoteUpdates()`.
    pub fn resume_remote_updates(&mut self) {
        self.suppress_remote_updates = false;
    }

    /// `AbstractContainerMenu.getCarried()`.
    pub fn get_carried(&self) -> &InventoryItemStack {
        &self.carried
    }

    /// `AbstractContainerMenu.setCarried(ItemStack)`.
    pub fn set_carried(&mut self, stack: InventoryItemStack) {
        self.carried = stack;
    }

    /// `AbstractContainerMenu.getStateId()`.
    pub const fn get_state_id(&self) -> i32 {
        self.state_id
    }

    /// `AbstractContainerMenu.incrementStateId()` — `(stateId + 1) & 32767`.
    pub fn increment_state_id(&mut self) -> i32 {
        self.state_id = self.state_id.wrapping_add(1) & STATE_ID_MASK;
        self.state_id
    }

    /// `AbstractContainerMenu.findSlot(Container, int)`.
    pub fn find_slot(&self, container_id: usize, container_slot: usize) -> Option<usize> {
        self.slots.iter().enumerate().find_map(|(i, slot)| {
            (slot.container_id == container_id && slot.get_container_slot() == container_slot)
                .then_some(i)
        })
    }

    /// `AbstractContainerMenu.transferState(AbstractContainerMenu)`.
    pub fn transfer_state(&mut self, other: &AbstractContainerMenu) {
        let mut other_map: IndexMap<(usize, usize), usize> = IndexMap::new();
        for (idx, slot) in other.slots.iter().enumerate() {
            other_map.insert((slot.container_id, slot.get_container_slot()), idx);
        }
        for (idx, slot) in self.slots.iter().enumerate() {
            if let Some(&other_idx) = other_map.get(&(slot.container_id, slot.get_container_slot()))
            {
                self.last_slots[idx] = other.last_slots[other_idx].clone();
                self.remote_slots[idx].copy_from(&other.remote_slots[other_idx]);
            }
        }
    }

    /// `AbstractContainerMenu.removed(Player)` — returns any carried item back
    /// to `player_inventory`, returning any overflow that must be dropped.
    pub fn removed(
        &mut self,
        player_inventory: &mut PlayerInventory,
    ) -> Option<InventoryItemStack> {
        let carried = self.carried.copy_and_clear();
        if carried.is_empty() {
            return None;
        }
        player_inventory.place_item_back_in_inventory(carried)
    }

    /// `AbstractContainerMenu.moveItemStackTo(ItemStack, int, int, boolean)`.
    ///
    /// Exact Paper two-pass semantics:
    /// 1. If `stack.is_stackable()`, merge into occupied matching slots in
    ///    `[start_slot, end_slot)` (ascending, or descending when `backwards`).
    /// 2. If items remain, place into empty slots in `[start_slot, end_slot)`
    ///    respecting `slot.may_place` and `slot.get_max_stack_size_for_item`.
    pub fn move_item_stack_to(
        &self,
        containers: &mut [&mut dyn Container],
        stack: &mut InventoryItemStack,
        start_slot: usize,
        end_slot: usize,
        backwards: bool,
    ) -> bool {
        if start_slot >= end_slot || end_slot > self.slots.len() || stack.is_empty() {
            return false;
        }
        let mut anything_changed = false;

        if stack.is_stackable() {
            let mut i = if backwards { end_slot - 1 } else { start_slot };
            while !stack.is_empty()
                && (if backwards {
                    i >= start_slot
                } else {
                    i < end_slot
                })
            {
                let slot = &self.slots[i];
                let container = &mut *containers[slot.container_id];
                let target = slot.get_item(container).clone();
                if !target.is_empty()
                    && InventoryItemStack::is_same_item_same_components(stack, &target)
                {
                    let total = target.get_count().wrapping_add(stack.get_count());
                    let max_size = slot.get_max_stack_size_for_item(container, stack);
                    if total <= max_size {
                        stack.set_count(0);
                        let mut updated = target;
                        updated.set_count(total);
                        slot.set(container, updated);
                        anything_changed = true;
                    } else if target.get_count() < max_size {
                        stack.shrink(max_size - target.get_count());
                        let mut updated = target;
                        updated.set_count(max_size);
                        slot.set(container, updated);
                        anything_changed = true;
                    }
                }
                if backwards {
                    if i == start_slot {
                        break;
                    }
                    i -= 1;
                } else {
                    i += 1;
                }
            }
        }

        if !stack.is_empty() {
            let mut i = if backwards { end_slot - 1 } else { start_slot };
            while if backwards {
                i >= start_slot
            } else {
                i < end_slot
            } {
                let slot = &self.slots[i];
                let container = &mut *containers[slot.container_id];
                if slot.get_item(container).is_empty() && slot.may_place(stack) {
                    let max_size = slot.get_max_stack_size_for_item(container, stack);
                    let moved = stack.split(max_size);
                    slot.set_by_player(container, moved);
                    anything_changed = true;
                    break;
                }
                if backwards {
                    if i == start_slot {
                        break;
                    }
                    i -= 1;
                } else {
                    i += 1;
                }
            }
        }

        anything_changed
    }

    /// `AbstractContainerMenu.clicked(int, int, ContainerInput, Player)`.
    ///
    /// `player_container_id` identifies which entry in `containers` is the
    /// player's inventory (used for `ContainerInput::Swap`).
    /// `quick_move_fn` implements the concrete menu's `quickMoveStack(Player, int)`.
    #[allow(clippy::too_many_arguments)]
    pub fn clicked<F>(
        &mut self,
        slot_index: i32,
        button_num: i32,
        container_input: ContainerInput,
        ctx: &ContainerAccessContext,
        containers: &mut [&mut dyn Container],
        player_container_id: Option<usize>,
        mut quick_move_fn: F,
    ) -> ClickOutcome
    where
        F: FnMut(&AbstractContainerMenu, &mut [&mut dyn Container], usize) -> InventoryItemStack,
    {
        let mut outcome = ClickOutcome::default();

        if container_input == ContainerInput::QuickCraft {
            self.handle_quick_craft(slot_index, button_num, ctx, containers, &mut outcome);
            return outcome;
        }

        if self.quickcraft_status != 0 {
            self.reset_quick_craft();
            return outcome;
        }

        match container_input {
            ContainerInput::Pickup | ContainerInput::QuickMove
                if button_num == 0 || button_num == 1 =>
            {
                let click_action = if button_num == 0 {
                    ClickAction::Primary
                } else {
                    ClickAction::Secondary
                };
                if slot_index == SLOT_CLICKED_OUTSIDE {
                    if !self.carried.is_empty() {
                        if click_action == ClickAction::Primary {
                            let dropped = self.carried.copy_and_clear();
                            outcome.dropped_items.push(dropped);
                        } else {
                            let dropped = self.carried.split(1);
                            if !dropped.is_empty() {
                                outcome.dropped_items.push(dropped);
                            }
                        }
                    }
                } else if container_input == ContainerInput::QuickMove {
                    let Some(idx) = self.valid_slot_index(slot_index) else {
                        outcome.rejected = true;
                        return outcome;
                    };
                    if !self.slots[idx].may_pickup(ctx) {
                        outcome.rejected = true;
                        return outcome;
                    }
                    let mut clicked = quick_move_fn(self, containers, idx);
                    while !clicked.is_empty() {
                        let cid = self.slots[idx].container_id;
                        let slot_item = self.slots[idx].get_item(&*containers[cid]);
                        if !InventoryItemStack::is_same_item(slot_item, &clicked) {
                            break;
                        }
                        clicked = quick_move_fn(self, containers, idx);
                    }
                } else {
                    let Some(idx) = self.valid_slot_index(slot_index) else {
                        outcome.rejected = true;
                        return outcome;
                    };
                    self.handle_pickup_click(idx, click_action, ctx, containers, &mut outcome);
                }
            }
            ContainerInput::Swap
                if (0..9).contains(&button_num) || button_num == SLOT_OFFHAND as i32 =>
            {
                let Some(idx) = self.valid_slot_index(slot_index) else {
                    outcome.rejected = true;
                    return outcome;
                };
                let Some(player_cid) = player_container_id else {
                    outcome.rejected = true;
                    return outcome;
                };
                if player_cid >= containers.len() {
                    outcome.rejected = true;
                    return outcome;
                }
                self.handle_swap_click(
                    idx,
                    button_num as usize,
                    player_cid,
                    ctx,
                    containers,
                    &mut outcome,
                );
            }
            ContainerInput::Clone => {
                if !ctx.creative_mode || !self.carried.is_empty() {
                    outcome.rejected = true;
                    return outcome;
                }
                let Some(idx) = self.valid_slot_index(slot_index) else {
                    outcome.rejected = true;
                    return outcome;
                };
                let cid = self.slots[idx].container_id;
                let item = self.slots[idx].get_item(&*containers[cid]);
                if !item.is_empty() {
                    self.carried = item.copy_with_count(item.get_max_stack_size());
                }
            }
            ContainerInput::Throw => {
                if !self.carried.is_empty() {
                    outcome.rejected = true;
                    return outcome;
                }
                let Some(idx) = self.valid_slot_index(slot_index) else {
                    outcome.rejected = true;
                    return outcome;
                };
                let cid = self.slots[idx].container_id;
                let count = if button_num == 0 {
                    1
                } else {
                    self.slots[idx].get_item(&*containers[cid]).get_count()
                };
                if let Some(removed) =
                    self.slots[idx].try_remove(&mut *containers[cid], count, count, ctx)
                {
                    outcome.dropped_items.push(removed);
                } else {
                    outcome.rejected = true;
                }
            }
            ContainerInput::PickupAll if slot_index >= 0 => {
                let Some(idx) = self.valid_slot_index(slot_index) else {
                    outcome.rejected = true;
                    return outcome;
                };
                self.handle_pickup_all(idx, ctx, containers);
            }
            _ => {
                outcome.rejected = true;
            }
        }

        outcome
    }

    fn handle_quick_craft(
        &mut self,
        slot_index: i32,
        button_num: i32,
        ctx: &ContainerAccessContext,
        containers: &mut [&mut dyn Container],
        outcome: &mut ClickOutcome,
    ) {
        let prev_status = self.quickcraft_status;
        self.quickcraft_status = Self::get_quickcraft_header(button_num);

        if (prev_status != QUICKCRAFT_HEADER_CONTINUE
            || self.quickcraft_status != QUICKCRAFT_HEADER_END)
            && prev_status != self.quickcraft_status
        {
            self.reset_quick_craft();
            outcome.rejected = true;
            return;
        }
        if self.carried.is_empty() {
            self.reset_quick_craft();
            outcome.rejected = true;
            return;
        }

        match self.quickcraft_status {
            QUICKCRAFT_HEADER_START => {
                self.quickcraft_type = Self::get_quickcraft_type(button_num);
                if Self::is_valid_quickcraft_type(self.quickcraft_type, ctx.creative_mode) {
                    self.quickcraft_status = QUICKCRAFT_HEADER_CONTINUE;
                    self.quickcraft_slots.clear();
                } else {
                    self.reset_quick_craft();
                    outcome.rejected = true;
                }
            }
            QUICKCRAFT_HEADER_CONTINUE => {
                let Some(idx) = self.valid_slot_index(slot_index) else {
                    outcome.rejected = true;
                    return;
                };
                let slot = &self.slots[idx];
                let container = &*containers[slot.container_id];
                let carried = self.carried.clone();
                if Self::can_item_quick_replace(slot.get_item(container), &carried, true)
                    && slot.may_place(&carried)
                    && (self.quickcraft_type == QUICKCRAFT_TYPE_CLONE
                        || carried.get_count() > self.quickcraft_slots.len() as i32)
                {
                    self.quickcraft_slots.insert(idx);
                }
            }
            QUICKCRAFT_HEADER_END => {
                if !self.quickcraft_slots.is_empty() {
                    if self.quickcraft_slots.len() == 1 {
                        let single_idx = self.quickcraft_slots[0];
                        let button = if self.quickcraft_type == QUICKCRAFT_TYPE_GREEDY {
                            1
                        } else {
                            0
                        };
                        self.reset_quick_craft();
                        let click_action = if button == 0 {
                            ClickAction::Primary
                        } else {
                            ClickAction::Secondary
                        };
                        self.handle_pickup_click(
                            single_idx,
                            click_action,
                            ctx,
                            containers,
                            outcome,
                        );
                        return;
                    }

                    let source = self.carried.clone();
                    if source.is_empty() {
                        self.reset_quick_craft();
                        return;
                    }
                    let mut remaining = source.get_count();
                    let slot_count = self.quickcraft_slots.len();
                    let dragged: Vec<usize> = self.quickcraft_slots.iter().copied().collect();

                    for idx in dragged {
                        let slot = &self.slots[idx];
                        let container = &mut *containers[slot.container_id];
                        let slot_item = slot.get_item(container);
                        if Self::can_item_quick_replace(slot_item, &source, true)
                            && slot.may_place(&source)
                            && (self.quickcraft_type == QUICKCRAFT_TYPE_CLONE
                                || source.get_count() >= slot_count as i32)
                        {
                            let carry = slot_item.get_count();
                            let max_size = source
                                .get_max_stack_size()
                                .min(slot.get_max_stack_size_for_item(container, &source));
                            let place = Self::get_quick_craft_place_count(
                                slot_count,
                                self.quickcraft_type,
                                &source,
                            );
                            let new_count = (place.wrapping_add(carry)).min(max_size);
                            remaining -= new_count - carry;
                            slot.set_by_player(container, source.copy_with_count(new_count));
                        }
                    }

                    self.carried = source.copy_with_count(remaining);
                }
                self.reset_quick_craft();
            }
            _ => {
                self.reset_quick_craft();
                outcome.rejected = true;
            }
        }
    }

    fn handle_pickup_click(
        &mut self,
        idx: usize,
        click_action: ClickAction,
        ctx: &ContainerAccessContext,
        containers: &mut [&mut dyn Container],
        outcome: &mut ClickOutcome,
    ) {
        let slot = &self.slots[idx];
        let container = &mut *containers[slot.container_id];
        let clicked = slot.get_item(container).clone();
        let carried = self.carried.clone();

        if clicked.is_empty() {
            if !carried.is_empty() {
                let amount = if click_action == ClickAction::Primary {
                    carried.get_count()
                } else {
                    1
                };
                if !slot.may_place(&carried) {
                    outcome.rejected = true;
                    return;
                }
                self.carried = slot.safe_insert_amount(container, carried, amount);
            }
        } else if slot.may_pickup(ctx) {
            if carried.is_empty() {
                let amount = if click_action == ClickAction::Primary {
                    clicked.get_count()
                } else {
                    (clicked.get_count() + 1) / 2
                };
                if let Some(taken) = slot.try_remove(container, amount, i32::MAX, ctx) {
                    self.carried = taken;
                } else {
                    outcome.rejected = true;
                }
            } else if slot.may_place(&carried) {
                if InventoryItemStack::is_same_item_same_components(&clicked, &carried) {
                    let amount = if click_action == ClickAction::Primary {
                        carried.get_count()
                    } else {
                        1
                    };
                    self.carried = slot.safe_insert_amount(container, carried, amount);
                } else if carried.get_count()
                    <= slot.get_max_stack_size_for_item(container, &carried)
                {
                    self.carried = clicked;
                    slot.set_by_player(container, carried);
                }
            } else if InventoryItemStack::is_same_item_same_components(&clicked, &carried) {
                let max_take = carried.get_max_stack_size() - carried.get_count();
                if let Some(taken) = slot.try_remove(container, clicked.get_count(), max_take, ctx)
                {
                    self.carried.grow(taken.get_count());
                } else {
                    outcome.rejected = true;
                }
            } else {
                outcome.rejected = true;
            }
        } else {
            outcome.rejected = true;
        }
    }

    fn handle_swap_click(
        &mut self,
        idx: usize,
        player_slot: usize,
        player_cid: usize,
        ctx: &ContainerAccessContext,
        containers: &mut [&mut dyn Container],
        outcome: &mut ClickOutcome,
    ) {
        let slot_cid = self.slots[idx].container_id;
        let source = containers[player_cid].get_item(player_slot).clone();
        let target = self.slots[idx].get_item(&*containers[slot_cid]).clone();

        if source.is_empty() && target.is_empty() {
            return;
        }

        if source.is_empty() {
            if self.slots[idx].may_pickup(ctx) {
                let max_stack = target.get_max_stack_size();
                if let Some(taken) = self.slots[idx].try_remove(
                    &mut *containers[slot_cid],
                    max_stack,
                    max_stack,
                    ctx,
                ) {
                    containers[player_cid].set_item(player_slot, taken);
                } else {
                    outcome.rejected = true;
                }
            } else {
                outcome.rejected = true;
            }
        } else if target.is_empty() {
            if self.slots[idx].may_place(&source) {
                let max_size =
                    self.slots[idx].get_max_stack_size_for_item(&*containers[slot_cid], &source);
                if source.get_count() > max_size {
                    let mut remaining = source;
                    let moved = remaining.split(max_size);
                    containers[player_cid].set_item(player_slot, remaining);
                    self.slots[idx].set_by_player(&mut *containers[slot_cid], moved);
                } else {
                    containers[player_cid].set_item(player_slot, InventoryItemStack::empty());
                    self.slots[idx].set_by_player(&mut *containers[slot_cid], source);
                }
            } else {
                outcome.rejected = true;
            }
        } else if self.slots[idx].may_pickup(ctx) && self.slots[idx].may_place(&source) {
            let max_size =
                self.slots[idx].get_max_stack_size_for_item(&*containers[slot_cid], &source);
            if source.get_count() > max_size {
                let mut remaining = source;
                let moved = remaining.split(max_size);
                containers[player_cid].set_item(player_slot, remaining);
                self.slots[idx].set_by_player(&mut *containers[slot_cid], moved);
                // Put displaced `target` back into player's main inventory (never armor/offhand),
                // merging into compatible stacks first, then empty slots, else drop.
                let mut displaced = target;
                let main_size = containers[player_cid]
                    .get_container_size()
                    .min(INVENTORY_SIZE);
                if displaced.is_stackable() {
                    for s in 0..main_size {
                        if displaced.is_empty() {
                            break;
                        }
                        let existing = containers[player_cid].get_item(s).clone();
                        if !existing.is_empty()
                            && InventoryItemStack::is_same_item_same_components(
                                &existing, &displaced,
                            )
                        {
                            let max = containers[player_cid].get_max_stack_size_for_item(&existing);
                            let space = max - existing.get_count();
                            if space > 0 {
                                let move_count = displaced.get_count().min(space);
                                let mut updated = existing;
                                updated.grow(move_count);
                                displaced.shrink(move_count);
                                containers[player_cid].set_item(s, updated);
                            }
                        }
                    }
                }
                for s in 0..main_size {
                    if displaced.is_empty() {
                        break;
                    }
                    if containers[player_cid].get_item(s).is_empty() {
                        let max = containers[player_cid].get_max_stack_size_for_item(&displaced);
                        if max > 0 {
                            let moved = displaced.split(max);
                            containers[player_cid].set_item(s, moved);
                        }
                    }
                }
                if !displaced.is_empty() {
                    outcome.dropped_items.push(displaced);
                }
            } else {
                let taken = self.slots[idx].safe_take(
                    &mut *containers[slot_cid],
                    target.get_count(),
                    target.get_count(),
                    ctx,
                );
                if !taken.is_empty() {
                    containers[player_cid].set_item(player_slot, taken);
                    self.slots[idx].set_by_player(&mut *containers[slot_cid], source);
                } else {
                    outcome.rejected = true;
                }
            }
        } else {
            outcome.rejected = true;
        }
    }

    fn handle_pickup_all(
        &mut self,
        clicked_idx: usize,
        ctx: &ContainerAccessContext,
        containers: &mut [&mut dyn Container],
    ) {
        let clicked_slot = &self.slots[clicked_idx];
        let clicked_cid = clicked_slot.container_id;
        if self.carried.is_empty()
            || (clicked_slot.has_item(&*containers[clicked_cid]) && clicked_slot.may_pickup(ctx))
        {
            return;
        }
        // Paper two-pass PICKUP_ALL: pass 0 takes from non-full stacks, pass 1 takes from full stacks.
        for pass in 0..2 {
            for i in 0..self.slots.len() {
                if self.carried.get_count() >= self.carried.get_max_stack_size() {
                    return;
                }
                let slot = &self.slots[i];
                let container = &mut *containers[slot.container_id];
                let item = slot.get_item(container).clone();
                if !item.is_empty()
                    && Self::can_item_quick_replace(&item, &self.carried, true)
                    && slot.may_pickup(ctx)
                {
                    let is_full = item.get_count() == item.get_max_stack_size();
                    if pass == 0 && is_full {
                        continue;
                    }
                    let take = item
                        .get_count()
                        .min(self.carried.get_max_stack_size() - self.carried.get_count());
                    if let Some(removed) = slot.try_remove(container, take, take, ctx) {
                        self.carried.grow(removed.get_count());
                    }
                }
            }
        }
    }

    /// `AbstractContainerMenu.resetQuickCraft()`.
    pub fn reset_quick_craft(&mut self) {
        self.quickcraft_status = 0;
        self.quickcraft_slots.clear();
    }

    /// `AbstractContainerMenu.getQuickcraftType(int)`.
    pub const fn get_quickcraft_type(mask: i32) -> i32 {
        (mask >> 2) & 3
    }

    /// `AbstractContainerMenu.getQuickcraftHeader(int)`.
    pub const fn get_quickcraft_header(mask: i32) -> i32 {
        mask & 3
    }

    /// `AbstractContainerMenu.getQuickcraftMask(int, int)`.
    pub const fn get_quickcraft_mask(header: i32, quickcraft_type: i32) -> i32 {
        (header & 3) | ((quickcraft_type & 3) << 2)
    }

    /// `AbstractContainerMenu.isValidQuickcraftType(int, Player)`.
    pub const fn is_valid_quickcraft_type(quickcraft_type: i32, creative_mode: bool) -> bool {
        match quickcraft_type {
            QUICKCRAFT_TYPE_CHARITABLE | QUICKCRAFT_TYPE_GREEDY => true,
            QUICKCRAFT_TYPE_CLONE => creative_mode,
            _ => false,
        }
    }

    /// `AbstractContainerMenu.getQuickCraftPlaceCount(Set<Slot>, int, ItemStack)`.
    pub fn get_quick_craft_place_count(
        slot_count: usize,
        quickcraft_type: i32,
        stack: &InventoryItemStack,
    ) -> i32 {
        if slot_count == 0 {
            return stack.get_count();
        }
        match quickcraft_type {
            QUICKCRAFT_TYPE_CHARITABLE => mth::floor(stack.get_count() as f32 / slot_count as f32),
            QUICKCRAFT_TYPE_GREEDY => 1,
            QUICKCRAFT_TYPE_CLONE => stack.get_max_stack_size(),
            _ => stack.get_count(),
        }
    }

    /// `AbstractContainerMenu.canItemQuickReplace(Slot, ItemStack, boolean)`.
    pub fn can_item_quick_replace(
        slot_item: &InventoryItemStack,
        stack: &InventoryItemStack,
        ignore_size: bool,
    ) -> bool {
        if slot_item.is_empty() {
            return true;
        }
        if !InventoryItemStack::is_same_item_same_components(stack, slot_item) {
            return false;
        }
        let added = if ignore_size { 0 } else { stack.get_count() };
        slot_item.get_count().wrapping_add(added) <= stack.get_max_stack_size()
    }

    /// `AbstractContainerMenu.getRedstoneSignalFromContainer(Container)` —
    /// exact Paper formula using `Mth.lerpDiscrete(totalPercent / size, 0, 15)`.
    pub fn get_redstone_signal_from_container(container: Option<&dyn Container>) -> i32 {
        let Some(container) = container else {
            return 0;
        };
        let size = container.get_container_size();
        if size == 0 {
            return 0;
        }
        let mut total_percent: f32 = 0.0;
        for slot in 0..size {
            let item = container.get_item(slot);
            if !item.is_empty() {
                let max = container.get_max_stack_size_for_item(item);
                if max > 0 {
                    total_percent += item.get_count() as f32 / max as f32;
                }
            }
        }
        total_percent /= size as f32;
        mth::lerp_discrete(total_percent, 0, 15)
    }

    fn valid_slot_index(&self, slot_index: i32) -> Option<usize> {
        if slot_index < 0 {
            return None;
        }
        let idx = slot_index as usize;
        (idx < self.slots.len()).then_some(idx)
    }

    fn read_slot_item(&self, slot: &Slot, containers: &[&dyn Container]) -> InventoryItemStack {
        containers
            .get(slot.container_id)
            .map_or_else(InventoryItemStack::empty, |c| slot.get_item(*c).clone())
    }

    fn trigger_slot_listeners(&mut self, index: usize, current: &InventoryItemStack) {
        if !InventoryItemStack::matches(&self.last_slots[index], current) {
            self.last_slots[index] = current.clone();
            for listener in self.listeners.values_mut() {
                listener.slot_changed(self.container_id, index, current);
            }
        }
    }

    fn synchronize_slot_to_remote(&mut self, index: usize, current: &InventoryItemStack) {
        if self.suppress_remote_updates {
            return;
        }
        if !self.remote_slots[index].matches(current) {
            self.remote_slots[index].force(current);
            let state_id = self.increment_state_id();
            if let Some(sync) = self.synchronizer.as_mut() {
                sync.send_slot_change(self.container_id, index, current, state_id);
            }
        }
    }

    fn synchronize_carried_to_remote(&mut self) {
        if self.suppress_remote_updates {
            return;
        }
        if !self.remote_carried.matches(&self.carried) {
            self.remote_carried.force(&self.carried);
            let state_id = self.increment_state_id();
            if let Some(sync) = self.synchronizer.as_mut() {
                sync.send_carried_change(self.container_id, &self.carried, state_id);
            }
        }
    }

    fn update_data_slot_listeners(&mut self, index: usize, value: i32) {
        for listener in self.listeners.values_mut() {
            listener.data_changed(self.container_id, index, value);
        }
    }

    fn synchronize_data_slot_to_remote(&mut self, index: usize, value: i32) {
        if self.suppress_remote_updates {
            return;
        }
        if self.remote_data_slots[index] != value {
            self.remote_data_slots[index] = value;
            if let Some(sync) = self.synchronizer.as_mut() {
                sync.send_data_change(self.container_id, index, value);
            }
        }
    }
}
