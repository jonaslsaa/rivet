//! WorldgenDataExtractor — dumps the deterministic MC 26.2 worldgen noise
//! registry, the per-biome climate configuration, and the multi-noise biome
//! source preset parameter points from a live Paper load (issue #354).
//!
//! The noise + biome registries are datapack-loaded (WORLDGEN_REGISTRIES), so
//! their ids are assigned at runtime by `ResourceManagerRegistryLoadTask` from
//! a `TreeMap<Identifier, Resource>` sorted by `Identifier` compareTo (path
//! first, then namespace) — i.e. id 0 = `minecraft:aquifer_barrier` for noise
//! and `minecraft:badlands` for biomes, alphabetical. The multi-noise preset
//! parameter points are hardcoded in `MultiNoiseBiomeSourceParameterList.Preset`
//! (overworld -> `OverworldBiomeBuilder`, nether -> the inline list), so they
//! are read through the public static `knownPresets()` rather than invented or
//! copied by hand.
//!
//! The load sequence below mirrors `WorldLoader.load` (identical to
//! `BiomeTagExtractor`): vanilla pack source -> STATIC layer ->
//! `TagLoader.loadTagsForExistingRegistries` -> `buildUpdatedLookups` ->
//! `RegistryDataLoader.load(WORLDGEN_REGISTRIES)` -> `replaceFrom(WORLDGEN)`.
//!
//! Output JSON (written to --output) is a single object with:
//!   generator / minecraft_version / protocol_version / world_version
//!   noise   : name -> { id, firstOctave, amplitudes[] }   (dense 0..n, byId order)
//!   biomes  : name -> { id, temperature, downfall, has_precipitation,
//!                       temperature_modifier }            (dense 0..n, byId order)
//!   presets : preset id -> [ { biome, temperature{min,max}, humidity{min,max},
//!                             continentalness{min,max}, erosion{min,max},
//!                             depth{min,max}, weirdness{min,max}, offset } ]
//!             in the builder's value order (never sorted)
//!   probe   : noise_count / biome_count / preset_count / per-preset point counts
//!
//! Parameter min/max are the quantized longs (`Climate.quantizeCoord`, i.e.
//! `(long)(coord * 10000.0F)`) exactly as stored in the runtime `ParameterPoint`,
//! so the generated Rust table can reconstruct the exact values with no float
//! round-trip through the fixture.
//!
//! Determinism: registry keys, preset ids, and element ids are all ordered; the
//! point list order is the builder's fixed value order, so two independent runs
//! are byte-identical.

import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.mojang.datafixers.util.Pair;
import java.io.BufferedWriter;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.VanillaPackResources;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterList;
import net.minecraft.world.level.levelgen.synth.NormalNoise;

public final class WorldgenDataExtractor {
    private WorldgenDataExtractor() {}

    private static String argValue(String[] args, String name) {
        for (int i = 0; i + 1 < args.length; i++) {
            if (args[i].equals(name)) {
                return args[i + 1];
            }
        }
        return null;
    }

    /** Dense name -> {id, ...} element table in byId order (listElements is byId). */
    private static <T> JsonObject elementTable(Registry<T> registry, ElementWriter<T> writer) {
        JsonObject elements = new JsonObject();
        registry.listElements().forEach(h -> {
            T value = h.value();
            int id = registry.getId(value);
            if (id < 0) {
                throw new IllegalStateException("unregistered element in byId iteration: " + h.key());
            }
            elements.add(h.key().identifier().toString(), writer.write(id, value));
        });
        return elements;
    }

    @FunctionalInterface
    private interface ElementWriter<T> {
        JsonObject write(int id, T value);
    }

    /** A `Climate.Parameter` pair of quantized longs as {"min":..,"max":..}. */
    private static void addParameter(JsonObject out, String name, Climate.Parameter p) {
        JsonObject span = new JsonObject();
        span.addProperty("min", p.min());
        span.addProperty("max", p.max());
        out.add(name, span);
    }

