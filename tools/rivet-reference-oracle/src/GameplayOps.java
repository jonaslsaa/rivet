package dev.rivet.oracle;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.mojang.serialization.Codec;
import com.mojang.serialization.DataResult;
import dev.rivet.oracle.RivetReferenceOracle.Op;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashSet;
import java.util.LinkedHashSet;
import java.util.stream.Collectors;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.Tag;
import net.minecraft.nbt.TagParser;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryOps;
import net.minecraft.resources.ResourceKey;
import net.minecraft.tags.EnchantmentTags;
import net.minecraft.util.RandomSource;
import net.minecraft.world.effect.MobEffect;
import net.minecraft.world.entity.ai.attributes.AttributeModifier;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.crafting.CraftingInput;
import net.minecraft.world.item.crafting.CraftingRecipe;
import net.minecraft.world.item.crafting.Recipe;
import net.minecraft.world.item.crafting.RecipeHolder;
import net.minecraft.world.item.crafting.RecipeInput;
import net.minecraft.world.item.crafting.SingleItemRecipe;
import net.minecraft.world.item.crafting.SingleRecipeInput;
import net.minecraft.world.item.crafting.SmithingRecipe;
import net.minecraft.world.item.crafting.SmithingRecipeInput;
import net.minecraft.world.item.crafting.AbstractCookingRecipe;
import net.minecraft.world.item.enchantment.Enchantable;
import net.minecraft.world.item.enchantment.Enchantment;
import net.minecraft.world.item.enchantment.EnchantmentHelper;
import net.minecraft.world.item.enchantment.EnchantmentInstance;
import net.minecraft.core.component.DataComponents;
import net.minecraft.world.scores.ScoreboardSaveData;

/**
 * The gameplay-data oracle ops (VERIFICATION section 5 item 8): {@code recipe.match},
 * {@code loot.roll}, {@code enchant.select}, {@code effect}, {@code trade.offers} and
 * {@code scoreboard.saved_data}. Each op runs real Paper classes over the vanilla
 * data pack the way the server loads it, and answers with canonical JSON the Rust
 * handlers must reproduce. Registered from {@code RivetReferenceOracle.registry()};
 * the helpers it shares (op record, request parsing, the frozen server registries)
 * are package-private there.
 */
final class GameplayOps {
    private GameplayOps() {
    }

    /**
     * The gameplay handlers load and run vanilla code that logs ("Unknown objective ...", loot and
     * trade warnings) to the process's stdout, the stream live mode speaks JSON Lines on. The first
     * request switches the root logger off for good; the answers never depend on a log line.
     */
    private static RivetReferenceOracle.Handler quiet(final RivetReferenceOracle.Handler handler) {
        return (request, dec) -> {
            if (org.apache.logging.log4j.LogManager.getRootLogger().getLevel() != org.apache.logging.log4j.Level.OFF) {
                org.apache.logging.log4j.core.config.Configurator.setRootLevel(org.apache.logging.log4j.Level.OFF);
            }
            return handler.run(request, dec);
        };
    }

    static void register(final Map<String, Op> ops) {
        RivetReferenceOracle.register(ops, new Op(
            "effect",
            List.of("category", "color", "modifiers"),
            quiet((request, dec) -> effect(request, dec)),
            error -> false,
            GameplayOps::effectCheck
        ));
        RivetReferenceOracle.register(ops, new Op(
            "scoreboard.saved_data",
            List.of("accept", "phase", "codec", "loaded"),
            quiet((request, dec) -> scoreboardSavedData(request, dec)),
            error -> error instanceof com.mojang.brigadier.exceptions.CommandSyntaxException
        ));
        RivetReferenceOracle.register(ops, new Op(
            "enchant.select",
            List.of("costs", "enchantments", "slot_cost"),
            quiet((request, dec) -> enchantSelect(request, dec)),
            error -> false,
            GameplayOps::enchantCheck
        ));
        RivetReferenceOracle.register(ops, new Op(
            "loot.roll",
            List.of("items", "threw"),
            quiet((request, dec) -> lootRoll(request, dec)),
            error -> false,
            GameplayOps::lootCheck
        ));
        RivetReferenceOracle.register(ops, new Op(
            "trade.offers",
            List.of("offers", "threw"),
            quiet((request, dec) -> tradeOffers(request, dec)),
            error -> false,
            GameplayOps::tradeCheck
        ));
        RivetReferenceOracle.register(ops, new Op(
            "recipe.match",
            List.of("match", "result", "threw"),
            quiet((request, dec) -> recipeMatch(request, dec)),
            error -> false,
            GameplayOps::recipeCheck
        ));
    }


