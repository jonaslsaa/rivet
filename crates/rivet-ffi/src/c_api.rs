//! Exported `extern "C"` v1 symbols with production `catch_unwind` panic-to-status mapping.
#![allow(unsafe_code)]

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::abi::{
    RivetAbiVersion, RivetBlockPos, RivetByteView, RivetBytesResult, RivetCallbackResult,
    RivetCallbackToken, RivetEntityHandle, RivetEntityHandleResult, RivetEntityInfo,
    RivetEntityLookupResult, RivetEventCallback, RivetEventResult, RivetOwnedBytes,
    RivetRuntimeHandle, RivetRuntimeResult, RivetStatus, RivetTickResult,
};
use crate::runtime::{
    copy_byte_view, create_runtime, free_owned_bytes, lookup_runtime, remove_runtime,
    vec_into_owned_bytes,
};

/// Executes `f` inside `std::panic::catch_unwind`, mapping any Rust panic to
/// `on_panic()` so unwinding never crosses the `extern "C"` boundary.
pub(crate) fn catch_ffi_panic<T>(f: impl FnOnce() -> T, on_panic: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(payload) => {
            // Drop panic payload inside catch_unwind too so even a panicking
            // Drop impl on a panic payload cannot abort across FFI.
            let _ = catch_unwind(AssertUnwindSafe(|| drop(payload)));
            on_panic()
        }
    }
}

/// Returns the `rivet-ffi` C ABI semantic version.
#[unsafe(no_mangle)]
pub extern "C" fn rivet_ffi_v1_abi_version() -> RivetAbiVersion {
    RivetAbiVersion::CURRENT
}

/// Initializes a new `rivet-ffi` runtime instance after negotiating ABI
/// compatibility.
///
/// Major version must match [`RivetAbiVersion::CURRENT.major`] and
/// `expected_minor` must not exceed [`RivetAbiVersion::CURRENT.minor`].
#[unsafe(no_mangle)]
pub extern "C" fn rivet_ffi_v1_init(
    expected_major: u32,
    expected_minor: u32,
) -> RivetRuntimeResult {
    catch_ffi_panic(
        || {
            if expected_major != RivetAbiVersion::CURRENT.major
                || expected_minor > RivetAbiVersion::CURRENT.minor
            {
                return RivetRuntimeResult {
                    status: RivetStatus::AbiVersionMismatch,
                    reserved: 0,
                    handle: RivetRuntimeHandle::default(),
                };
            }
            match create_runtime() {
                Ok(handle) => RivetRuntimeResult {
                    status: RivetStatus::Ok,
                    reserved: 0,
                    handle,
                },
                Err(status) => RivetRuntimeResult {
                    status,
                    reserved: 0,
                    handle: RivetRuntimeHandle::default(),
                },
            }
        },
        || RivetRuntimeResult {
            status: RivetStatus::PanicCaught,
            reserved: 0,
            handle: RivetRuntimeHandle::default(),
        },
    )
}

/// Shuts down a runtime instance, advancing its generation and waiting for all
/// in-flight event callbacks to finish (quiescence).
#[unsafe(no_mangle)]
pub extern "C" fn rivet_ffi_v1_shutdown(runtime: RivetRuntimeHandle) -> RivetStatus {
    catch_ffi_panic(
        || {
            let instance = match remove_runtime(runtime) {
                Ok(inst) => inst,
                Err(status) => return status,
            };
            match instance.shutdown_and_quiesce() {
                Ok(()) => RivetStatus::Ok,
                Err(status) => status,
            }
        },
        || RivetStatus::PanicCaught,
    )
}

/// Advances the runtime's logical tick counter by one and returns the new tick.
#[unsafe(no_mangle)]
pub extern "C" fn rivet_ffi_v1_tick(runtime: RivetRuntimeHandle) -> RivetTickResult {
    catch_ffi_panic(
        || {
            let instance = match lookup_runtime(runtime) {
                Ok(inst) => inst,
                Err(status) => {
                    return RivetTickResult {
                        status,
                        reserved: 0,
                        tick_number: 0,
                    };
                }
            };
            match instance.tick() {
                Ok(tick_number) => RivetTickResult {
                    status: RivetStatus::Ok,
                    reserved: 0,
                    tick_number,
                },
                Err(status) => RivetTickResult {
                    status,
                    reserved: 0,
                    tick_number: 0,
                },
            }
        },
        || RivetTickResult {
            status: RivetStatus::PanicCaught,
            reserved: 0,
            tick_number: 0,
        },
    )
}

