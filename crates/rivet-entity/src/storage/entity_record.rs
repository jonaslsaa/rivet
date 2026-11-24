//! Current-format entity NBT persistence, passenger recursion, UUID/network-ID
//! restoration, and Paper per-chunk entity save/load limits.
//!
//! Java sources (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`):
//! - `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/entity/Entity.java`
//! - `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/entity/EntityType.java`
//! - `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/chunk/storage/EntityStorage.java`

use indexmap::IndexMap;
use rivet_core::shared_constants::WORLD_VERSION;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::double_tag::DoubleTag;
use rivet_nbt::float_tag::FloatTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::nbt_utils::add_current_data_version;
use rivet_nbt::tag::Tag;
use rivet_registry::core::{
    BlockPos, ChunkPos, SectionPos, Vec3, uuid_from_int_array, uuid_to_int_array,
};
use rivet_util::mth::{self, Uuid};
use rivet_util::random::{SingleThreadedRandomSource, random_support};
use std::collections::HashMap;
use thiserror::Error;

/// `Entity.TAG_ID`.
pub const TAG_ID: &str = "id";
/// `Entity.TAG_UUID`.
pub const TAG_UUID: &str = "UUID";
/// `Entity.TAG_PASSENGERS`.
pub const TAG_PASSENGERS: &str = "Passengers";
/// `EntityStorage.ENTITIES_TAG`.
pub const TAG_ENTITIES: &str = "Entities";
/// `EntityStorage.POSITION_TAG`.
pub const TAG_POSITION: &str = "Position";
/// `NbtUtils.DATA_VERSION_TAG`.
pub const TAG_DATA_VERSION: &str = "DataVersion";

/// Maximum horizontal entity coordinate clamped by Paper `Entity.load`.
pub const MAX_HORIZONTAL_POSITION: f64 = 3.0000512E7;
/// Maximum vertical entity coordinate clamped by Paper `Entity.load`.
pub const MAX_VERTICAL_POSITION: f64 = 2.0E7;
/// Maximum velocity component magnitude before Paper `Entity.load` zeroes it.
pub const MAX_MOTION_COMPONENT: f64 = 10.0;

/// Known base-entity NBT keys handled explicitly by `StoredEntityRecord`
/// (excluded when copying `extra_tags`).
const MANAGED_ENTITY_KEYS: &[&str] = &[
    TAG_ID,
    TAG_UUID,
    TAG_PASSENGERS,
    "Pos",
    "Motion",
    "Rotation",
    "fall_distance",
    "Fire",
    "Air",
    "OnGround",
    "Invulnerable",
    "PortalCooldown",
    "WorldUUIDMost",
    "WorldUUIDLeast",
    "Bukkit.persist",
];

/// Errors returned by current-format entity persistence and section storage.
///
/// Malformed or unsupported current-format data fails hard; historical
/// `DataVersion` payloads are explicitly rejected rather than silently
/// migrated or coerced.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum EntityStorageError {
    #[error("entity chunk tag is missing required DataVersion")]
    MissingDataVersion,

    #[error(
        "unsupported entity chunk DataVersion {found} (current format is {expected}; historical conversion is not performed here)"
    )]
    UnsupportedDataVersion { found: i32, expected: i32 },

    #[error("entity chunk tag is missing required Position int array")]
    MissingPosition,

    #[error("entity chunk Position must be a 2-element IntArrayTag, found length {found}")]
    MalformedPosition { found: usize },

    #[error("misplaced entity chunk: expected ({}, {}), found ({}, {})", expected.x(), expected.z(), stored.x(), stored.z())]
    MisplacedChunk {
        expected: ChunkPos,
        stored: ChunkPos,
    },

    #[error("entity chunk Entities tag is not a ListTag")]
    MalformedEntitiesList,

    #[error("entity list element at index {index} is not a CompoundTag")]
    MalformedEntityElement { index: usize },

    #[error("entity tag is missing required string id")]
    MissingEntityId,

    #[error("entity tag has empty id")]
    EmptyEntityId,

    #[error("entity tag has malformed UUID (expected 4-int array)")]
    MalformedUuid,

    #[error("entity tag has non-finite position")]
    NonFinitePosition,

    #[error("entity tag has non-finite rotation")]
    NonFiniteRotation,

    #[error("entity tag has malformed {field} list (expected {expected} numeric elements)")]
    MalformedVector {
        field: &'static str,
        expected: usize,
    },

    #[error("entity Passengers tag is not a ListTag of CompoundTags")]
    MalformedPassengersList,

    #[error("duplicate entity UUID {uuid:?}")]
    DuplicateEntityUuid { uuid: Uuid },

    #[error("duplicate entity network id {id}")]
    DuplicateNetworkId { id: i32 },

    #[error("entity storage backend I/O error: {message}")]
    Backend { message: String },
}

