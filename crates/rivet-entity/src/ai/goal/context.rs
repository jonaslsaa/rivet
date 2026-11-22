//! Entity/world seam traits and value types used by classic Paper 26.2 AI goals.
//!
//! Per `OWNERSHIP.md`, goals do not hold shared mutable pointers (`Arc<RwLock>`)
//! to entities or worlds. Instead, the tick thread passes `&mut C` implementing
//! [`GoalContext`] into [`crate::ai::goal::GoalSelector::tick`], and entity
//! references are plain [`EntityId`] handles.

/// Opaque entity identifier within a world tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(pub u64);

/// Opaque team identifier for scoreboard team equality checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TeamId(pub u32);

/// Opaque entity type / class tag for type-filtered targeting and alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityTypeId(pub u32);

/// Opaque item stack descriptor used by [`crate::ai::goal::UseItemGoal`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemStackRef {
    pub item_id: String,
    pub count: i32,
}

impl ItemStackRef {
    pub const EMPTY: Self = Self {
        item_id: String::new(),
        count: 0,
    };

    pub fn new(item_id: impl Into<String>, count: i32) -> Self {
        Self {
            item_id: item_id.into(),
            count,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.count <= 0 || self.item_id.is_empty()
    }
}

/// Opaque sound identifier used by [`crate::ai::goal::UseItemGoal`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SoundId(pub String);

/// Opaque damage-type tag identifier used by [`crate::ai::goal::PanicGoal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DamageTypeTagId(pub u32);

/// Default `DamageTypeTags.PANIC_CAUSES` tag identifier.
pub const DAMAGE_TYPE_TAG_PANIC_CAUSES: DamageTypeTagId = DamageTypeTagId(1);

/// 3D floating-point position/velocity vector (`net.minecraft.world.phys.Vec3`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn length_sqr(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn length(self) -> f64 {
        self.length_sqr().sqrt()
    }

    /// Mirrors `Vec3.normalize()` in Minecraft 26.2 (`length < 1.0E-5F ? ZERO : scale(1/length)`).
    pub fn normalize(self) -> Self {
        let len = self.length();
        if len < 1.0e-5_f32 as f64 {
            Self::ZERO
        } else {
            Self::new(self.x / len, self.y / len, self.z / len)
        }
    }

    pub fn scale(self, factor: f64) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }

    pub fn distance_to_sqr(self, other: Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        dx * dx + dy * dy + dz * dz
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

/// Integer block coordinates (`net.minecraft.core.BlockPos`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// Axis-aligned bounding box (`net.minecraft.world.phys.AABB`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Aabb {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl Aabb {
    pub const fn new(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Self {
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    /// Mirrors `AABB.unitCubeFromLowerCorner(Vec3)`.
    pub fn unit_cube_from_lower_corner(pos: Vec3) -> Self {
        Self::new(pos.x, pos.y, pos.z, pos.x + 1.0, pos.y + 1.0, pos.z + 1.0)
    }

    /// Mirrors `AABB.inflate(double x, double y, double z)`.
    pub fn inflate(self, x: f64, y: f64, z: f64) -> Self {
        Self::new(
            self.min_x - x,
            self.min_y - y,
            self.min_z - z,
            self.max_x + x,
            self.max_y + y,
            self.max_z + z,
        )
    }
}

/// Mirrors `net.minecraft.world.InteractionHand`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InteractionHand {
    MainHand,
    OffHand,
}

/// Mirrors `net.minecraft.world.entity.EquipmentSlot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipmentSlot {
    MainHand,
    OffHand,
    Feet,
    Legs,
    Chest,
    Head,
    Body,
    Saddle,
}

/// Mirrors CraftBukkit/Paper `org.bukkit.event.entity.EntityTargetEvent.TargetReason`
/// variants passed by Paper 26.2 target goals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetReason {
    ClosestEntity,
    ClosestPlayer,
    ForgotTarget,
    TargetAttackedEntity,
    TargetAttackedNearbyEntity,
    TargetAttackedOwner,
    OwnerAttackedTarget,
    Unknown,
}

