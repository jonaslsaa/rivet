//! Paper 26.2 `FallLocation`, `CombatEntry`, and `CombatTracker`.

use std::collections::VecDeque;

use super::damage_source::{DamageEntityRef, DamageSource, DeathMessage};
use super::damage_type::{DamageTypeTag, DeathMessageType};

/// Mirrors `net.minecraft.world.damagesource.FallLocation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FallLocation {
    Generic,
    Ladder,
    Vines,
    WeepingVines,
    TwistingVines,
    Scaffolding,
    OtherClimbable,
    Water,
}

impl FallLocation {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Ladder => "ladder",
            Self::Vines => "vines",
            Self::WeepingVines => "weeping_vines",
            Self::TwistingVines => "twisting_vines",
            Self::Scaffolding => "scaffolding",
            Self::OtherClimbable => "other_climbable",
            Self::Water => "water",
        }
    }

    /// Mirrors `FallLocation#languageKey()`: `"death.fell.accident." + id`.
    pub fn language_key(self) -> String {
        format!("death.fell.accident.{}", self.id())
    }

    /// Mirrors `FallLocation#blockToFallLocation(BlockState)` using vanilla block IDs.
    pub fn from_climbable_block_id(block_id: &str) -> Self {
        match block_id {
            "minecraft:ladder" => Self::Ladder,
            "minecraft:vine" => Self::Vines,
            "minecraft:weeping_vines" | "minecraft:weeping_vines_plant" => Self::WeepingVines,
            "minecraft:twisting_vines" | "minecraft:twisting_vines_plant" => Self::TwistingVines,
            "minecraft:scaffolding" => Self::Scaffolding,
            _ => Self::OtherClimbable,
        }
    }

    /// Mirrors `FallLocation#getCurrentFallLocation(LivingEntity)`.
    pub fn current(last_climbable_block_id: Option<&str>, is_in_water: bool) -> Option<Self> {
        if let Some(block_id) = last_climbable_block_id {
            Some(Self::from_climbable_block_id(block_id))
        } else if is_in_water {
            Some(Self::Water)
        } else {
            None
        }
    }
}

/// Mirrors `net.minecraft.world.damagesource.CombatEntry`.
#[derive(Debug, Clone, PartialEq)]
pub struct CombatEntry {
    pub source: DamageSource,
    pub damage: f32,
    pub fall_location: Option<FallLocation>,
    pub fall_distance: f32,
}

/// Mirrors `net.minecraft.world.damagesource.CombatTracker`.
#[derive(Debug, Clone, PartialEq)]
pub struct CombatTracker {
    entries: VecDeque<CombatEntry>,
    last_damage_time: i32,
    combat_start_time: i32,
    combat_end_time: i32,
    in_combat: bool,
    taking_damage: bool,
    /// Paper MC-301114 `maxTrackingCombatEntries` (`None` = disabled/unbounded;
    /// Paper defaults to `Some(10_240)`).
    max_entries: Option<usize>,
}

impl Default for CombatTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl CombatTracker {
    pub const RESET_DAMAGE_STATUS_TIME: i32 = 100;
    pub const RESET_COMBAT_STATUS_TIME: i32 = 300;
    pub const DEFAULT_PAPER_MAX_ENTRIES: usize = 10_240;

    pub fn new() -> Self {
        Self::with_max_entries(Some(Self::DEFAULT_PAPER_MAX_ENTRIES))
    }

    pub fn with_max_entries(max_entries: Option<usize>) -> Self {
        Self {
            entries: VecDeque::new(),
            last_damage_time: 0,
            combat_start_time: 0,
            combat_end_time: 0,
            in_combat: false,
            taking_damage: false,
            max_entries,
        }
    }

    pub fn entries(&self) -> &VecDeque<CombatEntry> {
        &self.entries
    }

    pub fn is_in_combat(&self) -> bool {
        self.in_combat
    }