/// Axis-aligned bounding box matching `net.minecraft.world.phys.AABB`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl Aabb {
    /// Creates an AABB from min/max corners (ordering coordinates like Java's `new AABB`).
    #[must_use]
    pub fn new(x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) -> Self {
        Self {
            min_x: x0.min(x1),
            min_y: y0.min(y1),
            min_z: z0.min(z1),
            max_x: x0.max(x1),
            max_y: y0.max(y1),
            max_z: z0.max(z1),
        }
    }

    /// Creates a unit-sized bounding box centered horizontally around `pos`.
    #[must_use]
    pub fn unit_at(pos: Vec3) -> Self {
        Self::new(
            pos.x - 0.5,
            pos.y,
            pos.z - 0.5,
            pos.x + 0.5,
            pos.y + 1.0,
            pos.z + 0.5,
        )
    }

    /// `AABB.intersects(AABB)` — strict open-interval overlap check matching Java.
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.min_x < other.max_x
            && self.max_x > other.min_x
            && self.min_y < other.max_y
            && self.max_y > other.min_y
            && self.min_z < other.max_z
            && self.max_z > other.min_z
    }
}

/// Port of `net.minecraft.world.entity.Entity.RemovalReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntityRemovalReason {
    Killed,
    Discarded,
    UnloadedToChunk,
    UnloadedWithPlayer,
    ChangedDimension,
}

impl EntityRemovalReason {
    /// `RemovalReason.shouldDestroy()`.
    #[must_use]
    pub const fn should_destroy(self) -> bool {
        matches!(self, Self::Killed | Self::Discarded)
    }

    /// `RemovalReason.shouldSave()` (only `UNLOADED_TO_CHUNK(false, true)` saves).
    #[must_use]
    pub const fn should_save(self) -> bool {
        matches!(self, Self::UnloadedToChunk)
    }
}

/// Port of `net.minecraft.world.entity.Entity.ENTITY_COUNTER` (`AtomicInteger`).
///
/// Allocates monotonically increasing wrapping `i32` network IDs starting at `1`
/// (`incrementAndGet()` on an initial `0` counter).
#[derive(Clone, Debug)]
pub struct NetworkIdAllocator {
    next: i32,
}

impl Default for NetworkIdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkIdAllocator {
    /// Creates an allocator whose first allocated ID is `1`.
    #[must_use]
    pub const fn new() -> Self {
        Self { next: 0 }
    }

    /// Creates an allocator starting after `last_issued` (next call returns `last_issued.wrapping_add(1)`).
    #[must_use]
    pub const fn starting_after(last_issued: i32) -> Self {
        Self { next: last_issued }
    }

    /// Allocates the next wrapping `i32` network entity ID.
    pub fn next_id(&mut self) -> i32 {
        self.next = self.next.wrapping_add(1);
        self.next
    }

    /// Returns the last issued ID without advancing.
    #[must_use]
    pub const fn current(&self) -> i32 {
        self.next
    }
}

/// Port of Paper's `chunks.entityPerChunkSaveLimit` configuration.
///
/// A negative limit (or absent entry) means unlimited. A non-negative limit `n`
/// caps the number of entities of that type loaded or saved per chunk to `n`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EntitySaveLimits {
    limits: IndexMap<String, i32>,
}

impl EntitySaveLimits {
    /// Creates an empty limit map (no per-type caps).
    #[must_use]
    pub fn new() -> Self {
        Self {
            limits: IndexMap::new(),
        }
    }

