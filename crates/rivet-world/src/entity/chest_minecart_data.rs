//! Typed Entity/AbstractMinecart persisted state. Compound fields below are
//! actual CustomData/PersistentDataContainer payloads, not opaque entity tags.
use super::{NativeChestMinecart, doubles};
use rivet_nbt::{
    compound_tag::CompoundTag, float_tag::FloatTag, list_tag::ListTag, nbt_ops::NbtOps, tag::Tag,
};
use rivet_registry::{
    block_state::BlockState,
    block_state_codec::block_state_codec,
    core::{Vec3, uuid_codec},
};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::mth::Uuid;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MinecartLoadError {
    #[error("native chest minecart loading unavailable: {0}")]
    Unsupported(String),
    #[error("invalid native chest minecart: {0}")]
    Invalid(String),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTriState {
    NotSet,
    True,
    False,
}
impl NativeTriState {
    fn parse(value: &str) -> Option<Self> {
        match value {
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
pub(super) struct MinecartSavedState {
    pub motion: Vec3,
    pub rotation: [f32; 2],
    pub old_rotation: [f32; 2],
    pub fall_distance: f64,
    pub fire: i16,
    pub air: i32,
    pub on_ground: bool,
    pub invulnerable: bool,
    pub portal_cooldown: i32,
    pub age: i32,
    pub persist: bool,
    pub visible_by_default: bool,
    pub invisible: bool,
    pub max_air: i32,
    pub custom_name_visible: bool,
    pub silent: bool,
    pub no_gravity: bool,
    pub glowing: bool,
    pub frozen: i32,
    pub visual_fire: NativeTriState,
    pub custom_data: CompoundTag,
    pub bukkit_values: CompoundTag,
    pub origin: Option<Vec3>,
    pub origin_world: Option<Uuid>,
    pub from_spawner: bool,
    pub from_portal: bool,
    pub spawn_reason: Option<&'static str>,
    pub freeze_lock: bool,
    pub display: Option<BlockState>,
    pub display_offset: i32,
    pub flipped: bool,
    pub has_ticked: bool,
    pub friction: NativeTriState,
}
impl Default for MinecartSavedState {
    fn default() -> Self {
        Self {
            motion: Vec3::new(0.0, 0.0, 0.0),
            rotation: [0.0; 2],
            old_rotation: [0.0; 2],
            fall_distance: 0.0,
            fire: 0,
            air: 300,
            on_ground: false,
            invulnerable: false,
            portal_cooldown: 0,
            age: 0,
            persist: true,
            visible_by_default: true,
            invisible: false,
            max_air: 300,
            custom_name_visible: false,
            silent: false,
            no_gravity: false,
            glowing: false,
            frozen: 0,
            visual_fire: NativeTriState::NotSet,
            custom_data: CompoundTag::new(),
            bukkit_values: CompoundTag::new(),
            origin: None,
            origin_world: None,
            from_spawner: false,
            from_portal: false,
            spawn_reason: None,
            freeze_lock: false,
            display: None,
            display_offset: 8,
            flipped: false,
            has_ticked: true,
            friction: NativeTriState::NotSet,
        }
    }
}
fn vector<const N: usize>(input: &CompoundTag, key: &str) -> Option<[f64; N]> {
    let values = NbtOps
        .get_stream(input.get(key)?)
        .result_or_partial_silent()?;
    // ListCodec retains valid elements on decode errors; fixedSize accepts its
    // first N as a partial when extra entries remain.
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
fn rotation(input: &CompoundTag) -> [f32; 2] {
    let values = input
        .get("Rotation")
        .and_then(|v| NbtOps.get_stream(v).result_or_partial_silent())
        .unwrap_or_default();
    let values: Vec<_> = values
        .iter()
        .filter_map(|v| v.as_number().map(|n| n.float_value()))
        .collect();
    if values.len() >= 2 {
        [values[0], values[1]]
    } else {
        [0.0; 2]
    }
}
fn reason(value: &str) -> Option<&'static str> {
    const REASONS: &[&str] = &[
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
    ];
    REASONS.iter().copied().find(|v| *v == value)
}
impl NativeChestMinecart {
    /// Entity.load on an ALREADY genuinely constructed chest minecart. The
    /// caller must not optimize away constructor UUID/ID consumption when the
    /// input contains a replacement UUID. Unsupported families remain errors.
    pub fn load(&mut self, input: &CompoundTag) -> Result<(), MinecartLoadError> {
        self.load_root(input)?;
        if input
            .get_list("Passengers")
            .is_some_and(|list| !list.list.is_empty())
        {
            return Err(MinecartLoadError::Unsupported(
                "nonempty Passengers requires its actual runtime/codec".into(),
            ));
        }
        Ok(())
    }
    /// Entity.load itself does not recurse passengers; the outer loader must
    /// run postLoad (including limits) before requesting passenger support.
    pub fn load_root(&mut self, input: &CompoundTag) -> Result<(), MinecartLoadError> {
        for field in ["Items", "Tags"] {
            if input.get_list(field).is_some_and(|v| !v.list.is_empty()) {
                return Err(MinecartLoadError::Unsupported(format!(
                    "nonempty {field} requires its actual runtime/codec"
                )));
            }
        }
        if input.get("CustomName").is_some() {
            return Err(MinecartLoadError::Unsupported(
                "CustomName component codec".into(),
            ));
        }
        if input.get("Paper.LootableData").is_some() {
            return Err(MinecartLoadError::Unsupported(
                "Paper loot replenishment state".into(),
            ));
        }
        let p = vector::<3>(input, "Pos").unwrap_or([0.0; 3]);
        let p = Vec3::new(
            p[0].clamp(-3.0000512E7, 3.0000512E7),
            p[1].clamp(-2.0E7, 2.0E7),
            p[2].clamp(-3.0000512E7, 3.0000512E7),
        );
        // Paper setPosRaw refuses a nonfinite vector instead of installing it.
        if p.x.is_finite() && p.y.is_finite() && p.z.is_finite() {
            self.position = p;
        }
        self.old_position = self.position;
        let mut saved = MinecartSavedState::default();
        let motion = vector::<3>(input, "Motion")
            .unwrap_or([0.0; 3])
            .map(|v| if v.abs() > 10.0 { 0.0 } else { v });
        if motion.iter().all(|v| v.is_finite()) {
            saved.motion = Vec3::new(motion[0], motion[1], motion[2]);
        }
        let rotation = rotation(input);
        let yaw = rotation[0];
        let pitch = rotation[1];
        if yaw.is_finite() {
            saved.old_rotation[0] = yaw;
            saved.rotation[0] = yaw % 360.0;
        }
        if pitch.is_finite() {
            saved.rotation[1] = (pitch % 360.0).clamp(-90.0, 90.0);
            saved.old_rotation[1] = saved.rotation[1];
        }
        saved.fall_distance = input.get_double_or("fall_distance", 0.0);
        saved.fire = input.get_short_or("Fire", 0);
        saved.air = input.get_int_or("Air", 300);
        saved.on_ground = input.get_boolean_or("OnGround", false);
        saved.invulnerable = input.get_boolean_or("Invulnerable", false);
        saved.portal_cooldown = input.get_int_or("PortalCooldown", 0);
        if let Some(uuid) = uuid(input, "UUID") {
            self.uuid = uuid;
        }
        saved.custom_name_visible = input.get_boolean_or("CustomNameVisible", false);
        saved.silent = input.get_boolean_or("Silent", false);
        saved.no_gravity = input.get_boolean_or("NoGravity", false);
        saved.glowing = input.get_boolean_or("Glowing", false);
        saved.frozen = input.get_int_or("TicksFrozen", 0);
        saved.visual_fire = if let Some(value) = input.get_string("Paper.FireOverride") {
            NativeTriState::parse(value).unwrap_or(NativeTriState::NotSet)
        } else {
            input
                .get_boolean("HasVisualFire")
                .map(|v| {
                    if v {
                        NativeTriState::True
                    } else {
                        NativeTriState::False
                    }
                })
                .unwrap_or(NativeTriState::NotSet)
        };
        saved.custom_data = input.get_compound("data").cloned().unwrap_or_default();
        saved.display = input.get("DisplayState").and_then(|tag| {
            block_state_codec::<NbtOps>()
                .parse(&NbtOps, tag)
                .result_or_partial_silent()
        });
        saved.display_offset = input.get_int_or("DisplayOffset", 8);
        saved.flipped = input.get_boolean_or("FlippedRotation", false);
        saved.has_ticked = input.get_boolean_or("HasTicked", false);
        saved.friction = input
            .get_string("Paper.FrictionState")
            .and_then(|s| NativeTriState::parse(s))
            .unwrap_or(NativeTriState::NotSet);
        self.loot_table = input.get_string("LootTable").and_then(|key| {
            rivet_registry::Identifier::try_parse_result(key)
                .ok()
                .flatten()
        });
        self.loot_seed = input.get_long_or("LootTableSeed", 0);
        saved.age = input.get_int_or("Spigot.ticksLived", 0);
        saved.persist = input.get_boolean_or("Bukkit.persist", true);
        saved.visible_by_default = input.get_boolean_or("Bukkit.visibleByDefault", true);
        saved.max_air = input.get_int_or("Bukkit.MaxAirSupply", 300);
        saved.bukkit_values = input
            .get_compound("BukkitValues")
            .cloned()
            .unwrap_or_default();
        saved.invisible = input.get_boolean_or("Bukkit.invisible", false);
        saved.origin = vector::<3>(input, "Paper.Origin").map(|v| Vec3::new(v[0], v[1], v[2]));
        if saved.origin.is_some() {
            saved.origin_world = Some(uuid(input, "Paper.OriginWorld").unwrap_or(self.world_uuid));
        }
        saved.from_spawner = input.get_boolean_or("Paper.FromMobSpawner", false);
        saved.from_portal = input.get_boolean_or("Paper.FromNetherPortal", false);
        saved.spawn_reason = Some(
            input
                .get_string("Paper.SpawnReason")
                .and_then(|s| reason(s))
                .unwrap_or(if saved.from_spawner {
                    "SPAWNER"
                } else {
                    "DEFAULT"
                }),
        );
        saved.freeze_lock = input.get_boolean_or("Paper.FreezeLock", false);
        self.saved = saved;
        Ok(())
    }
}
impl MinecartSavedState {
    pub fn write(&self, tag: &mut CompoundTag) {
        tag.put(
            "Motion".into(),
            doubles([self.motion.x, self.motion.y, self.motion.z]),
        );
        tag.put(
            "Rotation".into(),
            Tag::List(ListTag::with_list(
                self.rotation
                    .iter()
                    .map(|v| Tag::Float(FloatTag::value_of(*v)))
                    .collect(),
            )),
        );
        tag.put_double("fall_distance", self.fall_distance);
        tag.put_short("Fire", self.fire);
        tag.put_short("Air", self.air as i16);
        tag.put_boolean("OnGround", self.on_ground);
        tag.put_boolean("Invulnerable", self.invulnerable);
        tag.put_int("PortalCooldown", self.portal_cooldown);
        tag.put_int("Spigot.ticksLived", self.age);
        for (name, value) in [
            ("CustomNameVisible", self.custom_name_visible),
            ("Silent", self.silent),
            ("NoGravity", self.no_gravity),
            ("Glowing", self.glowing),
            ("Bukkit.invisible", self.invisible),
            ("Paper.FromMobSpawner", self.from_spawner),
            ("Paper.FromNetherPortal", self.from_portal),
            ("Paper.FreezeLock", self.freeze_lock),
        ] {
            if value {
                tag.put_boolean(name, true);
            }
        }
        if !self.persist {
            tag.put_boolean("Bukkit.persist", false);
        }
        if !self.visible_by_default {
            tag.put_boolean("Bukkit.visibleByDefault", false);
        }
        if self.max_air != 300 {
            tag.put_int("Bukkit.MaxAirSupply", self.max_air);
        }
        if self.frozen > 0 {
            tag.put_int("TicksFrozen", self.frozen);
        }
        if self.visual_fire == NativeTriState::True {
            tag.put_boolean("HasVisualFire", true);
        }
        if self.visual_fire != NativeTriState::NotSet {
            tag.put_string("Paper.FireOverride", self.visual_fire.name());
        }
        for (name, value) in [
            ("data", &self.custom_data),
            ("BukkitValues", &self.bukkit_values),
        ] {
            if !value.tags.is_empty() {
                tag.put(name.into(), Tag::Compound(value.clone()));
            }
        }
        if let Some(origin) = self.origin {
            tag.put(
                "Paper.Origin".into(),
                doubles([origin.x, origin.y, origin.z]),
            );
            if let Some(uuid) = self.origin_world {
                tag.put_int_array(
                    "Paper.OriginWorld",
                    rivet_registry::core::uuid_to_int_array(uuid).to_vec(),
                );
            }
        }
        if let Some(reason) = self.spawn_reason {
            tag.put_string("Paper.SpawnReason", reason);
        }
        if let Some(state) = self.display {
            tag.put(
                "DisplayState".into(),
                Tag::Compound(rivet_nbt::nbt_utils::write_block_state(state)),
            );
        }
        if self.display_offset != 8 {
            tag.put_int("DisplayOffset", self.display_offset);
        }
        tag.put_boolean("FlippedRotation", self.flipped);
        tag.put_boolean("HasTicked", self.has_ticked);
        if self.friction != NativeTriState::NotSet {
            tag.put_string("Paper.FrictionState", self.friction.name());
        }
    }
}
