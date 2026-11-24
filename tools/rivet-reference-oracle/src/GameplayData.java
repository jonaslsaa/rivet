package dev.rivet.oracle;

import java.lang.reflect.Method;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.packs.resources.ResourceManager;
import net.minecraft.util.profiling.InactiveProfiler;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.crafting.RecipeManager;
import net.minecraft.world.item.crafting.RecipeMap;
import net.minecraft.world.level.Level;

/**
 * The reloadable half of the vanilla data pack (loot tables, the recipe manager) and the
 * minimal live-server context the gameplay ops need, built once on first use.
 *
 * <p>Registries and recipes load the way {@code ReloadableServerResources} loads them, from the
 * vanilla trusted pack repository over {@code RivetReferenceOracle.ServerRegistries}' layers
 * ({@link ReloadableServerRegistries#reload}; the recipe manager's own {@code prepare}).
 * There is no running {@code MinecraftServer}: the {@link ServerLevel} an op needs is a
 * {@link LevelShell}, an unconstructed level (allocated, not constructed) that answers only
 * the getters those ops reach, over the frozen registries.
 */
final class GameplayData {
    private GameplayData() {
    }

    private static ResourceManager manager;
    private static ReloadableServerRegistries.LoadResult reloadable;
    private static RecipeManager recipes;
    private static LevelShell level;

    /** Everything is loaded lazily and quietly: registry loading logs to the JSON-Lines stream. */
    private static synchronized void ensure() {
        if (level != null) {
            return;
        }
        var logLevel = org.apache.logging.log4j.LogManager.getRootLogger().getLevel();
        org.apache.logging.log4j.core.config.Configurator.setRootLevel(org.apache.logging.log4j.Level.OFF);
        try {
            // Static registry tags are applied by ServerRegistries.load; the reload sees them bound.
            LayeredRegistryAccess<RegistryLayer> layers = RivetReferenceOracle.ServerRegistries.LAYERS;
            var packs = net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
            MinecraftServer.configurePackRepository(
                packs, net.minecraft.world.level.WorldDataConfiguration.DEFAULT, true, false
            );
            manager = new MultiPackResourceManager(PackType.SERVER_DATA, packs.openAllSelected());
            CompletableFuture<ReloadableServerRegistries.LoadResult> loading =
                ReloadableServerRegistries.reload(layers, List.of(), manager, Runnable::run);
            reloadable = loading.join();
            HolderLookup.Provider lookup = reloadable.lookupWithUpdatedTags();
            recipes = new RecipeManager(lookup);
            Method prepare = RecipeManager.class.getDeclaredMethod(
                "prepare", ResourceManager.class, net.minecraft.util.profiling.ProfilerFiller.class
            );
            prepare.setAccessible(true);
            recipes.recipes = (RecipeMap) prepare.invoke(recipes, manager, InactiveProfiler.INSTANCE);
            level = LevelShell.create(reloadable.layers().compositeAccess(), lookup);
        } catch (Exception error) {
            throw new IllegalStateException("cannot load the reloadable vanilla data", error);
        } finally {
            org.apache.logging.log4j.core.config.Configurator.setRootLevel(logLevel);
        }
    }

    static RecipeMap recipes() {
        ensure();
        return recipes.recipes;
    }

    static ReloadableServerRegistries.LoadResult reloadable() {
        ensure();
        return reloadable;
    }

    static ServerLevel level() {
        ensure();
        return level;
    }

    static final sun.misc.Unsafe UNSAFE;

    static {
        try {
            var field = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
            field.setAccessible(true);
            UNSAFE = (sun.misc.Unsafe) field.get(null);
        } catch (Exception error) {
            throw new ExceptionInInitializerError(error);
        }
    }

    /** A server whose only live part is its reloadable registries (loot tables) and registry access. */
    static final class ServerShell extends net.minecraft.server.dedicated.DedicatedServer {
        private ServerShell() {
            super(null, null, null, null, null, null, null, null, null, null, null, null);
        }

        ReloadableServerRegistries.Holder holder;
        RegistryAccess.Frozen access;

        @Override
        public ReloadableServerRegistries.Holder reloadableRegistries() {
            return this.holder;
        }

        @Override
        public RegistryAccess.Frozen registryAccess() {
            return this.access;
        }
    }

    /** See the class comment: an unconstructed {@link ServerLevel} answering the getters the ops reach. */
    static final class LevelShell extends ServerLevel {
        private RegistryAccess.Frozen access;
        private ServerShell server;
        io.papermc.paper.configuration.WorldConfiguration config;
        int nextId = 100;

