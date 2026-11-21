pub mod experience;
pub mod food_constants;
pub mod food_data;
pub mod food_properties;
pub mod foods;
pub mod player_survival;

#[cfg(test)]
mod tests;

pub use experience::{ExperienceSaveData, ExperienceState, xp_needed_for_next_level};
pub use food_constants::FoodConstants;
pub use food_data::{FoodData, FoodSaveData};
pub use food_properties::{FoodProperties, FoodPropertiesBuilder};
pub use foods::Foods;
pub use player_survival::{MovementKind, PlayerSurvivalState, RestoreOutcome, SurvivalSaveData};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurvivalGameRules {
    pub natural_health_regeneration: bool,
    pub keep_inventory: bool,
}

impl Default for SurvivalGameRules {
    fn default() -> Self {
        Self {
            natural_health_regeneration: true,
            keep_inventory: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExhaustionConfig {
    pub jump_walk_exhaustion: f32,
    pub jump_sprint_exhaustion: f32,
    pub combat_exhaustion: f32,
    pub regen_exhaustion: f32,
    pub swim_multiplier: f32,
    pub sprint_multiplier: f32,
    pub other_multiplier: f32,
}

impl Default for ExhaustionConfig {
    fn default() -> Self {
        Self {
            jump_walk_exhaustion: FoodConstants::EXHAUSTION_JUMP,
            jump_sprint_exhaustion: FoodConstants::EXHAUSTION_SPRINT_JUMP,
            combat_exhaustion: FoodConstants::EXHAUSTION_ATTACK,
            regen_exhaustion: FoodConstants::EXHAUSTION_HEAL,
            swim_multiplier: FoodConstants::EXHAUSTION_SWIM,
            sprint_multiplier: FoodConstants::EXHAUSTION_SPRINT,
            other_multiplier: FoodConstants::EXHAUSTION_WALK,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegainReason {
    Satiated,
    Regen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExhaustionReason {
    Regen,
    Attack,
    Damaged,
    Jump,
    JumpSprint,
    Swim,
    WalkUnderwater,
    WalkOnWater,
    Sprint,
    Crouch,
    Walk,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurvivalEvent {
    FoodLevelChange {
        old_food_level: i32,
        new_food_level: i32,
    },
    SendHealthPacket {
        health: f32,
        food_level: i32,
        saturation: f32,
    },
    Heal {
        amount: f32,
        reason: RegainReason,
        is_fast_regen: bool,
    },
    StarveDamage {
        amount: f32,
    },
    Exhaustion {
        amount: f32,
        reason: ExhaustionReason,
    },
    LevelUpSound {
        volume: f32,
        pitch: f32,
    },
    SendExperiencePacket {
        progress: f32,
        total_experience: i32,
        level: i32,
    },
    PlayerLevelChange {
        old_level: i32,
        new_level: i32,
    },
}

/// Narrow state seam bridging E6 (`Player`/`ServerPlayer`) to C3 survival logic
/// without sharing `Player.java` ownership across issues.
pub trait SurvivalPlayerHost {
    fn health(&self) -> f32;
    fn max_health(&self) -> f32;
    fn is_invulnerable(&self) -> bool;
    fn may_fly(&self) -> bool;
    fn is_spectator(&self) -> bool;
    fn tick_count(&self) -> i32;

    fn is_hurt(&self) -> bool {
        let health = self.health();
        health > 0.0 && health < self.max_health()
    }

    fn heal(&mut self, amount: f32, reason: RegainReason, is_fast_regen: bool);
    fn hurt_starve(&mut self, amount: f32) -> bool;
    fn increase_score(&mut self, delta: i32);

    /// Synchronous Bukkit/Paper `FoodLevelChangeEvent` dispatch hook.
    /// Return `Some(food_level)` (possibly modified) to apply the change, or `None` to cancel.
    fn on_food_level_change(&mut self, _old_food_level: i32, new_food_level: i32) -> Option<i32> {
        Some(new_food_level)
    }

    /// Synchronous Bukkit/Paper `EntityExhaustionEvent` dispatch hook.
    /// Return `Some(exhaustion)` (possibly modified) to apply exhaustion, or `None` to cancel.
    fn on_exhaustion(&mut self, _reason: ExhaustionReason, exhaustion: f32) -> Option<f32> {
        Some(exhaustion)
    }
}
