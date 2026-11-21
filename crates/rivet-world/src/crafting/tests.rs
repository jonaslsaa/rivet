//! Paper-grounded unit tests for `crates/rivet-world/src/crafting/`.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use rivet_registry::Identifier;
use serde_json::json;

use super::*;

fn item(name: &str) -> ItemId {
    ItemId::from_name(name).unwrap_or_else(|| panic!("unknown vanilla item '{name}'"))
}

fn stack(name: &str, count: i32) -> ItemStack {
    ItemStack::new(item(name), count)
}

fn key(path: &str) -> RecipeKey {
    recipe_key(Identifier::with_default_namespace(path))
}

fn empty_tag_resolver(_tag: &Identifier) -> Vec<ItemId> {
    Vec::new()
}

fn planks_tag_resolver(tag: &Identifier) -> Vec<ItemId> {
    if tag.to_string() == "minecraft:planks" {
        vec![
            item("oak_planks"),
            item("birch_planks"),
            item("spruce_planks"),
        ]
    } else {
        Vec::new()
    }
}

#[test]
fn recipe_types_match_paper_registration_order() {
    let paths: Vec<&str> = RecipeType::ALL.iter().map(|t| t.path()).collect();
    assert_eq!(
        paths,
        vec![
            "crafting",
            "smelting",
            "blasting",
            "smoking",
            "campfire_cooking",
            "stonecutting",
            "smithing",
        ]
    );
    for rt in RecipeType::ALL {
        assert_eq!(RecipeType::from_name(rt.path()), Some(rt));
        assert_eq!(
            RecipeType::from_name(&rt.identifier().to_string()),
            Some(rt)
        );
    }
    assert_eq!(RecipeType::from_name("nonexistent"), None);
}

#[test]
fn ingredient_validation_and_matching() {
    // Controlled negatives: empty and air are rejected (Paper Ingredient constructor).
    assert_eq!(Ingredient::of_items(vec![]), Err(IngredientError::Empty));
    assert_eq!(
        Ingredient::of_item(ItemId::AIR),
        Err(IngredientError::ContainsAir)
    );
    assert_eq!(
        Ingredient::of_items(vec![item("stick"), ItemId::AIR]),
        Err(IngredientError::ContainsAir)
    );
    assert_eq!(
        Ingredient::of_exact_stacks(vec![ItemStack::empty()]),
        Err(IngredientError::ContainsAir)
    );

    // Direct multi-item ingredient.
    let ing = Ingredient::of_items(vec![item("oak_planks"), item("birch_planks")]).unwrap();
    assert!(!ing.is_exact());
    assert!(ing.test(&stack("oak_planks", 1)));
    assert!(ing.test(&stack("birch_planks", 64)));
    assert!(!ing.test(&stack("spruce_planks", 1)));
    assert!(!ing.test(&ItemStack::empty()));

    // Optional ingredient: None matches empty stack, rejects non-empty stack.
    assert!(Ingredient::test_optional(None, &ItemStack::empty()));
    assert!(!Ingredient::test_optional(None, &stack("stick", 1)));
    assert!(Ingredient::test_optional(
        Some(&ing),
        &stack("oak_planks", 1)
    ));
    assert!(!Ingredient::test_optional(Some(&ing), &ItemStack::empty()));

    // Paper exact-stack ingredient checks components too.
    let mut comps = BTreeMap::new();
    comps.insert(
        Identifier::with_default_namespace("custom_name"),
        "Magic Stick".to_owned(),
    );
    let exact_stack = ItemStack::with_components(item("stick"), 1, comps.clone());
    let exact_ing = Ingredient::of_exact_stacks(vec![exact_stack.clone()]).unwrap();
    assert!(exact_ing.is_exact());
    assert!(exact_ing.test(&exact_stack));
    assert!(!exact_ing.test(&stack("stick", 1)));
}

