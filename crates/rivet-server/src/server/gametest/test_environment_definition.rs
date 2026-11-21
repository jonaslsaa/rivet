//! `net.minecraft.gametest.framework.TestEnvironmentDefinition` and
//! `GameTestEnvironments` (Paper 26.2).
//!
//! Covers all 7 built-in environment definition types, `GameTestEnvironments`
//! constants/bootstrap, and fail-closed JSON & NBT decoding matching Paper's
//! `TestEnvironmentDefinition.DIRECT_CODEC` and `CODEC`.
//!
//! Runtime `setup(ServerLevel)` / `teardown(ServerLevel)` execution is deferred
//! to issue #430.

use indexmap::IndexMap;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::tag::Tag;
use rivet_registry::core::Difficulty;
use rivet_registry::{Identifier, ResourceKey};
use serde_json::Value as JsonValue;

use crate::server::gametest::registries::{TEST_ENVIRONMENT, TEST_ENVIRONMENT_DEFINITION_TYPE};

/// `GameTestEnvironments.DEFAULT = "default"`.
pub const DEFAULT_ENVIRONMENT_NAME: &str = "default";

/// `GameTestEnvironments.DEFAULT_KEY` (`minecraft:test_environment` / `minecraft:default`).
pub fn default_environment_key() -> ResourceKey<TestEnvironmentDefinition> {
    ResourceKey::create(
        &TEST_ENVIRONMENT,
        Identifier::with_default_namespace(DEFAULT_ENVIRONMENT_NAME),
    )
}

/// `GameTestEnvironments.create(String)` — key in `minecraft:test_environment`.
pub fn environment_key(name: &str) -> ResourceKey<TestEnvironmentDefinition> {
    ResourceKey::create(&TEST_ENVIRONMENT, Identifier::with_default_namespace(name))
}

/// `GameTestEnvironments.bootstrap` — returns the canonical built-in
/// `minecraft:default` environment (`AllOf([])`).
pub fn default_environment_definition() -> TestEnvironmentDefinition {
    TestEnvironmentDefinition::AllOf {
        definitions: Vec::new(),
    }
}

/// Built-in keys registered by `TestEnvironmentDefinition.bootstrap(Registry)`
/// in exact Paper bootstrap order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TestEnvironmentDefinitionType {
    AllOf,
    ClockTime,
    Difficulty,
    Function,
    GameRules,
    TimelineAttributes,
    Weather,
}

impl TestEnvironmentDefinitionType {
    /// Exact Paper bootstrap order (`TestEnvironmentDefinition.java:43-51`).
    pub const BOOTSTRAP_ORDER: [Self; 7] = [
        Self::AllOf,
        Self::ClockTime,
        Self::Difficulty,
        Self::Function,
        Self::GameRules,
        Self::TimelineAttributes,
        Self::Weather,
    ];

    /// Serialized path in the `minecraft:` namespace.
    pub const fn path(self) -> &'static str {
        match self {
            Self::AllOf => "all_of",
            Self::ClockTime => "clock_time",
            Self::Difficulty => "difficulty",
            Self::Function => "function",
            Self::GameRules => "game_rules",
            Self::TimelineAttributes => "timeline_attributes",
            Self::Weather => "weather",
        }
    }

    /// Canonical `Identifier` (`minecraft:<path>`).
    pub fn identifier(self) -> Identifier {
        Identifier::with_default_namespace(self.path())
    }

    /// Canonical `ResourceKey` in `minecraft:test_environment_definition_type`.
    pub fn resource_key(self) -> ResourceKey<TestEnvironmentDefinitionType> {
        ResourceKey::create(&TEST_ENVIRONMENT_DEFINITION_TYPE, self.identifier())
    }

    /// Parse from an `Identifier` (must be in `minecraft:` namespace).
    pub fn from_identifier(id: &Identifier) -> Option<Self> {
        if id.namespace() != "minecraft" {
            return None;
        }
        match id.path() {
            "all_of" => Some(Self::AllOf),
            "clock_time" => Some(Self::ClockTime),
            "difficulty" => Some(Self::Difficulty),
            "function" => Some(Self::Function),
            "game_rules" => Some(Self::GameRules),
            "timeline_attributes" => Some(Self::TimelineAttributes),
            "weather" => Some(Self::Weather),
            _ => None,
        }
    }
}