    // ---- corpus closure (record time) --------------------------------------------------------

    /** The seeds per sweep cell of every seeded gameplay op. */
    static final int SEEDS = 16;

    /**
     * Require that {@code requests} (those of {@code kind}, when {@code kind} is non-null) are exactly the
     * cross product of the distinct values they carry per dimension, each cell once, and return those values.
     */
    static List<Set<String>> requireCrossProduct(
        final String what, final List<JsonObject> requests, final String kind, final String... dimensions
    ) {
        List<JsonObject> cells = kind == null ? requests
            : requests.stream().filter(r -> kind.equals(r.get("kind").getAsString())).toList();
        List<Set<String>> values = new ArrayList<>();
        for (String dimension : dimensions) {
            Set<String> distinct = new LinkedHashSet<>();
            cells.forEach(cell -> distinct.add(cell.get(dimension).toString()));
            values.add(distinct);
        }
        long expected = 1;
        for (Set<String> distinct : values) {
            expected *= distinct.size();
        }
        Set<String> seen = new HashSet<>();
        for (JsonObject cell : cells) {
            String key = java.util.Arrays.stream(dimensions).map(d -> cell.get(d).toString()).collect(Collectors.joining("|"));
            if (!seen.add(key)) {
                throw new IllegalStateException(what + ": duplicate cell " + key);
            }
        }
        if (cells.isEmpty() || seen.size() != expected) {
            throw new IllegalStateException(what + ": corpus is " + seen.size() + " cells, the cross product of "
                + String.join("/", dimensions) + " is " + expected);
        }
        return values;
    }

    private static void requireSame(final String what, final Set<String> corpus, final Set<String> registry) {
        if (!corpus.equals(registry)) {
            Set<String> missing = new TreeSet<>(registry);
            missing.removeAll(corpus);
            Set<String> extra = new TreeSet<>(corpus);
            extra.removeAll(registry);
            throw new IllegalStateException(what + ": corpus does not close over the registry; missing " + missing + ", unknown " + extra);
        }
    }

    private static Set<String> quoted(final Iterable<Identifier> ids) {
        Set<String> out = new HashSet<>();
        ids.forEach(id -> out.add("\"" + id + "\""));
        return out;
    }

    /** {@code effect}: every registered mob effect x one shared amplifier list. */
    static void effectCheck(final List<JsonObject> requests) {
        var values = requireCrossProduct("effect", requests, null, "effect", "amplifier");
        requireSame("effect", values.get(0), quoted(BuiltInRegistries.MOB_EFFECT.keySet()));
    }

    /** {@code enchant.select}: every enchantment x levels; items x slots x bookcases x seeds; items x powers x seeds. */
    static void enchantCheck(final List<JsonObject> requests) {
        var cost = requireCrossProduct("enchant.select cost", requests, "cost", "enchantment", "level");
        requireSame("enchant.select cost", cost.get(0), quoted(
            RivetReferenceOracle.ServerRegistries.ACCESS.lookupOrThrow(Registries.ENCHANTMENT).listElementIds()
                .map(key -> key.identifier()).toList()));
        var slot = requireCrossProduct("enchant.select slot_cost", requests, "slot_cost", "item", "slot", "bookcases", "seed");
        var select = requireCrossProduct("enchant.select select", requests, "select", "item", "power", "seed");
        if (slot.get(3).size() != SEEDS || select.get(2).size() != SEEDS) {
            throw new IllegalStateException("enchant.select: every cell needs exactly " + SEEDS + " seeds");
        }
    }

    /** {@code recipe.match}: every recipe of the loaded data pack appears, and nothing else. */
    static void recipeCheck(final List<JsonObject> requests) {
        Set<String> corpus = new HashSet<>();
        requests.forEach(r -> corpus.add(r.get("recipe").toString()));
        requireSame("recipe.match", corpus, quoted(
            GameplayData.recipes().values().stream().map(holder -> holder.id().identifier()).toList()));
    }

    // ---- request helpers -----------------------------------------------------

    static int requiredInt(final JsonObject request, final String property) {
        JsonElement value = request.get(property);
        if (value == null || !value.isJsonPrimitive() || !value.getAsJsonPrimitive().isNumber()) {
            throw new IllegalArgumentException("property '" + property + "' must be an integer");
        }
        return value.getAsInt();
    }

    static long requiredLong(final JsonObject request, final String property) {
        JsonElement value = request.get(property);
        if (value == null || !value.isJsonPrimitive() || !value.getAsJsonPrimitive().isNumber()) {
            throw new IllegalArgumentException("property '" + property + "' must be an integer");
        }
        return value.getAsLong();
    }

