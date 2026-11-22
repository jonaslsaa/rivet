#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex, OnceLock};
use std::thread;

use rivet_ffi::{
    RIVET_EVENT_CANCEL, RIVET_EVENT_CONTINUE, RIVET_FFI_ABI_MAJOR, RIVET_FFI_ABI_MINOR,
    RIVET_FFI_MAX_PAYLOAD_BYTES, RivetBlockPos, RivetByteView, RivetCallbackToken,
    RivetEntityHandle, RivetOwnedBytes, RivetRuntimeHandle, RivetStatus, rivet_ffi_v1_abi_version,
    rivet_ffi_v1_bytes_free, rivet_ffi_v1_entity_describe_json, rivet_ffi_v1_entity_despawn,
    rivet_ffi_v1_entity_lookup, rivet_ffi_v1_entity_spawn, rivet_ffi_v1_init,
    rivet_ffi_v1_publish_event, rivet_ffi_v1_register_event_callback, rivet_ffi_v1_shutdown,
    rivet_ffi_v1_tick, rivet_ffi_v1_unregister_event_callback,
};

fn byte_view(bytes: &[u8]) -> RivetByteView {
    RivetByteView {
        ptr: if bytes.is_empty() {
            std::ptr::null()
        } else {
            bytes.as_ptr()
        },
        len: u32::try_from(bytes.len()).expect("len fits in u32"),
        reserved: 0,
    }
}

#[test]
fn abi_version_negotiation_and_runtime_lifecycle() {
    let ver = rivet_ffi_v1_abi_version();
    assert_eq!(ver.major, RIVET_FFI_ABI_MAJOR);
    assert_eq!(ver.minor, RIVET_FFI_ABI_MINOR);

    // Wrong major or future minor is rejected fail-closed.
    assert_eq!(
        rivet_ffi_v1_init(RIVET_FFI_ABI_MAJOR + 1, 0).status,
        RivetStatus::AbiVersionMismatch
    );
    assert_eq!(
        rivet_ffi_v1_init(RIVET_FFI_ABI_MAJOR, RIVET_FFI_ABI_MINOR + 1).status,
        RivetStatus::AbiVersionMismatch
    );

    let init = rivet_ffi_v1_init(RIVET_FFI_ABI_MAJOR, RIVET_FFI_ABI_MINOR);
    assert_eq!(init.status, RivetStatus::Ok);
    assert_ne!(init.handle.generation, 0);

    let t1 = rivet_ffi_v1_tick(init.handle);
    assert_eq!(t1.status, RivetStatus::Ok);
    assert_eq!(t1.tick_number, 1);

    let t2 = rivet_ffi_v1_tick(init.handle);
    assert_eq!(t2.status, RivetStatus::Ok);
    assert_eq!(t2.tick_number, 2);

    assert_eq!(rivet_ffi_v1_shutdown(init.handle), RivetStatus::Ok);

    // Stale runtime handle is rejected after shutdown.
    assert_eq!(
        rivet_ffi_v1_tick(init.handle).status,
        RivetStatus::HandleStale
    );
    assert_eq!(rivet_ffi_v1_shutdown(init.handle), RivetStatus::HandleStale);

    // Zero handle is always invalid.
    assert_eq!(
        rivet_ffi_v1_tick(RivetRuntimeHandle::default()).status,
        RivetStatus::HandleInvalid
    );
}

