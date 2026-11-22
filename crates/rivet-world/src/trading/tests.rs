//! Paper-grounded unit tests for `rivet_world::trading`.

use rivet_nbt::{
    byte_tag::ByteTag, compound_tag::CompoundTag, int_tag::IntTag, string_tag::StringTag, tag::Tag,
};
use rivet_registry::identifier::Identifier;
use rivet_util::random::LegacyRandomSource;

use super::*;

fn id(path: &str) -> Identifier {
    Identifier::with_default_namespace(path)
}

fn stack(path: &str, count: i32) -> TradeItemStack {
    TradeItemStack::new(id(path), count)
}

fn cost(path: &str, count: i32) -> ItemCost {
    ItemCost::new(id(path), count)
}

#[test]
fn item_cost_matches_item_and_exact_components() {
    let pred = DataComponentExactPredicate::empty()
        .expect(
            id("custom_name"),
            Tag::String(StringTag {
                value: "Shiny".to_string(),
            }),
        )
        .expect(id("damage"), Tag::Int(IntTag { value: 3 }));
    let item_cost = ItemCost::with_components(id("iron_sword"), 1, pred);

    let matching = stack("iron_sword", 1).with_components([
        (
            id("custom_name"),
            Tag::String(StringTag {
                value: "Shiny".to_string(),
            }),
        ),
        (id("damage"), Tag::Int(IntTag { value: 3 })),
        (id("unbreakable"), Tag::Byte(ByteTag { value: 1 })),
    ]);
    let wrong_component = stack("iron_sword", 1).with_components([
        (
            id("custom_name"),
            Tag::String(StringTag {
                value: "Dull".to_string(),
            }),
        ),
        (id("damage"), Tag::Int(IntTag { value: 3 })),
    ]);
    let wrong_item = stack("diamond_sword", 1).with_components([
        (
            id("custom_name"),
            Tag::String(StringTag {
                value: "Shiny".to_string(),
            }),
        ),
        (id("damage"), Tag::Int(IntTag { value: 3 })),
    ]);

    assert!(item_cost.test(&matching));
    assert!(!item_cost.test(&wrong_component));
    assert!(!item_cost.test(&wrong_item));
    assert!(!item_cost.test(&TradeItemStack::empty()));

    // copy_with_count on a zero-count non-air stack must stay empty (ItemStack.copyWithCount)
    let depleted = stack("emerald", 0);
    assert!(depleted.is_empty());
    assert!(depleted.copy_with_count(1).is_empty());
}

#[test]
fn merchant_offer_modified_cost_and_clamping() {
    let mut offer = MerchantOffer::new(
        ItemCost::with_max_stack_size(id("emerald"), 10, 16),
        stack("diamond", 1),
        12,
        5,
        0.2,
    );

    // Base cost when demand=0, special_price=0
    assert_eq!(offer.cost_a().count(), 10);

    // Positive demand: floor(10 * 3 * 0.2) = 6 -> 10 + 6 = 16 (at max_stack_size)
    offer.demand = 3;
    assert_eq!(offer.cost_a().count(), 16);

    // Clamp upper bound to max_stack_size (16)
    offer.demand = 10;
    assert_eq!(offer.cost_a().count(), 16);

    // Negative demand does NOT lower price (Math.max(0, floor(...)))
    offer.demand = -5;
    assert_eq!(offer.cost_a().count(), 10);

    // Special price discount clamps lower bound to 1
    offer.set_special_price_diff(-50);
    assert_eq!(offer.cost_a().count(), 1);
}

#[test]
fn merchant_offer_demand_update_paper_config_toggle() {
    // uses = 2, max_uses = 12 -> delta = 2 - (12 - 2) = -8
    let mut vanilla_offer = MerchantOffer::new(cost("emerald", 4), stack("bread", 1), 12, 1, 0.05);
    vanilla_offer.uses = 2;
    vanilla_offer.update_demand(false);
    assert_eq!(vanilla_offer.demand(), -8);

    // Paper misc.preventNegativeVillagerDemand = true clamps demand >= 0
    let mut paper_offer = MerchantOffer::new(cost("emerald", 4), stack("bread", 1), 12, 1, 0.05);
    paper_offer.uses = 2;
    paper_offer.update_demand(true);
    assert_eq!(paper_offer.demand(), 0);

    // Exhausted offer: uses = 12, max_uses = 12 -> delta = 12 - 0 = +12
    paper_offer.uses = 12;
    paper_offer.update_demand(true);
    assert_eq!(paper_offer.demand(), 12);
}

