//! Comprehensive Paper 26.2 behavioral and exact-RNG-consumption tests for
//! `crates/rivet-world/src/loot`.

use std::rc::Rc;

use rivet_registry::Identifier;
use rivet_util::mth;
use rivet_util::problem_reporter::Collector;
use rivet_util::random::{LegacyRandomSource, RandomSource};

use super::*;

fn id(path: &str) -> Identifier {
    Identifier::with_default_namespace(path)
}

fn empty_params() -> LootParams {
    LootParams::builder()
        .create(LootContextParamSets::empty())
        .expect("empty params should build")
}

fn make_context<'a>(params: LootParams, seed: i64, resolver: &'a LootRegistry) -> LootContext<'a> {
    LootContext::builder(params, resolver)
        .with_optional_random_source(Some(LootRandom::Legacy(LegacyRandomSource::new(seed))))
        .create(None)
}

// ---------------------------------------------------------------------------
// 1. Exact Paper RNG consumption & pool roll semantics
// ---------------------------------------------------------------------------

#[test]
fn single_valid_entry_skips_random_selection() {
    let registry = LootRegistry::new();
    let table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .set_rolls(NumberProvider::constant(3.0))
                .add(LootPoolEntryContainer::item(id("diamond")))
                .build(),
        )
        .build();

    let seed = 0x1234_5678_9ABC_DEF0_u64 as i64;
    let mut ctx = make_context(empty_params(), seed, &registry);
    let mut items = Vec::new();
    table.get_random_items_raw(&mut ctx, &mut |stack| items.push(stack));

    assert_eq!(items.len(), 3);
    assert!(
        items
            .iter()
            .all(|s| s.item() == &id("diamond") && s.count() == 1)
    );

    // Exact RNG check: 0 calls consumed because constant rolls + 1 valid entry.
    let mut expected_rng = LegacyRandomSource::new(seed);
    assert_eq!(ctx.random_mut().next_long(), expected_rng.next_long());
}

#[test]
fn failed_pool_condition_consumes_zero_roll_rng() {
    let registry = LootRegistry::new();
    let table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .when(LootItemCondition::ParamPresent(LootContextParams::origin()))
                .set_rolls(NumberProvider::uniform_between(1.0, 5.0))
                .set_bonus_rolls(NumberProvider::uniform_between(0.0, 3.0))
                .add(LootPoolEntryContainer::item(id("emerald")))
                .add(LootPoolEntryContainer::item(id("gold_ingot")))
                .build(),
        )
        .build();

    let seed = 42;
    let mut ctx = make_context(empty_params(), seed, &registry);
    let mut items = Vec::new();
    table.get_random_items_raw(&mut ctx, &mut |stack| items.push(stack));

    assert!(items.is_empty());

    // Pool condition failed before evaluating rolls/bonus_rolls -> 0 RNG calls.
    let mut expected_rng = LegacyRandomSource::new(seed);
    assert_eq!(ctx.random_mut().next_long(), expected_rng.next_long());
}

#[test]
fn weighted_selection_exact_rng_and_order() {
    let registry = LootRegistry::new();
    let table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .set_rolls(NumberProvider::constant(10.0))
                .add(LootPoolEntryContainer::item(id("coal")).with_weight(5))
                .add(LootPoolEntryContainer::item(id("iron_ingot")).with_weight(3))
                .add(LootPoolEntryContainer::item(id("diamond")).with_weight(2))
                .build(),
        )
        .build();

    let seed = 987_654_321_i64;
    let mut ctx = make_context(empty_params(), seed, &registry);
    let mut actual = Vec::new();
    table.get_random_items_raw(&mut ctx, &mut |stack| actual.push(stack.item().clone()));

    // Mirror Paper's exact algorithm with an independent LegacyRandomSource.
    let mut oracle_rng = LegacyRandomSource::new(seed);
    let weights = [(id("coal"), 5), (id("iron_ingot"), 3), (id("diamond"), 2)];
    let mut expected = Vec::new();
    for _ in 0..10 {
        let mut idx = oracle_rng.next_int_bound(10);
        for (item, w) in &weights {
            idx -= *w;
            if idx < 0 {
                expected.push(item.clone());
                break;
            }
        }
    }

    assert_eq!(actual, expected);
    assert_eq!(ctx.random_mut().next_long(), oracle_rng.next_long());
}