#[test]
fn entity_generational_handles_aba_protection_and_owned_bytes() {
    let rt = rivet_ffi_v1_init(RIVET_FFI_ABI_MAJOR, RIVET_FFI_ABI_MINOR);
    assert_eq!(rt.status, RivetStatus::Ok);

    let pos = RivetBlockPos {
        world_id: 1,
        x: 10,
        y: 64,
        z: -20,
    };

    // SAFETY: slice pointer is valid for the duration of the synchronous call.
    let spawn1 = unsafe { rivet_ffi_v1_entity_spawn(rt.handle, 42, pos, byte_view(b"zombie_1")) };
    assert_eq!(spawn1.status, RivetStatus::Ok);
    assert_eq!(spawn1.handle.generation, 1);

    let lookup1 = rivet_ffi_v1_entity_lookup(rt.handle, spawn1.handle);
    assert_eq!(lookup1.status, RivetStatus::Ok);
    assert_eq!(lookup1.info.pos, pos);
    assert_eq!(lookup1.info.alive, 1);

    // Wrong type_id on an otherwise valid slot/generation is rejected.
    let mut bad_type = spawn1.handle;
    bad_type.type_id = 999;
    assert_eq!(
        rivet_ffi_v1_entity_lookup(rt.handle, bad_type).status,
        RivetStatus::HandleInvalid
    );

    // Owned JSON bytes allocation & explicit free.
    let desc = rivet_ffi_v1_entity_describe_json(rt.handle, spawn1.handle);
    assert_eq!(desc.status, RivetStatus::Ok);
    assert!(!desc.bytes.ptr.is_null());
    // SAFETY: `desc.bytes` is owned by us until `rivet_ffi_v1_bytes_free`.
    let json = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(
            desc.bytes.ptr,
            desc.bytes.len as usize,
        ))
        .expect("valid UTF-8 JSON")
        .to_owned()
    };
    assert!(json.contains("\"name\":\"zombie_1\""));
    assert!(json.contains("\"x\":10"));
    // SAFETY: freeing the buffer returned by `rivet_ffi_v1_entity_describe_json` once.
    assert_eq!(
        unsafe { rivet_ffi_v1_bytes_free(desc.bytes) },
        RivetStatus::Ok
    );

    // Despawn and verify stale rejection + ABA protection on slot reuse.
    assert_eq!(
        rivet_ffi_v1_entity_despawn(rt.handle, spawn1.handle),
        RivetStatus::Ok
    );
    assert_eq!(
        rivet_ffi_v1_entity_lookup(rt.handle, spawn1.handle).status,
        RivetStatus::HandleStale
    );

    // SAFETY: valid slice.
    let spawn2 = unsafe { rivet_ffi_v1_entity_spawn(rt.handle, 42, pos, byte_view(b"zombie_2")) };
    assert_eq!(spawn2.status, RivetStatus::Ok);
    assert_eq!(spawn2.handle.slot, spawn1.handle.slot);
    assert_eq!(spawn2.handle.generation, spawn1.handle.generation + 1);

    // Old handle still fails with HandleStale even though the slot was reused.
    assert_eq!(
        rivet_ffi_v1_entity_lookup(rt.handle, spawn1.handle).status,
        RivetStatus::HandleStale
    );
    assert_eq!(
        rivet_ffi_v1_entity_lookup(rt.handle, spawn2.handle).status,
        RivetStatus::Ok
    );

    // Zero entity handle is invalid.
    assert_eq!(
        rivet_ffi_v1_entity_lookup(rt.handle, RivetEntityHandle::default()).status,
        RivetStatus::HandleInvalid
    );

    assert_eq!(rivet_ffi_v1_shutdown(rt.handle), RivetStatus::Ok);
}

#[test]
fn bounded_buffer_and_validation_negatives() {
    let rt = rivet_ffi_v1_init(RIVET_FFI_ABI_MAJOR, RIVET_FFI_ABI_MINOR);
    assert_eq!(rt.status, RivetStatus::Ok);

    let pos = RivetBlockPos::default();

    // Null pointer with non-zero length -> NullPointer.
    let null_nonempty = RivetByteView {
        ptr: std::ptr::null(),
        len: 4,
        reserved: 0,
    };
    assert_eq!(
        // SAFETY: intentionally testing null-pointer validation path.
        unsafe { rivet_ffi_v1_entity_spawn(rt.handle, 1, pos, null_nonempty) }.status,
        RivetStatus::NullPointer
    );

    // Oversized length (> 1 MiB) -> BufferTooLarge without dereferencing pointer.
    let oversized = RivetByteView {
        ptr: std::ptr::null(),
        len: RIVET_FFI_MAX_PAYLOAD_BYTES + 1,
        reserved: 0,
    };
    assert_eq!(
        // SAFETY: length check runs before any dereference.
        unsafe { rivet_ffi_v1_entity_spawn(rt.handle, 1, pos, oversized) }.status,
        RivetStatus::BufferTooLarge
    );

    // Invalid UTF-8 -> InvalidUtf8.
    let bad_utf8 = [0xffu8, 0xfeu8, 0xfdu8];
    assert_eq!(
        // SAFETY: valid slice of invalid UTF-8 bytes.
        unsafe { rivet_ffi_v1_entity_spawn(rt.handle, 1, pos, byte_view(&bad_utf8)) }.status,
        RivetStatus::InvalidUtf8
    );

    // Empty name with null pointer -> Ok.
    assert_eq!(
        // SAFETY: len == 0.
        unsafe { rivet_ffi_v1_entity_spawn(rt.handle, 1, pos, byte_view(&[])) }.status,
        RivetStatus::Ok
    );

    // bytes_free validation: default-zero is Ok; null with non-zero len is InvalidArgument.
    assert_eq!(
        // SAFETY: zeroed buffer.
        unsafe { rivet_ffi_v1_bytes_free(RivetOwnedBytes::default()) },
        RivetStatus::Ok
    );
    assert_eq!(
        // SAFETY: null pointer is rejected before Vec::from_raw_parts.
        unsafe {
            rivet_ffi_v1_bytes_free(RivetOwnedBytes {
                ptr: std::ptr::null_mut(),
                len: 1,
                cap: 1,
            })
        },
        RivetStatus::InvalidArgument
    );

    // Null callback registration -> NullPointer.
    assert_eq!(
        rivet_ffi_v1_register_event_callback(rt.handle, 7, None, 0).status,
        RivetStatus::NullPointer
    );

    assert_eq!(rivet_ffi_v1_shutdown(rt.handle), RivetStatus::Ok);
}

