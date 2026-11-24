//! Combat, weapon, tool, and enchanting leaf item components:
//! - `Weapon` (`net.minecraft.world.item.component.Weapon`)
//! - `AttackRange` (`net.minecraft.world.item.component.AttackRange`)
//! - `SwingAnimationType` & `SwingAnimation` (`net.minecraft.world.item.SwingAnimationType`, `...component.SwingAnimation`)
//! - `DamageResistant` (`net.minecraft.world.item.component.DamageResistant`)
//! - `BlocksAttacks`, `DamageReduction`, `ItemDamageFunction` (`net.minecraft.world.item.component.BlocksAttacks`)
//! - `PiercingWeapon` (`net.minecraft.world.item.component.PiercingWeapon`)
//! - `KineticWeapon` & `KineticWeaponCondition` (`net.minecraft.world.item.component.KineticWeapon`)
//! - `Tool` & `ToolRule` (`net.minecraft.world.item.component.Tool`)
//! - `Enchantable` (`net.minecraft.world.item.enchantment.Enchantable`)
//! - `Repairable` (`net.minecraft.world.item.enchantment.Repairable`)

use crate::item::codec_from_fns;
use crate::item::components::core::{
    HolderRef, HolderSetRef, SoundEvent, decode_sound, encode_sound, holder_set_ref_codec,
};
use crate::item::components::{
    get_map_field, result_of, validate_float_range, validate_int_range,
    validate_non_negative_float, validate_non_negative_int, validate_positive_float,
    validate_positive_int,
};
use crate::loot::java_round_f32;
use rivet_registry::Identifier;
use rivet_serialization::{Codec, DataResult, DynamicOps};
use rivet_util::mth;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Weapon
// ---------------------------------------------------------------------------

/// `Weapon.AXE_DISABLES_BLOCKING_FOR_SECONDS = 5.0F`.
pub const AXE_DISABLES_BLOCKING_FOR_SECONDS: f32 = 5.0;

/// Port of `net.minecraft.world.item.component.Weapon`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weapon {
    pub item_damage_per_attack: i32,
    pub disable_blocking_for_seconds: f32,
}

impl Weapon {
    /// The codec-level field validators (Paper's `Codec.validate` /
    /// range codecs), which fail on encode as well as decode.
    pub fn validate(&self) -> Result<(), String> {
        result_of(validate_non_negative_int(
            self.item_damage_per_attack,
            "item_damage_per_attack",
        ))?;
        result_of(validate_non_negative_float(
            self.disable_blocking_for_seconds,
            "disable_blocking_for_seconds",
        ))?;
        Ok(())
    }

    pub fn try_new(
        item_damage_per_attack: i32,
        disable_blocking_for_seconds: f32,
    ) -> Result<Self, String> {
        let value = Self {
            item_damage_per_attack,
            disable_blocking_for_seconds,
        };
        value.validate()?;
        Ok(value)
    }

    pub const fn with_damage(item_damage_per_attack: i32) -> Self {
        Self {
            item_damage_per_attack,
            disable_blocking_for_seconds: 0.0,
        }
    }
}

impl Default for Weapon {
    fn default() -> Self {
        Self {
            item_damage_per_attack: 1,
            disable_blocking_for_seconds: 0.0,
        }
    }
}

