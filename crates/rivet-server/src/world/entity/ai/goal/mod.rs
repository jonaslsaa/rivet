//! Paper 26.2 `net.minecraft.world.entity.ai.goal` scheduling and classic goal families.
//!
//! Mirrors `net.minecraft.world.entity.ai.goal.*` and `net.minecraft.world.entity.ai.goal.target.*`
//! at pinned Paper commit `0a993450f129c4942c2a9ed45ba047412b4667cf`.

pub mod context;
pub mod float_goal;
#[allow(clippy::module_inception)]
pub mod goal;
pub mod goal_selector;
pub mod leap_at_target_goal;
pub mod look_at_player_goal;
pub mod melee_attack_goal;
pub mod panic_goal;
pub mod random_look_around_goal;
pub mod random_stroll_goal;
pub mod target;
pub mod use_item_goal;

#[cfg(test)]
mod tests;

pub use context::{
    DAMAGE_TYPE_TAG_PANIC_CAUSES, DamageTypeTagId, EntitySnapshot, EntityTypeId, EquipmentSlot,
    GoalContext, GoalRandom, ItemStackRef, PathHandle, PathNode, SoundId, TargetReason,
    TargetingConditions, TeamId,
};
pub use float_goal::FloatGoal;
pub use goal::{
    Goal, GoalFlag, GoalFlags, GoalFlagsIter, GoalId, WrappedGoal, adjusted_tick_delay,
    reduced_tick_delay,
};
pub use goal_selector::{GoalSelector, tick_mob_selectors};
pub use leap_at_target_goal::LeapAtTargetGoal;
pub use look_at_player_goal::{LookAtPlayerGoal, LookAtTargetType};
pub use melee_attack_goal::MeleeAttackGoal;
pub use panic_goal::PanicGoal;
pub use random_look_around_goal::RandomLookAroundGoal;
pub use random_stroll_goal::{RandomStrollGoal, WaterAvoidingRandomStrollGoal};
pub use target::{
    AttackableTargetType, HurtByTargetGoal, NearestAttackableTargetGoal, OwnerHurtByTargetGoal,
    OwnerHurtTargetGoal, ResetUniversalAngerTargetGoal, TargetGoalState,
};
pub use use_item_goal::UseItemGoal;
