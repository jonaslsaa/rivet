//! Thread-confined runtime state, entity table, and callback dispatcher for
//! `rivet-ffi`.
//!
//! # Thread confinement (`OWNERSHIP.md` §JVM-adapter, `DECISIONS.md` D5/D6)
//!
//! A runtime is owned by the OS thread that created it with
//! `rivet_ffi_v1_init` (the host's dedicated tick-synchronized thread). Its
//! state lives in that thread's thread-local table and is never reachable from
//! any other thread, so it needs no lock. Event callbacks run synchronously on
//! the owner thread, which means an invocation can only be in flight on the
//! owner's own call stack.
//!
//! A lock-free global slot table records only `(generation, owner thread tag)`
//! per runtime slot. It keeps runtime handles globally unique (no ABA aliasing
//! across threads) and lets a call from any other thread fail with
//! [`RivetStatus::WrongThread`] without touching the runtime's state.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::abi::{
    RIVET_EVENT_CANCEL, RIVET_FFI_MAX_PAYLOAD_BYTES, RivetBlockPos, RivetByteView,
    RivetCallbackToken, RivetEntityHandle, RivetEntityInfo, RivetEventCallback, RivetOwnedBytes,
    RivetRuntimeHandle, RivetStatus,
};
use crate::handle_table::{
    GenerationalTable, HandleInsertError, HandleLookupError, next_generation,
};

const MAX_RUNTIMES: usize = 64;
const MAX_ENTITIES_PER_RUNTIME: u32 = 65_536;
const MAX_CALLBACKS_PER_RUNTIME: u32 = 4_096;

/// Ownership word per runtime slot: the high 32 bits hold the slot generation
/// (`0` = never allocated), the low 32 bits hold the owner thread tag
/// (`0` = free). Only the owner thread writes a claimed word; other threads
/// only claim free words with a compare-exchange.
static RUNTIME_SLOTS: [AtomicU64; MAX_RUNTIMES] = [const { AtomicU64::new(0) }; MAX_RUNTIMES];

/// Source of process-unique, never-reused thread tags (`0` is reserved).
static NEXT_THREAD_TAG: AtomicU32 = AtomicU32::new(1);

thread_local! {
    /// This thread's tag, or `0` if it has never created a runtime.
    static THREAD_TAG: Cell<u32> = const { Cell::new(0) };
    /// Runtimes owned by this thread, indexed by runtime slot.
    static OWNED_RUNTIMES: RefCell<OwnedRuntimes> = const {
        RefCell::new(OwnedRuntimes { slots: Vec::new() })
    };
}

/// Per-thread runtime table. Dropping it at thread exit releases every slot
/// the thread still owns so a dead thread never pins runtime capacity.
struct OwnedRuntimes {
    slots: Vec<Option<RuntimeState>>,
}

impl Drop for OwnedRuntimes {
    fn drop(&mut self) {
        for (slot, entry) in self.slots.iter().enumerate() {
            if let Some(state) = entry {
                release_slot(slot, state.generation);
            }
        }
    }
}

pub(crate) struct EntityRecord {
    pub type_id: u32,
    pub pos: RivetBlockPos,
    pub name: String,
    pub alive: bool,
}

struct CallbackRecord {
    event_kind: u32,
    callback: RivetEventCallback,
    user_data: u64,
    /// Invocations of this callback currently on the owner thread's stack.
    active_calls: u32,
}

struct RuntimeState {
    generation: u32,
    tick_number: u64,
    /// `publish_event` dispatches of this runtime currently on the stack.
    dispatch_depth: u32,
    entities: GenerationalTable<EntityRecord>,
    callbacks: GenerationalTable<CallbackRecord>,
    /// Deterministic registration order of live callback tokens.
    callback_order: Vec<RivetCallbackToken>,
}