/// `Weapon.CODEC`.
pub fn weapon_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Weapon, Ops>> {
    codec_from_fns(
        "weapon",
        |v: &Weapon, ops: &Ops| {
            if let Err(e) = v.validate() {
                return DataResult::error(e);
            }
            let mut entries = Vec::new();
            if v.item_damage_per_attack != 1 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("item_damage_per_attack".to_string()),
                    ops.create_int(v.item_damage_per_attack),
                ));
            }
            if v.disable_blocking_for_seconds != 0.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("disable_blocking_for_seconds".to_string()),
                    ops.create_float(v.disable_blocking_for_seconds),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("Weapon must be a map");
            };
            let item_damage_per_attack =
                if let Some(val) = get_map_field(ops, map.as_ref(), "item_damage_per_attack") {
                    let n_res = ops.get_number_value(&val);
                    let Some(n) = n_res.result() else {
                        return DataResult::error("Weapon.item_damage_per_attack must be a number");
                    };
                    let v = n.int_value();
                    if v < 0 {
                        return DataResult::error(format!("Value must be non-negative: {v}"));
                    }
                    v
                } else {
                    1
                };
            let disable_blocking_for_seconds = if let Some(val) =
                get_map_field(ops, map.as_ref(), "disable_blocking_for_seconds")
            {
                let n_res = ops.get_number_value(&val);
                let Some(n) = n_res.result() else {
                    return DataResult::error(
                        "Weapon.disable_blocking_for_seconds must be a number",
                    );
                };
                let f = n.float_value();
                if let Some(err) =
                    validate_non_negative_float(f, "disable_blocking_for_seconds").error_ref()
                {
                    return DataResult::error(err.message().to_string());
                }
                f
            } else {
                0.0
            };
            DataResult::success(Weapon {
                item_damage_per_attack,
                disable_blocking_for_seconds,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// AttackRange
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.AttackRange`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttackRange {
    pub min_reach: f32,
    pub max_reach: f32,
    pub min_creative_reach: f32,
    pub max_creative_reach: f32,
    pub hitbox_margin: f32,
    pub mob_factor: f32,
}

impl AttackRange {
    /// The codec-level field validators (Paper's `Codec.validate` /
    /// range codecs), which fail on encode as well as decode.
    pub fn validate(&self) -> Result<(), String> {
        for (name, val, max) in [
            ("min_reach", self.min_reach, 64.0_f32),
            ("max_reach", self.max_reach, 64.0_f32),
            ("min_creative_reach", self.min_creative_reach, 64.0_f32),
            ("max_creative_reach", self.max_creative_reach, 64.0_f32),
            ("hitbox_margin", self.hitbox_margin, 1.0_f32),
            ("mob_factor", self.mob_factor, 2.0_f32),
        ] {
            result_of(validate_float_range(val, 0.0, max, name))?;
        }
        Ok(())
    }

    pub const DEFAULT: Self = Self {
        min_reach: 0.0,
        max_reach: 3.0,
        min_creative_reach: 0.0,
        max_creative_reach: 5.0,
        hitbox_margin: 0.3,
        mob_factor: 1.0,
    };

    pub fn try_new(
        min_reach: f32,
        max_reach: f32,
        min_creative_reach: f32,
        max_creative_reach: f32,
        hitbox_margin: f32,
        mob_factor: f32,
    ) -> Result<Self, String> {
        let value = Self {
            min_reach,
            max_reach,
            min_creative_reach,
            max_creative_reach,
            hitbox_margin,
            mob_factor,
        };
        value.validate()?;
        Ok(value)
    }

    pub const fn effective_min_range(self, is_creative: bool) -> f32 {
        if is_creative {
            self.min_creative_reach
        } else {
            self.min_reach
        }
    }

    pub const fn effective_max_range(self, is_creative: bool) -> f32 {
        if is_creative {
            self.max_creative_reach
        } else {
            self.max_reach
        }
    }
}

impl Default for AttackRange {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// `AttackRange.CODEC`.
pub fn attack_range_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<AttackRange, Ops>> {
    codec_from_fns(
        "attack_range",
        |v: &AttackRange, ops: &Ops| {
            if let Err(e) = v.validate() {
                return DataResult::error(e);
            }
            let mut entries = Vec::new();
            if v.min_reach != 0.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("min_reach".to_string()),
                    ops.create_float(v.min_reach),
                ));
            }
            if v.max_reach != 3.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("max_reach".to_string()),
                    ops.create_float(v.max_reach),
                ));
            }
            if v.min_creative_reach != 0.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("min_creative_reach".to_string()),
                    ops.create_float(v.min_creative_reach),
                ));
            }
            if v.max_creative_reach != 5.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("max_creative_reach".to_string()),
                    ops.create_float(v.max_creative_reach),
                ));
            }
            if v.hitbox_margin != 0.3 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("hitbox_margin".to_string()),
                    ops.create_float(v.hitbox_margin),
                ));
            }
            if v.mob_factor != 1.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("mob_factor".to_string()),
                    ops.create_float(v.mob_factor),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("AttackRange must be a map");
            };
            let read_f = |field: &str, default: f32, max: f32| -> Result<f32, String> {
                match get_map_field(ops, map.as_ref(), field) {
                    Some(val) => {
                        let n = ops
                            .get_number_value(&val)
                            .result()
                            .copied()
                            .ok_or_else(|| format!("AttackRange.{field} must be a number"))?;
                        let f = n.float_value();
                        if let Some(err) = validate_float_range(f, 0.0, max, field).error_ref() {
                            return Err(err.message().to_string());
                        }
                        Ok(f)
                    }
                    None => Ok(default),
                }
            };
            let min_reach = match read_f("min_reach", 0.0, 64.0) {
                Ok(v) => v,
                Err(e) => return DataResult::error(e),
            };
            let max_reach = match read_f("max_reach", 3.0, 64.0) {
                Ok(v) => v,
                Err(e) => return DataResult::error(e),
            };
            let min_creative_reach = match read_f("min_creative_reach", 0.0, 64.0) {
                Ok(v) => v,
                Err(e) => return DataResult::error(e),
            };
            let max_creative_reach = match read_f("max_creative_reach", 5.0, 64.0) {
                Ok(v) => v,
                Err(e) => return DataResult::error(e),
            };
            let hitbox_margin = match read_f("hitbox_margin", 0.3, 1.0) {
                Ok(v) => v,
                Err(e) => return DataResult::error(e),
            };
            let mob_factor = match read_f("mob_factor", 1.0, 2.0) {
                Ok(v) => v,
                Err(e) => return DataResult::error(e),
            };
            DataResult::success(AttackRange {
                min_reach,
                max_reach,
                min_creative_reach,
                max_creative_reach,
                hitbox_margin,
                mob_factor,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// SwingAnimationType & SwingAnimation
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.SwingAnimationType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum SwingAnimationType {
    None = 0,
    Whack = 1,
    Stab = 2,
}

impl SwingAnimationType {
    pub const fn id(self) -> i32 {
        self as i32
    }

    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Whack => "whack",
            Self::Stab => "stab",
        }
    }

    pub fn from_serialized_name(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "whack" => Some(Self::Whack),
            "stab" => Some(Self::Stab),
            _ => None,
        }
    }

    pub const fn by_id(id: i32) -> Self {
        match id {
            1 => Self::Whack,
            2 => Self::Stab,
            _ => Self::None,
        }
    }
}

/// `SwingAnimationType.CODEC`.
pub fn swing_animation_type_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<SwingAnimationType, Ops>> {
    codec_from_fns(
        "swing_animation_type",
        |v: &SwingAnimationType, ops: &Ops| {
            DataResult::success(ops.create_string(v.serialized_name().to_string()))
        },
        |ops: &Ops, input: &Ops::Output| {
            ops.get_string_value(input).flat_map(|s| {
                SwingAnimationType::from_serialized_name(&s)
                    .map(DataResult::success)
                    .unwrap_or_else(|| {
                        DataResult::error(format!("Unknown SwingAnimationType: {s}"))
                    })
            })
        },
    )
}

/// Port of `net.minecraft.world.item.component.SwingAnimation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SwingAnimation {
    pub animation_type: SwingAnimationType,
    pub duration: i32,
}

impl SwingAnimation {
    /// The codec-level field validators (Paper's `Codec.validate` /
    /// range codecs), which fail on encode as well as decode.
    pub fn validate(&self) -> Result<(), String> {
        result_of(validate_positive_int(self.duration, "duration"))?;
        Ok(())
    }

    /// `SwingAnimation.DEFAULT = new SwingAnimation(SwingAnimationType.WHACK, 6)`.
    pub const DEFAULT: Self = Self {
        animation_type: SwingAnimationType::Whack,
        duration: 6,
    };

    pub fn try_new(animation_type: SwingAnimationType, duration: i32) -> Result<Self, String> {
        let value = Self {
            animation_type,
            duration,
        };
        value.validate()?;
        Ok(value)
    }
}

impl Default for SwingAnimation {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// `SwingAnimation.CODEC`.
pub fn swing_animation_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<SwingAnimation, Ops>> {
    let ty_codec = swing_animation_type_codec::<Ops>();
    let enc_ty = Arc::clone(&ty_codec);
    let dec_ty = ty_codec;
    codec_from_fns(
        "swing_animation",
        move |v: &SwingAnimation, ops: &Ops| {
            if let Err(e) = v.validate() {
                return DataResult::error(e);
            }
            let mut entries = Vec::new();
            if v.animation_type != SwingAnimationType::Whack {
                let encoded = enc_ty.encode_start(ops, &v.animation_type);
                match encoded.result().cloned() {
                    Some(ev) => entries.push(rivet_serialization::Pair::of(
                        ops.create_string("type".to_string()),
                        ev,
                    )),
                    None => return DataResult::error("Failed to encode SwingAnimation.type"),
                }
            }
            if v.duration != 6 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("duration".to_string()),
                    ops.create_int(v.duration),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("SwingAnimation must be a map");
            };
            let animation_type = if let Some(val) = get_map_field(ops, map.as_ref(), "type") {
                match dec_ty.parse(ops, &val).result().copied() {
                    Some(t) => t,
                    None => return DataResult::error("Invalid SwingAnimation.type"),
                }
            } else {
                SwingAnimationType::Whack
            };
            let duration = if let Some(val) = get_map_field(ops, map.as_ref(), "duration") {
                let n_res = ops.get_number_value(&val);
                let Some(n) = n_res.result() else {
                    return DataResult::error("SwingAnimation.duration must be a number");
                };
                let d = n.int_value();
                if d <= 0 {
                    return DataResult::error(format!("Value must be positive: {d}"));
                }
                d
            } else {
                6
            };
            DataResult::success(SwingAnimation {
                animation_type,
                duration,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// DamageResistant
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.DamageResistant`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DamageResistant {
    pub types: HolderSetRef,
}

impl DamageResistant {
    pub fn new(types: HolderSetRef) -> Self {
        Self { types }
    }
}

/// `DamageResistant.CODEC`.
pub fn damage_resistant_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<DamageResistant, Ops>> {
    let hs = holder_set_ref_codec::<Ops>();
    let enc_hs = Arc::clone(&hs);
    let dec_hs = hs;
    codec_from_fns(
        "damage_resistant",
        move |v: &DamageResistant, ops: &Ops| {
            let encoded = enc_hs.encode_start(ops, &v.types);
            match encoded.result().cloned() {
                Some(ev) => {
                    DataResult::success(ops.create_map(vec![rivet_serialization::Pair::of(
                        ops.create_string("types".to_string()),
                        ev,
                    )]))
                }
                None => DataResult::error("Failed to encode DamageResistant.types"),
            }
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("DamageResistant must be a map");
            };
            let Some(val) = get_map_field(ops, map.as_ref(), "types") else {
                return DataResult::error("DamageResistant missing required field 'types'");
            };
            dec_hs
                .parse(ops, &val)
                .map_owned(|types| DamageResistant { types })
        },
    )
}

// ---------------------------------------------------------------------------
// BlocksAttacks, DamageReduction, ItemDamageFunction
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.BlocksAttacks.DamageReduction`.
#[derive(Clone, Debug, PartialEq)]
pub struct DamageReduction {
    pub horizontal_blocking_angle: f32,
    pub damage_type: Option<HolderSetRef>,
    pub base: f32,
    pub factor: f32,
}

impl DamageReduction {
    /// The codec-level field validators (Paper's `Codec.validate` /
    /// range codecs), which fail on encode as well as decode.
    pub fn validate(&self) -> Result<(), String> {
        result_of(validate_positive_float(
            self.horizontal_blocking_angle,
            "horizontal_blocking_angle",
        ))?;
        Ok(())
    }

    /// Default reduction used by `BlocksAttacks`: `(90.0F, Optional.empty(), 0.0F, 1.0F)`.
    pub fn default_reduction() -> Self {
        Self {
            horizontal_blocking_angle: 90.0,
            damage_type: None,
            base: 0.0,
            factor: 1.0,
        }
    }

    pub fn try_new(
        horizontal_blocking_angle: f32,
        damage_type: Option<HolderSetRef>,
        base: f32,
        factor: f32,
    ) -> Result<Self, String> {
        let value = Self {
            horizontal_blocking_angle,
            damage_type,
            base,
            factor,
        };
        value.validate()?;
        Ok(value)
    }

    /// Port of `DamageReduction.resolve(DamageSource, float dealtDamage, double angle)`
    /// (caller passes `matches_type` after evaluating `damage_type` against the source).
    pub fn resolve(&self, matches_type: bool, dealt_damage: f32, angle_radians: f64) -> f32 {
        if !matches_type {
            return 0.0;
        }
        // Java: `Mth.DEG_TO_RAD * horizontalBlockingAngle` is a float product, widened afterwards.
        if angle_radians > f64::from(mth::DEG_TO_RAD * self.horizontal_blocking_angle) {
            return 0.0;
        }
        mth::clamp_f32(self.base + self.factor * dealt_damage, 0.0, dealt_damage)
    }
}

/// `BlocksAttacks.DamageReduction.CODEC`.
pub fn damage_reduction_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<DamageReduction, Ops>> {
    let hs = holder_set_ref_codec::<Ops>();
    let enc_hs = Arc::clone(&hs);
    let dec_hs = hs;
    codec_from_fns(
        "damage_reduction",
        move |v: &DamageReduction, ops: &Ops| {
            if let Err(e) = v.validate() {
                return DataResult::error(e);
            }
            let mut entries = Vec::new();
            if v.horizontal_blocking_angle != 90.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("horizontal_blocking_angle".to_string()),
                    ops.create_float(v.horizontal_blocking_angle),
                ));
            }
            if let Some(dt) = &v.damage_type {
                match enc_hs.encode_start(ops, dt).result().cloned() {
                    Some(ev) => entries.push(rivet_serialization::Pair::of(
                        ops.create_string("type".to_string()),
                        ev,
                    )),
                    None => return DataResult::error("Failed to encode DamageReduction.type"),
                }
            }
            entries.push(rivet_serialization::Pair::of(
                ops.create_string("base".to_string()),
                ops.create_float(v.base),
            ));
            entries.push(rivet_serialization::Pair::of(
                ops.create_string("factor".to_string()),
                ops.create_float(v.factor),
            ));
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("DamageReduction must be a map");
            };
            let horizontal_blocking_angle =
                if let Some(val) = get_map_field(ops, map.as_ref(), "horizontal_blocking_angle") {
                    let n_res = ops.get_number_value(&val);
                    let Some(n) = n_res.result() else {
                        return DataResult::error(
                            "DamageReduction.horizontal_blocking_angle must be a number",
                        );
                    };
                    let f = n.float_value();
                    if let Some(err) =
                        validate_positive_float(f, "horizontal_blocking_angle").error_ref()
                    {
                        return DataResult::error(err.message().to_string());
                    }
                    f
                } else {
                    90.0
                };
            let damage_type = if let Some(val) = get_map_field(ops, map.as_ref(), "type") {
                match dec_hs.parse(ops, &val).result().cloned() {
                    Some(t) => Some(t),
                    None => return DataResult::error("Invalid DamageReduction.type"),
                }
            } else {
                None
            };
            let Some(base_val) = get_map_field(ops, map.as_ref(), "base") else {
                return DataResult::error("DamageReduction missing required field 'base'");
            };
            let Some(base) = ops
                .get_number_value(&base_val)
                .result()
                .map(|n| n.float_value())
            else {
                return DataResult::error("DamageReduction.base must be a number");
            };
            let Some(factor_val) = get_map_field(ops, map.as_ref(), "factor") else {
                return DataResult::error("DamageReduction missing required field 'factor'");
            };
            let Some(factor) = ops
                .get_number_value(&factor_val)
                .result()
                .map(|n| n.float_value())
            else {
                return DataResult::error("DamageReduction.factor must be a number");
            };
            DataResult::success(DamageReduction {
                horizontal_blocking_angle,
                damage_type,
                base,
                factor,
            })
        },
    )
}

/// Port of `net.minecraft.world.item.component.BlocksAttacks.ItemDamageFunction`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemDamageFunction {
    pub threshold: f32,
    pub base: f32,
    pub factor: f32,
}

impl ItemDamageFunction {
    /// The codec-level field validators (Paper's `Codec.validate` /
    /// range codecs), which fail on encode as well as decode.
    pub fn validate(&self) -> Result<(), String> {
        result_of(validate_non_negative_float(self.threshold, "threshold"))?;
        Ok(())
    }

    /// `ItemDamageFunction.DEFAULT = new ItemDamageFunction(1.0F, 0.0F, 1.0F)`.
    pub const DEFAULT: Self = Self {
        threshold: 1.0,
        base: 0.0,
        factor: 1.0,
    };

    pub fn try_new(threshold: f32, base: f32, factor: f32) -> Result<Self, String> {
        let value = Self {
            threshold,
            base,
            factor,
        };
        value.validate()?;
        Ok(value)
    }

    /// Exact port of `ItemDamageFunction.apply(float dealtDamage)`.
    pub fn apply(self, dealt_damage: f32) -> i32 {
        if dealt_damage < self.threshold {
            0
        } else {
            mth::floor(self.base + self.factor * dealt_damage)
        }
    }
}

impl Default for ItemDamageFunction {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// `BlocksAttacks.ItemDamageFunction.CODEC`.
pub fn item_damage_function_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<ItemDamageFunction, Ops>> {
    codec_from_fns(
        "item_damage_function",
        |v: &ItemDamageFunction, ops: &Ops| {
            if let Err(e) = v.validate() {
                return DataResult::error(e);
            }
            let entries = vec![
                rivet_serialization::Pair::of(
                    ops.create_string("threshold".to_string()),
                    ops.create_float(v.threshold),
                ),
                rivet_serialization::Pair::of(
                    ops.create_string("base".to_string()),
                    ops.create_float(v.base),
                ),
                rivet_serialization::Pair::of(
                    ops.create_string("factor".to_string()),
                    ops.create_float(v.factor),
                ),
            ];
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("ItemDamageFunction must be a map");
            };
            let Some(t_val) = get_map_field(ops, map.as_ref(), "threshold") else {
                return DataResult::error("ItemDamageFunction missing required field 'threshold'");
            };
            let Some(threshold) = ops
                .get_number_value(&t_val)
                .result()
                .map(|n| n.float_value())
            else {
                return DataResult::error("ItemDamageFunction.threshold must be a number");
            };
            if let Some(err) = validate_non_negative_float(threshold, "threshold").error_ref() {
                return DataResult::error(err.message().to_string());
            }
            let Some(b_val) = get_map_field(ops, map.as_ref(), "base") else {
                return DataResult::error("ItemDamageFunction missing required field 'base'");
            };
            let Some(base) = ops
                .get_number_value(&b_val)
                .result()
                .map(|n| n.float_value())
            else {
                return DataResult::error("ItemDamageFunction.base must be a number");
            };
            let Some(f_val) = get_map_field(ops, map.as_ref(), "factor") else {
                return DataResult::error("ItemDamageFunction missing required field 'factor'");
            };
            let Some(factor) = ops
                .get_number_value(&f_val)
                .result()
                .map(|n| n.float_value())
            else {
                return DataResult::error("ItemDamageFunction.factor must be a number");
            };
            DataResult::success(ItemDamageFunction {
                threshold,
                base,
                factor,
            })
        },
    )
}

/// Port of `net.minecraft.world.item.component.BlocksAttacks`.
#[derive(Clone, Debug, PartialEq)]
pub struct BlocksAttacks {
    pub block_delay_seconds: f32,
    pub disable_cooldown_scale: f32,
    pub damage_reductions: Vec<DamageReduction>,
    pub item_damage: ItemDamageFunction,
    pub bypassed_by: Option<HolderSetRef>,
    pub block_sound: Option<HolderRef<SoundEvent>>,
    pub disabled_sound: Option<HolderRef<SoundEvent>>,
}

impl BlocksAttacks {
    /// The codec-level field validators (Paper's `Codec.validate` /
    /// range codecs), which fail on encode as well as decode.
    pub fn validate(&self) -> Result<(), String> {
        result_of(validate_non_negative_float(
            self.block_delay_seconds,
            "block_delay_seconds",
        ))?;
        result_of(validate_non_negative_float(
            self.disable_cooldown_scale,
            "disable_cooldown_scale",
        ))?;
        Ok(())
    }

    pub fn default_value() -> Self {
        Self {
            block_delay_seconds: 0.0,
            disable_cooldown_scale: 1.0,
            damage_reductions: vec![DamageReduction::default_reduction()],
            item_damage: ItemDamageFunction::DEFAULT,
            bypassed_by: None,
            block_sound: None,
            disabled_sound: None,
        }
    }

    /// `BlocksAttacks.blockDelayTicks() = Math.round(blockDelaySeconds * 20.0F)`.
    pub fn block_delay_ticks(&self) -> i32 {
        java_round_f32(self.block_delay_seconds * 20.0_f32)
    }
}

impl Default for BlocksAttacks {
    fn default() -> Self {
        Self::default_value()
    }
}

/// `BlocksAttacks.CODEC`.
pub fn blocks_attacks_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BlocksAttacks, Ops>> {
    let red_codec = damage_reduction_codec::<Ops>();
    let dmg_codec = item_damage_function_codec::<Ops>();
    let enc_red = Arc::clone(&red_codec);
    let dec_red = red_codec;
    let enc_dmg = Arc::clone(&dmg_codec);
    let dec_dmg = dmg_codec;
    let hs_codec = holder_set_ref_codec::<Ops>();
    let enc_hs = Arc::clone(&hs_codec);
    let dec_hs = hs_codec;
    codec_from_fns(
        "blocks_attacks",
        move |v: &BlocksAttacks, ops: &Ops| {
            if let Err(e) = v.validate() {
                return DataResult::error(e);
            }
            let mut entries = Vec::new();
            if v.block_delay_seconds != 0.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("block_delay_seconds".to_string()),
                    ops.create_float(v.block_delay_seconds),
                ));
            }
            if v.disable_cooldown_scale != 1.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("disable_cooldown_scale".to_string()),
                    ops.create_float(v.disable_cooldown_scale),
                ));
            }
            let default_reds = vec![DamageReduction::default_reduction()];
            if v.damage_reductions != default_reds {
                let mut list = Vec::with_capacity(v.damage_reductions.len());
                for r in &v.damage_reductions {
                    match enc_red.encode_start(ops, r).result().cloned() {
                        Some(ev) => list.push(ev),
                        None => {
                            return DataResult::error(
                                "Failed to encode BlocksAttacks.damage_reductions",
                            );
                        }
                    }
                }
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("damage_reductions".to_string()),
                    ops.create_list(list),
                ));
            }
            if v.item_damage != ItemDamageFunction::DEFAULT {
                match enc_dmg.encode_start(ops, &v.item_damage).result().cloned() {
                    Some(ev) => entries.push(rivet_serialization::Pair::of(
                        ops.create_string("item_damage".to_string()),
                        ev,
                    )),
                    None => return DataResult::error("Failed to encode BlocksAttacks.item_damage"),
                }
            }
            if let Some(set) = &v.bypassed_by {
                match enc_hs.encode_start(ops, set).result().cloned() {
                    Some(ev) => entries.push(rivet_serialization::Pair::of(
                        ops.create_string("bypassed_by".to_string()),
                        ev,
                    )),
                    None => return DataResult::error("Failed to encode BlocksAttacks.bypassed_by"),
                }
            }
            if let Some(s) = &v.block_sound {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("block_sound".to_string()),
                    encode_sound(s, ops),
                ));
            }
            if let Some(s) = &v.disabled_sound {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("disabled_sound".to_string()),
                    encode_sound(s, ops),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("BlocksAttacks must be a map");
            };
            let block_delay_seconds = if let Some(val) =
                get_map_field(ops, map.as_ref(), "block_delay_seconds")
            {
                let n_res = ops.get_number_value(&val);
                let Some(n) = n_res.result() else {
                    return DataResult::error("BlocksAttacks.block_delay_seconds must be a number");
                };
                let f = n.float_value();
                if let Some(err) = validate_non_negative_float(f, "block_delay_seconds").error_ref()
                {
                    return DataResult::error(err.message().to_string());
                }
                f
            } else {
                0.0
            };
            let disable_cooldown_scale =
                if let Some(val) = get_map_field(ops, map.as_ref(), "disable_cooldown_scale") {
                    let n_res = ops.get_number_value(&val);
                    let Some(n) = n_res.result() else {
                        return DataResult::error(
                            "BlocksAttacks.disable_cooldown_scale must be a number",
                        );
                    };
                    let f = n.float_value();
                    if let Some(err) =
                        validate_non_negative_float(f, "disable_cooldown_scale").error_ref()
                    {
                        return DataResult::error(err.message().to_string());
                    }
                    f
                } else {
                    1.0
                };
            let damage_reductions =
                if let Some(val) = get_map_field(ops, map.as_ref(), "damage_reductions") {
                    let Some(items) = ops.get_stream(&val).result().cloned() else {
                        return DataResult::error("BlocksAttacks.damage_reductions must be a list");
                    };
                    let mut out = Vec::with_capacity(items.len());
                    for item in &items {
                        match dec_red.parse(ops, item).result().cloned() {
                            Some(r) => out.push(r),
                            None => {
                                return DataResult::error(
                                    "Invalid entry in BlocksAttacks.damage_reductions",
                                );
                            }
                        }
                    }
                    out
                } else {
                    vec![DamageReduction::default_reduction()]
                };
            let item_damage = if let Some(val) = get_map_field(ops, map.as_ref(), "item_damage") {
                match dec_dmg.parse(ops, &val).result().copied() {
                    Some(d) => d,
                    None => return DataResult::error("Invalid BlocksAttacks.item_damage"),
                }
            } else {
                ItemDamageFunction::DEFAULT
            };
            let bypassed_by = if let Some(val) = get_map_field(ops, map.as_ref(), "bypassed_by") {
                let hs_res = dec_hs.parse(ops, &val);
                match hs_res.result().cloned() {
                    Some(set) => Some(set),
                    None => {
                        return DataResult::error(
                            hs_res
                                .error_ref()
                                .map(|e| e.message().to_string())
                                .unwrap_or_else(|| "Invalid BlocksAttacks.bypassed_by".to_string()),
                        );
                    }
                }
            } else {
                None
            };
            let block_sound = if let Some(val) = get_map_field(ops, map.as_ref(), "block_sound") {
                match decode_sound(ops, &val).result().cloned() {
                    Some(id) => Some(id),
                    None => return DataResult::error("Invalid BlocksAttacks.block_sound"),
                }
            } else {
                None
            };
            let disabled_sound =
                if let Some(val) = get_map_field(ops, map.as_ref(), "disabled_sound") {
                    match decode_sound(ops, &val).result().cloned() {
                        Some(id) => Some(id),
                        None => return DataResult::error("Invalid BlocksAttacks.disabled_sound"),
                    }
                } else {
                    None
                };
            DataResult::success(BlocksAttacks {
                block_delay_seconds,
                disable_cooldown_scale,
                damage_reductions,
                item_damage,
                bypassed_by,
                block_sound,
                disabled_sound,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// PiercingWeapon
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.PiercingWeapon`.
#[derive(Clone, Debug, PartialEq)]
pub struct PiercingWeapon {
    pub deals_knockback: bool,
    pub dismounts: bool,
    pub sound: Option<HolderRef<SoundEvent>>,
    pub hit_sound: Option<HolderRef<SoundEvent>>,
}

impl Default for PiercingWeapon {
    fn default() -> Self {
        Self {
            deals_knockback: true,
            dismounts: false,
            sound: None,
            hit_sound: None,
        }
    }
}

/// `PiercingWeapon.CODEC`.
pub fn piercing_weapon_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<PiercingWeapon, Ops>> {
    codec_from_fns(
        "piercing_weapon",
        |v: &PiercingWeapon, ops: &Ops| {
            let mut entries = Vec::new();
            if !v.deals_knockback {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("deals_knockback".to_string()),
                    ops.create_boolean(false),
                ));
            }
            if v.dismounts {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("dismounts".to_string()),
                    ops.create_boolean(true),
                ));
            }
            if let Some(s) = &v.sound {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("sound".to_string()),
                    encode_sound(s, ops),
                ));
            }
            if let Some(s) = &v.hit_sound {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("hit_sound".to_string()),
                    encode_sound(s, ops),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("PiercingWeapon must be a map");
            };
            let deals_knockback =
                if let Some(val) = get_map_field(ops, map.as_ref(), "deals_knockback") {
                    match ops.get_boolean_value(&val).result().copied() {
                        Some(b) => b,
                        None => {
                            return DataResult::error(
                                "PiercingWeapon.deals_knockback must be a boolean",
                            );
                        }
                    }
                } else {
                    true
                };
            let dismounts = if let Some(val) = get_map_field(ops, map.as_ref(), "dismounts") {
                match ops.get_boolean_value(&val).result().copied() {
                    Some(b) => b,
                    None => return DataResult::error("PiercingWeapon.dismounts must be a boolean"),
                }
            } else {
                false
            };
            let sound = if let Some(val) = get_map_field(ops, map.as_ref(), "sound") {
                match decode_sound(ops, &val).result().cloned() {
                    Some(id) => Some(id),
                    None => return DataResult::error("Invalid PiercingWeapon.sound"),
                }
            } else {
                None
            };
            let hit_sound = if let Some(val) = get_map_field(ops, map.as_ref(), "hit_sound") {
                match decode_sound(ops, &val).result().cloned() {
                    Some(id) => Some(id),
                    None => return DataResult::error("Invalid PiercingWeapon.hit_sound"),
                }
            } else {
                None
            };
            DataResult::success(PiercingWeapon {
                deals_knockback,
                dismounts,
                sound,
                hit_sound,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// KineticWeapon & KineticWeaponCondition
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.KineticWeapon.Condition`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KineticWeaponCondition {
    pub max_duration_ticks: i32,
    pub min_speed: f32,
    pub min_relative_speed: f32,
}

impl KineticWeaponCondition {
    /// The codec-level field validators (Paper's `Codec.validate` /
    /// range codecs), which fail on encode as well as decode.
    pub fn validate(&self) -> Result<(), String> {
        result_of(validate_non_negative_int(
            self.max_duration_ticks,
            "max_duration_ticks",
        ))?;
        Ok(())
    }

    pub fn try_new(
        max_duration_ticks: i32,
        min_speed: f32,
        min_relative_speed: f32,
    ) -> Result<Self, String> {
        let value = Self {
            max_duration_ticks,
            min_speed,
            min_relative_speed,
        };
        value.validate()?;
        Ok(value)
    }

    /// Exact port of `KineticWeapon.Condition.test(int, double, double, double)`.
    pub fn test(
        self,
        ticks_used: i32,
        attacker_speed: f64,
        relative_speed: f64,
        entity_factor: f64,
    ) -> bool {
        ticks_used <= self.max_duration_ticks
            && attacker_speed >= (self.min_speed as f64) * entity_factor
            && relative_speed >= (self.min_relative_speed as f64) * entity_factor
    }
}

/// `KineticWeapon.Condition.CODEC`.
pub fn kinetic_weapon_condition_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<KineticWeaponCondition, Ops>> {
    codec_from_fns(
        "kinetic_weapon_condition",
        |v: &KineticWeaponCondition, ops: &Ops| {
            if let Err(e) = v.validate() {
                return DataResult::error(e);
            }
            let mut entries = vec![rivet_serialization::Pair::of(
                ops.create_string("max_duration_ticks".to_string()),
                ops.create_int(v.max_duration_ticks),
            )];
            if v.min_speed != 0.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("min_speed".to_string()),
                    ops.create_float(v.min_speed),
                ));
            }
            if v.min_relative_speed != 0.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("min_relative_speed".to_string()),
                    ops.create_float(v.min_relative_speed),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("KineticWeapon.Condition must be a map");
            };
            let Some(d_val) = get_map_field(ops, map.as_ref(), "max_duration_ticks") else {
                return DataResult::error(
                    "KineticWeapon.Condition missing required field 'max_duration_ticks'",
                );
            };
            let Some(max_duration_ticks) =
                ops.get_number_value(&d_val).result().map(|n| n.int_value())
            else {
                return DataResult::error(
                    "KineticWeapon.Condition.max_duration_ticks must be a number",
                );
            };
            if max_duration_ticks < 0 {
                return DataResult::error(format!(
                    "Value must be non-negative: {max_duration_ticks}"
                ));
            }
            let min_speed = if let Some(val) = get_map_field(ops, map.as_ref(), "min_speed") {
                match ops.get_number_value(&val).result().map(|n| n.float_value()) {
                    Some(f) => f,
                    None => {
                        return DataResult::error(
                            "KineticWeapon.Condition.min_speed must be a number",
                        );
                    }
                }
            } else {
                0.0
            };
            let min_relative_speed =
                if let Some(val) = get_map_field(ops, map.as_ref(), "min_relative_speed") {
                    match ops.get_number_value(&val).result().map(|n| n.float_value()) {
                        Some(f) => f,
                        None => {
                            return DataResult::error(
                                "KineticWeapon.Condition.min_relative_speed must be a number",
                            );
                        }
                    }
                } else {
                    0.0
                };
            DataResult::success(KineticWeaponCondition {
                max_duration_ticks,
                min_speed,
                min_relative_speed,
            })
        },
    )
}