#[test]
fn merchant_offer_satisfied_by_and_take_negatives() {
    let single_cost = MerchantOffer::new(cost("emerald", 5), stack("compass", 1), 12, 2, 0.05);

    // Negative: extra non-empty buyB rejected when offer has no costB
    assert!(!single_cost.satisfied_by(&stack("emerald", 5), &stack("stick", 1)));
    // Negative: insufficient count rejected
    assert!(!single_cost.satisfied_by(&stack("emerald", 4), &TradeItemStack::empty()));

    let mut buy_a = stack("emerald", 7);
    let mut buy_b = TradeItemStack::empty();
    assert!(single_cost.take(&mut buy_a, &mut buy_b));
    assert_eq!(buy_a.count(), 2);
    assert!(buy_b.is_empty());

    // Two-cost offer shrinks both slots
    let two_cost = MerchantOffer::with_cost_b(
        cost("emerald", 3),
        Some(cost("book", 1)),
        stack("enchanted_book", 1),
        0,
        12,
        10,
        0.2,
        0,
    );
    let mut slot_a = stack("emerald", 3);
    let mut slot_b = stack("book", 2);
    assert!(two_cost.take(&mut slot_a, &mut slot_b));
    assert!(slot_a.is_empty());
    assert_eq!(slot_b.count(), 1);
}

#[test]
fn merchant_offers_selection_hint_edge_ordering() {
    let offer_0 = MerchantOffer::new(cost("emerald", 1), stack("apple", 1), 12, 1, 0.05);
    let offer_1 = MerchantOffer::new(cost("emerald", 1), stack("bread", 1), 12, 1, 0.05);
    let offer_2 = MerchantOffer::new(cost("wheat", 20), stack("emerald", 1), 12, 1, 0.05);
    let offers = MerchantOffers::from_vec(vec![offer_0, offer_1, offer_2]);

    let em = stack("emerald", 1);
    let empty = TradeItemStack::empty();

    // selectionHint == 0 scans from index 0 (never short-circuits)
    assert_eq!(offers.get_recipe_index_for(&em, &empty, 0), Some(0));

    // selectionHint == 1 (> 0 and < len) selects offer 1 directly
    assert_eq!(offers.get_recipe_index_for(&em, &empty, 1), Some(1));

    // Negative edge case: valid selectionHint == 2 that does NOT match returns None
    // without falling back to offers 0 or 1!
    assert_eq!(offers.get_recipe_index_for(&em, &empty, 2), None);

    // Out-of-bounds selectionHint (== len or negative) falls back to ordered scan
    assert_eq!(offers.get_recipe_index_for(&em, &empty, 3), Some(0));
    assert_eq!(offers.get_recipe_index_for(&em, &empty, -1), Some(0));
}

#[test]
fn merchant_container_slot_promotion_swapped_orientation_and_cancellation() {
    let offer_0 = MerchantOffer::with_cost_b(
        cost("emerald", 4),
        Some(cost("book", 1)),
        stack("enchanted_book", 1),
        0,
        2,
        5,
        0.2,
        0,
    );
    let mut villager = VillagerMerchantState::new(MerchantOffers::from_vec(vec![offer_0]));
    let mut container = MerchantContainer::new();

    // Put payment in reversed order (book in slot 0, emerald in slot 1)
    container.payment_slot_0 = stack("book", 1);
    container.payment_slot_1 = stack("emerald", 8);
    container.update_sell_item(villager.offers());
    assert_eq!(container.active_offer_index(), Some(0));
    assert_eq!(container.result_slot, stack("enchanted_book", 1));
    assert_eq!(container.future_xp(), 5);

    // Controlled negative: cancelled Paper PlayerPurchaseEvent consumes nothing
    let cancelled = container.take_result(&mut villager, |_| false);
    assert!(cancelled.is_none());
    assert_eq!(container.payment_slot_0.count(), 1);
    assert_eq!(container.payment_slot_1.count(), 8);
    assert_eq!(villager.offers().get(0).unwrap().uses(), 0);

    // Accepted purchase consumes reversed slots, increments uses and villager XP
    let tx = container
        .take_result(&mut villager, |_| true)
        .expect("trade should succeed");
    assert_eq!(tx.offer_index, 0);
    assert_eq!(tx.result, stack("enchanted_book", 1));
    assert_eq!(tx.awarded_xp, 5);
    assert!(container.payment_slot_0.is_empty());
    assert_eq!(container.payment_slot_1.count(), 4);
    assert_eq!(villager.villager_xp(), 5);
    assert_eq!(villager.offers().get(0).unwrap().uses(), 1);
    // Result cleared because book slot is now empty
    assert!(container.result_slot.is_empty());

    // Single-cost offer with slot 0 empty promotes slot 1 as buyA
    let single = MerchantOffer::new(cost("emerald", 4), stack("bread", 1), 1, 2, 0.05);
    let mut trader =
        WanderingTraderMerchantState::new(MerchantOffers::from_vec(vec![single]), 48000);
    assert!(!trader.show_progress_bar());
    assert!(!trader.can_restock());

    let mut c2 = MerchantContainer::new();
    c2.payment_slot_1 = stack("emerald", 4);
    c2.update_sell_item(trader.offers());
    assert_eq!(c2.result_slot, stack("bread", 1));
    let tx2 = c2.take_result(&mut trader, |_| true).unwrap();
    assert_eq!(tx2.result, stack("bread", 1));
    assert!(c2.payment_slot_1.is_empty());
    // Offer is now out of stock -> cannot purchase again
    c2.payment_slot_0 = stack("emerald", 4);
    c2.update_sell_item(trader.offers());
    assert!(c2.result_slot.is_empty());
    assert!(c2.take_result(&mut trader, |_| true).is_none());
}

