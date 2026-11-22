//! Paper 26.2 `DamageType`, `DamageTypes`, `DamageEffects`, `DamageScaling`,
//! `DeathMessageType`, and `DamageTypeTags`.

use std::collections::BTreeSet;

/// Mirrors `net.minecraft.world.damagesource.DamageEffects`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DamageEffects {
    Hurt,
    Thorns,
    Drowning,
    Burning,
    Poking,
    Freezing,
}

impl DamageEffects {
    pub const ALL: [Self; 6] = [
        Self::Hurt,
        Self::Thorns,
        Self::Drowning,
        Self::Burning,
        Self::Poking,
        Self::Freezing,
    ];

    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Hurt => "hurt",
            Self::Thorns => "thorns",
            Self::Drowning => "drowning",
            Self::Burning => "burning",
            Self::Poking => "poking",
            Self::Freezing => "freezing",
        }
    }

    pub const fn sound_id(self) -> &'static str {
        match self {
            Self::Hurt | Self::Thorns => "minecraft:entity.player.hurt",
            Self::Drowning => "minecraft:entity.player.hurt_drown",
            Self::Burning => "minecraft:entity.player.hurt_on_fire",
            Self::Poking => "minecraft:entity.player.hurt_sweet_berry_bush",
            Self::Freezing => "minecraft:entity.player.hurt_freeze",
        }
    }

    pub fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "hurt" => Some(Self::Hurt),
            "thorns" => Some(Self::Thorns),
            "drowning" => Some(Self::Drowning),
            "burning" => Some(Self::Burning),
            "poking" => Some(Self::Poking),
            "freezing" => Some(Self::Freezing),
            _ => None,
        }
    }
}

/// Mirrors `net.minecraft.world.damagesource.DamageScaling`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DamageScaling {
    Never,
    WhenCausedByLivingNonPlayer,
    Always,
}

impl DamageScaling {
    pub const ALL: [Self; 3] = [Self::Never, Self::WhenCausedByLivingNonPlayer, Self::Always];

    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::WhenCausedByLivingNonPlayer => "when_caused_by_living_non_player",
            Self::Always => "always",
        }
    }

    pub fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "never" => Some(Self::Never),
            "when_caused_by_living_non_player" => Some(Self::WhenCausedByLivingNonPlayer),
            "always" => Some(Self::Always),
            _ => None,
        }
    }
}

/// Mirrors `net.minecraft.world.damagesource.DeathMessageType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeathMessageType {
    Default,
    FallVariants,
    IntentionalGameDesign,
}

impl DeathMessageType {
    pub const ALL: [Self; 3] = [
        Self::Default,
        Self::FallVariants,
        Self::IntentionalGameDesign,
    ];

    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::FallVariants => "fall_variants",
            Self::IntentionalGameDesign => "intentional_game_design",
        }
    }

    pub fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "default" => Some(Self::Default),
            "fall_variants" => Some(Self::FallVariants),
            "intentional_game_design" => Some(Self::IntentionalGameDesign),
            _ => None,
        }
    }
}

/// Mirrors `net.minecraft.world.damagesource.DamageType`.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageType {
    pub msg_id: String,
    pub scaling: DamageScaling,
    pub exhaustion: f32,
    pub effects: DamageEffects,
    pub death_message_type: DeathMessageType,
}

impl DamageType {
    pub fn new(
        msg_id: impl Into<String>,
        scaling: DamageScaling,
        exhaustion: f32,
        effects: DamageEffects,
        death_message_type: DeathMessageType,
    ) -> Self {
        Self {
            msg_id: msg_id.into(),
            scaling,
            exhaustion,
            effects,
            death_message_type,
        }
    }

    pub fn with_scaling(
        msg_id: impl Into<String>,
        scaling: DamageScaling,
        exhaustion: f32,
    ) -> Self {
        Self::new(
            msg_id,
            scaling,
            exhaustion,
            DamageEffects::Hurt,
            DeathMessageType::Default,
        )
    }

