use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use indexmap::IndexMap;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_protocol::game::clientbound_update_attributes_packet::{AttributeModifier, Operation};
use rivet_registry::generated::registries::POTION_BY_ID;
use rivet_text::ChatFormatting;

use super::*;

#[derive(Default)]
struct MockTarget {
    tick_count: i32,
    health: f32,
    max_health: f32,
    absorption: f32,
    undead: bool,
    player: bool,
    immune_to: Vec<MobEffectId>,
    cancel_ticks_for: Vec<MobEffectId>,
    exhaustion: f32,
    saturation_food: i32,
    magic_damage_taken: Vec<(f32, bool)>,
    wither_damage_taken: Vec<f32>,
    healed: Vec<f32>,
    modifiers: IndexMap<&'static str, Vec<AttributeModifier>>,
    sounds: Vec<&'static str>,
    wind_charged_kills: i32,
    weaving_kills: i32,
    oozing_kills: i32,
    infested_hurts: Vec<(i32, f32)>,
    raid_omen_triggered: bool,
}

impl MockTarget {
    fn new(health: f32, max_health: f32) -> Self {
        Self {
            health,
            max_health,
            ..Self::default()
        }
    }

    fn modifier_count(&self, attribute: &'static str) -> usize {
        self.modifiers.get(attribute).map_or(0, Vec::len)
    }

    fn attribute_value(&self, attribute: &'static str, base: f64, min: f64, max: f64) -> f64 {
        let mods = self
            .modifiers
            .get(attribute)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        calculate_attribute_value(base, mods, min, max)
    }
}

impl EffectTarget for MockTarget {
    fn tick_count(&self) -> i32 {
        self.tick_count
    }

    fn health(&self) -> f32 {
        self.health
    }

    fn max_health(&self) -> f32 {
        self.max_health
    }

    fn absorption_amount(&self) -> f32 {
        self.absorption
    }

    fn set_absorption_amount(&mut self, amount: f32) {
        self.absorption = amount.max(0.0);
    }

    fn is_inverted_heal_and_harm(&self) -> bool {
        self.undead
    }

    fn is_player(&self) -> bool {
        self.player
    }

    fn can_be_affected(&self, instance: &MobEffectInstance) -> bool {
        !self.immune_to.contains(&instance.effect())
    }

    fn heal(&mut self, amount: f32) {
        self.healed.push(amount);
        self.health = (self.health + amount).min(self.max_health);
    }

    fn hurt_magic(&mut self, amount: f32, indirect: bool) {
        self.magic_damage_taken.push((amount, indirect));
        self.health = (self.health - amount).max(0.0);
    }

    fn hurt_wither(&mut self, amount: f32) {
        self.wither_damage_taken.push(amount);
        self.health = (self.health - amount).max(0.0);
    }

    fn cause_food_exhaustion(&mut self, amount: f32) {
        self.exhaustion += amount;
    }

    fn eat_saturation(&mut self, food: i32, _saturation_modifier: f32) {
        self.saturation_food += food;
    }

    fn add_attribute_modifier(&mut self, attribute: &'static str, modifier: AttributeModifier) {
        let entry = self.modifiers.entry(attribute).or_default();
        entry.retain(|m| m.id != modifier.id);
        entry.push(modifier);
    }

    fn remove_attribute_modifier(&mut self, attribute: &'static str, modifier_id: &str) {
        if let Some(entry) = self.modifiers.get_mut(attribute) {
            entry.retain(|m| m.id.to_string() != modifier_id);
        }
    }

    fn on_sound(&mut self, sound: SoundTrigger) {
        self.sounds.push(sound.sound_id);
    }

    fn on_effect_tick_event(&mut self, effect: MobEffectId, _amplifier: i32) -> bool {
        !self.cancel_ticks_for.contains(&effect)
    }

    fn on_raid_omen_trigger(&mut self, _amplifier: i32) -> bool {
        self.raid_omen_triggered = true;
        false
    }

    fn on_wind_charged_killed(&mut self, _amplifier: i32) {
        self.wind_charged_kills += 1;
    }

    fn on_weaving_killed(&mut self, _amplifier: i32) {
        self.weaving_kills += 1;
    }

    fn on_oozing_killed(&mut self, _amplifier: i32) {
        self.oozing_kills += 1;
    }

    fn on_infested_hurt(&mut self, amplifier: i32, damage: f32) {
        self.infested_hurts.push((amplifier, damage));
    }
}