impl RuntimeState {
    fn new(generation: u32) -> Self {
        Self {
            generation,
            tick_number: 0,
            dispatch_depth: 0,
            entities: GenerationalTable::new(MAX_ENTITIES_PER_RUNTIME),
            callbacks: GenerationalTable::new(MAX_CALLBACKS_PER_RUNTIME),
            callback_order: Vec::new(),
        }
    }

    fn entity(&self, handle: RivetEntityHandle) -> Result<&EntityRecord, RivetStatus> {
        let rec = self
            .entities
            .get(handle.slot, handle.generation)
            .map_err(map_lookup_err)?;
        if rec.type_id != handle.type_id {
            return Err(RivetStatus::HandleInvalid);
        }
        Ok(rec)
    }
}

const fn pack_slot(generation: u32, owner: u32) -> u64 {
    ((generation as u64) << 32) | owner as u64
}

const fn unpack_slot(word: u64) -> (u32, u32) {
    ((word >> 32) as u32, word as u32)
}

fn release_slot(slot: usize, generation: u32) {
    RUNTIME_SLOTS[slot].store(pack_slot(next_generation(generation), 0), Ordering::Release);
}

fn claim_thread_tag() -> Result<u32, RivetStatus> {
    let tag = THREAD_TAG.get();
    if tag != 0 {
        return Ok(tag);
    }
    let tag = NEXT_THREAD_TAG
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |t| t.checked_add(1))
        .map_err(|_| RivetStatus::CapacityExceeded)?;
    THREAD_TAG.set(tag);
    Ok(tag)
}

/// Validates `handle` against the global slot table and confirms the calling
/// thread owns it.
fn resolve_slot(handle: RivetRuntimeHandle) -> Result<usize, RivetStatus> {
    if handle.generation == 0 {
        return Err(RivetStatus::HandleInvalid);
    }
    let slot = handle.slot as usize;
    let word = RUNTIME_SLOTS.get(slot).ok_or(RivetStatus::HandleInvalid)?;
    let (generation, owner) = unpack_slot(word.load(Ordering::Acquire));
    if generation == 0 {
        return Err(RivetStatus::HandleInvalid);
    }
    if generation != handle.generation || owner == 0 {
        return Err(RivetStatus::HandleStale);
    }
    if owner != THREAD_TAG.get() {
        return Err(RivetStatus::WrongThread);
    }
    Ok(slot)
}

/// Runs `f` on the calling thread's state for `handle`. The state stays
/// borrowed while `f` runs, so `f` must never call foreign code.
fn with_runtime<R>(
    handle: RivetRuntimeHandle,
    f: impl FnOnce(&mut RuntimeState) -> Result<R, RivetStatus>,
) -> Result<R, RivetStatus> {
    let slot = resolve_slot(handle)?;
    OWNED_RUNTIMES
        .try_with(|owned| {
            let mut owned = owned.borrow_mut();
            let state = owned
                .slots
                .get_mut(slot)
                .and_then(Option::as_mut)
                .ok_or(RivetStatus::HandleStale)?;
            f(state)
        })
        // The thread is tearing down its thread-locals; its runtimes are gone.
        .unwrap_or(Err(RivetStatus::HandleStale))
}

/// Creates a runtime owned by the calling thread.
pub(crate) fn create_runtime() -> Result<RivetRuntimeHandle, RivetStatus> {
    let tag = claim_thread_tag()?;
    for (slot, word) in RUNTIME_SLOTS.iter().enumerate() {
        let current = word.load(Ordering::Acquire);
        let (generation, owner) = unpack_slot(current);
        if owner != 0 {
            continue;
        }
        let generation = if generation == 0 { 1 } else { generation };
        if word
            .compare_exchange(
                current,
                pack_slot(generation, tag),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            continue;
        }
        let stored = OWNED_RUNTIMES.try_with(|owned| {
            let mut owned = owned.borrow_mut();
            if owned.slots.len() <= slot {
                owned.slots.resize_with(slot + 1, || None);
            }
            owned.slots[slot] = Some(RuntimeState::new(generation));
        });
        if stored.is_err() {
            release_slot(slot, generation);
            return Err(RivetStatus::HandleStale);
        }
        return Ok(RivetRuntimeHandle {
            slot: slot as u32,
            generation,
        });
    }
    Err(RivetStatus::CapacityExceeded)
}

