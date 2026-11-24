//! Conformance tests for native world, entity, player, item, inventory, and
//! menu capability facades (#508).

use std::thread;

use rivet_api::native::{
    BatchMode, BlockPos, BlockStateSnapshot, Capability, CapabilitySet, ChunkPos, CommandOutcome,
    EntityCommand, FacadeError, GameMode, HandleError, InventoryCommand, InventoryKind,
    ItemStackSnapshot, MenuCommand, PlayerCommand, SyntheticWorldState, TickCommand,
    TickCommandBatch, WorldCommand, WorldId,
};

fn all_facade_caps() -> CapabilitySet {
    CapabilitySet::from_iter([
        Capability::WorldRead,
        Capability::WorldWrite,
        Capability::EntityRead,
        Capability::EntityWrite,
        Capability::PlayerRead,
        Capability::PlayerWrite,
        Capability::InventoryRead,
        Capability::InventoryWrite,
        Capability::MenuRead,
        Capability::MenuWrite,
    ])
}

#[test]
fn block_pos_to_chunk_pos_uses_java_arithmetic_right_shift() {
    assert_eq!(BlockPos::new(0, 64, 0).chunk_pos(), ChunkPos::new(0, 0));
    assert_eq!(BlockPos::new(15, 64, 15).chunk_pos(), ChunkPos::new(0, 0));
    assert_eq!(BlockPos::new(16, 64, 16).chunk_pos(), ChunkPos::new(1, 1));
    assert_eq!(BlockPos::new(-1, 64, -1).chunk_pos(), ChunkPos::new(-1, -1));
    assert_eq!(
        BlockPos::new(-16, 64, -16).chunk_pos(),
        ChunkPos::new(-1, -1)
    );
    assert_eq!(
        BlockPos::new(-17, 64, -17).chunk_pos(),
        ChunkPos::new(-2, -2)
    );
}

#[test]
fn world_facade_reads_writes_and_rejects_unloaded_or_oob() {
    let mut state = SyntheticWorldState::new();
    let overworld = state.create_world("world", "minecraft:overworld", -64, 384);
    state.load_chunk(overworld, ChunkPos::new(0, 0)).unwrap();

    let caps = all_facade_caps();
    let mut view = state.view(&caps);

    let info = view.world_info(overworld).unwrap();
    assert_eq!(info.name, "world");
    assert_eq!(info.min_y, -64);
    assert_eq!(info.max_y_exclusive(), 320);
    assert_eq!(info.loaded_chunks, 1);

    let pos = BlockPos::new(3, 64, 5);
    assert_eq!(
        view.get_block(overworld, pos).unwrap().block_id,
        "minecraft:air"
    );

    view.execute(TickCommand::World(WorldCommand::SetBlock {
        world: overworld,
        pos,
        state: BlockStateSnapshot::simple("minecraft:stone"),
    }))
    .unwrap();
    assert_eq!(
        view.get_block(overworld, pos).unwrap().block_id,
        "minecraft:stone"
    );

    // Unloaded chunk fails closed.
    let unloaded_pos = BlockPos::new(32, 64, 0);
    assert_eq!(
        view.get_block(overworld, unloaded_pos),
        Err(FacadeError::ChunkUnloaded {
            world: overworld,
            chunk: ChunkPos::new(2, 0),
        })
    );

    // Out-of-bounds Y fails closed.
    let oob_pos = BlockPos::new(0, 320, 0);
    assert_eq!(
        view.get_block(overworld, oob_pos),
        Err(FacadeError::OutOfWorldBounds {
            world: overworld,
            pos: oob_pos,
            min_y: -64,
            max_y_exclusive: 320,
        })
    );

    // Unknown world fails closed.
    assert_eq!(
        view.world_info(WorldId::from_raw(999)),
        Err(FacadeError::WorldNotFound(WorldId::from_raw(999)))
    );
}

