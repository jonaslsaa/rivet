//! `net.minecraft.gametest.framework` exception hierarchy and `GameTestEvent` (Paper 26.2).
//!
//! Mirrors:
//! - `GameTestException.java`
//! - `GameTestAssertException.java`
//! - `GameTestAssertPosException.java`
//! - `GameTestTimeoutException.java`
//! - `UnknownGameTestException.java`
//! - `ExhaustedAttemptsException.java`
//! - `GameTestEvent.java`

use std::fmt;

use rivet_registry::core::BlockPos;

/// `net.minecraft.gametest.framework.GameTestAssertException` (Paper 26.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameTestAssertException {
    pub message: String,
    pub tick: i32,
}

impl GameTestAssertException {
    pub fn new(message: impl Into<String>, tick: i32) -> Self {
        Self {
            message: message.into(),
            tick,
        }
    }

    /// Paper: `Component.translatable("test.error.tick", this.message, this.tick)`
    /// resolved against en_us (`"%s on tick %s"`).
    pub fn description(&self) -> String {
        format!("{} on tick {}", self.message, self.tick)
    }
}

impl fmt::Display for GameTestAssertException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.description())
    }
}

/// `net.minecraft.gametest.framework.GameTestAssertPosException` (Paper 26.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameTestAssertPosException {
    pub message: String,
    pub absolute_pos: BlockPos,
    pub relative_pos: BlockPos,
    pub tick: i32,
}

impl GameTestAssertPosException {
    pub fn new(
        message: impl Into<String>,
        absolute_pos: BlockPos,
        relative_pos: BlockPos,
        tick: i32,
    ) -> Self {
        Self {
            message: message.into(),
            absolute_pos,
            relative_pos,
            tick,
        }
    }

    /// Paper: `Component.translatable("test.error.position", message, absX, absY, absZ, relX, relY, relZ, tick)`
    /// resolved against en_us (`"%s at %s, %s, %s (relative: %s, %s, %s) on tick %s"`).
    pub fn description(&self) -> String {
        format!(
            "{} at {}, {}, {} (relative: {}, {}, {}) on tick {}",
            self.message,
            self.absolute_pos.get_x(),
            self.absolute_pos.get_y(),
            self.absolute_pos.get_z(),
            self.relative_pos.get_x(),
            self.relative_pos.get_y(),
            self.relative_pos.get_z(),
            self.tick
        )
    }

    /// Paper: `getMessageToShowAtBlock()`.
    pub fn message_to_show_at_block(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for GameTestAssertPosException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.description())
    }
}

/// `net.minecraft.gametest.framework.GameTestTimeoutException` (Paper 26.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameTestTimeoutException {
    pub message: String,
}

impl GameTestTimeoutException {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Paper: `Component.translatable("test.error.timeout.no_result", maxTicks)`
    /// resolved against en_us.
    pub fn no_result(max_ticks: i32) -> Self {
        Self::new(format!("Didn't succeed or fail within {max_ticks} ticks"))
    }

    /// Paper: `Component.translatable("test.error.timeout.no_sequences_finished", maxTicks)`
    /// resolved against en_us.
    pub fn no_sequences_finished(max_ticks: i32) -> Self {
        Self::new(format!("No sequences finished within {max_ticks} ticks"))
    }

    pub fn description(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for GameTestTimeoutException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// `net.minecraft.gametest.framework.UnknownGameTestException` (Paper 26.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownGameTestException {
    pub reason: String,
}

impl UnknownGameTestException {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    /// Paper: `Component.translatable("test.error.unknown", this.reason.getMessage())`
    /// resolved against en_us (`"Unknown internal error: %s"`).
    pub fn description(&self) -> String {
        format!("Unknown internal error: {}", self.reason)
    }
}

impl fmt::Display for UnknownGameTestException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

/// `net.minecraft.gametest.framework.ExhaustedAttemptsException` (Paper 26.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExhaustedAttemptsException {
    pub attempts: i32,
    pub successes: i32,
    pub required_successes: i32,
    pub max_attempts: i32,
    pub cause: Option<Box<GameTestException>>,
}

