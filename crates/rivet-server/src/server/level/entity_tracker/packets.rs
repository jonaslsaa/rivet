//! Typed tracker packet surface consumed by `ServerEntity` and `TrackedEntity`.
//!
//! Consumes the packet structs already delivered by #90 in `rivet-protocol`
//! (`ClientboundAddEntityPacket`, `ClientboundSetEntityDataPacket`,
//! `ClientboundUpdateAttributesPacket`, `ClientboundSetPassengersPacket`,
//! `ClientboundRemoveEntitiesPacket`) without re-owning wire codecs.

use rivet_protocol::game::{
    ClientboundAddEntityPacket, ClientboundRemoveEntitiesPacket, ClientboundSetEntityDataPacket,
    ClientboundSetPassengersPacket, ClientboundUpdateAttributesPacket,
};
use rivet_registry::world::phys::Vec3;

/// `net.minecraft.world.entity.EquipmentSlot` in Java declaration order
/// (`EquipmentSlot.VALUES`), used for `ServerEntity::sendPairingData` and
/// `LivingEntity::detectEquipmentUpdates`.
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

impl EquipmentSlot {
    /// `EquipmentSlot.VALUES` in Java declaration order.
    pub const VALUES: [Self; 8] = [
        Self::MainHand,
        Self::OffHand,
        Self::Feet,
        Self::Legs,
        Self::Chest,
        Self::Head,
        Self::Body,
        Self::Saddle,
    ];
}

/// An opaque equipment item payload carried in `ClientboundSetEquipmentPacket`.
///
/// The tracker does not own item serialization; callers supply the item payload
/// representation (e.g. encoded stack bytes or item summary) and the tracker
/// preserves slot ordering and non-empty filtering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipmentEntry {
    pub slot: EquipmentSlot,
    pub item_id: String,
    pub count: i32,
}

impl EquipmentEntry {
    pub fn new(slot: EquipmentSlot, item_id: impl Into<String>, count: i32) -> Self {
        Self {
            slot,
            item_id: item_id.into(),
            count,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.count <= 0 || self.item_id.is_empty()
    }
}

/// `NewMinecartBehavior.MinecartStep` payload for `ClientboundMoveMinecartPacket`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinecartStep {
    pub position: Vec3,
    pub movement: Vec3,
    pub y_rot: f32,
    pub x_rot: f32,
    pub weight: f32,
}

/// Packets emitted by `ServerEntity` and `ChunkMap.TrackedEntity`.
#[derive(Debug, Clone, PartialEq)]
pub enum EntityTrackerPacket {
    /// `ClientboundBundlePacket`.
    Bundle(Vec<EntityTrackerPacket>),
    /// `ClientboundAddEntityPacket` (delivered by #90).
    AddEntity(ClientboundAddEntityPacket),
    /// `ClientboundSetEntityDataPacket` (delivered by #90).
    SetEntityData(ClientboundSetEntityDataPacket),
    /// `ClientboundUpdateAttributesPacket` (delivered by #90).
    UpdateAttributes(ClientboundUpdateAttributesPacket),
    /// `ClientboundSetEquipmentPacket` (Paper `sanitize` flag preserved).
    SetEquipment {
        entity_id: i32,
        slots: Vec<EquipmentEntry>,
        sanitize: bool,
    },
    /// `ClientboundSetPassengersPacket` (delivered by #90).
    SetPassengers(ClientboundSetPassengersPacket),
    /// `ClientboundSetEntityLinkPacket` (`dest_id == 0` when unlinked).
    SetEntityLink { source_id: i32, dest_id: i32 },
    /// `ClientboundRemoveEntitiesPacket` (delivered by #90).
    RemoveEntities(ClientboundRemoveEntitiesPacket),
    /// `ClientboundEntityPositionSyncPacket.of(entity)`.
    EntityPositionSync {
        entity_id: i32,
        position: Vec3,
        delta_movement: Vec3,
        y_rot: f32,
        x_rot: f32,
        on_ground: bool,
    },
    /// `ClientboundMoveEntityPacket.Pos`.
    MoveEntityPos {
        entity_id: i32,
        xa: i16,
        ya: i16,
        za: i16,
        on_ground: bool,
    },
    /// `ClientboundMoveEntityPacket.Rot`.
    MoveEntityRot {
        entity_id: i32,
        y_rot: i8,
        x_rot: i8,
        on_ground: bool,
    },
    /// `ClientboundMoveEntityPacket.PosRot`.
    MoveEntityPosRot {
        entity_id: i32,
        xa: i16,
        ya: i16,
        za: i16,
        y_rot: i8,
        x_rot: i8,
        on_ground: bool,
    },
    /// `ClientboundMoveMinecartPacket`.
    MoveMinecart {
        entity_id: i32,
        steps: Vec<MinecartStep>,
    },
    /// `ClientboundSetEntityMotionPacket`.
    SetEntityMotion { entity_id: i32, movement: Vec3 },
    /// `ClientboundProjectilePowerPacket`.
    ProjectilePower {
        entity_id: i32,
        acceleration_power: f64,
    },
    /// `ClientboundRotateHeadPacket`.
    RotateHead { entity_id: i32, y_head_rot: i8 },
    /// Map update packet emitted for an `ItemFrame` holding a `MapItem`.
    MapItemUpdate { map_id: i32, target_player_id: i32 },
}
