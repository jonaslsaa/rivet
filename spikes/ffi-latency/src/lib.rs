//! Minimal C ABI for the JVM-adapter FFI de-risk spike (epic #14, sub-issue #81).
//!
//! This crate is intentionally OUTSIDE the rivet cargo workspace (own `[workspace]`
//! table above). It exists only to measure the FFM boundary: scalar downcalls,
//! handle-table lookups that re-resolve per call (OWNERSHIP §JVM-adapter),
//! batched event publishes, and Rust->Java->Rust callbacks. No production
//! adapter architecture beyond what the spike proves.
//!
//! C ABI stability: only fixed-width integers and one `#[repr(C)]` struct cross
//! the boundary. IDs (world id, entity id) are marshaled as u64; never pointers
//! into Rust arenas.
//!
//! Panic safety: a Rust panic unwinding across `extern "C"` is UB, so every
//! exported fn wraps its body in `catch_unwind` and returns an error code on
//! panic. A Java/plugin *exception* can never unwind through this boundary
//! either: the Java upcall target (`RivetFfi.onCallback`) catches every
//! `Throwable` and returns an explicit status code, which the callback
//! dispatchers below convert into an ABI-safe error result (`ERR_CALLBACK`).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

pub const API_VERSION: u32 = 1;

/// Sentinel for "entity not found" from `rvf_get_entity_x`.
pub const NOT_FOUND: i32 = i32::MIN;
pub const ERR: i64 = -1;

/// Callback dispatch statuses, mirrored in `RivetFfi.java`.
pub const OK: i32 = 0;
pub const ERR_NO_CALLBACK: i32 = -1;
pub const ERR_CALLBACK: i32 = -2;

/// Event batch element. Java mirrors this exact layout (see RivetFfi.java).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Event {
    pub entity: u64,
    pub event_id: i32,
    pub payload: i64,
}

/// The callback registered from Java (an FFM upcall stub address).
/// `ctx` is an opaque u64 that Java passed at registration (unused by Rust).
/// Returns `OK` on success or a nonzero status if the Java callback threw, so a
/// foreign exception is contained on the Java side and never unwinds through Rust.
type Callback =
    extern "C" fn(world: u64, entity: u64, event_id: i32, payload: i64, ctx: u64) -> i32;

#[derive(Clone, Copy)]
struct Entity {
    x: i32,
}

struct World {
    /// Slot-map handle table: index -> live entity. Generation in `gens`.
    entities: Vec<Option<Entity>>,
    gens: Vec<u32>,
    free: Vec<u32>,
    tick: u64,
    callback: Option<Callback>,
    ctx: u64,
}

impl World {
    fn new() -> Self {
        World {
            entities: Vec::new(),
            gens: Vec::new(),
            free: Vec::new(),
            tick: 0,
            callback: None,
            ctx: 0,
        }
    }

    /// Generational EntityId: `(generation << 32) | index`. 0 is never a valid id.
    fn spawn(&mut self) -> u64 {
        let index = match self.free.pop() {
            Some(i) => i,
            None => {
                let i = self.entities.len() as u32;
                self.entities.push(None);
                self.gens.push(0);
                i
            }
        };
        let gen = self.gens[index as usize].wrapping_add(1);
        self.gens[index as usize] = gen;
        self.entities[index as usize] = Some(Entity { x: 0 });
        ((gen as u64) << 32) | index as u64
    }

    fn free_entity(&mut self, id: u64) -> bool {
        if let Some((gen, index)) = split_entity(id) {
            if self
                .entities
                .get(index as usize)
                .and_then(|e| e.as_ref())
                .is_some()
                && self.gens.get(index as usize) == Some(&gen)
            {
                self.entities[index as usize] = None;
                self.gens[index as usize] = gen.wrapping_add(1);
                self.free.push(index);
                return true;
            }
        }
        false
    }

    fn entity(&self, id: u64) -> Option<&Entity> {
        let (gen, index) = split_entity(id)?;
        let e = self.entities.get(index as usize)?.as_ref()?;
        (self.gens.get(index as usize) == Some(&gen)).then_some(e)
    }

    fn entity_mut(&mut self, id: u64) -> Option<&mut Entity> {
        let (gen, index) = split_entity(id)?;
        if self.gens.get(index as usize) != Some(&gen) {
            return None;
        }
        self.entities.get_mut(index as usize)?.as_mut()
    }
}

fn split_entity(id: u64) -> Option<(u32, u32)> {
    if id == 0 {
        return None;
    }
    Some(((id >> 32) as u32, (id & 0xffff_ffff) as u32))
}

static WORLDS: LazyLock<Mutex<HashMap<u64, World>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_WORLD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn with_world<T>(id: u64, f: impl FnOnce(&World) -> T) -> Option<T> {
    WORLDS.lock().ok().and_then(|g| g.get(&id).map(f))
}

fn with_world_mut<T>(id: u64, f: impl FnOnce(&mut World) -> T) -> Option<T> {
    WORLDS.lock().ok().and_then(|mut g| g.get_mut(&id).map(f))
}

