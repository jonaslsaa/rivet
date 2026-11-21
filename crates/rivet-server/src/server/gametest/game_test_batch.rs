//! `net.minecraft.gametest.framework.GameTestBatch` (Paper 26.2).
//!
//! Pure value type holding a batch index, non-empty ordered collection of test
//! items, and the batch's environment holder. Execution of `GameTestInfo` is
//! deferred to issue #430, so `GameTestBatch<T>` is parameterized over the
//! test-info payload type `T`.

use rivet_registry::Holder;

use crate::server::gametest::test_environment_definition::TestEnvironmentDefinition;

/// Error returned when constructing an invalid [`GameTestBatch`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GameTestBatchError {
    /// Exact Paper `IllegalArgumentException` message from `GameTestBatch.java`.
    #[error("A GameTestBatch must include at least one GameTestInfo!")]
    EmptyBatch,
}

/// `GameTestBatch(int index, Collection<GameTestInfo> gameTestInfos, Holder<TestEnvironmentDefinition<?>> environment)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameTestBatch<T> {
    index: i32,
    game_test_infos: Vec<T>,
    environment: Holder<TestEnvironmentDefinition>,
}

impl<T> GameTestBatch<T> {
    /// Construct a batch, failing closed with [`GameTestBatchError::EmptyBatch`]
    /// if `game_test_infos` is empty (matching Paper's compact constructor).
    pub fn new(
        index: i32,
        game_test_infos: Vec<T>,
        environment: Holder<TestEnvironmentDefinition>,
    ) -> Result<Self, GameTestBatchError> {
        if game_test_infos.is_empty() {
            return Err(GameTestBatchError::EmptyBatch);
        }
        Ok(Self {
            index,
            game_test_infos,
            environment,
        })
    }

    pub const fn index(&self) -> i32 {
        self.index
    }

    pub fn game_test_infos(&self) -> &[T] {
        &self.game_test_infos
    }

    pub const fn environment(&self) -> &Holder<TestEnvironmentDefinition> {
        &self.environment
    }
}
