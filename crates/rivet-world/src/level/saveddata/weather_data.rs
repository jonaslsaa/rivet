//! Port of `net.minecraft.world.level.saveddata.WeatherData` (MC 26.2).
//!
//! The per-world weather state persisted as `data/weather.dat`: the three
//! timer ints plus the raining/thundering booleans, a `CODEC` over the five
//! mandatory fields, and the `TYPE` `SavedDataType`.
//!
//! ## Seams
//!
//! - **CraftBukkit event hooks.** Java's `setRaining(boolean)`/`setThundering(
//!   boolean)` fire `WeatherChangeEvent`/`ThunderChangeEvent` on the
//!   `Bukkit` server and bail out (leaving the field unchanged, still not
//!   dirty) when the event is cancelled; the no-op early-return happens BEFORE
//!   the event machinery, so a same-value call never fires an event nor marks
//!   dirty. Paper adds a `cause` overload
//!   (`setRaining(boolean, WeatherChangeEvent.Cause)` /
//!   `setThundering(boolean, ThunderChangeEvent.Cause)`). The Bukkit plugin
//!   event system is not ported (see `OWNERSHIP.md`), so the port keeps the
//!   guard-and-assign core (`same value -> return` + `assign; setDirty()`) and
//!   drops the event dispatch.
//! - **Paper `ServerLevel` field / `setLevel`.** Java also carries a
//!   `private @Nullable ServerLevel level` field set via the server-internal
//!   `setLevel(ServerLevel)`, solely to feed the CraftBukkit events
//!   (`this.level == null ? null : this.level.getWorld()`). The concrete level
//!   lives in `rivet-server` (the `mc.server.level` unit, not yet ported), and
//!   rivet-world cannot depend on it. The field and `setLevel` are dropped here
//!   — nothing reads them without the event dispatch. When `mc.server.level`
//!   lands and wires the real event dispatch, the `setRaining`/`setThundering`
//!   re-typing happens in that owning unit.
//!
//! RivetTodo(#26): `setRaining`/`setThundering` drop the CraftBukkit
//! `WeatherChangeEvent`/`ThunderChangeEvent` dispatch + cancellation bail-out
//! (and the `@Nullable ServerLevel level` field + `setLevel()` that feed them —
//! see the seams above). A cancelled event in Java leaves the field unchanged
//! and un-dirtied; the dispatch is re-added here when the CraftBukkit event
//! system lands (epic #26, OWNERSHIP.md "Events (Bukkit/Paper layer)").

use super::saved_data::SavedData;
use super::saved_data_type::SavedDataType;
use crate::level::saveddata::stub_data_fix_types::DataFixTypes;
use rivet_registry::Identifier;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::{Arc, LazyLock};

/// `net.minecraft.world.level.saveddata.WeatherData`.
#[derive(Debug, Clone)]
pub struct WeatherData {
    /// The `SavedData` base (`private boolean dirty`).
    base: SavedData,
    /// `this.clearWeatherTime`.
    clear_weather_time: i32,
    /// `this.rainTime`.
    rain_time: i32,
    /// `this.thunderTime`.
    thunder_time: i32,
    /// `this.raining`.
    raining: bool,
    /// `this.thundering`.
    thundering: bool,
}

impl WeatherData {
    /// `WeatherData()` — the no-arg constructor with all-zero/false state.
    pub fn new() -> Self {
        WeatherData {
            base: SavedData::default(),
            clear_weather_time: 0,
            rain_time: 0,
            thunder_time: 0,
            raining: false,
            thundering: false,
        }
    }

    /// `WeatherData(int clearWeatherTime, int rainTime, int thunderTime,
    /// boolean raining, boolean thundering)`.
    pub fn new_with(
        clear_weather_time: i32,
        rain_time: i32,
        thunder_time: i32,
        raining: bool,
        thundering: bool,
    ) -> Self {
        WeatherData {
            base: SavedData::default(),
            clear_weather_time,
            rain_time,
            thunder_time,
            raining,
            thundering,
        }
    }

