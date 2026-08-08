//! ItemRecipeExtractor — dumps the deterministic MC 26.2 item metadata and the
//! recipe table from a live Paper load (issue #186).
//!
//! Items: `BuiltInRegistries.ITEM` is a static builtin registry, so its
//! registration order (id 0..n) and ids come straight from the real registry.
//! Two per-item facts are NOT derivable from the RegistryDumpReport and are the
//! reason this half of the extraction exists:
//!
//! - `getDefaultMaxStackSize()` — `components().getOrDefault(MAX_STACK_SIZE, 1)`,
//!   where the bound components come from the holder components the Item
//!   constructor registers into `BuiltInRegistries.DATA_COMPONENT_INITIALIZERS`.
//!   Those are NOT bound by `VanillaRegistries.createLookup()` (that only runs
//!   `RegistrySetBuilder.build`); the binding happens in
//!   `ReloadableServerResources`/`RegistryComponentsReport` via
//!   `DATA_COMPONENT_INITIALIZERS.build(registries).forEach(PendingComponents::apply)`,
//!   which the helper reproduces so `getDefaultMaxStackSize()` returns the
//!   effective value (64 from `COMMON_ITEM_COMPONENTS` unless overridden).
//! - `requiredFeatures()` — the `FeatureFlagSet` each item requires, rendered as
//!   sorted `FeatureFlags.REGISTRY.toNames(...)` identifiers (e.g. vanilla
//!   `minecraft:vanilla`, experimental `minecraft:redstone_experiments`).
//!
//! The `crafting_remaining_item` (bucket -> empty bucket etc.) is dumped too, as
//! the item id of `Item.getCraftingRemainder()`.
//!
//! Recipes: the canonical source is the vanilla datapack embedded in the server
//! jar at `data/minecraft/recipe/*.json`. Rather than byte-copying it, the
//! helper loads it through the exact path `RecipeManager.prepare` uses —
//! `FileToIdConverter.registry(Registries.RECIPE)` + `scanDirectory` with
//! `Recipe.CODEC` (the serializer-by-name dispatch) into a `TreeMap<Identifier,
//! Recipe<?>>` sorted by `Identifier` compareTo — then re-encodes each recipe
//! with the same `Recipe.CODEC` and writes it in `DataProvider.saveStable`
//! canonical form (`GsonHelper.writeValue` with the `type`/`parent`-first
//! `KEY_COMPARATOR`). This proves every recipe parses and pins the canonical
//! re-encoded form, not a raw file copy: a recipe that silently failed
//! `scanDirectory` would make the parsed count fall short of the file count and
//! the helper throws.
//!
//! Before the recipe scan the helper applies the vanilla static tags, because
//! `Ingredient` references (`#minecraft:planks` etc.) resolve through the item
//! registry's bound tags — `RecipeManager` only runs after
//! `MinecraftServer` has loaded them (`TagLoader.loadTagsForExistingRegistries`
//! on the STATIC layer, committed by `Registry.PendingTags::apply` in
//! `ReloadableServerResources.updateComponentsAndStaticRegistryTags`). The
//! helper reproduces that step so tag-bearing recipes parse.
//!
//! Output JSON (written to --output) is a single object, written with the same
//! `GsonHelper.writeValue` + `DataProvider.KEY_COMPARATOR` used by `saveStable`:
//!   generator / minecraft_version / protocol_version / world_version
//!   items    : item name -> { id, max_stack_size, feature_flags[], and
//!              crafting_remaining_item when present } (keys alphabetically
//!              sorted like the RegistryDumpReport; id recovers registration order)
//!   recipes  : recipe name -> canonical re-encoded Recipe.CODEC JSON
//!   probe    : item_count / recipe_count / recipe_file_count (live-load anchors)
//!
//! Determinism: item keys are sorted by the canonical writer, recipe iteration
//! is a TreeMap ordered by Identifier compareTo, and feature flags are sorted,
//! so two independent runs are byte-identical.

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.stream.JsonWriter;
import com.mojang.serialization.DynamicOps;
import com.mojang.serialization.JsonOps;
import java.io.BufferedWriter;
import java.io.ByteArrayOutputStream;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.SortedMap;
import java.util.TreeMap;
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.component.DataComponentInitializers;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.DataProvider;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.resources.FileToIdConverter;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.VanillaPackResources;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.packs.resources.SimpleJsonResourceReloadListener;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.GsonHelper;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStackTemplate;
import net.minecraft.world.item.crafting.Recipe;
import net.minecraft.world.flag.FeatureFlags;

public final class ItemRecipeExtractor {
    private ItemRecipeExtractor() {}

    private static String argValue(String[] args, String name) {
        for (int i = 0; i + 1 < args.length; i++) {
            if (args[i].equals(name)) {
                return args[i + 1];
            }
        }
        return null;
    }

    /** Dump one item's metadata into `meta`, keyed by its registered name. */
    private static void itemEntry(JsonObject items, Registry<Item> itemRegistry, Holder.Reference<Item> holder) {
        Item item = holder.value();
        Identifier key = itemRegistry.getKey(item);
        if (key == null) {
            throw new IllegalStateException("null key for item id " + itemRegistry.getId(item));
        }
        JsonObject meta = new JsonObject();
        meta.addProperty("id", itemRegistry.getId(item));
        meta.addProperty("max_stack_size", item.getDefaultMaxStackSize());

        JsonArray flags = new JsonArray();
        List<String> sortedFlags = new ArrayList<>();
        FeatureFlags.REGISTRY.toNames(item.requiredFeatures()).forEach(id -> sortedFlags.add(id.toString()));
        sortedFlags.sort(String::compareTo);
        sortedFlags.forEach(flags::add);
        meta.add("feature_flags", flags);

        ItemStackTemplate remainder = item.getCraftingRemainder();
        if (remainder != null) {
            Identifier remainderKey = itemRegistry.getKey(remainder.item().value());
            if (remainderKey == null) {
                throw new IllegalStateException("crafting remainder not registered for " + key);
            }
            meta.addProperty("crafting_remaining_item", remainderKey.toString());
        }
        items.add(key.toString(), meta);
    }