#[test]
fn placement_info_dense_indices_and_empty_slots() {
    let stick = Ingredient::of_item(item("stick")).unwrap();
    let iron = Ingredient::of_item(item("iron_ingot")).unwrap();
    let info = PlacementInfo::create_from_optionals(vec![
        None,
        Some(iron.clone()),
        None,
        Some(stick.clone()),
    ]);
    assert!(!info.is_impossible_to_place());
    assert_eq!(info.ingredients().len(), 2);
    assert_eq!(
        info.slots_to_ingredient_index(),
        &[PlacementInfo::EMPTY_SLOT, 0, PlacementInfo::EMPTY_SLOT, 1]
    );

    // Empty tag ingredient makes PlacementInfo unplaceable.
    let empty_tag =
        Ingredient::of_tag(Identifier::with_default_namespace("empty_tag"), vec![]).unwrap();
    let unplaceable = PlacementInfo::create_from_optionals(vec![Some(stick), Some(empty_tag)]);
    assert!(unplaceable.is_impossible_to_place());
}

#[test]
fn crafting_input_trims_empty_borders_and_tracks_offsets() {
    // 3x3 grid with items only in bottom-right 2x1:
    // . . .
    // . . .
    // . X Y
    let grid = vec![
        ItemStack::empty(),
        ItemStack::empty(),
        ItemStack::empty(),
        ItemStack::empty(),
        ItemStack::empty(),
        ItemStack::empty(),
        ItemStack::empty(),
        stack("stick", 1),
        stack("coal", 1),
    ];
    let positioned = CraftingInput::of_positioned(3, 3, grid);
    assert_eq!(positioned.left, 1);
    assert_eq!(positioned.top, 2);
    assert_eq!(positioned.input.width(), 2);
    assert_eq!(positioned.input.height(), 1);
    assert_eq!(positioned.input.ingredient_count(), 2);
    assert_eq!(positioned.input.get_item_xy(0, 0).item(), item("stick"));
    assert_eq!(positioned.input.get_item_xy(1, 0).item(), item("coal"));

    // All-empty grid yields CraftingInput::EMPTY.
    let empty = CraftingInput::of_positioned(2, 2, vec![ItemStack::empty(); 4]);
    assert!(empty.input.is_empty());
    assert_eq!(empty.input.width(), 0);
    assert_eq!(empty.input.height(), 0);
}

#[test]
fn shaped_pattern_shrinking_mirroring_and_negatives() {
    let mut key_map = IndexMap::new();
    key_map.insert('X', Ingredient::of_item(item("iron_ingot")).unwrap());
    key_map.insert('#', Ingredient::of_item(item("stick")).unwrap());

    // Asymmetric iron axe pattern with surrounding whitespace that shrinks to 2x3.
    let pattern = ShapedRecipePattern::of(
        &key_map,
        &["XX ".to_owned(), "X# ".to_owned(), " # ".to_owned()],
    )
    .unwrap();
    assert_eq!(pattern.width(), 2);
    assert_eq!(pattern.height(), 3);
    assert_eq!(pattern.ingredient_count(), 5);
    assert!(!pattern.is_symmetrical());

    // Normal orientation matches.
    let normal = CraftingInput::of(
        2,
        3,
        vec![
            stack("iron_ingot", 1),
            stack("iron_ingot", 1),
            stack("iron_ingot", 1),
            stack("stick", 1),
            ItemStack::empty(),
            stack("stick", 1),
        ],
    );
    assert!(pattern.matches(&normal));

    // Horizontally mirrored orientation also matches (Paper tries mirrored first).
    let mirrored = CraftingInput::of(
        2,
        3,
        vec![
            stack("iron_ingot", 1),
            stack("iron_ingot", 1),
            stack("stick", 1),
            stack("iron_ingot", 1),
            stack("stick", 1),
            ItemStack::empty(),
        ],
    );
    assert!(pattern.matches(&mirrored));

    // Wrong item in empty slot fails.
    let wrong = CraftingInput::of(
        2,
        3,
        vec![
            stack("iron_ingot", 1),
            stack("iron_ingot", 1),
            stack("iron_ingot", 1),
            stack("stick", 1),
            stack("stick", 1),
            stack("stick", 1),
        ],
    );
    assert!(!pattern.matches(&wrong));

    // Controlled negatives for pattern validation.
    assert_eq!(
        ShapedRecipePattern::of(&key_map, &[]),
        Err(PatternError::EmptyPattern)
    );
    assert_eq!(
        ShapedRecipePattern::of(&key_map, &["X".into(), "X".into(), "X".into(), "X".into()]),
        Err(PatternError::TooManyRows)
    );
    assert_eq!(
        ShapedRecipePattern::of(&key_map, &["XXXX".into()]),
        Err(PatternError::TooManyColumns)
    );
    assert_eq!(
        ShapedRecipePattern::of(&key_map, &["XX".into(), "X".into()]),
        Err(PatternError::UnevenRows)
    );
    assert_eq!(
        ShapedRecipePattern::of(&key_map, &["X?".into()]),
        Err(PatternError::UndefinedSymbol('?'))
    );
    assert_eq!(
        ShapedRecipePattern::of(&key_map, &["X".into()]),
        Err(PatternError::UnusedSymbols(vec!['#']))
    );
    let mut bad_key = key_map.clone();
    bad_key.insert(' ', Ingredient::of_item(item("stick")).unwrap());
    assert_eq!(
        ShapedRecipePattern::of(&bad_key, &["X".into()]),
        Err(PatternError::SpaceInKey)
    );
}