#[test]
fn luck_adjusts_bonus_rolls_and_entry_weights_with_zero_weight_filtering() {
    // Entry A: weight 2, quality -2 -> at luck 1.0, effective weight = floor(2 - 2) = 0 (filtered)
    // Entry B: weight 1, quality 1  -> at luck 1.0, effective weight = floor(1 + 1) = 2 (sole valid)
    // Rolls: 1 + floor(1.5 * 1.0) = 2
    let registry = LootRegistry::new();
    let table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .set_rolls(NumberProvider::constant(1.0))
                .set_bonus_rolls(NumberProvider::constant(1.5))
                .add(
                    LootPoolEntryContainer::item(id("rotten_flesh"))
                        .with_weight(2)
                        .with_quality(-2),
                )
                .add(
                    LootPoolEntryContainer::item(id("golden_apple"))
                        .with_weight(1)
                        .with_quality(1),
                )
                .build(),
        )
        .build();

    let mut params_builder = LootParams::builder();
    params_builder.with_luck(1.0);
    let params = params_builder
        .create(LootContextParamSets::empty())
        .unwrap();

    let seed = 77;
    let mut ctx = make_context(params, seed, &registry);
    let mut items = Vec::new();
    table.get_random_items_raw(&mut ctx, &mut |stack| items.push(stack));

    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|s| s.item() == &id("golden_apple")));

    // Only 1 entry remained with weight > 0 -> 0 RNG calls.
    let mut oracle_rng = LegacyRandomSource::new(seed);
    assert_eq!(ctx.random_mut().next_long(), oracle_rng.next_long());
}

// ---------------------------------------------------------------------------
// 2. Composite entries, TagEntry, DynamicLoot, EmptyLootItem
// ---------------------------------------------------------------------------

#[test]
fn alternatives_short_circuits_without_consuming_later_child_rng() {
    let registry = LootRegistry::new();
    let alt = LootPoolEntryContainer::alternatives(vec![
        LootPoolEntryContainer::item(id("first")),
        LootPoolEntryContainer::item(id("second")).when(LootItemCondition::random_chance(0.5)),
    ]);

    let table = LootTable::builder()
        .with_pool(LootPool::builder().add(alt).build())
        .build();

    let seed = 555;
    let mut ctx = make_context(empty_params(), seed, &registry);
    let mut items = Vec::new();
    table.get_random_items_raw(&mut ctx, &mut |s| items.push(s.item().clone()));

    assert_eq!(items, vec![id("first")]);
    // Second child's random_chance was never evaluated -> 0 RNG calls.
    let mut oracle = LegacyRandomSource::new(seed);
    assert_eq!(ctx.random_mut().next_long(), oracle.next_long());
}

#[test]
fn entry_group_and_sequence_semantics() {
    let registry = LootRegistry::new();

    // Group expands all children whose conditions pass, even if an earlier child fails.
    let group = LootPoolEntryContainer::group(vec![
        LootPoolEntryContainer::item(id("fail_first"))
            .when(LootItemCondition::ParamPresent(LootContextParams::origin())),
        LootPoolEntryContainer::item(id("succeed_second")),
    ]);

    let table_group = LootTable::builder()
        .with_pool(LootPool::builder().add(group).build())
        .build();

    let mut ctx = make_context(empty_params(), 1, &registry);
    let mut items = Vec::new();
    table_group.get_random_items_raw(&mut ctx, &mut |s| items.push(s.item().clone()));
    assert_eq!(items, vec![id("succeed_second")]);

    // Sequence stops at the first failing child.
    let seq = LootPoolEntryContainer::sequence(vec![
        LootPoolEntryContainer::item(id("first")),
        LootPoolEntryContainer::item(id("fail_mid"))
            .when(LootItemCondition::ParamPresent(LootContextParams::origin())),
        LootPoolEntryContainer::item(id("never_reached")),
    ]);

    let table_seq = LootTable::builder()
        .with_pool(LootPool::builder().add(seq).build())
        .build();

    let mut ctx = make_context(empty_params(), 1, &registry);
    let mut seq_items = Vec::new();
    table_seq.get_random_items_raw(&mut ctx, &mut |s| seq_items.push(s.item().clone()));
    // Only "first" expanded before "fail_mid" stopped the sequence.
    assert_eq!(seq_items, vec![id("first")]);
}

#[test]
fn tag_entry_expand_vs_contents_preserves_tag_order() {
    let mut registry = LootRegistry::new();
    registry.register_item_tag(
        id("gems"),
        vec![id("diamond"), id("emerald"), id("amethyst_shard")],
    );

    // expand = false: emits ALL items from the tag in exact tag order on a single roll, 0 RNG calls.
    let table_all = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(LootPoolEntryContainer::tag_contents(id("gems")))
                .build(),
        )
        .build();

    let seed = 101;
    let mut ctx = make_context(empty_params(), seed, &registry);
    let mut all_items = Vec::new();
    table_all.get_random_items_raw(&mut ctx, &mut |s| all_items.push(s.item().clone()));
    assert_eq!(
        all_items,
        vec![id("diamond"), id("emerald"), id("amethyst_shard")]
    );
    let mut oracle = LegacyRandomSource::new(seed);
    assert_eq!(ctx.random_mut().next_long(), oracle.next_long());

    // expand = true: expands into 3 equal-weight entries -> picks 1 using next_int_bound(3).
    let table_exp = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(LootPoolEntryContainer::expand_tag(id("gems")))
                .build(),
        )
        .build();

    let mut ctx = make_context(empty_params(), seed, &registry);
    let mut exp_items = Vec::new();
    table_exp.get_random_items_raw(&mut ctx, &mut |s| exp_items.push(s.item().clone()));

    let mut oracle = LegacyRandomSource::new(seed);
    let expected_idx = oracle.next_int_bound(3) as usize;
    let expected_tag = [id("diamond"), id("emerald"), id("amethyst_shard")];
    assert_eq!(exp_items, vec![expected_tag[expected_idx].clone()]);
    assert_eq!(ctx.random_mut().next_long(), oracle.next_long());
}