/// Spawns an entity in the runtime's generational handle table.
///
/// `name_utf8` is validated for bounded size (`<= 1 MiB`) and valid UTF-8 and
/// copied synchronously; `rivet-ffi` never retains `name_utf8.ptr`.
///
/// # Safety
/// If `name_utf8.len > 0`, `name_utf8.ptr` must be valid for reads of
/// `name_utf8.len` bytes for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rivet_ffi_v1_entity_spawn(
    runtime: RivetRuntimeHandle,
    type_id: u32,
    pos: RivetBlockPos,
    name_utf8: RivetByteView,
) -> RivetEntityHandleResult {
    catch_ffi_panic(
        || {
            let instance = match lookup_runtime(runtime) {
                Ok(inst) => inst,
                Err(status) => {
                    return RivetEntityHandleResult {
                        status,
                        reserved: 0,
                        handle: RivetEntityHandle::default(),
                    };
                }
            };
            // SAFETY: Caller satisfies `name_utf8` contract.
            let raw = match unsafe { copy_byte_view(name_utf8) } {
                Ok(bytes) => bytes,
                Err(status) => {
                    return RivetEntityHandleResult {
                        status,
                        reserved: 0,
                        handle: RivetEntityHandle::default(),
                    };
                }
            };
            let name = match String::from_utf8(raw) {
                Ok(s) => s,
                Err(_) => {
                    return RivetEntityHandleResult {
                        status: RivetStatus::InvalidUtf8,
                        reserved: 0,
                        handle: RivetEntityHandle::default(),
                    };
                }
            };
            match instance.spawn_entity(type_id, pos, name) {
                Ok(handle) => RivetEntityHandleResult {
                    status: RivetStatus::Ok,
                    reserved: 0,
                    handle,
                },
                Err(status) => RivetEntityHandleResult {
                    status,
                    reserved: 0,
                    handle: RivetEntityHandle::default(),
                },
            }
        },
        || RivetEntityHandleResult {
            status: RivetStatus::PanicCaught,
            reserved: 0,
            handle: RivetEntityHandle::default(),
        },
    )
}

/// Resolves a generational entity handle and returns its snapshot by value.
#[unsafe(no_mangle)]
pub extern "C" fn rivet_ffi_v1_entity_lookup(
    runtime: RivetRuntimeHandle,
    handle: RivetEntityHandle,
) -> RivetEntityLookupResult {
    catch_ffi_panic(
        || {
            let instance = match lookup_runtime(runtime) {
                Ok(inst) => inst,
                Err(status) => {
                    return RivetEntityLookupResult {
                        status,
                        reserved: 0,
                        info: RivetEntityInfo::default(),
                    };
                }
            };
            match instance.lookup_entity(handle) {
                Ok(info) => RivetEntityLookupResult {
                    status: RivetStatus::Ok,
                    reserved: 0,
                    info,
                },
                Err(status) => RivetEntityLookupResult {
                    status,
                    reserved: 0,
                    info: RivetEntityInfo::default(),
                },
            }
        },
        || RivetEntityLookupResult {
            status: RivetStatus::PanicCaught,
            reserved: 0,
            info: RivetEntityInfo::default(),
        },
    )
}

/// Allocates a UTF-8 JSON description of an entity as a [`RivetOwnedBytes`]
/// buffer. On `RivetStatus::Ok`, the caller owns `result.bytes` and must free
/// it exactly once via [`rivet_ffi_v1_bytes_free`].
#[unsafe(no_mangle)]
pub extern "C" fn rivet_ffi_v1_entity_describe_json(
    runtime: RivetRuntimeHandle,
    handle: RivetEntityHandle,
) -> RivetBytesResult {
    catch_ffi_panic(
        || {
            let instance = match lookup_runtime(runtime) {
                Ok(inst) => inst,
                Err(status) => {
                    return RivetBytesResult {
                        status,
                        reserved: 0,
                        bytes: RivetOwnedBytes::default(),
                    };
                }
            };
            match instance
                .describe_entity_json(handle)
                .and_then(vec_into_owned_bytes)
            {
                Ok(bytes) => RivetBytesResult {
                    status: RivetStatus::Ok,
                    reserved: 0,
                    bytes,
                },
                Err(status) => RivetBytesResult {
                    status,
                    reserved: 0,
                    bytes: RivetOwnedBytes::default(),
                },
            }
        },
        || RivetBytesResult {
            status: RivetStatus::PanicCaught,
            reserved: 0,
            bytes: RivetOwnedBytes::default(),
        },
    )
}