    /** Canonical saveStable-style writer: the same bytes `DataProvider.saveStable` produces. */
    private static byte[] canonicalBytes(JsonElement root) throws Exception {
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        try (JsonWriter writer = new JsonWriter(new OutputStreamWriter(bytes, StandardCharsets.UTF_8))) {
            writer.setSerializeNulls(false);
            writer.setIndent("  ");
            GsonHelper.writeValue(writer, root, DataProvider.KEY_COMPARATOR);
        }
        bytes.write('\n');
        return bytes.toByteArray();
    }

    public static void main(String[] args) {
        // Bootstrap.bootStrap() replaces System.err with a logger-routed stream,
        // so an uncaught trace would vanish into the (suppressed) logger. Print
        // failures to the real pre-bootstrap stderr so the calling process sees
        // them on the captured pipe.
        java.io.PrintStream realErr = System.err;
        try {
            mainImpl(args);
        } catch (Throwable t) {
            t.printStackTrace(realErr);
            System.exit(1);
        }
    }

    private static void mainImpl(String[] args) throws Exception {
        String output = argValue(args, "--output");
        String mcVersion = argValue(args, "--version");
        if (output == null) {
            throw new IllegalArgumentException("--output <path> is required");
        }

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        VanillaPackResources vanilla = ServerPacksSource.createVanillaPackSource();
        try (MultiPackResourceManager resources = new MultiPackResourceManager(PackType.SERVER_DATA, List.<PackResources>of(vanilla))) {
            // Load + commit the vanilla static tags onto the static registries
            // (the same step `MinecraftServer.reloadResources` performs before
            // `RecipeManager.prepare` runs), so `Ingredient` tag references in
            // recipes resolve. See the module docs.
            LayeredRegistryAccess<RegistryLayer> layers = RegistryLayer.createRegistryAccess();
            List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, layers.getLayer(RegistryLayer.STATIC));
            staticTags.forEach(Registry.PendingTags::apply);

            // The lookup `ReloadableServerResources`/`RegistryComponentsReport`
            // bind holder components against. `createLookup()` alone only runs
            // `RegistrySetBuilder.build`; the DATA_COMPONENT_INITIALIZERS build+
            // apply step below is what binds each item's holder components so
            // `getDefaultMaxStackSize()` returns the effective value.
            HolderLookup.Provider registries = VanillaRegistries.createLookup();
            BuiltInRegistries.DATA_COMPONENT_INITIALIZERS
                .build(registries)
                .forEach(DataComponentInitializers.PendingComponents::apply);

            JsonObject root = new JsonObject();
            root.addProperty("generator", "ItemRecipeExtractor (Bootstrap + static tags + VanillaRegistries.createLookup + DATA_COMPONENT_INITIALIZERS + Recipe.CODEC)");
            root.addProperty("minecraft_version", mcVersion);
            root.addProperty("protocol_version", SharedConstants.getProtocolVersion());
            root.addProperty("world_version", SharedConstants.WORLD_VERSION);

            Registry<Item> itemRegistry = BuiltInRegistries.ITEM;
            JsonObject items = new JsonObject();
            itemRegistry.listElements().forEach(holder -> itemEntry(items, itemRegistry, holder));
            root.add("items", items);

            DynamicOps<JsonElement> ops = registries.createSerializationContext(JsonOps.INSTANCE);
            FileToIdConverter lister = FileToIdConverter.registry(Registries.RECIPE);
            int recipeFileCount;
            SortedMap<Identifier, Recipe<?>> recipes = new TreeMap<>();
            recipeFileCount = lister.listMatchingResources(resources).size();
            SimpleJsonResourceReloadListener.scanDirectory(resources, lister, ops, Recipe.CODEC, recipes);

            if (recipes.size() != recipeFileCount) {
                throw new IllegalStateException(
                    "recipe parse incomplete: " + recipes.size() + "/" + recipeFileCount + " parsed by Recipe.CODEC"
                );
            }
            JsonObject recipeOut = new JsonObject();
            for (Map.Entry<Identifier, Recipe<?>> entry : recipes.entrySet()) {
                JsonElement reencoded = Recipe.CODEC
                    .encodeStart(ops, entry.getValue())
                    .getOrThrow(err -> new IllegalStateException("re-encode " + entry.getKey() + ": " + err));
                recipeOut.add(entry.getKey().toString(), reencoded);
            }
            root.add("recipes", recipeOut);

            // The probe counts travel inside the fixture JSON (which the extractor
            // writes deterministically) because Bootstrap.wrapStreams() routes
            // System.out into the logger; the live probe re-runs this extractor
            // and asserts them.
            JsonObject probe = new JsonObject();
            probe.addProperty("item_count", items.size());
            probe.addProperty("recipe_count", recipes.size());
            probe.addProperty("recipe_file_count", recipeFileCount);
            root.add("probe", probe);

            try (Writer w = new BufferedWriter(
                new OutputStreamWriter(new FileOutputStream(output), StandardCharsets.UTF_8)
            )) {
                w.write(new String(canonicalBytes(root), StandardCharsets.UTF_8));
            }
        }
    }
}
