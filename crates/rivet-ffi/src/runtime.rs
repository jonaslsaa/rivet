//! Runtime registry, entity table, and quiescent callback dispatcher for `rivet-ffi`.

use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::ThreadId;

use crate::abi::{
    RIVET_EVENT_CANCEL, RIVET_FFI_MAX_PAYLOAD_BYTES, RivetBlockPos, RivetByteView,
    RivetCallbackToken, RivetEntityHandle, RivetEntityInfo, RivetEventCallback, RivetOwnedBytes,
    RivetRuntimeHandle, RivetStatus,
};
use crate::handle_table::{GenerationalTable, HandleInsertError, HandleLookupError};

const MAX_RUNTIMES: u32 = 64;
const MAX_ENTITIES_PER_RUNTIME: u32 = 65_536;
const MAX_CALLBACKS_PER_RUNTIME: u32 = 4_096;

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
    active_calls: u32,
    unregistered: bool,
    calling_threads: Vec<ThreadId>,
}

struct RuntimeState {
    tick_number: u64,
    shutting_down: bool,
    entities: GenerationalTable<EntityRecord>,
    callbacks: GenerationalTable<CallbackRecord>,
    /// Deterministic registration order of live callback tokens.
    callback_order: Vec<RivetCallbackToken>,
}

pub(crate) struct RuntimeInstance {
    state: Mutex<RuntimeState>,
    quiesce_cv: Condvar,
}

impl RuntimeInstance {
    fn new() -> Self {
        Self {
            state: Mutex::new(RuntimeState {
                tick_number: 0,
                shutting_down: false,
                entities: GenerationalTable::new(MAX_ENTITIES_PER_RUNTIME),
                callbacks: GenerationalTable::new(MAX_CALLBACKS_PER_RUNTIME),
                callback_order: Vec::new(),
            }),
            quiesce_cv: Condvar::new(),
        }
    }

