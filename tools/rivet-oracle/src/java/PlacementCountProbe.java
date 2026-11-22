import java.io.IOException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.levelgen.placement.NoiseBasedCountPlacement;

/**
 * Paper-side oracle probe for {@link NoiseBasedCountPlacement#count(RandomSource, BlockPos)}.
 *
 * <p>Evaluates the exact 10 golden cases tested in
 * {@code crates/rivet-world/src/levelgen/placement/noise_based_count_placement.rs}
 * against the pinned Paper 26.2 runtime (no server boot required) and writes
 * deterministic JSON to {@code <out>/placement-count.json}.
 *
 * <p>Usage:
 *   java -cp <classes>:<paper-26.2.jar>:<libraries/*> PlacementCountProbe \
 *       --output <dir> [--paper <paper-pin>]
 */
public final class PlacementCountProbe {
    private static final String DEFAULT_PAPER = "26.2-DEV-main@0a99345";

    private record Case(int x, int z, int noiseToCountRatio, double noiseFactor, double noiseOffset) {}

    private static final Case[] CASES = new Case[] {
        new Case(8, 8, 10, 0.01, 0.0),
        new Case(8, 8, 10, 3.0, 0.0),
        new Case(-8, -8, 10, 0.01, 0.0),
        new Case(8, 8, -3, 0.01, 0.0),
        new Case(8, 8, 0, 0.01, 0.0),
        new Case(8, 8, 5, 0.01, -5.0),
        new Case(8, 8, 10, 0.01, 0.5),
        new Case(8, 8, 10, 1.0e6, 0.0),
        new Case(123, -456, 7, 3.5, -0.25),
        new Case(8, 8, Integer.MAX_VALUE, 0.01, 2.0),
    };

    public static void main(String[] args) throws Exception {
        Path outDir = null;
        String paperPin = DEFAULT_PAPER;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--output" -> outDir = Path.of(args[++i]);
                case "--paper" -> paperPin = args[++i];
                default -> throw new IllegalArgumentException("unknown arg: " + args[i]);
            }
        }
        if (outDir == null) {
            throw new IllegalArgumentException("--output <dir> is required");
        }
        Files.createDirectories(outDir);

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        Method countMethod = NoiseBasedCountPlacement.class.getDeclaredMethod(
            "count", RandomSource.class, BlockPos.class);
        countMethod.setAccessible(true);

        RandomSource random = RandomSource.create(0L);

        StringBuilder json = new StringBuilder();
        json.append("{\n");
        json.append("  \"paper\": \"").append(escape(paperPin)).append("\",\n");
        json.append("  \"cases\": [\n");
        for (int i = 0; i < CASES.length; i++) {
            Case c = CASES[i];
            NoiseBasedCountPlacement placement =
                NoiseBasedCountPlacement.of(c.noiseToCountRatio(), c.noiseFactor(), c.noiseOffset());
            int count = (Integer) countMethod.invoke(placement, random, new BlockPos(c.x(), 0, c.z()));

            json.append("    {\n");
            json.append("      \"x\": ").append(c.x()).append(",\n");
            json.append("      \"z\": ").append(c.z()).append(",\n");
            json.append("      \"noiseToCountRatio\": ").append(c.noiseToCountRatio()).append(",\n");
            json.append("      \"noiseFactor\": ").append(c.noiseFactor()).append(",\n");
            json.append("      \"noiseFactorBits\": ").append(Double.doubleToLongBits(c.noiseFactor())).append(",\n");
            json.append("      \"noiseOffset\": ").append(c.noiseOffset()).append(",\n");
            json.append("      \"noiseOffsetBits\": ").append(Double.doubleToLongBits(c.noiseOffset())).append(",\n");
            json.append("      \"count\": ").append(count).append("\n");
            json.append("    }").append(i + 1 < CASES.length ? "," : "").append("\n");
        }
        json.append("  ]\n");
        json.append("}\n");

        Path outFile = outDir.resolve("placement-count.json");
        Files.writeString(outFile, json.toString(), StandardCharsets.UTF_8);
        System.out.println("wrote " + outFile + " (" + CASES.length + " cases)");
    }

    private static String escape(String s) {
        StringBuilder sb = new StringBuilder(s.length());
        for (int i = 0; i < s.length(); i++) {
            char ch = s.charAt(i);
            switch (ch) {
                case '\\' -> sb.append("\\\\");
                case '"' -> sb.append("\\\"");
                case '\n' -> sb.append("\\n");
                case '\r' -> sb.append("\\r");
                case '\t' -> sb.append("\\t");
                default -> sb.append(ch);
            }
        }
        return sb.toString();
    }
}