    /// Sets the per-chunk limit for an entity type ID (e.g. `"minecraft:arrow"`).
    pub fn set_limit(&mut self, entity_type_id: impl Into<String>, limit: i32) {
        self.limits.insert(entity_type_id.into(), limit);
    }

    /// Returns the configured limit for `entity_type_id`, or `-1` if unlimited.
    #[must_use]
    pub fn limit_for(&self, entity_type_id: &str) -> i32 {
        self.limits.get(entity_type_id).copied().unwrap_or(-1)
    }

    /// Checks whether one more entity of `entity_type_id` can be accepted under
    /// `counts`, incrementing `counts` with wrapping `i32` addition if allowed.
    pub fn try_consume(&self, entity_type_id: &str, counts: &mut HashMap<String, i32>) -> bool {
        let save_limit = self.limit_for(entity_type_id);
        if save_limit <= -1 {
            return true;
        }
        let current = counts.get(entity_type_id).copied().unwrap_or(0);
        if current >= save_limit {
            return false;
        }
        counts.insert(entity_type_id.to_string(), current.wrapping_add(1));
        true
    }
}

/// Current-format persisted entity record corresponding to `Entity.saveWithoutId` / `Entity.load`.
///
/// Carries base entity state, restored UUID, assigned network ID, passenger/vehicle
/// relationships, and any entity-family-specific NBT fields in `extra_tags`.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredEntityRecord {
    /// Namespaced entity type ID (`"id"`, e.g. `"minecraft:pig"`).
    pub id: String,
    /// Runtime network entity ID assigned on load or creation.
    pub network_id: i32,
    /// Persistent entity UUID (`"UUID"` 4-int array).
    pub uuid: Uuid,
    /// Entity position (`"Pos"`).
    pub pos: Vec3,
    /// Entity velocity (`"Motion"`).
    pub motion: Vec3,
    /// Yaw rotation (`"Rotation"[0]`).
    pub y_rot: f32,
    /// Pitch rotation (`"Rotation"[1]`).
    pub x_rot: f32,
    /// Fall distance (`"fall_distance"`).
    pub fall_distance: f64,
    /// Remaining fire ticks (`"Fire"`).
    pub fire_ticks: i16,
    /// Air supply (`"Air"`).
    pub air_supply: i16,
    /// On-ground flag (`"OnGround"`).
    pub on_ground: bool,
    /// Invulnerable flag (`"Invulnerable"`).
    pub invulnerable: bool,
    /// Portal cooldown ticks (`"PortalCooldown"`).
    pub portal_cooldown: i32,
    /// Optional world UUID (`"WorldUUIDMost"` / `"WorldUUIDLeast"`).
    pub world_uuid: Option<Uuid>,
    /// CraftBukkit/Paper `persist` flag (`"Bukkit.persist"`, defaults to `true`).
    pub persist: bool,
    /// Mirrors `EntityAccess.isAlwaysTicking()`.
    pub always_ticking: bool,
    /// Current removal reason, if removed.
    pub removal_reason: Option<EntityRemovalReason>,
    /// Runtime network ID of the vehicle this entity is riding, if any.
    pub vehicle_id: Option<i32>,
    /// Runtime network IDs of direct passengers riding this entity (in order).
    pub passenger_ids: Vec<i32>,
    /// Nested passenger records used during recursive NBT decode/encode.
    pub passengers: Vec<StoredEntityRecord>,
    /// Bounding box used for section spatial queries.
    pub bounding_box: Aabb,
    /// Entity-family-specific NBT fields preserved verbatim across save/load.
    pub extra_tags: CompoundTag,
}