static CALL_COUNT: AtomicU32 = AtomicU32::new(0);
static LAST_USER_DATA: AtomicU64 = AtomicU64::new(0);

extern "C" fn counting_callback(
    _event_kind: u32,
    payload_ptr: *const u8,
    payload_len: u32,
    user_data: u64,
) -> i32 {
    CALL_COUNT.fetch_add(1, Ordering::SeqCst);
    LAST_USER_DATA.store(user_data, Ordering::SeqCst);
    if payload_len > 0 && !payload_ptr.is_null() {
        // SAFETY: valid for duration of callback.
        let slice = unsafe { std::slice::from_raw_parts(payload_ptr, payload_len as usize) };
        if slice == b"cancel" {
            return RIVET_EVENT_CANCEL;
        }
    }
    RIVET_EVENT_CONTINUE
}

#[test]
fn event_callback_dispatch_cancellation_and_unregister() {
    CALL_COUNT.store(0, Ordering::SeqCst);
    LAST_USER_DATA.store(0, Ordering::SeqCst);

    let rt = rivet_ffi_v1_init(RIVET_FFI_ABI_MAJOR, RIVET_FFI_ABI_MINOR);
    assert_eq!(rt.status, RivetStatus::Ok);

    let reg = rivet_ffi_v1_register_event_callback(rt.handle, 100, Some(counting_callback), 0xCAFE);
    assert_eq!(reg.status, RivetStatus::Ok);

    // SAFETY: valid slice.
    let res1 = unsafe { rivet_ffi_v1_publish_event(rt.handle, 100, byte_view(b"hello")) };
    assert_eq!(res1.status, RivetStatus::Ok);
    assert_eq!(res1.delivered_count, 1);
    assert_eq!(res1.cancelled, 0);
    assert_eq!(CALL_COUNT.load(Ordering::SeqCst), 1);
    assert_eq!(LAST_USER_DATA.load(Ordering::SeqCst), 0xCAFE);

    // SAFETY: valid slice.
    let res2 = unsafe { rivet_ffi_v1_publish_event(rt.handle, 100, byte_view(b"cancel")) };
    assert_eq!(res2.status, RivetStatus::Ok);
    assert_eq!(res2.delivered_count, 1);
    assert_eq!(res2.cancelled, 1);

    assert_eq!(
        rivet_ffi_v1_unregister_event_callback(rt.handle, reg.token),
        RivetStatus::Ok
    );
    // Second unregister fails with CallbackNotFound.
    assert_eq!(
        rivet_ffi_v1_unregister_event_callback(rt.handle, reg.token),
        RivetStatus::CallbackNotFound
    );

    // SAFETY: valid slice.
    let res3 = unsafe { rivet_ffi_v1_publish_event(rt.handle, 100, byte_view(b"after")) };
    assert_eq!(res3.status, RivetStatus::Ok);
    assert_eq!(res3.delivered_count, 0);

    assert_eq!(rivet_ffi_v1_shutdown(rt.handle), RivetStatus::Ok);
}

struct ReentrantState {
    runtime: RivetRuntimeHandle,
    token: RivetCallbackToken,
    observed_status: Option<RivetStatus>,
}

static REENTRANT_STATE: OnceLock<Mutex<Option<ReentrantState>>> = OnceLock::new();

extern "C" fn reentrant_unregister_callback(
    _event_kind: u32,
    _payload_ptr: *const u8,
    _payload_len: u32,
    _user_data: u64,
) -> i32 {
    let (runtime, token) = {
        let guard = REENTRANT_STATE.get().unwrap().lock().unwrap();
        let st = guard.as_ref().unwrap();
        (st.runtime, st.token)
    };
    let status = rivet_ffi_v1_unregister_event_callback(runtime, token);
    REENTRANT_STATE
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .observed_status = Some(status);
    RIVET_EVENT_CONTINUE
}

