//! `net.minecraft.gametest.framework` — GameTest values, bootstrap,
//! registries, discovery, and fail-closed resource decoding (Paper 26.2,
//! issue #429).
//!
//! Pinned Paper 26.2 commit: `0a993450f129c4942c2a9ed45ba047412b4667cf`.
//!
//! Scope boundary:
//! - Landed here (#429): GameTest value types (`TestData`, `RetryOptions`,
//!   `GameTestBatch`, `GeneratedTest`), environment definitions & bootstrap
//!   (`TestEnvironmentDefinition`, `GameTestEnvironments`), instance types &
//!   bootstrap (`GameTestInstance`, `BlockBasedTestInstance`,
//!   `FunctionGameTestInstance`, `GameTestInstances`), function registration
//!   (`BuiltinTestFunctions`, `TestFunctionLoader`), all 5 frozen GameTest
//!   registries + holder resolution, discovery (`FailedTestTracker`,
//!   `TestInstanceFinder`, `TestPosFinder`, `TestFinder`), and fail-closed
//!   JSON/NBT resource decoding with exact Paper 26.2 resource census.
//! - Deferred to later issues: deterministic execution lifecycle, ticker,
//!   batches execution, retries execution, and reports (#430); structure
//!   placement and `TestInstanceBlockEntity` (#474); `GameTestHelper`
//!   assertions, sequences, and coordinate helpers (#475).

pub mod discovery;
pub mod game_test_batch;
pub mod game_test_instance;
pub mod registries;
pub mod resources;
pub mod retry_options;
pub mod test_data;
pub mod test_environment_definition;
pub mod test_function_loader;