#[test]
fn shapeless_recipe_bipartite_overlapping_ingredients() {
    // Recipe requires [#planks, oak_planks].
    // Input has [oak_planks, birch_planks].
    // Greedy matching could mistakenly bind #planks -> oak_planks and fail on oak_planks;
    // bipartite augmenting-path matching must succeed.
    let planks = Ingredient::of_tag(
        Identifier::with_default_namespace("planks"),
        vec![item("oak_planks"), item("birch_planks")],
    )
    .unwrap();
    let oak_only = Ingredient::of_item(item("oak_planks")).unwrap();

    let recipe = ShapelessRecipe {
        common_info: CommonInfo::default(),
        book_info: CraftingBookInfo::default(),
        ingredients: vec![planks, oak_only],
        result: ItemStackTemplate::new(item("stick"), 4),
    };

    let input = CraftingInput::of(2, 1, vec![stack("oak_planks", 1), stack("birch_planks", 1)]);
    assert!(recipe.matches(&input));
    assert_eq!(recipe.assemble(&input), stack("stick", 4));

    // Controlled negative: two birch planks cannot satisfy oak_only.
    let bad_input = CraftingInput::of(
        2,
        1,
        vec![stack("birch_planks", 1), stack("birch_planks", 1)],
    );
    assert!(!recipe.matches(&bad_input));
}