/// Runs `f`, mapping a panic to `err`. Prevents unwind across the C boundary.
fn guarded<T: Copy>(err: T, f: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(_) => err,
    }
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// Returns the API version, so Java can assert ABI compatibility.
#[no_mangle]
pub extern "C" fn rfv_api_version() -> u32 {
    API_VERSION
}

/// Creates a world (handle table) and returns its id, or 0 on failure/panic.
#[no_mangle]
pub extern "C" fn rfv_create_world() -> u64 {
    guarded(0, || {
        let mut g = WORLDS.lock().unwrap();
        let id = NEXT_WORLD.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        g.insert(id, World::new());
        id
    })
}

#[no_mangle]
pub extern "C" fn rfv_destroy_world(id: u64) -> i32 {
    guarded(-1, || {
        let mut g = WORLDS.lock().unwrap();
        if g.remove(&id).is_some() {
            0
        } else {
            -1
        }
    })
}

// ---------------------------------------------------------------------------
// Handle table (OWNERSHIP §JVM-adapter: marshal IDs, re-resolve per call)
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn rfv_spawn_entity(world: u64) -> u64 {
    guarded(0, || with_world_mut(world, |w| w.spawn()).unwrap_or(0))
}

#[no_mangle]
pub extern "C" fn rfv_free_entity(world: u64, entity: u64) -> i32 {
    guarded(-1, || {
        with_world_mut(world, |w| if w.free_entity(entity) { 0 } else { -1 }).unwrap_or(-1)
    })
}

/// Handle lookup, re-resolved per call. Returns NOT_FOUND for a stale/missing id.
#[no_mangle]
pub extern "C" fn rfv_get_entity_x(world: u64, entity: u64) -> i32 {
    guarded(NOT_FOUND, || {
        with_world(world, |w| {
            w.entity(entity).map(|e| e.x).unwrap_or(NOT_FOUND)
        })
        .unwrap_or(NOT_FOUND)
    })
}

#[no_mangle]
pub extern "C" fn rfv_set_entity_x(world: u64, entity: u64, x: i32) -> i32 {
    guarded(-1, || {
        with_world_mut(world, |w| match w.entity_mut(entity) {
            Some(e) => {
                e.x = x;
                0
            }
            None => -1,
        })
        .unwrap_or(-1)
    })
}

// ---------------------------------------------------------------------------
// Scalar + tick
// ---------------------------------------------------------------------------

/// Bare scalar downcall (no table access): returns the world's tick counter.
#[no_mangle]
pub extern "C" fn rfv_tick(world: u64) -> u64 {
    guarded(0, || {
        with_world_mut(world, |w| {
            w.tick += 1;
            w.tick
        })
        .unwrap_or(0)
    })
}

// ---------------------------------------------------------------------------
// Batched mutation / event publish
// ---------------------------------------------------------------------------

/// Applies `count` events from `events` (a contiguous array of `Event`).
/// Returns the number applied, or ERR (-1) on invalid args/panic.
///
/// # Safety
///
/// `events` must be valid for `count * size_of::<Event>()` bytes, or null with
/// `count == 0`. The JVM passes a MemorySegment whose address is stable for the
/// call's duration.
#[no_mangle]
pub unsafe extern "C" fn rfv_apply_events(world: u64, events: *const Event, count: usize) -> i64 {
    guarded(ERR, || {
        if count > 0 && events.is_null() {
            return ERR;
        }
        let slice = if count == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(events, count)
        };
        with_world_mut(world, |w| {
            let mut applied = 0i64;
            for ev in slice {
                if let Some(e) = w.entity_mut(ev.entity) {
                    // Payload doubles as the new x for the spike; a real adapter
                    // would marshal a packed BlockPos instead.
                    e.x = ev.payload as i32;
                    applied += 1;
                }
            }
            applied
        })
        .unwrap_or(ERR)
    })
}

// ---------------------------------------------------------------------------
// Rust -> Java -> Rust callback (upcall stub)
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn rfv_register_callback(world: u64, cb: u64, ctx: u64) -> i32 {
    guarded(-1, || {
        let f: Callback = unsafe { std::mem::transmute(cb) };
        with_world_mut(world, |w| {
            w.callback = Some(f);
            w.ctx = ctx;
            0
        })
        .unwrap_or(-1)
    })
}

/// Synchronously invokes the registered Java callback once (Rust -> Java), and
/// the Java callback is expected to call back into Rust (Java -> Rust). Returns
/// `OK` if invoked, `ERR_NO_CALLBACK` if none is registered, or `ERR_CALLBACK`
/// if the Java callback threw (the exception was contained on the Java side and
/// did not unwind through Rust).
#[no_mangle]
pub extern "C" fn rfv_dispatch_callback(
    world: u64,
    entity: u64,
    event_id: i32,
    payload: i64,
) -> i32 {
    guarded(ERR_NO_CALLBACK, || {
        let cb = {
            let g = WORLDS.lock().unwrap();
            g.get(&world).map(|w| (w.callback, w.ctx))
        };
        match cb {
            Some((Some(f), ctx)) => {
                if f(world, entity, event_id, payload, ctx) == OK {
                    OK
                } else {
                    ERR_CALLBACK
                }
            }
            _ => ERR_NO_CALLBACK,
        }
    })
}

