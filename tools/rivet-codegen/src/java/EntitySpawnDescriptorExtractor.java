//! EntitySpawnDescriptorExtractor — Paper 26.2 authoritative extractor for the
//! exact set of CREATURE `EntityType` spawn descriptors reachable from
//! overworld `possibleBiomes()` `MobSpawnSettings`.
//!
//! Derivation (zero hand-entered entity lists or hardcoded lookup maps):
//!   1. Boot Paper's vanilla datapack + worldgen/dimension registries using the
//!      same sequence as `WorldgenFeatureDataExtractor`.
//!   2. Walk `biomeSource.possibleBiomes()` and collect every `EntityType<?>`
//!      referenced by `biome.getMobSettings().getMobs(MobCategory.CREATURE)`.
//!   3. Sort the distinct `EntityType<?>` closure by dense registry ID
//!      (`BuiltInRegistries.ENTITY_TYPE.getId(type)`).
//!   4. Extract live runtime metadata (`id`, `name`, `category`, `can_summon`,
//!      `allowed_in_peaceful`, `width`, `height`, `spawn_dimensions_scale`,
//!      `fire_immune`, `immune_to_tag`, `placement`, `heightmap`) directly from
//!      Paper's `EntityType` and `SpawnPlacements` instances.
//!   5. Extract the stable `factory` and `spawn_predicate` symbolic keys
//!      directly from `EntityTypes.class` and `SpawnPlacements.class` bytecode
//!      (`invokedynamic` `LambdaMetafactory` bootstrap method handles) via
//!      JDK 25's `java.lang.classfile` API, cross-checked against live static
//!      fields and `SpawnPlacements.DATA_BY_TYPE`.

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStreamWriter;
import java.io.Writer;
import java.lang.classfile.ClassFile;
import java.lang.classfile.ClassModel;
import java.lang.classfile.CodeElement;
import java.lang.classfile.CodeModel;
import java.lang.classfile.MethodModel;
import java.lang.classfile.Opcode;
import java.lang.classfile.instruction.FieldInstruction;
import java.lang.classfile.instruction.InvokeDynamicInstruction;
import java.lang.classfile.instruction.InvokeInstruction;
import java.lang.constant.ConstantDesc;
import java.lang.constant.DirectMethodHandleDesc;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
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
import net.minecraft.tags.TagKey;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.MobCategory;
import net.minecraft.world.entity.SpawnPlacementType;
import net.minecraft.world.entity.SpawnPlacementTypes;
import net.minecraft.world.entity.SpawnPlacements;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.MobSpawnSettings;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;

public final class EntitySpawnDescriptorExtractor {
    private static final String GENERATOR_NAME =
        "EntitySpawnDescriptorExtractor (possible-biome CREATURE entity spawn descriptors)";

    private EntitySpawnDescriptorExtractor() {}

    private static String argValue(String[] args, String name) {
        for (int i = 0; i + 1 < args.length; i++) {
            if (args[i].equals(name)) {
                return args[i + 1];
            }
        }
        return null;
    }

    private static byte[] readClassBytes(Class<?> cls) throws Exception {
        String resource = "/" + cls.getName().replace('.', '/') + ".class";
        try (InputStream in = cls.getResourceAsStream(resource)) {
            if (in == null) {
                throw new IllegalStateException("Could not load class bytes for " + cls.getName());
            }
            return in.readAllBytes();
        }
    }

    private static String formatMethodHandle(DirectMethodHandleDesc mh) {
        String owner = mh.owner().descriptorString();
        if (owner.startsWith("L") && owner.endsWith(";")) {
            owner = owner.substring(1, owner.length() - 1).replace('/', '.');
        }
        String method = mh.methodName().equals("<init>") ? "new" : mh.methodName();
        return owner + "::" + method;
    }

    private static DirectMethodHandleDesc extractLambdaTarget(InvokeDynamicInstruction indy) {
        for (ConstantDesc arg : indy.bootstrapArgs()) {
            if (arg instanceof DirectMethodHandleDesc dmh) {
                return dmh;
            }
        }
        return null;
    }