    static Identifier requiredId(final JsonObject request, final String property) {
        return Identifier.parse(RivetReferenceOracle.requiredString(request, property));
    }

    // ---- effect ----------------------------------------------------------------

    /**
     * {@code effect {effect, amplifier}}: {@code MobEffect.createModifiers(amplifier)} in
     * the order Paper's attribute map yields them, plus the category and colour.
     */
    private static JsonObject effect(final JsonObject request, final JsonObject dec) {
        Identifier id = requiredId(request, "effect");
        int amplifier = requiredInt(request, "amplifier");
        MobEffect effect = BuiltInRegistries.MOB_EFFECT.getOptional(id)
            .orElseThrow(() -> new IllegalArgumentException("unknown effect " + id));
        JsonArray modifiers = new JsonArray();
        int[] index = {0};
        effect.createModifiers(amplifier, (attribute, modifier) -> {
            JsonObject entry = new JsonObject();
            entry.addProperty("attribute", attribute.unwrapKey().orElseThrow().identifier().toString());
            entry.addProperty("id", modifier.id().toString());
            entry.addProperty("amount", RivetReferenceOracle.f64Bits(modifier.amount()));
            dec.addProperty("modifiers." + index[0]++ + ".amount", Double.toString(modifier.amount()));
            entry.addProperty("operation", modifier.operation().getSerializedName());
            modifiers.add(entry);
        });
        JsonObject result = new JsonObject();
        result.addProperty("category", effect.getCategory().name().toLowerCase(Locale.ROOT));
        result.addProperty("color", effect.getColor());
        result.add("modifiers", modifiers);
        return result;
    }

    // ---- scoreboard.saved_data -----------------------------------------------------

    /** A plain scoreboard with {@code ServerScoreboard}'s own load and store (no server needed). */
    private static final class ScoreboardShell extends net.minecraft.world.scores.Scoreboard {
        void load(final ScoreboardSaveData.Packed data) {
            data.objectives().forEach(this::loadObjective);
            data.scores().forEach(this::loadPlayerScore);
            data.displaySlots().forEach((slot, name) -> this.setDisplayObjective(slot, this.getObjective(name)));
            data.teams().forEach(this::loadPlayerTeam);
        }

        ScoreboardSaveData.Packed store() {
            return new ScoreboardSaveData.Packed(
                this.packObjectives(), this.packPlayerScores(), this.packDisplaySlots(), this.packPlayerTeams()
            );
        }
    }

    /**
     * {@code scoreboard.saved_data {input}}: decode the SNBT saved-data compound with
     * {@code ScoreboardSaveData.TYPE.codec()} under NbtOps, then answer the codec's own
     * re-encoding ({@code codec}) and the encoding after loading it into a scoreboard and
     * storing it back ({@code loaded}). A decline names its phase: {@code decode},
     * {@code encode} or {@code load} (a scoreboard rejecting the data, e.g. a duplicate
     * objective name).
     */
    private static JsonObject scoreboardSavedData(final JsonObject request, final JsonObject dec) throws Exception {
        Tag input = TagParser.create(NbtOps.INSTANCE).parseFully(RivetReferenceOracle.requiredString(request, "input"));
        Codec<ScoreboardSaveData> codec = ScoreboardSaveData.TYPE.codec();
        RegistryOps<Tag> ops = RegistryOps.create(NbtOps.INSTANCE, RivetReferenceOracle.ServerRegistries.ACCESS);
        JsonObject result = new JsonObject();
        ScoreboardSaveData saved;
        try {
            DataResult<ScoreboardSaveData> decoded = codec.parse(ops, input);
            if (decoded.result().isEmpty()) {
                dec.addProperty("error", decoded.error().map(DataResult.Error::message).orElse(""));
                return decline(result, "decode");
            }
            saved = decoded.result().get();
        } catch (RuntimeException error) {
            dec.addProperty("thrown", error.toString());
            return decline(result, "decode");
        }
        String direct;
        try {
            DataResult<Tag> encoded = codec.encodeStart(ops, saved);
            if (encoded.result().isEmpty()) {
                dec.addProperty("error", encoded.error().map(DataResult.Error::message).orElse(""));
                return decline(result, "encode");
            }
            direct = canonicalSnbt(encoded.result().get());
        } catch (RuntimeException error) {
            dec.addProperty("thrown", error.toString());
            return decline(result, "encode");
        }
        String loaded;
        try {
            ScoreboardShell scoreboard = new ScoreboardShell();
            scoreboard.load(saved.getData());
            DataResult<Tag> encoded = codec.encodeStart(ops, new ScoreboardSaveData(scoreboard.store()));
            if (encoded.result().isEmpty()) {
                dec.addProperty("error", encoded.error().map(DataResult.Error::message).orElse(""));
                return decline(result, "encode");
            }
            loaded = canonicalSnbt(encoded.result().get());
        } catch (RuntimeException error) {
            dec.addProperty("thrown", error.toString());
            return decline(result, "load");
        }
        result.addProperty("accept", true);
        result.addProperty("codec", direct);
        result.addProperty("loaded", loaded);
        return result;
    }