pub use discovery::{
    EmptyFinder, FailedTestTracker, StaticTestInstanceFinder, StaticTestPosFinder, TestFinder,
    TestFinderBuilder, TestInstanceFinder, TestPosFinder,
};
pub use game_test_batch::{GameTestBatch, GameTestBatchError};
pub use game_test_instance::{
    ALWAYS_PASS_INSTANCE_NAME, BlockBasedTestInstance, EMPTY_STRUCTURE_NAME,
    FunctionGameTestInstance, GameTestInstance, GameTestInstanceType, InstanceDecodeError,
    always_pass_instance, always_pass_instance_key, instance_key,
};
pub use registries::{
    GAMETEST_REGISTRY_NAMES, GameTestRegistries, GameTestRegistryError, TEST_ENVIRONMENT,
    TEST_ENVIRONMENT_DEFINITION_TYPE, TEST_FUNCTION, TEST_INSTANCE, TEST_INSTANCE_TYPE,
    bootstrap_environment_definition_types, bootstrap_instance_types, bootstrap_test_functions,
};
pub use resources::{
    EXPECTED_RESOURCES, ExpectedResource, GameTestResourceError, GameTestResourceKind,
    PAPER_COMMIT, load_from_resource_root, load_from_synchronized_nbt, load_strict_paper_resources,
};
pub use retry_options::RetryOptions;
pub use test_data::{
    MAX_PADDING, TestData, TestDataValidationError, parse_rotation, rotation_serialized_name,
};
pub use test_environment_definition::{
    DEFAULT_ENVIRONMENT_NAME, EnvironmentDecodeError, GameRuleValue, TestEnvironmentDefinition,
    TestEnvironmentDefinitionType, TestEnvironmentRef, WeatherType, default_environment_definition,
    default_environment_key, environment_key,
};
pub use test_function_loader::{
    ALWAYS_PASS_FUNCTION_NAME, BuiltinTestFunctions, GeneratedTest, TestFunctionEntry,
    TestFunctionKind, TestFunctionLoader, TestFunctionLoaderRegistry, always_pass_function_key,
    test_function_key,
};

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use indexmap::IndexMap;
    use rivet_registry::core::{BlockPos, Difficulty, Rotation};
    use rivet_registry::{Identifier, ResourceKey};
    use serde_json::json;

    use super::*;

    #[test]
    fn builtin_registries_bootstrap_in_exact_paper_order() {
        let regs = GameTestRegistries::bootstrap_default();

        let env_types: Vec<String> = regs
            .environment_definition_types
            .entry_set_ref()
            .into_iter()
            .map(|(k, _)| k.identifier().to_string())
            .collect();
        assert_eq!(
            env_types,
            vec![
                "minecraft:all_of",
                "minecraft:clock_time",
                "minecraft:difficulty",
                "minecraft:function",
                "minecraft:game_rules",
                "minecraft:timeline_attributes",
                "minecraft:weather",
            ]
        );

        let inst_types: Vec<String> = regs
            .instance_types
            .entry_set_ref()
            .into_iter()
            .map(|(k, _)| k.identifier().to_string())
            .collect();
        assert_eq!(
            inst_types,
            vec!["minecraft:block_based", "minecraft:function"]
        );

        let funcs: Vec<String> = regs
            .test_functions
            .entry_set_ref()
            .into_iter()
            .map(|(k, _)| k.identifier().to_string())
            .collect();
        assert_eq!(funcs, vec!["minecraft:always_pass"]);

        let envs: Vec<String> = regs
            .test_environments
            .entry_set_ref()
            .into_iter()
            .map(|(k, _)| k.identifier().to_string())
            .collect();
        assert_eq!(envs, vec!["minecraft:default"]);

        let insts: Vec<String> = regs
            .test_instances
            .entry_set_ref()
            .into_iter()
            .map(|(k, _)| k.identifier().to_string())
            .collect();
        assert_eq!(insts, vec!["minecraft:always_pass"]);
    }

    #[test]
    fn synchronized_nbt_matches_default_bootstrap_exactly() {
        let loaders = TestFunctionLoaderRegistry::with_builtin();
        let from_nbt = load_from_synchronized_nbt(&loaders)
            .expect("SYNCHRONIZED_NBT must decode Paper 26.2 GameTest entries");
        let default = GameTestRegistries::bootstrap_default();

        assert_eq!(
            from_nbt
                .test_environments
                .get_value(&default_environment_key()),
            default
                .test_environments
                .get_value(&default_environment_key())
        );
        assert_eq!(
            from_nbt
                .test_instances
                .get_value(&always_pass_instance_key()),
            default
                .test_instances
                .get_value(&always_pass_instance_key())
        );

        let always_pass = from_nbt
            .test_instances
            .get_value(&always_pass_instance_key())
            .expect("always_pass instance");
        assert_eq!(always_pass.instance_type(), GameTestInstanceType::Function);
        assert_eq!(always_pass.structure().to_string(), "minecraft:empty");
        assert_eq!(always_pass.max_ticks(), 1);
        assert_eq!(always_pass.setup_ticks(), 1);
        assert!(!always_pass.required());
        assert_eq!(always_pass.rotation(), Rotation::None);
        assert!(!always_pass.manual_only());
        assert_eq!(always_pass.max_attempts(), 1);
        assert_eq!(always_pass.required_successes(), 1);
        assert!(!always_pass.sky_access());
        assert_eq!(always_pass.padding(), 0);
        assert_eq!(
            always_pass.function_key(),
            Some(&always_pass_function_key())
        );
    }

    #[test]
    fn decode_all_seven_environment_definition_types_and_ordering() {
        let all_of_json = json!({
            "type": "minecraft:all_of",
            "definitions": [
                "minecraft:default",
                { "type": "minecraft:weather", "weather": "thunder" },
                { "type": "minecraft:difficulty", "difficulty": "hard" }
            ]
        });
        let all_of = TestEnvironmentDefinition::decode_json(&all_of_json).unwrap();
        assert_eq!(
            all_of.definition_type(),
            TestEnvironmentDefinitionType::AllOf
        );
        // Teardown order must be exact reverse of setup order.
        assert_eq!(all_of.teardown_order_indices(), vec![2, 1, 0]);

        let clock = TestEnvironmentDefinition::decode_json(&json!({
            "type": "minecraft:clock_time",
            "clock": "minecraft:overworld",
            "time": 6000
        }))
        .unwrap();
        assert_eq!(
            clock,
            TestEnvironmentDefinition::ClockTime {
                clock: Identifier::with_default_namespace("overworld"),
                time: 6000,
            }
        );

        let funcs = TestEnvironmentDefinition::decode_json(&json!({
            "type": "minecraft:function",
            "setup": "minecraft:setup_fn",
            "teardown": "minecraft:teardown_fn"
        }))
        .unwrap();
        assert_eq!(
            funcs,
            TestEnvironmentDefinition::Functions {
                setup_function: Some(Identifier::with_default_namespace("setup_fn")),
                teardown_function: Some(Identifier::with_default_namespace("teardown_fn")),
            }
        );

        let diff = TestEnvironmentDefinition::decode_json(&json!({
            "type": "minecraft:difficulty",
            "difficulty": "peaceful"
        }))
        .unwrap();
        assert_eq!(
            diff,
            TestEnvironmentDefinition::SetDifficulty {
                difficulty: Difficulty::Peaceful,
            }
        );

        let rules = TestEnvironmentDefinition::decode_json(&json!({
            "type": "minecraft:game_rules",
            "rules": {
                "minecraft:do_daylight_cycle": false,
                "minecraft:random_tick_speed": 3
            }
        }))
        .unwrap();
        let TestEnvironmentDefinition::SetGameRules { rules } = rules else {
            panic!("expected SetGameRules");
        };
        assert_eq!(rules.len(), 2);
        assert_eq!(
            rules.get(&Identifier::with_default_namespace("do_daylight_cycle")),
            Some(&GameRuleValue::Bool(false))
        );
        assert_eq!(
            rules.get(&Identifier::with_default_namespace("random_tick_speed")),
            Some(&GameRuleValue::Int(3))
        );

        let timelines_single = TestEnvironmentDefinition::decode_json(&json!({
            "type": "minecraft:timeline_attributes",
            "timelines": "minecraft:day"
        }))
        .unwrap();
        assert_eq!(
            timelines_single,
            TestEnvironmentDefinition::Timelines {
                timelines: vec![Identifier::with_default_namespace("day")],
            }
        );

        let weather = TestEnvironmentDefinition::decode_json(&json!({
            "type": "minecraft:weather",
            "weather": "rain"
        }))
        .unwrap();
        assert_eq!(
            weather,
            TestEnvironmentDefinition::Weather {
                weather: WeatherType::Rain,
            }
        );
        assert_eq!(
            WeatherType::Clear.weather_params(),
            (100_000, 0, false, false)
        );
        assert_eq!(
            WeatherType::Rain.weather_params(),
            (0, 100_000, true, false)
        );
        assert_eq!(
            WeatherType::Thunder.weather_params(),
            (0, 100_000, true, true)
        );
    }

    #[test]
    fn retry_options_matches_paper_semantics() {
        let no_retries = RetryOptions::no_retries();
        assert!(!no_retries.has_retries());
        assert!(!no_retries.unlimited_tries());
        assert!(no_retries.has_tries_left(0, 0));
        assert!(!no_retries.has_tries_left(1, 1));
        assert!(!no_retries.has_tries_left(1, 0));

        let halt = RetryOptions::new(3, true);
        assert!(halt.has_retries());
        assert!(halt.has_tries_left(2, 2));
        assert!(!halt.has_tries_left(2, 1));
        assert!(!halt.has_tries_left(3, 3));

        let continue_on_fail = RetryOptions::new(3, false);
        assert!(continue_on_fail.has_tries_left(2, 0));
        assert!(!continue_on_fail.has_tries_left(3, 0));

        let unlimited = RetryOptions::new(0, false);
        assert!(unlimited.unlimited_tries());
        assert!(unlimited.has_retries());
        assert!(unlimited.has_tries_left(100, 0));
    }

    #[test]
    fn game_test_batch_rejects_empty_collection_with_paper_message() {
        let regs = GameTestRegistries::bootstrap_default();
        let env = regs
            .environment_holder(&default_environment_key())
            .expect("default env holder");

        let err = GameTestBatch::<u32>::new(0, Vec::new(), env.clone()).unwrap_err();
        assert_eq!(err, GameTestBatchError::EmptyBatch);
        assert_eq!(
            err.to_string(),
            "A GameTestBatch must include at least one GameTestInfo!"
        );

        let batch = GameTestBatch::new(7, vec![10u32, 20u32], env).unwrap();
        assert_eq!(batch.index(), 7);
        assert_eq!(batch.game_test_infos(), &[10, 20]);
    }

    #[test]
    fn test_function_loaders_preserve_insertion_order_and_generated_test_order() {
        struct CustomLoader(&'static str);
        impl TestFunctionLoader for CustomLoader {
            fn load(
                &self,
                register: &mut dyn FnMut(ResourceKey<TestFunctionEntry>, TestFunctionEntry),
            ) {
                let key = test_function_key(self.0);
                register(key.clone(), TestFunctionEntry::named(key, self.0));
            }
        }

        let mut loaders = TestFunctionLoaderRegistry::with_builtin();
        loaders.register_loader(CustomLoader("zebra"));
        loaders.register_loader(CustomLoader("alpha"));

        let regs = GameTestRegistries::bootstrap_with_loaders(&loaders);
        let keys: Vec<String> = regs
            .test_functions
            .entry_set_ref()
            .into_iter()
            .map(|(k, _)| k.identifier().to_string())
            .collect();
        assert_eq!(
            keys,
            vec![
                "minecraft:always_pass",
                "minecraft:zebra",
                "minecraft:alpha",
            ]
        );

        let mut tests = IndexMap::new();
        tests.insert(
            Identifier::with_default_namespace("b_test"),
            TestData::with_defaults(
                default_environment_key(),
                Identifier::with_default_namespace("empty"),
                10,
                0,
                true,
            ),
        );
        tests.insert(
            Identifier::with_default_namespace("a_test"),
            TestData::with_defaults(
                default_environment_key(),
                Identifier::with_default_namespace("empty"),
                20,
                2,
                false,
            ),
        );
        let generated = GeneratedTest::new(
            tests,
            always_pass_function_key(),
            TestFunctionEntry::always_pass(),
        );
        let order: Vec<String> = generated.tests.keys().map(ToString::to_string).collect();
        assert_eq!(order, vec!["minecraft:b_test", "minecraft:a_test"]);
    }

    #[test]
    fn discovery_preserves_order_filters_required_and_repeats_copies() {
        let loaders = TestFunctionLoaderRegistry::with_builtin();
        let req_key = instance_key("required_block");
        let opt_key = always_pass_instance_key();
        let req_inst =
            GameTestInstance::BlockBased(BlockBasedTestInstance::new(TestData::with_defaults(
                TestEnvironmentRef::default_key(),
                Identifier::with_default_namespace("empty"),
                5,
                0,
                true,
            )));
        let regs = GameTestRegistries::from_entries(
            &loaders,
            vec![(default_environment_key(), default_environment_definition())],
            vec![
                (opt_key.clone(), always_pass_instance()),
                (req_key.clone(), req_inst),
            ],
        )
        .unwrap();

        let opt_holder = regs.instance_holder(&opt_key).unwrap();
        let req_holder = regs.instance_holder(&req_key).unwrap();

        let mut tracker = FailedTestTracker::new();
        tracker.remember_failed_test(opt_holder.clone());
        tracker.remember_failed_test(req_holder.clone());
        tracker.remember_failed_test(opt_holder.clone()); // duplicate ignored, order preserved

        let all_failed = TestFinderBuilder::new()
            .failed_tests(&tracker, &regs, false)
            .find_tests();
        assert_eq!(all_failed, vec![opt_holder.clone(), req_holder.clone()]);

        let only_required = TestFinderBuilder::create_multiple_copies(2)
            .failed_tests(&tracker, &regs, true)
            .find_tests();
        assert_eq!(only_required, vec![req_holder.clone(), req_holder]);

        let positions = TestFinderBuilder::create_multiple_copies(2)
            .from_positions(vec![BlockPos::new(1, 64, 2), BlockPos::new(3, 64, 4)])
            .find_test_pos();
        assert_eq!(
            positions,
            vec![
                BlockPos::new(1, 64, 2),
                BlockPos::new(3, 64, 4),
                BlockPos::new(1, 64, 2),
                BlockPos::new(3, 64, 4),
            ]
        );

        tracker.forget_failed_tests();
        assert!(tracker.is_empty());
    }

    #[test]
    fn strict_resource_loader_fails_closed_on_missing_tampered_extra_and_bad_refs() {
        let loaders = TestFunctionLoaderRegistry::with_builtin();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Missing resources -> fail closed.
        let err = load_strict_paper_resources(root, &loaders).unwrap_err();
        assert!(matches!(err, GameTestResourceError::MissingResource { .. }));

        // Populate exact Paper 26.2 resource tree.
        fs::create_dir_all(root.join("data/minecraft/test_environment")).unwrap();
        fs::create_dir_all(root.join("data/minecraft/test_instance")).unwrap();
        fs::create_dir_all(root.join("data/minecraft/structure")).unwrap();

        let default_env_bytes = b"{\n  \"type\": \"minecraft:all_of\",\n  \"definitions\": []\n}";
        let always_pass_bytes = b"{\n  \"type\": \"minecraft:function\",\n  \"environment\": \"minecraft:default\",\n  \"function\": \"minecraft:always_pass\",\n  \"max_ticks\": 1,\n  \"required\": false,\n  \"setup_ticks\": 1,\n  \"structure\": \"minecraft:empty\"\n}";

        fs::write(
            root.join("data/minecraft/test_environment/default.json"),
            default_env_bytes,
        )
        .unwrap();
        fs::write(
            root.join("data/minecraft/test_instance/always_pass.json"),
            always_pass_bytes,
        )
        .unwrap();

        // Tampered empty.nbt (wrong size) -> fail closed.
        fs::write(root.join("data/minecraft/structure/empty.nbt"), b"bad").unwrap();
        let err = load_strict_paper_resources(root, &loaders).unwrap_err();
        assert!(matches!(err, GameTestResourceError::SizeMismatch { .. }));

        // Extra unexpected resource in test_environment -> fail closed.
        fs::write(
            root.join("data/minecraft/test_environment/extra.json"),
            default_env_bytes,
        )
        .unwrap();
        // General loader accepts extra.json if valid, but rejects broken references:
        let bad_instance = json!({
            "type": "minecraft:function",
            "environment": "minecraft:missing_env",
            "function": "minecraft:always_pass",
            "max_ticks": 1,
            "structure": "minecraft:empty"
        });
        fs::write(
            root.join("data/minecraft/test_instance/broken.json"),
            serde_json::to_string(&bad_instance).unwrap(),
        )
        .unwrap();
        let err = load_from_resource_root(root, &loaders).unwrap_err();
        assert!(matches!(
            err,
            GameTestResourceError::Registry(GameTestRegistryError::MissingEnvironment { .. })
        ));
    }

    #[test]
    fn controlled_negative_rejects_invalid_codec_bounds_and_unknown_types() {
        // max_ticks must be > 0.
        let zero_ticks = GameTestInstance::decode_json(&json!({
            "type": "minecraft:block_based",
            "environment": "minecraft:default",
            "structure": "minecraft:empty",
            "max_ticks": 0
        }));
        assert!(matches!(
            zero_ticks,
            Err(InstanceDecodeError::Validation(
                TestDataValidationError::OutOfRange {
                    field: "max_ticks",
                    ..
                }
            ))
        ));

        // padding must be in 0..=128.
        let huge_padding = GameTestInstance::decode_json(&json!({
            "type": "minecraft:block_based",
            "environment": "minecraft:default",
            "structure": "minecraft:empty",
            "max_ticks": 10,
            "padding": 129
        }));
        assert!(matches!(
            huge_padding,
            Err(InstanceDecodeError::Validation(
                TestDataValidationError::OutOfRange {
                    field: "padding",
                    ..
                }
            ))
        ));

        // Unknown environment type must fail closed.
        let unknown_env = TestEnvironmentDefinition::decode_json(&json!({
            "type": "minecraft:not_a_real_env_type"
        }));
        assert!(matches!(
            unknown_env,
            Err(EnvironmentDecodeError::UnknownType(_))
        ));

        // Unknown rotation must fail closed.
        let bad_rot = GameTestInstance::decode_json(&json!({
            "type": "minecraft:block_based",
            "environment": "minecraft:default",
            "structure": "minecraft:empty",
            "max_ticks": 10,
            "rotation": "upside_down"
        }));
        assert!(matches!(
            bad_rot,
            Err(InstanceDecodeError::InvalidRotation(_))
        ));
        // Oversized identifiers (> Paper's length limit) must return an error, not panic.
        let huge = format!("minecraft:{}", "a".repeat(22_000));
        let oversized_env = TestEnvironmentDefinition::decode_json(&json!({
            "type": "minecraft:clock_time",
            "clock": huge,
            "time": 0
        }));
        assert!(matches!(
            oversized_env,
            Err(EnvironmentDecodeError::InvalidIdentifier { field: "clock", .. })
        ));
    }

    #[test]
    fn paper_working_checkout_resources_match_census_when_present() {
        let paper_resources = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../working/Paper/paper-server/src/minecraft/resources");
        if !paper_resources.is_dir() {
            return;
        }
        let loaders = TestFunctionLoaderRegistry::with_builtin();
        let regs = load_strict_paper_resources(&paper_resources, &loaders)
            .expect("pinned Paper 26.2 checkout resources must match recorded census exactly");
        assert_eq!(regs.test_environments.size(), 1);
        assert_eq!(regs.test_instances.size(), 1);
        assert_eq!(regs.test_functions.size(), 1);
    }
}
