//! Native Entity/LivingEntity/Mob/AgeableMob/Animal state for generation and
//! supported persisted animal reconstruction. Constructor/finalize/load/save
//! execute native typed state; AI ticks, damage, nonempty equipment/effects,
//! and passenger riding remain explicit capability boundaries.
use super::farm_variants::{FarmFamily, FarmFamilyRegistry};
use rivet_nbt::{compound_tag::CompoundTag, list_tag::ListTag, tag::Tag};
use rivet_registry::core::{BlockPos, Vec3, uuid_to_int_array};
use rivet_registry::{
    Identifier, RegistryAccess, TagKey,
    biome_id::BiomeId,
    holder::{Holder, HolderId},
    registries::BIOME,
};
use rivet_util::{RandomSource, mth::Uuid};
use std::sync::Arc;
#[path = "generation_creature_load.rs"]
mod load_data;
pub use load_data::CreatureLoadError;

#[cfg(test)]
#[path = "generation_creature_fixture.rs"]
mod fixture;

#[derive(Debug, Clone, Copy)]
pub struct NativeEntityIdentity {
    pub entity_id: i32,
    pub uuid: Uuid,
    pub world_uuid: Uuid,
}
#[derive(Debug)]
pub struct NativeEntityState {
    identity: NativeEntityIdentity,
    position: Vec3,
    old_position: Vec3,
    rotation: [f32; 2],
    old_rotation: [f32; 2],
    motion: Vec3,
    fall_distance: f64,
    fire: i16,
    air: i32,
    on_ground: bool,
    invulnerable: bool,
    portal_cooldown: i32,
    age: i32,
    generation: bool,
    removed: bool,
    loaded: load_data::EntityLoadState,
    lifecycle: super::tracking::NativeLifecycleState,
}
impl NativeEntityState {
    pub fn new(identity: NativeEntityIdentity) -> Self {
        Self {
            identity,
            position: Vec3::new(0.0, 0.0, 0.0),
            old_position: Vec3::new(0.0, 0.0, 0.0),
            rotation: [0.0; 2],
            old_rotation: [0.0; 2],
            motion: Vec3::new(0.0, 0.0, 0.0),
            fall_distance: 0.0,
            fire: 0,
            air: 300,
            on_ground: false,
            invulnerable: false,
            portal_cooldown: 0,
            age: 0,
            generation: false,
            removed: false,
            loaded: Default::default(),
            lifecycle: Default::default(),
        }
    }
    pub fn identity(&self) -> NativeEntityIdentity {
        self.identity
    }
    pub fn set_uuid(&mut self, uuid: Uuid) {
        self.identity.uuid = uuid;
    }
    pub fn is_removed(&self) -> bool {
        self.removed
    }
    pub fn position(&self) -> Vec3 {
        self.position
    }
    pub fn block_position(&self) -> BlockPos {
        BlockPos::containing(self.position.x, self.position.y, self.position.z)
    }
    pub fn old_position(&self) -> Vec3 {
        self.old_position
    }
    pub fn rotation(&self) -> [f32; 2] {
        self.rotation
    }
    pub fn lifecycle(&self) -> &super::tracking::NativeLifecycleState {
        &self.lifecycle
    }
    pub fn mark_created(&mut self) {
        self.old_position = self.position;
        self.old_rotation = self.rotation;
    }
    pub fn mark_tracking_end(&mut self) {
        self.lifecycle.valid = false;
    }
    pub fn is_alive(&self) -> bool {
        !self.removed
    }
    pub fn generation(&self) -> bool {
        self.generation
    }
    pub fn set_generation(&mut self, value: bool) {
        self.generation = value;
    }
    pub fn discard(&mut self) {
        self.removed = true;
    }
    pub fn snap_to(&mut self, pos: Vec3, yaw: f32, pitch: f32) {
        if pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite() {
            self.position = pos;
        }
        if yaw.is_finite() {
            self.rotation[0] = yaw;
        }
        if pitch.is_finite() {
            self.rotation[1] = (pitch % 360.0).clamp(-90.0, 90.0);
        }
        self.old_position = self.position;
        self.old_rotation = self.rotation;
    }
    fn save(&self, id: &str) -> Option<CompoundTag> {
        if self.removed || !self.loaded.persist {
            return None;
        }
        let mut tag = CompoundTag::new();
        tag.put_string("id", id);
        tag.put(
            "Pos".into(),
            doubles([self.position.x, self.position.y, self.position.z]),
        );
        tag.put(
            "Motion".into(),
            doubles([self.motion.x, self.motion.y, self.motion.z]),
        );
        let mut rotation = ListTag::new();
        for value in self.rotation {
            rotation
                .list
                .push(Tag::Float(rivet_nbt::float_tag::FloatTag::value_of(value)));
        }
        tag.put("Rotation".into(), Tag::List(rotation));
        tag.put_double("fall_distance", self.fall_distance);
        tag.put_short("Fire", self.fire);
        tag.put_short("Air", self.air as i16);
        tag.put_boolean("OnGround", self.on_ground);
        tag.put_boolean("Invulnerable", self.invulnerable);
        tag.put_int("PortalCooldown", self.portal_cooldown);
        tag.put_int_array("UUID", uuid_to_int_array(self.identity.uuid).to_vec());
        tag.put_long("WorldUUIDMost", self.identity.world_uuid.most);
        tag.put_long("WorldUUIDLeast", self.identity.world_uuid.least);
        tag.put_int("Bukkit.updateLevel", 2);
        tag.put_int("Spigot.ticksLived", self.age);
        self.loaded.write(&mut tag);
        Some(tag)
    }
}
fn doubles(values: [f64; 3]) -> Tag {
    Tag::List(ListTag::with_list(
        values
            .into_iter()
            .map(|v| Tag::Double(rivet_nbt::double_tag::DoubleTag::value_of(v)))
            .collect(),
    ))
}

