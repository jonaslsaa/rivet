//! Per-world ServerClockManager ownership/bootstrap. Full registry clock keys
//! and timeline markers are independent of the world's default clock. Network
//! timeline data retains those fields; no filtered attribute track is used.
use super::world_clocks::{ClockError, ClockState, ClockTimeMarker, WorldClocks};
use rivet_nbt::{
    compound_tag::CompoundTag, nbt_accounter::NbtAccounter, nbt_io, nbt_ops::NbtOps, tag::Tag,
};
use rivet_protocol::protocol::game::{
    clientbound_set_time::ClientboundSetTimePacket, clock_network_state::ClockNetworkState,
};
use rivet_registry::{
    Identifier, RegistryAccess, generated::registry_data::SYNCHRONIZED_NBT, holder::Holder,
    registries,
};
use rivet_serialization::{
    codec::{self, Codec},
    data_result::DataResult,
};
use rivet_util::DataInputStream;
use std::{
    collections::{HashMap, HashSet},
    num::NonZeroI32,
    path::Path,
    sync::Arc,
};

#[derive(Debug, thiserror::Error)]
pub enum LevelClockError {
    #[error(transparent)]
    Clock(#[from] ClockError),
    #[error("world clock registry/bootstrap unavailable: {0}")]
    Registry(String),
    #[error("world_clocks DataVersion {version} requires unavailable migration to {expected}")]
    DataFixUnavailable { version: i32, expected: i32 },
    #[error("global affects-all-worlds clock ownership requires the actual server-global manager")]
    GlobalClockManagerUnavailable,
    #[error("world clock saved-data encoding failed: {0}")]
    Encode(String),
}

/// ServerPlayer's actual initial field values are relativeTime=true,
/// timeOffset=0. Future per-player time APIs must retain their actual values
/// here; this is not inferred from a packet or the world's game time.
#[derive(Debug, Clone, Copy)]
pub struct PlayerClockProjection {
    pub relative_time: bool,
    pub time_offset: i64,
}
impl Default for PlayerClockProjection {
    fn default() -> Self {
        Self {
            relative_time: true,
            time_offset: 0,
        }
    }
}

pub struct LevelClockRuntime {
    clocks: WorldClocks,
    default_clock: Option<Identifier>,
    advance_time: bool,
}

/// The actual connection registry keys and dense IDs, shared by the clock
/// manager and set-time wire encoder. Old `day`/`weather` placeholder keys are
/// not real WorldClock definitions (day is a timeline, not a clock).
pub fn pinned_clock_registry_access() -> Result<RegistryAccess, LevelClockError> {
    let mut builder = rivet_registry::RegistryBuilder::new(&*registries::WORLD_CLOCK);
    for key in clock_keys()? {
        builder.register(
            &rivet_registry::ResourceKey::create(&*registries::WORLD_CLOCK, key),
            Arc::new(rivet_registry::registries::WorldClock),
            rivet_registry::RegistrationInfo::BUILT_IN,
        );
    }
    Ok(RegistryAccess::from_single_registry(
        (*registries::WORLD_CLOCK).clone(),
        builder.freeze(),
    ))
}

impl LevelClockRuntime {
    /// Source default initialization over every resolved clock definition.
    /// Appropriate for a genuinely new/synthetic world, not a read failure
    /// fallback supplied by a join packet builder.
    pub fn new(dimension_type: &Identifier, advance_time: bool) -> Result<Self, LevelClockError> {
        let keys = clock_keys()?;
        let mut clocks = WorldClocks::new(keys.clone())?;
        for (name, bytes) in elements("minecraft:timeline")? {
            let timeline = payload(bytes)?;
            let clock = Identifier::parse(
                timeline
                    .get_string("clock")
                    .ok_or_else(|| LevelClockError::Registry(format!("{name}: missing clock")))?,
            );
            clocks.state(&clock)?;
            let period = timeline
                .get_int("period_ticks")
                .map(|value| {
                    NonZeroI32::new(value)
                        .filter(|value| value.get() > 0)
                        .ok_or_else(|| LevelClockError::Registry(format!("{name}: invalid period")))
                })
                .transpose()?;
            if let Some(markers) = timeline.get_compound("time_markers") {
                for (key, value) in &markers.tags {
                    let (ticks, show_in_commands) = match value {
                        Tag::Compound(record) => (
                            record.get_int("ticks").ok_or_else(|| {
                                LevelClockError::Registry(format!("{name}: invalid marker"))
                            })?,
                            record.get_boolean_or("show_in_commands", false),
                        ),
                        value => (
                            value
                                .as_number()
                                .map(|value| value.int_value())
                                .ok_or_else(|| {
                                    LevelClockError::Registry(format!("{name}: invalid marker"))
                                })?,
                            false,
                        ),
                    };
                    clocks.register_marker(
                        &clock,
                        Identifier::parse(key),
                        ClockTimeMarker {
                            ticks,
                            period,
                            show_in_commands,
                        },
                    )?;
                }
            }
        }
        let dimension = element("minecraft:dimension_type", dimension_type)?;
        let default_clock = dimension
            .get_string("default_clock")
            .map(|name| Identifier::parse(name));
        if let Some(key) = &default_clock {
            clocks.state(key)?;
        }
        clocks.mark_created();
        Ok(Self {
            clocks,
            default_clock,
            advance_time,
        })
    }

