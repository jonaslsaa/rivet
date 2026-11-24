use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};
use rivet_registry::{Identifier, core::Direction};
use rivet_text::Component;

use crate::crafting::{
    CommonInfo, CookingBookCategory, CookingBookInfo, CookingKind, CookingRecipe, Ingredient,
    ItemId as CraftingItemId, ItemStackTemplate, Recipe, RecipeHolder, RecipeManager, recipe_key,
};
use crate::effect::{PotionBrewing, PotionId};
use crate::inventory::{
    Container, ContainerAccessContext, ContainerData, InventoryItemStack, ItemId,
};

use super::*;

fn stack(name: &str, count: i32) -> InventoryItemStack {
    let id = ItemId::from_name(name).unwrap_or_else(|| panic!("unknown item {name}"));
    InventoryItemStack::new(id, count)
}

fn single_stack(name: &str) -> InventoryItemStack {
    let id = ItemId::from_name(name).unwrap_or_else(|| panic!("unknown item {name}"));
    InventoryItemStack::with_max_stack_size(id, 1, 1)
}

fn recipe_manager_with_cooking(
    id: &str,
    kind: CookingKind,
    ingredient_name: &str,
    result_name: &str,
    cooking_time: i32,
) -> RecipeManager {
    let ing_id =
        CraftingItemId::from_name(ingredient_name).unwrap_or_else(|| panic!("{ingredient_name}"));
    let res_id = CraftingItemId::from_name(result_name).unwrap_or_else(|| panic!("{result_name}"));
    let recipe = Recipe::Cooking(CookingRecipe {
        kind,
        common_info: CommonInfo {
            show_notification: true,
        },
        book_info: CookingBookInfo {
            category: CookingBookCategory::Misc,
            group: String::new(),
        },
        ingredient: Ingredient::of_item(ing_id).expect("valid ingredient"),
        result: ItemStackTemplate::new(res_id, 1),
        experience: 0.7,
        cooking_time,
    });
    let holder = RecipeHolder::new(recipe_key(Identifier::parse(id)), recipe);
    RecipeManager::from_recipes(vec![holder]).expect("valid recipe manager")
}

#[test]
fn chest_and_barrel_ordering_dirtying_lid_and_persistence() {
    let mut chest = ChestBlockEntity::new();
    assert_eq!(chest.get_container_size(), CHEST_CONTAINER_SIZE);
    assert!(chest.is_empty());
    assert_eq!(chest.base.changed_count(), 0);

    chest.set_item(0, stack("minecraft:iron_ingot", 10));
    chest.set_item(26, stack("minecraft:coal", 5));
    assert_eq!(chest.base.changed_count(), 2);

    let removed = chest.remove_item(0, 4);
    assert_eq!(removed.get_count(), 4);
    assert_eq!(chest.get_item(0).get_count(), 6);
    assert_eq!(chest.base.changed_count(), 3);

    // Viewer open count and lid openness progression.
    let ctx = ContainerAccessContext::survival(1);
    chest.start_open(&ctx);
    assert_eq!(chest.open_count(), 1);
    chest.tick_lid();
    assert!((chest.openness() - 0.1).abs() < 1e-6);
    chest.stop_open(&ctx);
    assert_eq!(chest.open_count(), 0);
    chest.tick_lid();
    assert!((chest.openness() - 0.0).abs() < 1e-6);

    // Double chest combines 27 + 27 = 54 slots in exact order.
    let mut right = ChestBlockEntity::new();
    right.set_item(0, stack("minecraft:gold_ingot", 3));
    let mut nbt = CompoundTag::new();
    chest.base.custom_name = Some(Component::literal("Storage"));
    let mut key_predicate = CompoundTag::new();
    let mut key_components = CompoundTag::new();
    key_components.put_string("minecraft:custom_name", "secret");
    key_predicate.put("components".into(), Tag::Compound(key_components));
    chest.base.lock_key = LockCode::new(key_predicate.clone());
    chest.save_additional(&mut nbt);
    // `CustomName` is a plain-text `Component` (encoded as a string tag) and
    // `lock` is the `ItemPredicate.CODEC` compound.
    assert_eq!(
        nbt.get_string("CustomName").map(String::as_str),
        Some("Storage")
    );
    assert_eq!(nbt.get_compound("lock"), Some(&key_predicate));

    let mut reloaded = ChestBlockEntity::new();
    reloaded.load_additional(&nbt);
    assert_eq!(reloaded.get_item(0), chest.get_item(0));
    assert_eq!(reloaded.get_item(26), chest.get_item(26));
    assert_eq!(
        reloaded.base.custom_name,
        Some(Component::literal("Storage"))
    );
    assert_eq!(reloaded.base.lock_key.predicate(), Some(&key_predicate));

    let double = ChestBlockEntity::double_chest(reloaded, right);
    assert_eq!(double.get_container_size(), 54);
    assert_eq!(double.get_item(0).get_count(), 6);
    assert_eq!(double.get_item(27).get_count(), 3);

    // Barrel open/close state and loot-table persistence.
    let mut barrel = BarrelBlockEntity::new();
    assert_eq!(barrel.get_container_size(), BARREL_CONTAINER_SIZE);
    barrel.start_open(&ctx);
    assert!(barrel.is_open());
    barrel.stop_open(&ctx);
    assert!(!barrel.is_open());
    barrel.base.loot_table = Some(Identifier::parse("minecraft:chests/simple_dungeon"));
    barrel.base.loot_table_seed = 42;
    let mut barrel_nbt = CompoundTag::new();
    barrel.save_additional(&mut barrel_nbt);
    // Paper's `RandomizableContainerBlockEntity` carries `lootableData`, so
    // `trySaveLootTable` returns false and `Items` is still written.
    assert_eq!(
        barrel_nbt.get_list("Items").map(|items| items.list.len()),
        Some(0)
    );

    let mut barrel_reloaded = BarrelBlockEntity::new();
    barrel_reloaded.load_additional(&barrel_nbt);
    assert_eq!(
        barrel_reloaded.base.loot_table,
        Some(Identifier::parse("minecraft:chests/simple_dungeon"))
    );
    assert_eq!(barrel_reloaded.base.loot_table_seed, 42);
}