impl StoredEntityRecord {
    /// Creates a new default entity record at `pos` with the given type, network ID, and UUID.
    #[must_use]
    pub fn new(id: impl Into<String>, network_id: i32, uuid: Uuid, pos: Vec3) -> Self {
        Self {
            id: id.into(),
            network_id,
            uuid,
            pos,
            motion: Vec3::new(0.0, 0.0, 0.0),
            y_rot: 0.0,
            x_rot: 0.0,
            fall_distance: 0.0,
            fire_ticks: 0,
            air_supply: 300,
            on_ground: false,
            invulnerable: false,
            portal_cooldown: 0,
            world_uuid: None,
            persist: true,
            always_ticking: false,
            removal_reason: None,
            vehicle_id: None,
            passenger_ids: Vec::new(),
            passengers: Vec::new(),
            bounding_box: Aabb::unit_at(pos),
            extra_tags: CompoundTag::new(),
        }
    }

    /// Returns the entity's integer block position (`Entity.blockPosition()`).
    #[must_use]
    pub fn block_position(&self) -> BlockPos {
        BlockPos::new(
            mth::floor_d(self.pos.x),
            mth::floor_d(self.pos.y),
            mth::floor_d(self.pos.z),
        )
    }

    /// Returns the packed `SectionPos` long for this entity's current block position.
    #[must_use]
    pub fn section_key(&self) -> i64 {
        let bp = self.block_position();
        SectionPos::as_long_block_pos(&bp)
    }

    /// Returns the packed `ChunkPos` long for this entity's current block position.
    #[must_use]
    pub fn chunk_key(&self) -> i64 {
        let bp = self.block_position();
        ChunkPos::pack_block_pos(&bp)
    }

    /// Returns `true` if this entity is currently riding a vehicle (`Entity.isPassenger()`).
    #[must_use]
    pub const fn is_passenger(&self) -> bool {
        self.vehicle_id.is_some()
    }

    /// Returns `true` if this entity has been removed (`Entity.isRemoved()`).
    #[must_use]
    pub const fn is_removed(&self) -> bool {
        self.removal_reason.is_some()
    }

    /// Mirrors `EntityAccess.shouldBeSaved()` / `Entity.saveAsPassenger` eligibility:
    /// must be persistent, have a non-empty type ID, and either not removed or
    /// removed for a saveable reason (`UnloadedToChunk`).
    #[must_use]
    pub fn should_be_saved(&self) -> bool {
        if !self.persist || self.id.is_empty() {
            return false;
        }
        match self.removal_reason {
            Some(reason) => reason.should_save(),
            None => true,
        }
    }

    /// Updates position and shifts the bounding box by the same delta.
    pub fn set_position(&mut self, new_pos: Vec3) {
        let dx = new_pos.x - self.pos.x;
        let dy = new_pos.y - self.pos.y;
        let dz = new_pos.z - self.pos.z;
        self.pos = new_pos;
        self.bounding_box = Aabb::new(
            self.bounding_box.min_x + dx,
            self.bounding_box.min_y + dy,
            self.bounding_box.min_z + dz,
            self.bounding_box.max_x + dx,
            self.bounding_box.max_y + dy,
            self.bounding_box.max_z + dz,
        );
    }

    /// Encodes this entity as a root entity (`Entity.save`).
    ///
    /// Returns `None` if the entity is a passenger (`is_passenger()`) or not saveable.
    #[must_use]
    pub fn save_root(&self) -> Option<CompoundTag> {
        if self.is_passenger() {
            return None;
        }
        self.save_as_passenger(None)
    }