/// Reference to a [`TestEnvironmentDefinition`], matching Paper's
/// `RegistryFileCodec.create(Registries.TEST_ENVIRONMENT, DIRECT_CODEC)`:
/// either a registry key reference (`"minecraft:default"`) or an inline
/// definition object (`{ "type": "minecraft:..." }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestEnvironmentRef {
    Key(ResourceKey<TestEnvironmentDefinition>),
    Inline(Box<TestEnvironmentDefinition>),
}

impl TestEnvironmentRef {
    /// Construct a key reference to `minecraft:default`.
    pub fn default_key() -> Self {
        Self::Key(default_environment_key())
    }

    /// Construct a key reference from an `Identifier`.
    pub fn key_from_id(id: Identifier) -> Self {
        Self::Key(ResourceKey::create(&TEST_ENVIRONMENT, id))
    }

    /// Decode from JSON (`RegistryFileCodec`: string -> key, object -> inline).
    pub fn decode_json(value: &JsonValue) -> Result<Self, EnvironmentDecodeError> {
        match value {
            JsonValue::String(s) => {
                let id = parse_identifier_strict(s, "environment")?;
                Ok(Self::key_from_id(id))
            }
            JsonValue::Object(_) => Ok(Self::Inline(Box::new(
                TestEnvironmentDefinition::decode_json(value)?,
            ))),
            other => Err(EnvironmentDecodeError::InvalidFieldType {
                field: "environment",
                expected: "identifier string or inline environment object",
                actual: json_type_name(other),
            }),
        }
    }

    /// Decode from NBT (`String` -> key, `Compound` -> inline).
    pub fn decode_nbt(tag: &Tag) -> Result<Self, EnvironmentDecodeError> {
        match tag {
            Tag::String(s) => {
                let id = parse_identifier_strict(&s.value, "environment")?;
                Ok(Self::key_from_id(id))
            }
            Tag::Compound(_) => Ok(Self::Inline(Box::new(
                TestEnvironmentDefinition::decode_nbt(tag)?,
            ))),
            other => Err(EnvironmentDecodeError::InvalidFieldType {
                field: "environment",
                expected: "string or compound tag",
                actual: nbt_type_name(other),
            }),
        }
    }
}

/// `TestEnvironmentDefinition.Weather.Type` (Paper 26.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WeatherType {
    Clear,
    Rain,
    Thunder,
}

impl WeatherType {
    /// Exact Paper enum declaration order (`CLEAR`, `RAIN`, `THUNDER`).
    pub const VALUES: [Self; 3] = [Self::Clear, Self::Rain, Self::Thunder];

    /// Serialized name (`"clear"`, `"rain"`, `"thunder"`).
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Rain => "rain",
            Self::Thunder => "thunder",
        }
    }

    /// Parse from Paper's serialized name.
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "clear" => Some(Self::Clear),
            "rain" => Some(Self::Rain),
            "thunder" => Some(Self::Thunder),
            _ => None,
        }
    }

    /// Exact Paper `Weather.Type` parameters: `(clear_time, rain_time, raining, thundering)`.
    pub const fn weather_params(self) -> (i32, i32, bool, bool) {
        match self {
            Self::Clear => (100_000, 0, false, false),
            Self::Rain => (0, 100_000, true, false),
            Self::Thunder => (0, 100_000, true, true),
        }
    }
}

/// Typed value for `TestEnvironmentDefinition.SetGameRules` (`GameRuleMap`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameRuleValue {
    Bool(bool),
    Int(i32),
}

/// `net.minecraft.gametest.framework.TestEnvironmentDefinition` — all 7 Paper
/// variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestEnvironmentDefinition {
    /// `TestEnvironmentDefinition.AllOf(List<Holder<TestEnvironmentDefinition<?>>> definitions)`.
    /// Setup runs in list order; teardown runs in reverse activation order.
    AllOf {
        definitions: Vec<TestEnvironmentRef>,
    },
    /// `TestEnvironmentDefinition.ClockTime(Holder<WorldClock> clock, int time)`.
    ClockTime { clock: Identifier, time: i32 },
    /// `TestEnvironmentDefinition.Functions(Optional<Identifier> setupFunction, Optional<Identifier> teardownFunction)`.
    Functions {
        setup_function: Option<Identifier>,
        teardown_function: Option<Identifier>,
    },
    /// `TestEnvironmentDefinition.SetDifficulty(Difficulty difficulty)`.
    SetDifficulty { difficulty: Difficulty },
    /// `TestEnvironmentDefinition.SetGameRules(GameRuleMap gameRulesMap)`.
    /// Uses `IndexMap` to preserve observable JSON/NBT iteration order.
    SetGameRules {
        rules: IndexMap<Identifier, GameRuleValue>,
    },
    /// `TestEnvironmentDefinition.Timelines(HolderSet<Timeline> timelines)`.
    Timelines { timelines: Vec<Identifier> },
    /// `TestEnvironmentDefinition.Weather(Weather.Type weather)`.
    Weather { weather: WeatherType },
}