#[test]
fn villager_restock_schedule_and_special_prices() {
    let mut offer_normal = MerchantOffer::new(
        cost("emerald", 20),
        stack("diamond_chestplate", 1),
        3,
        15,
        0.2,
    );
    offer_normal.uses = 3;
    let mut offer_ignore =
        MerchantOffer::new(cost("emerald", 20), stack("diamond_boots", 1), 3, 15, 0.2);
    offer_ignore.ignore_discounts = true;

    let mut villager =
        VillagerMerchantState::new(MerchantOffers::from_vec(vec![offer_normal, offer_ignore]));

    // Reputation + Hero of the Village discounts skip ignore_discounts offers
    villager.update_special_prices(10, Some(1));
    // Reputation: -floor(10 * 0.2) = -2; Hero amp=1: modifier=0.3625, floor(0.3625*20)=7 -> -7 => total -9
    assert_eq!(villager.offers.get(0).unwrap().special_price_diff(), -9);
    assert_eq!(villager.offers.get(0).unwrap().cost_a().count(), 11);
    assert_eq!(villager.offers.get(1).unwrap().special_price_diff(), 0);
    assert_eq!(villager.offers.get(1).unwrap().cost_a().count(), 20);

    villager.reset_special_prices();
    assert_eq!(villager.offers.get(0).unwrap().special_price_diff(), 0);

    // Restock #1 allowed immediately when needs_to_restock
    assert!(villager.should_restock(1000, 1));
    villager.restock(1000, |_| true);
    assert_eq!(villager.number_of_restocks_today, 1);
    assert_eq!(villager.offers.get(0).unwrap().uses(), 0);
    assert_eq!(villager.offers.get(0).unwrap().demand(), 3);

    // Use again: blocked during 2400-tick cooldown (game_time <= 3400)
    villager.offers.get_mut(0).unwrap().uses = 1;
    assert!(!villager.should_restock(3400, 1));
    // Allowed once game_time > 3400
    assert!(villager.should_restock(3401, 1));
    villager.restock(3401, |_| true);
    assert_eq!(villager.number_of_restocks_today, 2);

    // Third restock on same day denied
    villager.offers.get_mut(0).unwrap().uses = 1;
    assert!(!villager.should_restock(7000, 1));

    // New day resets restocks today and allows restock again
    assert!(villager.should_restock(7100, 2));
    assert_eq!(villager.number_of_restocks_today, 0);

    // Day rollover with missed restocks runs catch_up_demand():
    // With 1 restock done today (1 missed), offer uses reset and demand updates once.
    villager.offers.get_mut(0).unwrap().uses = 2;
    villager.offers.get_mut(0).unwrap().demand = 5;
    villager.number_of_restocks_today = 1;
    villager.last_restock_game_time = 7100;
    // Day 3 triggers reset_number_of_restocks() -> catch_up_demand() (1 missed update):
    // uses reset to 0 first, then update_demand runs: 5 + 0 - (3 - 0) = 2.
    assert!(!villager.should_restock(7200, 3));
    assert_eq!(villager.number_of_restocks_today, 0);
    assert_eq!(villager.offers.get(0).unwrap().uses(), 0);
    assert_eq!(villager.offers.get(0).unwrap().demand(), 2);

    // Villager progression XP is awarded even when reward_exp == false
    villager.offers.get_mut(0).unwrap().reward_exp = false;
    let xp_before = villager.villager_xp();
    villager.notify_trade(0);
    assert_eq!(villager.villager_xp(), xp_before + 15);
}

