//! GameTest registry keys, frozen registries, and Paper 26.2 bootstrap
//! (`Registries.java`, `BuiltInRegistries.java`, `GameTestEnvironments`,
//! `GameTestInstances`, `BuiltinTestFunctions`).
//!
//! Uses `rivet-registry`'s `RegistryBuilder<T>` → `freeze()` → frozen
//! `Registry<T>` + `Holder<T>` architecture (OWNERSHIP.md §Registries).

use std::sync::{Arc, LazyLock};

use rivet_registry::{
    Holder, HolderGetter, Identifier, RegistrationInfo, Registry, RegistryBuilder, ResourceKey,
};

use crate::server::gametest::game_test_instance::{
    GameTestInstance, GameTestInstanceType, always_pass_instance, always_pass_instance_key,
};
use crate::server::gametest::test_environment_definition::{
    TestEnvironmentDefinition, TestEnvironmentDefinitionType, TestEnvironmentRef,
    default_environment_definition, default_environment_key,
};
use crate::server::gametest::test_function_loader::{
    TestFunctionEntry, TestFunctionLoaderRegistry,
};

/// `Registries.TEST_ENVIRONMENT_DEFINITION_TYPE` (`minecraft:test_environment_definition_type`).
pub static TEST_ENVIRONMENT_DEFINITION_TYPE: LazyLock<
    ResourceKey<Registry<TestEnvironmentDefinitionType>>,
> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace(
        "test_environment_definition_type",
    ))
});

/// `Registries.TEST_FUNCTION` (`minecraft:test_function`).
pub static TEST_FUNCTION: LazyLock<ResourceKey<Registry<TestFunctionEntry>>> =
    LazyLock::new(|| {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test_function"))
    });

/// `Registries.TEST_INSTANCE_TYPE` (`minecraft:test_instance_type`).
pub static TEST_INSTANCE_TYPE: LazyLock<ResourceKey<Registry<GameTestInstanceType>>> =
    LazyLock::new(|| {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test_instance_type"))
    });

/// `Registries.TEST_ENVIRONMENT` (`minecraft:test_environment`).
pub static TEST_ENVIRONMENT: LazyLock<ResourceKey<Registry<TestEnvironmentDefinition>>> =
    LazyLock::new(|| {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test_environment"))
    });

/// `Registries.TEST_INSTANCE` (`minecraft:test_instance`).
pub static TEST_INSTANCE: LazyLock<ResourceKey<Registry<GameTestInstance>>> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace("test_instance"))
});

/// The 5 GameTest registry identifiers in canonical order.
pub const GAMETEST_REGISTRY_NAMES: [&str; 5] = [
    "minecraft:test_environment_definition_type",
    "minecraft:test_instance_type",
    "minecraft:test_function",
    "minecraft:test_environment",
    "minecraft:test_instance",
];

/// Frozen GameTest registry bundle (OWNERSHIP.md §Registries — immutable once
/// bootstrapped, no `Arc<RwLock>`).
#[derive(Debug)]
pub struct GameTestRegistries {
    pub environment_definition_types: Registry<TestEnvironmentDefinitionType>,
    pub instance_types: Registry<GameTestInstanceType>,
    pub test_functions: Registry<TestFunctionEntry>,
    pub test_environments: Registry<TestEnvironmentDefinition>,
    pub test_instances: Registry<GameTestInstance>,
}

/// Reference validation error across the frozen GameTest registries.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GameTestRegistryError {
    #[error("test_instance `{instance}` references unknown test_environment `{environment}`")]
    MissingEnvironment {
        instance: String,
        environment: String,
    },
    #[error("test_environment `{owner}` references unknown nested test_environment `{referenced}`")]
    MissingNestedEnvironment { owner: String, referenced: String },
    #[error("Trying to access missing test function: {function} (referenced by `{instance}`)")]
    MissingTestFunction { instance: String, function: String },
}

impl GameTestRegistries {
    /// Bootstrap the 3 built-in registries (`test_environment_definition_type`,
    /// `test_instance_type`, `test_function`) plus the canonical Paper 26.2
    /// built-in data registries (`test_environment:default`,
    /// `test_instance:always_pass`).
    pub fn bootstrap_default() -> Self {
        Self::bootstrap_with_loaders(&TestFunctionLoaderRegistry::with_builtin())
    }

