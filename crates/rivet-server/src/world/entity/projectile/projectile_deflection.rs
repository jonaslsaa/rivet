//! Port of `net.minecraft.world.entity.projectile.ProjectileDeflection` (Paper 26.2).

use rivet_util::random::RandomSource;

use crate::world::entity::projectile::projectile::ProjectileState;
use crate::world::entity::projectile::world_view::EntitySnapshot;

/// `net.minecraft.world.entity.projectile.ProjectileDeflection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProjectileDeflection {
    #[default]
    None,
    Reverse,
    AimDeflect,
    MomentumDeflect,
}

impl ProjectileDeflection {
    /// Applies the Paper deflection transformation to `projectile`.
    pub fn apply<R: RandomSource>(
        self,
        projectile: &mut ProjectileState,
        entity: Option<&EntitySnapshot>,
        random: &mut R,
    ) {
        match self {
            Self::None => {}
            Self::Reverse => {
                let rotation = 170.0_f32 + random.next_float() * 20.0_f32;
                projectile.delta_movement = projectile.delta_movement.scale(-0.5);
                projectile.y_rot += rotation;
                projectile.y_rot_o += rotation;
                projectile.needs_sync = true;
            }
            Self::AimDeflect => {
                if let Some(e) = entity {
                    projectile.delta_movement = e.look_angle;
                    projectile.needs_sync = true;
                }
            }
            Self::MomentumDeflect => {
                if let Some(e) = entity {
                    projectile.delta_movement = e.delta_movement.normalize();
                    projectile.needs_sync = true;
                }
            }
        }
    }
}