    pub fn with_effects(
        msg_id: impl Into<String>,
        exhaustion: f32,
        effects: DamageEffects,
    ) -> Self {
        Self::new(
            msg_id,
            DamageScaling::WhenCausedByLivingNonPlayer,
            exhaustion,
            effects,
            DeathMessageType::Default,
        )
    }

    pub fn simple(msg_id: impl Into<String>, exhaustion: f32) -> Self {
        Self::new(
            msg_id,
            DamageScaling::WhenCausedByLivingNonPlayer,
            exhaustion,
            DamageEffects::Hurt,
            DeathMessageType::Default,
        )
    }
}

/// Vanilla damage type registry keys from `net.minecraft.world.damagesource.DamageTypes` (MC 26.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DamageTypeKey {
    InFire,
    Campfire,
    LightningBolt,
    OnFire,
    Lava,
    HotFloor,
    SulfurCubeHot,
    InWall,
    Cramming,
    Drown,
    Starve,
    Cactus,
    Fall,
    EnderPearl,
    FlyIntoWall,
    FellOutOfWorld,
    Generic,
    Magic,
    Wither,
    DragonBreath,
    DryOut,
    SweetBerryBush,
    Freeze,
    Stalagmite,
    FallingBlock,
    FallingAnvil,
    FallingStalactite,
    Sting,
    MobAttack,
    MobAttackNoAggro,
    PlayerAttack,
    Spear,
    Arrow,
    Trident,
    MobProjectile,
    Spit,
    WindCharge,
    Fireworks,
    Fireball,
    UnattributedFireball,
    WitherSkull,
    Thrown,
    IndirectMagic,
    Thorns,
    Explosion,
    PlayerExplosion,
    SonicBoom,
    BadRespawnPoint,
    OutsideBorder,
    GenericKill,
    MaceSmash,
}

impl DamageTypeKey {
    pub const ALL: [Self; 51] = [
        Self::InFire,
        Self::Campfire,
        Self::LightningBolt,
        Self::OnFire,
        Self::Lava,
        Self::HotFloor,
        Self::SulfurCubeHot,
        Self::InWall,
        Self::Cramming,
        Self::Drown,
        Self::Starve,
        Self::Cactus,
        Self::Fall,
        Self::EnderPearl,
        Self::FlyIntoWall,
        Self::FellOutOfWorld,
        Self::Generic,
        Self::Magic,
        Self::Wither,
        Self::DragonBreath,
        Self::DryOut,
        Self::SweetBerryBush,
        Self::Freeze,
        Self::Stalagmite,
        Self::FallingBlock,
        Self::FallingAnvil,
        Self::FallingStalactite,
        Self::Sting,
        Self::MobAttack,
        Self::MobAttackNoAggro,
        Self::PlayerAttack,
        Self::Spear,
        Self::Arrow,
        Self::Trident,
        Self::MobProjectile,
        Self::Spit,
        Self::WindCharge,
        Self::Fireworks,
        Self::Fireball,
        Self::UnattributedFireball,
        Self::WitherSkull,
        Self::Thrown,
        Self::IndirectMagic,
        Self::Thorns,
        Self::Explosion,
        Self::PlayerExplosion,
        Self::SonicBoom,
        Self::BadRespawnPoint,
        Self::OutsideBorder,
        Self::GenericKill,
        Self::MaceSmash,
    ];

