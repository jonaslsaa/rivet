//! Port of Paper 26.2 `net.minecraft.world.entity.ai.navigation.*`:
//! - `PathNavigation`
//! - `GroundPathNavigation`
//! - `FlyingPathNavigation`
//! - `WaterBoundPathNavigation`
//! - `AmphibiousPathNavigation`
//! - `WallClimberNavigation`

use rivet_util::mth;

use crate::ai::goal::context::{BlockPos, Vec3};
use crate::ai::navigation::control::MoveControl;
use crate::ai::navigation::evaluator::{
    AmphibiousNodeEvaluator, FlyNodeEvaluator, NodeEvaluator, SwimNodeEvaluator, WalkNodeEvaluator,
};
use crate::ai::navigation::path::Path;
use crate::ai::navigation::path_finder::PathFinder;
use crate::ai::navigation::path_type::PathType;
use crate::ai::navigation::world_view::{MobPathingState, PathWorldView};

/// Concrete navigation variant mirroring Paper 26.2's `PathNavigation` subclasses.
#[derive(Debug)]
pub enum NavigationVariant {
    Ground {
        evaluator: WalkNodeEvaluator,
        avoid_sun: bool,
        can_path_to_targets_below_surface: bool,
    },
    Flying {
        evaluator: FlyNodeEvaluator,
    },
    WaterBound {
        evaluator: SwimNodeEvaluator,
        allow_breaching: bool,
    },
    Amphibious {
        evaluator: AmphibiousNodeEvaluator,
    },
    WallClimber {
        evaluator: WalkNodeEvaluator,
        avoid_sun: bool,
        can_path_to_targets_below_surface: bool,
        path_to_position: Option<BlockPos>,
    },
}

/// Port of `net.minecraft.world.entity.ai.navigation.PathNavigation` in Paper 26.2.
#[derive(Debug)]
pub struct PathNavigation {
    pub variant: NavigationVariant,
    pub path: Option<Path>,
    pub speed_modifier: f64,
    pub tick: i32,
    last_stucked_check: i32,
    last_stucked_check_pos: Vec3,
    timeout_cached_node: Option<BlockPos>,
    timeout_timer: i64,
    last_timeout_check: i64,
    timeout_limit: f64,
    pub max_distance_to_waypoint: f32,
    pub has_delayed_recomputation: bool,
    time_last_recompute: i64,
    target_pos: Option<BlockPos>,
    reach_range: i32,
    max_visited_nodes_multiplier: f32,
    path_finder: PathFinder,
    is_stuck: bool,
    required_path_length: f32,
}

impl PathNavigation {
    pub const MAX_TIME_RECOMPUTE: i64 = 20;
    pub const STUCK_CHECK_INTERVAL: i32 = 100;
    pub const DEFAULT_FOLLOW_RANGE: f32 = 16.0;

    #[must_use]
    pub fn ground(max_visited_nodes: i32) -> Self {
        Self::with_variant(
            NavigationVariant::Ground {
                evaluator: WalkNodeEvaluator::new(),
                avoid_sun: false,
                can_path_to_targets_below_surface: false,
            },
            max_visited_nodes,
        )
    }

    #[must_use]
    pub fn flying(max_visited_nodes: i32) -> Self {
        Self::with_variant(
            NavigationVariant::Flying {
                evaluator: FlyNodeEvaluator::new(),
            },
            max_visited_nodes,
        )
    }

    #[must_use]
    pub fn water_bound(max_visited_nodes: i32, allow_breaching: bool) -> Self {
        Self::with_variant(
            NavigationVariant::WaterBound {
                evaluator: SwimNodeEvaluator::new(allow_breaching),
                allow_breaching,
            },
            max_visited_nodes,
        )
    }

    #[must_use]
    pub fn amphibious(max_visited_nodes: i32) -> Self {
        Self::with_variant(
            NavigationVariant::Amphibious {
                evaluator: AmphibiousNodeEvaluator::new(false),
            },
            max_visited_nodes,
        )
    }

