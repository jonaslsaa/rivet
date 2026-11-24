//! Port of Paper 26.2 `net.minecraft.world.entity.ai.navigation.*`:
//! - `PathNavigation`
//! - `GroundPathNavigation`
//! - `FlyingPathNavigation`
//! - `WaterBoundPathNavigation`
//! - `AmphibiousPathNavigation`
//! - `WallClimberNavigation`

use rivet_util::mth;

use crate::ai::goal::context::{BlockPos, GoalRandom, Vec3};
use crate::ai::navigation::control::MoveControl;
use crate::ai::navigation::evaluator::{
    AmphibiousNodeEvaluator, FlyNodeEvaluator, NodeEvaluator, SwimNodeEvaluator, WalkNodeEvaluator,
};
use crate::ai::navigation::path::Path;
use crate::ai::navigation::path_finder::PathFinder;
use crate::ai::navigation::path_type::{PathType, PathTypeCache};
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
    last_stuck_check: i32,
    last_stuck_check_pos: Vec3,
    timeout_cached_node: BlockPos,
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
    /// `mob.getAttributeValue(Attributes.FOLLOW_RANGE)`, kept current by
    /// [`PathNavigation::on_follow_range_updated`].
    follow_range: f64,
    required_path_length: f32,
}

impl PathNavigation {
    pub const MAX_TIME_RECOMPUTE: i64 = 20;
    pub const STUCK_CHECK_INTERVAL: i32 = 100;
    pub const STUCK_THRESHOLD_DISTANCE_FACTOR: f32 = 0.25;
    /// Initial `PathNavigation.requiredPathLength`.
    pub const DEFAULT_REQUIRED_PATH_LENGTH: f32 = 16.0;

    #[must_use]
    pub fn ground(follow_range: f64) -> Self {
        Self::with_variant(
            NavigationVariant::Ground {
                evaluator: WalkNodeEvaluator::new(),
                avoid_sun: false,
                can_path_to_targets_below_surface: false,
            },
            follow_range,
        )
    }

    #[must_use]
    pub fn flying(follow_range: f64) -> Self {
        Self::with_variant(
            NavigationVariant::Flying {
                evaluator: FlyNodeEvaluator::new(),
            },
            follow_range,
        )
    }

    #[must_use]
    pub fn water_bound(follow_range: f64, allow_breaching: bool) -> Self {
        Self::with_variant(
            NavigationVariant::WaterBound {
                evaluator: SwimNodeEvaluator::new(allow_breaching),
                allow_breaching,
            },
            follow_range,
        )
    }

    #[must_use]
    pub fn amphibious(follow_range: f64) -> Self {
        Self::with_variant(
            NavigationVariant::Amphibious {
                evaluator: AmphibiousNodeEvaluator::new(false),
            },
            follow_range,
        )
    }

    #[must_use]
    pub fn wall_climber(follow_range: f64) -> Self {
        Self::with_variant(
            NavigationVariant::WallClimber {
                evaluator: WalkNodeEvaluator::new(),
                avoid_sun: false,
                can_path_to_targets_below_surface: false,
                path_to_position: None,
            },
            follow_range,
        )
    }

    /// Mirrors the `PathNavigation(Mob, Level)` constructor: `follow_range` is the
    /// mob's `FOLLOW_RANGE` attribute (base) value at construction, and the path
    /// finder starts with `Mth.floor(followRange * 16.0)` visited nodes.
    #[must_use]
    pub fn with_variant(variant: NavigationVariant, follow_range: f64) -> Self {
        Self {
            variant,
            path: None,
            speed_modifier: 1.0,
            tick: 0,
            last_stuck_check: 0,
            last_stuck_check_pos: Vec3::ZERO,
            timeout_cached_node: BlockPos::new(0, 0, 0),
            timeout_timer: 0,
            last_timeout_check: 0,
            timeout_limit: 0.0,
            max_distance_to_waypoint: 0.5,
            has_delayed_recomputation: false,
            time_last_recompute: 0,
            target_pos: None,
            reach_range: 0,
            max_visited_nodes_multiplier: 1.0,
            path_finder: PathFinder::new(mth::floor_d(follow_range * 16.0)),
            is_stuck: false,
            follow_range,
            required_path_length: Self::DEFAULT_REQUIRED_PATH_LENGTH,
        }
    }

    /// Mirrors `Mob.onAttributeUpdated` for `FOLLOW_RANGE`: records the new
    /// attribute value and calls `updatePathfinderMaxVisitedNodes()`.
    pub fn on_follow_range_updated(&mut self, follow_range: f64) {
        self.follow_range = follow_range;
        self.update_pathfinder_max_visited_nodes();
    }

