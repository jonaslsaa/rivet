//! Status-effect runtime (`LivingEntity` active-effect lifecycle, ticking,
//! instantaneous effects, curing/removal hooks, particles, and attribute modifier
//! updates).
//!
//! Mirrors Paper 26.2 `LivingEntity.java`, `MobEffect.java`, `HealOrHarmMobEffect.java`,
//! `RegenerationMobEffect.java`, `PoisonMobEffect.java`, `WitherMobEffect.java`,
//! `HungerMobEffect.java`, `SaturationMobEffect.java`, `AbsorptionMobEffect.java`,
//! `ClearAllStatusEffectsConsumeEffect.java`, and `RemoveStatusEffectsConsumeEffect.java`.

use indexmap::IndexMap;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::tag::Tag;
use rivet_protocol::game::clientbound_update_attributes_packet::AttributeModifier;

use crate::effect::{EffectParticle, MobEffectId, MobEffectInstance};

/// Cause of a status-effect mutation (mirrors Bukkit/Paper `EntityPotionEffectEvent.Cause`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectUpdateCause {
    Unknown,
    PotionDrink,
    PotionSplash,
    AreaEffectCloud,
    Arrow,
    Beacon,
    Conduit,
    Food,
    Milk,
    Honey,
    Totem,
    Attack,
    Expiration,
    Death,
    Command,
    Plugin,
    Conversion,
    WitherRose,
    TurtleHelmet,
    Dolphin,
    Axolotl,
    Warden,
    PatrolCaptain,
    Nautilus,
}

/// Reason an entity was removed from the world (`Entity.RemovalReason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectRemovalReason {
    Killed,
    Discarded,
    UnloadedToChunk,
    UnloadedWithPlayer,
    ChangedDimension,
}

/// Kind of removal reported for attribute/callback cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RemovalKind {
    Expired,
    Removed,
    Replaced,
}

/// Sound emitted when an effect is newly added (`MobEffect.onEffectAdded`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SoundTrigger {
    pub sound_id: &'static str,
}

/// Integration trait implemented by living entities (and test harnesses) so the
/// status-effect runtime can apply tick effects, instantaneous effects, attribute
/// modifiers, sounds, and species-specific hooks without depending on `rivet-server`
/// or concrete entity structs.
pub trait EffectTarget {
    /// Current entity tick counter (`Entity.tickCount`), used for infinite-duration effects.
    fn tick_count(&self) -> i32;

    /// Current health (`LivingEntity.getHealth()`).
    fn health(&self) -> f32;

    /// Maximum health (`LivingEntity.getMaxHealth()`).
    fn max_health(&self) -> f32;

    /// Current absorption amount (`LivingEntity.getAbsorptionAmount()`).
    fn absorption_amount(&self) -> f32;

    /// Set absorption amount (`LivingEntity.setAbsorptionAmount(float)`).
    fn set_absorption_amount(&mut self, amount: f32);

    /// Whether Instant Health and Instant Damage are inverted (`LivingEntity.isInvertedHealAndHarm()`,
    /// true for undead mobs).
    fn is_inverted_heal_and_harm(&self) -> bool {
        false
    }

    /// Whether this entity is a player (required for Hunger exhaustion and Saturation food gain).
    fn is_player(&self) -> bool {
        false
    }

    /// Whether this entity is alive (`LivingEntity.isAlive()`).
    fn is_alive(&self) -> bool {
        self.health() > 0.0
    }

    /// Whether this entity can be affected by `instance` (`LivingEntity.canBeAffected(MobEffectInstance)`).
    ///
    /// Species implementations override this (e.g. spiders immune to poison, undead immune to
    /// poison/regeneration, wither immune to wither, ender dragon immune to all).
    fn can_be_affected(&self, _instance: &MobEffectInstance) -> bool {
        true
    }

    /// Heal by `amount` (`LivingEntity.heal(float)`).
    fn heal(&mut self, amount: f32);

    /// Deal magic damage (`DamageSources.magic()` or `DamageSources.indirectMagic()`).
    fn hurt_magic(&mut self, amount: f32, indirect: bool);

    /// Deal wither damage (`DamageSources.wither()`).
    fn hurt_wither(&mut self, amount: f32);

    /// Add player food exhaustion (`Player.causeFoodExhaustion(float)`).
    fn cause_food_exhaustion(&mut self, _amount: f32) {}