/// Destroys a runtime owned by the calling thread, dropping its entities and
/// callbacks and advancing its slot generation.
///
/// Rejected with [`RivetStatus::CallbackReentrantUnregister`], leaving the
/// runtime fully intact, while any `publish_event` dispatch of this runtime is
/// on the stack (i.e. when called from inside one of its callbacks). Otherwise
/// no callback invocation can be in flight, so shutdown is quiescent on return.
pub(crate) fn shutdown_runtime(handle: RivetRuntimeHandle) -> Result<(), RivetStatus> {
    let slot = resolve_slot(handle)?;
    let removed = OWNED_RUNTIMES
        .try_with(|owned| {
            let mut owned = owned.borrow_mut();
            let entry = owned.slots.get_mut(slot).ok_or(RivetStatus::HandleStale)?;
            let state = entry.as_ref().ok_or(RivetStatus::HandleStale)?;
            if state.dispatch_depth > 0 {
                return Err(RivetStatus::CallbackReentrantUnregister);
            }
            let state = entry.take().ok_or(RivetStatus::HandleStale)?;
            release_slot(slot, state.generation);
            Ok(state)
        })
        .unwrap_or(Err(RivetStatus::HandleStale))?;
    drop(removed);
    Ok(())
}

pub(crate) fn tick(handle: RivetRuntimeHandle) -> Result<u64, RivetStatus> {
    with_runtime(handle, |state| {
        state.tick_number = state.tick_number.wrapping_add(1);
        Ok(state.tick_number)
    })
}

pub(crate) fn spawn_entity(
    handle: RivetRuntimeHandle,
    type_id: u32,
    pos: RivetBlockPos,
    name: String,
) -> Result<RivetEntityHandle, RivetStatus> {
    with_runtime(handle, |state| {
        let (slot, generation) = state
            .entities
            .insert(EntityRecord {
                type_id,
                pos,
                name,
                alive: true,
            })
            .map_err(map_insert_err)?;
        Ok(RivetEntityHandle {
            slot,
            generation,
            type_id,
            flags: 0,
        })
    })
}

pub(crate) fn lookup_entity(
    runtime: RivetRuntimeHandle,
    handle: RivetEntityHandle,
) -> Result<RivetEntityInfo, RivetStatus> {
    with_runtime(runtime, |state| {
        let rec = state.entity(handle)?;
        Ok(RivetEntityInfo {
            handle,
            pos: rec.pos,
            alive: u32::from(rec.alive),
            reserved: 0,
        })
    })
}

pub(crate) fn describe_entity_json(
    runtime: RivetRuntimeHandle,
    handle: RivetEntityHandle,
) -> Result<Vec<u8>, RivetStatus> {
    with_runtime(runtime, |state| {
        let rec = state.entity(handle)?;
        let escaped_name = escape_json_string(&rec.name);
        let json = format!(
            "{{\"slot\":{},\"generation\":{},\"type_id\":{},\"world_id\":{},\"x\":{},\"y\":{},\"z\":{},\"alive\":{},\"name\":\"{}\"}}",
            handle.slot,
            handle.generation,
            rec.type_id,
            rec.pos.world_id,
            rec.pos.x,
            rec.pos.y,
            rec.pos.z,
            rec.alive,
            escaped_name,
        );
        Ok(json.into_bytes())
    })
}

pub(crate) fn despawn_entity(
    runtime: RivetRuntimeHandle,
    handle: RivetEntityHandle,
) -> Result<(), RivetStatus> {
    with_runtime(runtime, |state| {
        state.entity(handle)?;
        state
            .entities
            .remove(handle.slot, handle.generation)
            .map_err(map_lookup_err)?;
        Ok(())
    })
}