#[test]
fn callback_reentrant_self_unregister_returns_error_without_deadlock() {
    let cell = REENTRANT_STATE.get_or_init(|| Mutex::new(None));
    let rt = rivet_ffi_v1_init(RIVET_FFI_ABI_MAJOR, RIVET_FFI_ABI_MINOR);
    assert_eq!(rt.status, RivetStatus::Ok);

    let reg = rivet_ffi_v1_register_event_callback(
        rt.handle,
        200,
        Some(reentrant_unregister_callback),
        0,
    );
    assert_eq!(reg.status, RivetStatus::Ok);

    *cell.lock().unwrap() = Some(ReentrantState {
        runtime: rt.handle,
        token: reg.token,
        observed_status: None,
    });

    // SAFETY: empty slice.
    let res = unsafe { rivet_ffi_v1_publish_event(rt.handle, 200, byte_view(&[])) };
    assert_eq!(res.status, RivetStatus::Ok);
    assert_eq!(res.delivered_count, 1);

    let observed = cell.lock().unwrap().take().unwrap().observed_status;
    assert_eq!(observed, Some(RivetStatus::CallbackReentrantUnregister));

    assert_eq!(
        rivet_ffi_v1_unregister_event_callback(rt.handle, reg.token),
        RivetStatus::Ok
    );
    assert_eq!(rivet_ffi_v1_shutdown(rt.handle), RivetStatus::Ok);
}

struct QuiesceSync {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
    callback_done: Arc<AtomicBool>,
}

static QUIESCE_SYNC: OnceLock<Mutex<Option<QuiesceSync>>> = OnceLock::new();

extern "C" fn blocking_quiesce_callback(
    _event_kind: u32,
    _payload_ptr: *const u8,
    _payload_len: u32,
    _user_data: u64,
) -> i32 {
    let sync = {
        let guard = QUIESCE_SYNC.get().unwrap().lock().unwrap();
        let s = guard.as_ref().unwrap();
        (
            Arc::clone(&s.entered),
            Arc::clone(&s.release),
            Arc::clone(&s.callback_done),
        )
    };
    sync.0.wait();
    sync.1.wait();
    sync.2.store(true, Ordering::SeqCst);
    RIVET_EVENT_CONTINUE
}

#[test]
fn unregister_blocks_until_inflight_callback_completes_quiescence() {
    let cell = QUIESCE_SYNC.get_or_init(|| Mutex::new(None));
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let callback_done = Arc::new(AtomicBool::new(false));
    let unregister_done = Arc::new(AtomicBool::new(false));

    *cell.lock().unwrap() = Some(QuiesceSync {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
        callback_done: Arc::clone(&callback_done),
    });

    let rt = rivet_ffi_v1_init(RIVET_FFI_ABI_MAJOR, RIVET_FFI_ABI_MINOR);
    assert_eq!(rt.status, RivetStatus::Ok);

    let reg =
        rivet_ffi_v1_register_event_callback(rt.handle, 300, Some(blocking_quiesce_callback), 0);
    assert_eq!(reg.status, RivetStatus::Ok);

    let rt_handle = rt.handle;
    let publisher = thread::spawn(move || {
        // SAFETY: empty slice.
        unsafe { rivet_ffi_v1_publish_event(rt_handle, 300, byte_view(&[])) }
    });

    // Wait until callback is actively executing on publisher thread.
    entered.wait();

    let token = reg.token;
    let unreg_done_flag = Arc::clone(&unregister_done);
    let cb_done_check = Arc::clone(&callback_done);
    let unregistrant = thread::spawn(move || {
        let status = rivet_ffi_v1_unregister_event_callback(rt_handle, token);
        assert_eq!(status, RivetStatus::Ok);
        // Must only return AFTER callback_done is true.
        assert!(cb_done_check.load(Ordering::SeqCst));
        unreg_done_flag.store(true, Ordering::SeqCst);
    });

    // Give unregistrant a brief window to enter the wait; it must still be blocked.
    thread::sleep(std::time::Duration::from_millis(30));
    assert!(!unregister_done.load(Ordering::SeqCst));

    // Release the callback and verify both threads complete cleanly.
    release.wait();
    let pub_res = publisher.join().expect("publisher joined");
    assert_eq!(pub_res.status, RivetStatus::Ok);
    assert_eq!(pub_res.delivered_count, 1);

    unregistrant.join().expect("unregistrant joined");
    assert!(unregister_done.load(Ordering::SeqCst));

    *cell.lock().unwrap() = None;
    assert_eq!(rivet_ffi_v1_shutdown(rt.handle), RivetStatus::Ok);
}
