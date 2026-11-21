use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use super::*;

#[derive(Default)]
struct ScriptedRandom {
    floats: VecDeque<f32>,
    doubles: VecDeque<f64>,
    ints: VecDeque<i32>,
}

impl ScriptedRandom {
    fn push_float(&mut self, v: f32) {
        self.floats.push_back(v);
    }

    fn push_double(&mut self, v: f64) {
        self.doubles.push_back(v);
    }

    fn push_int(&mut self, v: i32) {
        self.ints.push_back(v);
    }
}

impl GoalRandom for ScriptedRandom {
    fn next_float(&mut self) -> f32 {
        self.floats.pop_front().unwrap_or(0.0)
    }

    fn next_double(&mut self) -> f64 {
        self.doubles.pop_front().unwrap_or(0.0)
    }

    fn next_int(&mut self, bound: i32) -> i32 {
        let v = self.ints.pop_front().unwrap_or(0);
        if bound > 0 { v.rem_euclid(bound) } else { 0 }
    }
}

struct MockContext {
    random: ScriptedRandom,
    mob_id: EntityId,
    mob_type: EntityTypeId,
    game_time: i64,
    pos: Vec3,
    eye_y: f64,
    block_pos: BlockPos,
    bounding_box: Aabb,
    delta_movement: Vec3,
    on_ground: bool,
    has_controlling_passenger: bool,
    no_action_time: i32,
    team: Option<TeamId>,
    within_home: bool,
    follow_range: f64,
    in_water: bool,
    water_height: f64,
    fluid_jump_threshold: f64,
    in_lava: bool,
    on_fire: bool,
    closest_water: Option<BlockPos>,
    panic_tags: Vec<DamageTypeTagId>,
    last_hurt_by_mob: Option<EntitySnapshot>,
    last_hurt_by_mob_timestamp: i32,
    universal_anger: bool,
    can_float: bool,
    jumps: usize,
    look_targets: Vec<(f64, f64, f64)>,
    look_entities: Vec<EntityId>,
    nav_done: bool,
    nav_move_result: bool,
    nav_moves_xyz: Vec<(f64, f64, f64, f64)>,
    nav_moves_entity: Vec<(EntityId, f64)>,
    nav_moves_path: Vec<(Option<PathHandle>, f64)>,
    nav_stops: usize,
    created_path: Option<PathHandle>,
    default_random_pos: Option<Vec3>,
    land_random_pos: Option<Vec3>,
    target: Option<EntitySnapshot>,
    set_target_log: Vec<(Option<EntityId>, TargetReason)>,
    entities: HashMap<EntityId, EntitySnapshot>,
    nearest_player: Option<EntitySnapshot>,
    nearest_entity: Option<EntitySnapshot>,
    can_attack: bool,
    targeting_passes: bool,
    has_los: bool,
    in_melee_range: bool,
    aggressive: bool,
    swings: Vec<InteractionHand>,
    hurt_targets: Vec<EntityId>,
    alerted_same_type: Vec<(Aabb, EntityId, Vec<EntityTypeId>)>,
    universal_anger_refreshes: usize,
    universal_anger_alerts: Vec<Aabb>,
    is_tame: bool,
    is_ordered_to_sit: bool,
    owner_id: Option<EntityId>,
    owner_last_hurt_by: Option<(EntitySnapshot, i32)>,
    owner_last_hurt: Option<(EntitySnapshot, i32)>,
    wants_to_attack: bool,
    is_using_item: bool,
    equipped: Vec<(EquipmentSlot, ItemStackRef)>,
    started_using: Vec<InteractionHand>,
    sounds: Vec<(SoundId, f32, f32)>,
}

impl Default for MockContext {
    fn default() -> Self {
        Self {
            random: ScriptedRandom::default(),
            mob_id: EntityId(1),
            mob_type: EntityTypeId(10),
            game_time: 100,
            pos: Vec3::ZERO,
            eye_y: 1.5,
            block_pos: BlockPos::new(0, 0, 0),
            bounding_box: Aabb::new(-0.3, 0.0, -0.3, 0.3, 1.8, 0.3),
            delta_movement: Vec3::ZERO,
            on_ground: true,
            has_controlling_passenger: false,
            no_action_time: 0,
            team: None,
            within_home: true,
            follow_range: 16.0,
            in_water: false,
            water_height: 0.0,
            fluid_jump_threshold: 0.4,
            in_lava: false,
            on_fire: false,
            closest_water: None,
            panic_tags: Vec::new(),
            last_hurt_by_mob: None,
            last_hurt_by_mob_timestamp: 0,
            universal_anger: false,
            can_float: false,
            jumps: 0,
            look_targets: Vec::new(),
            look_entities: Vec::new(),
            nav_done: true,
            nav_move_result: true,
            nav_moves_xyz: Vec::new(),
            nav_moves_entity: Vec::new(),
            nav_moves_path: Vec::new(),
            nav_stops: 0,
            created_path: None,
            default_random_pos: None,
            land_random_pos: None,
            target: None,
            set_target_log: Vec::new(),
            entities: HashMap::new(),
            nearest_player: None,
            nearest_entity: None,
            can_attack: true,
            targeting_passes: true,
            has_los: true,
            in_melee_range: false,
            aggressive: false,
            swings: Vec::new(),
            hurt_targets: Vec::new(),
            alerted_same_type: Vec::new(),
            universal_anger_refreshes: 0,
            universal_anger_alerts: Vec::new(),
            is_tame: false,
            is_ordered_to_sit: false,
            owner_id: None,
            owner_last_hurt_by: None,
            owner_last_hurt: None,
            wants_to_attack: true,
            is_using_item: false,
            equipped: Vec::new(),
            started_using: Vec::new(),
            sounds: Vec::new(),
        }
    }
}

