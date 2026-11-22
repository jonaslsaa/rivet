//! `net.minecraft.gametest.framework.GameTestInfo` (Paper 26.2).
//!
//! Mirrors `GameTestInfo.java` lifecycle, setup delay, per-tick callback
//! execution, sequence progression, timeout enforcement, and `copyReset()`.

use std::collections::BTreeMap;
use std::sync::Arc;

use rivet_registry::core::{BlockPos, Rotation};
use rivet_registry::{Holder, Identifier};

use crate::server::gametest::exception::GameTestException;
use crate::server::gametest::game_test_instance::GameTestInstance;
use crate::server::gametest::game_test_sequence::{GameTestSequence, SequenceStepEffect};
use crate::server::gametest::registries::GameTestRegistries;
use crate::server::gametest::reporter::GameTestReportEntry;
use crate::server::gametest::retry_options::RetryOptions;
use crate::server::gametest::test_environment_definition::TestEnvironmentDefinition;

/// Callback scheduled via `GameTestInfo::set_run_at_tick_time`.
pub type TickCallback = Box<dyn FnOnce(&mut TestExecutionHandle) -> Result<(), GameTestException>>;

/// Executable test body invoked when `GameTestInfo::start_test` runs at `tick_count == 0`.
pub type TestBodyFn =
    Arc<dyn Fn(&mut TestExecutionHandle) -> Result<(), GameTestException> + Send + Sync>;

/// Registry of named executable test bodies (mirrors `Registries.TEST_FUNCTION`
/// runtime dispatch).
#[derive(Clone, Default)]
pub struct TestFunctionRegistry {
    functions: BTreeMap<Identifier, TestBodyFn>,
}

impl std::fmt::Debug for TestFunctionRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestFunctionRegistry")
            .field("keys", &self.functions.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl TestFunctionRegistry {
    /// Creates a registry pre-populated with Paper's `minecraft:always_pass`.
    pub fn with_builtins() -> Self {
        let mut reg = Self::default();
        reg.register(
            Identifier::with_default_namespace("always_pass"),
            |handle| {
                handle.succeed();
                Ok(())
            },
        );
        reg
    }

    pub fn register<F>(&mut self, id: Identifier, body: F)
    where
        F: Fn(&mut TestExecutionHandle) -> Result<(), GameTestException> + Send + Sync + 'static,
    {
        self.functions.insert(id, Arc::new(body));
    }

    pub fn get(&self, id: &Identifier) -> Option<&TestBodyFn> {
        self.functions.get(id)
    }

    pub fn contains(&self, id: &Identifier) -> bool {
        self.functions.contains_key(id)
    }
}

/// Handle passed to a running test body or tick callback (minimal subset of
/// `GameTestHelper` needed for M3 lifecycle, timing, callbacks, and sequences).
pub struct TestExecutionHandle {
    current_tick: i32,
    succeeded: bool,
    failed: Option<GameTestException>,
    scheduled: Vec<(i64, TickCallback)>,
    sequences: Vec<GameTestSequence>,
}

impl TestExecutionHandle {
    fn new(current_tick: i32) -> Self {
        Self {
            current_tick,
            succeeded: false,
            failed: None,
            scheduled: Vec::new(),
            sequences: Vec::new(),
        }
    }

    pub const fn current_tick(&self) -> i32 {
        self.current_tick
    }

    /// Paper: `GameTestHelper.succeed()` -> `testInfo.succeed()`.
    pub fn succeed(&mut self) {
        self.succeeded = true;
    }

    /// Paper: `GameTestHelper.fail(Component)` -> `testInfo.fail(new GameTestAssertException(msg, tick))`.
    pub fn fail(&mut self, message: impl Into<String>) {
        self.failed = Some(GameTestException::assert(message, self.current_tick));
    }

    /// Paper: `GameTestHelper.fail(Component, BlockPos)`.
    pub fn fail_at_pos(
        &mut self,
        message: impl Into<String>,
        absolute_pos: BlockPos,
        relative_pos: BlockPos,
    ) {
        self.failed = Some(GameTestException::assert_pos(
            message,
            absolute_pos,
            relative_pos,
            self.current_tick,
        ));
    }

    /// Fail with an explicit `GameTestException`.
    pub fn fail_with(&mut self, err: GameTestException) {
        self.failed = Some(err);
    }

    /// Paper: `GameTestInfo.setRunAtTickTime(long tick, Runnable)`.
    pub fn run_at_tick_time<F>(&mut self, tick: i64, callback: F)
    where
        F: FnOnce(&mut TestExecutionHandle) -> Result<(), GameTestException> + 'static,
    {
        self.scheduled.push((tick, Box::new(callback)));
    }

    /// Paper: `GameTestHelper.runAfterDelay(long delay, Runnable)`.
    pub fn run_after_delay<F>(&mut self, delay: i64, callback: F)
    where
        F: FnOnce(&mut TestExecutionHandle) -> Result<(), GameTestException> + 'static,
    {
        self.run_at_tick_time(i64::from(self.current_tick).wrapping_add(delay), callback);
    }

    /// Paper: `GameTestHelper.startSequence()` -> `GameTestSequence(parent)` sets
    /// `lastTick = parent.getTick()`.
    pub fn add_sequence(&mut self, mut sequence: GameTestSequence) {
        sequence.set_start_tick(i64::from(self.current_tick));
        self.sequences.push(sequence);
    }
}

/// Terminal transition reported by [`GameTestInfo::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameTestTickOutcome {
    /// Still waiting on chunks, setup delay, or running.
    Running,
    /// Just completed with success on this tick.
    Passed,
    /// Just completed with failure on this tick.
    Failed,
}