    pub fn is_taking_damage(&self) -> bool {
        self.taking_damage
    }

    pub fn last_damage_time(&self) -> i32 {
        self.last_damage_time
    }

    /// Mirrors `CombatTracker#shouldEnterCombat(DamageSource)`.
    pub fn should_enter_combat(source: &DamageSource) -> bool {
        source.causing_entity().is_some_and(|e| e.is_living)
    }

    /// Mirrors `CombatTracker#recordDamage(DamageSource, float)`.
    ///
    /// Returns `true` if the entity transitioned from out-of-combat into combat (`onEnterCombat`).
    pub fn record_damage(
        &mut self,
        source: DamageSource,
        damage: f32,
        fall_location: Option<FallLocation>,
        fall_distance: f32,
        tick_count: i32,
        is_alive: bool,
    ) -> bool {
        self.recheck_status(tick_count, is_alive);
        let should_enter = Self::should_enter_combat(&source);
        // Paper MC-301114 (`CombatTracker#recordDamageAndCheckCombatState`, lines 51-59):
        // trim while `entries.len() > max_entries` BEFORE appending the new entry.
        if !self.entries.is_empty()
            && let Some(max_entries) = self.max_entries
        {
            while self.entries.len() > max_entries {
                self.entries.pop_front();
            }
        }
        self.entries.push_back(CombatEntry {
            source,
            damage,
            fall_location,
            fall_distance,
        });
        self.last_damage_time = tick_count;
        self.taking_damage = true;
        if !self.in_combat && is_alive && should_enter {
            self.in_combat = true;
            self.combat_start_time = tick_count;
            self.combat_end_time = tick_count;
            true
        } else {
            false
        }
    }

    /// Mirrors `CombatTracker#getCombatDuration()`.
    pub fn combat_duration(&self, tick_count: i32) -> i32 {
        if self.in_combat {
            tick_count.wrapping_sub(self.combat_start_time)
        } else {
            self.combat_end_time.wrapping_sub(self.combat_start_time)
        }
    }

    /// Mirrors `CombatTracker#recheckStatus()`.
    ///
    /// Returns `true` if the entity transitioned out of combat (`onLeaveCombat`).
    pub fn recheck_status(&mut self, tick_count: i32, is_alive: bool) -> bool {
        let reset = if self.in_combat {
            Self::RESET_COMBAT_STATUS_TIME
        } else {
            Self::RESET_DAMAGE_STATUS_TIME
        };
        if self.taking_damage
            && (!is_alive || tick_count.wrapping_sub(self.last_damage_time) > reset)
        {
            self.reset_combat_state(tick_count)
        } else {
            false
        }
    }

    /// Mirrors `CombatTracker#resetCombatState()`.
    ///
    /// Returns `true` if the entity was in combat and therefore left combat (`onLeaveCombat`).
    pub fn reset_combat_state(&mut self, tick_count: i32) -> bool {
        let was_in_combat = self.in_combat;
        self.taking_damage = false;
        self.in_combat = false;
        self.combat_end_time = tick_count;
        self.entries.clear();
        was_in_combat
    }

    /// Mirrors `CombatTracker#getMostSignificantFall()`.
    pub fn most_significant_fall(&self) -> Option<&CombatEntry> {
        let mut best: Option<&CombatEntry> = None;
        let mut alt: Option<&CombatEntry> = None;
        let mut alt_damage: f32 = 0.0;
        let mut best_fall: f32 = 0.0;

        for i in 0..self.entries.len() {
            let entry = &self.entries[i];
            let previous = if i > 0 {
                Some(&self.entries[i - 1])
            } else {
                None
            };
            let is_fake_fall = entry
                .source
                .is_tag(DamageTypeTag::AlwaysMostSignificantFall);
            let fall_distance = if is_fake_fall {
                f32::MAX
            } else {
                entry.fall_distance
            };
            if (entry.source.is_tag(DamageTypeTag::IsFall) || is_fake_fall)
                && fall_distance > 0.0
                && (best.is_none() || fall_distance > best_fall)
            {
                best = if i > 0 { previous } else { Some(entry) };
                best_fall = fall_distance;
            }

            if entry.fall_location.is_some() && (alt.is_none() || entry.damage > alt_damage) {
                alt = Some(entry);
                alt_damage = entry.damage;
            }
        }

        if best_fall > 5.0 && best.is_some() {
            best
        } else if alt_damage > 5.0 && alt.is_some() {
            alt
        } else {
            None
        }
    }