#[test]
fn registry_parity_mob_effects_and_potions() {
    assert_eq!(MobEffectId::ALL.len(), MobEffectId::COUNT);
    for (idx, effect) in MobEffectId::ALL.into_iter().enumerate() {
        assert_eq!(effect.id() as usize, idx);
        assert_eq!(MobEffectId::by_id(idx as i32), Some(effect));
        assert_eq!(MobEffectId::by_name(effect.identifier()), Some(effect));
        assert_eq!(MobEffectId::by_name(effect.path()), Some(effect));
        assert_eq!(effect.def().id, effect);
    }
    assert_eq!(MobEffectId::by_id(MobEffectId::COUNT as i32), None);
    assert_eq!(MobEffectId::by_name("minecraft:nonexistent"), None);

    // 1:1 parity with generated minecraft:potion registry
    assert_eq!(PotionId::COUNT, POTION_BY_ID.len());
    for (idx, potion) in PotionId::ALL.into_iter().enumerate() {
        assert_eq!(potion.id() as usize, idx);
        assert_eq!(potion.identifier(), POTION_BY_ID[idx]);
        assert_eq!(PotionId::by_id(idx as u16), Some(potion));
        assert_eq!(PotionId::by_name(POTION_BY_ID[idx]), Some(potion));
    }
}

#[test]
fn mob_effect_categories_particles_and_cadence() {
    assert_eq!(
        MobEffectCategory::Beneficial.tooltip_formatting(),
        ChatFormatting::Blue
    );
    assert_eq!(
        MobEffectCategory::Neutral.tooltip_formatting(),
        ChatFormatting::Blue
    );
    assert_eq!(
        MobEffectCategory::Harmful.tooltip_formatting(),
        ChatFormatting::Red
    );

    assert!(MobEffectId::InstantHealth.def().is_instantaneous());
    assert!(MobEffectId::InstantDamage.def().is_instantaneous());
    assert!(MobEffectId::Saturation.def().is_instantaneous());
    assert!(!MobEffectId::Regeneration.def().is_instantaneous());

    // Ambient vs non-ambient particle alpha
    let speed_def = MobEffectId::Speed.def();
    assert_eq!(
        speed_def.create_particle_options(false),
        EffectParticle::EntityEffect {
            argb: argb::color_with_alpha(255, speed_def.color),
        }
    );
    assert_eq!(
        speed_def.create_particle_options(true),
        EffectParticle::EntityEffect {
            argb: argb::color_with_alpha(AMBIENT_ALPHA, speed_def.color),
        }
    );
    assert_eq!(
        MobEffectId::Oozing.def().create_particle_options(false),
        EffectParticle::Simple("minecraft:item_slime")
    );

    // Cadence: Regeneration 50 >> amp, Poison 25 >> amp, Wither 40 >> amp
    let regen = MobEffectId::Regeneration.def();
    assert!(regen.should_apply_effect_tick_this_tick(50, 0));
    assert!(!regen.should_apply_effect_tick_this_tick(49, 0));
    assert!(regen.should_apply_effect_tick_this_tick(25, 1));
    assert!(regen.should_apply_effect_tick_this_tick(1, 6)); // 50 >> 6 == 0 -> every tick

    let poison = MobEffectId::Poison.def();
    assert!(poison.should_apply_effect_tick_this_tick(25, 0));
    assert!(!poison.should_apply_effect_tick_this_tick(24, 0));
    assert!(poison.should_apply_effect_tick_this_tick(12, 1));

    let wither = MobEffectId::Wither.def();
    assert!(wither.should_apply_effect_tick_this_tick(40, 0));
    assert!(!wither.should_apply_effect_tick_this_tick(39, 0));
    assert!(wither.should_apply_effect_tick_this_tick(20, 1));

    assert!(
        MobEffectId::RaidOmen
            .def()
            .should_apply_effect_tick_this_tick(1, 0)
    );
    assert!(
        !MobEffectId::RaidOmen
            .def()
            .should_apply_effect_tick_this_tick(2, 0)
    );
}