/// `net.minecraft.gametest.framework.GameTestInfo` (Paper 26.2).
pub struct GameTestInfo {
    id: Identifier,
    test_holder: Holder<GameTestInstance>,
    test: GameTestInstance,
    environment: Holder<TestEnvironmentDefinition>,
    test_block_pos: Option<BlockPos>,
    rotation: Rotation,
    extra_rotation: Rotation,
    retry_options: RetryOptions,
    timeout_ticks: i32,
    started: bool,
    done: bool,
    placed_structure: bool,
    chunks_loaded: bool,
    missing_block_entity: bool,
    tick_count: i32,
    setup_tick: Option<i64>,
    start_tick: Option<i64>,
    end_tick: Option<i64>,
    run_time_ms: i64,
    error: Option<GameTestException>,
    run_at_tick_time_map: BTreeMap<i64, Vec<TickCallback>>,
    sequences: Vec<GameTestSequence>,
}

impl GameTestInfo {
    /// Paper: `new GameTestInfo(Holder.Reference<GameTestInstance> test, Rotation extraRotation, ServerLevel level, RetryOptions retryOptions)`.
    ///
    /// Resolves the test instance and its environment holder once against
    /// `registries` (OWNERSHIP.md §Registries — frozen immutable registries).
    pub fn new(
        id: Identifier,
        test_holder: Holder<GameTestInstance>,
        registries: &GameTestRegistries,
        extra_rotation: Rotation,
        retry_options: RetryOptions,
    ) -> Self {
        let test = registries
            .resolve_instance(&test_holder)
            .expect("GameTestInfo test holder must resolve in GameTestRegistries")
            .clone();
        let environment = registries
            .holder_for_environment_ref(&test.info().environment)
            .expect("GameTestInfo environment must resolve in GameTestRegistries");
        Self::from_resolved(
            id,
            test_holder,
            test,
            environment,
            extra_rotation,
            retry_options,
        )
    }

