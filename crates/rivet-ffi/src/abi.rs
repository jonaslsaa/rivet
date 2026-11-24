//! Fixed-width, versioned C ABI types and layout assertions for `rivet-ffi` v1.
//!
//! All boundary types use fixed-width integer primitives (`u32`, `i32`, `u64`),
//! explicit padding/reserved fields, and tagged status/value results. No `usize`,
//! arena pointers, retained caller buffers, or sentinel values cross the ABI.

/// Major ABI version. Incompatible layout or semantic changes bump this.
pub const RIVET_FFI_ABI_MAJOR: u32 = 1;
/// Minor ABI version. Additive backward-compatible symbols bump this.
pub const RIVET_FFI_ABI_MINOR: u32 = 0;
/// Patch ABI version. Bug fixes preserving ABI compatibility bump this.
pub const RIVET_FFI_ABI_PATCH: u32 = 0;

/// Maximum byte payload accepted by bounded C ABI buffer inputs (1 MiB).
pub const RIVET_FFI_MAX_PAYLOAD_BYTES: u32 = 1_048_576;

/// Event callback return code: allow event to continue.
pub const RIVET_EVENT_CONTINUE: i32 = 0;
/// Event callback return code: cancel event.
pub const RIVET_EVENT_CANCEL: i32 = 1;

/// Semantic version of the `rivet-ffi` C ABI.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RivetAbiVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub reserved: u32,
}

impl RivetAbiVersion {
    pub const CURRENT: Self = Self {
        major: RIVET_FFI_ABI_MAJOR,
        minor: RIVET_FFI_ABI_MINOR,
        patch: RIVET_FFI_ABI_PATCH,
        reserved: 0,
    };
}

/// Tagged status codes returned across the C ABI boundary.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RivetStatus {
    Ok = 0,
    InvalidArgument = 1,
    NullPointer = 2,
    AbiVersionMismatch = 3,
    HandleInvalid = 4,
    HandleStale = 5,
    BufferTooLarge = 6,
    InvalidUtf8 = 7,
    CallbackNotFound = 8,
    /// Unregister or shutdown called while the affected callback or dispatch
    /// is executing on the owner thread's stack; nothing was changed.
    CallbackReentrantUnregister = 9,
    /// The runtime handle is owned by a different thread (the one that called
    /// `rivet_ffi_v1_init`); nothing was changed.
    WrongThread = 10,
    PanicCaught = 11,
    CapacityExceeded = 12,
}

/// Opaque generational handle to an initialized `rivet-ffi` runtime instance.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RivetRuntimeHandle {
    pub slot: u32,
    pub generation: u32,
}

/// Opaque generational handle to an entity inside a runtime instance.
///
/// Mirrors `OWNERSHIP.md` §JVM-adapter: callers hold only `(slot, generation)`
/// plus a non-authoritative kind tag; every lookup re-resolves through the
/// owning handle table.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RivetEntityHandle {
    pub slot: u32,
    pub generation: u32,
    pub type_id: u32,
    pub flags: u32,
}

/// Fixed-width block position value marshaled by value across the C ABI.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RivetBlockPos {
    pub world_id: u32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Opaque generational token identifying a registered event callback.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RivetCallbackToken {
    pub slot: u32,
    pub generation: u32,
}

/// Borrowed input byte view passed by a foreign caller for the duration of a
/// single synchronous call.
///
/// `rivet-ffi` never retains `ptr` after returning from the call.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RivetByteView {
    pub ptr: *const u8,
    pub len: u32,
    pub reserved: u32,
}

/// Rust-allocated output byte buffer returned across the C ABI.
///
/// Must be released exactly once via [`crate::rivet_ffi_v1_bytes_free`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RivetOwnedBytes {
    pub ptr: *mut u8,
    pub len: u32,
    pub cap: u32,
}

impl Default for RivetOwnedBytes {
    fn default() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }
}

/// Snapshot of an entity record returned by value from `rivet_ffi_v1_entity_lookup`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RivetEntityInfo {
    pub handle: RivetEntityHandle,
    pub pos: RivetBlockPos,
    pub alive: u32,
    pub reserved: u32,
}

/// Tagged result for `rivet_ffi_v1_init`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RivetRuntimeResult {
    pub status: RivetStatus,
    pub reserved: u32,
    pub handle: RivetRuntimeHandle,
}

/// Tagged result for `rivet_ffi_v1_tick`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RivetTickResult {
    pub status: RivetStatus,
    pub reserved: u32,
    pub tick_number: u64,
}