    /// Paper's default affectsAllWorlds=false uses per-dimension SavedData.
    /// A configured global owner must be bound separately, not silently read
    /// from this dimension's file or replaced by a new local clock manager.
    pub fn load(
        dimension_path: &Path,
        dimension_type: &Identifier,
        advance_time: bool,
        affects_all_worlds: bool,
    ) -> Result<Self, LevelClockError> {
        if affects_all_worlds {
            return Err(LevelClockError::GlobalClockManagerUnavailable);
        }
        let mut runtime = Self::new(dimension_type, advance_time)?;
        if let Some(data) = read_saved(dimension_path)?
            && let Some(states) = packed_codec().parse(&NbtOps, &data).result_or_partial(
                |error| tracing::error!(%error,"Failed to parse world clock SavedData"),
            )
        {
            for (clock, state) in states {
                runtime.clocks.load_state(&clock, state)?;
            }
            runtime.clocks.mark_saved();
        }
        Ok(runtime)
    }

    pub fn clocks(&self) -> &WorldClocks {
        &self.clocks
    }
    pub fn clocks_mut(&mut self) -> &mut WorldClocks {
        &mut self.clocks
    }
    pub fn default_clock(&self) -> Option<&Identifier> {
        self.default_clock.as_ref()
    }
    pub fn advance_time(&self) -> bool {
        self.advance_time
    }
    /// Bind a real GameRules.ADVANCE_TIME change before the next tick/sync.
    pub fn set_advance_time(&mut self, advance_time: bool) {
        self.advance_time = advance_time;
    }
    pub fn tick(&mut self) {
        self.clocks.tick(self.advance_time);
    }

    pub fn full_sync_packet(
        &self,
        access: &RegistryAccess,
        game_time: i64,
        player: Option<PlayerClockProjection>,
    ) -> Result<ClientboundSetTimePacket, LevelClockError> {
        let registry = access.lookup(&*registries::WORLD_CLOCK).ok_or_else(|| {
            LevelClockError::Registry("missing connection WORLD_CLOCK registry".into())
        })?;
        let names = registry.key_set();
        if names.len() != self.clocks.states().count() {
            return Err(LevelClockError::Registry(
                "connection and level clock definitions differ".into(),
            ));
        }
        // Distinct map keys are commutative on the wire. Use registry order,
        // preserving the established golden, not a claim of JVM identity-hash
        // HashMap iteration order or gameplay-observable sampling order.
        let mut updates = Vec::with_capacity(names.len());
        for (id, key) in names.into_iter().enumerate() {
            let state = self.clocks.state(&key)?;
            let network = self.project_state(&key, state, player);
            updates.push((
                Holder::reference(registry.registry_id(), id as u32),
                network,
            ));
        }
        Ok(ClientboundSetTimePacket::new(game_time, updates))
    }

    pub fn project_state(
        &self,
        key: &Identifier,
        state: ClockState,
        player: Option<PlayerClockProjection>,
    ) -> ClockNetworkState {
        if self.default_clock.as_ref() == Some(key)
            && let Some(player) = player
        {
            let ticks = if player.relative_time {
                state.total_ticks
            } else {
                state.total_ticks.wrapping_sub(state.total_ticks % 24_000)
            }
            .wrapping_add(player.time_offset);
            return ClockNetworkState::new(
                ticks,
                state.partial_tick,
                if !player.relative_time || state.paused || !self.advance_time {
                    0.0
                } else {
                    state.rate
                },
            );
        }
        state.network_state(self.advance_time)
    }