#[test]
fn cooking_stonecutting_and_smithing_behavior() {
    // Smelting recipe (default 200 ticks)
    let smelt = CookingRecipe {
        kind: CookingKind::Smelting,
        common_info: CommonInfo::default(),
        book_info: CookingBookInfo {
            category: CookingBookCategory::Misc,
            group: "iron_ingot".to_owned(),
        },
        ingredient: Ingredient::of_item(item("raw_iron")).unwrap(),
        result: ItemStackTemplate::new(item("iron_ingot"), 1),
        experience: 0.7,
        cooking_time: DEFAULT_SMELTING_TIME,
    };
    assert!(smelt.matches(&SingleRecipeInput::new(stack("raw_iron", 1))));
    assert!(!smelt.matches(&SingleRecipeInput::new(stack("raw_gold", 1))));
    assert_eq!(
        smelt.assemble(&SingleRecipeInput::new(stack("raw_iron", 1))),
        stack("iron_ingot", 1)
    );
    assert_eq!(smelt.display()[0].type_path(), "furnace");

    // Stonecutter
    let cut_stairs = StonecutterRecipe {
        common_info: CommonInfo::default(),
        ingredient: Ingredient::of_item(item("stone")).unwrap(),
        result: ItemStackTemplate::new(item("stone_stairs"), 1),
    };
    let cut_slab = StonecutterRecipe {
        common_info: CommonInfo::default(),
        ingredient: Ingredient::of_item(item("stone")).unwrap(),
        result: ItemStackTemplate::new(item("stone_slab"), 2),
    };

    // SmithingTransform with copy_data_components = true vs false
    let transform_copy = SmithingTransformRecipe {
        common_info: CommonInfo::default(),
        template: Some(Ingredient::of_item(item("netherite_upgrade_smithing_template")).unwrap()),
        base: Ingredient::of_item(item("diamond_sword")).unwrap(),
        addition: Some(Ingredient::of_item(item("netherite_ingot")).unwrap()),
        result: ItemStackTemplate::new(item("netherite_sword"), 1),
        copy_data_components: true,
    };
    let mut base_comps = BTreeMap::new();
    base_comps.insert(
        Identifier::with_default_namespace("enchantments"),
        "sharpness:5".to_owned(),
    );
    let smithing_in = SmithingRecipeInput::new(
        stack("netherite_upgrade_smithing_template", 1),
        ItemStack::with_components(item("diamond_sword"), 1, base_comps.clone()),
        stack("netherite_ingot", 1),
    );
    assert!(transform_copy.matches(&smithing_in));
    let upgraded = transform_copy.assemble(&smithing_in);
    assert_eq!(upgraded.item(), item("netherite_sword"));
    assert_eq!(upgraded.components(), &base_comps);

    let mut transform_no_copy = transform_copy.clone();
    transform_no_copy.copy_data_components = false;
    let upgraded_clean = transform_no_copy.assemble(&smithing_in);
    assert!(upgraded_clean.components().is_empty());

    // SmithingTrim rejects re-applying the exact same trim (returns EMPTY).
    let trim = SmithingTrimRecipe {
        common_info: CommonInfo::default(),
        template: Ingredient::of_item(item("ward_armor_trim_smithing_template")).unwrap(),
        base: Ingredient::of_item(item("diamond_chestplate")).unwrap(),
        addition: Ingredient::of_item(item("gold_ingot")).unwrap(),
        pattern: Identifier::with_default_namespace("ward"),
    };
    let trim_in = SmithingRecipeInput::new(
        stack("ward_armor_trim_smithing_template", 1),
        stack("diamond_chestplate", 1),
        stack("gold_ingot", 1),
    );
    let trimmed = trim.assemble(&trim_in);
    assert!(!trimmed.is_empty());
    let repeat_trim_in = SmithingRecipeInput::new(
        stack("ward_armor_trim_smithing_template", 1),
        trimmed,
        stack("gold_ingot", 1),
    );
    assert!(trim.assemble(&repeat_trim_in).is_empty());

    // Manager property sets & stonecutter ordering.
    let manager = RecipeManager::from_recipes(vec![
        RecipeHolder::new(key("iron_smelt"), Recipe::Cooking(smelt)),
        RecipeHolder::new(key("stone_stairs"), Recipe::Stonecutter(cut_stairs)),
        RecipeHolder::new(key("stone_slab"), Recipe::Stonecutter(cut_slab)),
        RecipeHolder::new(
            key("netherite_sword"),
            Recipe::SmithingTransform(transform_copy),
        ),
    ])
    .unwrap();

    let furnace_set = manager
        .property_set(&Identifier::with_default_namespace(
            RecipePropertySet::FURNACE_INPUT,
        ))
        .unwrap();
    assert!(furnace_set.test(&stack("raw_iron", 1)));
    assert!(!furnace_set.test(&stack("stone", 1)));

    let selected = manager
        .stonecutter_recipes()
        .select_by_input(&stack("stone", 1));
    assert_eq!(selected.len(), 2);
    assert_eq!(
        selected.entries()[0].recipe.recipe.as_ref().unwrap().id,
        key("stone_stairs")
    );
    assert_eq!(
        selected.entries()[1].recipe.recipe.as_ref().unwrap().id,
        key("stone_slab")
    );
}

