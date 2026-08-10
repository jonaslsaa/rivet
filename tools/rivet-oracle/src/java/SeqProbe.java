import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.PrintWriter;
import java.nio.file.Path;
import java.util.List;
import net.minecraft.core.BlockPos;
import net.minecraft.resources.Identifier;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.PositionalRandomFactory;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;

/**
 * Samples the pinned Paper 26.2 {@code PositionalRandomFactory} default
 * overloads taking {@code BlockPos} / {@code Identifier} and emits stable
 * golden JSON.  This is the Paper-side sampling half of issue #208: the two
 * default interface methods are
 * {@code at(BlockPos) { return at(pos.getX(), pos.getY(), pos.getZ()); }} and
 * {@code fromHashOf(Identifier) { return fromHashOf(name.toString()); }}, so
 * the emitted values are exactly what a faithful Rust port of those overloads
 * must reproduce — the seed derived from the delegate call, not the overload.
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" SeqProbe --output dir/ [--paper pin]
 *
 * Each emitted value is the raw {@code int}/{@code long} from the yielded
 * {@code RandomSource} (no doubleToLongBits — these are integral). A `paper`
 * provenance string is recorded for self-description. No registry/version boot
 * is needed: {@code LegacyRandomSource} / {@code XoroshiroRandomSource} and
 * their positional factories are value-leaf.
 */
public final class SeqProbe {
    private SeqProbe() {}

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
            throw new IllegalArgumentException("Usage: SeqProbe --output <dir> [--paper <pin>]");
        }

        JsonObject root = new JsonObject();
        root.addProperty("paper", paper);

        // The two concrete factory kinds, constructed the same way the Rust
        // port constructs them (LegacyPositionalRandomFactory::new(seed) /
        // XoroshiroPositionalRandomFactory::new(seedLo, seedHi)).
        JsonObject factories = new JsonObject();
        factories.add("legacy",
            probeFactory(new LegacyRandomSource.LegacyPositionalRandomFactory(99L)));
        factories.add("xoroshiro",
            probeFactory(new XoroshiroRandomSource.XoroshiroPositionalRandomFactory(99L, 1234L)));
        root.add("factories", factories);

        Path out = Path.of(output).resolve("seq-random.json");
        out.getParent().toFile().mkdirs();
        try (PrintWriter w = new PrintWriter(out.toFile())) {
            w.println(new GsonBuilder().setPrettyPrinting().create().toJson(root));
        }
        System.out.println("wrote " + out);
    }

    private static JsonObject probeFactory(PositionalRandomFactory f) {
        JsonObject out = new JsonObject();
        JsonArray at = new JsonArray();
        for (int[] pos : List.of(
            new int[] {0, 0, 0},
            new int[] {1, 2, 3},
            new int[] {-123, 64, 456},
            new int[] {30000000, 500, -30000000},
            new int[] {Integer.MIN_VALUE, -64, Integer.MAX_VALUE}
        )) {
            JsonObject e = new JsonObject();
            e.add("pos", arr(pos));
            RandomSource r = f.at(new BlockPos(pos[0], pos[1], pos[2]));
            e.add("ints", nextInts(r, 3));
            e.add("longs", nextLongs(r, 2));
            at.add(e);
        }
        out.add("at", at);

        JsonArray fromHashOf = new JsonArray();
        for (String id : List.of(
            "minecraft:overworld", "minecraft:stone", "minecraft:foo/bar",
            "a:b", "minecraft:custom_thing"
        )) {
            JsonObject e = new JsonObject();
            e.addProperty("id", id);
            RandomSource r = f.fromHashOf(Identifier.parse(id));
            e.add("ints", nextInts(r, 3));
            e.add("longs", nextLongs(r, 2));
            fromHashOf.add(e);
        }
        out.add("fromHashOf", fromHashOf);
        return out;
    }

    private static JsonArray arr(int[] a) {
        JsonArray arr = new JsonArray();
        for (int v : a) {
            arr.add(v);
        }
        return arr;
    }

    private static JsonArray nextInts(RandomSource r, int n) {
        JsonArray arr = new JsonArray();
        for (int i = 0; i < n; i++) {
            arr.add(r.nextInt());
        }
        return arr;
    }

    private static JsonArray nextLongs(RandomSource r, int n) {
        JsonArray arr = new JsonArray();
        for (int i = 0; i < n; i++) {
            arr.add(r.nextLong());
        }
        return arr;
    }
}