#[test]
fn dynamic_loot_and_empty_entry_behavior() {
    let registry = LootRegistry::new();
    let mut params_builder = LootParams::builder();
    params_builder
        .with_dynamic_drop(
            id("contents"),
            Rc::new(|out| {
                out(LootItemStack::with_count(id("apple"), 2));
                out(LootItemStack::with_count(id("bread"), 1));
            }),
        )
        .unwrap();
    let params = params_builder
        .create(LootContextParamSets::empty())
        .unwrap();

    let table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(LootPoolEntryContainer::dynamic(id("contents")))
                .apply(LootItemFunction::set_count_with_add(
                    NumberProvider::constant(1.0),
                    true,
                ))
                .build(),
        )
        .with_pool(
            LootPool::builder()
                .add(LootPoolEntryContainer::empty())
                .build(),
        )
        .build();

    let mut ctx = make_context(params, 1, &registry);
    let mut items = Vec::new();
    table.get_random_items_raw(&mut ctx, &mut |s| items.push((s.item().clone(), s.count())));

    assert_eq!(items, vec![(id("apple"), 3), (id("bread"), 2)]);
}

// ---------------------------------------------------------------------------
// 3. Number providers & IntRange
// ---------------------------------------------------------------------------

#[test]
fn java_round_and_number_providers_exact_rng() {
    let registry = LootRegistry::new();

    // Java Math.round(float) semantics: floor(x + 0.5)
    assert_eq!(java_round_f32(2.5), 3);
    assert_eq!(java_round_f32(-2.5), -2);
    assert_eq!(java_round_f32(f32::NAN), 0);

    // UniformGenerator exact RNG vs Mth.nextInt / Mth.nextFloat
    let seed = 2026;
    let mut ctx = make_context(empty_params(), seed, &registry);
    let mut oracle = LegacyRandomSource::new(seed);

    let uniform = NumberProvider::uniform_between(2.0, 7.0);
    assert_eq!(uniform.get_int(&mut ctx), mth::next_int(&mut oracle, 2, 7));
    assert_eq!(
        uniform.get_float(&mut ctx),
        mth::next_float(&mut oracle, 2.0, 7.0)
    );
    assert_eq!(ctx.random_mut().next_long(), oracle.next_long());

    // BinomialDistributionGenerator exact RNG
    let mut ctx = make_context(empty_params(), seed, &registry);
    let mut oracle = LegacyRandomSource::new(seed);
    let binom = NumberProvider::binomial(5, 0.4);
    let got = binom.get_int(&mut ctx);
    let mut expected = 0;
    for _ in 0..5 {
        if oracle.next_float() < 0.4 {
            expected += 1;
        }
    }
    assert_eq!(got, expected);
    assert_eq!(ctx.random_mut().next_long(), oracle.next_long());
}

#[test]
fn scoreboard_value_and_int_range() {
    let registry = LootRegistry::new();
    let mut params_builder = LootParams::builder();
    params_builder.with_score("player1", "kills", 7);
    let params = params_builder
        .create(LootContextParamSets::empty())
        .unwrap();

    let mut ctx = make_context(params, 1, &registry);
    let present = NumberProvider::score(ScoreTarget::Fixed("player1".to_string()), "kills", 0.5);
    assert_eq!(present.get_float(&mut ctx), 3.5);
    assert_eq!(present.get_int(&mut ctx), 4); // Java Math.round(3.5) == 4

    let missing = NumberProvider::score(ScoreTarget::Fixed("nobody".to_string()), "kills", 2.0);
    assert_eq!(missing.get_float(&mut ctx), 0.0);

    let range = IntRange::range(2, 5);
    assert_eq!(range.clamp(&mut ctx, 1), 2);
    assert_eq!(range.clamp(&mut ctx, 4), 4);
    assert_eq!(range.clamp(&mut ctx, 9), 5);
    assert!(range.test(&mut ctx, 2));
    assert!(range.test(&mut ctx, 5));
    assert!(!range.test(&mut ctx, 6));

    assert_eq!(IntRange::exact(4).unpack_exact(), Some(4));
    assert_eq!(IntRange::range(4, 5).unpack_exact(), None);
}