    /// Encodes this entity (and recursively its `passengers`) to current-format NBT
    /// (`Entity.saveAsPassenger` / `Entity.saveWithoutId`).
    ///
    /// When `vehicle_pos` is `Some`, saves `Pos` as `(vehicle.x, self.y, vehicle.z)`
    /// matching Paper `Entity.saveWithoutId` lines 2642-2646.
    #[must_use]
    pub fn save_as_passenger(&self, vehicle_pos: Option<Vec3>) -> Option<CompoundTag> {
        if !self.should_be_saved() {
            return None;
        }

        let mut tag = CompoundTag::new();
        tag.put_string(TAG_ID, &self.id);

        let saved_pos = match vehicle_pos {
            Some(vp) => Vec3::new(vp.x, self.pos.y, vp.z),
            None => self.pos,
        };
        let mut pos_list = ListTag::new();
        pos_list.add(Tag::Double(DoubleTag::value_of(saved_pos.x)));
        pos_list.add(Tag::Double(DoubleTag::value_of(saved_pos.y)));
        pos_list.add(Tag::Double(DoubleTag::value_of(saved_pos.z)));
        tag.put("Pos".to_string(), Tag::List(pos_list));

        let mut motion_list = ListTag::new();
        motion_list.add(Tag::Double(DoubleTag::value_of(self.motion.x)));
        motion_list.add(Tag::Double(DoubleTag::value_of(self.motion.y)));
        motion_list.add(Tag::Double(DoubleTag::value_of(self.motion.z)));
        tag.put("Motion".to_string(), Tag::List(motion_list));

        // Paper/CraftBukkit: reset NaN yaw/pitch to 0.0 on save.
        let y_rot = if self.y_rot.is_nan() { 0.0 } else { self.y_rot };
        let x_rot = if self.x_rot.is_nan() { 0.0 } else { self.x_rot };
        let mut rot_list = ListTag::new();
        rot_list.add(Tag::Float(FloatTag::value_of(y_rot)));
        rot_list.add(Tag::Float(FloatTag::value_of(x_rot)));
        tag.put("Rotation".to_string(), Tag::List(rot_list));

        tag.put_double("fall_distance", self.fall_distance);
        tag.put_short("Fire", self.fire_ticks);
        tag.put_short("Air", self.air_supply);
        tag.put_boolean("OnGround", self.on_ground);
        tag.put_boolean("Invulnerable", self.invulnerable);
        tag.put_int("PortalCooldown", self.portal_cooldown);
        tag.put_int_array(TAG_UUID, uuid_to_int_array(self.uuid).to_vec());

        if let Some(world_uuid) = self.world_uuid {
            tag.put_long("WorldUUIDLeast", world_uuid.least);
            tag.put_long("WorldUUIDMost", world_uuid.most);
        }

        if !self.persist {
            tag.put_boolean("Bukkit.persist", false);
        }

        // Preserve entity-family-specific extra tags without overwriting managed base keys.
        for (key, value) in self.extra_tags.entry_set() {
            if !MANAGED_ENTITY_KEYS.contains(&key.as_str()) {
                tag.put(key.clone(), value.copy_tag());
            }
        }

        // Recursively encode passengers; omit `"Passengers"` when empty (Paper lines 2730-2743).
        if !self.passengers.is_empty() {
            let mut passengers_list = ListTag::new();
            for passenger in &self.passengers {
                if let Some(passenger_tag) = passenger.save_as_passenger(Some(saved_pos)) {
                    passengers_list.add(Tag::Compound(passenger_tag));
                }
            }
            if !passengers_list.list.is_empty() {
                tag.put(TAG_PASSENGERS.to_string(), Tag::List(passengers_list));
            }
        }

        Some(tag)
    }