    #[must_use]
    pub fn wall_climber(max_visited_nodes: i32) -> Self {
        Self::with_variant(
            NavigationVariant::WallClimber {
                evaluator: WalkNodeEvaluator::new(),
                avoid_sun: false,
                can_path_to_targets_below_surface: false,
                path_to_position: None,
            },
            max_visited_nodes,
        )
    }

    #[must_use]
    pub fn with_variant(variant: NavigationVariant, max_visited_nodes: i32) -> Self {
        Self {
            variant,
            path: None,
            speed_modifier: 1.0,
            tick: 0,
            last_stucked_check: 0,
            last_stucked_check_pos: Vec3::ZERO,
            timeout_cached_node: None,
            timeout_timer: 0,
            last_timeout_check: 0,
            timeout_limit: 0.0,
            max_distance_to_waypoint: 0.5,
            has_delayed_recomputation: false,
            time_last_recompute: 0,
            target_pos: None,
            reach_range: 0,
            max_visited_nodes_multiplier: 1.0,
            path_finder: PathFinder::new(max_visited_nodes),
            is_stuck: false,
            required_path_length: Self::DEFAULT_FOLLOW_RANGE,
        }
    }

    pub fn set_follow_range(&mut self, follow_range: f32) {
        self.required_path_length = follow_range;
    }

    pub fn set_speed_modifier(&mut self, speed_modifier: f64) {
        self.speed_modifier = speed_modifier;
    }

    pub fn set_max_visited_nodes_multiplier(&mut self, multiplier: f32) {
        self.max_visited_nodes_multiplier = multiplier;
    }

    pub fn reset_max_visited_nodes_multiplier(&mut self) {
        self.max_visited_nodes_multiplier = 1.0;
    }

    pub fn set_can_float(&mut self, can_float: bool) {
        match &mut self.variant {
            NavigationVariant::Ground { evaluator, .. }
            | NavigationVariant::WallClimber { evaluator, .. } => {
                evaluator.config.can_float = can_float;
            }
            NavigationVariant::Flying { evaluator } => {
                evaluator.config_mut().can_float = can_float;
            }
            // WaterBoundPathNavigation and AmphibiousPathNavigation ignore setCanFloat
            NavigationVariant::WaterBound { .. } | NavigationVariant::Amphibious { .. } => {}
        }
    }

    pub fn set_can_open_doors(&mut self, can_open_doors: bool) {
        match &mut self.variant {
            NavigationVariant::Ground { evaluator, .. }
            | NavigationVariant::WallClimber { evaluator, .. } => {
                evaluator.config.can_open_doors = can_open_doors;
            }
            NavigationVariant::Flying { evaluator } => {
                evaluator.config_mut().can_open_doors = can_open_doors;
            }
            NavigationVariant::WaterBound { evaluator, .. } => {
                evaluator.config.can_open_doors = can_open_doors;
            }
            NavigationVariant::Amphibious { evaluator } => {
                evaluator.config_mut().can_open_doors = can_open_doors;
            }
        }
    }

    pub fn set_avoid_sun(&mut self, avoid_sun: bool) {
        if let NavigationVariant::Ground {
            avoid_sun: flag, ..
        }
        | NavigationVariant::WallClimber {
            avoid_sun: flag, ..
        } = &mut self.variant
        {
            *flag = avoid_sun;
        }
    }

    pub fn set_can_walk_over_fences(&mut self, can_walk_over_fences: bool) {
        if let NavigationVariant::Ground { evaluator, .. }
        | NavigationVariant::WallClimber { evaluator, .. } = &mut self.variant
        {
            evaluator.config.can_walk_over_fences = can_walk_over_fences;
        }
    }

    #[must_use]
    pub const fn can_navigate_ground(&self) -> bool {
        matches!(
            self.variant,
            NavigationVariant::Ground { .. }
                | NavigationVariant::Amphibious { .. }
                | NavigationVariant::WallClimber { .. }
        )
    }

    #[must_use]
    pub fn is_done(&self) -> bool {
        self.path.as_ref().is_none_or(Path::is_done)
    }

    #[must_use]
    pub fn is_in_progress(&self) -> bool {
        !self.is_done()
    }