impl GoalContext for MockContext {
    type Random = ScriptedRandom;

    fn random(&mut self) -> &mut Self::Random {
        &mut self.random
    }

    fn mob_id(&self) -> EntityId {
        self.mob_id
    }

    fn mob_type(&self) -> EntityTypeId {
        self.mob_type
    }

    fn game_time(&self) -> i64 {
        self.game_time
    }

    fn pos(&self) -> Vec3 {
        self.pos
    }

    fn eye_y(&self) -> f64 {
        self.eye_y
    }

    fn block_pos(&self) -> BlockPos {
        self.block_pos
    }

    fn bounding_box(&self) -> Aabb {
        self.bounding_box
    }

    fn delta_movement(&self) -> Vec3 {
        self.delta_movement
    }

    fn set_delta_movement(&mut self, vx: f64, vy: f64, vz: f64) {
        self.delta_movement = Vec3::new(vx, vy, vz);
    }

    fn on_ground(&self) -> bool {
        self.on_ground
    }

    fn has_controlling_passenger(&self) -> bool {
        self.has_controlling_passenger
    }

    fn no_action_time(&self) -> i32 {
        self.no_action_time
    }

    fn team(&self) -> Option<TeamId> {
        self.team
    }

    fn is_within_home(&self, _pos: BlockPos) -> bool {
        self.within_home
    }

    fn follow_range(&self) -> f64 {
        self.follow_range
    }

    fn is_in_water(&self) -> bool {
        self.in_water
    }

    fn water_fluid_height(&self) -> f64 {
        self.water_height
    }

    fn fluid_jump_threshold(&self) -> f64 {
        self.fluid_jump_threshold
    }

    fn is_in_lava(&self) -> bool {
        self.in_lava
    }

    fn is_on_fire(&self) -> bool {
        self.on_fire
    }

    fn find_closest_water(&self, _xz_dist: i32, _y_dist: i32) -> Option<BlockPos> {
        self.closest_water
    }

    fn has_panic_causing_damage(&self, tag: DamageTypeTagId) -> bool {
        self.panic_tags.contains(&tag)
    }

    fn last_hurt_by_mob(&self) -> Option<EntitySnapshot> {
        self.last_hurt_by_mob
    }

    fn last_hurt_by_mob_timestamp(&self) -> i32 {
        self.last_hurt_by_mob_timestamp
    }

    fn universal_anger_enabled(&self) -> bool {
        self.universal_anger
    }

    fn forget_current_target_and_refresh_universal_anger(&mut self) {
        self.universal_anger_refreshes += 1;
        self.target = None;
    }

    fn alert_nearby_same_type_universal_anger(&mut self, search_area: Aabb) {
        self.universal_anger_alerts.push(search_area);
    }

    fn jump(&mut self) {
        self.jumps += 1;
    }

    fn set_can_float(&mut self, can_float: bool) {
        self.can_float = can_float;
    }

    fn set_look_at(&mut self, x: f64, y: f64, z: f64) {
        self.look_targets.push((x, y, z));
    }

    fn set_look_at_entity(&mut self, target: EntityId, _y_max_rot: f32, _x_max_rot: f32) {
        self.look_entities.push(target);
    }

    fn nav_is_done(&self) -> bool {
        self.nav_done
    }

    fn nav_move_to_xyz(&mut self, x: f64, y: f64, z: f64, speed: f64) -> bool {
        self.nav_moves_xyz.push((x, y, z, speed));
        self.nav_done = !self.nav_move_result;
        self.nav_move_result
    }

    fn nav_move_to_entity(&mut self, target: EntityId, speed: f64) -> bool {
        self.nav_moves_entity.push((target, speed));
        self.nav_move_result
    }

    fn nav_move_to_path(&mut self, path: Option<PathHandle>, speed: f64) -> bool {
        self.nav_moves_path.push((path, speed));
        self.nav_done = !self.nav_move_result;
        self.nav_move_result
    }

    fn nav_create_path_to_entity(
        &mut self,
        _target: EntityId,
        _accuracy: i32,
    ) -> Option<PathHandle> {
        self.created_path
    }

    fn nav_stop(&mut self) {
        self.nav_stops += 1;
        self.nav_done = true;
    }

    fn default_random_pos(&mut self, _xz_range: i32, _y_range: i32) -> Option<Vec3> {
        self.default_random_pos
    }

    fn land_random_pos(&mut self, _xz_range: i32, _y_range: i32) -> Option<Vec3> {
        self.land_random_pos
    }

    fn target(&self) -> Option<EntitySnapshot> {
        self.target
    }

    fn set_target(&mut self, target: Option<EntityId>, reason: TargetReason) {
        self.set_target_log.push((target, reason));
        self.target = target.and_then(|id| self.entities.get(&id).copied());
    }

    fn can_attack(&self, _target: EntityId) -> bool {
        self.can_attack
    }

    fn test_targeting_conditions(
        &self,
        _target: EntityId,
        _conditions: TargetingConditions,
    ) -> bool {
        self.targeting_passes
    }

    fn has_line_of_sight(&mut self, _target: EntityId) -> bool {
        self.has_los
    }

    fn is_within_melee_attack_range(&self, _target: EntityId) -> bool {
        self.in_melee_range
    }

    fn set_aggressive(&mut self, aggressive: bool) {
        self.aggressive = aggressive;
    }

    fn swing(&mut self, hand: InteractionHand) {
        self.swings.push(hand);
    }

    fn do_hurt_target(&mut self, target: EntityId) -> bool {
        self.hurt_targets.push(target);
        true
    }

