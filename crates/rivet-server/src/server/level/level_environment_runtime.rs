//! Live environmental queries over canonical chunks and named clock state.
//! Weather strengths are supplied by the actual weather owner, never inferred
//! from game time or a default clear-weather flag.
use super::ServerLevel;
use crate::server::level::world_clocks::WorldClocks;
use rivet_registry::core::{BlockPos, ChunkPos};
use rivet_world::biome::gameplay_environment::{
    EnvironmentWeather, GameplayBoolean, GameplayEnvironment, GameplayEnvironmentError,
};

impl ServerLevel {
    pub fn sample_gameplay_environment(
        &self,
        environment: &GameplayEnvironment,
        clocks: &WorldClocks,
        attribute: GameplayBoolean,
        pos: BlockPos,
        weather: EnvironmentWeather,
    ) -> Result<bool, GameplayEnvironmentError> {
        environment.sample_at(
            attribute,
            || {
                let chunk_pos = ChunkPos::containing(&pos);
                let chunk = self
                    .chunk_map
                    .get_chunk(chunk_pos)
                    .ok_or(GameplayEnvironmentError::MissingBiomeChunk(chunk_pos))?;
                let biome = chunk.generation_base().get_noise_biome(
                    pos.get_x() >> 2,
                    pos.get_y() >> 2,
                    pos.get_z() >> 2,
                );
                Ok(rivet_registry::biome_id::BiomeId(biome.0))
            },
            |key| {
                clocks
                    .state(key)
                    .map(|state| state.total_ticks)
                    .map_err(|_| GameplayEnvironmentError::MissingClock(key.clone()))
            },
            weather,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::level::{ServerLevelConfig, world_clocks::ClockState};
    use rivet_registry::Identifier;

    #[test]
    fn live_environment_uses_named_clock_not_game_time_or_missing_chunk_defaults() {
        let mut level = ServerLevel::new_region_backed(ServerLevelConfig::default());
        let overworld = Identifier::parse("minecraft:overworld");
        let environment =
            GameplayEnvironment::from_pinned_registries(&overworld, &overworld).unwrap();
        let mut clocks = WorldClocks::new([overworld.clone()]).unwrap();
        let pos = BlockPos::new(100_000, 64, -100_000);
        let weather = EnvironmentWeather {
            rain: 0.,
            thunder: 0.,
        };
        level.set_game_time(13_000);
        assert!(
            !level
                .sample_gameplay_environment(
                    &environment,
                    &clocks,
                    GameplayBoolean::BeesStayInHive,
                    pos,
                    weather
                )
                .unwrap()
        );
        clocks
            .load_state(
                &overworld,
                ClockState {
                    total_ticks: 13_000,
                    ..ClockState::default()
                },
            )
            .unwrap();
        level.set_game_time(0);
        assert!(
            level
                .sample_gameplay_environment(
                    &environment,
                    &clocks,
                    GameplayBoolean::BeesStayInHive,
                    pos,
                    weather
                )
                .unwrap()
        );
        assert!(
            level.chunk_map.is_empty(),
            "non-positional source layers must not load a chunk"
        );
        let absent = WorldClocks::new([]).unwrap();
        assert!(matches!(
            level.sample_gameplay_environment(
                &environment,
                &absent,
                GameplayBoolean::CreakingActive,
                pos,
                weather
            ),
            Err(GameplayEnvironmentError::MissingClock(_))
        ));
    }
}
