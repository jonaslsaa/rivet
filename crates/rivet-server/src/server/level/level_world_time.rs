//! ServerLevel.tickTime and its actual per-world scheduled-events SavedData.
//! Named clocks are separate; ADVANCE_TIME does not gate this game-time clock.
use super::ServerLevel;
use rivet_nbt::{compound_tag::CompoundTag, nbt_accounter::NbtAccounter, nbt_io, tag::Tag};
use rivet_registry::Identifier;
use rivet_util::DataInputStream;
use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap},
    path::Path,
};

#[derive(Debug, thiserror::Error)]
pub enum WorldTimeError {
    #[error("the world has no installed game-time/scheduled-events owner")]
    NotInstalled,
    #[error("scheduled_events DataVersion {stored} requires unavailable migration to {current}")]
    DataFixUnavailable { stored: i32, current: i32 },
    #[error("scheduled function execution unavailable: {0}")]
    ExecutionUnavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionCallback {
    Function(Identifier),
    FunctionTag(Identifier),
}

/// The real function manager resolves and executes through its game-loop
/// command sender. Absence of that manager is not an empty function registry.
pub trait ScheduledFunctionExecutor {
    fn execute_function(
        &mut self,
        world: &mut ServerLevel,
        id: &Identifier,
    ) -> Result<(), WorldTimeError>;
    fn execute_function_tag(
        &mut self,
        world: &mut ServerLevel,
        id: &Identifier,
    ) -> Result<(), WorldTimeError>;
}
pub struct UnavailableScheduledFunctions;
impl ScheduledFunctionExecutor for UnavailableScheduledFunctions {
    fn execute_function(
        &mut self,
        _: &mut ServerLevel,
        id: &Identifier,
    ) -> Result<(), WorldTimeError> {
        Err(WorldTimeError::ExecutionUnavailable(format!(
            "function {id}: no native ServerFunctionManager"
        )))
    }
    fn execute_function_tag(
        &mut self,
        _: &mut ServerLevel,
        id: &Identifier,
    ) -> Result<(), WorldTimeError> {
        Err(WorldTimeError::ExecutionUnavailable(format!(
            "function tag {id}: no native ServerFunctionManager"
        )))
    }
}

#[derive(Debug, Clone)]
struct Event {
    time: i64,
    sequence: u64,
    id: String,
    callback: FunctionCallback,
}
impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        (self.time, self.sequence) == (other.time, other.sequence)
    }
}
impl Eq for Event {}
impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Event {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max heap; Paper is signed time, then UnsignedLong ID.
        (other.time, other.sequence).cmp(&(self.time, self.sequence))
    }
}

