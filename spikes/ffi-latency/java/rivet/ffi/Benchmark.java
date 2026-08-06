package rivet.ffi;

import java.lang.foreign.MemorySegment;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

/**
 * Event-storm FFI benchmark (epic #14 / sub-issue #81).
 *
 * Measures, against a release-built Rust cdylib:
 *   1. scalar downcall            (rfv_tick)
 *   2. handle-table lookup        (rfv_get_entity_x, re-resolved per call)
 *   3. batched event publish      (rfv_apply_events, per-element cost)
 *   4. callback round-trip        (rfv_dispatch_callback: Rust->Java->Rust)
 *   5. event storm                (rfv_event_storm: N Rust->Java callbacks)
 *
 * Correctness assertions run BEFORE any timing. Each scenario has a warmup
 * phase (JIT) followed by a measured phase; the first/cold call is reported
 * separately so JVM startup/JIT is distinguishable from steady state.
 *
 * Output is machine-readable JSON written to the file named by -Dout=... (or
 * "results/benchmark.json"). It contains no absolute paths.
 */
public final class Benchmark {

    // Scenario knobs (counts are per-measurement-iteration, not per-run).
    static final int SCALAR_WARMUP = 200_000;
    static final int SCALAR_ITERS = 200_000;
    static final int LOOKUP_WARMUP = 100_000;
    static final int LOOKUP_ITERS = 100_000;
    static final int BATCH_WARMUP = 5_000;
    static final int BATCH_ITERS = 5_000;
    static final int CALLBACK_WARMUP = 50_000;
    static final int CALLBACK_ITERS = 50_000;
    static final int[] STORM_SIZES = {1_000, 10_000, 100_000};
    static final int STORM_WARMUP = 20;

    static long WORLD = 0;
    static long entity = 0;
    static long callbackCount = 0;
    static long[] callbackTimes = new long[0];
    static long lastCallbackTick = 0;

    public static void main(String[] args) throws Exception {
        long mainStart = System.nanoTime();
        int api = RivetFfi.apiVersion(); // warm the class + FFM linker

        // Benchmark world + entity, created before the cold call so it measures
        // the first native transition, not a table miss on a nonexistent world.
        WORLD = RivetFfi.createWorld();
        if (WORLD == 0) throw new IllegalStateException("createWorld failed");
        entity = RivetFfi.spawnEntity(WORLD);
        if (entity == 0) throw new IllegalStateException("spawnEntity failed");

        long coldNs = timeIt(() -> RivetFfi.tick(WORLD));
        long tInit = System.nanoTime() - mainStart;

        assertCorrectness(); // its own throwaway world; must pass first

        Json root = new Json();
        root.set("schema", "rivet-ffi-latency-spike/v1");
        root.set("jdk", System.getProperty("java.version") + " " + System.getProperty("java.vendor"));
        root.set("os", System.getProperty("os.name") + " " + System.getProperty("os.arch"));
        root.set("ffi_abi_version", api);

        Json startup = new Json();
        startup.set("process_init_ns", tInit);
        startup.set("cold_first_scalar_call_ns", coldNs);
        root.set("startup", startup);

        // --- 1. scalar downcall (steady state) ------------------------------
        Json scalar = measure("scalar_call_tick",
                SCALAR_WARMUP, SCALAR_ITERS, () -> RivetFfi.tick(WORLD));
        root.set("scalar_call", scalar);

        // --- 2. handle-table lookup -----------------------------------------
        Json lookup = measure("handle_lookup_get_entity_x",
                LOOKUP_WARMUP, LOOKUP_ITERS, () -> RivetFfi.getEntityX(WORLD, entity));
        root.set("handle_lookup", lookup);

        // --- 3. batched event publish ---------------------------------------
        Json batches = new Json();
        int[] sizes = {1, 4, 16, 64, 256, 1024, 4096};
        for (int size : sizes) {
            batches.set("n=" + size, measureBatch(size));
        }
        root.set("batched_publish", batches);

        // --- 4. callback round-trip (Rust->Java->Rust) ----------------------
        // rfv_dispatch_callback only round-trips when a callback is registered
        // on the world; without one, lib.rs takes a fast-path miss (returns -1
        // with no Java upcall), which is not the path this scenario exists to
        // measure. Install the sink on WORLD first and assert rc==0 so a future
        // regression fails loudly instead of silently timing the miss.
        installStormCallback(WORLD, CALLBACK_ITERS + 1);
        Json cb = measure("callback_roundtrip",
                CALLBACK_WARMUP, CALLBACK_ITERS,
                () -> {
                    if (RivetFfi.dispatchCallback(WORLD, entity, 7, 42) != 0) {
                        throw new AssertionError("dispatchCallback rc != 0 during roundtrip");
                    }
                });
        root.set("callback_roundtrip", cb);

        // --- 5. event storm --------------------------------------------------
        Json storms = new Json();
        for (int n : STORM_SIZES) {
            storms.set("n=" + n, runStorm(n));
        }
        root.set("event_storm", storms);

        // --- verdict ----------------------------------------------------------
        root.set("verdict", verdict(root));

        String out = System.getProperty("out", "results/benchmark.json");
        String json = root.render();
        java.nio.file.Files.writeString(java.nio.file.Path.of(out), json);
        System.out.println(json);
        System.err.println("wrote " + out);
    }

