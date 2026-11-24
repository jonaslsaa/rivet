//! Comprehensive unit tests for M3 GameTest lifecycle, ticker, batches,
//! retries, sequences, isolation, cleanup, and deterministic reporting
//! (issue #430, grounded in Paper 26.2 commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI32, Ordering},
};

use rivet_registry::core::{BlockPos, Rotation};
use rivet_registry::{Holder, Identifier, RegistrationInfo, RegistryBuilder, ResourceKey};

use crate::server::gametest::exception::GameTestException;
use crate::server::gametest::game_test_batch_factory::{
    MAX_TESTS_PER_BATCH, direct_test_info, divide_into_batches,
};
use crate::server::gametest::game_test_info::{GameTestInfo, TestFunctionRegistry};
use crate::server::gametest::game_test_instance::{
    FunctionGameTestInstance, GameTestInstance, always_pass_instance_key,
};
use crate::server::gametest::game_test_runner::{
    DeterministicGridSpawner, EnvironmentHook, GameTestBatchListener, GameTestLifecycleEvent,
    GameTestListener, GameTestRunner, GameTestRunnerConfig,
};
use crate::server::gametest::game_test_sequence::{GameTestSequence, SequenceParent};
use crate::server::gametest::game_test_ticker::{GameTestTicker, TestInfoId, TickerState};
use crate::server::gametest::multiple_test_tracker::MultipleTestTracker;
use crate::server::gametest::registries::{
    GameTestRegistries, TEST_ENVIRONMENT, TEST_FUNCTION, TEST_INSTANCE,
};
use crate::server::gametest::reporter::{
    GameTestOutcome, GameTestReportEntry, GameTestRunStatus, JUnitLikeTestReporter, LogReportLevel,
    LogTestReporter, TestReporter,
};
use crate::server::gametest::retry_options::RetryOptions;
use crate::server::gametest::test_data::TestData;
use crate::server::gametest::test_environment_definition::{
    TestEnvironmentDefinition, TestEnvironmentRef, WeatherType, default_environment_definition,
    default_environment_key,
};
use crate::server::gametest::test_function_loader::{
    TestFunctionEntry, TestFunctionLoaderRegistry,
};

fn build_registries_with(
    extra_envs: Vec<(Identifier, TestEnvironmentDefinition)>,
    extra_tests: Vec<(Identifier, GameTestInstance)>,
) -> Arc<GameTestRegistries> {
    let base =
        GameTestRegistries::bootstrap_with_loaders(&TestFunctionLoaderRegistry::with_builtin());

    let mut env_builder = RegistryBuilder::new(&TEST_ENVIRONMENT);
    env_builder.register(
        &default_environment_key(),
        Arc::new(default_environment_definition()),
        RegistrationInfo::BUILT_IN,
    );
    for (id, def) in extra_envs {
        let key = ResourceKey::create(&TEST_ENVIRONMENT, id);
        env_builder.register(&key, Arc::new(def), RegistrationInfo::BUILT_IN);
    }
    let test_environments = env_builder.freeze();

    let mut inst_builder = RegistryBuilder::new(&TEST_INSTANCE);
    for (id, inst) in extra_tests {
        let key = ResourceKey::create(&TEST_INSTANCE, id);
        inst_builder.register(&key, Arc::new(inst), RegistrationInfo::BUILT_IN);
    }
    let test_instances = inst_builder.freeze();

    Arc::new(GameTestRegistries {
        environment_definition_types: base.environment_definition_types,
        instance_types: base.instance_types,
        test_functions: base.test_functions,
        test_environments,
        test_instances,
    })
}

fn fn_instance(
    fn_name: &str,
    env: TestEnvironmentRef,
    max_ticks: i32,
    setup_ticks: i32,
    required: bool,
    max_attempts: i32,
    required_successes: i32,
) -> GameTestInstance {
    let fn_key = ResourceKey::<TestFunctionEntry>::create(
        &TEST_FUNCTION,
        Identifier::with_default_namespace(fn_name),
    );
    let data = TestData::new(
        env,
        Identifier::with_default_namespace("empty"),
        max_ticks,
        setup_ticks,
        required,
        Rotation::None,
        false,
        max_attempts,
        required_successes,
        false,
        0,
    );
    GameTestInstance::Function(FunctionGameTestInstance::new(fn_key, data))
}

fn make_info(
    registries: &GameTestRegistries,
    name: &str,
    retry_options: RetryOptions,
) -> GameTestInfo {
    let id = Identifier::with_default_namespace(name);
    let key = ResourceKey::create(&TEST_INSTANCE, id.clone());
    let holder = registries
        .instance_holder(&key)
        .expect("registered test instance");
    GameTestInfo::new(id, holder, registries, Rotation::None, retry_options)
}

