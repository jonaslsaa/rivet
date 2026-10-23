//! Actual persisted Entity→LivingEntity→Mob→AgeableMob→Animal load chain.
//! Runtime-heavy equipment/effects/passengers remain explicit refusals, never
//! opaque entity tags retained as apparent native loading success.
use super::*;
use rivet_nbt::{nbt_ops::NbtOps, tag::Tag};
use rivet_registry::core::uuid_codec;
use rivet_serialization::dynamic_ops::DynamicOps;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CreatureLoadError {
    #[error("native creature loading unavailable: {0}")]
    Unsupported(String),
    #[error("invalid native creature: {0}")]
    Invalid(String),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum TriState {
    #[default]
    NotSet,
    True,
    False,
}
impl TriState {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "NOT_SET" => Some(Self::NotSet),
            "TRUE" => Some(Self::True),
            "FALSE" => Some(Self::False),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::NotSet => "NOT_SET",
            Self::True => "TRUE",
            Self::False => "FALSE",
        }
    }
}
#[derive(Debug)]
pub(super) struct EntityLoadState {
    pub persist: bool,
    visible: bool,
    max_air: i32,
    invisible: bool,
    name_visible: bool,
    silent: bool,
    no_gravity: bool,
    glowing: bool,
    frozen: i32,
    visual_fire: TriState,
    custom_data: CompoundTag,
    bukkit_values: CompoundTag,
    origin: Option<Vec3>,
    origin_world: Option<Uuid>,
    from_spawner: bool,
    from_portal: bool,
    spawn_reason: Option<String>,
    freeze_lock: bool,
}
impl Default for EntityLoadState {
    fn default() -> Self {
        Self {
            persist: true,
            visible: true,
            max_air: 300,
            invisible: false,
            name_visible: false,
            silent: false,
            no_gravity: false,
            glowing: false,
            frozen: 0,
            visual_fire: TriState::NotSet,
            custom_data: CompoundTag::new(),
            bukkit_values: CompoundTag::new(),
            origin: None,
            origin_world: None,
            from_spawner: false,
            from_portal: false,
            spawn_reason: None,
            freeze_lock: false,
        }
    }
}
impl EntityLoadState {
    pub fn write(&self, tag: &mut CompoundTag) {
        for (key, value) in [
            ("CustomNameVisible", self.name_visible),
            ("Silent", self.silent),
            ("NoGravity", self.no_gravity),
            ("Glowing", self.glowing),
            ("Bukkit.invisible", self.invisible),
            ("Paper.FromMobSpawner", self.from_spawner),
            ("Paper.FromNetherPortal", self.from_portal),
            ("Paper.FreezeLock", self.freeze_lock),
        ] {
            if value {
                tag.put_boolean(key, true);
            }
        }
        if !self.visible {
            tag.put_boolean("Bukkit.visibleByDefault", false);
        }
        if self.max_air != 300 {
            tag.put_int("Bukkit.MaxAirSupply", self.max_air);
        }
        if self.frozen > 0 {
            tag.put_int("TicksFrozen", self.frozen);
        }
        if self.visual_fire == TriState::True {
            tag.put_boolean("HasVisualFire", true);
        }
        if self.visual_fire != TriState::NotSet {
            tag.put_string("Paper.FireOverride", self.visual_fire.name());
        }
        for (key, data) in [
            ("data", &self.custom_data),
            ("BukkitValues", &self.bukkit_values),
        ] {
            if !data.tags.is_empty() {
                tag.put(key.into(), Tag::Compound(data.clone()));
            }
        }
        if let Some(pos) = self.origin {
            tag.put("Paper.Origin".into(), doubles([pos.x, pos.y, pos.z]));
            if let Some(uuid) = self.origin_world {
                tag.put_int_array("Paper.OriginWorld", uuid_to_int_array(uuid).to_vec());
            }
        }
        if let Some(reason) = &self.spawn_reason {
            tag.put_string("Paper.SpawnReason", reason);
        }
    }
}
#[derive(Debug, Default)]
pub(super) struct MobLoadState {
    pub fall_flying: bool,
    no_ai: bool,
    friction: TriState,
    death_loot: Option<Identifier>,
    death_seed: i64,
}
impl MobLoadState {
    pub fn write(&self, tag: &mut CompoundTag) {
        if self.no_ai {
            tag.put_boolean("NoAI", true);
        }
        if self.friction != TriState::NotSet {
            tag.put_string("Paper.FrictionState", self.friction.name());
        }
        if let Some(key) = &self.death_loot {
            tag.put_string("DeathLootTable", &key.to_string());
        }
        if self.death_seed != 0 {
            tag.put_long("DeathLootTableSeed", self.death_seed);
        }
    }
}

