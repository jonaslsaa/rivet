import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import java.io.PrintWriter;
import java.nio.file.Path;
import java.util.List;
import java.util.TreeMap;
import java.util.Map;
import net.minecraft.SharedConstants;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
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
import net.minecraft.world.level.levelgen.SurfaceRules;
import net.minecraft.world.level.levelgen.SurfaceRules.ConditionSource;
import net.minecraft.world.level.levelgen.SurfaceRules.RuleSource;
import net.minecraft.data.worldgen.SurfaceRuleData;
import com.mojang.serialization.JsonOps;
import net.minecraft.resources.RegistryOps;

/**
 * Captures the pinned Paper 26.2 `SurfaceRuleData` static surface trees under
 * `RuleSource.CODEC` / `ConditionSource.CODEC` (the `MATERIAL_RULE` /
 * `MATERIAL_CONDITION` by-name dispatches) as golden JSON, plus structural
 * occurrence-count stats and the referenced biome holder list.
 *
 * This is the byte-exactness oracle for the merged surface-rules codec port:
 * the Rust `surface_rule_data_golden.rs` integration test re-encodes the nether
 * tree with `rule_source_codec`/`condition_source_codec` under `RegistryOps`
 * and asserts the re-encoded bytes equal the canonical bytes here (both
 * sides normalized by serde_json, so the `1.7976931348623157E308` Java
 * exponent casing is a shared, pinned deviation).
 *
 * The probe boots the vanilla registries exactly like ComposedNoiseProbe (no
 * server boot) so the `BIOME` registry is populated for
 * `RegistryCodecs.homogeneousList(Registries.BIOME)` holder-set encoding — the
 * `SurfaceRuleData.*` builders take a `HolderGetter<Biome>`.
 *
 * Emits `surface-rule-data.json`:
 *   - `presets`: for each of `nether`, `overworld`, `overworldLike` (both
 *     flag combos), `end`, `air`, the canonical `RuleSource.CODEC` JSON under
 *     `RegistryOps`.
 *   - `node-types`: occurrence-count stats over all dispatch `"type"`
 *     discriminators (both condition and rule arms, unclassified — the Rust
 *     test classifies each against the known condition/rule key sets).
 *   - `blocks`: block names a `block` rule carries.
 *   - `biomes`: the referenced biome holder identifiers (canonical order).
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" SurfaceRuleDataProbe --output dir/ [--paper pin]
 */
