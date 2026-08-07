//! BiomeTagExtractor — dumps the deterministic MC 26.2 biome id table and the
//! tag network-serialization content from a live Paper registry load (issue #49).
//!
//! The biome registry is datapack-loaded (not in BuiltInRegistries), so its ids
//! are assigned at runtime by `ResourceManagerRegistryLoadTask`: elements are
//! registered from a `TreeMap<Identifier, Resource>` sorted by `Identifier`
//! compareTo (path first, then namespace) — i.e. id 0 = `minecraft:badlands`,
//! alphabetical. The tag content is exactly what
//! `TagNetworkSerialization.serializeTagsToNetwork` produces for the
//! `ClientboundUpdateTagsPacket`: every `networkSafeRegistries` registry
//! (WORLDGEN networkable + STATIC) that carries at least one bound tag, mapped
//! to tag-location -> element ids in the tag JSON file's value order.
//!
//! The load sequence below mirrors `WorldLoader.load`: vanilla pack source ->
//! STATIC layer -> TagLoader.loadTagsForExistingRegistries (static tags) ->
//! buildUpdatedLookups -> RegistryDataLoader.load(WORLDGEN_REGISTRIES) ->
//! replaceFrom(WORLDGEN) -> static PendingTags.apply -> serializeTagsToNetwork.
//!
//! Output JSON (written to --output) is a single object with:
//!   generator / minecraft_version / protocol_version / world_version
//!   biomes      : name -> id (dense 0..n, byId order)
//!   registries  : per tag-carrying registry key:
//!       elements  : name -> id (dense 0..n, byId order)
//!       tags      : tag location -> [element names in tag file value order]
//! Element tables are included for *every* tag-carrying registry so the codegen
//! can cross-check the 7 report-backed surfaces (block, item, entity_type,
//! fluid, game_event, potion, point_of_interest_type) against the existing
//! generated tables and validate every tag element against a known surface.
//!
//! Determinism: registry keys, tag locations, and (implicitly) byId element
//! order are all ordered, so two independent runs are byte-identical. The tag
//! element *id list order* is preserved (it is the wire order — never sorted).

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import it.unimi.dsi.fastutil.ints.IntList;
import java.io.BufferedWriter;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.Writer;
import java.lang.reflect.Field;
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
import net.minecraft.tags.TagNetworkSerialization;
import net.minecraft.world.level.biome.Biome;

public final class BiomeTagExtractor {
    private BiomeTagExtractor() {}

    private static String argValue(String[] args, String name) {
        for (int i = 0; i + 1 < args.length; i++) {
            if (args[i].equals(name)) {
                return args[i + 1];
            }
        }
        return null;
    }

    /** The private `NetworkPayload.tags` map (tag location -> element ids). */
    @SuppressWarnings("unchecked")
    private static Map<Identifier, IntList> tagsOf(TagNetworkSerialization.NetworkPayload p) throws Exception {
        Field f = TagNetworkSerialization.NetworkPayload.class.getDeclaredField("tags");
        f.setAccessible(true);
        return (Map<Identifier, IntList>) f.get(p);
    }

    /** Dense name -> id element table in byId order (listElements is byId). */
    private static <T> JsonObject elementTable(Registry<T> registry) {
        JsonObject elements = new JsonObject();
        registry.listElements().forEach(h -> {
            T value = h.value();
            Identifier key = registry.getKey(value);
            if (key == null) {
                throw new IllegalStateException("null key for element id " + registry.getId(value));
            }
            elements.addProperty(key.toString(), registry.getId(value));
        });
        return elements;
    }

    /** Tag location -> element names, preserving the tag file's value order. */
    private static <T> JsonObject tagTable(Registry<T> registry, Map<Identifier, IntList> payload) {
        JsonObject tags = new JsonObject();
        List<Map.Entry<Identifier, IntList>> sorted = new ArrayList<>(payload.entrySet());
        sorted.sort(Comparator.comparing(e -> e.getKey().toString()));
        for (Map.Entry<Identifier, IntList> entry : sorted) {
            JsonArray names = new JsonArray();
            for (int i = 0; i < entry.getValue().size(); i++) {
                int id = entry.getValue().getInt(i);
                T value = registry.get(id).map(Holder.Reference::value).orElse(null);
                Identifier name = value == null ? null : registry.getKey(value);
                if (name == null) {
                    throw new IllegalStateException(
                        "tag " + entry.getKey() + " references unregistered element id " + id
                    );
                }
                names.add(name.toString());
            }
            tags.add(entry.getKey().toString(), names);
        }
        return tags;
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
        root.addProperty("generator", "BiomeTagExtractor (Bootstrap + RegistryDataLoader + TagNetworkSerialization)");
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
            LayeredRegistryAccess<RegistryLayer> resourcesLoadContext = initialLayers.replaceFrom(
                RegistryLayer.WORLDGEN, worldgen, RegistryAccess.EMPTY
            );
            staticLayerTags.forEach(Registry.PendingTags::apply);

            Registry<Biome> biomes = worldgen.lookupOrThrow(Registries.BIOME);
            root.add("biomes", elementTable(biomes));

            Map<ResourceKey<? extends Registry<?>>, TagNetworkSerialization.NetworkPayload> netTags =
                TagNetworkSerialization.serializeTagsToNetwork(resourcesLoadContext);
            RegistryAccess.Frozen composite = resourcesLoadContext.compositeAccess();

            JsonObject registries = new JsonObject();
            List<Map.Entry<ResourceKey<? extends Registry<?>>, TagNetworkSerialization.NetworkPayload>> sorted =
                new ArrayList<>(netTags.entrySet());
            sorted.sort(Comparator.comparing(e -> e.getKey().identifier().toString()));

            int totalTags = 0;
            for (Map.Entry<ResourceKey<? extends Registry<?>>, TagNetworkSerialization.NetworkPayload> entry : sorted) {
                ResourceKey<? extends Registry<?>> registryKey = entry.getKey();
                Registry<?> registry = composite.lookup(registryKey).orElseThrow(
                    () -> new IllegalStateException("tag registry " + registryKey + " not in the layered access")
                );
                JsonObject registryOut = new JsonObject();
                registryOut.add("elements", elementTable(registry));
                Map<Identifier, IntList> payload = tagsOf(entry.getValue());
                registryOut.add("tags", tagTable(registry, payload));
                registries.add(registryKey.identifier().toString(), registryOut);
                totalTags += payload.size();
            }
            root.add("registries", registries);

            // Bootstrap.wrapStreams() replaces System.out with a logger-routed
            // stream, so the probe counts travel inside the fixture JSON (which
            // the extractor writes deterministically) rather than on stdout.
            // The live probe re-runs this extractor and asserts the counts.
            JsonObject probe = new JsonObject();
            probe.addProperty("biome_count", root.getAsJsonObject("biomes").size());
            probe.addProperty("tag_registry_count", registries.size());
            probe.addProperty("tag_count", totalTags);
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