/// Mirrors `net.minecraft.world.entity.ai.targeting.TargetingConditions`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetingConditions {
    pub is_combat: bool,
    pub range: f64,
    pub check_line_of_sight: bool,
    pub test_invisible: bool,
    /// Mirrors Paper `EntitySelector.notRiding(mob)` selector attached by `LookAtPlayerGoal`
    /// when `lookAtType == Player.class`.
    pub exclude_riding_mob: bool,
}

impl TargetingConditions {
    pub const DEFAULT: Self = Self::for_combat();

    pub const fn for_combat() -> Self {
        Self {
            is_combat: true,
            range: -1.0,
            check_line_of_sight: true,
            test_invisible: true,
            exclude_riding_mob: false,
        }
    }

    pub const fn for_non_combat() -> Self {
        Self {
            is_combat: false,
            range: -1.0,
            check_line_of_sight: true,
            test_invisible: true,
            exclude_riding_mob: false,
        }
    }

    pub const fn range(mut self, range: f64) -> Self {
        self.range = range;
        self
    }

    pub const fn ignore_line_of_sight(mut self) -> Self {
        self.check_line_of_sight = false;
        self
    }

    pub const fn ignore_invisibility_testing(mut self) -> Self {
        self.test_invisible = false;
        self
    }

    pub const fn exclude_riding_mob(mut self) -> Self {
        self.exclude_riding_mob = true;
        self
    }
}

/// End node of a computed path (`Path.getEndNode()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathNode {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Lightweight handle representing a computed navigation path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathHandle {
    pub id: u64,
    pub end_node: Option<PathNode>,
}

/// Snapshot of a candidate or active target entity queried by goals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntitySnapshot {
    pub id: EntityId,
    pub entity_type: EntityTypeId,
    pub is_player: bool,
    pub is_creative_or_spectator: bool,
    pub is_alive: bool,
    pub pos: Vec3,
    pub eye_y: f64,
    pub block_pos: BlockPos,
    pub team: Option<TeamId>,
}

/// Random number source used by AI goals (`net.minecraft.util.RandomSource`).
pub trait GoalRandom {
    fn next_float(&mut self) -> f32;
    fn next_double(&mut self) -> f64;
    fn next_int(&mut self, bound: i32) -> i32;
}

/// Mob + world execution context passed into goals on each tick.
///
/// Methods have conservative defaults where a goal family does not require them,
/// allowing minimal test doubles and incremental integration with `rivet-world`.
pub trait GoalContext {
    type Random: GoalRandom;

    fn random(&mut self) -> &mut Self::Random;

    // --- Identity & spatial state ---
    fn mob_id(&self) -> EntityId;
    fn mob_type(&self) -> EntityTypeId;
    fn game_time(&self) -> i64;
    fn pos(&self) -> Vec3;
    fn eye_y(&self) -> f64;
    fn block_pos(&self) -> BlockPos;
    fn bounding_box(&self) -> Aabb;
    fn delta_movement(&self) -> Vec3;
    fn set_delta_movement(&mut self, vx: f64, vy: f64, vz: f64);
    fn on_ground(&self) -> bool;
    fn has_controlling_passenger(&self) -> bool {
        false
    }
    fn no_action_time(&self) -> i32 {
        0
    }
    fn team(&self) -> Option<TeamId> {
        None
    }
    fn is_within_home(&self, _pos: BlockPos) -> bool {
        true
    }
    fn follow_range(&self) -> f64 {
        16.0
    }

    // --- Fluids & fire ---
    fn is_in_water(&self) -> bool {
        false
    }
    fn water_fluid_height(&self) -> f64 {
        0.0
    }
    fn fluid_jump_threshold(&self) -> f64 {
        0.4
    }
    fn is_in_lava(&self) -> bool {
        false
    }
    fn is_on_fire(&self) -> bool {
        false
    }
    fn find_closest_water(&self, _xz_dist: i32, _y_dist: i32) -> Option<BlockPos> {
        None
    }