    pub(crate) fn tick(&self) -> Result<u64, RivetStatus> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.shutting_down {
            return Err(RivetStatus::HandleStale);
        }
        state.tick_number = state.tick_number.wrapping_add(1);
        Ok(state.tick_number)
    }

    pub(crate) fn spawn_entity(
        &self,
        type_id: u32,
        pos: RivetBlockPos,
        name: String,
    ) -> Result<RivetEntityHandle, RivetStatus> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.shutting_down {
            return Err(RivetStatus::HandleStale);
        }
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
    }

    pub(crate) fn lookup_entity(
        &self,
        handle: RivetEntityHandle,
    ) -> Result<RivetEntityInfo, RivetStatus> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.shutting_down {
            return Err(RivetStatus::HandleStale);
        }
        let rec = state
            .entities
            .get(handle.slot, handle.generation)
            .map_err(map_lookup_err)?;
        if rec.type_id != handle.type_id {
            return Err(RivetStatus::HandleInvalid);
        }
        Ok(RivetEntityInfo {
            handle,
            pos: rec.pos,
            alive: u32::from(rec.alive),
            reserved: 0,
        })
    }

    pub(crate) fn describe_entity_json(
        &self,
        handle: RivetEntityHandle,
    ) -> Result<Vec<u8>, RivetStatus> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.shutting_down {
            return Err(RivetStatus::HandleStale);
        }
        let rec = state
            .entities
            .get(handle.slot, handle.generation)
            .map_err(map_lookup_err)?;
        if rec.type_id != handle.type_id {
            return Err(RivetStatus::HandleInvalid);
        }
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
    }

    pub(crate) fn despawn_entity(&self, handle: RivetEntityHandle) -> Result<(), RivetStatus> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.shutting_down {
            return Err(RivetStatus::HandleStale);
        }
        let rec = state
            .entities
            .get(handle.slot, handle.generation)
            .map_err(map_lookup_err)?;
        if rec.type_id != handle.type_id {
            return Err(RivetStatus::HandleInvalid);
        }
        state
            .entities
            .remove(handle.slot, handle.generation)
            .map_err(map_lookup_err)?;
        Ok(())
    }

    pub(crate) fn register_callback(
        &self,
        event_kind: u32,
        callback: RivetEventCallback,
        user_data: u64,
    ) -> Result<RivetCallbackToken, RivetStatus> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.shutting_down {
            return Err(RivetStatus::HandleStale);
        }
        let (slot, generation) = state
            .callbacks
            .insert(CallbackRecord {
                event_kind,
                callback,
                user_data,
                active_calls: 0,
                unregistered: false,
                calling_threads: Vec::new(),
            })
            .map_err(map_insert_err)?;
        let token = RivetCallbackToken { slot, generation };
        state.callback_order.push(token);
        Ok(token)
    }

    /// Unregisters a callback and blocks until all in-flight invocations on
    /// other threads have finished (quiescence).
    ///
    /// Rejects reentrant unregistration from the thread currently executing
    /// that same callback with [`RivetStatus::CallbackReentrantUnregister`]
    /// instead of deadlocking.
    pub(crate) fn unregister_callback(&self, token: RivetCallbackToken) -> Result<(), RivetStatus> {
        let current_thread = std::thread::current().id();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());

        {
            let rec = state
                .callbacks
                .get_mut(token.slot, token.generation)
                .map_err(|_| RivetStatus::CallbackNotFound)?;
            if rec.calling_threads.contains(&current_thread) {
                return Err(RivetStatus::CallbackReentrantUnregister);
            }
            if rec.unregistered && rec.active_calls == 0 {
                return Err(RivetStatus::CallbackNotFound);
            }
            rec.unregistered = true;
        }

        state.callback_order.retain(|t| *t != token);

        loop {
            match state.callbacks.get(token.slot, token.generation) {
                Ok(rec) if rec.active_calls > 0 => {
                    state = self
                        .quiesce_cv
                        .wait(state)
                        .unwrap_or_else(|e| e.into_inner());
                }
                Ok(_) => {
                    let _ = state.callbacks.remove(token.slot, token.generation);
                    return Ok(());
                }
                Err(_) => return Ok(()),
            }
        }
    }

    /// Dispatches `event_kind` with a copied bounded payload to all registered
    /// callbacks in registration order.
    pub(crate) fn publish_event(
        &self,
        event_kind: u32,
        payload: &[u8],
    ) -> Result<(u32, bool), RivetStatus> {
        let current_thread = std::thread::current().id();

        let targets: Vec<(RivetCallbackToken, RivetEventCallback, u64)> = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.shutting_down {
                return Err(RivetStatus::HandleStale);
            }
            let order = state.callback_order.clone();
            let mut out = Vec::new();
            for token in order {
                if let Ok(rec) = state.callbacks.get_mut(token.slot, token.generation)
                    && !rec.unregistered
                    && rec.event_kind == event_kind
                {
                    rec.active_calls += 1;
                    rec.calling_threads.push(current_thread);
                    out.push((token, rec.callback, rec.user_data));
                }
            }
            out
        };

        // RAII guard ensures active_calls are always decremented even if a
        // callback unwinds or panics.
        struct ActiveCallGuard<'a> {
            runtime: &'a RuntimeInstance,
            remaining: Vec<RivetCallbackToken>,
            thread: ThreadId,
        }

        impl Drop for ActiveCallGuard<'_> {
            fn drop(&mut self) {
                if self.remaining.is_empty() {
                    return;
                }
                let mut state = self.runtime.state.lock().unwrap_or_else(|e| e.into_inner());
                let mut notify = false;
                for token in self.remaining.drain(..) {
                    if let Ok(rec) = state.callbacks.get_mut(token.slot, token.generation) {
                        rec.active_calls = rec.active_calls.saturating_sub(1);
                        if let Some(pos) =
                            rec.calling_threads.iter().position(|t| *t == self.thread)
                        {
                            rec.calling_threads.swap_remove(pos);
                        }
                        if rec.active_calls == 0 {
                            notify = true;
                        }
                    }
                }
                if notify {
                    self.runtime.quiesce_cv.notify_all();
                }
            }
        }

        let mut guard = ActiveCallGuard {
            runtime: self,
            remaining: targets.iter().map(|(t, _, _)| *t).collect(),
            thread: current_thread,
        };

        let payload_len = u32::try_from(payload.len()).map_err(|_| RivetStatus::BufferTooLarge)?;
        let payload_ptr = if payload.is_empty() {
            std::ptr::null()
        } else {
            payload.as_ptr()
        };

        let mut delivered = 0u32;
        let mut cancelled = false;

        for (idx, (_token, callback, user_data)) in targets.into_iter().enumerate() {
            let ret = callback(event_kind, payload_ptr, payload_len, user_data);
            delivered += 1;
            if ret == RIVET_EVENT_CANCEL {
                cancelled = true;
            }
            // Mark this token's invocation complete immediately so a waiting
            // unregister_callback can make progress without waiting for later
            // callbacks in the same dispatch.
            let done_token = guard.remaining[idx];
            guard.remaining[idx] = RivetCallbackToken::default();
            if done_token.generation != 0 {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if let Ok(rec) = state
                    .callbacks
                    .get_mut(done_token.slot, done_token.generation)
                {
                    rec.active_calls = rec.active_calls.saturating_sub(1);
                    if let Some(pos) = rec
                        .calling_threads
                        .iter()
                        .position(|t| *t == current_thread)
                    {
                        rec.calling_threads.swap_remove(pos);
                    }
                    if rec.active_calls == 0 {
                        self.quiesce_cv.notify_all();
                    }
                }
            }
        }
        guard.remaining.clear();

        Ok((delivered, cancelled))
    }

    /// Marks the runtime shutting down, unregisters all callbacks, and waits
    /// until every in-flight callback invocation finishes.
    pub(crate) fn shutdown_and_quiesce(&self) -> Result<(), RivetStatus> {
        let current_thread = std::thread::current().id();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.shutting_down = true;
        let tokens: Vec<RivetCallbackToken> = state.callback_order.drain(..).collect();
        for token in &tokens {
            if let Ok(rec) = state.callbacks.get_mut(token.slot, token.generation) {
                if rec.calling_threads.contains(&current_thread) {
                    return Err(RivetStatus::CallbackReentrantUnregister);
                }
                rec.unregistered = true;
            }
        }
        loop {
            let any_active = tokens.iter().any(|t| {
                state
                    .callbacks
                    .get(t.slot, t.generation)
                    .is_ok_and(|rec| rec.active_calls > 0)
            });
            if !any_active {
                break;
            }
            state = self
                .quiesce_cv
                .wait(state)
                .unwrap_or_else(|e| e.into_inner());
        }
        for token in tokens {
            let _ = state.callbacks.remove(token.slot, token.generation);
        }
        Ok(())
    }
}

fn registry() -> &'static Mutex<GenerationalTable<Arc<RuntimeInstance>>> {
    static REGISTRY: OnceLock<Mutex<GenerationalTable<Arc<RuntimeInstance>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(GenerationalTable::new(MAX_RUNTIMES)))
}

pub(crate) fn create_runtime() -> Result<RivetRuntimeHandle, RivetStatus> {
    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    let (slot, generation) = reg
        .insert(Arc::new(RuntimeInstance::new()))
        .map_err(map_insert_err)?;
    Ok(RivetRuntimeHandle { slot, generation })
}

pub(crate) fn lookup_runtime(
    handle: RivetRuntimeHandle,
) -> Result<Arc<RuntimeInstance>, RivetStatus> {
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    reg.get(handle.slot, handle.generation)
        .cloned()
        .map_err(map_lookup_err)
}

pub(crate) fn remove_runtime(
    handle: RivetRuntimeHandle,
) -> Result<Arc<RuntimeInstance>, RivetStatus> {
    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    reg.remove(handle.slot, handle.generation)
        .map_err(map_lookup_err)
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
