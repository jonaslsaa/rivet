package rivet.ffi;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemoryLayout;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodType;

/**
 * Java FFM (Panama) shim over the spike C ABI (see spikes/ffi-latency/src/lib.rs).
 *
 * Mirrors the stable C ABI exactly: only fixed-width integers and one #[repr(C)]
 * struct cross the boundary. All IDs (world, entity) are marshaled as u64 longs;
 * the entity id is a generational handle (generation << 32 | index), re-resolved
 * per call on the Rust side (OWNERSHIP §JVM-adapter). No JNI glue, no native code
 * in this file.
 */
public final class RivetFfi {

    /** Must match API_VERSION in lib.rs. */
    public static final int API_VERSION = 1;
    /** Must match NOT_FOUND in lib.rs. */
    public static final int NOT_FOUND = Integer.MIN_VALUE;
    /** Callback statuses, mirrored from lib.rs. */
    public static final int OK = 0;
    public static final int ERR_NO_CALLBACK = -1;
    public static final int ERR_CALLBACK = -2;

    private static final Linker LINKER = Linker.nativeLinker();
    private static final Arena ARENA = Arena.ofShared();

    // --- downcall handles -------------------------------------------------
    static final MethodHandle RFV_API_VERSION = downcall("rfv_api_version",
            FunctionDescriptor.of(ValueLayout.JAVA_INT));
    static final MethodHandle RFV_CREATE_WORLD = downcall("rfv_create_world",
            FunctionDescriptor.of(ValueLayout.JAVA_LONG));
    static final MethodHandle RFV_DESTROY_WORLD = downcall("rfv_destroy_world",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG));
    static final MethodHandle RFV_SPAWN_ENTITY = downcall("rfv_spawn_entity",
            FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG));
    static final MethodHandle RFV_FREE_ENTITY = downcall("rfv_free_entity",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG));
    static final MethodHandle RFV_GET_ENTITY_X = downcall("rfv_get_entity_x",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG));
    static final MethodHandle RFV_SET_ENTITY_X = downcall("rfv_set_entity_x",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT));
    static final MethodHandle RFV_TICK = downcall("rfv_tick",
            FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG));
    static final MethodHandle RFV_APPLY_EVENTS = downcall("rfv_apply_events",
            FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));
    static final MethodHandle RFV_REGISTER_CALLBACK = downcall("rfv_register_callback",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG));
    static final MethodHandle RFV_DISPATCH_CALLBACK = downcall("rfv_dispatch_callback",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT, ValueLayout.JAVA_LONG));
    static final MethodHandle RFV_EVENT_STORM = downcall("rfv_event_storm",
            FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG));

    // --- Event struct layout (matches #[repr(C)] Event in lib.rs) ---------
    public static final MemoryLayout EVENT_LAYOUT = MemoryLayout.structLayout(
            ValueLayout.JAVA_LONG,       // entity  (u64)
            ValueLayout.JAVA_INT,        // event_id (i32)
            MemoryLayout.paddingLayout(4),
            ValueLayout.JAVA_LONG);      // payload (i64)
    public static final long EVENT_SIZE = EVENT_LAYOUT.byteSize(); // 24
    private static final long OFF_ENTITY = 0;
    private static final long OFF_EVENT_ID = 8;
    private static final long OFF_PAYLOAD = 16;

    private static MethodHandle downcall(String name, FunctionDescriptor fd) {
        SymbolLookup lib = LibraryHolder.LOOKUP;
        return LINKER.downcallHandle(lib.find(name).orElseThrow(
                () -> new IllegalStateException("missing C symbol: " + name)), fd);
    }

    /** Lazily loads the cdylib; the path comes from -Dffi.lib=... at runtime. */
    private static final class LibraryHolder {
        static final SymbolLookup LOOKUP;
        static {
            String path = System.getProperty("ffi.lib");
            if (path == null) {
                path = System.getenv("FFI_LIB");
            }
            if (path == null) {
                throw new IllegalStateException(
                        "set -Dffi.lib=<path> (or FFI_LIB) to the built libffi_latency_spike dylib");
            }
            LOOKUP = SymbolLookup.libraryLookup(path, ARENA);
        }
    }

    private RivetFfi() {}

    // --- thin wrappers ------------------------------------------------------
    public static int apiVersion() { return (int) invoke(RFV_API_VERSION); }
    public static long createWorld() { return (long) invoke(RFV_CREATE_WORLD); }
    public static int destroyWorld(long world) { return (int) invoke(RFV_DESTROY_WORLD, world); }
    public static long spawnEntity(long world) { return (long) invoke(RFV_SPAWN_ENTITY, world); }
    public static int freeEntity(long world, long entity) { return (int) invoke(RFV_FREE_ENTITY, world, entity); }
    public static int getEntityX(long world, long entity) { return (int) invoke(RFV_GET_ENTITY_X, world, entity); }
    public static int setEntityX(long world, long entity, int x) { return (int) invoke(RFV_SET_ENTITY_X, world, entity, x); }
    public static long tick(long world) { return (long) invoke(RFV_TICK, world); }
    public static int registerCallback(long world, long callbackAddr, long ctx) {
        return (int) invoke(RFV_REGISTER_CALLBACK, world, callbackAddr, ctx);
    }
    public static int dispatchCallback(long world, long entity, int eventId, long payload) {
        return (int) invoke(RFV_DISPATCH_CALLBACK, world, entity, eventId, payload);
    }
    public static long eventStorm(long world, long entity, long count) {
        return (long) invoke(RFV_EVENT_STORM, world, entity, count);
    }

    /** Applies `count` events from the pre-filled segment. Returns number applied. */
    public static long applyEvents(long world, MemorySegment events, long count) {
        return (long) invoke(RFV_APPLY_EVENTS, world, events, count);
    }

    private static Object invoke(MethodHandle mh, Object... args) {
        try {
            return mh.invokeWithArguments(args);
        } catch (Throwable t) {
            throw new RuntimeException("native call failed: " + t, t);
        }
    }

    // --- Event batch writer -------------------------------------------------
    /** Writes one Event at index `i` in a segment allocated with EVENT_LAYOUT. */
    public static void writeEvent(MemorySegment seg, long i, long entity, int eventId, long payload) {
        long off = i * EVENT_SIZE;
        seg.set(ValueLayout.JAVA_LONG, off + OFF_ENTITY, entity);
        seg.set(ValueLayout.JAVA_INT, off + OFF_EVENT_ID, eventId);
        seg.set(ValueLayout.JAVA_LONG, off + OFF_PAYLOAD, payload);
    }

    public static MemorySegment allocateEvents(long count) {
        return ARENA.allocate(count * EVENT_SIZE, EVENT_LAYOUT.byteAlignment());
    }

    // --- upcall (Rust -> Java) ----------------------------------------------
    /**
     * Java-side upcall target invoked by Rust. A plugin handler would be called
     * here.
     *
     * Boundary guard: a Java/plugin exception thrown in the handler must NEVER
     * unwind through the Rust cdylib (Rust's catch_unwind cannot intercept a
     * foreign exception). Every {@link Throwable} is caught here and converted
     * to an explicit ABI-safe status code (`ERR_CALLBACK`) that the Rust side
     * surfaces as an error result from the callback dispatchers.
     */
    static int onCallback(long world, long entity, int eventId, long payload, long ctx) {
        try {
            CallbackSink sink = CallbackState.CAPTURE;
            if (sink != null) {
                sink.onCallback(world, entity, eventId, payload, ctx);
            }
            return OK;
        } catch (Throwable t) {
            System.err.println("[rivet-ffi] plugin callback threw (contained at the FFI boundary): " + t);
            return ERR_CALLBACK;
        }
    }

    private static final MethodHandle CALLBACK_TARGET;
    static {
        try {
            CALLBACK_TARGET = MethodHandles.lookup().findStatic(
                    RivetFfi.class, "onCallback",
                    MethodType.methodType(int.class,
                            long.class, long.class, int.class, long.class, long.class));
        } catch (ReflectiveOperationException e) {
            throw new ExceptionInInitializerError(e);
        }
    }

    /** Registers `sink` as the Java callback and returns the upcall stub address. */
    public static long installCallback(long world, CallbackSink sink) {
        CallbackState.CAPTURE = sink;
        MemorySegment stub = LINKER.upcallStub(CALLBACK_TARGET,
                FunctionDescriptor.of(ValueLayout.JAVA_INT,
                        ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG, ValueLayout.JAVA_INT,
                        ValueLayout.JAVA_LONG, ValueLayout.JAVA_LONG),
                ARENA);
        registerCallback(world, stub.address(), 0);
        return stub.address();
    }

    public interface CallbackSink {
        void onCallback(long world, long entity, int eventId, long payload, long ctx);
    }

    /** One-at-a-time capture slot so the callback reaches the active sink. */
    static final class CallbackState {
        static CallbackSink CAPTURE;
    }
}
