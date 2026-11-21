//! `net.minecraft.gametest.framework.FailedTestTracker`,
//! `TestInstanceFinder`, `TestPosFinder`, and `TestFinder` (Paper 26.2).
//!
//! Pure discovery value/query layer without runtime execution or world block
//! entity lookup (deferred to issues #430 and #474).

use rivet_registry::Holder;
use rivet_registry::core::BlockPos;

use crate::server::gametest::game_test_instance::GameTestInstance;
use crate::server::gametest::registries::GameTestRegistries;

/// `net.minecraft.gametest.framework.FailedTestTracker` (Paper 26.2).
///
/// Tracks failed test instance holders in deterministic insertion order
/// without duplicates (Paper uses `Sets.newHashSet()`, which is
/// non-deterministic; preserving insertion order is deterministic per PORTING.md).
#[derive(Debug, Default, Clone)]
pub struct FailedTestTracker {
    last_failed_tests: Vec<Holder<GameTestInstance>>,
}

impl FailedTestTracker {
    pub fn new() -> Self {
        Self {
            last_failed_tests: Vec::new(),
        }
    }

    /// `FailedTestTracker.getLastFailedTests()`.
    pub fn last_failed_tests(&self) -> impl Iterator<Item = &Holder<GameTestInstance>> {
        self.last_failed_tests.iter()
    }

    /// `FailedTestTracker.rememberFailedTest(Holder.Reference<GameTestInstance>)`.
    pub fn remember_failed_test(&mut self, test: Holder<GameTestInstance>) {
        if !self.last_failed_tests.contains(&test) {
            self.last_failed_tests.push(test);
        }
    }

    /// `FailedTestTracker.forgetFailedTests()`.
    pub fn forget_failed_tests(&mut self) {
        self.last_failed_tests.clear();
    }

    pub fn len(&self) -> usize {
        self.last_failed_tests.len()
    }

    pub fn is_empty(&self) -> bool {
        self.last_failed_tests.is_empty()
    }
}

/// `net.minecraft.gametest.framework.TestInstanceFinder`.
pub trait TestInstanceFinder {
    fn find_tests(&self) -> Vec<Holder<GameTestInstance>>;
}

/// `net.minecraft.gametest.framework.TestPosFinder`.
pub trait TestPosFinder {
    fn find_test_pos(&self) -> Vec<BlockPos>;
}

/// Empty finder returning no tests and no positions (`TestFinder.NO_FUNCTIONS` /
/// `TestFinder.NO_STRUCTURES`).
#[derive(Debug, Default, Clone, Copy)]
pub struct EmptyFinder;

impl TestInstanceFinder for EmptyFinder {
    fn find_tests(&self) -> Vec<Holder<GameTestInstance>> {
        Vec::new()
    }
}

impl TestPosFinder for EmptyFinder {
    fn find_test_pos(&self) -> Vec<BlockPos> {
        Vec::new()
    }
}

/// Static list-backed [`TestInstanceFinder`].
#[derive(Debug, Clone, Default)]
pub struct StaticTestInstanceFinder {
    tests: Vec<Holder<GameTestInstance>>,
}

impl StaticTestInstanceFinder {
    pub fn new(tests: Vec<Holder<GameTestInstance>>) -> Self {
        Self { tests }
    }
}

impl TestInstanceFinder for StaticTestInstanceFinder {
    fn find_tests(&self) -> Vec<Holder<GameTestInstance>> {
        self.tests.clone()
    }
}

/// Static list-backed [`TestPosFinder`].
#[derive(Debug, Clone, Default)]
pub struct StaticTestPosFinder {
    positions: Vec<BlockPos>,
}

impl StaticTestPosFinder {
    pub fn new(positions: Vec<BlockPos>) -> Self {
        Self { positions }
    }
}

impl TestPosFinder for StaticTestPosFinder {
    fn find_test_pos(&self) -> Vec<BlockPos> {
        self.positions.clone()
    }
}

