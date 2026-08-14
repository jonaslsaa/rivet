//! Port of `net.minecraft.world.level.saveddata.WanderingTraderData` (MC 26.2).
//!
//! The per-world wandering-trader spawn config persisted as `data/
//! wandering_trader.dat`: the `spawn_delay`/`spawn_chance` ints, the
//! `CODEC` over the two optional (defaulting) fields, the `TYPE`
//! `SavedDataType`, and the setters that mark the blob dirty only when a value
//! actually changes (so a no-op call never forces a re-save).

use super::saved_data::SavedData;
use super::saved_data_type::SavedDataType;
use crate::level::saveddata::stub_data_fix_types::DataFixTypes;
use rivet_registry::Identifier;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `WanderingTraderData.spawn_delay` default — `24000`.
pub const DEFAULT_SPAWN_DELAY: i32 = 24000;
/// `WanderingTraderData.spawn_chance` default — `25`.
pub const DEFAULT_SPAWN_CHANCE: i32 = 25;

/// `net.minecraft.world.level.saveddata.WanderingTraderData`.
#[derive(Debug, Clone)]
pub struct WanderingTraderData {
    /// The `SavedData` base (`private boolean dirty`).
    base: SavedData,
    /// `this.spawnDelay`.
    spawn_delay: i32,
    /// `this.spawnChance`.
    spawn_chance: i32,
}

impl WanderingTraderData {
    /// `WanderingTraderData()` — `this(24000, 25)`.
    pub fn new() -> Self {
        Self::new_with(DEFAULT_SPAWN_DELAY, DEFAULT_SPAWN_CHANCE)
    }

    /// `WanderingTraderData(int spawnDelay, int spawnChance)`.
    pub fn new_with(spawn_delay: i32, spawn_chance: i32) -> Self {
        WanderingTraderData {
            base: SavedData::default(),
            spawn_delay,
            spawn_chance,
        }
    }