#[test]
fn furnace_smelting_lifecycle_fuel_remainder_and_persistence() {
    let recipes = recipe_manager_with_cooking(
        "minecraft:iron_ingot_from_smelting_raw_iron",
        CookingKind::Smelting,
        "minecraft:raw_iron",
        "minecraft:iron_ingot",
        200,
    );
    let fuels = FuelValues::vanilla_burn_times();
    let mut furnace = new_furnace();

    furnace.set_item_with_recipes(SLOT_INPUT, stack("minecraft:raw_iron", 2), &recipes);
    furnace.set_item(SLOT_FUEL, single_stack("minecraft:lava_bucket"));

    // Tick 1 ignites lava bucket (20000 ticks), replaces fuel with empty bucket, and advances progress to 1.
    let t1 = furnace.server_tick(&recipes, &fuels);
    assert!(t1.lit_changed);
    assert!(t1.is_lit);
    assert!(!t1.burned_item);
    assert!(t1.dirty);
    assert_eq!(furnace.lit_total_time, 20000);
    assert_eq!(furnace.lit_time_remaining, 20000);
    assert_eq!(
        furnace.get_item(SLOT_FUEL).item().name(),
        Some("minecraft:bucket")
    );
    assert_eq!(furnace.get(DATA_COOKING_PROGRESS), 1);

    // Remaining 199 ticks finish first raw_iron -> 1 iron_ingot.
    for _ in 0..198 {
        let r = furnace.server_tick(&recipes, &fuels);
        assert!(!r.burned_item);
    }
    let t200 = furnace.server_tick(&recipes, &fuels);
    assert!(t200.burned_item);
    assert_eq!(furnace.get_item(SLOT_INPUT).get_count(), 1);
    assert_eq!(
        furnace.get_item(SLOT_RESULT).item().name(),
        Some("minecraft:iron_ingot")
    );
    assert_eq!(furnace.get_item(SLOT_RESULT).get_count(), 1);
    assert_eq!(
        furnace.recipes_used().get(&Identifier::parse(
            "minecraft:iron_ingot_from_smelting_raw_iron"
        )),
        Some(&1)
    );

    // Save/load preserves timers, CookSpeedMultiplier, RecipesUsed, and slots.
    furnace.cook_speed_multiplier = 2.0;
    let mut tag = CompoundTag::new();
    furnace.save_additional(&mut tag);
    let mut restored = new_furnace();
    restored.load_additional(&tag);
    assert_eq!(restored.lit_time_remaining, furnace.lit_time_remaining);
    assert_eq!(restored.lit_total_time, furnace.lit_total_time);
    assert_eq!(restored.cooking_total_time, furnace.cooking_total_time);
    assert!((restored.cook_speed_multiplier - 2.0).abs() < 1e-9);
    assert_eq!(restored.recipes_used(), furnace.recipes_used());
    assert_eq!(
        restored.get_item(SLOT_RESULT),
        furnace.get_item(SLOT_RESULT)
    );
}

#[test]
fn blast_furnace_smoker_half_burn_cooling_and_sided_negative_cases() {
    let fuels = FuelValues::vanilla_burn_times();
    let furnace = new_furnace();
    let blast = new_blast_furnace();
    let smoker = new_smoker();
    let coal = stack("minecraft:coal", 1);
    assert_eq!(furnace.get_burn_duration(&fuels, &coal), 1600);
    assert_eq!(blast.get_burn_duration(&fuels, &coal), 800);
    assert_eq!(smoker.get_burn_duration(&fuels, &coal), 800);

    // Controlled negative cases for furnace sided access:
    assert_eq!(furnace.get_slots_for_face(Direction::Up), &[SLOT_INPUT]);
    assert_eq!(
        furnace.get_slots_for_face(Direction::Down),
        &[SLOT_RESULT, SLOT_FUEL]
    );
    assert_eq!(furnace.get_slots_for_face(Direction::North), &[SLOT_FUEL]);
    // Result slot rejects placement; fuel slot rejects non-fuel.
    assert!(!furnace.can_place_item_through_face(SLOT_RESULT, &coal, Some(Direction::Up)));
    assert!(!furnace.can_place_item_through_face(
        SLOT_FUEL,
        &stack("minecraft:cobblestone", 1),
        Some(Direction::North)
    ));
    // Downward extraction from fuel slot rejects coal, allows empty/water bucket.
    assert!(!furnace.can_take_item_through_face(SLOT_FUEL, &coal, Direction::Down));
    assert!(furnace.can_take_item_through_face(
        SLOT_FUEL,
        &single_stack("minecraft:bucket"),
        Direction::Down
    ));
    assert!(furnace.can_take_item_through_face(
        SLOT_FUEL,
        &single_stack("minecraft:water_bucket"),
        Direction::Down
    ));

    // Cooling when fuel expires mid-cook decreases by BURN_COOL_SPEED (2) clamped to 0.
    let recipes = recipe_manager_with_cooking(
        "minecraft:stone_from_smelting_cobblestone",
        CookingKind::Smelting,
        "minecraft:cobblestone",
        "minecraft:stone",
        200,
    );
    let mut cooling = new_furnace();
    cooling.set_item_with_recipes(SLOT_INPUT, stack("minecraft:cobblestone", 1), &recipes);
    cooling.lit_time_remaining = 5;
    cooling.lit_total_time = 5;
    for _ in 0..4 {
        cooling.server_tick(&recipes, &fuels);
    }
    assert_eq!(cooling.cooking_timer, 4);
    // Tick 5: lit drops from 1 -> 0 (not lit), no fuel -> cools by 2? Actually at tick 5
    // is_lit becomes false, no fuel, so else-if branch cools cooking_timer from 4 -> 2!
    let t5 = cooling.server_tick(&recipes, &fuels);
    assert!(t5.lit_changed);
    assert!(!t5.is_lit);
    assert_eq!(cooling.cooking_timer, 2);
    cooling.server_tick(&recipes, &fuels);
    assert_eq!(cooling.cooking_timer, 0);
}