fn vector<const N: usize>(input: &CompoundTag, key: &str) -> Option<[f64; N]> {
    let values = NbtOps
        .get_stream(input.get(key)?)
        .result_or_partial_silent()?;
    let values: Vec<_> = values
        .iter()
        .filter_map(|v| v.as_number().map(|n| n.double_value()))
        .collect();
    (values.len() >= N).then(|| std::array::from_fn(|i| values[i]))
}
fn uuid(input: &CompoundTag, key: &str) -> Option<Uuid> {
    uuid_codec::<NbtOps>()
        .parse(&NbtOps, input.get(key)?)
        .result_or_partial_silent()
}
fn unsupported(message: &str) -> CreatureLoadError {
    CreatureLoadError::Unsupported(message.into())
}

impl NativeEntityState {
    fn load_entity(&mut self, input: &CompoundTag) -> Result<(), CreatureLoadError> {
        for key in ["Tags"] {
            if input.get_list(key).is_some_and(|v| !v.list.is_empty()) {
                return Err(unsupported(&format!(
                    "nonempty {key} requires native runtime"
                )));
            }
        }
        if input.get("CustomName").is_some() {
            return Err(unsupported("CustomName component codec"));
        }
        let pos = vector::<3>(input, "Pos").unwrap_or([0.; 3]);
        let pos = Vec3::new(
            pos[0].clamp(-3.0000512E7, 3.0000512E7),
            pos[1].clamp(-2.0E7, 2.0E7),
            pos[2].clamp(-3.0000512E7, 3.0000512E7),
        );
        if pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite() {
            self.position = pos;
        }
        let motion = vector::<3>(input, "Motion")
            .unwrap_or([0.; 3])
            .map(|v| if v.abs() > 10. { 0. } else { v });
        if motion.iter().all(|v| v.is_finite()) {
            self.motion = Vec3::new(motion[0], motion[1], motion[2]);
        }
        let rotation = vector::<2>(input, "Rotation")
            .unwrap_or([0.; 2])
            .map(|v| v as f32);
        if rotation[0].is_finite() {
            self.rotation[0] = rotation[0];
        }
        if rotation[1].is_finite() {
            self.rotation[1] = (rotation[1] % 360.).clamp(-90., 90.);
        }
        self.old_position = self.position;
        self.old_rotation = self.rotation;
        self.rotation[0] %= 360.;
        self.fall_distance = input.get_double_or("fall_distance", 0.);
        self.fire = input.get_short_or("Fire", 0);
        self.air = input.get_int_or("Air", self.loaded.max_air);
        self.on_ground = input.get_boolean_or("OnGround", false);
        self.invulnerable = input.get_boolean_or("Invulnerable", false);
        self.portal_cooldown = input.get_int_or("PortalCooldown", 0);
        if let Some(uuid) = uuid(input, "UUID") {
            self.identity.uuid = uuid;
        }
        let mut loaded = EntityLoadState {
            name_visible: input.get_boolean_or("CustomNameVisible", false),
            silent: input.get_boolean_or("Silent", false),
            no_gravity: input.get_boolean_or("NoGravity", false),
            glowing: input.get_boolean_or("Glowing", false),
            frozen: input.get_int_or("TicksFrozen", 0),
            ..EntityLoadState::default()
        };
        loaded.visual_fire = if let Some(s) = input.get_string("Paper.FireOverride") {
            TriState::parse(s).unwrap_or_default()
        } else {
            input
                .get_boolean("HasVisualFire")
                .map(|v| if v { TriState::True } else { TriState::False })
                .unwrap_or_default()
        };
        loaded.custom_data = input.get_compound("data").cloned().unwrap_or_default();
        loaded.bukkit_values = input
            .get_compound("BukkitValues")
            .cloned()
            .unwrap_or_default();
        self.age = input.get_int_or("Spigot.ticksLived", 0);
        loaded.persist = input.get_boolean_or("Bukkit.persist", true);
        loaded.visible = input.get_boolean_or("Bukkit.visibleByDefault", true);
        loaded.max_air = input.get_int_or("Bukkit.MaxAirSupply", self.loaded.max_air);
        loaded.invisible = input.get_boolean_or("Bukkit.invisible", false);
        loaded.origin = vector::<3>(input, "Paper.Origin").map(|v| Vec3::new(v[0], v[1], v[2]));
        if loaded.origin.is_some() {
            loaded.origin_world =
                Some(uuid(input, "Paper.OriginWorld").unwrap_or(self.identity.world_uuid));
        }
        loaded.from_spawner = input.get_boolean_or("Paper.FromMobSpawner", false);
        loaded.from_portal = input.get_boolean_or("Paper.FromNetherPortal", false);
        loaded.freeze_lock = input.get_boolean_or("Paper.FreezeLock", false);
        let fallback = if loaded.from_spawner {
            "SPAWNER"
        } else if !input.get_boolean_or("PersistenceRequired", false) {
            "NATURAL"
        } else {
            "DEFAULT"
        };
        // Accept the real Bukkit enum names; malformed strings retain source
        // natural/default inference, rather than saving an arbitrary string.
        loaded.spawn_reason = Some(
            input
                .get_string("Paper.SpawnReason")
                .map(String::as_str)
                .filter(|s| spawn_reason(s))
                .unwrap_or(fallback)
                .to_owned(),
        );
        self.loaded = loaded;
        Ok(())
    }
}
fn spawn_reason(s: &str) -> bool {
    [
        "NATURAL",
        "JOCKEY",
        "CHUNK_GEN",
        "SPAWNER",
        "TRIAL_SPAWNER",
        "EGG",
        "SPAWNER_EGG",
        "LIGHTNING",
        "BUILD_SNOWMAN",
        "BUILD_IRONGOLEM",
        "BUILD_COPPERGOLEM",
        "BUILD_WITHER",
        "VILLAGE_DEFENSE",
        "VILLAGE_INVASION",
        "BREEDING",
        "SLIME_SPLIT",
        "REINFORCEMENTS",
        "NETHER_PORTAL",
        "DISPENSE_EGG",
        "INFECTION",
        "CURED",
        "OCELOT_BABY",
        "SILVERFISH_BLOCK",
        "MOUNT",
        "TRAP",
        "ENDER_PEARL",
        "SHOULDER_ENTITY",
        "DROWNED",
        "SHEARED",
        "EXPLOSION",
        "RAID",
        "PATROL",
        "BEEHIVE",
        "PIGLIN_ZOMBIFIED",
        "SPELL",
        "FROZEN",
        "METAMORPHOSIS",
        "DUPLICATION",
        "COMMAND",
        "ENCHANTMENT",
        "OMINOUS_ITEM_SPAWNER",
        "BUCKET",
        "POTION_EFFECT",
        "REANIMATE",
        "REHYDRATION",
        "CUSTOM",
        "DEFAULT",
    ]
    .contains(&s)
}

