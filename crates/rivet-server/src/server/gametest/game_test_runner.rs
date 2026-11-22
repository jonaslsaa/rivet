//! `net.minecraft.gametest.framework.GameTestRunner`, `GameTestListener`,
//! `GameTestBatchListener`, and `ReportGameListener` (Paper 26.2).
//!
//! Mirrors `GameTestRunner.java` and `ReportGameListener.java`:
//! - Batch progression (`runBatch`) with `clearBetweenBatches`,
//!   `StructureSpawner::on_batch_start` / `spawn_structure`, prior environment
//!   teardown (including `AllOf` reverse teardown order), current environment
//!   activation, `testBatchStarting` / `testBatchFinished` hooks, and
//!   `haltOnError` handling.
//! - Retry & flaky-test state machine (`ReportGameListener`) with exact Paper
//!   early-exhaustion condition (`maxAttempts - attempts + successes >= requiredSuccesses`),
//!   `RetryOptions::has_tries_left`, and `runScheduledRerunTests` re-batching.
//! - Deterministic synchronous tick-thread execution (`tick()` / `run_to_completion()`)
//!   producing a stable [`GameTestExecutionTranscript`].

use std::sync::Arc;

use rivet_registry::core::BlockPos;
use rivet_registry::{Holder, Identifier};

use crate::server::gametest::discovery::FailedTestTracker;
use crate::server::gametest::exception::{ExhaustedAttemptsException, GameTestException};
use crate::server::gametest::game_test_batch::GameTestBatch;
use crate::server::gametest::game_test_batch_factory::{MAX_TESTS_PER_BATCH, divide_into_batches};
use crate::server::gametest::game_test_info::{
    GameTestInfo, GameTestTickOutcome, TestFunctionRegistry,
};
use crate::server::gametest::game_test_ticker::{GameTestTicker, TestInfoId};
use crate::server::gametest::multiple_test_tracker::MultipleTestTracker;
use crate::server::gametest::registries::GameTestRegistries;
use crate::server::gametest::reporter::{
    GameTestExecutionTranscript, GameTestOutcome, GameTestRunRecord, GameTestRunStatus,
    TestReporter,
};
use crate::server::gametest::test_environment_definition::{
    TestEnvironmentDefinition, TestEnvironmentRef,
};

/// Event recorded by lifecycle observers for deterministic testing and inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameTestLifecycleEvent {
    BatchStructuresCleared {
        batch_index: i32,
        count: usize,
    },
    SpawnerOnBatchStart {
        batch_index: i32,
    },
    StructureSpawned {
        id: Identifier,
        pos: BlockPos,
    },
    EnvironmentTeardown {
        id: Identifier,
        step_index: usize,
    },
    EnvironmentSetup {
        id: Identifier,
        step_index: usize,
    },
    BatchStarting {
        batch_index: i32,
    },
    TestStructureLoaded {
        id: Identifier,
        attempt: i32,
    },
    TestPassed {
        id: Identifier,
        tick_count: i32,
    },
    TestFailed {
        id: Identifier,
        tick_count: i32,
        error: String,
    },
    TestAddedForRerun {
        id: Identifier,
        attempt_scheduled: i32,
    },
    BatchFinished {
        batch_index: i32,
    },
    ForcedChunksCleared {
        batch_index: i32,
    },
    RunnerStopped,
}

/// `net.minecraft.gametest.framework.GameTestBatchListener` (Paper 26.2).
pub trait GameTestBatchListener {
    fn test_batch_starting(
        &mut self,
        batch_index: i32,
        environment: &Holder<TestEnvironmentDefinition>,
    );
    fn test_batch_finished(
        &mut self,
        batch_index: i32,
        environment: &Holder<TestEnvironmentDefinition>,
    );
}

/// `net.minecraft.gametest.framework.GameTestListener` (Paper 26.2).
pub trait GameTestListener {
    fn test_structure_loaded(&mut self, info: &GameTestInfo);
    fn test_passed(&mut self, info: &GameTestInfo);
    fn test_failed(&mut self, info: &GameTestInfo);
    fn test_added_for_rerun(&mut self, original: &GameTestInfo, copy: &GameTestInfo);
}