    /**
     * Parse `EntityTypes.<clinit>` bytecode to map each static `EntityType` field
     * name to its `EntityType.EntityFactory` method-reference symbol.
     */
    private static Map<String, String> extractFactorySymbols() throws Exception {
        ClassModel model = ClassFile.of().parse(readClassBytes(EntityTypes.class));
        Map<String, String> byField = new HashMap<>();
        String entityTypesInternal = EntityTypes.class.getName().replace('.', '/');

        for (MethodModel method : model.methods()) {
            if (!method.methodName().equalsString("<clinit>")) {
                continue;
            }
            CodeModel code = method.code().orElseThrow(
                () -> new IllegalStateException("EntityTypes.<clinit> has no code attribute")
            );
            String pendingFactory = null;
            for (CodeElement elem : code) {
                if (elem instanceof InvokeDynamicInstruction indy) {
                    if (indy.typeSymbol().returnType().descriptorString()
                            .equals("Lnet/minecraft/world/entity/EntityType$EntityFactory;")) {
                        DirectMethodHandleDesc mh = extractLambdaTarget(indy);
                        if (mh == null) {
                            throw new IllegalStateException(
                                "EntityType$EntityFactory invokedynamic missing DirectMethodHandleDesc"
                            );
                        }
                        pendingFactory = formatMethodHandle(mh);
                    }
                } else if (elem instanceof FieldInstruction fi
                        && fi.opcode() == Opcode.PUTSTATIC
                        && fi.owner().asInternalName().equals(entityTypesInternal)
                        && fi.typeSymbol().descriptorString()
                            .equals("Lnet/minecraft/world/entity/EntityType;")) {
                    if (pendingFactory != null) {
                        String fieldName = fi.name().stringValue();
                        String prev = byField.put(fieldName, pendingFactory);
                        if (prev != null) {
                            throw new IllegalStateException(
                                "Duplicate EntityTypes factory extraction for field " + fieldName
                            );
                        }
                        pendingFactory = null;
                    }
                }
            }
        }
        return byField;
    }

    /**
     * Parse `SpawnPlacements.<clinit>` bytecode to map each `EntityTypes.<FIELD>`
     * registered via `SpawnPlacements.register(...)` to its `SpawnPredicate`
     * method-reference symbol.
     */
    private static Map<String, String> extractSpawnPredicateSymbols() throws Exception {
        ClassModel model = ClassFile.of().parse(readClassBytes(SpawnPlacements.class));
        Map<String, String> byField = new HashMap<>();
        String entityTypesInternal = EntityTypes.class.getName().replace('.', '/');
        String spawnPlacementsInternal = SpawnPlacements.class.getName().replace('.', '/');

        for (MethodModel method : model.methods()) {
            if (!method.methodName().equalsString("<clinit>")) {
                continue;
            }
            CodeModel code = method.code().orElseThrow(
                () -> new IllegalStateException("SpawnPlacements.<clinit> has no code attribute")
            );
            String pendingEntityField = null;
            String pendingPredicate = null;
            for (CodeElement elem : code) {
                if (elem instanceof FieldInstruction fi
                        && fi.opcode() == Opcode.GETSTATIC
                        && fi.owner().asInternalName().equals(entityTypesInternal)
                        && fi.typeSymbol().descriptorString()
                            .equals("Lnet/minecraft/world/entity/EntityType;")) {
                    pendingEntityField = fi.name().stringValue();
                } else if (elem instanceof InvokeDynamicInstruction indy
                        && indy.typeSymbol().returnType().descriptorString()
                            .equals("Lnet/minecraft/world/entity/SpawnPlacements$SpawnPredicate;")) {
                    DirectMethodHandleDesc mh = extractLambdaTarget(indy);
                    if (mh == null) {
                        throw new IllegalStateException(
                            "SpawnPredicate invokedynamic missing DirectMethodHandleDesc"
                        );
                    }
                    pendingPredicate = formatMethodHandle(mh);
                } else if (elem instanceof InvokeInstruction inv
                        && inv.opcode() == Opcode.INVOKESTATIC
                        && inv.owner().asInternalName().equals(spawnPlacementsInternal)
                        && inv.name().equalsString("register")) {
                    if (pendingEntityField == null || pendingPredicate == null) {
                        throw new IllegalStateException(
                            "SpawnPlacements.register call missing entity field ("
                                + pendingEntityField + ") or predicate (" + pendingPredicate + ")"
                        );
                    }
                    String prev = byField.put(pendingEntityField, pendingPredicate);
                    if (prev != null) {
                        throw new IllegalStateException(
                            "Duplicate SpawnPlacements.register extraction for field " + pendingEntityField
                        );
                    }
                    pendingEntityField = null;
                    pendingPredicate = null;
                }
            }
        }
        return byField;
    }