    /// Bootstrap with a custom [`TestFunctionLoaderRegistry`].
    pub fn bootstrap_with_loaders(loaders: &TestFunctionLoaderRegistry) -> Self {
        let environment_definition_types = bootstrap_environment_definition_types();
        let instance_types = bootstrap_instance_types();
        let test_functions = bootstrap_test_functions(loaders);

        let mut env_builder = RegistryBuilder::new(&TEST_ENVIRONMENT);
        env_builder.register(
            &default_environment_key(),
            Arc::new(default_environment_definition()),
            RegistrationInfo::BUILT_IN,
        );
        let test_environments = env_builder.freeze();

        let mut inst_builder = RegistryBuilder::new(&TEST_INSTANCE);
        inst_builder.register(
            &always_pass_instance_key(),
            Arc::new(always_pass_instance()),
            RegistrationInfo::BUILT_IN,
        );
        let test_instances = inst_builder.freeze();

        Self {
            environment_definition_types,
            instance_types,
            test_functions,
            test_environments,
            test_instances,
        }
    }

    /// Build a [`GameTestRegistries`] from explicit decoded environments and
    /// instances (used by the fail-closed resource loader), validating all
    /// cross-references before returning.
    pub fn from_entries(
        loaders: &TestFunctionLoaderRegistry,
        environments: Vec<(
            ResourceKey<TestEnvironmentDefinition>,
            TestEnvironmentDefinition,
        )>,
        instances: Vec<(ResourceKey<GameTestInstance>, GameTestInstance)>,
    ) -> Result<Self, GameTestRegistryError> {
        let environment_definition_types = bootstrap_environment_definition_types();
        let instance_types = bootstrap_instance_types();
        let test_functions = bootstrap_test_functions(loaders);

        let mut env_builder = RegistryBuilder::new(&TEST_ENVIRONMENT);
        for (key, def) in environments {
            env_builder.register(&key, Arc::new(def), RegistrationInfo::BUILT_IN);
        }
        let test_environments = env_builder.freeze();

        let mut inst_builder = RegistryBuilder::new(&TEST_INSTANCE);
        for (key, inst) in instances {
            inst_builder.register(&key, Arc::new(inst), RegistrationInfo::BUILT_IN);
        }
        let test_instances = inst_builder.freeze();

        let bundle = Self {
            environment_definition_types,
            instance_types,
            test_functions,
            test_environments,
            test_instances,
        };
        bundle.validate_references()?;
        Ok(bundle)
    }

    /// Validate that every `test_instance` references a registered
    /// `test_environment` (including nested `AllOf` refs) and, for
    /// `FunctionGameTestInstance`, a registered `test_function`.
    pub fn validate_references(&self) -> Result<(), GameTestRegistryError> {
        for (env_key, env_def) in self.test_environments.entry_set_ref() {
            self.validate_env_nested(&env_key.identifier().to_string(), env_def)?;
        }

        for (inst_key, inst) in self.test_instances.entry_set_ref() {
            let inst_name = inst_key.identifier().to_string();
            self.validate_env_ref_for_instance(&inst_name, inst.batch())?;

            if let Some(func_key) = inst.function_key()
                && self.test_functions.get_value(func_key).is_none()
            {
                return Err(GameTestRegistryError::MissingTestFunction {
                    instance: inst_name,
                    function: func_key.identifier().to_string(),
                });
            }
        }
        Ok(())
    }

