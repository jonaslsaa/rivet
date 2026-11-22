use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::{Identifier, core::Direction};

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
    chest.base.custom_name = Some("Storage".to_string());
    chest.base.lock_key = LockCode::new("secret");
    chest.save_additional(&mut nbt);

    let mut reloaded = ChestBlockEntity::new();
    reloaded.load_additional(&nbt);
    assert_eq!(reloaded.get_item(0), chest.get_item(0));
    assert_eq!(reloaded.get_item(26), chest.get_item(26));
    assert_eq!(reloaded.base.custom_name.as_deref(), Some("Storage"));
    assert_eq!(reloaded.base.lock_key.key(), Some("secret"));

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
    assert!(barrel_nbt.get("Items").is_none());

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
