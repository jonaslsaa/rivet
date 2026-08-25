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

#[cfg(test)]
use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Condvar;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SendError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};

use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::core::ChunkPos;
use rivet_world::chunk::storage::{RegionFileStorage, RegionFileVersion, RegionStorageInfo};

/// A save request: an owned chunk position and its owned serialized
/// `CompoundTag`, constructed by the tick thread and handed to the worker.
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, thiserror::Error)]
pub enum StorageWorkerError {
    /// The send half was already shut down (drain/close has begun); the owned
    /// save request is returned so the caller does not lose it.
    #[error("chunk storage worker send closed")]
    SendClosed(ChunkSave),
}

/// A stable snapshot of the I/O identity retained in a shutdown outcome.
///
/// `io::Error` is not cloneable and may carry a non-cloneable source chain, so
/// repeated shutdown calls expose this explicit snapshot instead of rebuilding
/// an `io::Error` that could silently lose metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageWorkerErrorSnapshot {
    /// The standard I/O error category.
    pub kind: io::ErrorKind,
    /// The platform error number, when the source supplied one.
    pub raw_os_error: Option<i32>,
    /// The rendered error message.
    pub message: String,
}

impl StorageWorkerErrorSnapshot {
    fn from_error(error: &io::Error) -> Self {
        Self {
            kind: error.kind(),
            raw_os_error: error.raw_os_error(),
            message: error.to_string(),
        }
    }
}

/// Outcome of [`ChunkStorageWorker::shutdown`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StorageWorkerOutcome {
    /// The first I/O error observed on any accepted write, flush, or close, if
    /// any. Accepted writes are never silently dropped — if any failed, this is
    /// the first such error.
    pub first_error: Option<StorageWorkerErrorSnapshot>,
    /// True if the worker thread panicked while processing saves.
    pub panicked: bool,
    /// Accepted saves that were not written because the worker panicked. The
    /// current save and every queued save are retained in FIFO order so the
    /// caller can retry or otherwise account for them.
    pub recovered_saves: Vec<ChunkSave>,
}

/// Process-local ownership of one canonical writable storage folder.
static STORAGE_FOLDER_OWNERS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    /// Test-only handoff used to change cwd after `start` has created the folder.
    static SWITCH_CWD_AFTER_CREATE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

#[cfg(test)]
fn switch_cwd_after_create_for_test() -> io::Result<()> {
    SWITCH_CWD_AFTER_CREATE.with(|pending| {
        let Some(path) = pending.borrow_mut().take() else {
            return Ok(());
        };
        std::env::set_current_dir(path)
    })
}

fn storage_folder_owners() -> &'static Mutex<HashSet<PathBuf>> {
    STORAGE_FOLDER_OWNERS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Resolve `folder` to an absolute path before any filesystem side effect.
fn absolute_storage_folder(folder: &Path) -> io::Result<PathBuf> {
    if folder.is_absolute() {
        Ok(folder.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(folder))
    }
}

/// Resolve an existing path through symlinks, or resolve its deepest existing
/// ancestor and append only the suffix that is genuinely missing. The ancestor
/// search deliberately keeps the original `..` components: `exists` and
/// `canonicalize` then apply the operating system's traversal order instead of
/// collapsing `ParentDir` lexically before a preceding symlink is resolved.
fn canonical_storage_folder(folder: &Path) -> io::Result<PathBuf> {
    let absolute = if folder.is_absolute() {
        folder.to_path_buf()
    } else {
        std::env::current_dir()?.join(folder)
    };

    let mut existing = absolute.as_path();
    while !existing.exists() {
        existing = existing.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "storage folder has no existing ancestor: {}",
                    folder.display()
                ),
            )
        })?;
    }

    let suffix = absolute.strip_prefix(existing).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "storage folder has no existing ancestor: {}",
                folder.display()
            ),
        )
    })?;
    let mut canonical = fs::canonicalize(existing)?;
    for component in suffix.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                canonical.pop();
            }
            std::path::Component::Normal(name) => canonical.push(name),
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "storage folder has invalid missing suffix: {}",
                        folder.display()
                    ),
                ));
            }
        }
    }
    Ok(canonical)
}

struct StorageFolderOwner {
    canonical_folder: PathBuf,
}

impl StorageFolderOwner {
    fn acquire(folder: &Path) -> io::Result<Self> {
        let canonical_folder = canonical_storage_folder(folder)?;
        let mut owners = storage_folder_owners()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !owners.insert(canonical_folder.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "another chunk storage worker already owns {}",
                    canonical_folder.display()
                ),
            ));
        }
        Ok(Self { canonical_folder })
    }
}