    // =========================================================================
    // Correctness assertions — these MUST pass before any timing.
    // =========================================================================
    static void assertCorrectness() {
        int api = RivetFfi.apiVersion();
        if (api != RivetFfi.API_VERSION) {
            throw new AssertionError("ABI version mismatch: " + api);
        }

        long w = RivetFfi.createWorld();
        if (w == 0) throw new AssertionError("createWorld returned 0");

        long a = RivetFfi.spawnEntity(w);
        long b = RivetFfi.spawnEntity(w);
        if (a == 0 || b == 0 || a == b) throw new AssertionError("spawn ids: " + a + "," + b);

        // set/get round-trip
        if (RivetFfi.setEntityX(w, a, 123) != 0) throw new AssertionError("setEntityX failed");
        if (RivetFfi.getEntityX(w, a) != 123) throw new AssertionError("getEntityX != 123");

        // stale handle after free -> NOT_FOUND
        if (RivetFfi.freeEntity(w, b) != 0) throw new AssertionError("freeEntity failed");
        if (RivetFfi.getEntityX(w, b) != RivetFfi.NOT_FOUND)
            throw new AssertionError("stale handle should be NOT_FOUND");

        // re-allocated slot gets a fresh generation (stale old handle still dead)
        long b2 = RivetFfi.spawnEntity(w);
        if (b2 == b) throw new AssertionError("generation reuse: same id returned");
        if (RivetFfi.getEntityX(w, b) != RivetFfi.NOT_FOUND)
            throw new AssertionError("old-generation handle must stay NOT_FOUND");

        // batched apply_events
        long count = 3;
        MemorySegment seg = RivetFfi.allocateEvents(count);
        RivetFfi.writeEvent(seg, 0, a, 1, 111);
        RivetFfi.writeEvent(seg, 1, b2, 2, 222);
        RivetFfi.writeEvent(seg, 2, a, 3, 333); // duplicate entity, last wins
        long applied = RivetFfi.applyEvents(w, seg, count);
        if (applied != count) throw new AssertionError("applyEvents applied " + applied + " != " + count);
        if (RivetFfi.getEntityX(w, a) != 333) throw new AssertionError("batch entity a x != 333");
        if (RivetFfi.getEntityX(w, b2) != 222) throw new AssertionError("batch entity b2 x != 222");

        // unknown entity in batch is skipped but the rest still apply
        RivetFfi.writeEvent(seg, 0, 0xFFFF_FFFFL << 32, 9, 999); // stale id
        applied = RivetFfi.applyEvents(w, seg, count);
        if (applied != count - 1) throw new AssertionError("stale batch apply: " + applied);
        if (RivetFfi.getEntityX(w, a) != 333) throw new AssertionError("batch mutation clobbered");

        // callback: Rust -> Java -> Rust
        long before = RivetFfi.tick(w);
        installStormCallback(w, 64); // small sink for assertions
        int rc = RivetFfi.dispatchCallback(w, a, 99, 5);
        if (rc != 0) throw new AssertionError("dispatchCallback rc=" + rc);
        if (callbackCount != 1) throw new AssertionError("callbackCount=" + callbackCount);
        // the Java callback ticked the world (Java->Rust inside the upcall)
        long after = RivetFfi.tick(w);
        if (after <= before) throw new AssertionError("callback did not call back into Rust");

        // --- exception containment: a throwing plugin callback must never unwind through Rust.
        // The upcall target catches the Throwable and returns ERR_CALLBACK; the
        // dispatchers surface it as an explicit error result. If this were not
        // contained, the JVM would abort/UB rather than reach these assertions.
        RivetFfi.installCallback(w, (ww, ee, ev, payload, ctx) -> {
            throw new RuntimeException("intentional plugin exception (spike test)");
        });
        rc = RivetFfi.dispatchCallback(w, a, 55, 1);
        if (rc != RivetFfi.ERR_CALLBACK)
            throw new AssertionError("throwing callback: rc=" + rc + " expected ERR_CALLBACK");
        // storm aborts at the first event instead of unwinding through Rust
        long dispatched = RivetFfi.eventStorm(w, a, 100);
        if (dispatched != 0)
            throw new AssertionError("throwing storm: dispatched=" + dispatched + " expected abort at 0");
        // the world is still fully functional after the contained failures
        if (RivetFfi.getEntityX(w, a) != 333) throw new AssertionError("world corrupted after contained throw");
        // recovery: a healthy callback installed after the throwing one works
        installStormCallback(w, 64);
        rc = RivetFfi.dispatchCallback(w, a, 99, 5);
        if (rc != 0) throw new AssertionError("dispatch after contained throw rc=" + rc);

        if (RivetFfi.destroyWorld(w) != 0) throw new AssertionError("destroyWorld failed");
        System.err.println("[ok] correctness assertions passed");
    }

