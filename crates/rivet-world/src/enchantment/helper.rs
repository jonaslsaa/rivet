//! Port of `net.minecraft.world.item.enchantment.EnchantmentHelper` and
//! `EnchantedItemInUse` (MC 26.2).
//!
//! Provides:
//! - Item enchantment lookup & mutation (`get_item_enchantment_level`,
//!   `update_enchantments`, `set_enchantments`, `has_enchantments`).
//! - Enchanting-table cost calculation & deterministic offer selection
//!   (`get_enchantment_cost`, `select_enchantment`,
//!   `get_available_enchantment_results`, `filter_compatible_enchantments`,
//!   `enchant_item`, `enchant_item_with_provider`).
//! - Exact attribute modifier traversal (`for_each_modifier`).
//! - Execution interfaces for future combat (`#23`) and item-use consumers
//!   (`modify_damage`, `get_damage_protection`, `has_damage_immunity`,
//!   `modify_knockback`, `modify_armor_effectiveness`, `modify_item_damage`,
//!   `modify_projectile_count`, `modify_projectile_spread`,
//!   `modify_projectile_piercing`, `modify_fishing_time_reduction`,
//!   `modify_fishing_luck_bonus`, `modify_crossbow_charge_time`,
//!   `collect_post_attack_effects`, `has_effect`, `has_tag`).

use rivet_protocol::game::clientbound_update_attributes_packet::AttributeModifier;
use rivet_registry::Identifier;
use rivet_util::mth;
use rivet_util::random::RandomSource;
use rivet_util::weighted::WeightedRandom;

use crate::enchantment::definition::{Enchantment, EnchantmentInstance};
use crate::enchantment::effects::{
    ConditionalEffect, EnchantmentEffectComponent, EnchantmentEntityEffect, EnchantmentEvalContext,
    EnchantmentValueEffect, java_round_f32,
};
use crate::enchantment::item_enchantments::{
    EnchantableItemStack, ItemEnchantments, MutableItemEnchantments,
};
use crate::enchantment::providers::{DifficultyContext, EnchantmentProvider};
use crate::enchantment::registry::EnchantmentRegistry;
use crate::enchantment::{EnchantmentTarget, EquipmentSlot};

/// `net.minecraft.world.item.enchantment.EnchantedItemInUse` — nullable slot
/// and owner ID, plus optional break-callback indicator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnchantedItemInUse {
    pub item: EnchantableItemStack,
    pub in_slot: Option<EquipmentSlot>,
    pub owner_entity_id: Option<i32>,
}

impl EnchantedItemInUse {
    pub fn new(
        item: EnchantableItemStack,
        in_slot: Option<EquipmentSlot>,
        owner_entity_id: Option<i32>,
    ) -> Self {
        Self {
            item,
            in_slot,
            owner_entity_id,
        }
    }
}

/// `EnchantmentHelper.getItemEnchantmentLevel(Holder<Enchantment>, ItemInstance)`.
pub fn get_item_enchantment_level(enchantment: &Identifier, item: &EnchantableItemStack) -> i32 {
    item.active_enchantments()
        .map_or(0, |e| e.get_level(enchantment))
}

/// `EnchantmentHelper.getEnchantmentsForCrafting(ItemStack)`.
pub fn get_enchantments_for_crafting(item: &EnchantableItemStack) -> ItemEnchantments {
    item.component_enchantments()
        .cloned()
        .unwrap_or_else(ItemEnchantments::empty)
}

/// `EnchantmentHelper.setEnchantments(ItemStack, ItemEnchantments)`.
pub fn set_enchantments(item: &mut EnchantableItemStack, enchantments: ItemEnchantments) {
    item.set_component_enchantments(enchantments);
}

/// `EnchantmentHelper.hasAnyEnchantments(ItemStack)`.
pub fn has_any_enchantments(item: &EnchantableItemStack) -> bool {
    item.component_enchantments().is_some_and(|e| !e.is_empty())
}

/// `EnchantmentHelper.updateEnchantments(ItemStack, Consumer<Mutable>, boolean createIfMissing)`.
pub fn update_enchantments(
    item: &mut EnchantableItemStack,
    consumer: impl FnOnce(&mut MutableItemEnchantments),
    create_component_if_missing: bool,
) -> ItemEnchantments {
    let old = match item.component_enchantments() {
        Some(existing) => existing.clone(),
        None if create_component_if_missing => ItemEnchantments::empty(),
        None => return ItemEnchantments::empty(),
    };

    let mut mutable = old.to_mutable();
    consumer(&mut mutable);
    let updated = mutable.to_immutable();
    item.set_component_enchantments(updated.clone());
    updated
}