pub(crate) fn register_callback(
    runtime: RivetRuntimeHandle,
    event_kind: u32,
    callback: RivetEventCallback,
    user_data: u64,
) -> Result<RivetCallbackToken, RivetStatus> {
    with_runtime(runtime, |state| {
        let (slot, generation) = state
            .callbacks
            .insert(CallbackRecord {
                event_kind,
                callback,
                user_data,
                active_calls: 0,
            })
            .map_err(map_insert_err)?;
        let token = RivetCallbackToken { slot, generation };
        state.callback_order.push(token);
        Ok(token)
    })
}

/// Unregisters a callback. On `Ok`, no invocation of it is in flight and it
/// will never be invoked again, including later in a dispatch that is
/// currently on the stack.
///
/// Rejected with [`RivetStatus::CallbackReentrantUnregister`], leaving the
/// callback registered, while that callback is itself executing (it is on the
/// owner thread's stack, so waiting for it would deadlock).
pub(crate) fn unregister_callback(
    runtime: RivetRuntimeHandle,
    token: RivetCallbackToken,
) -> Result<(), RivetStatus> {
    with_runtime(runtime, |state| {
        let rec = state
            .callbacks
            .get(token.slot, token.generation)
            .map_err(|_| RivetStatus::CallbackNotFound)?;
        if rec.active_calls > 0 {
            return Err(RivetStatus::CallbackReentrantUnregister);
        }
        state
            .callbacks
            .remove(token.slot, token.generation)
            .map_err(|_| RivetStatus::CallbackNotFound)?;
        state.callback_order.retain(|t| *t != token);
        Ok(())
    })
}

/// Decrements the dispatch depth when a `publish_event` dispatch leaves the
/// stack, including on unwind.
struct DispatchGuard {
    runtime: RivetRuntimeHandle,
}

impl Drop for DispatchGuard {
    fn drop(&mut self) {
        let _ = with_runtime(self.runtime, |state| {
            state.dispatch_depth = state.dispatch_depth.saturating_sub(1);
            Ok(())
        });
    }
}

/// Releases one claimed callback invocation, including on unwind.
struct InvocationGuard {
    runtime: RivetRuntimeHandle,
    token: RivetCallbackToken,
}

impl Drop for InvocationGuard {
    fn drop(&mut self) {
        let _ = with_runtime(self.runtime, |state| {
            if let Ok(rec) = state
                .callbacks
                .get_mut(self.token.slot, self.token.generation)
            {
                rec.active_calls = rec.active_calls.saturating_sub(1);
            }
            Ok(())
        });
    }
}

/// Dispatches `event_kind` with a copied bounded payload, synchronously on the
/// owner thread, to the callbacks registered for it, in registration order.
///
/// The target list is the registration order at dispatch start. Each
/// invocation is claimed just before it runs; a target unregistered in the
/// meantime (e.g. by an earlier callback of the same dispatch) is skipped and
/// not counted as delivered.
pub(crate) fn publish_event(
    runtime: RivetRuntimeHandle,
    event_kind: u32,
    payload: &[u8],
) -> Result<(u32, bool), RivetStatus> {
    let payload_len = u32::try_from(payload.len()).map_err(|_| RivetStatus::BufferTooLarge)?;
    let payload_ptr = if payload.is_empty() {
        std::ptr::null()
    } else {
        payload.as_ptr()
    };

    let targets = with_runtime(runtime, |state| {
        state.dispatch_depth += 1;
        let targets: Vec<RivetCallbackToken> = state
            .callback_order
            .iter()
            .copied()
            .filter(|t| {
                state
                    .callbacks
                    .get(t.slot, t.generation)
                    .is_ok_and(|rec| rec.event_kind == event_kind)
            })
            .collect();
        Ok(targets)
    })?;
    let _dispatch = DispatchGuard { runtime };

    let mut delivered = 0u32;
    let mut cancelled = false;
    for token in targets {
        let claimed = with_runtime(runtime, |state| {
            Ok(state
                .callbacks
                .get_mut(token.slot, token.generation)
                .ok()
                .map(|rec| {
                    rec.active_calls += 1;
                    (rec.callback, rec.user_data)
                }))
        })?;
        let Some((callback, user_data)) = claimed else {
            continue;
        };
        let _invocation = InvocationGuard { runtime, token };
        let ret = callback(event_kind, payload_ptr, payload_len, user_data);
        delivered += 1;
        if ret == RIVET_EVENT_CANCEL {
            cancelled = true;
        }
    }

    Ok((delivered, cancelled))
}