#[test]
fn deterministic_trade_set_generation_and_validation() {
    let valid_a = VillagerTrade::new(
        TradeCost::new(id("emerald"), TradeNumberProvider::between(2.0, 4.0)),
        stack("coal", 15),
    );
    let valid_b = VillagerTrade::new(
        TradeCost::new(id("emerald"), TradeNumberProvider::exactly(3.0)),
        stack("iron_ingot", 4),
    );
    // Controlled negative: cost < 1 produces None
    let invalid_zero_cost = VillagerTrade::new(
        TradeCost::new(id("emerald"), TradeNumberProvider::exactly(0.0)),
        stack("gold_ingot", 1),
    );
    // Controlled negative: failing merchant predicate produces None
    let mut invalid_pred = VillagerTrade::new(
        TradeCost::new(id("emerald"), TradeNumberProvider::exactly(5.0)),
        stack("diamond", 1),
    );
    invalid_pred.merchant_predicate_passes = false;

    let set = TradeSet::new(
        vec![valid_a, valid_b, invalid_zero_cost, invalid_pred],
        TradeNumberProvider::exactly(2.0),
        false,
        Some(TradeSets::id("trade_set/armorer/level_1")),
    );

    let mut rng_1 = LegacyRandomSource::new(42);
    let mut offers_1 = MerchantOffers::new();
    add_offers_from_trade_set(&set, &mut offers_1, &mut rng_1, |_| true);

    let mut rng_2 = LegacyRandomSource::new(42);
    let mut offers_2 = MerchantOffers::new();
    add_offers_from_trade_set(&set, &mut offers_2, &mut rng_2, |_| true);

    assert_eq!(offers_1, offers_2);
    assert_eq!(offers_1.len(), 2);

    // Double trade price for enchanted result
    let mut enchanted_trade = VillagerTrade::new(
        TradeCost::new(id("emerald"), TradeNumberProvider::exactly(5.0)),
        stack("enchanted_book", 1),
    );
    enchanted_trade.additional_cost = 6;
    enchanted_trade.double_trade_price_enchanted = true;
    let mut rng_3 = LegacyRandomSource::new(7);
    let offer = enchanted_trade.get_offer(&mut rng_3).unwrap();
    assert_eq!(offer.item_cost_a().count(), 17); // 5 + (6 * 2)

    // Java Math.round(-0.5f) == 0 (ties toward +infinity), so additional_cost = 1 yields cost 1
    let mut negative_half_trade = VillagerTrade::new(
        TradeCost::new(id("emerald"), TradeNumberProvider::exactly(-0.5)),
        stack("bread", 1),
    );
    negative_half_trade.additional_cost = 1;
    let offer_neg_half = negative_half_trade.get_offer(&mut rng_3).unwrap();
    assert_eq!(offer_neg_half.item_cost_a().count(), 1);
    assert_eq!(TradeNumberProvider::exactly(-1.5).get_int(&mut rng_3), -1);
    assert_eq!(
        TradeNumberProvider::between(-0.5, -0.5).get_int(&mut rng_3),
        0
    );

    // TradeSets constants & defaults match TradeSets.java
    assert_eq!(
        TradeSets::WANDERING_TRADER_ORDER,
        [
            "wandering_trader/buying",
            "wandering_trader/uncommon",
            "wandering_trader/common"
        ]
    );
    assert_eq!(
        TradeSets::villager_level("librarian", 5),
        Some(id("librarian/level_5"))
    );
    assert_eq!(TradeSets::villager_level("librarian", 6), None);
    assert_eq!(
        TradeSets::default_trade_count("wandering_trader/common"),
        TradeNumberProvider::exactly(5.0)
    );
    assert_eq!(
        TradeSets::default_trade_count("librarian/level_5"),
        TradeNumberProvider::exactly(3.0)
    );
    assert_eq!(
        TradeSets::default_trade_count("farmer/level_1"),
        TradeNumberProvider::exactly(2.0)
    );
}