    /// Decodes a single entity and its recursive `Passengers` from current-format NBT
    /// (`EntityType.loadEntityRecursive` + `Entity.load`).
    ///
    /// Allocates a fresh network ID from `allocator` for this entity and each
    /// passenger, restores `UUID` from the 4-int array, clamps `Pos` and `Motion`
    /// identically to Paper `Entity.load`, and rejects malformed/non-finite data.
    pub fn load_recursive(
        tag: &CompoundTag,
        allocator: &mut NetworkIdAllocator,
        limits: &EntitySaveLimits,
        loaded_counts: &mut HashMap<String, i32>,
    ) -> Result<Option<Self>, EntityStorageError> {
        let Some(id_str) = tag.get_string(TAG_ID) else {
            return Err(EntityStorageError::MissingEntityId);
        };
        if id_str.is_empty() {
            return Err(EntityStorageError::EmptyEntityId);
        }

        // Paper per-chunk entity load limit (`EntityType.loadEntitiesRecursive`).
        if !limits.try_consume(id_str, loaded_counts) {
            return Ok(None);
        }

        let pos = match tag.tags.get("Pos") {
            Some(raw) => decode_vec3(raw, "Pos")?,
            None => Vec3::new(0.0, 0.0, 0.0),
        };
        if !pos.x.is_finite() || !pos.y.is_finite() || !pos.z.is_finite() {
            return Err(EntityStorageError::NonFinitePosition);
        }
        let clamped_pos = Vec3::new(
            pos.x
                .clamp(-MAX_HORIZONTAL_POSITION, MAX_HORIZONTAL_POSITION),
            pos.y.clamp(-MAX_VERTICAL_POSITION, MAX_VERTICAL_POSITION),
            pos.z
                .clamp(-MAX_HORIZONTAL_POSITION, MAX_HORIZONTAL_POSITION),
        );

        let raw_motion = match tag.tags.get("Motion") {
            Some(raw) => decode_vec3(raw, "Motion")?,
            None => Vec3::new(0.0, 0.0, 0.0),
        };
        let clamped_motion = Vec3::new(
            if raw_motion.x.abs() > MAX_MOTION_COMPONENT {
                0.0
            } else {
                raw_motion.x
            },
            if raw_motion.y.abs() > MAX_MOTION_COMPONENT {
                0.0
            } else {
                raw_motion.y
            },
            if raw_motion.z.abs() > MAX_MOTION_COMPONENT {
                0.0
            } else {
                raw_motion.z
            },
        );

        let (y_rot, x_rot) = match tag.tags.get("Rotation") {
            Some(raw) => decode_vec2_f32(raw, "Rotation")?,
            None => (0.0, 0.0),
        };
        if !y_rot.is_finite() || !x_rot.is_finite() {
            return Err(EntityStorageError::NonFiniteRotation);
        }

        let network_id = allocator.next_id();

        let uuid = match tag.tags.get(TAG_UUID) {
            Some(Tag::IntArray(arr)) => {
                if arr.data.len() != 4 {
                    return Err(EntityStorageError::MalformedUuid);
                }
                let ints = [arr.data[0], arr.data[1], arr.data[2], arr.data[3]];
                uuid_from_int_array(&ints)
            }
            Some(_) => return Err(EntityStorageError::MalformedUuid),
            None => {
                // Matches Paper `Entity` constructor: `Mth.createInsecureUUID(RandomSource.create())`.
                let mut rng =
                    SingleThreadedRandomSource::new(random_support::generate_unique_seed());
                mth::create_insecure_uuid(&mut rng)
            }
        };

        let world_uuid = match (
            tag.get_long("WorldUUIDMost"),
            tag.get_long("WorldUUIDLeast"),
        ) {
            (Some(most), Some(least)) => Some(Uuid { most, least }),
            _ => None,
        };

        let mut extra_tags = CompoundTag::new();
        for (key, value) in tag.entry_set() {
            if !MANAGED_ENTITY_KEYS.contains(&key.as_str()) {
                extra_tags.put(key.clone(), value.copy_tag());
            }
        }

        let mut passengers = Vec::new();
        let mut passenger_ids = Vec::new();
        if let Some(raw_passengers) = tag.tags.get(TAG_PASSENGERS) {
            let Tag::List(list) = raw_passengers else {
                return Err(EntityStorageError::MalformedPassengersList);
            };
            for element in list.iter() {
                let Tag::Compound(passenger_tag) = element else {
                    return Err(EntityStorageError::MalformedPassengersList);
                };
                if let Some(mut passenger) =
                    Self::load_recursive(passenger_tag, allocator, limits, loaded_counts)?
                {
                    passenger.vehicle_id = Some(network_id);
                    passenger_ids.push(passenger.network_id);
                    passengers.push(passenger);
                }
            }
        }

        Ok(Some(Self {
            id: id_str.clone(),
            network_id,
            uuid,
            pos: clamped_pos,
            motion: clamped_motion,
            y_rot,
            x_rot,
            fall_distance: tag.get_double_or("fall_distance", 0.0),
            fire_ticks: tag.get_short_or("Fire", 0),
            air_supply: tag.get_short_or("Air", 300),
            on_ground: tag.get_boolean_or("OnGround", false),
            invulnerable: tag.get_boolean_or("Invulnerable", false),
            portal_cooldown: tag.get_int_or("PortalCooldown", 0),
            world_uuid,
            persist: tag.get_boolean_or("Bukkit.persist", true),
            always_ticking: false,
            removal_reason: None,
            vehicle_id: None,
            passenger_ids,
            passengers,
            bounding_box: Aabb::unit_at(clamped_pos),
            extra_tags,
        }))
    }