/// `EnchantmentHelper.runIterationOnItem(ItemStack, EnchantmentVisitor)`.
///
/// Iterates the item's active enchantments in deterministic registered-name order.
pub fn run_iteration_on_item<'a>(
    item: &EnchantableItemStack,
    registry: &'a EnchantmentRegistry,
    mut visitor: impl FnMut(&'a Enchantment, i32),
) {
    let Some(enchantments) = item.active_enchantments() else {
        return;
    };
    for (id, level) in enchantments.entry_set() {
        if let Some(def) = registry.get(id) {
            visitor(def, level);
        }
    }
}

/// `EnchantmentHelper.runIterationOnItem(ItemStack, EquipmentSlot, LivingEntity, EnchantmentInSlotVisitor)`.
///
/// Skips empty items and enchantments whose slot groups do not match `slot`.
pub fn run_iteration_on_item_in_slot<'a>(
    item: &EnchantableItemStack,
    slot: EquipmentSlot,
    registry: &'a EnchantmentRegistry,
    mut visitor: impl FnMut(&'a Enchantment, i32),
) {
    if item.is_empty() {
        return;
    }
    let Some(enchantments) = item.active_enchantments() else {
        return;
    };
    if enchantments.is_empty() {
        return;
    }
    for (id, level) in enchantments.entry_set() {
        if let Some(def) = registry.get(id)
            && def.matching_slot(slot)
        {
            visitor(def, level);
        }
    }
}

/// `EnchantmentHelper.forEachModifier(ItemStack, EquipmentSlot, BiConsumer<Holder<Attribute>, AttributeModifier>)`.
///
/// Emits `(attribute_id, AttributeModifier)` in exact deterministic order for
/// every matching enchantment attribute effect on `item` for `slot`.
pub fn for_each_modifier(
    item: &EnchantableItemStack,
    slot: EquipmentSlot,
    registry: &EnchantmentRegistry,
    mut consumer: impl FnMut(&Identifier, AttributeModifier),
) {
    run_iteration_on_item(item, registry, |enchantment, level| {
        if enchantment.matching_slot(slot) {
            for effect in &enchantment.effects.attributes {
                consumer(&effect.attribute, effect.get_modifier(level, slot));
            }
        }
    });
}

/// `EnchantmentHelper.has(ItemStack, DataComponentType<?>)`.
pub fn has_effect(
    item: &EnchantableItemStack,
    component: EnchantmentEffectComponent,
    registry: &EnchantmentRegistry,
) -> bool {
    let mut found = false;
    run_iteration_on_item(item, registry, |enchantment, _| {
        if enchantment.effects.has(component) {
            found = true;
        }
    });
    found
}

/// `EnchantmentHelper.hasTag(ItemStack, TagKey<Enchantment>)`.
pub fn has_tag(
    item: &EnchantableItemStack,
    tag_name: &str,
    registry: &EnchantmentRegistry,
) -> bool {
    let Some(enchantments) = item.active_enchantments() else {
        return false;
    };
    enchantments
        .key_set()
        .any(|id| registry.is_in_tag(id, tag_name))
}

/// `EnchantmentHelper.getEnchantmentCost(RandomSource, int slot, int bookcases, ItemStack)`.
///
/// Exact Paper 26.2 enchanting-table slot cost formula.
pub fn get_enchantment_cost<R: RandomSource>(
    random: &mut R,
    slot: i32,
    mut bookcases: i32,
    item: &EnchantableItemStack,
) -> i32 {
    if item.enchantable.is_none() {
        return 0;
    }

    if bookcases > 15 {
        bookcases = 15;
    }

    let selected = random
        .next_int_bound(8)
        .wrapping_add(1)
        .wrapping_add(bookcases >> 1)
        .wrapping_add(random.next_int_bound(bookcases.wrapping_add(1)));

    if slot == 0 {
        (selected / 3).max(1)
    } else if slot == 1 {
        (selected.wrapping_mul(2) / 3).wrapping_add(1)
    } else {
        selected.max(bookcases.wrapping_mul(2))
    }
}

/// `EnchantmentHelper.isEnchantmentCompatible(Collection<Holder<Enchantment>>, Holder<Enchantment>)`.
pub fn is_enchantment_compatible<'a>(
    existing: impl IntoIterator<Item = &'a Identifier>,
    target: &Identifier,
    registry: &EnchantmentRegistry,
) -> bool {
    for ench in existing {
        if !registry.are_compatible(ench, target) {
            return false;
        }
    }
    true
}