    fn validate_env_nested(
        &self,
        owner: &str,
        def: &TestEnvironmentDefinition,
    ) -> Result<(), GameTestRegistryError> {
        if let TestEnvironmentDefinition::AllOf { definitions } = def {
            for r in definitions {
                match r {
                    TestEnvironmentRef::Key(key) => {
                        if self.test_environments.get_value(key).is_none() {
                            return Err(GameTestRegistryError::MissingNestedEnvironment {
                                owner: owner.to_string(),
                                referenced: key.identifier().to_string(),
                            });
                        }
                    }
                    TestEnvironmentRef::Inline(inner) => {
                        self.validate_env_nested(owner, inner)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_env_ref_for_instance(
        &self,
        instance: &str,
        env_ref: &TestEnvironmentRef,
    ) -> Result<(), GameTestRegistryError> {
        match env_ref {
            TestEnvironmentRef::Key(key) => {
                if self.test_environments.get_value(key).is_none() {
                    return Err(GameTestRegistryError::MissingEnvironment {
                        instance: instance.to_string(),
                        environment: key.identifier().to_string(),
                    });
                }
                Ok(())
            }
            TestEnvironmentRef::Inline(inner) => self.validate_env_nested(instance, inner),
        }
    }

    /// Resolve a [`Holder<GameTestInstance>`] against `self.test_instances`.
    pub fn resolve_instance<'a>(
        &'a self,
        holder: &'a Holder<GameTestInstance>,
    ) -> Option<&'a GameTestInstance> {
        match holder {
            Holder::Direct(inst) => Some(inst),
            Holder::Reference { registry, id } => {
                if *registry == self.test_instances.registry_id() {
                    self.test_instances.by_id(*id as i32)
                } else {
                    None
                }
            }
        }
    }

    /// Resolve a [`Holder<TestEnvironmentDefinition>`] against
    /// `self.test_environments`.
    pub fn resolve_environment<'a>(
        &'a self,
        holder: &'a Holder<TestEnvironmentDefinition>,
    ) -> Option<&'a TestEnvironmentDefinition> {
        match holder {
            Holder::Direct(env) => Some(env),
            Holder::Reference { registry, id } => {
                if *registry == self.test_environments.registry_id() {
                    self.test_environments.by_id(*id as i32)
                } else {
                    None
                }
            }
        }
    }

    /// Convert a [`TestEnvironmentRef`] into a [`Holder<TestEnvironmentDefinition>`]
    /// bound to `self.test_environments` (or `Holder::Direct` for inline defs).
    pub fn holder_for_environment_ref(
        &self,
        env_ref: &TestEnvironmentRef,
    ) -> Option<Holder<TestEnvironmentDefinition>> {
        match env_ref {
            TestEnvironmentRef::Key(key) => self.test_environments.get(key),
            TestEnvironmentRef::Inline(def) => Some(Holder::Direct((**def).clone())),
        }
    }

    /// Return a `Holder::Reference` for a registered `test_instance` key.
    pub fn instance_holder(
        &self,
        key: &ResourceKey<GameTestInstance>,
    ) -> Option<Holder<GameTestInstance>> {
        self.test_instances.get(key)
    }

    /// Return a `Holder::Reference` for a registered `test_environment` key.
    pub fn environment_holder(
        &self,
        key: &ResourceKey<TestEnvironmentDefinition>,
    ) -> Option<Holder<TestEnvironmentDefinition>> {
        self.test_environments.get(key)
    }

    /// All registered `test_instance` holders in deterministic insertion order.
    pub fn all_instance_holders(&self) -> Vec<Holder<GameTestInstance>> {
        let reg_id = self.test_instances.registry_id();
        (0..self.test_instances.size() as u32)
            .map(|id| Holder::Reference {
                registry: reg_id,
                id,
            })
            .collect()
    }
}

/// `TestEnvironmentDefinition.bootstrap(Registry)` — registers all 7 built-in
/// types in exact Paper order.
pub fn bootstrap_environment_definition_types() -> Registry<TestEnvironmentDefinitionType> {
    let mut builder = RegistryBuilder::new(&TEST_ENVIRONMENT_DEFINITION_TYPE);
    for def_type in TestEnvironmentDefinitionType::BOOTSTRAP_ORDER {
        builder.register(
            &def_type.resource_key(),
            Arc::new(def_type),
            RegistrationInfo::BUILT_IN,
        );
    }
    builder.freeze()
}

/// `GameTestInstance.bootstrap(Registry)` — registers `block_based` and
/// `function` in exact Paper order.
pub fn bootstrap_instance_types() -> Registry<GameTestInstanceType> {
    let mut builder = RegistryBuilder::new(&TEST_INSTANCE_TYPE);
    for inst_type in GameTestInstanceType::BOOTSTRAP_ORDER {
        builder.register(
            &inst_type.resource_key(),
            Arc::new(inst_type),
            RegistrationInfo::BUILT_IN,
        );
    }
    builder.freeze()
}

/// `BuiltinTestFunctions.bootstrap(Registry)` / `TestFunctionLoader.runLoaders(Registry)`.
pub fn bootstrap_test_functions(
    loaders: &TestFunctionLoaderRegistry,
) -> Registry<TestFunctionEntry> {
    let mut builder = RegistryBuilder::new(&TEST_FUNCTION);
    loaders.run_loaders(&mut builder);
    builder.freeze()
}