    /// Mirrors `CombatTracker#getDeathMessage()`.
    pub fn death_message(&self, victim_name: &str, kill_credit_name: Option<&str>) -> DeathMessage {
        let Some(killing_blow) = self.entries.back() else {
            return DeathMessage::translatable(
                "death.attack.generic",
                vec![victim_name.to_owned()],
            );
        };

        let kill_source = &killing_blow.source;
        match kill_source.damage_type().death_message_type {
            DeathMessageType::FallVariants => {
                if let Some(fall_entry) = self.most_significant_fall() {
                    Self::fall_message(victim_name, fall_entry, kill_source.causing_entity())
                } else {
                    kill_source.localized_death_message(victim_name, kill_credit_name)
                }
            }
            DeathMessageType::IntentionalGameDesign => {
                let death_msg = format!("death.attack.{}", kill_source.msg_id());
                let link_key = format!("{death_msg}.link");
                DeathMessage {
                    translation_key: format!("{death_msg}.message"),
                    args: vec![victim_name.to_owned(), link_key.clone()],
                    intentional_game_design_link_key: Some(link_key),
                }
            }
            DeathMessageType::Default => {
                kill_source.localized_death_message(victim_name, kill_credit_name)
            }
        }
    }

    fn fall_message(
        victim_name: &str,
        fall_entry: &CombatEntry,
        killer_entity: Option<&DamageEntityRef>,
    ) -> DeathMessage {
        let fall_source = &fall_entry.source;
        if fall_source.is_tag(DamageTypeTag::IsFall)
            || fall_source.is_tag(DamageTypeTag::AlwaysMostSignificantFall)
        {
            let location = fall_entry.fall_location.unwrap_or(FallLocation::Generic);
            return DeathMessage::translatable(
                location.language_key(),
                vec![victim_name.to_owned()],
            );
        }

        let fall_attacker = fall_source.causing_entity();
        let fall_attacker_name = fall_attacker.map(|e| e.display_name.as_str());
        let killer_name = killer_entity.map(|e| e.display_name.as_str());

        if let Some(fall_name) = fall_attacker_name
            && killer_name != Some(fall_name)
        {
            return Self::assisted_fall_message(
                victim_name,
                fall_attacker,
                fall_name,
                "death.fell.assist.item",
                "death.fell.assist",
            );
        }
        if let Some(kill_name) = killer_name {
            return Self::assisted_fall_message(
                victim_name,
                killer_entity,
                kill_name,
                "death.fell.finish.item",
                "death.fell.finish",
            );
        }

        DeathMessage::translatable("death.fell.killer", vec![victim_name.to_owned()])
    }

    fn assisted_fall_message(
        victim_name: &str,
        attacker: Option<&DamageEntityRef>,
        attacker_name: &str,
        item_key: &str,
        plain_key: &str,
    ) -> DeathMessage {
        let held_custom_name = attacker
            .filter(|e| e.is_living)
            .and_then(|e| e.main_hand_custom_name.as_deref());
        match held_custom_name {
            Some(item_name) => DeathMessage::translatable(
                item_key,
                vec![
                    victim_name.to_owned(),
                    attacker_name.to_owned(),
                    item_name.to_owned(),
                ],
            ),
            None => DeathMessage::translatable(
                plain_key,
                vec![victim_name.to_owned(), attacker_name.to_owned()],
            ),
        }
    }
}