// ---------------------------------------------------------------------------
// 4. Conditions, Functions, and Decoration Order
// ---------------------------------------------------------------------------

#[test]
fn function_decoration_order_entry_then_pool_then_table() {
    // Start with count 1:
    // Entry: set_count(3) -> 3
    // Pool:  set_count_with_add(2, true) -> 5
    // Table: limit_count(1..=4) -> 4
    let registry = LootRegistry::new();
    let table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(
                    LootPoolEntryContainer::item(id("arrow"))
                        .apply(LootItemFunction::set_count(NumberProvider::constant(3.0))),
                )
                .apply(LootItemFunction::set_count_with_add(
                    NumberProvider::constant(2.0),
                    true,
                ))
                .build(),
        )
        .apply(LootItemFunction::limit_count(IntRange::range(1, 4)))
        .build();

    let mut ctx = make_context(empty_params(), 1, &registry);
    let mut items = Vec::new();
    table.get_random_items_raw(&mut ctx, &mut |s| items.push(s));

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count(), 4);
}

#[test]
fn explosion_condition_and_decay_exact_rng() {
    let registry = LootRegistry::new();
    let mut params_builder = LootParams::builder();
    params_builder
        .with_parameter(
            LootContextParams::block_state(),
            ContextValue::BlockState(id("stone")),
        )
        .with_parameter(
            LootContextParams::origin(),
            ContextValue::Origin([0.0, 64.0, 0.0]),
        )
        .with_parameter(
            LootContextParams::tool(),
            ContextValue::Tool(LootItemStack::new(id("diamond_pickaxe"))),
        )
        .with_optional_parameter(
            LootContextParams::explosion_radius(),
            Some(ContextValue::Float(4.0)),
        );
    let params = params_builder
        .create(LootContextParamSets::block())
        .unwrap();

    let seed = 314_159;
    let mut ctx = make_context(params, seed, &registry);
    let mut oracle = LegacyRandomSource::new(seed);

    let survives = LootItemCondition::SurvivesExplosion.test(&mut ctx);
    assert_eq!(survives, oracle.next_float() <= 1.0_f32 / 4.0_f32);

    let decay = LootItemFunction::explosion_decay();
    let stack = LootItemStack::with_count(id("cobblestone"), 6);
    let decayed = decay.apply(stack, &mut ctx);

    let mut expected_count = 0;
    let prob = 1.0_f32 / 4.0_f32;
    for _ in 0..6 {
        if oracle.next_float() <= prob {
            expected_count += 1;
        }
    }
    assert_eq!(decayed.count(), expected_count);
    assert_eq!(ctx.random_mut().next_long(), oracle.next_long());
}

#[test]
fn apply_bonus_formulas_exact_rng() {
    let registry = LootRegistry::new();
    let fortune = id("fortune");
    let tool = LootItemStack::new(id("diamond_pickaxe")).with_enchantment(fortune.clone(), 3);
    let mut params_builder = LootParams::builder();
    params_builder
        .with_parameter(
            LootContextParams::block_state(),
            ContextValue::BlockState(id("diamond_ore")),
        )
        .with_parameter(
            LootContextParams::origin(),
            ContextValue::Origin([0.0, 12.0, 0.0]),
        )
        .with_parameter(LootContextParams::tool(), ContextValue::Tool(tool));
    let params = params_builder
        .create(LootContextParamSets::block())
        .unwrap();

    // OreDrops
    let seed = 8888;
    let mut ctx = make_context(params.clone(), seed, &registry);
    let mut oracle = LegacyRandomSource::new(seed);

    let ore_fn = LootItemFunction::apply_ore_drops(fortune.clone());
    let out = ore_fn.apply(LootItemStack::with_count(id("diamond"), 1), &mut ctx);
    let mut expected_i = oracle.next_int_bound(3 + 2) - 1;
    if expected_i < 0 {
        expected_i = 0;
    }
    assert_eq!(out.count(), expected_i + 1);
    assert_eq!(ctx.random_mut().next_long(), oracle.next_long());

    // BinomialWithBonusCount
    let mut ctx = make_context(params.clone(), seed, &registry);
    let mut oracle = LegacyRandomSource::new(seed);
    let bin_fn = LootItemFunction::apply_binomial_bonus(fortune.clone(), 0.571_428_6, 3);
    let out = bin_fn.apply(LootItemStack::with_count(id("wheat_seeds"), 1), &mut ctx);
    let mut expected = 1;
    for _ in 0..(3 + 3) {
        if oracle.next_float() < 0.571_428_6 {
            expected += 1;
        }
    }
    assert_eq!(out.count(), expected);
    assert_eq!(ctx.random_mut().next_long(), oracle.next_long());

    // UniformBonusCount
    let mut ctx = make_context(params, seed, &registry);
    let mut oracle = LegacyRandomSource::new(seed);
    let uni_fn = LootItemFunction::apply_uniform_bonus(fortune, 2);
    let out = uni_fn.apply(LootItemStack::with_count(id("glowstone_dust"), 2), &mut ctx);
    let expected = 2 + oracle.next_int_bound(2 * 3 + 1);
    assert_eq!(out.count(), expected);
    assert_eq!(ctx.random_mut().next_long(), oracle.next_long());
}

