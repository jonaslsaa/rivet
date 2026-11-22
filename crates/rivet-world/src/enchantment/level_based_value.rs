//! Port of `net.minecraft.world.item.enchantment.LevelBasedValue` (MC 26.2).
//!
//! Computes level-parameterized floating-point values for enchantment effects.
//! All arithmetic preserves Java `float`/`double` semantics and `Mth` helpers.

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::tag::Tag;
use rivet_util::mth;

/// Errors returned when decoding a [`LevelBasedValue`] from NBT.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LevelBasedValueError {
    #[error("expected float or compound tag for LevelBasedValue, got tag id {0}")]
    InvalidTagType(i8),
    #[error("missing required field '{0}' in LevelBasedValue")]
    MissingField(&'static str),
    #[error("unknown LevelBasedValue type '{0}'")]
    UnknownType(String),
    #[error("expected float for field '{0}' in LevelBasedValue")]
    ExpectedFloat(&'static str),
    #[error("expected list of floats for field '{0}' in LevelBasedValue")]
    ExpectedFloatList(&'static str),
}

/// `net.minecraft.world.item.enchantment.LevelBasedValue`.
#[derive(Debug, Clone, PartialEq)]
pub enum LevelBasedValue {
    /// `LevelBasedValue.Constant` — returns `value` regardless of level.
    Constant { value: f32 },
    /// `LevelBasedValue.Linear` — `base + per_level_above_first * (level - 1)`.
    Linear {
        base: f32,
        per_level_above_first: f32,
    },
    /// `LevelBasedValue.Clamped` — `Mth.clamp(value.calculate(level), min, max)`.
    Clamped {
        value: Box<LevelBasedValue>,
        min: f32,
        max: f32,
    },
    /// `LevelBasedValue.Fraction` — `denominator == 0.0F ? 0.0F : numerator / denominator`.
    Fraction {
        numerator: Box<LevelBasedValue>,
        denominator: Box<LevelBasedValue>,
    },
    /// `LevelBasedValue.LevelsSquared` — `Mth.square(level) + added`.
    LevelsSquared { added: f32 },
    /// `LevelBasedValue.Exponent` — `(float) Math.pow(base.calculate(level), power.calculate(level))`.
    Exponent {
        base: Box<LevelBasedValue>,
        power: Box<LevelBasedValue>,
    },
    /// `LevelBasedValue.Lookup` — `level <= values.len() ? values[level - 1] : fallback.calculate(level)`.
    Lookup {
        values: Vec<f32>,
        fallback: Box<LevelBasedValue>,
    },
}

impl LevelBasedValue {
    /// `LevelBasedValue.constant(float)`.
    pub fn constant(value: f32) -> Self {
        Self::Constant { value }
    }

    /// `LevelBasedValue.perLevel(float)` — `base = per_level`, `per_level_above_first = per_level`.
    pub fn per_level(per_level: f32) -> Self {
        Self::Linear {
            base: per_level,
            per_level_above_first: per_level,
        }
    }

    /// `LevelBasedValue.perLevel(float base, float perLevelAboveFirst)`.
    pub fn per_level_with_base(base: f32, per_level_above_first: f32) -> Self {
        Self::Linear {
            base,
            per_level_above_first,
        }
    }

    /// `LevelBasedValue.lookup(List<Float>, LevelBasedValue)`.
    pub fn lookup(values: Vec<f32>, fallback: LevelBasedValue) -> Self {
        Self::Lookup {
            values,
            fallback: Box::new(fallback),
        }
    }

    /// `LevelBasedValue.clamped(LevelBasedValue, float, float)`.
    pub fn clamped(value: LevelBasedValue, min: f32, max: f32) -> Self {
        Self::Clamped {
            value: Box::new(value),
            min,
            max,
        }
    }

    /// `LevelBasedValue.fraction(LevelBasedValue, LevelBasedValue)`.
    pub fn fraction(numerator: LevelBasedValue, denominator: LevelBasedValue) -> Self {
        Self::Fraction {
            numerator: Box::new(numerator),
            denominator: Box::new(denominator),
        }
    }

    /// `LevelBasedValue.levelsSquared(float)`.
    pub fn levels_squared(added: f32) -> Self {
        Self::LevelsSquared { added }
    }

    /// `LevelBasedValue.exponent(LevelBasedValue, LevelBasedValue)`.
    pub fn exponent(base: LevelBasedValue, power: LevelBasedValue) -> Self {
        Self::Exponent {
            base: Box::new(base),
            power: Box::new(power),
        }
    }

    /// `LevelBasedValue.calculate(int level)`.
    pub fn calculate(&self, level: i32) -> f32 {
        match self {
            Self::Constant { value } => *value,
            Self::Linear {
                base,
                per_level_above_first,
            } => *base + *per_level_above_first * (level.wrapping_sub(1) as f32),
            Self::Clamped { value, min, max } => mth::clamp_f32(value.calculate(level), *min, *max),
            Self::Fraction {
                numerator,
                denominator,
            } => {
                let denom = denominator.calculate(level);
                if denom == 0.0_f32 {
                    0.0_f32
                } else {
                    numerator.calculate(level) / denom
                }
            }
            Self::LevelsSquared { added } => (mth::square_i32(level) as f32) + *added,
            Self::Exponent { base, power } => {
                let b = f64::from(base.calculate(level));
                let p = f64::from(power.calculate(level));
                b.powf(p) as f32
            }
            Self::Lookup { values, fallback } => {
                if level > 0 && (level as usize) <= values.len() {
                    values[(level - 1) as usize]
                } else {
                    fallback.calculate(level)
                }
            }
        }
    }

    /// Returns the registered type name (without `minecraft:` prefix) matching
    /// `LevelBasedValue.bootstrap`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Constant { .. } => "constant",
            Self::Linear { .. } => "linear",
            Self::Clamped { .. } => "clamped",
            Self::Fraction { .. } => "fraction",
            Self::LevelsSquared { .. } => "levels_squared",
            Self::Exponent { .. } => "exponent",
            Self::Lookup { .. } => "lookup",
        }
    }

    /// Decodes a [`LevelBasedValue`] from an NBT [`Tag`] (matches Paper's
    /// `Codec.either(Constant.CODEC, DISPATCH_CODEC)`).
    pub fn from_nbt(tag: &Tag) -> Result<Self, LevelBasedValueError> {
        if let Some(value) = tag_as_f32(tag) {
            return Ok(Self::Constant { value });
        }
        match tag {
            Tag::Compound(compound) => Self::from_compound(compound),
            other => Err(LevelBasedValueError::InvalidTagType(other.id())),
        }
    }

    fn from_compound(compound: &CompoundTag) -> Result<Self, LevelBasedValueError> {
        let raw_type =
            get_string(compound, "type").ok_or(LevelBasedValueError::MissingField("type"))?;
        let normalized = strip_minecraft_prefix(raw_type);
        match normalized {
            "linear" => Ok(Self::Linear {
                base: get_f32(compound, "base")?,
                per_level_above_first: get_f32(compound, "per_level_above_first")?,
            }),
            "clamped" => {
                let value_tag = compound
                    .tags
                    .get("value")
                    .ok_or(LevelBasedValueError::MissingField("value"))?;
                Ok(Self::Clamped {
                    value: Box::new(Self::from_nbt(value_tag)?),
                    min: get_f32(compound, "min")?,
                    max: get_f32(compound, "max")?,
                })
            }
            "fraction" => {
                let num_tag = compound
                    .tags
                    .get("numerator")
                    .ok_or(LevelBasedValueError::MissingField("numerator"))?;
                let den_tag = compound
                    .tags
                    .get("denominator")
                    .ok_or(LevelBasedValueError::MissingField("denominator"))?;
                Ok(Self::Fraction {
                    numerator: Box::new(Self::from_nbt(num_tag)?),
                    denominator: Box::new(Self::from_nbt(den_tag)?),
                })
            }
            "levels_squared" => Ok(Self::LevelsSquared {
                added: get_f32(compound, "added")?,
            }),
            "exponent" => {
                let base_tag = compound
                    .tags
                    .get("base")
                    .ok_or(LevelBasedValueError::MissingField("base"))?;
                let power_tag = compound
                    .tags
                    .get("power")
                    .ok_or(LevelBasedValueError::MissingField("power"))?;
                Ok(Self::Exponent {
                    base: Box::new(Self::from_nbt(base_tag)?),
                    power: Box::new(Self::from_nbt(power_tag)?),
                })
            }
            "lookup" => {
                let values_tag = compound
                    .tags
                    .get("values")
                    .ok_or(LevelBasedValueError::MissingField("values"))?;
                let values = match values_tag {
                    Tag::List(list) => list
                        .list
                        .iter()
                        .map(|t| {
                            tag_as_f32(t).ok_or(LevelBasedValueError::ExpectedFloatList("values"))
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => return Err(LevelBasedValueError::ExpectedFloatList("values")),
                };
                let fallback_tag = compound
                    .tags
                    .get("fallback")
                    .ok_or(LevelBasedValueError::MissingField("fallback"))?;
                Ok(Self::Lookup {
                    values,
                    fallback: Box::new(Self::from_nbt(fallback_tag)?),
                })
            }
            other => Err(LevelBasedValueError::UnknownType(other.to_owned())),
        }
    }
}

pub(crate) fn strip_minecraft_prefix(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

pub(crate) fn get_string<'a>(compound: &'a CompoundTag, key: &str) -> Option<&'a str> {
    match compound.tags.get(key) {
        Some(Tag::String(s)) => Some(s.value.as_str()),
        _ => None,
    }
}

pub(crate) fn tag_as_f32(tag: &Tag) -> Option<f32> {
    match tag {
        Tag::Float(f) => Some(f.value),
        Tag::Double(d) => Some(d.value as f32),
        Tag::Int(i) => Some(i.value as f32),
        Tag::Long(l) => Some(l.value as f32),
        Tag::Short(s) => Some(f32::from(s.value)),
        Tag::Byte(b) => Some(f32::from(b.value)),
        _ => None,
    }
}

fn get_f32(compound: &CompoundTag, key: &'static str) -> Result<f32, LevelBasedValueError> {
    let tag = compound
        .tags
        .get(key)
        .ok_or(LevelBasedValueError::MissingField(key))?;
    tag_as_f32(tag).ok_or(LevelBasedValueError::ExpectedFloat(key))
}
