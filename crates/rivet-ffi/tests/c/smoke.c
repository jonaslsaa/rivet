/*
 * C caller smoke test for the rivet-ffi v1 ABI, built and run by
 * tests/c_header_smoke.rs: compiled against include/rivet_ffi.h, linked
 * against the librivet_ffi cdylib, and exercises every exported symbol.
 */
#include <stdio.h>
#include <string.h>

#include "rivet_ffi.h"

#define CHECK(cond)                                                          \
    do {                                                                     \
        if (!(cond)) {                                                       \
            fprintf(stderr, "%s:%d: check failed: %s\n", __FILE__, __LINE__, \
                    #cond);                                                  \
            return 1;                                                        \
        }                                                                    \
    } while (0)

static uint32_t g_calls;
static uint64_t g_user_data;

static int32_t on_event(uint32_t event_kind, const uint8_t *payload_ptr,
                        uint32_t payload_len, uint64_t user_data) {
    g_calls++;
    g_user_data = user_data;
    if (event_kind == 7u && payload_len == 6u &&
        memcmp(payload_ptr, "cancel", 6) == 0) {
        return RIVET_EVENT_CANCEL;
    }
    return RIVET_EVENT_CONTINUE;
}

int main(void) {
    RivetAbiVersion version = rivet_ffi_v1_abi_version();
    CHECK(version.major == RIVET_FFI_ABI_MAJOR);
    CHECK(version.minor == RIVET_FFI_ABI_MINOR);
    CHECK(version.patch == RIVET_FFI_ABI_PATCH);

    CHECK(rivet_ffi_v1_init(RIVET_FFI_ABI_MAJOR + 1u, 0u).status ==
          RIVET_STATUS_ABI_VERSION_MISMATCH);

    RivetRuntimeResult rt =
        rivet_ffi_v1_init(RIVET_FFI_ABI_MAJOR, RIVET_FFI_ABI_MINOR);
    CHECK(rt.status == RIVET_STATUS_OK);
    CHECK(rt.handle.generation != 0u);

    RivetTickResult tick = rivet_ffi_v1_tick(rt.handle);
    CHECK(tick.status == RIVET_STATUS_OK);
    CHECK(tick.tick_number == 1u);

    /* Entity handle round trip. */
    static const char name[] = "zombie \"c\"";
    RivetBlockPos pos = {3u, -7, 64, 1234567};
    RivetByteView name_view = {(const uint8_t *)name,
                               (uint32_t)(sizeof name - 1u), 0u};
    RivetEntityHandleResult spawn =
        rivet_ffi_v1_entity_spawn(rt.handle, 42u, pos, name_view);
    CHECK(spawn.status == RIVET_STATUS_OK);
    CHECK(spawn.handle.generation != 0u);
    CHECK(spawn.handle.type_id == 42u);

    RivetEntityLookupResult lookup =
        rivet_ffi_v1_entity_lookup(rt.handle, spawn.handle);
    CHECK(lookup.status == RIVET_STATUS_OK);
    CHECK(lookup.info.handle.slot == spawn.handle.slot);
    CHECK(lookup.info.handle.generation == spawn.handle.generation);
    CHECK(lookup.info.pos.world_id == 3u && lookup.info.pos.x == -7 &&
          lookup.info.pos.y == 64 && lookup.info.pos.z == 1234567);
    CHECK(lookup.info.alive == 1u);

    RivetBytesResult desc =
        rivet_ffi_v1_entity_describe_json(rt.handle, spawn.handle);
    CHECK(desc.status == RIVET_STATUS_OK);
    CHECK(desc.bytes.ptr != NULL);
    CHECK(desc.bytes.len > 0u && desc.bytes.len < 512u);
    char json[512];
    memcpy(json, desc.bytes.ptr, desc.bytes.len);
    json[desc.bytes.len] = '\0';
    CHECK(strstr(json, "\"z\":1234567") != NULL);
    CHECK(strstr(json, "\"name\":\"zombie \\\"c\\\"\"") != NULL);
    CHECK(rivet_ffi_v1_bytes_free(desc.bytes) == RIVET_STATUS_OK);

    /* Callback round trip through a C function pointer. */
    CHECK(rivet_ffi_v1_register_event_callback(rt.handle, 7u, NULL, 0u).status ==
          RIVET_STATUS_NULL_POINTER);
    RivetCallbackResult reg =
        rivet_ffi_v1_register_event_callback(rt.handle, 7u, on_event, 0xC0FFEEu);
    CHECK(reg.status == RIVET_STATUS_OK);

    static const char cancel[] = "cancel";
    RivetByteView cancel_view = {(const uint8_t *)cancel,
                                 (uint32_t)(sizeof cancel - 1u), 0u};
    RivetEventResult published =
        rivet_ffi_v1_publish_event(rt.handle, 7u, cancel_view);
    CHECK(published.status == RIVET_STATUS_OK);
    CHECK(published.delivered_count == 1u);
    CHECK(published.cancelled == 1u);
    CHECK(g_calls == 1u);
    CHECK(g_user_data == 0xC0FFEEu);

    RivetByteView empty = {NULL, 0u, 0u};
    published = rivet_ffi_v1_publish_event(rt.handle, 8u, empty);
    CHECK(published.status == RIVET_STATUS_OK);
    CHECK(published.delivered_count == 0u);

    CHECK(rivet_ffi_v1_unregister_event_callback(rt.handle, reg.token) ==
          RIVET_STATUS_OK);
    CHECK(rivet_ffi_v1_unregister_event_callback(rt.handle, reg.token) ==
          RIVET_STATUS_CALLBACK_NOT_FOUND);

    /* Stale handles after despawn and shutdown. */
    CHECK(rivet_ffi_v1_entity_despawn(rt.handle, spawn.handle) ==
          RIVET_STATUS_OK);
    CHECK(rivet_ffi_v1_entity_lookup(rt.handle, spawn.handle).status ==
          RIVET_STATUS_HANDLE_STALE);
    CHECK(rivet_ffi_v1_shutdown(rt.handle) == RIVET_STATUS_OK);
    CHECK(rivet_ffi_v1_tick(rt.handle).status == RIVET_STATUS_HANDLE_STALE);

    puts("rivet_ffi C smoke: ok");
    return 0;
}