    /// Apply saturation food gain (`FoodData.eat(int food, float saturationModifier)`).
    fn eat_saturation(&mut self, _food: i32, _saturation_modifier: f32) {}

    /// Add or replace an attribute modifier on `attribute` (`AttributeMap` / `AttributeInstance`).
    fn add_attribute_modifier(&mut self, attribute: &'static str, modifier: AttributeModifier);

    /// Remove an attribute modifier by ID from `attribute`.
    fn remove_attribute_modifier(&mut self, attribute: &'static str, modifier_id: &str);

    /// Optional hook when `MobEffect.onEffectAdded` plays a sound.
    fn on_sound(&mut self, _sound: SoundTrigger) {}

    /// Paper `EntityEffectTickEvent` hook; return `false` to cancel this tick's effect application.
    fn on_effect_tick_event(&mut self, _effect: MobEffectId, _amplifier: i32) -> bool {
        true
    }

    /// Hook for `BadOmenMobEffect.applyEffectTick` (level-dependent raid/trial omen transition).
    /// Returns `true` to keep Bad Omen active, `false` to remove it.
    fn on_bad_omen_tick(&mut self, _amplifier: i32) -> bool {
        true
    }

    /// Hook for `RaidOmenMobEffect.applyEffectTick` (fires when `duration == 1`).
    /// Returns `true` to keep active, `false` to expire.
    fn on_raid_omen_trigger(&mut self, _amplifier: i32) -> bool {
        false
    }

    /// Hook for `WindChargedMobEffect.onMobRemoved` on `EffectRemovalReason::Killed`.
    fn on_wind_charged_killed(&mut self, _amplifier: i32) {}

    /// Hook for `WeavingMobEffect.onMobRemoved` on `EffectRemovalReason::Killed`.
    fn on_weaving_killed(&mut self, _amplifier: i32) {}

    /// Hook for `OozingMobEffect.onMobRemoved` on `EffectRemovalReason::Killed`.
    fn on_oozing_killed(&mut self, _amplifier: i32) {}

    /// Hook for `InfestedMobEffect.onMobHurt`.
    fn on_infested_hurt(&mut self, _amplifier: i32, _damage: f32) {}
}

#[derive(Debug, Clone)]
enum DeferredAction {
    Add {
        instance: MobEffectInstance,
        cause: EffectUpdateCause,
        force: bool,
    },
    Remove {
        effect: MobEffectId,
        cause: EffectUpdateCause,
    },
    RemoveAll {
        cause: EffectUpdateCause,
    },
}

/// Active status-effect container mirroring `LivingEntity`'s `activeEffects` map and
/// tick/update lifecycle.
///
/// Uses [`IndexMap`] to preserve Java `LinkedHashMap` / deterministic observable iteration order.
#[derive(Debug, Clone, Default)]
pub struct ActiveEffects {
    effects: IndexMap<MobEffectId, MobEffectInstance>,
    effects_dirty: bool,
    is_ticking_effects: bool,
    effects_to_process: Vec<DeferredAction>,
    cached_particles: Vec<EffectParticle>,
    cached_ambient: bool,
}

impl ActiveEffects {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of active effects.
    #[inline]
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Whether there are no active effects.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// `LivingEntity.hasEffect(Holder<MobEffect>)`.
    #[inline]
    pub fn has_effect(&self, effect: MobEffectId) -> bool {
        self.effects.contains_key(&effect)
    }

    /// `LivingEntity.getEffect(Holder<MobEffect>)`.
    #[inline]
    pub fn get_effect(&self, effect: MobEffectId) -> Option<&MobEffectInstance> {
        self.effects.get(&effect)
    }

    /// Iterate over all active effects in deterministic insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &MobEffectInstance> {
        self.effects.values()
    }

    /// Read-only access to the underlying ordered map.
    pub fn as_map(&self) -> &IndexMap<MobEffectId, MobEffectInstance> {
        &self.effects
    }

    /// Cached visible particles updated by `update_invisibility_status` (`DATA_EFFECT_PARTICLES`).
    #[inline]
    pub fn cached_particles(&self) -> &[EffectParticle] {
        &self.cached_particles
    }

    /// Cached ambient flag updated by `update_invisibility_status` (`DATA_EFFECT_AMBIENCE_ID`).
    #[inline]
    pub fn is_ambient_only(&self) -> bool {
        self.cached_ambient
    }

    /// Whether `update_invisibility_status` needs to refresh cached particle/ambient metadata.
    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.effects_dirty
    }

