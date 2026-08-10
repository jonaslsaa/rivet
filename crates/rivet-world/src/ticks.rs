//! Port of `net.minecraft.world.ticks` (MC 26.2, #370) — the *value* layer
//! only.
//!
//! Java source: `working/Paper/.../net/minecraft/world/ticks/`. This slice
//! ports the serializable value surface that a stored chunk's `block_ticks`/
//! `fluid_ticks` lists and `UpgradeData`'s neighbor-tick lists decode into:
//!
//! - [`TickPriority`] (`TickPriority.java`) — the enum + `CODEC` (an int
//!   xmap with Java's clamp-on-out-of-range fallback).
//! - [`SavedTick`] (`SavedTick.java`) — the `record SavedTick<T>(T type,
//!   BlockPos pos, int delay, TickPriority priority)` value type, its faithful
//!   [`saved_tick_codec`] codec factory (fields `i`/`x`/`y`/`z`/`t`/`p`), and
//!   [`filter_tick_list_for_chunk`] (the per-chunk filter `filterTickListForChunk`).
//!
//! The execution/scheduling surfaces (`ScheduledTick`, `LevelChunkTicks`,
//! `ProtoChunkTicks`, `TickContainerAccess`, `unpack`, ...) are deliberately
//! deferred (RivetTodo below): this slice is the value/carry layer the
//! `SerializableChunkData` parser feeds and nothing schedules or executes.

use rivet_registry::core::{BlockPos, ChunkPos};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.ticks.TickPriority` — the scheduling-priority enum.
///
/// Values match the Java ordinal order (`EXTREMELY_HIGH(-3)` first), which is
/// the wire form the `CODEC` uses (`getValue` returns the raw value; decode is
/// `Codec.INT.xmap(TickPriority::byValue, TickPriority::getValue)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TickPriority {
    /// `EXTREMELY_HIGH(-3)`.
    ExtremelyHigh,
    /// `VERY_HIGH(-2)`.
    VeryHigh,
    /// `HIGH(-1)`.
    High,
    /// `NORMAL(0)`.
    Normal,
    /// `LOW(1)`.
    Low,
    /// `VERY_LOW(2)`.
    VeryLow,
    /// `EXTREMELY_LOW(3)`.
    ExtremelyLow,
}

impl TickPriority {
    /// `values()` — the seven priorities in declaration (ordinal) order.
    pub const fn all() -> [TickPriority; 7] {
        [
            TickPriority::ExtremelyHigh,
            TickPriority::VeryHigh,
            TickPriority::High,
            TickPriority::Normal,
            TickPriority::Low,
            TickPriority::VeryLow,
            TickPriority::ExtremelyLow,
        ]
    }

    /// `getValue()` — the raw int value.
    pub const fn value(self) -> i32 {
        match self {
            TickPriority::ExtremelyHigh => -3,
            TickPriority::VeryHigh => -2,
            TickPriority::High => -1,
            TickPriority::Normal => 0,
            TickPriority::Low => 1,
            TickPriority::VeryLow => 2,
            TickPriority::ExtremelyLow => 3,
        }
    }

    /// `byValue(int)` — the first priority whose value matches, else the
    /// clamped end (`value < EXTREMELY_HIGH.value ? EXTREMELY_HIGH :
    /// EXTREMELY_LOW`).
    pub fn by_value(value: i32) -> TickPriority {
        for priority in TickPriority::all() {
            if priority.value() == value {
                return priority;
            }
        }
        if value < TickPriority::ExtremelyHigh.value() {
            TickPriority::ExtremelyHigh
        } else {
            TickPriority::ExtremelyLow
        }
    }

    /// `TickPriority.CODEC` — `Codec.INT.xmap(TickPriority::byValue,
    /// TickPriority::getValue)`, as the ops-generic factory.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<TickPriority, Ops>> {
        codec::xmap(
            codec::int_codec::<Ops>(),
            Arc::new(|value: &i32| TickPriority::by_value(*value)),
            Arc::new(|priority: &TickPriority| priority.value()),
        )
    }
}

/// `net.minecraft.world.ticks.SavedTick<T>` — `record SavedTick<T>(T type,
/// BlockPos pos, int delay, TickPriority priority)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SavedTick<T> {
    /// `type` — the block/fluid id-handle (`Block` / `FluidId`).
    pub r#type: T,
    /// `pos` — the block position.
    pub pos: BlockPos,
    /// `delay` — the relative tick delay (`t`).
    pub delay: i32,
    /// `priority` — the scheduling priority (`p`).
    pub priority: TickPriority,
}

impl<T> SavedTick<T> {
    /// `new SavedTick<>(T type, BlockPos pos, int delay, TickPriority
    /// priority)`.
    pub fn new(r#type: T, pos: BlockPos, delay: i32, priority: TickPriority) -> Self {
        SavedTick {
            r#type,
            pos,
            delay,
            priority,
        }
    }