/// The fresh generic LivingEntity brain has no memory module or sensor types.
/// This is its real empty packed state, not a claim to implement Brain ticking.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeEmptyBrain;
#[derive(Debug)]
pub struct NativeLivingMob {
    pub entity: NativeEntityState,
    health: f32,
    max_health: f32,
    hurt_time: i16,
    death_time: i16,
    absorption: f32,
    impulse_grace: i32,
    brain: NativeEmptyBrain,
    follow_range: Option<FollowRange>,
    can_pick_up_loot: bool,
    persistence_required: bool,
    left_handed: bool,
    aware: bool,
    loaded: load_data::MobLoadState,
}
#[derive(Debug)]
struct FollowRange {
    base: f64,
    spawn_bonus: Option<f64>,
}
impl NativeLivingMob {
    fn new(
        identity: NativeEntityIdentity,
        max_health: f32,
        entity_random: &mut impl RandomSource,
    ) -> Self {
        let mut entity = NativeEntityState::new(identity);
        // LivingEntity constructor sets yaw, not snapTo: old rotation is still
        // the Entity constructor's zero rotation until actual placement.
        entity.rotation[0] = entity_random.next_float() * (std::f64::consts::PI * 2.0) as f32;
        Self {
            entity,
            health: max_health,
            max_health,
            hurt_time: 0,
            death_time: 0,
            absorption: 0.0,
            impulse_grace: 0,
            brain: NativeEmptyBrain,
            follow_range: None,
            can_pick_up_loot: false,
            persistence_required: false,
            left_handed: false,
            aware: true,
            loaded: Default::default(),
        }
    }
    fn finalize_spawn(&mut self, random: &mut impl RandomSource) {
        let attribute = self.follow_range.get_or_insert(FollowRange {
            base: 16.0,
            spawn_bonus: None,
        });
        if attribute.spawn_bonus.is_none() {
            attribute.spawn_bonus = Some(random.triangle_f64(0.0, 0.11485000000000001));
        }
        self.left_handed = random.next_float() < 0.05;
    }
    fn write(&self, tag: &mut CompoundTag) {
        tag.put_float("Health", self.health);
        tag.put_short("HurtTime", self.hurt_time);
        tag.put_short("DeathTime", self.death_time);
        tag.put_float("AbsorptionAmount", self.absorption);
        tag.put_int(
            "current_impulse_context_reset_grace_time",
            self.impulse_grace,
        );
        tag.put_boolean("FallFlying", self.loaded.fall_flying);
        let NativeEmptyBrain = self.brain;
        let mut brain = CompoundTag::new();
        brain.put("memories".into(), Tag::Compound(CompoundTag::new()));
        tag.put("Brain".into(), Tag::Compound(brain));
        let mut attributes = ListTag::new();
        if let Some(follow) = &self.follow_range {
            let mut attribute = CompoundTag::new();
            attribute.put_string("id", "minecraft:follow_range");
            attribute.put_double("base", follow.base);
            if let Some(amount) = follow.spawn_bonus {
                let mut modifier = CompoundTag::new();
                modifier.put_string("id", "minecraft:random_spawn_bonus");
                modifier.put_double("amount", amount);
                modifier.put_string("operation", "add_multiplied_base");
                attribute.put(
                    "modifiers".into(),
                    Tag::List(ListTag::with_list(vec![Tag::Compound(modifier)])),
                );
            }
            attributes.list.push(Tag::Compound(attribute));
        }
        tag.put("attributes".into(), Tag::List(attributes));
        tag.put_boolean("CanPickUpLoot", self.can_pick_up_loot);
        tag.put_boolean("PersistenceRequired", self.persistence_required);
        tag.put_boolean("LeftHanded", self.left_handed);
        tag.put_boolean("Bukkit.Aware", self.aware);
        self.loaded.write(tag);
    }
}

