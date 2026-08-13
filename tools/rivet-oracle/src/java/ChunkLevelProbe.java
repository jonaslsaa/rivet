import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.PrintWriter;
import java.nio.file.Path;
import net.minecraft.server.level.ChunkLevel;
import net.minecraft.server.level.FullChunkStatus;
import net.minecraft.world.level.chunk.status.ChunkStatus;

/**
 * Samples the pinned Paper 26.2 {@code net.minecraft.server.level.ChunkLevel}
 * value layer (the {@code mc.server.level.pipeline.level} manifest unit) and
 * emits stable golden JSON.  This is the Paper-side sampling half of the
 * pipeline-level value port: {@code ChunkLevel}'s level↔status mappings are
 * derived from the generation pyramid's FULL step accumulated dependencies
 * ({@code ChunkPyramid.GENERATION_PYRAMID.getStepTo(ChunkStatus.FULL).
 * accumulatedDependencies()}), so every emitted value is exactly what a
 * faithful Rust port of {@code ChunkLevel} must reproduce — generated from the
 * real Java, never hand-written.
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" ChunkLevelProbe --output dir/ [--paper pin]
 *
 * {@code ChunkStatus}' static registration touches {@code BuiltInRegistries},
 * so {@code SharedConstants.tryDetectVersion()} + {@code Bootstrap.bootStrap()}
 * must run first (mirroring SynthNoiseProbe / WorldGenSampler). A `paper`
 * provenance string is recorded for self-description. Deterministic across
 * boots for the pinned Paper.
 */