#[test]
fn attribute_calculation_exact_paper_order() {
    let mods = vec![
        AttributeModifier {
            id: rivet_registry::Identifier::parse("minecraft:m3"),
            amount: 0.5,
            operation: Operation::AddMultipliedTotal,
        },
        AttributeModifier {
            id: rivet_registry::Identifier::parse("minecraft:m1"),
            amount: 4.0,
            operation: Operation::AddValue,
        },
        AttributeModifier {
            id: rivet_registry::Identifier::parse("minecraft:m2"),
            amount: 0.25,
            operation: Operation::AddMultipliedBase,
        },
        AttributeModifier {
            id: rivet_registry::Identifier::parse("minecraft:m4"),
            amount: 0.2,
            operation: Operation::AddMultipliedTotal,
        },
    ];
    // Step 1: base = 10 + 4 = 14
    // Step 2: result = 14 + (14 * 0.25) = 17.5
    // Step 3: result = 17.5 * 1.5 * 1.2 = 31.5
    let val = calculate_attribute_value(10.0, &mods, 0.0, 100.0);
    assert!((val - 31.5).abs() < 1e-9, "got {val}");

    // Clamping and NaN handling
    assert_eq!(calculate_attribute_value(10.0, &mods, 0.0, 20.0), 20.0);
    assert_eq!(calculate_attribute_value(f64::NAN, &[], 1.0, 20.0), 1.0);
}

#[test]
fn instance_clamping_infinite_and_scaling() {
    let clamped_low = MobEffectInstance::with_duration_and_amplifier(MobEffectId::Speed, 100, -5);
    assert_eq!(clamped_low.amplifier(), MIN_AMPLIFIER);

    let clamped_high = MobEffectInstance::with_duration_and_amplifier(MobEffectId::Speed, 100, 999);
    assert_eq!(clamped_high.amplifier(), MAX_AMPLIFIER);

    let inf = MobEffectInstance::with_duration(MobEffectId::NightVision, INFINITE_DURATION);
    let finite = MobEffectInstance::with_duration(MobEffectId::NightVision, 1000);
    assert!(inf.is_infinite_duration());
    assert!(finite.is_shorter_duration_than(&inf));
    assert!(!inf.is_shorter_duration_than(&finite));
    assert!(!inf.is_shorter_duration_than(&inf));
    assert!(!inf.ends_within(10_000));
    assert!(finite.ends_within(1000));
    assert!(!finite.ends_within(999));

    // Scaling: finite -> max(floor(d * scale), 1); infinite & 0 unchanged
    assert_eq!(finite.with_scaled_duration(0.25).duration(), 250);
    assert_eq!(
        MobEffectInstance::with_duration(MobEffectId::Speed, 2)
            .with_scaled_duration(0.1)
            .duration(),
        1
    );
    assert_eq!(inf.with_scaled_duration(0.25).duration(), INFINITE_DURATION);
    assert_eq!(
        MobEffectInstance::with_duration(MobEffectId::Speed, 0)
            .with_scaled_duration(0.25)
            .duration(),
        0
    );
}

#[test]
fn instance_stacking_hidden_chain_and_mc_259832() {
    // Speed I for 200 ticks, upgraded by Speed II for 50 ticks -> Speed I saved as hidden
    let mut inst = MobEffectInstance::with_duration_and_amplifier(MobEffectId::Speed, 200, 0);
    let stronger = MobEffectInstance::with_duration_and_amplifier(MobEffectId::Speed, 50, 1);
    assert!(inst.update(&stronger));
    assert_eq!(inst.amplifier(), 1);
    assert_eq!(inst.duration(), 50);
    let hidden = inst.hidden_effect().expect("lower amp saved as hidden");
    assert_eq!(hidden.amplifier(), 0);
    assert_eq!(hidden.duration(), 200);

    // Stronger + longer discards weaker
    let mut inst2 = MobEffectInstance::with_duration_and_amplifier(MobEffectId::Speed, 100, 0);
    let stronger_longer =
        MobEffectInstance::with_duration_and_amplifier(MobEffectId::Speed, 200, 1);
    assert!(inst2.update(&stronger_longer));
    assert!(inst2.hidden_effect().is_none());

    // Equal amp + longer extends duration
    let mut inst3 = MobEffectInstance::with_duration_and_amplifier(MobEffectId::Speed, 100, 1);
    let same_longer = MobEffectInstance::with_duration_and_amplifier(MobEffectId::Speed, 300, 1);
    assert!(inst3.update(&same_longer));
    assert_eq!(inst3.duration(), 300);

    // Paper MC-259832: chained hidden effects that simultaneously hit 0 must all be skipped
    // until a non-zero hidden effect is reached (or none left).
    let leaf = MobEffectInstance::with_duration_and_amplifier(MobEffectId::Speed, 5, 0);
    let mid = MobEffectInstance::full(
        MobEffectId::Speed,
        1,
        1,
        false,
        true,
        true,
        Some(Box::new(leaf)),
    );
    let mut top = MobEffectInstance::full(
        MobEffectId::Speed,
        1,
        2,
        false,
        true,
        true,
        Some(Box::new(mid)),
    );
    top.tick_down_duration(); // top=0, mid=0, leaf=4
    assert!(top.downgrade_to_hidden_effect());
    assert_eq!(top.amplifier(), 0);
    assert_eq!(top.duration(), 4);
    assert!(top.hidden_effect().is_none());
}