    /// `WeatherData.CODEC` — `RecordCodecBuilder.create` over the five
    /// mandatory `fieldOf` fields in declaration order. A missing or malformed
    /// field fails the whole decode.
    pub fn codec<Ops: DynamicOps + 'static>()
    -> Arc<dyn rivet_serialization::Codec<WeatherData, Ops>>
    where
        WeatherData: 'static,
    {
        record_builder::create(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|d: &WeatherData| d.clear_weather_time),
                    codec::field_of(codec::int_codec::<Ops>(), "clear_weather_time".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|d: &WeatherData| d.rain_time),
                    codec::field_of(codec::int_codec::<Ops>(), "rain_time".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|d: &WeatherData| d.thunder_time),
                    codec::field_of(codec::int_codec::<Ops>(), "thunder_time".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|d: &WeatherData| d.raining),
                    codec::field_of(codec::bool_codec::<Ops>(), "raining".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|d: &WeatherData| d.thundering),
                    codec::field_of(codec::bool_codec::<Ops>(), "thundering".to_string()),
                ))
                .apply(instance, Arc::new(WeatherData::new_with))
        })
    }

    /// `getClearWeatherTime()`.
    pub fn clear_weather_time(&self) -> i32 {
        self.clear_weather_time
    }

    /// `setClearWeatherTime(int)` — always marks dirty.
    pub fn set_clear_weather_time(&mut self, clear_weather_time: i32) {
        self.clear_weather_time = clear_weather_time;
        self.base.set_dirty();
    }

    /// `isThundering()`.
    pub fn is_thundering(&self) -> bool {
        self.thundering
    }

    /// `setThundering(boolean)` (Paper: `setThundering(boolean, Cause)`).
    ///
    /// Without the CraftBukkit event dispatch, this is the guard-and-assign
    /// core: a same-value call returns without marking dirty; otherwise the
    /// field is set and the blob marked dirty.
    ///
    /// RivetTodo(#26): the `ThunderChangeEvent` dispatch + cancellation
    /// bail-out and the `level`/`setLevel` feed are dropped (module-doc seam).
    pub fn set_thundering(&mut self, thundering: bool) {
        if self.thundering == thundering {
            return;
        }
        self.thundering = thundering;
        self.base.set_dirty();
    }

    /// `getThunderTime()`.
    pub fn thunder_time(&self) -> i32 {
        self.thunder_time
    }

    /// `setThunderTime(int)` — always marks dirty.
    pub fn set_thunder_time(&mut self, thunder_time: i32) {
        self.thunder_time = thunder_time;
        self.base.set_dirty();
    }

    /// `isRaining()`.
    pub fn is_raining(&self) -> bool {
        self.raining
    }

    /// `setRaining(boolean)` (Paper: `setRaining(boolean, Cause)`).
    ///
    /// Same guard-and-assign core as `set_thundering` (see the module doc's
    /// seam note for the dropped CraftBukkit event dispatch).
    ///
    /// RivetTodo(#26): the `WeatherChangeEvent` dispatch + cancellation
    /// bail-out and the `level`/`setLevel` feed are dropped (module-doc seam).
    pub fn set_raining(&mut self, raining: bool) {
        if self.raining == raining {
            return;
        }
        self.raining = raining;
        self.base.set_dirty();
    }

    /// `getRainTime()`.
    pub fn rain_time(&self) -> i32 {
        self.rain_time
    }

    /// `setRainTime(int)` — always marks dirty.
    pub fn set_rain_time(&mut self, rain_time: i32) {
        self.rain_time = rain_time;
        self.base.set_dirty();
    }

    // --- inherited `SavedData` surface ---

    /// `isDirty()`.
    pub fn is_dirty(&self) -> bool {
        self.base.is_dirty()
    }

    /// `setDirty()`.
    pub fn set_dirty(&mut self) {
        self.base.set_dirty();
    }

    /// `setDirty(boolean)`.
    pub fn set_dirty_flag(&mut self, dirty: bool) {
        self.base.set_dirty_flag(dirty);
    }
}

/// `WeatherData.TYPE` — `new SavedDataType<>(
/// Identifier.withDefaultNamespace("weather"), WeatherData::new, CODEC,
/// DataFixTypes.SAVED_DATA_WEATHER)`. The codec slot is the NbtOps-pinned
/// codec the disk runtime uses. Java's `static final TYPE` singleton is a
/// `LazyLock` static in the port.
pub static TYPE: LazyLock<SavedDataType<WeatherData>> = LazyLock::new(|| {
    SavedDataType::new(
        Identifier::with_default_namespace("weather"),
        Arc::new(WeatherData::new),
        WeatherData::codec::<rivet_nbt::nbt_ops::NbtOps>(),
        DataFixTypes::SavedDataWeather,
    )
});

impl Default for WeatherData {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::pair::Pair;

