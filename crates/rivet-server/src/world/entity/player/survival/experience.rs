//! Member slice of `Player`/`ServerPlayer` awaiting their class ports (ROADMAP lane 2B), not a Java class: the experience state `Player` ticks (`experienceLevel`,
//! `experienceProgress`, `totalExperience`), placed next to `Player`.

use rivet_util::mth::{clamp, clamp_f32};

use super::SurvivalEvent;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExperienceSaveData {
    pub xp_progress: Option<f32>,
    pub xp_level: Option<i32>,
    pub xp_total: Option<i32>,
}

/// Exact port of `Player` / `ServerPlayer` experience fields and progression logic.
#[derive(Debug, Clone, PartialEq)]
pub struct ExperienceState {
    pub experience_level: i32,
    pub total_experience: i32,
    pub experience_progress: f32,
    pub last_level_up_time: i32,
    pub last_sent_exp: i32,
    pub old_level: i32,
}

impl Default for ExperienceState {
    fn default() -> Self {
        Self::new()
    }
}

impl ExperienceState {
    pub const INITIAL_LAST_SENT_EXP: i32 = -99_999_999;

    #[must_use]
    pub const fn new() -> Self {
        Self {
            experience_level: 0,
            total_experience: 0,
            experience_progress: 0.0,
            last_level_up_time: 0,
            last_sent_exp: Self::INITIAL_LAST_SENT_EXP,
            old_level: -1,
        }
    }

    #[must_use]
    pub const fn xp_needed_for_next_level(&self) -> i32 {
        xp_needed_for_next_level(self.experience_level)
    }

    /// Exact port of `ServerPlayer.giveExperiencePoints` -> `Player.giveExperiencePoints`.
    /// Returns the score delta to pass to `host.increase_score` (0 if `points == 0`).
    /// `ServerPlayer.giveExperiencePoints` unconditionally invalidates `last_sent_exp`.
    pub fn give_experience_points(
        &mut self,
        points: i32,
        tick_count: i32,
        events: &mut Vec<SurvivalEvent>,
    ) -> i32 {
        if points != 0 {
            self.give_experience_points_inner(points, tick_count, events);
        }
        self.last_sent_exp = -1;
        points
    }

    fn give_experience_points_inner(
        &mut self,
        points: i32,
        tick_count: i32,
        events: &mut Vec<SurvivalEvent>,
    ) {
        self.experience_progress += points as f32 / self.xp_needed_for_next_level() as f32;
        self.total_experience = clamp(self.total_experience.wrapping_add(points), 0, i32::MAX);

        while self.experience_progress < 0.0 {
            let remaining = self.experience_progress * self.xp_needed_for_next_level() as f32;
            if self.experience_level > 0 {
                self.give_experience_levels_inner(-1, tick_count, events);
                self.experience_progress = 1.0 + remaining / self.xp_needed_for_next_level() as f32;
            } else {
                self.give_experience_levels_inner(-1, tick_count, events);
                self.experience_progress = 0.0;
            }
        }

        while self.experience_progress >= 1.0 {
            self.experience_progress =
                (self.experience_progress - 1.0) * self.xp_needed_for_next_level() as f32;
            self.give_experience_levels_inner(1, tick_count, events);
            self.experience_progress /= self.xp_needed_for_next_level() as f32;
        }
    }

    /// Exact port of `ServerPlayer.giveExperienceLevels` -> `Player.giveExperienceLevels`.
    /// `ServerPlayer.giveExperienceLevels` unconditionally invalidates `last_sent_exp`.
    pub fn give_experience_levels(
        &mut self,
        amount: i32,
        tick_count: i32,
        events: &mut Vec<SurvivalEvent>,
    ) {
        if amount != 0 {
            self.give_experience_levels_inner(amount, tick_count, events);
        }
        self.last_sent_exp = -1;
    }

    fn give_experience_levels_inner(
        &mut self,
        amount: i32,
        tick_count: i32,
        events: &mut Vec<SurvivalEvent>,
    ) {
        self.experience_level = self.experience_level.saturating_add(amount);
        if self.experience_level < 0 {
            self.experience_level = 0;
            self.experience_progress = 0.0;
            self.total_experience = 0;
        }

        if amount > 0
            && self.experience_level % 5 == 0
            && (self.last_level_up_time as f32) < (tick_count as f32) - 100.0
        {
            let vol = if self.experience_level > 30 {
                1.0
            } else {
                self.experience_level as f32 / 30.0
            };
            events.push(SurvivalEvent::LevelUpSound {
                volume: vol * 0.75,
                pitch: 1.0,
            });
            self.last_level_up_time = tick_count;
        }
    }

    /// Exact port of `ServerPlayer.setExperiencePoints`.
    pub fn set_experience_points(&mut self, amount: i32) {
        let limit = self.xp_needed_for_next_level() as f32;
        let max = (limit - 1.0) / limit;
        let experience_points_to_set = clamp_f32(amount as f32 / limit, 0.0, max);
        if experience_points_to_set != self.experience_progress {
            self.experience_progress = experience_points_to_set;
            self.last_sent_exp = -1;
        }
    }

    /// Exact port of `ServerPlayer.setExperienceLevels`.
    pub fn set_experience_levels(&mut self, amount: i32) {
        if amount != self.experience_level {
            self.experience_level = amount;
            self.last_sent_exp = -1;
        }
    }

    /// Exact port of `ServerPlayer.onEnchantmentPerformed` -> `Player.onEnchantmentPerformed`.
    pub fn on_enchantment_performed(&mut self, enchantment_cost: i32) {
        self.experience_level = self.experience_level.wrapping_sub(enchantment_cost);
        if self.experience_level < 0 {
            self.experience_level = 0;
            self.experience_progress = 0.0;
            self.total_experience = 0;
        }
        self.last_sent_exp = -1;
    }

    /// Exact port of `Player.getBaseExperienceReward(ServerLevel)`.
    #[must_use]
    pub const fn base_experience_reward(&self, keep_inventory: bool, is_spectator: bool) -> i32 {
        if !keep_inventory && !is_spectator {
            let scaled = self.experience_level.wrapping_mul(7);
            if scaled < 100 { scaled } else { 100 }
        } else {
            0
        }
    }

    pub fn read_save_data(&mut self, data: ExperienceSaveData) {
        self.experience_progress = data.xp_progress.unwrap_or(0.0);
        self.experience_level = data.xp_level.unwrap_or(0);
        self.total_experience = data.xp_total.unwrap_or(0);
    }

    #[must_use]
    pub const fn write_save_data(&self) -> ExperienceSaveData {
        ExperienceSaveData {
            xp_progress: Some(self.experience_progress),
            xp_level: Some(self.experience_level),
            xp_total: Some(self.total_experience),
        }
    }
}

/// Exact port of `Player.getXpNeededForNextLevel()`.
#[must_use]
pub const fn xp_needed_for_next_level(experience_level: i32) -> i32 {
    if experience_level >= 30 {
        112_i32.wrapping_add(experience_level.wrapping_sub(30).wrapping_mul(9))
    } else if experience_level >= 15 {
        37_i32.wrapping_add(experience_level.wrapping_sub(15).wrapping_mul(5))
    } else {
        7_i32.wrapping_add(experience_level.wrapping_mul(2))
    }
}