/// `EnchantmentHelper.filterCompatibleEnchantments(List<EnchantmentInstance>, EnchantmentInstance)`.
pub fn filter_compatible_enchantments(
    candidates: &mut Vec<EnchantmentInstance>,
    selected: &EnchantmentInstance,
    registry: &EnchantmentRegistry,
) {
    candidates
        .retain(|candidate| registry.are_compatible(&selected.enchantment, &candidate.enchantment));
}

/// `EnchantmentHelper.getAvailableEnchantmentResults(int value, ItemStack, Stream<Holder<Enchantment>>)`.
///
/// Filters `source` to enchantments that are primary for `item` (or any
/// enchantment when `item` is `minecraft:book`), then scans levels from
/// `max_level` down to `min_level` and picks the first level whose
/// `[min_cost, max_cost]` range contains `value`.
pub fn get_available_enchantment_results(
    value: i32,
    item: &EnchantableItemStack,
    source: &[&Enchantment],
) -> Vec<EnchantmentInstance> {
    let mut results = Vec::new();
    let is_book = item.is_book();

    for &enchantment in source {
        if !(enchantment.is_primary_item(&item.item_id) || is_book) {
            continue;
        }
        let mut level = enchantment.max_level();
        while level >= enchantment.min_level() {
            if value >= enchantment.min_cost(level) && value <= enchantment.max_cost(level) {
                results.push(EnchantmentInstance::new(enchantment.id.clone(), level));
                break;
            }
            level = level.wrapping_sub(1);
        }
    }

    results
}

/// `EnchantmentHelper.selectEnchantment(RandomSource, ItemStack, int enchantmentCost, Stream<Holder<Enchantment>>)`.
///
/// Exact Paper 26.2 weighted selection loop with enchantability bonus,
/// triangular ±15% random span, Java `Math.round(float)`, and iterative
/// halving with symmetric compatibility filtering.
pub fn select_enchantment<R: RandomSource>(
    random: &mut R,
    item: &EnchantableItemStack,
    mut enchantment_cost: i32,
    source: &[&Enchantment],
) -> Vec<EnchantmentInstance> {
    let mut results = Vec::new();
    let Some(enchantable) = item.enchantable else {
        return results;
    };

    let bonus_bound = (enchantable.value() / 4).wrapping_add(1);
    enchantment_cost = enchantment_cost
        .wrapping_add(1)
        .wrapping_add(random.next_int_bound(bonus_bound))
        .wrapping_add(random.next_int_bound(bonus_bound));

    let random_span = (random.next_float() + random.next_float() - 1.0_f32) * 0.15_f32;
    let adjusted = (enchantment_cost as f32) + (enchantment_cost as f32) * random_span;
    enchantment_cost = mth::clamp(java_round_f32(adjusted), 1, i32::MAX);

    let mut enchantments = get_available_enchantment_results(enchantment_cost, item, source);

    if !enchantments.is_empty() {
        if let Some(first) =
            WeightedRandom::get_random_item_from_total(random, &enchantments, |inst| {
                source
                    .iter()
                    .find(|e| e.id == inst.enchantment)
                    .map_or(0, |e| e.weight())
            })
        {
            results.push(first);
        }

        while random.next_int_bound(50) <= enchantment_cost {
            if let Some(last) = results.last() {
                let last_def = source.iter().copied().find(|e| e.id == last.enchantment);
                enchantments.retain(|candidate| {
                    let cand_def = source
                        .iter()
                        .copied()
                        .find(|e| e.id == candidate.enchantment);
                    match (last_def, cand_def) {
                        (Some(a), Some(b)) => Enchantment::are_compatible(a, b),
                        _ => false,
                    }
                });
            }

            if enchantments.is_empty() {
                break;
            }

            if let Some(next) =
                WeightedRandom::get_random_item_from_total(random, &enchantments, |inst| {
                    source
                        .iter()
                        .find(|e| e.id == inst.enchantment)
                        .map_or(0, |e| e.weight())
                })
            {
                results.push(next);
            }
            enchantment_cost /= 2;
        }
    }

    results
}

