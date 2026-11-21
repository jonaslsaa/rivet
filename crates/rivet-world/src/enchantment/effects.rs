//! Port of `net.minecraft.world.item.enchantment.effects.*`,
//! `ConditionalEffect`, `TargetedConditionalEffect`, and
//! `EnchantmentEffectComponents` (MC 26.2).

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::tag::Tag;
use rivet_protocol::game::clientbound_update_attributes_packet::{AttributeModifier, Operation};
use rivet_registry::Identifier;
use rivet_registry::generated::tags::{DAMAGE_TYPE_TAG_BY_NAME, ENTITY_TYPE_TAG_BY_NAME};
use rivet_util::random::RandomSource;

use crate::enchantment::level_based_value::{
    LevelBasedValue, LevelBasedValueError, get_string, strip_minecraft_prefix, tag_as_f32,
};
use crate::enchantment::{EnchantmentTarget, EquipmentSlot};

/// Errors returned when decoding enchantment effects from NBT.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EffectDecodeError {
    #[error("level-based value decode error: {0}")]
    LevelBasedValue(#[from] LevelBasedValueError),
    #[error("missing required field '{0}' in enchantment effect")]
    MissingField(&'static str),
    #[error("invalid field type for '{0}' in enchantment effect")]
    InvalidField(&'static str),
    #[error("unknown enchantment value effect type '{0}'")]
    UnknownValueEffect(String),
    #[error("unknown attribute operation '{0}'")]
    UnknownOperation(String),
    #[error("unknown enchantment target '{0}'")]
    UnknownTarget(String),
    #[error("invalid identifier '{0}'")]
    InvalidIdentifier(String),
}

/// `net.minecraft.world.item.enchantment.effects.EnchantmentValueEffect`.
#[derive(Debug, Clone, PartialEq)]
pub enum EnchantmentValueEffect {
    /// `AddValue` — `input_value + value.calculate(level)`.
    Add { value: LevelBasedValue },
    /// `MultiplyValue` — `input_value * factor.calculate(level)`.
    Multiply { factor: LevelBasedValue },
    /// `SetValue` — `value.calculate(level)`.
    Set { value: LevelBasedValue },
    /// `ScaleExponentially` — `(float) (input_value * Math.pow(base, exponent))`.
    ScaleExponentially {
        base: LevelBasedValue,
        exponent: LevelBasedValue,
    },
    /// `RemoveBinomial` — subtracts a `Binomial(n, p)` draw from `input_value`.
    RemoveBinomial { chance: LevelBasedValue },
    /// `AllOf.ValueEffects` — applies each effect in list order.
    AllOf {
        effects: Vec<EnchantmentValueEffect>,
    },
}

impl EnchantmentValueEffect {
    /// `EnchantmentValueEffect.process(int enchantmentLevel, RandomSource random, float inputValue)`.
    pub fn process<R: RandomSource>(
        &self,
        enchantment_level: i32,
        random: &mut R,
        input_value: f32,
    ) -> f32 {
        match self {
            Self::Add { value } => input_value + value.calculate(enchantment_level),
            Self::Multiply { factor } => input_value * factor.calculate(enchantment_level),
            Self::Set { value } => value.calculate(enchantment_level),
            Self::ScaleExponentially { base, exponent } => {
                let b = f64::from(base.calculate(enchantment_level));
                let e = f64::from(exponent.calculate(enchantment_level));
                (f64::from(input_value) * b.powf(e)) as f32
            }
            Self::RemoveBinomial { chance } => {
                process_remove_binomial(chance.calculate(enchantment_level), random, input_value)
            }
            Self::AllOf { effects } => {
                let mut current = input_value;
                for effect in effects {
                    current = effect.process(enchantment_level, random, current);
                }
                current
            }
        }
    }

    /// Decodes an [`EnchantmentValueEffect`] from an NBT [`CompoundTag`].
    pub fn from_nbt(compound: &CompoundTag) -> Result<Self, EffectDecodeError> {
        let raw_type =
            get_string(compound, "type").ok_or(EffectDecodeError::MissingField("type"))?;
        match strip_minecraft_prefix(raw_type) {
            "add" => {
                let tag = compound
                    .tags
                    .get("value")
                    .ok_or(EffectDecodeError::MissingField("value"))?;
                Ok(Self::Add {
                    value: LevelBasedValue::from_nbt(tag)?,
                })
            }
            "multiply" => {
                let tag = compound
                    .tags
                    .get("factor")
                    .ok_or(EffectDecodeError::MissingField("factor"))?;
                Ok(Self::Multiply {
                    factor: LevelBasedValue::from_nbt(tag)?,
                })
            }
            "set" => {
                let tag = compound
                    .tags
                    .get("value")
                    .ok_or(EffectDecodeError::MissingField("value"))?;
                Ok(Self::Set {
                    value: LevelBasedValue::from_nbt(tag)?,
                })
            }
            "exponential" => {
                let base_tag = compound
                    .tags
                    .get("base")
                    .ok_or(EffectDecodeError::MissingField("base"))?;
                let exp_tag = compound
                    .tags
                    .get("exponent")
                    .ok_or(EffectDecodeError::MissingField("exponent"))?;
                Ok(Self::ScaleExponentially {
                    base: LevelBasedValue::from_nbt(base_tag)?,
                    exponent: LevelBasedValue::from_nbt(exp_tag)?,
                })
            }
            "remove_binomial" => {
                let tag = compound
                    .tags
                    .get("chance")
                    .ok_or(EffectDecodeError::MissingField("chance"))?;
                Ok(Self::RemoveBinomial {
                    chance: LevelBasedValue::from_nbt(tag)?,
                })
            }
            "all_of" => {
                let list = match compound.tags.get("effects") {
                    Some(Tag::List(l)) => l,
                    Some(_) => return Err(EffectDecodeError::InvalidField("effects")),
                    None => return Err(EffectDecodeError::MissingField("effects")),
                };
                let mut effects = Vec::with_capacity(list.list.len());
                for item in &list.list {
                    let Tag::Compound(c) = item else {
                        return Err(EffectDecodeError::InvalidField("effects"));
                    };
                    effects.push(Self::from_nbt(c)?);
                }
                Ok(Self::AllOf { effects })
            }
            other => Err(EffectDecodeError::UnknownValueEffect(other.to_owned())),
        }
    }
}

/// Exact port of `RemoveBinomial.process(int, RandomSource, float)` in Paper 26.2.
///
/// Uses the exact Paper threshold (`!(n <= 128.0F) && !(n * p < 20.0F) && !(n * (1.0F - p) < 20.0F)`),
/// Java `Math.round(double)` (`floor(x + 0.5)`) on the Gaussian approximation, and
/// `for (int y = 0; y < n; y++)` Bernoulli draws otherwise.
fn process_remove_binomial<R: RandomSource>(p: f32, random: &mut R, n: f32) -> f32 {
    let mut drop: i32 = 0;
    let use_bernoulli = n <= 128.0_f32 || n * p < 20.0_f32 || n * (1.0_f32 - p) < 20.0_f32;
    if !use_bernoulli {
        let miu = f64::from(n * p).floor();
        let sigma = f64::from(n * p * (1.0_f32 - p)).sqrt();
        let rounded = java_round_f64(miu + random.next_gaussian() * sigma) as i32;
        drop = rounded.clamp(0, n as i32);
    } else {
        let mut y: i32 = 0;
        while (y as f32) < n {
            if random.next_float() < p {
                drop = drop.wrapping_add(1);
            }
            y = y.wrapping_add(1);
        }
    }
    n - (drop as f32)
}

/// Java `Math.round(double)` — `floor(x + 0.5)` for finite values, `0` for NaN.
#[inline]
pub(crate) fn java_round_f64(x: f64) -> i64 {
    if x.is_nan() {
        0
    } else if x >= i64::MAX as f64 {
        i64::MAX
    } else if x <= i64::MIN as f64 {
        i64::MIN
    } else {
        (x + 0.5_f64).floor() as i64
    }
}

/// Java `Math.round(float)` — `floor(x + 0.5)` for finite values, `0` for NaN.
#[inline]
pub(crate) fn java_round_f32(x: f32) -> i32 {
    if x.is_nan() {
        0
    } else if x >= i32::MAX as f32 {
        i32::MAX
    } else if x <= i32::MIN as f32 {
        i32::MIN
    } else {
        (x + 0.5_f32).floor() as i32
    }
}

/// `net.minecraft.world.item.enchantment.effects.EnchantmentAttributeEffect`.
#[derive(Debug, Clone, PartialEq)]
pub struct EnchantmentAttributeEffect {
    pub id: Identifier,
    pub attribute: Identifier,
    pub amount: LevelBasedValue,
    pub operation: Operation,
}

impl EnchantmentAttributeEffect {
    /// `EnchantmentAttributeEffect.idForSlot(StringRepresentable slot)` —
    /// `this.id.withSuffix("/" + slot.getSerializedName())`.
    pub fn id_for_slot(&self, slot_name: &str) -> Identifier {
        let path = format!("{}/{}", self.id.path(), slot_name);
        Identifier::from_namespace_and_path(self.id.namespace(), &path)
    }

    /// `EnchantmentAttributeEffect.getModifier(int level, StringRepresentable slot)`.
    pub fn get_modifier(&self, level: i32, slot: EquipmentSlot) -> AttributeModifier {
        AttributeModifier::new(
            self.id_for_slot(slot.serialized_name()),
            f64::from(self.amount.calculate(level)),
            self.operation,
        )
    }

    /// Decodes an [`EnchantmentAttributeEffect`] from an NBT [`CompoundTag`].
    pub fn from_nbt(compound: &CompoundTag) -> Result<Self, EffectDecodeError> {
        let id_str = get_string(compound, "id").ok_or(EffectDecodeError::MissingField("id"))?;
        let attr_str = get_string(compound, "attribute")
            .ok_or(EffectDecodeError::MissingField("attribute"))?;
        let op_str = get_string(compound, "operation")
            .ok_or(EffectDecodeError::MissingField("operation"))?;
        let amount_tag = compound
            .tags
            .get("amount")
            .ok_or(EffectDecodeError::MissingField("amount"))?;

        let id = Identifier::try_parse(id_str)
            .ok_or_else(|| EffectDecodeError::InvalidIdentifier(id_str.to_owned()))?;
        let attribute = Identifier::try_parse(attr_str)
            .ok_or_else(|| EffectDecodeError::InvalidIdentifier(attr_str.to_owned()))?;
        let operation = parse_operation(op_str)?;
        let amount = LevelBasedValue::from_nbt(amount_tag)?;

        Ok(Self {
            id,
            attribute,
            amount,
            operation,
        })
    }
}

fn parse_operation(name: &str) -> Result<Operation, EffectDecodeError> {
    match name {
        "add_value" => Ok(Operation::AddValue),
        "add_multiplied_base" => Ok(Operation::AddMultipliedBase),
        "add_multiplied_total" => Ok(Operation::AddMultipliedTotal),
        other => Err(EffectDecodeError::UnknownOperation(other.to_owned())),
    }
}

/// `net.minecraft.world.item.enchantment.effects.DamageImmunity` — marker unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DamageImmunity;

/// High-level descriptor of an entity/location-based enchantment effect
/// (`EnchantmentEntityEffect` / `EnchantmentLocationBasedEffect`) decoded from
/// vanilla registry NBT, exposed as a clean execution seam for future combat
/// (`#23`) and world-tick consumers.
#[derive(Debug, Clone, PartialEq)]
pub enum EnchantmentEntityEffect {
    Ignite {
        duration: LevelBasedValue,
    },
    ApplyMobEffect {
        to_apply: Identifier,
        min_duration: LevelBasedValue,
        max_duration: LevelBasedValue,
        min_amplifier: LevelBasedValue,
        max_amplifier: LevelBasedValue,
    },
    DamageEntity {
        damage_type: Identifier,
        min_damage: LevelBasedValue,
        max_damage: LevelBasedValue,
    },
    DamageItem {
        amount: LevelBasedValue,
    },
    Explode {
        raw: CompoundTag,
    },
    PlaySound {
        sound_events: Vec<Identifier>,
        volume: f32,
        pitch: f32,
    },
    SummonEntity {
        entity: Identifier,
    },
    AllOf {
        effects: Vec<EnchantmentEntityEffect>,
    },
    /// Any other registered entity/location effect (e.g. `replace_disk`,
    /// `spawn_particles`, `change_item_damage`, `apply_exhaustion`,
    /// `apply_impulse`) preserved with its type ID and raw NBT payload.
    Custom {
        effect_type: Identifier,
        raw: CompoundTag,
    },
}

impl EnchantmentEntityEffect {
    /// Decodes an entity/location effect from an NBT compound.
    pub fn from_nbt(compound: &CompoundTag) -> Result<Self, EffectDecodeError> {
        let raw_type =
            get_string(compound, "type").ok_or(EffectDecodeError::MissingField("type"))?;
        match strip_minecraft_prefix(raw_type) {
            "ignite" => {
                let dur = compound
                    .tags
                    .get("duration")
                    .ok_or(EffectDecodeError::MissingField("duration"))?;
                Ok(Self::Ignite {
                    duration: LevelBasedValue::from_nbt(dur)?,
                })
            }
            "apply_mob_effect" => {
                let to_apply_str = get_string(compound, "to_apply")
                    .ok_or(EffectDecodeError::MissingField("to_apply"))?;
                let to_apply = Identifier::try_parse(to_apply_str)
                    .ok_or_else(|| EffectDecodeError::InvalidIdentifier(to_apply_str.to_owned()))?;
                Ok(Self::ApplyMobEffect {
                    to_apply,
                    min_duration: LevelBasedValue::from_nbt(
                        compound
                            .tags
                            .get("min_duration")
                            .ok_or(EffectDecodeError::MissingField("min_duration"))?,
                    )?,
                    max_duration: LevelBasedValue::from_nbt(
                        compound
                            .tags
                            .get("max_duration")
                            .ok_or(EffectDecodeError::MissingField("max_duration"))?,
                    )?,
                    min_amplifier: LevelBasedValue::from_nbt(
                        compound
                            .tags
                            .get("min_amplifier")
                            .ok_or(EffectDecodeError::MissingField("min_amplifier"))?,
                    )?,
                    max_amplifier: LevelBasedValue::from_nbt(
                        compound
                            .tags
                            .get("max_amplifier")
                            .ok_or(EffectDecodeError::MissingField("max_amplifier"))?,
                    )?,
                })
            }
            "damage_entity" => {
                let dt_str = get_string(compound, "damage_type")
                    .ok_or(EffectDecodeError::MissingField("damage_type"))?;
                let damage_type = Identifier::try_parse(dt_str)
                    .ok_or_else(|| EffectDecodeError::InvalidIdentifier(dt_str.to_owned()))?;
                Ok(Self::DamageEntity {
                    damage_type,
                    min_damage: LevelBasedValue::from_nbt(
                        compound
                            .tags
                            .get("min_damage")
                            .ok_or(EffectDecodeError::MissingField("min_damage"))?,
                    )?,
                    max_damage: LevelBasedValue::from_nbt(
                        compound
                            .tags
                            .get("max_damage")
                            .ok_or(EffectDecodeError::MissingField("max_damage"))?,
                    )?,
                })
            }
            "damage_item" => {
                let amt = compound
                    .tags
                    .get("amount")
                    .ok_or(EffectDecodeError::MissingField("amount"))?;
                Ok(Self::DamageItem {
                    amount: LevelBasedValue::from_nbt(amt)?,
                })
            }
            "explode" => Ok(Self::Explode {
                raw: compound.clone(),
            }),
            "play_sound" => {
                let sound_tag = compound
                    .tags
                    .get("sound")
                    .ok_or(EffectDecodeError::MissingField("sound"))?;
                let sound_events = match sound_tag {
                    Tag::String(s) => {
                        let id = Identifier::try_parse(&s.value)
                            .ok_or_else(|| EffectDecodeError::InvalidIdentifier(s.value.clone()))?;
                        vec![id]
                    }
                    Tag::List(list) => {
                        let mut ids = Vec::with_capacity(list.list.len());
                        for item in &list.list {
                            let Tag::String(s) = item else {
                                return Err(EffectDecodeError::InvalidField("sound"));
                            };
                            let id = Identifier::try_parse(&s.value).ok_or_else(|| {
                                EffectDecodeError::InvalidIdentifier(s.value.clone())
                            })?;
                            ids.push(id);
                        }
                        ids
                    }
                    _ => return Err(EffectDecodeError::InvalidField("sound")),
                };
                let volume = compound
                    .tags
                    .get("volume")
                    .and_then(tag_as_f32)
                    .unwrap_or(1.0);
                let pitch = compound
                    .tags
                    .get("pitch")
                    .and_then(tag_as_f32)
                    .unwrap_or(1.0);
                Ok(Self::PlaySound {
                    sound_events,
                    volume,
                    pitch,
                })
            }
            "summon_entity" => {
                let entity_str = get_string(compound, "entity")
                    .ok_or(EffectDecodeError::MissingField("entity"))?;
                let entity = Identifier::try_parse(entity_str)
                    .ok_or_else(|| EffectDecodeError::InvalidIdentifier(entity_str.to_owned()))?;
                Ok(Self::SummonEntity { entity })
            }
            "all_of" => {
                let list = match compound.tags.get("effects") {
                    Some(Tag::List(l)) => l,
                    Some(_) => return Err(EffectDecodeError::InvalidField("effects")),
                    None => return Err(EffectDecodeError::MissingField("effects")),
                };
                let mut effects = Vec::with_capacity(list.list.len());
                for item in &list.list {
                    let Tag::Compound(c) = item else {
                        return Err(EffectDecodeError::InvalidField("effects"));
                    };
                    effects.push(Self::from_nbt(c)?);
                }
                Ok(Self::AllOf { effects })
            }
            _ => {
                let effect_type = Identifier::try_parse(raw_type)
                    .ok_or_else(|| EffectDecodeError::InvalidIdentifier(raw_type.to_owned()))?;
                Ok(Self::Custom {
                    effect_type,
                    raw: compound.clone(),
                })
            }
        }
    }
}

/// Evaluation context passed to conditional enchantment effects by combat
/// (`#23`), item-use, and loot consumers.
#[derive(Debug, Clone, Default)]
pub struct EnchantmentEvalContext {
    /// Enchantment level currently being evaluated (used by `random_chance_with_enchanted_bonus` etc.).
    pub enchantment_level: i32,
    /// Entity type identifier of the `this` (target/victim) entity, e.g. `"minecraft:zombie"`.
    pub entity_type: Option<Identifier>,
    /// Whether the entity is in water or rain (used by Impaling / Riptide).
    pub in_water_or_rain: bool,
    /// Damage type identifier, e.g. `"minecraft:fall"`, `"minecraft:on_fire"`.
    pub damage_type: Option<Identifier>,
    /// Whether the damage source is direct (`DamageSource.isDirect()`).
    pub damage_is_direct: Option<bool>,
    /// Whether the world is currently thundering (used by Channeling).
    pub is_thundering: bool,
    /// Whether the target position can see the sky.
    pub can_see_sky: bool,
    /// Whether the entity is on the ground.
    pub on_ground: Option<bool>,
    /// Optional explicit damage tags active on the damage source (overrides/extends static table).
    pub extra_damage_tags: Vec<String>,
    /// Optional explicit entity tags active on the entity (overrides/extends static table).
    pub extra_entity_tags: Vec<String>,
}

impl EnchantmentEvalContext {
    /// Checks whether the context's `entity_type` belongs to `tag_or_id`
    /// (`"#minecraft:..."` tag or exact entity type ID).
    pub fn matches_entity_type(&self, tag_or_id: &str) -> bool {
        let Some(ref et) = self.entity_type else {
            return false;
        };
        let et_str = et.to_string();
        if let Some(tag) = tag_or_id.strip_prefix('#') {
            if self.extra_entity_tags.iter().any(|t| t == tag) {
                return true;
            }
            ENTITY_TYPE_TAG_BY_NAME
                .get(tag)
                .is_some_and(|members| members.contains(&et_str.as_str()))
        } else {
            et_str == tag_or_id
        }
    }

    /// Checks whether the context's `damage_type` belongs to `tag` (without leading `#`).
    pub fn has_damage_tag(&self, tag: &str) -> bool {
        if self.extra_damage_tags.iter().any(|t| t == tag) {
            return true;
        }
        let Some(ref dt) = self.damage_type else {
            return false;
        };
        let dt_str = dt.to_string();
        DAMAGE_TYPE_TAG_BY_NAME
            .get(tag)
            .is_some_and(|members| members.contains(&dt_str.as_str()))
    }
}

/// Decoded loot-condition predicate from an enchantment effect's `requirements`
/// compound (`LootItemCondition` subset used by vanilla MC 26.2 enchantments).
#[derive(Debug, Clone, PartialEq)]
pub enum EffectCondition {
    AllOf(Vec<EffectCondition>),
    AnyOf(Vec<EffectCondition>),
    Inverted(Box<EffectCondition>),
    DamageSourceProperties {
        tags: Vec<(String, bool)>,
        is_direct: Option<bool>,
    },
    EntityProperties {
        entity_type: Option<String>,
        on_ground: Option<bool>,
        can_see_sky: Option<bool>,
    },
    WeatherCheck {
        thundering: Option<bool>,
    },
    RandomChance {
        chance: LevelBasedValue,
    },
    /// Uninterpreted / world-dependent condition preserved as raw NBT.
    Opaque(CompoundTag),
}

impl EffectCondition {
    /// Evaluates this condition against an [`EnchantmentEvalContext`] and RNG.
    pub fn matches<R: RandomSource>(&self, ctx: &EnchantmentEvalContext, random: &mut R) -> bool {
        match self {
            Self::AllOf(terms) => terms.iter().all(|t| t.matches(ctx, random)),
            Self::AnyOf(terms) => terms.iter().any(|t| t.matches(ctx, random)),
            Self::Inverted(inner) => !inner.matches(ctx, random),
            Self::DamageSourceProperties { tags, is_direct } => {
                if let Some(expected_direct) = is_direct
                    && ctx.damage_is_direct != Some(*expected_direct)
                {
                    return false;
                }
                for (tag_id, expected) in tags {
                    if ctx.has_damage_tag(tag_id) != *expected {
                        return false;
                    }
                }
                true
            }
            Self::EntityProperties {
                entity_type,
                on_ground,
                can_see_sky,
            } => {
                if let Some(et) = entity_type
                    && !ctx.matches_entity_type(et)
                {
                    return false;
                }
                if let Some(og) = on_ground
                    && ctx.on_ground != Some(*og)
                {
                    return false;
                }
                if let Some(sky) = can_see_sky
                    && ctx.can_see_sky != *sky
                {
                    return false;
                }
                true
            }
            Self::WeatherCheck { thundering } => {
                thundering.is_none_or(|expected| ctx.is_thundering == expected)
            }
            Self::RandomChance { chance } => {
                random.next_float() < chance.calculate(ctx.enchantment_level)
            }
            Self::Opaque(_) => false,
        }
    }

    /// Decodes a `requirements` NBT compound into an [`EffectCondition`].
    pub fn from_nbt(compound: &CompoundTag) -> Self {
        let Some(cond_type) = get_string(compound, "condition") else {
            return Self::Opaque(compound.clone());
        };
        match strip_minecraft_prefix(cond_type) {
            "all_of" => {
                let terms = match compound.tags.get("terms") {
                    Some(Tag::List(l)) => l
                        .list
                        .iter()
                        .filter_map(|t| match t {
                            Tag::Compound(c) => Some(Self::from_nbt(c)),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                Self::AllOf(terms)
            }
            "any_of" => {
                let terms = match compound.tags.get("terms") {
                    Some(Tag::List(l)) => l
                        .list
                        .iter()
                        .filter_map(|t| match t {
                            Tag::Compound(c) => Some(Self::from_nbt(c)),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                Self::AnyOf(terms)
            }
            "inverted" => match compound.tags.get("term") {
                Some(Tag::Compound(c)) => Self::Inverted(Box::new(Self::from_nbt(c))),
                _ => Self::Opaque(compound.clone()),
            },
            "damage_source_properties" => {
                let mut tags = Vec::new();
                let mut is_direct = None;
                if let Some(Tag::Compound(pred)) = compound.tags.get("predicate") {
                    if let Some(Tag::Byte(b)) = pred.tags.get("is_direct") {
                        is_direct = Some(b.value != 0);
                    }
                    if let Some(Tag::List(tag_list)) = pred.tags.get("tags") {
                        for item in &tag_list.list {
                            if let Tag::Compound(tc) = item
                                && let Some(id) = get_string(tc, "id")
                            {
                                let expected = match tc.tags.get("expected") {
                                    Some(Tag::Byte(b)) => b.value != 0,
                                    _ => true,
                                };
                                tags.push((id.to_owned(), expected));
                            }
                        }
                    }
                }
                Self::DamageSourceProperties { tags, is_direct }
            }
            "entity_properties" => {
                let mut entity_type = None;
                let mut on_ground = None;
                let mut can_see_sky = None;
                if let Some(Tag::Compound(pred)) = compound.tags.get("predicate") {
                    if let Some(et) = get_string(pred, "minecraft:entity_type")
                        .or_else(|| get_string(pred, "type"))
                    {
                        entity_type = Some(et.to_owned());
                    }
                    if let Some(Tag::Compound(flags)) = pred.tags.get("flags")
                        && let Some(Tag::Byte(b)) = flags.tags.get("is_on_ground")
                    {
                        on_ground = Some(b.value != 0);
                    }
                    if let Some(Tag::Compound(loc)) = pred
                        .tags
                        .get("minecraft:location")
                        .or_else(|| pred.tags.get("location"))
                        && let Some(Tag::Byte(b)) = loc.tags.get("can_see_sky")
                    {
                        can_see_sky = Some(b.value != 0);
                    }
                }
                Self::EntityProperties {
                    entity_type,
                    on_ground,
                    can_see_sky,
                }
            }
            "weather_check" => {
                let thundering = match compound.tags.get("thundering") {
                    Some(Tag::Byte(b)) => Some(b.value != 0),
                    _ => None,
                };
                Self::WeatherCheck { thundering }
            }
            "random_chance" => {
                if let Some(tag) = compound.tags.get("chance")
                    && let Ok(val) = LevelBasedValue::from_nbt(tag)
                {
                    return Self::RandomChance { chance: val };
                }
                Self::Opaque(compound.clone())
            }
            _ => Self::Opaque(compound.clone()),
        }
    }
}

/// `net.minecraft.world.item.enchantment.ConditionalEffect<T>`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionalEffect<T> {
    pub effect: T,
    pub requirements: Option<EffectCondition>,
}

impl<T> ConditionalEffect<T> {
    pub fn unconditional(effect: T) -> Self {
        Self {
            effect,
            requirements: None,
        }
    }

    pub fn with_requirements(effect: T, requirements: Option<EffectCondition>) -> Self {
        Self {
            effect,
            requirements,
        }
    }

    /// `ConditionalEffect.matches(LootContext)`.
    pub fn matches<R: RandomSource>(&self, ctx: &EnchantmentEvalContext, random: &mut R) -> bool {
        self.requirements
            .as_ref()
            .is_none_or(|cond| cond.matches(ctx, random))
    }
}

/// `net.minecraft.world.item.enchantment.TargetedConditionalEffect<T>`.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetedConditionalEffect<T> {
    pub enchanted: EnchantmentTarget,
    pub affected: EnchantmentTarget,
    pub effect: T,
    pub requirements: Option<EffectCondition>,
}

impl<T> TargetedConditionalEffect<T> {
    /// `TargetedConditionalEffect.matches(LootContext)`.
    pub fn matches<R: RandomSource>(&self, ctx: &EnchantmentEvalContext, random: &mut R) -> bool {
        self.requirements
            .as_ref()
            .is_none_or(|cond| cond.matches(ctx, random))
    }
}

/// All 31 effect component keys from `EnchantmentEffectComponents.java` (MC 26.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnchantmentEffectComponent {
    DamageProtection,
    DamageImmunity,
    Damage,
    SmashDamagePerFallenBlock,
    Knockback,
    ArmorEffectiveness,
    PostAttack,
    PostPiercingAttack,
    HitBlock,
    ItemDamage,
    EquipmentDrops,
    LocationChanged,
    Tick,
    AmmoUse,
    ProjectilePiercing,
    ProjectileSpawned,
    ProjectileSpread,
    ProjectileCount,
    TridentReturnAcceleration,
    FishingTimeReduction,
    FishingLuckBonus,
    BlockExperience,
    MobExperience,
    RepairWithXp,
    Attributes,
    CrossbowChargeTime,
    CrossbowChargingSounds,
    TridentSound,
    PreventEquipmentDrop,
    PreventArmorChange,
    TridentSpinAttackStrength,
}

impl EnchantmentEffectComponent {
    pub fn serialized_name(self) -> &'static str {
        match self {
            Self::DamageProtection => "minecraft:damage_protection",
            Self::DamageImmunity => "minecraft:damage_immunity",
            Self::Damage => "minecraft:damage",
            Self::SmashDamagePerFallenBlock => "minecraft:smash_damage_per_fallen_block",
            Self::Knockback => "minecraft:knockback",
            Self::ArmorEffectiveness => "minecraft:armor_effectiveness",
            Self::PostAttack => "minecraft:post_attack",
            Self::PostPiercingAttack => "minecraft:post_piercing_attack",
            Self::HitBlock => "minecraft:hit_block",
            Self::ItemDamage => "minecraft:item_damage",
            Self::EquipmentDrops => "minecraft:equipment_drops",
            Self::LocationChanged => "minecraft:location_changed",
            Self::Tick => "minecraft:tick",
            Self::AmmoUse => "minecraft:ammo_use",
            Self::ProjectilePiercing => "minecraft:projectile_piercing",
            Self::ProjectileSpawned => "minecraft:projectile_spawned",
            Self::ProjectileSpread => "minecraft:projectile_spread",
            Self::ProjectileCount => "minecraft:projectile_count",
            Self::TridentReturnAcceleration => "minecraft:trident_return_acceleration",
            Self::FishingTimeReduction => "minecraft:fishing_time_reduction",
            Self::FishingLuckBonus => "minecraft:fishing_luck_bonus",
            Self::BlockExperience => "minecraft:block_experience",
            Self::MobExperience => "minecraft:mob_experience",
            Self::RepairWithXp => "minecraft:repair_with_xp",
            Self::Attributes => "minecraft:attributes",
            Self::CrossbowChargeTime => "minecraft:crossbow_charge_time",
            Self::CrossbowChargingSounds => "minecraft:crossbow_charging_sounds",
            Self::TridentSound => "minecraft:trident_sound",
            Self::PreventEquipmentDrop => "minecraft:prevent_equipment_drop",
            Self::PreventArmorChange => "minecraft:prevent_armor_change",
            Self::TridentSpinAttackStrength => "minecraft:trident_spin_attack_strength",
        }
    }
}

/// Typed effect map attached to an [`crate::enchantment::Enchantment`], mirroring
/// Paper's `DataComponentMap effects` populated with `EnchantmentEffectComponents`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EnchantmentEffects {
    pub damage_protection: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub damage_immunity: Vec<ConditionalEffect<DamageImmunity>>,
    pub damage: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub smash_damage_per_fallen_block: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub knockback: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub armor_effectiveness: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub post_attack: Vec<TargetedConditionalEffect<EnchantmentEntityEffect>>,
    pub post_piercing_attack: Vec<ConditionalEffect<EnchantmentEntityEffect>>,
    pub hit_block: Vec<ConditionalEffect<EnchantmentEntityEffect>>,
    pub item_damage: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub equipment_drops: Vec<TargetedConditionalEffect<EnchantmentValueEffect>>,
    pub location_changed: Vec<ConditionalEffect<EnchantmentEntityEffect>>,
    pub tick: Vec<ConditionalEffect<EnchantmentEntityEffect>>,
    pub ammo_use: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub projectile_piercing: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub projectile_spawned: Vec<ConditionalEffect<EnchantmentEntityEffect>>,
    pub projectile_spread: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub projectile_count: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub trident_return_acceleration: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub fishing_time_reduction: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub fishing_luck_bonus: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub block_experience: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub mob_experience: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub repair_with_xp: Vec<ConditionalEffect<EnchantmentValueEffect>>,
    pub attributes: Vec<EnchantmentAttributeEffect>,
    pub crossbow_charge_time: Option<EnchantmentValueEffect>,
    pub trident_spin_attack_strength: Option<EnchantmentValueEffect>,
    pub prevent_equipment_drop: bool,
    pub prevent_armor_change: bool,
}

impl EnchantmentEffects {
    /// Returns whether the given [`EnchantmentEffectComponent`] is present
    /// (mirrors `effects.has(DataComponentType)`).
    pub fn has(&self, component: EnchantmentEffectComponent) -> bool {
        match component {
            EnchantmentEffectComponent::DamageProtection => !self.damage_protection.is_empty(),
            EnchantmentEffectComponent::DamageImmunity => !self.damage_immunity.is_empty(),
            EnchantmentEffectComponent::Damage => !self.damage.is_empty(),
            EnchantmentEffectComponent::SmashDamagePerFallenBlock => {
                !self.smash_damage_per_fallen_block.is_empty()
            }
            EnchantmentEffectComponent::Knockback => !self.knockback.is_empty(),
            EnchantmentEffectComponent::ArmorEffectiveness => !self.armor_effectiveness.is_empty(),
            EnchantmentEffectComponent::PostAttack => !self.post_attack.is_empty(),
            EnchantmentEffectComponent::PostPiercingAttack => !self.post_piercing_attack.is_empty(),
            EnchantmentEffectComponent::HitBlock => !self.hit_block.is_empty(),
            EnchantmentEffectComponent::ItemDamage => !self.item_damage.is_empty(),
            EnchantmentEffectComponent::EquipmentDrops => !self.equipment_drops.is_empty(),
            EnchantmentEffectComponent::LocationChanged => !self.location_changed.is_empty(),
            EnchantmentEffectComponent::Tick => !self.tick.is_empty(),
            EnchantmentEffectComponent::AmmoUse => !self.ammo_use.is_empty(),
            EnchantmentEffectComponent::ProjectilePiercing => !self.projectile_piercing.is_empty(),
            EnchantmentEffectComponent::ProjectileSpawned => !self.projectile_spawned.is_empty(),
            EnchantmentEffectComponent::ProjectileSpread => !self.projectile_spread.is_empty(),
            EnchantmentEffectComponent::ProjectileCount => !self.projectile_count.is_empty(),
            EnchantmentEffectComponent::TridentReturnAcceleration => {
                !self.trident_return_acceleration.is_empty()
            }
            EnchantmentEffectComponent::FishingTimeReduction => {
                !self.fishing_time_reduction.is_empty()
            }
            EnchantmentEffectComponent::FishingLuckBonus => !self.fishing_luck_bonus.is_empty(),
            EnchantmentEffectComponent::BlockExperience => !self.block_experience.is_empty(),
            EnchantmentEffectComponent::MobExperience => !self.mob_experience.is_empty(),
            EnchantmentEffectComponent::RepairWithXp => !self.repair_with_xp.is_empty(),
            EnchantmentEffectComponent::Attributes => !self.attributes.is_empty(),
            EnchantmentEffectComponent::CrossbowChargeTime => self.crossbow_charge_time.is_some(),
            EnchantmentEffectComponent::CrossbowChargingSounds => false,
            EnchantmentEffectComponent::TridentSound => false,
            EnchantmentEffectComponent::PreventEquipmentDrop => self.prevent_equipment_drop,
            EnchantmentEffectComponent::PreventArmorChange => self.prevent_armor_change,
            EnchantmentEffectComponent::TridentSpinAttackStrength => {
                self.trident_spin_attack_strength.is_some()
            }
        }
    }

    /// Decodes the `effects` NBT compound of a vanilla enchantment definition.
    pub fn from_nbt(compound: &CompoundTag) -> Result<Self, EffectDecodeError> {
        let mut out = Self::default();
        for (key, value) in &compound.tags {
            match strip_minecraft_prefix(key) {
                "damage_protection" => out.damage_protection = decode_cond_value_list(value)?,
                "damage_immunity" => out.damage_immunity = decode_cond_immunity_list(value)?,
                "damage" => out.damage = decode_cond_value_list(value)?,
                "smash_damage_per_fallen_block" => {
                    out.smash_damage_per_fallen_block = decode_cond_value_list(value)?;
                }
                "knockback" => out.knockback = decode_cond_value_list(value)?,
                "armor_effectiveness" => out.armor_effectiveness = decode_cond_value_list(value)?,
                "post_attack" => out.post_attack = decode_targeted_entity_list(value)?,
                "post_piercing_attack" => {
                    out.post_piercing_attack = decode_cond_entity_list(value)?;
                }
                "hit_block" => out.hit_block = decode_cond_entity_list(value)?,
                "item_damage" => out.item_damage = decode_cond_value_list(value)?,
                "equipment_drops" => out.equipment_drops = decode_targeted_value_list(value)?,
                "location_changed" => out.location_changed = decode_cond_entity_list(value)?,
                "tick" => out.tick = decode_cond_entity_list(value)?,
                "ammo_use" => out.ammo_use = decode_cond_value_list(value)?,
                "projectile_piercing" => out.projectile_piercing = decode_cond_value_list(value)?,
                "projectile_spawned" => out.projectile_spawned = decode_cond_entity_list(value)?,
                "projectile_spread" => out.projectile_spread = decode_cond_value_list(value)?,
                "projectile_count" => out.projectile_count = decode_cond_value_list(value)?,
                "trident_return_acceleration" => {
                    out.trident_return_acceleration = decode_cond_value_list(value)?;
                }
                "fishing_time_reduction" => {
                    out.fishing_time_reduction = decode_cond_value_list(value)?;
                }
                "fishing_luck_bonus" => out.fishing_luck_bonus = decode_cond_value_list(value)?,
                "block_experience" => out.block_experience = decode_cond_value_list(value)?,
                "mob_experience" => out.mob_experience = decode_cond_value_list(value)?,
                "repair_with_xp" => out.repair_with_xp = decode_cond_value_list(value)?,
                "attributes" => {
                    let Tag::List(list) = value else {
                        return Err(EffectDecodeError::InvalidField("attributes"));
                    };
                    for item in &list.list {
                        let Tag::Compound(c) = item else {
                            return Err(EffectDecodeError::InvalidField("attributes"));
                        };
                        out.attributes
                            .push(EnchantmentAttributeEffect::from_nbt(c)?);
                    }
                }
                "crossbow_charge_time" => {
                    let Tag::Compound(c) = value else {
                        return Err(EffectDecodeError::InvalidField("crossbow_charge_time"));
                    };
                    out.crossbow_charge_time = Some(EnchantmentValueEffect::from_nbt(c)?);
                }
                "trident_spin_attack_strength" => {
                    let Tag::Compound(c) = value else {
                        return Err(EffectDecodeError::InvalidField(
                            "trident_spin_attack_strength",
                        ));
                    };
                    out.trident_spin_attack_strength = Some(EnchantmentValueEffect::from_nbt(c)?);
                }
                "prevent_equipment_drop" => out.prevent_equipment_drop = true,
                "prevent_armor_change" => out.prevent_armor_change = true,
                // Sound-list components (`crossbow_charging_sounds`, `trident_sound`) or
                // future components are accepted without failing decode.
                _ => {}
            }
        }
        Ok(out)
    }
}

fn decode_requirements(compound: &CompoundTag) -> Option<EffectCondition> {
    match compound.tags.get("requirements") {
        Some(Tag::Compound(c)) => Some(EffectCondition::from_nbt(c)),
        _ => None,
    }
}

fn decode_cond_value_list(
    tag: &Tag,
) -> Result<Vec<ConditionalEffect<EnchantmentValueEffect>>, EffectDecodeError> {
    let Tag::List(list) = tag else {
        return Err(EffectDecodeError::InvalidField("effect_list"));
    };
    let mut out = Vec::with_capacity(list.list.len());
    for item in &list.list {
        let Tag::Compound(c) = item else {
            return Err(EffectDecodeError::InvalidField("effect_entry"));
        };
        let Some(Tag::Compound(eff)) = c.tags.get("effect") else {
            return Err(EffectDecodeError::MissingField("effect"));
        };
        out.push(ConditionalEffect {
            effect: EnchantmentValueEffect::from_nbt(eff)?,
            requirements: decode_requirements(c),
        });
    }
    Ok(out)
}

fn decode_cond_immunity_list(
    tag: &Tag,
) -> Result<Vec<ConditionalEffect<DamageImmunity>>, EffectDecodeError> {
    let Tag::List(list) = tag else {
        return Err(EffectDecodeError::InvalidField("damage_immunity"));
    };
    let mut out = Vec::with_capacity(list.list.len());
    for item in &list.list {
        let Tag::Compound(c) = item else {
            return Err(EffectDecodeError::InvalidField("damage_immunity"));
        };
        out.push(ConditionalEffect {
            effect: DamageImmunity,
            requirements: decode_requirements(c),
        });
    }
    Ok(out)
}

fn decode_cond_entity_list(
    tag: &Tag,
) -> Result<Vec<ConditionalEffect<EnchantmentEntityEffect>>, EffectDecodeError> {
    let Tag::List(list) = tag else {
        return Err(EffectDecodeError::InvalidField("entity_effect_list"));
    };
    let mut out = Vec::with_capacity(list.list.len());
    for item in &list.list {
        let Tag::Compound(c) = item else {
            return Err(EffectDecodeError::InvalidField("entity_effect_entry"));
        };
        let Some(Tag::Compound(eff)) = c.tags.get("effect") else {
            return Err(EffectDecodeError::MissingField("effect"));
        };
        out.push(ConditionalEffect {
            effect: EnchantmentEntityEffect::from_nbt(eff)?,
            requirements: decode_requirements(c),
        });
    }
    Ok(out)
}

fn decode_targeted_entity_list(
    tag: &Tag,
) -> Result<Vec<TargetedConditionalEffect<EnchantmentEntityEffect>>, EffectDecodeError> {
    let Tag::List(list) = tag else {
        return Err(EffectDecodeError::InvalidField("post_attack"));
    };
    let mut out = Vec::with_capacity(list.list.len());
    for item in &list.list {
        let Tag::Compound(c) = item else {
            return Err(EffectDecodeError::InvalidField("post_attack"));
        };
        let enchanted_str =
            get_string(c, "enchanted").ok_or(EffectDecodeError::MissingField("enchanted"))?;
        let affected_str =
            get_string(c, "affected").ok_or(EffectDecodeError::MissingField("affected"))?;
        let Some(Tag::Compound(eff)) = c.tags.get("effect") else {
            return Err(EffectDecodeError::MissingField("effect"));
        };
        out.push(TargetedConditionalEffect {
            enchanted: EnchantmentTarget::from_serialized_name(enchanted_str)
                .ok_or_else(|| EffectDecodeError::UnknownTarget(enchanted_str.to_owned()))?,
            affected: EnchantmentTarget::from_serialized_name(affected_str)
                .ok_or_else(|| EffectDecodeError::UnknownTarget(affected_str.to_owned()))?,
            effect: EnchantmentEntityEffect::from_nbt(eff)?,
            requirements: decode_requirements(c),
        });
    }
    Ok(out)
}

fn decode_targeted_value_list(
    tag: &Tag,
) -> Result<Vec<TargetedConditionalEffect<EnchantmentValueEffect>>, EffectDecodeError> {
    let Tag::List(list) = tag else {
        return Err(EffectDecodeError::InvalidField("equipment_drops"));
    };
    let mut out = Vec::with_capacity(list.list.len());
    for item in &list.list {
        let Tag::Compound(c) = item else {
            return Err(EffectDecodeError::InvalidField("equipment_drops"));
        };
        let enchanted_str =
            get_string(c, "enchanted").ok_or(EffectDecodeError::MissingField("enchanted"))?;
        // Paper: equipment_drops defaults `affected` to `VICTIM`.
        let affected = match get_string(c, "affected") {
            Some(s) => EnchantmentTarget::from_serialized_name(s)
                .ok_or_else(|| EffectDecodeError::UnknownTarget(s.to_owned()))?,
            None => EnchantmentTarget::Victim,
        };
        let Some(Tag::Compound(eff)) = c.tags.get("effect") else {
            return Err(EffectDecodeError::MissingField("effect"));
        };
        out.push(TargetedConditionalEffect {
            enchanted: EnchantmentTarget::from_serialized_name(enchanted_str)
                .ok_or_else(|| EffectDecodeError::UnknownTarget(enchanted_str.to_owned()))?,
            affected,
            effect: EnchantmentValueEffect::from_nbt(eff)?,
            requirements: decode_requirements(c),
        });
    }
    Ok(out)
}