public final class ChunkLevelProbe {
    private ChunkLevelProbe() {}

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
            throw new IllegalArgumentException("Usage: ChunkLevelProbe --output <dir> [--paper <pin>]");
        }

        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();

        JsonObject root = new JsonObject();
        root.addProperty("paper", paper);

        JsonObject constants = new JsonObject();
        constants.addProperty("FULL_CHUNK_LEVEL", ChunkLevel.FULL_CHUNK_LEVEL);
        constants.addProperty("BLOCK_TICKING_LEVEL", ChunkLevel.BLOCK_TICKING_LEVEL);
        constants.addProperty("ENTITY_TICKING_LEVEL", ChunkLevel.ENTITY_TICKING_LEVEL);
        constants.addProperty("RADIUS_AROUND_FULL_CHUNK", ChunkLevel.RADIUS_AROUND_FULL_CHUNK);
        constants.addProperty("MAX_LEVEL", ChunkLevel.MAX_LEVEL);
        root.add("constants", constants);

        // byStatus(ChunkStatus) for every ladder rung, plus the hostile
        // FULL/EMPTY extremes that collapse onto MAX_LEVEL.
        JsonArray byStatus = new JsonArray();
        for (ChunkStatus status : ChunkStatus.getStatusList()) {
            JsonObject e = new JsonObject();
            e.addProperty("status", status.getName());
            e.addProperty("index", status.getIndex());
            e.addProperty("level", ChunkLevel.byStatus(status));
            byStatus.add(e);
        }
        root.add("byStatus", byStatus);

        // generationStatus(level): level -> status (null when outside the
        // generation radius). Covers the full level band 0..48 (the boundaries
        // at ENTITY/BLOCK/FULL_CHUNK_LEVEL, the radius edge at MAX_LEVEL, and
        // past it) plus i32 extremes.
        JsonArray generationStatus = new JsonArray();
        for (int level : levels()) {
            JsonObject e = new JsonObject();
            e.addProperty("level", level);
            ChunkStatus status = ChunkLevel.generationStatus(level);
            e.add("status", status == null ? JsonNull() : new Gson().toJsonTree(status.getName()));
            generationStatus.add(e);
        }
        root.add("generationStatus", generationStatus);

        // getStatusAroundFullChunk(distance) with the EMPTY default, covering
        // negative distances, 0, the dependency band 1..RADIUS, past the
        // radius, and i32 extremes.
        JsonArray statusAround = new JsonArray();
        for (int distance : distances()) {
            JsonObject e = new JsonObject();
            e.addProperty("distance", distance);
            e.addProperty("status", ChunkLevel.getStatusAroundFullChunk(distance).getName());
            statusAround.add(e);
        }
        root.add("statusAroundFullChunk", statusAround);

        // getStatusAroundFullChunk(distance, defaultValue): the non-null default
        // is returned past the radius (null default -> null, exercised by
        // generationStatus above).
        JsonArray statusAroundDefault = new JsonArray();
        for (int distance : new int[] {-7, 0, 1, 11, 12, 13, Integer.MAX_VALUE}) {
            JsonObject e = new JsonObject();
            e.addProperty("distance", distance);
            e.addProperty("status", ChunkLevel.getStatusAroundFullChunk(distance, ChunkStatus.BIOMES).getName());
            statusAroundDefault.add(e);
        }
        root.add("statusAroundFullChunkDefault", statusAroundDefault);

        // byStatus(FullChunkStatus): the level each status ladder rung maps to.
        JsonArray byFullStatus = new JsonArray();
        for (FullChunkStatus fullStatus : FullChunkStatus.values()) {
            JsonObject e = new JsonObject();
            e.addProperty("fullStatus", fullStatus.name());
            e.addProperty("ordinal", fullStatus.ordinal());
            e.addProperty("level", ChunkLevel.byStatus(fullStatus));
            byFullStatus.add(e);
        }
        root.add("byFullStatus", byFullStatus);

        // fullStatus(level): the inverse of byStatus(FullChunkStatus).
        JsonArray fullStatus = new JsonArray();
        for (int level : new int[] {
            Integer.MIN_VALUE, -100, 0, 1, 30, 31, 32, 33, 34, 43, 44, 45, 100, Integer.MAX_VALUE
        }) {
            JsonObject e = new JsonObject();
            e.addProperty("level", level);
            e.addProperty("fullStatus", ChunkLevel.fullStatus(level).name());
            fullStatus.add(e);
        }
        root.add("fullStatus", fullStatus);

        // isEntityTicking / isBlockTicking / isLoaded level thresholds.
        JsonArray predicates = new JsonArray();
        for (int level : new int[] {
            Integer.MIN_VALUE, -100, 0, 1, 30, 31, 32, 33, 34, 43, 44, 45, 100, Integer.MAX_VALUE
        }) {
            JsonObject e = new JsonObject();
            e.addProperty("level", level);
            e.addProperty("isEntityTicking", ChunkLevel.isEntityTicking(level));
            e.addProperty("isBlockTicking", ChunkLevel.isBlockTicking(level));
            e.addProperty("isLoaded", ChunkLevel.isLoaded(level));
            predicates.add(e);
        }
        root.add("predicates", predicates);

        // FullChunkStatus ordinal ladder: isOrAfter is the ordinal comparison
        // `this.ordinal() >= step.ordinal()`. Pins every pair so a reordered
        // Rust enum can never silently pass.
        JsonArray ordinals = new JsonArray();
        FullChunkStatus[] ladder = FullChunkStatus.values();
        for (int i = 0; i < ladder.length; i++) {
            JsonObject e = new JsonObject();
            e.addProperty("ordinal", ladder[i].ordinal());
            e.addProperty("name", ladder[i].name());
            ordinals.add(e);
        }
        root.add("fullChunkStatusOrdinals", ordinals);
        JsonArray isOrAfter = new JsonArray();
        for (FullChunkStatus a : ladder) {
            for (FullChunkStatus b : ladder) {
                JsonObject e = new JsonObject();
                e.addProperty("this", a.name());
                e.addProperty("step", b.name());
                e.addProperty("result", a.isOrAfter(b));
                isOrAfter.add(e);
            }
        }
        root.add("fullChunkStatusIsOrAfter", isOrAfter);

        Gson gson = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();
        Path outDir = Path.of(output);
        java.nio.file.Files.createDirectories(outDir);
        try (PrintWriter writer = new PrintWriter(outDir.resolve("chunk-level-goldens.json").toFile(), "UTF-8")) {
            gson.toJson(root, writer);
        }
        System.out.println("wrote chunk-level-goldens.json");
    }

    private static int[] levels() {
        int[] base = new int[] {
            Integer.MIN_VALUE, -100, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
            17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37,
            38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 100, Integer.MAX_VALUE
        };
        return base;
    }

    private static int[] distances() {
        return new int[] {
            Integer.MIN_VALUE, -100, -7, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 100,
            Integer.MAX_VALUE
        };
    }

    private static com.google.gson.JsonElement JsonNull() {
        return com.google.gson.JsonNull.INSTANCE;
    }
}