    private static Map<EntityType<?>, String> mapEntityTypesToFieldNames() throws Exception {
        Map<EntityType<?>, String> byType = new IdentityHashMap<>();
        for (Field f : EntityTypes.class.getDeclaredFields()) {
            int mods = f.getModifiers();
            if (!Modifier.isStatic(mods) || f.getType() != EntityType.class) {
                continue;
            }
            f.setAccessible(true);
            EntityType<?> value = (EntityType<?>) f.get(null);
            if (value == null) {
                throw new IllegalStateException("Null static EntityType field: " + f.getName());
            }
            String prev = byType.put(value, f.getName());
            if (prev != null) {
                throw new IllegalStateException(
                    "EntityType instance bound to multiple fields: " + prev + " and " + f.getName()
                );
            }
        }
        return byType;
    }

    private static String placementKey(SpawnPlacementType placement) {
        if (placement == SpawnPlacementTypes.ON_GROUND) {
            return "on_ground";
        }
        if (placement == SpawnPlacementTypes.NO_RESTRICTIONS) {
            return "no_restrictions";
        }
        if (placement == SpawnPlacementTypes.IN_WATER) {
            return "in_water";
        }
        if (placement == SpawnPlacementTypes.IN_LAVA) {
            return "in_lava";
        }
        throw new IllegalStateException("Unknown SpawnPlacementType instance: " + placement);
    }