    /// Flattens this root entity and its nested `passengers` tree in pre-order
    /// (vehicle before passengers, matching Paper `Entity.getSelfAndPassengers`
    /// / `EntityType.loadEntitiesRecursive`), wiring `vehicle_id` and `passenger_ids`
    /// links from the nested tree if not already populated.
    #[must_use]
    pub fn flatten_tree(mut self) -> Vec<StoredEntityRecord> {
        let self_id = self.network_id;
        let mut nested = std::mem::take(&mut self.passengers);
        for child in &mut nested {
            child.vehicle_id = Some(self_id);
            if !self.passenger_ids.contains(&child.network_id) {
                self.passenger_ids.push(child.network_id);
            }
        }
        let mut out = vec![self];
        for child in nested {
            out.extend(child.flatten_tree());
        }
        out
    }
}

/// Encodes a chunk's entities into a current-format `CompoundTag`
/// (`EntityStorage.storeEntities`).
///
/// Accepts either pre-nested root trees or flattened slices (such as those
/// returned by [`decode_chunk_entities_nbt`] / `EntityStorage::load_entities`),
/// reconstructing nested `Passengers` from `passenger_ids` when needed.
///
/// Writes `DataVersion = WORLD_VERSION`, `Entities`, and `Position = [x, z]`.
/// Enforces Paper's per-chunk `EntitySaveLimits` and `Entity.save` eligibility.
#[must_use]
pub fn encode_chunk_entities_nbt(
    pos: ChunkPos,
    entities: &[StoredEntityRecord],
    limits: &EntitySaveLimits,
) -> CompoundTag {
    let by_id: HashMap<i32, &StoredEntityRecord> = entities
        .iter()
        .map(|entity| (entity.network_id, entity))
        .collect();

    let mut entities_list = ListTag::new();
    let mut saved_counts: HashMap<String, i32> = HashMap::new();

    for entity in entities {
        if entity.is_passenger() {
            continue;
        }
        if !limits.try_consume(&entity.id, &mut saved_counts) {
            continue;
        }
        let mut visited = std::collections::HashSet::new();
        let reconstructed = reconstruct_passenger_tree(entity, &by_id, &mut visited);
        if let Some(encoded) = reconstructed.save_root() {
            entities_list.add(Tag::Compound(encoded));
        }
    }

    let mut chunk_tag = CompoundTag::new();
    add_current_data_version(&mut chunk_tag);
    chunk_tag.put(TAG_ENTITIES.to_string(), Tag::List(entities_list));
    chunk_tag.put_int_array(TAG_POSITION, vec![pos.x(), pos.z()]);
    chunk_tag
}

fn reconstruct_passenger_tree(
    entity: &StoredEntityRecord,
    by_id: &HashMap<i32, &StoredEntityRecord>,
    visited: &mut std::collections::HashSet<i32>,
) -> StoredEntityRecord {
    visited.insert(entity.network_id);
    let mut out = entity.clone();
    if out.passengers.is_empty() && !out.passenger_ids.is_empty() {
        out.passengers = out
            .passenger_ids
            .iter()
            .filter_map(|pid| {
                if visited.contains(pid) {
                    return None;
                }
                by_id
                    .get(pid)
                    .map(|child| reconstruct_passenger_tree(child, by_id, visited))
            })
            .collect();
    }
    out
}