#[test]
fn instance_equality_hashing_and_compare_to_cutoff() {
    let a = MobEffectInstance::full(
        MobEffectId::Speed,
        100,
        1,
        false,
        true,
        true,
        Some(Box::new(MobEffectInstance::with_duration(
            MobEffectId::Speed,
            500,
        ))),
    );
    let b = MobEffectInstance::with_duration_and_amplifier(MobEffectId::Speed, 100, 1);
    // equals & hashCode ignore hidden_effect
    assert_eq!(a, b);
    let hash_of = |x: &MobEffectInstance| {
        let mut h = DefaultHasher::new();
        x.hash(&mut h);
        h.finish()
    };
    assert_eq!(hash_of(&a), hash_of(&b));

    // compareTo 32147 cutoff: when both > 32147, duration is ignored and color breaks ties
    let long_a = MobEffectInstance::with_duration(MobEffectId::Speed, 40_000);
    let long_b = MobEffectInstance::with_duration(MobEffectId::Speed, 50_000);
    assert_eq!(long_a.compare_java(&long_b), std::cmp::Ordering::Equal);

    // Below cutoff, shorter duration sorts first
    let short_a = MobEffectInstance::with_duration(MobEffectId::Speed, 100);
    let short_b = MobEffectInstance::with_duration(MobEffectId::Speed, 200);
    assert_eq!(short_a.compare_java(&short_b), std::cmp::Ordering::Less);

    // Infinite duration sorts after finite duration below cutoff
    let inf = MobEffectInstance::with_duration(MobEffectId::Speed, INFINITE_DURATION);
    assert_eq!(short_a.compare_java(&inf), std::cmp::Ordering::Less);
}

#[test]
fn instance_blend_state_darkness() {
    // Darkness uses setBlendDuration(22) => blend_in=22, blend_out=22, blend_out_advance=22
    let mut inst = MobEffectInstance::with_duration(MobEffectId::Darkness, 24);
    assert_eq!(inst.blend_factor(1.0), 0.0);
    inst.tick_client(); // duration becomes 23 (> 22), ramps in by 1/22
    assert!((inst.blend_factor(1.0) - (1.0 / 22.0)).abs() < 1e-6);
    inst.tick_client(); // duration becomes 22 (<= 22), ramps back out to 0
    assert!(inst.blend_factor(1.0).abs() < 1e-6);
}

#[test]
fn instance_nbt_roundtrip_and_defaults() {
    let hidden = MobEffectInstance::with_duration_and_amplifier(MobEffectId::Speed, 400, 0);
    let original = MobEffectInstance::full(
        MobEffectId::Speed,
        120,
        200,
        true,
        false,
        true,
        Some(Box::new(hidden)),
    );
    let nbt = original.save_nbt();
    let loaded = MobEffectInstance::load_nbt(&nbt).expect("valid NBT");
    assert_eq!(loaded, original);
    assert_eq!(loaded.amplifier(), 200);
    assert_eq!(loaded.hidden_effect().unwrap().duration(), 400);

    // Default show_icon falls back to show_particles when omitted
    let mut minimal = CompoundTag::new();
    minimal.put_string("id", "minecraft:poison");
    minimal.put_boolean("show_particles", false);
    let parsed = MobEffectInstance::load_nbt(&minimal).unwrap();
    assert_eq!(parsed.amplifier(), 0);
    assert_eq!(parsed.duration(), 0);
    assert!(!parsed.is_ambient());
    assert!(!parsed.is_visible());
    assert!(!parsed.show_icon());
}