/// `EnchantmentHelper.enchantItem(RandomSource, ItemStack, int, Stream<Holder<Enchantment>>)`.
///
/// Selects enchantments and applies them to `item`. If `item` is a plain
/// `minecraft:book`, transmutes it to `minecraft:enchanted_book` with
/// `stored_enchantments` populated (matching Paper's `ItemStack.transmuteCopy`).
pub fn enchant_item<R: RandomSource>(
    random: &mut R,
    item: &mut EnchantableItemStack,
    enchantment_cost: i32,
    source: &[&Enchantment],
) -> Vec<EnchantmentInstance> {
    let selected = select_enchantment(random, item, enchantment_cost, source);
    if item.is_book() {
        item.item_id = Identifier::with_default_namespace("enchanted_book");
        item.enchantments = None;
        item.stored_enchantments = Some(ItemEnchantments::empty());
    }
    update_enchantments(
        item,
        |mutable| {
            for inst in &selected {
                mutable.upgrade(inst.enchantment.clone(), inst.level);
            }
        },
        true,
    );
    selected
}

/// `EnchantmentHelper.enchantItemFromProvider(...)`.
pub fn enchant_item_from_provider<R: RandomSource>(
    item: &mut EnchantableItemStack,
    provider: &EnchantmentProvider,
    random: &mut R,
    difficulty: DifficultyContext,
    registry: &EnchantmentRegistry,
) -> ItemEnchantments {
    let snapshot = item.clone();
    update_enchantments(
        item,
        |mutable| {
            provider.enchant(&snapshot, mutable, random, difficulty, registry);
        },
        true,
    )
}

/// One enchanting-table slot offer (`EnchantmentMenu.slotsChanged` /
/// `org.bukkit.enchantments.EnchantmentOffer`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnchantingTableOffer {
    pub cost: i32,
    pub clue: Option<EnchantmentInstance>,
}

/// `EnchantmentMenu.getEnchantmentList(RegistryAccess, ItemStack, int slot, int enchantmentCost)`.
///
/// Seeds `random` with `(enchantment_seed.wrapping_add(slot)) as i64`, selects
/// enchantments from `#minecraft:in_enchanting_table`, and drops one random
/// enchantment when `item` is a plain `minecraft:book` with more than one
/// result.
pub fn get_enchanting_table_list<R: RandomSource>(
    random: &mut R,
    enchantment_seed: i32,
    item: &EnchantableItemStack,
    slot: i32,
    enchantment_cost: i32,
    registry: &EnchantmentRegistry,
) -> Vec<EnchantmentInstance> {
    random.set_seed(i64::from(enchantment_seed.wrapping_add(slot)));
    let source = registry.tag_members("minecraft:in_enchanting_table");
    if source.is_empty() {
        return Vec::new();
    }
    let mut list = select_enchantment(random, item, enchantment_cost, &source);
    if item.is_book() && list.len() > 1 {
        let remove_idx = random.next_int_bound(list.len() as i32) as usize;
        list.remove(remove_idx);
    }
    list
}

/// Generates all three enchanting-table slot offers matching
/// `EnchantmentMenu.slotsChanged` (MC 26.2).
pub fn generate_enchanting_table_offers<R: RandomSource>(
    random: &mut R,
    enchantment_seed: i32,
    bookcases: i32,
    item: &EnchantableItemStack,
    registry: &EnchantmentRegistry,
) -> [EnchantingTableOffer; 3] {
    random.set_seed(i64::from(enchantment_seed));
    let mut costs = [0_i32; 3];
    for (slot, cost) in costs.iter_mut().enumerate() {
        let mut c = get_enchantment_cost(random, slot as i32, bookcases, item);
        if c < (slot as i32) + 1 {
            c = 0;
        }
        *cost = c;
    }

    let mut offers = [
        EnchantingTableOffer {
            cost: costs[0],
            clue: None,
        },
        EnchantingTableOffer {
            cost: costs[1],
            clue: None,
        },
        EnchantingTableOffer {
            cost: costs[2],
            clue: None,
        },
    ];

    for (slot, offer) in offers.iter_mut().enumerate() {
        if offer.cost > 0 {
            let list = get_enchanting_table_list(
                random,
                enchantment_seed,
                item,
                slot as i32,
                offer.cost,
                registry,
            );
            if !list.is_empty() {
                let idx = random.next_int_bound(list.len() as i32) as usize;
                offer.clue = Some(list[idx].clone());
            }
        }
    }

    offers
}

// ============================================================================
// Execution interfaces for combat (#23) and item-use consumers
// ============================================================================