impl ExhaustedAttemptsException {
    pub fn new(
        attempts: i32,
        successes: i32,
        required_successes: i32,
        max_attempts: i32,
        cause: Option<GameTestException>,
    ) -> Self {
        Self {
            attempts,
            successes,
            required_successes,
            max_attempts,
            cause: cause.map(Box::new),
        }
    }

    /// Exact Paper message string from `ExhaustedAttemptsException.java`.
    pub fn message(&self) -> String {
        format!(
            "Not enough successes: {} out of {} attempts. Required successes: {}. max attempts: {}.",
            self.successes, self.attempts, self.required_successes, self.max_attempts
        )
    }
}

impl fmt::Display for ExhaustedAttemptsException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

/// Unified `net.minecraft.gametest.framework.GameTestException` hierarchy (Paper 26.2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GameTestException {
    #[error("{0}")]
    Assert(GameTestAssertException),
    #[error("{0}")]
    AssertPos(GameTestAssertPosException),
    #[error("{0}")]
    Timeout(GameTestTimeoutException),
    #[error("{0}")]
    Unknown(UnknownGameTestException),
    #[error("{0}")]
    ExhaustedAttempts(ExhaustedAttemptsException),
}

impl GameTestException {
    pub fn assert(message: impl Into<String>, tick: i32) -> Self {
        Self::Assert(GameTestAssertException::new(message, tick))
    }

    pub fn assert_pos(
        message: impl Into<String>,
        absolute_pos: BlockPos,
        relative_pos: BlockPos,
        tick: i32,
    ) -> Self {
        Self::AssertPos(GameTestAssertPosException::new(
            message,
            absolute_pos,
            relative_pos,
            tick,
        ))
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::Timeout(GameTestTimeoutException::new(message))
    }

    pub fn unknown(reason: impl Into<String>) -> Self {
        Self::Unknown(UnknownGameTestException::new(reason))
    }

    /// Paper `GameTestException.getDescription().getString()`.
    pub fn description(&self) -> String {
        match self {
            Self::Assert(e) => e.description(),
            Self::AssertPos(e) => e.description(),
            Self::Timeout(e) => e.description().to_owned(),
            Self::Unknown(e) => e.description(),
            Self::ExhaustedAttempts(e) => e.message(),
        }
    }

    /// Paper `Throwable.getMessage()` equivalent used by `ReportGameListener` and
    /// `JUnitLikeTestReporter`.
    pub fn message(&self) -> String {
        match self {
            Self::Assert(e) => e.description(),
            Self::AssertPos(e) => e.description(),
            Self::Timeout(e) => e.message.clone(),
            Self::Unknown(e) => e.reason.clone(),
            Self::ExhaustedAttempts(e) => e.message(),
        }
    }

    /// Optional underlying cause (matches `ExhaustedAttemptsException(..., testInfo.getError())`).
    pub fn cause(&self) -> Option<&GameTestException> {
        match self {
            Self::ExhaustedAttempts(e) => e.cause.as_deref(),
            _ => None,
        }
    }

    /// Root cause if this exception wraps a `GameTestAssertPosException`.
    pub fn root_assert_pos(&self) -> Option<&GameTestAssertPosException> {
        match self {
            Self::AssertPos(e) => Some(e),
            Self::ExhaustedAttempts(e) => e.cause.as_deref().and_then(Self::root_assert_pos),
            _ => None,
        }
    }
}

/// `net.minecraft.gametest.framework.GameTestEvent` (Paper 26.2).
///
/// Represents a single step inside a `GameTestSequence`.
pub struct GameTestEvent<F> {
    pub expected_delay: Option<i64>,
    pub minimum_delay: Option<i64>,
    pub assertion: F,
}

impl<F> GameTestEvent<F> {
    pub fn create(assertion: F) -> Self {
        Self {
            expected_delay: None,
            minimum_delay: None,
            assertion,
        }
    }

    pub fn create_at(expected_tick: i64, assertion: F) -> Self {
        Self {
            expected_delay: Some(expected_tick),
            minimum_delay: None,
            assertion,
        }
    }

    pub fn create_with_minimum_delay(minimum_delay: i64, assertion: F) -> Self {
        Self {
            expected_delay: None,
            minimum_delay: Some(minimum_delay),
            assertion,
        }
    }
}