#[test]
fn entity_facade_enforces_dead_and_generational_stale_handles() {
    let mut state = SyntheticWorldState::new();
    let world = state.create_world("world", "minecraft:overworld", -64, 384);
    let pig = state
        .spawn_entity(world, "minecraft:pig", 1.0, 64.0, 2.0)
        .unwrap();

    let caps = all_facade_caps();
    {
        let mut view = state.view(&caps);
        view.execute(TickCommand::Entity(EntityCommand::SetCustomName {
            handle: pig,
            name: Some("Babe".to_owned()),
        }))
        .unwrap();
        assert_eq!(
            view.get_entity(pig).unwrap().custom_name.as_deref(),
            Some("Babe")
        );

        // Reject NaN teleport coordinates.
        assert!(matches!(
            view.execute(TickCommand::Entity(EntityCommand::Teleport {
                handle: pig,
                world,
                x: f64::NAN,
                y: 64.0,
                z: 0.0,
                yaw: 0.0,
                pitch: 0.0,
            })),
            Err(FacadeError::InvalidValue {
                field: "position",
                ..
            })
        ));

        // Discard marks entity dead; subsequent mutations fail with EntityDead.
        view.execute(TickCommand::Entity(EntityCommand::Discard { handle: pig }))
            .unwrap();
        assert!(!view.get_entity(pig).unwrap().alive);
        assert_eq!(
            view.execute(TickCommand::Entity(EntityCommand::SetCustomName {
                handle: pig,
                name: None,
            })),
            Err(FacadeError::EntityDead(pig))
        );
    }

    // Removing the entity and spawning a new one reuses the slot at a newer generation;
    // the old handle must fail with EntityStale, never aliasing the new entity.
    state.remove_entity(pig).unwrap();
    let cow = state
        .spawn_entity(world, "minecraft:cow", 5.0, 64.0, 5.0)
        .unwrap();
    assert_eq!(cow.index(), pig.index());
    assert_ne!(cow.generation(), pig.generation());

    let view = state.view(&caps);
    assert!(matches!(
        view.get_entity(pig),
        Err(FacadeError::EntityStale(HandleError::Stale { .. }))
    ));
    assert_eq!(view.get_entity(cow).unwrap().entity_type, "minecraft:cow");
}

