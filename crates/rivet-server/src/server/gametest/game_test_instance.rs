//! `net.minecraft.gametest.framework.GameTestInstance`,
//! `BlockBasedTestInstance`, `FunctionGameTestInstance`, and
//! `GameTestInstances` (Paper 26.2).
//!
//! Runtime execution (`run(GameTestHelper)`) is deferred to issues #430/#474/#475.

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::tag::Tag;
use rivet_registry::core::Rotation;
use rivet_registry::{Identifier, ResourceKey};
use serde_json::Value as JsonValue;

use crate::server::gametest::registries::{TEST_FUNCTION, TEST_INSTANCE, TEST_INSTANCE_TYPE};
use crate::server::gametest::test_data::{
    TestData, TestDataValidationError, parse_rotation, rotation_serialized_name,
};
use crate::server::gametest::test_environment_definition::{
    EnvironmentDecodeError, TestEnvironmentRef, default_environment_key, json_type_name,
    nbt_type_name, parse_identifier_strict,
};
use crate::server::gametest::test_function_loader::{
    ALWAYS_PASS_FUNCTION_NAME, TestFunctionEntry, always_pass_function_key,
};

/// `GameTestInstances.ALWAYS_PASS` path (`"always_pass"`).
pub const ALWAYS_PASS_INSTANCE_NAME: &str = "always_pass";

/// `minecraft:empty` structure referenced by `GameTestInstances.ALWAYS_PASS`.
pub const EMPTY_STRUCTURE_NAME: &str = "empty";

/// `GameTestInstances.ALWAYS_PASS` resource key (`minecraft:test_instance` / `minecraft:always_pass`).
pub fn always_pass_instance_key() -> ResourceKey<GameTestInstance> {
    ResourceKey::create(
        &TEST_INSTANCE,
        Identifier::with_default_namespace(ALWAYS_PASS_INSTANCE_NAME),
    )
}

/// `GameTestInstances.create(String)` — key in `minecraft:test_instance`.
pub fn instance_key(name: &str) -> ResourceKey<GameTestInstance> {
    ResourceKey::create(&TEST_INSTANCE, Identifier::with_default_namespace(name))
}

/// Canonical `GameTestInstances.bootstrap` entry for `minecraft:always_pass`:
/// ```java
/// new FunctionGameTestInstance(
///     BuiltinTestFunctions.ALWAYS_PASS,
///     new TestData<>(defaultEnv, Identifier.withDefaultNamespace("empty"), 1, 1, false)
/// )
/// ```
pub fn always_pass_instance() -> GameTestInstance {
    GameTestInstance::Function(FunctionGameTestInstance::new(
        always_pass_function_key(),
        TestData::with_defaults(
            TestEnvironmentRef::Key(default_environment_key()),
            Identifier::with_default_namespace(EMPTY_STRUCTURE_NAME),
            1,
            1,
            false,
        ),
    ))
}

/// Built-in keys registered by `GameTestInstance.bootstrap(Registry)` in exact
/// Paper bootstrap order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameTestInstanceType {
    BlockBased,
    Function,
}

impl GameTestInstanceType {
    /// Exact Paper bootstrap order (`GameTestInstance.java:21-24`).
    pub const BOOTSTRAP_ORDER: [Self; 2] = [Self::BlockBased, Self::Function];

    /// Serialized path in the `minecraft:` namespace.
    pub const fn path(self) -> &'static str {
        match self {
            Self::BlockBased => "block_based",
            Self::Function => "function",
        }
    }

    /// Canonical `Identifier` (`minecraft:<path>`).
    pub fn identifier(self) -> Identifier {
        Identifier::with_default_namespace(self.path())
    }

    /// Canonical `ResourceKey` in `minecraft:test_instance_type`.
    pub fn resource_key(self) -> ResourceKey<GameTestInstanceType> {
        ResourceKey::create(&TEST_INSTANCE_TYPE, self.identifier())
    }

    /// Parse from an `Identifier` (must be in `minecraft:` namespace).
    pub fn from_identifier(id: &Identifier) -> Option<Self> {
        if id.namespace() != "minecraft" {
            return None;
        }
        match id.path() {
            "block_based" => Some(Self::BlockBased),
            "function" => Some(Self::Function),
            _ => None,
        }
    }
}

