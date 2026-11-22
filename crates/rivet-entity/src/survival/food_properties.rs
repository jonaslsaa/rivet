use super::food_constants::FoodConstants;

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