#[test]
fn player_inventory_and_menu_lifecycle_and_disconnect_cleanup() {
    let mut state = SyntheticWorldState::new();
    let world = state.create_world("world", "minecraft:overworld", -64, 384);
    let player = state
        .connect_player(world, [7u8; 16], "Steve", GameMode::Survival)
        .unwrap();
    let chest = state.create_inventory(InventoryKind::Chest, 27).unwrap();

    let caps = all_facade_caps();
    let menu = {
        let mut view = state.view(&caps);
        let diamond = ItemStackSnapshot::new("minecraft:diamond", 12, 64).unwrap();
        view.execute(TickCommand::Inventory(InventoryCommand::SetSlot {
            inventory: chest,
            slot: 0,
            item: Some(diamond.clone()),
        }))
        .unwrap();
        assert_eq!(view.get_slot(chest, 0).unwrap(), Some(diamond));

        // Directly constructed invalid stack limits (> 99) are rejected by SetSlot.
        let invalid_stack = ItemStackSnapshot {
            item_id: "minecraft:diamond".to_owned(),
            count: 100,
            max_stack_size: 100,
            custom_name: None,
            components: std::collections::BTreeMap::new(),
        };
        assert_eq!(
            view.execute(TickCommand::Inventory(InventoryCommand::SetSlot {
                inventory: chest,
                slot: 0,
                item: Some(invalid_stack),
            })),
            Err(FacadeError::InvalidItemCount {
                count: 100,
                max_stack_size: 100,
            })
        );

        // Setting an air stack normalizes to empty (None).
        let air = ItemStackSnapshot::new("minecraft:air", 1, 64).unwrap();
        assert!(air.is_empty());
        view.execute(TickCommand::Inventory(InventoryCommand::SetSlot {
            inventory: chest,
            slot: 0,
            item: Some(air),
        }))
        .unwrap();
        assert_eq!(view.get_slot(chest, 0).unwrap(), None);

        // Open menu for player.
        let outcome = view
            .execute(TickCommand::Menu(MenuCommand::OpenMenu {
                player,
                inventory: chest,
                title: "Loot Chest".to_owned(),
            }))
            .unwrap();
        let CommandOutcome::OpenedMenu(menu) = outcome else {
            panic!("expected OpenedMenu outcome");
        };
        assert_eq!(view.get_player(player).unwrap().open_menu, Some(menu));
        assert!(view.get_menu(menu).unwrap().open);

        // Player mutations.
        view.execute(TickCommand::Player(PlayerCommand::SetGameMode {
            handle: player,
            mode: GameMode::Creative,
        }))
        .unwrap();
        view.execute(TickCommand::Player(PlayerCommand::SendMessage {
            handle: player,
            message: "Welcome".to_owned(),
        }))
        .unwrap();
        menu
    };

    assert_eq!(state.player_chat_log(player).unwrap(), &["Welcome"]);

    // Disconnecting player closes open menu and makes player offline.
    state.disconnect_player(player).unwrap();
    let mut view = state.view(&caps);
    assert!(!view.get_menu(menu).unwrap().open);
    assert_eq!(
        view.execute(TickCommand::Player(PlayerCommand::SetFoodLevel {
            handle: player,
            food_level: 10,
        })),
        Err(FacadeError::PlayerOffline(player))
    );
    assert_eq!(
        view.execute(TickCommand::Menu(MenuCommand::CloseMenu { menu })),
        Err(FacadeError::MenuClosed(menu))
    );
    // Purging closed menus reclaims their slots so old handles become MenuStale.
    assert_eq!(state.purge_closed_menus(), 1);
    {
        let view = state.view(&caps);
        assert!(matches!(
            view.get_menu(menu),
            Err(FacadeError::MenuStale(HandleError::Stale { .. }))
        ));
    }

    // Unloading an occupied world disconnects its players and closes their menus.
    let player2 = state
        .connect_player(world, [8u8; 16], "Alex", GameMode::Survival)
        .unwrap();
    let menu2 = {
        let mut view = state.view(&caps);
        let CommandOutcome::OpenedMenu(m) = view
            .execute(TickCommand::Menu(MenuCommand::OpenMenu {
                player: player2,
                inventory: chest,
                title: "Chest".to_owned(),
            }))
            .unwrap()
        else {
            panic!("expected OpenedMenu");
        };
        m
    };
    assert!(state.unload_world(world));
    let mut view = state.view(&caps);
    assert!(!view.get_player(player2).unwrap().online);
    assert!(!view.get_menu(menu2).unwrap().open);
    assert_eq!(
        view.execute(TickCommand::Player(PlayerCommand::SetFoodLevel {
            handle: player2,
            food_level: 5,
        })),
        Err(FacadeError::PlayerOffline(player2))
    );
}

#[test]
fn atomic_batch_rolls_back_all_prior_mutations_on_failure() {
    let mut state = SyntheticWorldState::new();
    let world = state.create_world("world", "minecraft:overworld", -64, 384);
    state.load_chunk(world, ChunkPos::new(0, 0)).unwrap();
    let player = state
        .connect_player(world, [1u8; 16], "Alex", GameMode::Survival)
        .unwrap();
    let chest = state.create_inventory(InventoryKind::Chest, 9).unwrap();

    let caps = all_facade_caps();
    let mut view = state.view(&caps);

    let batch = TickCommandBatch::new(BatchMode::Atomic)
        .with(TickCommand::World(WorldCommand::SetBlock {
            world,
            pos: BlockPos::new(1, 64, 1),
            state: BlockStateSnapshot::simple("minecraft:gold_block"),
        }))
        .with(TickCommand::Player(PlayerCommand::SetGameMode {
            handle: player,
            mode: GameMode::Adventure,
        }))
        .with(TickCommand::Inventory(InventoryCommand::SetSlot {
            inventory: chest,
            slot: 0,
            item: Some(ItemStackSnapshot::new("minecraft:emerald", 3, 64).unwrap()),
        }))
        .with(TickCommand::Menu(MenuCommand::OpenMenu {
            player,
            inventory: chest,
            title: "Temp".to_owned(),
        }))
        // Failing command: slot out of bounds -> must roll back all 4 prior commands!
        .with(TickCommand::Inventory(InventoryCommand::SetSlot {
            inventory: chest,
            slot: 99,
            item: None,
        }));

    let receipt = view.execute_batch(batch).unwrap();
    assert!(!receipt.committed);
    assert_eq!(receipt.failed_count(), 1);
    assert_eq!(
        view.get_block(world, BlockPos::new(1, 64, 1))
            .unwrap()
            .block_id,
        "minecraft:air"
    );
    assert_eq!(
        view.get_player(player).unwrap().game_mode,
        GameMode::Survival
    );
    assert_eq!(view.get_player(player).unwrap().open_menu, None);
    assert_eq!(view.get_slot(chest, 0).unwrap(), None);
}