/// Port of `net.minecraft.world.item.component.KineticWeapon`.
#[derive(Clone, Debug, PartialEq)]
pub struct KineticWeapon {
    pub contact_cooldown_ticks: i32,
    pub delay_ticks: i32,
    pub dismount_conditions: Option<KineticWeaponCondition>,
    pub knockback_conditions: Option<KineticWeaponCondition>,
    pub damage_conditions: Option<KineticWeaponCondition>,
    pub forward_movement: f32,
    pub damage_multiplier: f32,
    pub sound: Option<HolderRef<SoundEvent>>,
    pub hit_sound: Option<HolderRef<SoundEvent>>,
}

impl KineticWeapon {
    /// The codec-level field validators (Paper's `Codec.validate` /
    /// range codecs), which fail on encode as well as decode.
    pub fn validate(&self) -> Result<(), String> {
        result_of(validate_non_negative_int(
            self.contact_cooldown_ticks,
            "contact_cooldown_ticks",
        ))?;
        result_of(validate_non_negative_int(self.delay_ticks, "delay_ticks"))?;
        Ok(())
    }
}

impl Default for KineticWeapon {
    fn default() -> Self {
        Self {
            contact_cooldown_ticks: 10,
            delay_ticks: 0,
            dismount_conditions: None,
            knockback_conditions: None,
            damage_conditions: None,
            forward_movement: 0.0,
            damage_multiplier: 1.0,
            sound: None,
            hit_sound: None,
        }
    }
}