#[test]
fn brewing_stand_lifecycle_interrupt_sided_and_persistence() {
    let brewing = PotionBrewing::vanilla();
    let mut stand = BrewingStandBlockEntity::new();

    // Load 3 water bottles, nether_wart, and blaze_powder.
    for slot in 0..3 {
        stand.set_item(
            slot,
            BrewingStandBlockEntity::make_potion_stack("minecraft:potion", PotionId::Water),
        );
    }
    stand.set_item(INGREDIENT_SLOT, stack("minecraft:nether_wart", 2));
    stand.set_item(FUEL_SLOT, stack("minecraft:blaze_powder", 1));

    // Tick 1: loads 20 fuel uses, consumes 1 -> 19 left, sets brew_time = 400, detects bottle bits.
    let t1 = stand.server_tick(&brewing);
    assert!(t1.bottle_bits_changed);
    assert_eq!(t1.bottle_bits, [true, true, true]);
    assert_eq!(stand.fuel, FUEL_USES - 1);
    assert_eq!(stand.brew_time, DEFAULT_BREW_TIME);
    assert!(stand.get_item(FUEL_SLOT).is_empty());

    // 399 more ticks count down to 1; tick 401 completes brew.
    for _ in 0..399 {
        let r = stand.server_tick(&brewing);
        assert!(!r.brewed);
    }
    let done = stand.server_tick(&brewing);
    assert!(done.brewed);
    assert_eq!(stand.brew_time, 0);
    assert_eq!(stand.get_item(INGREDIENT_SLOT).get_count(), 1);
    for slot in 0..3 {
        assert_eq!(
            BrewingStandBlockEntity::potion_of_stack(stand.get_item(slot)),
            Some(PotionId::Awkward)
        );
    }

    // Controlled negative case: swapping ingredient mid-brew resets timer to 0.
    stand.set_item(INGREDIENT_SLOT, stack("minecraft:redstone", 1));
    // Awkward + redstone is not brewable; use sugar -> swiftness instead, then cancel.
    stand.set_item(INGREDIENT_SLOT, stack("minecraft:sugar", 1));
    stand.server_tick(&brewing);
    assert_eq!(stand.brew_time, DEFAULT_BREW_TIME);
    stand.set_item(INGREDIENT_SLOT, stack("minecraft:glowstone_dust", 1));
    stand.server_tick(&brewing);
    assert_eq!(stand.brew_time, 0);

    // Sided rules: downward extraction blocks nether_wart from ingredient slot, allows glass_bottle.
    assert_eq!(stand.get_slots_for_face(Direction::Up), &[INGREDIENT_SLOT]);
    assert_eq!(
        stand.get_slots_for_face(Direction::Down),
        &[0, 1, 2, INGREDIENT_SLOT]
    );
    assert_eq!(
        stand.get_slots_for_face(Direction::East),
        &[0, 1, 2, FUEL_SLOT]
    );
    assert!(!stand.can_take_item_through_face(
        INGREDIENT_SLOT,
        &stack("minecraft:nether_wart", 1),
        Direction::Down
    ));
    assert!(stand.can_take_item_through_face(
        INGREDIENT_SLOT,
        &stack("minecraft:glass_bottle", 1),
        Direction::Down
    ));

    // Persistence round-trip.
    let mut tag = CompoundTag::new();
    stand.save_additional(&mut tag);
    let mut restored = BrewingStandBlockEntity::new();
    restored.load_additional(&tag);
    assert_eq!(restored.fuel, stand.fuel);
    assert_eq!(restored.brew_time, stand.brew_time);
    assert_eq!(restored.get_item(0), stand.get_item(0));
}

#[test]
fn hopper_push_before_pull_sided_furnace_conservation_and_cooldown() {
    let mut source_chest = ChestBlockEntity::new();
    source_chest.set_item(0, stack("minecraft:raw_iron", 2));
    source_chest.set_item(1, stack("minecraft:coal", 3));

    let mut hopper = HopperBlockEntity::new(Direction::Down);
    hopper.set_item(1, stack("minecraft:raw_iron", 1)); // slot 0 empty, slot 1 has item

    let mut furnace = new_furnace();

    // Tick 1: hopper pushes its own slot 1 into furnace top (slot 0) AND pulls source_chest slot 0 in the same tick!
    let moved = {
        let mut src = HopperTarget::Plain(&mut source_chest);
        let mut dst = HopperTarget::Worldly(&mut furnace);
        hopper.push_items_tick(1, Some(&mut src), Some(&mut dst))
    };
    assert!(moved);
    assert_eq!(hopper.cooldown_time, MOVE_ITEM_SPEED);
    assert_eq!(furnace.get_item(SLOT_INPUT).get_count(), 1);
    assert_eq!(hopper.get_item(0).get_count(), 1);
    assert_eq!(source_chest.get_item(0).get_count(), 1);

    // Total raw_iron strictly conserved: 1 (furnace) + 1 (hopper) + 1 (source) == 3.
    let total = furnace.get_item(SLOT_INPUT).get_count()
        + hopper.get_item(0).get_count()
        + source_chest.get_item(0).get_count();
    assert_eq!(total, 3);

    // While on cooldown (ticks 2..8), hopper does not move items.
    for t in 2..=8 {
        let mut src = HopperTarget::Plain(&mut source_chest);
        let mut dst = HopperTarget::Worldly(&mut furnace);
        assert!(!hopper.push_items_tick(t, Some(&mut src), Some(&mut dst)));
    }

    // Controlled negative case: disabled (redstone-powered) hopper does not transfer even at cooldown 0.
    hopper.enabled = false;
    {
        let mut src = HopperTarget::Plain(&mut source_chest);
        let mut dst = HopperTarget::Worldly(&mut furnace);
        assert!(!hopper.push_items_tick(9, Some(&mut src), Some(&mut dst)));
    }
    assert_eq!(furnace.get_item(SLOT_INPUT).get_count(), 1);

    // Bottom hopper pulling from furnace skips coal in slot 1 (sided rule) and takes result from slot 2 first.
    furnace.set_item(SLOT_FUEL, stack("minecraft:coal", 4));
    furnace.set_item(SLOT_RESULT, stack("minecraft:iron_ingot", 1));
    let mut bottom_hopper = HopperBlockEntity::new(Direction::Down);
    {
        let mut src = HopperTarget::Worldly(&mut furnace);
        assert!(bottom_hopper.suck_in_items(&mut src, 1));
    }
    assert_eq!(
        bottom_hopper.get_item(0).item().name(),
        Some("minecraft:iron_ingot")
    );
    assert!(furnace.get_item(SLOT_RESULT).is_empty());
    // Next pull cannot extract coal from fuel slot 1.
    {
        let mut src = HopperTarget::Worldly(&mut furnace);
        assert!(!bottom_hopper.suck_in_items(&mut src, 1));
    }
    assert_eq!(furnace.get_item(SLOT_FUEL).get_count(), 4);

    // Save/load round-trip.
    let mut tag = CompoundTag::new();
    hopper.save_additional(&mut tag);
    let mut restored = HopperBlockEntity::new(Direction::Down);
    restored.load_additional(&tag);
    assert_eq!(restored.cooldown_time, hopper.cooldown_time);
    assert_eq!(restored.get_item(0), hopper.get_item(0));
}

#[test]
fn crafter_round_robin_disabled_slots_redstone_and_persistence() {
    let mut crafter = CrafterBlockEntity::new();
    assert_eq!(crafter.get_container_size(), CRAFTER_CONTAINER_SIZE);
    assert_eq!(crafter.get_redstone_signal(), 0);

    // Disable slots 1..9 except slot 0 and slot 4.
    for slot in [1, 2, 3, 5, 6, 7, 8] {
        assert!(crafter.set_slot_state(slot, false));
    }
    assert_eq!(crafter.get_redstone_signal(), 7);

    // Hopper insertion distributes evenly between enabled slots 0 and 4.
    let mut hopper = HopperBlockEntity::new(Direction::Down);
    hopper.set_item(0, stack("minecraft:oak_planks", 3));
    for _ in 0..3 {
        let mut dst = HopperTarget::Plain(&mut crafter);
        assert!(hopper.eject_items(&mut dst, 1));
    }
    // Slot 0 gets 1st, slot 4 gets 2nd (smallerStackExist skips slot 0 because slot 4 is empty),
    // slot 0 gets 3rd (slot 4 has same count 1, not smaller).
    assert_eq!(crafter.get_item(0).get_count(), 2);
    assert_eq!(crafter.get_item(4).get_count(), 1);
    assert_eq!(crafter.get_redstone_signal(), 9);

    // Controlled negative case: occupied slot cannot be disabled.
    assert!(!crafter.set_slot_state(0, false));
    assert!(!crafter.is_slot_disabled(0));

    // Crafting ticks countdown fires completion on reaching 0.
    crafter.set_crafting_ticks_remaining(2);
    crafter.set_triggered(true);
    assert!(!crafter.server_tick());
    assert_eq!(crafter.crafting_ticks_remaining, 1);
    assert!(crafter.server_tick());
    assert_eq!(crafter.crafting_ticks_remaining, 0);

    // Save/load round-trip.
    crafter.set_crafting_ticks_remaining(4);
    let mut tag = CompoundTag::new();
    crafter.save_additional(&mut tag);
    let mut restored = CrafterBlockEntity::new();
    restored.load_additional(&tag);
    assert_eq!(restored.crafting_ticks_remaining, 4);
    assert!(restored.is_triggered());
    assert!(restored.is_slot_disabled(1));
    assert!(!restored.is_slot_disabled(0));
    assert!(!restored.is_slot_disabled(4));
    assert_eq!(restored.get_item(0).get_count(), 2);
    assert_eq!(restored.get_item(4).get_count(), 1);
}