#[test]
fn runtime_tick_by_tick_regen_poison_wither_hunger_absorption() {
    // Regeneration heals 1.0 on duration % 50 == 0 while health < max_health
    let mut active = ActiveEffects::new();
    let mut target = MockTarget::new(10.0, 20.0);
    assert!(active.add_effect(
        MobEffectInstance::with_duration(MobEffectId::Regeneration, 100),
        &mut target,
        EffectUpdateCause::PotionDrink,
    ));
    for _ in 0..100 {
        active.tick_effects(&mut target);
    }
    // Ticks at duration 100 and 50 -> +2.0 HP
    assert_eq!(target.healed, vec![1.0, 1.0]);
    assert_eq!(target.health, 12.0);
    assert!(!active.has_effect(MobEffectId::Regeneration));

    // Poison damages 1.0 on duration % 25 == 0, stops at health <= 1.0 (never kills)
    let mut target = MockTarget::new(2.0, 20.0);
    assert!(active.add_effect(
        MobEffectInstance::with_duration(MobEffectId::Poison, 50),
        &mut target,
        EffectUpdateCause::PotionSplash,
    ));
    for _ in 0..50 {
        active.tick_effects(&mut target);
    }
    assert_eq!(target.health, 1.0);
    assert_eq!(target.magic_damage_taken.len(), 1);

    // Wither damages 1.0 on duration % 40 == 0 and CAN kill
    let mut target = MockTarget::new(1.0, 20.0);
    assert!(active.add_effect(
        MobEffectInstance::with_duration(MobEffectId::Wither, 40),
        &mut target,
        EffectUpdateCause::WitherRose,
    ));
    active.tick_effects(&mut target);
    assert_eq!(target.health, 0.0);
    assert_eq!(target.wither_damage_taken, vec![1.0]);

    // Hunger adds 0.005 * (amp + 1) exhaustion per tick for players only
    let mut active = ActiveEffects::new();
    let mut player = MockTarget::new(20.0, 20.0);
    player.player = true;
    active.add_effect(
        MobEffectInstance::with_duration_and_amplifier(MobEffectId::Hunger, 10, 1),
        &mut player,
        EffectUpdateCause::Food,
    );
    for _ in 0..10 {
        active.tick_effects(&mut player);
    }
    assert!((player.exhaustion - 0.1).abs() < 1e-6);

    // Absorption sets min absorption 4 * (1 + amp) on start, expires immediately if absorption == 0
    let mut active = ActiveEffects::new();
    let mut target = MockTarget::new(20.0, 20.0);
    active.add_effect(
        MobEffectInstance::with_duration_and_amplifier(MobEffectId::Absorption, 100, 1),
        &mut target,
        EffectUpdateCause::Food,
    );
    assert_eq!(target.absorption, 8.0);
    // Reapplying weaker Absorption refills depleted hearts via onEffectStarted even when update returns false
    target.set_absorption_amount(0.0);
    let changed = active.add_effect(
        MobEffectInstance::with_duration_and_amplifier(MobEffectId::Absorption, 50, 0),
        &mut target,
        EffectUpdateCause::Food,
    );
    assert!(!changed);
    assert_eq!(target.absorption, 4.0);
    target.set_absorption_amount(0.0);
    active.tick_effects(&mut target);
    assert!(!active.has_effect(MobEffectId::Absorption));

    // Hidden-effect promotion does NOT refill depleted Absorption hearts
    let mut active = ActiveEffects::new();
    let mut target = MockTarget::new(20.0, 20.0);
    active.add_effect(
        MobEffectInstance::with_duration_and_amplifier(MobEffectId::Absorption, 100, 0),
        &mut target,
        EffectUpdateCause::Food,
    );
    active.add_effect(
        MobEffectInstance::with_duration_and_amplifier(MobEffectId::Absorption, 1, 1),
        &mut target,
        EffectUpdateCause::Food,
    );
    assert_eq!(target.absorption, 8.0);
    // Keep 1.0 absorption so the tick doesn't remove Absorption, then let amp 1 expire and promote hidden amp 0
    target.set_absorption_amount(1.0);
    active.tick_effects(&mut target);
    assert_eq!(
        active
            .get_effect(MobEffectId::Absorption)
            .unwrap()
            .amplifier(),
        0
    );
    assert_eq!(target.absorption, 1.0);
}