pub struct LevelWorldTime {
    tick_time: bool,
    queue: BinaryHeap<Event>,
    events: HashMap<String, HashMap<i64, u64>>,
    sequence: u64,
    dirty: bool,
}
impl LevelWorldTime {
    pub fn new(tick_time: bool) -> Self {
        // SavedDataStorage.computeIfAbsent -> set(new TimerQueue) marks dirty.
        Self {
            tick_time,
            queue: BinaryHeap::new(),
            events: HashMap::new(),
            sequence: 0,
            dirty: true,
        }
    }
    pub fn load(dimension: &Path, tick_time: bool) -> Result<Self, WorldTimeError> {
        let path = dimension.join("data/minecraft/scheduled_events.dat");
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::error!(%error,path=%path.display(),"Error loading scheduled events SavedData");
                }
                return Ok(Self::new(tick_time));
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
                tracing::error!(path=%path.display(),"Error loading scheduled events NBT");
                return Ok(Self::new(tick_time));
            }
        };
        let stored = root.get_int_or("DataVersion", 1343);
        let current = super::super::region_backed::EXPECTED_DATA_VERSION;
        if stored < current {
            return Err(WorldTimeError::DataFixUnavailable { stored, current });
        }
        let events = root
            .get_compound("data")
            .and_then(|data| data.get("events"));
        let Some(events) = events else {
            tracing::error!(path=%path.display(),"Failed to parse scheduled events SavedData");
            return Ok(Self::new(tick_time));
        };
        let mut result = Self::new(tick_time);
        // Packed constructor calls schedule for each parsed entry. Nonempty
        // successful loads therefore become dirty, unlike an empty load.
        result.dirty = false;
        // NbtOps.getStream accepts every CollectionTag, not just ListTag.
        // Numeric arrays contain no possible event compounds: their members
        // are invalid entries of a valid collection and listOf yields its
        // partial empty result, rather than failing the entire SavedData.
        let entries = match events {
            Tag::List(list) => &list.list,
            Tag::ByteArray(_) | Tag::IntArray(_) | Tag::LongArray(_) => {
                let nonempty = match events {
                    Tag::ByteArray(a) => !a.data.is_empty(),
                    Tag::IntArray(a) => !a.data.is_empty(),
                    Tag::LongArray(a) => !a.data.is_empty(),
                    _ => unreachable!(),
                };
                if nonempty {
                    tracing::error!(path=%path.display(),"Invalid scheduled event elements dropped by partial codec result");
                }
                return Ok(result);
            }
            _ => {
                tracing::error!(path=%path.display(),"Failed to parse scheduled events SavedData: not a collection");
                return Ok(Self::new(tick_time));
            }
        };
        for event in entries {
            let decoded = decode_event(event);
            match decoded {
                Some((id, time, callback)) => {
                    result.schedule(id, time, callback);
                }
                None => {
                    tracing::error!(path=%path.display(),"Invalid scheduled event dropped by partial codec result")
                }
            }
        }
        Ok(result)
    }
    pub fn schedule(&mut self, id: String, time: i64, callback: FunctionCallback) {
        let row = self.events.entry(id.clone()).or_default();
        if row.contains_key(&time) {
            return;
        }
        self.sequence = self.sequence.wrapping_add(1);
        row.insert(time, self.sequence);
        self.queue.push(Event {
            time,
            sequence: self.sequence,
            id,
            callback,
        });
        self.dirty = true;
    }
    pub fn remove(&mut self, id: &str) -> usize {
        let row = self.events.remove(id).unwrap_or_default();
        // An overdue tick removes (id,currentTick), not (id,triggerTime).
        // Preserve the source's resulting stale/missing table entries rather
        // than treating the heap as the table's authoritative replacement.
        self.queue
            .retain(|event| event.id != id || row.get(&event.time) != Some(&event.sequence));
        self.dirty = true;
        row.len()
    }
    fn pop_due(&mut self, current: i64) -> Option<FunctionCallback> {
        if self.queue.peek()?.time > current {
            return None;
        }
        let event = self.queue.pop().unwrap();
        if let Some(row) = self.events.get_mut(&event.id) {
            row.remove(&current);
            if row.is_empty() {
                self.events.remove(&event.id);
            }
        }
        self.dirty = true;
        Some(event.callback)
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }
    pub fn saved_data(&self) -> CompoundTag {
        let mut events: Vec<_> = self.queue.iter().collect();
        events.sort_by_key(|event| (event.time, event.sequence));
        let packed: Vec<_> = events
            .into_iter()
            .map(|event| {
                let mut callback = CompoundTag::new();
                let (kind, id) = match &event.callback {
                    FunctionCallback::Function(id) => ("minecraft:function", id),
                    FunctionCallback::FunctionTag(id) => ("minecraft:function_tag", id),
                };
                callback.put_string("type", kind);
                callback.put_string("id", &id.to_string());
                let mut packed = CompoundTag::new();
                packed.put_long("trigger_time", event.time);
                packed.put_string("id", &event.id);
                packed.put("callback".into(), Tag::Compound(callback));
                Tag::Compound(packed)
            })
            .collect();
        let mut data = CompoundTag::new();
        data.put(
            "events".into(),
            Tag::List(rivet_nbt::list_tag::ListTag::with_list(packed)),
        );
        let mut root = CompoundTag::new();
        root.put_int(
            "DataVersion",
            super::super::region_backed::EXPECTED_DATA_VERSION,
        );
        root.put("data".into(), Tag::Compound(data));
        root
    }
}
fn decode_event(event: &Tag) -> Option<(String, i64, FunctionCallback)> {
    let Tag::Compound(event) = event else {
        return None;
    };
    let id = event.get_string("id")?.to_owned();
    let time = event.get_long("trigger_time")?;
    let callback = event.get_compound("callback")?;
    let name = Identifier::try_parse_result(callback.get_string("id")?).ok()??;
    let kind = Identifier::try_parse_result(callback.get_string("type")?).ok()??;
    let callback = match kind.to_string().as_str() {
        "minecraft:function" => FunctionCallback::Function(name),
        "minecraft:function_tag" => FunctionCallback::FunctionTag(name),
        _ => return None,
    };
    Some((id, time, callback))
}