    fn entity_snapshot(&self, id: EntityId) -> Option<EntitySnapshot> {
        self.entities.get(&id).copied()
    }

    fn nearest_player(
        &self,
        _conditions: TargetingConditions,
        _x: f64,
        _eye_y: f64,
        _z: f64,
    ) -> Option<EntitySnapshot> {
        self.nearest_player
    }

    fn nearest_entity_of_type(
        &self,
        _entity_type: EntityTypeId,
        _area: Aabb,
        _conditions: TargetingConditions,
        _x: f64,
        _eye_y: f64,
        _z: f64,
    ) -> Option<EntitySnapshot> {
        self.nearest_entity
    }

    fn alert_nearby_same_type(
        &mut self,
        search_area: Aabb,
        attacker: EntityId,
        ignored_types: &[EntityTypeId],
    ) {
        self.alerted_same_type
            .push((search_area, attacker, ignored_types.to_vec()));
    }

    fn is_tame(&self) -> bool {
        self.is_tame
    }

    fn is_ordered_to_sit(&self) -> bool {
        self.is_ordered_to_sit
    }

    fn owner_id(&self) -> Option<EntityId> {
        self.owner_id
    }

    fn owner_last_hurt_by_mob(&self) -> Option<(EntitySnapshot, i32)> {
        self.owner_last_hurt_by
    }

    fn owner_last_hurt_mob(&self) -> Option<(EntitySnapshot, i32)> {
        self.owner_last_hurt
    }

    fn wants_to_attack(&self, _target: EntityId, _owner: EntityId) -> bool {
        self.wants_to_attack
    }

    fn is_using_item(&self) -> bool {
        self.is_using_item
    }

    fn set_item_slot(&mut self, slot: EquipmentSlot, item: ItemStackRef) {
        self.equipped.push((slot, item));
    }

    fn start_using_item(&mut self, hand: InteractionHand) {
        self.started_using.push(hand);
        self.is_using_item = true;
    }

    fn play_sound(&mut self, sound: &SoundId, volume: f32, pitch: f32) {
        self.sounds.push((sound.clone(), volume, pitch));
    }
}

fn snapshot(id: u64, pos: Vec3, is_player: bool) -> EntitySnapshot {
    EntitySnapshot {
        id: EntityId(id),
        entity_type: EntityTypeId(if is_player { 1 } else { 20 }),
        is_player,
        is_creative_or_spectator: false,
        is_alive: true,
        pos,
        eye_y: pos.y + 1.62,
        block_pos: BlockPos::new(pos.x as i32, pos.y as i32, pos.z as i32),
        team: None,
    }
}

/// Programmable test goal recording exact lifecycle event ordering.
#[derive(Clone)]
struct ProbeState {
    can_use: bool,
    can_continue: bool,
    interruptable: bool,
    every_tick: bool,
    flags: GoalFlags,
}

struct ProbeGoal {
    label: &'static str,
    state: Rc<RefCell<ProbeState>>,
    log: Rc<RefCell<Vec<String>>>,
}

impl ProbeGoal {
    fn new(
        label: &'static str,
        flags: GoalFlags,
        can_use: bool,
        can_continue: bool,
        interruptable: bool,
        every_tick: bool,
        log: Rc<RefCell<Vec<String>>>,
    ) -> (Self, Rc<RefCell<ProbeState>>) {
        let state = Rc::new(RefCell::new(ProbeState {
            can_use,
            can_continue,
            interruptable,
            every_tick,
            flags,
        }));
        (
            Self {
                label,
                state: Rc::clone(&state),
                log,
            },
            state,
        )
    }
}

impl Goal<()> for ProbeGoal {
    fn can_use(&mut self, _ctx: &mut ()) -> bool {
        self.log
            .borrow_mut()
            .push(format!("{}:can_use", self.label));
        self.state.borrow().can_use
    }

    fn can_continue_to_use(&mut self, _ctx: &mut ()) -> bool {
        self.log
            .borrow_mut()
            .push(format!("{}:can_continue", self.label));
        self.state.borrow().can_continue
    }

    fn is_interruptable(&self) -> bool {
        self.state.borrow().interruptable
    }

    fn start(&mut self, _ctx: &mut ()) {
        self.log.borrow_mut().push(format!("{}:start", self.label));
    }

    fn stop(&mut self, _ctx: &mut ()) {
        self.log.borrow_mut().push(format!("{}:stop", self.label));
    }

    fn requires_update_every_tick(&self) -> bool {
        self.state.borrow().every_tick
    }

    fn tick(&mut self, _ctx: &mut ()) {
        self.log.borrow_mut().push(format!("{}:tick", self.label));
    }

    fn flags(&self) -> GoalFlags {
        self.state.borrow().flags
    }

    fn set_flags(&mut self, required_control_flags: GoalFlags) {
        self.state
            .borrow_mut()
            .flags
            .set_for_goal(required_control_flags);
    }
}

// ---------------------------------------------------------------------------
// GoalSelector scheduling, ordering, flags, preemption, and controlled negatives
// ---------------------------------------------------------------------------

#[test]
fn reduced_tick_delay_matches_java_positive_ceil_div() {
    assert_eq!(reduced_tick_delay(0), 0);
    assert_eq!(reduced_tick_delay(1), 1);
    assert_eq!(reduced_tick_delay(2), 1);
    assert_eq!(reduced_tick_delay(5), 3);
    assert_eq!(reduced_tick_delay(10), 5);
    assert_eq!(reduced_tick_delay(60), 30);
    assert_eq!(reduced_tick_delay(120), 60);
    assert_eq!(reduced_tick_delay(-1), 0);
    assert_eq!(reduced_tick_delay(-2), -1);
    assert_eq!(reduced_tick_delay(-3), -1);
    assert_eq!(reduced_tick_delay(i32::MIN), 1_073_741_824);
    assert_eq!(adjusted_tick_delay(true, 5), 5);
    assert_eq!(adjusted_tick_delay(false, 5), 3);
}