/// `net.minecraft.gametest.framework.TestFinder` — pairs a
/// [`TestInstanceFinder`] with a [`TestPosFinder`].
pub struct TestFinder {
    test_instance_finder: Box<dyn TestInstanceFinder>,
    test_pos_finder: Box<dyn TestPosFinder>,
}

impl TestFinder {
    pub fn new(
        test_instance_finder: impl TestInstanceFinder + 'static,
        test_pos_finder: impl TestPosFinder + 'static,
    ) -> Self {
        Self {
            test_instance_finder: Box::new(test_instance_finder),
            test_pos_finder: Box::new(test_pos_finder),
        }
    }

    pub fn empty() -> Self {
        Self::new(EmptyFinder, EmptyFinder)
    }

    /// `TestFinder.findTests()`.
    pub fn find_tests(&self) -> Vec<Holder<GameTestInstance>> {
        self.test_instance_finder.find_tests()
    }

    /// `TestFinder.findTestPos()`.
    pub fn find_test_pos(&self) -> Vec<BlockPos> {
        self.test_pos_finder.find_test_pos()
    }
}

/// `TestFinder.Builder` — deterministic builder matching Paper's
/// `createMultipleCopies(int)`, `byResourceSelection`, `failedTests`, and
/// position-based discovery.
pub struct TestFinderBuilder {
    copies: usize,
}

impl Default for TestFinderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TestFinderBuilder {
    pub const fn new() -> Self {
        Self { copies: 1 }
    }

    /// `TestFinder.Builder.createMultipleCopies(int amount)` — repeats each
    /// discovered test/position `amount` times (preserving source order per
    /// repetition, matching Paper's `Collections.nCopies(amount, ...)`).
    /// Values `<= 0` produce 0 copies, matching `Collections.nCopies` / empty
    /// stream semantics without panicking.
    pub fn create_multiple_copies(amount: i32) -> Self {
        Self {
            copies: amount.max(0) as usize,
        }
    }

    fn repeat<T: Clone>(&self, items: Vec<T>) -> Vec<T> {
        if self.copies == 0 || items.is_empty() {
            return Vec::new();
        }
        if self.copies == 1 {
            return items;
        }
        let mut out = Vec::with_capacity(items.len().saturating_mul(self.copies));
        for _ in 0..self.copies {
            out.extend(items.iter().cloned());
        }
        out
    }

    /// `Builder.byResourceSelection` — selects tests from a holder collection
    /// (e.g. registry lookup or tag) and applies `copies`.
    pub fn by_resource_selection(&self, holders: Vec<Holder<GameTestInstance>>) -> TestFinder {
        TestFinder::new(
            StaticTestInstanceFinder::new(self.repeat(holders)),
            EmptyFinder,
        )
    }

    /// `Builder.failedTests(CommandContext, boolean onlyRequiredTests)`.
    ///
    /// Preserves [`FailedTestTracker`] insertion order and filters to
    /// `instance.required()` when `only_required_tests` is `true`.
    pub fn failed_tests(
        &self,
        tracker: &FailedTestTracker,
        registries: &GameTestRegistries,
        only_required_tests: bool,
    ) -> TestFinder {
        let mut selected = Vec::new();
        for holder in tracker.last_failed_tests() {
            if !only_required_tests {
                selected.push(holder.clone());
            } else if let Some(inst) = registries.resolve_instance(holder)
                && inst.required()
            {
                selected.push(holder.clone());
            }
        }
        TestFinder::new(
            StaticTestInstanceFinder::new(self.repeat(selected)),
            EmptyFinder,
        )
    }

    /// Position-based discovery (`nearest`, `allNearby`, `lookedAt`,
    /// `radius` in Paper) given resolved `BlockPos` coordinates.
    pub fn from_positions(&self, positions: Vec<BlockPos>) -> TestFinder {
        TestFinder::new(
            EmptyFinder,
            StaticTestPosFinder::new(self.repeat(positions)),
        )
    }
}
