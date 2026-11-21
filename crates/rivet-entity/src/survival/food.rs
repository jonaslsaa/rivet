use super::{
    Difficulty, ExhaustionConfig, ExhaustionReason, RegainReason, SurvivalEvent, SurvivalGameRules,
    SurvivalPlayerHost,
};

pub struct FoodConstants;

impl FoodConstants {
    pub const MAX_FOOD: i32 = 20;
    pub const MAX_SATURATION: f32 = 20.0;
    pub const START_SATURATION: f32 = 5.0;
    pub const SATURATION_FLOOR: f32 = 2.5;
    pub const EXHAUSTION_DROP: f32 = 4.0;
    pub const HEALTH_TICK_COUNT: i32 = 80;
    pub const HEALTH_TICK_COUNT_SATURATED: i32 = 10;
    pub const HEAL_LEVEL: i32 = 18;
    pub const SPRINT_LEVEL: i32 = 6;
    pub const STARVE_LEVEL: i32 = 0;
    pub const FOOD_SATURATION_POOR: f32 = 0.1;
    pub const FOOD_SATURATION_LOW: f32 = 0.3;
    pub const FOOD_SATURATION_NORMAL: f32 = 0.6;
    pub const FOOD_SATURATION_GOOD: f32 = 0.8;
    pub const FOOD_SATURATION_MAX: f32 = 1.0;
    pub const FOOD_SATURATION_SUPERNATURAL: f32 = 1.2;
    pub const EXHAUSTION_HEAL: f32 = 6.0;
    pub const EXHAUSTION_JUMP: f32 = 0.05;
    pub const EXHAUSTION_SPRINT_JUMP: f32 = 0.2;
    pub const EXHAUSTION_MINE: f32 = 0.005;
    pub const EXHAUSTION_ATTACK: f32 = 0.1;
    pub const EXHAUSTION_WALK: f32 = 0.0;
    pub const EXHAUSTION_CROUCH: f32 = 0.0;
    pub const EXHAUSTION_SPRINT: f32 = 0.1;
    pub const EXHAUSTION_SWIM: f32 = 0.01;
    pub const MAX_EXHAUSTION: f32 = 40.0;

