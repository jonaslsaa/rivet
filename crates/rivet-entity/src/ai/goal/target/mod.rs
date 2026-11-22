//! Paper 26.2 `net.minecraft.world.entity.ai.goal.target` family.

pub mod hurt_by_target_goal;
pub mod nearest_attackable_target_goal;
pub mod owner_hurt_by_target_goal;
pub mod owner_hurt_target_goal;
pub mod reset_universal_anger_target_goal;
pub mod target_goal;

pub use hurt_by_target_goal::HurtByTargetGoal;
pub use nearest_attackable_target_goal::{AttackableTargetType, NearestAttackableTargetGoal};
pub use owner_hurt_by_target_goal::OwnerHurtByTargetGoal;
pub use owner_hurt_target_goal::OwnerHurtTargetGoal;
pub use reset_universal_anger_target_goal::ResetUniversalAngerTargetGoal;
pub use target_goal::TargetGoalState;
