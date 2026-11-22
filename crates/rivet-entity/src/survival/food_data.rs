use rivet_util::mth::{clamp, clamp_f32, max_f32, min_f32};

use super::{
    Difficulty, ExhaustionConfig, ExhaustionReason, FoodConstants, FoodProperties, RegainReason,
    SurvivalEvent, SurvivalGameRules, SurvivalPlayerHost,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoodSaveData {
    pub food_level: Option<i32>,
    pub food_tick_timer: Option<i32>,
    pub food_saturation_level: Option<f32>,
    pub food_exhaustion_level: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FoodData {
    food_level: i32,
    saturation_level: f32,
    pub exhaustion_level: f32,
    tick_timer: i32,
    pub saturated_regen_rate: i32,
    pub unsaturated_regen_rate: i32,
    pub starvation_rate: i32,
}

impl Default for FoodData {
    fn default() -> Self {
        Self::new()
    }
}

impl FoodData {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            food_level: FoodConstants::MAX_FOOD,
            saturation_level: FoodConstants::START_SATURATION,
            exhaustion_level: 0.0,
            tick_timer: 0,
            saturated_regen_rate: FoodConstants::HEALTH_TICK_COUNT_SATURATED,
            unsaturated_regen_rate: FoodConstants::HEALTH_TICK_COUNT,
            starvation_rate: FoodConstants::HEALTH_TICK_COUNT,
        }
    }

    fn add(&mut self, food: i32, saturation: f32) {
        self.food_level = clamp(
            food.wrapping_add(self.food_level),
            0,
            FoodConstants::MAX_FOOD,
        );
        self.saturation_level = clamp_f32(
            saturation + self.saturation_level,
            0.0,
            self.food_level as f32,
        );
    }

    pub fn eat(&mut self, food: i32, saturation_modifier: f32) {
        self.add(
            food,
            FoodConstants::saturation_by_modifier(food, saturation_modifier),
        );
    }

    pub fn eat_properties(&mut self, properties: FoodProperties) {
        self.add(properties.nutrition, properties.saturation);
    }

    /// Paper/CraftBukkit `FoodData.eat(FoodProperties, ItemStack, ServerPlayer)`.
    /// Dispatches `on_food_level_change` synchronously before mutating food/saturation
    /// and emits immediate `SendHealthPacket` matching Paper line 40-44.
    pub fn eat_with_events(
        &mut self,
        host: &mut impl SurvivalPlayerHost,
        properties: FoodProperties,
        events: &mut Vec<SurvivalEvent>,
    ) {
        let old_food_level = self.food_level;
        let requested_food_level = properties.nutrition.wrapping_add(old_food_level);
        if let Some(accepted_food_level) =
            host.on_food_level_change(old_food_level, requested_food_level)
        {
            events.push(SurvivalEvent::FoodLevelChange {
                old_food_level,
                new_food_level: accepted_food_level,
            });
            self.add(
                accepted_food_level.wrapping_sub(old_food_level),
                properties.saturation,
            );
        }
        events.push(SurvivalEvent::SendHealthPacket {
            health: host.health(),
            food_level: self.food_level,
            saturation: self.saturation_level,
        });
    }

    /// Exact port of `net.minecraft.world.food.FoodData.tick(ServerPlayer)`.
    pub fn tick(
        &mut self,
        host: &mut impl SurvivalPlayerHost,
        difficulty: Difficulty,
        rules: SurvivalGameRules,
        exhaustion_config: &ExhaustionConfig,
        events: &mut Vec<SurvivalEvent>,
    ) {
        if self.exhaustion_level > FoodConstants::EXHAUSTION_DROP {
            self.exhaustion_level -= FoodConstants::EXHAUSTION_DROP;
            if self.saturation_level > 0.0 {
                self.saturation_level = max_f32(self.saturation_level - 1.0, 0.0);
            } else if difficulty != Difficulty::Peaceful {
                let old_food_level = self.food_level;
                let requested_food_level = self.food_level.wrapping_sub(1).max(0);
                if let Some(accepted_food_level) =
                    host.on_food_level_change(old_food_level, requested_food_level)
                {
                    events.push(SurvivalEvent::FoodLevelChange {
                        old_food_level,
                        new_food_level: accepted_food_level,
                    });
                    self.food_level = accepted_food_level;
                }
                events.push(SurvivalEvent::SendHealthPacket {
                    health: host.health(),
                    food_level: self.food_level,
                    saturation: self.saturation_level,
                });
            }
        }

        let natural_regen = rules.natural_health_regeneration;
        if natural_regen
            && self.saturation_level > 0.0
            && host.is_hurt()
            && self.food_level >= FoodConstants::MAX_FOOD
        {
            self.tick_timer = self.tick_timer.wrapping_add(1);
            if self.tick_timer >= self.saturated_regen_rate {
                let saturation_spent = min_f32(self.saturation_level, 6.0);
                let heal_amount = saturation_spent / 6.0;
                host.heal(heal_amount, RegainReason::Satiated, true);
                events.push(SurvivalEvent::Heal {
                    amount: heal_amount,
                    reason: RegainReason::Satiated,
                    is_fast_regen: true,
                });
                if !host.is_invulnerable()
                    && let Some(accepted_exhaustion) =
                        host.on_exhaustion(ExhaustionReason::Regen, saturation_spent)
                {
                    events.push(SurvivalEvent::Exhaustion {
                        amount: accepted_exhaustion,
                        reason: ExhaustionReason::Regen,
                    });
                    self.add_exhaustion(accepted_exhaustion);
                }
                self.tick_timer = 0;
            }
        } else if natural_regen && self.food_level >= FoodConstants::HEAL_LEVEL && host.is_hurt() {
            self.tick_timer = self.tick_timer.wrapping_add(1);
            if self.tick_timer >= self.unsaturated_regen_rate {
                host.heal(1.0, RegainReason::Satiated, false);
                events.push(SurvivalEvent::Heal {
                    amount: 1.0,
                    reason: RegainReason::Satiated,
                    is_fast_regen: false,
                });
                if !host.is_invulnerable()
                    && let Some(accepted_exhaustion) = host
                        .on_exhaustion(ExhaustionReason::Regen, exhaustion_config.regen_exhaustion)
                {
                    events.push(SurvivalEvent::Exhaustion {
                        amount: accepted_exhaustion,
                        reason: ExhaustionReason::Regen,
                    });
                    self.add_exhaustion(accepted_exhaustion);
                }
                self.tick_timer = 0;
            }
        } else if self.food_level <= FoodConstants::STARVE_LEVEL {
            self.tick_timer = self.tick_timer.wrapping_add(1);
            if self.tick_timer >= self.starvation_rate {
                let health = host.health();
                if health > 10.0
                    || difficulty == Difficulty::Hard
                    || (health > 1.0 && difficulty == Difficulty::Normal)
                {
                    host.hurt_starve(1.0);
                    events.push(SurvivalEvent::StarveDamage { amount: 1.0 });
                }
                self.tick_timer = 0;
            }
        } else {
            self.tick_timer = 0;
        }
    }

    pub fn read_save_data(&mut self, data: FoodSaveData) {
        self.food_level = data.food_level.unwrap_or(FoodConstants::MAX_FOOD);
        self.tick_timer = data.food_tick_timer.unwrap_or(0);
        self.saturation_level = data
            .food_saturation_level
            .unwrap_or(FoodConstants::START_SATURATION);
        self.exhaustion_level = data.food_exhaustion_level.unwrap_or(0.0);
    }

    #[must_use]
    pub const fn write_save_data(&self) -> FoodSaveData {
        FoodSaveData {
            food_level: Some(self.food_level),
            food_tick_timer: Some(self.tick_timer),
            food_saturation_level: Some(self.saturation_level),
            food_exhaustion_level: Some(self.exhaustion_level),
        }
    }

    #[must_use]
    pub const fn food_level(&self) -> i32 {
        self.food_level
    }

    #[must_use]
    pub const fn has_enough_food(&self) -> bool {
        (self.food_level as f32) > (FoodConstants::SPRINT_LEVEL as f32)
    }

    #[must_use]
    pub const fn needs_food(&self) -> bool {
        self.food_level < FoodConstants::MAX_FOOD
    }

    pub fn add_exhaustion(&mut self, amount: f32) {
        self.exhaustion_level = min_f32(
            self.exhaustion_level + amount,
            FoodConstants::MAX_EXHAUSTION,
        );
    }

    #[must_use]
    pub const fn saturation_level(&self) -> f32 {
        self.saturation_level
    }

    #[must_use]
    pub const fn tick_timer(&self) -> i32 {
        self.tick_timer
    }

    pub fn set_food_level(&mut self, food: i32) {
        self.food_level = food;
    }

    pub fn set_saturation(&mut self, saturation: f32) {
        self.saturation_level = saturation;
    }
}