    /** {@code key:0b} printed by the tag printer, minus braces and value: the key as SNBT spells it. */
    private static String keyText(final String key) {
        net.minecraft.nbt.CompoundTag probe = new net.minecraft.nbt.CompoundTag();
        probe.putByte(key, (byte) 0);
        String printed = probe.toString();
        return printed.substring(1, printed.length() - ":0b}".length());
    }

    /**
     * SNBT with every compound's keys sorted ({@code String.compareTo}, UTF-16 code unit order):
     * the canonical form of the scoreboard op. Compound key order is the Java hash order in Paper
     * and insertion order in Rivet (D12), so it is not a property of the codec; every other tag is
     * printed by Paper's own printer.
     */
    static String canonicalSnbt(final Tag tag) {
        if (tag instanceof net.minecraft.nbt.CompoundTag compound) {
            List<String> keys = new ArrayList<>(compound.keySet());
            keys.sort(Comparator.naturalOrder());
            StringBuilder out = new StringBuilder("{");
            for (String key : keys) {
                if (out.length() > 1) {
                    out.append(',');
                }
                out.append(keyText(key)).append(':').append(canonicalSnbt(compound.get(key)));
            }
            return out.append('}').toString();
        }
        if (tag instanceof net.minecraft.nbt.ListTag list) {
            StringBuilder out = new StringBuilder("[");
            for (int index = 0; index < list.size(); index++) {
                if (index > 0) {
                    out.append(',');
                }
                out.append(canonicalSnbt(list.get(index)));
            }
            return out.append(']').toString();
        }
        return tag.toString();
    }

    private static JsonObject decline(final JsonObject result, final String phase) {
        result.addProperty("accept", false);
        result.addProperty("phase", phase);
        return result;
    }

    // ---- enchant.select -----------------------------------------------------------------

    private static Holder<Enchantment> enchantment(final Identifier id) {
        return RivetReferenceOracle.ServerRegistries.ACCESS.lookupOrThrow(Registries.ENCHANTMENT)
            .get(ResourceKey.create(Registries.ENCHANTMENT, id))
            .orElseThrow(() -> new IllegalArgumentException("unknown enchantment " + id));
    }

    /** A stack of {@code item} whose enchantability is exactly {@code enchantable} (an input, never looked up). */
    private static ItemStack enchantableStack(final JsonObject request) {
        Item item = BuiltInRegistries.ITEM.getOptional(requiredId(request, "item"))
            .orElseThrow(() -> new IllegalArgumentException("unknown item"));
        ItemStack stack = new ItemStack(item);
        int enchantable = requiredInt(request, "enchantable");
        if (enchantable > 0) {
            stack.set(DataComponents.ENCHANTABLE, new Enchantable(enchantable));
        } else {
            stack.remove(DataComponents.ENCHANTABLE);
        }
        return stack;
    }

