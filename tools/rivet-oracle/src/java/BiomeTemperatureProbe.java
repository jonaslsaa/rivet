import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.PrintWriter;
import java.nio.file.Path;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeGenerationSettings;
import net.minecraft.world.level.biome.BiomeSpecialEffects;
import net.minecraft.world.level.biome.MobSpawnSettings;

/**
 * Grounding probe: emits the exact Paper 26.2 `Biome.getTemperature` /
 * `coldEnoughToSnow` / `getPrecipitationAt` outputs for constructed biomes
 * across a position grid, plus the raw noise values the temperature arithmetic
 * consumes (`TEMPERATURE_NOISE` for the snow-level drop, and the FROZEN
 * modifier's `FROZEN_TEMPERATURE_NOISE`/`BIOME_INFO_NOISE` terms) so the Rust
 * port can assert both the aggregate outputs and the FROZEN branch decisions
 * independently against real Paper values.
 *
 * Values are emitted as `doubleToLongBits` / `floatToIntBits` for exact bits.
 * The noise fields are package-private statics in `Biome`, read by reflection.
 * Deterministic across boots.
 */
public final class BiomeTemperatureProbe {
    private BiomeTemperatureProbe() {}

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
            throw new IllegalArgumentException("Usage: BiomeTemperatureProbe --output <dir> [--paper <pin>]");
        }

        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();

        // The shared position grid: every sample and every noise probe use the
        // same (x, z) set so the Rust side can correlate them. Includes
        // positions that sit near the FROZEN modifier's branch thresholds
        // (ice_patches ~ 0.3, groundValueSmallVariation ~ 0.8) so a moderate
        // constant drift (e.g. the * 7.0 amplitude, the 0.3/0.8 gates, the
        // edge scale) flips a sampled branch decision instead of being masked.
        // `outer` positions bracket the 0.3 gate on both sides; `inner`
        // positions bracket the 0.8 small-variation gate (with ice < 0.3 so
        // the inner gate is the deciding one).
        int[][] grid = {
            {0, 0}, {0, 8}, {8, 0}, {16, 16}, {32, 0}, {0, 32}, {100, 50}, {-50, 100},
            {200, 200}, {1234, 4321}, {7, 3}, {15, 15}, {0, 63}, {0, 1}, {8, 8}, {16, 8},
            {1, 49}, {2, 50},
            // outer: ice_patches just above/below 0.3.
            {0, 112}, {4, 175}, {5, 97},
            // inner: ice_patches < 0.3, small-variation just above/below 0.8.
            {2, 55}, {3, 52},
        };

        JsonObject root = new JsonObject();
        root.addProperty("paper", paper);

        JsonArray biomes = new JsonArray();
        // plain: NONE modifier, temp 0.8, hasPrecipitation true.
        biomes.add(sampleBiome("plain", true, 0.8f, Biome.TemperatureModifier.NONE, grid));
        // cold: NONE modifier, temp 0.0, hasPrecipitation true.
        biomes.add(sampleBiome("cold", true, 0.0f, Biome.TemperatureModifier.NONE, grid));
        // frozen: FROZEN modifier, temp 0.7, hasPrecipitation true.
        biomes.add(sampleBiome("frozen", true, 0.7f, Biome.TemperatureModifier.FROZEN, grid));
        // no-precip: NONE modifier, temp -1.0, hasPrecipitation false.
        biomes.add(sampleBiome("arid", false, -1.0f, Biome.TemperatureModifier.NONE, grid));
        root.add("biomes", biomes);

        // The raw noise values per (x, z): the snow-level `v` term and the three
        // FROZEN modifier noise samples (large/edge/small variation).
        JsonArray noise = new JsonArray();
        for (int[] p : grid) {
            JsonObject e = new JsonObject();
            e.addProperty("x", p[0]);
            e.addProperty("z", p[1]);
            double temp = temperatureNoise(p[0], p[1]);
            double v = temp * 8.0;
            e.addProperty("temperatureNoise", Double.doubleToLongBits(temp));
            e.addProperty("snowLevelV", Float.floatToIntBits((float) v));
            e.addProperty("frozenLarge", Double.doubleToLongBits(frozenLarge(p[0], p[1])));
            e.addProperty("frozenEdge", Double.doubleToLongBits(frozenEdge(p[0], p[1])));
            e.addProperty("frozenSmall", Double.doubleToLongBits(frozenSmall(p[0], p[1])));
            noise.add(e);
        }
        root.add("noise", noise);

        Gson gson = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();
        Path outDir = Path.of(output);
        java.nio.file.Files.createDirectories(outDir);
        try (PrintWriter writer = new PrintWriter(outDir.resolve("biome-temperature.json").toFile(), "UTF-8")) {
            gson.toJson(root, writer);
        }
        System.out.println("wrote " + outDir.resolve("biome-temperature.json"));
    }

    private static JsonObject sampleBiome(
        String name,
        boolean hasPrecip,
        float temp,
        Biome.TemperatureModifier mod,
        int[][] grid
    ) {
        Biome b = new Biome.BiomeBuilder()
            .hasPrecipitation(hasPrecip)
            .temperature(temp)
            .temperatureAdjustment(mod)
            .downfall(0.4f)
            .specialEffects(new BiomeSpecialEffects.Builder().waterColor(0).build())
            .mobSpawnSettings(MobSpawnSettings.EMPTY)
            .generationSettings(BiomeGenerationSettings.EMPTY)
            .build();
        JsonObject out = new JsonObject();
        out.addProperty("name", name);
        out.addProperty("hasPrecipitation", hasPrecip);
        out.addProperty("temperature", Float.floatToIntBits(temp));
        out.addProperty("temperatureModifier", mod.getSerializedName());
        JsonArray samples = new JsonArray();
        for (int[] p : grid) {
            int seaLevel = 63;
            int x = p[0], z = p[1];
            // Sample at the column top, at/above the snow level (y > seaLevel+17
            // = 80), and high enough that a FROZEN pin (0.2) minus the snow-level
            // drop crosses the 0.15 warmEnoughToRain boundary into SNOW: at
            // (0,0) y=150 the drop is (150-80)*0.05/40 = 0.0875, so the FROZEN
            // temperature is 0.2 - 0.0875 = 0.1125 < 0.15. The 0.15 boundary
            // (>= vs >, or a drifted threshold) is only caught by sampling below
            // AND above it.
            int[] ys = {1, 80, 81, 100, 120, 150, 200};
            for (int y : ys) {
                JsonObject s = new JsonObject();
                s.addProperty("x", x);
                s.addProperty("y", y);
                s.addProperty("z", z);
                s.addProperty("seaLevel", seaLevel);
                float t = b.getTemperature(new BlockPos(x, y, z), seaLevel);
                s.addProperty("getTemperature", Float.floatToIntBits(t));
                s.addProperty(
                    "coldEnoughToSnow",
                    b.coldEnoughToSnow(new BlockPos(x, y, z), seaLevel)
                );
                s.addProperty(
                    "warmEnoughToRain",
                    b.warmEnoughToRain(new BlockPos(x, y, z), seaLevel)
                );
                s.addProperty(
                    "getPrecipitationAt",
                    b.getPrecipitationAt(new BlockPos(x, y, z), seaLevel).getSerializedName()
                );
                // The FROZEN modifier's independent branch outcome computed from
                // Paper's raw noise (see TemperatureModifier.FROZEN): the test
                // recomputes it from the noise-array values it already pins
                // bit-exactly against Rust, so a branch-logic drift is caught.
                if (mod == Biome.TemperatureModifier.FROZEN) {
                    double large = frozenLarge(x, z) * 7.0;
                    double edge = frozenEdge(x, z);
                    double icePatches = large + edge;
                    double small = frozenSmall(x, z);
                    s.addProperty("frozenPins", icePatches < 0.3 && small < 0.8);
                }
                samples.add(s);
            }
        }
        out.add("samples", samples);
        return out;
    }

    private static double noiseValue(String field, double x, double z) {
        try {
            java.lang.reflect.Field f = Biome.class.getDeclaredField(field);
            f.setAccessible(true);
            Object noise = f.get(null);
            java.lang.reflect.Method m = noise.getClass().getMethod("getValue", double.class, double.class, boolean.class);
            return (double) m.invoke(noise, x, z, false);
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
    }

    /// `TEMPERATURE_NOISE.getValue(x / 8.0F, z / 8.0F, false)` — the snow-level
    /// noise (float-widened to double; the `x / 8.0F` is a float division).
    private static double temperatureNoise(int x, int z) {
        return noiseValue("TEMPERATURE_NOISE", x / 8.0F, z / 8.0F);
    }

    /// `FROZEN_TEMPERATURE_NOISE.getValue(x * 0.05, z * 0.05, false)`.
    private static double frozenLarge(int x, int z) {
        return noiseValue("FROZEN_TEMPERATURE_NOISE", x * 0.05, z * 0.05);
    }

    /// `BIOME_INFO_NOISE.getValue(x * 0.2, z * 0.2, false)`.
    private static double frozenEdge(int x, int z) {
        return noiseValue("BIOME_INFO_NOISE", x * 0.2, z * 0.2);
    }

    /// `BIOME_INFO_NOISE.getValue(x * 0.09, z * 0.09, false)`.
    private static double frozenSmall(int x, int z) {
        return noiseValue("BIOME_INFO_NOISE", x * 0.09, z * 0.09);
    }
}