#[test]
fn continue_on_error_batch_records_per_command_outcomes() {
    let mut state = SyntheticWorldState::new();
    let world = state.create_world("world", "minecraft:overworld", -64, 384);
    state.load_chunk(world, ChunkPos::new(0, 0)).unwrap();

    let caps = all_facade_caps();
    let mut view = state.view(&caps);

    let batch = TickCommandBatch::new(BatchMode::ContinueOnError)
        .with(TickCommand::World(WorldCommand::SetBlock {
            world,
            pos: BlockPos::new(0, 64, 0),
            state: BlockStateSnapshot::simple("minecraft:oak_planks"),
        }))
        .with(TickCommand::World(WorldCommand::SetBlock {
            world,
            pos: BlockPos::new(64, 64, 0), // unloaded chunk
            state: BlockStateSnapshot::simple("minecraft:oak_planks"),
        }))
        .with(TickCommand::World(WorldCommand::SetDayTime {
            world,
            day_time: 6000,
        }));

    let receipt = view.execute_batch(batch).unwrap();
    assert!(receipt.committed);
    assert_eq!(receipt.succeeded_count(), 2);
    assert_eq!(receipt.failed_count(), 1);
    assert_eq!(
        view.get_block(world, BlockPos::new(0, 64, 0))
            .unwrap()
            .block_id,
        "minecraft:oak_planks"
    );
    assert_eq!(view.world_info(world).unwrap().day_time, 6000);
}

#[test]
fn controlled_negatives_capability_off_thread_and_unverified_provider() {
    let mut state = SyntheticWorldState::new();
    let world = state.create_world("world", "minecraft:overworld", -64, 384);
    state.load_chunk(world, ChunkPos::new(0, 0)).unwrap();

    // 1. Missing capability fails closed.
    let read_only_world = CapabilitySet::from_iter([Capability::WorldRead]);
    {
        let mut view = state.view(&read_only_world);
        assert!(view.world_info(world).is_ok());
        assert_eq!(
            view.execute(TickCommand::World(WorldCommand::SetDayTime {
                world,
                day_time: 1000,
            })),
            Err(FacadeError::CapabilityDenied(Capability::WorldWrite))
        );

        // 2. Unverified M3 provider fails closed rather than silently skipping.
        assert!(matches!(
            view.require_provider("m3.world.level"),
            Err(FacadeError::ProviderUnverified {
                provider: "m3.world.level",
                ..
            })
        ));
    }

    // Once verified, require_provider succeeds.
    state.register_verified_provider("m3.world.level");
    {
        let view = state.view(&read_only_world);
        assert!(view.require_provider("m3.world.level").is_ok());
    }

    // 3. Off-thread access fails closed with NotOnTickThread.
    let err = thread::spawn(move || {
        let caps = all_facade_caps();
        let view = state.view(&caps);
        view.world_info(world)
    })
    .join()
    .unwrap();
    assert_eq!(err, Err(FacadeError::NotOnTickThread));
}