#[test]
fn runtime_instant_effects_undead_inversion_and_immunity() {
    let mut normal = MockTarget::new(10.0, 20.0);
    ActiveEffects::apply_instantaneous_effect(
        MobEffectId::InstantHealth,
        0,
        1.0,
        false,
        &mut normal,
    );
    assert_eq!(normal.health, 14.0); // 4 << 0 = 4

    // Proximity scaling: (int)(0.5 * 6 + 0.5) = 3
    ActiveEffects::apply_instantaneous_effect(
        MobEffectId::InstantDamage,
        0,
        0.5,
        true,
        &mut normal,
    );
    assert_eq!(normal.magic_damage_taken, vec![(3.0, true)]);
    assert_eq!(normal.health, 11.0);

    // Undead inverts Instant Health and Instant Damage
    let mut undead = MockTarget::new(10.0, 20.0);
    undead.undead = true;
    ActiveEffects::apply_instantaneous_effect(
        MobEffectId::InstantHealth,
        0,
        1.0,
        false,
        &mut undead,
    );
    assert_eq!(undead.magic_damage_taken, vec![(6.0, false)]);
    assert_eq!(undead.health, 4.0);
    ActiveEffects::apply_instantaneous_effect(
        MobEffectId::InstantDamage,
        1,
        1.0,
        false,
        &mut undead,
    );
    assert_eq!(undead.healed, vec![8.0]); // 4 << 1 = 8
    assert_eq!(undead.health, 12.0);

    // Controlled negative case: spider-like immunity to Poison
    let mut active = ActiveEffects::new();
    let mut spider = MockTarget::new(16.0, 16.0);
    spider.immune_to.push(MobEffectId::Poison);
    assert!(!active.add_effect(
        MobEffectInstance::with_duration(MobEffectId::Poison, 100),
        &mut spider,
        EffectUpdateCause::PotionSplash,
    ));
    assert!(!active.has_effect(MobEffectId::Poison));

    // Paper EntityEffectTickEvent cancellation prevents instantaneous effects
    let mut cancelled = MockTarget::new(10.0, 20.0);
    cancelled.cancel_ticks_for.push(MobEffectId::InstantHealth);
    cancelled.cancel_ticks_for.push(MobEffectId::InstantDamage);
    cancelled.cancel_ticks_for.push(MobEffectId::Saturation);
    ActiveEffects::apply_instantaneous_effect(
        MobEffectId::InstantHealth,
        0,
        1.0,
        false,
        &mut cancelled,
    );
    ActiveEffects::apply_instantaneous_effect(
        MobEffectId::InstantDamage,
        0,
        1.0,
        false,
        &mut cancelled,
    );
    ActiveEffects::apply_instantaneous_effect(
        MobEffectId::Saturation,
        0,
        1.0,
        false,
        &mut cancelled,
    );
    assert_eq!(cancelled.health, 10.0);
    assert!(cancelled.healed.is_empty());
    assert!(cancelled.magic_damage_taken.is_empty());
    assert!(cancelled.saturation_eaten.is_empty());
}

#[test]
fn runtime_attributes_curing_sounds_and_hooks() {
    let mut active = ActiveEffects::new();
    let mut target = MockTarget::new(20.0, 20.0);

    // Speed I adds +20% movement_speed (AddMultipliedTotal)
    active.add_effect(
        MobEffectInstance::with_duration_and_amplifier(MobEffectId::Speed, 100, 0),
        &mut target,
        EffectUpdateCause::PotionDrink,
    );
    assert_eq!(target.modifier_count("minecraft:movement_speed"), 1);
    assert!(
        (target.attribute_value("minecraft:movement_speed", 0.1, 0.0, 1024.0) - 0.12).abs() < 1e-9
    );

    // Upgrade to Speed II for 2 ticks -> +40%, then expire back to hidden Speed I -> +20%
    active.add_effect(
        MobEffectInstance::with_duration_and_amplifier(MobEffectId::Speed, 2, 1),
        &mut target,
        EffectUpdateCause::PotionDrink,
    );
    assert!(
        (target.attribute_value("minecraft:movement_speed", 0.1, 0.0, 1024.0) - 0.14).abs() < 1e-9
    );
    active.tick_effects(&mut target);
    active.tick_effects(&mut target);
    assert_eq!(
        active.get_effect(MobEffectId::Speed).unwrap().amplifier(),
        0
    );
    assert!(
        (target.attribute_value("minecraft:movement_speed", 0.1, 0.0, 1024.0) - 0.12).abs() < 1e-9
    );

    // Sound on added (BadOmen)
    active.add_effect(
        MobEffectInstance::with_duration(MobEffectId::BadOmen, 100),
        &mut target,
        EffectUpdateCause::PatrolCaptain,
    );
    assert_eq!(target.sounds, vec!["minecraft:event.mob_effect.bad_omen"]);

    // Honey removes Poison only, leaving Speed & BadOmen intact
    active.add_effect(
        MobEffectInstance::with_duration(MobEffectId::Poison, 100),
        &mut target,
        EffectUpdateCause::PotionSplash,
    );
    assert!(active.remove_effects_consume(&[MobEffectId::Poison], &mut target));
    assert!(!active.has_effect(MobEffectId::Poison));
    assert!(active.has_effect(MobEffectId::Speed));

    // On-kill & on-hurt hooks (WindCharged, Infested)
    active.add_effect(
        MobEffectInstance::with_duration(MobEffectId::WindCharged, 100),
        &mut target,
        EffectUpdateCause::PotionSplash,
    );
    active.add_effect(
        MobEffectInstance::with_duration(MobEffectId::Infested, 100),
        &mut target,
        EffectUpdateCause::PotionSplash,
    );
    active.on_mob_hurt(&mut target, 4.0);
    assert_eq!(target.infested_hurts, vec![(0, 4.0)]);
    active.on_mob_removed(&mut target, EffectRemovalReason::Discarded);
    assert_eq!(target.wind_charged_kills, 0);
    active.on_mob_removed(&mut target, EffectRemovalReason::Killed);
    assert_eq!(target.wind_charged_kills, 1);

    // Milk clears all active effects and removes all attribute modifiers
    assert!(active.clear_all_effects_consume(&mut target));
    assert!(active.is_empty());
    assert_eq!(target.modifier_count("minecraft:movement_speed"), 0);
}