impl TestEnvironmentDefinition {
    /// Built-in type tag for this definition.
    pub const fn definition_type(&self) -> TestEnvironmentDefinitionType {
        match self {
            Self::AllOf { .. } => TestEnvironmentDefinitionType::AllOf,
            Self::ClockTime { .. } => TestEnvironmentDefinitionType::ClockTime,
            Self::Functions { .. } => TestEnvironmentDefinitionType::Function,
            Self::SetDifficulty { .. } => TestEnvironmentDefinitionType::Difficulty,
            Self::SetGameRules { .. } => TestEnvironmentDefinitionType::GameRules,
            Self::Timelines { .. } => TestEnvironmentDefinitionType::TimelineAttributes,
            Self::Weather { .. } => TestEnvironmentDefinitionType::Weather,
        }
    }

    /// For `AllOf`, returns the teardown order (exact reverse of setup order,
    /// matching `AllOf.setup` / `AllOf.teardown` in Paper).
    pub fn teardown_order_indices(&self) -> Vec<usize> {
        match self {
            Self::AllOf { definitions } => (0..definitions.len()).rev().collect(),
            _ => Vec::new(),
        }
    }

    /// Decode from Paper JSON matching `TestEnvironmentDefinition.DIRECT_CODEC`.
    pub fn decode_json(value: &JsonValue) -> Result<Self, EnvironmentDecodeError> {
        let obj = value
            .as_object()
            .ok_or_else(|| EnvironmentDecodeError::InvalidFieldType {
                field: "<root>",
                expected: "JSON object",
                actual: json_type_name(value),
            })?;

        let type_str = required_json_str(obj, "type")?;
        let type_id = parse_identifier_strict(type_str, "type")?;
        let def_type = TestEnvironmentDefinitionType::from_identifier(&type_id)
            .ok_or_else(|| EnvironmentDecodeError::UnknownType(type_id.to_string()))?;

        match def_type {
            TestEnvironmentDefinitionType::AllOf => {
                let arr = obj
                    .get("definitions")
                    .ok_or(EnvironmentDecodeError::MissingField("definitions"))?
                    .as_array()
                    .ok_or_else(|| EnvironmentDecodeError::InvalidFieldType {
                        field: "definitions",
                        expected: "JSON array",
                        actual: json_type_name(&obj["definitions"]),
                    })?;
                let mut definitions = Vec::with_capacity(arr.len());
                for item in arr {
                    definitions.push(TestEnvironmentRef::decode_json(item)?);
                }
                Ok(Self::AllOf { definitions })
            }
            TestEnvironmentDefinitionType::ClockTime => {
                let clock = parse_identifier_strict(required_json_str(obj, "clock")?, "clock")?;
                let time = required_json_i32(obj, "time")?;
                if time < 0 {
                    return Err(EnvironmentDecodeError::OutOfRange {
                        field: "time",
                        value: time,
                        expected: "non-negative integer (>= 0)",
                    });
                }
                Ok(Self::ClockTime { clock, time })
            }
            TestEnvironmentDefinitionType::Difficulty => {
                let diff_str = required_json_str(obj, "difficulty")?;
                let difficulty = Difficulty::by_name(diff_str).ok_or_else(|| {
                    EnvironmentDecodeError::InvalidDifficulty(diff_str.to_string())
                })?;
                Ok(Self::SetDifficulty { difficulty })
            }
            TestEnvironmentDefinitionType::Function => {
                let setup_function = match obj.get("setup") {
                    Some(JsonValue::String(s)) => Some(parse_identifier_strict(s, "setup")?),
                    Some(other) => {
                        return Err(EnvironmentDecodeError::InvalidFieldType {
                            field: "setup",
                            expected: "identifier string",
                            actual: json_type_name(other),
                        });
                    }
                    None => None,
                };
                let teardown_function = match obj.get("teardown") {
                    Some(JsonValue::String(s)) => Some(parse_identifier_strict(s, "teardown")?),
                    Some(other) => {
                        return Err(EnvironmentDecodeError::InvalidFieldType {
                            field: "teardown",
                            expected: "identifier string",
                            actual: json_type_name(other),
                        });
                    }
                    None => None,
                };
                Ok(Self::Functions {
                    setup_function,
                    teardown_function,
                })
            }
            TestEnvironmentDefinitionType::GameRules => {
                let rules_val = obj
                    .get("rules")
                    .ok_or(EnvironmentDecodeError::MissingField("rules"))?;
                let rules_obj = rules_val.as_object().ok_or_else(|| {
                    EnvironmentDecodeError::InvalidFieldType {
                        field: "rules",
                        expected: "JSON object",
                        actual: json_type_name(rules_val),
                    }
                })?;
                let mut rules = IndexMap::with_capacity(rules_obj.len());
                for (k, v) in rules_obj {
                    let rule_id = parse_identifier_strict(k, "rules")?;
                    let rule_val = match v {
                        JsonValue::Bool(b) => GameRuleValue::Bool(*b),
                        JsonValue::Number(n) => {
                            let i = n.as_i64().and_then(|x| i32::try_from(x).ok()).ok_or(
                                EnvironmentDecodeError::InvalidFieldType {
                                    field: "rules",
                                    expected: "bool or 32-bit integer",
                                    actual: "out-of-range number",
                                },
                            )?;
                            GameRuleValue::Int(i)
                        }
                        other => {
                            return Err(EnvironmentDecodeError::InvalidFieldType {
                                field: "rules",
                                expected: "bool or 32-bit integer",
                                actual: json_type_name(other),
                            });
                        }
                    };
                    rules.insert(rule_id, rule_val);
                }
                Ok(Self::SetGameRules { rules })
            }
            TestEnvironmentDefinitionType::TimelineAttributes => {
                let timelines_val = obj
                    .get("timelines")
                    .ok_or(EnvironmentDecodeError::MissingField("timelines"))?;
                let mut timelines = Vec::new();
                match timelines_val {
                    JsonValue::String(s) => {
                        timelines.push(parse_identifier_strict(s, "timelines")?);
                    }
                    JsonValue::Array(arr) => {
                        timelines.reserve(arr.len());
                        for item in arr {
                            let s = item.as_str().ok_or_else(|| {
                                EnvironmentDecodeError::InvalidFieldType {
                                    field: "timelines",
                                    expected: "identifier string",
                                    actual: json_type_name(item),
                                }
                            })?;
                            timelines.push(parse_identifier_strict(s, "timelines")?);
                        }
                    }
                    other => {
                        return Err(EnvironmentDecodeError::InvalidFieldType {
                            field: "timelines",
                            expected: "identifier string or array of identifier strings",
                            actual: json_type_name(other),
                        });
                    }
                }
                Ok(Self::Timelines { timelines })
            }
            TestEnvironmentDefinitionType::Weather => {
                let weather_str = required_json_str(obj, "weather")?;
                let weather = WeatherType::by_name(weather_str).ok_or_else(|| {
                    EnvironmentDecodeError::InvalidWeather(weather_str.to_string())
                })?;
                Ok(Self::Weather { weather })
            }
        }
    }