/// `net.minecraft.gametest.framework.BlockBasedTestInstance`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockBasedTestInstance {
    info: TestData<TestEnvironmentRef>,
}

impl BlockBasedTestInstance {
    pub const fn new(info: TestData<TestEnvironmentRef>) -> Self {
        Self { info }
    }

    pub const fn info(&self) -> &TestData<TestEnvironmentRef> {
        &self.info
    }

    /// `BlockBasedTestInstance.typeDescription()` translation key.
    pub const fn type_description_key() -> &'static str {
        "test_instance.type.block_based"
    }
}

/// `net.minecraft.gametest.framework.FunctionGameTestInstance`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionGameTestInstance {
    function: ResourceKey<TestFunctionEntry>,
    info: TestData<TestEnvironmentRef>,
}

impl FunctionGameTestInstance {
    pub const fn new(
        function: ResourceKey<TestFunctionEntry>,
        info: TestData<TestEnvironmentRef>,
    ) -> Self {
        Self { function, info }
    }

    pub const fn function(&self) -> &ResourceKey<TestFunctionEntry> {
        &self.function
    }

    pub const fn info(&self) -> &TestData<TestEnvironmentRef> {
        &self.info
    }

    /// `FunctionGameTestInstance.typeDescription()` translation key.
    pub const fn type_description_key() -> &'static str {
        "test_instance.type.function"
    }

    /// Exact Paper missing-function error message:
    /// `"Trying to access missing test function: " + this.function.identifier()`.
    pub fn missing_function_message(&self) -> String {
        format!(
            "Trying to access missing test function: {}",
            self.function.identifier()
        )
    }
}

/// `net.minecraft.gametest.framework.GameTestInstance` — closed sum over the
/// two built-in instance codecs (`block_based`, `function`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameTestInstance {
    BlockBased(BlockBasedTestInstance),
    Function(FunctionGameTestInstance),
}

impl GameTestInstance {
    pub const fn instance_type(&self) -> GameTestInstanceType {
        match self {
            Self::BlockBased(_) => GameTestInstanceType::BlockBased,
            Self::Function(_) => GameTestInstanceType::Function,
        }
    }

    pub const fn info(&self) -> &TestData<TestEnvironmentRef> {
        match self {
            Self::BlockBased(inst) => inst.info(),
            Self::Function(inst) => inst.info(),
        }
    }

    /// `GameTestInstance.batch()` — the environment reference.
    pub const fn batch(&self) -> &TestEnvironmentRef {
        &self.info().environment
    }

    /// `GameTestInstance.structure()`.
    pub const fn structure(&self) -> &Identifier {
        &self.info().structure
    }

    /// `GameTestInstance.maxTicks()`.
    pub const fn max_ticks(&self) -> i32 {
        self.info().max_ticks
    }

    /// `GameTestInstance.setupTicks()`.
    pub const fn setup_ticks(&self) -> i32 {
        self.info().setup_ticks
    }

    /// `GameTestInstance.required()`.
    pub const fn required(&self) -> bool {
        self.info().required
    }

    /// `GameTestInstance.manualOnly()`.
    pub const fn manual_only(&self) -> bool {
        self.info().manual_only
    }

    /// `GameTestInstance.maxAttempts()`.
    pub const fn max_attempts(&self) -> i32 {
        self.info().max_attempts
    }

    /// `GameTestInstance.requiredSuccesses()`.
    pub const fn required_successes(&self) -> i32 {
        self.info().required_successes
    }