    public static void main(String[] args) throws Exception {
        String output = argValue(args, "--output");
        String mcVersion = argValue(args, "--version");
        String paper = argValue(args, "--paper");
        if (output == null || paper == null || mcVersion == null) {
            throw new IllegalArgumentException(
                "--output <path>, --version <mc>, and --paper <pin> are required"
            );
        }

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // ---- registries (identical sequence to WorldgenFeatureDataExtractor) ---
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
        ResourceManager resourceManager =
            new MultiPackResourceManager(PackType.SERVER_DATA, packRepository.openAllSelected());

        var layers = RegistryLayer.createRegistryAccess();
        List<net.minecraft.core.HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(
            layers.getAccessForLoading(RegistryLayer.WORLDGEN),
            TagLoader.loadTagsForExistingRegistries(resourceManager, layers.getLayer(RegistryLayer.STATIC))
        );
        RegistryAccess.Frozen worldGenRegistries = RegistryDataLoader.load(
            resourceManager, lookups, RegistryDataLoader.WORLDGEN_REGISTRIES, Runnable::run
        ).join();
        layers = layers.replaceFrom(RegistryLayer.WORLDGEN, worldGenRegistries);

        List<net.minecraft.core.HolderLookup.RegistryLookup<?>> staticAndWorldgen = new ArrayList<>(lookups);
        worldGenRegistries.listRegistries().forEach(staticAndWorldgen::add);
        RegistryAccess.Frozen dimensionRegistries = RegistryDataLoader.load(
            resourceManager, staticAndWorldgen, RegistryDataLoader.DIMENSION_REGISTRIES, Runnable::run
        ).join();
        layers = layers.replaceFrom(RegistryLayer.DIMENSIONS, dimensionRegistries);
        RegistryAccess registryAccess = layers.compositeAccess().freeze();

        Holder<net.minecraft.world.level.levelgen.presets.WorldPreset> preset =
            registryAccess.lookupOrThrow(Registries.WORLD_PRESET)
                .getOrThrow(net.minecraft.world.level.levelgen.presets.WorldPresets.NORMAL);
        NoiseBasedChunkGenerator generator =
            (NoiseBasedChunkGenerator) preset.value().createWorldDimensions().overworld();
        MultiNoiseBiomeSource biomeSource = (MultiNoiseBiomeSource) generator.getBiomeSource();

        List<Holder<Biome>> possibleBiomes = new ArrayList<>(biomeSource.possibleBiomes());
        if (possibleBiomes.size() < 2) {
            throw new IllegalStateException(
                "full overworld possible-biome list is degenerate (" + possibleBiomes.size() + " biome(s))"
            );
        }

        // ---- derive reachable CREATURE EntityType set -------------------------
        Set<EntityType<?>> reachable = new LinkedHashSet<>();
        for (Holder<Biome> biomeHolder : possibleBiomes) {
            MobSpawnSettings ms = biomeHolder.value().getMobSettings();
            for (var weighted : ms.getMobs(MobCategory.CREATURE).unwrap()) {
                EntityType<?> type = weighted.value().type();
                if (type.getCategory() != MobCategory.CREATURE) {
                    throw new IllegalStateException(
                        "Possible-biome CREATURE spawner references non-CREATURE entity "
                            + EntityType.getKey(type) + " (category=" + type.getCategory() + ")"
                    );
                }
                reachable.add(type);
            }
        }

        List<EntityType<?>> ordered = new ArrayList<>(reachable);
        ordered.sort(Comparator.comparingInt(BuiltInRegistries.ENTITY_TYPE::getId));

        // ---- bytecode + reflection metadata -----------------------------------
        Map<EntityType<?>, String> entityFieldNames = mapEntityTypesToFieldNames();
        Map<String, String> factoryByField = extractFactorySymbols();
        Map<String, String> predicateByField = extractSpawnPredicateSymbols();

        Field spawnScaleField = EntityType.class.getDeclaredField("spawnDimensionsScale");
        spawnScaleField.setAccessible(true);
        Field immuneToField = EntityType.class.getDeclaredField("immuneTo");
        immuneToField.setAccessible(true);
        Field factoryField = EntityType.class.getDeclaredField("factory");
        factoryField.setAccessible(true);
        Field dataByTypeField = SpawnPlacements.class.getDeclaredField("DATA_BY_TYPE");
        dataByTypeField.setAccessible(true);
        @SuppressWarnings("unchecked")
        Map<EntityType<?>, ?> spawnPlacementData = (Map<EntityType<?>, ?>) dataByTypeField.get(null);

        JsonArray descriptorsOut = new JsonArray();
        for (EntityType<?> type : ordered) {
            int registryId = BuiltInRegistries.ENTITY_TYPE.getId(type);
            if (registryId < 0) {
                throw new IllegalStateException("Unregistered entity type: " + type);
            }
            String name = EntityType.getKey(type).toString();
            String fieldName = entityFieldNames.get(type);
            if (fieldName == null) {
                throw new IllegalStateException("EntityType not found as static field in EntityTypes: " + name);
            }
            if (factoryField.get(type) == null) {
                throw new IllegalStateException("Null runtime factory for " + name);
            }
            String factorySymbol = factoryByField.get(fieldName);
            if (factorySymbol == null) {
                throw new IllegalStateException("Missing bytecode factory symbol for " + name + " (" + fieldName + ")");
            }
            if (!spawnPlacementData.containsKey(type)) {
                throw new IllegalStateException("Missing SpawnPlacements registration for " + name);
            }
            String predicateSymbol = predicateByField.get(fieldName);
            if (predicateSymbol == null) {
                throw new IllegalStateException(
                    "Missing bytecode SpawnPlacements predicate symbol for " + name + " (" + fieldName + ")"
                );
            }

            float spawnDimensionsScale = spawnScaleField.getFloat(type);
            @SuppressWarnings("unchecked")
            TagKey<Block> immuneTo = (TagKey<Block>) immuneToField.get(type);
            if (immuneTo == null) {
                throw new IllegalStateException("Null immuneTo tag for " + name);
            }
            SpawnPlacementType placement = SpawnPlacements.getPlacementType(type);
            Heightmap.Types heightmap = SpawnPlacements.getHeightmapType(type);

            JsonObject entry = new JsonObject();
            entry.addProperty("id", registryId);
            entry.addProperty("name", name);
            entry.addProperty("category", type.getCategory().getSerializedName());
            entry.addProperty("factory", factorySymbol);
            entry.addProperty("can_summon", type.canSummon());
            entry.addProperty("allowed_in_peaceful", type.isAllowedInPeaceful());
            entry.addProperty("width", Float.valueOf(type.getWidth()));
            entry.addProperty("height", Float.valueOf(type.getHeight()));
            entry.addProperty("spawn_dimensions_scale", Float.valueOf(spawnDimensionsScale));
            entry.addProperty("fire_immune", type.fireImmune());
            entry.addProperty("immune_to_tag", immuneTo.location().toString());
            entry.addProperty("placement", placementKey(placement));
            entry.addProperty("heightmap", heightmap.getSerializedName());
            entry.addProperty("spawn_predicate", predicateSymbol);
            descriptorsOut.add(entry);
        }

        JsonObject root = new JsonObject();
        root.addProperty("format", 1);
        root.addProperty("generator", GENERATOR_NAME);
        root.addProperty("paper", paper);
        root.addProperty("minecraft_version", mcVersion);
        root.addProperty("protocol_version", SharedConstants.getProtocolVersion());
        root.addProperty("world_version", SharedConstants.WORLD_VERSION);
        root.addProperty("possible_biome_count", possibleBiomes.size());
        root.add("descriptors", descriptorsOut);

        Gson gson = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();
        try (Writer writer = new BufferedWriter(
                new OutputStreamWriter(new FileOutputStream(output), StandardCharsets.UTF_8))) {
            gson.toJson(root, writer);
            writer.write('\n');
        }

        System.out.println("possible_biomes=" + possibleBiomes.size());
        System.out.println("creature_descriptors=" + ordered.size());
    }
}