/// Environment setup/teardown hook (`TestEnvironmentDefinition.setup` / `teardown`).
///
/// For `TestEnvironmentDefinition::AllOf`, the runner invokes `setup_leaf` in
/// forward declaration order and `teardown_leaf` in reverse order
/// (`AllOf::teardown_order_indices()`), matching Paper 26.2.
pub trait EnvironmentHook {
    fn setup_leaf(&mut self, def: &TestEnvironmentDefinition);
    fn teardown_leaf(&mut self, def: &TestEnvironmentDefinition);
}

/// Default no-op environment hook that still allows the runner to record exact
/// setup/teardown lifecycle events.
#[derive(Debug, Default)]
pub struct NoopEnvironmentHook;

impl EnvironmentHook for NoopEnvironmentHook {
    fn setup_leaf(&mut self, _def: &TestEnvironmentDefinition) {}
    fn teardown_leaf(&mut self, _def: &TestEnvironmentDefinition) {}
}

/// Structure placement & isolation cleanup hook (`GameTestRunner.StructureSpawner`).
pub trait StructureSpawner {
    fn on_batch_start(&mut self, batch_index: i32);

    /// Assign/place structure for `info` and return its block position if placed.
    fn spawn_structure(&mut self, info: &mut GameTestInfo) -> Option<BlockPos>;

    /// Clear structures from a previous batch when `clear_between_batches` is enabled.
    fn clear_structures(&mut self, positions: &[BlockPos]);
}

/// Deterministic grid spawner for headless/synthetic execution.
///
/// Places each test at a distinct `(x, y, z)` coordinate spaced by `stride`
/// blocks along X/Z so every test in a batch has an isolated deterministic position.
#[derive(Debug, Clone)]
pub struct DeterministicGridSpawner {
    origin: BlockPos,
    stride: i32,
    tests_per_row: i32,
    next_index: i32,
    occupied_in_batch: Vec<BlockPos>,
    cleared_positions: Vec<BlockPos>,
}

impl Default for DeterministicGridSpawner {
    fn default() -> Self {
        Self::new(BlockPos::new(0, 64, 0), 16, 8)
    }
}

impl DeterministicGridSpawner {
    pub fn new(origin: BlockPos, stride: i32, tests_per_row: i32) -> Self {
        Self {
            origin,
            stride: stride.max(1),
            tests_per_row: tests_per_row.max(1),
            next_index: 0,
            occupied_in_batch: Vec::new(),
            cleared_positions: Vec::new(),
        }
    }

    pub fn cleared_positions(&self) -> &[BlockPos] {
        &self.cleared_positions
    }
}

impl StructureSpawner for DeterministicGridSpawner {
    fn on_batch_start(&mut self, _batch_index: i32) {
        self.next_index = 0;
        self.occupied_in_batch.clear();
    }

    fn spawn_structure(&mut self, info: &mut GameTestInfo) -> Option<BlockPos> {
        let pos = info.test_block_pos().unwrap_or_else(|| {
            loop {
                let idx = self.next_index;
                self.next_index = self.next_index.wrapping_add(1);
                let col = idx % self.tests_per_row;
                let row = idx / self.tests_per_row;
                let candidate = BlockPos::new(
                    self.origin
                        .get_x()
                        .wrapping_add(col.wrapping_mul(self.stride)),
                    self.origin.get_y(),
                    self.origin
                        .get_z()
                        .wrapping_add(row.wrapping_mul(self.stride)),
                );
                if !self.occupied_in_batch.contains(&candidate) {
                    break candidate;
                }
            }
        });
        if !self.occupied_in_batch.contains(&pos) {
            self.occupied_in_batch.push(pos);
        }
        info.set_test_block_pos(Some(pos));
        info.set_placed(true);
        Some(pos)
    }

    fn clear_structures(&mut self, positions: &[BlockPos]) {
        self.cleared_positions.extend_from_slice(positions);
    }
}

/// Per-logical-test state mirroring `ReportGameListener.java`.
#[derive(Debug, Clone)]
struct LogicalTestState {
    attempts: i32,
    successes: i32,
    batch_index: i32,
    first_failure: Option<String>,
    final_error: Option<String>,
    outcome: Option<GameTestOutcome>,
    latest_info_id: TestInfoId,
}