#[test]
fn nbt_round_trip_and_codec_defaults() {
    let mut offer = MerchantOffer::with_cost_b(
        ItemCost::with_components(
            id("emerald"),
            12,
            DataComponentExactPredicate::empty()
                .expect(id("custom_model_data"), Tag::Int(IntTag { value: 99 })),
        ),
        Some(cost("book", 1)),
        stack("enchanted_book", 1),
        3,
        12,
        15,
        0.2,
        4,
    );
    offer.set_special_price_diff(-2);
    offer.ignore_discounts = true;

    let mut villager = VillagerMerchantState::new(MerchantOffers::from_vec(vec![offer]));
    villager.villager_xp = 75;
    villager.last_restock_game_time = 24000;
    villager.number_of_restocks_today = 1;

    let mut nbt = CompoundTag::new();
    villager.save_to_nbt(&mut nbt);
    let decoded = VillagerMerchantState::load_from_nbt(&nbt).unwrap();
    assert_eq!(decoded, villager);

    // Wandering trader round-trip
    let trader = WanderingTraderMerchantState::new(
        MerchantOffers::from_vec(vec![MerchantOffer::new(
            cost("emerald", 1),
            stack("sand", 8),
            8,
            0,
            0.05,
        )]),
        48000,
    );
    let mut trader_nbt = CompoundTag::new();
    trader.save_to_nbt(&mut trader_nbt);
    let decoded_trader = WanderingTraderMerchantState::load_from_nbt(&trader_nbt).unwrap();
    assert_eq!(decoded_trader, trader);

    // Paper MerchantOffer.CODEC default values when optional fields are omitted
    let mut minimal = CompoundTag::new();
    minimal.put(
        "buy".to_string(),
        Tag::Compound(cost("emerald", 2).to_nbt()),
    );
    minimal.put(
        "sell".to_string(),
        Tag::Compound(stack("bread", 1).to_nbt()),
    );
    let default_offer = MerchantOffer::from_nbt(&minimal).unwrap();
    assert_eq!(default_offer.uses(), 0);
    assert_eq!(default_offer.max_uses(), 4);
    assert!(default_offer.should_reward_exp());
    assert_eq!(default_offer.special_price_diff(), 0);
    assert_eq!(default_offer.demand(), 0);
    assert_eq!(default_offer.price_multiplier(), 0.0);
    assert_eq!(default_offer.xp(), 1);
    assert!(!default_offer.ignore_discounts);

    // Custom stack limits survive reload via minecraft:max_stack_size component and item resolver
    let capped_cost = ItemCost::with_components(
        id("emerald"),
        10,
        DataComponentExactPredicate::empty()
            .expect(id("max_stack_size"), Tag::Int(IntTag { value: 16 })),
    );
    let mut capped_offer = MerchantOffer::new(capped_cost, stack("diamond", 1), 12, 5, 0.2);
    capped_offer.demand = 10;
    let reloaded_capped = MerchantOffer::from_nbt(&capped_offer.to_nbt()).unwrap();
    assert_eq!(
        reloaded_capped.item_cost_a().item_stack().max_stack_size(),
        16
    );
    assert_eq!(reloaded_capped.cost_a().count(), 16);

    let pearl_offer = MerchantOffer::new(
        ItemCost::with_max_stack_size(id("ender_pearl"), 10, 16),
        stack("emerald", 1),
        12,
        5,
        0.2,
    );
    let mut pearl_nbt = pearl_offer.to_nbt();
    pearl_nbt.put_int("demand", 10);
    let reloaded_pearl = MerchantOffer::from_nbt_with_item_max_stack(&pearl_nbt, |item| {
        if item == &id("ender_pearl") { 16 } else { 64 }
    })
    .unwrap();
    assert_eq!(
        reloaded_pearl.item_cost_a().item_stack().max_stack_size(),
        16
    );
    assert_eq!(reloaded_pearl.cost_a().count(), 16);

    // Removing minecraft:max_stack_size via with_components restores the base max stack size.
    let overridden = TradeItemStack::with_max_stack_size(id("ender_pearl"), 4, 16)
        .with_components([(id("max_stack_size"), Tag::Int(IntTag { value: 8 }))]);
    assert_eq!(overridden.max_stack_size(), 8);
    let restored = overridden.with_components([]);
    assert_eq!(restored.max_stack_size(), 16);
    assert_eq!(
        TradeItemStack::from_nbt_with_item_max_stack(&restored.to_nbt(), |item| {
            if item == &id("ender_pearl") { 16 } else { 64 }
        })
        .unwrap()
        .max_stack_size(),
        16
    );
}