#[test]
fn runtime_nbt_and_util_queries() {
    let mut active = ActiveEffects::new();
    let mut target = MockTarget::new(20.0, 20.0);
    active.add_effect(
        MobEffectInstance::with_flags(MobEffectId::Haste, 200, 1, true, true),
        &mut target,
        EffectUpdateCause::Beacon,
    );
    active.add_effect(
        MobEffectInstance::with_flags(MobEffectId::BreathOfTheNautilus, 100, 0, true, false),
        &mut target,
        EffectUpdateCause::Nautilus,
    );
    active.tick_effects(&mut target);
    assert!(active.is_ambient_only());
    assert_eq!(active.cached_particles().len(), 1); // BreathOfTheNautilus was invisible

    assert!(MobEffectUtil::has_dig_speed(&active));
    assert_eq!(MobEffectUtil::get_dig_speed_amplification(&active), 1);
    assert!(MobEffectUtil::has_water_breathing(&active));
    assert!(!MobEffectUtil::should_effects_refill_air_supply(&active));

    assert_eq!(
        MobEffectUtil::format_duration(
            &MobEffectInstance::with_duration(MobEffectId::Speed, INFINITE_DURATION),
            1.0,
            20.0
        ),
        "effect.duration.infinite"
    );
    assert_eq!(MobEffectUtil::format_tick_duration(3600, 20.0), "03:00");
    assert_eq!(
        MobEffectUtil::format_tick_duration(72_000, 20.0),
        "01:00:00"
    );

    // ActiveEffects NBT round-trip on entity CompoundTag
    let mut entity_tag = CompoundTag::new();
    active.save_into_compound(&mut entity_tag);
    let restored = ActiveEffects::load_from_compound(&entity_tag);
    assert_eq!(restored.len(), 2);
    assert_eq!(
        restored.get_effect(MobEffectId::Haste).unwrap().amplifier(),
        1
    );
}

