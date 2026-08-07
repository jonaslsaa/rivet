//! Port of `net.minecraft.world.clock.ClockNetworkState` (#87) — the
//! `(totalTicks, partialTick, rate)` value the set-time clock-update map
//! carries.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/clock/ClockNetworkState.java`. `STREAM_CODEC` is a VarLong then two
//! floats. Pure value type; the wire codec crosses to `rivet-protocol` per
//! OWNERSHIP.md §Registries (the clock unit owns the record's full surface in
//! `rivet-world`).

use crate::codec::{StreamCodec, composite_3, float, var_long};
use crate::friendly_byte_buf::FriendlyByteBuf;

/// `ClockNetworkState` — the record `(totalTicks, partialTick, rate)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockNetworkState {
    /// `totalTicks`.
    total_ticks: i64,
    /// `partialTick`.
    partial_tick: f32,
    /// `rate`.
    rate: f32,
}

impl ClockNetworkState {
    /// The record's canonical constructor.
    pub fn new(total_ticks: i64, partial_tick: f32, rate: f32) -> Self {
        ClockNetworkState {
            total_ticks,
            partial_tick,
            rate,
        }
    }

    /// `ClockNetworkState.totalTicks()`.
    pub fn total_ticks(&self) -> i64 {
        self.total_ticks
    }

    /// `ClockNetworkState.partialTick()`.
    pub fn partial_tick(&self) -> f32 {
        self.partial_tick
    }

    /// `ClockNetworkState.rate()`.
    pub fn rate(&self) -> f32 {
        self.rate
    }

    /// `ClockNetworkState.STREAM_CODEC` — `VAR_LONG`, `FLOAT`, `FLOAT`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClockNetworkState> {
        composite_3(
            var_long(),
            ClockNetworkState::total_ticks,
            float(),
            ClockNetworkState::partial_tick,
            float(),
            ClockNetworkState::rate,
            ClockNetworkState::new,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn stream_codec_round_trips() {
        let value = ClockNetworkState::new(0, 0.0, 1.0);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClockNetworkState::stream_codec()
            .encode(&mut out, &value)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ClockNetworkState::stream_codec()
                .decode(&mut input)
                .unwrap(),
            value
        );
        assert_eq!(input.readable_bytes(), 0);
    }
}