    #[must_use]
    pub const fn is_stuck(&self) -> bool {
        self.is_stuck
    }

    #[must_use]
    pub const fn target_pos(&self) -> Option<BlockPos> {
        self.target_pos
    }

    pub fn stop(&mut self) {
        self.path = None;
    }

    /// Mirrors `PathNavigation.canCutCorner(PathType)` in Paper 26.2.
    #[must_use]
    pub const fn can_cut_corner(path_type: PathType) -> bool {
        !matches!(
            path_type,
            PathType::DangerFire | PathType::DangerOther | PathType::WalkableDoor
        )
    }

    /// Mirrors `PathNavigation.canUpdatePath()`.
    #[must_use]
    pub fn can_update_path(&self, mob: &MobPathingState) -> bool {
        match &self.variant {
            NavigationVariant::Ground { .. } | NavigationVariant::WallClimber { .. } => {
                mob.on_ground || mob.is_in_liquid() || mob.is_passenger
            }
            NavigationVariant::Flying { evaluator } => {
                (evaluator.config().can_float && mob.is_in_liquid()) || !mob.is_passenger
            }
            NavigationVariant::WaterBound {
                allow_breaching, ..
            } => *allow_breaching || mob.is_in_liquid(),
            NavigationVariant::Amphibious { .. } => true,
        }
    }

    /// Mirrors `PathNavigation.getTempMobPos()`.
    #[must_use]
    pub fn get_temp_mob_pos<W: PathWorldView + ?Sized>(
        &self,
        mob: &MobPathingState,
        world: &W,
    ) -> Vec3 {
        match &self.variant {
            NavigationVariant::Ground { .. } | NavigationVariant::WallClimber { .. } => {
                let mut y = mth::floor_d(mob.pos.y + 0.5);
                if self.can_float_flag() && mob.in_water {
                    let bx = mth::floor_d(mob.pos.x);
                    let bz = mth::floor_d(mob.pos.z);
                    let mut steps = 0;
                    while world.block_info(BlockPos::new(bx, y, bz)).is_water && steps < 16 {
                        y += 1;
                        steps += 1;
                    }
                }
                Vec3::new(mob.pos.x, y as f64, mob.pos.z)
            }
            NavigationVariant::Flying { .. } => mob.pos,
            NavigationVariant::WaterBound { .. } | NavigationVariant::Amphibious { .. } => {
                Vec3::new(
                    mob.pos.x,
                    mob.pos.y + f64::from(mob.bb_height) * 0.5,
                    mob.pos.z,
                )
            }
        }
    }

    fn can_float_flag(&self) -> bool {
        match &self.variant {
            NavigationVariant::Ground { evaluator, .. }
            | NavigationVariant::WallClimber { evaluator, .. } => evaluator.config.can_float,
            NavigationVariant::Flying { evaluator } => evaluator.config().can_float,
            NavigationVariant::WaterBound { .. } | NavigationVariant::Amphibious { .. } => false,
        }
    }

    /// Mirrors `PathNavigation.createPath(BlockPos, int accuracy)` in Paper 26.2.
    pub fn create_path<W: PathWorldView + ?Sized>(
        &mut self,
        mob: &mut MobPathingState,
        world: &W,
        mut target: BlockPos,
        accuracy: i32,
        game_time: i64,
    ) -> Option<Path> {
        // WallClimberNavigation records target position for direct fallback
        if let NavigationVariant::WallClimber {
            path_to_position, ..
        } = &mut self.variant
        {
            *path_to_position = Some(target);
        }

        // GroundPathNavigation surface adjustment
        if let NavigationVariant::Ground {
            can_path_to_targets_below_surface,
            ..
        }
        | NavigationVariant::WallClimber {
            can_path_to_targets_below_surface,
            ..
        } = &self.variant
        {
            let info = world.block_info(target);
            if info.is_air {
                let mut probe = BlockPos::new(target.x, target.y - 1, target.z);
                while probe.y >= world.min_y() && world.block_info(probe).is_air {
                    probe.y -= 1;
                }
                if probe.y >= world.min_y() {
                    target = BlockPos::new(probe.x, probe.y + 1, probe.z);
                }
            } else if !*can_path_to_targets_below_surface && info.has_collision() {
                let mut probe = BlockPos::new(target.x, target.y + 1, target.z);
                while probe.y < world.max_y() && world.block_info(probe).has_collision() {
                    probe.y += 1;
                }
                target = probe;
            }
        }

        self.create_path_targets(
            mob,
            world,
            &[target],
            accuracy,
            self.required_path_length,
            game_time,
        )
    }