/// Despawns an entity, advancing its slot generation so stale handles fail.
#[unsafe(no_mangle)]
pub extern "C" fn rivet_ffi_v1_entity_despawn(
    runtime: RivetRuntimeHandle,
    handle: RivetEntityHandle,
) -> RivetStatus {
    catch_ffi_panic(
        || {
            let instance = match lookup_runtime(runtime) {
                Ok(inst) => inst,
                Err(status) => return status,
            };
            match instance.despawn_entity(handle) {
                Ok(()) => RivetStatus::Ok,
                Err(status) => status,
            }
        },
        || RivetStatus::PanicCaught,
    )
}

/// Registers an event callback for `event_kind` and returns its opaque
/// generational token.
///
/// Passing a null function pointer (`None`) returns [`RivetStatus::NullPointer`].
#[unsafe(no_mangle)]
pub extern "C" fn rivet_ffi_v1_register_event_callback(
    runtime: RivetRuntimeHandle,
    event_kind: u32,
    callback: Option<RivetEventCallback>,
    user_data: u64,
) -> RivetCallbackResult {
    catch_ffi_panic(
        || {
            let Some(cb) = callback else {
                return RivetCallbackResult {
                    status: RivetStatus::NullPointer,
                    reserved: 0,
                    token: RivetCallbackToken::default(),
                };
            };
            let instance = match lookup_runtime(runtime) {
                Ok(inst) => inst,
                Err(status) => {
                    return RivetCallbackResult {
                        status,
                        reserved: 0,
                        token: RivetCallbackToken::default(),
                    };
                }
            };
            match instance.register_callback(event_kind, cb, user_data) {
                Ok(token) => RivetCallbackResult {
                    status: RivetStatus::Ok,
                    reserved: 0,
                    token,
                },
                Err(status) => RivetCallbackResult {
                    status,
                    reserved: 0,
                    token: RivetCallbackToken::default(),
                },
            }
        },
        || RivetCallbackResult {
            status: RivetStatus::PanicCaught,
            reserved: 0,
            token: RivetCallbackToken::default(),
        },
    )
}

/// Unregisters a previously registered event callback and waits for any
/// in-flight invocations of that callback to finish (quiescence).
#[unsafe(no_mangle)]
pub extern "C" fn rivet_ffi_v1_unregister_event_callback(
    runtime: RivetRuntimeHandle,
    token: RivetCallbackToken,
) -> RivetStatus {
    catch_ffi_panic(
        || {
            let instance = match lookup_runtime(runtime) {
                Ok(inst) => inst,
                Err(status) => return status,
            };
            match instance.unregister_callback(token) {
                Ok(()) => RivetStatus::Ok,
                Err(status) => status,
            }
        },
        || RivetStatus::PanicCaught,
    )
}

/// Dispatches an event with a bounded borrowed byte payload to all matching
/// registered callbacks.
///
/// `payload` is copied synchronously before dispatch and never retained.
///
/// # Safety
/// If `payload.len > 0`, `payload.ptr` must be valid for reads of `payload.len`
/// bytes for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rivet_ffi_v1_publish_event(
    runtime: RivetRuntimeHandle,
    event_kind: u32,
    payload: RivetByteView,
) -> RivetEventResult {
    catch_ffi_panic(
        || {
            let instance = match lookup_runtime(runtime) {
                Ok(inst) => inst,
                Err(status) => {
                    return RivetEventResult {
                        status,
                        delivered_count: 0,
                        cancelled: 0,
                        reserved: 0,
                    };
                }
            };
            // SAFETY: Caller satisfies `payload` contract.
            let copied = match unsafe { copy_byte_view(payload) } {
                Ok(bytes) => bytes,
                Err(status) => {
                    return RivetEventResult {
                        status,
                        delivered_count: 0,
                        cancelled: 0,
                        reserved: 0,
                    };
                }
            };
            match instance.publish_event(event_kind, &copied) {
                Ok((delivered_count, cancelled)) => RivetEventResult {
                    status: RivetStatus::Ok,
                    delivered_count,
                    cancelled: u32::from(cancelled),
                    reserved: 0,
                },
                Err(status) => RivetEventResult {
                    status,
                    delivered_count: 0,
                    cancelled: 0,
                    reserved: 0,
                },
            }
        },
        || RivetEventResult {
            status: RivetStatus::PanicCaught,
            delivered_count: 0,
            cancelled: 0,
            reserved: 0,
        },
    )
}

/// Frees a [`RivetOwnedBytes`] buffer allocated by `rivet-ffi`.
///
/// # Safety
/// `bytes` must either be default-zeroed or an unfreed buffer returned by
/// `rivet-ffi`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rivet_ffi_v1_bytes_free(bytes: RivetOwnedBytes) -> RivetStatus {
    catch_ffi_panic(
        || {
            // SAFETY: Caller satisfies `bytes` ownership contract.
            unsafe { free_owned_bytes(bytes) }
        },
        || RivetStatus::PanicCaught,
    )
}
