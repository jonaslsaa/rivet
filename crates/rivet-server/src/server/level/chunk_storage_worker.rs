//! G5 persistence foundation — a detached single-owner chunk storage worker.
//!
//! This is the first slice of generated-FULL-save persistence: a storage worker
//! that takes **owned** `(ChunkPos, CompoundTag)` save values off the tick thread
//! over a **bounded** channel and writes them through exactly one writable
//! `RegionFileStorage`, whose handles it alone owns. It is deliberately *not*
//! coupled to the unfinished G3/G4 serving pipeline — the tick thread's chunk
//! serialization and the actual generated-`LevelChunk` hookup are explicitly
//! deferred to the G3/G4 integration layer (see `OWNERSHIP.md` "Chunk storage
//! workers" and `docs/chunk-pipeline-spec.md` §8/§5).
//!
//! ## Ownership model (OWNERSHIP "Chunk storage workers" amendment)
//!
//! - The channel is `std::sync::mpsc::sync_channel` — a **bounded**,
//!   notify-blocking channel. Backpressure is the channel: a full channel blocks
//!   the sender (the tick thread) until the worker drains.
//! - Exactly **one** storage worker thread owns the writable `RegionFileStorage`
//!   (all its `RegionFile` handles). The tick thread never touches a
//!   `RegionFileStorage`; it only hands owned `CompoundTag` values across the
//!   channel. No game-state `Arc<RwLock>`, no `RegionFileStorage` shared across
//!   threads, no unbounded queue.
//! - The worker thread is **joined on shutdown** ([`ChunkStorageWorker::shutdown`]
//!   and `Drop`) — never a detached unjoined thread.
//!
//! ## Shutdown semantics (first slice: save-on-shutdown)
//!
//! Saves are written in FIFO order by the singleton worker (hence FIFO per
//! region too). On shutdown the worker drains every accepted save remaining in
//! the channel, writes each, then `flush`es and `close`s the storage. **Once
//! shutdown begins no new save is accepted**: [`ChunkStorageWorker::shutdown`]
//! drops the send half, so a subsequent [`ChunkStorageWorker::enqueue`] refuses
//! and returns the owned save request to the caller.
//!
//! ## Error semantics
//!
//! First-error reporting is preserved end to end. An accepted write that fails
//! is never silently dropped — the first error on any write/flush/close is
//! recorded and surfaced on [`StorageWorkerOutcome`]. A failed enqueue (channel
//! already shut down) returns the owned save request instead of discarding it.
//! If the worker thread panics, the panic is surfaced (`outcome.panicked`) and
//! reported as an error.
//!
//! ## Scope guardrails
//!
//! This slice is **save-on-shutdown only**. Autosave, unload-driven saves,
//! per-chunk store coalescing, and configuration breadth are explicitly deferred
//! to the G3/G4 integration layer. The established channel/thread/error
//! libraries are reused (`std` sync channel + `std` thread + `io::Result`); no
//! new dependencies are added.

use std::io;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Condvar;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SendError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::core::ChunkPos;
use rivet_world::chunk::storage::{RegionFileStorage, RegionStorageInfo};

/// A save request: an owned chunk position and its owned serialized
/// `CompoundTag`, constructed by the tick thread and handed to the worker.
#[derive(Debug)]
pub struct ChunkSave {
    /// The chunk this save targets (also the region-slot keying).
    pub pos: ChunkPos,
    /// The owned serialized chunk NBT to persist.
    pub tag: CompoundTag,
}

impl ChunkSave {
    /// Construct a save value for `pos` carrying `tag`.
    pub fn new(pos: ChunkPos, tag: CompoundTag) -> Self {
        Self { pos, tag }
    }
}

/// How a [`ChunkStorageWorker`] call failed.
#[derive(Debug)]
pub enum StorageWorkerError {
    /// The send half was already shut down (drain/close has begun); the owned
    /// save request is returned so the caller does not lose it.
    SendClosed(ChunkSave),
}

/// Outcome of [`ChunkStorageWorker::shutdown`].
#[derive(Debug, Default)]
pub struct StorageWorkerOutcome {
    /// The first I/O error observed on any accepted write, flush, or close, if
    /// any. Accepted writes are never silently dropped — if any failed, this is
    /// the first such error.
    pub first_error: Option<io::Error>,
    /// True if the worker thread panicked (observed via `JoinHandle`).
    pub panicked: bool,
}

/// Shared coordination between the owner (tick-thread side) and the worker.
///
/// In production this carries only the recorded first error. The pause/panic
/// fields exist solely for deterministic tests and compile out of production.
struct Shared {
    /// First I/O error from any worker write/flush/close, once.
    err: Mutex<Option<io::Error>>,
    #[cfg(test)]
    pause: Mutex<bool>,
    #[cfg(test)]
    pause_cv: Condvar,
    #[cfg(test)]
    panic_flag: AtomicBool,
}