    /// Mirrors `PathNavigation.createPath(Set<BlockPos>, int, boolean, int, float)`.
    pub fn create_path_targets<W: PathWorldView + ?Sized>(
        &mut self,
        mob: &mut MobPathingState,
        world: &W,
        targets: &[BlockPos],
        accuracy: i32,
        follow_range: f32,
        game_time: i64,
    ) -> Option<Path> {
        if targets.is_empty() {
            return None;
        }
        if mob.pos.y < world.min_y() as f64 {
            return None;
        }
        if !self.can_update_path(mob) {
            return None;
        }
        if let Some(existing) = &self.path
            && !existing.is_done()
            && self.target_pos.is_some_and(|tp| targets.contains(&tp))
        {
            return Some(existing.copy());
        }

        let max_visited_mult = self.max_visited_nodes_multiplier;
        let mut path = match &mut self.variant {
            NavigationVariant::Ground { evaluator, .. }
            | NavigationVariant::WallClimber { evaluator, .. } => self.path_finder.find_path(
                evaluator,
                world,
                mob,
                targets,
                follow_range,
                accuracy,
                max_visited_mult,
            ),
            NavigationVariant::Flying { evaluator } => self.path_finder.find_path(
                evaluator,
                world,
                mob,
                targets,
                follow_range,
                accuracy,
                max_visited_mult,
            ),
            NavigationVariant::WaterBound { evaluator, .. } => self.path_finder.find_path(
                evaluator,
                world,
                mob,
                targets,
                follow_range,
                accuracy,
                max_visited_mult,
            ),
            NavigationVariant::Amphibious { evaluator } => self.path_finder.find_path(
                evaluator,
                world,
                mob,
                targets,
                follow_range,
                accuracy,
                max_visited_mult,
            ),
        };

        if let Some(ref mut p) = path {
            self.trim_path(p, mob, world);
            self.target_pos = Some(p.get_target());
            self.reach_range = accuracy;
            self.reset_stucked_entities_position(mob, game_time);
        }
        path
    }

    /// Mirrors `GroundPathNavigation.trimPath()` (avoid-sun truncation).
    fn trim_path<W: PathWorldView + ?Sized>(
        &self,
        path: &mut Path,
        mob: &MobPathingState,
        world: &W,
    ) {
        let avoid_sun = matches!(
            self.variant,
            NavigationVariant::Ground {
                avoid_sun: true,
                ..
            } | NavigationVariant::WallClimber {
                avoid_sun: true,
                ..
            }
        );
        if avoid_sun
            && world.is_bright_outside()
            && !mob.has_helmet
            && !world.block_info(mob.block_position()).can_see_sky
        {
            for i in 0..path.get_node_count() {
                let node = path.get_node(i);
                if world.block_info(node.as_block_pos()).can_see_sky {
                    path.truncate_nodes(i);
                    return;
                }
            }
        }
    }

    /// Mirrors `PathNavigation.moveTo(double x, double y, double z, double speedModifier)`.
    #[allow(clippy::too_many_arguments)]
    pub fn move_to_xyz<W: PathWorldView + ?Sized>(
        &mut self,
        mob: &mut MobPathingState,
        world: &W,
        x: f64,
        y: f64,
        z: f64,
        speed_modifier: f64,
        game_time: i64,
    ) -> bool {
        let target = BlockPos::new(mth::floor_d(x), mth::floor_d(y), mth::floor_d(z));
        let path = self.create_path(mob, world, target, 1, game_time);
        self.move_to_path(path, speed_modifier, mob, world, game_time)
    }