        private LevelShell() {
            super(null, null, null, null, null, null, false, 0L, List.of(), false, null, null, null, null, null, null);
        }

        static LevelShell create(final RegistryAccess.Frozen access, final HolderLookup.Provider lookup) throws Exception {
            LevelShell shell = (LevelShell) UNSAFE.allocateInstance(LevelShell.class);
            shell.access = access;
            shell.data = levelData();
            shell.server = (ServerShell) UNSAFE.allocateInstance(ServerShell.class);
            shell.server.access = access;
            shell.server.holder = new ReloadableServerRegistries.Holder(lookup);
            var spigot = (org.spigotmc.SpigotWorldConfig) UNSAFE.allocateInstance(org.spigotmc.SpigotWorldConfig.class);
            var field = Level.class.getDeclaredField("spigotConfig");
            field.setAccessible(true);
            field.set(shell, spigot);
            var ctor = io.papermc.paper.configuration.WorldConfiguration.class.getDeclaredConstructor(
                org.spigotmc.SpigotWorldConfig.class, Identifier.class
            );
            ctor.setAccessible(true);
            shell.config = ctor.newInstance(spigot, Identifier.withDefaultNamespace("overworld"));
            populate(shell.config);
            // No layers: every environment attribute answers its default value.
            shell.setEnvironmentAttributes(net.minecraft.world.attribute.EnvironmentAttributeSystem.builder().build());
            return shell;
        }

        /** Level data: game time 0, NORMAL difficulty, every other getter its zero value. */
        private net.minecraft.world.level.storage.ServerLevelData data;

        private static net.minecraft.world.level.storage.ServerLevelData levelData() {
            return (net.minecraft.world.level.storage.ServerLevelData) java.lang.reflect.Proxy.newProxyInstance(
                GameplayData.class.getClassLoader(),
                new Class<?>[] {net.minecraft.world.level.storage.ServerLevelData.class},
                (proxy, method, args) -> {
                    if (method.isDefault()) {
                        return java.lang.invoke.MethodHandles.privateLookupIn(method.getDeclaringClass(), java.lang.invoke.MethodHandles.lookup())
                            .unreflectSpecial(method, method.getDeclaringClass()).bindTo(proxy).invokeWithArguments(args == null ? new Object[0] : args);
                    }
                    if (method.getName().equals("getDifficulty")) {
                        return net.minecraft.world.Difficulty.NORMAL;
                    }
                    if (method.getName().equals("getLevelName")) {
                        return "gameplay-oracle";
                    }
                    Class<?> type = method.getReturnType();
                    if (type == boolean.class) {
                        return false;
                    } else if (type == int.class) {
                        return 0;
                    } else if (type == long.class) {
                        return 0L;
                    } else if (type == float.class) {
                        return 0.0F;
                    } else if (type == double.class) {
                        return 0.0D;
                    }
                    return null;
                }
            );
        }

        /**
         * Give every configuration section of Paper's world config its defaults, the way the
         * configuration loader instantiates the object tree: each nested {@code ConfigurationPart}
         * field that is still null is constructed (inner classes take their enclosing part) and
         * populated in turn.
         */
        private static void populate(final Object part) throws Exception {
            for (var field : part.getClass().getDeclaredFields()) {
                Class<?> type = field.getType();
                if (!io.papermc.paper.configuration.ConfigurationPart.class.isAssignableFrom(type)
                    || field.isSynthetic() || java.lang.reflect.Modifier.isStatic(field.getModifiers())) {
                    continue;
                }
                field.setAccessible(true);
                if (field.get(part) == null) {
                    var ctor = type.getDeclaredConstructors()[0];
                    ctor.setAccessible(true);
                    field.set(part, java.lang.reflect.Modifier.isStatic(type.getModifiers()) ? ctor.newInstance() : ctor.newInstance(part));
                }
                populate(field.get(part));
            }
        }

        @Override
        public net.minecraft.world.level.storage.LevelData getLevelData() {
            return this.data;
        }

        @Override
        public MinecraftServer getServer() {
            return this.server;
        }

        @Override
        public int getNextEntityId() {
            return ++this.nextId;
        }

        @Override
        public FeatureFlagSet enabledFeatures() {
            return FeatureFlags.DEFAULT_FLAGS;
        }

        @Override
        public io.papermc.paper.configuration.WorldConfiguration paperConfig() {
            return this.config;
        }

        @Override
        public RegistryAccess registryAccess() {
            return this.access;
        }
    }
}