    /**
     * {@code enchant.select {kind, ...}}. Kinds:
     * <ul>
     * <li>{@code cost {enchantment, level}}: {@code Enchantment.getMinCost/getMaxCost(level)};</li>
     * <li>{@code slot_cost {item, enchantable, slot, bookcases, seed}}:
     * {@code EnchantmentHelper.getEnchantmentCost(RandomSource.create(seed), slot, bookcases, stack)};</li>
     * <li>{@code select {item, enchantable, power, seed}}:
     * {@code EnchantmentHelper.selectEnchantment(RandomSource.create(seed), stack, power, #in_enchanting_table)}.</li>
     * </ul>
     */
    private static JsonObject enchantSelect(final JsonObject request, final JsonObject dec) {
        String kind = RivetReferenceOracle.requiredString(request, "kind");
        JsonObject result = new JsonObject();
        switch (kind) {
            case "cost" -> {
                Enchantment enchantment = enchantment(requiredId(request, "enchantment")).value();
                int level = requiredInt(request, "level");
                JsonObject costs = new JsonObject();
                costs.addProperty("min", enchantment.getMinCost(level));
                costs.addProperty("max", enchantment.getMaxCost(level));
                result.add("costs", costs);
            }
            case "slot_cost" -> {
                ItemStack stack = enchantableStack(request);
                RandomSource random = RandomSource.create(requiredLong(request, "seed"));
                result.addProperty("slot_cost", EnchantmentHelper.getEnchantmentCost(
                    random, requiredInt(request, "slot"), requiredInt(request, "bookcases"), stack
                ));
            }
            case "select" -> {
                ItemStack stack = enchantableStack(request);
                RandomSource random = RandomSource.create(requiredLong(request, "seed"));
                HolderLookup.RegistryLookup<Enchantment> registry =
                    RivetReferenceOracle.ServerRegistries.ACCESS.lookupOrThrow(Registries.ENCHANTMENT);
                var source = registry.getOrThrow(EnchantmentTags.IN_ENCHANTING_TABLE).stream();
                List<EnchantmentInstance> picked =
                    EnchantmentHelper.selectEnchantment(random, stack, requiredInt(request, "power"), source);
                JsonArray list = new JsonArray();
                for (EnchantmentInstance instance : picked) {
                    JsonObject entry = new JsonObject();
                    entry.addProperty("enchantment", instance.enchantment().unwrapKey().orElseThrow().identifier().toString());
                    entry.addProperty("level", instance.level());
                    list.add(entry);
                }
                result.add("enchantments", list);
            }
            default -> throw new IllegalArgumentException("unknown enchant.select kind: " + kind);
        }
        return result;
    }


    // ---- loot.roll and trade.offers: the live-server context ------------------------------------------

    private static final net.minecraft.world.phys.Vec3 ORIGIN = new net.minecraft.world.phys.Vec3(0.5, 64.0, 0.5);

    /** A fresh entity of {@code typeId} in the level shell (entities are never shared between rolls). */
    private static net.minecraft.world.entity.Entity entity(final String typeId) {
        var type = BuiltInRegistries.ENTITY_TYPE.getOptional(Identifier.parse(typeId))
            .orElseThrow(() -> new IllegalArgumentException("unknown entity type " + typeId));
        var created = type.create(GameplayData.level(), net.minecraft.world.entity.EntitySpawnReason.COMMAND);
        if (created == null) {
            throw new IllegalStateException("entity type " + typeId + " cannot be created in the level shell");
        }
        created.setPos(ORIGIN);
        return created;
    }

    private static ItemStack tool(final String setName) {
        return new ItemStack(BuiltInRegistries.ITEM.getValue(Identifier.withDefaultNamespace(switch (setName) {
            case "fishing" -> "fishing_rod";
            case "shearing" -> "shears";
            case "archaeology" -> "brush";
            default -> "diamond_pickaxe";
        })));
    }

    /** Context key -> value for the sets' keys; a key the oracle cannot supply is an error, never silently absent. */
    @SuppressWarnings({"unchecked", "rawtypes"})
    private static net.minecraft.world.level.storage.loot.LootParams lootParams(
        final net.minecraft.util.context.ContextKeySet keys, final String setName, final String entityType,
        final String block, final float luck
    ) {
        var level = GameplayData.level();
        var builder = new net.minecraft.world.level.storage.loot.LootParams.Builder(level).withLuck(luck);
        for (var key : keys.allowed()) {
            boolean required = keys.required().contains(key);
            if (key == net.minecraft.world.level.storage.loot.parameters.LootContextParams.ORIGIN) {
                builder.withParameter(net.minecraft.world.level.storage.loot.parameters.LootContextParams.ORIGIN, ORIGIN);
            } else if (key == net.minecraft.world.level.storage.loot.parameters.LootContextParams.TOOL) {
                builder.withParameter(net.minecraft.world.level.storage.loot.parameters.LootContextParams.TOOL, tool(setName));
            } else if (key == net.minecraft.world.level.storage.loot.parameters.LootContextParams.BLOCK_STATE) {
                builder.withParameter(net.minecraft.world.level.storage.loot.parameters.LootContextParams.BLOCK_STATE,
                    BuiltInRegistries.BLOCK.getOptional(Identifier.parse(block))
                        .orElseThrow(() -> new IllegalArgumentException("unknown block " + block)).defaultBlockState());
            } else if (key == net.minecraft.world.level.storage.loot.parameters.LootContextParams.THIS_ENTITY && required) {
                builder.withParameter(net.minecraft.world.level.storage.loot.parameters.LootContextParams.THIS_ENTITY, entity(entityType));
            } else if (key == net.minecraft.world.level.storage.loot.parameters.LootContextParams.TARGET_ENTITY && required) {
                builder.withParameter(net.minecraft.world.level.storage.loot.parameters.LootContextParams.TARGET_ENTITY, entity(entityType));
            } else if (key == net.minecraft.world.level.storage.loot.parameters.LootContextParams.DAMAGE_SOURCE) {
                var generic = RivetReferenceOracle.ServerRegistries.ACCESS.lookupOrThrow(Registries.DAMAGE_TYPE)
                    .getOrThrow(net.minecraft.world.damagesource.DamageTypes.GENERIC);
                builder.withParameter(net.minecraft.world.level.storage.loot.parameters.LootContextParams.DAMAGE_SOURCE,
                    new net.minecraft.world.damagesource.DamageSource(generic));
            } else if (required) {
                throw new IllegalStateException("the oracle cannot supply required loot parameter " + key.name() + " of set " + setName);
            }
        }
        return builder.create(keys);
    }