    /// Mirrors `PathNavigation.moveTo(@Nullable Path newPath, double speedModifier)`.
    pub fn move_to_path<W: PathWorldView + ?Sized>(
        &mut self,
        new_path: Option<Path>,
        speed_modifier: f64,
        mob: &MobPathingState,
        world: &W,
        game_time: i64,
    ) -> bool {
        let Some(new_path) = new_path else {
            self.path = None;
            return false;
        };

        if !new_path.same_as(self.path.as_ref()) {
            self.path = Some(new_path);
        }

        if self.is_done() {
            return false;
        }

        if let Some(ref p) = self.path
            && p.get_node_count() == 0
        {
            return false;
        }

        self.speed_modifier = speed_modifier;
        let temp_pos = self.get_temp_mob_pos(mob, world);
        self.last_stucked_check = self.tick;
        self.last_stucked_check_pos = temp_pos;
        let _ = game_time;
        true
    }

    /// Mirrors `PathNavigation.recomputePath()`.
    pub fn recompute_path<W: PathWorldView + ?Sized>(
        &mut self,
        mob: &mut MobPathingState,
        world: &W,
        game_time: i64,
    ) {
        if game_time - self.time_last_recompute > Self::MAX_TIME_RECOMPUTE {
            if let Some(target_pos) = self.target_pos {
                self.path = None;
                self.path = self.create_path(mob, world, target_pos, self.reach_range, game_time);
                self.time_last_recompute = game_time;
                self.has_delayed_recomputation = false;
            }
        } else {
            self.has_delayed_recomputation = true;
        }
    }

    /// Mirrors `PathNavigation.tick()` (including `FlyingPathNavigation` and
    /// `WallClimberNavigation` overrides) in Paper 26.2.
    pub fn tick<W: PathWorldView + ?Sized>(
        &mut self,
        mob: &mut MobPathingState,
        world: &W,
        move_control: &mut MoveControl,
        game_time: i64,
    ) {
        self.tick += 1;
        if self.has_delayed_recomputation {
            self.recompute_path(mob, world, game_time);
        }

        if self.is_done() {
            // WallClimberNavigation direct fallback when no path exists
            if let NavigationVariant::WallClimber {
                path_to_position, ..
            } = &mut self.variant
                && let Some(target) = *path_to_position
            {
                let closer_to_center = |bp: BlockPos| {
                    let dx = bp.x as f64 + 0.5 - mob.pos.x;
                    let dy = bp.y as f64 + 0.5 - mob.pos.y;
                    let dz = bp.z as f64 + 0.5 - mob.pos.z;
                    let dist_sqr = dx * dx + dy * dy + dz * dz;
                    let radius = f64::from(mob.bb_width);
                    dist_sqr < radius * radius
                };
                let at_mob_y = BlockPos::new(target.x, mth::floor_d(mob.pos.y), target.z);
                if closer_to_center(target)
                    || (mob.pos.y > target.y as f64 && closer_to_center(at_mob_y))
                {
                    *path_to_position = None;
                } else {
                    move_control.set_wanted_position(
                        target.x as f64,
                        target.y as f64,
                        target.z as f64,
                        self.speed_modifier,
                    );
                }
            }
            return;
        }

        if self.can_update_path(mob) {
            self.follow_the_path(mob, world, game_time);
        } else {
            let temp_pos = self.get_temp_mob_pos(mob, world);
            if let Some(ref mut path) = self.path
                && path.get_next_node_index() < path.get_node_count()
            {
                let next_pos =
                    path.get_entity_pos_at_node(mob.bb_width, path.get_next_node_index());
                if temp_pos.y > next_pos.y
                    && !mob.on_ground
                    && mth::floor_d(temp_pos.x) == mth::floor_d(next_pos.x)
                    && mth::floor_d(temp_pos.z) == mth::floor_d(next_pos.z)
                {
                    path.advance();
                }
            }
        }

        if !self.is_done() {
            let next_pos = self
                .path
                .as_ref()
                .unwrap()
                .get_next_entity_pos(mob.bb_width);
            let target_y = match self.variant {
                NavigationVariant::Ground { .. } | NavigationVariant::WallClimber { .. } => {
                    let bp = BlockPos::new(
                        mth::floor_d(next_pos.x),
                        mth::floor_d(next_pos.y),
                        mth::floor_d(next_pos.z),
                    );
                    let below = BlockPos::new(bp.x, bp.y - 1, bp.z);
                    let info = world.block_info(below);
                    if info.is_air {
                        next_pos.y
                    } else {
                        below.y as f64 + info.collision_max_y.unwrap_or(0.0)
                    }
                }
                _ => next_pos.y,
            };
            move_control.set_wanted_position(next_pos.x, target_y, next_pos.z, self.speed_modifier);
        }
    }