    #[must_use]
    pub const fn saturation_by_modifier(nutrition: i32, modifier: f32) -> f32 {
        nutrition as f32 * modifier * 2.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoodProperties {
    pub nutrition: i32,
    pub saturation: f32,
    pub can_always_eat: bool,
}

impl FoodProperties {
    #[must_use]
    pub const fn new(nutrition: i32, saturation: f32, can_always_eat: bool) -> Self {
        Self {
            nutrition,
            saturation,
            can_always_eat,
        }
    }

    #[must_use]
    pub const fn builder() -> FoodPropertiesBuilder {
        FoodPropertiesBuilder {
            nutrition: 0,
            saturation_modifier: 0.0,
            can_always_eat: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoodPropertiesBuilder {
    nutrition: i32,
    saturation_modifier: f32,
    can_always_eat: bool,
}

impl FoodPropertiesBuilder {
    #[must_use]
    pub const fn nutrition(mut self, nutrition: i32) -> Self {
        self.nutrition = nutrition;
        self
    }

    #[must_use]
    pub const fn saturation_modifier(mut self, saturation_modifier: f32) -> Self {
        self.saturation_modifier = saturation_modifier;
        self
    }

    #[must_use]
    pub const fn always_edible(mut self) -> Self {
        self.can_always_eat = true;
        self
    }

    #[must_use]
    pub const fn build(self) -> FoodProperties {
        let saturation =
            FoodConstants::saturation_by_modifier(self.nutrition, self.saturation_modifier);
        FoodProperties::new(self.nutrition, saturation, self.can_always_eat)
    }
}

pub struct Foods;

impl Foods {
    const fn stew(nutrition: i32) -> FoodPropertiesBuilder {
        FoodProperties::builder()
            .nutrition(nutrition)
            .saturation_modifier(FoodConstants::FOOD_SATURATION_NORMAL)
    }

    pub const APPLE: FoodProperties = FoodProperties::builder()
        .nutrition(4)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_LOW)
        .build();
    pub const BAKED_POTATO: FoodProperties = FoodProperties::builder()
        .nutrition(5)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_NORMAL)
        .build();
    pub const BEEF: FoodProperties = FoodProperties::builder()
        .nutrition(3)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_LOW)
        .build();
    pub const BEETROOT: FoodProperties = FoodProperties::builder()
        .nutrition(1)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_NORMAL)
        .build();
    pub const BEETROOT_SOUP: FoodProperties = Self::stew(6).build();
    pub const BREAD: FoodProperties = FoodProperties::builder()
        .nutrition(5)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_NORMAL)
        .build();
    pub const CARROT: FoodProperties = FoodProperties::builder()
        .nutrition(3)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_NORMAL)
        .build();
    pub const CHICKEN: FoodProperties = FoodProperties::builder()
        .nutrition(2)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_LOW)
        .build();
    pub const CHORUS_FRUIT: FoodProperties = FoodProperties::builder()
        .nutrition(4)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_LOW)
        .always_edible()
        .build();
    pub const COD: FoodProperties = FoodProperties::builder()
        .nutrition(2)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_POOR)
        .build();
    pub const COOKED_BEEF: FoodProperties = FoodProperties::builder()
        .nutrition(8)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_GOOD)
        .build();
    pub const COOKED_CHICKEN: FoodProperties = FoodProperties::builder()
        .nutrition(6)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_NORMAL)
        .build();
    pub const COOKED_COD: FoodProperties = FoodProperties::builder()
        .nutrition(5)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_NORMAL)
        .build();
    pub const COOKED_MUTTON: FoodProperties = FoodProperties::builder()
        .nutrition(6)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_GOOD)
        .build();
    pub const COOKED_PORKCHOP: FoodProperties = FoodProperties::builder()
        .nutrition(8)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_GOOD)
        .build();
    pub const COOKED_RABBIT: FoodProperties = FoodProperties::builder()
        .nutrition(5)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_NORMAL)
        .build();
    pub const COOKED_SALMON: FoodProperties = FoodProperties::builder()
        .nutrition(6)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_GOOD)
        .build();
    pub const COOKIE: FoodProperties = FoodProperties::builder()
        .nutrition(2)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_POOR)
        .build();
    pub const DRIED_KELP: FoodProperties = FoodProperties::builder()
        .nutrition(1)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_LOW)
        .build();
    pub const ENCHANTED_GOLDEN_APPLE: FoodProperties = FoodProperties::builder()
        .nutrition(4)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_SUPERNATURAL)
        .always_edible()
        .build();
    pub const GOLDEN_APPLE: FoodProperties = FoodProperties::builder()
        .nutrition(4)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_SUPERNATURAL)
        .always_edible()
        .build();
    pub const GOLDEN_CARROT: FoodProperties = FoodProperties::builder()
        .nutrition(6)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_SUPERNATURAL)
        .build();
    pub const HONEY_BOTTLE: FoodProperties = FoodProperties::builder()
        .nutrition(6)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_POOR)
        .always_edible()
        .build();
    pub const MELON_SLICE: FoodProperties = FoodProperties::builder()
        .nutrition(2)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_LOW)
        .build();
    pub const MUSHROOM_STEW: FoodProperties = Self::stew(6).build();
    pub const MUTTON: FoodProperties = FoodProperties::builder()
        .nutrition(2)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_LOW)
        .build();
    pub const POISONOUS_POTATO: FoodProperties = FoodProperties::builder()
        .nutrition(2)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_LOW)
        .build();
    pub const PORKCHOP: FoodProperties = FoodProperties::builder()
        .nutrition(3)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_LOW)
        .build();
    pub const POTATO: FoodProperties = FoodProperties::builder()
        .nutrition(1)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_LOW)
        .build();
    pub const PUFFERFISH: FoodProperties = FoodProperties::builder()
        .nutrition(1)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_POOR)
        .build();
    pub const PUMPKIN_PIE: FoodProperties = FoodProperties::builder()
        .nutrition(8)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_LOW)
        .build();
    pub const RABBIT: FoodProperties = FoodProperties::builder()
        .nutrition(3)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_LOW)
        .build();
    pub const RABBIT_STEW: FoodProperties = Self::stew(10).build();
    pub const ROTTEN_FLESH: FoodProperties = FoodProperties::builder()
        .nutrition(4)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_POOR)
        .build();
    pub const SALMON: FoodProperties = FoodProperties::builder()
        .nutrition(2)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_POOR)
        .build();
    pub const SPIDER_EYE: FoodProperties = FoodProperties::builder()
        .nutrition(2)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_GOOD)
        .build();
    pub const SUSPICIOUS_STEW: FoodProperties = Self::stew(6).always_edible().build();
    pub const SWEET_BERRIES: FoodProperties = FoodProperties::builder()
        .nutrition(2)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_POOR)
        .build();
    pub const GLOW_BERRIES: FoodProperties = FoodProperties::builder()
        .nutrition(2)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_POOR)
        .build();
    pub const TROPICAL_FISH: FoodProperties = FoodProperties::builder()
        .nutrition(1)
        .saturation_modifier(FoodConstants::FOOD_SATURATION_POOR)
        .build();
}

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
        self.food_level = mth_clamp_i32(
            food.wrapping_add(self.food_level),
            0,
            FoodConstants::MAX_FOOD,
        );
        self.saturation_level = mth_clamp_f32(
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
    /// Emits `FoodLevelChange` and immediate `SendHealthPacket` matching Paper line 40-44.
    pub fn eat_with_events(
        &mut self,
        properties: FoodProperties,
        current_health: f32,
        events: &mut Vec<SurvivalEvent>,
    ) {
        let old_food_level = self.food_level;
        let requested_food_level = properties.nutrition.wrapping_add(old_food_level);
        events.push(SurvivalEvent::FoodLevelChange {
            old_food_level,
            new_food_level: requested_food_level,
        });
        self.add(
            requested_food_level.wrapping_sub(old_food_level),
            properties.saturation,
        );
        events.push(SurvivalEvent::SendHealthPacket {
            health: current_health,
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
                self.saturation_level = java_max_f32(self.saturation_level - 1.0, 0.0);
            } else if difficulty != Difficulty::Peaceful {
                let old_food_level = self.food_level;
                let new_food_level = self.food_level.wrapping_sub(1).max(0);
                events.push(SurvivalEvent::FoodLevelChange {
                    old_food_level,
                    new_food_level,
                });
                self.food_level = new_food_level;
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
                let saturation_spent = java_min_f32(self.saturation_level, 6.0);
                let heal_amount = saturation_spent / 6.0;
                host.heal(heal_amount, RegainReason::Satiated, true);
                events.push(SurvivalEvent::Heal {
                    amount: heal_amount,
                    reason: RegainReason::Satiated,
                    is_fast_regen: true,
                });
                if !host.is_invulnerable() {
                    events.push(SurvivalEvent::Exhaustion {
                        amount: saturation_spent,
                        reason: ExhaustionReason::Regen,
                    });
                    self.add_exhaustion(saturation_spent);
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
                if !host.is_invulnerable() {
                    let regen_exhaustion = exhaustion_config.regen_exhaustion;
                    events.push(SurvivalEvent::Exhaustion {
                        amount: regen_exhaustion,
                        reason: ExhaustionReason::Regen,
                    });
                    self.add_exhaustion(regen_exhaustion);
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
        self.exhaustion_level = java_min_f32(
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

/// Java `Mth.clamp(int, int, int)`: `Math.min(Math.max(value, min), max)`.
#[inline]
pub(crate) fn mth_clamp_i32(value: i32, min: i32, max: i32) -> i32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Java `Mth.clamp(float, float, float)`: `value < min ? min : Math.min(value, max)`.
#[inline]
pub(crate) fn mth_clamp_f32(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else {
        java_min_f32(value, max)
    }
}

/// Java `Math.min(float, float)` semantics (NaN propagates).
#[inline]
pub(crate) fn java_min_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 && b.is_sign_negative() {
        b
    } else if a <= b {
        a
    } else {
        b
    }
}

/// Java `Math.max(float, float)` semantics (NaN propagates).
#[inline]
pub(crate) fn java_max_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        b
    } else if a >= b {
        a
    } else {
        b
    }
}