    // --- Damage & panic ---
    fn has_panic_causing_damage(&self, _tag: DamageTypeTagId) -> bool {
        false
    }
    fn last_hurt_by_mob(&self) -> Option<EntitySnapshot> {
        None
    }
    fn last_hurt_by_mob_timestamp(&self) -> i32 {
        0
    }
    fn universal_anger_enabled(&self) -> bool {
        false
    }
    fn forget_current_target_and_refresh_universal_anger(&mut self) {}
    fn alert_nearby_same_type_universal_anger(&mut self, _search_area: Aabb) {}

    // --- Controls & navigation ---
    fn jump(&mut self) {}
    fn set_can_float(&mut self, _can_float: bool) {}
    fn set_look_at(&mut self, _x: f64, _y: f64, _z: f64) {}
    fn set_look_at_entity(&mut self, _target: EntityId, _y_max_rot: f32, _x_max_rot: f32) {}
    fn nav_is_done(&self) -> bool {
        true
    }
    fn nav_move_to_xyz(&mut self, _x: f64, _y: f64, _z: f64, _speed: f64) -> bool {
        false
    }
    fn nav_move_to_entity(&mut self, _target: EntityId, _speed: f64) -> bool {
        false
    }
    fn nav_move_to_path(&mut self, _path: Option<PathHandle>, _speed: f64) -> bool {
        false
    }
    fn nav_create_path_to_entity(
        &mut self,
        _target: EntityId,
        _accuracy: i32,
    ) -> Option<PathHandle> {
        None
    }
    fn nav_stop(&mut self) {}
    fn default_random_pos(&mut self, _xz_range: i32, _y_range: i32) -> Option<Vec3> {
        None
    }
    fn land_random_pos(&mut self, _xz_range: i32, _y_range: i32) -> Option<Vec3> {
        None
    }

    // --- Combat, sensing & targeting ---
    fn target(&self) -> Option<EntitySnapshot> {
        None
    }
    fn set_target(&mut self, _target: Option<EntityId>, _reason: TargetReason) {}
    fn can_attack(&self, _target: EntityId) -> bool {
        true
    }
    fn test_targeting_conditions(
        &self,
        _target: EntityId,
        _conditions: TargetingConditions,
    ) -> bool {
        true
    }
    fn has_line_of_sight(&mut self, _target: EntityId) -> bool {
        true
    }
    fn is_within_melee_attack_range(&self, _target: EntityId) -> bool {
        false
    }
    fn set_aggressive(&mut self, _aggressive: bool) {}
    fn swing(&mut self, _hand: InteractionHand) {}
    fn do_hurt_target(&mut self, _target: EntityId) -> bool {
        false
    }
    fn entity_snapshot(&self, _id: EntityId) -> Option<EntitySnapshot> {
        None
    }
    fn nearest_player(
        &self,
        _conditions: TargetingConditions,
        _x: f64,
        _eye_y: f64,
        _z: f64,
    ) -> Option<EntitySnapshot> {
        None
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
        None
    }
    fn alert_nearby_same_type(
        &mut self,
        _search_area: Aabb,
        _attacker: EntityId,
        _ignored_types: &[EntityTypeId],
    ) {
    }

    // --- Tamable / owner hooks ---
    fn is_tame(&self) -> bool {
        false
    }
    fn is_ordered_to_sit(&self) -> bool {
        false
    }
    fn owner_id(&self) -> Option<EntityId> {
        None
    }
    fn owner_last_hurt_by_mob(&self) -> Option<(EntitySnapshot, i32)> {
        None
    }
    fn owner_last_hurt_mob(&self) -> Option<(EntitySnapshot, i32)> {
        None
    }
    fn wants_to_attack(&self, _target: EntityId, _owner: EntityId) -> bool {
        true
    }

    // --- Item usage ---
    fn is_using_item(&self) -> bool {
        false
    }
    fn set_item_slot(&mut self, _slot: EquipmentSlot, _item: ItemStackRef) {}
    fn start_using_item(&mut self, _hand: InteractionHand) {}
    fn play_sound(&mut self, _sound: &SoundId, _volume: f32, _pitch: f32) {}
}
