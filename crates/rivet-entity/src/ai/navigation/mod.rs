//! Port of Paper 26.2 pathfinding, navigation, movement/look/jump/body controls,
//! and target selection conditions (`net.minecraft.world.level.pathfinder.*`,
//! `net.minecraft.world.entity.ai.navigation.*`, `net.minecraft.world.entity.ai.control.*`,
//! `net.minecraft.world.entity.ai.targeting.TargetingConditions`).

pub mod control;
pub mod evaluator;
#[allow(clippy::module_inception)]
pub mod navigation;
pub mod node;
pub mod path;
pub mod path_finder;
pub mod path_type;
pub mod targeting;
pub mod world_view;

#[cfg(test)]
mod tests;

pub use control::{
    BodyRotationControl, FlyingMoveControl, JumpControl, LookControl, MoveControl, MoveOperation,
    SmoothSwimmingLookControl, SmoothSwimmingMoveControl, rotate_towards, rotlerp,
};
pub use evaluator::{
    AmphibiousNodeEvaluator, EvaluatorConfig, FlyNodeEvaluator, NodeEvaluator, SwimNodeEvaluator,
    WalkNodeEvaluator, check_neighbour_blocks, get_floor_level_static, get_path_type_static,
};
pub use navigation::{NavigationVariant, PathNavigation};
pub use node::{BinaryHeap, Node, NodeArena, Target};
pub use path::{Path, PathDebugData};
pub use path_finder::PathFinder;
pub use path_type::{
    Direction, PathComputationType, PathType, PathTypeCache, block_pos_as_long, fastutil_mix_long,
};
pub use targeting::{Difficulty, TargetEntityView, TargetSelector, TargetingConditions};
pub use world_view::{BlockPathInfo, MobPathingState, PathWorldView, PathfindingContext};