/// Evaluates a list of conditional value effects in order on `value`.
fn apply_conditional_value_effects<R: RandomSource>(
    effects: &[ConditionalEffect<EnchantmentValueEffect>],
    level: i32,
    ctx: &EnchantmentEvalContext,
    random: &mut R,
    mut value: f32,
) -> f32 {
    let mut level_ctx = ctx.clone();
    level_ctx.enchantment_level = level;
    for cond_effect in effects {
        if cond_effect.matches(&level_ctx, random) {
            value = cond_effect.effect.process(level, random, value);
        }
    }
    value
}

/// `EnchantmentHelper.modifyDamage(ServerLevel, ItemStack, Entity, DamageSource, float)`.
pub fn modify_damage<R: RandomSource>(
    weapon: &EnchantableItemStack,
    base_damage: f32,
    ctx: &EnchantmentEvalContext,
    random: &mut R,
    registry: &EnchantmentRegistry,
) -> f32 {
    let mut damage = base_damage;
    run_iteration_on_item(weapon, registry, |enchantment, level| {
        damage = apply_conditional_value_effects(
            &enchantment.effects.damage,
            level,
            ctx,
            random,
            damage,
        );
    });
    damage
}

/// `EnchantmentHelper.getDamageProtection(ServerLevel, LivingEntity, DamageSource)`
/// across an ordered slice of `(slot, item)` equipment pieces.
pub fn get_damage_protection<R: RandomSource>(
    equipment: &[(EquipmentSlot, &EnchantableItemStack)],
    ctx: &EnchantmentEvalContext,
    random: &mut R,
    registry: &EnchantmentRegistry,
) -> f32 {
    let mut protection = 0.0_f32;
    for &(slot, item) in equipment {
        run_iteration_on_item_in_slot(item, slot, registry, |enchantment, level| {
            protection = apply_conditional_value_effects(
                &enchantment.effects.damage_protection,
                level,
                ctx,
                random,
                protection,
            );
        });
    }
    protection
}

/// `EnchantmentHelper.isImmuneToDamage(ServerLevel, LivingEntity, DamageSource)`.
pub fn is_immune_to_damage<R: RandomSource>(
    equipment: &[(EquipmentSlot, &EnchantableItemStack)],
    ctx: &EnchantmentEvalContext,
    random: &mut R,
    registry: &EnchantmentRegistry,
) -> bool {
    let mut immune = false;
    for &(slot, item) in equipment {
        if immune {
            break;
        }
        run_iteration_on_item_in_slot(item, slot, registry, |enchantment, level| {
            let mut level_ctx = ctx.clone();
            level_ctx.enchantment_level = level;
            for cond in &enchantment.effects.damage_immunity {
                if cond.matches(&level_ctx, random) {
                    immune = true;
                }
            }
        });
    }
    immune
}

/// `EnchantmentHelper.modifyKnockback(ServerLevel, ItemStack, Entity, DamageSource, float)`.
pub fn modify_knockback<R: RandomSource>(
    weapon: &EnchantableItemStack,
    base_knockback: f32,
    ctx: &EnchantmentEvalContext,
    random: &mut R,
    registry: &EnchantmentRegistry,
) -> f32 {
    let mut knockback = base_knockback;
    run_iteration_on_item(weapon, registry, |enchantment, level| {
        knockback = apply_conditional_value_effects(
            &enchantment.effects.knockback,
            level,
            ctx,
            random,
            knockback,
        );
    });
    knockback
}

/// `EnchantmentHelper.modifyArmorEffectiveness(ServerLevel, ItemStack, Entity, DamageSource, float)`.
pub fn modify_armor_effectiveness<R: RandomSource>(
    weapon: &EnchantableItemStack,
    base_effectiveness: f32,
    ctx: &EnchantmentEvalContext,
    random: &mut R,
    registry: &EnchantmentRegistry,
) -> f32 {
    let mut eff = base_effectiveness;
    run_iteration_on_item(weapon, registry, |enchantment, level| {
        eff = apply_conditional_value_effects(
            &enchantment.effects.armor_effectiveness,
            level,
            ctx,
            random,
            eff,
        );
    });
    eff
}

/// `EnchantmentHelper.processDurabilityChange(ServerLevel, ItemStack, int)` —
/// evaluates `minecraft:item_damage` (e.g. Unbreaking's `remove_binomial`).
pub fn process_durability_change<R: RandomSource>(
    item: &EnchantableItemStack,
    amount: i32,
    ctx: &EnchantmentEvalContext,
    random: &mut R,
    registry: &EnchantmentRegistry,
) -> i32 {
    let mut modified = amount as f32;
    run_iteration_on_item(item, registry, |enchantment, level| {
        modified = apply_conditional_value_effects(
            &enchantment.effects.item_damage,
            level,
            ctx,
            random,
            modified,
        );
    });
    modified as i32
}