#[test]
fn table_bonus_clamps_enchantment_level_and_uses_exact_rng() {
    let registry = LootRegistry::new();
    let fortune = id("fortune");
    // Level 5 exceeds table length 3 -> clamps to index 2 (probability 0.5).
    let tool = LootItemStack::new(id("diamond_shovel")).with_enchantment(fortune.clone(), 5);
    let mut params_builder = LootParams::builder();
    params_builder
        .with_parameter(
            LootContextParams::block_state(),
            ContextValue::BlockState(id("gravel")),
        )
        .with_parameter(
            LootContextParams::origin(),
            ContextValue::Origin([0.0, 64.0, 0.0]),
        )
        .with_parameter(LootContextParams::tool(), ContextValue::Tool(tool));
    let params = params_builder
        .create(LootContextParamSets::block())
        .unwrap();

    let seed = 4242;
    let mut ctx = make_context(params, seed, &registry);
    let mut oracle = LegacyRandomSource::new(seed);

    let cond = LootItemCondition::table_bonus(fortune, vec![0.1, 0.25, 0.5]);
    assert_eq!(cond.test(&mut ctx), oracle.next_float() < 0.5);
    assert_eq!(ctx.random_mut().next_long(), oracle.next_long());
}

// ---------------------------------------------------------------------------
// 5. Recursion detection & missing references (controlled negatives)
// ---------------------------------------------------------------------------

#[test]
fn mutual_table_recursion_stops_cleanly_and_pops_breadcrumbs() {
    let mut registry = LootRegistry::new();
    let table_a = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(LootPoolEntryContainer::group(vec![
                    LootPoolEntryContainer::item(id("stick")),
                    LootPoolEntryContainer::loot_table_reference(id("table_b")),
                ]))
                .build(),
        )
        .build();
    let table_b = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(LootPoolEntryContainer::group(vec![
                    LootPoolEntryContainer::item(id("coal")),
                    LootPoolEntryContainer::loot_table_reference(id("table_a")),
                ]))
                .build(),
        )
        .build();

    registry.register_table(id("table_a"), table_a);
    registry.register_table(id("table_b"), table_b);

    let registered_a = registry.get_table(&id("table_a")).unwrap();
    let ptr_a = VisitedEntry::TablePtr((registered_a as *const LootTable) as usize);
    let ptr_b = VisitedEntry::TablePtr(
        (registry.get_table(&id("table_b")).unwrap() as *const LootTable) as usize,
    );

    let mut ctx = make_context(empty_params(), 123, &registry);
    let mut items = Vec::new();
    registered_a.get_random_items_raw(&mut ctx, &mut |s| {
        items.push(s.item().clone());
    });

    // Breadcrumbs popped cleanly: a second run succeeds and leaves no stale breadcrumbs.
    let mut items2 = Vec::new();
    registered_a.get_random_items_raw(&mut ctx, &mut |s| {
        items2.push(s.item().clone());
    });

    assert!(!ctx.has_visited_element(&ptr_a));
    assert!(!ctx.has_visited_element(&ptr_b));
    assert_eq!(items.len(), items2.len());
}

#[test]
fn self_referencing_registered_table_runs_productive_pool_only_once() {
    let mut registry = LootRegistry::new();
    let self_ref = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(LootPoolEntryContainer::item(id("gold_nugget")))
                .build(),
        )
        .with_pool(
            LootPool::builder()
                .add(LootPoolEntryContainer::loot_table_reference(id("self_ref")))
                .build(),
        )
        .build();
    registry.register_table(id("self_ref"), self_ref);

    let registered = registry.get_table(&id("self_ref")).unwrap();
    let items = registered.get_random_items_with_seed(empty_params(), &registry, 42);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item(), &id("gold_nugget"));
}

