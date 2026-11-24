use super::{
    Difficulty, ExhaustionConfig, ExhaustionReason, ExperienceSaveData, ExperienceState,
    RegainReason, SurvivalEvent, SurvivalGameRules, SurvivalPlayerHost,
};
use crate::world::food::food_data::{FoodData, FoodSaveData};
use crate::world::food::food_properties::FoodProperties;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MovementKind {
    Swimming,
    UnderWater,
    InWater,
    OnGroundSprinting,
    OnGroundCrouching,
    OnGroundWalking,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurvivalSaveData {
    pub food: FoodSaveData,
    pub experience: ExperienceSaveData,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RestoreOutcome {
    /// Health to assign to the respawned player (`Some(old_health)` when `restore_all`,
    /// `None` when the caller should leave health at max health).
    pub restored_health: Option<f32>,
    pub kept_inventory_and_xp: bool,
}

/// Authoritative survival progression state attached to the E6 `Player`/`ServerPlayer` shell.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSurvivalState {
    pub food: FoodData,
    pub experience: ExperienceState,
    pub last_sent_health: f32,
    pub last_sent_food: i32,
    pub last_food_saturation_zero: bool,
    pub score: i32,
}

impl Default for PlayerSurvivalState {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerSurvivalState {
    pub const INITIAL_LAST_SENT_HEALTH: f32 = -1.0e8_f32;
    pub const INITIAL_LAST_SENT_FOOD: i32 = -99_999_999;

    #[must_use]
    pub const fn new() -> Self {
        Self {
            food: FoodData::new(),
            experience: ExperienceState::new(),
            last_sent_health: Self::INITIAL_LAST_SENT_HEALTH,
            last_sent_food: Self::INITIAL_LAST_SENT_FOOD,
            last_food_saturation_zero: true,
            score: 0,
        }
    }

    /// Exact port of `Player.canEat(boolean)`.
    #[must_use]
    pub fn can_eat(&self, host: &impl SurvivalPlayerHost, can_always_eat: bool) -> bool {
        host.is_invulnerable() || can_always_eat || self.food.needs_food()
    }

    /// Exact port of `Player.hasEnoughFoodToDoExhaustiveManoeuvres()`.
    #[must_use]
    pub fn has_enough_food_to_sprint(&self, host: &impl SurvivalPlayerHost) -> bool {
        self.food.has_enough_food() || host.may_fly()
    }

    /// Exact port of `Player.causeFoodExhaustion(float, ExhaustionReason)`.
    pub fn cause_food_exhaustion(
        &mut self,
        host: &mut impl SurvivalPlayerHost,
        amount: f32,
        reason: ExhaustionReason,
        events: &mut Vec<SurvivalEvent>,
    ) {
        if !host.is_invulnerable()
            && let Some(accepted_amount) = host.on_exhaustion(reason, amount)
        {
            events.push(SurvivalEvent::Exhaustion {
                amount: accepted_amount,
                reason,
            });
            self.food.add_exhaustion(accepted_amount);
        }
    }

    /// Exact port of `FoodData.eat(FoodProperties, ItemStack, ServerPlayer)`.
    pub fn eat_food(
        &mut self,
        host: &mut impl SurvivalPlayerHost,
        properties: FoodProperties,
        events: &mut Vec<SurvivalEvent>,
    ) {
        self.food.eat_with_events(host, properties, events);
    }

    /// Exact port of `ServerPlayer.giveExperiencePoints(int)`.
    pub fn give_experience_points(
        &mut self,
        host: &mut impl SurvivalPlayerHost,
        points: i32,
        events: &mut Vec<SurvivalEvent>,
    ) {
        let score_delta = self
            .experience
            .give_experience_points(points, host.tick_count(), events);
        if score_delta != 0 {
            self.score = self.score.wrapping_add(score_delta);
            host.increase_score(score_delta);
        }
    }

    /// Exact port of `ServerPlayer.giveExperienceLevels(int)`.
    pub fn give_experience_levels(
        &mut self,
        host: &impl SurvivalPlayerHost,
        amount: i32,
        events: &mut Vec<SurvivalEvent>,
    ) {
        self.experience
            .give_experience_levels(amount, host.tick_count(), events);
    }

    /// Exact port of `ServerPlayer.jumpFromGround()` exhaustion portion.
    pub fn on_jump(
        &mut self,
        host: &mut impl SurvivalPlayerHost,
        is_sprinting: bool,
        config: &ExhaustionConfig,
        events: &mut Vec<SurvivalEvent>,
    ) {
        if is_sprinting {
            self.cause_food_exhaustion(
                host,
                config.jump_sprint_exhaustion,
                ExhaustionReason::JumpSprint,
                events,
            );
        } else {
            self.cause_food_exhaustion(
                host,
                config.jump_walk_exhaustion,
                ExhaustionReason::Jump,
                events,
            );
        }
    }

    /// Exact port of `ServerPlayer.checkMovementStatistics(dx, dy, dz)` exhaustion portion.
    pub fn check_movement_exhaustion(
        &mut self,
        host: &mut impl SurvivalPlayerHost,
        kind: MovementKind,
        delta: [f64; 3],
        config: &ExhaustionConfig,
        events: &mut Vec<SurvivalEvent>,
    ) {
        let [dx, dy, dz] = delta;
        let (distance_cm, multiplier, reason) = match kind {
            MovementKind::Swimming => {
                let d = java_round_f32(((dx * dx + dy * dy + dz * dz).sqrt() as f32) * 100.0);
                (d, config.swim_multiplier, ExhaustionReason::Swim)
            }
            MovementKind::UnderWater => {
                let d = java_round_f32(((dx * dx + dy * dy + dz * dz).sqrt() as f32) * 100.0);
                (d, config.swim_multiplier, ExhaustionReason::WalkUnderwater)
            }
            MovementKind::InWater => {
                let d = java_round_f32(((dx * dx + dz * dz).sqrt() as f32) * 100.0);
                (d, config.swim_multiplier, ExhaustionReason::WalkOnWater)
            }
            MovementKind::OnGroundSprinting => {
                let d = java_round_f32(((dx * dx + dz * dz).sqrt() as f32) * 100.0);
                (d, config.sprint_multiplier, ExhaustionReason::Sprint)
            }
            MovementKind::OnGroundCrouching => {
                let d = java_round_f32(((dx * dx + dz * dz).sqrt() as f32) * 100.0);
                (d, config.other_multiplier, ExhaustionReason::Crouch)
            }
            MovementKind::OnGroundWalking => {
                let d = java_round_f32(((dx * dx + dz * dz).sqrt() as f32) * 100.0);
                (d, config.other_multiplier, ExhaustionReason::Walk)
            }
        };

        if distance_cm > 0 {
            let amount = multiplier * (distance_cm as f32) * 0.01;
            self.cause_food_exhaustion(host, amount, reason, events);
        }
    }

    /// Exact port of `ServerPlayer.tickRegeneration()`, called inside `Player.aiStep()`
    /// BEFORE `FoodData.tick(ServerPlayer)` in `ServerPlayer.doTick()`.
    pub fn tick_regeneration(
        &mut self,
        host: &mut impl SurvivalPlayerHost,
        difficulty: Difficulty,
        rules: SurvivalGameRules,
        events: &mut Vec<SurvivalEvent>,
    ) {
        if difficulty == Difficulty::Peaceful && rules.natural_health_regeneration {
            let tick_count = host.tick_count();
            if tick_count % 20 == 0 {
                if host.health() < host.max_health() {
                    host.heal(1.0, RegainReason::Regen, false);
                    events.push(SurvivalEvent::Heal {
                        amount: 1.0,
                        reason: RegainReason::Regen,
                        is_fast_regen: false,
                    });
                }
                let saturation = self.food.saturation_level();
                if saturation < 20.0 {
                    self.food.set_saturation(saturation + 1.0);
                }
            }

            if tick_count % 10 == 0 && self.food.needs_food() {
                self.food
                    .set_food_level(self.food.food_level().wrapping_add(1));
            }
        }
    }

    /// Exact port of the survival slice of `ServerPlayer.doTick()`:
    /// 1. When `is_valid_in_world && (!is_spectator || !touching_unloaded_chunk)`:
    ///    a. `tick_regeneration` (from `super.tick()` -> `aiStep()`)
    ///    b. `food.tick(...)`
    /// 2. Health / food / saturation-zero sync packet emission check
    /// 3. Experience sync packet emission check
    /// 4. CraftBukkit `PlayerLevelChangeEvent` check (`oldLevel`)
    pub fn do_tick(
        &mut self,
        host: &mut impl SurvivalPlayerHost,
        difficulty: Difficulty,
        rules: SurvivalGameRules,
        exhaustion_config: &ExhaustionConfig,
        is_valid_in_world: bool,
        touching_unloaded_chunk: bool,
    ) -> Vec<SurvivalEvent> {
        let mut events = Vec::new();

        if is_valid_in_world && (!host.is_spectator() || !touching_unloaded_chunk) {
            self.tick_regeneration(host, difficulty, rules, &mut events);
            self.food
                .tick(host, difficulty, rules, exhaustion_config, &mut events);
        }

        let health = host.health();
        let food_level = self.food.food_level();
        let saturation_level = self.food.saturation_level();
        let saturation_zero = saturation_level == 0.0;

        if health != self.last_sent_health
            || self.last_sent_food != food_level
            || saturation_zero != self.last_food_saturation_zero
        {
            events.push(SurvivalEvent::SendHealthPacket {
                health,
                food_level,
                saturation: saturation_level,
            });
            self.last_sent_health = health;
            self.last_sent_food = food_level;
            self.last_food_saturation_zero = saturation_zero;
        }

        if self.experience.total_experience != self.experience.last_sent_exp {
            self.experience.last_sent_exp = self.experience.total_experience;
            events.push(SurvivalEvent::SendExperiencePacket {
                progress: self.experience.experience_progress,
                total_experience: self.experience.total_experience,
                level: self.experience.experience_level,
            });
        }

        if self.experience.old_level == -1 {
            self.experience.old_level = self.experience.experience_level;
        }

        if self.experience.old_level != self.experience.experience_level {
            events.push(SurvivalEvent::PlayerLevelChange {
                old_level: self.experience.old_level,
                new_level: self.experience.experience_level,
            });
            self.experience.old_level = self.experience.experience_level;
        }

        events
    }

    /// Exact port of `ServerPlayer.resetSentInfo()`.
    pub fn reset_sent_info(&mut self) {
        self.last_sent_health = Self::INITIAL_LAST_SENT_HEALTH;
        self.experience.last_sent_exp = -1;
    }

    /// Exact port of survival fields in `ServerPlayer.restoreFrom(ServerPlayer, boolean)`.
    pub fn restore_from(
        &mut self,
        old: &Self,
        old_health: f32,
        old_is_spectator: bool,
        restore_all: bool,
        rules: SurvivalGameRules,
    ) -> RestoreOutcome {
        let outcome = if restore_all {
            self.food = old.food.clone();
            self.transfer_xp_and_score(old);
            RestoreOutcome {
                restored_health: Some(old_health),
                kept_inventory_and_xp: true,
            }
        } else {
            self.food = FoodData::new();
            let keep = rules.keep_inventory || old_is_spectator;
            if keep {
                self.transfer_xp_and_score(old);
            } else {
                self.experience = ExperienceState::new();
                self.score = 0;
            }
            RestoreOutcome {
                restored_health: None,
                kept_inventory_and_xp: keep,
            }
        };

        self.experience.last_sent_exp = -1;
        self.last_sent_health = -1.0;
        self.last_sent_food = -1;

        outcome
    }

    fn transfer_xp_and_score(&mut self, old: &Self) {
        self.experience.experience_level = old.experience.experience_level;
        self.experience.total_experience = old.experience.total_experience;
        self.experience.experience_progress = old.experience.experience_progress;
        self.score = old.score;
    }

    pub fn read_save_data(&mut self, data: SurvivalSaveData) {
        self.food.read_save_data(data.food);
        self.experience.read_save_data(data.experience);
    }

    #[must_use]
    pub const fn write_save_data(&self) -> SurvivalSaveData {
        SurvivalSaveData {
            food: self.food.write_save_data(),
            experience: self.experience.write_save_data(),
        }
    }
}

/// Exact port of Java `Math.round(float)` (JDK-6430675).
#[inline]
pub(crate) fn java_round_f32(value: f32) -> i32 {
    if value.is_nan() {
        0
    } else {
        (value as f64 + 0.5_f64).floor() as i32
    }
}