    /// Construct directly from an already-resolved instance and environment holder.
    pub fn from_resolved(
        id: Identifier,
        test_holder: Holder<GameTestInstance>,
        test: GameTestInstance,
        environment: Holder<TestEnvironmentDefinition>,
        extra_rotation: Rotation,
        retry_options: RetryOptions,
    ) -> Self {
        let timeout_ticks = test.max_ticks();
        let rotation = extra_rotation.get_rotated(test.rotation());
        Self {
            id,
            test_holder,
            test,
            environment,
            test_block_pos: None,
            rotation,
            extra_rotation,
            retry_options,
            timeout_ticks,
            started: false,
            done: false,
            placed_structure: false,
            chunks_loaded: true,
            missing_block_entity: false,
            tick_count: 0,
            setup_tick: None,
            start_tick: None,
            end_tick: None,
            run_time_ms: 0,
            error: None,
            run_at_tick_time_map: BTreeMap::new(),
            sequences: Vec::new(),
        }
    }

    pub fn id(&self) -> &Identifier {
        &self.id
    }

    pub fn test(&self) -> &GameTestInstance {
        &self.test
    }

    pub fn test_holder(&self) -> &Holder<GameTestInstance> {
        &self.test_holder
    }

    pub fn structure(&self) -> &Identifier {
        self.test.structure()
    }

    pub fn batch(&self) -> &Holder<TestEnvironmentDefinition> {
        &self.environment
    }

    pub fn test_block_pos(&self) -> Option<BlockPos> {
        self.test_block_pos
    }

    pub fn set_test_block_pos(&mut self, pos: Option<BlockPos>) {
        self.test_block_pos = pos;
    }

    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    pub fn extra_rotation(&self) -> Rotation {
        self.extra_rotation
    }

    pub fn retry_options(&self) -> RetryOptions {
        self.retry_options
    }

    pub fn max_ticks(&self) -> i32 {
        self.timeout_ticks
    }

    pub fn setup_ticks(&self) -> i32 {
        self.test.setup_ticks()
    }

    pub fn max_attempts(&self) -> i32 {
        self.test.max_attempts()
    }

    pub fn required_successes(&self) -> i32 {
        self.test.required_successes()
    }

    pub fn is_required(&self) -> bool {
        self.test.required()
    }

    pub fn is_optional(&self) -> bool {
        !self.is_required()
    }

    pub fn is_flaky(&self) -> bool {
        self.max_attempts() > 1
    }