    /// Mirrors `PathNavigation.followThePath()` in Paper 26.2.
    fn follow_the_path<W: PathWorldView + ?Sized>(
        &mut self,
        mob: &MobPathingState,
        world: &W,
        game_time: i64,
    ) {
        let temp_pos = self.get_temp_mob_pos(mob, world);
        self.max_distance_to_waypoint = if mob.bb_width > 0.75 {
            mob.bb_width / 2.0
        } else {
            0.75 - mob.bb_width / 2.0
        };

        let max_y_dist = match self.variant {
            NavigationVariant::WaterBound { .. } => 0.5_f64,
            _ => 1.0_f64,
        };

        let should_advance = if let Some(ref path) = self.path {
            let target_block = path.get_next_node_pos();
            let dx = (mob.pos.x - (target_block.x as f64 + 0.5)).abs();
            let dy = (mob.pos.y - target_block.y as f64).abs();
            let dz = (mob.pos.z - (target_block.z as f64 + 0.5)).abs();
            let close_enough = dx < f64::from(self.max_distance_to_waypoint)
                && dz < f64::from(self.max_distance_to_waypoint)
                && dy < max_y_dist;

            let can_cut = Self::can_cut_corner(path.get_next_node().path_type)
                && self.should_target_next_node_in_direction(mob, world, temp_pos, path);
            close_enough || can_cut
        } else {
            false
        };

        if should_advance && let Some(ref mut path) = self.path {
            path.advance();
        }

        self.do_stucked_detection(mob, temp_pos, game_time);
    }

    fn should_target_next_node_in_direction<W: PathWorldView + ?Sized>(
        &self,
        mob: &MobPathingState,
        world: &W,
        mob_position: Vec3,
        path: &Path,
    ) -> bool {
        if path.get_next_node_index() + 1 >= path.get_node_count() {
            return false;
        }

        let cur = path.get_next_node_pos();
        let current_node = Vec3::new(cur.x as f64 + 0.5, cur.y as f64, cur.z as f64 + 0.5);
        if mob_position.distance_to_sqr(current_node) >= 4.0 {
            return false;
        }

        if self.can_move_directly(
            mob,
            world,
            mob_position,
            path.get_next_entity_pos(mob.bb_width),
        ) {
            return true;
        }

        let next = path.get_node_pos(path.get_next_node_index() + 1);
        let next_node = Vec3::new(next.x as f64 + 0.5, next.y as f64, next.z as f64 + 0.5);
        let mob_to_current = Vec3::new(
            current_node.x - mob_position.x,
            current_node.y - mob_position.y,
            current_node.z - mob_position.z,
        );
        let mob_to_next = Vec3::new(
            next_node.x - mob_position.x,
            next_node.y - mob_position.y,
            next_node.z - mob_position.z,
        );
        let mob_to_current_sqr = mob_to_current.x * mob_to_current.x
            + mob_to_current.y * mob_to_current.y
            + mob_to_current.z * mob_to_current.z;
        let mob_to_next_sqr = mob_to_next.x * mob_to_next.x
            + mob_to_next.y * mob_to_next.y
            + mob_to_next.z * mob_to_next.z;
        let closer_to_next_than_current = mob_to_next_sqr < mob_to_current_sqr;
        let within_current_block = mob_to_current_sqr < 0.5;
        if !closer_to_next_than_current && !within_current_block {
            return false;
        }

        let normalize = |v: Vec3, len_sqr: f64| {
            let len = len_sqr.sqrt();
            if len < 1.0e-4 {
                Vec3::ZERO
            } else {
                Vec3::new(v.x / len, v.y / len, v.z / len)
            }
        };
        let mob_direction = normalize(mob_to_current, mob_to_current_sqr);
        let path_direction = normalize(mob_to_next, mob_to_next_sqr);
        let dot = path_direction.x * mob_direction.x
            + path_direction.y * mob_direction.y
            + path_direction.z * mob_direction.z;
        dot < 0.0
    }