    /// Namespaced identifier matching `DamageTypes` in Paper 26.2.
    pub const fn id(self) -> &'static str {
        match self {
            Self::InFire => "minecraft:in_fire",
            Self::Campfire => "minecraft:campfire",
            Self::LightningBolt => "minecraft:lightning_bolt",
            Self::OnFire => "minecraft:on_fire",
            Self::Lava => "minecraft:lava",
            Self::HotFloor => "minecraft:hot_floor",
            Self::SulfurCubeHot => "minecraft:sulfur_cube_hot",
            Self::InWall => "minecraft:in_wall",
            Self::Cramming => "minecraft:cramming",
            Self::Drown => "minecraft:drown",
            Self::Starve => "minecraft:starve",
            Self::Cactus => "minecraft:cactus",
            Self::Fall => "minecraft:fall",
            Self::EnderPearl => "minecraft:ender_pearl",
            Self::FlyIntoWall => "minecraft:fly_into_wall",
            Self::FellOutOfWorld => "minecraft:out_of_world",
            Self::Generic => "minecraft:generic",
            Self::Magic => "minecraft:magic",
            Self::Wither => "minecraft:wither",
            Self::DragonBreath => "minecraft:dragon_breath",
            Self::DryOut => "minecraft:dry_out",
            Self::SweetBerryBush => "minecraft:sweet_berry_bush",
            Self::Freeze => "minecraft:freeze",
            Self::Stalagmite => "minecraft:stalagmite",
            Self::FallingBlock => "minecraft:falling_block",
            Self::FallingAnvil => "minecraft:falling_anvil",
            Self::FallingStalactite => "minecraft:falling_stalactite",
            Self::Sting => "minecraft:sting",
            Self::MobAttack => "minecraft:mob_attack",
            Self::MobAttackNoAggro => "minecraft:mob_attack_no_aggro",
            Self::PlayerAttack => "minecraft:player_attack",
            Self::Spear => "minecraft:spear",
            Self::Arrow => "minecraft:arrow",
            Self::Trident => "minecraft:trident",
            Self::MobProjectile => "minecraft:mob_projectile",
            Self::Spit => "minecraft:spit",
            Self::WindCharge => "minecraft:wind_charge",
            Self::Fireworks => "minecraft:fireworks",
            Self::Fireball => "minecraft:fireball",
            Self::UnattributedFireball => "minecraft:unattributed_fireball",
            Self::WitherSkull => "minecraft:wither_skull",
            Self::Thrown => "minecraft:thrown",
            Self::IndirectMagic => "minecraft:indirect_magic",
            Self::Thorns => "minecraft:thorns",
            Self::Explosion => "minecraft:explosion",
            Self::PlayerExplosion => "minecraft:player_explosion",
            Self::SonicBoom => "minecraft:sonic_boom",
            Self::BadRespawnPoint => "minecraft:bad_respawn_point",
            Self::OutsideBorder => "minecraft:outside_border",
            Self::GenericKill => "minecraft:generic_kill",
            Self::MaceSmash => "minecraft:mace_smash",
        }
    }

    /// Exact vanilla `DamageType` definition from `DamageTypes.bootstrap`.
    pub fn vanilla_type(self) -> DamageType {
        match self {
            Self::InFire | Self::Campfire => {
                DamageType::with_effects("inFire", 0.1, DamageEffects::Burning)
            }
            Self::LightningBolt => DamageType::simple("lightningBolt", 0.1),
            Self::OnFire => DamageType::with_effects("onFire", 0.0, DamageEffects::Burning),
            Self::Lava => DamageType::with_effects("lava", 0.1, DamageEffects::Burning),
            Self::HotFloor => DamageType::with_effects("hotFloor", 0.1, DamageEffects::Burning),
            Self::SulfurCubeHot => {
                DamageType::with_effects("sulfurCubeHot", 0.1, DamageEffects::Burning)
            }
            Self::InWall => DamageType::simple("inWall", 0.0),
            Self::Cramming => DamageType::simple("cramming", 0.0),
            Self::Drown => DamageType::with_effects("drown", 0.0, DamageEffects::Drowning),
            Self::Starve => DamageType::simple("starve", 0.0),
            Self::Cactus => DamageType::simple("cactus", 0.1),
            Self::Fall | Self::EnderPearl => DamageType::new(
                "fall",
                DamageScaling::WhenCausedByLivingNonPlayer,
                0.0,
                DamageEffects::Hurt,
                DeathMessageType::FallVariants,
            ),
            Self::FlyIntoWall => DamageType::simple("flyIntoWall", 0.0),
            Self::FellOutOfWorld => DamageType::simple("outOfWorld", 0.0),
            Self::Generic => DamageType::simple("generic", 0.0),
            Self::Magic => DamageType::simple("magic", 0.0),
            Self::Wither => DamageType::simple("wither", 0.0),
            Self::DragonBreath => DamageType::simple("dragonBreath", 0.0),
            Self::DryOut => DamageType::simple("dryout", 0.1),
            Self::SweetBerryBush => {
                DamageType::with_effects("sweetBerryBush", 0.1, DamageEffects::Poking)
            }
            Self::Freeze => DamageType::with_effects("freeze", 0.0, DamageEffects::Freezing),
            Self::Stalagmite => DamageType::simple("stalagmite", 0.0),
            Self::FallingBlock => DamageType::simple("fallingBlock", 0.1),
            Self::FallingAnvil => DamageType::simple("anvil", 0.1),
            Self::FallingStalactite => DamageType::simple("fallingStalactite", 0.1),
            Self::Sting => DamageType::simple("sting", 0.1),
            Self::MobAttack
            | Self::MobAttackNoAggro
            | Self::MobProjectile
            | Self::Spit
            | Self::WindCharge => DamageType::simple("mob", 0.1),
            Self::PlayerAttack => DamageType::simple("player", 0.1),
            Self::Spear => DamageType::simple("spear", 0.1),
            Self::Arrow => DamageType::simple("arrow", 0.1),
            Self::Trident => DamageType::simple("trident", 0.1),
            Self::Fireworks => DamageType::simple("fireworks", 0.1),
            Self::UnattributedFireball => {
                DamageType::with_effects("onFire", 0.1, DamageEffects::Burning)
            }
            Self::Fireball => DamageType::with_effects("fireball", 0.1, DamageEffects::Burning),
            Self::WitherSkull => DamageType::simple("witherSkull", 0.1),
            Self::Thrown => DamageType::simple("thrown", 0.1),
            Self::IndirectMagic => DamageType::simple("indirectMagic", 0.0),
            Self::Thorns => DamageType::with_effects("thorns", 0.1, DamageEffects::Thorns),
            Self::Explosion => DamageType::with_scaling("explosion", DamageScaling::Always, 0.1),
            Self::PlayerExplosion => {
                DamageType::with_scaling("explosion.player", DamageScaling::Always, 0.1)
            }
            Self::SonicBoom => DamageType::with_scaling("sonic_boom", DamageScaling::Always, 0.0),
            Self::BadRespawnPoint => DamageType::new(
                "badRespawnPoint",
                DamageScaling::Always,
                0.1,
                DamageEffects::Hurt,
                DeathMessageType::IntentionalGameDesign,
            ),
            Self::OutsideBorder => DamageType::simple("outsideBorder", 0.0),
            Self::GenericKill => DamageType::simple("genericKill", 0.0),
            Self::MaceSmash => DamageType::simple("mace_smash", 0.1),
        }
    }

    /// Checks vanilla tag membership matching `data/minecraft/tags/damage_type/*.json` in Paper 26.2.
    pub const fn is_in_tag(self, tag: DamageTypeTag) -> bool {
        use DamageTypeKey as K;
        match tag {
            DamageTypeTag::DamagesHelmet => {
                matches!(
                    self,
                    K::FallingAnvil | K::FallingBlock | K::FallingStalactite
                )
            }
            DamageTypeTag::BypassesArmor => matches!(
                self,
                K::OnFire
                    | K::InWall
                    | K::Cramming
                    | K::Drown
                    | K::FlyIntoWall
                    | K::Generic
                    | K::Wither
                    | K::DragonBreath
                    | K::Starve
                    | K::Fall
                    | K::EnderPearl
                    | K::Freeze
                    | K::Stalagmite
                    | K::Magic
                    | K::IndirectMagic
                    | K::FellOutOfWorld
                    | K::GenericKill
                    | K::SonicBoom
                    | K::OutsideBorder
            ),
            DamageTypeTag::BypassesShield => {
                self.is_in_tag(DamageTypeTag::BypassesArmor)
                    || matches!(
                        self,
                        K::Cactus
                            | K::Campfire
                            | K::DryOut
                            | K::FallingAnvil
                            | K::FallingStalactite
                            | K::HotFloor
                            | K::SulfurCubeHot
                            | K::InFire
                            | K::Lava
                            | K::LightningBolt
                            | K::SweetBerryBush
                    )
            }
            DamageTypeTag::BypassesInvulnerability => {
                matches!(self, K::FellOutOfWorld | K::GenericKill)
            }
            // No vanilla damage types are in `bypasses_cooldown` by default in MC 26.2.
            DamageTypeTag::BypassesCooldown => false,
            DamageTypeTag::BypassesEffects => matches!(self, K::Starve),
            DamageTypeTag::BypassesResistance => {
                matches!(self, K::FellOutOfWorld | K::GenericKill)
            }
            DamageTypeTag::BypassesEnchantments => matches!(self, K::SonicBoom),
            DamageTypeTag::IsFire => matches!(
                self,
                K::InFire
                    | K::Campfire
                    | K::OnFire
                    | K::Lava
                    | K::HotFloor
                    | K::SulfurCubeHot
                    | K::UnattributedFireball
                    | K::Fireball
            ),
            DamageTypeTag::IsProjectile => matches!(
                self,
                K::Arrow
                    | K::Trident
                    | K::MobProjectile
                    | K::UnattributedFireball
                    | K::Fireball
                    | K::WitherSkull
                    | K::Thrown
                    | K::WindCharge
            ),
            DamageTypeTag::WitchResistantTo => {
                matches!(self, K::Magic | K::IndirectMagic | K::SonicBoom | K::Thorns)
            }
            DamageTypeTag::IsExplosion => matches!(
                self,
                K::Fireworks | K::Explosion | K::PlayerExplosion | K::BadRespawnPoint
            ),
            DamageTypeTag::IsFall => matches!(self, K::Fall | K::EnderPearl | K::Stalagmite),
            DamageTypeTag::IsDrowning => matches!(self, K::Drown),
            DamageTypeTag::IsFreezing => matches!(self, K::Freeze),
            DamageTypeTag::IsLightning => matches!(self, K::LightningBolt),
            DamageTypeTag::NoAnger => matches!(self, K::MobAttackNoAggro),
            DamageTypeTag::NoImpact => matches!(self, K::Drown),
            DamageTypeTag::AlwaysMostSignificantFall => matches!(self, K::FellOutOfWorld),
            DamageTypeTag::WitherImmuneTo => matches!(self, K::Drown),
            DamageTypeTag::SulfurCubeWithBlockImmuneTo => {
                self.is_in_tag(DamageTypeTag::IsExplosion)
                    || matches!(
                        self,
                        K::Arrow
                            | K::Cactus
                            | K::DryOut
                            | K::Fall
                            | K::FallingAnvil
                            | K::FallingBlock
                            | K::FallingStalactite
                            | K::Freeze
                            | K::MaceSmash
                            | K::HotFloor
                            | K::MobAttack
                            | K::MobAttackNoAggro
                            | K::MobProjectile
                            | K::PlayerAttack
                            | K::Spear
                            | K::Spit
                            | K::Stalagmite
                            | K::Sting
                            | K::SulfurCubeHot
                            | K::SweetBerryBush
                            | K::Thrown
                            | K::Trident
                            | K::WindCharge
                    )
            }
            DamageTypeTag::IgnitesArmorStands => matches!(self, K::InFire | K::Campfire),
            DamageTypeTag::BurnsArmorStands => matches!(self, K::OnFire),
            DamageTypeTag::AvoidsGuardianThorns => {
                self.is_in_tag(DamageTypeTag::IsExplosion) || matches!(self, K::Magic | K::Thorns)
            }
            DamageTypeTag::AlwaysTriggersSilverfish => matches!(self, K::Magic),
            DamageTypeTag::AlwaysHurtsEnderDragons => self.is_in_tag(DamageTypeTag::IsExplosion),
            DamageTypeTag::NoKnockback => matches!(
                self,
                K::Explosion
                    | K::PlayerExplosion
                    | K::BadRespawnPoint
                    | K::InFire
                    | K::LightningBolt
                    | K::OnFire
                    | K::Lava
                    | K::HotFloor
                    | K::SulfurCubeHot
                    | K::InWall
                    | K::Cramming
                    | K::Drown
                    | K::Starve
                    | K::Cactus
                    | K::Fall
                    | K::EnderPearl
                    | K::FlyIntoWall
                    | K::FellOutOfWorld
                    | K::Generic
                    | K::Magic
                    | K::Wither
                    | K::DragonBreath
                    | K::DryOut
                    | K::SweetBerryBush
                    | K::Freeze
                    | K::Stalagmite
                    | K::OutsideBorder
                    | K::GenericKill
                    | K::Campfire
                    | K::Spear
            ),
            DamageTypeTag::AlwaysKillsArmorStands => matches!(
                self,
                K::Arrow | K::Trident | K::Fireball | K::WitherSkull | K::WindCharge
            ),
            DamageTypeTag::CanBreakArmorStand => {
                matches!(self, K::PlayerExplosion) || self.is_in_tag(DamageTypeTag::IsPlayerAttack)
            }
            DamageTypeTag::BypassesWolfArmor => {
                self.is_in_tag(DamageTypeTag::BypassesInvulnerability)
                    || matches!(
                        self,
                        K::Cramming
                            | K::Drown
                            | K::DryOut
                            | K::Freeze
                            | K::InWall
                            | K::IndirectMagic
                            | K::Magic
                            | K::OutsideBorder
                            | K::Starve
                            | K::Thorns
                            | K::Wither
                    )
            }
            DamageTypeTag::IsPlayerAttack => {
                matches!(self, K::PlayerAttack | K::Spear | K::MaceSmash)
            }
            DamageTypeTag::BurnFromStepping => {
                matches!(self, K::Campfire | K::HotFloor | K::SulfurCubeHot)
            }
            DamageTypeTag::PanicCauses => {
                self.is_in_tag(DamageTypeTag::PanicEnvironmentalCauses)
                    || self.is_in_tag(DamageTypeTag::IsPlayerAttack)
                    || matches!(
                        self,
                        K::Arrow
                            | K::DragonBreath
                            | K::Explosion
                            | K::Fireball
                            | K::Fireworks
                            | K::IndirectMagic
                            | K::Magic
                            | K::MobAttack
                            | K::MobProjectile
                            | K::PlayerExplosion
                            | K::SonicBoom
                            | K::Sting
                            | K::Thrown
                            | K::Trident
                            | K::UnattributedFireball
                            | K::WindCharge
                            | K::Wither
                            | K::WitherSkull
                    )
            }
            DamageTypeTag::PanicEnvironmentalCauses => matches!(
                self,
                K::Cactus
                    | K::Freeze
                    | K::HotFloor
                    | K::SulfurCubeHot
                    | K::InFire
                    | K::Lava
                    | K::LightningBolt
                    | K::OnFire
            ),
            DamageTypeTag::IsMaceSmash => matches!(self, K::MaceSmash),
        }
    }
}