impl ServerLevel {
    /// Preserve the existing complete level.dat and save the same tick's
    /// scheduled events/clocks before committing Data.Time. Validation occurs
    /// first. Like Paper's separate files this is not a multi-file transaction:
    /// an error retains already-completed writes and the remaining dirty owners.
    /// Missing metadata is explicit and still permits the deliberate fresh
    /// identity/SavedData-only save; no incomplete level.dat is fabricated.
    #[cfg(unix)]
    pub fn save_world_saved_data_and_time(
        &mut self,
    ) -> std::io::Result<super::super::level_storage_access::LevelDataTimeSave> {
        use super::super::level_storage_access::LevelDataTimeSave;
        let storage = self.writable_storage.clone().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "level has no writable world storage",
            )
        })?;
        let prepared = storage.prepare_existing_level_data_time(self.game_time())?;
        self.save_world_saved_data()?;
        match prepared {
            Some(prepared) => {
                prepared.commit()?;
                Ok(LevelDataTimeSave::Written)
            }
            None => Ok(LevelDataTimeSave::MissingLevelData),
        }
    }
    /// Save registered per-world clock/timer data under the retained writable
    /// dimension lock. Encoding alone never acknowledges a successful write;
    /// each owner is marked clean only after its gzip file has completed.
    #[cfg(unix)]
    pub fn save_world_saved_data(&mut self) -> std::io::Result<()> {
        let storage = self.writable_storage.as_ref().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "level has no writable world storage",
            )
        })?;
        let folder = storage
            .dimension_path(self.dimension())
            .join("data/minecraft");
        if let Some(clocks) = self.clock_runtime.as_mut()
            && clocks.clocks().is_dirty()
        {
            let data = clocks.saved_data().map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
            })?;
            std::fs::create_dir_all(&folder)?;
            nbt_io::write_compressed(
                &data,
                std::fs::File::create(folder.join("world_clocks.dat"))?,
            )?;
            clocks.clocks_mut().mark_saved();
        }
        if let Some(time) = self.world_time.as_mut()
            && time.is_dirty()
        {
            std::fs::create_dir_all(&folder)?;
            nbt_io::write_compressed(
                &time.saved_data(),
                std::fs::File::create(folder.join("scheduled_events.dat"))?,
            )?;
            time.mark_saved();
        }
        Ok(())
    }

    pub fn install_world_time(&mut self, runtime: LevelWorldTime) {
        self.world_time = Some(runtime);
    }
    pub fn world_time(&self) -> Option<&LevelWorldTime> {
        self.world_time.as_ref()
    }
    pub fn world_time_mut(&mut self) -> Option<&mut LevelWorldTime> {
        self.world_time.as_mut()
    }
    /// Actual tickTime body. The owning world tick gates this at runsNormally.
    /// A callback failure retains gameTime/dequeued-event effects, as Java does.
    pub fn tick_world_time(
        &mut self,
        functions: &mut impl ScheduledFunctionExecutor,
    ) -> Result<(), WorldTimeError> {
        let runtime = self
            .world_time
            .as_ref()
            .ok_or(WorldTimeError::NotInstalled)?;
        if !runtime.tick_time {
            return Ok(());
        }
        let current = self.game_time().wrapping_add(1);
        self.set_game_time(current);
        loop {
            let callback = self.world_time.as_mut().unwrap().pop_due(current);
            match callback {
                None => return Ok(()),
                Some(FunctionCallback::Function(id)) => functions.execute_function(self, &id)?,
                Some(FunctionCallback::FunctionTag(id)) => {
                    functions.execute_function_tag(self, &id)?
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn scheduled_queue_matches_actual_paper_order_reentrancy_and_failure() {
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/timer-queue.jsonl").lines()
        {
            let row: Value = serde_json::from_str(line).unwrap();
            let mut queue = LevelWorldTime::new(true);
            let mut log = Vec::new();
            let mut snapshots = Vec::new();
            for command in row["commands"].as_array().unwrap() {
                match command[0].as_str().unwrap() {
                    "schedule" => queue.schedule(
                        command[1].as_str().unwrap().into(),
                        command[2].as_i64().unwrap(),
                        FunctionCallback::Function(Identifier::with_default_namespace(
                            command[3].as_str().unwrap(),
                        )),
                    ),
                    "remove" => log.push(json!([
                        "removed",
                        queue.remove(command[1].as_str().unwrap())
                    ])),
                    "tick" => {
                        let time = command[1].as_i64().unwrap();
                        while let Some(FunctionCallback::Function(id)) = queue.pop_due(time) {
                            let name = id.path();
                            log.push(json!([name, time]));
                            match name {
                                "reschedule" => queue.schedule(
                                    "child".into(),
                                    time,
                                    FunctionCallback::Function(Identifier::with_default_namespace(
                                        "child",
                                    )),
                                ),
                                "remove" => log.push(json!(["removed", queue.remove("same")])),
                                "throw" => {
                                    log.push(json!(["error"]));
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => unreachable!(),
                }
                let mut ids: Vec<_> = queue.events.keys().cloned().collect();
                ids.sort();
                snapshots.push(json!({"ids":ids,"dirty":queue.is_dirty()}));
            }
            assert_eq!(json!(log), row["log"], "{line}");
            assert_eq!(json!(snapshots), row["snapshots"], "{line}");
        }
    }

    #[test]
    fn scheduled_saved_data_matches_actual_paper_codec_partial_and_duplicate_rules() {
        use rivet_nbt::tag_parser::parse_compound_fully;
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("data/minecraft");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("scheduled_events.dat");
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/timer-queue-codec.jsonl")
                .lines()
        {
            let row: Value = serde_json::from_str(line).unwrap();
            let mut input = CompoundTag::new();
            input.put_int(
                "DataVersion",
                super::super::super::region_backed::EXPECTED_DATA_VERSION,
            );
            input.put(
                "data".into(),
                Tag::Compound(parse_compound_fully(row["input"].as_str().unwrap()).unwrap()),
            );
            nbt_io::write_compressed(&input, std::fs::File::create(&path).unwrap()).unwrap();
            let loaded = LevelWorldTime::load(root.path(), true).unwrap();
            if row["present"].as_bool().unwrap() {
                assert_eq!(loaded.is_dirty(), row["dirty"].as_bool().unwrap(), "{line}");
                let encoded = parse_compound_fully(row["encoded"].as_str().unwrap()).unwrap();
                assert_eq!(
                    loaded.saved_data().get_compound("data"),
                    Some(&encoded),
                    "{line}"
                );
            } else {
                assert!(loaded.queue.is_empty());
                assert!(
                    loaded.is_dirty(),
                    "computeIfAbsent installs a new owner after failed parsing"
                );
            }
        }
        let mut old = CompoundTag::new();
        old.put_int("DataVersion", 1);
        nbt_io::write_compressed(&old, std::fs::File::create(path).unwrap()).unwrap();
        assert!(matches!(
            LevelWorldTime::load(root.path(), true),
            Err(WorldTimeError::DataFixUnavailable { stored: 1, .. })
        ));
    }

    #[test]
    fn saved_events_load_into_tick_time_without_conflating_named_clocks() {
        use super::super::ServerLevelConfig;
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("data/minecraft");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scheduled_events.dat");
        let mut queue = LevelWorldTime::new(true);
        queue.schedule(
            "demo".into(),
            42,
            FunctionCallback::Function(Identifier::with_default_namespace("demo")),
        );
        nbt_io::write_compressed(&queue.saved_data(), std::fs::File::create(&path).unwrap())
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let mut world = ServerLevel::new(ServerLevelConfig::default());
        world.set_game_time(41);
        world.install_world_time(LevelWorldTime::load(root.path(), true).unwrap());
        assert!(
            world.world_time().unwrap().is_dirty(),
            "Packed constructor schedules decoded entries"
        );
        let clock = Identifier::with_default_namespace("overworld");
        let before_clock = world
            .clock_runtime()
            .unwrap()
            .clocks()
            .state(&clock)
            .unwrap();
        assert!(matches!(
            world.tick_world_time(&mut UnavailableScheduledFunctions),
            Err(WorldTimeError::ExecutionUnavailable(_))
        ));
        assert_eq!(
            world.game_time(),
            42,
            "time commits before callback execution"
        );
        assert_eq!(
            world
                .clock_runtime()
                .unwrap()
                .clocks()
                .state(&clock)
                .unwrap(),
            before_clock
        );
        assert!(
            world.world_time().unwrap().queue.is_empty(),
            "throwing callback was already dequeued"
        );
        world
            .tick_world_time(&mut UnavailableScheduledFunctions)
            .unwrap();
        assert_eq!(world.game_time(), 43);
        assert_eq!(
            std::fs::read(path).unwrap(),
            before,
            "tick/encoding is not a storage flush"
        );
        world.set_game_time(i64::MAX);
        world
            .tick_world_time(&mut UnavailableScheduledFunctions)
            .unwrap();
        assert_eq!(world.game_time(), i64::MIN);
        world.install_world_time(LevelWorldTime::new(false));
        world
            .tick_world_time(&mut UnavailableScheduledFunctions)
            .unwrap();
        assert_eq!(world.game_time(), i64::MIN, "tickTime=false never advances");
    }

    #[cfg(unix)]
    #[test]
    fn saved_data_flush_acknowledges_only_completed_clock_and_timer_writes() {
        use super::super::ServerLevelConfig;
        use crate::server::level::level_storage_access::LevelStorageAccess;
        use std::sync::Arc;
        let root = tempfile::tempdir().unwrap();
        let storage = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
        let mut world = ServerLevel::new(ServerLevelConfig::default());
        let dimension = storage.dimension_path(world.dimension());
        let folder = dimension.join("data/minecraft");
        // Force just the scheduled-events write to fail after clocks commit.
        std::fs::create_dir_all(folder.join("scheduled_events.dat")).unwrap();
        world.writable_storage = Some(storage);
        world.install_world_time(LevelWorldTime::new(true));
        assert!(world.save_world_saved_data().is_err());
        assert!(!world.clock_runtime().unwrap().clocks().is_dirty());
        assert!(world.world_time().unwrap().is_dirty());
        std::fs::remove_dir(folder.join("scheduled_events.dat")).unwrap();
        world.save_world_saved_data().unwrap();
        assert!(!world.world_time().unwrap().is_dirty());
        let saved = LevelWorldTime::load(&dimension, true).unwrap();
        assert!(!saved.is_dirty());
        assert!(saved.queue.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn existing_metadata_time_and_scheduled_queue_reload_from_the_same_save() {
        use super::super::ServerLevelConfig;
        use crate::server::level::level_storage_access::{LevelDataTimeSave, LevelStorageAccess};
        use std::sync::Arc;
        let root = tempfile::tempdir().unwrap();
        let storage = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
        let mut world = ServerLevel::new(ServerLevelConfig::default());
        let dimension = storage.dimension_path(world.dimension());
        let metadata = root.path().join("level.dat");
        let mut data = CompoundTag::new();
        data.put("Data".into(), Tag::Compound(CompoundTag::new()));
        nbt_io::write_compressed(&data, std::fs::File::create(&metadata).unwrap()).unwrap();
        world.writable_storage = Some(storage);
        world.set_game_time(41);
        world.install_world_time(LevelWorldTime::new(true));
        world.world_time_mut().unwrap().schedule(
            "later".into(),
            45,
            FunctionCallback::Function(Identifier::with_default_namespace("later")),
        );
        assert_eq!(
            world.save_world_saved_data_and_time().unwrap(),
            LevelDataTimeSave::Written
        );
        let saved = nbt_io::read_compressed(
            std::fs::File::open(&metadata).unwrap(),
            &mut NbtAccounter::unlimited_heap(),
        )
        .unwrap();
        assert_eq!(
            saved.get_compound("Data").unwrap().get_long("Time"),
            Some(41)
        );
        let queue = LevelWorldTime::load(&dimension, true).unwrap();
        assert_eq!(queue.queue.peek().unwrap().time, 45);
        // Malformed existing metadata must fail before writing/acknowledging
        // new scheduled state; an absent Data compound is not fabricated.
        nbt_io::write_compressed(
            &CompoundTag::new(),
            std::fs::File::create(&metadata).unwrap(),
        )
        .unwrap();
        world.world_time_mut().unwrap().schedule(
            "new".into(),
            46,
            FunctionCallback::Function(Identifier::with_default_namespace("new")),
        );
        assert!(world.save_world_saved_data_and_time().is_err());
        assert!(world.world_time().unwrap().is_dirty());
        let queue = LevelWorldTime::load(&dimension, true).unwrap();
        assert_eq!(queue.queue.len(), 1);
    }
}
