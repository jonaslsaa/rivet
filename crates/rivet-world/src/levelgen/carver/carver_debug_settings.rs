//! Port of `net.minecraft.world.level.levelgen.carver.CarverDebugSettings`
//! (class, 26.2) — the carver debug-mode block-state overrides.
//!
//! Java: a five-field value (`boolean debugMode`, the `airState`/`waterState`/
//! `lavaState`/`barrierState` `BlockState`s), the `DEFAULT` constant
//! (`ACACIA_BUTTON`/`CANDLE`/`STAINED_GLASS.orange()`/`GLASS` default states),
//! three `of(...)` factories, and the `CODEC`:
//!
//! ```java
//! RecordCodecBuilder.create(i -> i.group(
//!     Codec.BOOL.optionalFieldOf("debug_mode", false).forGetter(CarverDebugSettings::isDebugMode),
//!     BlockState.CODEC.optionalFieldOf("air_state", DEFAULT.getAirState()).forGetter(CarverDebugSettings::getAirState),
//!     BlockState.CODEC.optionalFieldOf("water_state", DEFAULT.getAirState()).forGetter(CarverDebugSettings::getWaterState),
//!     BlockState.CODEC.optionalFieldOf("lava_state", DEFAULT.getAirState()).forGetter(CarverDebugSettings::getLavaState),
//!     BlockState.CODEC.optionalFieldOf("barrier_state", DEFAULT.getAirState()).forGetter(CarverDebugSettings::getBarrierState)
//! ).apply(i, CarverDebugSettings::new))
//! ```
//!
//! Note the `water_state`/`lava_state`/`barrier_state` optional defaults are
//! `DEFAULT.getAirState()` — all four block-state fields default to the *air*
//! state, not to their own getters' `DEFAULT` values. The `CODEC` is a
//! `Codec` (not a `MapCodec`), so `RecordCodecBuilder.create` → the ops-generic
//! `carver_debug_settings_codec::<Ops>()` factory below.
//!
//! `optionalFieldOf` in the Rust codec infrastructure requires
//! `F: JavaEquals` for the with-default form, and `BlockState` has no
//! `JavaEquals` impl (orphan rule — the impl can only live in `rivet-registry`,
//! outside this crate). The four block-state fields therefore use the raw
//! `codec::optional_field` + `map_codec::xmap` form with a local
//! `same_block_state` equality (BlockState is `Copy + PartialEq`), matching the
//! DFU `Objects.equals` omission test exactly for the equal-to-default case.
//!
//! `CarverDebugSettings` itself IS a local type, so it implements `JavaEquals`
//! (the `optional_field_of` bound on the `debug_settings` field of
//! `CarverConfigurationBase`).

use crate::block::blocks::Blocks;
use rivet_registry::block_state::BlockState;
use rivet_serialization::codec::{self, Codec, JavaEquals};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `CarverDebugSettings.DEFAULT` — `new(false, Blocks.ACACIA_BUTTON.
/// defaultBlockState(), Blocks.CANDLE.defaultBlockState(), Blocks.STAINED_GLASS.
/// orange().defaultBlockState(), Blocks.GLASS.defaultBlockState())`. The
/// `Blocks.STAINED_GLASS.orange()` color pick is the `ORANGE_STAINED_GLASS`
/// block's default state.
///
/// `BlockState::of` is not `const` (it does a generated-table lookup), so the
/// Java `DEFAULT` constant is the `Default` impl (called once per use; the
/// codec paths hold it in an `Arc`/closure, matching the Java `static final`
/// lifetime for all practical purposes).
impl Default for CarverDebugSettings {
    fn default() -> Self {
        CarverDebugSettings {
            debug_mode: false,
            air_state: BlockState::of(Blocks::ACACIA_BUTTON.id()),
            water_state: BlockState::of(Blocks::CANDLE.id()),
            lava_state: BlockState::of(Blocks::ORANGE_STAINED_GLASS.id()),
            barrier_state: BlockState::of(Blocks::GLASS.id()),
        }
    }
}