    #[test]
    fn no_arg_constructor_has_zeroed_state() {
        let data = WeatherData::new();
        assert_eq!(data.clear_weather_time(), 0);
        assert_eq!(data.rain_time(), 0);
        assert_eq!(data.thunder_time(), 0);
        assert!(!data.is_raining());
        assert!(!data.is_thundering());
        assert!(!data.is_dirty());
    }

    #[test]
    fn timer_setters_always_mark_dirty() {
        let mut data = WeatherData::new();
        data.set_clear_weather_time(100);
        assert_eq!(data.clear_weather_time(), 100);
        assert!(data.is_dirty());

        let mut data = WeatherData::new();
        data.set_thunder_time(200);
        assert_eq!(data.thunder_time(), 200);
        assert!(data.is_dirty());

        let mut data = WeatherData::new();
        data.set_rain_time(300);
        assert_eq!(data.rain_time(), 300);
        assert!(data.is_dirty());
    }

    #[test]
    fn weather_setters_skip_same_value() {
        let mut data = WeatherData::new_with(0, 0, 0, false, false);
        // Same value → no dirty.
        data.set_raining(false);
        assert!(!data.is_dirty());
        data.set_thundering(false);
        assert!(!data.is_dirty());
        // Change → dirty.
        data.set_raining(true);
        assert!(data.is_raining());
        assert!(data.is_dirty());
        let mut data = WeatherData::new();
        data.set_thundering(true);
        assert!(data.is_thundering());
        assert!(data.is_dirty());
    }

    #[test]
    fn codec_round_trips() {
        let ops = JsonOps::INSTANCE;
        let codec = WeatherData::codec::<JsonOps>();
        let value = WeatherData::new_with(10, 20, 30, true, false);
        let encoded = codec
            .encode_start(&ops, &value)
            .get_or_throw("encode")
            .clone();
        let obj = encoded.as_object().expect("object");
        assert_eq!(
            obj.keys().collect::<Vec<_>>(),
            vec![
                "clear_weather_time",
                "rain_time",
                "thunder_time",
                "raining",
                "thundering"
            ]
        );
        let decoded = codec.decode(&ops, &encoded).get_or_throw("decode").clone();
        assert_eq!(decoded.0.clear_weather_time(), 10);
        assert_eq!(decoded.0.rain_time(), 20);
        assert_eq!(decoded.0.thunder_time(), 30);
        assert!(decoded.0.is_raining());
        assert!(!decoded.0.is_thundering());
    }

    #[test]
    fn codec_requires_all_fields() {
        let ops = JsonOps::INSTANCE;
        let codec = WeatherData::codec::<JsonOps>();
        // Missing "thundering" fails the whole decode (mandatory field).
        let missing = ops.create_map(vec![
            Pair::of(
                ops.create_string("clear_weather_time".to_string()),
                ops.create_int(0),
            ),
            Pair::of(
                ops.create_string("rain_time".to_string()),
                ops.create_int(0),
            ),
            Pair::of(
                ops.create_string("thunder_time".to_string()),
                ops.create_int(0),
            ),
            Pair::of(
                ops.create_string("raining".to_string()),
                ops.create_boolean(false),
            ),
        ]);
        assert!(codec.decode(&ops, &missing).result().is_none());
        // Malformed present field fails too.
        let malformed = ops.create_map(vec![
            Pair::of(
                ops.create_string("clear_weather_time".to_string()),
                ops.create_int(0),
            ),
            Pair::of(
                ops.create_string("rain_time".to_string()),
                ops.create_int(0),
            ),
            Pair::of(
                ops.create_string("thunder_time".to_string()),
                ops.create_int(0),
            ),
            Pair::of(
                ops.create_string("raining".to_string()),
                ops.create_boolean(false),
            ),
            Pair::of(
                ops.create_string("thundering".to_string()),
                ops.create_string("bogus".to_string()),
            ),
        ]);
        assert!(codec.decode(&ops, &malformed).result().is_none());
    }

    #[test]
    fn type_has_expected_identity() {
        let t: &SavedDataType<WeatherData> = &TYPE;
        assert_eq!(t.id().to_string(), "minecraft:weather");
        assert_eq!(t.data_fix_type(), DataFixTypes::SavedDataWeather);
        assert_eq!(t.to_string(), "SavedDataType[minecraft:weather]");
        let constructed = (t.constructor())();
        assert_eq!(constructed.clear_weather_time(), 0);
    }
}