/// Configuration for [`GameTestRunner`], mirroring `GameTestRunner.Builder` defaults.
#[derive(Debug, Clone)]
pub struct GameTestRunnerConfig {
    pub halt_on_error: bool,
    pub clear_between_batches: bool,
    pub max_tests_per_batch: usize,
    pub tick_delay: i32,
    pub seed: i64,
    pub discovered_count: usize,
    pub resource_provenance: String,
}

impl Default for GameTestRunnerConfig {
    fn default() -> Self {
        Self {
            halt_on_error: false,
            clear_between_batches: false,
            max_tests_per_batch: MAX_TESTS_PER_BATCH,
            tick_delay: 0,
            seed: 0,
            discovered_count: 0,
            resource_provenance: String::new(),
        }
    }
}

/// `net.minecraft.gametest.framework.GameTestRunner` (Paper 26.2).
///
/// Owns its arena of `GameTestInfo`s, `GameTestTicker`, `FailedTestTracker`,
/// logical retry states (`ReportGameListener`), and batch progression state
/// on the synchronous tick thread.
pub struct GameTestRunner {
    config: GameTestRunnerConfig,
    registries: Arc<GameTestRegistries>,
    arena: Vec<GameTestInfo>,
    batches: Vec<GameTestBatch<TestInfoId>>,
    all_test_infos: Vec<TestInfoId>,
    /// Maps each `TestInfoId` (including rerun copies) to its logical test index.
    info_to_logical: Vec<usize>,
    logical_states: Vec<LogicalTestState>,
    ticker: GameTestTicker,
    failed_tracker: FailedTestTracker,
    scheduled_for_rerun: Vec<TestInfoId>,
    current_batch_index: Option<usize>,
    current_batch_tracker: Option<MultipleTestTracker>,
    current_environment: Option<Holder<TestEnvironmentDefinition>>,
    previous_batch_positions: Vec<BlockPos>,
    batch_listeners: Vec<Box<dyn GameTestBatchListener>>,
    test_listeners: Vec<Box<dyn GameTestListener>>,
    reporters: Vec<Box<dyn TestReporter>>,
    env_hook: Box<dyn EnvironmentHook>,
    spawner: Box<dyn StructureSpawner>,
    events: Vec<GameTestLifecycleEvent>,
    started: bool,
    stopped: bool,
    finished: bool,
    halted_on_error: bool,
    current_level_tick: i64,
}

impl GameTestRunner {
    /// Create a runner from pre-built `GameTestInfo` items, batching them via
    /// [`divide_into_batches`] with `config.max_tests_per_batch`.
    pub fn new(
        infos: Vec<GameTestInfo>,
        registries: Arc<GameTestRegistries>,
        config: GameTestRunnerConfig,
    ) -> Self {
        let mut arena = Vec::with_capacity(infos.len());
        let mut info_to_logical = Vec::with_capacity(infos.len());
        let mut logical_states = Vec::with_capacity(infos.len());
        let mut initial_ids = Vec::with_capacity(infos.len());

        for (idx, info) in infos.into_iter().enumerate() {
            let id = TestInfoId(idx);
            arena.push(info);
            info_to_logical.push(idx);
            logical_states.push(LogicalTestState {
                attempts: 0,
                successes: 0,
                batch_index: 0,
                first_failure: None,
                final_error: None,
                outcome: None,
                latest_info_id: id,
            });
            initial_ids.push(id);
        }

        let batches = divide_into_batches(
            initial_ids,
            config.max_tests_per_batch,
            &registries.test_environments,
            |id| arena[id.0].batch().clone(),
        );

        let mut all_test_infos = Vec::with_capacity(arena.len());
        for batch in &batches {
            for &id in batch.game_test_infos() {
                all_test_infos.push(id);
                let logical_idx = info_to_logical[id.0];
                logical_states[logical_idx].batch_index = batch.index();
            }
        }

        Self {
            config,
            registries,
            arena,
            batches,
            all_test_infos,
            info_to_logical,
            logical_states,
            ticker: GameTestTicker::new(),
            failed_tracker: FailedTestTracker::new(),
            scheduled_for_rerun: Vec::new(),
            current_batch_index: None,
            current_batch_tracker: None,
            current_environment: None,
            previous_batch_positions: Vec::new(),
            batch_listeners: Vec::new(),
            test_listeners: Vec::new(),
            reporters: Vec::new(),
            env_hook: Box::new(NoopEnvironmentHook),
            spawner: Box::new(DeterministicGridSpawner::default()),
            events: Vec::new(),
            started: false,
            stopped: false,
            finished: false,
            halted_on_error: false,
            current_level_tick: 0,
        }
    }