    /// `SavedTick.probe(T, BlockPos)` — `new SavedTick<>(type, pos, 0,
    /// TickPriority.NORMAL)`.
    pub fn probe(r#type: T, pos: BlockPos) -> Self {
        SavedTick::new(r#type, pos, 0, TickPriority::Normal)
    }
}

/// `SavedTick.codec(Codec<T>)` — the faithful codec factory.
///
/// Java builds a `MapCodec<BlockPos>` over `x`/`y`/`z`, then a record codec
/// over `i` (the type codec), `pos`, `t` (`Codec.INT`), `p`
/// (`TickPriority.CODEC`). Decode/encode therefore use the exact field order
/// `i, x, y, z, t, p` and DFU's error/partial accumulation.
pub fn saved_tick_codec<T, Ops>(
    type_codec: Arc<dyn Codec<T, Ops>>,
) -> Arc<dyn Codec<SavedTick<T>, Ops>>
where
    T: 'static + Clone + Send + Sync,
    Ops: DynamicOps + 'static,
{
    let pos_codec = record_builder::map_codec::<BlockPos, Ops>(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|pos: &BlockPos| pos.get_x()),
                "x".to_string(),
                codec::int_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|pos: &BlockPos| pos.get_y()),
                "y".to_string(),
                codec::int_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|pos: &BlockPos| pos.get_z()),
                "z".to_string(),
                codec::int_codec::<Ops>(),
            ))
            .apply(instance, Arc::new(BlockPos::new))
    });

    record_builder::create::<SavedTick<T>, Ops>(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|tick: &SavedTick<T>| tick.r#type.clone()),
                "i".to_string(),
                type_codec,
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|tick: &SavedTick<T>| tick.pos),
                pos_codec,
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|tick: &SavedTick<T>| tick.delay),
                "t".to_string(),
                codec::int_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|tick: &SavedTick<T>| tick.priority),
                "p".to_string(),
                TickPriority::codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(
                    |r#type: T, pos: BlockPos, delay: i32, priority: TickPriority| {
                        SavedTick::new(r#type, pos, delay, priority)
                    },
                ),
            )
    })
}

/// `SavedTick.filterTickListForChunk(List<SavedTick<T>>, ChunkPos)` — keep
/// only the ticks whose `BlockPos` packs to the given chunk.
///
/// Java compares `ChunkPos.pack(tick.pos()) == chunkPos.pack()`. Ticks are
/// retained in list order; the `y` coordinate is irrelevant to the match.
pub fn filter_tick_list_for_chunk<T>(
    saved_ticks: &[SavedTick<T>],
    chunk_pos: &ChunkPos,
) -> Vec<SavedTick<T>>
where
    T: Clone,
{
    let pos_key = chunk_pos.pack();
    saved_ticks
        .iter()
        .filter(|tick| ChunkPos::pack_block_pos(&tick.pos) == pos_key)
        .cloned()
        .collect()
}

// RivetTodo(#370): execution/scheduler surfaces of `net.minecraft.world.ticks`
// (`ScheduledTick`, `LevelChunkTicks`, `ProtoChunkTicks`, ...) are deferred to
// the tick-execution slice; this is the value/carry layer only.

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;

    /// Java `TickPriority` values and the clamp fallback.
    #[test]
    fn tick_priority_values_and_clamping() {
        assert_eq!(TickPriority::ExtremelyHigh.value(), -3);
        assert_eq!(TickPriority::VeryHigh.value(), -2);
        assert_eq!(TickPriority::High.value(), -1);
        assert_eq!(TickPriority::Normal.value(), 0);
        assert_eq!(TickPriority::Low.value(), 1);
        assert_eq!(TickPriority::VeryLow.value(), 2);
        assert_eq!(TickPriority::ExtremelyLow.value(), 3);
        for priority in TickPriority::all() {
            assert_eq!(TickPriority::by_value(priority.value()), priority);
        }
        // Out-of-range values clamp to the nearest end.
        assert_eq!(TickPriority::by_value(-4), TickPriority::ExtremelyHigh);
        assert_eq!(TickPriority::by_value(-100), TickPriority::ExtremelyHigh);
        assert_eq!(TickPriority::by_value(4), TickPriority::ExtremelyLow);
        assert_eq!(TickPriority::by_value(100), TickPriority::ExtremelyLow);
    }

    /// Round-trip a SavedTick through the codec over JsonOps, checking the
    /// exact encoded shape (field names/values).
    #[test]
    fn saved_tick_codec_roundtrips_json_shape() {
        use rivet_registry::core::BlockPos;
        use serde_json::json;

        let type_codec: Arc<dyn Codec<String, JsonOps>> = codec::string_codec();
        let codec = saved_tick_codec(type_codec);
        let tick = SavedTick::new(
            "minecraft:stone".to_string(),
            BlockPos::new(1, 2, 3),
            5,
            TickPriority::Low,
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &tick)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"i": "minecraft:stone", "x": 1, "y": 2, "z": 3, "t": 5, "p": 1})
        );
        let decoded = codec.parse(&JsonOps::INSTANCE, &encoded);
        let decoded = decoded.result().expect("decode should succeed");
        assert_eq!(*decoded, tick);
    }

    /// The filter keeps only this chunk's ticks, in list order, ignoring y.
    #[test]
    fn filter_tick_list_for_chunk_keeps_matching_ordered() {
        use rivet_registry::core::BlockPos;

        let chunk = ChunkPos::new(2, -3);
        let ticks = vec![
            SavedTick::new("a", BlockPos::new(33, 0, -47), 1, TickPriority::Normal), // in (2,-3)
            SavedTick::new("b", BlockPos::new(1, 100, -3), 2, TickPriority::Low),    // chunk (0,-1)
            SavedTick::new("c", BlockPos::new(32, -64, -48), 3, TickPriority::High), // in (2,-3)
        ];
        let kept = filter_tick_list_for_chunk(&ticks, &chunk);
        assert_eq!(
            kept.iter().map(|tick| tick.r#type).collect::<Vec<_>>(),
            vec!["a", "c"]
        );
    }
}