    /// Standard SavedData envelope. Saving/acknowledging it belongs to the
    /// writable storage owner; merely constructing it never clears dirty.
    pub fn saved_data(&self) -> Result<CompoundTag, LevelClockError> {
        let states = self
            .clocks
            .states()
            .map(|(key, state)| (key.clone(), state))
            .collect();
        // SavedDataStorage's save path uses getOrThrow, unlike its load path's
        // resultOrPartial. Never turn an invalid live rate into a partial save.
        let encoded = packed_codec().encode_start(&NbtOps, &states);
        let data = encoded.result().cloned().ok_or_else(|| {
            LevelClockError::Encode(encoded.error_ref().map_or_else(
                || "PackedClockStates".to_owned(),
                |error| error.message().to_owned(),
            ))
        })?;
        let mut root = CompoundTag::new();
        root.put_int("DataVersion", super::region_backed::EXPECTED_DATA_VERSION);
        root.put("data".into(), data);
        Ok(root)
    }
}

fn packed_codec() -> Arc<dyn Codec<HashMap<Identifier, ClockState>, NbtOps>> {
    let keys: HashSet<_> = clock_keys()
        .expect("pinned world clock registry")
        .into_iter()
        .collect();
    let key = codec::validate(
        rivet_registry::identifier::identifier_codec::<NbtOps>(),
        Arc::new(move |key: &Identifier| {
            if keys.contains(key) {
                DataResult::success(key.clone())
            } else {
                DataResult::error(format!("Unknown world_clock {key}"))
            }
        }),
    );
    codec::unbounded_map(key, ClockState::codec::<NbtOps>())
}

fn clock_keys() -> Result<Vec<Identifier>, LevelClockError> {
    Ok(elements("minecraft:world_clock")?
        .iter()
        .map(|(key, _)| Identifier::parse(key))
        .collect())
}
fn elements(
    registry: &str,
) -> Result<rivet_registry::generated::registry_data::SynchronizedRegistryElements, LevelClockError>
{
    SYNCHRONIZED_NBT
        .iter()
        .find(|(key, _)| *key == registry)
        .map(|(_, value)| *value)
        .ok_or_else(|| LevelClockError::Registry(registry.into()))
}
fn payload(bytes: &[u8]) -> Result<CompoundTag, LevelClockError> {
    match nbt_io::read_any_tag(
        &mut DataInputStream::new(bytes),
        &mut NbtAccounter::unlimited_heap(),
    )
    .map_err(|error| LevelClockError::Registry(error.to_string()))?
    {
        Tag::Compound(data) => Ok(data),
        _ => Err(LevelClockError::Registry("noncompound payload".into())),
    }
}
fn element(registry: &str, key: &Identifier) -> Result<CompoundTag, LevelClockError> {
    let bytes = elements(registry)?
        .iter()
        .find(|(name, _)| Identifier::parse(name) == *key)
        .ok_or_else(|| LevelClockError::Registry(format!("{registry}: {key}")))?
        .1;
    payload(bytes)
}
fn read_saved(dimension: &Path) -> Result<Option<Tag>, LevelClockError> {
    let path = dimension.join("data/minecraft/world_clocks.dat");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            tracing::error!(%error,path=%path.display(),"Error loading saved world clocks");
            return Ok(None);
        }
    };
    let root = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if bytes.starts_with(&[0x1f, 0x8b]) {
            nbt_io::read_compressed(&bytes[..], &mut NbtAccounter::unlimited_heap())
        } else {
            nbt_io::read(
                &mut DataInputStream::new(bytes.as_slice()),
                &mut NbtAccounter::unlimited_heap(),
            )
        }
    }));
    let root = match root {
        Ok(Ok(root)) => root,
        _ => {
            tracing::error!(path=%path.display(),"Error loading world clock SavedData NBT");
            return Ok(None);
        }
    };
    let version = root.get_int_or("DataVersion", 1343);
    let expected = super::region_backed::EXPECTED_DATA_VERSION;
    if version < expected {
        return Err(LevelClockError::DataFixUnavailable { version, expected });
    }
    Ok(root.get("data").cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::tag_parser::parse_compound_fully;

    #[test]
    fn actual_paper_clock_saved_codec_and_player_full_sync_projection() {
        let access = pinned_clock_registry_access().unwrap();
        let mut cases = 0;
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/world-clock-bootstrap.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            if row["kind"] == "codec" {
                let data = Tag::Compound(
                    parse_compound_fully(row["input_snbt"].as_str().unwrap()).unwrap(),
                );
                let result = packed_codec().parse(&NbtOps, &data);
                assert_eq!(result.is_error(), row["error"].as_bool().unwrap(), "{line}");
                let partial = result.result_or_partial(|_| {});
                match (partial, row["output_snbt"].as_str()) {
                    (Some(states), Some(expected)) => assert_eq!(
                        packed_codec()
                            .encode_start(&NbtOps, &states)
                            .result_or_partial(|_| {})
                            .unwrap(),
                        Tag::Compound(parse_compound_fully(expected).unwrap()),
                        "{line}"
                    ),
                    (None, None) => {}
                    _ => panic!("partial result mismatch: {line}"),
                }
            } else {
                let mut runtime = LevelClockRuntime::new(
                    &Identifier::parse(row["dimension"].as_str().unwrap()),
                    row["advance"].as_bool().unwrap(),
                )
                .unwrap();
                for key in clock_keys().unwrap() {
                    runtime
                        .clocks_mut()
                        .load_state(
                            &key,
                            ClockState {
                                total_ticks: row["ticks"].as_i64().unwrap(),
                                partial_tick: 0.75,
                                rate: 0.25,
                                paused: row["paused"].as_bool().unwrap(),
                            },
                        )
                        .unwrap();
                }
                let packet = runtime
                    .full_sync_packet(
                        &access,
                        12345,
                        Some(PlayerClockProjection {
                            relative_time: row["relative"].as_bool().unwrap(),
                            time_offset: row["offset"].as_i64().unwrap(),
                        }),
                    )
                    .unwrap();
                assert_eq!(packet.game_time(), row["game_time"].as_i64().unwrap());
                let names = access.lookup(&*registries::WORLD_CLOCK).unwrap().key_set();
                for ((_, state), name) in packet.clock_updates().iter().zip(names) {
                    let expected = &row["updates"][name.to_string()];
                    assert_eq!(
                        state.total_ticks(),
                        expected["total"].as_i64().unwrap(),
                        "{line}: {name}"
                    );
                    assert_eq!(
                        state.partial_tick().to_bits(),
                        expected["partial_bits"].as_i64().unwrap() as u32,
                        "{line}"
                    );
                    assert_eq!(
                        state.rate().to_bits(),
                        expected["rate_bits"].as_i64().unwrap() as u32,
                        "{line}"
                    );
                }
            }
            cases += 1;
        }
        assert_eq!(cases, 302);
    }

    #[test]
    fn saved_clock_boot_preserves_state_dirty_boundaries_and_actual_markers() {
        let root = tempfile::tempdir().unwrap();
        let dimension = Identifier::parse("minecraft:overworld");
        let mut runtime = LevelClockRuntime::load(root.path(), &dimension, true, false).unwrap();
        assert!(
            runtime.clocks().is_dirty(),
            "computeIfAbsent constructs and sets dirty"
        );
        assert_eq!(
            runtime
                .clocks()
                .total_ticks_to_marker(&dimension, &Identifier::parse("minecraft:night"))
                .unwrap(),
            Some(13000)
        );
        runtime
            .clocks_mut()
            .load_state(
                &dimension,
                ClockState {
                    total_ticks: 13000,
                    partial_tick: 0.5,
                    rate: 0.25,
                    paused: false,
                },
            )
            .unwrap();
        let data = runtime.saved_data().unwrap();
        assert!(
            runtime.clocks().is_dirty(),
            "encoding is not a save acknowledgement"
        );
        let folder = root.path().join("data/minecraft");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("world_clocks.dat");
        nbt_io::write_compressed(&data, std::fs::File::create(&path).unwrap()).unwrap();
        let mut loaded = LevelClockRuntime::load(root.path(), &dimension, false, false).unwrap();
        assert!(!loaded.clocks().is_dirty());
        loaded.tick();
        assert_eq!(
            loaded.clocks().state(&dimension).unwrap().total_ticks,
            13000
        );
        loaded.set_advance_time(true);
        loaded.tick();
        assert_eq!(
            loaded.clocks().state(&dimension).unwrap().partial_tick,
            0.75
        );
        assert!(loaded.clocks().is_dirty());
        let mut old = data;
        old.put_int("DataVersion", 1);
        nbt_io::write_compressed(&old, std::fs::File::create(&path).unwrap()).unwrap();
        assert!(matches!(
            LevelClockRuntime::load(root.path(), &dimension, true, false),
            Err(LevelClockError::DataFixUnavailable { .. })
        ));
        assert!(matches!(
            LevelClockRuntime::load(root.path(), &dimension, true, true),
            Err(LevelClockError::GlobalClockManagerUnavailable)
        ));
        loaded
            .clocks_mut()
            .load_state(
                &dimension,
                ClockState {
                    rate: 0.0,
                    ..ClockState::default()
                },
            )
            .unwrap();
        assert!(
            matches!(loaded.saved_data(), Err(LevelClockError::Encode(_))),
            "save uses source getOrThrow, never a partial/defaulted live rate"
        );
    }
}
