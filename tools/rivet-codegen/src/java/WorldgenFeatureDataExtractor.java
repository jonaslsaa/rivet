//! WorldgenFeatureDataExtractor — the Paper-side data foundation for the seed-42
//! FEATURES checkpoint (the `feature-data` capture slice).
//!
//! A live Paper registry load (the same WorldGenSampler load sequence:
//! `ServerPacksSource.createVanillaTrustedRepository()` ->
//! `configurePackRepository` with the `minecraft`+`vanilla` datapacks ->
//! `RegistryDataLoader.load(WORLDGEN_REGISTRIES)` ->
//! `RegistryDataLoader.load(DIMENSION_REGISTRIES)` -> composite access) is
//! sampled to materialize the deterministic seed-42 decoration data:
//!
//!   1. `reachable_biomes` — the biome set that can drive FEATURES placement
//!      into the committed 2x2 grid {(3,3),(4,3),(3,4),(4,4)}. The chunks that
//!      can write into that grid (blockStateWriteRadius(1)) are chunks 2..5,
//!      and each writer's FEATURES pass reads the biome map of its own 3x3
//!      neighborhood, so the biome read set is chunks 1..6. The biome source is
//!      sampled at every quart position and every Y quart (-64..319 blocks) —
//!      the depth parameter varies by Y, so both surface biomes and the deep
//!      `lush_caves` biome appear.
//!   2. `biomes` — the full `BiomeGenerationSettings` of each reachable biome:
//!      id, the carver identity names, and the per-step `features` lists (each
//!      `HolderSet<PlacedFeature>` in the builder's step order, each placed
//!      feature in the holder-set order).
//!   3. `placed_features` / `configured_features` — the transitive closure of
//!      referenced registry entries, each stored as its full
//!      `RegistryOps`-encoded JSON (the exact datapack JSON shape: holder
//!      references are strings, inline values are nested).
//!
//! The closure rule (what a future FEATURES port must be able to decode):
//!   * placed set starts from the reachable biomes' direct per-step placed
//!     features, and grows by every placed-feature reference found inside a
//!     configured feature's RegistryOps-encoded JSON (e.g. `random_selector`
//!     configs reference `WeightedPlacedFeature` holders);
//!   * configured set starts from the placed features' `feature` holders, and
//!     grows by every configured-feature reference found inside a configured
//!     feature's encoded JSON (sub-features, etc.);
//!   * iterated to fixpoint. Registry membership disambiguates a bare string in
//!     the encoded JSON (a block-state `Name` is an object field, never a bare
//!     holder ref; only registry-reference holders encode as bare strings).
//!
//! Determinism: the reachable biome list is sorted by id (the registry's dense
//! id order), the per-step feature lists keep the runtime holder-set order, the
//! feature element objects are emitted name-sorted, and the whole dump is
//! written with a fixed pretty-printer — two independent runs are byte-identical
//! (the probe asserts this against the committed fixture).

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.mojang.serialization.DataResult;
import com.mojang.serialization.JsonOps;
import java.io.BufferedWriter;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Deque;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.QuartPos;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.RegistryOps;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.packs.resources.ResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.feature.ConfiguredFeature;
import net.minecraft.world.level.levelgen.placement.PlacedFeature;

public final class WorldgenFeatureDataExtractor {
    private WorldgenFeatureDataExtractor() {}

    private static String argValue(String[] args, String name) {
        for (int i = 0; i + 1 < args.length; i++) {
            if (args[i].equals(name)) {
                return args[i + 1];
            }
        }
        return null;
    }