#[test]
fn empty_selection_finishes_with_explicit_non_success() {
    let registries = Arc::new(GameTestRegistries::bootstrap_default());
    let functions = TestFunctionRegistry::with_builtins();
    let config = GameTestRunnerConfig {
        discovered_count: 5,
        seed: 42,
        resource_provenance: "test-provenance".to_owned(),
        ..GameTestRunnerConfig::default()
    };
    let mut runner = GameTestRunner::new(Vec::new(), registries, config);

    let reporter_finished = Arc::new(AtomicI32::new(0));
    let rf = Arc::clone(&reporter_finished);
    struct CountingReporter(Arc<AtomicI32>);
    impl TestReporter for CountingReporter {
        fn on_test_failed(&mut self, _entry: &GameTestReportEntry) {}
        fn on_test_success(&mut self, _entry: &GameTestReportEntry) {}
        fn finish(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    runner.add_reporter(Box::new(CountingReporter(rf)));

    let transcript = runner.run_to_completion(&functions, 100);
    assert_eq!(runner.current_level_tick(), 0);
    assert!(runner.is_finished());
    assert_eq!(reporter_finished.load(Ordering::SeqCst), 1);

    assert_eq!(transcript.status, GameTestRunStatus::ZeroSelected);
    assert!(!transcript.is_success());
    assert_eq!(transcript.discovered_count, 5);
    assert_eq!(transcript.selected_count, 0);
    assert_eq!(transcript.executed_count, 0);
    assert_eq!(transcript.passed_count, 0);
    assert_eq!(transcript.failed_required_count, 0);
    assert_eq!(transcript.failed_optional_count, 0);
    assert_eq!(transcript.skipped_count, 0);
    assert_eq!(transcript.seed, 42);
    assert_eq!(transcript.resource_provenance, "test-provenance");
}

#[test]
fn exact_paper_setup_tick_and_delay_timing() {
    // Paper: tickCount = -(setupTicks + tickDelay + 1); tickInternal increments
    // tickCount first and starts when tickCount >= 0.
    // With setup_ticks = 2, tick_delay = 3 => initial tick_count = -6.
    // Ticks 1..=5 => tick_count = -5..-1; tick 6 => tick_count = 0 => starts & succeeds.
    let registries = build_registries_with(
        vec![],
        vec![(
            Identifier::with_default_namespace("delayed_pass"),
            fn_instance(
                "always_pass",
                TestEnvironmentRef::Key(default_environment_key()),
                10,
                2,
                true,
                1,
                1,
            ),
        )],
    );
    let functions = TestFunctionRegistry::with_builtins();
    let info = make_info(&registries, "delayed_pass", RetryOptions::no_retries());

    let config = GameTestRunnerConfig {
        tick_delay: 3,
        resource_provenance: "paper-timing".to_owned(),
        ..GameTestRunnerConfig::default()
    };
    let mut runner = GameTestRunner::new(vec![info], registries, config);
    let transcript = runner.run_to_completion(&functions, 50);

    assert_eq!(runner.current_level_tick(), 6);
    assert!(runner.is_finished());
    assert!(transcript.is_success());
    assert_eq!(transcript.records.len(), 1);
    let rec = &transcript.records[0];
    assert_eq!(rec.setup_tick, Some(0));
    assert_eq!(rec.start_tick, Some(6));
    assert_eq!(rec.end_tick, Some(6));
    assert_eq!(rec.final_test_tick, 0);
    assert_eq!(rec.outcome, GameTestOutcome::Passed);
}

#[test]
fn intentionally_failing_synthetic_test_paper_timing_and_reports() {
    // Paper: when fail() is called during tickInternal at tick_count = 5 (server tick 7),
    // GameTestInfo sets `error` on tick 7 and calls `finish()` + `testFailed` on the
    // NEXT tick (server tick 8, tick_count = 6).
    let registries = build_registries_with(
        vec![],
        vec![(
            Identifier::with_default_namespace("synthetic_fail"),
            fn_instance(
                "fail_at_5",
                TestEnvironmentRef::Key(default_environment_key()),
                20,
                0,
                true,
                1,
                1,
            ),
        )],
    );
    let mut functions = TestFunctionRegistry::with_builtins();
    functions.register(Identifier::with_default_namespace("fail_at_5"), |handle| {
        handle.run_after_delay(5, |h| {
            h.fail("intentional synthetic failure");
            Ok(())
        });
        Ok(())
    });

    let info = make_info(&registries, "synthetic_fail", RetryOptions::no_retries());
    let config = GameTestRunnerConfig {
        seed: 777,
        resource_provenance: "synthetic-fail".to_owned(),
        ..GameTestRunnerConfig::default()
    };
    let mut runner = GameTestRunner::new(vec![info], registries, config);

    let log_capture = Arc::new(Mutex::new(LogTestReporter::new()));
    let junit_capture = Arc::new(Mutex::new(JUnitLikeTestReporter::new("gametest")));

    struct SharedLogReporter(Arc<Mutex<LogTestReporter>>);
    impl TestReporter for SharedLogReporter {
        fn on_test_failed(&mut self, entry: &GameTestReportEntry) {
            self.0.lock().unwrap().on_test_failed(entry);
        }
        fn on_test_success(&mut self, entry: &GameTestReportEntry) {
            self.0.lock().unwrap().on_test_success(entry);
        }
        fn finish(&mut self) {
            self.0.lock().unwrap().finish();
        }
    }

    struct SharedJUnitReporter(Arc<Mutex<JUnitLikeTestReporter>>);
    impl TestReporter for SharedJUnitReporter {
        fn on_test_failed(&mut self, entry: &GameTestReportEntry) {
            self.0.lock().unwrap().on_test_failed(entry);
        }
        fn on_test_success(&mut self, entry: &GameTestReportEntry) {
            self.0.lock().unwrap().on_test_success(entry);
        }
        fn finish(&mut self) {
            self.0.lock().unwrap().finish();
        }
    }

    runner.add_reporter(Box::new(SharedLogReporter(Arc::clone(&log_capture))));
    runner.add_reporter(Box::new(SharedJUnitReporter(Arc::clone(&junit_capture))));

    let transcript = runner.run_to_completion(&functions, 50);
    // setup_ticks = 0, Paper spawners' startExecution(1) => initial tick_count = -2.
    // Server tick 2 => tick_count = 0 (starts, schedules callback for tick 5).
    // Server tick 7 => tick_count = 5 (callback calls fail(), sets error).
    // Server tick 8 => tick_count = 6 (finish() runs, testFailed fires).
    assert_eq!(runner.current_level_tick(), 8);
    assert!(runner.is_finished());
    assert_eq!(runner.failed_tracker().len(), 1);

    assert_eq!(transcript.status, GameTestRunStatus::Failed);
    assert!(!transcript.is_success());
    assert_eq!(transcript.failed_required_count, 1);
    let rec = &transcript.records[0];
    assert_eq!(rec.setup_tick, Some(0));
    assert_eq!(rec.start_tick, Some(2));
    assert_eq!(rec.end_tick, Some(8));
    assert_eq!(rec.outcome, GameTestOutcome::FailedRequired);
    assert!(
        rec.first_failure
            .as_deref()
            .unwrap_or("")
            .contains("intentional synthetic failure")
    );
    assert_eq!(rec.first_failure, rec.final_error);

    let logs = log_capture.lock().unwrap();
    assert_eq!(logs.entries().len(), 1);
    assert_eq!(logs.entries()[0].level, LogReportLevel::Error);
    assert_eq!(
        logs.entries()[0].message,
        "minecraft:synthetic_fail failed at 0, 64, 0! intentional synthetic failure on tick 5"
    );

    let junit = junit_capture.lock().unwrap();
    assert!(junit.is_finished());
    let xml = junit.to_xml();
    assert!(xml.contains("<testsuite"));
    assert!(xml.contains("<failure message=\""));
    assert!(xml.contains("minecraft:synthetic_fail"));
    assert!(xml.contains("intentional synthetic failure"));
}

#[test]
fn timeout_triggers_strictly_after_max_ticks_not_at_equality() {
    // Paper: `if (this.tickCount > this.timeoutTicks)` — NOT `>=`.
    // Test A succeeds at exact tick_count == max_ticks (5) => PASSES.
    // Test B never succeeds with max_ticks == 5 => times out at tick_count == 6,
    // finishes at server tick 9 (startExecution(1) => tick_count 0 on server tick 2).
    let registries = build_registries_with(
        vec![],
        vec![
            (
                Identifier::with_default_namespace("pass_at_exact_max"),
                fn_instance(
                    "succeed_at_5",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    true,
                    1,
                    1,
                ),
            ),
            (
                Identifier::with_default_namespace("timeout_after_5"),
                fn_instance(
                    "never_finish",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    true,
                    1,
                    1,
                ),
            ),
        ],
    );
    let mut functions = TestFunctionRegistry::with_builtins();
    functions.register(
        Identifier::with_default_namespace("succeed_at_5"),
        |handle| {
            handle.run_at_tick_time(5, |h| {
                h.succeed();
                Ok(())
            });
            Ok(())
        },
    );
    functions.register(
        Identifier::with_default_namespace("never_finish"),
        |_handle| Ok(()),
    );

    let infos = vec![
        make_info(&registries, "pass_at_exact_max", RetryOptions::no_retries()),
        make_info(&registries, "timeout_after_5", RetryOptions::no_retries()),
    ];
    let mut runner = GameTestRunner::new(infos, registries, GameTestRunnerConfig::default());
    let transcript = runner.run_to_completion(&functions, 50);

    // pass_at_exact_max finishes on server tick 7 (tick_count == 5).
    // timeout_after_5 exceeds timeout on server tick 8 (tick_count == 6 > 5)
    // and calls finish() on server tick 9.
    assert_eq!(runner.current_level_tick(), 9);
    assert_eq!(transcript.passed_count, 1);
    assert_eq!(transcript.failed_required_count, 1);
    assert_eq!(transcript.records[0].outcome, GameTestOutcome::Passed);
    assert_eq!(transcript.records[0].end_tick, Some(7));
    assert_eq!(
        transcript.records[1].outcome,
        GameTestOutcome::FailedRequired
    );
    assert_eq!(transcript.records[1].end_tick, Some(9));
    assert_eq!(
        transcript.records[1].final_error.as_deref(),
        Some("Didn't succeed or fail within 5 ticks")
    );
}

#[test]
fn flaky_test_retries_and_early_exhaustion_match_paper_report_listener() {
    // Test 1: max_attempts = 3, required_successes = 2.
    // Attempt 1 fails, attempts 2 and 3 pass => overall PASS (attempts=3, successes=2, retry_count=2).
    // Test 2: max_attempts = 4, required_successes = 3.
    // Attempt 1 fails, attempt 2 fails => remaining = 4 - 2 + 0 = 2 < 3 => early exhaustion!
    // Must stop at attempts == 2 (never runs attempts 3 or 4).
    let registries = build_registries_with(
        vec![],
        vec![
            (
                Identifier::with_default_namespace("flaky_passes"),
                fn_instance(
                    "flaky_pass_fn",
                    TestEnvironmentRef::Key(default_environment_key()),
                    10,
                    0,
                    true,
                    3,
                    2,
                ),
            ),
            (
                Identifier::with_default_namespace("flaky_exhausts_early"),
                fn_instance(
                    "always_fail_fn",
                    TestEnvironmentRef::Key(default_environment_key()),
                    10,
                    0,
                    true,
                    4,
                    3,
                ),
            ),
        ],
    );

    let flaky_counter = Arc::new(AtomicI32::new(0));
    let fc = Arc::clone(&flaky_counter);
    let fail_counter = Arc::new(AtomicI32::new(0));
    let fail_c = Arc::clone(&fail_counter);

    let mut functions = TestFunctionRegistry::with_builtins();
    functions.register(
        Identifier::with_default_namespace("flaky_pass_fn"),
        move |handle| {
            let n = fc.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                handle.fail("transient failure on attempt 1");
            } else {
                handle.succeed();
            }
            Ok(())
        },
    );
    functions.register(
        Identifier::with_default_namespace("always_fail_fn"),
        move |handle| {
            let n = fail_c.fetch_add(1, Ordering::SeqCst);
            handle.fail(format!("fail #{n}"));
            Ok(())
        },
    );

    let infos = vec![
        make_info(&registries, "flaky_passes", RetryOptions::no_retries()),
        make_info(
            &registries,
            "flaky_exhausts_early",
            RetryOptions::no_retries(),
        ),
    ];
    let mut runner = GameTestRunner::new(infos, registries, GameTestRunnerConfig::default());
    let log_capture = Arc::new(Mutex::new(LogTestReporter::new()));
    struct SharedLogReporter(Arc<Mutex<LogTestReporter>>);
    impl TestReporter for SharedLogReporter {
        fn on_test_failed(&mut self, entry: &GameTestReportEntry) {
            self.0.lock().unwrap().on_test_failed(entry);
        }
        fn on_test_success(&mut self, entry: &GameTestReportEntry) {
            self.0.lock().unwrap().on_test_success(entry);
        }
    }
    runner.add_reporter(Box::new(SharedLogReporter(Arc::clone(&log_capture))));
    runner.run_to_completion(&functions, 100);

    assert!(runner.is_finished());
    // Paper `rerunTest` adds every copy to `allTestInfos`: 2 originals + 2 + 1 copies.
    assert_eq!(runner.all_test_infos().len(), 5);
    // Paper `GlobalTestReporter.onTestFailed(testInfo)` reports the last attempt's own
    // error, not the `ExhaustedAttemptsException` used for chat output.
    {
        let logs = log_capture.lock().unwrap();
        assert_eq!(logs.entries().len(), 1);
        assert!(
            logs.entries()[0].message.ends_with("! fail #1 on tick 0"),
            "{:?}",
            logs.entries()[0].message
        );
    }
    assert_eq!(flaky_counter.load(Ordering::SeqCst), 3);
    // Early exhaustion: 4 - 2 + 0 = 2 < 3, so only 2 attempts run!
    assert_eq!(fail_counter.load(Ordering::SeqCst), 2);

    let transcript = runner.build_transcript();
    assert_eq!(transcript.passed_count, 1);
    assert_eq!(transcript.failed_required_count, 1);

    let r0 = &transcript.records[0];
    assert_eq!(r0.outcome, GameTestOutcome::Passed);
    assert_eq!(r0.attempts, 3);
    assert_eq!(r0.retry_count, 2);
    assert_eq!(r0.successes, 2);
    assert!(
        r0.first_failure
            .as_deref()
            .unwrap_or("")
            .contains("transient failure on attempt 1")
    );
    assert!(r0.final_error.is_none());

    let r1 = &transcript.records[1];
    assert_eq!(r1.outcome, GameTestOutcome::FailedRequired);
    assert_eq!(r1.attempts, 2);
    assert_eq!(r1.retry_count, 1);
    assert_eq!(r1.successes, 0);
    assert!(r1.final_error.as_deref().unwrap_or("").contains(
        "Not enough successes: 0 out of 2 attempts. Required successes: 3. max attempts: 4."
    ));
}

#[test]
fn optional_failure_does_not_fail_overall_run() {
    let registries = build_registries_with(
        vec![],
        vec![
            (
                Identifier::with_default_namespace("req_pass"),
                fn_instance(
                    "always_pass",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    true,
                    1,
                    1,
                ),
            ),
            (
                Identifier::with_default_namespace("opt_fail"),
                fn_instance(
                    "opt_fail_fn",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    false,
                    1,
                    1,
                ),
            ),
        ],
    );
    let mut functions = TestFunctionRegistry::with_builtins();
    functions.register(
        Identifier::with_default_namespace("opt_fail_fn"),
        |handle| {
            handle.fail("optional boom");
            Ok(())
        },
    );

    let infos = vec![
        make_info(&registries, "req_pass", RetryOptions::no_retries()),
        make_info(&registries, "opt_fail", RetryOptions::no_retries()),
    ];
    let mut runner = GameTestRunner::new(infos, registries, GameTestRunnerConfig::default());
    runner.run_to_completion(&functions, 50);

    let transcript = runner.build_transcript();
    assert_eq!(transcript.status, GameTestRunStatus::Passed);
    assert!(transcript.is_success());
    assert_eq!(transcript.passed_count, 1);
    assert_eq!(transcript.failed_optional_count, 1);
    assert_eq!(transcript.failed_required_count, 0);
    assert_eq!(
        transcript.records[1].outcome,
        GameTestOutcome::FailedOptional
    );
}

#[test]
fn batch_hooks_all_of_reverse_teardown_and_isolation_cleanup() {
    let env_a_id = Identifier::with_default_namespace("env_a");
    let env_b_id = Identifier::with_default_namespace("env_b");
    let leaf1_id = Identifier::with_default_namespace("leaf1");
    let leaf2_id = Identifier::with_default_namespace("leaf2");
    let leaf3_id = Identifier::with_default_namespace("leaf3");

    let leaf1_key = ResourceKey::create(&TEST_ENVIRONMENT, leaf1_id.clone());
    let leaf2_key = ResourceKey::create(&TEST_ENVIRONMENT, leaf2_id.clone());
    let leaf3_key = ResourceKey::create(&TEST_ENVIRONMENT, leaf3_id.clone());
    let env_a_key = ResourceKey::create(&TEST_ENVIRONMENT, env_a_id.clone());
    let env_b_key = ResourceKey::create(&TEST_ENVIRONMENT, env_b_id.clone());
    let clock = Identifier::with_default_namespace("overworld");

    let extra_envs = vec![
        (
            leaf1_id.clone(),
            TestEnvironmentDefinition::Weather {
                weather: WeatherType::Clear,
            },
        ),
        (
            leaf2_id.clone(),
            TestEnvironmentDefinition::ClockTime {
                clock: clock.clone(),
                time: 1000,
            },
        ),
        (
            leaf3_id.clone(),
            TestEnvironmentDefinition::Weather {
                weather: WeatherType::Rain,
            },
        ),
        (
            env_a_id.clone(),
            TestEnvironmentDefinition::AllOf {
                definitions: vec![
                    TestEnvironmentRef::Key(leaf1_key),
                    TestEnvironmentRef::Key(leaf2_key),
                    TestEnvironmentRef::Key(leaf3_key),
                ],
            },
        ),
        (
            env_b_id.clone(),
            TestEnvironmentDefinition::ClockTime { clock, time: 6000 },
        ),
    ];

    let extra_tests = vec![
        (
            Identifier::with_default_namespace("t_a1"),
            fn_instance(
                "always_pass",
                TestEnvironmentRef::Key(env_a_key.clone()),
                5,
                0,
                true,
                1,
                1,
            ),
        ),
        (
            Identifier::with_default_namespace("t_b1"),
            fn_instance(
                "always_pass",
                TestEnvironmentRef::Key(env_b_key.clone()),
                5,
                0,
                true,
                1,
                1,
            ),
        ),
        (
            Identifier::with_default_namespace("t_a2"),
            fn_instance(
                "always_pass",
                TestEnvironmentRef::Key(env_a_key),
                5,
                0,
                true,
                1,
                1,
            ),
        ),
    ];

    let registries = build_registries_with(extra_envs, extra_tests);
    let functions = TestFunctionRegistry::with_builtins();

    let infos = vec![
        make_info(&registries, "t_a1", RetryOptions::no_retries()),
        make_info(&registries, "t_b1", RetryOptions::no_retries()),
        make_info(&registries, "t_a2", RetryOptions::no_retries()),
    ];

    let config = GameTestRunnerConfig {
        clear_between_batches: true,
        ..GameTestRunnerConfig::default()
    };
    let mut runner = GameTestRunner::new(infos, registries, config);

    let hook_log = Arc::new(Mutex::new(Vec::<String>::new()));
    let hl = Arc::clone(&hook_log);
    struct RecordingEnvHook(Arc<Mutex<Vec<String>>>);
    impl EnvironmentHook for RecordingEnvHook {
        fn setup_leaf(&mut self, def: &TestEnvironmentDefinition) {
            self.0.lock().unwrap().push(format!("setup:{def:?}"));
        }
        fn teardown_leaf(&mut self, def: &TestEnvironmentDefinition) {
            self.0.lock().unwrap().push(format!("teardown:{def:?}"));
        }
    }
    runner.set_environment_hook(Box::new(RecordingEnvHook(hl)));

    let batch_log = Arc::new(Mutex::new(Vec::<String>::new()));
    let bl = Arc::clone(&batch_log);
    struct RecordingBatchListener(Arc<Mutex<Vec<String>>>);
    impl GameTestBatchListener for RecordingBatchListener {
        fn test_batch_starting(
            &mut self,
            batch_index: i32,
            _env: &Holder<TestEnvironmentDefinition>,
        ) {
            self.0.lock().unwrap().push(format!("start:{batch_index}"));
        }
        fn test_batch_finished(
            &mut self,
            batch_index: i32,
            _env: &Holder<TestEnvironmentDefinition>,
        ) {
            self.0.lock().unwrap().push(format!("finish:{batch_index}"));
        }
    }
    runner.add_batch_listener(Box::new(RecordingBatchListener(bl)));

    let test_log = Arc::new(Mutex::new(Vec::<String>::new()));
    let tl = Arc::clone(&test_log);
    struct RecordingTestListener(Arc<Mutex<Vec<String>>>);
    impl GameTestListener for RecordingTestListener {
        fn test_structure_loaded(&mut self, info: &GameTestInfo) {
            self.0.lock().unwrap().push(format!("loaded:{}", info.id()));
        }
        fn test_passed(&mut self, info: &GameTestInfo) {
            self.0.lock().unwrap().push(format!("passed:{}", info.id()));
        }
        fn test_failed(&mut self, info: &GameTestInfo) {
            self.0.lock().unwrap().push(format!("failed:{}", info.id()));
        }
        fn test_added_for_rerun(&mut self, original: &GameTestInfo, _copy: &GameTestInfo) {
            self.0
                .lock()
                .unwrap()
                .push(format!("rerun:{}", original.id()));
        }
    }
    runner.add_test_listener(Box::new(RecordingTestListener(tl)));

    runner.run_to_completion(&functions, 50);
    assert!(runner.is_finished());

    // Grouping preserves first-seen environment order: env_a (t_a1, t_a2) then env_b (t_b1).
    // Paper numbers batches per environment, so env_b's only batch is also index 0.
    assert_eq!(
        *batch_log.lock().unwrap(),
        vec!["start:0", "finish:0", "start:0", "finish:0"]
    );
    assert_eq!(
        *test_log.lock().unwrap(),
        vec![
            "loaded:minecraft:t_a1",
            "loaded:minecraft:t_a2",
            "passed:minecraft:t_a1",
            "passed:minecraft:t_a2",
            "loaded:minecraft:t_b1",
            "passed:minecraft:t_b1",
        ]
    );

    // Check AllOf setup order (leaf1, leaf2, leaf3) and reverse teardown order (leaf3, leaf2, leaf1)!
    let env_events: Vec<(String, Identifier)> = runner
        .events()
        .iter()
        .filter_map(|ev| match ev {
            GameTestLifecycleEvent::EnvironmentSetup { id, .. } => {
                Some(("setup".to_owned(), id.clone()))
            }
            GameTestLifecycleEvent::EnvironmentTeardown { id, .. } => {
                Some(("teardown".to_owned(), id.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        env_events,
        vec![
            ("setup".to_owned(), leaf1_id.clone()),
            ("setup".to_owned(), leaf2_id.clone()),
            ("setup".to_owned(), leaf3_id.clone()),
            ("teardown".to_owned(), leaf3_id),
            ("teardown".to_owned(), leaf2_id),
            ("teardown".to_owned(), leaf1_id),
            ("setup".to_owned(), env_b_id.clone()),
            ("teardown".to_owned(), env_b_id),
        ]
    );

    // Check isolation cleanup: Batch 0 had 2 structures cleared before Batch 1 started,
    // forced chunks cleared after each batch (by position in the batch list), and
    // nothing is cleared after the final batch.
    assert!(
        runner
            .events()
            .contains(&GameTestLifecycleEvent::BatchStructuresCleared {
                batch_index: 0,
                count: 2
            })
    );
    assert!(
        runner
            .events()
            .contains(&GameTestLifecycleEvent::ForcedChunksCleared { batch_index: 0 })
    );
    assert!(
        runner
            .events()
            .contains(&GameTestLifecycleEvent::ForcedChunksCleared { batch_index: 1 })
    );
    assert_eq!(
        runner
            .events()
            .iter()
            .filter(|ev| matches!(ev, GameTestLifecycleEvent::BatchStructuresCleared { .. }))
            .count(),
        1
    );
}

#[test]
fn halt_on_error_stops_before_subsequent_batches() {
    let env_a_key = ResourceKey::create(
        &TEST_ENVIRONMENT,
        Identifier::with_default_namespace("env_a"),
    );
    let env_b_key = ResourceKey::create(
        &TEST_ENVIRONMENT,
        Identifier::with_default_namespace("env_b"),
    );
    let clock = Identifier::with_default_namespace("overworld");
    let registries = build_registries_with(
        vec![
            (
                Identifier::with_default_namespace("env_a"),
                TestEnvironmentDefinition::ClockTime {
                    clock: clock.clone(),
                    time: 100,
                },
            ),
            (
                Identifier::with_default_namespace("env_b"),
                TestEnvironmentDefinition::ClockTime { clock, time: 200 },
            ),
        ],
        vec![
            (
                Identifier::with_default_namespace("fail_first"),
                fn_instance("boom", TestEnvironmentRef::Key(env_a_key), 5, 0, true, 1, 1),
            ),
            (
                Identifier::with_default_namespace("should_skip"),
                fn_instance(
                    "always_pass",
                    TestEnvironmentRef::Key(env_b_key),
                    5,
                    0,
                    true,
                    1,
                    1,
                ),
            ),
        ],
    );
    let mut functions = TestFunctionRegistry::with_builtins();
    functions.register(Identifier::with_default_namespace("boom"), |h| {
        h.fail("hard failure");
        Ok(())
    });

    let infos = vec![
        make_info(&registries, "fail_first", RetryOptions::no_retries()),
        make_info(&registries, "should_skip", RetryOptions::no_retries()),
    ];
    let config = GameTestRunnerConfig {
        halt_on_error: true,
        ..GameTestRunnerConfig::default()
    };
    let mut runner = GameTestRunner::new(infos, registries, config);
    runner.run_to_completion(&functions, 50);

    assert!(runner.is_finished());
    assert!(runner.is_stopped());
    let transcript = runner.build_transcript();
    assert_eq!(transcript.status, GameTestRunStatus::Failed);
    assert_eq!(transcript.failed_required_count, 1);
    assert_eq!(transcript.skipped_count, 1);
    assert_eq!(transcript.records[1].outcome, GameTestOutcome::Skipped);
}

#[test]
fn ticker_idle_running_halting_state_machine() {
    let mut ticker = GameTestTicker::new();
    assert_eq!(ticker.state(), TickerState::Idle);
    assert!(ticker.is_empty());

    ticker.add(TestInfoId(0));
    ticker.add(TestInfoId(1));
    assert_eq!(ticker.len(), 2);

    // Calling clear() during tick_with transitions Running -> Halting and clears
    // after the current tick pass completes.
    let mut visited = Vec::new();
    let res = ticker.tick_with(|id, t| {
        visited.push(id);
        if id == TestInfoId(0) {
            t.clear();
            assert_eq!(t.state(), TickerState::Halting);
        }
        false
    });
    assert!(res.halted);
    assert_eq!(visited, vec![TestInfoId(0), TestInfoId(1)]);
    assert_eq!(ticker.state(), TickerState::Idle);
    assert!(ticker.is_empty());
}

#[test]
fn sequence_then_idle_and_then_execute_after_timing() {
    let registries = build_registries_with(
        vec![],
        vec![(
            Identifier::with_default_namespace("seq_test"),
            fn_instance(
                "seq_fn",
                TestEnvironmentRef::Key(default_environment_key()),
                20,
                0,
                true,
                1,
                1,
            ),
        )],
    );
    let mut functions = TestFunctionRegistry::with_builtins();
    let executed_at = Arc::new(AtomicI32::new(-1));
    let ea = Arc::clone(&executed_at);
    functions.register(
        Identifier::with_default_namespace("seq_fn"),
        move |handle| {
            let ea_inner = Arc::clone(&ea);
            let seq = GameTestSequence::new()
                .then_idle(3)
                .then_execute_after(2, move |tick| {
                    ea_inner.store(tick, Ordering::SeqCst);
                    Ok(())
                })
                .then_succeed();
            handle.add_sequence(seq);
            Ok(())
        },
    );

    let info = make_info(&registries, "seq_test", RetryOptions::no_retries());
    let mut runner = GameTestRunner::new(vec![info], registries, GameTestRunnerConfig::default());
    runner.run_to_completion(&functions, 50);

    let transcript = runner.build_transcript();
    assert!(transcript.is_success());
    assert_eq!(executed_at.load(Ordering::SeqCst), 5);
}

#[test]
fn batch_factory_partitions_at_max_tests_per_batch_and_multiple_tracker_progress_bar() {
    let registries = GameTestRegistries::bootstrap_default();
    let holder = registries
        .instance_holder(&always_pass_instance_key())
        .expect("always_pass");
    let items: Vec<GameTestInfo> = (0..53)
        .map(|i| {
            direct_test_info(
                Identifier::with_default_namespace(&format!("t_{i}")),
                holder.clone(),
                &registries,
            )
        })
        .collect();

    let batches = divide_into_batches(
        items,
        MAX_TESTS_PER_BATCH,
        &registries.test_environments,
        |info| info.batch().clone(),
    );
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].index(), 0);
    assert_eq!(batches[0].game_test_infos().len(), 50);
    assert_eq!(batches[1].index(), 1);
    assert_eq!(batches[1].game_test_infos().len(), 3);

    // Verify MultipleTestTracker progress bar characters match Paper exactly.
    let reg2 = build_registries_with(
        vec![],
        vec![
            (
                Identifier::with_default_namespace("m0"),
                fn_instance(
                    "always_pass",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    true,
                    1,
                    1,
                ),
            ),
            (
                Identifier::with_default_namespace("m1"),
                fn_instance(
                    "never",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    true,
                    1,
                    1,
                ),
            ),
            (
                Identifier::with_default_namespace("m2"),
                fn_instance(
                    "always_pass",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    true,
                    1,
                    1,
                ),
            ),
            (
                Identifier::with_default_namespace("m3"),
                fn_instance(
                    "fail_fn",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    false,
                    1,
                    1,
                ),
            ),
            (
                Identifier::with_default_namespace("m4"),
                fn_instance(
                    "fail_fn",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    true,
                    1,
                    1,
                ),
            ),
        ],
    );
    let mut fns = TestFunctionRegistry::with_builtins();
    fns.register(Identifier::with_default_namespace("never"), |_| Ok(()));
    fns.register(Identifier::with_default_namespace("fail_fn"), |h| {
        h.fail("boom");
        Ok(())
    });

    let mut arena = vec![
        make_info(&reg2, "m0", RetryOptions::no_retries()),
        make_info(&reg2, "m1", RetryOptions::no_retries()),
        make_info(&reg2, "m2", RetryOptions::no_retries()),
        make_info(&reg2, "m3", RetryOptions::no_retries()),
        make_info(&reg2, "m4", RetryOptions::no_retries()),
    ];
    for (idx, info) in arena.iter_mut().enumerate() {
        info.set_placed(true);
        info.set_test_block_pos(Some(BlockPos::new(idx as i32 * 10, 64, 0)));
    }
    // m0: not started
    // m1: started, ongoing
    arena[1].start_execution(0, 0);
    arena[1].tick(1, &fns);
    // m2: passed
    arena[2].start_execution(0, 0);
    arena[2].tick(1, &fns);
    // m3: failed optional
    arena[3].start_execution(0, 0);
    arena[3].tick(1, &fns);
    arena[3].tick(2, &fns);
    // m4: failed required
    arena[4].start_execution(0, 0);
    arena[4].tick(1, &fns);
    arena[4].tick(2, &fns);

    let tracker = MultipleTestTracker::from_tests([
        TestInfoId(0),
        TestInfoId(1),
        TestInfoId(2),
        TestInfoId(3),
        TestInfoId(4),
    ]);
    assert_eq!(
        tracker.progress_bar(&arena),
        format!(
            "[{}{}{}{}{}]",
            MultipleTestTracker::NOT_STARTED_TEST_CHAR,
            MultipleTestTracker::ONGOING_TEST_CHAR,
            MultipleTestTracker::SUCCESSFUL_TEST_CHAR,
            MultipleTestTracker::FAILED_OPTIONAL_TEST_CHAR,
            MultipleTestTracker::FAILED_REQUIRED_TEST_CHAR,
        )
    );
    assert_eq!(tracker.done_count(&arena), 3);
    assert_eq!(tracker.failed_required_count(&arena), 1);
    assert_eq!(tracker.failed_optional_count(&arena), 1);
}

#[test]
fn deterministic_repeated_runs_produce_identical_transcripts() {
    let run_once = || {
        let registries = build_registries_with(
            vec![],
            vec![
                (
                    Identifier::with_default_namespace("a_pass"),
                    fn_instance(
                        "always_pass",
                        TestEnvironmentRef::Key(default_environment_key()),
                        10,
                        1,
                        true,
                        1,
                        1,
                    ),
                ),
                (
                    Identifier::with_default_namespace("b_fail"),
                    fn_instance(
                        "fail_fn",
                        TestEnvironmentRef::Key(default_environment_key()),
                        10,
                        0,
                        true,
                        1,
                        1,
                    ),
                ),
            ],
        );
        let mut functions = TestFunctionRegistry::with_builtins();
        functions.register(Identifier::with_default_namespace("fail_fn"), |h| {
            h.fail_with(GameTestException::assert_pos(
                "block mismatch",
                BlockPos::new(10, 64, 20),
                BlockPos::new(1, 0, 2),
                h.current_tick(),
            ));
            Ok(())
        });
        let infos = vec![
            make_info(&registries, "a_pass", RetryOptions::no_retries()),
            make_info(&registries, "b_fail", RetryOptions::no_retries()),
        ];
        let config = GameTestRunnerConfig {
            seed: 12345,
            resource_provenance: "paper-26.2".to_owned(),
            ..GameTestRunnerConfig::default()
        };
        let mut runner = GameTestRunner::new(infos, registries, config);
        runner.set_spawner(Box::new(DeterministicGridSpawner::new(
            BlockPos::new(0, 64, 0),
            16,
            8,
        )));
        runner.run_to_completion(&functions, 50);
        runner.build_transcript()
    };

    let t1 = run_once();
    let t2 = run_once();
    assert_eq!(t1, t2);
}

/// Minimal `GameTestInfo` stand-in recording what a sequence does to its parent.
#[derive(Default)]
struct RecordingParent {
    tick: i32,
    error: Option<GameTestException>,
    succeeded: bool,
}

impl SequenceParent for RecordingParent {
    fn tick(&self) -> i32 {
        self.tick
    }
    fn fail(&mut self, error: GameTestException) {
        self.error = Some(error);
    }
    fn succeed(&mut self) {
        // Paper `GameTestInfo.succeed()` is a no-op once an error is recorded.
        if self.error.is_none() {
            self.succeeded = true;
        }
    }
}

impl RecordingParent {
    fn error_message(&self) -> String {
        self.error
            .as_ref()
            .map(GameTestException::message)
            .unwrap_or_default()
    }
}

#[test]
fn sequence_terminal_failures_fail_immediately_instead_of_retrying() {
    // 1) then_execute assertion error fails the parent immediately (executeWithoutFail)
    //    and the sequence keeps going; the later succeed() is a no-op.
    let mut parent = RecordingParent::default();
    let mut seq = GameTestSequence::new()
        .then_execute(|tick| Err(GameTestException::assert("execute boom", tick)))
        .then_succeed();
    seq.tick_and_continue(&mut parent, 0);
    assert_eq!(parent.error_message(), "execute boom on tick 0");
    assert!(!parent.succeeded);
    assert!(seq.is_empty());

    // 2) then_fail fails immediately instead of being swallowed as a wait.
    let mut parent = RecordingParent::default();
    let mut seq = GameTestSequence::new()
        .then_fail(|| GameTestException::assert("explicit fail", 0))
        .then_succeed();
    seq.tick_and_continue(&mut parent, 0);
    assert_eq!(parent.error_message(), "explicit fail on tick 0");
    assert!(!parent.succeeded);

    // 3) expected_delay mismatch fails the parent and stops before then_succeed.
    let mut parent = RecordingParent {
        tick: 2,
        ..RecordingParent::default()
    };
    let mut seq = GameTestSequence::new()
        .then_wait_until_at(5, |_| Ok(()))
        .then_succeed();
    seq.tick_and_continue(&mut parent, 2);
    assert_eq!(
        parent.error_message(),
        "Succeeded in invalid tick: expected 5 on tick 2"
    );
    assert!(!parent.succeeded);
    assert_eq!(seq.len(), 1);

    // 4) then_wait_at_least runs its assertion first, then fails with minimum_tick.
    let ran = Arc::new(AtomicI32::new(0));
    let ran_inner = Arc::clone(&ran);
    let mut parent = RecordingParent {
        tick: 1,
        ..RecordingParent::default()
    };
    let mut seq = GameTestSequence::new().then_wait_at_least(3, move |_| {
        ran_inner.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    seq.tick_and_continue(&mut parent, 1);
    assert_eq!(ran.load(Ordering::SeqCst), 1);
    assert_eq!(
        parent.error_message(),
        "Succeeded before expected tick: expected to wait 3 on tick 1"
    );
}

#[test]
fn sequence_execute_for_and_idle_compare_ticks_like_paper() {
    // thenExecuteFor(3) from lastTick 0 runs the check on ticks 0..=2 and completes
    // on tick 3 without running it.
    let runs = Arc::new(Mutex::new(Vec::new()));
    let runs_inner = Arc::clone(&runs);
    let completed = Arc::new(AtomicI32::new(-1));
    let completed_inner = Arc::clone(&completed);
    let mut seq = GameTestSequence::new()
        .then_execute_for(3, move |tick| {
            runs_inner.lock().unwrap().push(tick);
            Ok(())
        })
        .then_execute(move |tick| {
            completed_inner.store(tick, Ordering::SeqCst);
            Ok(())
        });
    let mut parent = RecordingParent::default();
    for tick in 0..=3 {
        parent.tick = tick;
        seq.tick_and_continue(&mut parent, tick);
    }
    assert_eq!(*runs.lock().unwrap(), vec![0, 1, 2]);
    assert_eq!(completed.load(Ordering::SeqCst), 3);
    assert!(parent.error.is_none());

    // A skipped tick compares ticks rather than counting runs.
    let runs = Arc::new(AtomicI32::new(0));
    let runs_inner = Arc::clone(&runs);
    let mut seq = GameTestSequence::new().then_execute_for(3, move |_| {
        runs_inner.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let mut parent = RecordingParent::default();
    seq.tick_and_continue(&mut parent, 0);
    parent.tick = 5;
    seq.tick_and_continue(&mut parent, 5);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(seq.is_empty());

    // At timeout an unfinished then_idle fails the parent with not_completed.
    let mut seq = GameTestSequence::new().then_idle(3).then_succeed();
    let mut parent = RecordingParent {
        tick: 1,
        ..RecordingParent::default()
    };
    seq.tick_and_fail_if_not_complete(&mut parent, 1);
    assert_eq!(
        parent.error_message(),
        "Test timed out before sequence completed on tick 1"
    );
}

#[test]
fn sequence_success_on_timeout_tick_still_fails_with_no_sequences_finished() {
    // Paper `tickInternal`: past the timeout, sequences tick via
    // tickAndFailIfNotComplete and then `if (this.error == null) fail(...)` — a
    // sequence that succeeds on that tick still ends as a timeout failure.
    let registries = build_registries_with(
        vec![],
        vec![(
            Identifier::with_default_namespace("late_success"),
            fn_instance(
                "late_success_fn",
                TestEnvironmentRef::Key(default_environment_key()),
                5,
                0,
                true,
                1,
                1,
            ),
        )],
    );
    let mut functions = TestFunctionRegistry::with_builtins();
    functions.register(Identifier::with_default_namespace("late_success_fn"), |h| {
        h.add_sequence(GameTestSequence::new().then_idle(6).then_succeed());
        Ok(())
    });
    let infos = vec![make_info(
        &registries,
        "late_success",
        RetryOptions::no_retries(),
    )];
    let mut runner = GameTestRunner::new(infos, registries, GameTestRunnerConfig::default());
    let transcript = runner.run_to_completion(&functions, 50);

    assert_eq!(transcript.failed_required_count, 1);
    let rec = &transcript.records[0];
    assert_eq!(rec.outcome, GameTestOutcome::FailedRequired);
    assert_eq!(rec.final_test_tick, 6);
    assert_eq!(
        rec.final_error.as_deref(),
        Some("No sequences finished within 5 ticks")
    );
}

#[test]
fn late_sequence_anchors_delays_to_parent_tick() {
    let registries = build_registries_with(
        vec![],
        vec![(
            Identifier::with_default_namespace("late_seq"),
            fn_instance(
                "late_seq_fn",
                TestEnvironmentRef::Key(default_environment_key()),
                30,
                0,
                true,
                1,
                1,
            ),
        )],
    );
    let mut functions = TestFunctionRegistry::with_builtins();
    functions.register(Identifier::with_default_namespace("late_seq_fn"), |h| {
        h.run_at_tick_time(10, |h2| {
            h2.add_sequence(GameTestSequence::new().then_idle(5).then_succeed());
            Ok(())
        });
        Ok(())
    });

    let infos = vec![make_info(
        &registries,
        "late_seq",
        RetryOptions::no_retries(),
    )];
    let mut runner = GameTestRunner::new(infos, registries, GameTestRunnerConfig::default());
    let transcript = runner.run_to_completion(&functions, 50);

    assert!(transcript.is_success());
    assert_eq!(
        transcript.records[0].final_test_tick, 15,
        "sequence added at tick 10 with then_idle(5) must succeed at tick 15, not 10"
    );
}

#[test]
fn unspawned_batch_never_advances_and_is_not_success() {
    // Paper fires testBatchFinished/runBatch(next) only from a tracked test's
    // listener, so a batch in which every spawn failed never advances.
    struct RejectingSpawner;
    impl super::StructureSpawner for RejectingSpawner {
        fn on_batch_start(&mut self, _batch_index: i32) {}
        fn spawn_structure(&mut self, _info: &mut GameTestInfo) -> Option<BlockPos> {
            None
        }
        fn clear_structures(&mut self, _positions: &[BlockPos]) {}
    }

    let registries = build_registries_with(
        vec![],
        vec![(
            Identifier::with_default_namespace("unspawned_req"),
            fn_instance(
                "always_pass",
                TestEnvironmentRef::Key(default_environment_key()),
                5,
                0,
                true,
                1,
                1,
            ),
        )],
    );
    let functions = TestFunctionRegistry::with_builtins();
    let infos = vec![make_info(
        &registries,
        "unspawned_req",
        RetryOptions::no_retries(),
    )];
    let mut runner = GameTestRunner::new(infos, registries, GameTestRunnerConfig::default());
    runner.set_spawner(Box::new(RejectingSpawner));
    let transcript = runner.run_to_completion(&functions, 20);

    assert!(!runner.is_finished());
    assert_eq!(transcript.status, GameTestRunStatus::Incomplete);
    assert!(!transcript.is_success());
    assert_eq!(transcript.skipped_count, 1);
    assert_eq!(transcript.executed_count, 0);
}

#[test]
fn halt_on_error_still_records_same_tick_completions() {
    let registries = build_registries_with(
        vec![],
        vec![
            (
                Identifier::with_default_namespace("a_fail"),
                fn_instance(
                    "boom",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    true,
                    1,
                    1,
                ),
            ),
            (
                Identifier::with_default_namespace("b_pass_same_tick"),
                fn_instance(
                    "always_pass",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    true,
                    1,
                    1,
                ),
            ),
        ],
    );
    let mut functions = TestFunctionRegistry::with_builtins();
    functions.register(Identifier::with_default_namespace("boom"), |h| {
        h.fail("hard failure");
        Ok(())
    });

    let infos = vec![
        make_info(&registries, "a_fail", RetryOptions::no_retries()),
        make_info(&registries, "b_pass_same_tick", RetryOptions::no_retries()),
    ];
    let config = GameTestRunnerConfig {
        halt_on_error: true,
        ..GameTestRunnerConfig::default()
    };
    let mut runner = GameTestRunner::new(infos, registries, config);
    let transcript = runner.run_to_completion(&functions, 20);

    assert_eq!(transcript.status, GameTestRunStatus::Failed);
    assert_eq!(transcript.failed_required_count, 1);
    assert_eq!(
        transcript.passed_count, 1,
        "test that completed on the same tick must still be recorded as Passed"
    );
    assert_eq!(transcript.skipped_count, 0);
}

#[test]
fn deterministic_grid_spawner_skips_preserved_positions_to_avoid_collisions() {
    let registries = build_registries_with(
        vec![],
        vec![
            (
                Identifier::with_default_namespace("preserved"),
                fn_instance(
                    "always_pass",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    true,
                    1,
                    1,
                ),
            ),
            (
                Identifier::with_default_namespace("fresh"),
                fn_instance(
                    "always_pass",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    true,
                    1,
                    1,
                ),
            ),
        ],
    );
    let mut preserved = make_info(&registries, "preserved", RetryOptions::no_retries());
    let origin = BlockPos::new(0, 64, 0);
    preserved.set_test_block_pos(Some(origin));
    let mut fresh = make_info(&registries, "fresh", RetryOptions::no_retries());

    use super::StructureSpawner;
    let mut spawner = DeterministicGridSpawner::new(origin, 16, 8);
    spawner.on_batch_start(0);
    let p1 = spawner.spawn_structure(&mut preserved).unwrap();
    let p2 = spawner.spawn_structure(&mut fresh).unwrap();

    assert_eq!(p1, origin);
    assert_ne!(
        p1, p2,
        "fresh grid allocation must skip slots already occupied by preserved positions"
    );
}

#[test]
fn junit_xml_strips_illegal_xml_1_0_control_characters() {
    use rivet_registry::core::Rotation;
    let mut reporter = JUnitLikeTestReporter::new("2026-01-01T00:00:00Z");
    reporter.on_test_failed(&GameTestReportEntry {
        id: Identifier::with_default_namespace("ctrl_test"),
        structure: Identifier::with_default_namespace("empty"),
        required: true,
        rotation: Rotation::None,
        test_block_pos: None,
        run_time_ms: 10,
        tick_count: 1,
        error: Some(GameTestException::assert(
            "bad\u{0000}null\u{001B}esc\tand\nok",
            1,
        )),
    });
    reporter.finish();
    let xml = reporter.to_xml();
    assert!(!xml.contains('\u{0000}'), "NUL must be stripped from XML");
    assert!(!xml.contains('\u{001B}'), "ESC must be stripped from XML");
    assert!(xml.contains("badnullesc\tand\nok"));
}

#[test]
fn halt_on_error_halts_on_optional_failure() {
    // Paper's batch tracker `testFailed` halts whenever `haltOnError` is set,
    // without checking `isRequired()`.
    let env_b_key = ResourceKey::create(
        &TEST_ENVIRONMENT,
        Identifier::with_default_namespace("env_b"),
    );
    let registries = build_registries_with(
        vec![(
            Identifier::with_default_namespace("env_b"),
            TestEnvironmentDefinition::ClockTime {
                clock: Identifier::with_default_namespace("overworld"),
                time: 200,
            },
        )],
        vec![
            (
                Identifier::with_default_namespace("optional_fail"),
                fn_instance(
                    "boom",
                    TestEnvironmentRef::Key(default_environment_key()),
                    5,
                    0,
                    false,
                    1,
                    1,
                ),
            ),
            (
                Identifier::with_default_namespace("later_batch"),
                fn_instance(
                    "always_pass",
                    TestEnvironmentRef::Key(env_b_key),
                    5,
                    0,
                    true,
                    1,
                    1,
                ),
            ),
        ],
    );
    let mut functions = TestFunctionRegistry::with_builtins();
    functions.register(Identifier::with_default_namespace("boom"), |h| {
        h.fail("optional failure");
        Ok(())
    });
    let infos = vec![
        make_info(&registries, "optional_fail", RetryOptions::no_retries()),
        make_info(&registries, "later_batch", RetryOptions::no_retries()),
    ];
    let config = GameTestRunnerConfig {
        halt_on_error: true,
        ..GameTestRunnerConfig::default()
    };
    let mut runner = GameTestRunner::new(infos, registries, config);
    let transcript = runner.run_to_completion(&functions, 50);

    assert!(runner.is_stopped());
    assert_eq!(transcript.failed_optional_count, 1);
    assert_eq!(transcript.records[1].outcome, GameTestOutcome::Skipped);
}

#[test]
fn junit_xml_matches_jdk_transformer_output() {
    let mut reporter = JUnitLikeTestReporter::new("2026-01-01T00:00:00Z");
    reporter.on_test_success(&GameTestReportEntry {
        id: Identifier::with_default_namespace("pass_it"),
        structure: Identifier::with_default_namespace("empty"),
        required: true,
        rotation: Rotation::None,
        test_block_pos: Some(BlockPos::new(1, 2, 3)),
        run_time_ms: 1000,
        tick_count: 20,
        error: None,
    });
    reporter.on_test_failed(&GameTestReportEntry {
        id: Identifier::with_default_namespace("opt_it"),
        structure: Identifier::with_default_namespace("empty"),
        required: false,
        rotation: Rotation::None,
        test_block_pos: Some(BlockPos::new(1, 2, 3)),
        run_time_ms: 50,
        tick_count: 1,
        error: Some(GameTestException::assert("it's <bad>", 1)),
    });
    reporter.finish();
    assert_eq!(
        reporter.to_xml(),
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\
         <testsuite><testsuite time=\"1.05\" timestamp=\"2026-01-01T00:00:00Z\">\
         <testcase classname=\"minecraft:empty\" name=\"minecraft:pass_it\" time=\"1.0\"/>\
         <testcase classname=\"minecraft:empty\" name=\"minecraft:opt_it\" time=\"0.05\">\
         <skipped message=\"(1, 2, 3) it's &lt;bad&gt; on tick 1\"/></testcase>\
         </testsuite></testsuite>"
    );

    let empty = JUnitLikeTestReporter::new("t");
    assert_eq!(
        empty.to_xml(),
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\
         <testsuite><testsuite time=\"0.0\" timestamp=\"t\"/></testsuite>"
    );
}
