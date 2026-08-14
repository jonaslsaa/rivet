//! STUB(mc.util.datafix) — `net.minecraft.util.datafix.DataFixTypes`, the
//! enum of DFU `TypeReference` handles the saved-data storage indexes by
//! (`SAVED_DATA_WEATHER`, `SAVED_DATA_WANDERING_TRADER`, ...).
//!
//! The real port is owned by the pending `mc.util.datafix` unit
//! (rivet-util). This unit declares the variant surface its consumers need —
//! the payloads `WanderingTraderData`/`WeatherData` use
//! (`SAVED_DATA_WANDERING_TRADER` / `SAVED_DATA_WEATHER`),
//! `SAVED_DATA_WORLD_GEN_SETTINGS` (the `WorldGenSettings.TYPE` handle,
//! `mc.world.level.levelgen.settings`), `SAVED_DATA_WORLD_BORDER` (the
//! `mc.world.level.border` `WorldBorder` saved-data, #612), and
//! `SAVED_DATA_GAME_RULES` (the `GameRuleMap.TYPE` handle,
//! `mc.world.level.gamerules`, #613) — plus the `NONE` Paper no-op. This is not
//! the full Java enum (many other variants remain absent, e.g. `LEVEL`,
//! `PLAYER`, `CHUNK`); the stub is replaced wholesale when `mc.util.datafix`
//! lands.

/// `net.minecraft.util.datafix.DataFixTypes` — value-identity only.
///
/// The full enum carries `TypeReference type` and the `wrapCodec`/`update`
/// DFU machinery; this stub keeps the variants needed by the saveddata value
/// unit and is replaced when `mc.util.datafix` lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataFixTypes {
    /// Paper's no-op fixer for custom types that need no DFU upgrade.
    None,
    /// `References.SAVED_DATA_WANDERING_TRADER`.
    SavedDataWanderingTrader,
    /// `References.SAVED_DATA_WEATHER`.
    SavedDataWeather,
    /// `References.SAVED_DATA_WORLD_GEN_SETTINGS` — consumed by the
    /// `WorldGenSettings.TYPE` handle (`mc.world.level.levelgen.settings`).
    SavedDataWorldGenSettings,
    /// `References.SAVED_DATA_WORLD_BORDER` — the `WorldBorder` saved-data
    /// (`mc.world.level.border`, #612).
    SavedDataWorldBorder,
    /// `References.SAVED_DATA_GAME_RULES` — the `GameRuleMap` saved-data
    /// (`mc.world.level.gamerules`, #613).
    SavedDataGameRules,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_border_variant_is_scoped_and_distinct() {
        // `SAVED_DATA_WORLD_BORDER` is a distinct value-identity variant the
        // #612 `WorldBorder` saved-data will consume.
        assert_ne!(
            DataFixTypes::SavedDataWorldBorder,
            DataFixTypes::SavedDataWorldGenSettings
        );
        assert_ne!(
            DataFixTypes::SavedDataWorldBorder,
            DataFixTypes::SavedDataWeather
        );
    }

    #[test]
    fn game_rules_variant_is_scoped_and_distinct() {
        // `SAVED_DATA_GAME_RULES` is a distinct value-identity variant the
        // `GameRuleMap.TYPE` handle (#613) consumes.
        assert_ne!(
            DataFixTypes::SavedDataGameRules,
            DataFixTypes::SavedDataWorldBorder
        );
        assert_ne!(
            DataFixTypes::SavedDataGameRules,
            DataFixTypes::SavedDataWeather
        );
        assert_ne!(
            DataFixTypes::SavedDataGameRules,
            DataFixTypes::SavedDataWorldGenSettings
        );
    }
}