    pub fn add_batch_listener(&mut self, listener: Box<dyn GameTestBatchListener>) {
        self.batch_listeners.push(listener);
    }

    pub fn add_test_listener(&mut self, listener: Box<dyn GameTestListener>) {
        self.test_listeners.push(listener);
    }

    pub fn add_reporter(&mut self, reporter: Box<dyn TestReporter>) {
        self.reporters.push(reporter);
    }

    pub fn set_environment_hook(&mut self, hook: Box<dyn EnvironmentHook>) {
        self.env_hook = hook;
    }

    pub fn set_spawner(&mut self, spawner: Box<dyn StructureSpawner>) {
        self.spawner = spawner;
    }

    pub fn batches(&self) -> &[GameTestBatch<TestInfoId>] {
        &self.batches
    }

    pub fn arena(&self) -> &[GameTestInfo] {
        &self.arena
    }

    pub fn arena_mut(&mut self) -> &mut [GameTestInfo] {
        &mut self.arena
    }

    pub fn all_test_infos(&self) -> &[TestInfoId] {
        &self.all_test_infos
    }

    pub fn ticker(&self) -> &GameTestTicker {
        &self.ticker
    }

    pub fn failed_tracker(&self) -> &FailedTestTracker {
        &self.failed_tracker
    }