    /// `LivingEntity.areAllEffectsAmbient(Collection<MobEffectInstance>)`.
    ///
    /// Returns `true` if every visible effect in `effects` is ambient (including when none are visible).
    pub fn are_all_effects_ambient<'a>(
        effects: impl IntoIterator<Item = &'a MobEffectInstance>,
    ) -> bool {
        for effect in effects {
            if effect.is_visible() && !effect.is_ambient() {
                return false;
            }
        }
        true
    }

    /// `LivingEntity.addEffect(MobEffectInstance, Entity, Cause)`.
    ///
    /// Returns `true` if the effect was added or updated (or queued during ticking).
    pub fn add_effect(
        &mut self,
        instance: MobEffectInstance,
        target: &mut dyn EffectTarget,
        cause: EffectUpdateCause,
    ) -> bool {
        if self.is_ticking_effects {
            self.effects_to_process.push(DeferredAction::Add {
                instance,
                cause,
                force: false,
            });
            return true;
        }

        if !target.can_be_affected(&instance) {
            return false;
        }

        let effect_id = instance.effect();
        let incoming_amp = instance.amplifier();
        let changed = if let Some(existing) = self.effects.get_mut(&effect_id) {
            let upgraded = existing.update(&instance);
            if upgraded {
                let updated_inst = existing.clone();
                Self::on_effect_updated(&updated_inst, true, target);
                self.effects_dirty = true;
            }
            upgraded
        } else {
            let added = instance.clone();
            self.effects.insert(effect_id, instance);
            Self::on_effect_added(&added, target);
            self.effects_dirty = true;
            true
        };
        Self::on_effect_started(effect_id, incoming_amp, target);
        changed
    }

    /// `LivingEntity.forceAddEffect(MobEffectInstance, Entity, Cause)` — replaces any
    /// existing instance of the same effect unconditionally if `can_be_affected` permits.
    pub fn force_add_effect(
        &mut self,
        instance: MobEffectInstance,
        target: &mut dyn EffectTarget,
        cause: EffectUpdateCause,
    ) {
        if self.is_ticking_effects {
            self.effects_to_process.push(DeferredAction::Add {
                instance,
                cause,
                force: true,
            });
            return;
        }

        if !target.can_be_affected(&instance) {
            return;
        }

        let effect_id = instance.effect();
        let added = instance.clone();
        let previous = self.effects.insert(effect_id, instance);
        if let Some(prev) = previous {
            Self::remove_attribute_modifiers(prev.effect(), target);
            Self::on_effect_updated(&added, true, target);
        } else {
            Self::on_effect_added(&added, target);
        }
        self.effects_dirty = true;
    }

    /// `LivingEntity.removeEffect(Holder<MobEffect>, Cause)`.
    ///
    /// Returns `true` if the effect was present and removed (or queued during ticking).
    pub fn remove_effect(
        &mut self,
        effect: MobEffectId,
        target: &mut dyn EffectTarget,
        cause: EffectUpdateCause,
    ) -> bool {
        if self.is_ticking_effects {
            self.effects_to_process
                .push(DeferredAction::Remove { effect, cause });
            return self.effects.contains_key(&effect);
        }

        if let Some(removed) = self.effects.shift_remove(&effect) {
            Self::on_effects_removed(&[removed], target);
            self.effects_dirty = true;
            true
        } else {
            false
        }
    }

    /// `LivingEntity.removeAllEffects(Cause)` — removes all active effects.
    ///
    /// Returns `true` if at least one effect was removed.
    pub fn remove_all_effects(
        &mut self,
        target: &mut dyn EffectTarget,
        cause: EffectUpdateCause,
    ) -> bool {
        if self.is_ticking_effects {
            self.effects_to_process
                .push(DeferredAction::RemoveAll { cause });
            return !self.effects.is_empty();
        }

        if self.effects.is_empty() {
            return false;
        }

        let removed: Vec<MobEffectInstance> = self.effects.drain(..).map(|(_, v)| v).collect();
        Self::on_effects_removed(&removed, target);
        self.effects_dirty = true;
        true
    }

    /// `ClearAllStatusEffectsConsumeEffect.apply(...)` (e.g. Milk Bucket).
    pub fn clear_all_effects_consume(&mut self, target: &mut dyn EffectTarget) -> bool {
        self.remove_all_effects(target, EffectUpdateCause::Milk)
    }

    /// `RemoveStatusEffectsConsumeEffect.apply(...)` (e.g. Honey Bottle clearing Poison).
    pub fn remove_effects_consume(
        &mut self,
        effects: &[MobEffectId],
        target: &mut dyn EffectTarget,
    ) -> bool {
        let mut removed_any = false;
        for &effect in effects {
            if self.remove_effect(effect, target, EffectUpdateCause::Honey) {
                removed_any = true;
            }
        }
        removed_any
    }

    /// `MobEffect.applyInstantenousEffect(Entity source, Entity indirectSource, LivingEntity target, int amplifier, double proximity)`
    /// for instantaneous effects (`InstantHealth`, `InstantDamage`, `Saturation`).
    pub fn apply_instantaneous_effect(
        effect: MobEffectId,
        amplifier: i32,
        proximity: f64,
        indirect: bool,
        target: &mut dyn EffectTarget,
    ) {
        let clamped_amp = amplifier.clamp(0, 255);
        match effect {
            MobEffectId::InstantHealth => {
                if target.is_inverted_heal_and_harm() {
                    let amount = (proximity * f64::from(6_i32.wrapping_shl(clamped_amp as u32))
                        + 0.5) as i32;
                    target.hurt_magic(amount as f32, indirect);
                } else {
                    let amount = (proximity * f64::from(4_i32.wrapping_shl(clamped_amp as u32))
                        + 0.5) as i32;
                    target.heal(amount as f32);
                }
            }
            MobEffectId::InstantDamage => {
                if target.is_inverted_heal_and_harm() {
                    let amount = (proximity * f64::from(4_i32.wrapping_shl(clamped_amp as u32))
                        + 0.5) as i32;
                    target.heal(amount as f32);
                } else {
                    let amount = (proximity * f64::from(6_i32.wrapping_shl(clamped_amp as u32))
                        + 0.5) as i32;
                    target.hurt_magic(amount as f32, indirect);
                }
            }
            MobEffectId::Saturation => {
                if target.is_player() {
                    target.eat_saturation(clamped_amp.wrapping_add(1), 1.0);
                }
            }
            _ => {
                let _ = Self::apply_effect_tick(effect, clamped_amp, target);
            }
        }
    }

    /// `LivingEntity.tickEffects()` — exact Paper 26.2 server tick order:
    ///
    /// 1. Set `is_ticking_effects = true`.
    /// 2. Iterate over active effects in deterministic order:
    ///    - If `!has_remaining_duration()`, mark for removal.
    ///    - Compute `tick_count = if infinite { target.tick_count() } else { instance.duration() }`.
    ///    - If `should_apply_effect_tick_this_tick(tick_count, amp)` and `target.on_effect_tick_event(...)`:
    ///      call `apply_effect_tick`; if it returns `false`, mark for removal.
    ///    - Otherwise `tick_down_duration()`; if `downgrade_to_hidden_effect()` returns `true`,
    ///      call `on_effect_updated(instance, true, target)` and mark dirty.
    ///    - If `!has_remaining_duration()`, mark for removal.
    /// 3. Remove all expired effects and call `on_effects_removed` on them.
    /// 4. Set `is_ticking_effects = false` and drain any deferred `effects_to_process` actions.
    /// 5. If `effects_dirty`, run `update_invisibility_status()` and clear `effects_dirty`.
    pub fn tick_effects(&mut self, target: &mut dyn EffectTarget) {
        self.is_ticking_effects = true;

        let mut expired: Vec<MobEffectId> = Vec::new();
        let mut downgraded: Vec<MobEffectInstance> = Vec::new();

        for (&effect_id, instance) in &mut self.effects {
            if !instance.has_remaining_duration() {
                expired.push(effect_id);
                continue;
            }

            let tick_count = if instance.is_infinite_duration() {
                target.tick_count()
            } else {
                instance.duration()
            };

            let def = effect_id.def();
            if def.should_apply_effect_tick_this_tick(tick_count, instance.amplifier())
                && target.on_effect_tick_event(effect_id, instance.amplifier())
                && !Self::apply_effect_tick(effect_id, instance.amplifier(), target)
            {
                expired.push(effect_id);
                continue;
            }

            instance.tick_down_duration();
            if instance.downgrade_to_hidden_effect() {
                downgraded.push(instance.clone());
            }

            if !instance.has_remaining_duration() {
                expired.push(effect_id);
            }
        }

        for inst in &downgraded {
            Self::on_effect_updated(inst, true, target);
            self.effects_dirty = true;
        }

        if !expired.is_empty() {
            let mut removed_instances = Vec::with_capacity(expired.len());
            for effect_id in expired {
                if let Some(removed) = self.effects.shift_remove(&effect_id) {
                    removed_instances.push(removed);
                }
            }
            if !removed_instances.is_empty() {
                Self::on_effects_removed(&removed_instances, target);
                self.effects_dirty = true;
            }
        }

        self.is_ticking_effects = false;

        if !self.effects_to_process.is_empty() {
            let pending = std::mem::take(&mut self.effects_to_process);
            for action in pending {
                match action {
                    DeferredAction::Add {
                        instance,
                        cause,
                        force,
                    } => {
                        if force {
                            self.force_add_effect(instance, target, cause);
                        } else {
                            let _ = self.add_effect(instance, target, cause);
                        }
                    }
                    DeferredAction::Remove { effect, cause } => {
                        let _ = self.remove_effect(effect, target, cause);
                    }
                    DeferredAction::RemoveAll { cause } => {
                        let _ = self.remove_all_effects(target, cause);
                    }
                }
            }
        }

        if self.effects_dirty {
            self.update_invisibility_status();
            self.effects_dirty = false;
        }
    }

    /// `LivingEntity.updateInvisibilityStatus()` — updates `cached_particles` and
    /// `cached_ambient` from the current active effects.
    pub fn update_invisibility_status(&mut self) {
        if self.effects.is_empty() {
            self.cached_particles.clear();
            self.cached_ambient = false;
        } else {
            self.cached_particles.clear();
            for effect in self.effects.values() {
                if effect.is_visible() {
                    self.cached_particles.push(effect.particle_options());
                }
            }
            self.cached_ambient = Self::are_all_effects_ambient(self.effects.values());
        }
    }

    /// Propagate `MobEffectInstance.onMobRemoved` to all active effects.
    pub fn on_mob_removed(&self, target: &mut dyn EffectTarget, reason: EffectRemovalReason) {
        if reason != EffectRemovalReason::Killed {
            return;
        }
        for instance in self.effects.values() {
            match instance.effect() {
                MobEffectId::WindCharged => target.on_wind_charged_killed(instance.amplifier()),
                MobEffectId::Weaving => target.on_weaving_killed(instance.amplifier()),
                MobEffectId::Oozing => target.on_oozing_killed(instance.amplifier()),
                _ => {}
            }
        }
    }

    /// Propagate `MobEffectInstance.onMobHurt` to all active effects.
    pub fn on_mob_hurt(&self, target: &mut dyn EffectTarget, damage: f32) {
        for instance in self.effects.values() {
            if instance.effect() == MobEffectId::Infested {
                target.on_infested_hurt(instance.amplifier(), damage);
            }
        }
    }

    /// Serialize active effects to NBT (`active_effects` list on `LivingEntity`).
    pub fn save_nbt(&self) -> ListTag {
        let mut list = ListTag::new();
        for instance in self.effects.values() {
            list.add(Tag::Compound(instance.save_nbt()));
        }
        list
    }

    /// Deserialize active effects from NBT (`active_effects` list on `LivingEntity`).
    ///
    /// Invalid or unrecognized entries are skipped, matching `LivingEntity.readAdditionalSaveData`.
    pub fn load_nbt(list: &ListTag) -> Self {
        let mut active = Self::new();
        for i in 0..list.size() {
            if let Some(compound) = list.get_compound(i)
                && let Some(instance) = MobEffectInstance::load_nbt(compound)
            {
                active.effects.insert(instance.effect(), instance);
            }
        }
        active.effects_dirty = true;
        active.update_invisibility_status();
        active
    }

    /// Save `active_effects` into a parent entity `CompoundTag` if non-empty.
    pub fn save_into_compound(&self, entity_tag: &mut CompoundTag) {
        if !self.effects.is_empty() {
            entity_tag.put("active_effects".to_string(), Tag::List(self.save_nbt()));
        }
    }

    /// Load `active_effects` from a parent entity `CompoundTag`.
    pub fn load_from_compound(entity_tag: &CompoundTag) -> Self {
        match entity_tag.get_list("active_effects") {
            Some(list) => Self::load_nbt(list),
            None => Self::new(),
        }
    }

    fn on_effect_added(instance: &MobEffectInstance, target: &mut dyn EffectTarget) {
        let def = instance.effect_def();
        Self::add_attribute_modifiers(instance.effect(), instance.amplifier(), target);
        if let Some(sound_id) = def.sound_on_added {
            target.on_sound(SoundTrigger { sound_id });
        }
    }

    fn on_effect_updated(
        instance: &MobEffectInstance,
        reapply_attributes: bool,
        target: &mut dyn EffectTarget,
    ) {
        if reapply_attributes {
            Self::remove_attribute_modifiers(instance.effect(), target);
            Self::add_attribute_modifiers(instance.effect(), instance.amplifier(), target);
        }
    }

    fn on_effects_removed(removed: &[MobEffectInstance], target: &mut dyn EffectTarget) {
        for instance in removed {
            Self::remove_attribute_modifiers(instance.effect(), target);
        }
    }

    fn on_effect_started(effect: MobEffectId, amplifier: i32, target: &mut dyn EffectTarget) {
        if effect == MobEffectId::Absorption {
            // AbsorptionMobEffect.onEffectStarted:
            // mob.setAbsorptionAmount(Math.max(mob.getAbsorptionAmount(), 4 * (1 + amplifier)))
            let min_absorption = (4_i32.wrapping_mul(1_i32.wrapping_add(amplifier))) as f32;
            let next = target.absorption_amount().max(min_absorption);
            target.set_absorption_amount(next);
        }
    }

    fn add_attribute_modifiers(effect: MobEffectId, amplifier: i32, target: &mut dyn EffectTarget) {
        for template in effect.def().attribute_modifiers {
            target.remove_attribute_modifier(template.attribute, template.modifier_id);
            target.add_attribute_modifier(template.attribute, template.create(amplifier));
        }
    }

    fn remove_attribute_modifiers(effect: MobEffectId, target: &mut dyn EffectTarget) {
        for template in effect.def().attribute_modifiers {
            target.remove_attribute_modifier(template.attribute, template.modifier_id);
        }
    }

    /// Exact Paper `MobEffect.applyEffectTick(ServerLevel, LivingEntity, int amplifier)`.
    fn apply_effect_tick(
        effect: MobEffectId,
        amplifier: i32,
        target: &mut dyn EffectTarget,
    ) -> bool {
        match effect {
            MobEffectId::Regeneration => {
                if target.health() < target.max_health() {
                    target.heal(1.0);
                }
                true
            }
            MobEffectId::Poison => {
                if target.health() > 1.0 {
                    target.hurt_magic(1.0, false);
                }
                true
            }
            MobEffectId::Wither => {
                target.hurt_wither(1.0);
                true
            }
            MobEffectId::Hunger => {
                if target.is_player() {
                    target.cause_food_exhaustion(0.005_f32 * (amplifier.wrapping_add(1) as f32));
                }
                true
            }
            MobEffectId::Saturation => {
                if target.is_player() {
                    target.eat_saturation(amplifier.wrapping_add(1), 1.0);
                }
                true
            }
            MobEffectId::InstantHealth => {
                if target.is_inverted_heal_and_harm() {
                    target.hurt_magic(6_i32.wrapping_shl(amplifier as u32) as f32, false);
                } else {
                    let heal = 4_i32.wrapping_shl(amplifier as u32).max(0);
                    target.heal(heal as f32);
                }
                true
            }
            MobEffectId::InstantDamage => {
                if target.is_inverted_heal_and_harm() {
                    let heal = 4_i32.wrapping_shl(amplifier as u32).max(0);
                    target.heal(heal as f32);
                } else {
                    target.hurt_magic(6_i32.wrapping_shl(amplifier as u32) as f32, false);
                }
                true
            }
            MobEffectId::Absorption => target.absorption_amount() > 0.0,
            MobEffectId::BadOmen => target.on_bad_omen_tick(amplifier),
            MobEffectId::RaidOmen => target.on_raid_omen_trigger(amplifier),
            _ => true,
        }
    }
}