    /// `GameTestInstance.skyAccess()`.
    pub const fn sky_access(&self) -> bool {
        self.info().sky_access
    }

    /// `GameTestInstance.rotation()`.
    pub const fn rotation(&self) -> Rotation {
        self.info().rotation
    }

    /// `GameTestInstance.padding()`.
    pub const fn padding(&self) -> i32 {
        self.info().padding
    }

    /// `GameTestInstance.typeDescription()` translation key.
    pub const fn type_description_key(&self) -> &'static str {
        match self {
            Self::BlockBased(_) => BlockBasedTestInstance::type_description_key(),
            Self::Function(_) => FunctionGameTestInstance::type_description_key(),
        }
    }

    /// If this is a `Function` instance, returns its test function key.
    pub const fn function_key(&self) -> Option<&ResourceKey<TestFunctionEntry>> {
        match self {
            Self::BlockBased(_) => None,
            Self::Function(inst) => Some(inst.function()),
        }
    }

    /// Decode from Paper JSON matching `GameTestInstance.DIRECT_CODEC`.
    pub fn decode_json(value: &JsonValue) -> Result<Self, InstanceDecodeError> {
        let obj = value
            .as_object()
            .ok_or_else(|| InstanceDecodeError::InvalidFieldType {
                field: "<root>",
                expected: "JSON object",
                actual: json_type_name(value),
            })?;

        let type_str = required_json_str(obj, "type")?;
        let type_id = parse_identifier_strict(type_str, "type")?;
        let inst_type = GameTestInstanceType::from_identifier(&type_id)
            .ok_or_else(|| InstanceDecodeError::UnknownType(type_id.to_string()))?;

        let info = decode_test_data_json(obj)?;
        info.validate()?;

        match inst_type {
            GameTestInstanceType::BlockBased => {
                Ok(Self::BlockBased(BlockBasedTestInstance::new(info)))
            }
            GameTestInstanceType::Function => {
                let func_str = required_json_str(obj, "function")?;
                let func_id = parse_identifier_strict(func_str, "function")?;
                let func_key = ResourceKey::create(&TEST_FUNCTION, func_id);
                Ok(Self::Function(FunctionGameTestInstance::new(
                    func_key, info,
                )))
            }
        }
    }

    /// Decode from Paper NBT matching `GameTestInstance.DIRECT_CODEC`
    /// (used for `rivet_registry::generated::registry_data::SYNCHRONIZED_NBT`).
    pub fn decode_nbt(tag: &Tag) -> Result<Self, InstanceDecodeError> {
        let Tag::Compound(compound) = tag else {
            return Err(InstanceDecodeError::InvalidFieldType {
                field: "<root>",
                expected: "compound tag",
                actual: nbt_type_name(tag),
            });
        };

        let type_str = required_nbt_str(compound, "type")?;
        let type_id = parse_identifier_strict(type_str, "type")?;
        let inst_type = GameTestInstanceType::from_identifier(&type_id)
            .ok_or_else(|| InstanceDecodeError::UnknownType(type_id.to_string()))?;

        let info = decode_test_data_nbt(compound)?;
        info.validate()?;

        match inst_type {
            GameTestInstanceType::BlockBased => {
                Ok(Self::BlockBased(BlockBasedTestInstance::new(info)))
            }
            GameTestInstanceType::Function => {
                let func_str = required_nbt_str(compound, "function")?;
                let func_id = parse_identifier_strict(func_str, "function")?;
                let func_key = ResourceKey::create(&TEST_FUNCTION, func_id);
                Ok(Self::Function(FunctionGameTestInstance::new(
                    func_key, info,
                )))
            }
        }
    }

    /// Return the default `ALWAYS_PASS` function name for diagnostics.
    pub const fn always_pass_function_name() -> &'static str {
        ALWAYS_PASS_FUNCTION_NAME
    }
}