impl Drop for StorageFolderOwner {
    fn drop(&mut self) {
        let mut owners = storage_folder_owners()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        owners.remove(&self.canonical_folder);
    }
}

struct AcceptanceState {
    accepting: bool,
    active_senders: usize,
}

/// Shared coordination between the owner (tick-thread side) and the worker.
///
/// In production this carries the recorded first error plus the acceptance
/// barrier used to close the send side before recovery/finalization. The
/// pause/panic fields exist solely for deterministic tests and compile out of
/// production.
struct Shared {
    /// First I/O error from any worker write/flush/close, once.
    err: Mutex<Option<io::Error>>,
    /// Acceptance barrier: no new enqueue may start after shutdown or panic
    /// recovery closes the gate; in-flight senders finish before recovery drains.
    acceptance: Mutex<AcceptanceState>,
    acceptance_cv: Condvar,
    #[cfg(test)]
    pause: Mutex<bool>,
    #[cfg(test)]
    pause_cv: Condvar,
    #[cfg(test)]
    panic_flag: AtomicBool,
    #[cfg(test)]
    panic_after_write: AtomicBool,
}

impl Shared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            err: Mutex::new(None),
            acceptance: Mutex::new(AcceptanceState {
                accepting: true,
                active_senders: 0,
            }),
            acceptance_cv: Condvar::new(),
            #[cfg(test)]
            pause: Mutex::new(false),
            #[cfg(test)]
            pause_cv: Condvar::new(),
            #[cfg(test)]
            panic_flag: AtomicBool::new(false),
            #[cfg(test)]
            panic_after_write: AtomicBool::new(false),
        })
    }

    /// Record `err` if it is the first error seen.
    fn record_first_err(&self, err: io::Error) {
        let mut guard = self
            .err
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.is_none() {
            *guard = Some(err);
        }
    }

    /// Reserve an enqueue before it can block in `SyncSender::send`.
    fn begin_enqueue(&self) -> bool {
        let mut state = self
            .acceptance
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.accepting {
            return false;
        }
        state.active_senders += 1;
        self.acceptance_cv.notify_all();
        true
    }

    /// Finish an enqueue reservation and wake recovery/shutdown waiters.
    fn finish_enqueue(&self) {
        let mut state = self
            .acceptance
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.active_senders -= 1;
        // Wake recovery after every accepted sender completes so it can drain
        // the newly queued value before another blocked sender fills the
        // bounded channel.
        self.acceptance_cv.notify_all();
    }

    /// Close public enqueue acceptance. Existing reservations remain valid and
    /// are accounted for before a recovery drain proceeds.
    fn stop_accepting(&self) {
        let mut state = self
            .acceptance
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.accepting = false;
        if state.active_senders == 0 {
            self.acceptance_cv.notify_all();
        }
    }

    fn wait_for_senders(&self) {
        let mut state = self
            .acceptance
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while state.active_senders != 0 {
            state = self
                .acceptance_cv
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    #[cfg(test)]
    fn wait_for_active_senders(&self, minimum: usize) {
        let mut state = self
            .acceptance
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while state.active_senders < minimum {
            state = self
                .acceptance_cv
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
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

/// Owned data returned by the worker thread so shutdown can recover accepted
/// saves even when processing or finalization panics.
#[derive(Debug, Default)]
struct WorkerThreadOutcome {
    panicked: bool,
    recovered_saves: Vec<ChunkSave>,
}

/// The worker's owned state. Keeping the current save in this struct, rather
/// than in a local that is moved into `RegionFileStorage::write`, lets the
/// unwind boundary recover it together with the channel queue.
struct WorkerSession {
    rx: Receiver<ChunkSave>,
    storage: RegionFileStorage,
    shared: Arc<Shared>,
    current: Option<ChunkSave>,
    recovered_saves: Vec<ChunkSave>,
}

impl WorkerSession {
    fn new(
        rx: Receiver<ChunkSave>,
        info: RegionStorageInfo,
        folder: PathBuf,
        version: RegionFileVersion,
        shared: Arc<Shared>,
    ) -> Self {
        Self {
            rx,
            storage: RegionFileStorage::new_with_version(
                info, folder, /* sync */ false, version,
            ),
            shared,
            current: None,
            recovered_saves: Vec::new(),
        }
    }

    /// Process accepted saves in FIFO order until the sender disconnects.
    /// `current` remains owned by the session through the borrowed storage call
    /// so an unwind leaves it recoverable.
    fn run(&mut self) {
        loop {
            #[cfg(test)]
            {
                self.shared.wait_while_paused();
            }
            self.current = match self.rx.recv() {
                Ok(save) => Some(save),
                Err(_) => break,
            };

            #[cfg(test)]
            if self.shared.panic_flag.load(Ordering::SeqCst) {
                panic!("chunk storage worker test panic before write");
            }

            let save = self
                .current
                .as_ref()
                .expect("received save must become current");
            if let Err(error) = self.storage.write_ref(&save.pos, Some(&save.tag)) {
                self.shared.record_first_err(error);
            }
            // A normal return, including an I/O error, accounts for the
            // current save. Only an unwind before this point recovers it.
            self.current = None;

            #[cfg(test)]
            if self.shared.panic_after_write.load(Ordering::SeqCst) {
                panic!("chunk storage worker test panic after write");
            }
        }
    }

    /// Close public acceptance, retain the current save first, then drain the
    /// accepted channel values in FIFO order. In-flight enqueue reservations may
    /// still complete after the first drain, so keep draining around the sender
    /// barrier until every accepted send has returned.
    fn recover_saves(&mut self) {
        self.shared.stop_accepting();
        if let Some(current) = self.current.take() {
            self.recovered_saves.push(current);
        }
        loop {
            self.recovered_saves.extend(self.rx.try_iter());
            let state = self
                .shared
                .acceptance
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.active_senders == 0 {
                break;
            }
            // A completed sender can refill the bounded channel while another
            // sender remains blocked. Wake once per completion, then loop back
            // to drain again instead of waiting for all senders in one stretch.
            drop(
                self.shared
                    .acceptance_cv
                    .wait(state)
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            );
        }
        self.recovered_saves.extend(self.rx.try_iter());
    }

    /// Always attempt both storage finalization operations. A panic from either
    /// operation is recorded without preventing the other operation from being
    /// attempted.
    fn finalize(&mut self) -> bool {
        let mut panicked = false;
        match catch_unwind(AssertUnwindSafe(|| self.storage.flush())) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => self.shared.record_first_err(error),
            Err(_) => panicked = true,
        }
        match catch_unwind(AssertUnwindSafe(|| self.storage.close())) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => self.shared.record_first_err(error),
            Err(_) => panicked = true,
        }
        panicked
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
    handle: Option<JoinHandle<WorkerThreadOutcome>>,
    owner: Option<StorageFolderOwner>,
    shutdown_outcome: Option<StorageWorkerOutcome>,
}

impl ChunkStorageWorker {
    /// Start a storage worker writing chunk saves to `folder` via `info`.
    ///
    /// `channel_capacity` is the bound on in-flight (queued, not-yet-written)
    /// save values; it must be `> 0` (an unbounded queue is never created).
    /// `version` is snapshotted explicitly for this worker and must have a
    /// writable wrapper; unsupported selections fail before the worker or its
    /// folder starts. A valid start resolves the requested folder to one
    /// absolute path before creating it, reserves that path's canonical
    /// identity, and hands the same absolute path to the worker. The worker
    /// thread is spawned immediately and owns a fresh writable
    /// `RegionFileStorage`.
    pub fn start(
        info: RegionStorageInfo,
        folder: PathBuf,
        channel_capacity: usize,
        version: RegionFileVersion,
    ) -> io::Result<Self> {
        if channel_capacity == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "chunk storage worker channel capacity must be > 0 (no unbounded queue)",
            ));
        }

        // Validate the explicit codec before any filesystem side effect. The
        // default deflate selection is not a writable Rivet path, so callers
        // must choose a supported codec deliberately (D13 uses `none`).
        let _ = version.wrap_output(io::sink())?;
        // Resolve relative input once, before any filesystem side effect. Keep
        // the unresolved suffix so the OS still applies symlink-before-`..`
        // traversal when creating and canonicalizing the path.
        let folder = absolute_storage_folder(&folder)?;
        fs::create_dir_all(&folder)?;
        #[cfg(test)]
        switch_cwd_after_create_for_test()?;
        let owner = StorageFolderOwner::acquire(&folder)?;
        let worker_folder = owner.canonical_folder.clone();

        let (tx, rx): (SyncSender<ChunkSave>, Receiver<ChunkSave>) = sync_channel(channel_capacity);
        // `sync_channel(n)` holds up to n save values in flight; the (n+1)th
        // `send` blocks until the worker receives one.

        let shared = Shared::new();
        let worker_shared = Arc::clone(&shared);
        let handle = thread::Builder::new()
            .name("rivet-chunk-storage-worker".to_string())
            .spawn(move || worker_loop(rx, info, worker_folder, version, worker_shared))?;

        Ok(Self {
            tx: Some(tx),
            shared,
            handle: Some(handle),
            owner: Some(owner),
            shutdown_outcome: None,
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
        if !self.shared.begin_enqueue() {
            return Err(StorageWorkerError::SendClosed(save));
        }
        let result = match tx.send(save) {
            Ok(()) => Ok(()),
            Err(SendError(returned)) => Err(StorageWorkerError::SendClosed(returned)),
        };
        self.shared.finish_enqueue();
        result
    }

    /// Drain the channel, write every accepted save, flush and close the
    /// storage, join the worker, and report the outcome. After this returns the
    /// shutdown transition is complete; subsequent [`enqueue`] refuses with the
    /// owned request returned.
    ///
    /// [`enqueue`]: ChunkStorageWorker::enqueue
    pub fn shutdown(&mut self) -> StorageWorkerOutcome {
        if let Some(outcome) = &self.shutdown_outcome {
            return outcome.clone();
        }

        // Close acceptance before joining. Enqueues that already reserved a
        // send may still complete; the worker's normal drain or panic recovery
        // accounts for them before finalization. No later enqueue can return Ok.
        self.shared.stop_accepting();
        // A test may have paused the worker while an accepted sender is blocked
        // on a full channel. Release it before waiting for that sender, or
        // shutdown would deadlock in the test-only harness (production never
        // pauses).
        #[cfg(test)]
        self.shared.set_paused(false);
        self.shared.wait_for_senders();
        // Drop the send half: the worker drains the remaining queue, then sees
        // `Disconnected`, flushes, closes, and returns.
        self.tx = None;

        let worker_outcome = match self.handle.take() {
            Some(handle) => handle.join().unwrap_or_else(|_| WorkerThreadOutcome {
                // The worker boundary catches ordinary processing and
                // finalization panics. This is a last-resort marker for an
                // unrecoverable panic outside that boundary.
                panicked: true,
                recovered_saves: Vec::new(),
            }),
            None => WorkerThreadOutcome::default(),
        };
        let first_error = self
            .shared
            .err
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .map(|error| StorageWorkerErrorSnapshot::from_error(&error));
        // Release the process-local reservation only after the worker has
        // stopped and RegionFileStorage has either closed or unwound.
        self.owner.take();
        let outcome = StorageWorkerOutcome {
            first_error,
            panicked: worker_outcome.panicked,
            recovered_saves: worker_outcome.recovered_saves,
        };
        self.shutdown_outcome = Some(outcome.clone());
        outcome
    }

    #[cfg(test)]
    fn set_paused(&self, paused: bool) {
        self.shared.set_paused(paused);
    }

    #[cfg(test)]
    fn set_panic(&self) {
        self.shared.panic_flag.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    fn set_panic_after_write(&self) {
        self.shared.panic_after_write.store(true, Ordering::SeqCst);
    }
}

impl Drop for ChunkStorageWorker {
    /// Guarantee no orphaned thread: if the owner is dropped without an explicit
    /// [`shutdown`], disconnect and join (which drains, flushes, and closes).
    /// Any I/O error or worker panic is reported through tracing without
    /// unwinding from `Drop`.
    ///
    /// [`shutdown`]: ChunkStorageWorker::shutdown
    fn drop(&mut self) {
        if self.handle.is_some() {
            let outcome = self.shutdown();
            if let Some(error) = outcome.first_error {
                tracing::error!(
                    error = %error.message,
                    error_kind = ?error.kind,
                    raw_os_error = ?error.raw_os_error,
                    "chunk storage worker dropped with an I/O error"
                );
            }
            if outcome.panicked {
                tracing::error!("chunk storage worker thread panicked during Drop");
            }
            if !outcome.recovered_saves.is_empty() {
                let recovered_positions: Vec<_> = outcome
                    .recovered_saves
                    .iter()
                    .map(|save| save.pos)
                    .collect();
                tracing::error!(
                    recovered_saves = outcome.recovered_saves.len(),
                    recovered_positions = ?recovered_positions,
                    "chunk storage worker Drop cannot return recovered accepted save payloads; they are discarded"
                );
            }
        } else {
            // This is only a defensive path for partially-constructed values;
            // normal shutdown has already released the owner above.
            self.owner.take();
        }
    }
}

/// The worker-thread body: owns the writable storage exclusively, drains the
/// bounded channel in FIFO order, then flushes and closes on disconnect. The
/// unwind boundary retains the current save and drains every queued save before
/// attempting both finalization operations.
fn worker_loop(
    rx: Receiver<ChunkSave>,
    info: RegionStorageInfo,
    folder: PathBuf,
    version: RegionFileVersion,
    shared: Arc<Shared>,
) -> WorkerThreadOutcome {
    let mut session = WorkerSession::new(rx, info, folder, version, shared);
    let mut panicked = catch_unwind(AssertUnwindSafe(|| session.run())).is_err();
    if panicked {
        session.recover_saves();
    }
    panicked |= session.finalize();
    WorkerThreadOutcome {
        panicked,
        recovered_saves: session.recovered_saves,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rivet_nbt::int_tag::IntTag;
    use rivet_nbt::tag::Tag;
    use rivet_world::chunk::storage::{RegionFileStorage, RegionFileVersion, RegionStorageInfo};

    const TEST_DATA_VERSION: i32 = 4903;

    static CURRENT_DIR_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct RestoreCurrentDir(PathBuf);

    impl Drop for RestoreCurrentDir {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).expect("restore test cwd");
        }
    }

    fn storage_info() -> RegionStorageInfo {
        // Chunk-data storage so the read path runs the (satisfied) coordinate
        // guard; tags carry matching fixed keys.
        RegionStorageInfo::new(
            "storage-worker-test".to_string(),
            rivet_world::level::overworld(),
            "region".to_string(),
            true,
        )
    }

    fn region_info() -> RegionStorageInfo {
        storage_info()
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
        RegionFileStorage::new_read_only_with_version(
            storage_info(),
            folder.to_path_buf(),
            RegionFileVersion::VERSION_NONE,
        )
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
        ChunkStorageWorker::start(
            region_info(),
            folder.to_path_buf(),
            capacity,
            RegionFileVersion::VERSION_NONE,
        )
        .unwrap()
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
        std::fs::create_dir_all(folder).unwrap();
        let owner = StorageFolderOwner::acquire(folder).unwrap();
        let worker_folder = owner.canonical_folder.clone();
        let (tx, rx) = sync_channel(capacity);
        let shared = Shared::new();
        shared.set_paused(true);
        let worker_shared = Arc::clone(&shared);
        let info = region_info();
        let version = RegionFileVersion::VERSION_NONE;
        let handle = thread::Builder::new()
            .name("rivet-chunk-storage-worker-test".to_string())
            .spawn(move || worker_loop(rx, info, worker_folder, version, worker_shared))
            .unwrap();
        ChunkStorageWorker {
            tx: Some(tx),
            shared,
            handle: Some(handle),
            owner: Some(owner),
            shutdown_outcome: None,
        }
    }

    #[test]
    fn start_rejects_default_deflate_before_spawn_or_folder_creation() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("not-created");
        let err = match ChunkStorageWorker::start(
            storage_info(),
            folder.clone(),
            1,
            RegionFileVersion::VERSION_DEFLATE,
        ) {
            Ok(_) => panic!("unsupported default writer must be rejected at start"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
        assert!(
            !folder.exists(),
            "codec preflight must not create the folder"
        );
    }

    #[test]
    fn rejected_zero_capacity_never_makes_unbounded_queue() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("worker");
        let err = match ChunkStorageWorker::start(
            region_info(),
            folder.clone(),
            0,
            RegionFileVersion::VERSION_NONE,
        ) {
            Ok(_) => panic!("zero-capacity worker must be refused"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(
            !folder.exists(),
            "invalid capacity must not create a folder"
        );
    }

    #[test]
    fn relative_folder_is_not_reresolved_after_cwd_changes() {
        let _cwd_lock = CURRENT_DIR_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap();
        let original_cwd = std::env::current_dir().unwrap();
        let _restore_cwd = RestoreCurrentDir(original_cwd);

        let temp = tempfile::tempdir().unwrap();
        let first_cwd = temp.path().join("first");
        let second_cwd = temp.path().join("second");
        fs::create_dir(&first_cwd).unwrap();
        fs::create_dir(&second_cwd).unwrap();
        std::env::set_current_dir(&first_cwd).unwrap();

        SWITCH_CWD_AFTER_CREATE.with(|pending| {
            *pending.borrow_mut() = Some(second_cwd.clone());
        });
        let relative_folder = PathBuf::from("storage");
        let mut worker = ChunkStorageWorker::start(
            region_info(),
            relative_folder,
            1,
            RegionFileVersion::VERSION_NONE,
        )
        .unwrap();
        assert_eq!(std::env::current_dir().unwrap(), second_cwd);

        let pos = ChunkPos::new(6, -3);
        worker.enqueue(save_at(pos, 41)).unwrap();
        assert!(worker.shutdown().first_error.is_none());

        let first_storage = first_cwd.join("storage");
        let second_storage = second_cwd.join("storage");
        assert!(first_storage.exists(), "creation must use the first cwd");
        assert!(
            !second_storage.exists(),
            "ownership and worker handoff must not re-resolve the relative path"
        );
        let mut reader = open_reader(&first_storage);
        assert_read_marker(&mut reader, pos, 41);
        reader.close().unwrap();
    }

    #[test]
    fn duplicate_canonical_folder_is_refused_and_released_after_shutdown() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("region");
        std::fs::create_dir(&folder).unwrap();
        let alias = folder.join(".");
        let mut first = ChunkStorageWorker::start(
            storage_info(),
            folder.clone(),
            1,
            RegionFileVersion::VERSION_NONE,
        )
        .unwrap();

        let duplicate = match ChunkStorageWorker::start(
            storage_info(),
            alias,
            1,
            RegionFileVersion::VERSION_NONE,
        ) {
            Ok(_) => panic!("two workers must not own one canonical folder"),
            Err(error) => error,
        };
        assert_eq!(duplicate.kind(), io::ErrorKind::AlreadyExists);
        let outcome = first.shutdown();
        assert!(outcome.first_error.is_none());
        std::fs::remove_dir(&folder).unwrap();

        let mut restarted = ChunkStorageWorker::start(
            storage_info(),
            folder.clone(),
            1,
            RegionFileVersion::VERSION_NONE,
        )
        .unwrap();
        let outcome = restarted.shutdown();
        assert!(outcome.first_error.is_none());
        assert!(
            folder.exists(),
            "a valid start materializes its storage folder"
        );
        std::fs::remove_dir(&folder).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn duplicate_symlink_alias_is_refused_and_released_after_drop() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("region");
        let alias = root.path().join("region-alias");
        std::fs::create_dir(&folder).unwrap();
        symlink(&folder, &alias).unwrap();
        let first = ChunkStorageWorker::start(
            storage_info(),
            folder.clone(),
            1,
            RegionFileVersion::VERSION_NONE,
        )
        .unwrap();

        let duplicate = match ChunkStorageWorker::start(
            storage_info(),
            alias.clone(),
            1,
            RegionFileVersion::VERSION_NONE,
        ) {
            Ok(_) => panic!("a symlink alias must not bypass folder ownership"),
            Err(error) => error,
        };
        assert_eq!(duplicate.kind(), io::ErrorKind::AlreadyExists);
        drop(first);

        let mut restarted = ChunkStorageWorker::start(
            storage_info(),
            alias.clone(),
            1,
            RegionFileVersion::VERSION_NONE,
        )
        .expect("Drop releases the symlink target owner");
        let outcome = restarted.shutdown();
        assert!(outcome.first_error.is_none());
        std::fs::remove_dir(&folder).unwrap();
        std::fs::remove_file(&alias).unwrap();
        assert!(!folder.exists());
        assert!(!alias.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_parentdir_resolves_target_for_writes_and_ownership() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base");
        let outside = root.path().join("outside");
        std::fs::create_dir(&base).unwrap();
        std::fs::create_dir(&outside).unwrap();
        symlink(&outside, base.join("link")).unwrap();

        // The kernel resolves `link` before the following `..`, so this is
        // `<root>/region`, not the lexically collapsed `<root>/base/region`.
        let requested = base.join("link").join("..").join("region");
        let actual = root.path().join("region");
        let lexical = base.join("region");
        let mut first = ChunkStorageWorker::start(
            storage_info(),
            requested,
            1,
            RegionFileVersion::VERSION_NONE,
        )
        .unwrap();

        let duplicate = match ChunkStorageWorker::start(
            storage_info(),
            actual.clone(),
            1,
            RegionFileVersion::VERSION_NONE,
        ) {
            Ok(_) => panic!("the OS-resolved target must be one-owner"),
            Err(error) => error,
        };
        assert_eq!(duplicate.kind(), io::ErrorKind::AlreadyExists);

        let pos = ChunkPos::new(5, -7);
        first.enqueue(save_at(pos, 37)).unwrap();
        assert!(first.shutdown().first_error.is_none());

        let mut reader = open_reader(&actual);
        assert_read_marker(&mut reader, pos, 37);
        reader.close().unwrap();
        assert!(actual.exists(), "writes must reach the OS-resolved target");
        assert!(
            !lexical.exists(),
            "writes must not use the lexically collapsed base/region path"
        );
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
        assert!(matches!(
            tx.try_send(save_at(ChunkPos::new(0, 2), 3)),
            Err(std::sync::mpsc::TrySendError::Full(_))
        ));

        // Synchronize immediately before the real blocking send. Unlike a
        // timeout, this proves the sender has entered the send attempt before
        // the main thread checks that it cannot complete.
        let entered_send = Arc::new((Mutex::new(false), Condvar::new()));
        let entered_send_thread = Arc::clone(&entered_send);
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let blocked_sender = std::thread::spawn(move || {
            let (entered, entered_cv) = &*entered_send_thread;
            *entered.lock().unwrap() = true;
            entered_cv.notify_one();
            let result = tx.send(save_at(ChunkPos::new(0, 2), 3));
            done_tx.send(result).unwrap();
        });
        let (entered, entered_cv) = &*entered_send;
        let mut entered = entered.lock().unwrap();
        while !*entered {
            entered = entered_cv.wait(entered).unwrap();
        }
        drop(entered);
        assert!(
            done_rx.try_recv().is_err(),
            "the sender must still be blocked after entering the full sync_channel send"
        );

        worker.set_paused(false);
        done_rx
            .recv()
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
        let mut worker = start_paused_worker(temp.path(), 1);
        let pos = ChunkPos::new(-4, 9);
        worker.enqueue(save_at(pos, 1)).unwrap();

        // Reserve a second accepted send and block it behind the paused worker's
        // full queue. Shutdown must release the pause before waiting for this
        // sender, otherwise the public transition would deadlock.
        let tx = worker.tx.as_ref().unwrap().clone();
        let shared = Arc::clone(&worker.shared);
        assert!(shared.begin_enqueue());
        let sender = thread::spawn(move || {
            let result = tx.send(save_at(pos, 2));
            drop(tx);
            shared.finish_enqueue();
            result
        });
        worker.shared.wait_for_active_senders(1);

        let outcome = worker.shutdown();
        assert!(sender.join().unwrap().is_ok());
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
        let folder = temp.path().join("storage");
        let mut worker = start_worker(&folder, 1);
        std::fs::remove_dir(&folder).unwrap();
        std::fs::write(&folder, b"fixture").unwrap();
        worker.enqueue(save_at(ChunkPos::ZERO, 1)).unwrap();

        let outcome = worker.shutdown();
        assert!(outcome.first_error.is_some(), "write failure must surface");
        assert!(!outcome.panicked, "an I/O failure is not a worker panic");
    }

    #[test]
    fn repeated_shutdown_retains_io_error_and_panic_outcome() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("storage");
        let mut worker = start_worker(&folder, 1);
        std::fs::remove_dir(&folder).unwrap();
        std::fs::write(&folder, b"fixture").unwrap();
        worker.enqueue(save_at(ChunkPos::ZERO, 1)).unwrap();

        let first = worker.shutdown();
        let first_error = first
            .first_error
            .as_ref()
            .expect("write failure must carry an I/O snapshot");
        assert!(matches!(
            first_error.kind,
            io::ErrorKind::AlreadyExists | io::ErrorKind::NotADirectory
        ));
        assert!(first_error.raw_os_error.is_some());
        assert!(!first_error.message.is_empty());
        let second = worker.shutdown();
        assert_eq!(second.first_error, first.first_error);
        assert_eq!(
            second
                .first_error
                .as_ref()
                .and_then(|error| error.raw_os_error),
            first_error.raw_os_error,
            "repeated shutdown must retain the raw OS error"
        );
        assert!(!first.panicked);
        assert!(!second.panicked);

        let temp = tempfile::tempdir().unwrap();
        let mut panicked = start_paused_worker(temp.path(), 2);
        let panic_first_save = save_at(ChunkPos::new(1, 1), 11);
        let panic_second_save = save_at(ChunkPos::new(2, 2), 12);
        panicked.enqueue(panic_first_save.clone()).unwrap();
        panicked.enqueue(panic_second_save.clone()).unwrap();
        panicked.set_panic();
        let first = panicked.shutdown();
        let second = panicked.shutdown();
        assert!(first.panicked);
        assert!(second.panicked);
        assert_eq!(first.recovered_saves, [panic_first_save, panic_second_save]);
        assert_eq!(second.recovered_saves, first.recovered_saves);
    }

    #[test]
    fn panic_recovers_current_and_queued_saves_after_a_successful_write() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("region");
        let first_save = save_at(ChunkPos::new(3, 3), 21);
        let queued_save = save_at(ChunkPos::new(4, 4), 22);
        let mut worker = start_paused_worker(&folder, 2);
        worker.enqueue(first_save.clone()).unwrap();
        worker.enqueue(queued_save.clone()).unwrap();
        worker.set_panic_after_write();

        let outcome = worker.shutdown();
        assert!(outcome.panicked);
        assert!(outcome.first_error.is_none());
        assert_eq!(outcome.recovered_saves, [queued_save]);

        let mut reader = open_reader(&folder);
        assert_read_marker(&mut reader, first_save.pos, 21);
        reader.close().unwrap();
    }

    #[test]
    fn drop_reports_io_error_without_unwinding() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let temp = tempfile::tempdir().unwrap();
            let folder = temp.path().join("storage");
            let worker = start_worker(&folder, 1);
            std::fs::remove_dir(&folder).unwrap();
            std::fs::write(&folder, b"fixture").unwrap();
            worker.enqueue(save_at(ChunkPos::ZERO, 1)).unwrap();
            drop(worker);
        }));
        assert!(
            result.is_ok(),
            "Drop must report worker I/O errors, not panic"
        );
    }

    #[test]
    fn worker_panic_is_reported_by_shutdown() {
        let temp = tempfile::tempdir().unwrap();
        let mut worker = start_paused_worker(temp.path(), 1);
        let save = save_at(ChunkPos::ZERO, 1);
        worker.enqueue(save.clone()).unwrap();
        worker.set_panic();

        let outcome = worker.shutdown();
        assert!(outcome.panicked, "worker panic must be observable");
        assert_eq!(outcome.recovered_saves, [save]);
    }

    #[test]
    fn concurrent_enqueue_is_closed_before_panic_recovery_drain() {
        let temp = tempfile::tempdir().unwrap();
        let first_save = save_at(ChunkPos::new(7, 7), 31);
        let queued_save_a = save_at(ChunkPos::new(8, 8), 32);
        let queued_save_b = save_at(ChunkPos::new(9, 9), 33);
        let worker = Arc::new(start_paused_worker(temp.path(), 1));
        worker.enqueue(first_save.clone()).unwrap();

        let sender_worker_a = Arc::clone(&worker);
        let queued_save_for_sender_a = queued_save_a.clone();
        let sender_a = thread::spawn(move || sender_worker_a.enqueue(queued_save_for_sender_a));
        worker.shared.wait_for_active_senders(1);

        let sender_worker_b = Arc::clone(&worker);
        let queued_save_for_sender_b = queued_save_b.clone();
        let sender_b = thread::spawn(move || sender_worker_b.enqueue(queued_save_for_sender_b));
        worker.shared.wait_for_active_senders(2);

        worker.set_panic();
        worker.set_paused(false);
        assert!(
            sender_a.join().unwrap().is_ok(),
            "the first enqueue reserved before panic must complete successfully"
        );
        assert!(
            sender_b.join().unwrap().is_ok(),
            "the second enqueue reserved before panic must complete successfully"
        );
        let worker = match Arc::try_unwrap(worker) {
            Ok(worker) => worker,
            Err(_) => panic!("sender thread retained the worker owner"),
        };
        let mut worker = worker;
        let outcome = worker.shutdown();
        assert!(outcome.panicked);
        assert_eq!(
            outcome.recovered_saves.len(),
            3,
            "every accepted concurrent enqueue must be retained by recovery"
        );
        assert!(outcome.recovered_saves.contains(&first_save));
        assert!(outcome.recovered_saves.contains(&queued_save_a));
        assert!(outcome.recovered_saves.contains(&queued_save_b));
    }

    #[test]
    fn drop_reports_panic_and_releases_folder_owner() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("region");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let worker = start_paused_worker(&folder, 1);
            worker.enqueue(save_at(ChunkPos::ZERO, 1)).unwrap();
            worker.set_panic();
            drop(worker);
        }));
        assert!(result.is_ok(), "Drop must not unwind on worker panic");

        let mut replacement = start_worker(&folder, 1);
        let outcome = replacement.shutdown();
        assert!(outcome.first_error.is_none());
        if folder.exists() {
            std::fs::remove_dir_all(folder).unwrap();
        }
    }

    #[test]
    fn drop_unpauses_drains_and_joins_the_worker() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("region");
        let pos = ChunkPos::new(2, 6);
        {
            let worker = start_paused_worker(&folder, 1);
            worker.enqueue(save_at(pos, 17)).unwrap();
            // Drop releases the test pause, drains the accepted save, flushes,
            // closes, and joins before the storage can be reopened below.
        }

        let mut reader = open_reader(&folder);
        assert_read_marker(&mut reader, pos, 17);
        reader.close().unwrap();
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
