//! Paper 26.2 `DamageSource` and `DamageSources`.

use std::fmt;

use super::damage_type::{DamageScaling, DamageTagSet, DamageType, DamageTypeKey, DamageTypeTag};

/// Arena/ID-based entity snapshot carried by a [`DamageSource`].
///
/// Mirrors the minimal entity properties inspected by `DamageSource`, `CombatTracker`,
/// and `LivingEntity` damage attribution without taking ownership of `LivingEntity`.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageEntityRef {
    pub entity_id: i32,
    pub display_name: String,
    pub is_living: bool,
    pub is_player: bool,
    pub is_creative_player: bool,
    /// If the entity is a tame wolf with an owner, the owner player reference for kill credit.
    pub tame_owner_player: Option<Box<DamageEntityRef>>,
    /// Custom display name of the entity's main-hand item (`DataComponents.CUSTOM_NAME`), if any.
    pub main_hand_custom_name: Option<String>,
    /// Entity world position `[x, y, z]` used as fallback for `DamageSource::source_position`.
    pub position: Option<[f64; 3]>,
}

impl DamageEntityRef {
    pub fn living(entity_id: i32, display_name: impl Into<String>) -> Self {
        Self {
            entity_id,
            display_name: display_name.into(),
            is_living: true,
            is_player: false,
            is_creative_player: false,
            tame_owner_player: None,
            main_hand_custom_name: None,
            position: None,
        }
    }

    pub fn player(entity_id: i32, display_name: impl Into<String>, instabuild: bool) -> Self {
        Self {
            entity_id,
            display_name: display_name.into(),
            is_living: true,
            is_player: true,
            is_creative_player: instabuild,
            tame_owner_player: None,
            main_hand_custom_name: None,
            position: None,
        }
    }

    pub fn non_living(entity_id: i32, display_name: impl Into<String>) -> Self {
        Self {
            entity_id,
            display_name: display_name.into(),
            is_living: false,
            is_player: false,
            is_creative_player: false,
            tame_owner_player: None,
            main_hand_custom_name: None,
            position: None,
        }
    }

    pub fn with_main_hand_custom_name(mut self, name: impl Into<String>) -> Self {
        self.main_hand_custom_name = Some(name.into());
        self
    }

    pub fn with_position(mut self, pos: [f64; 3]) -> Self {
        self.position = Some(pos);
        self
    }

    pub fn with_tame_owner_player(mut self, owner: DamageEntityRef) -> Self {
        self.tame_owner_player = Some(Box::new(owner));
        self
    }
}

/// Structured translatable death message produced by `DamageSource` / `CombatTracker`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathMessage {
    pub translation_key: String,
    pub args: Vec<String>,
    /// Populated only for `DeathMessageType::IntentionalGameDesign` (`<deathMsg>.link`).
    pub intentional_game_design_link_key: Option<String>,
}

impl DeathMessage {
    pub fn translatable(key: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            translation_key: key.into(),
            args,
            intentional_game_design_link_key: None,
        }
    }
}

/// Mirrors `net.minecraft.world.damagesource.DamageSource`.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageSource {
    type_key: DamageTypeKey,
    damage_type: DamageType,
    extra_tags: DamageTagSet,
    causing_entity: Option<DamageEntityRef>,
    direct_entity: Option<DamageEntityRef>,
    damage_source_position: Option<[f64; 3]>,
    critical: bool,
}

impl DamageSource {
    pub fn new(
        type_key: DamageTypeKey,
        direct_entity: Option<DamageEntityRef>,
        causing_entity: Option<DamageEntityRef>,
        damage_source_position: Option<[f64; 3]>,
    ) -> Self {
        Self {
            type_key,
            damage_type: type_key.vanilla_type(),
            extra_tags: DamageTagSet::new(),
            causing_entity,
            direct_entity,
            damage_source_position,
            critical: false,
        }
    }

    pub fn from_type(type_key: DamageTypeKey) -> Self {
        Self::new(type_key, None, None, None)
    }

    pub fn with_causing(type_key: DamageTypeKey, causing_entity: Option<DamageEntityRef>) -> Self {
        Self::new(type_key, causing_entity.clone(), causing_entity, None)
    }