#[test]
fn selector_exact_three_phase_lifecycle_and_insertion_order() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut selector = GoalSelector::new();

    let (g1, s1) = ProbeGoal::new(
        "A",
        GoalFlags::MOVE,
        true,
        true,
        true,
        false,
        Rc::clone(&log),
    );
    let (g2, _s2) = ProbeGoal::new(
        "B",
        GoalFlags::LOOK,
        true,
        true,
        true,
        false,
        Rc::clone(&log),
    );
    selector.add_goal(2, g1);
    selector.add_goal(1, g2);

    selector.tick(&mut ());
    assert_eq!(
        *log.borrow(),
        vec![
            "A:can_use",
            "A:start",
            "B:can_use",
            "B:start",
            "A:tick",
            "B:tick",
        ]
    );

    // On tick 2, A refuses to continue -> cleanup stops A, then update sees A's can_use=false,
    // B continues and ticks.
    log.borrow_mut().clear();
    s1.borrow_mut().can_continue = false;
    s1.borrow_mut().can_use = false;
    selector.tick(&mut ());
    assert_eq!(
        *log.borrow(),
        vec![
            "A:can_continue",
            "A:stop",
            "B:can_continue",
            "A:can_use",
            "B:tick"
        ]
    );
}

#[test]
fn selector_priority_preemption_equal_priority_and_non_interruptable() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut selector = GoalSelector::new();

    let (low, _low_s) = ProbeGoal::new(
        "low",
        GoalFlags::MOVE,
        true,
        true,
        true,
        false,
        Rc::clone(&log),
    );
    let (equal, _eq_s) = ProbeGoal::new(
        "equal",
        GoalFlags::MOVE,
        true,
        true,
        true,
        false,
        Rc::clone(&log),
    );
    let (high, high_s) = ProbeGoal::new(
        "high",
        GoalFlags::MOVE,
        false,
        true,
        false,
        false,
        Rc::clone(&log),
    );
    let (highest, highest_s) = ProbeGoal::new(
        "highest",
        GoalFlags::MOVE,
        false,
        true,
        true,
        false,
        Rc::clone(&log),
    );

    selector.add_goal(5, low);
    selector.add_goal(5, equal);
    selector.add_goal(2, high);
    selector.add_goal(0, highest);

    // Tick 1: low starts. equal (same priority 5) cannot replace low, so equal:can_use is NOT called.
    selector.tick(&mut ());
    assert_eq!(
        *log.borrow(),
        vec![
            "low:can_use",
            "low:start",
            "high:can_use",
            "highest:can_use",
            "low:tick"
        ]
    );

    // Tick 2: high becomes usable and preempts low (2 < 5). low stops before high starts.
    log.borrow_mut().clear();
    high_s.borrow_mut().can_use = true;
    selector.tick(&mut ());
    assert_eq!(
        *log.borrow(),
        vec![
            "low:can_continue",
            "high:can_use",
            "low:stop",
            "high:start",
            "high:tick",
        ]
    );

    // Tick 3: highest (priority 0 < 2) becomes usable, but high is NOT interruptable -> blocked,
    // and highest:can_use is NOT called!
    log.borrow_mut().clear();
    highest_s.borrow_mut().can_use = true;
    selector.tick(&mut ());
    assert_eq!(*log.borrow(), vec!["high:can_continue", "high:tick"]);
}

#[test]
fn selector_multi_flag_conflict_checks_all_flags_before_stopping_any() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut selector = GoalSelector::new();

    // MOVE is interruptable (prio 5), LOOK is NOT interruptable (prio 5).
    let (move_g, _) = ProbeGoal::new(
        "move",
        GoalFlags::MOVE,
        true,
        true,
        true,
        false,
        Rc::clone(&log),
    );
    let (look_g, _) = ProbeGoal::new(
        "look",
        GoalFlags::LOOK,
        true,
        true,
        false,
        false,
        Rc::clone(&log),
    );
    let (both_g, _) = ProbeGoal::new(
        "both",
        GoalFlags::MOVE.union(GoalFlags::LOOK),
        true,
        true,
        true,
        false,
        Rc::clone(&log),
    );

    selector.add_goal(5, move_g);
    selector.add_goal(5, look_g);
    selector.add_goal(1, both_g);

    selector.tick(&mut ());
    // "both" cannot replace "look" because look is non-interruptable, so "move" is NOT stopped
    // and "both:can_use" is never called.
    assert_eq!(
        *log.borrow(),
        vec![
            "move:can_use",
            "move:start",
            "look:can_use",
            "look:start",
            "move:tick",
            "look:tick"
        ]
    );
}

#[test]
fn selector_flagless_goals_default_to_unknown_behavior_and_conflict() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut selector = GoalSelector::new();

    // Both goals pass GoalFlags::EMPTY -> normalized to UNKNOWN_BEHAVIOR per Paper 26.2.
    let (g1, _) = ProbeGoal::new(
        "u1",
        GoalFlags::EMPTY,
        true,
        true,
        true,
        false,
        Rc::clone(&log),
    );
    let (g2, _) = ProbeGoal::new(
        "u2",
        GoalFlags::EMPTY,
        true,
        true,
        true,
        false,
        Rc::clone(&log),
    );
    let id1 = selector.add_goal(3, g1);
    selector.add_goal(3, g2);

    selector.tick(&mut ());
    assert_eq!(selector.locked_flag(GoalFlag::UnknownBehavior), Some(id1));
    assert_eq!(*log.borrow(), vec!["u1:can_use", "u1:start", "u1:tick"]);

    // Disabling UNKNOWN_BEHAVIOR stops u1 and blocks u2.
    log.borrow_mut().clear();
    selector.disable_control_flag(GoalFlag::UnknownBehavior);
    selector.tick(&mut ());
    assert_eq!(*log.borrow(), vec!["u1:stop"]);
    assert!(!selector.has_tasks());
}