    /// Mirrors `PathNavigation.updatePathfinderMaxVisitedNodes()`.
    pub fn update_pathfinder_max_visited_nodes(&mut self) {
        let max_visited_nodes = mth::floor(self.max_path_length() * 16.0);
        self.path_finder.set_max_visited_nodes(max_visited_nodes);
    }

    /// Mirrors `PathNavigation.setRequiredPathLength(float)`.
    pub fn set_required_path_length(&mut self, length: f32) {
        self.required_path_length = length;
        self.update_pathfinder_max_visited_nodes();
    }

    /// Mirrors `PathNavigation.getMaxPathLength()`:
    /// `Math.max((float) followRange, requiredPathLength)`.
    #[must_use]
    pub fn max_path_length(&self) -> f32 {
        mth::max_f32(self.follow_range as f32, self.required_path_length)
    }

    #[must_use]
    pub const fn max_visited_nodes(&self) -> i32 {
        self.path_finder.max_visited_nodes
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
            NavigationVariant::Ground { .. } | NavigationVariant::WallClimber { .. } => Vec3::new(
                mob.pos.x,
                f64::from(self.get_surface_y(mob, world)),
                mob.pos.z,
            ),
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

    /// Mirrors `GroundPathNavigation.getSurfaceY()`.
    fn get_surface_y<W: PathWorldView + ?Sized>(&self, mob: &MobPathingState, world: &W) -> i32 {
        if mob.in_water && self.can_float_flag() {
            let block_pos = mob.block_position();
            let mut surface = block_pos.y;
            let mut steps = 0;
            while world
                .block_info(BlockPos::new(block_pos.x, surface, block_pos.z))
                .is_water_block
            {
                surface += 1;
                steps += 1;
                if steps > 16 {
                    return block_pos.y;
                }
            }
            surface
        } else {
            mth::floor_d(mob.pos.y + 0.5)
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

    /// Mirrors `PathNavigation.createPath(BlockPos, int reachRange)` in Paper 26.2,
    /// including the `GroundPathNavigation` and `WallClimberNavigation` overrides.
    pub fn create_path<W: PathWorldView + ?Sized>(
        &mut self,
        mob: &mut MobPathingState,
        world: &W,
        cache: &mut PathTypeCache,
        random: &mut dyn GoalRandom,
        mut target: BlockPos,
        reach_range: i32,
    ) -> Option<Path> {
        // WallClimberNavigation records the requested position before the surface search.
        if let NavigationVariant::WallClimber {
            path_to_position, ..
        } = &mut self.variant
        {
            *path_to_position = Some(target);
        }

        if let NavigationVariant::Ground {
            can_path_to_targets_below_surface,
            ..
        }
        | NavigationVariant::WallClimber {
            can_path_to_targets_below_surface,
            ..
        } = &self.variant
        {
            // `getChunkNow(...) == null`
            if !world.block_info(target).is_loaded {
                return None;
            }
            if !*can_path_to_targets_below_surface {
                target = Self::find_surface_position(world, target);
            }
        }

        self.create_path_targets(
            mob,
            world,
            cache,
            random,
            &[target],
            reach_range,
            self.max_path_length(),
        )
    }

    /// Mirrors `GroundPathNavigation.findSurfacePosition(LevelChunk, BlockPos, int)`.
    fn find_surface_position<W: PathWorldView + ?Sized>(world: &W, mut pos: BlockPos) -> BlockPos {
        if world.block_info(pos).is_air {
            let mut column = BlockPos::new(pos.x, pos.y - 1, pos.z);
            while column.y >= world.min_y() && world.block_info(column).is_air {
                column.y -= 1;
            }
            if column.y >= world.min_y() {
                return BlockPos::new(column.x, column.y + 1, column.z);
            }

            column.y = pos.y + 1;
            while column.y <= world.max_y() && world.block_info(column).is_air {
                column.y += 1;
            }
            pos = column;
        }

        if !world.block_info(pos).is_solid {
            return pos;
        }

        let mut column = BlockPos::new(pos.x, pos.y + 1, pos.z);
        while column.y <= world.max_y() && world.block_info(column).is_solid {
            column.y += 1;
        }
        column
    }

    /// Mirrors `PathNavigation.createPath(Set<BlockPos>, Entity, int, boolean, int, float)`.
    ///
    /// `max_path_length` is [`PathNavigation::max_path_length`] for every overload
    /// except `createPath(BlockPos, int, int)`. `cache` is the level's
    /// `ServerLevel.getPathTypeCache()`, shared by every mob's search and
    /// invalidated per position by `ServerLevel.sendBlockUpdated`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_path_targets<W: PathWorldView + ?Sized>(
        &mut self,
        mob: &mut MobPathingState,
        world: &W,
        cache: &mut PathTypeCache,
        random: &mut dyn GoalRandom,
        targets: &[BlockPos],
        reach_range: i32,
        max_path_length: f32,
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

        // Paper - don't path out of world border
        let targets: Vec<BlockPos> = targets
            .iter()
            .copied()
            .filter(|&target| world.is_within_world_border(target))
            .collect();
        if targets.is_empty() {
            return None;
        }
        let targets = targets.as_slice();

        let multiplier = self.max_visited_nodes_multiplier;
        let path = match &mut self.variant {
            NavigationVariant::Ground { evaluator, .. }
            | NavigationVariant::WallClimber { evaluator, .. } => self.path_finder.find_path(
                evaluator,
                world,
                cache,
                mob,
                random,
                targets,
                max_path_length,
                reach_range,
                multiplier,
            ),
            NavigationVariant::Flying { evaluator } => self.path_finder.find_path(
                evaluator,
                world,
                cache,
                mob,
                random,
                targets,
                max_path_length,
                reach_range,
                multiplier,
            ),
            NavigationVariant::WaterBound { evaluator, .. } => self.path_finder.find_path(
                evaluator,
                world,
                cache,
                mob,
                random,
                targets,
                max_path_length,
                reach_range,
                multiplier,
            ),
            NavigationVariant::Amphibious { evaluator } => self.path_finder.find_path(
                evaluator,
                world,
                cache,
                mob,
                random,
                targets,
                max_path_length,
                reach_range,
                multiplier,
            ),
        };

        if let Some(ref p) = path {
            self.target_pos = Some(p.get_target());
            self.reach_range = reach_range;
            self.reset_stuck_timeout();
        }
        path
    }

    /// Mirrors `PathNavigation.trimPath()` plus the `GroundPathNavigation`
    /// avoid-sun override. Called from `moveTo(Path, double)` on the current path.
    fn trim_path<W: PathWorldView + ?Sized>(&mut self, mob: &MobPathingState, world: &W) {
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
        let Some(path) = self.path.as_mut() else {
            return;
        };

        for i in 0..path.get_node_count() {
            let node = path.get_node(i).clone();
            let next_node = (i + 1 < path.get_node_count()).then(|| path.get_node(i + 1).clone());
            if world.block_info(node.as_block_pos()).is_cauldron {
                path.replace_node(i, node.clone_and_move(node.x, node.y + 1, node.z));
                if let Some(next_node) = next_node
                    && node.y >= next_node.y
                {
                    path.replace_node(
                        i + 1,
                        node.clone_and_move(next_node.x, node.y + 1, next_node.z),
                    );
                }
            }
        }

        if avoid_sun {
            let mob_pos = BlockPos::new(
                mth::floor_d(mob.pos.x),
                mth::floor_d(mob.pos.y + 0.5),
                mth::floor_d(mob.pos.z),
            );
            if world.block_info(mob_pos).can_see_sky {
                return;
            }
            for i in 0..path.get_node_count() {
                if world
                    .block_info(path.get_node(i).as_block_pos())
                    .can_see_sky
                {
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
        cache: &mut PathTypeCache,
        random: &mut dyn GoalRandom,
        x: f64,
        y: f64,
        z: f64,
        speed_modifier: f64,
    ) -> bool {
        let target = BlockPos::new(mth::floor_d(x), mth::floor_d(y), mth::floor_d(z));
        let path = self.create_path(mob, world, cache, random, target, 1);
        self.move_to_path(path, speed_modifier, mob, world)
    }

    /// Mirrors `PathNavigation.moveTo(@Nullable Path newPath, double speedModifier)`.
    pub fn move_to_path<W: PathWorldView + ?Sized>(
        &mut self,
        new_path: Option<Path>,
        speed_modifier: f64,
        mob: &MobPathingState,
        world: &W,
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

        self.trim_path(mob, world);
        if self.path.as_ref().is_none_or(|p| p.get_node_count() == 0) {
            return false;
        }

        self.speed_modifier = speed_modifier;
        let mob_pos = self.get_temp_mob_pos(mob, world);
        self.last_stuck_check = self.tick;
        self.last_stuck_check_pos = mob_pos;
        true
    }

    /// Mirrors `PathNavigation.recomputePath()`.
    pub fn recompute_path<W: PathWorldView + ?Sized>(
        &mut self,
        mob: &mut MobPathingState,
        world: &W,
        cache: &mut PathTypeCache,
        random: &mut dyn GoalRandom,
        game_time: i64,
    ) {
        if game_time.wrapping_sub(self.time_last_recompute) <= Self::MAX_TIME_RECOMPUTE
            || !self.can_update_path(mob)
        {
            self.has_delayed_recomputation = true;
        } else if let Some(target_pos) = self.target_pos {
            self.path = None;
            self.path = self.create_path(mob, world, cache, random, target_pos, self.reach_range);
            self.time_last_recompute = game_time;
            self.has_delayed_recomputation = false;
        }
    }

    /// Mirrors `PathNavigation.tick()` (including the `FlyingPathNavigation` and
    /// `WallClimberNavigation` overrides) in Paper 26.2.
    pub fn tick<W: PathWorldView + ?Sized>(
        &mut self,
        mob: &mut MobPathingState,
        world: &W,
        cache: &mut PathTypeCache,
        random: &mut dyn GoalRandom,
        move_control: &mut MoveControl,
        game_time: i64,
    ) {
        // WallClimberNavigation.tick: runs super.tick() only while a path is in progress.
        if let NavigationVariant::WallClimber {
            path_to_position, ..
        } = &mut self.variant
            && self.path.as_ref().is_none_or(Path::is_done)
        {
            if let Some(target) = *path_to_position {
                let closer_to_center = |bp: BlockPos| {
                    let dx = bp.x as f64 + 0.5 - mob.pos.x;
                    let dy = bp.y as f64 + 0.5 - mob.pos.y;
                    let dz = bp.z as f64 + 0.5 - mob.pos.z;
                    let dist_sqr = dx * dx + dy * dy + dz * dz;
                    let radius = f64::from(mob.bb_width);
                    dist_sqr < radius * radius
                };
                let at_mob_y = BlockPos::new(target.x, mth::floor_d(mob.pos.y), target.z);
                if !closer_to_center(target)
                    && (mob.pos.y <= target.y as f64 || !closer_to_center(at_mob_y))
                {
                    move_control.set_wanted_position(
                        target.x as f64,
                        target.y as f64,
                        target.z as f64,
                        self.speed_modifier,
                    );
                } else {
                    *path_to_position = None;
                }
            }
            return;
        }

        self.tick = self.tick.wrapping_add(1);
        if self.has_delayed_recomputation {
            self.recompute_path(mob, world, cache, random, game_time);
        }

        if self.is_done() {
            return;
        }

        if self.can_update_path(mob) {
            self.follow_the_path(mob, world, game_time);
        } else if let NavigationVariant::Flying { .. } = self.variant {
            if let Some(ref mut path) = self.path
                && !path.is_done()
            {
                let pos = path.get_next_entity_pos(mob.bb_width);
                if mob.block_position()
                    == BlockPos::new(
                        mth::floor_d(pos.x),
                        mth::floor_d(pos.y),
                        mth::floor_d(pos.z),
                    )
                {
                    path.advance();
                }
            }
        } else {
            let temp_pos = self.get_temp_mob_pos(mob, world);
            if let Some(ref mut path) = self.path
                && !path.is_done()
            {
                let next_pos = path.get_next_entity_pos(mob.bb_width);
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
            // `getGroundY`: overridden to `target.y` by the water-bound and
            // amphibious navigations; FlyingPathNavigation.tick uses `target.y`.
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

        self.do_stuck_detection(mob, temp_pos, game_time);
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
            if len < f64::from(1.0e-5_f32) {
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
    fn do_stuck_detection(&mut self, mob: &MobPathingState, mob_pos: Vec3, game_time: i64) {
        if self.tick.wrapping_sub(self.last_stuck_check) > Self::STUCK_CHECK_INTERVAL {
            let effective_speed: f32 = if mob.speed >= 1.0 {
                mob.speed
            } else {
                mob.speed * mob.speed
            };
            let threshold_distance: f32 =
                effective_speed * 100.0 * Self::STUCK_THRESHOLD_DISTANCE_FACTOR;
            if mob_pos.distance_to_sqr(self.last_stuck_check_pos)
                < f64::from(threshold_distance * threshold_distance)
            {
                self.is_stuck = true;
                self.stop();
            } else {
                self.is_stuck = false;
            }
            self.last_stuck_check = self.tick;
            self.last_stuck_check_pos = mob_pos;
        }

        if let Some(ref path) = self.path
            && !path.is_done()
        {
            let pos = path.get_next_node_pos();
            if pos == self.timeout_cached_node {
                self.timeout_timer = self
                    .timeout_timer
                    .wrapping_add(game_time.wrapping_sub(self.last_timeout_check));
            } else {
                self.timeout_cached_node = pos;
                let node_center = Vec3::new(pos.x as f64 + 0.5, pos.y as f64, pos.z as f64 + 0.5);
                let dist_to_node = mob_pos.distance_to_sqr(node_center).sqrt();
                self.timeout_limit = if mob.speed > 0.0 {
                    dist_to_node / f64::from(mob.speed) * 20.0
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
        self.reset_stuck_timeout();
        self.stop();
    }

    /// Mirrors `PathNavigation.resetStuckTimeout()`.
    fn reset_stuck_timeout(&mut self) {
        self.timeout_cached_node = BlockPos::new(0, 0, 0);
        self.timeout_timer = 0;
        self.timeout_limit = 0.0;
        self.is_stuck = false;
    }

    /// Mirrors `PathNavigation.canMoveDirectly(Vec3, Vec3)` and its overrides.
    #[must_use]
    pub fn can_move_directly<W: PathWorldView + ?Sized>(
        &self,
        mob: &MobPathingState,
        world: &W,
        start_pos: Vec3,
        stop_pos: Vec3,
    ) -> bool {
        match self.variant {
            NavigationVariant::Flying { .. } => {
                Self::is_clear_for_movement_between(mob, world, start_pos, stop_pos, true)
            }
            NavigationVariant::Amphibious { .. } => {
                mob.is_in_liquid()
                    && Self::is_clear_for_movement_between(mob, world, start_pos, stop_pos, false)
            }
            NavigationVariant::WaterBound { .. } => {
                Self::is_clear_for_movement_between(mob, world, start_pos, stop_pos, false)
            }
            NavigationVariant::Ground { .. } | NavigationVariant::WallClimber { .. } => false,
        }
    }

    /// Mirrors `PathNavigation.isClearForMovementBetween(Mob, Vec3, Vec3, boolean)`:
    /// the clip ends at the mob's mid-height above `stop_pos`.
    fn is_clear_for_movement_between<W: PathWorldView + ?Sized>(
        mob: &MobPathingState,
        world: &W,
        start_pos: Vec3,
        stop_pos: Vec3,
        blocked_by_fluids: bool,
    ) -> bool {
        let to = Vec3::new(
            stop_pos.x,
            stop_pos.y + f64::from(mob.bb_height) * 0.5,
            stop_pos.z,
        );
        world.is_clear_for_movement_between(start_pos, to, blocked_by_fluids)
    }

    /// Mirrors `PathNavigation.isStableDestination(BlockPos)` and its overrides.
    #[must_use]
    pub fn is_stable_destination<W: PathWorldView + ?Sized>(
        &self,
        world: &W,
        pos: BlockPos,
    ) -> bool {
        let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
        match self.variant {
            NavigationVariant::Amphibious { .. } => !world.block_info(below).is_air,
            NavigationVariant::WaterBound { .. } => !world.block_info(pos).is_solid_render,
            NavigationVariant::Flying { .. } => world.block_info(pos).entity_can_stand_on,
            NavigationVariant::Ground { .. } | NavigationVariant::WallClimber { .. } => {
                world.block_info(below).is_solid_render
            }
        }
    }

    /// Mirrors `PathNavigation.shouldRecomputePath(BlockPos)`, called by
    /// `ServerLevel.sendBlockUpdated` for a changed block.
    #[must_use]
    pub fn should_recompute_path(&self, mob: &MobPathingState, pos: BlockPos) -> bool {
        if self.has_delayed_recomputation {
            return false;
        }
        let Some(path) = self.path.as_ref() else {
            return false;
        };
        if path.is_done() || path.get_node_count() == 0 {
            return false;
        }
        let Some(target) = path.get_end_node() else {
            return false;
        };
        let middle_x = (f64::from(target.x) + mob.pos.x) / 2.0;
        let middle_y = (f64::from(target.y) + mob.pos.y) / 2.0;
        let middle_z = (f64::from(target.z) + mob.pos.z) / 2.0;
        // `Vec3i.closerToCenterThan(Position, double)`
        let dx = f64::from(pos.x) + 0.5 - middle_x;
        let dy = f64::from(pos.y) + 0.5 - middle_y;
        let dz = f64::from(pos.z) + 0.5 - middle_z;
        // `!path.isDone()` guarantees `nextNodeIndex < nodeCount`.
        let distance = (path.get_node_count() - path.get_next_node_index()) as f64;
        dx * dx + dy * dy + dz * dz < distance * distance
    }
}
