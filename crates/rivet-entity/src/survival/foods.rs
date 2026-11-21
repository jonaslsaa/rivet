use super::food_constants::FoodConstants;
use super::food_properties::{FoodProperties, FoodPropertiesBuilder};

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
