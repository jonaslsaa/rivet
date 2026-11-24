//! Port of `net.minecraft.world.entity.ai.targeting.TargetingConditions` in Paper 26.2.

use crate::world::entity::ai::goal::context::{EntityId, Vec3};

/// Mirrors `net.minecraft.world.Difficulty` for peaceful combat target checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

/// Entity properties inspected by `TargetingConditions.test(level, targeter, target)`.
pub trait TargetEntityView {
    fn entity_id(&self) -> EntityId;
    fn pos(&self) -> Vec3;
    /// Mirrors `LivingEntity.canBeSeenByAnyone()`.
    fn can_be_seen_by_anyone(&self) -> bool;
    /// Mirrors `LivingEntity.canBeSeenAsEnemy()`.
    fn can_be_seen_as_enemy(&self) -> bool;
    /// Mirrors `targeter.canAttack(target)`.
    fn can_attack(&self, target_id: EntityId) -> bool;
    /// Mirrors `targeter.isAlliedTo(target)`.
    fn is_allied_to(&self, target_id: EntityId) -> bool;
    /// Mirrors `target.getVisibilityPercent(@Nullable Entity targeter)`.
    fn visibility_percent(&self, targeter: Option<EntityId>) -> f64;
    /// Mirrors `((Mob) targeter).getSensing().hasLineOfSight(target)`.
    fn has_line_of_sight(&self, target_id: EntityId) -> bool;
    /// Mirrors `targeter instanceof Mob`.
    fn is_mob(&self) -> bool {
        true
    }
}

/// Selector predicate matching `TargetingConditions.Selector` (`(LivingEntity, ServerLevel) -> bool`).
pub type TargetSelector<'a, E> = &'a dyn Fn(&E) -> bool;

/// Port of `net.minecraft.world.entity.ai.targeting.TargetingConditions` in Paper 26.2.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetingConditions {
    is_combat: bool,
    range: f64,
    check_line_of_sight: bool,
    test_invisible: bool,
}

impl TargetingConditions {
    pub const DEFAULT: Self = Self::for_combat();
    pub const MIN_VISIBILITY_DISTANCE_FOR_INVISIBLE_TARGET: f64 = 2.0;

    #[must_use]
    pub const fn for_combat() -> Self {
        Self {
            is_combat: true,
            range: -1.0,
            check_line_of_sight: true,
            test_invisible: true,
        }
    }

    #[must_use]
    pub const fn for_non_combat() -> Self {
        Self {
            is_combat: false,
            range: -1.0,
            check_line_of_sight: true,
            test_invisible: true,
        }
    }

    #[must_use]
    pub const fn copy(&self) -> Self {
        *self
    }

    #[must_use]
    pub const fn range(mut self, range: f64) -> Self {
        self.range = range;
        self
    }

    #[must_use]
    pub const fn ignore_line_of_sight(mut self) -> Self {
        self.check_line_of_sight = false;
        self
    }

    #[must_use]
    pub const fn ignore_invisibility_testing(mut self) -> Self {
        self.test_invisible = false;
        self
    }

    #[must_use]
    pub const fn is_combat(&self) -> bool {
        self.is_combat
    }

    #[must_use]
    pub const fn get_range(&self) -> f64 {
        self.range
    }

    #[must_use]
    pub const fn checks_line_of_sight(&self) -> bool {
        self.check_line_of_sight
    }

    #[must_use]
    pub const fn tests_invisible(&self) -> bool {
        self.test_invisible
    }

    /// Exact port of `TargetingConditions.test(ServerLevel, @Nullable LivingEntity targeter, LivingEntity target)`.
    #[must_use]
    pub fn test<E: TargetEntityView>(
        &self,
        difficulty: Difficulty,
        targeter: Option<&E>,
        target: &E,
        selector: Option<TargetSelector<'_, E>>,
    ) -> bool {
        if let Some(t) = targeter
            && t.entity_id() == target.entity_id()
        {
            return false;
        }
        if !target.can_be_seen_by_anyone() {
            return false;
        }
        if let Some(pred) = selector
            && !pred(target)
        {
            return false;
        }

        if let Some(targeter) = targeter {
            // `LivingEntity.canAttack` owns the enemy-visibility and peaceful
            // (player-only) rules; Paper checks no difficulty here.
            if self.is_combat
                && (!targeter.can_attack(target.entity_id())
                    || targeter.is_allied_to(target.entity_id()))
            {
                return false;
            }

            if self.range > 0.0 {
                let modifier = if self.test_invisible {
                    target.visibility_percent(Some(targeter.entity_id()))
                } else {
                    1.0
                };
                let visibility_distance = rivet_util::mth::max_f64(
                    self.range * modifier,
                    Self::MIN_VISIBILITY_DISTANCE_FOR_INVISIBLE_TARGET,
                );
                let dist_sqr = targeter.pos().distance_to_sqr(target.pos());
                if dist_sqr > visibility_distance * visibility_distance {
                    return false;
                }
            }

            if self.check_line_of_sight
                && targeter.is_mob()
                && !targeter.has_line_of_sight(target.entity_id())
            {
                return false;
            }
        } else if self.is_combat
            && (!target.can_be_seen_as_enemy() || difficulty == Difficulty::Peaceful)
        {
            return false;
        }

        true
    }
}