    private static String setName(final net.minecraft.util.context.ContextKeySet keys) {
        for (var field : net.minecraft.world.level.storage.loot.parameters.LootContextParamSets.class.getFields()) {
            try {
                if (field.get(null) == keys) {
                    return field.getName().toLowerCase(Locale.ROOT);
                }
            } catch (IllegalAccessException error) {
                throw new IllegalStateException(error);
            }
        }
        throw new IllegalStateException("unregistered context key set");
    }

    private static final Codec<ItemStack> STACK_CODEC = ItemStack.OPTIONAL_CODEC;

    private static String snbt(final ItemStack stack) {
        RegistryOps<Tag> ops = RegistryOps.create(NbtOps.INSTANCE, RivetReferenceOracle.ServerRegistries.ACCESS);
        return STACK_CODEC.encodeStart(ops, stack).getOrThrow().toString();
    }

    /**
     * {@code loot.roll {table, seed, luck, entity, block}}: {@code LootTable.getRandomItems(params, seed)}
     * for the vanilla table over documented, fixed {@code LootParams}, built from the table's own context
     * key set: ORIGIN = (0.5, 64, 0.5); luck = the request's; TOOL = a diamond pickaxe ({@code fishing}:
     * fishing rod, {@code shearing}: shears, {@code archaeology}: brush); BLOCK_STATE = the default state
     * of {@code block}; THIS_ENTITY/TARGET_ENTITY (only where the set requires them) = a fresh
     * {@code entity} created in the level shell ({@link GameplayData.LevelShell}, no running server);
     * DAMAGE_SOURCE = {@code generic}, no attacker. No killer player, block entity or explosion radius is
     * supplied, so conditions that need them (killed_by_player, survives_explosion's radius) take their
     * absent-parameter branch. The answer is the ordered stacks as SNBT ({@code ItemStack.OPTIONAL_CODEC}).
     * An exception thrown inside Paper's roll (including one caused by the level shell lacking a method)
     * is {@code threw}.
     */
    private static JsonObject lootRoll(final JsonObject request, final JsonObject dec) {
        Identifier id = requiredId(request, "table");
        var table = GameplayData.reloadable().lookupWithUpdatedTags().lookupOrThrow(Registries.LOOT_TABLE)
            .get(ResourceKey.create(Registries.LOOT_TABLE, id))
            .orElseThrow(() -> new IllegalArgumentException("unknown loot table " + id)).value();
        JsonObject result = new JsonObject();
        try {
            var keys = table.getParamSet();
            var params = lootParams(keys, setName(keys), RivetReferenceOracle.requiredString(request, "entity"),
                RivetReferenceOracle.requiredString(request, "block"), request.get("luck").getAsFloat());
            JsonArray items = new JsonArray();
            for (ItemStack stack : table.getRandomItems(params, requiredLong(request, "seed"))) {
                items.add(snbt(stack));
            }
            result.add("items", items);
        } catch (RuntimeException error) {
            dec.addProperty("thrown", error.toString());
            result = new JsonObject();
            result.add("items", new JsonArray());
            result.addProperty("threw", error.getClass().getName());
        }
        return result;
    }

    /** {@code loot.roll}: every loot table of the data pack x one shared list of {@link #SEEDS} seeds. */
    static void lootCheck(final List<JsonObject> requests) {
        var values = requireCrossProduct("loot.roll", requests, null, "table", "seed");
        requireSame("loot.roll", values.get(0), quoted(
            GameplayData.reloadable().lookupWithUpdatedTags().lookupOrThrow(Registries.LOOT_TABLE).listElementIds()
                .map(key -> key.identifier()).toList()));
        if (values.get(1).size() != SEEDS) {
            throw new IllegalStateException("loot.roll: every table needs exactly " + SEEDS + " seeds");
        }
    }