impl Shared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            err: Mutex::new(None),
            #[cfg(test)]
            pause: Mutex::new(false),
            #[cfg(test)]
            pause_cv: Condvar::new(),
            #[cfg(test)]
            panic_flag: AtomicBool::new(false),
        })
    }

    /// Record `err` if it is the first error seen.
    fn record_first_err(&self, err: io::Error) {
        let mut guard = self.err.lock().unwrap();
        if guard.is_none() {
            *guard = Some(err);
        }
    }

    #[cfg(test)]
    fn wait_while_paused(&self) {
        let mut paused = self.pause.lock().unwrap();
        while *paused {
            paused = self.pause_cv.wait(paused).unwrap();
        }
    }

    #[cfg(test)]
    fn set_paused(&self, paused: bool) {
        let mut guard = self.pause.lock().unwrap();
        *guard = paused;
        if !paused {
            self.pause_cv.notify_all();
        }
    }
}

/// A detached, single-owner chunk storage worker.
///
/// The tick thread holds `ChunkStorageWorker` and calls
/// [`ChunkStorageWorker::enqueue`] (blocking under backpressure). The worker
/// thread exclusively owns a writable `RegionFileStorage`; no other thread ever
/// touches it. Shutdown via [`ChunkStorageWorker::shutdown`] (or `Drop`) drains,
/// flushes, closes, and **joins** the worker.
pub struct ChunkStorageWorker {
    tx: Option<SyncSender<ChunkSave>>,
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
}

impl ChunkStorageWorker {
    /// Start a storage worker writing chunk saves to `folder` via `info`.
    ///
    /// `channel_capacity` is the bound on in-flight (queued, not-yet-written)
    /// save values; it must be `> 0` (an unbounded queue is never created). The
    /// worker thread is spawned immediately and owns a fresh writable
    /// `RegionFileStorage`.
    pub fn start(
        info: RegionStorageInfo,
        folder: PathBuf,
        channel_capacity: usize,
    ) -> io::Result<Self> {
        if channel_capacity == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "chunk storage worker channel capacity must be > 0 (no unbounded queue)",
            ));
        }

        let (tx, rx): (SyncSender<ChunkSave>, Receiver<ChunkSave>) = sync_channel(channel_capacity);
        // `sync_channel(n)` holds up to n save values in flight; the (n+1)th
        // `send` blocks until the worker receives one.

        let shared = Shared::new();
        let worker_shared = Arc::clone(&shared);
        let handle = thread::Builder::new()
            .name("rivet-chunk-storage-worker".to_string())
            .spawn(move || worker_loop(rx, info, folder, worker_shared))?;

        Ok(Self {
            tx: Some(tx),
            shared,
            handle: Some(handle),
        })
    }

    /// Send an owned save value to the worker.
    ///
    /// Backpressure: if the bounded channel is full this blocks until the worker
    /// drains. If the worker has begun shutting down (send half dropped), the
    /// owned save request is returned via [`StorageWorkerError::SendClosed`]
    /// rather than silently dropped.
    pub fn enqueue(&self, save: ChunkSave) -> Result<(), StorageWorkerError> {
        let Some(tx) = self.tx.as_ref() else {
            return Err(StorageWorkerError::SendClosed(save));
        };
        match tx.send(save) {
            Ok(()) => Ok(()),
            Err(SendError(returned)) => Err(StorageWorkerError::SendClosed(returned)),
        }
    }

    /// Drain the channel, write every accepted save, flush and close the
    /// storage, join the worker, and report the outcome. After this returns the
    /// shutdown transition is complete; subsequent [`enqueue`] refuses with the
    /// owned request returned.
    ///
    /// [`enqueue`]: ChunkStorageWorker::enqueue
    pub fn shutdown(&mut self) -> StorageWorkerOutcome {
        // Drop the send half: the worker drains the remaining queue, then sees
        // `Disconnected`, flushes, closes, and returns. No save can be accepted
        // from here on.
        self.tx = None;
        // A test may have paused the worker; release it so it can reach the
        // drain/flush/close and return (production never pauses).
        #[cfg(test)]
        self.shared.set_paused(false);

        let panicked = match self.handle.take() {
            Some(handle) => handle.join().is_err(),
            None => false,
        };
        let first_error = self.shared.err.lock().unwrap().take();
        StorageWorkerOutcome {
            first_error,
            panicked,
        }
    }

    #[cfg(test)]
    fn set_paused(&self, paused: bool) {
        self.shared.set_paused(paused);
    }

    #[cfg(test)]
    fn set_panic(&self) {
        self.shared.panic_flag.store(true, Ordering::SeqCst);
    }
}