/// Tagged result for `rivet_ffi_v1_entity_spawn`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RivetEntityHandleResult {
    pub status: RivetStatus,
    pub reserved: u32,
    pub handle: RivetEntityHandle,
}

/// Tagged result for `rivet_ffi_v1_entity_lookup`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RivetEntityLookupResult {
    pub status: RivetStatus,
    pub reserved: u32,
    pub info: RivetEntityInfo,
}

/// Tagged result for `rivet_ffi_v1_register_event_callback`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RivetCallbackResult {
    pub status: RivetStatus,
    pub reserved: u32,
    pub token: RivetCallbackToken,
}

/// Tagged result for `rivet_ffi_v1_publish_event`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RivetEventResult {
    pub status: RivetStatus,
    pub delivered_count: u32,
    pub cancelled: u32,
    pub reserved: u32,
}

/// Tagged result for `rivet_ffi_v1_entity_describe_json`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RivetBytesResult {
    pub status: RivetStatus,
    pub reserved: u32,
    pub bytes: RivetOwnedBytes,
}

/// C callback signature for event listeners.
///
/// `user_data` is an opaque caller-chosen `u64` correlation token (never a raw
/// Java heap pointer). `payload_ptr` is valid only for the duration of the
/// callback invocation and must not be retained.
///
/// Returns [`RIVET_EVENT_CONTINUE`] (`0`) to continue or [`RIVET_EVENT_CANCEL`]
/// (`1`) to mark the event cancelled.
pub type RivetEventCallback =
    extern "C" fn(event_kind: u32, payload_ptr: *const u8, payload_len: u32, user_data: u64) -> i32;

// Compile-time size and alignment assertions for every C ABI type.
const _: () = {
    use std::mem::{align_of, size_of};

    assert!(size_of::<RivetStatus>() == 4);
    assert!(align_of::<RivetStatus>() == 4);

    assert!(size_of::<RivetAbiVersion>() == 16);
    assert!(align_of::<RivetAbiVersion>() == 4);

    assert!(size_of::<RivetRuntimeHandle>() == 8);
    assert!(align_of::<RivetRuntimeHandle>() == 4);

    assert!(size_of::<RivetEntityHandle>() == 16);
    assert!(align_of::<RivetEntityHandle>() == 4);

    assert!(size_of::<RivetBlockPos>() == 16);
    assert!(align_of::<RivetBlockPos>() == 4);

    assert!(size_of::<RivetCallbackToken>() == 8);
    assert!(align_of::<RivetCallbackToken>() == 4);

    assert!(size_of::<RivetEntityInfo>() == 40);
    assert!(align_of::<RivetEntityInfo>() == 4);

    assert!(size_of::<RivetRuntimeResult>() == 16);
    assert!(align_of::<RivetRuntimeResult>() == 4);

    assert!(size_of::<RivetTickResult>() == 16);
    assert!(align_of::<RivetTickResult>() == 8);

    assert!(size_of::<RivetEntityHandleResult>() == 24);
    assert!(align_of::<RivetEntityHandleResult>() == 4);

    assert!(size_of::<RivetEntityLookupResult>() == 48);
    assert!(align_of::<RivetEntityLookupResult>() == 4);

    assert!(size_of::<RivetCallbackResult>() == 16);
    assert!(align_of::<RivetCallbackResult>() == 4);

    assert!(size_of::<RivetEventResult>() == 16);
    assert!(align_of::<RivetEventResult>() == 4);

    // Pointer-bearing structs have exact 64-bit / 32-bit layouts.
    #[cfg(target_pointer_width = "64")]
    {
        assert!(size_of::<RivetByteView>() == 16);
        assert!(align_of::<RivetByteView>() == 8);

        assert!(size_of::<RivetOwnedBytes>() == 16);
        assert!(align_of::<RivetOwnedBytes>() == 8);

        assert!(size_of::<RivetBytesResult>() == 24);
        assert!(align_of::<RivetBytesResult>() == 8);
    }
    #[cfg(target_pointer_width = "32")]
    {
        assert!(size_of::<RivetByteView>() == 12);
        assert!(align_of::<RivetByteView>() == 4);

        assert!(size_of::<RivetOwnedBytes>() == 12);
        assert!(align_of::<RivetOwnedBytes>() == 4);

        assert!(size_of::<RivetBytesResult>() == 20);
        assert!(align_of::<RivetBytesResult>() == 4);
    }
};