    /**
     * {@code trade.offers {trade_set, variant, seed}}: the offers of a vanilla trade set for a fresh
     * villager of the given variant (the context {@code AbstractVillager.addOffersFromTradeSet} builds:
     * ORIGIN, THIS_ENTITY, ADDITIONAL_COST_COMPONENT_ALLOWED; random = {@code RandomSource.create(seed)}).
     * The selection loops are transcribed from {@code AbstractVillager.addOffersFromItemListings} and
     * {@code ...WithoutDuplicates} (package-private statics that end in Paper's Bukkit acquire event, which
     * needs a running server); every offer is {@code VillagerTrade.getOffer(context)} itself. Offers are
     * answered through {@code MerchantOffer.CODEC} as SNBT.
     */
    private static JsonObject tradeOffers(final JsonObject request, final JsonObject dec) {
        Identifier id = requiredId(request, "trade_set");
        var set = RivetReferenceOracle.ServerRegistries.ACCESS.lookupOrThrow(Registries.TRADE_SET)
            .get(ResourceKey.create(Registries.TRADE_SET, id))
            .orElseThrow(() -> new IllegalArgumentException("unknown trade set " + id)).value();
        JsonObject result = new JsonObject();
        try {
            var villager = entity(RivetReferenceOracle.requiredString(request, "merchant"));
            String variant = RivetReferenceOracle.requiredString(request, "variant");
            if (villager instanceof net.minecraft.world.entity.npc.villager.Villager v) {
                var type = RivetReferenceOracle.ServerRegistries.ACCESS.lookupOrThrow(Registries.VILLAGER_TYPE)
                    .getOrThrow(ResourceKey.create(Registries.VILLAGER_TYPE, Identifier.parse(variant)));
                v.setVillagerData(v.getVillagerData().withType(type));
            }
            var context = new net.minecraft.world.level.storage.loot.LootContext.Builder(
                new net.minecraft.world.level.storage.loot.LootParams.Builder(GameplayData.level())
                    .withParameter(net.minecraft.world.level.storage.loot.parameters.LootContextParams.ORIGIN, ORIGIN)
                    .withParameter(net.minecraft.world.level.storage.loot.parameters.LootContextParams.THIS_ENTITY, villager)
                    .withParameter(net.minecraft.world.level.storage.loot.parameters.LootContextParams.ADDITIONAL_COST_COMPONENT_ALLOWED, net.minecraft.util.Unit.INSTANCE)
                    .create(net.minecraft.world.level.storage.loot.parameters.LootContextParamSets.VILLAGER_TRADE)
            ).withOptionalRandomSeed(requiredLong(request, "seed")).create(set.randomSequence());
            int wanted = set.calculateNumberOfTrades(context);
            var offers = new net.minecraft.world.item.trading.MerchantOffers();
            var candidates = new ArrayList<Holder<net.minecraft.world.item.trading.VillagerTrade>>();
            set.getTrades().forEach(candidates::add);
            int found = 0;
            while (found < wanted && !candidates.isEmpty()) {
                if (set.allowDuplicates()) {
                    int roll = context.getRandom().nextInt(candidates.size());
                    var offer = candidates.get(roll).value().getOffer(context);
                    if (offer == null) {
                        candidates.remove(roll);
                    } else {
                        offers.add(offer);
                        found++;
                    }
                } else {
                    var trade = candidates.remove(context.getRandom().nextInt(candidates.size()));
                    var offer = trade.value().getOffer(context);
                    if (offer != null) {
                        offers.add(offer);
                        found++;
                    }
                }
            }
            RegistryOps<Tag> ops = RegistryOps.create(NbtOps.INSTANCE, RivetReferenceOracle.ServerRegistries.ACCESS);
            JsonArray list = new JsonArray();
            for (var offer : offers) {
                list.add(net.minecraft.world.item.trading.MerchantOffer.CODEC.encodeStart(ops, offer).getOrThrow().toString());
            }
            result.add("offers", list);
        } catch (RuntimeException error) {
            dec.addProperty("thrown", error.toString());
            result = new JsonObject();
            result.add("offers", new JsonArray());
            result.addProperty("threw", error.getClass().getName());
        }
        return result;
    }

