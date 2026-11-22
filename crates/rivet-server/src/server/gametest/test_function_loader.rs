//! `net.minecraft.gametest.framework.BuiltinTestFunctions`,
//! `TestFunctionLoader`, and `GeneratedTest` (Paper 26.2).
//!
//! Runtime `Consumer<GameTestHelper>` execution is deferred to issues
//! #430/#475; this module ports the keys, loader ordering, registration
//! metadata, and `GeneratedTest` value record.

use std::sync::Arc;

use indexmap::IndexMap;
use rivet_registry::{Identifier, RegistrationInfo, RegistryBuilder, ResourceKey};

use crate::server::gametest::registries::TEST_FUNCTION;
use crate::server::gametest::test_data::TestData;
use crate::server::gametest::test_environment_definition::TestEnvironmentDefinition;

/// `BuiltinTestFunctions.ALWAYS_PASS` path (`"always_pass"`).
pub const ALWAYS_PASS_FUNCTION_NAME: &str = "always_pass";

/// `BuiltinTestFunctions.ALWAYS_PASS` resource key (`minecraft:test_function` / `minecraft:always_pass`).
pub fn always_pass_function_key() -> ResourceKey<TestFunctionEntry> {
    ResourceKey::create(
        &TEST_FUNCTION,
        Identifier::with_default_namespace(ALWAYS_PASS_FUNCTION_NAME),
    )
}

/// `BuiltinTestFunctions.create(String)` — key in `minecraft:test_function`.
pub fn test_function_key(name: &str) -> ResourceKey<TestFunctionEntry> {
    ResourceKey::create(&TEST_FUNCTION, Identifier::with_default_namespace(name))
}

/// Registered entry in `minecraft:test_function`.
///
/// Carries the function's identity and kind; executable `GameTestHelper`
/// bodies are attached in issue #475.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TestFunctionEntry {
    pub key: ResourceKey<TestFunctionEntry>,
    pub kind: TestFunctionKind,
}

/// Distinguishes built-in test functions from custom/generated registrations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TestFunctionKind {
    /// `BuiltinTestFunctions.ALWAYS_PASS_INSTANCE` (`GameTestHelper::succeed`).
    AlwaysPass,
    /// Named custom/generated test function registered by a `TestFunctionLoader`.
    Named(String),
}

impl TestFunctionEntry {
    /// Canonical `BuiltinTestFunctions.ALWAYS_PASS` entry.
    pub fn always_pass() -> Self {
        Self {
            key: always_pass_function_key(),
            kind: TestFunctionKind::AlwaysPass,
        }
    }

    /// Named test function entry.
    pub fn named(key: ResourceKey<TestFunctionEntry>, label: impl Into<String>) -> Self {
        Self {
            key,
            kind: TestFunctionKind::Named(label.into()),
        }
    }
}

/// `net.minecraft.gametest.framework.TestFunctionLoader` — loads test
/// functions into the `test_function` registry in deterministic order.
pub trait TestFunctionLoader {
    fn load(&self, register: &mut dyn FnMut(ResourceKey<TestFunctionEntry>, TestFunctionEntry));
}

/// Built-in loader mirroring `BuiltinTestFunctions extends TestFunctionLoader`.
#[derive(Debug, Default, Clone, Copy)]
pub struct BuiltinTestFunctions;

impl TestFunctionLoader for BuiltinTestFunctions {
    fn load(&self, register: &mut dyn FnMut(ResourceKey<TestFunctionEntry>, TestFunctionEntry)) {
        register(always_pass_function_key(), TestFunctionEntry::always_pass());
    }
}

/// Ordered collection of [`TestFunctionLoader`]s matching Paper's
/// `TestFunctionLoader.loaders` (`List<TestFunctionLoader>`) without relying
/// on mutable process-global state.
#[derive(Default)]
pub struct TestFunctionLoaderRegistry {
    loaders: Vec<Box<dyn TestFunctionLoader>>,
}

impl TestFunctionLoaderRegistry {
    /// Empty loader list.
    pub fn new() -> Self {
        Self {
            loaders: Vec::new(),
        }
    }

    /// Standard Paper loader list seeded with [`BuiltinTestFunctions`].
    pub fn with_builtin() -> Self {
        let mut registry = Self::new();
        registry.register_loader(BuiltinTestFunctions);
        registry
    }

    /// `TestFunctionLoader.registerLoader(TestFunctionLoader)` — appends in
    /// insertion order.
    pub fn register_loader(&mut self, loader: impl TestFunctionLoader + 'static) {
        self.loaders.push(Box::new(loader));
    }

    /// Number of registered loaders.
    pub fn len(&self) -> usize {
        self.loaders.len()
    }

    /// Whether no loaders are registered.
    pub fn is_empty(&self) -> bool {
        self.loaders.is_empty()
    }

    /// `TestFunctionLoader.runLoaders(Registry)` — runs each loader in
    /// insertion order and registers each emitted function into `builder`.
    pub fn run_loaders(&self, builder: &mut RegistryBuilder<TestFunctionEntry>) {
        for loader in &self.loaders {
            loader.load(&mut |key, entry| {
                builder.register(&key, Arc::new(entry), RegistrationInfo::BUILT_IN);
            });
        }
    }
}

/// `net.minecraft.gametest.framework.GeneratedTest` (Paper 26.2).
///
/// Uses `IndexMap` so multi-test iteration order is deterministic and
/// insertion-ordered (PORTING.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTest {
    pub tests: IndexMap<Identifier, TestData<ResourceKey<TestEnvironmentDefinition>>>,
    pub function_key: ResourceKey<TestFunctionEntry>,
    pub function: TestFunctionEntry,
}

impl GeneratedTest {
    /// Multi-test constructor matching
    /// `GeneratedTest(Map<Identifier, TestData<ResourceKey<TestEnvironmentDefinition<?>>>>, ResourceKey, Consumer)`.
    pub fn new(
        tests: IndexMap<Identifier, TestData<ResourceKey<TestEnvironmentDefinition>>>,
        function_key: ResourceKey<TestFunctionEntry>,
        function: TestFunctionEntry,
    ) -> Self {
        Self {
            tests,
            function_key,
            function,
        }
    }

    /// Single-test overload matching
    /// `GeneratedTest(Identifier, TestData<ResourceKey<TestEnvironmentDefinition<?>>>, Consumer)`.
    pub fn single(
        id: Identifier,
        test_data: TestData<ResourceKey<TestEnvironmentDefinition>>,
        function: TestFunctionEntry,
    ) -> Self {
        let function_key = ResourceKey::create(&TEST_FUNCTION, id.clone());
        let mut tests = IndexMap::with_capacity(1);
        tests.insert(id, test_data);
        Self {
            tests,
            function_key,
            function,
        }
    }
}