    public static void main(String[] args) throws Exception {
        String output = argValue(args, "--output");
        String mcVersion = argValue(args, "--version");
        String paper = argValue(args, "--paper");
        long seed = 42L;
        String seedArg = argValue(args, "--seed");
        if (seedArg != null) {
            seed = Long.parseLong(seedArg);
        }
        if (output == null || paper == null) {
            throw new IllegalArgumentException("--output <path> and --paper <pin> are required");
        }

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        JsonObject root = new JsonObject();
        root.addProperty("format", 1);
        root.addProperty("generator", "WorldgenFeatureDataExtractor (seed-42 reachable-biome + feature closure)");
        root.addProperty("paper", paper);
        root.addProperty("minecraft_version", mcVersion);
        root.addProperty("protocol_version", SharedConstants.getProtocolVersion());
        root.addProperty("world_version", SharedConstants.WORLD_VERSION);
        root.addProperty("seed", seed);
        root.addProperty("grid_min_chunk", 1);
        root.addProperty("grid_max_chunk", 6);
        root.addProperty("committed_grid", "{(3,3),(4,3),(3,4),(4,4)}");

        // ---- registries -------------------------------------------------------
        PackRepository packRepository = ServerPacksSource.createVanillaTrustedRepository();
        MinecraftServer.configurePackRepository(
            packRepository,
            new net.minecraft.world.level.WorldDataConfiguration(
                new net.minecraft.world.level.DataPackConfig(List.of("minecraft", "vanilla"), List.of()),
                FeatureFlags.DEFAULT_FLAGS
            ),
            true,
            false
        );
        ResourceManager resourceManager = new MultiPackResourceManager(PackType.SERVER_DATA, packRepository.openAllSelected());

        var layers = RegistryLayer.createRegistryAccess();
        List<net.minecraft.core.HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(
            layers.getAccessForLoading(RegistryLayer.WORLDGEN),
            TagLoader.loadTagsForExistingRegistries(resourceManager, layers.getLayer(RegistryLayer.STATIC))
        );
        RegistryAccess.Frozen worldGenRegistries = RegistryDataLoader.load(
            resourceManager, lookups, RegistryDataLoader.WORLDGEN_REGISTRIES, Runnable::run
        ).join();
        layers = layers.replaceFrom(RegistryLayer.WORLDGEN, worldGenRegistries);

        List<net.minecraft.core.HolderLookup.RegistryLookup<?>> staticAndWorldgen = new java.util.ArrayList<>(lookups);
        worldGenRegistries.listRegistries().forEach(staticAndWorldgen::add);
        RegistryAccess.Frozen dimensionRegistries = RegistryDataLoader.load(
            resourceManager, staticAndWorldgen, RegistryDataLoader.DIMENSION_REGISTRIES, Runnable::run
        ).join();
        layers = layers.replaceFrom(RegistryLayer.DIMENSIONS, dimensionRegistries);
        RegistryAccess registryAccess = layers.compositeAccess().freeze();

        Registry<Biome> biomeReg = registryAccess.lookupOrThrow(Registries.BIOME);
        Registry<PlacedFeature> placedReg = registryAccess.lookupOrThrow(Registries.PLACED_FEATURE);
        Registry<ConfiguredFeature<?, ?>> configuredReg = registryAccess.lookupOrThrow(Registries.CONFIGURED_FEATURE);
        // The composite access includes the STATIC layer (block registry + bound
        // block tags), which feature configs reference — RegistryOps needs it.
        RegistryOps<JsonElement> regOps = RegistryOps.create(JsonOps.INSTANCE, registryAccess);

        // ---- reachable biome set ---------------------------------------------
        Holder<net.minecraft.world.level.levelgen.presets.WorldPreset> preset =
            registryAccess.lookupOrThrow(Registries.WORLD_PRESET)
                .getOrThrow(net.minecraft.world.level.levelgen.presets.WorldPresets.NORMAL);
        NoiseBasedChunkGenerator generator = (NoiseBasedChunkGenerator) preset.value().createWorldDimensions().overworld();
        RandomState randomState = RandomState.create(registryAccess, NoiseGeneratorSettings.OVERWORLD, seed);
        Climate.Sampler sampler = randomState.sampler();
        MultiNoiseBiomeSource biomeSource = (MultiNoiseBiomeSource) generator.getBiomeSource();

        int lo = QuartPos.fromBlock(1 << 4);
        int hi = QuartPos.fromBlock((6 << 4) + 15);
        int qyMin = QuartPos.fromBlock(-64);
        int qyMax = QuartPos.fromBlock(319);
        // id -> name, sorted by id (the registry's dense id order).
        Map<Integer, String> reachable = new TreeMap<>();
        for (int qx = lo; qx <= hi; qx++) {
            for (int qz = lo; qz <= hi; qz++) {
                for (int qy = qyMin; qy <= qyMax; qy++) {
                    Holder<Biome> b = biomeSource.getNoiseBiome(qx, qy, qz, sampler);
                    String name = b.unwrapKey().map(k -> k.identifier().toString()).orElse("?");
                    reachable.put(biomeReg.getId(b.value()), name);
                }
            }
        }
        if (reachable.size() < 2) {
            throw new IllegalStateException(
                "seed-42 reachable biome set is degenerate (" + reachable.size() + " biome(s)) — non-vacuity failed"
            );
        }
        JsonArray reachableArr = new JsonArray();
        for (String name : reachable.values()) {
            reachableArr.add(name);
        }
        root.add("reachable_biomes", reachableArr);

        // ---- biome generation settings ----------------------------------------
        JsonObject biomesOut = new JsonObject();
        for (Map.Entry<Integer, String> e : reachable.entrySet()) {
            int id = e.getKey();
            String name = e.getValue();
            Biome biome = biomeReg.getOrThrow(ResourceKey.create(Registries.BIOME, net.minecraft.resources.Identifier.parse(name))).value();
            JsonObject bj = new JsonObject();
            bj.addProperty("id", id);
            JsonArray carvers = new JsonArray();
            biome.getGenerationSettings().getCarvers().forEach(h ->
                carvers.add(h.unwrapKey().map(k -> k.identifier().toString()).orElse("?")));
            bj.add("carvers", carvers);
            JsonArray steps = new JsonArray();
            for (int step = 0; step < biome.getGenerationSettings().features().size(); step++) {
                JsonArray names = new JsonArray();
                for (Holder<PlacedFeature> h : biome.getGenerationSettings().features().get(step)) {
                    names.add(h.unwrapKey().map(k -> k.identifier().toString()).orElse("?"));
                }
                steps.add(names);
            }
            bj.add("features", steps);
            biomesOut.add(name, bj);
        }
        root.add("biomes", biomesOut);

        // ---- feature closure ---------------------------------------------------
        // placedNames starts from the biomes' direct placed features.
        TreeSet<String> placedNames = new TreeSet<>();
        for (JsonElement bj : biomesOut.asMap().values()) {
            for (JsonElement step : bj.getAsJsonObject().getAsJsonArray("features")) {
                for (JsonElement name : step.getAsJsonArray()) {
                    placedNames.add(name.getAsString());
                }
            }
        }

        TreeSet<String> configuredNames = new TreeSet<>();
        Deque<String> placedWork = new ArrayDeque<>(placedNames);
        Deque<String> configuredWork = new ArrayDeque<>();
        Set<String> placedSeen = new HashSet<>();
        Set<String> configuredSeen = new HashSet<>();

        while (!placedWork.isEmpty() || !configuredWork.isEmpty()) {
            while (!placedWork.isEmpty()) {
                String pname = placedWork.poll();
                if (!placedSeen.add(pname)) {
                    continue;
                }
                placedNames.add(pname);
                PlacedFeature pf = placedReg.getOrThrow(
                    ResourceKey.create(Registries.PLACED_FEATURE, net.minecraft.resources.Identifier.parse(pname))
                ).value();
                String cname = pf.feature().unwrapKey().map(k -> k.identifier().toString()).orElse("?");
                if (!cname.contains("?")) {
                    configuredWork.add(cname);
                }
            }
            while (!configuredWork.isEmpty()) {
                String cname = configuredWork.poll();
                if (!configuredSeen.add(cname)) {
                    continue;
                }
                configuredNames.add(cname);
                ConfiguredFeature<?, ?> cf = configuredReg.getOrThrow(
                    ResourceKey.create(Registries.CONFIGURED_FEATURE, net.minecraft.resources.Identifier.parse(cname))
                ).value();
                DataResult<JsonElement> enc = ConfiguredFeature.DIRECT_CODEC.encodeStart(regOps, cf);
                JsonElement elem = enc.result().orElseThrow(() -> new IllegalStateException(
                    "failed to encode configured feature " + cname + ": " + enc.error().orElseThrow()
                ));
                collectFeatureRefs(elem, "placed", placedWork, placedReg, configuredReg);
                collectFeatureRefs(elem, "configured", configuredWork, placedReg, configuredReg);
            }
        }

        // ---- emit the element tables -------------------------------------------
        JsonObject placedOut = new JsonObject();
        for (String name : placedNames) {
            PlacedFeature pf = placedReg.getOrThrow(
                ResourceKey.create(Registries.PLACED_FEATURE, net.minecraft.resources.Identifier.parse(name))
            ).value();
            DataResult<JsonElement> enc = PlacedFeature.DIRECT_CODEC.encodeStart(regOps, pf);
            JsonElement elem = enc.result().orElseThrow(() -> new IllegalStateException(
                "failed to encode placed feature " + name + ": " + enc.error().orElseThrow()
            ));
            JsonObject entry = new JsonObject();
            entry.addProperty("id", placedReg.getId(pf));
            entry.add("json", elem);
            placedOut.add(name, entry);
        }
        root.add("placed_features", placedOut);

        JsonObject configuredOut = new JsonObject();
        for (String name : configuredNames) {
            ConfiguredFeature<?, ?> cf = configuredReg.getOrThrow(
                ResourceKey.create(Registries.CONFIGURED_FEATURE, net.minecraft.resources.Identifier.parse(name))
            ).value();
            DataResult<JsonElement> enc = ConfiguredFeature.DIRECT_CODEC.encodeStart(regOps, cf);
            JsonElement elem = enc.result().orElseThrow(() -> new IllegalStateException(
                "failed to encode configured feature " + name + ": " + enc.error().orElseThrow()
            ));
            JsonObject entry = new JsonObject();
            entry.addProperty("id", configuredReg.getId(cf));
            entry.add("json", elem);
            configuredOut.add(name, entry);
        }
        root.add("configured_features", configuredOut);

        // ---- probe counts -------------------------------------------------------
        JsonObject probe = new JsonObject();
        probe.addProperty("reachable_biome_count", reachable.size());
        probe.addProperty("placed_feature_count", placedNames.size());
        probe.addProperty("configured_feature_count", configuredNames.size());
        JsonObject perBiome = new JsonObject();
        for (String name : biomesOut.asMap().keySet()) {
            JsonObject bj = biomesOut.getAsJsonObject(name);
            int total = 0;
            JsonArray perStep = new JsonArray();
            for (JsonElement step : bj.getAsJsonArray("features")) {
                total += step.getAsJsonArray().size();
                perStep.add(step.getAsJsonArray().size());
            }
            JsonObject p = new JsonObject();
            p.addProperty("total", total);
            p.add("per_step", perStep);
            perBiome.add(name, p);
        }
        probe.add("per_biome", perBiome);
        root.add("probe", probe);

        String json = new GsonBuilder().setPrettyPrinting().create().toJson(root);
        try (Writer w = new BufferedWriter(
            new OutputStreamWriter(new FileOutputStream(output), StandardCharsets.UTF_8)
        )) {
            w.write(json);
            w.write("\n");
        }
    }