#[test]
fn recipe_manager_last_match_priority_duplicate_rejection_and_caches() {
    let r1 = RecipeHolder::new(
        key("first_stick_recipe"),
        Recipe::Shapeless(ShapelessRecipe {
            common_info: CommonInfo::default(),
            book_info: CraftingBookInfo {
                category: CraftingBookCategory::Misc,
                group: "sticks".to_owned(),
            },
            ingredients: vec![Ingredient::of_item(item("oak_planks")).unwrap()],
            result: ItemStackTemplate::new(item("stick"), 2),
        }),
    );
    let r2 = RecipeHolder::new(
        key("second_stick_recipe"),
        Recipe::Shapeless(ShapelessRecipe {
            common_info: CommonInfo::default(),
            book_info: CraftingBookInfo {
                category: CraftingBookCategory::Misc,
                group: "sticks".to_owned(),
            },
            ingredients: vec![Ingredient::of_item(item("oak_planks")).unwrap()],
            result: ItemStackTemplate::new(item("stick"), 4),
        }),
    );

    // Duplicate ID is rejected.
    let dup_err = RecipeManager::from_recipes(vec![r1.clone(), r1.clone()]);
    assert!(matches!(dup_err, Err(RecipeMapError::DuplicateId(_))));

    let mut manager = RecipeManager::from_recipes(vec![r1, r2]).unwrap();
    let input = CraftingInput::of(1, 1, vec![stack("oak_planks", 1)]);

    // Paper ordering rule: later recipe wins (list.getLast()).
    let matched = manager.get_crafting_recipe_for(&input).unwrap();
    assert_eq!(matched.id, key("second_stick_recipe"));
    assert_eq!(
        matched.value.assemble_crafting(&input),
        Some(stack("stick", 4))
    );

    // Empty input returns None.
    assert!(
        manager
            .get_crafting_recipe_for(&CraftingInput::empty())
            .is_none()
    );

    // Display unpacking assigns sequential IDs and shared group ID.
    let d1 = manager.list_displays_for_recipe(&key("first_stick_recipe"));
    let d2 = manager.list_displays_for_recipe(&key("second_stick_recipe"));
    assert_eq!(d1[0].id, RecipeDisplayId(0));
    assert_eq!(d2[0].id, RecipeDisplayId(1));
    assert_eq!(d1[0].group, Some(0));
    assert_eq!(d2[0].group, Some(0));

    // CachedCheck remembers the matched recipe.
    let mut check = CachedCheck::new(RecipeType::Crafting);
    assert_eq!(
        check.get_crafting(&input, &manager).unwrap().id,
        key("second_stick_recipe")
    );
    assert_eq!(check.last_recipe(), Some(&key("second_stick_recipe")));

    // RecipeCache caches hit and invalidates when manager mutates.
    let mut cache = RecipeCache::new(4);
    assert_eq!(
        cache.get(&manager, &input).unwrap().id,
        key("second_stick_recipe")
    );
    assert!(manager.remove_recipe(&key("second_stick_recipe")));
    assert_eq!(
        cache.get(&manager, &input).unwrap().id,
        key("first_stick_recipe")
    );
}

#[test]
fn crafting_remainders_for_container_items() {
    let input = CraftingInput::of(
        3,
        1,
        vec![
            stack("water_bucket", 1),
            stack("honey_bottle", 1),
            stack("wheat", 1),
        ],
    );
    let remainders = default_crafting_remainder(&input);
    assert_eq!(remainders[0], stack("bucket", 1));
    assert_eq!(remainders[1], stack("glass_bottle", 1));
    assert!(remainders[2].is_empty());
}