#[test]
fn selector_controlled_negative_max_priority_cannot_replace_no_goal() {
    // Paper's NO_GOAL has priority Integer.MAX_VALUE. A goal registered at i32::MAX fails
    // `goal.getPriority() < NO_GOAL.getPriority()` and therefore never starts.
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut selector = GoalSelector::new();
    let (g, _) = ProbeGoal::new(
        "max",
        GoalFlags::MOVE,
        true,
        true,
        true,
        false,
        Rc::clone(&log),
    );
    selector.add_goal(i32::MAX, g);

    selector.tick(&mut ());
    assert!(log.borrow().is_empty());
    assert!(!selector.has_tasks());
}

#[test]
fn selector_disabled_flags_stop_running_and_block_starting() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut selector = GoalSelector::new();

    let (g, _) = ProbeGoal::new(
        "m",
        GoalFlags::MOVE,
        true,
        true,
        true,
        false,
        Rc::clone(&log),
    );
    selector.add_goal(1, g);

    selector.tick(&mut ());
    assert!(selector.has_tasks());

    log.borrow_mut().clear();
    selector.set_control_flag(GoalFlag::Move, false);
    selector.tick(&mut ());
    assert_eq!(*log.borrow(), vec!["m:stop"]);
    assert!(!selector.has_tasks());

    log.borrow_mut().clear();
    selector.set_control_flag(GoalFlag::Move, true);
    selector.tick(&mut ());
    assert_eq!(*log.borrow(), vec!["m:can_use", "m:start", "m:tick"]);
}

#[test]
fn selector_inactive_tick_remove_and_mob_cadence() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut target_sel = GoalSelector::new();
    let mut goal_sel = GoalSelector::new();

    // EAR 2 inactiveTick() returns true on every 3rd call.
    assert!(!goal_sel.inactive_tick());
    assert!(!goal_sel.inactive_tick());
    assert!(goal_sel.inactive_tick());
    assert!(!goal_sel.inactive_tick());

    let (tg, _) = ProbeGoal::new(
        "T",
        GoalFlags::TARGET,
        true,
        true,
        true,
        false,
        Rc::clone(&log),
    );
    let (g_every, _) = ProbeGoal::new(
        "E",
        GoalFlags::JUMP,
        true,
        true,
        true,
        true,
        Rc::clone(&log),
    );
    let (g_half, _) = ProbeGoal::new(
        "H",
        GoalFlags::MOVE,
        true,
        true,
        true,
        false,
        Rc::clone(&log),
    );
    target_sel.add_goal(1, tg);
    let every_id = goal_sel.add_goal(1, g_every);
    goal_sel.add_goal(2, g_half);

    // tick_count = 1 -> full tick (targetSelector before goalSelector).
    tick_mob_selectors(&mut target_sel, &mut goal_sel, &mut (), 1, 0);
    assert_eq!(
        *log.borrow(),
        vec![
            "T:can_use",
            "T:start",
            "T:tick",
            "E:can_use",
            "E:start",
            "H:can_use",
            "H:start",
            "E:tick",
            "H:tick",
        ]
    );

    // tick_count = 3, entity_id = 0 -> (3+0) % 2 != 0 && tick_count > 1 -> only every-tick goal E ticks!
    log.borrow_mut().clear();
    tick_mob_selectors(&mut target_sel, &mut goal_sel, &mut (), 3, 0);
    assert_eq!(*log.borrow(), vec!["E:tick"]);

    // remove_goal stops running goal E immediately.
    log.borrow_mut().clear();
    goal_sel.remove_goal(every_id, &mut ());
    assert_eq!(*log.borrow(), vec!["E:stop"]);
}

// ---------------------------------------------------------------------------
// Classic movement, look, combat, and use-item goals
// ---------------------------------------------------------------------------

#[test]
fn float_goal_water_threshold_lava_and_jump_chance() {
    let mut ctx = MockContext::default();
    FloatGoal::init_navigation(&mut ctx);
    assert!(ctx.can_float);

    let mut goal = FloatGoal::new();
    assert_eq!(
        <FloatGoal as Goal<MockContext>>::flags(&goal),
        GoalFlags::JUMP
    );
    assert!(<FloatGoal as Goal<MockContext>>::requires_update_every_tick(&goal));

    // Below fluid jump threshold -> cannot use.
    ctx.in_water = true;
    ctx.water_height = 0.4;
    ctx.fluid_jump_threshold = 0.4;
    assert!(!goal.can_use(&mut ctx));

    // Strictly above threshold -> can use.
    ctx.water_height = 0.41;
    assert!(goal.can_use(&mut ctx));

    // Lava also activates even when not in water.
    ctx.in_water = false;
    ctx.in_lava = true;
    assert!(goal.can_use(&mut ctx));

    // Tick jumps when next_float() < 0.8 and skips when >= 0.8.
    ctx.random.push_float(0.79);
    ctx.random.push_float(0.8);
    goal.tick(&mut ctx);
    goal.tick(&mut ctx);
    assert_eq!(ctx.jumps, 1);
}

