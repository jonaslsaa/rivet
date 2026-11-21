//! Port of `net.minecraft.world.entity.projectile.arrow.Arrow` (Paper 26.2).

use rivet_nbt::compound_tag::CompoundTag;
use rivet_util::random::RandomSource;

use crate::projectile::arrow::abstract_arrow::{AbstractArrow, ArrowHooks};
use crate::projectile::phys::{EntityId, Vec3};
use crate::projectile::world_view::{PickupItem, PostHurtEffect, ProjectileWorld};

/// `Arrow.EXPOSED_POTION_DECAY_TIME`.
pub const EXPOSED_POTION_DECAY_TIME: i32 = 600;
/// `Arrow.NO_EFFECT_COLOR`.
pub const NO_EFFECT_COLOR: i32 = -1;
/// Paper default potion duration scale for tipped arrows (`0.125F`).
pub const TIPPED_ARROW_DURATION_SCALE: f32 = 0.125;

/// Single mob effect carried by a tipped `Arrow`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PotionEffect {
    pub effect_id: String,
    pub duration: i32,
    pub amplifier: i32,
}

/// Potion contents attached to a tipped `Arrow` (`PotionContents`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PotionContents {
    pub effects: Vec<PotionEffect>,
    pub custom_color: Option<i32>,
}

impl PotionContents {
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty() && self.custom_color.is_none()
    }

    pub fn color(&self) -> i32 {
        if let Some(c) = self.custom_color {
            return c;
        }
        if self.effects.is_empty() {
            NO_EFFECT_COLOR
        } else {
            // Default Minecraft water/potion color when effects exist without custom color.
            0x385DC6
        }
    }
}

/// Port of `net.minecraft.world.entity.projectile.arrow.Arrow`.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrow {
    pub abstract_arrow: AbstractArrow,
    pub effect_color: i32,
    pub potion_duration_scale: f32,
}

impl Arrow {
    pub fn new(id: EntityId, position: Vec3) -> Self {
        Self {
            abstract_arrow: AbstractArrow::new(id, position, PickupItem::simple("minecraft:arrow")),
            effect_color: NO_EFFECT_COLOR,
            potion_duration_scale: TIPPED_ARROW_DURATION_SCALE,
        }
    }

    pub fn get_potion_contents(&self) -> Option<&PotionContents> {
        self.abstract_arrow.pickup_item.potion_contents.as_ref()
    }

    /// `Arrow.setPotionContents(PotionContents)`.
    pub fn set_potion_contents(&mut self, contents: Option<PotionContents>) {
        match contents {
            Some(c) if !c.is_empty() => {
                self.abstract_arrow.pickup_item.item_id = "minecraft:tipped_arrow".to_owned();
                self.abstract_arrow.pickup_item.potion_contents = Some(c);
            }
            _ => {
                self.abstract_arrow.pickup_item.item_id = "minecraft:arrow".to_owned();
                self.abstract_arrow.pickup_item.potion_contents = None;
            }
        }
        self.update_color();
    }

    /// `Arrow.addEffect(MobEffectInstance)`.
    pub fn add_effect(&mut self, effect: PotionEffect) {
        let mut contents = self
            .abstract_arrow
            .pickup_item
            .potion_contents
            .take()
            .unwrap_or_default();
        contents.effects.push(effect);
        self.set_potion_contents(Some(contents));
    }

    /// `Arrow.updateColor()`.
    pub fn update_color(&mut self) {
        self.effect_color = self
            .get_potion_contents()
            .map_or(NO_EFFECT_COLOR, PotionContents::color);
    }

    /// `Arrow.tick()`: ticks `AbstractArrow` with tipped-arrow post-hurt hooks and decays
    /// potion effects after `EXPOSED_POTION_DECAY_TIME` (600) ticks in the ground.
    pub fn tick<W: ProjectileWorld, R: RandomSource>(&mut self, world: &mut W, random: &mut R) {
        let mut hooks = TippedArrowHooks {
            potion_duration_scale: self.potion_duration_scale,
        };
        self.abstract_arrow
            .tick_with_hooks(world, random, &mut hooks);

        if self.abstract_arrow.in_ground
            && self.abstract_arrow.in_ground_time != 0
            && self.get_potion_contents().is_some_and(|p| !p.is_empty())
            && self.abstract_arrow.in_ground_time >= EXPOSED_POTION_DECAY_TIME
        {
            self.set_potion_contents(None);
        }
    }

    pub fn save_nbt(&self, tag: &mut CompoundTag) {
        self.abstract_arrow.save_nbt(tag);
        if let Some(contents) = self.get_potion_contents()
            && let Some(color) = contents.custom_color
        {
            tag.put_int("CustomPotionColor", color);
        }
    }

    pub fn load_nbt(&mut self, tag: &CompoundTag) {
        self.abstract_arrow.load_nbt(tag);
        if let Some(color) = tag.get_int("CustomPotionColor") {
            let mut contents = self
                .abstract_arrow
                .pickup_item
                .potion_contents
                .take()
                .unwrap_or_default();
            contents.custom_color = Some(color);
            self.set_potion_contents(Some(contents));
        } else {
            self.update_color();
        }
    }
}

struct TippedArrowHooks {
    potion_duration_scale: f32,
}

impl ArrowHooks for TippedArrowHooks {
    fn post_hurt_effects<W: ProjectileWorld>(
        &mut self,
        arrow: &AbstractArrow,
        world: &mut W,
        target_id: EntityId,
    ) {
        let Some(contents) = arrow.pickup_item.potion_contents.as_ref() else {
            return;
        };
        let source = arrow.projectile.get_effect_source(world);
        for effect in &contents.effects {
            let scaled_duration =
                ((effect.duration as f32) * self.potion_duration_scale).round() as i32;
            let duration = scaled_duration.max(1);
            world.apply_post_hurt_effect(
                target_id,
                PostHurtEffect {
                    effect_id: effect.effect_id.clone(),
                    duration,
                    amplifier: effect.amplifier,
                    effect_source: source,
                },
            );
        }
    }
}