/// Decodes a current-format chunk entity `CompoundTag` into flattened entities
/// with restored UUIDs, fresh network IDs, and wired passenger/vehicle links
/// (`EntityStorage.loadEntities` + `EntityType.loadEntitiesRecursive`).
///
/// Hard-fails on missing or non-current `DataVersion`, missing/malformed/misplaced
/// `Position`, or malformed `Entities` / entity payloads.
pub fn decode_chunk_entities_nbt(
    expected_pos: ChunkPos,
    tag: &CompoundTag,
    allocator: &mut NetworkIdAllocator,
    limits: &EntitySaveLimits,
) -> Result<Vec<StoredEntityRecord>, EntityStorageError> {
    let Some(data_version) = tag.get_int(TAG_DATA_VERSION) else {
        return Err(EntityStorageError::MissingDataVersion);
    };
    if data_version != WORLD_VERSION {
        return Err(EntityStorageError::UnsupportedDataVersion {
            found: data_version,
            expected: WORLD_VERSION,
        });
    }

    let Some(pos_ints) = tag.get_int_array(TAG_POSITION) else {
        return Err(EntityStorageError::MissingPosition);
    };
    if pos_ints.len() != 2 {
        return Err(EntityStorageError::MalformedPosition {
            found: pos_ints.len(),
        });
    }
    let stored_pos = ChunkPos::new(pos_ints[0], pos_ints[1]);
    if stored_pos != expected_pos {
        return Err(EntityStorageError::MisplacedChunk {
            expected: expected_pos,
            stored: stored_pos,
        });
    }

    let Some(raw_entities) = tag.tags.get(TAG_ENTITIES) else {
        return Ok(Vec::new());
    };
    let Tag::List(entities_list) = raw_entities else {
        return Err(EntityStorageError::MalformedEntitiesList);
    };

    let mut loaded = Vec::new();
    let mut loaded_counts: HashMap<String, i32> = HashMap::new();
    for (index, element) in entities_list.iter().enumerate() {
        let Tag::Compound(entity_tag) = element else {
            return Err(EntityStorageError::MalformedEntityElement { index });
        };
        if let Some(root) =
            StoredEntityRecord::load_recursive(entity_tag, allocator, limits, &mut loaded_counts)?
        {
            loaded.extend(root.flatten_tree());
        }
    }

    Ok(loaded)
}

fn decode_vec3(tag: &Tag, field: &'static str) -> Result<Vec3, EntityStorageError> {
    let Tag::List(list) = tag else {
        return Err(EntityStorageError::MalformedVector { field, expected: 3 });
    };
    if list.list.len() != 3 {
        return Err(EntityStorageError::MalformedVector { field, expected: 3 });
    }
    let x = tag_as_f64(&list.list[0])
        .ok_or(EntityStorageError::MalformedVector { field, expected: 3 })?;
    let y = tag_as_f64(&list.list[1])
        .ok_or(EntityStorageError::MalformedVector { field, expected: 3 })?;
    let z = tag_as_f64(&list.list[2])
        .ok_or(EntityStorageError::MalformedVector { field, expected: 3 })?;
    Ok(Vec3::new(x, y, z))
}

fn decode_vec2_f32(tag: &Tag, field: &'static str) -> Result<(f32, f32), EntityStorageError> {
    let Tag::List(list) = tag else {
        return Err(EntityStorageError::MalformedVector { field, expected: 2 });
    };
    if list.list.len() != 2 {
        return Err(EntityStorageError::MalformedVector { field, expected: 2 });
    }
    let x = tag_as_f32(&list.list[0])
        .ok_or(EntityStorageError::MalformedVector { field, expected: 2 })?;
    let y = tag_as_f32(&list.list[1])
        .ok_or(EntityStorageError::MalformedVector { field, expected: 2 })?;
    Ok((x, y))
}

fn tag_as_f64(tag: &Tag) -> Option<f64> {
    match tag {
        Tag::Double(d) => Some(d.value),
        Tag::Float(f) => Some(f64::from(f.value)),
        Tag::Int(i) => Some(f64::from(i.value)),
        Tag::Short(s) => Some(f64::from(s.value)),
        Tag::Byte(b) => Some(f64::from(b.value)),
        Tag::Long(l) => Some(l.value as f64),
        _ => None,
    }
}

fn tag_as_f32(tag: &Tag) -> Option<f32> {
    match tag {
        Tag::Float(f) => Some(f.value),
        Tag::Double(d) => Some(d.value as f32),
        Tag::Int(i) => Some(i.value as f32),
        Tag::Short(s) => Some(f32::from(s.value)),
        Tag::Byte(b) => Some(f32::from(b.value)),
        Tag::Long(l) => Some(l.value as f32),
        _ => None,
    }
}