#[test]
fn container_nbt_preserves_typed_item_components() {
    use rivet_nbt::{int_tag::IntTag, list_tag::ListTag, tag::Tag};

    let mut comp = CompoundTag::new();
    comp.put_int("minecraft:damage", 12);
    let mut ench = CompoundTag::new();
    ench.put_int("minecraft:sharpness", 5);
    comp.put("minecraft:enchantments".into(), Tag::Compound(ench));
    let mut lore = ListTag::new();
    lore.list.push(Tag::Int(IntTag::value_of(7)));
    comp.put("minecraft:custom_list".into(), Tag::List(lore));
    comp.put_string("minecraft:custom_name", "Excalibur");

    let mut entry = CompoundTag::new();
    entry.put_byte("Slot", 3);
    entry.put_string("id", "minecraft:diamond_sword");
    entry.put_int("count", 1);
    entry.put("components".into(), Tag::Compound(comp));

    let mut list = rivet_nbt::list_tag::ListTag::new();
    list.list.push(Tag::Compound(entry));
    let mut input = CompoundTag::new();
    input.put("Items".into(), Tag::List(list));

    let mut chest = ChestBlockEntity::new();
    chest.load_additional(&input);
    let loaded = chest.get_item(3);
    assert_eq!(loaded.item().name(), Some("minecraft:diamond_sword"));
    assert_eq!(loaded.components().len(), 4);

    let mut saved = CompoundTag::new();
    chest.save_additional(&mut saved);
    let saved_items = saved.get_list("Items").expect("Items saved");
    let Tag::Compound(saved_entry) = &saved_items.list[0] else {
        panic!("expected compound entry");
    };
    let saved_comp = saved_entry.get_compound("components").expect("components");
    assert_eq!(saved_comp.get_int("minecraft:damage"), Some(12));
    assert_eq!(
        saved_comp
            .get_compound("minecraft:enchantments")
            .and_then(|e| e.get_int("minecraft:sharpness")),
        Some(5)
    );
    assert_eq!(
        saved_comp
            .get_list("minecraft:custom_list")
            .map(|l| l.list.len()),
        Some(1)
    );
    assert_eq!(
        saved_comp
            .get_string("minecraft:custom_name")
            .map(String::as_str),
        Some("Excalibur")
    );
}

#[test]
fn loot_table_container_keeps_loot_table_until_unpacked_with_level_context() {
    use crate::loot::{
        LootItemFunction, LootPool, LootPoolEntryContainer, LootRandomState, LootRegistry,
        LootTable, NumberProvider,
    };

    let mut input = CompoundTag::new();
    input.put_string("LootTable", "minecraft:chests/simple_dungeon");
    input.put_long("LootTableSeed", 42);

    let mut chest = ChestBlockEntity::new();
    chest.load_additional(&input);
    let loot_id = Identifier::parse("minecraft:chests/simple_dungeon");
    assert_eq!(chest.base.loot_table.as_ref(), Some(&loot_id));

    // Level-less `setItem`: `unpackLootTable` has no server, so the pending
    // loot table and seed survive the mutation.
    chest.set_item(0, stack("minecraft:emerald", 1));
    assert_eq!(chest.base.loot_table.as_ref(), Some(&loot_id));
    assert_eq!(chest.base.loot_table_seed, 42);

    // Paper writes both the loot table and `Items` (lootableData is non-null).
    let mut saved = CompoundTag::new();
    chest.save_additional(&mut saved);
    assert_eq!(
        saved.get_string("LootTable").map(String::as_str),
        Some("minecraft:chests/simple_dungeon")
    );
    assert_eq!(saved.get_long("LootTableSeed"), Some(42));
    assert_eq!(
        saved.get_list("Items").map(|items| items.list.len()),
        Some(1)
    );
    assert!(saved.get("Paper.LootableData").is_none());

    let mut restored = ChestBlockEntity::new();
    restored.load_additional(&saved);
    assert_eq!(restored.base.loot_table.as_ref(), Some(&loot_id));
    assert_eq!(
        restored.get_item(0).item().name(),
        Some("minecraft:emerald")
    );

    // Level-bound unpack fills the free slots from the table, clears the loot
    // table first (default `auto-replenish: false`), and records `lastFill`.
    let mut registry = LootRegistry::new();
    registry.register_table(
        loot_id.clone(),
        LootTable::builder()
            .with_pool(
                LootPool::builder()
                    .set_rolls(NumberProvider::constant(1.0))
                    .add(
                        LootPoolEntryContainer::item(Identifier::parse("minecraft:diamond"))
                            .apply(LootItemFunction::set_count(NumberProvider::constant(3.0))),
                    )
                    .build(),
            )
            .build(),
    );
    let mut random_state = LootRandomState::new();
    let mut ctx = LootUnpackContext {
        loot: &registry,
        random_state: &mut random_state,
        origin: [0.5, 64.5, 0.5],
        now_millis: 1_700_000_000_000,
    };
    restored.unpack_loot_table(&mut ctx);
    assert!(restored.base.loot_table.is_none());
    assert_eq!(restored.base.loot_table_seed, 42);
    assert_eq!(restored.base.lootable_data.last_fill, 1_700_000_000_000);
    assert_eq!(
        restored.get_item(0).item().name(),
        Some("minecraft:emerald")
    );
    let diamonds: i32 = (1..CHEST_CONTAINER_SIZE)
        .map(|slot| restored.get_item(slot))
        .filter(|item| item.item().name() == Some("minecraft:diamond"))
        .map(InventoryItemStack::get_count)
        .sum();
    assert_eq!(diamonds, 3);

    // A second unpack is a no-op once the loot table is cleared.
    let before = restored.clone();
    restored.unpack_loot_table(&mut ctx);
    assert_eq!(restored, before);
}

