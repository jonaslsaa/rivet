//! Port of `net.minecraft.world.item.enchantment` (Paper / MC 26.2).
//!
//! Provides:
//! - [`LevelBasedValue`] — level-parameterized value expressions.
//! - [`EnchantmentValueEffect`], [`EnchantmentAttributeEffect`],
//!   [`EnchantmentEntityEffect`], [`ConditionalEffect`],
//!   [`TargetedConditionalEffect`], [`EnchantmentEffects`] — effect models and
//!   execution.
//! - [`Enchantment`], [`EnchantmentDefinition`], [`Cost`],
//!   [`EnchantmentInstance`], [`NamedHolderSet`] — definitions and symmetric
//!   compatibility.
//! - [`EnchantmentRegistry`] — data-driven registry decoded directly from
//!   `rivet-registry`'s generated MC 26.2 `SYNCHRONIZED_NBT` and tags.
//! - [`ItemEnchantments`], [`MutableItemEnchantments`], [`Enchantable`],
//!   [`Repairable`], [`EnchantableItemStack`] — ordered item enchantments and
//!   eligibility.
//! - [`EnchantmentProvider`], [`vanilla_providers`] — `single`, `by_cost`, and
//!   `by_cost_with_difficulty` providers.
//! - [`helper`] — `EnchantmentHelper` cost calculation, deterministic weighted
//!   offer selection, exact attribute modifier traversal, and execution seams
//!   for combat (`#23`).

pub mod definition;
pub mod effects;
pub mod helper;
pub mod item_enchantments;
pub mod level_based_value;
pub mod providers;
pub mod registry;

pub use definition::{
    Cost, Enchantment, EnchantmentDecodeError, EnchantmentDefinition, EnchantmentInstance,
    MAX_LEVEL, NamedHolderSet,
};
pub use effects::{
    ConditionalEffect, DamageImmunity, EffectCondition, EffectDecodeError,
    EnchantmentAttributeEffect, EnchantmentEffectComponent, EnchantmentEffects,
    EnchantmentEntityEffect, EnchantmentEvalContext, EnchantmentValueEffect,
    TargetedConditionalEffect,
};
pub use helper::{EnchantedItemInUse, EnchantingTableOffer, MatchedPostAttackEffect};
pub use item_enchantments::{
    Enchantable, EnchantableItemStack, InvalidEnchantableValue, InvalidEnchantmentLevel,
    ItemEnchantments, MutableItemEnchantments, Repairable,
};
pub use level_based_value::{LevelBasedValue, LevelBasedValueError};
pub use providers::{DifficultyContext, EnchantmentProvider, vanilla_providers};
pub use registry::{EnchantmentRegistry, EnchantmentRegistryError};

/// `net.minecraft.world.item.enchantment.EnchantmentTarget` (MC 26.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnchantmentTarget {
    Attacker,
    DamagingEntity,
    Victim,
}

impl EnchantmentTarget {
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Attacker => "attacker",
            Self::DamagingEntity => "damaging_entity",
            Self::Victim => "victim",
        }
    }

    pub fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "attacker" => Some(Self::Attacker),
            "damaging_entity" => Some(Self::DamagingEntity),
            "victim" => Some(Self::Victim),
            _ => None,
        }
    }

    /// `EnchantmentTarget.NON_DAMAGE_CODEC` — only `Attacker` and `Victim`.
    pub fn from_non_damage_name(name: &str) -> Option<Self> {
        match name {
            "attacker" => Some(Self::Attacker),
            "victim" => Some(Self::Victim),
            _ => None,
        }
    }
}

/// `net.minecraft.world.entity.EquipmentSlot` (MC 26.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipmentSlot {
    Mainhand,
    Offhand,
    Feet,
    Legs,
    Chest,
    Head,
    Body,
    Saddle,
}