#[test]
fn server_recipe_book_discovery_removal_and_untrusted_load() {
    let normal_holder = RecipeHolder::new(
        key("torch"),
        Recipe::Shapeless(ShapelessRecipe {
            common_info: CommonInfo {
                show_notification: true,
            },
            book_info: CraftingBookInfo::default(),
            ingredients: vec![
                Ingredient::of_item(item("coal")).unwrap(),
                Ingredient::of_item(item("stick")).unwrap(),
            ],
            result: ItemStackTemplate::new(item("torch"), 4),
        }),
    );
    let special_holder = RecipeHolder::new(
        key("repair_item"),
        Recipe::Special {
            serializer: RecipeSerializerId::CraftingSpecialRepairItem,
            category: CraftingBookCategory::Misc,
        },
    );

    let manager =
        RecipeManager::from_recipes(vec![normal_holder.clone(), special_holder.clone()]).unwrap();
    let mut book = ServerRecipeBook::new();

    // Special recipes are ignored on add_recipes.
    let (added, entries) = book.add_recipes([&normal_holder, &special_holder], &manager);
    assert_eq!(added, 1);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].notification);
    assert!(entries[0].highlight);
    assert!(book.contains(&key("torch")));
    assert!(!book.contains(&key("repair_item")));
    assert!(book.is_highlighted(&key("torch")));

    // Initial recipe book sets notification = false and preserves highlight.
    let initial = book.initial_recipe_book_entries(&manager);
    assert_eq!(initial.len(), 1);
    assert!(!initial[0].notification);
    assert!(initial[0].highlight);

    // Settings per RecipeBookType.
    book.set_open(RecipeBookType::Crafting, true);
    book.set_filtering(RecipeBookType::Furnace, true);
    assert!(book.is_open(RecipeBookType::Crafting));
    assert!(book.is_filtering(RecipeBookType::Furnace));
    assert!(!book.is_open(RecipeBookType::Smoker));

    // Pack + load_untrusted drops invalid/removed keys.
    let mut packed = book.pack();
    packed.known.push(key("removed_recipe"));
    packed.highlight.push(key("removed_recipe"));

    let mut reloaded = ServerRecipeBook::new();
    reloaded.load_untrusted(packed, |k| manager.by_key(k).is_some());
    assert!(reloaded.contains(&key("torch")));
    assert!(!reloaded.contains(&key("removed_recipe")));
    assert!(!reloaded.is_highlighted(&key("removed_recipe")));
    assert!(reloaded.is_open(RecipeBookType::Crafting));
    assert!(reloaded.is_filtering(RecipeBookType::Furnace));

    // Removing recipe clears both known and highlight.
    let (removed, display_ids) = reloaded.remove_recipes([&normal_holder], &manager);
    assert_eq!(removed, 1);
    assert_eq!(display_ids, vec![RecipeDisplayId(0)]);
    assert!(!reloaded.contains(&key("torch")));
    assert!(!reloaded.is_highlighted(&key("torch")));
}

