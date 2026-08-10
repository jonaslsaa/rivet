import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import java.io.PrintWriter;
import java.nio.file.Path;
import java.util.List;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.levelgen.DensityFunction;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.levelgen.synth.BlendedNoise;
import net.minecraft.world.level.levelgen.synth.ImprovedNoise;
import net.minecraft.world.level.levelgen.synth.NoiseUtils;
import net.minecraft.world.level.levelgen.synth.NormalNoise;
import net.minecraft.world.level.levelgen.synth.PerlinNoise;
import net.minecraft.world.level.levelgen.synth.PerlinSimplexNoise;
import net.minecraft.world.level.levelgen.synth.SimplexNoise;

/**
 * Samples the pinned Paper 26.2 `net.minecraft.world.level.levelgen.synth`
 * primitive-noise classes and emits stable golden JSON fixtures.  This is the
 * Paper-side sampling half of issue #177: it runs against the real Paper
 * runtime (no registry boot — the synth classes are value-leaf) and is
 * deterministic for a fixed seed + fixed construction parameters.
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" SynthNoiseProbe --output dir/ [--paper pin]
 *
 * Each emitted value is printed via doubleToLongBits so the Rust side can
 * assert exact bits. A `paper` provenance string is recorded for self-
 * description. Boundary / hostile coordinates are included per class.
 */
