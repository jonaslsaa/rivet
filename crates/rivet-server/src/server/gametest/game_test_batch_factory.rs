//! `net.minecraft.gametest.framework.GameTestBatchFactory` (Paper 26.2).
//!
//! Mirrors `GameTestBatchFactory.java`:
//! - `DIRECT` test decorator (`Rotation::None`, `RetryOptions::no_retries()`)
//! - `from_test_info` / `divide_into_batches` grouping by environment and
//!   partitioning into batches of at most `MAX_TESTS_PER_BATCH` (50) while
//!   preserving deterministic source order.

use indexmap::IndexMap;
use rivet_registry::core::Rotation;
use rivet_registry::{Holder, HolderLookup, Identifier};

use crate::server::gametest::game_test_batch::GameTestBatch;
use crate::server::gametest::game_test_info::GameTestInfo;
use crate::server::gametest::game_test_instance::GameTestInstance;
use crate::server::gametest::registries::GameTestRegistries;
use crate::server::gametest::retry_options::RetryOptions;
use crate::server::gametest::test_environment_definition::TestEnvironmentDefinition;

/// Paper: `GameTestBatchFactory.MAX_TESTS_PER_BATCH` (50).
pub const MAX_TESTS_PER_BATCH: usize = 50;

/// Paper: `GameTestBatchFactory.DIRECT`.
///
/// Wraps a test instance holder into a single `GameTestInfo` with
/// `Rotation::None` and `RetryOptions::no_retries()`.
pub fn direct_test_info(
    id: Identifier,
    instance: Holder<GameTestInstance>,
    registries: &GameTestRegistries,
) -> GameTestInfo {
    GameTestInfo::new(
        id,
        instance,
        registries,
        Rotation::None,
        RetryOptions::no_retries(),
    )
}

/// Group items by their environment key in deterministic first-seen order and
/// partition each group into batches of at most `max_tests_per_batch`
/// (matching Paper `GameTestBatchFactory.fromGameTestInfo(int maxTestsPerBatch)`).
///
/// If `max_tests_per_batch == 0`, clamps to `1` to prevent empty/infinite chunks.
pub fn divide_into_batches<T, F>(
    items: impl IntoIterator<Item = T>,
    max_tests_per_batch: usize,
    env_lookup: &dyn HolderLookup<TestEnvironmentDefinition>,
    mut environment_of: F,
) -> Vec<GameTestBatch<T>>
where
    F: FnMut(&T) -> Holder<TestEnvironmentDefinition>,
{
    let chunk_size = max_tests_per_batch.max(1);
    let mut grouped: IndexMap<String, (Holder<TestEnvironmentDefinition>, Vec<T>)> =
        IndexMap::new();

    for item in items {
        let env = environment_of(&item);
        let key = match &env {
            Holder::Reference { .. } => env.key(env_lookup).identifier().to_string(),
            Holder::Direct(def) => format!("direct:{def:?}"),
        };
        grouped
            .entry(key)
            .or_insert_with(|| (env, Vec::new()))
            .1
            .push(item);
    }

    let mut batches = Vec::new();
    let mut batch_index: i32 = 0;

    for (_, (env, mut group_items)) in grouped {
        while !group_items.is_empty() {
            let take = group_items.len().min(chunk_size);
            let chunk: Vec<T> = group_items.drain(..take).collect();
            let batch = GameTestBatch::new(batch_index, chunk, env.clone())
                .expect("chunk is guaranteed non-empty");
            batches.push(batch);
            batch_index = batch_index.wrapping_add(1);
        }
    }

    batches
}

/// Convenience wrapper matching Paper's `GameTestBatchFactory.fromGameTestInfo()`
/// with the default `MAX_TESTS_PER_BATCH` (50).
pub fn divide_into_default_batches<T, F>(
    items: impl IntoIterator<Item = T>,
    env_lookup: &dyn HolderLookup<TestEnvironmentDefinition>,
    environment_of: F,
) -> Vec<GameTestBatch<T>>
where
    F: FnMut(&T) -> Holder<TestEnvironmentDefinition>,
{
    divide_into_batches(items, MAX_TESTS_PER_BATCH, env_lookup, environment_of)
}
