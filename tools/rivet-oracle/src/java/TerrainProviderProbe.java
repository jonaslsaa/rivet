import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.PrintWriter;
import java.nio.file.Path;
import java.util.List;
import java.util.Locale;
import net.minecraft.data.worldgen.TerrainProvider;
import net.minecraft.util.BoundedFloatFunction;
import net.minecraft.util.CubicSpline;

/**
 * Samples the pinned Paper 26.2 `net.minecraft.data.worldgen.TerrainProvider`
 * value leaves and emits stable golden JSON (the `mc.data.worldgen.prereq`
 * unit): the overworld offset/factor/jaggedness `CubicSpline` builders and the
 * `peaksAndValleys` ridge function.
 *
 * The spline coordinates are the identity `BoundedFloatFunction` (unbounded),
 * so the four inputs (`continents`/`erosion`/`ridges`/`weirdness`) all sample
 * at the sweep coordinate — a valid deterministic check of the knot structure
 * and hermite arithmetic at coincident inputs. The real structural parity is
 * the `parityString()` output (all locations/derivatives/values as `%.3f`),
 * asserted byte-for-byte against the Rust port's `parity_string()`.
 *
 * Every emitted value is the raw Java `float` formatted with
 * `Double.toHexString` (bit-exact), plus the parity string. Both the plain and
 * amplified variants are emitted so the `Float2FloatFunction` transformers
 * (`AMPLIFIED_OFFSET`/`AMPLIFIED_FACTOR`/`AMPLIFIED_JAGGEDNESS`) are exercised
 * bit-exactly. No registry/version boot is needed: these are value-leaf.
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" TerrainProviderProbe --output dir/ [--paper pin]
 */
public final class TerrainProviderProbe {
    private TerrainProviderProbe() {}

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
            throw new IllegalArgumentException("Usage: TerrainProviderProbe --output <dir> [--paper <pin>]");
        }

        JsonObject root = new JsonObject();
        root.addProperty("paper", paper);

        // identity coordinate for all four terrain inputs (unbounded)
        BoundedFloatFunction<Float> identity = BoundedFloatFunction.IDENTITY;

        JsonArray cases = new JsonArray();
        cases.add(splineCase("offset_plain", TerrainProvider.overworldOffset(identity, identity, identity, false), identity));
        cases.add(splineCase("offset_amplified", TerrainProvider.overworldOffset(identity, identity, identity, true), identity));
        cases.add(splineCase("factor_plain", TerrainProvider.overworldFactor(identity, identity, identity, identity, false), identity));
        cases.add(splineCase("factor_amplified", TerrainProvider.overworldFactor(identity, identity, identity, identity, true), identity));
        cases.add(splineCase("jaggedness_plain", TerrainProvider.overworldJaggedness(identity, identity, identity, identity, false), identity));
        cases.add(splineCase("jaggedness_amplified", TerrainProvider.overworldJaggedness(identity, identity, identity, identity, true), identity));
        root.add("cases", cases);

        // peaksAndValleys over a sweep of weirdness
        JsonArray peaks = new JsonArray();
        for (float w : new float[] {
            -1.0F, -0.9F, -0.75F, -0.6666667F, -0.6F, -0.5F, -0.4F, -0.33333334F,
            -0.3F, -0.2F, -0.1F, 0.0F, 0.1F, 0.2F, 0.3F, 0.33333334F, 0.4F, 0.5F,
            0.6F, 0.6666667F, 0.75F, 0.9F, 1.0F
        }) {
            JsonObject o = new JsonObject();
            o.addProperty("weirdness", hexF(w));
            o.addProperty("value", hexF(TerrainProvider.peaksAndValleys(w)));
            peaks.add(o);
        }
        root.add("peaks_and_valleys", peaks);

        Path out = Path.of(output).resolve("terrain-provider-goldens.json");
        out.getParent().toFile().mkdirs();
        try (PrintWriter w = new PrintWriter(out.toFile())) {
            w.println(new GsonBuilder().setPrettyPrinting().create().toJson(root));
        }
        System.out.println("wrote " + out);
    }

    private static JsonObject splineCase(
        String name,
        CubicSpline<BoundedFloatFunction<Float>> s,
        BoundedFloatFunction<Float> identity
    ) {
        JsonObject o = new JsonObject();
        o.addProperty("name", name);
        o.addProperty("min", hexF(s.minValue()));
        o.addProperty("max", hexF(s.maxValue()));
        o.addProperty("parity", parityOf(s));
        o.add("samples", samples(s, List.of(
            -1.0F, -0.9F, -0.75F, -0.51F, -0.44F, -0.33333334F, -0.15F, -0.1F,
            0.0F, 0.2F, 0.4F, 0.65F, 0.9F, 1.0F
        )));
        return o;
    }

    private static String parityOf(CubicSpline<BoundedFloatFunction<Float>> s) {
        // The identity coordinate is an anonymous BoundedFloatFunction instance,
        // so its toString appends a per-JVM-run identity hash. Strip it so a
        // regeneration is byte-reproducible across runs.
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

    private static String hexF(float v) {
        return String.format(Locale.ROOT, "%s", Float.floatToIntBits(v) == 0 ? "0x0.0p0" : Double.toHexString(v));
    }
}
