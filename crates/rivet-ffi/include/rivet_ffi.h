#ifndef RIVET_FFI_H
#define RIVET_FFI_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RIVET_FFI_ABI_MAJOR 1u
#define RIVET_FFI_ABI_MINOR 0u
#define RIVET_FFI_ABI_PATCH 0u
#define RIVET_FFI_MAX_PAYLOAD_BYTES 1048576u

#define RIVET_EVENT_CONTINUE 0
#define RIVET_EVENT_CANCEL 1

typedef int32_t RivetStatus;
enum {
    RIVET_STATUS_OK = 0,
    RIVET_STATUS_INVALID_ARGUMENT = 1,
    RIVET_STATUS_NULL_POINTER = 2,
    RIVET_STATUS_ABI_VERSION_MISMATCH = 3,
    RIVET_STATUS_HANDLE_INVALID = 4,
    RIVET_STATUS_HANDLE_STALE = 5,
    RIVET_STATUS_BUFFER_TOO_LARGE = 6,
    RIVET_STATUS_INVALID_UTF8 = 7,
    RIVET_STATUS_CALLBACK_NOT_FOUND = 8,
    RIVET_STATUS_CALLBACK_REENTRANT_UNREGISTER = 9,
    RIVET_STATUS_WRONG_THREAD = 10,
    RIVET_STATUS_PANIC_CAUGHT = 11,
    RIVET_STATUS_CAPACITY_EXCEEDED = 12
};

typedef struct RivetAbiVersion {
    uint32_t major;
    uint32_t minor;
    uint32_t patch;
    uint32_t reserved;
} RivetAbiVersion;

typedef struct RivetRuntimeHandle {
    uint32_t slot;
    uint32_t generation;
} RivetRuntimeHandle;

typedef struct RivetEntityHandle {
    uint32_t slot;
    uint32_t generation;
    uint32_t type_id;
    uint32_t flags;
} RivetEntityHandle;

typedef struct RivetBlockPos {
    uint32_t world_id;
    int32_t x;
    int32_t y;
    int32_t z;
} RivetBlockPos;

typedef struct RivetCallbackToken {
    uint32_t slot;
    uint32_t generation;
} RivetCallbackToken;

typedef struct RivetByteView {
    const uint8_t *ptr;
    uint32_t len;
    uint32_t reserved;
} RivetByteView;

typedef struct RivetOwnedBytes {
    uint8_t *ptr;
    uint32_t len;
    uint32_t cap;
} RivetOwnedBytes;

typedef struct RivetEntityInfo {
    RivetEntityHandle handle;
    RivetBlockPos pos;
    uint32_t alive;
    uint32_t reserved;
} RivetEntityInfo;

typedef struct RivetRuntimeResult {
    RivetStatus status;
    uint32_t reserved;
    RivetRuntimeHandle handle;
} RivetRuntimeResult;

typedef struct RivetTickResult {
    RivetStatus status;
    uint32_t reserved;
    uint64_t tick_number;
} RivetTickResult;

typedef struct RivetEntityHandleResult {
    RivetStatus status;
    uint32_t reserved;
    RivetEntityHandle handle;
} RivetEntityHandleResult;

typedef struct RivetEntityLookupResult {
    RivetStatus status;
    uint32_t reserved;
    RivetEntityInfo info;
} RivetEntityLookupResult;

typedef struct RivetCallbackResult {
    RivetStatus status;
    uint32_t reserved;
    RivetCallbackToken token;
} RivetCallbackResult;

typedef struct RivetEventResult {
    RivetStatus status;
    uint32_t delivered_count;
    uint32_t cancelled;
    uint32_t reserved;
} RivetEventResult;

typedef struct RivetBytesResult {
    RivetStatus status;
    uint32_t reserved;
    RivetOwnedBytes bytes;
} RivetBytesResult;

typedef int32_t (*RivetEventCallback)(
    uint32_t event_kind,
    const uint8_t *payload_ptr,
    uint32_t payload_len,
    uint64_t user_data
);

RivetAbiVersion rivet_ffi_v1_abi_version(void);
RivetRuntimeResult rivet_ffi_v1_init(uint32_t expected_major, uint32_t expected_minor);
RivetStatus rivet_ffi_v1_shutdown(RivetRuntimeHandle runtime);
RivetTickResult rivet_ffi_v1_tick(RivetRuntimeHandle runtime);

RivetEntityHandleResult rivet_ffi_v1_entity_spawn(
    RivetRuntimeHandle runtime,
    uint32_t type_id,
    RivetBlockPos pos,
    RivetByteView name_utf8
);
RivetEntityLookupResult rivet_ffi_v1_entity_lookup(
    RivetRuntimeHandle runtime,
    RivetEntityHandle handle
);
RivetBytesResult rivet_ffi_v1_entity_describe_json(
    RivetRuntimeHandle runtime,
    RivetEntityHandle handle
);
RivetStatus rivet_ffi_v1_entity_despawn(
    RivetRuntimeHandle runtime,
    RivetEntityHandle handle
);

RivetCallbackResult rivet_ffi_v1_register_event_callback(
    RivetRuntimeHandle runtime,
    uint32_t event_kind,
    RivetEventCallback callback,
    uint64_t user_data
);
RivetStatus rivet_ffi_v1_unregister_event_callback(
    RivetRuntimeHandle runtime,
    RivetCallbackToken token
);
RivetEventResult rivet_ffi_v1_publish_event(
    RivetRuntimeHandle runtime,
    uint32_t event_kind,
    RivetByteView payload
);
RivetStatus rivet_ffi_v1_bytes_free(RivetOwnedBytes bytes);

#ifdef __cplusplus
}
#endif

#endif /* RIVET_FFI_H */