    /// Decode from Paper NBT matching `TestEnvironmentDefinition.DIRECT_CODEC`
    /// (used for `rivet_registry::generated::registry_data::SYNCHRONIZED_NBT`).
    pub fn decode_nbt(tag: &Tag) -> Result<Self, EnvironmentDecodeError> {
        let Tag::Compound(compound) = tag else {
            return Err(EnvironmentDecodeError::InvalidFieldType {
                field: "<root>",
                expected: "compound tag",
                actual: nbt_type_name(tag),
            });
        };

        let type_str = required_nbt_str(compound, "type")?;
        let type_id = parse_identifier_strict(type_str, "type")?;
        let def_type = TestEnvironmentDefinitionType::from_identifier(&type_id)
            .ok_or_else(|| EnvironmentDecodeError::UnknownType(type_id.to_string()))?;

        match def_type {
            TestEnvironmentDefinitionType::AllOf => {
                let list_tag = compound
                    .get("definitions")
                    .ok_or(EnvironmentDecodeError::MissingField("definitions"))?;
                let Tag::List(items) = list_tag else {
                    return Err(EnvironmentDecodeError::InvalidFieldType {
                        field: "definitions",
                        expected: "list tag",
                        actual: nbt_type_name(list_tag),
                    });
                };
                let mut definitions = Vec::with_capacity(items.list.len());
                for item in &items.list {
                    definitions.push(TestEnvironmentRef::decode_nbt(item)?);
                }
                Ok(Self::AllOf { definitions })
            }
            TestEnvironmentDefinitionType::ClockTime => {
                let clock = parse_identifier_strict(required_nbt_str(compound, "clock")?, "clock")?;
                let time = required_nbt_i32(compound, "time")?;
                if time < 0 {
                    return Err(EnvironmentDecodeError::OutOfRange {
                        field: "time",
                        value: time,
                        expected: "non-negative integer (>= 0)",
                    });
                }
                Ok(Self::ClockTime { clock, time })
            }
            TestEnvironmentDefinitionType::Difficulty => {
                let diff_str = required_nbt_str(compound, "difficulty")?;
                let difficulty = Difficulty::by_name(diff_str).ok_or_else(|| {
                    EnvironmentDecodeError::InvalidDifficulty(diff_str.to_string())
                })?;
                Ok(Self::SetDifficulty { difficulty })
            }
            TestEnvironmentDefinitionType::Function => {
                let setup_function = match compound.get("setup") {
                    Some(Tag::String(s)) => Some(parse_identifier_strict(&s.value, "setup")?),
                    Some(other) => {
                        return Err(EnvironmentDecodeError::InvalidFieldType {
                            field: "setup",
                            expected: "string tag",
                            actual: nbt_type_name(other),
                        });
                    }
                    None => None,
                };
                let teardown_function = match compound.get("teardown") {
                    Some(Tag::String(s)) => Some(parse_identifier_strict(&s.value, "teardown")?),
                    Some(other) => {
                        return Err(EnvironmentDecodeError::InvalidFieldType {
                            field: "teardown",
                            expected: "string tag",
                            actual: nbt_type_name(other),
                        });
                    }
                    None => None,
                };
                Ok(Self::Functions {
                    setup_function,
                    teardown_function,
                })
            }
            TestEnvironmentDefinitionType::GameRules => {
                let rules_tag = compound
                    .get("rules")
                    .ok_or(EnvironmentDecodeError::MissingField("rules"))?;
                let Tag::Compound(rules_compound) = rules_tag else {
                    return Err(EnvironmentDecodeError::InvalidFieldType {
                        field: "rules",
                        expected: "compound tag",
                        actual: nbt_type_name(rules_tag),
                    });
                };
                let mut rules = IndexMap::with_capacity(rules_compound.tags.len());
                for (k, v) in &rules_compound.tags {
                    let rule_id = parse_identifier_strict(k, "rules")?;
                    let rule_val = match v {
                        Tag::Byte(b) => GameRuleValue::Bool(b.value != 0),
                        Tag::Int(i) => GameRuleValue::Int(i.value),
                        other => {
                            return Err(EnvironmentDecodeError::InvalidFieldType {
                                field: "rules",
                                expected: "byte (bool) or int tag",
                                actual: nbt_type_name(other),
                            });
                        }
                    };
                    rules.insert(rule_id, rule_val);
                }
                Ok(Self::SetGameRules { rules })
            }
            TestEnvironmentDefinitionType::TimelineAttributes => {
                let timelines_tag = compound
                    .get("timelines")
                    .ok_or(EnvironmentDecodeError::MissingField("timelines"))?;
                let mut timelines = Vec::new();
                match timelines_tag {
                    Tag::String(s) => {
                        timelines.push(parse_identifier_strict(&s.value, "timelines")?);
                    }
                    Tag::List(items) => {
                        timelines.reserve(items.list.len());
                        for item in &items.list {
                            let Tag::String(s) = item else {
                                return Err(EnvironmentDecodeError::InvalidFieldType {
                                    field: "timelines",
                                    expected: "string tag",
                                    actual: nbt_type_name(item),
                                });
                            };
                            timelines.push(parse_identifier_strict(&s.value, "timelines")?);
                        }
                    }
                    other => {
                        return Err(EnvironmentDecodeError::InvalidFieldType {
                            field: "timelines",
                            expected: "string or list tag",
                            actual: nbt_type_name(other),
                        });
                    }
                }
                Ok(Self::Timelines { timelines })
            }
            TestEnvironmentDefinitionType::Weather => {
                let weather_str = required_nbt_str(compound, "weather")?;
                let weather = WeatherType::by_name(weather_str).ok_or_else(|| {
                    EnvironmentDecodeError::InvalidWeather(weather_str.to_string())
                })?;
                Ok(Self::Weather { weather })
            }
        }
    }
}

