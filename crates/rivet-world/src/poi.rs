//! Pinned builtin PoiTypes and native PoiRecord ticket state. The section
//! section index is in [`PoiSection`]; disk manager, village-distance
//! propagation and FULL integration are separate layers.
use rivet_registry::{
    Holder, Identifier, RegistrationInfo, RegistryBuilder, ResourceKey,
    block_state::BlockState,
    core::{BlockPos, block_pos_codec},
    generated::{
        block_properties::BlockPropertyId,
        block_states::{BLOCK_STATE_COUNT, StateId},
    },
    holder_lookup::HolderLookup,
    registry::Registry,
    registry_file_codec::RegistryFixedCodec,
    registry_ops::RegistryOpsLookup,
};
use rivet_serialization::{
    codec::{self, Codec},
    dynamic_ops::DynamicOps,
    record_builder::{self, RecordCodecBuilder},
};
use std::{
    hash::{Hash, Hasher},
    sync::{
        Arc, LazyLock,
        atomic::{AtomicI32, Ordering},
    },
};

mod poi_section;
pub use poi_section::{PackedPoiSection, PoiOccupancy, PoiSection, PoiSectionError};
mod distance;
pub use distance::{Delayed26WayDistancePropagator3D, DistancePropagationError};

/// Canonical identity in the complete pinned builtin POI registry. Private
/// representation prevents an unregistered type from acquiring ticket state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoiType(u8);
pub static POINT_OF_INTEREST_TYPE: LazyLock<ResourceKey<Registry<PoiType>>> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace("point_of_interest_type"))
});