#[test]
fn paper_item_nbt_uses_prototype_max_stack_size_and_potion_contents_compound() {
    // Paper item NBT carries no max-stack field: it comes from the prototype.
    let mut input = CompoundTag::new();
    let mut items = rivet_nbt::list_tag::ListTag::new();
    let mut sword = CompoundTag::new();
    sword.put_byte("Slot", 0);
    sword.put_string("id", "minecraft:diamond_sword");
    sword.put_int("count", 1);
    items.list.push(Tag::Compound(sword));
    let mut pearls = CompoundTag::new();
    pearls.put_byte("Slot", 1);
    pearls.put_string("id", "minecraft:ender_pearl");
    pearls.put_int("count", 2);
    items.list.push(Tag::Compound(pearls));
    input.put("Items".into(), Tag::List(items));

    let mut chest = ChestBlockEntity::new();
    chest.load_additional(&input);
    assert_eq!(chest.get_item(0).get_max_stack_size(), 1);
    assert_eq!(chest.get_item(1).get_max_stack_size(), 16);
    assert!(!HopperBlockEntity::can_merge_items(
        chest.get_item(0),
        &single_stack("minecraft:diamond_sword")
    ));
    let mut saved = CompoundTag::new();
    chest.save_additional(&mut saved);
    let Some(Tag::Compound(entry)) = saved.get_list("Items").and_then(|l| l.list.first()) else {
        panic!("saved sword entry");
    };
    assert!(entry.get("max_stack_size").is_none());

    // Paper `PotionContents.FULL_CODEC` compound decodes to its potion.
    let mut contents = CompoundTag::new();
    contents.put_string("potion", "minecraft:awkward");
    let mut components = CompoundTag::new();
    components.put("minecraft:potion_contents".into(), Tag::Compound(contents));
    let mut potion = CompoundTag::new();
    potion.put_byte("Slot", 0);
    potion.put_string("id", "minecraft:potion");
    potion.put_int("count", 1);
    potion.put("components".into(), Tag::Compound(components.clone()));
    let mut brewing_items = rivet_nbt::list_tag::ListTag::new();
    brewing_items.list.push(Tag::Compound(potion));
    let mut brewing_input = CompoundTag::new();
    brewing_input.put("Items".into(), Tag::List(brewing_items));

    let mut stand = BrewingStandBlockEntity::new();
    stand.load_additional(&brewing_input);
    assert_eq!(
        BrewingStandBlockEntity::potion_of_stack(stand.get_item(0)),
        Some(PotionId::Awkward)
    );
    let mut brewing_saved = CompoundTag::new();
    stand.save_additional(&mut brewing_saved);
    let Some(Tag::Compound(saved_potion)) =
        brewing_saved.get_list("Items").and_then(|l| l.list.first())
    else {
        panic!("saved potion entry");
    };
    assert_eq!(saved_potion.get_compound("components"), Some(&components));
    // Freshly mixed potions use the same compound encoding.
    let made = BrewingStandBlockEntity::make_potion_stack("minecraft:potion", PotionId::Awkward);
    assert_eq!(made.get_max_stack_size(), 1);
    assert_eq!(
        BrewingStandBlockEntity::potion_of_stack(&made),
        Some(PotionId::Awkward)
    );
}

#[test]
fn furnace_preserves_components_and_max_stack_size_in_recipes() {
    use std::collections::BTreeMap;

    use crate::inventory::ComponentMap;

    // Recipe with exact-component input and unstackable + component-bearing result.
    let mut in_comps = BTreeMap::new();
    in_comps.insert(
        Identifier::parse("minecraft:custom_name"),
        "Blessed Ore".to_string(),
    );
    let exact_input = crate::crafting::ItemStack::with_components(
        CraftingItemId::from_name("minecraft:raw_iron").expect("raw_iron"),
        1,
        in_comps,
    );

    let mut out_comps = BTreeMap::new();
    out_comps.insert(
        Identifier::parse("minecraft:custom_name"),
        "Relic Sword".to_string(),
    );
    let result_template = ItemStackTemplate::with_components(
        CraftingItemId::from_name("minecraft:iron_sword").expect("iron_sword"),
        1,
        out_comps,
    );

    let recipe = CookingRecipe {
        kind: CookingKind::Smelting,
        common_info: CommonInfo {
            show_notification: true,
        },
        book_info: CookingBookInfo {
            category: CookingBookCategory::Misc,
            group: String::new(),
        },
        ingredient: Ingredient::of_exact_stacks(vec![exact_input]).expect("exact ingredient"),
        result: result_template,
        experience: 1.0,
        cooking_time: 10,
    };
    let manager = RecipeManager::from_recipes(vec![RecipeHolder::new(
        recipe_key(Identifier::parse("minecraft:exact_sword_smelt")),
        Recipe::Cooking(recipe),
    )])
    .expect("valid recipe manager");
    let fuel = FuelValues::vanilla_burn_times();

    let mut furnace = new_furnace();
    furnace.set_item(SLOT_FUEL, stack("minecraft:coal", 1));

    // Plain input without the required component must NOT match.
    furnace.set_item_with_recipes(SLOT_INPUT, stack("minecraft:raw_iron", 1), &manager);
    let tick = furnace.server_tick(&manager, &fuel);
    assert!(!tick.is_lit);

    // Matching component-bearing input DOES cook and preserves result components and max_stack_size=1.
    let mut inv_comps = ComponentMap::new();
    inv_comps.insert(
        "minecraft:custom_name".to_string(),
        "Blessed Ore".to_string(),
    );
    let tagged_input = stack("minecraft:raw_iron", 2).with_components(inv_comps);
    furnace.set_item_with_recipes(SLOT_INPUT, tagged_input, &manager);
    for _ in 0..10 {
        furnace.server_tick(&manager, &fuel);
    }
    let output = furnace.get_item(SLOT_RESULT);
    assert_eq!(output.item().name(), Some("minecraft:iron_sword"));
    assert_eq!(output.get_count(), 1);
    assert_eq!(output.get_max_stack_size(), 1);
    assert_eq!(
        output
            .components()
            .get("minecraft:custom_name")
            .map(String::as_str),
        Some("Relic Sword")
    );

    // Second item must NOT overstack into the unstackable (max_stack_size=1) sword result slot.
    for _ in 0..15 {
        furnace.server_tick(&manager, &fuel);
    }
    assert_eq!(furnace.get_item(SLOT_RESULT).get_count(), 1);
    assert_eq!(furnace.get_item(SLOT_INPUT).get_count(), 1);
}