#[test]
fn condition_and_function_reference_recursion_and_missing_refs() {
    let mut registry = LootRegistry::new();
    registry.register_condition(
        id("self_cond"),
        LootItemCondition::Reference(id("self_cond")),
    );
    registry.register_function(id("self_fn"), LootItemFunction::reference(id("self_fn")));

    let mut ctx = make_context(empty_params(), 1, &registry);

    // Recursive condition -> false, breadcrumb popped.
    assert!(!LootItemCondition::Reference(id("self_cond")).test(&mut ctx));
    assert!(!ctx.has_visited_element(&VisitedEntry::ConditionId(id("self_cond"))));

    // Missing condition -> false.
    assert!(!LootItemCondition::Reference(id("missing_cond")).test(&mut ctx));

    // Recursive function -> identity, breadcrumb popped.
    let stack = LootItemStack::with_count(id("apple"), 3);
    let out = LootItemFunction::reference(id("self_fn")).apply(stack.clone(), &mut ctx);
    assert_eq!(out, stack);
    assert!(!ctx.has_visited_element(&VisitedEntry::FunctionId(id("self_fn"))));

    // Missing function -> identity.
    let out2 = LootItemFunction::reference(id("missing_fn")).apply(stack.clone(), &mut ctx);
    assert_eq!(out2, stack);

    // Missing nested table -> empty (no-op).
    let table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(LootPoolEntryContainer::loot_table_reference(id(
                    "missing_table",
                )))
                .build(),
        )
        .build();
    let mut items = Vec::new();
    table.get_random_items_raw(&mut ctx, &mut |s| items.push(s));
    assert!(items.is_empty());
}

// ---------------------------------------------------------------------------
// 6. Stack splitting, disabled items, seeded execution, and container fill
// ---------------------------------------------------------------------------

#[test]
fn public_stack_splitting_and_disabled_items() {
    let registry = LootRegistry::new();
    let table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(
                    LootPoolEntryContainer::item(id("ender_pearl"))
                        .apply(LootItemFunction::set_count(NumberProvider::constant(40.0))),
                )
                .build(),
        )
        .build();

    // Default max_stack_size is 64: 130 items -> [64, 64, 2]
    let big_table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(
                    LootPoolEntryContainer::item(id("cobblestone"))
                        .apply(LootItemFunction::set_count(NumberProvider::constant(130.0))),
                )
                .build(),
        )
        .build();

    let items = big_table.get_random_items_with_seed(empty_params(), &registry, 99);
    let counts: Vec<i32> = items.iter().map(LootItemStack::count).collect();
    assert_eq!(counts, vec![64, 64, 2]);

    // With split_overstacked_loot = false -> single stack of 130
    let mut unsplit_builder = LootParams::builder();
    unsplit_builder.with_split_overstacked_loot(false);
    let unsplit_params = unsplit_builder
        .create(LootContextParamSets::empty())
        .unwrap();
    let unsplit = big_table.get_random_items_with_seed(unsplit_params, &registry, 99);
    assert_eq!(unsplit.len(), 1);
    assert_eq!(unsplit[0].count(), 130);

    // Disabled item is omitted completely
    let mut disabled_builder = LootParams::builder();
    disabled_builder.with_disabled_item(id("ender_pearl"));
    let disabled_params = disabled_builder
        .create(LootContextParamSets::empty())
        .unwrap();
    let disabled = table.get_random_items_with_seed(disabled_params, &registry, 99);
    assert!(disabled.is_empty());
}

#[test]
fn container_fill_is_deterministic_with_seed_and_splits_stacks() {
    let registry = LootRegistry::new();
    let table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .set_rolls(NumberProvider::constant(3.0))
                .add(
                    LootPoolEntryContainer::item(id("iron_ingot"))
                        .apply(LootItemFunction::set_count(NumberProvider::constant(8.0))),
                )
                .build(),
        )
        .build();

    let mut chest_a = SimpleLootContainer::new(27);
    let mut chest_b = SimpleLootContainer::new(27);

    table.fill(&mut chest_a, empty_params(), &registry, 12345);
    table.fill(&mut chest_b, empty_params(), &registry, 12345);

    assert_eq!(chest_a.slots(), chest_b.slots());

    let total_iron: i32 = chest_a
        .slots()
        .iter()
        .filter(|s| !s.is_empty())
        .map(LootItemStack::count)
        .sum();
    assert_eq!(total_iron, 24);
}

#[test]
fn named_random_sequence_and_default_rng_advance_across_unseeded_rolls() {
    let mut registry = LootRegistry::new();
    registry.register_random_sequence(id("chests/simple"), RandomSequenceSeed::Legacy(777));

    let table = LootTable::builder()
        .set_random_sequence(id("chests/simple"))
        .with_pool(
            LootPool::builder()
                .set_rolls(NumberProvider::uniform_between(1.0, 5.0))
                .add(LootPoolEntryContainer::item(id("bread")))
                .build(),
        )
        .build();

    let items_a = table.get_random_items_with_seed(empty_params(), &registry, RANDOMIZE_SEED);
    let items_b = table.get_random_items_with_seed(empty_params(), &registry, RANDOMIZE_SEED);

    let mut oracle = LegacyRandomSource::new(777);
    let expected_a = mth::next_int(&mut oracle, 1, 5) as usize;
    let expected_b = mth::next_int(&mut oracle, 1, 5) as usize;
    assert_eq!(items_a.len(), expected_a);
    assert_eq!(items_b.len(), expected_b);

    // Without a named sequence, successive unseeded rolls also advance default_random.
    registry.set_default_random(LootRandom::Legacy(LegacyRandomSource::new(12345)));
    let unseq_table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .set_rolls(NumberProvider::uniform_between(1.0, 100.0))
                .add(LootPoolEntryContainer::item(id("apple")))
                .build(),
        )
        .build();
    let r1 = unseq_table.get_random_items_with_seed(empty_params(), &registry, RANDOMIZE_SEED);
    let r2 = unseq_table.get_random_items_with_seed(empty_params(), &registry, RANDOMIZE_SEED);
    let mut default_oracle = LegacyRandomSource::new(12345);
    assert_eq!(
        r1.len(),
        mth::next_int(&mut default_oracle, 1, 100) as usize
    );
    assert_eq!(
        r2.len(),
        mth::next_int(&mut default_oracle, 1, 100) as usize
    );
}