/// `KineticWeapon.CODEC`.
pub fn kinetic_weapon_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<KineticWeapon, Ops>> {
    let cond_codec = kinetic_weapon_condition_codec::<Ops>();
    let enc_cond = Arc::clone(&cond_codec);
    let dec_cond = cond_codec;
    codec_from_fns(
        "kinetic_weapon",
        move |v: &KineticWeapon, ops: &Ops| {
            if let Err(e) = v.validate() {
                return DataResult::error(e);
            }
            let mut entries = Vec::new();
            if v.contact_cooldown_ticks != 10 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("contact_cooldown_ticks".to_string()),
                    ops.create_int(v.contact_cooldown_ticks),
                ));
            }
            if v.delay_ticks != 0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("delay_ticks".to_string()),
                    ops.create_int(v.delay_ticks),
                ));
            }
            for (name, cond) in [
                ("dismount_conditions", v.dismount_conditions),
                ("knockback_conditions", v.knockback_conditions),
                ("damage_conditions", v.damage_conditions),
            ] {
                if let Some(c) = cond {
                    match enc_cond.encode_start(ops, &c).result().cloned() {
                        Some(ev) => entries.push(rivet_serialization::Pair::of(
                            ops.create_string(name.to_string()),
                            ev,
                        )),
                        None => {
                            return DataResult::error(format!(
                                "Failed to encode KineticWeapon.{name}"
                            ));
                        }
                    }
                }
            }
            if v.forward_movement != 0.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("forward_movement".to_string()),
                    ops.create_float(v.forward_movement),
                ));
            }
            if v.damage_multiplier != 1.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("damage_multiplier".to_string()),
                    ops.create_float(v.damage_multiplier),
                ));
            }
            if let Some(s) = &v.sound {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("sound".to_string()),
                    encode_sound(s, ops),
                ));
            }
            if let Some(s) = &v.hit_sound {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("hit_sound".to_string()),
                    encode_sound(s, ops),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("KineticWeapon must be a map");
            };
            let contact_cooldown_ticks =
                if let Some(val) = get_map_field(ops, map.as_ref(), "contact_cooldown_ticks") {
                    let n_res = ops.get_number_value(&val);
                    let Some(n) = n_res.result() else {
                        return DataResult::error(
                            "KineticWeapon.contact_cooldown_ticks must be a number",
                        );
                    };
                    let v = n.int_value();
                    if v < 0 {
                        return DataResult::error(format!("Value must be non-negative: {v}"));
                    }
                    v
                } else {
                    10
                };
            let delay_ticks = if let Some(val) = get_map_field(ops, map.as_ref(), "delay_ticks") {
                let n_res = ops.get_number_value(&val);
                let Some(n) = n_res.result() else {
                    return DataResult::error("KineticWeapon.delay_ticks must be a number");
                };
                let v = n.int_value();
                if v < 0 {
                    return DataResult::error(format!("Value must be non-negative: {v}"));
                }
                v
            } else {
                0
            };
            let parse_cond = |name: &str| -> Result<Option<KineticWeaponCondition>, String> {
                match get_map_field(ops, map.as_ref(), name) {
                    Some(val) => dec_cond
                        .parse(ops, &val)
                        .result()
                        .copied()
                        .map(Some)
                        .ok_or_else(|| format!("Invalid KineticWeapon.{name}")),
                    None => Ok(None),
                }
            };
            let dismount_conditions = match parse_cond("dismount_conditions") {
                Ok(v) => v,
                Err(e) => return DataResult::error(e),
            };
            let knockback_conditions = match parse_cond("knockback_conditions") {
                Ok(v) => v,
                Err(e) => return DataResult::error(e),
            };
            let damage_conditions = match parse_cond("damage_conditions") {
                Ok(v) => v,
                Err(e) => return DataResult::error(e),
            };
            let forward_movement =
                if let Some(val) = get_map_field(ops, map.as_ref(), "forward_movement") {
                    match ops.get_number_value(&val).result().map(|n| n.float_value()) {
                        Some(f) => f,
                        None => {
                            return DataResult::error(
                                "KineticWeapon.forward_movement must be a number",
                            );
                        }
                    }
                } else {
                    0.0
                };
            let damage_multiplier =
                if let Some(val) = get_map_field(ops, map.as_ref(), "damage_multiplier") {
                    match ops.get_number_value(&val).result().map(|n| n.float_value()) {
                        Some(f) => f,
                        None => {
                            return DataResult::error(
                                "KineticWeapon.damage_multiplier must be a number",
                            );
                        }
                    }
                } else {
                    1.0
                };
            let sound = if let Some(val) = get_map_field(ops, map.as_ref(), "sound") {
                match decode_sound(ops, &val).result().cloned() {
                    Some(id) => Some(id),
                    None => return DataResult::error("Invalid KineticWeapon.sound"),
                }
            } else {
                None
            };
            let hit_sound = if let Some(val) = get_map_field(ops, map.as_ref(), "hit_sound") {
                match decode_sound(ops, &val).result().cloned() {
                    Some(id) => Some(id),
                    None => return DataResult::error("Invalid KineticWeapon.hit_sound"),
                }
            } else {
                None
            };
            DataResult::success(KineticWeapon {
                contact_cooldown_ticks,
                delay_ticks,
                dismount_conditions,
                knockback_conditions,
                damage_conditions,
                forward_movement,
                damage_multiplier,
                sound,
                hit_sound,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Tool & ToolRule
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.Tool.Rule`.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolRule {
    pub blocks: HolderSetRef,
    pub speed: Option<f32>,
    pub correct_for_drops: Option<bool>,
}

impl ToolRule {
    /// The codec-level field validators (Paper's `Codec.validate` /
    /// range codecs), which fail on encode as well as decode.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(speed) = self.speed {
            result_of(validate_positive_float(speed, "speed"))?;
        }
        Ok(())
    }

    pub fn mines_and_drops(blocks: HolderSetRef, speed: f32) -> Self {
        Self {
            blocks,
            speed: Some(speed),
            correct_for_drops: Some(true),
        }
    }

    pub fn denies_drops(blocks: HolderSetRef) -> Self {
        Self {
            blocks,
            speed: None,
            correct_for_drops: Some(false),
        }
    }

    pub fn override_speed(blocks: HolderSetRef, speed: f32) -> Self {
        Self {
            blocks,
            speed: Some(speed),
            correct_for_drops: None,
        }
    }
}

/// `Tool.Rule.CODEC`.
pub fn tool_rule_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ToolRule, Ops>> {
    let hs = holder_set_ref_codec::<Ops>();
    let enc_hs = Arc::clone(&hs);
    let dec_hs = hs;
    codec_from_fns(
        "tool_rule",
        move |v: &ToolRule, ops: &Ops| {
            if let Err(e) = v.validate() {
                return DataResult::error(e);
            }
            let Some(blocks_val) = enc_hs.encode_start(ops, &v.blocks).result().cloned() else {
                return DataResult::error("Failed to encode Tool.Rule.blocks");
            };
            let mut entries = vec![rivet_serialization::Pair::of(
                ops.create_string("blocks".to_string()),
                blocks_val,
            )];
            if let Some(speed) = v.speed {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("speed".to_string()),
                    ops.create_float(speed),
                ));
            }
            if let Some(correct) = v.correct_for_drops {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("correct_for_drops".to_string()),
                    ops.create_boolean(correct),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("Tool.Rule must be a map");
            };
            let Some(blocks_in) = get_map_field(ops, map.as_ref(), "blocks") else {
                return DataResult::error("Tool.Rule missing required field 'blocks'");
            };
            let Some(blocks) = dec_hs.parse(ops, &blocks_in).result().cloned() else {
                return DataResult::error("Invalid Tool.Rule.blocks");
            };
            let speed = if let Some(val) = get_map_field(ops, map.as_ref(), "speed") {
                let n_res = ops.get_number_value(&val);
                let Some(n) = n_res.result() else {
                    return DataResult::error("Tool.Rule.speed must be a number");
                };
                let f = n.float_value();
                if let Some(err) = validate_positive_float(f, "speed").error_ref() {
                    return DataResult::error(err.message().to_string());
                }
                Some(f)
            } else {
                None
            };
            let correct_for_drops = if let Some(val) =
                get_map_field(ops, map.as_ref(), "correct_for_drops")
            {
                match ops.get_boolean_value(&val).result().copied() {
                    Some(b) => Some(b),
                    None => {
                        return DataResult::error("Tool.Rule.correct_for_drops must be a boolean");
                    }
                }
            } else {
                None
            };
            DataResult::success(ToolRule {
                blocks,
                speed,
                correct_for_drops,
            })
        },
    )
}

/// Port of `net.minecraft.world.item.component.Tool`.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub rules: Vec<ToolRule>,
    pub default_mining_speed: f32,
    pub damage_per_block: i32,
    pub can_destroy_blocks_in_creative: bool,
}

impl Tool {
    /// The codec-level field validators (Paper's `Codec.validate` /
    /// range codecs), which fail on encode as well as decode.
    pub fn validate(&self) -> Result<(), String> {
        result_of(validate_non_negative_int(
            self.damage_per_block,
            "damage_per_block",
        ))?;
        Ok(())
    }

    pub fn new(
        rules: Vec<ToolRule>,
        default_mining_speed: f32,
        damage_per_block: i32,
        can_destroy_blocks_in_creative: bool,
    ) -> Self {
        Self {
            rules,
            default_mining_speed,
            damage_per_block,
            can_destroy_blocks_in_creative,
        }
    }

    /// `Tool.getMiningSpeed(BlockState)` evaluated against a block identifier
    /// for direct holder sets (first matching rule with a speed wins).
    pub fn mining_speed_for_direct(&self, block_id: &Identifier) -> f32 {
        for rule in &self.rules {
            if let Some(speed) = rule.speed
                && rule.blocks.contains_direct(block_id)
            {
                return speed;
            }
        }
        self.default_mining_speed
    }

    /// `Tool.isCorrectForDrops(BlockState)` evaluated against a block identifier
    /// for direct holder sets (first matching rule with `correct_for_drops` wins).
    pub fn is_correct_for_drops_direct(&self, block_id: &Identifier) -> bool {
        for rule in &self.rules {
            if let Some(correct) = rule.correct_for_drops
                && rule.blocks.contains_direct(block_id)
            {
                return correct;
            }
        }
        false
    }
}

/// `Tool.CODEC`.
pub fn tool_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Tool, Ops>> {
    let rc = tool_rule_codec::<Ops>();
    let enc_r = Arc::clone(&rc);
    let dec_r = rc;
    codec_from_fns(
        "tool",
        move |v: &Tool, ops: &Ops| {
            if let Err(e) = v.validate() {
                return DataResult::error(e);
            }
            let mut rules_out = Vec::with_capacity(v.rules.len());
            for rule in &v.rules {
                match enc_r.encode_start(ops, rule).result().cloned() {
                    Some(ev) => rules_out.push(ev),
                    None => return DataResult::error("Failed to encode Tool.rules"),
                }
            }
            let mut entries = vec![rivet_serialization::Pair::of(
                ops.create_string("rules".to_string()),
                ops.create_list(rules_out),
            )];
            if v.default_mining_speed != 1.0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("default_mining_speed".to_string()),
                    ops.create_float(v.default_mining_speed),
                ));
            }
            if v.damage_per_block != 1 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("damage_per_block".to_string()),
                    ops.create_int(v.damage_per_block),
                ));
            }
            if !v.can_destroy_blocks_in_creative {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("can_destroy_blocks_in_creative".to_string()),
                    ops.create_boolean(false),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("Tool must be a map");
            };
            let Some(rules_in) = get_map_field(ops, map.as_ref(), "rules") else {
                return DataResult::error("Tool missing required field 'rules'");
            };
            let Some(items) = ops.get_stream(&rules_in).result().cloned() else {
                return DataResult::error("Tool.rules must be a list");
            };
            let mut rules = Vec::with_capacity(items.len());
            for item in &items {
                match dec_r.parse(ops, item).result().cloned() {
                    Some(r) => rules.push(r),
                    None => return DataResult::error("Invalid entry in Tool.rules"),
                }
            }
            let default_mining_speed = if let Some(val) =
                get_map_field(ops, map.as_ref(), "default_mining_speed")
            {
                match ops.get_number_value(&val).result().map(|n| n.float_value()) {
                    Some(f) => f,
                    None => return DataResult::error("Tool.default_mining_speed must be a number"),
                }
            } else {
                1.0
            };
            let damage_per_block =
                if let Some(val) = get_map_field(ops, map.as_ref(), "damage_per_block") {
                    let n_res = ops.get_number_value(&val);
                    let Some(n) = n_res.result() else {
                        return DataResult::error("Tool.damage_per_block must be a number");
                    };
                    let d = n.int_value();
                    if d < 0 {
                        return DataResult::error(format!("Value must be non-negative: {d}"));
                    }
                    d
                } else {
                    1
                };
            let can_destroy_blocks_in_creative = if let Some(val) =
                get_map_field(ops, map.as_ref(), "can_destroy_blocks_in_creative")
            {
                match ops.get_boolean_value(&val).result().copied() {
                    Some(b) => b,
                    None => {
                        return DataResult::error(
                            "Tool.can_destroy_blocks_in_creative must be a boolean",
                        );
                    }
                }
            } else {
                true
            };
            DataResult::success(Tool {
                rules,
                default_mining_speed,
                damage_per_block,
                can_destroy_blocks_in_creative,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// Enchantable & Repairable
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.enchantment.Enchantable`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Enchantable {
    pub value: i32,
}

impl Enchantable {
    pub fn try_new(value: i32) -> Result<Self, String> {
        if value <= 0 {
            return Err(format!(
                "Enchantment value must be positive, but was {value}"
            ));
        }
        Ok(Self { value })
    }
}

/// `Enchantable.CODEC`.
pub fn enchantable_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Enchantable, Ops>> {
    codec_from_fns(
        "enchantable",
        |v: &Enchantable, ops: &Ops| {
            if let Some(err) = validate_int_range(v.value, 1, i32::MAX, "value").error_ref() {
                return DataResult::error(err.message().to_string());
            }
            DataResult::success(ops.create_map(vec![rivet_serialization::Pair::of(
                ops.create_string("value".to_string()),
                ops.create_int(v.value),
            )]))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("Enchantable must be a map");
            };
            let Some(val) = get_map_field(ops, map.as_ref(), "value") else {
                return DataResult::error("Enchantable missing required field 'value'");
            };
            let n_res = ops.get_number_value(&val);
            let Some(n) = n_res.result() else {
                return DataResult::error("Enchantable.value must be a number");
            };
            let v = n.int_value();
            match Enchantable::try_new(v) {
                Ok(e) => DataResult::success(e),
                Err(msg) => DataResult::error(msg),
            }
        },
    )
}

/// Port of `net.minecraft.world.item.enchantment.Repairable`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Repairable {
    pub items: HolderSetRef,
}

impl Repairable {
    pub fn new(items: HolderSetRef) -> Self {
        Self { items }
    }
}

/// `Repairable.CODEC`.
pub fn repairable_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Repairable, Ops>> {
    let hs = holder_set_ref_codec::<Ops>();
    let enc_hs = Arc::clone(&hs);
    let dec_hs = hs;
    codec_from_fns(
        "repairable",
        move |v: &Repairable, ops: &Ops| match enc_hs.encode_start(ops, &v.items).result().cloned()
        {
            Some(ev) => DataResult::success(ops.create_map(vec![rivet_serialization::Pair::of(
                ops.create_string("items".to_string()),
                ev,
            )])),
            None => DataResult::error("Failed to encode Repairable.items"),
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("Repairable must be a map");
            };
            let Some(val) = get_map_field(ops, map.as_ref(), "items") else {
                return DataResult::error("Repairable missing required field 'items'");
            };
            dec_hs
                .parse(ops, &val)
                .map_owned(|items| Repairable { items })
        },
    )
}