impl NativeAnimal {
    pub(super) fn tracking_metadata(&self) -> Vec<super::super::tracking::MetadataItem> {
        use super::super::tracking::{MetadataItem as Item, MetadataValue as V};
        let e = &self.mob.entity;
        let s = &e.loaded;
        let flags = (if s.invisible { 32 } else { 0 })
            | (if s.glowing { 64 } else { 0 })
            | (if self.mob.loaded.fall_flying { -128 } else { 0 });
        vec![
            Item::new(0, V::Byte(flags), V::Byte(0)),
            Item::new(1, V::Int(e.air), V::Int(300)),
            Item::new(3, V::Boolean(s.name_visible), V::Boolean(false)),
            Item::new(4, V::Boolean(s.silent), V::Boolean(false)),
            Item::new(5, V::Boolean(s.no_gravity), V::Boolean(false)),
            Item::new(7, V::Int(s.frozen), V::Int(0)),
            Item::new(9, V::Float(self.mob.health), V::Float(1.)),
            Item::new(
                15,
                V::Byte(
                    (if self.mob.loaded.no_ai { 1 } else { 0 })
                        | (if self.mob.left_handed { 2 } else { 0 }),
                ),
                V::Byte(0),
            ),
            Item::new(16, V::Boolean(self.is_baby()), V::Boolean(false)),
        ]
    }
    fn load_animal(&mut self, input: &CompoundTag) -> Result<(), CreatureLoadError> {
        self.mob.entity.load_entity(input)?;
        for key in ["active_effects"] {
            if input.get_list(key).is_some_and(|v| !v.list.is_empty()) {
                return Err(unsupported(key));
            }
        }
        for key in ["equipment", "drop_chances"] {
            if input.get_compound(key).is_some_and(|v| !v.tags.is_empty()) {
                return Err(unsupported(key));
            }
        }
        if input
            .get_compound("Brain")
            .and_then(|v| v.get_compound("memories"))
            .is_some_and(|v| !v.tags.is_empty())
        {
            return Err(unsupported("nonempty Brain memories"));
        }
        for key in [
            "sleeping_pos",
            "leash",
            "home_pos",
            "last_hurt_by_player",
            "last_hurt_by_mob",
            "current_explosion_impact_pos",
            "locator_bar_icon",
            "Bukkit.MaxHealth",
        ] {
            if input.get(key).is_some() {
                return Err(unsupported(key));
            }
        }
        if input.get_int_or("home_radius", -1) >= 0 {
            return Err(unsupported("native home radius"));
        }
        if let Some(attributes) = input.get_list("attributes") {
            for tag in &attributes.list {
                let Some(attribute) = tag.as_compound() else {
                    continue;
                };
                let Some(id) = attribute
                    .get_string("id")
                    .and_then(|id| Identifier::try_parse(id))
                else {
                    continue;
                };
                if id != Identifier::with_default_namespace("follow_range") {
                    return Err(unsupported("non-follow-range native attribute"));
                }
                let mut follow = FollowRange {
                    base: attribute.get_double_or("base", 0.),
                    spawn_bonus: self
                        .mob
                        .follow_range
                        .as_ref()
                        .and_then(|value| value.spawn_bonus),
                };
                if let Some(modifiers) = attribute.get_list("modifiers") {
                    for modifier in &modifiers.list {
                        let Some(modifier) = modifier.as_compound() else {
                            continue;
                        };
                        let Some(id) = modifier
                            .get_string("id")
                            .and_then(|id| Identifier::try_parse(id))
                        else {
                            continue;
                        };
                        let Some(amount) = modifier.get_double("amount") else {
                            continue;
                        };
                        let Some(operation) = modifier.get_string("operation") else {
                            continue;
                        };
                        if id != Identifier::with_default_namespace("random_spawn_bonus")
                            || operation != "add_multiplied_base"
                        {
                            return Err(unsupported("native attribute modifier"));
                        }
                        follow.spawn_bonus = Some(amount);
                    }
                }
                self.mob.follow_range = Some(follow);
            }
        }
        let absorption = input.get_float_or("AbsorptionAmount", 0.);
        self.mob.absorption = if absorption.is_nan() { 0. } else { absorption };
        let health = input.get_float_or("Health", self.mob.max_health);
        self.mob.health = if health.is_nan() {
            self.mob.max_health
        } else {
            health.clamp(0., self.mob.max_health)
        };
        self.mob.hurt_time = input.get_short_or("HurtTime", 0);
        self.mob.death_time = input.get_short_or("DeathTime", 0);
        self.mob.impulse_grace = input.get_int_or("current_impulse_context_reset_grace_time", 0);
        self.mob.can_pick_up_loot = input.get_boolean_or("CanPickUpLoot", false);
        self.mob.persistence_required = input.get_boolean_or("PersistenceRequired", false);
        self.mob.left_handed = input.get_boolean_or("LeftHanded", false);
        self.mob.aware = input.get_boolean_or("Bukkit.Aware", true);
        self.mob.loaded = MobLoadState {
            fall_flying: input.get_boolean_or("FallFlying", false),
            no_ai: input.get_boolean_or("NoAI", false),
            friction: input
                .get_string("Paper.FrictionState")
                .and_then(|value| TriState::parse(value))
                .unwrap_or_default(),
            death_loot: input
                .get_string("DeathLootTable")
                .and_then(|id| Identifier::try_parse(id)),
            death_seed: input.get_long_or("DeathLootTableSeed", 0),
        };
        self.age = input.get_int_or("Age", 0);
        self.forced_age = input.get_int_or("ForcedAge", 0);
        self.age_locked = input.get_boolean_or("AgeLocked", false);
        self.in_love = input.get_int_or("InLove", 0);
        self.love_cause = uuid(input, "LoveCause");
        Ok(())
    }
}

