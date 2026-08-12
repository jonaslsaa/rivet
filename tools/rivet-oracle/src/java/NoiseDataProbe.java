import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.mojang.serialization.Lifecycle;
import java.io.PrintWriter;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.Registry;
import net.minecraft.data.worldgen.BootstrapContext;
import net.minecraft.data.worldgen.NoiseData;
import net.minecraft.resources.ResourceKey;
import net.minecraft.world.level.levelgen.synth.NormalNoise;

/**
 * Captures the declaration order and parameters of `NoiseData.bootstrap`
 * (the `mc.data.worldgen.prereq` unit): drives the full `NoiseData.bootstrap`
 * into an anonymous `BootstrapContext` that records every `register` call in
 * order, then emits the registered keys and `NoiseParameters` as golden JSON.
 *
 * The keys come from the static `Noises` fields and the method body is pure
 * value construction — no registry boot. `bootstrap` never calls `lookup`, so
 * the recording context answers it with `null` (value-leaf probe).
 *
 * Every registered parameter vector is emitted as its exact `firstOctave` and
 * amplitude `double` list (Gson prints the raw `Double`), so the Rust port can
 * assert the full registered key/parameter order byte-for-byte against the
 * fixture, including the exact `0.013333333333333334` tails.
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" NoiseDataProbe --output dir/ [--paper pin]
 */
public final class NoiseDataProbe {
    private NoiseDataProbe() {}

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
            throw new IllegalArgumentException("Usage: NoiseDataProbe --output <dir> [--paper <pin>]");
        }

        List<ResourceKey<NormalNoise.NoiseParameters>> keys = new ArrayList<>();
        List<NormalNoise.NoiseParameters> params = new ArrayList<>();
        BootstrapContext<NormalNoise.NoiseParameters> context = new BootstrapContext<>() {
            @Override
            public Holder.Reference<NormalNoise.NoiseParameters> register(
                final ResourceKey<NormalNoise.NoiseParameters> key,
                final NormalNoise.NoiseParameters value,
                final Lifecycle lifecycle
            ) {
                keys.add(key);
                params.add(value);
                return null;
            }

            @Override
            public <S> HolderGetter<S> lookup(final ResourceKey<? extends Registry<? extends S>> key) {
                return null;
            }
        };
        NoiseData.bootstrap(context);

        JsonObject root = new JsonObject();
        root.addProperty("paper", paper);
        JsonArray registrations = new JsonArray();
        for (int i = 0; i < keys.size(); i++) {
            JsonObject o = new JsonObject();
            o.addProperty("key", keys.get(i).identifier().toString());
            NormalNoise.NoiseParameters p = params.get(i);
            o.addProperty("firstOctave", p.firstOctave());
            JsonArray amplitudes = new JsonArray();
            for (double a : p.amplitudes()) {
                amplitudes.add(a);
            }
            o.add("amplitudes", amplitudes);
            registrations.add(o);
        }
        root.add("registrations", registrations);

        Path out = Path.of(output).resolve("noise-data-goldens.json");
        out.getParent().toFile().mkdirs();
        try (PrintWriter w = new PrintWriter(out.toFile())) {
            w.println(new GsonBuilder().setPrettyPrinting().create().toJson(root));
        }
        System.out.println("wrote " + out);
    }
}