public final class SynthNoiseProbe {
    private SynthNoiseProbe() {}

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
            throw new IllegalArgumentException("Usage: SynthNoiseProbe --output <dir> [--paper <pin>]");
        }

        // The synth classes are value-leaf, but some static initializers
        // (e.g. `Registries.NOISE` referenced by `NormalNoise.NoiseParameters.CODEC`,
        // `ImprovedNoise`/`SimplexNoise` parity config) touch `BuiltInRegistries`,
        // which requires bootstrap. Mirror WorldGenSampler's boot.
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();

        JsonObject root = new JsonObject();
        root.addProperty("paper", paper);

        // ---- SimplexNoise ---------------------------------------------------
        JsonArray simplex = new JsonArray();
        for (long seed : new long[] {0L, 12345L, -987654321L}) {
            for (String source : new String[] {"xoroshiro", "legacy"}) {
                RandomSource r = random(source, seed);
                SimplexNoise s = new SimplexNoise(r);
                JsonObject e = new JsonObject();
                e.addProperty("seed", seed);
                e.addProperty("source", source);
                e.add("xo", bits(s.xo));
                e.add("yo", bits(s.yo));
                e.add("zo", bits(s.zo));
                e.addProperty("p0", perm(s, 0));
                e.addProperty("p255", perm(s, 255));
                double[][] xy = {
                    {0.0, 0.0}, {0.5, 0.5}, {1.5, -2.25}, {10.0, 10.0}, {-10.0, 10.0},
                    {100.0, -100.0}, {0.1, 0.2}, {-0.5, 0.5}, {123.456, -789.012},
                    {1.0e6, -1.0e6}, {1.0e-7, 1.0e-7}, {Double.MAX_VALUE / 1.0e9, 1.0},
                    {-1.0e9, 1.0e-3}, {255.0, 255.0}, {-256.0, 256.0}
                };
                JsonArray vals = new JsonArray();
                for (double[] p : xy) {
                    JsonObject v = new JsonObject();
                    v.addProperty("x", p[0]);
                    v.addProperty("y", p[1]);
                    v.add("v2", bits(s.getValue(p[0], p[1])));
                    v.add("v3", bits(s.getValue(p[0], p[1], 0.5)));
                    vals.add(v);
                }
                e.add("vals", vals);
                simplex.add(e);
            }
        }
        root.add("simplex", simplex);

        // ---- ImprovedNoise --------------------------------------------------
        JsonArray improved = new JsonArray();
        for (long seed : new long[] {0L, 12345L, -987654321L}) {
            for (String source : new String[] {"xoroshiro", "legacy"}) {
                RandomSource r = random(source, seed);
                ImprovedNoise n = new ImprovedNoise(r);
                JsonObject e = new JsonObject();
                e.addProperty("seed", seed);
                e.addProperty("source", source);
                e.add("xo", bits(n.xo));
                e.add("yo", bits(n.yo));
                e.add("zo", bits(n.zo));
                e.addProperty("p0", perm(n, 0));
                e.addProperty("p255", perm(n, 255));
                double[][] xyz = {
                    {0.0, 0.0, 0.0}, {1.0, 1.0, 1.0}, {0.5, -1.5, 2.5}, {10.0, 10.0, 10.0},
                    {-10.0, 10.0, -10.0}, {100.0, -100.0, 100.0}, {0.1, 0.2, 0.3},
                    {-0.5, 0.5, -0.5}, {123.456, -789.012, 3.14159}, {255.0, 256.0, -255.0},
                    {1.0e6, -1.0e6, 1.0e6}, {1.0e-7, 1.0e-7, 1.0e-7},
                    {-1.0e9, 1.0e-3, 7.0}, {Double.MIN_VALUE, 0.0, 0.0},
                    // Hostile: floor saturates to Integer.MAX_VALUE, so
                    // `p(x + 1)` etc. must wrap (Java int arithmetic) rather
                    // than overflow.
                    {Double.MAX_VALUE / 1.0e9, Double.MAX_VALUE / 1.0e9, Double.MAX_VALUE / 1.0e9}
                };
                JsonArray vals = new JsonArray();
                for (double[] p : xyz) {
                    JsonObject v = new JsonObject();
                    v.addProperty("x", p[0]);
                    v.addProperty("y", p[1]);
                    v.addProperty("z", p[2]);
                    v.add("v", bits(n.noise(p[0], p[1], p[2])));
                    v.add("vDeprecated", bits(n.noise(p[0], p[1], p[2], 0.25, 0.5)));
                    double[] deriv = new double[3];
                    double dv = n.noiseWithDerivative(p[0], p[1], p[2], deriv);
                    v.add("vDeriv", bits(dv));
                    v.add("d0", bits(deriv[0]));
                    v.add("d1", bits(deriv[1]));
                    v.add("d2", bits(deriv[2]));
                    vals.add(v);
                }
                e.add("vals", vals);
                improved.add(e);
            }
        }
        root.add("improved", improved);

        // ---- PerlinSimplexNoise ---------------------------------------------
        JsonArray perlinSimplex = new JsonArray();
        List<List<Integer>> psoctaveSets = List.of(
            List.of(-3, -2, -1, 0, 1, 2, 3),
            List.of(-1, 0, 1),
            List.of(0)
        );
        for (long seed : new long[] {0L, 12345L, -987654321L}) {
            for (String source : new String[] {"xoroshiro", "legacy"}) {
                for (List<Integer> octaves : psoctaveSets) {
                    RandomSource r = random(source, seed);
                    PerlinSimplexNoise p = new PerlinSimplexNoise(r, octaves);
                    JsonObject e = new JsonObject();
                    e.addProperty("seed", seed);
                    e.addProperty("source", source);
                    e.addProperty("octaves", octaves.toString());
                    double[][] xy = {
                        {0.0, 0.0}, {0.5, 0.5}, {1.5, -2.25}, {10.0, 10.0}, {-10.0, 10.0},
                        {100.0, -100.0}, {0.1, 0.2}, {-0.5, 0.5}, {123.456, -789.012},
                        {1.0e6, -1.0e6}, {1.0e-7, 1.0e-7}
                    };
                    JsonArray vals = new JsonArray();
                    for (double[] pt : xy) {
                        JsonObject v = new JsonObject();
                        v.addProperty("x", pt[0]);
                        v.addProperty("y", pt[1]);
                        v.add("vTrue", bits(p.getValue(pt[0], pt[1], true)));
                        v.add("vFalse", bits(p.getValue(pt[0], pt[1], false)));
                        vals.add(v);
                    }
                    e.add("vals", vals);
                    perlinSimplex.add(e);
                }
            }
        }
        root.add("perlin_simplex", perlinSimplex);

        // ---- PerlinNoise ----------------------------------------------------
        JsonArray perlin = new JsonArray();
        List<List<Integer>> octaveSets = List.of(
            List.of(-4, -3, -2, -1, 0, 1, 2, 3, 4),
            List.of(-1, 0, 1),
            List.of(0),
            List.of(-3, 0)
        );
        for (long seed : new long[] {0L, 12345L, -987654321L}) {
            for (String source : new String[] {"xoroshiro", "legacy"}) {
                for (List<Integer> octaves : octaveSets) {
                    RandomSource r = random(source, seed);
                    PerlinNoise p = PerlinNoise.create(r, octaves.stream().mapToInt(Integer::intValue));
                    JsonObject e = new JsonObject();
                    e.addProperty("seed", seed);
                    e.addProperty("source", source);
                    e.addProperty("octaves", octaves.toString());
                    e.add("maxValue", bits(maxValue(p)));
                    e.add("maxBrokenValue", bits(p.maxBrokenValue(1.5)));
                    double[][] xyz = {
                        {0.0, 0.0, 0.0}, {1.0, 1.0, 1.0}, {0.5, -1.5, 2.5}, {10.0, 10.0, 10.0},
                        {-10.0, 10.0, -10.0}, {100.0, -100.0, 100.0}, {0.1, 0.2, 0.3},
                        {123.456, -789.012, 3.14159}, {255.0, 256.0, -255.0},
                        {1.0e6, -1.0e6, 1.0e6}, {1.0e-7, 1.0e-7, 1.0e-7},
                        // Hostile: |coord * factor| far exceeds PerlinNoise.wrap's
                        // ROUND_OFF (3.3554432e7), so the wrap branch fires in
                        // real worldgen (block coords reach ±30M and scale by up
                        // to 684.412). Pins the wrap path against Paper.
                        {1.0e9, -1.0e9, 3.0e8}
                    };
                    JsonArray vals = new JsonArray();
                    for (double[] pt : xyz) {
                        JsonObject v = new JsonObject();
                        v.addProperty("x", pt[0]);
                        v.addProperty("y", pt[1]);
                        v.addProperty("z", pt[2]);
                        v.add("v", bits(p.getValue(pt[0], pt[1], pt[2])));
                        v.add("vDeprecated", bits(p.getValue(pt[0], pt[1], pt[2], 0.25, 0.5)));
                        vals.add(v);
                    }
                    e.add("vals", vals);
                    perlin.add(e);
                }
            }
        }
        root.add("perlin", perlin);

        // PerlinNoise via create(random, firstOctave, firstAmplitude, ...)
        JsonArray perlinAmplitudes = new JsonArray();
        for (long seed : new long[] {0L, 12345L}) {
            for (String source : new String[] {"xoroshiro", "legacy"}) {
                RandomSource r = random(source, seed);
                PerlinNoise p = PerlinNoise.create(r, -3, 1.0, 1.0, 0.5, 0.25, 0.125);
                JsonObject e = new JsonObject();
                e.addProperty("seed", seed);
                e.addProperty("source", source);
                e.add("maxValue", bits(maxValue(p)));
                e.add("v", bits(p.getValue(3.25, -7.5, 0.75)));
                perlinAmplitudes.add(e);
            }
        }
        root.add("perlin_amplitudes", perlinAmplitudes);

        // ---- NormalNoise ----------------------------------------------------
        JsonArray normal = new JsonArray();
        for (long seed : new long[] {0L, 12345L, -987654321L}) {
            for (String source : new String[] {"xoroshiro", "legacy"}) {
                RandomSource r = random(source, seed);
                NormalNoise n = NormalNoise.create(r, -3, 1.0, 1.0, 0.5, 0.25, 0.125);
                JsonObject e = new JsonObject();
                e.addProperty("seed", seed);
                e.addProperty("source", source);
                e.add("maxValue", bits(n.maxValue()));
                double[][] xyz = {
                    {0.0, 0.0, 0.0}, {1.0, 1.0, 1.0}, {0.5, -1.5, 2.5}, {10.0, 10.0, 10.0},
                    {-10.0, 10.0, -10.0}, {100.0, -100.0, 100.0}, {0.1, 0.2, 0.3},
                    {123.456, -789.012, 3.14159}, {1.0e6, -1.0e6, 1.0e6}, {1.0e-7, 1.0e-7, 1.0e-7}
                };
                JsonArray vals = new JsonArray();
                for (double[] pt : xyz) {
                    JsonObject v = new JsonObject();
                    v.addProperty("x", pt[0]);
                    v.addProperty("y", pt[1]);
                    v.addProperty("z", pt[2]);
                    v.add("v", bits(n.getValue(pt[0], pt[1], pt[2])));
                    vals.add(v);
                }
                e.add("vals", vals);
                normal.add(e);
            }
        }
        root.add("normal", normal);

        // NormalNoise legacy-nether-biome construction (useNewInitialization=false)
        JsonArray normalLegacy = new JsonArray();
        for (long seed : new long[] {0L, 12345L}) {
            for (String source : new String[] {"xoroshiro", "legacy"}) {
                RandomSource r = random(source, seed);
                NormalNoise n = NormalNoise.createLegacyNetherBiome(
                    r, new NormalNoise.NoiseParameters(-3, List.of(1.0, 1.0, 0.5, 0.25)));
                JsonObject e = new JsonObject();
                e.addProperty("seed", seed);
                e.addProperty("source", source);
                e.add("maxValue", bits(n.maxValue()));
                e.add("v", bits(n.getValue(3.25, -7.5, 0.75)));
                normalLegacy.add(e);
            }
        }
        root.add("normal_legacy", normalLegacy);

        // ---- BlendedNoise ---------------------------------------------------
        JsonArray blended = new JsonArray();
        double[] scales = {1.0, 4.0, 0.5};
        for (double xzScale : scales) {
            for (double yScale : scales) {
                for (double xzFactor : new double[] {80.0, 160.0}) {
                    for (double yFactor : new double[] {160.0, 320.0}) {
                        for (double smear : new double[] {1.0, 2.0}) {
                            BlendedNoise b = BlendedNoise.createUnseeded(xzScale, yScale, xzFactor, yFactor, smear);
                            JsonObject e = new JsonObject();
                            e.addProperty("xzScale", xzScale);
                            e.addProperty("yScale", yScale);
                            e.addProperty("xzFactor", xzFactor);
                            e.addProperty("yFactor", yFactor);
                            e.addProperty("smear", smear);
                            e.add("maxValue", bits(b.maxValue()));
                            e.add("minValue", bits(b.minValue()));
                            double[][] xyz = {
                                {0.0, 0.0, 0.0}, {1.0, 1.0, 1.0}, {0.5, -1.5, 2.5},
                                {10.0, 10.0, 10.0}, {-10.0, 10.0, -10.0}, {100.0, -100.0, 100.0},
                                {255.0, 256.0, -255.0}, {1000.0, 0.0, -1000.0}, {-1.0e4, 1.0e4, 1.0e4}
                            };
                            JsonArray vals = new JsonArray();
                            for (double[] pt : xyz) {
                                JsonObject v = new JsonObject();
                                v.addProperty("x", pt[0]);
                                v.addProperty("y", pt[1]);
                                v.addProperty("z", pt[2]);
                                v.add("v", bits(b.compute(new DensityFunction.SinglePointContext((int) pt[0], (int) pt[1], (int) pt[2]))));
                                vals.add(v);
                            }
                            e.add("vals", vals);
                            blended.add(e);
                        }
                    }
                }
            }
        }
        root.add("blended", blended);

        // ---- NoiseUtils -----------------------------------------------------
        JsonArray noiseUtils = new JsonArray();
        {
            JsonObject e = new JsonObject();
            double[] ns = {-1.0, -0.5, 0.0, 0.25, 0.5, 0.75, 1.0, 1.5};
            JsonArray biases = new JsonArray();
            for (double nv : ns) {
                for (double f : new double[] {0.0, 0.5, 1.0, -1.0}) {
                    JsonObject b = new JsonObject();
                    b.addProperty("noise", nv);
                    b.addProperty("factor", f);
                    b.add("v", bits(NoiseUtils.biasTowardsExtreme(nv, f)));
                    biases.add(b);
                }
            }
            e.add("bias", biases);
            StringBuilder sb = new StringBuilder();
            byte[] p = new byte[256];
            for (int i = 0; i < 256; i++) p[i] = (byte) (255 - i);
            NoiseUtils.parityNoiseOctaveConfigString(sb, 1.2345678, -9.8765432, 0.000123456, p);
            e.addProperty("parityByte", sb.toString());
            StringBuilder sb2 = new StringBuilder();
            int[] pi = new int[256];
            for (int i = 0; i < 256; i++) pi[i] = i * 37;
            NoiseUtils.parityNoiseOctaveConfigString(sb2, 1.2345678, -9.8765432, 0.000123456, pi);
            e.addProperty("parityInt", sb2.toString());
            // Exact decimal ties: 1.0625 / -2.0625 are exact binary halves, so
            // Java's `%.3f` must round half-away-from-zero ("1.063"/"-2.063"),
            // not half-even ("1.062"). Pins the midpoint formatting exactly.
            StringBuilder sb3 = new StringBuilder();
            NoiseUtils.parityNoiseOctaveConfigString(sb3, 1.0625, -2.0625, 0.0625, p);
            e.addProperty("parityByteTie", sb3.toString());
            StringBuilder sb4 = new StringBuilder();
            NoiseUtils.parityNoiseOctaveConfigString(sb4, 1.0625, -2.0625, 0.0625, pi);
            e.addProperty("parityIntTie", sb4.toString());
            noiseUtils.add(e);
        }
        root.add("noise_utils", noiseUtils);

        Gson gson = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();
        Path outDir = Path.of(output);
        java.nio.file.Files.createDirectories(outDir);
        try (PrintWriter writer = new PrintWriter(outDir.resolve("synth-noise.json").toFile(), "UTF-8")) {
            gson.toJson(root, writer);
        }
        System.out.println("wrote synth-noise.json");
    }

    private static RandomSource random(final String source, final long seed) {
        if (source.equals("xoroshiro")) {
            return new XoroshiroRandomSource(seed);
        } else {
            return new LegacyRandomSource(seed);
        }
    }

    /** `Double.doubleToLongBits` — the bit-exact golden representation. */
    private static JsonPrimitive bits(final double d) {
        return new JsonPrimitive(Double.doubleToLongBits(d));
    }

    /** Reads the private permutation array `p` at `index` via reflection. */
    private static int perm(final Object noise, final int index) {
        try {
            java.lang.reflect.Field f = noise.getClass().getDeclaredField("p");
            f.setAccessible(true);
            Object arr = f.get(noise);
            if (arr instanceof byte[]) {
                return ((byte[]) arr)[index];
            }
            return ((int[]) arr)[index];
        } catch (ReflectiveOperationException e) {
            throw new RuntimeException("reflection failed for p", e);
        }
    }

    /** Invokes the protected `maxValue()` on a PerlinNoise via reflection. */
    private static double maxValue(final PerlinNoise noise) {
        try {
            java.lang.reflect.Method m = PerlinNoise.class.getDeclaredMethod("maxValue");
            m.setAccessible(true);
            return (Double) m.invoke(noise);
        } catch (ReflectiveOperationException e) {
            throw new RuntimeException("reflection failed for maxValue", e);
        }
    }
}
