import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.PrintWriter;
import java.nio.file.Path;
import java.util.List;
import java.util.Locale;
import net.minecraft.util.BoundedFloatFunction;
import net.minecraft.util.CubicSpline;

/**
 * Samples the pinned Paper 26.2 value leaves that issue #372 ports
 * ({@code CubicSpline}, {@code BoundedFloatFunction}) and emits stable golden
 * JSON.  The probe drives the spline builder through a matrix of cases that
 * exercise every branch of {@code CubicSpline.Multipoint}:
 *
 *  - interpolation (derivatives nonzero), the hermite correction term;
 *  - linear extension off both low and high ends (coordinate bounds outside
 *    the locations array), including the multi-valued edge over/under-shoot
 *    computation in the min/max constructor;
 *  - constant splines (a raw float vs a one-point multipoint);
 *  - {@code sample} on a nested {@code CubicSpline} value (values are
 *    themselves splines).
 *
 * For each case we emit: sampled outputs over a sweep of coordinates, the
 * spline min/max (the constructor-computed bounds), the parity string, and —
 * where the spline is a {@code Multipoint} — the raw locations/derivatives
 * arrays (the packed-point round trip is exercised by the Rust codec tests;
 * the probe records the ordering the builder produces).  The coordinate
 * function is the identity over {@code Float}, whose bounds are unbounded.
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" SplineProbe --output dir/ [--paper pin]
 *
 * Each emitted value is the raw Java {@code float} formatted with
 * {@code Double.toHexString} (bit-exact) and also as {@code %.3f} — the parity
 * format — so a Rust port can compare both the exact bits and the parity
 * string.  No registry/version boot is needed: these are value-leaf.
 */
public final class SplineProbe {
    private SplineProbe() {}