#[test]
fn furnace_hopper_inserted_input_recomputes_cook_time_and_multiplier() {
    let recipes = recipe_manager_with_cooking(
        "minecraft:custom_fast_smelt",
        CookingKind::Smelting,
        "minecraft:raw_iron",
        "minecraft:iron_ingot",
        40,
    );
    let fuel = FuelValues::vanilla_burn_times();

    let mut furnace = new_furnace();
    furnace.cook_speed_multiplier = 2.0; // 40 / 2.0 = 20 ticks
    furnace.set_item(SLOT_FUEL, stack("minecraft:coal", 1));

    // Insert input via hopper into a level-bound furnace (`setItem` recomputes).
    let mut hopper = HopperBlockEntity::new(Direction::Down);
    hopper.set_item(0, stack("minecraft:raw_iron", 1));
    assert!(hopper.eject_items(&mut HopperTarget::Furnace(&mut furnace, &recipes), 1));
    assert_eq!(furnace.get_item(SLOT_INPUT).get_count(), 1);
    assert_eq!(furnace.cooking_total_time, 20);
    assert_eq!(furnace.cooking_timer, 0);

    for tick_idx in 1..=20 {
        let res = furnace.server_tick(&recipes, &fuel);
        if tick_idx < 20 {
            assert!(!res.burned_item, "should not finish before tick 20");
        } else {
            assert!(res.burned_item, "should finish at tick 20");
        }
    }
    assert_eq!(furnace.cooking_total_time, 20);
    assert_eq!(
        furnace.get_item(SLOT_RESULT).item().name(),
        Some("minecraft:iron_ingot")
    );
    assert_eq!(furnace.get_item(SLOT_RESULT).get_count(), 1);
}

#[test]
fn fuel_values_expands_vanilla_tags_and_removes_non_flammable_wood() {
    let fuel = FuelValues::vanilla_burn_times();

    assert_eq!(
        fuel.burn_duration(&stack("minecraft:stripped_oak_log", 1)),
        300
    );
    assert_eq!(fuel.burn_duration(&stack("minecraft:red_wool", 1)), 100);
    assert_eq!(fuel.burn_duration(&stack("minecraft:red_carpet", 1)), 67);
    assert_eq!(fuel.burn_duration(&stack("minecraft:spruce_door", 1)), 200);
    assert_eq!(
        fuel.burn_duration(&stack("minecraft:oak_hanging_sign", 1)),
        800
    );
    assert_eq!(fuel.burn_duration(&stack("minecraft:white_banner", 1)), 300);
    assert_eq!(fuel.burn_duration(&stack("minecraft:dead_bush", 1)), 100);

    // NON_FLAMMABLE_WOOD tag entries must be excluded.
    assert!(!fuel.is_fuel(&stack("minecraft:crimson_planks", 1)));
    assert!(!fuel.is_fuel(&stack("minecraft:warped_stem", 1)));
    assert!(!fuel.is_fuel(&stack("minecraft:crimson_door", 1)));
}

#[test]
fn hopper_chain_sets_receiving_empty_hopper_cooldown() {
    // Case 1: receiving hopper has not ticked yet this tick (dest.ticked_game_time < from.ticked_game_time)
    // -> cooldown set to MOVE_ITEM_SPEED (8).
    let mut upstream = HopperBlockEntity::new(Direction::East);
    upstream.ticked_game_time = 100;
    upstream.set_item(0, stack("minecraft:stone", 2));

    let mut downstream = HopperBlockEntity::new(Direction::East);
    downstream.ticked_game_time = 99;
    downstream.cooldown_time = 0;

    assert!(upstream.eject_items(&mut HopperTarget::Hopper(&mut downstream), 1));
    assert_eq!(downstream.get_item(0).get_count(), 1);
    assert_eq!(downstream.cooldown_time, MOVE_ITEM_SPEED);

    // Downstream must not immediately forward the item on the same tick.
    let mut sink = ChestBlockEntity::new();
    assert!(!downstream.push_items_tick(100, Some(&mut HopperTarget::Plain(&mut sink)), None));
    assert!(sink.is_empty());

    // Case 2: receiving hopper already ticked on the same game tick
    // (dest.ticked_game_time >= from.ticked_game_time) -> skip_tick_count = 1 -> cooldown = 7.
    let mut downstream_same_tick = HopperBlockEntity::new(Direction::East);
    downstream_same_tick.ticked_game_time = 100;
    downstream_same_tick.cooldown_time = 0;

    assert!(upstream.eject_items(&mut HopperTarget::Hopper(&mut downstream_same_tick), 1));
    assert_eq!(downstream_same_tick.cooldown_time, MOVE_ITEM_SPEED - 1);

    // Case 3: custom cooldown (> 8) is preserved.
    let mut upstream2 = HopperBlockEntity::new(Direction::East);
    upstream2.ticked_game_time = 100;
    upstream2.set_item(0, stack("minecraft:stone", 1));

    let mut custom_cd_hopper = HopperBlockEntity::new(Direction::East);
    custom_cd_hopper.ticked_game_time = 99;
    custom_cd_hopper.set_cooldown(20);
    assert!(upstream2.eject_items(&mut HopperTarget::Hopper(&mut custom_cd_hopper), 1));
    assert_eq!(custom_cd_hopper.cooldown_time, 20);
}

#[test]
fn base_container_nbt_reads_paper_lock_predicate_and_styled_custom_name() {
    // A string `lock` is not an `ItemPredicate.CODEC` compound: NO_LOCK.
    let mut legacy = CompoundTag::new();
    legacy.put_string("lock", "Key");
    let mut chest = ChestBlockEntity::new();
    chest.load_additional(&legacy);
    assert!(chest.base.lock_key.is_unlocked());
    let mut saved = CompoundTag::new();
    chest.save_additional(&mut saved);
    assert!(saved.get("lock").is_none());

    // Styled `CustomName` compounds decode through `ComponentSerialization.CODEC`.
    let mut name = CompoundTag::new();
    name.put_string("text", "Loot");
    name.put_string("color", "gold");
    let mut input = CompoundTag::new();
    input.put("CustomName".into(), Tag::Compound(name));
    let mut barrel = BarrelBlockEntity::new();
    barrel.load_additional(&input);
    let custom_name = barrel.base.custom_name.clone().expect("styled custom name");
    assert_eq!(custom_name.get_string(), "Loot");
    let mut barrel_saved = CompoundTag::new();
    barrel.save_additional(&mut barrel_saved);
    let saved_name = barrel_saved
        .get_compound("CustomName")
        .expect("styled name stays a compound");
    assert_eq!(
        saved_name.get_string("color").map(String::as_str),
        Some("gold")
    );
}