    pub fn with_direct_and_causing(
        type_key: DamageTypeKey,
        direct_entity: Option<DamageEntityRef>,
        causing_entity: Option<DamageEntityRef>,
    ) -> Self {
        Self::new(type_key, direct_entity, causing_entity, None)
    }

    pub fn with_position(type_key: DamageTypeKey, damage_source_position: [f64; 3]) -> Self {
        Self::new(type_key, None, None, Some(damage_source_position))
    }

    /// Paper/CraftBukkit: returns a clone marked as critical (arrows / sweeping).
    pub fn critical(&self) -> Self {
        let mut copy = self.clone();
        copy.critical = true;
        copy
    }

    pub fn is_critical(&self) -> bool {
        self.critical
    }

    /// Attaches additional tag overrides (e.g. data-pack tags like `bypasses_cooldown`).
    pub fn with_extra_tag(mut self, tag: DamageTypeTag) -> Self {
        self.extra_tags = self.extra_tags.with_tag(tag);
        self
    }

    pub fn type_key(&self) -> DamageTypeKey {
        self.type_key
    }

    pub fn damage_type(&self) -> &DamageType {
        &self.damage_type
    }

    pub fn food_exhaustion(&self) -> f32 {
        self.damage_type.exhaustion
    }

    /// Mirrors `DamageSource#isDirect()`: `this.causingEntity == this.directEntity`.
    pub fn is_direct(&self) -> bool {
        match (&self.causing_entity, &self.direct_entity) {
            (None, None) => true,
            (Some(c), Some(d)) => c.entity_id == d.entity_id,
            _ => false,
        }
    }

    pub fn direct_entity(&self) -> Option<&DamageEntityRef> {
        self.direct_entity.as_ref()
    }

    pub fn causing_entity(&self) -> Option<&DamageEntityRef> {
        self.causing_entity.as_ref()
    }

    pub fn msg_id(&self) -> &str {
        &self.damage_type.msg_id
    }

    /// Mirrors `DamageSource#scalesWithDifficulty()`.
    pub fn scales_with_difficulty(&self) -> bool {
        match self.damage_type.scaling {
            DamageScaling::Never => false,
            DamageScaling::WhenCausedByLivingNonPlayer => self
                .causing_entity
                .as_ref()
                .is_some_and(|e| e.is_living && !e.is_player),
            DamageScaling::Always => true,
        }
    }

    /// Mirrors `DamageSource#isCreativePlayer()`.
    pub fn is_creative_player(&self) -> bool {
        self.causing_entity
            .as_ref()
            .is_some_and(|e| e.is_player && e.is_creative_player)
    }

    /// Mirrors `DamageSource#getSourcePosition()`.
    pub fn source_position(&self) -> Option<[f64; 3]> {
        if let Some(pos) = self.damage_source_position {
            Some(pos)
        } else {
            self.direct_entity.as_ref().and_then(|e| e.position)
        }
    }

    /// Mirrors `DamageSource#sourcePositionRaw()`.
    pub fn source_position_raw(&self) -> Option<[f64; 3]> {
        self.damage_source_position
    }

    pub fn is_tag(&self, tag: DamageTypeTag) -> bool {
        self.extra_tags.contains(self.type_key, tag)
    }

    pub fn is_type(&self, key: DamageTypeKey) -> bool {
        self.type_key == key
    }

    /// Mirrors `DamageSource#getLocalizedDeathMessage(LivingEntity victim)`.
    pub fn localized_death_message(
        &self,
        victim_name: &str,
        kill_credit_name: Option<&str>,
    ) -> DeathMessage {
        let death_msg = format!("death.attack.{}", self.damage_type.msg_id);
        if self.causing_entity.is_none() && self.direct_entity.is_none() {
            match kill_credit_name {
                Some(killer) => DeathMessage::translatable(
                    format!("{death_msg}.player"),
                    vec![victim_name.to_owned(), killer.to_owned()],
                ),
                None => DeathMessage::translatable(death_msg, vec![victim_name.to_owned()]),
            }
        } else {
            let name = match (&self.causing_entity, &self.direct_entity) {
                (Some(c), _) => c.display_name.as_str(),
                (None, Some(d)) => d.display_name.as_str(),
                (None, None) => unreachable!(),
            };
            let held_custom_name = self
                .causing_entity
                .as_ref()
                .filter(|e| e.is_living)
                .and_then(|e| e.main_hand_custom_name.as_deref());
            match held_custom_name {
                Some(item_name) => DeathMessage::translatable(
                    format!("{death_msg}.item"),
                    vec![
                        victim_name.to_owned(),
                        name.to_owned(),
                        item_name.to_owned(),
                    ],
                ),
                None => DeathMessage::translatable(
                    death_msg,
                    vec![victim_name.to_owned(), name.to_owned()],
                ),
            }
        }
    }
}

