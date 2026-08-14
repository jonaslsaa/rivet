//! `net.minecraft.world.level.saveddata` — the per-world persisted-data value
//! layer (the `mc.world.level.saveddata` unit).
//!
//! The abstract [`SavedData`] base (the `dirty` flag), the [`SavedDataType`]
//! registry-entry record binding a blob's `Identifier`/constructor/`CODEC`/
//! `DataFixTypes`, and the two concrete 26.2 payloads: [`WanderingTraderData`]
//! (`data/wandering_trader.dat`) and [`WeatherData`] (`data/weather.dat`).
//! Paper's 26.2 `package-info.java` is `@NullMarked`; the port is all-owned
//! `Option`/non-null by construction.
//!
//! The load/save/disk lifecycle belongs to the `ServerLevel` storage runtime
//! (`level/storage`, `mc.server.level`); this unit carries the value types,
//! codecs, and `SavedDataType` handles the runtime will call.
//!
//! `DataFixTypes` is the `mc.util.datafix` unit's enum; this unit declares the
//! variants its consumers reference (`SAVED_DATA_WEATHER`,
//! `SAVED_DATA_WANDERING_TRADER`, `SAVED_DATA_WORLD_GEN_SETTINGS` for the
//! `mc.world.level.levelgen.settings` unit's `WorldGenSettings.TYPE`,
//! `SAVED_DATA_WORLD_BORDER` for the #612 `WorldBorder` saved-data, and
//! `SAVED_DATA_GAME_RULES` for the `mc.world.level.gamerules` unit's
//! `GameRuleMap.TYPE`, #613) plus the reserved `NONE` Paper no-op as a
//! value-identity stub (see [`stub_data_fix_types`]). This is not the full
//! Java enum.

pub mod saved_data;
pub mod saved_data_type;
pub mod stub_data_fix_types;
pub mod wandering_trader_data;
pub mod weather_data;

pub use saved_data::SavedData;
pub use saved_data_type::SavedDataType;
pub use wandering_trader_data::WanderingTraderData;
pub use weather_data::WeatherData;
