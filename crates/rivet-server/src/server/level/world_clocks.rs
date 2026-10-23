//! Named world clocks are not game time. Source: ServerClockManager and
//! ClockTimeMarker. The host supplies actual broadcast/cache-invalidation
//! effects; this module does not invent players, packet order or a tick loop.
use rivet_protocol::protocol::game::clock_network_state::ClockNetworkState;
use rivet_registry::Identifier;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockState {
    pub total_ticks: i64,
    pub partial_tick: f32,
    pub rate: f32,
    pub paused: bool,
}
impl Default for ClockState {
    fn default() -> Self {
        Self {
            total_ticks: 0,
            partial_tick: 0.,
            rate: 1.,
            paused: false,
        }
    }
}
impl ClockState {
    fn tick(&mut self) {
        if !self.paused {
            self.partial_tick += self.rate;
            let full = rivet_util::mth::floor(self.partial_tick);
            self.partial_tick -= full as f32;
            self.total_ticks = self.total_ticks.wrapping_add(i64::from(full));
        }
    }
    pub fn network_state(self, advance_time: bool) -> ClockNetworkState {
        ClockNetworkState::new(
            self.total_ticks,
            self.partial_tick,
            if self.paused || !advance_time {
                0.
            } else {
                self.rate
            },
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ClockTimeMarker {
    pub ticks: i32,
    pub period: Option<std::num::NonZeroI32>,
    pub show_in_commands: bool,
}
impl ClockTimeMarker {
    pub fn repetition_count(self, total: i64) -> i64 {
        match self.period {
            None => i64::from(total >= i64::from(self.ticks)),
            Some(period) => {
                let period = i64::from(period.get());
                (total.wrapping_div(period)).wrapping_add(i64::from(
                    total.wrapping_rem(period) >= i64::from(self.ticks),
                ))
            }
        }
    }
    pub fn resolve_time(self, total: i64) -> i64 {
        let Some(period) = self.period else {
            return i64::from(self.ticks);
        };
        let period = i64::from(period.get());
        let duration = i64::from(self.ticks).wrapping_sub(total.wrapping_rem(period));
        total.wrapping_add(if duration > 0 {
            duration
        } else {
            period.wrapping_add(duration)
        })
    }
    pub fn occurs_at(self, total: i64) -> bool {
        i64::from(self.ticks)
            == self
                .period
                .map_or(total, |period| total.wrapping_rem(i64::from(period.get())))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClockError {
    #[error("no initialized world clock {0}")]
    UnknownClock(Identifier),
    #[error("duplicate world clock {0}")]
    DuplicateClock(Identifier),
    #[error("world clock runtime effect unavailable: {0}")]
    Effect(String),
}
/// Broadcast must apply Paper's actual per-player time projection. It receives
/// the unprojected state, including paused, rather than an invented shared packet.
pub trait ClockEffects {
    fn broadcast(&mut self, clock: &Identifier, state: ClockState) -> Result<(), ClockError>;
    fn invalidate_environment_tick_cache(&mut self) -> Result<(), ClockError>;
}
#[derive(Debug)]
struct ClockInstance {
    state: ClockState,
    markers: HashMap<Identifier, ClockTimeMarker>,
}
#[derive(Debug)]
pub struct WorldClocks {
    instances: HashMap<Identifier, ClockInstance>,
    dirty: bool,
}
impl WorldClocks {
    /// Keys must come from the resolved registry owner, not the dimension key.
    pub fn new(keys: impl IntoIterator<Item = Identifier>) -> Result<Self, ClockError> {
        let mut instances = HashMap::new();
        for key in keys {
            if instances
                .insert(
                    key.clone(),
                    ClockInstance {
                        state: ClockState::default(),
                        markers: HashMap::new(),
                    },
                )
                .is_some()
            {
                return Err(ClockError::DuplicateClock(key));
            }
        }
        Ok(Self {
            instances,
            dirty: false,
        })
    }
    fn instance_mut(&mut self, clock: &Identifier) -> Result<&mut ClockInstance, ClockError> {
        self.instances
            .get_mut(clock)
            .ok_or_else(|| ClockError::UnknownClock(clock.clone()))
    }
    pub fn state(&self, clock: &Identifier) -> Result<ClockState, ClockError> {
        self.instances
            .get(clock)
            .map(|instance| instance.state)
            .ok_or_else(|| ClockError::UnknownClock(clock.clone()))
    }
    /// Decoded persisted state is installed after registry/marker initialization.
    /// Loading does not broadcast or mark dirty, matching ClockInstance.loadFrom.
    pub fn load_state(&mut self, clock: &Identifier, state: ClockState) -> Result<(), ClockError> {
        self.instance_mut(clock)?.state = state;
        Ok(())
    }
    pub fn register_marker(
        &mut self,
        clock: &Identifier,
        key: Identifier,
        marker: ClockTimeMarker,
    ) -> Result<(), ClockError> {
        self.instance_mut(clock)?.markers.insert(key, marker);
        Ok(())
    }
    pub fn tick(&mut self, advance_time: bool) {
        if advance_time {
            for instance in self.instances.values_mut() {
                instance.state.tick();
            }
            self.dirty = true;
        }
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }
    pub fn states(&self) -> impl Iterator<Item = (&Identifier, ClockState)> {
        self.instances
            .iter()
            .map(|(key, instance)| (key, instance.state))
    }
    fn modify(
        &mut self,
        clock: &Identifier,
        effects: &mut impl ClockEffects,
        action: impl FnOnce(&mut ClockInstance),
    ) -> Result<(), ClockError> {
        let instance = self.instance_mut(clock)?;
        action(instance);
        effects.broadcast(clock, instance.state)?;
        self.dirty = true;
        effects.invalidate_environment_tick_cache()
    }
    pub fn set_total_ticks(
        &mut self,
        clock: &Identifier,
        ticks: i64,
        effects: &mut impl ClockEffects,
    ) -> Result<(), ClockError> {
        self.modify(clock, effects, |instance| {
            instance.state.total_ticks = ticks;
            instance.state.partial_tick = 0.;
        })
    }
    pub fn add_ticks(
        &mut self,
        clock: &Identifier,
        ticks: i64,
        effects: &mut impl ClockEffects,
    ) -> Result<(), ClockError> {
        self.modify(clock, effects, |instance| {
            instance.state.total_ticks = instance.state.total_ticks.wrapping_add(ticks).max(0)
        })
    }
    pub fn set_rate(
        &mut self,
        clock: &Identifier,
        rate: f32,
        effects: &mut impl ClockEffects,
    ) -> Result<(), ClockError> {
        self.modify(clock, effects, |instance| instance.state.rate = rate)
    }
    pub fn set_paused(
        &mut self,
        clock: &Identifier,
        paused: bool,
        effects: &mut impl ClockEffects,
    ) -> Result<(), ClockError> {
        self.modify(clock, effects, |instance| instance.state.paused = paused)
    }
    pub fn total_ticks_to_marker(
        &self,
        clock: &Identifier,
        marker: &Identifier,
    ) -> Result<Option<i64>, ClockError> {
        let instance = self
            .instances
            .get(clock)
            .ok_or_else(|| ClockError::UnknownClock(clock.clone()))?;
        Ok(instance
            .markers
            .get(marker)
            .map(|marker| marker.resolve_time(instance.state.total_ticks)))
    }
    pub fn move_to_marker(
        &mut self,
        clock: &Identifier,
        marker: &Identifier,
        effects: &mut impl ClockEffects,
    ) -> Result<bool, ClockError> {
        let mut moved = false;
        self.modify(clock, effects, |instance| {
            if let Some(marker) = instance.markers.get(marker) {
                instance.state.total_ticks = marker.resolve_time(instance.state.total_ticks);
                instance.state.partial_tick = 0.;
                moved = true;
            }
        })?;
        Ok(moved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_paper_clock_ticks_network_freeze_and_markers() {
        let mut cases = 0;
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/world-clock.jsonl").lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            if row["kind"] == "tick" {
                let mut state = ClockState {
                    total_ticks: row["start"].as_i64().unwrap(),
                    partial_tick: f32::from_bits(row["partial_bits"].as_i64().unwrap() as u32),
                    rate: f32::from_bits(row["rate_bits"].as_i64().unwrap() as u32),
                    paused: row["paused"].as_bool().unwrap(),
                };
                for _ in 0..row["steps"].as_u64().unwrap() {
                    state.tick();
                }
                assert_eq!(state.total_ticks, row["total"].as_i64().unwrap(), "{row}");
                assert_eq!(
                    state.partial_tick.to_bits(),
                    row["result_partial_bits"].as_i64().unwrap() as u32,
                    "{row}"
                );
                assert_eq!(
                    state.network_state(true).rate().to_bits(),
                    row["network_rate_bits"].as_i64().unwrap() as u32,
                    "{row}"
                );
                assert_eq!(
                    state.network_state(false).rate().to_bits(),
                    row["frozen_rate_bits"].as_i64().unwrap() as u32,
                    "{row}"
                );
            } else {
                let marker = ClockTimeMarker {
                    ticks: row["ticks"].as_i64().unwrap() as i32,
                    period: row["period"]
                        .as_i64()
                        .map(|v| std::num::NonZeroI32::new(v as i32).unwrap()),
                    show_in_commands: true,
                };
                let total = row["total"].as_i64().unwrap();
                assert_eq!(
                    marker.resolve_time(total),
                    row["resolve"].as_i64().unwrap(),
                    "{row}"
                );
                assert_eq!(
                    marker.repetition_count(total),
                    row["count"].as_i64().unwrap(),
                    "{row}"
                );
                assert_eq!(
                    marker.occurs_at(total),
                    row["occurs"].as_bool().unwrap(),
                    "{row}"
                );
            }
            cases += 1;
        }
        assert_eq!(cases, 216);
    }

    struct Effects {
        fail_broadcast: bool,
        states: Vec<ClockState>,
        invalidations: usize,
    }
    impl ClockEffects for Effects {
        fn broadcast(&mut self, _: &Identifier, state: ClockState) -> Result<(), ClockError> {
            if self.fail_broadcast {
                return Err(ClockError::Effect("transport".into()));
            }
            self.states.push(state);
            Ok(())
        }
        fn invalidate_environment_tick_cache(&mut self) -> Result<(), ClockError> {
            self.invalidations += 1;
            Ok(())
        }
    }
    #[test]
    fn world_clock_mutation_retains_source_effect_order_and_distinct_clocks() {
        let day = Identifier::parse("minecraft:overworld");
        let end = Identifier::parse("minecraft:the_end");
        let mut clocks = WorldClocks::new([day.clone(), end.clone()]).unwrap();
        let mut effects = Effects {
            fail_broadcast: true,
            states: Vec::new(),
            invalidations: 0,
        };
        assert!(clocks.set_total_ticks(&day, 42, &mut effects).is_err());
        assert_eq!(clocks.state(&day).unwrap().total_ticks, 42);
        assert!(!clocks.is_dirty());
        assert_eq!(effects.invalidations, 0);
        effects.fail_broadcast = false;
        clocks.add_ticks(&day, i64::MAX, &mut effects).unwrap();
        assert_eq!(clocks.state(&day).unwrap().total_ticks, 0);
        assert!(clocks.is_dirty());
        assert_eq!(effects.invalidations, 1);
        clocks.set_rate(&day, 0.25, &mut effects).unwrap();
        clocks.set_paused(&end, true, &mut effects).unwrap();
        clocks.mark_saved();
        clocks.tick(false);
        assert!(!clocks.is_dirty());
        for _ in 0..4 {
            clocks.tick(true);
        }
        assert_eq!(clocks.state(&day).unwrap().total_ticks, 1);
        assert_eq!(clocks.state(&end).unwrap().total_ticks, 0);
        assert!(
            !clocks
                .move_to_marker(&day, &Identifier::parse("minecraft:missing"), &mut effects)
                .unwrap()
        );
        assert_eq!(
            effects.invalidations, 4,
            "missing markers still execute source modify effects"
        );
    }
}