fn decode_test_data_json(
    obj: &serde_json::Map<String, JsonValue>,
) -> Result<TestData<TestEnvironmentRef>, InstanceDecodeError> {
    let env_val = obj
        .get("environment")
        .ok_or(InstanceDecodeError::MissingField("environment"))?;
    let environment = TestEnvironmentRef::decode_json(env_val)?;

    let structure = parse_identifier_strict(required_json_str(obj, "structure")?, "structure")?;
    let max_ticks = required_json_i32(obj, "max_ticks")?;
    let setup_ticks = optional_json_i32(obj, "setup_ticks", 0)?;
    let required = optional_json_bool(obj, "required", true)?;
    let rotation = match obj.get("rotation") {
        Some(v) => {
            let s = v
                .as_str()
                .ok_or_else(|| InstanceDecodeError::InvalidFieldType {
                    field: "rotation",
                    expected: "string",
                    actual: json_type_name(v),
                })?;
            parse_rotation(s).ok_or_else(|| InstanceDecodeError::InvalidRotation(s.to_string()))?
        }
        None => Rotation::None,
    };
    let manual_only = optional_json_bool(obj, "manual_only", false)?;
    let max_attempts = optional_json_i32(obj, "max_attempts", 1)?;
    let required_successes = optional_json_i32(obj, "required_successes", 1)?;
    let sky_access = optional_json_bool(obj, "sky_access", false)?;
    let padding = optional_json_i32(obj, "padding", 0)?;

    // Keep rotation_serialized_name referenced so serialization stays aligned.
    let _ = rotation_serialized_name(rotation);

    Ok(TestData::new(
        environment,
        structure,
        max_ticks,
        setup_ticks,
        required,
        rotation,
        manual_only,
        max_attempts,
        required_successes,
        sky_access,
        padding,
    ))
}

fn decode_test_data_nbt(
    compound: &CompoundTag,
) -> Result<TestData<TestEnvironmentRef>, InstanceDecodeError> {
    let env_tag = compound
        .get("environment")
        .ok_or(InstanceDecodeError::MissingField("environment"))?;
    let environment = TestEnvironmentRef::decode_nbt(env_tag)?;

    let structure = parse_identifier_strict(required_nbt_str(compound, "structure")?, "structure")?;
    let max_ticks = required_nbt_i32(compound, "max_ticks")?;
    let setup_ticks = optional_nbt_i32(compound, "setup_ticks", 0)?;
    let required = optional_nbt_bool(compound, "required", true)?;
    let rotation = match compound.get("rotation") {
        Some(Tag::String(s)) => parse_rotation(&s.value)
            .ok_or_else(|| InstanceDecodeError::InvalidRotation(s.value.clone()))?,
        Some(other) => {
            return Err(InstanceDecodeError::InvalidFieldType {
                field: "rotation",
                expected: "string tag",
                actual: nbt_type_name(other),
            });
        }
        None => Rotation::None,
    };
    let manual_only = optional_nbt_bool(compound, "manual_only", false)?;
    let max_attempts = optional_nbt_i32(compound, "max_attempts", 1)?;
    let required_successes = optional_nbt_i32(compound, "required_successes", 1)?;
    let sky_access = optional_nbt_bool(compound, "sky_access", false)?;
    let padding = optional_nbt_i32(compound, "padding", 0)?;

    Ok(TestData::new(
        environment,
        structure,
        max_ticks,
        setup_ticks,
        required,
        rotation,
        manual_only,
        max_attempts,
        required_successes,
        sky_access,
        padding,
    ))
}