    /// Mirrors `PathNavigation.doStuckDetection(Vec3)` in Paper 26.2.
    fn do_stucked_detection(&mut self, mob: &MobPathingState, temp_pos: Vec3, game_time: i64) {
        if self.tick - self.last_stucked_check > Self::STUCK_CHECK_INTERVAL {
            let effective_speed = if mob.speed >= 1.0 {
                mob.speed
            } else {
                mob.speed * mob.speed
            };
            let min_move_dist = f64::from(effective_speed) * 100.0 * 0.25;
            if temp_pos.distance_to_sqr(self.last_stucked_check_pos) < min_move_dist * min_move_dist
            {
                self.is_stuck = true;
                self.stop();
            } else {
                self.is_stuck = false;
            }
            self.last_stucked_check = self.tick;
            self.last_stucked_check_pos = temp_pos;
        }

        if let Some(ref path) = self.path
            && !path.is_done()
        {
            let next_node = path.get_next_node_pos();
            if self.timeout_cached_node == Some(next_node) {
                self.timeout_timer += game_time - self.last_timeout_check;
            } else {
                self.timeout_cached_node = Some(next_node);
                let dist = temp_pos
                    .distance_to_sqr(Vec3::new(
                        next_node.x as f64,
                        next_node.y as f64,
                        next_node.z as f64,
                    ))
                    .sqrt();
                self.timeout_limit = if mob.speed > 0.0 {
                    dist / f64::from(mob.speed) * 100.0
                } else {
                    0.0
                };
            }

            if self.timeout_limit > 0.0 && self.timeout_timer as f64 > self.timeout_limit * 3.0 {
                self.timeout_path();
            }
            self.last_timeout_check = game_time;
        }
    }

    fn timeout_path(&mut self) {
        self.reset_stucked_entities_position_raw();
        self.stop();
    }

    fn reset_stucked_entities_position(&mut self, _mob: &MobPathingState, game_time: i64) {
        self.reset_stucked_entities_position_raw();
        self.last_timeout_check = game_time;
    }

    fn reset_stucked_entities_position_raw(&mut self) {
        self.timeout_cached_node = None;
        self.timeout_timer = 0;
        self.timeout_limit = 0.0;
    }

    /// Mirrors `PathNavigation.canMoveDirectly(Vec3, Vec3)`.
    #[must_use]
    pub fn can_move_directly<W: PathWorldView + ?Sized>(
        &self,
        mob: &MobPathingState,
        world: &W,
        from: Vec3,
        to: Vec3,
    ) -> bool {
        match self.variant {
            NavigationVariant::Flying { .. } => world.is_clear_for_movement_between(from, to, true),
            NavigationVariant::Amphibious { .. } => {
                mob.is_in_liquid() && world.is_clear_for_movement_between(from, to, false)
            }
            NavigationVariant::WaterBound { .. } => {
                world.is_clear_for_movement_between(from, to, false)
            }
            NavigationVariant::Ground { .. } | NavigationVariant::WallClimber { .. } => false,
        }
    }

    /// Mirrors `AmphibiousPathNavigation.isStableDestination(BlockPos)`.
    #[must_use]
    pub fn is_stable_destination<W: PathWorldView + ?Sized>(
        &self,
        world: &W,
        pos: BlockPos,
    ) -> bool {
        let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
        match self.variant {
            NavigationVariant::Amphibious { .. } => !world.block_info(below).is_air,
            NavigationVariant::WaterBound { .. } => !world.block_info(pos).has_collision(),
            _ => world.block_info(below).has_collision(),
        }
    }
}
