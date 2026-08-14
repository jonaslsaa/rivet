//! STUB(mc.util.datafix) — `net.minecraft.util.datafix.DataFixTypes`, the
//! enum of DFU `TypeReference` handles the saved-data storage indexes by
//! (`SAVED_DATA_WEATHER`, `SAVED_DATA_WANDERING_TRADER`, ...).
//!
//! The real port is owned by the pending `mc.util.datafix` unit
//! (rivet-util). This unit's payloads reference the two variants
//! `WanderingTraderData`/`WeatherData` use (`SAVED_DATA_WANDERING_TRADER` /
//! `SAVED_DATA_WEATHER`); the `NONE` Paper no-op variant is additionally
//! declared for parity with the future `mc.util.datafix` port but is not used
//! by any value in this unit. The stub declares the enum's variant surface as
//! a value-identity enum without the DFU machinery; the owning unit's port
//! replaces it wholesale.

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
}