#[test]
fn random_look_around_and_look_at_player_goals() {
    let mut ctx = MockContext {
        pos: Vec3::new(10.0, 64.0, 20.0),
        eye_y: 65.5,
        ..Default::default()
    };

    let mut random_look = RandomLookAroundGoal::new();
    assert_eq!(
        <RandomLookAroundGoal as Goal<MockContext>>::flags(&random_look),
        GoalFlags::MOVE.union(GoalFlags::LOOK)
    );
    ctx.random.push_float(0.02); // >= 0.02 -> false
    assert!(!random_look.can_use(&mut ctx));
    ctx.random.push_float(0.01); // < 0.02 -> true
    assert!(random_look.can_use(&mut ctx));

    ctx.random.push_double(0.0); // angle = 0 -> cos=1, sin=0
    ctx.random.push_int(5); // look_time = 25
    random_look.start(&mut ctx);
    assert_eq!(random_look.look_time(), 25);
    assert!(random_look.can_continue_to_use(&mut ctx));
    random_look.tick(&mut ctx);
    assert_eq!(random_look.look_time(), 24);
    assert_eq!(ctx.look_targets.last(), Some(&(11.0, 65.5, 20.0)));

    // LookAtPlayerGoal with only_horizontal=true.
    let player = snapshot(2, Vec3::new(13.0, 64.0, 24.0), true);
    ctx.entities.insert(player.id, player);
    ctx.nearest_player = Some(player);
    let mut look_player = LookAtPlayerGoal::with_options(LookAtTargetType::Player, 8.0, 0.5, true);
    ctx.random.push_float(0.1);
    assert!(look_player.can_use(&mut ctx));
    ctx.random.push_int(10); // raw 50 -> adjusted_tick_delay(false, 50) = 25
    look_player.start(&mut ctx);
    assert_eq!(look_player.look_time(), 25);
    look_player.tick(&mut ctx);
    // only_horizontal uses mob's eye_y (65.5), not player's eye_y (65.62).
    assert_eq!(ctx.look_targets.last(), Some(&(13.0, 65.5, 24.0)));
    assert_eq!(look_player.look_time(), 24);
    look_player.stop(&mut ctx);
    assert_eq!(look_player.look_at(), None);
}

#[test]
fn random_stroll_and_water_avoiding_stroll_goals() {
    let mut ctx = MockContext {
        default_random_pos: Some(Vec3::new(4.0, 64.0, -2.0)),
        land_random_pos: Some(Vec3::new(7.0, 64.0, 3.0)),
        ..Default::default()
    };

    let mut stroll = RandomStrollGoal::new(1.0);
    ctx.no_action_time = 100;
    assert!(!stroll.can_use(&mut ctx));

    // trigger() bypasses no_action_time and random interval.
    stroll.trigger();
    assert!(stroll.can_use(&mut ctx));
    assert_eq!(stroll.wanted_position(), Vec3::new(4.0, 64.0, -2.0));
    stroll.start(&mut ctx);
    assert_eq!(ctx.nav_moves_xyz.len(), 1);
    assert!(stroll.can_continue_to_use(&mut ctx));
    stroll.stop(&mut ctx);
    assert_eq!(ctx.nav_stops, 1);

    // WaterAvoidingRandomStrollGoal prefers land_random_pos when in water.
    let mut water_avoid = WaterAvoidingRandomStrollGoal::new(0.8);
    ctx.in_water = true;
    water_avoid.trigger();
    assert!(water_avoid.can_use(&mut ctx));
    assert_eq!(water_avoid.wanted_position(), Vec3::new(7.0, 64.0, 3.0));
}

#[test]
fn panic_goal_prefers_water_when_on_fire() {
    let mut ctx = MockContext::default();
    let mut panic = PanicGoal::new(1.25);

    // Not panicked yet.
    assert!(!panic.can_use(&mut ctx));

    ctx.panic_tags.push(DAMAGE_TYPE_TAG_PANIC_CAUSES);
    ctx.on_fire = true;
    ctx.closest_water = Some(BlockPos::new(2, 63, -4));
    ctx.default_random_pos = Some(Vec3::new(9.0, 64.0, 9.0));

    assert!(panic.can_use(&mut ctx));
    assert_eq!(panic.target_position(), Vec3::new(2.0, 63.0, -4.0));
    panic.start(&mut ctx);
    assert!(panic.is_running());
    assert!(panic.can_continue_to_use(&mut ctx));
    panic.stop(&mut ctx);
    assert!(!panic.is_running());
}

#[test]
fn melee_attack_goal_cooldown_repath_and_attack() {
    let mut ctx = MockContext::default();
    let target = snapshot(42, Vec3::new(2.0, 0.0, 0.0), true);
    ctx.entities.insert(target.id, target);
    ctx.target = Some(target);
    ctx.created_path = Some(PathHandle {
        id: 1,
        end_node: Some(PathNode { x: 2, y: 0, z: 0 }),
    });
    ctx.in_melee_range = true;

    let mut melee = MeleeAttackGoal::new(1.0, true);
    // game_time = 10 -> 10 - 0 < 20 -> cooldown blocks can_use.
    ctx.game_time = 10;
    assert!(!melee.can_use(&mut ctx));

    ctx.game_time = 20;
    assert!(melee.can_use(&mut ctx));
    melee.start(&mut ctx);
    assert!(ctx.aggressive);

    ctx.random.push_int(3);
    melee.tick(&mut ctx);
    assert_eq!(ctx.swings, vec![InteractionHand::MainHand]);
    assert_eq!(ctx.hurt_targets, vec![target.id]);
    assert_eq!(melee.ticks_until_next_attack(), 20);

    // While path recalculation is on cooldown, stationary targets must NOT draw from random.
    ctx.random.push_float(0.5);
    melee.tick(&mut ctx);
    assert_eq!(ctx.random.floats.len(), 1);

    // On stop, creative/spectator targets are cleared.
    if let Some(t) = ctx.target.as_mut() {
        t.is_creative_or_spectator = true;
    }
    melee.stop(&mut ctx);
    assert!(!ctx.aggressive);
    assert_eq!(
        ctx.set_target_log.last(),
        Some(&(None, TargetReason::ForgotTarget))
    );
}

