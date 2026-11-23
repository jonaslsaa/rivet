//! `rivet-ffi`: minimal versioned C ABI (`cdylib` + `rlib`) and generational
//! handle-table design for the Java FFM/Panama adapter (`OWNERSHIP.md` §JVM-adapter).
//!
//! # Contract Summary
//!
//! - **Fixed-width versioned types**: every ABI struct uses explicit `u32`/`i32`/`u64`
//!   fields with explicit padding and compile-time size/alignment assertions.
//! - **Generational handles**: runtimes, entities, and callback tokens use
//!   `(slot, generation)` pairs (`generation == 0` is invalid); removing an
//!   entry increments its slot generation so stale handles return
//!   [`RivetStatus::HandleStale`] rather than aliasing recycled slots.
//! - **Bounded buffers & ownership**: input [`RivetByteView`] buffers are
//!   bounds-checked (`<= 1 MiB`), validated, and copied synchronously — foreign
//!   memory is never retained. Output [`RivetOwnedBytes`] buffers are allocated
//!   by Rust and freed via [`rivet_ffi_v1_bytes_free`].
//! - **Thread confinement**: a runtime is owned by the OS thread that called
//!   [`rivet_ffi_v1_init`] (the host's dedicated tick-synchronized thread,
//!   `DECISIONS.md` D5/D6). Its state is thread-local and lock-free; every
//!   call taking its handle from any other thread returns
//!   [`RivetStatus::WrongThread`]. The owner must be a platform thread, not a
//!   Java virtual thread that may migrate between carriers.
//! - **Callback quiescence**: callbacks run synchronously on the owner thread,
//!   so an invocation is only ever in flight on the owner's own stack. When
//!   [`rivet_ffi_v1_unregister_event_callback`] or [`rivet_ffi_v1_shutdown`]
//!   returns [`RivetStatus::Ok`], no affected invocation is in flight and none
//!   will start. Unregistering a callback that is currently executing, or
//!   shutting down a runtime from inside one of its callbacks, returns
//!   [`RivetStatus::CallbackReentrantUnregister`] and changes nothing. A
//!   callback unregistered mid-dispatch before its turn is skipped.
//! - **Panic containment**: every `extern "C"` entrypoint wraps its body in
//!   `std::panic::catch_unwind` and maps any panic to [`RivetStatus::PanicCaught`].

pub mod abi;
mod c_api;
pub mod handle_table;
mod runtime;

pub use abi::{
    RIVET_EVENT_CANCEL, RIVET_EVENT_CONTINUE, RIVET_FFI_ABI_MAJOR, RIVET_FFI_ABI_MINOR,
    RIVET_FFI_ABI_PATCH, RIVET_FFI_MAX_PAYLOAD_BYTES, RivetAbiVersion, RivetBlockPos,
    RivetByteView, RivetBytesResult, RivetCallbackResult, RivetCallbackToken, RivetEntityHandle,
    RivetEntityHandleResult, RivetEntityInfo, RivetEntityLookupResult, RivetEventCallback,
    RivetEventResult, RivetOwnedBytes, RivetRuntimeHandle, RivetRuntimeResult, RivetStatus,
    RivetTickResult,
};
pub use c_api::{
    rivet_ffi_v1_abi_version, rivet_ffi_v1_bytes_free, rivet_ffi_v1_entity_describe_json,
    rivet_ffi_v1_entity_despawn, rivet_ffi_v1_entity_lookup, rivet_ffi_v1_entity_spawn,
    rivet_ffi_v1_init, rivet_ffi_v1_publish_event, rivet_ffi_v1_register_event_callback,
    rivet_ffi_v1_shutdown, rivet_ffi_v1_tick, rivet_ffi_v1_unregister_event_callback,
};
pub use handle_table::{GenerationalTable, HandleInsertError, HandleLookupError};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c_api::catch_ffi_panic;

    #[test]
    fn catch_ffi_panic_maps_unwind_to_panic_caught_status() {
        let status = catch_ffi_panic(
            || -> RivetStatus {
                panic!("simulated internal panic");
            },
            || RivetStatus::PanicCaught,
        );
        assert_eq!(status, RivetStatus::PanicCaught);
    }
}