impl Drop for ChunkStorageWorker {
    /// Guarantee no orphaned thread: if the owner is dropped without an explicit
    /// [`shutdown`], disconnect and join (which drains, flushes, and closes).
    ///
    /// [`shutdown`]: ChunkStorageWorker::shutdown
    fn drop(&mut self) {
        if self.handle.is_some() {
            self.tx = None;
            #[cfg(test)]
            self.shared.set_paused(false);
            let _ = self.handle.take().unwrap().join();
        }
    }
}

/// The worker-thread body: owns the writable storage exclusively, drains the
/// bounded channel in FIFO order, then flushes and closes on disconnect.
fn worker_loop(
    rx: Receiver<ChunkSave>,
    info: RegionStorageInfo,
    folder: PathBuf,
    shared: Arc<Shared>,
) {
    let mut storage = RegionFileStorage::new(info, folder, /* sync */ false);

    while let Ok(save) = {
        #[cfg(test)]
        {
            if shared.panic_flag.load(Ordering::SeqCst) {
                panic!("chunk storage worker test panic");
            }
            shared.wait_while_paused();
        }
        rx.recv()
    } {
        match storage.write(&save.pos, Some(save.tag)) {
            Ok(()) => {}
            Err(err) => shared.record_first_err(err),
        }
    }

    // Drain complete: flush (fsync) then close every region, first-error kept.
    if let Err(err) = storage.flush() {
        shared.record_first_err(err);
    }
    match storage.close() {
        Ok(()) => {}
        Err(err) => shared.record_first_err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rivet_nbt::int_tag::IntTag;
    use rivet_nbt::tag::Tag;
    use rivet_world::chunk::storage::{RegionFileStorage, RegionFileVersion, RegionStorageInfo};

    const TEST_DATA_VERSION: i32 = 4903;

    fn region_info() -> RegionStorageInfo {
        // D13 pins the writable byte-identity path to the implemented `none`
        // codec. Every test in this binary selects the same process-global value.
        RegionFileVersion::configure("none");
        // Chunk-data storage so the read path runs the (satisfied) coordinate
        // guard; tags carry matching fixed keys.
        RegionStorageInfo::new(
            "storage-worker-test".to_string(),
            rivet_world::level::overworld(),
            "region".to_string(),
            true,
        )
    }

    /// A save whose tag carries DataVersion + xPos + zPos, so the chunk-data
    /// coordinate guard on read is satisfied.
    fn save_at(pos: ChunkPos, marker: i32) -> ChunkSave {
        let mut tag = CompoundTag::new();
        tag.put_int("DataVersion", TEST_DATA_VERSION);
        tag.put_int("xPos", pos.x());
        tag.put_int("zPos", pos.z());
        tag.put_int("marker", marker);
        ChunkSave::new(pos, tag)
    }

    fn open_reader(folder: &std::path::Path) -> RegionFileStorage {
        RegionFileStorage::new_read_only(region_info(), folder.to_path_buf())
    }

    /// Assert a chunk at `pos` roundtrips with the expected `marker`.
    fn assert_read_marker(storage: &mut RegionFileStorage, pos: ChunkPos, expected: i32) {
        let tag = storage
            .read(&pos)
            .unwrap()
            .unwrap_or_else(|| panic!("chunk at {pos:?} missing after save"));
        let marker = tag
            .get("marker")
            .and_then(|t| match t {
                Tag::Int(v) => Some(v.value),
                _ => None,
            })
            .expect("marker int present");
        assert_eq!(marker, expected, "chunk at {pos:?} marker mismatch");
    }

    fn start_worker(folder: &std::path::Path, capacity: usize) -> ChunkStorageWorker {
        ChunkStorageWorker::start(region_info(), folder.to_path_buf(), capacity).unwrap()
    }

    #[test]
    fn exact_write_read_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let mut worker = start_worker(temp.path(), 16);

        let a = ChunkPos::new(0, 0);
        let b = ChunkPos::new(3, -5);
        worker.enqueue(save_at(a, 7)).unwrap();
        worker.enqueue(save_at(b, 9)).unwrap();

        let outcome = worker.shutdown();
        assert!(outcome.first_error.is_none(), "no write error");
        assert!(!outcome.panicked);

        let mut reader = open_reader(temp.path());
        assert_read_marker(&mut reader, a, 7);
        assert_read_marker(&mut reader, b, 9);
    }

    fn start_paused_worker(folder: &std::path::Path, capacity: usize) -> ChunkStorageWorker {
        let (tx, rx) = sync_channel(capacity);
        let shared = Shared::new();
        shared.set_paused(true);
        let worker_shared = Arc::clone(&shared);
        let info = region_info();
        let folder = folder.to_path_buf();
        let handle = thread::Builder::new()
            .name("rivet-chunk-storage-worker-test".to_string())
            .spawn(move || worker_loop(rx, info, folder, worker_shared))
            .unwrap();
        ChunkStorageWorker {
            tx: Some(tx),
            shared,
            handle: Some(handle),
        }
    }

    #[test]
    fn rejected_zero_capacity_never_makes_unbounded_queue() {
        let temp = tempfile::tempdir().unwrap();
        let err = match ChunkStorageWorker::start(region_info(), temp.path().to_path_buf(), 0) {
            Ok(_) => panic!("zero-capacity worker must be refused"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn bounded_backpressure_blocks_sender_until_the_worker_drains() {
        let temp = tempfile::tempdir().unwrap();
        let mut worker = start_paused_worker(temp.path(), 2);

        // The worker starts paused before it can receive. Exactly two saves fit
        // in the bounded queue; a third send on the same channel must block.
        worker.enqueue(save_at(ChunkPos::new(0, 0), 1)).unwrap();
        worker.enqueue(save_at(ChunkPos::new(0, 1), 2)).unwrap();
        let tx = worker.tx.as_ref().unwrap().clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let blocked_sender = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let result = tx.send(save_at(ChunkPos::new(0, 2), 3));
            done_tx.send(result).unwrap();
        });
        started_rx.recv().unwrap();
        assert!(
            done_rx
                .recv_timeout(std::time::Duration::from_millis(50))
                .is_err(),
            "the third send returned while the capacity-2 queue was full"
        );

        worker.set_paused(false);
        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("unpausing the worker releases the blocked sender")
            .expect("worker channel remains open");
        blocked_sender.join().unwrap();
        let outcome = worker.shutdown();
        assert!(outcome.first_error.is_none());
        assert!(!outcome.panicked);
    }

    #[test]
    fn shutdown_drains_fifo_and_rejects_new_saves_with_ownership_returned() {
        let temp = tempfile::tempdir().unwrap();
        let mut worker = start_paused_worker(temp.path(), 4);
        let pos = ChunkPos::new(-4, 9);
        worker.enqueue(save_at(pos, 1)).unwrap();
        worker.enqueue(save_at(pos, 2)).unwrap();

        let outcome = worker.shutdown();
        assert!(outcome.first_error.is_none());
        assert!(!outcome.panicked);

        let mut reader = open_reader(temp.path());
        assert_read_marker(&mut reader, pos, 2);

        let rejected = save_at(ChunkPos::new(8, 8), 3);
        match worker.enqueue(rejected) {
            Err(StorageWorkerError::SendClosed(returned)) => {
                assert_eq!(returned.pos, ChunkPos::new(8, 8));
                assert_eq!(
                    returned.tag.get("marker"),
                    Some(&Tag::Int(IntTag::value_of(3))),
                    "the caller gets the owned save back unchanged"
                );
            }
            Ok(()) => panic!("shutdown worker accepted a new save"),
        }
    }

    #[test]
    fn write_failure_is_reported_and_the_worker_still_joins() {
        let temp = tempfile::tempdir().unwrap();
        let not_a_directory = temp.path().join("not-a-directory");
        std::fs::write(&not_a_directory, b"fixture").unwrap();
        let mut worker = start_worker(&not_a_directory, 1);
        worker.enqueue(save_at(ChunkPos::ZERO, 1)).unwrap();

        let outcome = worker.shutdown();
        assert!(outcome.first_error.is_some(), "write failure must surface");
        assert!(!outcome.panicked, "an I/O failure is not a worker panic");
    }

    #[test]
    fn worker_panic_is_reported_by_shutdown() {
        let temp = tempfile::tempdir().unwrap();
        let mut worker = start_paused_worker(temp.path(), 1);
        worker.enqueue(save_at(ChunkPos::ZERO, 1)).unwrap();
        worker.set_panic();

        let outcome = worker.shutdown();
        assert!(outcome.panicked, "JoinHandle panic must be observable");
    }

    #[test]
    fn drop_unpauses_drains_and_joins_the_worker() {
        let temp = tempfile::tempdir().unwrap();
        let pos = ChunkPos::new(2, 6);
        {
            let worker = start_paused_worker(temp.path(), 1);
            worker.enqueue(save_at(pos, 17)).unwrap();
            // Drop releases the test pause, drains the accepted save, flushes,
            // closes, and joins before the storage can be reopened below.
        }

        let mut reader = open_reader(temp.path());
        assert_read_marker(&mut reader, pos, 17);
    }
}