#[test]
fn leap_at_target_goal_distance_bounds_and_impulse() {
    let mut ctx = MockContext::default();
    let mut leap = LeapAtTargetGoal::new(0.4);

    // Too close (dist^2 = 1.0 < 4.0) -> controlled negative.
    let close = snapshot(5, Vec3::new(1.0, 0.0, 0.0), true);
    ctx.entities.insert(close.id, close);
    ctx.target = Some(close);
    assert!(!leap.can_use(&mut ctx));

    // Too far (dist^2 = 25.0 > 16.0) -> controlled negative.
    let far = snapshot(6, Vec3::new(5.0, 0.0, 0.0), true);
    ctx.entities.insert(far.id, far);
    ctx.target = Some(far);
    assert!(!leap.can_use(&mut ctx));

    // Valid range (dist^2 = 9.0 in [4.0, 16.0]).
    let mid = snapshot(7, Vec3::new(3.0, 0.0, 0.0), true);
    ctx.entities.insert(mid.id, mid);
    ctx.target = Some(mid);
    ctx.delta_movement = Vec3::new(0.5, 0.0, 0.0);
    ctx.random.push_int(0);
    assert!(leap.can_use(&mut ctx));

    leap.start(&mut ctx);
    // normalize((3,0,0))*0.4 + (0.5,0,0)*0.2 = (0.5, 0.4, 0.0)
    assert!((ctx.delta_movement.x - 0.5).abs() < 1e-6);
    assert!((ctx.delta_movement.y - 0.4).abs() < 1e-6);
    assert!((ctx.delta_movement.z).abs() < 1e-6);
}