    /** {@code trade.offers}: every trade set of the data pack x one shared list of {@link #SEEDS} seeds. */
    static void tradeCheck(final List<JsonObject> requests) {
        var values = requireCrossProduct("trade.offers", requests, null, "trade_set", "seed");
        requireSame("trade.offers", values.get(0), quoted(
            RivetReferenceOracle.ServerRegistries.ACCESS.lookupOrThrow(Registries.TRADE_SET).listElementIds()
                .map(key -> key.identifier()).toList()));
        if (values.get(1).size() != SEEDS) {
            throw new IllegalStateException("trade.offers: every trade set needs exactly " + SEEDS + " seeds");
        }
    }

    // ---- recipe.match --------------------------------------------------------------------

    /** Parse a corpus cell: {@code ""} is empty, otherwise {@code <item id>[#<count>]}. */
    static ItemStack stack(final JsonElement cell) {
        String text = cell.getAsString();
        if (text.isEmpty()) {
            return ItemStack.EMPTY;
        }
        int hash = text.indexOf('#');
        int count = hash < 0 ? 1 : Integer.parseInt(text.substring(hash + 1));
        Identifier id = Identifier.parse(hash < 0 ? text : text.substring(0, hash));
        Item item = BuiltInRegistries.ITEM.getOptional(id).orElseThrow(() -> new IllegalArgumentException("unknown item " + id));
        return new ItemStack(item, count);
    }

    /**
     * {@code recipe.match {recipe, kind, ...}} over the recipe manager loaded from the vanilla
     * data pack. {@code kind} selects the input: {@code crafting {width, height, grid}},
     * {@code single {input}}, {@code smithing {template, base, addition}}. The answer is
     * {@code Recipe.matches(input, level)} and, on a match, the assembled stack:
     * {@code {item, count, components}} with the sorted ids of its component patch
     * ({@code !id} for a removal). An exception thrown by Paper's own matching or
     * assembling is {@code threw} (its class name) instead of an answer.
     */
    @SuppressWarnings({"unchecked", "rawtypes"})
    private static JsonObject recipeMatch(final JsonObject request, final JsonObject dec) {
        Identifier id = requiredId(request, "recipe");
        RecipeHolder<?> holder = GameplayData.recipes().byKey.get(ResourceKey.create(Registries.RECIPE, id));
        if (holder == null) {
            throw new IllegalArgumentException("unknown recipe " + id);
        }
        Recipe recipe = holder.value();
        String kind = RivetReferenceOracle.requiredString(request, "kind");
        RecipeInput input = switch (kind) {
            case "crafting" -> {
                int width = requiredInt(request, "width");
                int height = requiredInt(request, "height");
                List<ItemStack> items = new ArrayList<>();
                for (JsonElement cell : request.getAsJsonArray("grid")) {
                    items.add(stack(cell));
                }
                yield CraftingInput.of(width, height, items);
            }
            case "single" -> new SingleRecipeInput(stack(request.get("input")));
            case "smithing" -> new SmithingRecipeInput(
                stack(request.get("template")), stack(request.get("base")), stack(request.get("addition"))
            );
            default -> throw new IllegalArgumentException("unknown recipe.match kind: " + kind);
        };
        boolean fits = switch (kind) {
            case "crafting" -> recipe instanceof CraftingRecipe;
            case "single" -> recipe instanceof SingleItemRecipe || recipe instanceof AbstractCookingRecipe;
            default -> recipe instanceof SmithingRecipe;
        };
        if (!fits) {
            throw new IllegalArgumentException("recipe " + id + " (" + recipe.getClass().getSimpleName() + ") takes no " + kind + " input");
        }
        JsonObject result = new JsonObject();
        try {
            boolean matches = recipe.matches(input, GameplayData.level());
            result.addProperty("match", matches);
            if (matches) {
                result.add("result", describeStack(recipe.assemble(input)));
            }
        } catch (RuntimeException error) {
            dec.addProperty("thrown", error.toString());
            result = new JsonObject();
            result.addProperty("match", false);
            result.addProperty("threw", error.getClass().getName());
        }
        return result;
    }

    private static JsonObject describeStack(final ItemStack stack) {
        JsonObject out = new JsonObject();
        out.addProperty("item", BuiltInRegistries.ITEM.getKey(stack.getItem()).toString());
        out.addProperty("count", stack.getCount());
        Set<String> ids = new TreeSet<>();
        for (var entry : stack.getComponentsPatch().entrySet()) {
            String key = BuiltInRegistries.DATA_COMPONENT_TYPE.getKey(entry.getKey()).toString();
            ids.add(entry.getValue().isPresent() ? key : "!" + key);
        }
        JsonArray components = new JsonArray();
        ids.forEach(components::add);
        out.add("components", components);
        return out;
    }
}