impl EquipmentSlot {
    pub const ALL: [Self; 8] = [
        Self::Mainhand,
        Self::Offhand,
        Self::Feet,
        Self::Legs,
        Self::Chest,
        Self::Head,
        Self::Body,
        Self::Saddle,
    ];

    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Mainhand => "mainhand",
            Self::Offhand => "offhand",
            Self::Feet => "feet",
            Self::Legs => "legs",
            Self::Chest => "chest",
            Self::Head => "head",
            Self::Body => "body",
            Self::Saddle => "saddle",
        }
    }
}

/// `net.minecraft.world.entity.EquipmentSlotGroup` (MC 26.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipmentSlotGroup {
    Any,
    Mainhand,
    Offhand,
    Hand,
    Feet,
    Legs,
    Chest,
    Head,
    Armor,
    Body,
    Saddle,
}

impl EquipmentSlotGroup {
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Mainhand => "mainhand",
            Self::Offhand => "offhand",
            Self::Hand => "hand",
            Self::Feet => "feet",
            Self::Legs => "legs",
            Self::Chest => "chest",
            Self::Head => "head",
            Self::Armor => "armor",
            Self::Body => "body",
            Self::Saddle => "saddle",
        }
    }

    pub fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "any" => Some(Self::Any),
            "mainhand" => Some(Self::Mainhand),
            "offhand" => Some(Self::Offhand),
            "hand" => Some(Self::Hand),
            "feet" => Some(Self::Feet),
            "legs" => Some(Self::Legs),
            "chest" => Some(Self::Chest),
            "head" => Some(Self::Head),
            "armor" => Some(Self::Armor),
            "body" => Some(Self::Body),
            "saddle" => Some(Self::Saddle),
            _ => None,
        }
    }

    /// `EquipmentSlotGroup.test(EquipmentSlot)`.
    pub const fn test(self, slot: EquipmentSlot) -> bool {
        match self {
            Self::Any => true,
            Self::Mainhand => matches!(slot, EquipmentSlot::Mainhand),
            Self::Offhand => matches!(slot, EquipmentSlot::Offhand),
            Self::Hand => matches!(slot, EquipmentSlot::Mainhand | EquipmentSlot::Offhand),
            Self::Feet => matches!(slot, EquipmentSlot::Feet),
            Self::Legs => matches!(slot, EquipmentSlot::Legs),
            Self::Chest => matches!(slot, EquipmentSlot::Chest),
            Self::Head => matches!(slot, EquipmentSlot::Head),
            Self::Armor => matches!(
                slot,
                EquipmentSlot::Feet
                    | EquipmentSlot::Legs
                    | EquipmentSlot::Chest
                    | EquipmentSlot::Head
            ),
            Self::Body => matches!(slot, EquipmentSlot::Body),
            Self::Saddle => matches!(slot, EquipmentSlot::Saddle),
        }
    }

    /// Ordered slots matched by this group (mirrors Paper's `EquipmentSlotGroup.slots()`).
    pub const fn slots(self) -> &'static [EquipmentSlot] {
        match self {
            Self::Any => &EquipmentSlot::ALL,
            Self::Mainhand => &[EquipmentSlot::Mainhand],
            Self::Offhand => &[EquipmentSlot::Offhand],
            Self::Hand => &[EquipmentSlot::Mainhand, EquipmentSlot::Offhand],
            Self::Feet => &[EquipmentSlot::Feet],
            Self::Legs => &[EquipmentSlot::Legs],
            Self::Chest => &[EquipmentSlot::Chest],
            Self::Head => &[EquipmentSlot::Head],
            Self::Armor => &[
                EquipmentSlot::Feet,
                EquipmentSlot::Legs,
                EquipmentSlot::Chest,
                EquipmentSlot::Head,
            ],
            Self::Body => &[EquipmentSlot::Body],
            Self::Saddle => &[EquipmentSlot::Saddle],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_protocol::game::clientbound_update_attributes_packet::Operation;
    use rivet_registry::Identifier;
    use rivet_util::random::{LegacyRandomSource, XoroshiroRandomSource};

    fn id(path: &str) -> Identifier {
        Identifier::with_default_namespace(path)
    }

    #[test]
    fn vanilla_registry_decodes_all_43_enchantments_in_order() {
        let reg = EnchantmentRegistry::vanilla();
        assert_eq!(reg.len(), 43);
        assert_eq!(reg.get_by_index(0).unwrap().id, id("aqua_affinity"));
        assert_eq!(reg.get_by_index(42).unwrap().id, id("wind_burst"));

        let sharpness = reg.get(&id("sharpness")).unwrap();
        assert_eq!(sharpness.weight(), 10);
        assert_eq!(sharpness.max_level(), 5);
        assert_eq!(sharpness.min_cost(1), 1);
        assert_eq!(sharpness.min_cost(5), 45);
        assert_eq!(sharpness.max_cost(1), 21);
        assert_eq!(sharpness.max_cost(5), 65);
        assert!(sharpness.matching_slot(EquipmentSlot::Mainhand));
        assert!(!sharpness.matching_slot(EquipmentSlot::Head));
    }

    #[test]
    fn level_based_value_variants_match_paper() {
        assert_eq!(LevelBasedValue::constant(2.5).calculate(1), 2.5);
        assert_eq!(LevelBasedValue::constant(2.5).calculate(5), 2.5);

        let lin = LevelBasedValue::per_level_with_base(1.0, 0.5);
        assert_eq!(lin.calculate(1), 1.0);
        assert_eq!(lin.calculate(5), 3.0);

        let clamped = LevelBasedValue::clamped(LevelBasedValue::per_level(1.5), 2.0, 4.0);
        assert_eq!(clamped.calculate(1), 2.0);
        assert_eq!(clamped.calculate(2), 3.0);
        assert_eq!(clamped.calculate(5), 4.0);

        let frac_zero = LevelBasedValue::fraction(
            LevelBasedValue::constant(10.0),
            LevelBasedValue::constant(0.0),
        );
        assert_eq!(frac_zero.calculate(1), 0.0);

        let frac = LevelBasedValue::fraction(
            LevelBasedValue::constant(1.0),
            LevelBasedValue::per_level_with_base(2.0, 1.0),
        );
        assert!((frac.calculate(3) - 0.25).abs() < 1e-6);

        let sq = LevelBasedValue::levels_squared(1.0);
        assert_eq!(sq.calculate(1), 2.0);
        assert_eq!(sq.calculate(5), 26.0);

        let exp = LevelBasedValue::exponent(
            LevelBasedValue::constant(2.0),
            LevelBasedValue::per_level(1.0),
        );
        assert_eq!(exp.calculate(3), 8.0);

        let lookup = LevelBasedValue::lookup(vec![1.0, 3.0, 6.0], LevelBasedValue::per_level(10.0));
        assert_eq!(lookup.calculate(1), 1.0);
        assert_eq!(lookup.calculate(3), 6.0);
        assert_eq!(lookup.calculate(4), 40.0);
    }

    #[test]
    fn symmetric_compatibility_and_exclusive_sets() {
        let reg = EnchantmentRegistry::vanilla();

        // Same enchantment is never compatible with itself.
        assert!(!reg.are_compatible(&id("sharpness"), &id("sharpness")));

        // Damage exclusive set: sharpness vs smite vs bane_of_arthropods.
        assert!(!reg.are_compatible(&id("sharpness"), &id("smite")));
        assert!(!reg.are_compatible(&id("smite"), &id("sharpness")));
        assert!(!reg.are_compatible(&id("fortune"), &id("silk_touch")));
        assert!(!reg.are_compatible(&id("infinity"), &id("mending")));
        assert!(!reg.are_compatible(&id("mending"), &id("infinity")));
        assert!(!reg.are_compatible(&id("multishot"), &id("piercing")));
        assert!(!reg.are_compatible(&id("protection"), &id("blast_protection")));

        // Asymmetric exclusive_set in NBT (riptide declares exclusive_set/riptide = [loyalty, channeling]):
        // symmetric check must reject both directions.
        assert!(!reg.are_compatible(&id("riptide"), &id("loyalty")));
        assert!(!reg.are_compatible(&id("loyalty"), &id("riptide")));
        assert!(reg.are_compatible(&id("loyalty"), &id("channeling")));

        // Orthogonal enchantments are compatible.
        assert!(reg.are_compatible(&id("sharpness"), &id("unbreaking")));
        assert!(reg.are_compatible(&id("sharpness"), &id("looting")));
    }

    #[test]
    fn item_enchantments_ordering_clamping_and_negative_validation() {
        // Controlled negative: Enchantable rejects <= 0.
        assert_eq!(Enchantable::new(0), Err(InvalidEnchantableValue(0)));
        assert_eq!(Enchantable::new(-5), Err(InvalidEnchantableValue(-5)));
        assert_eq!(Enchantable::new(10).unwrap().value(), 10);

        // Controlled negative: ItemEnchantments rejects level > 255 or < 0.
        let mut bad = std::collections::BTreeMap::new();
        bad.insert(id("sharpness"), 256);
        assert!(ItemEnchantments::try_from_map(bad).is_err());

        let mut bad_neg = std::collections::BTreeMap::new();
        bad_neg.insert(id("sharpness"), -1);
        assert!(ItemEnchantments::try_from_map(bad_neg).is_err());

        // Ordering edge case: inserted out of order, iterated in deterministic registered-name order.
        let mut m = MutableItemEnchantments::default();
        m.set(id("unbreaking"), 3);
        m.set(id("fire_aspect"), 2);
        m.set(id("sharpness"), 300); // clamped to 255
        m.upgrade(id("unbreaking"), 1); // keeps max (3)
        m.upgrade(id("unbreaking"), 5); // upgrades to 5
        m.set(id("looting"), 0); // <= 0 removed / ignored
        let imm = m.to_immutable();

        let entries: Vec<(String, i32)> =
            imm.entry_set().map(|(k, v)| (k.to_string(), v)).collect();
        assert_eq!(
            entries,
            vec![
                ("minecraft:fire_aspect".to_owned(), 2),
                ("minecraft:sharpness".to_owned(), 255),
                ("minecraft:unbreaking".to_owned(), 5),
            ]
        );
    }

    #[test]
    fn deterministic_enchanting_table_cost_and_selection() {
        let reg = EnchantmentRegistry::vanilla();
        let table_pool = reg.tag_members("minecraft:in_enchanting_table");
        assert!(!table_pool.is_empty());

        let sword =
            EnchantableItemStack::new(id("diamond_sword"), Some(Enchantable::new(10).unwrap()));

        // Same seed produces identical costs and offers across repeated runs.
        let mut r1 = LegacyRandomSource::new(123456789);
        let mut r2 = LegacyRandomSource::new(123456789);
        let costs1 = [
            helper::get_enchantment_cost(&mut r1, 0, 15, &sword),
            helper::get_enchantment_cost(&mut r1, 1, 15, &sword),
            helper::get_enchantment_cost(&mut r1, 2, 15, &sword),
        ];
        let costs2 = [
            helper::get_enchantment_cost(&mut r2, 0, 15, &sword),
            helper::get_enchantment_cost(&mut r2, 1, 15, &sword),
            helper::get_enchantment_cost(&mut r2, 2, 15, &sword),
        ];
        assert_eq!(costs1, costs2);
        assert!(costs1[0] >= 1);
        assert!(costs1[2] >= 30);

        // Bookcases > 15 are clamped to 15.
        let mut r_clamp1 = LegacyRandomSource::new(999);
        let mut r_clamp2 = LegacyRandomSource::new(999);
        assert_eq!(
            helper::get_enchantment_cost(&mut r_clamp1, 2, 15, &sword),
            helper::get_enchantment_cost(&mut r_clamp2, 2, 50, &sword)
        );

        // Non-enchantable item yields cost 0 and empty selection without consuming RNG.
        let stick = EnchantableItemStack::new(id("stick"), None);
        let mut r_stick = LegacyRandomSource::new(42);
        assert_eq!(helper::get_enchantment_cost(&mut r_stick, 2, 15, &stick), 0);
        assert!(helper::select_enchantment(&mut r_stick, &stick, 30, &table_pool).is_empty());

        // Selection is deterministic and every pair of selected enchantments is compatible.
        let mut rs1 = XoroshiroRandomSource::new(0x5EED_CAFE);
        let mut rs2 = XoroshiroRandomSource::new(0x5EED_CAFE);
        let sel1 = helper::select_enchantment(&mut rs1, &sword, 30, &table_pool);
        let sel2 = helper::select_enchantment(&mut rs2, &sword, 30, &table_pool);
        assert_eq!(sel1, sel2);
        assert!(!sel1.is_empty());
        for i in 0..sel1.len() {
            let def_i = reg.get(&sel1[i].enchantment).unwrap();
            assert!(def_i.is_primary_item(&sword.item_id));
            assert!(sel1[i].level >= def_i.min_level() && sel1[i].level <= def_i.max_level());
            for j in (i + 1)..sel1.len() {
                assert!(reg.are_compatible(&sel1[i].enchantment, &sel1[j].enchantment));
            }
        }
    }

    #[test]
    fn book_enchanting_transmutes_to_enchanted_book() {
        let reg = EnchantmentRegistry::vanilla();
        let table_pool = reg.tag_members("minecraft:in_enchanting_table");
        let mut book = EnchantableItemStack::new(id("book"), Some(Enchantable::new(1).unwrap()));
        let mut rng = LegacyRandomSource::new(777);

        let selected = helper::enchant_item(&mut rng, &mut book, 30, &table_pool);
        assert!(!selected.is_empty());
        assert!(book.is_enchanted_book());
        assert!(book.enchantments.is_none());
        let stored = book.stored_enchantments.as_ref().unwrap();
        for inst in &selected {
            assert_eq!(stored.get_level(&inst.enchantment), inst.level);
        }
    }

    #[test]
    fn exact_attribute_modifier_application_and_slot_filtering() {
        let reg = EnchantmentRegistry::vanilla();
        let mut boots =
            EnchantableItemStack::new(id("diamond_boots"), Some(Enchantable::new(10).unwrap()));
        helper::update_enchantments(
            &mut boots,
            |m| {
                m.set(id("depth_strider"), 3);
                m.set(id("aqua_affinity"), 1); // head-only, must not fire on feet
            },
            true,
        );

        let mut feet_mods = Vec::new();
        helper::for_each_modifier(&boots, EquipmentSlot::Feet, reg, |attr, modifier| {
            feet_mods.push((attr.clone(), modifier));
        });
        assert_eq!(feet_mods.len(), 1);
        assert_eq!(feet_mods[0].0, id("water_movement_efficiency"));
        assert_eq!(feet_mods[0].1.id, id("enchantment.depth_strider/feet"));
        assert!((feet_mods[0].1.amount - 1.0).abs() < 1e-6);
        assert_eq!(feet_mods[0].1.operation, Operation::AddValue);

        // Wrong slot yields no depth_strider modifier, only aqua_affinity on Head.
        let mut head_mods = Vec::new();
        helper::for_each_modifier(&boots, EquipmentSlot::Head, reg, |attr, modifier| {
            head_mods.push((attr.clone(), modifier));
        });
        assert_eq!(head_mods.len(), 1);
        assert_eq!(head_mods[0].0, id("submerged_mining_speed"));
        assert_eq!(head_mods[0].1.id, id("enchantment.aqua_affinity/head"));
        assert_eq!(head_mods[0].1.operation, Operation::AddMultipliedTotal);
    }

    #[test]
    fn execution_seams_damage_protection_and_unbreaking() {
        let reg = EnchantmentRegistry::vanilla();
        let mut sword =
            EnchantableItemStack::new(id("diamond_sword"), Some(Enchantable::new(10).unwrap()));
        helper::update_enchantments(
            &mut sword,
            |m| {
                m.set(id("sharpness"), 5);
                m.set(id("smite"), 5);
                m.set(id("unbreaking"), 3);
            },
            true,
        );

        let mut rng = LegacyRandomSource::new(1);
        // Zombie is undead: Sharpness V (+3.0) + Smite V (+12.5) = +15.5 on base 7.0 -> 22.5.
        let undead_ctx = EnchantmentEvalContext {
            entity_type: Some(id("zombie")),
            ..Default::default()
        };
        let dmg_undead = helper::modify_damage(&sword, 7.0, &undead_ctx, &mut rng, reg);
        assert!((dmg_undead - 22.5).abs() < 1e-5, "got {dmg_undead}");

        // Cow is not undead: only Sharpness V (+3.0) applies -> 10.0.
        let cow_ctx = EnchantmentEvalContext {
            entity_type: Some(id("cow")),
            ..Default::default()
        };
        let dmg_cow = helper::modify_damage(&sword, 7.0, &cow_ctx, &mut rng, reg);
        assert!((dmg_cow - 10.0).abs() < 1e-5, "got {dmg_cow}");

        // RemoveBinomial exact Bernoulli vs Gaussian determinism.
        let unbreaking_effect = EnchantmentValueEffect::RemoveBinomial {
            chance: LevelBasedValue::constant(0.5),
        };
        let mut r_a = LegacyRandomSource::new(2026);
        let mut r_b = LegacyRandomSource::new(2026);
        assert_eq!(
            unbreaking_effect.process(1, &mut r_a, 10.0),
            unbreaking_effect.process(1, &mut r_b, 10.0)
        );
        // Large n > 128 triggers Gaussian branch deterministically.
        assert_eq!(
            unbreaking_effect.process(1, &mut r_a, 200.0),
            unbreaking_effect.process(1, &mut r_b, 200.0)
        );
    }

    #[test]
    fn vanilla_providers_apply_deterministically() {
        let reg = EnchantmentRegistry::vanilla();
        let mut crossbow =
            EnchantableItemStack::new(id("crossbow"), Some(Enchantable::new(1).unwrap()));
        let provider =
            vanilla_providers::get(vanilla_providers::RAID_PILLAGER_POST_WAVE_5).unwrap();
        let mut rng = LegacyRandomSource::new(99);
        let applied = helper::enchant_item_from_provider(
            &mut crossbow,
            &provider,
            &mut rng,
            DifficultyContext::new(0.5),
            reg,
        );
        assert_eq!(applied.get_level(&id("quick_charge")), 2);

        // Quick Charge II reduces crossbow charge time by 0.5s (1.25 -> 0.75).
        let charge = helper::modify_crossbow_charge_time(&crossbow, 1.25, &mut rng, reg);
        assert!((charge - 0.75).abs() < 1e-6);
    }

    #[test]
    fn enchanting_table_offers_are_deterministic_and_handle_non_enchantable() {
        let reg = EnchantmentRegistry::vanilla();
        let sword =
            EnchantableItemStack::new(id("diamond_sword"), Some(Enchantable::new(10).unwrap()));
        let mut r1 = LegacyRandomSource::new(0);
        let mut r2 = LegacyRandomSource::new(0);
        let offers1 = helper::generate_enchanting_table_offers(&mut r1, 1337, 15, &sword, reg);
        let offers2 = helper::generate_enchanting_table_offers(&mut r2, 1337, 15, &sword, reg);
        assert_eq!(offers1, offers2);
        assert!(offers1[2].cost >= 30);
        assert!(offers1.iter().all(|o| o.clue.is_some()));

        // Controlled negative: non-enchantable item yields zero-cost empty offers.
        let stick = EnchantableItemStack::new(id("stick"), None);
        let empty_offers = helper::generate_enchanting_table_offers(&mut r1, 1337, 15, &stick, reg);
        assert!(empty_offers.iter().all(|o| o.cost == 0 && o.clue.is_none()));
    }
}