/// Actual builtin registration order and values. Each invocation creates its
/// own registry authority; holders from separate registries cannot be mixed.
pub fn bootstrap_poi_types() -> Registry<PoiType> {
    let mut registry = RegistryBuilder::new(&POINT_OF_INTEREST_TYPE);
    for kind in PoiType::all() {
        registry.register(
            &ResourceKey::create(&POINT_OF_INTEREST_TYPE, Identifier::parse(kind.name())),
            Arc::new(kind),
            RegistrationInfo::BUILT_IN,
        );
    }
    registry.freeze()
}
const TYPES: [(&str, i32, i32); 21] = [
    ("minecraft:armorer", 1, 1),
    ("minecraft:butcher", 1, 1),
    ("minecraft:cartographer", 1, 1),
    ("minecraft:cleric", 1, 1),
    ("minecraft:farmer", 1, 1),
    ("minecraft:fisherman", 1, 1),
    ("minecraft:fletcher", 1, 1),
    ("minecraft:leatherworker", 1, 1),
    ("minecraft:librarian", 1, 1),
    ("minecraft:mason", 1, 1),
    ("minecraft:shepherd", 1, 1),
    ("minecraft:toolsmith", 1, 1),
    ("minecraft:weaponsmith", 1, 1),
    ("minecraft:home", 1, 1),
    ("minecraft:meeting", 32, 6),
    ("minecraft:beehive", 0, 1),
    ("minecraft:bee_nest", 0, 1),
    ("minecraft:nether_portal", 0, 1),
    ("minecraft:lodestone", 0, 1),
    ("minecraft:test_instance", 0, 1),
    ("minecraft:lightning_rod", 0, 1),
];
static MATCHING_STATES: LazyLock<Vec<Vec<StateId>>> = LazyLock::new(|| {
    let mut states = vec![Vec::new(); TYPES.len()];
    for id in 0..BLOCK_STATE_COUNT {
        let state = BlockState::new(StateId(id));
        if let Some(kind) = PoiType::for_state(state) {
            states[kind.0 as usize].push(state.id());
        }
    }
    states
});
impl PoiType {
    pub fn all() -> impl Iterator<Item = Self> {
        (0..TYPES.len() as u8).map(Self)
    }
    pub fn name(self) -> &'static str {
        TYPES[self.0 as usize].0
    }
    pub fn max_tickets(self) -> i32 {
        TYPES[self.0 as usize].1
    }
    pub fn valid_range(self) -> i32 {
        TYPES[self.0 as usize].2
    }
    pub fn matching_states(self) -> &'static [StateId] {
        &MATCHING_STATES[self.0 as usize]
    }
    pub fn by_name(name: &Identifier) -> Option<Self> {
        let name = name.to_string();
        TYPES
            .iter()
            .position(|(key, _, _)| *key == name)
            .map(|i| Self(i as u8))
    }
    /// PoiTypes.bootstrap's explicit block families. Only bed HEAD states are
    /// HOME; the four cauldrons and all eight rod oxidization/wax states join
    /// their source family. No arbitrary name suffix is treated as a POI.
    pub fn for_state(state: BlockState) -> Option<Self> {
        let index = match state.block().name() {
            "minecraft:blast_furnace" => 0,
            "minecraft:smoker" => 1,
            "minecraft:cartography_table" => 2,
            "minecraft:brewing_stand" => 3,
            "minecraft:composter" => 4,
            "minecraft:barrel" => 5,
            "minecraft:fletching_table" => 6,
            "minecraft:cauldron"
            | "minecraft:lava_cauldron"
            | "minecraft:water_cauldron"
            | "minecraft:powder_snow_cauldron" => 7,
            "minecraft:lectern" => 8,
            "minecraft:stonecutter" => 9,
            "minecraft:loom" => 10,
            "minecraft:smithing_table" => 11,
            "minecraft:grindstone" => 12,
            "minecraft:white_bed"
            | "minecraft:orange_bed"
            | "minecraft:magenta_bed"
            | "minecraft:light_blue_bed"
            | "minecraft:yellow_bed"
            | "minecraft:lime_bed"
            | "minecraft:pink_bed"
            | "minecraft:gray_bed"
            | "minecraft:light_gray_bed"
            | "minecraft:cyan_bed"
            | "minecraft:purple_bed"
            | "minecraft:blue_bed"
            | "minecraft:brown_bed"
            | "minecraft:green_bed"
            | "minecraft:red_bed"
            | "minecraft:black_bed" => {
                let part = state
                    .get_property(BlockPropertyId::Part)
                    .expect("registered bed part");
                if BlockPropertyId::Part.values()[part as usize] != "head" {
                    return None;
                }
                13
            }
            "minecraft:bell" => 14,
            "minecraft:beehive" => 15,
            "minecraft:bee_nest" => 16,
            "minecraft:nether_portal" => 17,
            "minecraft:lodestone" => 18,
            "minecraft:test_instance_block" => 19,
            "minecraft:lightning_rod"
            | "minecraft:exposed_lightning_rod"
            | "minecraft:weathered_lightning_rod"
            | "minecraft:oxidized_lightning_rod"
            | "minecraft:waxed_lightning_rod"
            | "minecraft:waxed_exposed_lightning_rod"
            | "minecraft:waxed_weathered_lightning_rod"
            | "minecraft:waxed_oxidized_lightning_rod" => 20,
            _ => return None,
        };
        Some(Self(index))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedPoiRecord {
    pub pos: BlockPos,
    pub poi_type: Holder<PoiType>,
    pub free_tickets: i32,
}
impl PackedPoiRecord {
    pub fn codec<Ops: DynamicOps + RegistryOpsLookup + 'static>() -> Arc<dyn Codec<Self, Ops>> {
        record_builder::create(|instance| {
            instance
                .group(RecordCodecBuilder::of_named(
                    Arc::new(|r: &Self| r.pos),
                    "pos".into(),
                    block_pos_codec::<Ops>(),
                ))
                .and(RecordCodecBuilder::of_named(
                    Arc::new(|r: &Self| r.poi_type.clone()),
                    "type".into(),
                    Arc::new(RegistryFixedCodec::create(&POINT_OF_INTEREST_TYPE)),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|r: &Self| r.free_tickets),
                    codec::optional_field_of("free_tickets", codec::int_codec::<Ops>(), 0),
                ))
                .apply(
                    instance,
                    Arc::new(|pos, poi_type, free_tickets| Self {
                        pos,
                        poi_type,
                        free_tickets,
                    }),
                )
        })
    }
    /// Stored free tickets are not clamped; source accepts negative or above-
    /// maximum values and its subsequent operations preserve their semantics.
    pub fn unpack(
        self,
        lookup: &dyn HolderLookup<PoiType>,
        set_dirty: Arc<dyn Fn() + Send + Sync>,
    ) -> PoiRecord {
        let resolved_type = *self.poi_type.value(lookup);
        PoiRecord {
            pos: self.pos,
            poi_type: self.poi_type,
            free_tickets: AtomicI32::new(self.free_tickets),
            resolved_type,
            set_dirty,
        }
    }
}
/// A shared POI object, mutated by its canonical tick-thread owner. The ticket
/// cell permits both section indices to retain the same record without a lock
/// held across setDirty (which may inspect the updated occupancy). Atomics make
/// the owning world movable; they do not introduce concurrent ticket semantics.
pub struct PoiRecord {
    pos: BlockPos,
    poi_type: Holder<PoiType>,
    free_tickets: AtomicI32,
    resolved_type: PoiType,
    set_dirty: Arc<dyn Fn() + Send + Sync>,
}
impl PoiRecord {
    pub fn new(
        pos: BlockPos,
        poi_type: Holder<PoiType>,
        lookup: &dyn HolderLookup<PoiType>,
        set_dirty: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let free_tickets = poi_type.value(lookup).max_tickets();
        PackedPoiRecord {
            pos,
            poi_type,
            free_tickets,
        }
        .unpack(lookup, set_dirty)
    }
    pub fn pack(&self) -> PackedPoiRecord {
        PackedPoiRecord {
            pos: self.pos,
            poi_type: self.poi_type.clone(),
            free_tickets: self.free_tickets(),
        }
    }
    pub fn pos(&self) -> BlockPos {
        self.pos
    }
    pub fn poi_type(&self) -> &Holder<PoiType> {
        &self.poi_type
    }
    pub fn free_tickets(&self) -> i32 {
        self.free_tickets.load(Ordering::Relaxed)
    }
    pub fn has_space(&self) -> bool {
        self.free_tickets() > 0
    }
    pub fn is_occupied(&self) -> bool {
        self.free_tickets() != self.resolved_type.max_tickets()
    }
    pub fn acquire_ticket(&self) -> bool {
        if self.free_tickets() <= 0 {
            return false;
        }
        self.free_tickets
            .store(self.free_tickets().wrapping_sub(1), Ordering::Relaxed);
        (self.set_dirty)();
        true
    }
    pub fn release_ticket(&self) -> bool {
        if self.free_tickets() >= self.resolved_type.max_tickets() {
            return false;
        }
        self.free_tickets
            .store(self.free_tickets().wrapping_add(1), Ordering::Relaxed);
        (self.set_dirty)();
        true
    }
}
impl PartialEq for PoiRecord {
    fn eq(&self, other: &Self) -> bool {
        self.pos() == other.pos()
    }
}
impl Eq for PoiRecord {}
impl Hash for PoiRecord {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pos().hash(state);
    }
}

#[cfg(test)]
#[path = "poi_fixture.rs"]
mod fixture;