public final class SurfaceRuleDataProbe {
    private SurfaceRuleDataProbe() {}

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
            throw new IllegalArgumentException("Usage: SurfaceRuleDataProbe --output <dir> [--paper <pin>]");
        }

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        PackRepository packRepository = ServerPacksSource.createVanillaTrustedRepository();
        MinecraftServer.configurePackRepository(
            packRepository,
            new net.minecraft.world.level.WorldDataConfiguration(
                new net.minecraft.world.level.DataPackConfig(
                    List.of("minecraft", "vanilla"), List.of()
                ),
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
        HolderGetter<Biome> biomes = registryAccess.lookupOrThrow(Registries.BIOME);

        RegistryOps<JsonElement> ops = RegistryOps.create(JsonOps.INSTANCE, registryAccess);

        JsonObject root = new JsonObject();
        root.addProperty("paper", paper);
        root.addProperty("format", 1);

        JsonArray presets = new JsonArray();
        addPreset(presets, ops, "nether", SurfaceRuleData.nether(biomes));
        addPreset(presets, ops, "overworld", SurfaceRuleData.overworld(biomes));
        addPreset(presets, ops, "overworld_like_true_false_true", SurfaceRuleData.overworldLike(biomes, true, false, true));
        addPreset(presets, ops, "overworld_like_false_false_true", SurfaceRuleData.overworldLike(biomes, false, false, true));
        addPreset(presets, ops, "end", SurfaceRuleData.end());
        addPreset(presets, ops, "air", SurfaceRuleData.air());
        root.add("presets", presets);

        // Structural occurrence-count stats over every preset: the dispatch
        // `"type"` discriminators (conditions and rules unclassified), the
        // block names a `block` rule carries, and the biome holder ids.
        TreeMap<String, Integer> nodeTypes = new TreeMap<>();
        TreeMap<String, Integer> blocks = new TreeMap<>();
        TreeMap<String, Integer> biomesUsed = new TreeMap<>();
        for (JsonElement preset : presets) {
            JsonObject entry = preset.getAsJsonObject();
            countStruct(nodeTypes, blocks, biomesUsed, entry.getAsJsonObject("json"));
        }
        root.add("node-types", toSorted(nodeTypes));
        root.add("blocks", toSorted(blocks));
        root.add("biomes", toSorted(biomesUsed));

        Path outDir = Path.of(output);
        java.nio.file.Files.createDirectories(outDir);
        try (PrintWriter writer = new PrintWriter(outDir.resolve("surface-rule-data.json").toFile(), "UTF-8")) {
            writer.println(new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(root));
        }
        System.out.println(
            "captured " + presets.size() + " surface-rule presets ("
                + nodeTypes.size() + " dispatch types) to "
                + outDir.resolve("surface-rule-data.json")
        );
    }

    private static void addPreset(
        JsonArray presets, RegistryOps<JsonElement> ops, String name, RuleSource rule
    ) {
        JsonElement encoded = SurfaceRules.RuleSource.CODEC
            .encodeStart(ops, rule)
            .result().orElseThrow(() -> new IllegalStateException("encode failed for " + name));
        JsonObject entry = new JsonObject();
        entry.addProperty("name", name);
        entry.add("json", encoded);
        presets.add(entry);
    }

    /// Recursively walk a decoded rule/condition element, tallying dispatch
    /// types by `"type"`, `block` rule names by `result_state.Name`, and biome
    /// holders by the `biome_is` field (compact bare id or id list).
    private static void countStruct(
        TreeMap<String, Integer> nodeTypes,
        TreeMap<String, Integer> blocks,
        TreeMap<String, Integer> biomes,
        JsonElement e
    ) {
        if (e.isJsonObject()) {
            JsonObject o = e.getAsJsonObject();
            if (o.has("type")) {
                String type = o.get("type").getAsString();
                bump(nodeTypes, type);
                if (type.equals("minecraft:block") && o.has("result_state")) {
                    JsonObject rs = o.getAsJsonObject("result_state");
                    if (rs.has("Name")) {
                        bump(blocks, rs.get("Name").getAsString());
                    }
                }
                if (o.has("biome_is")) {
                    JsonElement b = o.get("biome_is");
                    if (b.isJsonPrimitive()) {
                        bump(biomes, b.getAsString());
                    } else if (b.isJsonArray()) {
                        for (JsonElement el : b.getAsJsonArray()) {
                            if (el.isJsonPrimitive()) {
                                bump(biomes, el.getAsString());
                            }
                        }
                    }
                }
            }
            for (Map.Entry<String, JsonElement> kv : o.entrySet()) {
                if (kv.getKey().equals("type")) {
                    continue;
                }
                countStruct(nodeTypes, blocks, biomes, kv.getValue());
            }
        } else if (e.isJsonArray()) {
            for (JsonElement el : e.getAsJsonArray()) {
                countStruct(nodeTypes, blocks, biomes, el);
            }
        }
    }

    private static void bump(TreeMap<String, Integer> m, String key) {
        m.merge(key, 1, Integer::sum);
    }

    /// Emit a stat map as a JSON object of `key -> count` (TreeMap = sorted).
    private static JsonObject toSorted(TreeMap<String, Integer> m) {
        JsonObject o = new JsonObject();
        for (Map.Entry<String, Integer> kv : m.entrySet()) {
            o.addProperty(kv.getKey(), kv.getValue());
        }
        return o;
    }
}