    // =========================================================================
    // Timing helpers
    // =========================================================================
    interface Op { void run(); }

    static long timeIt(Op op) {
        long s = System.nanoTime();
        op.run();
        return System.nanoTime() - s;
    }

    /** Runs warmup, then records per-iteration ns; returns a stats Json. */
    static Json measure(String name, int warmup, int iters, Op op) {
        for (int i = 0; i < warmup; i++) op.run();
        long[] ns = new long[iters];
        for (int i = 0; i < iters; i++) {
            long s = System.nanoTime();
            op.run();
            ns[i] = System.nanoTime() - s;
        }
        Json j = stats(name, ns);
        j.set("warmup_iters", warmup);
        return j;
    }

    /** Batch publish benchmark: one downcall per batch, cost amortized. */
    static Json measureBatch(int size) {
        MemorySegment seg = RivetFfi.allocateEvents(size);
        for (int i = 0; i < size; i++) RivetFfi.writeEvent(seg, i, entity, i, i + 1);
        Op op = () -> RivetFfi.applyEvents(WORLD, seg, size);
        for (int i = 0; i < BATCH_WARMUP; i++) op.run();
        long[] batchNs = new long[BATCH_ITERS];
        for (int i = 0; i < BATCH_ITERS; i++) {
            long s = System.nanoTime();
            op.run();
            batchNs[i] = System.nanoTime() - s;
        }
        Json j = stats("batched_publish_n=" + size, batchNs);
        j.set("batch_size", size);
        j.set("ns_per_event", j.num("p50_ns") / (double) size);
        return j;
    }