#[test]
fn json_loader_parses_vanilla_recipes() {
    let shaped_json = json!({
        "type": "minecraft:crafting_shaped",
        "category": "misc",
        "group": "sticks",
        "pattern": [
            "#",
            "#"
        ],
        "key": {
            "#": "#minecraft:planks"
        },
        "result": {
            "id": "minecraft:stick",
            "count": 4
        }
    });
    let shaped = parse_recipe_json(&shaped_json, &planks_tag_resolver).unwrap();
    assert_eq!(shaped.recipe_type(), RecipeType::Crafting);
    assert_eq!(shaped.group(), "sticks");
    let grid = CraftingInput::of(1, 2, vec![stack("oak_planks", 1), stack("birch_planks", 1)]);
    assert!(shaped.matches_crafting(&grid));
    assert_eq!(shaped.assemble_crafting(&grid), Some(stack("stick", 4)));

    // Smelting JSON uses Paper default cookingtime=200 and experience=0.0 when omitted.
    let smelting_json = json!({
        "type": "minecraft:smelting",
        "ingredient": "minecraft:cobblestone",
        "result": "minecraft:stone"
    });
    let Recipe::Cooking(smelt) = parse_recipe_json(&smelting_json, &empty_tag_resolver).unwrap()
    else {
        panic!("expected cooking recipe");
    };
    assert_eq!(smelt.cooking_time, DEFAULT_SMELTING_TIME);
    assert_eq!(smelt.experience, 0.0_f32);
    assert_eq!(smelt.result, ItemStackTemplate::new(item("stone"), 1));

    // Controlled negative: unknown serializer fails cleanly.
    let bad_json = json!({ "type": "minecraft:does_not_exist" });
    assert!(matches!(
        parse_recipe_json(&bad_json, &empty_tag_resolver),
        Err(RecipeJsonError::UnknownSerializer(_))
    ));

    // Controlled negative: invalid tag and trim pattern identifiers return errors instead of panicking.
    let bad_tag_json = json!({
        "type": "minecraft:smelting",
        "ingredient": "#minecraft:BAD TAG!",
        "result": "minecraft:stone"
    });
    assert!(matches!(
        parse_recipe_json(&bad_tag_json, &empty_tag_resolver),
        Err(RecipeJsonError::InvalidField {
            field: "ingredient",
            ..
        })
    ));

    let bad_pattern_json = json!({
        "type": "minecraft:smithing_trim",
        "template": "minecraft:coast_armor_trim_smithing_template",
        "base": "minecraft:diamond_chestplate",
        "addition": "minecraft:gold_ingot",
        "pattern": "INVALID PATTERN!"
    });
    assert!(matches!(
        parse_recipe_json(&bad_pattern_json, &empty_tag_resolver),
        Err(RecipeJsonError::InvalidField {
            field: "pattern",
            ..
        })
    ));

    // Result components (e.g. suspicious_stew_effects) are preserved on crafted output.
    let stew_json = json!({
        "type": "minecraft:crafting_shapeless",
        "ingredients": [
            "minecraft:brown_mushroom",
            "minecraft:red_mushroom",
            "minecraft:bowl",
            "minecraft:allium"
        ],
        "result": {
            "id": "minecraft:suspicious_stew",
            "count": 1,
            "components": {
                "minecraft:suspicious_stew_effects": [{"id": "minecraft:fire_resistance", "duration": 60}]
            }
        }
    });
    let stew = parse_recipe_json(&stew_json, &empty_tag_resolver).unwrap();
    let stew_grid = CraftingInput::of(
        2,
        2,
        vec![
            stack("brown_mushroom", 1),
            stack("red_mushroom", 1),
            stack("bowl", 1),
            stack("allium", 1),
        ],
    );
    let crafted_stew = stew.assemble_crafting(&stew_grid).unwrap();
    assert_eq!(crafted_stew.item(), item("suspicious_stew"));
    assert!(
        crafted_stew
            .components()
            .contains_key(&Identifier::with_default_namespace(
                "suspicious_stew_effects"
            ))
    );

    // Controlled negative: result count 0, negative, or > 99 is rejected.
    let zero_count_json = json!({
        "type": "minecraft:smelting",
        "ingredient": "minecraft:cobblestone",
        "result": { "id": "minecraft:stone", "count": 0 }
    });
    assert!(matches!(
        parse_recipe_json(&zero_count_json, &empty_tag_resolver),
        Err(RecipeJsonError::InvalidField {
            field: "result.count",
            ..
        })
    ));

    let air_result_json = json!({
        "type": "minecraft:smelting",
        "ingredient": "minecraft:cobblestone",
        "result": "minecraft:air"
    });
    assert!(matches!(
        parse_recipe_json(&air_result_json, &empty_tag_resolver),
        Err(RecipeJsonError::InvalidField {
            field: "result",
            ..
        })
    ));

    // Controlled negative: shapeless recipes with 0 or > 9 ingredients are rejected.
    let empty_shapeless = json!({
        "type": "minecraft:crafting_shapeless",
        "ingredients": [],
        "result": "minecraft:stick"
    });
    assert!(matches!(
        parse_recipe_json(&empty_shapeless, &empty_tag_resolver),
        Err(RecipeJsonError::InvalidField {
            field: "ingredients",
            ..
        })
    ));

    let ten_shapeless = json!({
        "type": "minecraft:crafting_shapeless",
        "ingredients": vec!["minecraft:stick"; 10],
        "result": "minecraft:stick"
    });
    assert!(matches!(
        parse_recipe_json(&ten_shapeless, &empty_tag_resolver),
        Err(RecipeJsonError::InvalidField {
            field: "ingredients",
            ..
        })
    ));
}
