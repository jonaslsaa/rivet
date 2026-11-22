//! `net.minecraft.gametest.framework.MultipleTestTracker` (Paper 26.2).
//!
//! Mirrors `MultipleTestTracker.java`: tracks a collection of `GameTestInfo`
//! instances, computes required/optional failure counts, completion state,
//! and Paper's exact ASCII progress bar (`[ _+xX]`).

use crate::server::gametest::game_test_info::GameTestInfo;
use crate::server::gametest::game_test_ticker::TestInfoId;

/// `net.minecraft.gametest.framework.MultipleTestTracker` (Paper 26.2).
#[derive(Debug, Default, Clone)]
pub struct MultipleTestTracker {
    tests: Vec<TestInfoId>,
}

impl MultipleTestTracker {
    pub const NOT_STARTED_TEST_CHAR: char = ' ';
    pub const ONGOING_TEST_CHAR: char = '_';
    pub const SUCCESSFUL_TEST_CHAR: char = '+';
    pub const FAILED_OPTIONAL_TEST_CHAR: char = 'x';
    pub const FAILED_REQUIRED_TEST_CHAR: char = 'X';

    pub fn new() -> Self {
        Self { tests: Vec::new() }
    }

    pub fn from_tests(tests: impl IntoIterator<Item = TestInfoId>) -> Self {
        Self {
            tests: tests.into_iter().collect(),
        }
    }

    pub fn add_test_to_track(&mut self, id: TestInfoId) {
        self.tests.push(id);
    }

    pub fn tests(&self) -> &[TestInfoId] {
        &self.tests
    }

    pub fn total_count(&self) -> usize {
        self.tests.len()
    }

    pub fn failed_required_count(&self, arena: &[GameTestInfo]) -> usize {
        self.tests
            .iter()
            .filter(|id| {
                let info = &arena[id.0];
                info.has_failed() && info.is_required()
            })
            .count()
    }

    pub fn failed_optional_count(&self, arena: &[GameTestInfo]) -> usize {
        self.tests
            .iter()
            .filter(|id| {
                let info = &arena[id.0];
                info.has_failed() && info.is_optional()
            })
            .count()
    }

    pub fn done_count(&self, arena: &[GameTestInfo]) -> usize {
        self.tests.iter().filter(|id| arena[id.0].is_done()).count()
    }

    pub fn has_failed_required(&self, arena: &[GameTestInfo]) -> bool {
        self.failed_required_count(arena) > 0
    }

    pub fn has_failed_optional(&self, arena: &[GameTestInfo]) -> bool {
        self.failed_optional_count(arena) > 0
    }

    pub fn failed_required(&self, arena: &[GameTestInfo]) -> Vec<TestInfoId> {
        self.tests
            .iter()
            .copied()
            .filter(|id| {
                let info = &arena[id.0];
                info.has_failed() && info.is_required()
            })
            .collect()
    }

    pub fn failed_optional(&self, arena: &[GameTestInfo]) -> Vec<TestInfoId> {
        self.tests
            .iter()
            .copied()
            .filter(|id| {
                let info = &arena[id.0];
                info.has_failed() && info.is_optional()
            })
            .collect()
    }

    pub fn is_done(&self, arena: &[GameTestInfo]) -> bool {
        self.done_count(arena) == self.total_count()
    }

    /// Paper: `MultipleTestTracker.getProgressBar()`.
    ///
    /// Format: `[` followed by one character per test (` `, `+`, `X`, `x`, `_`),
    /// then `]`.
    pub fn progress_bar(&self, arena: &[GameTestInfo]) -> String {
        let mut buf = String::with_capacity(self.tests.len() + 2);
        buf.push('[');
        for id in &self.tests {
            let info = &arena[id.0];
            let ch = if !info.has_started() {
                Self::NOT_STARTED_TEST_CHAR
            } else if info.has_succeeded() {
                Self::SUCCESSFUL_TEST_CHAR
            } else if info.has_failed() {
                if info.is_required() {
                    Self::FAILED_REQUIRED_TEST_CHAR
                } else {
                    Self::FAILED_OPTIONAL_TEST_CHAR
                }
            } else {
                Self::ONGOING_TEST_CHAR
            };
            buf.push(ch);
        }
        buf.push(']');
        buf
    }

    /// Paper: `MultipleTestTracker.toString()`.
    pub fn summary_string(&self, arena: &[GameTestInfo]) -> String {
        let done = self.done_count(arena);
        let total = self.total_count();
        let failed_req = self.failed_required_count(arena);
        let failed_opt = self.failed_optional_count(arena);
        let mut s = format!("{done}/{total} tests completed");
        if failed_req > 0 {
            s.push_str(&format!(" ({failed_req} failed)"));
        }
        if failed_opt > 0 {
            s.push_str(&format!(" ({failed_opt} optional failed)"));
        }
        s.push(' ');
        s.push_str(&self.progress_bar(arena));
        s
    }
}