/// Mirrors `net.minecraft.tags.DamageTypeTags` (MC 26.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DamageTypeTag {
    DamagesHelmet,
    BypassesArmor,
    BypassesShield,
    BypassesInvulnerability,
    BypassesCooldown,
    BypassesEffects,
    BypassesResistance,
    BypassesEnchantments,
    IsFire,
    IsProjectile,
    WitchResistantTo,
    IsExplosion,
    IsFall,
    IsDrowning,
    IsFreezing,
    IsLightning,
    NoAnger,
    NoImpact,
    AlwaysMostSignificantFall,
    WitherImmuneTo,
    SulfurCubeWithBlockImmuneTo,
    IgnitesArmorStands,
    BurnsArmorStands,
    AvoidsGuardianThorns,
    AlwaysTriggersSilverfish,
    AlwaysHurtsEnderDragons,
    NoKnockback,
    AlwaysKillsArmorStands,
    CanBreakArmorStand,
    BypassesWolfArmor,
    IsPlayerAttack,
    BurnFromStepping,
    PanicCauses,
    PanicEnvironmentalCauses,
    IsMaceSmash,
}

impl DamageTypeTag {
    pub const fn id(self) -> &'static str {
        match self {
            Self::DamagesHelmet => "minecraft:damages_helmet",
            Self::BypassesArmor => "minecraft:bypasses_armor",
            Self::BypassesShield => "minecraft:bypasses_shield",
            Self::BypassesInvulnerability => "minecraft:bypasses_invulnerability",
            Self::BypassesCooldown => "minecraft:bypasses_cooldown",
            Self::BypassesEffects => "minecraft:bypasses_effects",
            Self::BypassesResistance => "minecraft:bypasses_resistance",
            Self::BypassesEnchantments => "minecraft:bypasses_enchantments",
            Self::IsFire => "minecraft:is_fire",
            Self::IsProjectile => "minecraft:is_projectile",
            Self::WitchResistantTo => "minecraft:witch_resistant_to",
            Self::IsExplosion => "minecraft:is_explosion",
            Self::IsFall => "minecraft:is_fall",
            Self::IsDrowning => "minecraft:is_drowning",
            Self::IsFreezing => "minecraft:is_freezing",
            Self::IsLightning => "minecraft:is_lightning",
            Self::NoAnger => "minecraft:no_anger",
            Self::NoImpact => "minecraft:no_impact",
            Self::AlwaysMostSignificantFall => "minecraft:always_most_significant_fall",
            Self::WitherImmuneTo => "minecraft:wither_immune_to",
            Self::SulfurCubeWithBlockImmuneTo => "minecraft:sulfur_cube_with_block_immune_to",
            Self::IgnitesArmorStands => "minecraft:ignites_armor_stands",
            Self::BurnsArmorStands => "minecraft:burns_armor_stands",
            Self::AvoidsGuardianThorns => "minecraft:avoids_guardian_thorns",
            Self::AlwaysTriggersSilverfish => "minecraft:always_triggers_silverfish",
            Self::AlwaysHurtsEnderDragons => "minecraft:always_hurts_ender_dragons",
            Self::NoKnockback => "minecraft:no_knockback",
            Self::AlwaysKillsArmorStands => "minecraft:always_kills_armor_stands",
            Self::CanBreakArmorStand => "minecraft:can_break_armor_stand",
            Self::BypassesWolfArmor => "minecraft:bypasses_wolf_armor",
            Self::IsPlayerAttack => "minecraft:is_player_attack",
            Self::BurnFromStepping => "minecraft:burn_from_stepping",
            Self::PanicCauses => "minecraft:panic_causes",
            Self::PanicEnvironmentalCauses => "minecraft:panic_environmental_causes",
            Self::IsMaceSmash => "minecraft:mace_smash",
        }
    }
}

/// Optional per-source tag overrides for data-driven or test-configured damage types.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DamageTagSet {
    extra: BTreeSet<DamageTypeTag>,
}

impl DamageTagSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_tag(mut self, tag: DamageTypeTag) -> Self {
        self.extra.insert(tag);
        self
    }

    pub fn contains(&self, key: DamageTypeKey, tag: DamageTypeTag) -> bool {
        key.is_in_tag(tag) || self.extra.contains(&tag)
    }
}