#[test]
fn use_item_goal_equips_uses_clears_and_plays_sound() {
    let mut ctx = MockContext::default();
    let potion = ItemStackRef::new("minecraft:potion", 1);
    let sound = SoundId("minecraft:entity.witch.drink".to_string());
    let mut goal = UseItemGoal::new(
        potion.clone(),
        Some(sound.clone()),
        |c: &mut MockContext| c.game_time >= 100,
    );
    assert_eq!(goal.flags(), GoalFlags::UNKNOWN_BEHAVIOR);

    assert!(goal.can_use(&mut ctx));
    goal.start(&mut ctx);
    assert!(goal.can_continue_to_use(&mut ctx));
    assert_eq!(
        ctx.equipped.last(),
        Some(&(EquipmentSlot::MainHand, potion))
    );
    assert_eq!(ctx.started_using, vec![InteractionHand::MainHand]);

    ctx.random.push_float(0.5); // pitch = 0.5 * 0.2 + 0.9 = 1.0
    goal.stop(&mut ctx);
    assert_eq!(
        ctx.equipped.last(),
        Some(&(EquipmentSlot::MainHand, ItemStackRef::EMPTY))
    );
    assert_eq!(ctx.sounds.len(), 1);
    assert_eq!(ctx.sounds[0].0, sound);
    assert!((ctx.sounds[0].2 - 1.0).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// Target goal family
// ---------------------------------------------------------------------------

#[test]
fn target_goal_unseen_memory_team_and_must_reach_cache() {
    let mut ctx = MockContext::default();
    let enemy = snapshot(9, Vec3::new(4.0, 0.0, 0.0), true);
    ctx.entities.insert(enemy.id, enemy);
    ctx.target = Some(enemy);

    let mut state = TargetGoalState::new(true, true);
    // must_reach fails when path end node is too far (dx=2 -> dx^2 = 4.0 > 2.25).
    ctx.created_path = Some(PathHandle {
        id: 1,
        end_node: Some(PathNode { x: 2, y: 0, z: 0 }),
    });
    assert!(!state.can_attack(&mut ctx, Some(enemy), TargetingConditions::DEFAULT));

    // Large coordinate delta (e.g. 50_000 blocks) must wrap like Java int instead of panicking in debug builds.
    state.start();
    ctx.created_path = Some(PathHandle {
        id: 3,
        end_node: Some(PathNode {
            x: 50_000,
            y: 0,
            z: 0,
        }),
    });
    let _ = state.can_attack(&mut ctx, Some(enemy), TargetingConditions::DEFAULT);

    // Reset cache and supply close end node (dx=1, dz=1 -> 2.0 <= 2.25).
    state.start();
    ctx.created_path = Some(PathHandle {
        id: 2,
        end_node: Some(PathNode { x: 3, y: 0, z: 1 }),
    });
    assert!(state.can_attack(&mut ctx, Some(enemy), TargetingConditions::DEFAULT));

    // Unseen ticks: reduced_tick_delay(60) = 30. 30 unseen checks pass, 31st fails.
    ctx.has_los = false;
    for _ in 0..30 {
        assert!(state.can_continue_to_use(&mut ctx));
    }
    assert!(!state.can_continue_to_use(&mut ctx));

    // Same team clears target_mob and stops continuing.
    state.start();
    ctx.has_los = true;
    ctx.team = Some(TeamId(7));
    if let Some(t) = ctx.target.as_mut() {
        t.team = Some(TeamId(7));
    }
    assert!(!state.can_continue_to_use(&mut ctx));
}

#[test]
fn hurt_by_nearest_owner_and_universal_anger_target_goals() {
    let mut ctx = MockContext::default();
    let attacker = snapshot(100, Vec3::new(3.0, 0.0, 0.0), false);
    ctx.entities.insert(attacker.id, attacker);
    ctx.last_hurt_by_mob = Some(attacker);
    ctx.last_hurt_by_mob_timestamp = 15;

    let mut hurt_by = HurtByTargetGoal::new(vec![]).set_alert_others(vec![]);
    assert!(hurt_by.can_use(&mut ctx));
    hurt_by.start(&mut ctx);
    assert_eq!(
        ctx.set_target_log.last(),
        Some(&(Some(attacker.id), TargetReason::TargetAttackedEntity))
    );
    assert_eq!(ctx.alerted_same_type.len(), 1);
    // Same timestamp will not re-trigger.
    assert!(!hurt_by.can_use(&mut ctx));

    // NearestAttackableTargetGoal with Player sets ClosestPlayer reason.
    let player = snapshot(200, Vec3::new(5.0, 0.0, 0.0), true);
    ctx.entities.insert(player.id, player);
    ctx.nearest_player = Some(player);
    let mut nearest = NearestAttackableTargetGoal::new(AttackableTargetType::Player, true);
    ctx.random.push_int(0);
    assert!(nearest.can_use(&mut ctx));
    nearest.start(&mut ctx);
    assert_eq!(
        ctx.set_target_log.last(),
        Some(&(Some(player.id), TargetReason::ClosestPlayer))
    );
    // Clearing combat target must NOT be restored by NearestAttackableTargetGoal.
    ctx.target = None;
    assert!(!nearest.can_continue_to_use(&mut ctx));
    assert!(ctx.target.is_none());

    // OwnerHurtByTargetGoal & OwnerHurtTargetGoal require tame & not sitting.
    ctx.is_tame = true;
    ctx.is_ordered_to_sit = false;
    ctx.owner_id = Some(player.id);
    ctx.owner_last_hurt_by = Some((attacker, 22));
    ctx.owner_last_hurt = Some((attacker, 25));

    let mut owner_hurt_by = OwnerHurtByTargetGoal::new();
    assert!(owner_hurt_by.can_use(&mut ctx));
    owner_hurt_by.start(&mut ctx);
    assert_eq!(
        ctx.set_target_log.last(),
        Some(&(Some(attacker.id), TargetReason::TargetAttackedOwner))
    );
    ctx.target = None;
    assert!(!owner_hurt_by.can_continue_to_use(&mut ctx));
    assert!(ctx.target.is_none());

    let mut owner_hurt = OwnerHurtTargetGoal::new();
    assert!(owner_hurt.can_use(&mut ctx));
    owner_hurt.start(&mut ctx);
    assert_eq!(
        ctx.set_target_log.last(),
        Some(&(Some(attacker.id), TargetReason::OwnerAttackedTarget))
    );
    ctx.target = None;
    assert!(!owner_hurt.can_continue_to_use(&mut ctx));
    assert!(ctx.target.is_none());

    // ResetUniversalAngerTargetGoal triggers on player hurt when gamerule enabled.
    ctx.universal_anger = true;
    ctx.last_hurt_by_mob = Some(player);
    ctx.last_hurt_by_mob_timestamp = 30;
    let mut reset_anger = ResetUniversalAngerTargetGoal::new(true);
    assert_eq!(
        <ResetUniversalAngerTargetGoal as Goal<MockContext>>::flags(&reset_anger),
        GoalFlags::UNKNOWN_BEHAVIOR
    );
    assert!(reset_anger.can_use(&mut ctx));
    reset_anger.start(&mut ctx);
    assert_eq!(ctx.universal_anger_refreshes, 1);
    assert_eq!(ctx.universal_anger_alerts.len(), 1);
    assert!(!reset_anger.can_use(&mut ctx));
}

#[test]
fn goal_selector_locks_flags_mutated_during_can_use() {
    struct MutatingFlagGoal {
        flags: GoalFlags,
        started: bool,
    }
    impl Goal<MockContext> for MutatingFlagGoal {
        fn can_use(&mut self, _ctx: &mut MockContext) -> bool {
            self.set_flags(GoalFlags::from_flags([GoalFlag::Move, GoalFlag::Look]));
            true
        }
        fn start(&mut self, _ctx: &mut MockContext) {
            self.started = true;
        }
        fn flags(&self) -> GoalFlags {
            self.flags
        }
        fn set_flags(&mut self, required_control_flags: GoalFlags) {
            self.flags = required_control_flags.normalize();
        }
    }

    let mut ctx = MockContext::default();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut selector = GoalSelector::<MockContext>::new();
    let look_id = selector.add_goal(
        5,
        ProbeGoal::new("look", GoalFlags::from_flag(GoalFlag::Look), Rc::clone(&log)),
    );
    selector.tick(&mut ctx);
    assert!(selector.get_goal(look_id).unwrap().is_running());

    // Add a higher-priority goal that starts with Move only, then adds Look inside can_use.
    let mutating_id = selector.add_goal(
        1,
        MutatingFlagGoal {
            flags: GoalFlags::from_flag(GoalFlag::Move),
            started: false,
        },
    );
    selector.tick(&mut ctx);
    assert!(selector.get_goal(mutating_id).unwrap().is_running());
    assert!(!selector.get_goal(look_id).unwrap().is_running());
    assert_eq!(selector.locked_goal_for(GoalFlag::Move), Some(mutating_id));
    assert_eq!(selector.locked_goal_for(GoalFlag::Look), Some(mutating_id));
}