#[test]
fn brewing_stand_resumes_loaded_brew_and_last_dragon_breath_leaves_no_remainder() {
    let brewing = PotionBrewing::vanilla();
    let mut stand = BrewingStandBlockEntity::new();
    stand.set_item(
        0,
        BrewingStandBlockEntity::make_potion_stack("minecraft:splash_potion", PotionId::Awkward),
    );
    stand.set_item(INGREDIENT_SLOT, stack("minecraft:dragon_breath", 1));
    stand.brew_time = 2;
    stand.fuel = 5;
    let mut tag = CompoundTag::new();
    stand.save_additional(&mut tag);

    let mut restored = BrewingStandBlockEntity::new();
    restored.load_additional(&tag);
    let changed_before = restored.base.changed_count();

    // `ingredient` is restored from slot 3 while brewing, so the brew continues;
    // `lastPotionCount` starts null, so the first tick reports the bottle bits;
    // every brewing tick marks the block entity changed.
    let t1 = restored.server_tick(&brewing);
    assert!(!t1.brewed);
    assert_eq!(restored.brew_time, 1);
    assert!(t1.bottle_bits_changed);
    assert_eq!(t1.bottle_bits, [true, false, false]);
    assert_eq!(restored.base.changed_count(), changed_before + 1);

    let t2 = restored.server_tick(&brewing);
    assert!(t2.brewed);
    assert!(!t2.bottle_bits_changed);
    assert_eq!(restored.base.changed_count(), changed_before + 2);
    assert_eq!(
        restored.get_item(0).item().name(),
        Some("minecraft:lingering_potion")
    );
    // The emptied ingredient stack reports `Items.AIR`, which has no remainder.
    assert!(restored.get_item(INGREDIENT_SLOT).is_empty());
    assert!(t2.dropped_remainders.is_empty());
}

#[test]
fn furnace_paper_cook_completion_timer_and_persistence_edges() {
    let recipes = recipe_manager_with_cooking(
        "minecraft:iron_ingot_from_smelting_raw_iron",
        CookingKind::Smelting,
        "minecraft:raw_iron",
        "minecraft:iron_ingot",
        200,
    );
    let fuels = FuelValues::vanilla_burn_times();

    // A timer already past the total finishes on the next lit tick (`>=`).
    let mut furnace = new_furnace();
    furnace.set_item(SLOT_INPUT, stack("minecraft:raw_iron", 1));
    furnace.set_item(SLOT_FUEL, stack("minecraft:coal", 1));
    furnace.cooking_timer = 150;
    furnace.cooking_total_time = 100;
    let done = furnace.server_tick(&recipes, &fuels);
    assert!(done.burned_item);
    assert_eq!(furnace.cooking_timer, 0);
    assert_eq!(furnace.cooking_total_time, 200);

    // `cookingTimer == 0` recomputes the total from the recipe.
    let mut fresh = new_furnace();
    fresh.set_item(SLOT_INPUT, stack("minecraft:raw_iron", 2));
    fresh.set_item(SLOT_FUEL, stack("minecraft:coal", 1));
    fresh.set(DATA_COOKING_TOTAL_TIME, 5);
    fresh.server_tick(&recipes, &fuels);
    assert_eq!(fresh.cooking_total_time, 200);
    assert_eq!(fresh.cooking_timer, 1);

    // `recipe == null` with an ingredient leaves the timer untouched; no
    // ingredient while lit resets it.
    let mut lit = new_furnace();
    lit.lit_time_remaining = 10;
    lit.set_item(SLOT_INPUT, stack("minecraft:stone", 1));
    lit.cooking_timer = 7;
    lit.server_tick(&recipes, &fuels);
    assert_eq!(lit.cooking_timer, 7);
    lit.set_item(SLOT_INPUT, InventoryItemStack::empty());
    lit.server_tick(&recipes, &fuels);
    assert_eq!(lit.cooking_timer, 0);

    // Level-bound `setItem` applies `getTotalCookTime` immediately.
    let mut bound = new_furnace();
    bound.set_item_with_recipes(SLOT_INPUT, stack("minecraft:raw_iron", 1), &recipes);
    assert_eq!(bound.get(DATA_COOKING_TOTAL_TIME), 200);
    bound.set_item_with_recipes(SLOT_INPUT, stack("minecraft:stone", 1), &recipes);
    assert_eq!(bound.get(DATA_COOKING_TOTAL_TIME), BURN_TIME_STANDARD);
    // Level-less `setItem` leaves the timers alone.
    bound.cooking_timer = 3;
    bound.set_item(SLOT_INPUT, stack("minecraft:raw_iron", 1));
    assert_eq!(bound.cooking_timer, 3);
    assert_eq!(bound.cooking_total_time, BURN_TIME_STANDARD);

    // `RecipesUsed` is always stored, empty when nothing was cooked.
    let mut tag = CompoundTag::new();
    bound.save_additional(&mut tag);
    assert_eq!(tag.get_short("cooking_total_time"), Some(200));
    assert_eq!(tag.get_compound("RecipesUsed"), Some(&CompoundTag::new()));
}

#[test]
fn hopper_cooldown_when_full_after_failed_push_and_pull() {
    // Push: every chest slot holds a different, non-mergeable item.
    let mut hopper = HopperBlockEntity::new(Direction::Down);
    hopper.set_item(0, stack("minecraft:stone", 1));
    let mut chest = ChestBlockEntity::new();
    for slot in 0..CHEST_CONTAINER_SIZE {
        chest.set_item(slot, stack("minecraft:dirt", 1));
    }
    assert!(!hopper.push_items_tick(1, None, Some(&mut HopperTarget::Plain(&mut chest))));
    assert_eq!(hopper.cooldown_time, MOVE_ITEM_SPEED);

    // Freeing a slot does not help until the cooldown expires.
    chest.set_item(0, InventoryItemStack::empty());
    for game_time in 2..=8 {
        assert!(!hopper.push_items_tick(
            game_time,
            None,
            Some(&mut HopperTarget::Plain(&mut chest))
        ));
    }
    assert!(hopper.push_items_tick(9, None, Some(&mut HopperTarget::Plain(&mut chest))));
    assert_eq!(chest.get_item(0).item().name(), Some("minecraft:stone"));

    // Pull: the hopper has room but cannot accept the source item.
    let mut puller = HopperBlockEntity::new(Direction::Down);
    for (slot, name) in [
        "minecraft:stone",
        "minecraft:cobblestone",
        "minecraft:granite",
        "minecraft:diorite",
        "minecraft:andesite",
    ]
    .into_iter()
    .enumerate()
    {
        puller.set_item(slot, stack(name, 1));
    }
    puller.set_cooldown(0);
    let mut source = ChestBlockEntity::new();
    source.set_item(0, stack("minecraft:dirt", 1));
    let source_changes = source.base.changed_count();
    assert!(!puller.suck_in_items(&mut HopperTarget::Plain(&mut source), 1));
    assert_eq!(puller.cooldown_time, MOVE_ITEM_SPEED);
    assert!(source.base.changed_count() > source_changes);
    assert_eq!(source.get_item(0).get_count(), 1);
    assert_eq!((HOPPER_EMPTY, HOPPER_HAS_ITEMS, HOPPER_IS_FULL), (0, 1, 2));
}