    pub fn has_started(&self) -> bool {
        self.started
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn has_succeeded(&self) -> bool {
        self.done && self.error.is_none()
    }

    pub fn has_failed(&self) -> bool {
        self.error.is_some()
    }

    pub fn tick_count(&self) -> i32 {
        self.tick_count
    }

    pub fn setup_tick(&self) -> Option<i64> {
        self.setup_tick
    }

    pub fn start_tick(&self) -> Option<i64> {
        self.start_tick
    }

    pub fn end_tick(&self) -> Option<i64> {
        self.end_tick
    }

    pub fn run_time_ms(&self) -> i64 {
        self.run_time_ms
    }

    pub fn error(&self) -> Option<&GameTestException> {
        self.error.as_ref()
    }

    pub fn is_placed(&self) -> bool {
        self.placed_structure
    }

    pub fn set_placed(&mut self, placed: bool) {
        self.placed_structure = placed;
    }

    pub fn set_chunks_loaded(&mut self, loaded: bool) {
        self.chunks_loaded = loaded;
    }

    pub fn set_missing_block_entity(&mut self, missing: bool) {
        self.missing_block_entity = missing;
    }

    /// Paper: `startExecution(int tickDelay)`:
    /// `this.tickCount = -(this.test.value().setupTicks() + tickDelay + 1);`
    pub fn start_execution(&mut self, tick_delay: i32, current_level_tick: i64) {
        self.tick_count = -(self
            .test
            .setup_ticks()
            .wrapping_add(tick_delay)
            .wrapping_add(1));
        self.setup_tick = Some(current_level_tick);
    }

    /// Paper: `setRunAtTickTime(long time, Runnable assertAtTickTime)`.
    pub fn set_run_at_tick_time<F>(&mut self, time: i64, callback: F)
    where
        F: FnOnce(&mut TestExecutionHandle) -> Result<(), GameTestException> + 'static,
    {
        self.run_at_tick_time_map
            .entry(time)
            .or_default()
            .push(Box::new(callback));
    }

    pub fn add_sequence(&mut self, mut sequence: GameTestSequence) {
        sequence.set_start_tick(i64::from(self.tick_count));
        self.sequences.push(sequence);
    }

    /// Paper: `succeed()` — finishes immediately if `this.error == null`.
    pub fn succeed(&mut self, current_level_tick: i64) {
        if self.error.is_none() {
            self.finish(current_level_tick);
        }
    }

    /// Paper: `fail(GameTestException error)` — records error; `finish()` runs on next `tick()`.
    pub fn fail(&mut self, error: GameTestException) {
        self.error = Some(error);
    }

    /// Paper: `fail(Component message)` — creates `GameTestAssertException(message, tickCount)`.
    pub fn fail_message(&mut self, message: impl Into<String>) {
        self.fail(GameTestException::assert(message, self.tick_count));
    }

    fn finish(&mut self, current_level_tick: i64) {
        if !self.done {
            self.done = true;
            self.end_tick = Some(current_level_tick);
            if let Some(start) = self.start_tick {
                let elapsed_ticks = current_level_tick.wrapping_sub(start).max(0);
                self.run_time_ms = elapsed_ticks.wrapping_mul(50);
            }
        }
    }

    /// Paper: `GameTestInfo.tick(GameTestRunner runner)`.
    ///
    /// Advances this test by one synchronous server tick and returns whether
    /// it transitioned to `Passed`, `Failed`, or remains `Running`.
    pub fn tick(
        &mut self,
        current_level_tick: i64,
        registry: &TestFunctionRegistry,
    ) -> GameTestTickOutcome {
        if self.done {
            return if self.error.is_some() {
                GameTestTickOutcome::Failed
            } else {
                GameTestTickOutcome::Passed
            };
        }

        if !self.placed_structure {
            self.fail_message("test.error.ticking_without_structure");
        }

        if self.test_block_pos.is_none() || self.missing_block_entity {
            self.fail_message("test.error.missing_block_entity");
        }

        if self.error.is_some() {
            self.finish(current_level_tick);
        } else if !self.chunks_loaded {
            return GameTestTickOutcome::Running;
        }

        self.tick_internal(current_level_tick, registry);

        if self.done {
            if self.error.is_some() {
                GameTestTickOutcome::Failed
            } else {
                GameTestTickOutcome::Passed
            }
        } else {
            GameTestTickOutcome::Running
        }
    }

    fn tick_internal(&mut self, current_level_tick: i64, registry: &TestFunctionRegistry) {
        self.tick_count = self.tick_count.wrapping_add(1);
        if self.tick_count >= 0 {
            if !self.started {
                self.start_test(current_level_tick, registry);
            }

            // Execute all callbacks with targetTick <= self.tick_count in deterministic tick order.
            let due_ticks: Vec<i64> = self
                .run_at_tick_time_map
                .range(..=i64::from(self.tick_count))
                .map(|(&k, _)| k)
                .collect();
            for tick_key in due_ticks {
                if let Some(callbacks) = self.run_at_tick_time_map.remove(&tick_key) {
                    for cb in callbacks {
                        let mut handle = TestExecutionHandle::new(self.tick_count);
                        let res = cb(&mut handle);
                        self.apply_handle(handle, res, current_level_tick);
                    }
                }
            }

            if self.tick_count > self.timeout_ticks {
                if self.sequences.is_empty() {
                    self.fail(GameTestException::Timeout(
                        crate::server::gametest::exception::GameTestTimeoutException::no_result(
                            self.test.max_ticks(),
                        ),
                    ));
                } else {
                    let tick = i64::from(self.tick_count);
                    let mut first_err: Option<GameTestException> = None;
                    let mut any_succeeded = false;
                    for seq in &mut self.sequences {
                        if let Some(err) = seq.tick_and_fail_if_not_complete(tick)
                            && first_err.is_none()
                        {
                            first_err = Some(err);
                        }
                        if seq.take_completed_effect() == Some(SequenceStepEffect::Succeed) {
                            any_succeeded = true;
                        }
                    }
                    if let Some(err) = first_err {
                        self.fail(err);
                    } else if any_succeeded {
                        self.succeed(current_level_tick);
                    }
                    if self.error.is_none() && !self.done {
                        self.fail(GameTestException::Timeout(
                            crate::server::gametest::exception::GameTestTimeoutException::no_sequences_finished(
                                self.test.max_ticks(),
                            ),
                        ));
                    }
                }
            } else {
                let tick = i64::from(self.tick_count);
                let mut first_err: Option<GameTestException> = None;
                let mut any_succeeded = false;
                for seq in &mut self.sequences {
                    if let Some(err) = seq.tick_and_continue(tick)
                        && first_err.is_none()
                    {
                        first_err = Some(err);
                    }
                    if seq.take_completed_effect() == Some(SequenceStepEffect::Succeed) {
                        any_succeeded = true;
                    }
                }
                if let Some(err) = first_err {
                    self.fail(err);
                } else if any_succeeded {
                    self.succeed(current_level_tick);
                }
            }
        }
    }

    fn start_test(&mut self, current_level_tick: i64, registry: &TestFunctionRegistry) {
        if !self.started {
            self.started = true;
            self.start_tick = Some(current_level_tick);

            match &self.test {
                GameTestInstance::BlockBased(_) => {
                    // Block-based tests wait for world/test block signals; no function body.
                }
                GameTestInstance::Function(func_inst) => {
                    let fn_id = func_inst.function().identifier().clone();
                    if let Some(body) = registry.get(&fn_id).cloned() {
                        let mut handle = TestExecutionHandle::new(self.tick_count);
                        let res = body(&mut handle);
                        self.apply_handle(handle, res, current_level_tick);
                    } else {
                        self.fail(GameTestException::unknown(format!(
                            "Unregistered test function: {fn_id}"
                        )));
                    }
                }
            }
        }
    }

    fn apply_handle(
        &mut self,
        handle: TestExecutionHandle,
        result: Result<(), GameTestException>,
        current_level_tick: i64,
    ) {
        for (tick, cb) in handle.scheduled {
            self.run_at_tick_time_map.entry(tick).or_default().push(cb);
        }
        for seq in handle.sequences {
            self.sequences.push(seq);
        }
        if let Err(err) = result {
            self.fail(err);
            return;
        }
        if let Some(err) = handle.failed {
            self.fail(err);
            return;
        }
        if handle.succeeded {
            self.succeed(current_level_tick);
        }
    }

    /// Paper: `GameTestInfo.copyReset()`.
    ///
    /// Creates a fresh `GameTestInfo` for a retry/rerun with the same test
    /// holder, extra rotation, retry options, and test block position.
    pub fn copy_reset(&self) -> Self {
        let mut copy = Self::from_resolved(
            self.id.clone(),
            self.test_holder.clone(),
            self.test.clone(),
            self.environment.clone(),
            self.extra_rotation,
            self.retry_options,
        );
        copy.test_block_pos = self.test_block_pos;
        copy
    }

    /// Snapshot this info into a [`GameTestReportEntry`] for [`TestReporter`].
    pub fn to_report_entry(&self) -> GameTestReportEntry {
        GameTestReportEntry {
            id: self.id.clone(),
            structure: self.structure().clone(),
            required: self.is_required(),
            rotation: self.rotation,
            test_block_pos: self.test_block_pos,
            run_time_ms: self.run_time_ms,
            tick_count: self.tick_count,
            error: self.error.clone(),
        }
    }
}