/// Validates and copies a caller-borrowed [`RivetByteView`] into an owned
/// `Vec<u8>`. Never retains `view.ptr`.
///
/// # Safety
/// Caller must ensure that if `view.len > 0`, `view.ptr` is valid for reads of
/// `view.len` bytes for the duration of this call.
#[allow(unsafe_code)]
pub(crate) unsafe fn copy_byte_view(view: RivetByteView) -> Result<Vec<u8>, RivetStatus> {
    if view.len > RIVET_FFI_MAX_PAYLOAD_BYTES {
        return Err(RivetStatus::BufferTooLarge);
    }
    if view.len == 0 {
        return Ok(Vec::new());
    }
    if view.ptr.is_null() {
        return Err(RivetStatus::NullPointer);
    }
    // SAFETY: Caller guarantees `view.ptr` is valid for `view.len` bytes for
    // the duration of the synchronous FFI call; we immediately copy out.
    let slice = unsafe { std::slice::from_raw_parts(view.ptr, view.len as usize) };
    Ok(slice.to_vec())
}

/// Transfers ownership of a Rust `Vec<u8>` into a [`RivetOwnedBytes`] buffer
/// that must be freed via `rivet_ffi_v1_bytes_free`.
pub(crate) fn vec_into_owned_bytes(vec: Vec<u8>) -> Result<RivetOwnedBytes, RivetStatus> {
    let len = u32::try_from(vec.len()).map_err(|_| RivetStatus::BufferTooLarge)?;
    let cap = u32::try_from(vec.capacity()).map_err(|_| RivetStatus::BufferTooLarge)?;
    let mut vec = std::mem::ManuallyDrop::new(vec);
    Ok(RivetOwnedBytes {
        ptr: vec.as_mut_ptr(),
        len,
        cap,
    })
}

/// Releases a [`RivetOwnedBytes`] previously allocated by `rivet-ffi`.
///
/// # Safety
/// `bytes` must either be the zero/null default or an unfreed buffer returned
/// by `rivet-ffi`.
#[allow(unsafe_code)]
pub(crate) unsafe fn free_owned_bytes(bytes: RivetOwnedBytes) -> RivetStatus {
    if bytes.ptr.is_null() {
        return if bytes.len == 0 && bytes.cap == 0 {
            RivetStatus::Ok
        } else {
            RivetStatus::InvalidArgument
        };
    }
    if bytes.len > bytes.cap {
        return RivetStatus::InvalidArgument;
    }
    // SAFETY: Caller guarantees `bytes` came from `vec_into_owned_bytes` and
    // has not been freed previously.
    let vec = unsafe { Vec::from_raw_parts(bytes.ptr, bytes.len as usize, bytes.cap as usize) };
    drop(vec);
    RivetStatus::Ok
}

fn map_lookup_err(err: HandleLookupError) -> RivetStatus {
    match err {
        HandleLookupError::Invalid => RivetStatus::HandleInvalid,
        HandleLookupError::Stale => RivetStatus::HandleStale,
    }
}

fn map_insert_err(err: HandleInsertError) -> RivetStatus {
    match err {
        HandleInsertError::CapacityExceeded => RivetStatus::CapacityExceeded,
    }
}

fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}