impl super::NativeEntityState {
    pub fn begin_tracking(&mut self) {
        self.lifecycle.in_world = true;
        self.lifecycle.valid = true;
    }
    pub fn initialize_tracking_origin(&mut self) {
        self.loaded.origin.get_or_insert(self.position);
        self.loaded
            .origin_world
            .get_or_insert(self.identity.world_uuid);
    }
}
impl NativeGenerationCreature {
    pub fn load(&mut self, input: &CompoundTag) -> Result<(), CreatureLoadError> {
        self.load_root(input)?;
        if input
            .get_list("Passengers")
            .is_some_and(|list| !list.list.is_empty())
        {
            return Err(unsupported("nonempty Passengers requires native runtime"));
        }
        Ok(())
    }
    /// Actual root Entity.load chain only. EntityType.loadEntityRecursive
    /// performs its postLoad callback/limits BEFORE passenger recursion.
    pub fn load_root(&mut self, input: &CompoundTag) -> Result<(), CreatureLoadError> {
        match self {
            Self::Sheep(sheep) => {
                sheep.animal.load_animal(input)?;
                sheep.sheared = input.get_boolean_or("Sheared", false);
                // DyeColor.LEGACY_ID_CODEC is Codec.BYTE over the boxed
                // Number, not NumericTag.byteValue (which floors floats).
                let id = input
                    .get("Color")
                    .and_then(|value| {
                        rivet_serialization::codec::byte_codec::<NbtOps>()
                            .parse(&NbtOps, value)
                            .result_or_partial_silent()
                    })
                    .unwrap_or(0);
                sheep.color = match id {
                    1 => SheepColor::Orange,
                    2 => SheepColor::Magenta,
                    3 => SheepColor::LightBlue,
                    4 => SheepColor::Yellow,
                    5 => SheepColor::Lime,
                    6 => SheepColor::Pink,
                    7 => SheepColor::Gray,
                    8 => SheepColor::LightGray,
                    9 => SheepColor::Cyan,
                    10 => SheepColor::Purple,
                    11 => SheepColor::Blue,
                    12 => SheepColor::Brown,
                    13 => SheepColor::Green,
                    14 => SheepColor::Red,
                    15 => SheepColor::Black,
                    _ => SheepColor::White,
                };
            }
            Self::Farm(animal) => {
                animal.animal.load_animal(input)?;
                if let Some(key) = input
                    .get_string("variant")
                    .and_then(|id| Identifier::try_parse(id))
                    && let Some(id) = animal.registry.variant_by_name(&key)
                {
                    animal.variant = id;
                }
                if let Some(key) = input
                    .get_string("sound_variant")
                    .and_then(|id| Identifier::try_parse(id))
                    && let Some(id) = animal.registry.sound_by_name(&key)
                {
                    animal.sound_variant = id;
                }
                if animal.family() == FarmFamily::Chicken {
                    animal.chicken_jockey = input.get_boolean_or("IsChickenJockey", false);
                    if let Some(time) = input.get_int("EggLayTime") {
                        animal.egg_time = Some(time);
                    }
                }
            }
        }
        Ok(())
    }
    pub fn is_alive(&self) -> bool {
        let mob = match self {
            Self::Sheep(s) => &s.animal.mob,
            Self::Farm(a) => &a.animal.mob,
        };
        mob.entity.is_alive() && mob.health > 0.
    }
}