#[test]
fn potion_contents_color_composition_nbt_and_brewing() {
    assert_eq!(PotionContents::EMPTY.color(), BASE_POTION_COLOR);
    assert!(!PotionContents::EMPTY.has_effects());
    assert_eq!(
        PotionContents::EMPTY.translation_key("item.minecraft.potion.effect."),
        "item.minecraft.potion.effect.empty"
    );

    let swift = PotionContents::from_potion(PotionId::LongSwiftness);
    assert!(swift.is(PotionId::LongSwiftness));
    assert!(swift.has_effects());
    assert_eq!(
        swift.translation_key("item.minecraft.potion.effect."),
        "item.minecraft.potion.effect.swiftness"
    );

    // Adding a custom effect makes is(potion) false
    let mixed = swift.with_effect_added(MobEffectInstance::with_duration_and_amplifier(
        MobEffectId::Strength,
        600,
        1,
    ));
    assert!(!mixed.is(PotionId::LongSwiftness));
    assert_eq!(mixed.all_effects().len(), 2);

    // Weighted ARGB color: Speed (weight 1) + Strength II (weight 2)
    let c_speed = MobEffectId::Speed.def().color();
    let c_str = MobEffectId::Strength.def().color();
    let expected = argb::color_rgb(
        (argb::red(c_speed) + 2 * argb::red(c_str)) / 3,
        (argb::green(c_speed) + 2 * argb::green(c_str)) / 3,
        (argb::blue(c_speed) + 2 * argb::blue(c_str)) / 3,
    );
    assert_eq!(mixed.color(), expected);

    // NBT round-trip
    let mut with_meta = mixed.clone();
    with_meta.custom_color = Some(0x112233);
    with_meta.custom_name = Some("custom_elixir".to_string());
    let tag = with_meta.save_nbt();
    let reloaded = PotionContents::load_nbt(&tag);
    assert_eq!(reloaded, with_meta);
    assert_eq!(reloaded.color(), 0x112233);
    assert_eq!(
        reloaded.translation_key("item.minecraft.potion.effect."),
        "item.minecraft.potion.effect.custom_elixir"
    );

    // Applying a Healing + Swiftness potion splits instant vs timed effects
    let combo = PotionContents::from_potion(PotionId::Healing)
        .with_effect_added(MobEffectInstance::with_duration(MobEffectId::Speed, 400));
    let mut active = ActiveEffects::new();
    let mut target = MockTarget::new(10.0, 20.0);
    combo.apply_to_target(
        &mut active,
        &mut target,
        0.25,
        EffectUpdateCause::PotionSplash,
    );
    assert_eq!(target.health, 14.0);
    assert_eq!(
        active.get_effect(MobEffectId::Speed).unwrap().duration(),
        100
    );

    // Vanilla brewing graph
    let brewing = PotionBrewing::vanilla();
    let water = PotionContents::from_potion(PotionId::Water);
    let (item, awkward) = brewing
        .mix("minecraft:nether_wart", "minecraft:potion", &water)
        .unwrap();
    assert_eq!(item, "minecraft:potion");
    assert!(awkward.is(PotionId::Awkward));

    let (_, swiftness) = brewing
        .mix("minecraft:sugar", "minecraft:potion", &awkward)
        .unwrap();
    assert!(swiftness.is(PotionId::Swiftness));

    let (_, strong_swift) = brewing
        .mix("minecraft:glowstone_dust", "minecraft:potion", &swiftness)
        .unwrap();
    assert!(strong_swift.is(PotionId::StrongSwiftness));

    let (splash_item, splash_contents) = brewing
        .mix("minecraft:gunpowder", "minecraft:potion", &strong_swift)
        .unwrap();
    assert_eq!(splash_item, "minecraft:splash_potion");
    assert!(splash_contents.is(PotionId::StrongSwiftness));

    let (lingering_item, _) = brewing
        .mix("minecraft:dragon_breath", splash_item, &splash_contents)
        .unwrap();
    assert_eq!(lingering_item, "minecraft:lingering_potion");

    // Start mix also turns water into mundane
    let (_, mundane) = brewing
        .mix("minecraft:breeze_rod", "minecraft:potion", &water)
        .unwrap();
    assert!(mundane.is(PotionId::Mundane));

    let (_, wind) = brewing
        .mix("minecraft:breeze_rod", "minecraft:potion", &awkward)
        .unwrap();
    assert!(wind.is(PotionId::WindCharged));

    // Ingredients registered with addMix(AWKWARD, ...) in Paper do NOT brew water -> mundane
    for ingredient in [
        "minecraft:golden_carrot",
        "minecraft:turtle_helmet",
        "minecraft:pufferfish",
        "minecraft:phantom_membrane",
    ] {
        assert!(
            brewing
                .mix(ingredient, "minecraft:potion", &water)
                .is_none(),
            "{ingredient} must not brew water into mundane"
        );
        assert!(
            brewing
                .mix(ingredient, "minecraft:potion", &awkward)
                .is_some(),
            "{ingredient} must still brew awkward potion"
        );
    }

    // Brewing returns fresh PotionContents with only the resulting base potion (drops custom effects/color/name)
    let customized = PotionContents {
        potion: Some(PotionId::Swiftness),
        custom_color: Some(0x112233),
        custom_effects: vec![MobEffectInstance::with_duration(MobEffectId::Luck, 200)],
        custom_name: Some("custom_swift".to_string()),
    };
    let (splash_item_2, splash_brewed) = brewing
        .mix("minecraft:gunpowder", "minecraft:potion", &customized)
        .unwrap();
    assert_eq!(splash_item_2, "minecraft:splash_potion");
    assert_eq!(
        splash_brewed,
        PotionContents::from_potion(PotionId::Swiftness)
    );
    let (_, upgraded_brewed) = brewing
        .mix("minecraft:glowstone_dust", "minecraft:potion", &customized)
        .unwrap();
    assert_eq!(
        upgraded_brewed,
        PotionContents::from_potion(PotionId::StrongSwiftness)
    );
    // Container with no base potion cannot be brewed
    let custom_only = PotionContents::EMPTY
        .with_effect_added(MobEffectInstance::with_duration(MobEffectId::Luck, 200));
    assert!(
        brewing
            .mix("minecraft:gunpowder", "minecraft:potion", &custom_only)
            .is_none()
    );

    // Invalid container or ingredient -> None
    assert!(
        brewing
            .mix("minecraft:nether_wart", "minecraft:glass_bottle", &water)
            .is_none()
    );
}