    /// `WanderingTraderData.CODEC` — `RecordCodecBuilder.create` over
    /// `Codec.INT.optionalFieldOf("spawn_delay", 24000)` and
    /// `Codec.INT.optionalFieldOf("spawn_chance", 25)`. The optional fields are
    /// non-lenient with defaults: an absent field decodes to the default, a
    /// present-but-malformed field fails, and a field Java-equal to its default
    /// is omitted on encode. The declaration order is the encode order.
    pub fn codec<Ops: DynamicOps + 'static>()
    -> Arc<dyn rivet_serialization::Codec<WanderingTraderData, Ops>>
    where
        WanderingTraderData: 'static,
    {
        record_builder::create(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|d: &WanderingTraderData| d.spawn_delay),
                    codec::optional_field_of(
                        "spawn_delay",
                        codec::int_codec::<Ops>(),
                        DEFAULT_SPAWN_DELAY,
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|d: &WanderingTraderData| d.spawn_chance),
                    codec::optional_field_of(
                        "spawn_chance",
                        codec::int_codec::<Ops>(),
                        DEFAULT_SPAWN_CHANCE,
                    ),
                ))
                .apply(instance, Arc::new(WanderingTraderData::new_with))
        })
    }

    /// `WanderingTraderData.TYPE` — `new SavedDataType<>(
    /// Identifier.withDefaultNamespace("wandering_trader"),
    /// WanderingTraderData::new, CODEC,
    /// DataFixTypes.SAVED_DATA_WANDERING_TRADER)`. The codec slot is the
    /// NbtOps-pinned codec the disk runtime uses. Unlike Java's `static final
    /// TYPE` singleton, this builds a fresh equivalent value per call (equality
    /// is by `id` only, so the values are identical).
    pub fn type_() -> SavedDataType<WanderingTraderData> {
        SavedDataType::new(
            Identifier::with_default_namespace("wandering_trader"),
            Arc::new(WanderingTraderData::new),
            WanderingTraderData::codec::<rivet_nbt::nbt_ops::NbtOps>(),
            DataFixTypes::SavedDataWanderingTrader,
        )
    }

    /// `spawnDelay()`.
    pub fn spawn_delay(&self) -> i32 {
        self.spawn_delay
    }

    /// `setSpawnDelay(int)` — marks dirty only when the value changes.
    pub fn set_spawn_delay(&mut self, spawn_delay: i32) {
        if self.spawn_delay != spawn_delay {
            self.spawn_delay = spawn_delay;
            self.base.set_dirty_flag(true);
        }
    }

    /// `spawnChance()`.
    pub fn spawn_chance(&self) -> i32 {
        self.spawn_chance
    }

    /// `setSpawnChance(int)` — marks dirty only when the value changes.
    pub fn set_spawn_chance(&mut self, spawn_chance: i32) {
        if self.spawn_chance != spawn_chance {
            self.spawn_chance = spawn_chance;
            self.base.set_dirty_flag(true);
        }
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

impl Default for WanderingTraderData {
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
    fn defaults_are_24000_and_25() {
        let data = WanderingTraderData::new();
        assert_eq!(data.spawn_delay(), DEFAULT_SPAWN_DELAY);
        assert_eq!(data.spawn_chance(), DEFAULT_SPAWN_CHANCE);
        assert!(!data.is_dirty());
    }

    #[test]
    fn setters_only_set_dirty_on_change() {
        let mut data = WanderingTraderData::new();
        data.set_spawn_delay(12000);
        assert!(data.is_dirty());
        assert_eq!(data.spawn_delay(), 12000);

        let mut data = WanderingTraderData::new();
        data.set_spawn_delay(DEFAULT_SPAWN_DELAY);
        assert!(!data.is_dirty(), "same value must not mark dirty");

        let mut data = WanderingTraderData::new();
        data.set_spawn_chance(50);
        assert!(data.is_dirty());
        assert_eq!(data.spawn_chance(), 50);

        let mut data = WanderingTraderData::new();
        data.set_spawn_chance(DEFAULT_SPAWN_CHANCE);
        assert!(!data.is_dirty(), "same value must not mark dirty");
    }

    #[test]
    fn codec_round_trips() {
        let ops = JsonOps::INSTANCE;
        let codec = WanderingTraderData::codec::<JsonOps>();
        let value = WanderingTraderData::new_with(12000, 50);
        let encoded = codec
            .encode_start(&ops, &value)
            .get_or_throw("encode")
            .clone();
        let obj = encoded.as_object().expect("object");
        assert_eq!(
            obj.keys().collect::<Vec<_>>(),
            vec!["spawn_delay", "spawn_chance"]
        );
        let decoded = codec.decode(&ops, &encoded).get_or_throw("decode").clone();
        assert_eq!(decoded.0.spawn_delay(), 12000);
        assert_eq!(decoded.0.spawn_chance(), 50);
    }

    #[test]
    fn codec_omits_fields_equal_to_default_on_encode() {
        let ops = JsonOps::INSTANCE;
        let codec = WanderingTraderData::codec::<JsonOps>();
        // Both fields at their defaults → the encode object has no keys.
        let value = WanderingTraderData::new();
        let encoded = codec
            .encode_start(&ops, &value)
            .get_or_throw("encode")
            .clone();
        let obj = encoded.as_object().expect("object");
        assert_eq!(obj.keys().collect::<Vec<_>>(), Vec::<&str>::new());
    }

    #[test]
    fn codec_defaults_missing_fields_on_decode() {
        let ops = JsonOps::INSTANCE;
        let codec = WanderingTraderData::codec::<JsonOps>();
        // Only spawn_delay present; spawn_chance defaults to 25.
        let input = ops.create_map(vec![Pair::of(
            ops.create_string("spawn_delay".to_string()),
            ops.create_int(12000),
        )]);
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(decoded.0.spawn_delay(), 12000);
        assert_eq!(decoded.0.spawn_chance(), DEFAULT_SPAWN_CHANCE);
        // Both absent → both defaults.
        let empty = ops.create_map(Vec::new());
        let decoded = codec.decode(&ops, &empty).get_or_throw("decode").clone();
        assert_eq!(decoded.0.spawn_delay(), DEFAULT_SPAWN_DELAY);
        assert_eq!(decoded.0.spawn_chance(), DEFAULT_SPAWN_CHANCE);
    }

    #[test]
    fn codec_fails_on_malformed_present_field() {
        let ops = JsonOps::INSTANCE;
        let codec = WanderingTraderData::codec::<JsonOps>();
        // A present-but-non-numeric field is a decode error (non-lenient);
        // both optional fields are exercised.
        let malformed_spawn_delay = ops.create_map(vec![Pair::of(
            ops.create_string("spawn_delay".to_string()),
            ops.create_string("bogus".to_string()),
        )]);
        assert!(
            codec
                .decode(&ops, &malformed_spawn_delay)
                .result()
                .is_none()
        );
        let malformed_spawn_chance = ops.create_map(vec![Pair::of(
            ops.create_string("spawn_chance".to_string()),
            ops.create_string("bogus".to_string()),
        )]);
        assert!(
            codec
                .decode(&ops, &malformed_spawn_chance)
                .result()
                .is_none()
        );
    }

    #[test]
    fn type_has_expected_identity() {
        let t = WanderingTraderData::type_();
        assert_eq!(t.id().to_string(), "minecraft:wandering_trader");
        assert_eq!(t.data_fix_type(), DataFixTypes::SavedDataWanderingTrader);
        assert_eq!(t.to_string(), "SavedDataType[minecraft:wandering_trader]");
        let constructed = (t.constructor())();
        assert_eq!(constructed.spawn_delay(), DEFAULT_SPAWN_DELAY);
    }
}
