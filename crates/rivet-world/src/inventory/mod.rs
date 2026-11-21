//! Paper 26.2 inventory, slot, container, data-slot, remote synchronization,
//! menu, and player-inventory foundations (issue #460).
//!
//! Mirrors `net.minecraft.world.{Container, ContainerHelper, SimpleContainer,
//! CompoundContainer, MenuProvider, SimpleMenuProvider}`,
//! `net.minecraft.world.inventory.{Slot, ContainerData, SimpleContainerData,
//! DataSlot, RemoteSlot, ContainerListener, ContainerSynchronizer,
//! AbstractContainerMenu, ClickAction, ContainerInput, ContainerLevelAccess}`,
//! and the core `net.minecraft.world.entity.player.Inventory` storage behavior.

pub mod container;
pub mod data;
pub mod item_stack;
pub mod menu;
pub mod player_inventory;
pub mod remote;
pub mod slot;

pub use container::{
    CompoundContainer, Container, ContainerAccessContext, ContainerExt, ContainerHelper,
    ContainerIter, DEFAULT_DISTANCE_BUFFER, DEFAULT_MAX_STACK_SIZE, LARGE_MAX_STACK_SIZE,
    SimpleContainer,
};
pub use data::{ContainerData, DataSlot, SimpleContainerData};
pub use item_stack::{
    ComponentMap, DEFAULT_ITEM_MAX_STACK_SIZE, HashedStack, InventoryItemStack, ItemId,
};
pub use menu::{
    AbstractContainerMenu, ClickAction, ClickOutcome, ContainerInput, ContainerLevelAccess,
    MenuProvider, QUICKCRAFT_HEADER_CONTINUE, QUICKCRAFT_HEADER_END, QUICKCRAFT_HEADER_START,
    QUICKCRAFT_TYPE_CHARITABLE, QUICKCRAFT_TYPE_CLONE, QUICKCRAFT_TYPE_GREEDY,
    SLOT_CLICKED_OUTSIDE, STATE_ID_MASK, SimpleMenuProvider,
};
pub use player_inventory::{
    EQUIPMENT_SLOT_END_INCLUSIVE, EQUIPMENT_SLOT_START, INVENTORY_SIZE, InvalidHotbarSlot,
    NOT_FOUND_INDEX, PlayerInventory, SELECTION_SIZE, SLOT_BODY_ARMOR, SLOT_OFFHAND, SLOT_SADDLE,
};
pub use remote::{ContainerSynchronizer, ListenerId, MenuContainerListener, RemoteSlot};
pub use slot::{CustomSlotPolicy, DefaultSlotPolicy, Slot, SlotPolicy};

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use rivet_registry::core::BlockPos;
    use rivet_text::Component;

    use super::*;

    fn stone(count: i32) -> InventoryItemStack {
        InventoryItemStack::new(ItemId::from_name("minecraft:stone").unwrap(), count)
    }

    fn granite(count: i32) -> InventoryItemStack {
        InventoryItemStack::new(ItemId::from_name("minecraft:granite").unwrap(), count)
    }

    fn ender_pearl(count: i32) -> InventoryItemStack {
        InventoryItemStack::with_max_stack_size(
            ItemId::from_name("minecraft:ender_pearl").unwrap(),
            count,
            16,
        )
    }

    fn total_items(
        containers: &[&dyn Container],
        carried: &InventoryItemStack,
        dropped: &[InventoryItemStack],
    ) -> i32 {
        let mut total = carried.get_count();
        for c in containers {
            for slot in 0..c.get_container_size() {
                total += c.get_item(slot).get_count();
            }
        }
        for d in dropped {
            total += d.get_count();
        }
        total
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Event {
        ListenerSlot(u64, usize, i32),
        ListenerData(u64, usize, i32),
        SyncInitial(i32, usize, i32),
        SyncSlot(usize, i32, i32),
        SyncCarried(i32, i32),
        SyncData(usize, i32),
    }

    struct RecordingListener {
        id: u64,
        log: Rc<RefCell<Vec<Event>>>,
    }

    impl MenuContainerListener for RecordingListener {
        fn slot_changed(
            &mut self,
            _container_id: i32,
            slot_index: usize,
            stack: &InventoryItemStack,
        ) {
            self.log
                .borrow_mut()
                .push(Event::ListenerSlot(self.id, slot_index, stack.get_count()));
        }

        fn data_changed(&mut self, _container_id: i32, data_slot_index: usize, value: i32) {
            self.log
                .borrow_mut()
                .push(Event::ListenerData(self.id, data_slot_index, value));
        }
    }

    struct RecordingSync {
        log: Rc<RefCell<Vec<Event>>>,
    }

    impl ContainerSynchronizer for RecordingSync {
        fn send_initial_data(
            &mut self,
            container_id: i32,
            items: &[InventoryItemStack],
            _carried: &InventoryItemStack,
            _initial_data: &[i32],
            state_id: i32,
        ) {
            self.log
                .borrow_mut()
                .push(Event::SyncInitial(container_id, items.len(), state_id));
        }

        fn send_slot_change(
            &mut self,
            _container_id: i32,
            slot: usize,
            stack: &InventoryItemStack,
            state_id: i32,
        ) {
            self.log
                .borrow_mut()
                .push(Event::SyncSlot(slot, stack.get_count(), state_id));
        }

        fn send_carried_change(
            &mut self,
            _container_id: i32,
            stack: &InventoryItemStack,
            state_id: i32,
        ) {
            self.log
                .borrow_mut()
                .push(Event::SyncCarried(stack.get_count(), state_id));
        }

        fn send_data_change(&mut self, _container_id: i32, id: usize, value: i32) {
            self.log.borrow_mut().push(Event::SyncData(id, value));
        }
    }

    #[test]
    fn simple_container_merges_before_empty_clamps_and_tracks_changes() {
        let mut container = SimpleContainer::with_max_stack_size(3, 16);
        assert!(container.is_empty());
        assert!(container.get_item(99).is_empty());

        // Slot 1 has 10 stone, slot 0 is empty -> add_item merges into slot 1 first.
        container.set_item(1, stone(10));
        assert_eq!(container.changed_count(), 1);

        let rem = container.add_item(stone(10));
        assert!(rem.is_empty());
        assert_eq!(container.get_item(1).get_count(), 16);
        assert_eq!(container.get_item(0).get_count(), 4);
        assert!(container.get_item(2).is_empty());

        // Removing from an empty slot does not increment changed_count.
        let changes_before = container.changed_count();
        assert!(container.remove_item(2, 5).is_empty());
        assert_eq!(container.changed_count(), changes_before);

        // remove_item_type scans in reverse slot order (slot 1 before slot 0).
        let taken = container.remove_item_type(ItemId::from_name("minecraft:stone").unwrap(), 18);
        assert_eq!(taken.get_count(), 18);
        assert!(container.get_item(1).is_empty());
        assert_eq!(container.get_item(0).get_count(), 2);

        // Overflow returns exact remainder (item conservation).
        let leftover = container.add_item(stone(64));
        // Capacity left: slot 0 has 14, slot 1 has 16, slot 2 has 16 => 46 inserted, 18 leftover.
        assert_eq!(leftover.get_count(), 18);
        assert_eq!(
            container.count_item(ItemId::from_name("minecraft:stone").unwrap())
                + leftover.get_count(),
            66
        );
    }

    #[test]
    fn compound_container_delegates_across_boundary_and_min_stack_limit() {
        let mut c1 = SimpleContainer::with_max_stack_size(2, 32);
        let mut c2 = SimpleContainer::with_max_stack_size(3, 16);
        c1.set_item(0, stone(5));
        c2.set_item(2, granite(7));

        let mut compound = CompoundContainer::new(c1, c2);
        assert_eq!(compound.get_container_size(), 5);
        assert_eq!(compound.get_max_stack_size(), 16);
        assert_eq!(compound.get_item(0).get_count(), 5);
        assert_eq!(compound.get_item(4).get_count(), 7);

        let taken = compound.remove_item(4, 3);
        assert_eq!(taken.get_count(), 3);
        assert_eq!(compound.get_item(4).get_count(), 4);
    }

    #[test]
    fn slot_limits_and_negative_rejection_rules() {
        let ctx = ContainerAccessContext::survival(1);
        let mut container = SimpleContainer::new(3);

        // Slot 0: capped at max 4 items.
        let capped_slot = Slot::with_policy(
            0,
            0,
            0,
            0,
            Box::new(CustomSlotPolicy {
                max_stack_size: Some(4),
                ..Default::default()
            }),
        );
        let remainder = capped_slot.safe_insert(&mut container, stone(10));
        assert_eq!(container.get_item(0).get_count(), 4);
        assert_eq!(remainder.get_count(), 6);

        // Slot 1: output-style slot (may_place = false, may_pickup = true).
        container.set_item(1, stone(5));
        let output_slot = Slot::with_policy(
            0,
            1,
            0,
            0,
            Box::new(CustomSlotPolicy {
                allow_place: false,
                allow_pickup: true,
                ..Default::default()
            }),
        );
        // Controlled negative: cannot place into output slot.
        let rejected = output_slot.safe_insert(&mut container, granite(3));
        assert_eq!(rejected.get_count(), 3);
        assert_eq!(container.get_item(1).get_count(), 5);

        // Controlled negative: partial extraction denied when allow_modification is false.
        assert!(output_slot.try_remove(&mut container, 2, 2, &ctx).is_none());
        assert_eq!(container.get_item(1).get_count(), 5);
        // Full extraction succeeds.
        let full = output_slot.try_remove(&mut container, 5, 5, &ctx).unwrap();
        assert_eq!(full.get_count(), 5);
        assert!(container.get_item(1).is_empty());

        // Slot 2: locked pickup (may_pickup = false).
        container.set_item(2, stone(3));
        let locked_slot = Slot::with_policy(
            0,
            2,
            0,
            0,
            Box::new(CustomSlotPolicy {
                allow_pickup: false,
                ..Default::default()
            }),
        );
        assert!(locked_slot.try_remove(&mut container, 3, 3, &ctx).is_none());
        assert_eq!(container.get_item(2).get_count(), 3);
    }

    #[test]
    fn menu_state_id_wraps_at_32767_and_listener_ordering_matches_paper() {
        let mut container = SimpleContainer::new(2);
        container.set_item(0, stone(3));
        container.set_item(1, granite(5));

        let mut data = SimpleContainerData::new(2);
        data.set(0, 42);
        data.set(1, 99);

        let mut menu = AbstractContainerMenu::new(7);
        menu.add_slot(Slot::new(0, 0, 0, 0));
        menu.add_slot(Slot::new(0, 1, 18, 0));
        menu.add_data_slots(0, 2);

        // Verify 15-bit state_id wraparound.
        for _ in 0..STATE_ID_MASK {
            menu.increment_state_id();
        }
        assert_eq!(menu.get_state_id(), 32767);
        assert_eq!(menu.increment_state_id(), 0);
        assert_eq!(menu.increment_state_id(), 1);

        let log = Rc::new(RefCell::new(Vec::new()));
        let sync = Box::new(RecordingSync { log: log.clone() });
        menu.set_synchronizer(sync, &[&container], &[&data]);

        // Duplicate ListenerId is ignored; listeners fire in registration order.
        menu.add_slot_listener(
            ListenerId(1),
            Box::new(RecordingListener {
                id: 1,
                log: log.clone(),
            }),
            &[&container],
            &[&data],
        );
        menu.add_slot_listener(
            ListenerId(1),
            Box::new(RecordingListener {
                id: 99,
                log: log.clone(),
            }),
            &[&container],
            &[&data],
        );
        menu.add_slot_listener(
            ListenerId(2),
            Box::new(RecordingListener {
                id: 2,
                log: log.clone(),
            }),
            &[&container],
            &[&data],
        );

        log.borrow_mut().clear();

        // Mutate slot 0, slot 1, carried, and data slot 1.
        container.set_item(0, stone(4));
        container.set_item(1, granite(6));
        menu.set_carried(ender_pearl(2));
        data.set(1, 100);

        menu.broadcast_changes(&[&container], &[&data]);

        // Exact Paper order: slot 0 (L1, L2, sync), slot 1 (L1, L2, sync),
        // carried (sync), then data slot 1 (L1, L2, sync).
        let events = log.borrow().clone();
        assert_eq!(
            events,
            vec![
                Event::ListenerSlot(1, 0, 4),
                Event::ListenerSlot(2, 0, 4),
                Event::SyncSlot(0, 4, 3),
                Event::ListenerSlot(1, 1, 6),
                Event::ListenerSlot(2, 1, 6),
                Event::SyncSlot(1, 6, 4),
                Event::SyncCarried(2, 5),
                Event::ListenerData(1, 1, 100),
                Event::ListenerData(2, 1, 100),
                Event::SyncData(1, 100),
            ]
        );
    }

    #[test]
    fn move_item_stack_to_merges_first_and_respects_direction_and_limits() {
        let mut container = SimpleContainer::new(4);
        container.set_item(2, stone(60));

        let mut menu = AbstractContainerMenu::new(1);
        // Slot 0 rejects placement; slots 1..4 accept placement.
        menu.add_slot(Slot::with_policy(
            0,
            0,
            0,
            0,
            Box::new(CustomSlotPolicy {
                allow_place: false,
                ..Default::default()
            }),
        ));
        menu.add_slot(Slot::new(0, 1, 0, 0));
        menu.add_slot(Slot::new(0, 2, 0, 0));
        menu.add_slot(Slot::new(0, 3, 0, 0));

        // Forward: merges 4 into slot 2 (now 64), then skips unplaceable slot 0 and places 6 in slot 1.
        let mut incoming = stone(10);
        let mut refs: [&mut dyn Container; 1] = [&mut container];
        assert!(menu.move_item_stack_to(&mut refs, &mut incoming, 0, 4, false));
        assert!(incoming.is_empty());
        assert!(container.get_item(0).is_empty());
        assert_eq!(container.get_item(1).get_count(), 6);
        assert_eq!(container.get_item(2).get_count(), 64);

        // Backwards: places into slot 3 (highest index empty slot).
        let mut incoming_back = granite(5);
        let mut refs: [&mut dyn Container; 1] = [&mut container];
        assert!(menu.move_item_stack_to(&mut refs, &mut incoming_back, 0, 4, true));
        assert!(incoming_back.is_empty());
        assert_eq!(container.get_item(3).get_count(), 5);
    }

    #[test]
    fn click_transactions_preserve_item_conservation_and_reject_invalid_ops() {
        let survival = ContainerAccessContext::survival(1);
        let creative = ContainerAccessContext::creative(1);

        let mut chest = SimpleContainer::new(4);
        chest.set_item(0, stone(10));
        chest.set_item(1, stone(6));

        let mut player_inv = PlayerInventory::new();
        player_inv.set_item(0, granite(4));

        let mut menu = AbstractContainerMenu::new(2);
        for s in 0..4 {
            menu.add_slot(Slot::new(0, s, 0, 0));
        }
        menu.add_slot(Slot::new(1, 0, 0, 0));

        let initial_total = 20;
        let mut dropped = Vec::new();
        let noop_quick_move =
            |_: &AbstractContainerMenu, _: &mut [&mut dyn Container], _: usize| {
                InventoryItemStack::empty()
            };

        // 1. Primary pickup from slot 0 (10 stone -> carried).
        {
            let mut refs: [&mut dyn Container; 2] = [&mut chest, &mut player_inv];
            let out = menu.clicked(
                0,
                0,
                ContainerInput::Pickup,
                &survival,
                &mut refs,
                Some(1),
                noop_quick_move,
            );
            assert!(!out.rejected);
            dropped.extend(out.dropped_items);
        }
        assert_eq!(menu.get_carried().get_count(), 10);
        assert_eq!(
            total_items(&[&chest, &player_inv], menu.get_carried(), &dropped),
            initial_total
        );

        // 2. QuickCraft CHARITABLE drag across slots 0, 2, 3 (10 / 3 = 3 each, 1 left in carried).
        let start_mask = AbstractContainerMenu::get_quickcraft_mask(
            QUICKCRAFT_HEADER_START,
            QUICKCRAFT_TYPE_CHARITABLE,
        );
        let cont_mask = AbstractContainerMenu::get_quickcraft_mask(
            QUICKCRAFT_HEADER_CONTINUE,
            QUICKCRAFT_TYPE_CHARITABLE,
        );
        let end_mask = AbstractContainerMenu::get_quickcraft_mask(
            QUICKCRAFT_HEADER_END,
            QUICKCRAFT_TYPE_CHARITABLE,
        );
        {
            let mut refs: [&mut dyn Container; 2] = [&mut chest, &mut player_inv];
            assert!(
                !menu
                    .clicked(
                        SLOT_CLICKED_OUTSIDE,
                        start_mask,
                        ContainerInput::QuickCraft,
                        &survival,
                        &mut refs,
                        Some(1),
                        noop_quick_move
                    )
                    .rejected
            );
            assert!(
                !menu
                    .clicked(
                        0,
                        cont_mask,
                        ContainerInput::QuickCraft,
                        &survival,
                        &mut refs,
                        Some(1),
                        noop_quick_move
                    )
                    .rejected
            );
            assert!(
                !menu
                    .clicked(
                        2,
                        cont_mask,
                        ContainerInput::QuickCraft,
                        &survival,
                        &mut refs,
                        Some(1),
                        noop_quick_move
                    )
                    .rejected
            );
            assert!(
                !menu
                    .clicked(
                        3,
                        cont_mask,
                        ContainerInput::QuickCraft,
                        &survival,
                        &mut refs,
                        Some(1),
                        noop_quick_move
                    )
                    .rejected
            );
            assert!(
                !menu
                    .clicked(
                        SLOT_CLICKED_OUTSIDE,
                        end_mask,
                        ContainerInput::QuickCraft,
                        &survival,
                        &mut refs,
                        Some(1),
                        noop_quick_move
                    )
                    .rejected
            );
        }
        assert_eq!(chest.get_item(0).get_count(), 3);
        assert_eq!(chest.get_item(2).get_count(), 3);
        assert_eq!(chest.get_item(3).get_count(), 3);
        assert_eq!(menu.get_carried().get_count(), 1);
        assert_eq!(
            total_items(&[&chest, &player_inv], menu.get_carried(), &dropped),
            initial_total
        );

        // 3. PickupAll collects all stone from slots 0, 1, 2, 3 into carried (1 + 3 + 6 + 3 + 3 = 16).
        // Click an empty slot? Slot 0..3 are non-empty, so first drop carried? Instead place carried into slot 4?
        // Actually in Paper PICKUP_ALL requires clicking an empty slot (or one that cannot be picked up).
        // Let's take 1 from slot 0? No, let's click slot 0 with PickupAll when slot 0 is empty:
        // First clear slot 0 via swap with hotbar slot 1 (empty).
        {
            let mut refs: [&mut dyn Container; 2] = [&mut chest, &mut player_inv];
            let out = menu.clicked(
                0,
                1,
                ContainerInput::Swap,
                &survival,
                &mut refs,
                Some(1),
                noop_quick_move,
            );
            assert!(!out.rejected);
            dropped.extend(out.dropped_items);
        }
        assert!(chest.get_item(0).is_empty());
        assert_eq!(player_inv.get_item(1).get_count(), 3);
        {
            let mut refs: [&mut dyn Container; 2] = [&mut chest, &mut player_inv];
            let out = menu.clicked(
                0,
                0,
                ContainerInput::PickupAll,
                &survival,
                &mut refs,
                Some(1),
                noop_quick_move,
            );
            assert!(!out.rejected);
        }
        // Carried had 1 stone; chest had slots 1(6), 2(3), 3(3) => 1 + 12 = 13 stone in carried.
        assert_eq!(menu.get_carried().get_count(), 13);
        assert_eq!(
            total_items(&[&chest, &player_inv], menu.get_carried(), &dropped),
            initial_total
        );

        // 4. Outside secondary click drops 1 item; conservation still holds.
        {
            let mut refs: [&mut dyn Container; 2] = [&mut chest, &mut player_inv];
            let out = menu.clicked(
                SLOT_CLICKED_OUTSIDE,
                1,
                ContainerInput::Pickup,
                &survival,
                &mut refs,
                Some(1),
                noop_quick_move,
            );
            assert!(!out.rejected);
            dropped.extend(out.dropped_items);
        }
        assert_eq!(menu.get_carried().get_count(), 12);
        assert_eq!(
            total_items(&[&chest, &player_inv], menu.get_carried(), &dropped),
            initial_total
        );

        // 5. Controlled negative: survival Clone is rejected; creative Clone requires empty carried.
        {
            let mut refs: [&mut dyn Container; 2] = [&mut chest, &mut player_inv];
            let out = menu.clicked(
                4,
                2,
                ContainerInput::Clone,
                &survival,
                &mut refs,
                Some(1),
                noop_quick_move,
            );
            assert!(out.rejected);
            // Clear carried via menu.removed -> goes back to player_inv.
        }
        if let Some(overflow) = menu.removed(&mut player_inv) {
            dropped.push(overflow);
        }
        assert!(menu.get_carried().is_empty());
        assert_eq!(
            total_items(&[&chest, &player_inv], menu.get_carried(), &dropped),
            initial_total
        );

        // Creative clone succeeds when carried is empty.
        {
            let mut refs: [&mut dyn Container; 2] = [&mut chest, &mut player_inv];
            let out = menu.clicked(
                4,
                2,
                ContainerInput::Clone,
                &creative,
                &mut refs,
                Some(1),
                noop_quick_move,
            );
            assert!(!out.rejected);
        }
        assert_eq!(menu.get_carried().get_count(), 64);
    }

    #[test]
    fn player_inventory_hotbar_priority_and_conservation() {
        let mut inv = PlayerInventory::new();
        assert!(inv.set_selected_slot(4).is_ok());
        assert_eq!(inv.get_selected_slot(), 4);
        // Controlled negative: out-of-range hotbar slots rejected.
        assert_eq!(inv.set_selected_slot(-1), Err(InvalidHotbarSlot(-1)));
        assert_eq!(inv.set_selected_slot(9), Err(InvalidHotbarSlot(9)));

        // Remaining-space priority: selected (4) -> offhand (40) -> 0..36.
        inv.set_item(0, stone(10));
        inv.set_item(SLOT_OFFHAND, stone(10));
        inv.set_item(4, stone(10));
        assert_eq!(inv.get_slot_with_remaining_space(&stone(1)), 4);

        inv.set_item(4, stone(64));
        assert_eq!(
            inv.get_slot_with_remaining_space(&stone(1)),
            SLOT_OFFHAND as i32
        );

        inv.set_item(SLOT_OFFHAND, stone(64));
        assert_eq!(inv.get_slot_with_remaining_space(&stone(1)), 0);

        // Fill all 36 slots and verify place_item_back_in_inventory returns exact overflow.
        for s in 0..INVENTORY_SIZE {
            inv.set_item(s, granite(64));
        }
        let overflow = inv.place_item_back_in_inventory(stone(7)).unwrap();
        assert_eq!(overflow.get_count(), 7);
    }

    #[test]
    fn redstone_signal_and_menu_provider_seam() {
        let mut container = SimpleContainer::new(2);
        assert_eq!(
            AbstractContainerMenu::get_redstone_signal_from_container(Some(&container)),
            0
        );
        container.set_item(0, stone(64));
        container.set_item(1, stone(64));
        assert_eq!(
            AbstractContainerMenu::get_redstone_signal_from_container(Some(&container)),
            15
        );

        let access = ContainerLevelAccess::create("minecraft:overworld", BlockPos::new(10, 64, -5));
        assert!(access.is_block());
        assert_eq!(
            access.evaluate(|dim, pos| (dim.to_string(), pos.get_y())),
            Some(("minecraft:overworld".to_string(), 64))
        );
        assert_eq!(ContainerLevelAccess::Null.evaluate_or(99, |_, _| 1), 99);

        let provider = SimpleMenuProvider::new(Component::literal("Chest"), |cid, _| {
            Some(AbstractContainerMenu::new(cid))
        });
        assert_eq!(provider.get_display_name(), Component::literal("Chest"));
        let menu = provider
            .create_menu(3, &ContainerAccessContext::survival(1))
            .unwrap();
        assert_eq!(menu.container_id, 3);
    }

    #[test]
    fn differing_max_stack_size_components_do_not_merge_or_lose_items() {
        let mut container = SimpleContainer::new(1);
        let limited = InventoryItemStack::with_max_stack_size(ItemId(1), 16, 16);
        let normal = InventoryItemStack::with_max_stack_size(ItemId(1), 32, 64);

        assert!(!InventoryItemStack::is_same_item_same_components(
            &limited, &normal
        ));
        assert!(!InventoryItemStack::matches(&limited, &normal));
        assert_ne!(limited.component_hash(), normal.component_hash());

        container.set_item(0, limited);
        let slot = Slot::new(0, 0, 0, 0);
        let remainder = slot.safe_insert(&mut container, normal);
        assert_eq!(container.get_item(0).get_count(), 16);
        assert_eq!(remainder.get_count(), 32);
    }

    #[test]
    fn non_stackable_multi_count_insertion_preserves_extra_items() {
        let mut inv = PlayerInventory::new();
        let mut tools = InventoryItemStack::with_max_stack_size(ItemId(1), 2, 1);
        assert!(inv.add(&mut tools));
        assert_eq!(inv.get_item(0).get_count(), 1);
        assert_eq!(tools.get_count(), 1);
    }

    #[test]
    fn invalid_explicit_slot_does_not_consume_items() {
        let mut inv = PlayerInventory::new();
        let mut stack = stone(12);
        assert!(!inv.add_at_slot(999, &mut stack));
        assert_eq!(stack.get_count(), 12);
        assert!(!inv.add_at_slot(-5, &mut stack));
        assert_eq!(stack.get_count(), 12);
        assert!(inv.is_empty());
    }

    #[test]
    fn capped_swap_uses_main_inventory_only_and_never_equipment_slots() {
        let mut chest = SimpleContainer::new(1);
        chest.set_item(0, granite(10));

        let mut player_inv = PlayerInventory::new();
        // Hotbar slot 0 holds 32 stone; remaining 35 main slots are filled with granite
        // (slot 1 has space for 4 more granite; slots 2..36 are full).
        player_inv.set_item(0, stone(32));
        player_inv.set_item(1, granite(60));
        for s in 2..INVENTORY_SIZE {
            player_inv.set_item(s, granite(64));
        }

        let mut menu = AbstractContainerMenu::new(1);
        menu.add_slot(Slot::with_policy(
            0,
            0,
            0,
            0,
            Box::new(CustomSlotPolicy {
                max_stack_size: Some(16),
                ..CustomSlotPolicy::default()
            }),
        ));
        let survival = ContainerAccessContext::survival(1);
        let noop_quick_move = |_menu: &AbstractContainerMenu,
                               _containers: &mut [&mut dyn Container],
                               _slot: usize| InventoryItemStack::empty();

        let outcome = {
            let mut refs: [&mut dyn Container; 2] = [&mut chest, &mut player_inv];
            menu.clicked(
                0,
                0,
                ContainerInput::Swap,
                &survival,
                &mut refs,
                Some(1),
                noop_quick_move,
            )
        };
        assert!(!outcome.rejected);
        // Capped slot gets 16 stone; hotbar slot 0 keeps 16 stone.
        assert_eq!(chest.get_item(0).get_count(), 16);
        assert_eq!(player_inv.get_item(0).get_count(), 16);
        // Displaced 10 granite: 4 merged into main slot 1, 6 dropped, and armor/offhand stay empty.
        assert_eq!(player_inv.get_item(1).get_count(), 64);
        assert_eq!(outcome.dropped_items.len(), 1);
        assert_eq!(outcome.dropped_items[0].get_count(), 6);
        for eq_slot in INVENTORY_SIZE..player_inv.get_container_size() {
            assert!(
                player_inv.get_item(eq_slot).is_empty(),
                "equipment slot {eq_slot} must not receive displaced swap items"
            );
        }
    }
}