/// The `optionalFieldOf(..., DEFAULT)` bound on the `debug_settings` field of
/// `CarverConfigurationBase`'s codec. `CarverDebugSettings` is a local type, so
/// the impl is allowed (unlike `BlockState`, which cannot get one here — see
/// the module doc). Java compares the record via the field-wise
/// `Objects.equals`, which the derived `PartialEq` mirrors (all fields are
/// `Copy`/`Eq`).
impl JavaEquals for CarverDebugSettings {
    fn java_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// `CarverDebugSettings` — the `@Deprecated` carver debug block-state
/// overrides (`WorldCarver.getDebugState` consults them when debug is enabled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CarverDebugSettings {
    /// `debugMode` — `isDebugMode()`; `WorldCarver.isDebugEnabled` ORs it with
    /// `SharedConstants.DEBUG_CARVERS`.
    pub debug_mode: bool,
    /// `airState` — the `getDebugState` AIR replacement.
    pub air_state: BlockState,
    /// `waterState` — the `getDebugState` WATER replacement (WATERLOGGED set
    /// when the block supports it).
    pub water_state: BlockState,
    /// `lavaState` — the `getDebugState` LAVA replacement.
    pub lava_state: BlockState,
    /// `barrierState` — the `getCarveState` null-aquifer-substance
    /// replacement (debug mode only).
    pub barrier_state: BlockState,
}

impl CarverDebugSettings {
    /// `of(boolean, BlockState, BlockState, BlockState, BlockState)` — the
    /// full factory.
    pub fn of(
        debug_mode: bool,
        air_state: BlockState,
        water_state: BlockState,
        lava_state: BlockState,
        barrier_state: BlockState,
    ) -> Self {
        CarverDebugSettings {
            debug_mode,
            air_state,
            water_state,
            lava_state,
            barrier_state,
        }
    }

    /// `of(BlockState, BlockState, BlockState, BlockState)` — the
    /// `debugMode = false` factory.
    pub fn of_states(
        air_state: BlockState,
        water_state: BlockState,
        lava_state: BlockState,
        barrier_state: BlockState,
    ) -> Self {
        CarverDebugSettings::of(false, air_state, water_state, lava_state, barrier_state)
    }

    /// `of(boolean debugMode, BlockState airState)` — the one-block-state
    /// factory (water/lava/barrier fall back to `DEFAULT`).
    pub fn of_debug_mode_air(debug_mode: bool, air_state: BlockState) -> Self {
        let d = CarverDebugSettings::default();
        CarverDebugSettings {
            debug_mode,
            air_state,
            water_state: d.water_state,
            lava_state: d.lava_state,
            barrier_state: d.barrier_state,
        }
    }

    /// `isDebugMode()`.
    pub fn is_debug_mode(&self) -> bool {
        self.debug_mode
    }

    /// `getAirState()`.
    pub fn air_state(&self) -> BlockState {
        self.air_state
    }

    /// `getWaterState()`.
    pub fn water_state(&self) -> BlockState {
        self.water_state
    }

    /// `getLavaState()`.
    pub fn lava_state(&self) -> BlockState {
        self.lava_state
    }

    /// `getBarrierState()`.
    pub fn barrier_state(&self) -> BlockState {
        self.barrier_state
    }
}

/// `CarverDebugSettings.CODEC` — the ops-generic
/// `carver_debug_settings_codec::<Ops>()` factory (a record `Codec` over the
/// five optional fields, `RecordCodecBuilder.create`). The `debug_mode` field
/// uses the with-default `optional_field_of` (`bool: JavaEquals`); the four
/// `BlockState` fields use raw `optional_field` + `map_codec::xmap` with the
/// local equality (see the module doc).
pub fn carver_debug_settings_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<CarverDebugSettings, Ops>> {
    let debug_mode_field =
        codec::optional_field_of::<bool, Ops>("debug_mode", codec::bool_codec(), false);
    let air_field = optional_block_state_field::<Ops>("air_state");
    let water_field = optional_block_state_field::<Ops>("water_state");
    let lava_field = optional_block_state_field::<Ops>("lava_state");
    let barrier_field = optional_block_state_field::<Ops>("barrier_state");

    record_builder::create::<CarverDebugSettings, Ops>(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &CarverDebugSettings| c.debug_mode),
                debug_mode_field,
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CarverDebugSettings| c.air_state),
                air_field,
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CarverDebugSettings| c.water_state),
                water_field,
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CarverDebugSettings| c.lava_state),
                lava_field,
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CarverDebugSettings| c.barrier_state),
                barrier_field,
            ))
            .apply(
                instance,
                Arc::new(
                    |debug_mode: bool,
                     air: BlockState,
                     water: BlockState,
                     lava: BlockState,
                     barrier: BlockState| {
                        CarverDebugSettings::of(debug_mode, air, water, lava, barrier)
                    },
                ),
            )
    })
}