    pub fn events(&self) -> &[GameTestLifecycleEvent] {
        &self.events
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    pub fn current_level_tick(&self) -> i64 {
        self.current_level_tick
    }

    /// Paper: `GameTestRunner.start()`.
    ///
    /// Starts batch 0 if any batches exist; otherwise marks finished immediately
    /// (zero-selected tests produce [`GameTestRunStatus::ZeroSelected`]).
    pub fn start(&mut self) {
        self.started = true;
        self.stopped = false;
        if self.batches.is_empty() {
            self.finished = true;
            for reporter in &mut self.reporters {
                reporter.finish();
            }
            return;
        }
        self.run_batch(0);
    }

    /// Paper: `GameTestRunner.stop()`.
    ///
    /// Tears down the active environment (if any) and clears the ticker.
    pub fn stop(&mut self) {
        self.stopped = true;
        self.teardown_current_environment();
        let _ = self.ticker.clear();
        self.events.push(GameTestLifecycleEvent::RunnerStopped);
    }

    /// Paper: `GameTestRunner.rerunTest(GameTestInfo)`.
    ///
    /// Creates a `copy_reset()` of `original_id`, wires it to the same logical
    /// `ReportGameListener` state, notifies listeners, and schedules it for the
    /// post-batch rerun pass.
    pub fn rerun_test(&mut self, original_id: TestInfoId) -> TestInfoId {
        let copy = self.arena[original_id.0].copy_reset();
        let new_id = TestInfoId(self.arena.len());
        let logical_idx = self.info_to_logical[original_id.0];

        self.arena.push(copy);
        self.info_to_logical.push(logical_idx);
        self.logical_states[logical_idx].latest_info_id = new_id;

        for listener in &mut self.test_listeners {
            listener.test_added_for_rerun(&self.arena[original_id.0], &self.arena[new_id.0]);
        }

        let attempt_scheduled = self.logical_states[logical_idx].attempts.wrapping_add(1);
        self.events.push(GameTestLifecycleEvent::TestAddedForRerun {
            id: self.arena[new_id.0].id().clone(),
            attempt_scheduled,
        });

        self.scheduled_for_rerun.push(new_id);
        new_id
    }

    /// Advance the runner by a single synchronous server tick.
    ///
    /// Mirrors Paper's tick thread driving `GameTestTicker.tick()`.
    pub fn tick(&mut self, registry: &TestFunctionRegistry) {
        if !self.started {
            self.start();
        }
        if self.finished || self.stopped {
            return;
        }

        self.current_level_tick = self.current_level_tick.wrapping_add(1);
        let level_tick = self.current_level_tick;

        let mut newly_completed: Vec<(TestInfoId, GameTestTickOutcome)> = Vec::new();

        let tick_result = self.ticker.tick_with(|id, _ticker| {
            let outcome = self.arena[id.0].tick(level_tick, registry);
            match outcome {
                GameTestTickOutcome::Running => false,
                GameTestTickOutcome::Passed | GameTestTickOutcome::Failed => {
                    newly_completed.push((id, outcome));
                    true
                }
            }
        });

        for (id, outcome) in newly_completed {
            self.handle_test_completion(id, outcome);
        }

        if tick_result.halted || self.halted_on_error {
            self.stopped = true;
            self.finished = true;
            self.teardown_current_environment();
            self.finish_reporters();
            return;
        }

        self.check_batch_completion();
    }

    /// Drive the synchronous tick loop until all batches and scheduled reruns
    /// finish, or until `max_server_ticks` elapses.
    pub fn run_to_completion(
        &mut self,
        registry: &TestFunctionRegistry,
        max_server_ticks: u64,
    ) -> GameTestExecutionTranscript {
        if !self.started {
            self.start();
        }
        let mut ticks_run = 0u64;
        while !self.finished && !self.stopped && ticks_run < max_server_ticks {
            self.tick(registry);
            ticks_run += 1;
        }
        self.build_transcript()
    }

    /// Build the stable deterministic [`GameTestExecutionTranscript`] for the
    /// current state of the runner.
    pub fn build_transcript(&self) -> GameTestExecutionTranscript {
        let selected_count = self.logical_states.len();
        let discovered_count = self.config.discovered_count.max(selected_count);

        if selected_count == 0 {
            return GameTestExecutionTranscript::zero_selected(
                discovered_count,
                self.config.seed,
                self.config.resource_provenance.clone(),
            );
        }

        let mut records = Vec::with_capacity(selected_count);
        let mut executed_count = 0usize;
        let mut passed_count = 0usize;
        let mut failed_required_count = 0usize;
        let mut failed_optional_count = 0usize;
        let mut skipped_count = 0usize;
        let mut skipped_required_count = 0usize;

        for state in &self.logical_states {
            let info = &self.arena[state.latest_info_id.0];
            let outcome = state.outcome.unwrap_or(GameTestOutcome::Skipped);
            if state.attempts > 0 {
                executed_count += 1;
            }
            match outcome {
                GameTestOutcome::Passed => passed_count += 1,
                GameTestOutcome::FailedRequired => failed_required_count += 1,
                GameTestOutcome::FailedOptional => failed_optional_count += 1,
                GameTestOutcome::Skipped => {
                    skipped_count += 1;
                    if info.is_required() {
                        skipped_required_count += 1;
                    }
                }
            }

            let env_id = self.environment_identifier(info.batch());

            records.push(GameTestRunRecord {
                id: info.id().clone(),
                structure: info.structure().clone(),
                environment: env_id,
                batch_index: state.batch_index,
                required: info.is_required(),
                outcome,
                setup_tick: info.setup_tick(),
                start_tick: info.start_tick(),
                end_tick: info.end_tick(),
                final_test_tick: info.tick_count(),
                attempts: state.attempts,
                retry_count: state.attempts.saturating_sub(1),
                successes: state.successes,
                first_failure: state.first_failure.clone(),
                final_error: state.final_error.clone(),
                position: info.test_block_pos(),
            });
        }

        let status = if !self.finished {
            GameTestRunStatus::Incomplete
        } else if failed_required_count > 0 || skipped_required_count > 0 || self.halted_on_error {
            GameTestRunStatus::Failed
        } else {
            GameTestRunStatus::Passed
        };

        GameTestExecutionTranscript {
            status,
            discovered_count,
            selected_count,
            executed_count,
            passed_count,
            failed_required_count,
            failed_optional_count,
            skipped_count,
            seed: self.config.seed,
            resource_provenance: self.config.resource_provenance.clone(),
            records,
        }
    }

    fn run_batch(&mut self, batch_idx: usize) {
        if batch_idx >= self.batches.len() {
            // All batches done: tear down active environment and run scheduled reruns (Paper).
            self.teardown_current_environment();
            self.run_scheduled_rerun_tests();
            return;
        }

        if self.config.clear_between_batches && !self.previous_batch_positions.is_empty() {
            let prev = std::mem::take(&mut self.previous_batch_positions);
            let prev_batch_index = batch_idx.saturating_sub(1) as i32;
            self.spawner.clear_structures(&prev);
            self.events
                .push(GameTestLifecycleEvent::BatchStructuresCleared {
                    batch_index: prev_batch_index,
                    count: prev.len(),
                });
        }

        let batch_logical_index = self.batches[batch_idx].index();
        let batch_env = self.batches[batch_idx].environment().clone();
        let batch_test_ids: Vec<TestInfoId> = self.batches[batch_idx].game_test_infos().to_vec();

        // Paper: onBatchStart before creating structures.
        self.spawner.on_batch_start(batch_logical_index);
        self.events
            .push(GameTestLifecycleEvent::SpawnerOnBatchStart {
                batch_index: batch_logical_index,
            });

        // Paper: createStructuresForBatch.
        let mut spawned_ids = Vec::with_capacity(batch_test_ids.len());
        let mut spawned_positions = Vec::with_capacity(batch_test_ids.len());
        for id in batch_test_ids {
            let maybe_pos = self.spawner.spawn_structure(&mut self.arena[id.0]);
            if let Some(pos) = maybe_pos {
                spawned_positions.push(pos);
                self.events.push(GameTestLifecycleEvent::StructureSpawned {
                    id: self.arena[id.0].id().clone(),
                    pos,
                });
                spawned_ids.push(id);
            }
        }
        self.previous_batch_positions = spawned_positions;

        // Paper: tear down prior environment, then activate current environment.
        self.teardown_current_environment();
        self.setup_environment(&batch_env);
        self.current_environment = Some(batch_env.clone());

        // Paper: notify batch listeners testBatchStarting.
        for listener in &mut self.batch_listeners {
            listener.test_batch_starting(batch_logical_index, &batch_env);
        }
        self.events.push(GameTestLifecycleEvent::BatchStarting {
            batch_index: batch_logical_index,
        });

        // Track batch tests, notify testStructureLoaded, startExecution, and add to ticker.
        let tracker = MultipleTestTracker::from_tests(spawned_ids.iter().copied());
        self.current_batch_index = Some(batch_idx);
        self.current_batch_tracker = Some(tracker);

        let level_tick = self.current_level_tick;
        let tick_delay = self.config.tick_delay;
        for id in spawned_ids {
            let logical_idx = self.info_to_logical[id.0];
            self.logical_states[logical_idx].attempts =
                self.logical_states[logical_idx].attempts.wrapping_add(1);
            let attempt = self.logical_states[logical_idx].attempts;

            for listener in &mut self.test_listeners {
                listener.test_structure_loaded(&self.arena[id.0]);
            }
            self.events
                .push(GameTestLifecycleEvent::TestStructureLoaded {
                    id: self.arena[id.0].id().clone(),
                    attempt,
                });

            self.arena[id.0].start_execution(tick_delay, level_tick);
            self.ticker.add(id);
        }

        // If all tests failed to spawn, advance immediately.
        if self.ticker.is_empty() {
            self.check_batch_completion();
        }
    }

    fn handle_test_completion(&mut self, id: TestInfoId, outcome: GameTestTickOutcome) {
        let logical_idx = self.info_to_logical[id.0];
        let tick_count = self.arena[id.0].tick_count();
        let test_id = self.arena[id.0].id().clone();

        match outcome {
            GameTestTickOutcome::Passed => {
                for listener in &mut self.test_listeners {
                    listener.test_passed(&self.arena[id.0]);
                }
                self.events.push(GameTestLifecycleEvent::TestPassed {
                    id: test_id,
                    tick_count,
                });

                self.logical_states[logical_idx].successes =
                    self.logical_states[logical_idx].successes.wrapping_add(1);

                let retry_opts = self.arena[id.0].retry_options();
                let is_flaky = self.arena[id.0].is_flaky();
                let required_successes = self.arena[id.0].required_successes();
                let attempts = self.logical_states[logical_idx].attempts;
                let successes = self.logical_states[logical_idx].successes;

                if retry_opts.has_retries() {
                    self.report_passed(id, logical_idx);
                    if retry_opts.has_tries_left(attempts, successes) {
                        self.rerun_test(id);
                    }
                } else if !is_flaky || successes >= required_successes {
                    self.report_passed(id, logical_idx);
                } else {
                    self.rerun_test(id);
                }
            }
            GameTestTickOutcome::Failed => {
                let err_msg = self.arena[id.0]
                    .error()
                    .map(GameTestException::message)
                    .unwrap_or_default();
                if self.logical_states[logical_idx].first_failure.is_none() {
                    self.logical_states[logical_idx].first_failure = Some(err_msg.clone());
                }

                for listener in &mut self.test_listeners {
                    listener.test_failed(&self.arena[id.0]);
                }
                self.events.push(GameTestLifecycleEvent::TestFailed {
                    id: test_id,
                    tick_count,
                    error: err_msg,
                });

                let retry_opts = self.arena[id.0].retry_options();
                let is_flaky = self.arena[id.0].is_flaky();
                let max_attempts = self.arena[id.0].max_attempts();
                let required_successes = self.arena[id.0].required_successes();
                let attempts = self.logical_states[logical_idx].attempts;
                let successes = self.logical_states[logical_idx].successes;

                if !is_flaky {
                    self.report_failure(id, logical_idx, None);
                    if retry_opts.has_retries() && retry_opts.has_tries_left(attempts, successes) {
                        self.rerun_test(id);
                    }
                } else if max_attempts.wrapping_sub(attempts).wrapping_add(successes)
                    >= required_successes
                {
                    self.rerun_test(id);
                } else {
                    let exhausted =
                        GameTestException::ExhaustedAttempts(ExhaustedAttemptsException::new(
                            attempts,
                            successes,
                            required_successes,
                            max_attempts,
                            self.arena[id.0].error().cloned(),
                        ));
                    self.report_failure(id, logical_idx, Some(exhausted));
                }

                // Paper GameTestRunner.runBatch: haltOnError clears ticker and stops.
                if self.config.halt_on_error && self.arena[id.0].is_required() {
                    self.halted_on_error = true;
                    let _ = self.ticker.clear();
                }
            }
            GameTestTickOutcome::Running => {}
        }
    }

    fn report_passed(&mut self, id: TestInfoId, logical_idx: usize) {
        self.logical_states[logical_idx].outcome = Some(GameTestOutcome::Passed);
        self.logical_states[logical_idx].final_error = None;
        let entry = self.arena[id.0].to_report_entry();
        for reporter in &mut self.reporters {
            reporter.on_test_success(&entry);
        }
    }

    fn report_failure(
        &mut self,
        id: TestInfoId,
        logical_idx: usize,
        override_error: Option<GameTestException>,
    ) {
        let is_required = self.arena[id.0].is_required();
        self.logical_states[logical_idx].outcome = Some(if is_required {
            GameTestOutcome::FailedRequired
        } else {
            GameTestOutcome::FailedOptional
        });
        let mut entry = self.arena[id.0].to_report_entry();
        if let Some(err) = override_error {
            entry.error = Some(err);
        }
        self.logical_states[logical_idx].final_error =
            entry.error.as_ref().map(GameTestException::message);

        self.failed_tracker
            .remember_failed_test(self.arena[id.0].test_holder().clone());

        for reporter in &mut self.reporters {
            reporter.on_test_failed(&entry);
        }
    }

    fn check_batch_completion(&mut self) {
        let Some(batch_idx) = self.current_batch_index else {
            return;
        };
        let is_done = self
            .current_batch_tracker
            .as_ref()
            .is_some_and(|t| t.is_done(&self.arena));
        if !is_done {
            return;
        }

        let batch_logical_index = self.batches[batch_idx].index();
        let batch_env = self.batches[batch_idx].environment().clone();

        for listener in &mut self.batch_listeners {
            listener.test_batch_finished(batch_logical_index, &batch_env);
        }
        self.events.push(GameTestLifecycleEvent::BatchFinished {
            batch_index: batch_logical_index,
        });
        self.events
            .push(GameTestLifecycleEvent::ForcedChunksCleared {
                batch_index: batch_logical_index,
            });

        self.current_batch_tracker = None;
        self.current_batch_index = None;
        self.run_batch(batch_idx + 1);
    }

    /// Paper: `GameTestRunner.runScheduledRerunTests()`.
    fn run_scheduled_rerun_tests(&mut self) {
        if self.scheduled_for_rerun.is_empty() {
            if self.config.clear_between_batches && !self.previous_batch_positions.is_empty() {
                let prev = std::mem::take(&mut self.previous_batch_positions);
                let last_idx = self.batches.last().map_or(0, GameTestBatch::index);
                self.spawner.clear_structures(&prev);
                self.events
                    .push(GameTestLifecycleEvent::BatchStructuresCleared {
                        batch_index: last_idx,
                        count: prev.len(),
                    });
            }
            self.finished = true;
            self.finish_reporters();
            return;
        }

        let rerun_ids = std::mem::take(&mut self.scheduled_for_rerun);
        let new_batches = divide_into_batches(
            rerun_ids,
            self.config.max_tests_per_batch,
            &self.registries.test_environments,
            |id| self.arena[id.0].batch().clone(),
        );
        self.batches = new_batches;
        self.run_batch(0);
    }

    fn environment_identifier(&self, env: &Holder<TestEnvironmentDefinition>) -> Identifier {
        match env {
            Holder::Reference { .. } => env
                .key(&self.registries.test_environments)
                .identifier()
                .clone(),
            Holder::Direct(_) => Identifier::with_default_namespace("default"),
        }
    }

    fn setup_environment(&mut self, env: &Holder<TestEnvironmentDefinition>) {
        let env_id = self.environment_identifier(env);
        let Some(def) = self.registries.resolve_environment(env).cloned() else {
            return;
        };
        let mut step = 0usize;
        self.setup_definition_recursive(&def, &env_id, &mut step);
    }

    fn setup_definition_recursive(
        &mut self,
        def: &TestEnvironmentDefinition,
        root_id: &Identifier,
        step: &mut usize,
    ) {
        match def {
            TestEnvironmentDefinition::AllOf { definitions } => {
                for child_ref in definitions {
                    match child_ref {
                        TestEnvironmentRef::Key(key) => {
                            let child_id = key.identifier().clone();
                            if let Some(child_def) =
                                self.registries.test_environments.get_value(key).cloned()
                            {
                                self.setup_definition_recursive(&child_def, &child_id, step);
                            }
                        }
                        TestEnvironmentRef::Inline(inner) => {
                            self.setup_definition_recursive(inner, root_id, step);
                        }
                    }
                }
            }
            leaf => {
                self.env_hook.setup_leaf(leaf);
                self.events.push(GameTestLifecycleEvent::EnvironmentSetup {
                    id: root_id.clone(),
                    step_index: *step,
                });
                *step += 1;
            }
        }
    }

    fn teardown_current_environment(&mut self) {
        if let Some(env) = self.current_environment.take() {
            let env_id = self.environment_identifier(&env);
            let Some(def) = self.registries.resolve_environment(&env).cloned() else {
                return;
            };
            let mut step = 0usize;
            self.teardown_definition_recursive(&def, &env_id, &mut step);
        }
    }

    fn teardown_definition_recursive(
        &mut self,
        def: &TestEnvironmentDefinition,
        root_id: &Identifier,
        step: &mut usize,
    ) {
        match def {
            TestEnvironmentDefinition::AllOf { definitions } => {
                for idx in (0..definitions.len()).rev() {
                    match &definitions[idx] {
                        TestEnvironmentRef::Key(key) => {
                            let child_id = key.identifier().clone();
                            if let Some(child_def) =
                                self.registries.test_environments.get_value(key).cloned()
                            {
                                self.teardown_definition_recursive(&child_def, &child_id, step);
                            }
                        }
                        TestEnvironmentRef::Inline(inner) => {
                            self.teardown_definition_recursive(inner, root_id, step);
                        }
                    }
                }
            }
            leaf => {
                self.env_hook.teardown_leaf(leaf);
                self.events
                    .push(GameTestLifecycleEvent::EnvironmentTeardown {
                        id: root_id.clone(),
                        step_index: *step,
                    });
                *step += 1;
            }
        }
    }

    fn finish_reporters(&mut self) {
        for reporter in &mut self.reporters {
            reporter.finish();
        }
    }
}