impl fmt::Display for DamageSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DamageSource ({})", self.damage_type.msg_id)
    }
}

/// Mirrors `net.minecraft.world.damagesource.DamageSources`.
#[derive(Debug, Clone, Default)]
pub struct DamageSources;

impl DamageSources {
    pub fn new() -> Self {
        Self
    }

    pub fn in_fire(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::InFire)
    }

    pub fn campfire(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::Campfire)
    }

    pub fn lightning_bolt(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::LightningBolt)
    }

    pub fn on_fire(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::OnFire)
    }

    pub fn lava(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::Lava)
    }

    pub fn hot_floor(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::HotFloor)
    }

    pub fn sulfur_cube_hot(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::SulfurCubeHot)
    }

    pub fn in_wall(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::InWall)
    }

    pub fn cramming(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::Cramming)
    }

    pub fn drown(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::Drown)
    }

    pub fn starve(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::Starve)
    }

    pub fn cactus(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::Cactus)
    }

    pub fn fall(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::Fall)
    }

    pub fn ender_pearl(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::EnderPearl)
    }

    pub fn fly_into_wall(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::FlyIntoWall)
    }

    pub fn fell_out_of_world(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::FellOutOfWorld)
    }

    pub fn generic(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::Generic)
    }

    pub fn magic(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::Magic)
    }

    pub fn wither(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::Wither)
    }

    pub fn dragon_breath(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::DragonBreath)
    }

    pub fn dry_out(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::DryOut)
    }

    pub fn sweet_berry_bush(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::SweetBerryBush)
    }

    pub fn freeze(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::Freeze)
    }

    pub fn stalagmite(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::Stalagmite)
    }

    pub fn falling_block(&self, entity: DamageEntityRef) -> DamageSource {
        DamageSource::with_causing(DamageTypeKey::FallingBlock, Some(entity))
    }

    pub fn anvil(&self, entity: DamageEntityRef) -> DamageSource {
        DamageSource::with_causing(DamageTypeKey::FallingAnvil, Some(entity))
    }

    pub fn falling_stalactite(&self, entity: DamageEntityRef) -> DamageSource {
        DamageSource::with_causing(DamageTypeKey::FallingStalactite, Some(entity))
    }

    pub fn sting(&self, mob: DamageEntityRef) -> DamageSource {
        DamageSource::with_causing(DamageTypeKey::Sting, Some(mob))
    }

    pub fn mob_attack(&self, mob: DamageEntityRef) -> DamageSource {
        DamageSource::with_causing(DamageTypeKey::MobAttack, Some(mob))
    }

    pub fn no_aggro_mob_attack(&self, mob: DamageEntityRef) -> DamageSource {
        DamageSource::with_causing(DamageTypeKey::MobAttackNoAggro, Some(mob))
    }

    pub fn player_attack(&self, player: DamageEntityRef) -> DamageSource {
        DamageSource::with_causing(DamageTypeKey::PlayerAttack, Some(player))
    }

    pub fn spear(&self, owner: DamageEntityRef) -> DamageSource {
        DamageSource::with_causing(DamageTypeKey::Spear, Some(owner))
    }

    pub fn arrow(&self, arrow: DamageEntityRef, owner: Option<DamageEntityRef>) -> DamageSource {
        DamageSource::with_direct_and_causing(DamageTypeKey::Arrow, Some(arrow), owner)
    }

    pub fn trident(
        &self,
        trident: DamageEntityRef,
        owner: Option<DamageEntityRef>,
    ) -> DamageSource {
        DamageSource::with_direct_and_causing(DamageTypeKey::Trident, Some(trident), owner)
    }

    pub fn mob_projectile(
        &self,
        entity: DamageEntityRef,
        mob: Option<DamageEntityRef>,
    ) -> DamageSource {
        DamageSource::with_direct_and_causing(DamageTypeKey::MobProjectile, Some(entity), mob)
    }

    pub fn spit(&self, entity: DamageEntityRef, mob: Option<DamageEntityRef>) -> DamageSource {
        DamageSource::with_direct_and_causing(DamageTypeKey::Spit, Some(entity), mob)
    }

    pub fn wind_charge(
        &self,
        entity: DamageEntityRef,
        mob: Option<DamageEntityRef>,
    ) -> DamageSource {
        DamageSource::with_direct_and_causing(DamageTypeKey::WindCharge, Some(entity), mob)
    }

    pub fn fireworks(
        &self,
        rocket: DamageEntityRef,
        owner: Option<DamageEntityRef>,
    ) -> DamageSource {
        DamageSource::with_direct_and_causing(DamageTypeKey::Fireworks, Some(rocket), owner)
    }

    /// Mirrors `DamageSources#fireball`: switches to `UNATTRIBUTED_FIREBALL` when `owner` is `None`.
    pub fn fireball(
        &self,
        fireball: DamageEntityRef,
        owner: Option<DamageEntityRef>,
    ) -> DamageSource {
        match owner {
            None => DamageSource::with_causing(DamageTypeKey::UnattributedFireball, Some(fireball)),
            Some(owner) => DamageSource::with_direct_and_causing(
                DamageTypeKey::Fireball,
                Some(fireball),
                Some(owner),
            ),
        }
    }

    pub fn wither_skull(
        &self,
        wither_skull: DamageEntityRef,
        owner: DamageEntityRef,
    ) -> DamageSource {
        DamageSource::with_direct_and_causing(
            DamageTypeKey::WitherSkull,
            Some(wither_skull),
            Some(owner),
        )
    }

    pub fn thrown(&self, entity: DamageEntityRef, owner: Option<DamageEntityRef>) -> DamageSource {
        DamageSource::with_direct_and_causing(DamageTypeKey::Thrown, Some(entity), owner)
    }

    pub fn indirect_magic(
        &self,
        entity: DamageEntityRef,
        owner: Option<DamageEntityRef>,
    ) -> DamageSource {
        DamageSource::with_direct_and_causing(DamageTypeKey::IndirectMagic, Some(entity), owner)
    }

    pub fn thorns(&self, source: DamageEntityRef) -> DamageSource {
        DamageSource::with_causing(DamageTypeKey::Thorns, Some(source))
    }

    /// Mirrors `DamageSources#explosion(Entity, Entity)`: uses `PLAYER_EXPLOSION` only when
    /// both `entity` and `cause` are present.
    pub fn explosion(
        &self,
        entity: Option<DamageEntityRef>,
        cause: Option<DamageEntityRef>,
    ) -> DamageSource {
        let key = if cause.is_some() && entity.is_some() {
            DamageTypeKey::PlayerExplosion
        } else {
            DamageTypeKey::Explosion
        };
        DamageSource::with_direct_and_causing(key, entity, cause)
    }

    pub fn sonic_boom(&self, entity: DamageEntityRef) -> DamageSource {
        DamageSource::with_causing(DamageTypeKey::SonicBoom, Some(entity))
    }

    pub fn bad_respawn_point_explosion(&self, boom_pos: [f64; 3]) -> DamageSource {
        DamageSource::with_position(DamageTypeKey::BadRespawnPoint, boom_pos)
    }

    pub fn out_of_border(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::OutsideBorder)
    }

    pub fn generic_kill(&self) -> DamageSource {
        DamageSource::from_type(DamageTypeKey::GenericKill)
    }

    pub fn mace(&self, owner: DamageEntityRef) -> DamageSource {
        DamageSource::with_causing(DamageTypeKey::MaceSmash, Some(owner))
    }
}