/// `EnchantmentHelper.processProjectileCount(ServerLevel, ItemStack, Entity, int)` —
/// clamped to `>= 0` (exact Paper `Math.max(0, modifiedCount.intValue())`).
pub fn process_projectile_count<R: RandomSource>(
    weapon: &EnchantableItemStack,
    base_count: i32,
    ctx: &EnchantmentEvalContext,
    random: &mut R,
    registry: &EnchantmentRegistry,
) -> i32 {
    let mut count = base_count as f32;
    run_iteration_on_item(weapon, registry, |enchantment, level| {
        count = apply_conditional_value_effects(
            &enchantment.effects.projectile_count,
            level,
            ctx,
            random,
            count,
        );
    });
    (count as i32).max(0)
}

/// `EnchantmentHelper.processProjectileSpread(ServerLevel, ItemStack, Entity, float)` —
/// clamped to `>= 0.0F` (exact Paper `Math.max(0.0F, modifiedAngle.floatValue())`).
pub fn process_projectile_spread<R: RandomSource>(
    weapon: &EnchantableItemStack,
    base_spread: f32,
    ctx: &EnchantmentEvalContext,
    random: &mut R,
    registry: &EnchantmentRegistry,
) -> f32 {
    let mut spread = base_spread;
    run_iteration_on_item(weapon, registry, |enchantment, level| {
        spread = apply_conditional_value_effects(
            &enchantment.effects.projectile_spread,
            level,
            ctx,
            random,
            spread,
        );
    });
    spread.max(0.0_f32)
}

/// `EnchantmentHelper.getPiercingCount(ServerLevel, ItemStack, ItemStack)`.
pub fn get_piercing_count<R: RandomSource>(
    weapon: &EnchantableItemStack,
    ctx: &EnchantmentEvalContext,
    random: &mut R,
    registry: &EnchantmentRegistry,
) -> i32 {
    let mut count = 0.0_f32;
    run_iteration_on_item(weapon, registry, |enchantment, level| {
        count = apply_conditional_value_effects(
            &enchantment.effects.projectile_piercing,
            level,
            ctx,
            random,
            count,
        );
    });
    (count as i32).max(0)
}

/// `EnchantmentHelper.modifyCrossbowChargeTime(ItemStack, LivingEntity, float)`.
pub fn modify_crossbow_charge_time<R: RandomSource>(
    weapon: &EnchantableItemStack,
    base_time: f32,
    random: &mut R,
    registry: &EnchantmentRegistry,
) -> f32 {
    let mut time = base_time;
    run_iteration_on_item(weapon, registry, |enchantment, level| {
        if let Some(ref effect) = enchantment.effects.crossbow_charge_time {
            time = effect.process(level, random, time);
        }
    });
    time.max(0.0_f32)
}

/// A matched `post_attack` effect ready for combat (`#23`) execution.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchedPostAttackEffect {
    pub enchantment_id: Identifier,
    pub level: i32,
    pub affected: EnchantmentTarget,
    pub effect: EnchantmentEntityEffect,
}

/// Collects matching `post_attack` entity effects for a given `enchanted_role`
/// (`Attacker` or `Victim`) in deterministic order, providing the execution
/// seam for combat issue `#23`.
pub fn collect_post_attack_effects<R: RandomSource>(
    equipment: &[(EquipmentSlot, &EnchantableItemStack)],
    enchanted_role: EnchantmentTarget,
    ctx: &EnchantmentEvalContext,
    random: &mut R,
    registry: &EnchantmentRegistry,
) -> Vec<MatchedPostAttackEffect> {
    let mut matched = Vec::new();
    for &(slot, item) in equipment {
        run_iteration_on_item_in_slot(item, slot, registry, |enchantment, level| {
            let mut level_ctx = ctx.clone();
            level_ctx.enchantment_level = level;
            for targeted in &enchantment.effects.post_attack {
                if targeted.enchanted == enchanted_role && targeted.matches(&level_ctx, random) {
                    matched.push(MatchedPostAttackEffect {
                        enchantment_id: enchantment.id.clone(),
                        level,
                        affected: targeted.affected,
                        effect: targeted.effect.clone(),
                    });
                }
            }
        });
    }
    matched
}