#[derive(Debug)]
pub struct AgeableSpawnGroup {
    group_size: i32,
    should_spawn_baby: bool,
    baby_chance: f32,
}
impl Default for AgeableSpawnGroup {
    fn default() -> Self {
        Self {
            group_size: 0,
            should_spawn_baby: true,
            baby_chance: 0.05,
        }
    }
}
impl AgeableSpawnGroup {
    pub fn group_size(&self) -> i32 {
        self.group_size
    }
}
#[derive(Debug)]
pub struct NativeAnimal {
    pub mob: NativeLivingMob,
    age: i32,
    forced_age: i32,
    age_locked: bool,
    in_love: i32,
    love_cause: Option<Uuid>,
}
impl NativeAnimal {
    fn new(identity: NativeEntityIdentity, health: f32, random: &mut impl RandomSource) -> Self {
        Self {
            mob: NativeLivingMob::new(identity, health, random),
            age: 0,
            forced_age: 0,
            age_locked: false,
            in_love: 0,
            love_cause: None,
        }
    }
    pub fn is_baby(&self) -> bool {
        self.age < 0
    }
    fn finalize_spawn(
        &mut self,
        random: &mut impl RandomSource,
        group: &mut Option<AgeableSpawnGroup>,
    ) {
        let group = group.get_or_insert_with(AgeableSpawnGroup::default);
        if group.should_spawn_baby
            && group.group_size > 0
            && random.next_float() <= group.baby_chance
        {
            self.age = -24000;
        }
        group.group_size = group.group_size.wrapping_add(1);
        self.mob.finalize_spawn(random);
    }
    fn write(&self, tag: &mut CompoundTag) {
        self.mob.write(tag);
        tag.put_int("Age", self.age);
        tag.put_int("ForcedAge", self.forced_age);
        tag.put_boolean("AgeLocked", self.age_locked);
        tag.put_int("InLove", self.in_love);
        if let Some(uuid) = self.love_cause {
            tag.put_int_array("LoveCause", uuid_to_int_array(uuid).to_vec());
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheepClimate {
    Warm,
    Cold,
    Temperate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheepColor {
    White,
    Black,
    Gray,
    LightGray,
    Brown,
    Pink,
    Orange,
    Magenta,
    LightBlue,
    Yellow,
    Lime,
    Cyan,
    Purple,
    Blue,
    Green,
    Red,
}
impl SheepColor {
    pub fn id(self) -> i8 {
        match self {
            Self::White => 0,
            Self::Black => 15,
            Self::Gray => 7,
            Self::LightGray => 8,
            Self::Brown => 12,
            Self::Pink => 6,
            Self::Orange => 1,
            Self::Magenta => 2,
            Self::LightBlue => 3,
            Self::Yellow => 4,
            Self::Lime => 5,
            Self::Cyan => 9,
            Self::Purple => 10,
            Self::Blue => 11,
            Self::Green => 13,
            Self::Red => 14,
        }
    }
}
#[derive(Debug)]
pub struct NativeSheep {
    pub animal: NativeAnimal,
    color: SheepColor,
    sheared: bool,
}
impl NativeSheep {
    pub fn from_identity(
        identity: NativeEntityIdentity,
        entity_random: &mut impl RandomSource,
    ) -> Self {
        Self {
            animal: NativeAnimal::new(identity, 8.0, entity_random),
            color: SheepColor::White,
            sheared: false,
        }
    }
    pub fn entity(&self) -> &NativeEntityState {
        &self.animal.mob.entity
    }
    pub fn entity_mut(&mut self) -> &mut NativeEntityState {
        &mut self.animal.mob.entity
    }
    pub fn snap_to(&mut self, pos: Vec3, yaw: f32, pitch: f32) {
        self.entity_mut().snap_to(pos, yaw, pitch);
    }
    pub fn is_baby(&self) -> bool {
        self.animal.is_baby()
    }
    pub fn color(&self) -> SheepColor {
        self.color
    }
    /// Animal.getWalkTargetValue: grass gives10, otherwise the caller supplies
    /// the genuine level's light-based pathfinding cost (never guessed).
    pub fn check_spawn_rules(&self, below_is_grass: bool, pathfinding_light_cost: f32) -> bool {
        if below_is_grass {
            true
        } else {
            pathfinding_light_cost >= 0.0
        }
    }
    pub fn finalize_spawn(
        &mut self,
        climate: SheepClimate,
        random: &mut impl RandomSource,
        group: &mut Option<AgeableSpawnGroup>,
    ) {
        use SheepColor::*;
        let colors = match climate {
            SheepClimate::Temperate => [Black, Gray, LightGray, Brown, White],
            SheepClimate::Warm => [Gray, LightGray, White, Black, Brown],
            SheepClimate::Cold => [LightGray, Gray, White, Brown, Black],
        };
        let draw = random.next_int_bound(100);
        let index = if draw < 5 {
            0
        } else if draw < 10 {
            1
        } else if draw < 15 {
            2
        } else if draw < 18 {
            3
        } else {
            4
        };
        self.color = if index == 4 && random.next_int_bound(500) >= 499 {
            Pink
        } else {
            colors[index]
        };
        self.animal.finalize_spawn(random, group);
    }
    pub fn bounding_box(&self) -> [f64; 6] {
        let scale = if self.is_baby() { 0.5f32 } else { 1.0 };
        let half = f64::from(0.9f32 * scale) / 2.0;
        let height = f64::from(1.3f32 * scale);
        let p = self.entity().position();
        [
            p.x - half,
            p.y,
            p.z - half,
            p.x + half,
            p.y + height,
            p.z + half,
        ]
    }
    pub fn save(&self) -> Option<CompoundTag> {
        let mut tag = self.entity().save("minecraft:sheep")?;
        self.animal.write(&mut tag);
        tag.put_boolean("Sheared", self.sheared);
        tag.put_byte("Color", self.color.id());
        Some(tag)
    }
}

/// Cow/Pig/Chicken share Animal's native storage and finalize chain. The
/// family fixes constructor health/dimensions; Chicken owns its real egg
/// timer and consumes its constructor draw after LivingEntity's yaw draw.
#[derive(Debug)]
pub struct NativeFarmAnimal {
    pub animal: NativeAnimal,
    registry: Arc<FarmFamilyRegistry>,
    variant: HolderId,
    sound_variant: HolderId,
    egg_time: Option<i32>,
    chicken_jockey: bool,
}
impl NativeFarmAnimal {
    pub fn from_identity(
        identity: NativeEntityIdentity,
        registry: Arc<FarmFamilyRegistry>,
        random: &mut impl RandomSource,
    ) -> Self {
        let health = match registry.family {
            FarmFamily::Cow | FarmFamily::Pig => 10.0,
            FarmFamily::Chicken => 4.0,
        };
        let animal = NativeAnimal::new(identity, health, random);
        let egg_time =
            (registry.family == FarmFamily::Chicken).then(|| random.next_int_bound(6000) + 6000);
        Self {
            animal,
            variant: registry.default_variant(),
            sound_variant: registry.default_sound(),
            registry,
            egg_time,
            chicken_jockey: false,
        }
    }
    pub fn family(&self) -> FarmFamily {
        self.registry.family
    }
    pub fn finalize_spawn(
        &mut self,
        biome: &Holder<BiomeId>,
        random: &mut impl RandomSource,
        group: &mut Option<AgeableSpawnGroup>,
    ) {
        if let Some(variant) = self.registry.pick_variant(biome, random) {
            self.variant = variant;
        }
        self.sound_variant = self.registry.pick_sound(random);
        self.animal.finalize_spawn(random, group);
    }
    pub fn save(&self) -> Option<CompoundTag> {
        let mut tag = self
            .animal
            .mob
            .entity
            .save(&format!("minecraft:{}", self.family().name()))?;
        self.animal.write(&mut tag);
        tag.put_string(
            "variant",
            &self.registry.variant_name(self.variant).to_string(),
        );
        tag.put_string(
            "sound_variant",
            &self.registry.sound_name(self.sound_variant).to_string(),
        );
        if let Some(egg_time) = self.egg_time {
            tag.put_boolean("IsChickenJockey", self.chicken_jockey);
            tag.put_int("EggLayTime", egg_time);
        }
        Some(tag)
    }
    pub fn bounding_box(&self) -> [f64; 6] {
        let (width, height) = match self.family() {
            FarmFamily::Cow => (0.9f32, 1.4f32),
            FarmFamily::Pig => (0.9, 0.9),
            FarmFamily::Chicken => (0.4, 0.7),
        };
        let (width, height) = if self.animal.is_baby() {
            match self.family() {
                FarmFamily::Chicken => (0.3, 0.4),
                _ => (width * 0.5, height * 0.5),
            }
        } else {
            (width, height)
        };
        let half = f64::from(width) / 2.0;
        let height = f64::from(height);
        let p = self.animal.mob.entity.position();
        [
            p.x - half,
            p.y,
            p.z - half,
            p.x + half,
            p.y + height,
            p.z + half,
        ]
    }
}

#[derive(Debug)]
pub enum NativeGenerationCreature {
    Sheep(NativeSheep),
    Farm(NativeFarmAnimal),
}
impl NativeGenerationCreature {
    pub fn tracking_snapshot(&self) -> super::tracking::EntityTrackingSnapshot {
        use super::tracking::{EntityTrackingSnapshot, MetadataItem as Item, MetadataValue as V};
        let animal = match self {
            Self::Sheep(e) => &e.animal,
            Self::Farm(e) => &e.animal,
        };
        let mut metadata = animal.tracking_metadata();
        match self {
            Self::Sheep(e) => metadata.push(Item::new(
                18,
                V::Byte(e.color.id() | (if e.sheared { 16 } else { 0 })),
                V::Byte(0),
            )),
            Self::Farm(e) => {
                let (id, appearance, sound) = match e.family() {
                    FarmFamily::Cow => (18, 23, 24),
                    FarmFamily::Pig => (19, 28, 29),
                    FarmFamily::Chicken => (18, 30, 31),
                };
                metadata.push(Item::new(
                    id,
                    V::Registry {
                        serializer: appearance,
                        id: e.variant.0,
                    },
                    V::Registry {
                        serializer: appearance,
                        id: e.registry.default_variant().0,
                    },
                ));
                metadata.push(Item::new(
                    id + 1,
                    V::Registry {
                        serializer: sound,
                        id: e.sound_variant.0,
                    },
                    V::Registry {
                        serializer: sound,
                        id: e.registry.default_sound().0,
                    },
                ));
            }
        }
        let e = &animal.mob.entity;
        EntityTrackingSnapshot {
            position: e.position,
            movement: e.motion,
            rotation: e.rotation,
            head_rotation: e.rotation[0],
            on_ground: e.on_ground,
            metadata,
            experimental_minecart: false,
            living: true,
        }
    }
    pub fn entity(&self) -> &NativeEntityState {
        match self {
            Self::Sheep(value) => value.entity(),
            Self::Farm(value) => &value.animal.mob.entity,
        }
    }
    pub fn entity_mut(&mut self) -> &mut NativeEntityState {
        match self {
            Self::Sheep(value) => value.entity_mut(),
            Self::Farm(value) => &mut value.animal.mob.entity,
        }
    }
    pub fn snap_to(&mut self, pos: Vec3, yaw: f32, pitch: f32) {
        match self {
            Self::Sheep(value) => value.snap_to(pos, yaw, pitch),
            Self::Farm(value) => value.animal.mob.entity.snap_to(pos, yaw, pitch),
        }
    }
    pub fn bounding_box(&self) -> [f64; 6] {
        match self {
            Self::Sheep(value) => value.bounding_box(),
            Self::Farm(value) => value.bounding_box(),
        }
    }
    pub fn save(&self) -> Option<CompoundTag> {
        match self {
            Self::Sheep(value) => value.save(),
            Self::Farm(value) => value.save(),
        }
    }
    pub fn check_spawn_rules(&self, below_is_grass: bool, pathfinding_light_cost: f32) -> bool {
        below_is_grass || pathfinding_light_cost >= 0.0
    }
    pub fn finalize_spawn(
        &mut self,
        biome: &Holder<BiomeId>,
        access: &RegistryAccess,
        random: &mut impl RandomSource,
        group: &mut Option<AgeableSpawnGroup>,
    ) {
        match self {
            Self::Farm(value) => value.finalize_spawn(biome, random, group),
            Self::Sheep(value) => {
                let registry = access.lookup_or_throw(&BIOME);
                let tag = |name| TagKey::create(&BIOME, Identifier::with_default_namespace(name));
                let climate = if biome.is_tag(registry, &tag("spawns_warm_variant_farm_animals")) {
                    SheepClimate::Warm
                } else if biome.is_tag(registry, &tag("spawns_cold_variant_farm_animals")) {
                    SheepClimate::Cold
                } else {
                    SheepClimate::Temperate
                };
                value.finalize_spawn(climate, random, group);
            }
        }
    }
}