    /** Installs a callback sink that records arrival time and calls back into Rust. */
    static void installStormCallback(long world, int capacity) {
        callbackCount = 0;
        callbackTimes = new long[capacity];
        RivetFfi.installCallback(world, (w, e, ev, payload, ctx) -> {
            int idx = (int) callbackCount;
            if (idx < callbackTimes.length) callbackTimes[idx] = System.nanoTime();
            callbackCount++;
            lastCallbackTick = RivetFfi.tick(w); // Java -> Rust inside the upcall
        });
    }

    /** Event storm: measures per-event latency from callback arrival times. */
    static Json runStorm(int n) {
        for (int i = 0; i < STORM_WARMUP; i++) {
            installStormCallback(WORLD, n + 2);
            RivetFfi.eventStorm(WORLD, entity, n); // warmup: timestamps discarded
        }

        installStormCallback(WORLD, n + 2); // reset counter/array for the measured run
        long s = System.nanoTime();
        long dispatched = RivetFfi.eventStorm(WORLD, entity, n);
        long totalNs = System.nanoTime() - s;

        if (dispatched != n) throw new AssertionError("storm dispatched " + dispatched + " != " + n);
        if (callbackCount != n) throw new AssertionError("storm callbacks " + callbackCount + " != " + n);

        // inter-arrival deltas observed at the Java callback (per-event latency).
        long[] perEvent = new long[n - 1];
        for (int i = 1; i < n; i++) perEvent[i - 1] = callbackTimes[i] - callbackTimes[i - 1];

        Json j = stats("event_storm_n=" + n, perEvent);
        j.set("storm_total_ns", totalNs);
        j.set("events_per_sec", 1e9 * n / (double) totalNs);
        long span = callbackTimes[n - 1] - callbackTimes[0];
        j.set("storm_span_ns", span);
        return j;
    }

    // =========================================================================
    // Stats + JSON
    // =========================================================================
    static Json stats(String name, long[] samples) {
        long[] sorted = samples.clone();
        Arrays.sort(sorted);
        int n = sorted.length;
        Json j = new Json();
        j.set("name", name);
        j.set("samples", (int) n);
        j.set("min_ns", sorted[0]);
        j.set("p50_ns", sorted[pct(sorted, 50)]);
        j.set("p90_ns", sorted[pct(sorted, 90)]);
        j.set("p99_ns", sorted[pct(sorted, 99)]);
        j.set("max_ns", sorted[n - 1]);
        double sum = 0;
        for (long v : sorted) sum += v;
        j.set("mean_ns", sum / n);
        j.set("ops_per_sec", 1e9 * n / sum);
        return j;
    }

    static int pct(long[] sorted, int p) {
        return (int) Math.max(0, Math.min(sorted.length - 1, (long) sorted.length * p / 100));
    }

