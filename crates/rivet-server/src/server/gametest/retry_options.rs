//! `net.minecraft.gametest.framework.RetryOptions` (Paper 26.2).
//!
//! Pure value type controlling retry counts and failure halting for GameTest
//! runs. Retry execution itself is deferred to issue #430.

/// `RetryOptions(int numberOfTries, boolean haltOnFailure)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RetryOptions {
    pub number_of_tries: i32,
    pub halt_on_failure: bool,
}

impl RetryOptions {
    /// `RetryOptions.NO_RETRIES = new RetryOptions(1, true)`.
    pub const NO_RETRIES: Self = Self {
        number_of_tries: 1,
        halt_on_failure: true,
    };

    /// `new RetryOptions(numberOfTries, haltOnFailure)`.
    pub const fn new(number_of_tries: i32, halt_on_failure: bool) -> Self {
        Self {
            number_of_tries,
            halt_on_failure,
        }
    }

    /// `RetryOptions.noRetries()`.
    pub const fn no_retries() -> Self {
        Self::NO_RETRIES
    }

    /// `RetryOptions.unlimitedTries()` — `this.numberOfTries < 1`.
    pub const fn unlimited_tries(&self) -> bool {
        self.number_of_tries < 1
    }

    /// `RetryOptions.hasTriesLeft(int attempts, int successes)`.
    ///
    /// Exact Paper semantics:
    /// ```java
    /// boolean hasFailures = attempts != successes;
    /// boolean hasMoreAttempts = this.unlimitedTries() || attempts < this.numberOfTries;
    /// return hasMoreAttempts && (!hasFailures || !this.haltOnFailure);
    /// ```
    pub const fn has_tries_left(&self, attempts: i32, successes: i32) -> bool {
        let has_failures = attempts != successes;
        let has_more_attempts = self.unlimited_tries() || attempts < self.number_of_tries;
        has_more_attempts && (!has_failures || !self.halt_on_failure)
    }

    /// `RetryOptions.hasRetries()` — `this.numberOfTries != 1`.
    pub const fn has_retries(&self) -> bool {
        self.number_of_tries != 1
    }
}

impl Default for RetryOptions {
    fn default() -> Self {
        Self::NO_RETRIES
    }
}