/// Fail-closed decoding error for `GameTestInstance`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstanceDecodeError {
    #[error("missing required field `{0}`")]
    MissingField(&'static str),
    #[error("invalid type for field `{field}`: expected {expected}, got {actual}")]
    InvalidFieldType {
        field: &'static str,
        expected: &'static str,
        actual: &'static str,
    },
    #[error("unknown test_instance_type: `{0}`")]
    UnknownType(String),
    #[error("invalid rotation `{0}`")]
    InvalidRotation(String),
    #[error(transparent)]
    Environment(#[from] EnvironmentDecodeError),
    #[error(transparent)]
    Validation(#[from] TestDataValidationError),
}

fn required_json_str<'a>(
    obj: &'a serde_json::Map<String, JsonValue>,
    field: &'static str,
) -> Result<&'a str, InstanceDecodeError> {
    let val = obj
        .get(field)
        .ok_or(InstanceDecodeError::MissingField(field))?;
    val.as_str()
        .ok_or_else(|| InstanceDecodeError::InvalidFieldType {
            field,
            expected: "string",
            actual: json_type_name(val),
        })
}

fn required_json_i32(
    obj: &serde_json::Map<String, JsonValue>,
    field: &'static str,
) -> Result<i32, InstanceDecodeError> {
    let val = obj
        .get(field)
        .ok_or(InstanceDecodeError::MissingField(field))?;
    val.as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| InstanceDecodeError::InvalidFieldType {
            field,
            expected: "32-bit integer",
            actual: json_type_name(val),
        })
}

fn optional_json_i32(
    obj: &serde_json::Map<String, JsonValue>,
    field: &'static str,
    default: i32,
) -> Result<i32, InstanceDecodeError> {
    match obj.get(field) {
        Some(val) => val
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| InstanceDecodeError::InvalidFieldType {
                field,
                expected: "32-bit integer",
                actual: json_type_name(val),
            }),
        None => Ok(default),
    }
}

fn optional_json_bool(
    obj: &serde_json::Map<String, JsonValue>,
    field: &'static str,
    default: bool,
) -> Result<bool, InstanceDecodeError> {
    match obj.get(field) {
        Some(val) => val
            .as_bool()
            .ok_or_else(|| InstanceDecodeError::InvalidFieldType {
                field,
                expected: "boolean",
                actual: json_type_name(val),
            }),
        None => Ok(default),
    }
}

fn required_nbt_str<'a>(
    compound: &'a CompoundTag,
    field: &'static str,
) -> Result<&'a str, InstanceDecodeError> {
    let tag = compound
        .get(field)
        .ok_or(InstanceDecodeError::MissingField(field))?;
    match tag {
        Tag::String(s) => Ok(s.value.as_str()),
        other => Err(InstanceDecodeError::InvalidFieldType {
            field,
            expected: "string tag",
            actual: nbt_type_name(other),
        }),
    }
}

fn required_nbt_i32(
    compound: &CompoundTag,
    field: &'static str,
) -> Result<i32, InstanceDecodeError> {
    let tag = compound
        .get(field)
        .ok_or(InstanceDecodeError::MissingField(field))?;
    tag.as_number()
        .map(|n| n.int_value())
        .ok_or_else(|| InstanceDecodeError::InvalidFieldType {
            field,
            expected: "numeric tag",
            actual: nbt_type_name(tag),
        })
}

fn optional_nbt_i32(
    compound: &CompoundTag,
    field: &'static str,
    default: i32,
) -> Result<i32, InstanceDecodeError> {
    match compound.get(field) {
        Some(tag) => tag.as_number().map(|n| n.int_value()).ok_or_else(|| {
            InstanceDecodeError::InvalidFieldType {
                field,
                expected: "numeric tag",
                actual: nbt_type_name(tag),
            }
        }),
        None => Ok(default),
    }
}

fn optional_nbt_bool(
    compound: &CompoundTag,
    field: &'static str,
    default: bool,
) -> Result<bool, InstanceDecodeError> {
    match compound.get(field) {
        Some(tag) => tag
            .as_number()
            .map(|n| n.double_value() != 0.0)
            .ok_or_else(|| InstanceDecodeError::InvalidFieldType {
                field,
                expected: "numeric (bool) tag",
                actual: nbt_type_name(tag),
            }),
        None => Ok(default),
    }
}