    /**
     * Walk a RegistryOps-encoded JSON element, collecting bare-string values that
     * are registry references to placed/configured features. RegistryOps encodes
     * a holder reference as a bare JSON string; block-state `Name` fields are
     * object fields (never bare refs), so string membership in the target
     * registry disambiguates a bare string. Over-approximation is safe here
     * (extra entries never break decoding); under-approximation is not.
     */
    private static void collectFeatureRefs(
        JsonElement elem,
        String kind,
        Deque<String> worklist,
        Registry<PlacedFeature> placedReg,
        Registry<ConfiguredFeature<?, ?>> configuredReg
    ) {
        if (elem.isJsonPrimitive() && elem.getAsJsonPrimitive().isString()) {
            String s = elem.getAsString();
            if (s.startsWith("minecraft:")) {
                net.minecraft.resources.Identifier id = net.minecraft.resources.Identifier.parse(s);
                if (kind.equals("placed") && placedReg.get(id).isPresent()) {
                    worklist.add(s);
                } else if (kind.equals("configured") && configuredReg.get(id).isPresent()) {
                    worklist.add(s);
                }
            }
        } else if (elem.isJsonArray()) {
            for (JsonElement child : elem.getAsJsonArray()) {
                collectFeatureRefs(child, kind, worklist, placedReg, configuredReg);
            }
        } else if (elem.isJsonObject()) {
            for (JsonElement child : elem.getAsJsonObject().asMap().values()) {
                collectFeatureRefs(child, kind, worklist, placedReg, configuredReg);
            }
        }
    }
}
