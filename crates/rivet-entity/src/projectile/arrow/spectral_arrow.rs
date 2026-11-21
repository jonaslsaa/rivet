//! Port of `net.minecraft.world.entity.projectile.arrow.SpectralArrow` (Paper 26.2).

use rivet_nbt::compound_tag::CompoundTag;
use rivet_util::random::RandomSource;

use crate::projectile::arrow::abstract_arrow::{AbstractArrow, ArrowHooks};
use crate::projectile::phys::{EntityId, Vec3};
use crate::projectile::world_view::{PickupItem, PostHurtEffect, ProjectileWorld};

/// `SpectralArrow.DEFAULT_DURATION` (`200` ticks = 10 seconds).
pub const DEFAULT_DURATION: i32 = 200;

/// Port of `net.minecraft.world.entity.projectile.arrow.SpectralArrow`.
#[derive(Debug, Clone, PartialEq)]
pub struct SpectralArrow {
    pub abstract_arrow: AbstractArrow,
    pub duration: i32,
}

impl SpectralArrow {
    pub fn new(id: EntityId, position: Vec3) -> Self {
        Self {
            abstract_arrow: AbstractArrow::new(
                id,
                position,
                PickupItem::simple("minecraft:spectral_arrow"),
            ),
            duration: DEFAULT_DURATION,
        }
    }

    pub fn tick<W: ProjectileWorld, R: RandomSource>(&mut self, world: &mut W, random: &mut R) {
        let mut hooks = SpectralArrowHooks {
            duration: self.duration,
        };
        self.abstract_arrow
            .tick_with_hooks(world, random, &mut hooks);
    }

    /// `SpectralArrow.addAdditionalSaveData(ValueOutput)`.
    pub fn save_nbt(&self, tag: &mut CompoundTag) {
        self.abstract_arrow.save_nbt(tag);
        tag.put_int("Duration", self.duration);
    }

    /// `SpectralArrow.readAdditionalSaveData(ValueInput)`.
    pub fn load_nbt(&mut self, tag: &CompoundTag) {
        self.abstract_arrow.load_nbt(tag);
        self.duration = tag.get_int_or("Duration", DEFAULT_DURATION);
    }
}

struct SpectralArrowHooks {
    duration: i32,
}

impl ArrowHooks for SpectralArrowHooks {
    fn post_hurt_effects<W: ProjectileWorld>(
        &mut self,
        arrow: &AbstractArrow,
        world: &mut W,
        target_id: EntityId,
    ) {
        let source = arrow.projectile.get_effect_source(world);
        world.apply_post_hurt_effect(
            target_id,
            PostHurtEffect {
                effect_id: "minecraft:glowing".to_owned(),
                duration: self.duration,
                amplifier: 0,
                effect_source: source,
            },
        );
    }
}