/// Fail-closed decoding error for `TestEnvironmentDefinition`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnvironmentDecodeError {
    #[error("missing required field `{0}`")]
    MissingField(&'static str),
    #[error("invalid type for field `{field}`: expected {expected}, got {actual}")]
    InvalidFieldType {
        field: &'static str,
        expected: &'static str,
        actual: &'static str,
    },
    #[error("invalid identifier in field `{field}`: `{value}`")]
    InvalidIdentifier { field: &'static str, value: String },
    #[error("unknown test_environment_definition_type: `{0}`")]
    UnknownType(String),
    #[error("invalid difficulty `{0}`")]
    InvalidDifficulty(String),
    #[error("invalid weather `{0}`")]
    InvalidWeather(String),
    #[error("field `{field}` value {value} out of range (expected {expected})")]
    OutOfRange {
        field: &'static str,
        value: i32,
        expected: &'static str,
    },
}

pub(crate) fn parse_identifier_strict(
    raw: &str,
    field: &'static str,
) -> Result<Identifier, EnvironmentDecodeError> {
    match Identifier::try_parse_result(raw) {
        Ok(Some(id)) => Ok(id),
        Ok(None) | Err(_) => Err(EnvironmentDecodeError::InvalidIdentifier {
            field,
            value: raw.to_string(),
        }),
    }
}

fn required_json_str<'a>(
    obj: &'a serde_json::Map<String, JsonValue>,
    field: &'static str,
) -> Result<&'a str, EnvironmentDecodeError> {
    let val = obj
        .get(field)
        .ok_or(EnvironmentDecodeError::MissingField(field))?;
    val.as_str()
        .ok_or_else(|| EnvironmentDecodeError::InvalidFieldType {
            field,
            expected: "string",
            actual: json_type_name(val),
        })
}