#[test]
fn crafter_loads_loot_table_and_only_disables_empty_slots() {
    let mut items = rivet_nbt::list_tag::ListTag::new();
    let mut entry = CompoundTag::new();
    entry.put_byte("Slot", 0);
    entry.put_string("id", "minecraft:stone");
    entry.put_int("count", 1);
    items.list.push(Tag::Compound(entry));
    let mut input = CompoundTag::new();
    input.put_string("LootTable", "minecraft:chests/trial_chambers/supply");
    input.put_long("LootTableSeed", 5);
    input.put("Items".into(), Tag::List(items));
    input.put_int_array("disabled_slots", vec![0, 1]);

    let mut crafter = CrafterBlockEntity::new();
    crafter.load_additional(&input);
    assert!(!crafter.is_slot_disabled(0));
    assert!(crafter.is_slot_disabled(1));
    assert_eq!(
        crafter.base.loot_table,
        Some(Identifier::parse("minecraft:chests/trial_chambers/supply"))
    );
    assert_eq!(crafter.base.loot_table_seed, 5);
    assert_eq!(crafter.get_item(0).item().name(), Some("minecraft:stone"));

    let mut saved = CompoundTag::new();
    crafter.save_additional(&mut saved);
    assert_eq!(
        saved.get_string("LootTable").map(String::as_str),
        Some("minecraft:chests/trial_chambers/supply")
    );
    assert_eq!(saved.get_long("LootTableSeed"), Some(5));
    assert_eq!(saved.get_list("Items").map(|l| l.list.len()), Some(1));
    assert_eq!(saved.get_int_array("disabled_slots"), Some(&vec![1]));
}

#[test]
fn fuel_values_keep_zero_burn_entries_and_vanilla_craft_remainders() {
    let tiny = FuelValues::vanilla_burn_times_with_smelt_time(1);
    let stick = stack("minecraft:stick", 1);
    assert!(tiny.is_fuel(&stick));
    assert_eq!(tiny.burn_duration(&stick), 0);
    assert!(!tiny.is_fuel(&InventoryItemStack::empty()));

    let powder_snow = ItemId::from_name("minecraft:powder_snow_bucket").expect("powder snow");
    assert!(inventory_crafting_remainder(powder_snow).is_none());
    let water = ItemId::from_name("minecraft:water_bucket").expect("water bucket");
    let bucket = inventory_crafting_remainder(water).expect("bucket remainder");
    assert_eq!(bucket.item().name(), Some("minecraft:bucket"));
    assert_eq!(bucket.get_max_stack_size(), 16);
}

#[test]
fn processing_block_entities_still_valid_follows_context() {
    // `BaseContainerBlockEntity.stillValid` -> `Container.stillValidBlockEntity`:
    // false once the block entity is removed or the player is out of range.
    let ok = ContainerAccessContext::survival(1);
    let far = ContainerAccessContext {
        can_interact: false,
        ..ContainerAccessContext::survival(1)
    };
    let removed = ContainerAccessContext {
        is_removed: true,
        ..ContainerAccessContext::survival(1)
    };
    let chest = ChestBlockEntity::new();
    let hopper = HopperBlockEntity::new(Direction::Down);
    let furnace = new_furnace();
    for ctx in [&far, &removed] {
        assert!(!chest.still_valid(ctx));
        assert!(!hopper.still_valid(ctx));
        assert!(!furnace.still_valid(ctx));
    }
    assert!(chest.still_valid(&ok));
    assert!(hopper.still_valid(&ok));
    assert!(furnace.still_valid(&ok));
}

#[test]
fn hopper_failed_push_does_not_dirty_and_item_ops_do_not_set_changed() {
    // Paper `HopperBlockEntity.removeItem/setItem` never call `setChanged`, and a
    // failed `hopperPush` restores the count in place: the hopper stays clean.
    let mut chest = ChestBlockEntity::new();
    for slot in 0..27 {
        chest.set_item(slot, stack("minecraft:dirt", 64));
    }
    let mut hopper = HopperBlockEntity::new(Direction::Down);
    hopper.set_item(0, stack("minecraft:dirt", 1));
    let before = hopper.base.changed_count();
    // Chest is full, so `isFullContainer` short-circuits: nothing moves.
    assert!(!hopper.eject_items(&mut HopperTarget::Plain(&mut chest), 1));
    assert_eq!(hopper.base.changed_count(), before);

    // A chest with one dirt-accepting slot of 63: push succeeds; hopper slot
    // becomes 1 - 1 + 0 = empty, still without `setChanged` on the hopper.
    let mut chest = ChestBlockEntity::new();
    for slot in 0..27 {
        chest.set_item(slot, stack("minecraft:dirt", 64));
    }
    chest.set_item(5, stack("minecraft:dirt", 63));
    assert!(hopper.eject_items(&mut HopperTarget::Plain(&mut chest), 1));
    assert!(hopper.get_item(0).is_empty());
    assert_eq!(chest.get_item(5).get_count(), 64);
    assert_eq!(hopper.base.changed_count(), before);
    let _ = hopper.remove_item(0, 1);
    hopper.set_item(1, stack("minecraft:dirt", 2));
    assert_eq!(hopper.base.changed_count(), before);
}

#[test]
fn hopper_into_level_bound_furnace_resets_timers_like_set_item() {
    // Paper `AbstractFurnaceBlockEntity.setItem`: slot 0 replaced by a different
    // stack on a ServerLevel furnace => total = getTotalCookTime (raw_iron, 200
    // default smelting time here), cookingTimer = 0.
    let recipes = recipe_manager_with_cooking(
        "minecraft:test_smelt",
        CookingKind::Smelting,
        "minecraft:cobblestone",
        "minecraft:stone",
        200,
    );
    let mut furnace = new_furnace();
    furnace.cooking_timer = 100;
    furnace.cooking_total_time = 0;
    let mut hopper = HopperBlockEntity::new(Direction::Down);
    hopper.set_item(0, stack("minecraft:cobblestone", 1));
    assert!(hopper.eject_items(&mut HopperTarget::Furnace(&mut furnace, &recipes), 1));
    assert_eq!(furnace.cooking_timer, 0);
    assert_eq!(furnace.cooking_total_time, 200);
}

#[test]
fn load_all_items_follows_item_stack_with_slot_codec() {
    let mut list = rivet_nbt::list_tag::ListTag::new();
    let mut entry = |slot: i8, id: &str, count: Option<i32>| {
        let mut c = CompoundTag::new();
        c.put_byte("Slot", slot);
        c.put_string("id", id);
        if let Some(n) = count {
            c.put_int("count", n);
        }
        list.list.push(Tag::Compound(c));
    };
    entry(0, "stone", Some(5)); // Identifier default namespace -> minecraft:stone
    entry(1, "minecraft:stone", Some(100)); // intRange(1, 99) rejects
    entry(2, "minecraft:stone", Some(0)); // rejects
    entry(3, "minecraft:air", Some(1)); // Item.CODEC rejects air
    entry(4, "minecraft:stone", Some(99)); // upper bound ok
    let mut tag = CompoundTag::new();
    tag.put("Items".to_string(), Tag::List(list));
    let mut items = vec![InventoryItemStack::empty(); 5];
    load_all_items(&tag, &mut items);
    assert_eq!(items[0].item().name(), Some("minecraft:stone"));
    assert_eq!(items[0].get_count(), 5);
    assert!(items[1].is_empty());
    assert!(items[2].is_empty());
    assert!(items[3].is_empty());
    assert_eq!(items[4].get_count(), 99);
}