    public static void main(String[] args) throws Exception {
        String output = argValue(args, "--output");
        String mcVersion = argValue(args, "--version");
        if (output == null) {
            throw new IllegalArgumentException("--output <path> is required");
        }

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        JsonObject root = new JsonObject();
        root.addProperty("generator", "WorldgenDataExtractor (Bootstrap + RegistryDataLoader + knownPresets)");
        root.addProperty("minecraft_version", mcVersion);
        root.addProperty("protocol_version", SharedConstants.getProtocolVersion());
        root.addProperty("world_version", SharedConstants.WORLD_VERSION);

        VanillaPackResources vanilla = ServerPacksSource.createVanillaPackSource();
        try (MultiPackResourceManager resources = new MultiPackResourceManager(PackType.SERVER_DATA, List.<PackResources>of(vanilla))) {
            LayeredRegistryAccess<RegistryLayer> initialLayers = RegistryLayer.createRegistryAccess();
            List<Registry.PendingTags<?>> staticLayerTags = TagLoader.loadTagsForExistingRegistries(
                resources, initialLayers.getLayer(RegistryLayer.STATIC)
            );
            RegistryAccess.Frozen worldgenLoadContext = initialLayers.getAccessForLoading(RegistryLayer.WORLDGEN);
            List<HolderLookup.RegistryLookup<?>> wctx =
                TagLoader.buildUpdatedLookups(worldgenLoadContext, staticLayerTags);
            RegistryAccess.Frozen worldgen = RegistryDataLoader.load(
                    resources,
                    wctx,
                    RegistryDataLoader.WORLDGEN_REGISTRIES,
                    ForkJoinPool.commonPool()
                )
                .join();

            // -- noise registry (minecraft:worldgen/noise) -------------------------
            Registry<NormalNoise.NoiseParameters> noise = worldgen.lookupOrThrow(Registries.NOISE);
            root.add(
                "noise",
                elementTable(
                    noise,
                    (id, p) -> {
                        JsonObject entry = new JsonObject();
                        entry.addProperty("id", id);
                        entry.addProperty("firstOctave", p.firstOctave());
                        JsonArray amplitudes = new JsonArray();
                        for (int i = 0; i < p.amplitudes().size(); i++) {
                            amplitudes.add(p.amplitudes().getDouble(i));
                        }
                        entry.add("amplitudes", amplitudes);
                        return entry;
                    }
                )
            );

            // -- biome climate configuration (minecraft:worldgen/biome) ----------
            Registry<Biome> biomes = worldgen.lookupOrThrow(Registries.BIOME);
            root.add(
                "biomes",
                elementTable(
                    biomes,
                    (id, b) -> {
                        Biome.ClimateSettings climate = b.climateSettings;
                        JsonObject entry = new JsonObject();
                        entry.addProperty("id", id);
                        entry.addProperty("temperature", climate.temperature());
                        entry.addProperty("downfall", climate.downfall());
                        entry.addProperty("has_precipitation", climate.hasPrecipitation());
                        entry.addProperty("temperature_modifier", climate.temperatureModifier().getSerializedName());
                        return entry;
                    }
                )
            );

            // -- multi-noise biome source preset parameter points ------------------
            Map<MultiNoiseBiomeSourceParameterList.Preset, Climate.ParameterList<ResourceKey<Biome>>> presets =
                MultiNoiseBiomeSourceParameterList.knownPresets();
            JsonObject presetsOut = new JsonObject();
            List<MultiNoiseBiomeSourceParameterList.Preset> sortedPresets =
                new ArrayList<>(presets.keySet());
            // knownPresets() returns a HashMap (toMap), so sort by id for a
            // deterministic fixture; the point lists keep the builder's order.
            sortedPresets.sort(Comparator.comparing(p -> p.id().toString()));
            JsonObject probe = new JsonObject();
            probe.addProperty("noise_count", root.getAsJsonObject("noise").size());
            probe.addProperty("biome_count", biomes.size());
            probe.addProperty("preset_count", presets.size());
            for (MultiNoiseBiomeSourceParameterList.Preset preset : sortedPresets) {
                Climate.ParameterList<ResourceKey<Biome>> list = presets.get(preset);
                JsonArray points = new JsonArray();
                for (Pair<Climate.ParameterPoint, ResourceKey<Biome>> pair : list.values()) {
                    Climate.ParameterPoint pp = pair.getFirst();
                    JsonObject point = new JsonObject();
                    point.addProperty("biome", pair.getSecond().identifier().toString());
                    addParameter(point, "temperature", pp.temperature());
                    addParameter(point, "humidity", pp.humidity());
                    addParameter(point, "continentalness", pp.continentalness());
                    addParameter(point, "erosion", pp.erosion());
                    addParameter(point, "depth", pp.depth());
                    addParameter(point, "weirdness", pp.weirdness());
                    point.addProperty("offset", pp.offset());
                    points.add(point);
                }
                presetsOut.add(preset.id().toString(), points);
                probe.addProperty(preset.id().getPath() + "_point_count", points.size());
            }
            root.add("presets", presetsOut);
            root.add("probe", probe);
        }

        String json = new GsonBuilder().setPrettyPrinting().create().toJson(root);
        try (Writer w = new BufferedWriter(
            new OutputStreamWriter(new FileOutputStream(output), StandardCharsets.UTF_8)
        )) {
            w.write(json);
            w.write("\n");
        }
    }
}
