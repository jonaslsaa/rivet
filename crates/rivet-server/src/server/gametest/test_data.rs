//! `net.minecraft.gametest.framework.TestData` (Paper 26.2).
//!
//! Generic test metadata record parameterized by its environment reference type
//! (`EnvironmentType` in Paper: `Holder<TestEnvironmentDefinition<?>>` for
//! registered instances, `ResourceKey<TestEnvironmentDefinition<?>>` for
//! `GeneratedTest`, or [`TestEnvironmentRef`] when decoded from JSON/NBT).

use rivet_registry::Identifier;
use rivet_registry::core::Rotation;

/// Maximum `padding` accepted by Paper's `ExtraCodecs.intRange(0, 128)`.
pub const MAX_PADDING: i32 = 128;

/// `TestData<EnvironmentType>` — all 11 metadata fields with Paper's exact
/// defaults and validation bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestData<E> {
    pub environment: E,
    pub structure: Identifier,
    pub max_ticks: i32,
    pub setup_ticks: i32,
    pub required: bool,
    pub rotation: Rotation,
    pub manual_only: bool,
    pub max_attempts: i32,
    pub required_successes: i32,
    pub sky_access: bool,
    pub padding: i32,
}

impl<E> TestData<E> {
    /// Full constructor matching `TestData`'s canonical record constructor.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        environment: E,
        structure: Identifier,
        max_ticks: i32,
        setup_ticks: i32,
        required: bool,
        rotation: Rotation,
        manual_only: bool,
        max_attempts: i32,
        required_successes: i32,
        sky_access: bool,
        padding: i32,
    ) -> Self {
        Self {
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
        }
    }

    /// `new TestData(environment, structure, maxTicks, setupTicks, required, rotation)`
    /// with Paper's defaults (`manual_only = false`, `max_attempts = 1`,
    /// `required_successes = 1`, `sky_access = false`, `padding = 0`).
    pub const fn with_rotation(
        environment: E,
        structure: Identifier,
        max_ticks: i32,
        setup_ticks: i32,
        required: bool,
        rotation: Rotation,
    ) -> Self {
        Self::new(
            environment,
            structure,
            max_ticks,
            setup_ticks,
            required,
            rotation,
            false,
            1,
            1,
            false,
            0,
        )
    }

    /// `new TestData(environment, structure, maxTicks, setupTicks, required)`
    /// with `rotation = Rotation::None`.
    pub const fn with_defaults(
        environment: E,
        structure: Identifier,
        max_ticks: i32,
        setup_ticks: i32,
        required: bool,
    ) -> Self {
        Self::with_rotation(
            environment,
            structure,
            max_ticks,
            setup_ticks,
            required,
            Rotation::None,
        )
    }

    /// `TestData.map(Function<EnvironmentType, T>)`.
    pub fn map<T>(self, mapper: impl FnOnce(E) -> T) -> TestData<T> {
        TestData {
            environment: mapper(self.environment),
            structure: self.structure,
            max_ticks: self.max_ticks,
            setup_ticks: self.setup_ticks,
            required: self.required,
            rotation: self.rotation,
            manual_only: self.manual_only,
            max_attempts: self.max_attempts,
            required_successes: self.required_successes,
            sky_access: self.sky_access,
            padding: self.padding,
        }
    }

    /// Fallible variant of [`Self::map`] for resolving environment keys.
    pub fn try_map<T, Err>(
        self,
        mapper: impl FnOnce(E) -> Result<T, Err>,
    ) -> Result<TestData<T>, Err> {
        Ok(TestData {
            environment: mapper(self.environment)?,
            structure: self.structure,
            max_ticks: self.max_ticks,
            setup_ticks: self.setup_ticks,
            required: self.required,
            rotation: self.rotation,
            manual_only: self.manual_only,
            max_attempts: self.max_attempts,
            required_successes: self.required_successes,
            sky_access: self.sky_access,
            padding: self.padding,
        })
    }

    /// Validate all numeric range constraints enforced by `TestData.CODEC_WITH_TYPE`.
    pub fn validate(&self) -> Result<(), TestDataValidationError> {
        if self.max_ticks <= 0 {
            return Err(TestDataValidationError::OutOfRange {
                field: "max_ticks",
                value: self.max_ticks,
                expected: "positive integer (> 0)",
            });
        }
        if self.setup_ticks < 0 {
            return Err(TestDataValidationError::OutOfRange {
                field: "setup_ticks",
                value: self.setup_ticks,
                expected: "non-negative integer (>= 0)",
            });
        }
        if self.max_attempts <= 0 {
            return Err(TestDataValidationError::OutOfRange {
                field: "max_attempts",
                value: self.max_attempts,
                expected: "positive integer (> 0)",
            });
        }
        if self.required_successes <= 0 {
            return Err(TestDataValidationError::OutOfRange {
                field: "required_successes",
                value: self.required_successes,
                expected: "positive integer (> 0)",
            });
        }
        if !(0..=MAX_PADDING).contains(&self.padding) {
            return Err(TestDataValidationError::OutOfRange {
                field: "padding",
                value: self.padding,
                expected: "integer in 0..=128",
            });
        }
        Ok(())
    }
}

/// Validation failure on `TestData` numeric bounds.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TestDataValidationError {
    #[error("test_data field `{field}` value {value} out of range (expected {expected})")]
    OutOfRange {
        field: &'static str,
        value: i32,
        expected: &'static str,
    },
}

/// Parse Paper's `Rotation.CODEC` serialized name (`"none"`, `"clockwise_90"`,
/// `"180"`, `"counterclockwise_90"`).
pub fn parse_rotation(name: &str) -> Option<Rotation> {
    match name {
        "none" => Some(Rotation::None),
        "clockwise_90" => Some(Rotation::Clockwise90),
        "180" => Some(Rotation::Clockwise180),
        "counterclockwise_90" => Some(Rotation::Counterclockwise90),
        _ => None,
    }
}

/// Paper's `Rotation.getSerializedName()`.
pub const fn rotation_serialized_name(rotation: Rotation) -> &'static str {
    match rotation {
        Rotation::None => "none",
        Rotation::Clockwise90 => "clockwise_90",
        Rotation::Clockwise180 => "180",
        Rotation::Counterclockwise90 => "counterclockwise_90",
    }
}