/// `BlockState.CODEC.optionalFieldOf(name, DEFAULT.getAirState())` — the raw
/// `optional_field` + `xmap` form (no `JavaEquals` on `BlockState`; the
/// omission test is the local `==`, which matches `Objects.equals` for the
/// equal-to-default case). The default is the air state for every one of the
/// four block-state fields (Java's `optionalFieldOf(..., DEFAULT.getAirState())`).
fn optional_block_state_field<Ops: DynamicOps + 'static>(
    name: &str,
) -> Arc<dyn MapCodec<BlockState, Ops>> {
    let default_air = CarverDebugSettings::default().air_state;
    let inner = codec::optional_field::<BlockState, Ops>(
        name.to_string(),
        rivet_registry::block_state_codec::block_state_codec::<Ops>(),
        false,
    );
    map_codec::xmap(
        inner,
        Arc::new(move |o: &Option<BlockState>| o.unwrap_or(default_air)),
        Arc::new(
            move |a: &BlockState| {
                if *a == default_air { None } else { Some(*a) }
            },
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn round_trip(value: &CarverDebugSettings) -> serde_json::Value {
        let codec = carver_debug_settings_codec::<JsonOps>();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, value)
            .result()
            .expect("encode")
            .clone();
        let decoded = *codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode");
        assert_eq!(&decoded, value);
        encoded
    }

    #[test]
    fn default_has_the_paper_block_states() {
        // `CarverDebugSettings.DEFAULT` = `new(false, ACACIA_BUTTON, CANDLE,
        // STAINED_GLASS.orange(), GLASS)`.
        let default = CarverDebugSettings::default();
        assert!(!default.debug_mode);
        assert_eq!(default.air_state.block(), Blocks::ACACIA_BUTTON.id());
        assert_eq!(default.water_state.block(), Blocks::CANDLE.id());
        assert_eq!(
            default.lava_state.block(),
            Blocks::ORANGE_STAINED_GLASS.id()
        );
        assert_eq!(default.barrier_state.block(), Blocks::GLASS.id());
    }

    #[test]
    fn default_round_trips_and_omits_only_debug_mode_and_air_state() {
        // `DEFAULT` = `new(false, ACACIA_BUTTON, CANDLE, STAINED_GLASS.orange(),
        // GLASS)`. Each optional field's default is `DEFAULT.getAirState()`
        // (ACACIA_BUTTON), so encoding omits only `debug_mode` (default false)
        // and `air_state` (the ACACIA_BUTTON default); the three non-air states
        // are all present.
        let default = CarverDebugSettings::default();
        let encoded = round_trip(&default);
        let obj = encoded.as_object().expect("map");
        assert!(
            !obj.contains_key("debug_mode") && !obj.contains_key("air_state"),
            "expected DEFAULT to omit debug_mode and air_state: {encoded}"
        );
        assert!(obj.contains_key("water_state"), "{encoded}");
        assert!(obj.contains_key("lava_state"), "{encoded}");
        assert!(obj.contains_key("barrier_state"), "{encoded}");
    }

    #[test]
    fn present_non_default_fields_are_preserved() {
        // `of(true, air_state, water_state, lava_state, barrier_state)` with
        // distinct non-default states round-trips and encodes every field.
        let value = CarverDebugSettings::of(
            true,
            Blocks::STONE.default_block_state(),
            Blocks::WATER.default_block_state(),
            Blocks::LAVA.default_block_state(),
            Blocks::BARRIER.default_block_state(),
        );
        let encoded = round_trip(&value);
        let obj = encoded.as_object().expect("map");
        assert_eq!(obj.get("debug_mode"), Some(&json!(true)));
        assert!(obj.contains_key("air_state"));
        assert!(obj.contains_key("water_state"));
        assert!(obj.contains_key("lava_state"));
        assert!(obj.contains_key("barrier_state"));
    }

    #[test]
    fn air_equal_fields_are_omitted_independently() {
        // A value whose air_state differs from the default but whose
        // water_state stays at the air default omits only the latter.
        let default_air = CarverDebugSettings::default().air_state;
        let value = CarverDebugSettings::of_states(
            Blocks::STONE.default_block_state(),
            default_air,
            Blocks::LAVA.default_block_state(),
            default_air,
        );
        let encoded = round_trip(&value);
        let obj = encoded.as_object().expect("map");
        assert!(obj.contains_key("air_state"));
        assert!(!obj.contains_key("water_state"));
        assert!(obj.contains_key("lava_state"));
        assert!(!obj.contains_key("barrier_state"));
    }
}