fn required_json_i32(
    obj: &serde_json::Map<String, JsonValue>,
    field: &'static str,
) -> Result<i32, EnvironmentDecodeError> {
    let val = obj
        .get(field)
        .ok_or(EnvironmentDecodeError::MissingField(field))?;
    val.as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| EnvironmentDecodeError::InvalidFieldType {
            field,
            expected: "32-bit integer",
            actual: json_type_name(val),
        })
}

fn required_nbt_str<'a>(
    compound: &'a CompoundTag,
    field: &'static str,
) -> Result<&'a str, EnvironmentDecodeError> {
    let tag = compound
        .get(field)
        .ok_or(EnvironmentDecodeError::MissingField(field))?;
    match tag {
        Tag::String(s) => Ok(s.value.as_str()),
        other => Err(EnvironmentDecodeError::InvalidFieldType {
            field,
            expected: "string tag",
            actual: nbt_type_name(other),
        }),
    }
}

fn required_nbt_i32(
    compound: &CompoundTag,
    field: &'static str,
) -> Result<i32, EnvironmentDecodeError> {
    let tag = compound
        .get(field)
        .ok_or(EnvironmentDecodeError::MissingField(field))?;
    match tag {
        Tag::Int(i) => Ok(i.value),
        other => Err(EnvironmentDecodeError::InvalidFieldType {
            field,
            expected: "int tag",
            actual: nbt_type_name(other),
        }),
    }
}

pub(crate) fn json_type_name(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

pub(crate) fn nbt_type_name(tag: &Tag) -> &'static str {
    match tag {
        Tag::End(_) => "end",
        Tag::Byte(_) => "byte",
        Tag::Short(_) => "short",
        Tag::Int(_) => "int",
        Tag::Long(_) => "long",
        Tag::Float(_) => "float",
        Tag::Double(_) => "double",
        Tag::ByteArray(_) => "byte_array",
        Tag::String(_) => "string",
        Tag::List(_) => "list",
        Tag::Compound(_) => "compound",
        Tag::IntArray(_) => "int_array",
        Tag::LongArray(_) => "long_array",
    }
}