/// Event storm: dispatches `count` events to the Java callback for `entity`.
/// The Java callback records its own per-event latency (including its call back
/// into Rust) so Java can compute per-event percentiles. Returns the number of
/// events dispatched; the loop aborts at the first event whose callback returns
/// a nonzero status (Java threw), so a throwing callback stops the storm without
/// unwinding through Rust. Returns 0 if no callback is registered.
#[no_mangle]
pub extern "C" fn rfv_event_storm(world: u64, entity: u64, count: usize) -> u64 {
    guarded(0, || {
        let cb = {
            let g = WORLDS.lock().unwrap();
            g.get(&world).map(|w| (w.callback, w.ctx))
        };
        match cb {
            Some((Some(f), ctx)) => {
                for i in 0..count {
                    if f(world, entity, i as i32, 1, ctx) != OK {
                        return i as u64; // aborted at event i; that many dispatched
                    }
                }
                count as u64
            }
            _ => 0,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn ok_cb(_w: u64, _e: u64, _ev: i32, _p: i64, _ctx: u64) -> i32 {
        OK
    }

    extern "C" fn throwing_cb(_w: u64, _e: u64, _ev: i32, _p: i64, _ctx: u64) -> i32 {
        ERR_CALLBACK
    }

    fn addr(f: extern "C" fn(u64, u64, i32, i64, u64) -> i32) -> u64 {
        f as usize as u64
    }

    #[test]
    fn callback_error_status_is_propagated_not_unwound() {
        let w = rfv_create_world();
        let a = rfv_spawn_entity(w);

        // no callback registered -> explicit miss status
        assert_eq!(rfv_dispatch_callback(w, a, 1, 1), ERR_NO_CALLBACK);
        assert_eq!(rfv_event_storm(w, a, 5), 0);

        // healthy callback -> OK, storm dispatches all
        assert_eq!(rfv_register_callback(w, addr(ok_cb), 0), 0);
        assert_eq!(rfv_dispatch_callback(w, a, 1, 1), OK);
        assert_eq!(rfv_event_storm(w, a, 5), 5);

        // throwing callback -> explicit ERR_CALLBACK, storm aborts at event 0
        assert_eq!(rfv_register_callback(w, addr(throwing_cb), 0), 0);
        assert_eq!(rfv_dispatch_callback(w, a, 1, 1), ERR_CALLBACK);
        assert_eq!(rfv_event_storm(w, a, 5), 0);

        // world still functional after contained failures
        assert_eq!(rfv_get_entity_x(w, a), 0);
        assert_eq!(rfv_destroy_world(w), 0);
    }

    #[test]
    fn handle_generation_invalidates_stale_ids() {
        let mut w = World::new();
        let a = w.spawn();
        let b = w.spawn();
        assert!(a != b);

        w.free_entity(b);
        assert!(w.entity(b).is_none()); // stale handle rejected

        // The freed slot recycles with a fresh generation.
        let b2 = w.spawn();
        assert_ne!(b2, b);
        assert!(w.entity(b2).is_some());
        assert!(w.entity(b).is_none()); // old-generation handle still dead
    }

    #[test]
    fn handle_mutate_and_batch_apply() {
        let mut w = World::new();
        let a = w.spawn();
        assert_eq!(w.entity_mut(a).map(|e| e.x), Some(0));

        // batch apply: last-write-wins on duplicate entity, stale ids skipped
        let events = [
            Event {
                entity: a,
                event_id: 1,
                payload: 111,
            },
            Event {
                entity: a,
                event_id: 2,
                payload: 333,
            },
        ];
        let mut applied = 0;
        for ev in &events {
            if let Some(e) = w.entity_mut(ev.entity) {
                e.x = ev.payload as i32;
                applied += 1;
            }
        }
        assert_eq!(applied, 2);
        assert_eq!(w.entity(a).map(|e| e.x), Some(333));
    }

    #[test]
    fn rfv_create_destroy_and_api_version() {
        assert_eq!(rfv_api_version(), API_VERSION);
        let w = rfv_create_world();
        assert_ne!(w, 0);
        let a = rfv_spawn_entity(w);
        assert_ne!(a, 0);
        assert_eq!(rfv_get_entity_x(w, a), 0); // NOT_FOUND is only for missing ids
        assert_eq!(rfv_set_entity_x(w, a, 42), 0);
        assert_eq!(rfv_get_entity_x(w, a), 42);
        assert_eq!(rfv_tick(w), 1);
        assert_eq!(rfv_destroy_world(w), 0);
        // after destroy the world is gone
        assert_eq!(rfv_tick(w), 0);
    }
}