// ---------------------------------------------------------------------------
// 7. Validation & ProblemReporter
// ---------------------------------------------------------------------------

#[test]
fn validation_reports_missing_recursive_disallowed_params_and_unreachable() {
    let mut registry = LootRegistry::new();
    let cyclic = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(LootPoolEntryContainer::loot_table_reference(id("cyclic")))
                .add(LootPoolEntryContainer::loot_table_reference(id("missing")))
                .build(),
        )
        .build();
    registry.register_table(id("cyclic"), cyclic.clone());

    let collector = Rc::new(Collector::new());
    let vctx = ValidationContext::new(
        collector.clone(),
        LootContextParamSets::empty().clone(),
        Some(&registry),
    );
    vctx.validate_reference(LootReferenceKey::Table(id("cyclic")));

    let report = collector.get_report();
    assert!(
        report.contains("is recursively called"),
        "expected recursive problem, got:\n{report}"
    );
    assert!(
        report.contains("Missing element"),
        "expected missing reference problem, got:\n{report}"
    );

    // Without resolver -> ReferenceNotAllowedProblem
    let collector2 = Rc::new(Collector::new());
    let vctx_no_res = ValidationContext::without_resolver(
        collector2.clone(),
        LootContextParamSets::empty().clone(),
    );
    cyclic.validate(&vctx_no_res);
    assert!(
        collector2
            .get_report()
            .contains("references are not allowed"),
        "expected disallowed reference problem, got:\n{}",
        collector2.get_report()
    );

    // Unreachable alternative + missing context param
    let bad_table = LootTable::builder()
        .set_param_set(LootContextParamSets::empty().clone())
        .with_pool(
            LootPool::builder()
                .when(LootItemCondition::SurvivesExplosion) // needs explosion_radius
                .add(LootPoolEntryContainer::alternatives(vec![
                    LootPoolEntryContainer::item(id("always_matches")),
                    LootPoolEntryContainer::item(id("unreachable")),
                ]))
                .build(),
        )
        .build();

    let collector3 = Rc::new(Collector::new());
    let vctx3 = ValidationContext::without_resolver(
        collector3.clone(),
        LootContextParamSets::empty().clone(),
    );
    bad_table.validate(&vctx3);
    let report3 = collector3.get_report();
    assert!(
        report3.contains("Parameters") && report3.contains("are not provided"),
        "expected missing param problem, got:\n{report3}"
    );
    assert!(
        report3.contains("Unreachable"),
        "expected unreachable entry problem, got:\n{report3}"
    );
}

#[test]
fn context_map_builder_rejects_missing_required_and_disallowed_params() {
    // Block param set requires block_state, origin, tool.
    let err_missing = LootParams::builder()
        .create(LootContextParamSets::block())
        .unwrap_err();
    assert!(matches!(
        err_missing,
        ContextMapError::MissingRequired { .. }
    ));

    // Empty param set allows nothing.
    let mut disallowed_builder = LootParams::builder();
    disallowed_builder.with_parameter(LootContextParams::origin(), ContextValue::Origin([0.0; 3]));
    let err_disallowed = disallowed_builder
        .create(LootContextParamSets::empty())
        .unwrap_err();
    assert!(matches!(
        err_disallowed,
        ContextMapError::DisallowedParams { .. }
    ));
}

#[test]
fn empty_air_stack_returns_zero_count_and_does_not_hang_splitting() {
    let registry = LootRegistry::new();
    let air_table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(
                    LootPoolEntryContainer::item(id("air"))
                        .apply(LootItemFunction::set_count(NumberProvider::constant(64.0))),
                )
                .build(),
        )
        .build();

    let items = air_table.get_random_items_with_seed(empty_params(), &registry, 1);
    assert_eq!(items.len(), 1);
    assert!(items[0].is_empty());
    assert_eq!(items[0].count(), 0);
}

