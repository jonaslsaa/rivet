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