    /**
     * Per-tick budget: 20 TPS => 50 ms/tick. Adapter synchronous event dispatch is
     * budgeted at 10% of the tick (5 ms). The go/no-go weighs two observations:
     *  (a) strict synthetic worst case: a single 100k-event storm (~10-12 ms total
     *      dispatch — does NOT fit the 5 ms budget; recorded honestly);
     *  (b) realistic volume: real plugin event dispatch is tens-to-low-thousands
     *      of events/tick, so `max_events_per_tick_in_budget` (~41k-50k, from the
     *      ~100-120 ns storm cadence) is the real constraint and dwarfs any sane
     *      per-tick plugin volume.
     * The architectural constraint falls out of the same numbers: bulk state
     * mutation should go through the batched path (`rfv_apply_events`, hundreds
     * of x cheaper per event), while per-plugin handler dispatch uses the
     * callback path (a full Rust->Java->Rust round-trip ~215-250 ns).
     */
    static Json verdict(Json root) {
        long budgetNs = 50_000_000L / 10; // 10% of 50 ms tick
        Json storm = root.child("event_storm").child("n=100000");
        long stormTotal = (long) storm.num("storm_total_ns");
        double meanPerEventNs = storm.num("mean_ns");
        long maxEventsInBudget = meanPerEventNs > 0
                ? (long) (budgetNs / meanPerEventNs)
                : 0;

        // batched mutation is ~O(ns/event) amortized; the callback round-trip is
        // the per-handler dispatch cost (one Rust->Java->Rust hop), while the storm
        // mean_ns is the back-to-back callback cadence that drives per-tick capacity.
        Json batch4096 = root.child("batched_publish").child("n=4096");
        double batchPerEvent = batch4096.num("ns_per_event");
        double callbackPerEvent = root.child("callback_roundtrip").num("mean_ns");

        // Realistic upper bound on plugin events/tick. Paper servers commonly see
        // < 10k plugin events/sec; even 10k events/tick at 20 TPS is 200k events/s,
        // an extreme load. The ~41k-50k capacity leaves ample headroom.
        long realisticPerTick = 10_000L;
        boolean fitsRealistic = realisticPerTick <= maxEventsInBudget;

        Json v = new Json();
        v.set("tick_budget_ns", budgetNs);
        v.set("tick_budget_us", budgetNs / 1000.0);
        v.set("rationale", "20 TPS => 50ms/tick; adapter dispatch budgeted at 10% of tick");
        v.set("worst_storm_total_ns", stormTotal);
        v.set("fits_100k_storm_in_budget", stormTotal <= budgetNs);
        v.set("max_events_per_tick_in_budget", maxEventsInBudget);
        v.set("realistic_per_tick_assumption", realisticPerTick);
        v.set("fits_realistic_volume", fitsRealistic);
        v.set("go_no_go", fitsRealistic ? "GO" : "NO-GO");
        v.set("go_no_go_rationale",
                "storm per-event callback cadence ~" + String.format("%.0f", meanPerEventNs)
                + "ns, so " + maxEventsInBudget
                + " events/tick fit the 5ms budget, vs a realistic assumption of "
                + realisticPerTick + " events/tick. A single callback round-trip is ~"
                + String.format("%.0f", callbackPerEvent)
                + "ns. Bulk state should still batch "
                + "via rfv_apply_events (~" + String.format("%.1f", batchPerEvent)
                + "ns/event).");
        v.set("batched_ns_per_event", batchPerEvent);
        v.set("callback_ns_per_event", callbackPerEvent);
        v.set("batch_speedup_over_callback_x", batchPerEvent > 0
                ? callbackPerEvent / batchPerEvent : 0);
        return v;
    }

    // =========================================================================
    // Minimal JSON writer (no deps).
    // =========================================================================
    static final class Json {
        private final List<String> keys = new ArrayList<>();
        private final List<Object> vals = new ArrayList<>();

        void set(String k, Object v) {
            keys.add(k);
            vals.add(v);
        }

        Json child(String k) {
            int i = keys.indexOf(k);
            if (i < 0) throw new IllegalArgumentException("no key " + k);
            return (Json) vals.get(i);
        }

        double num(String k) {
            int i = keys.indexOf(k);
            if (i < 0) throw new IllegalArgumentException("no key " + k);
            return ((Number) vals.get(i)).doubleValue();
        }

        String render() {
            StringBuilder sb = new StringBuilder("{\n");
            for (int i = 0; i < keys.size(); i++) {
                sb.append("  ").append(q(keys.get(i))).append(": ").append(fmt(vals.get(i)));
                if (i + 1 < keys.size()) sb.append(',');
                sb.append('\n');
            }
            return sb.append('}').toString();
        }

        private static String fmt(Object v) {
            if (v instanceof Json j) {
                return j.render().replace("\n", "\n  ").replaceFirst("^\\{", "{\n  ");
            }
            if (v instanceof Double || v instanceof Float) {
                double d = ((Number) v).doubleValue();
                return Double.isFinite(d) ? Double.toString(d) : "null";
            }
            if (v instanceof Boolean b) return b.toString();
            if (v instanceof Number) return v.toString();
            return q(String.valueOf(v));
        }

        private static String q(String s) {
            return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
        }
    }
}