#[test]
fn custom_item_max_stack_size_splits_ender_pearls_into_16_16_8() {
    let mut registry = LootRegistry::new();
    registry.register_item_max_stack_size(id("ender_pearl"), 16);

    let table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(
                    LootPoolEntryContainer::item(id("ender_pearl"))
                        .apply(LootItemFunction::set_count(NumberProvider::constant(40.0))),
                )
                .build(),
        )
        .build();

    let items = table.get_random_items_with_seed(empty_params(), &registry, 99);
    let counts: Vec<i32> = items.iter().map(LootItemStack::count).collect();
    assert_eq!(counts, vec![16, 16, 8]);
}

#[test]
fn expanded_tag_entry_bypasses_entry_functions_matching_paper() {
    let mut registry = LootRegistry::new();
    registry.register_item_tag(id("single_gem"), vec![id("diamond")]);

    let table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(LootPoolEntryContainer::expand_tag(id("single_gem")).apply(
                    LootItemFunction::set_count(NumberProvider::uniform_between(2.0, 5.0)),
                ))
                .build(),
        )
        .build();

    let seed = 4242;
    let mut ctx = make_context(empty_params(), seed, &registry);
    let mut items = Vec::new();
    table.get_random_items_raw(&mut ctx, &mut |s| items.push(s));

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count(), 1);
    // Entry function was bypassed -> 0 RNG calls consumed.
    let mut oracle = LegacyRandomSource::new(seed);
    assert_eq!(ctx.random_mut().next_long(), oracle.next_long());
}

#[test]
fn single_child_group_propagates_failure_in_alternatives_and_sequence() {
    let registry = LootRegistry::new();
    let failing_group = LootPoolEntryContainer::group(vec![
        LootPoolEntryContainer::item(id("never"))
            .when(LootItemCondition::ParamPresent(LootContextParams::origin())),
    ]);

    // Inside Alternatives: single-child failing Group returns false -> fallback runs.
    let alt_table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(LootPoolEntryContainer::alternatives(vec![
                    failing_group.clone(),
                    LootPoolEntryContainer::item(id("fallback")),
                ]))
                .build(),
        )
        .build();
    let mut ctx = make_context(empty_params(), 1, &registry);
    let mut alt_items = Vec::new();
    alt_table.get_random_items_raw(&mut ctx, &mut |s| alt_items.push(s.item().clone()));
    assert_eq!(alt_items, vec![id("fallback")]);

    // Inside Sequence: single-child failing Group returns false -> stops sequence.
    let seq_table = LootTable::builder()
        .with_pool(
            LootPool::builder()
                .add(LootPoolEntryContainer::sequence(vec![
                    failing_group,
                    LootPoolEntryContainer::item(id("after")),
                ]))
                .build(),
        )
        .build();
    let mut ctx = make_context(empty_params(), 1, &registry);
    let mut seq_items = Vec::new();
    seq_table.get_random_items_raw(&mut ctx, &mut |s| seq_items.push(s.item().clone()));
    assert!(seq_items.is_empty());
}

#[test]
fn builtin_param_sets_match_paper_contracts() {
    let villager_trade = LootContextParamSets::get(&id("villager_trade")).unwrap();
    assert!(
        villager_trade
            .required()
            .contains(&LootContextParams::additional_cost_component_allowed())
    );

    let entity_interact = LootContextParamSets::get(&id("entity_interact")).unwrap();
    assert!(
        entity_interact
            .required()
            .contains(&LootContextParams::tool())
    );

    let generic = LootContextParamSets::all_params();
    assert!(
        generic
            .required()
            .contains(&LootContextParams::additional_cost_component_allowed())
    );
}

#[test]
fn survives_explosion_accepts_exact_boundary_draw() {
    let registry = LootRegistry::new();
    let mut params_builder = LootParams::builder();
    params_builder
        .with_parameter(
            LootContextParams::block_state(),
            ContextValue::BlockState(id("stone")),
        )
        .with_parameter(
            LootContextParams::origin(),
            ContextValue::Origin([0.0, 64.0, 0.0]),
        )
        .with_parameter(
            LootContextParams::tool(),
            ContextValue::Tool(LootItemStack::new(id("diamond_pickaxe"))),
        )
        .with_optional_parameter(
            LootContextParams::explosion_radius(),
            Some(ContextValue::Float(2.0)),
        );
    let params = params_builder
        .create(LootContextParamSets::block())
        .unwrap();

    // Seed whose first LegacyRandomSource::next_float() draw is exactly 0.5 == 1.0 / 2.0.
    let boundary_seed = 247_775_869_193_412_i64;
    let mut oracle = LegacyRandomSource::new(boundary_seed);
    assert_eq!(oracle.next_float(), 0.5_f32);

    let mut ctx = make_context(params, boundary_seed, &registry);
    assert!(LootItemCondition::SurvivesExplosion.test(&mut ctx));
}