    public static void main(String[] args) throws Exception {
        String output = null;
        String paper = "26.2-DEV-main@0a99345";
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--output" -> output = args[++i];
                case "--paper" -> paper = args[++i];
                default -> throw new IllegalArgumentException("Unknown arg: " + args[i]);
            }
        }
        if (output == null) {
            throw new IllegalArgumentException("Usage: SplineProbe --output <dir> [--paper <pin>]");
        }

        JsonObject root = new JsonObject();
        root.addProperty("paper", paper);

        // identity coordinate: unbounded
        BoundedFloatFunction<Float> identity = BoundedFloatFunction.IDENTITY;

        JsonArray cases = new JsonArray();

        // -- constant (raw float) --
        cases.add(constantCase("constant_raw", CubicSpline.constant(1.5f)));

        // -- single-point multipoint (constant via builder) --
        cases.add(constantCase(
            "constant_one_point",
            CubicSpline.builder(identity).addPoint(0.0F, 3.25F).build()));

        // -- two-point interpolation, zero derivatives --
        cases.add(splineCase("two_point_no_deriv", CubicSpline.builder(identity)
            .addPoint(-1.0F, 2.0F)
            .addPoint(1.0F, 4.0F)
            .build(), List.of(-2.0F, -1.5F, -1.0F, -0.5F, 0.0F, 0.5F, 1.0F, 1.5F, 2.0F)));

        // -- three-point with nonzero derivatives (hermite correction) --
        cases.add(splineCase("three_point_deriv", CubicSpline.builder(identity)
            .addPoint(-3.0F, 5.0F, 1.0F)
            .addPoint(0.0F, -1.0F, 0.0F)
            .addPoint(3.0F, 2.0F, -2.0F)
            .build(),
            List.of(-4.0F, -3.0F, -2.0F, -1.0F, 0.0F, 1.0F, 2.0F, 3.0F, 4.0F)));

        // -- points with negative slope, high end extension --
        cases.add(splineCase("neg_slope_extend", CubicSpline.builder(identity)
            .addPoint(0.0F, 10.0F, -1.0F)
            .addPoint(4.0F, 2.0F, 0.0F)
            .addPoint(8.0F, -3.0F, 0.5F)
            .build(),
            List.of(-2.0F, 0.0F, 2.0F, 4.0F, 6.0F, 8.0F, 10.0F, 12.0F)));

        // -- nested value splines (values are themselves splines) --
        CubicSpline<BoundedFloatFunction<Float>> inner = CubicSpline.builder(identity)
            .addPoint(0.0F, 1.0F)
            .addPoint(2.0F, 3.0F)
            .build();
        cases.add(splineCase("nested_values", CubicSpline.builder(identity)
            .addPoint(-2.0F, inner)
            .addPoint(2.0F, CubicSpline.constant(0.5F))
            .build(),
            List.of(-3.0F, -2.0F, -1.0F, 0.0F, 1.0F, 2.0F, 3.0F)));

        root.add("cases", cases);

        // -- hostile cases: builder-order validation, empty builder --
        JsonObject hostile = new JsonObject();
        hostile.add("descending_order", probeThrowing(() ->
            CubicSpline.builder(identity)
                .addPoint(2.0F, 1.0F)
                .addPoint(1.0F, 0.0F)
                .build()));
        hostile.add("equal_order", probeThrowing(() ->
            CubicSpline.builder(identity)
                .addPoint(1.0F, 0.0F)
                .addPoint(1.0F, 1.0F)
                .build()));
        hostile.add("empty_builder", probeThrowing(() -> CubicSpline.builder(identity).build()));
        root.add("hostile", hostile);

        Path out = Path.of(output).resolve("spline-goldens.json");
        out.getParent().toFile().mkdirs();
        try (PrintWriter w = new PrintWriter(out.toFile())) {
            w.println(new GsonBuilder().setPrettyPrinting().create().toJson(root));
        }
        System.out.println("wrote " + out);
    }

    private static JsonObject constantCase(String name, CubicSpline<BoundedFloatFunction<Float>> s) {
        JsonObject o = new JsonObject();
        o.addProperty("name", name);
        o.addProperty("min", hexF(s.minValue()));
        o.addProperty("max", hexF(s.maxValue()));
        o.addProperty("parity", parityOf(s));
        o.add("samples", samples(s, List.of(-3.0F, 0.0F, 2.5F, 100.0F)));
        return o;
    }

    private static JsonObject splineCase(
        String name, CubicSpline<BoundedFloatFunction<Float>> s, List<Float> coordinates
    ) {
        JsonObject o = new JsonObject();
        o.addProperty("name", name);
        o.addProperty("min", hexF(s.minValue()));
        o.addProperty("max", hexF(s.maxValue()));
        o.addProperty("parity", parityOf(s));
        o.add("samples", samples(s, coordinates));
        if (s instanceof CubicSpline.Multipoint<BoundedFloatFunction<Float>> mp) {
            JsonObject raw = new JsonObject();
            raw.add("locations", hexArray(mp.locations()));
            raw.add("derivatives", hexArray(mp.derivatives()));
            o.add("raw", raw);
        }
        return o;
    }

    private static String parityOf(CubicSpline<BoundedFloatFunction<Float>> s) {
        // The identity coordinate is an anonymous BoundedFloatFunction instance,
        // so its toString appends a per-JVM-run identity hash (`ClassName@hex`).
        // Strip it so a regeneration is byte-reproducible across runs.
        return s.parityString().replaceAll("@[0-9a-f]+", "");
    }

    private static JsonArray samples(CubicSpline<BoundedFloatFunction<Float>> s, List<Float> coords) {
        JsonArray out = new JsonArray();
        for (float c : coords) {
            JsonObject o = new JsonObject();
            o.addProperty("coordinate", hexF(c));
            o.addProperty("sample", hexF(CubicSpline.sample(s, c)));
            out.add(o);
        }
        return out;
    }

    private static JsonArray hexArray(float[] arr) {
        JsonArray out = new JsonArray();
        for (float v : arr) {
            out.add(hexF(v));
        }
        return out;
    }

    private static String hexF(float v) {
        return String.format(Locale.ROOT, "%s", Float.floatToIntBits(v) == 0 ? "0x0.0p0" : Double.toHexString(v));
    }

    private static JsonObject probeThrowing(ThrowingRunnable r) {
        JsonObject o = new JsonObject();
        try {
            r.run();
            o.addProperty("throws", false);
        } catch (IllegalArgumentException e) {
            o.addProperty("throws", true);
            o.addProperty("exception", "IllegalArgumentException");
            o.addProperty("message", e.getMessage());
        } catch (IllegalStateException e) {
            o.addProperty("throws", true);
            o.addProperty("exception", "IllegalStateException");
            o.addProperty("message", e.getMessage());
        } catch (RuntimeException e) {
            o.addProperty("throws", true);
            o.addProperty("exception", e.getClass().getSimpleName());
            o.addProperty("message", String.valueOf(e.getMessage()));
        }
        return o;
    }

    @FunctionalInterface
    private interface ThrowingRunnable {
        void run();
    }
}
