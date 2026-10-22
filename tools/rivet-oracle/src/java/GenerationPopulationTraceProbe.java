import com.google.gson.*;
import java.io.PrintStream;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.resources.*;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.flag.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.biome.*;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.levelgen.LegacyRandomSource;

/** Executes actual NaturalSpawner population, actual registered plains spawn
 * settings, placement predicates and EntityType feature gating. Controlled
 * collision/light/terrain inputs isolate the three early-continue boundaries;
 * no population algorithm, RNG or entity constructor is replaced. */
public final class GenerationPopulationTraceProbe {
    static final sun.misc.Unsafe UNSAFE;
    static { try { var f=sun.misc.Unsafe.class.getDeclaredField("theUnsafe"); f.setAccessible(true); UNSAFE=(sun.misc.Unsafe)f.get(null); }
        catch(Exception e) { throw new ExceptionInInitializerError(e); } }
    static final class LevelShell extends ServerLevel {
        FeatureFlagSet features; GameRules rules;
        private LevelShell() { super(null,null,null,null,null,null,false,0L,List.of(),false,null,null,null,null,null,null); }
        @Override public FeatureFlagSet enabledFeatures() { return features; }
        @Override public GameRules getGameRules() { return rules; }
        @Override public int getNextEntityId() { throw new AssertionError("a gate case unexpectedly constructed an entity"); }
    }
    static void run(PrintStream out, RegistryAccess access, String mode, long seed) throws Exception {
        run(out,access,mode,seed,0,0);
    }
    static void run(PrintStream out, RegistryAccess access, String mode, long seed, int chunkX, int chunkZ) throws Exception {
        ServerLevel level;
        if(mode.equals("positive_sheep")) {
            GenerationSheepProbe.access=access;level=GenerationSheepProbe.level();
            var rules=ServerLevel.class.getDeclaredField("gameRules");rules.setAccessible(true);rules.set(level,new GameRules(FeatureFlags.DEFAULT_FLAGS));
        } else {
            var shell=(LevelShell)UNSAFE.allocateInstance(LevelShell.class);
            shell.features=mode.equals("feature_gate")?FeatureFlagSet.of():FeatureFlags.DEFAULT_FLAGS;
            shell.rules=new GameRules(FeatureFlags.DEFAULT_FLAGS);level=shell;
        }
        var entityRandom=new ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom(42);
        net.minecraft.world.entity.Entity.SHARED_RANDOM=entityRandom;
        var biome=access.lookupOrThrow(Registries.BIOME).getOrThrow(ResourceKey.create(Registries.BIOME,Identifier.withDefaultNamespace("plains")));
        var dimension=access.lookupOrThrow(Registries.DIMENSION_TYPE).getOrThrow(ResourceKey.create(Registries.DIMENSION_TYPE,Identifier.withDefaultNamespace("overworld"))).value();
        var random=new LegacyRandomSource(seed);
        net.minecraft.util.RandomSource levelRandom=mode.equals("positive_sheep")
            ?net.minecraft.world.level.levelgen.RandomState.create(access,net.minecraft.world.level.levelgen.NoiseGeneratorSettings.OVERWORLD,42)
                .getOrCreateRandomFactory(Identifier.withDefaultNamespace("worldgen_region_random")).at(new BlockPos(chunkX*16,0,chunkZ*16))
            :new LegacyRandomSource(99);
        var border=new WorldBorder(); border.applyInitialSettings(0);
        var positions=new JsonArray();var entities=new JsonArray();int[] collisions={0},brightness={0};
        var region=(WorldGenLevel)Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(),new Class[]{WorldGenLevel.class},(p,m,a)->switch(m.getName()) {
            case "getLevel","getMinecraftWorld" -> level;
            case "getHeight" -> { var point=new JsonArray();point.add((int)a[1]);point.add((int)a[2]);positions.add(point);yield 64; }
            case "dimensionType" -> dimension;
            case "getWorldBorder" -> border;
            case "getMinY" -> -64;
            case "getMaxY" -> 319;
            case "getBlockState" -> {var pos=(BlockPos)a[0];yield (pos.getY()<64&&!mode.equals("position_reject")?Blocks.GRASS_BLOCK:Blocks.AIR).defaultBlockState();}
            case "getFluidState" -> net.minecraft.world.level.material.Fluids.EMPTY.defaultFluidState();
            case "noCollision" -> {collisions[0]++;yield !mode.equals("collision_reject");}
            case "getRawBrightness" -> {brightness[0]++;yield mode.equals("rule_reject")?0:15;}
            case "getRandom" -> levelRandom;
            case "getDifficulty" -> net.minecraft.world.Difficulty.NORMAL;
            case "registryAccess" -> access;
            case "getBiome" -> biome;
            case "containsAnyLiquid" -> false;
            case "isUnobstructed" -> true;
            case "getCurrentDifficultyAt" -> new net.minecraft.world.DifficultyInstance(net.minecraft.world.Difficulty.NORMAL,0,0,0);
            case "addFreshEntityWithPassengers" -> {
                var entity=(net.minecraft.world.entity.Entity)a[0];
                if(entity.getType()!=net.minecraft.world.entity.EntityTypes.SHEEP)throw new IllegalStateException("non-sheep population");
                var save=net.minecraft.world.level.storage.TagValueOutput.createWithContext(net.minecraft.util.ProblemReporter.DISCARDING,access);
                if(!entity.save(save))throw new AssertionError("fresh sheep save failed");
                entities.add(save.buildResult().toString());yield null;
            }
            default -> throw new UnsupportedOperationException(m.toString());
        });
        net.minecraft.world.level.NaturalSpawner.spawnMobsForChunkGeneration(region,biome,new ChunkPos(chunkX,chunkZ),random);
        var row=new JsonObject();row.addProperty("mode",mode);row.addProperty("seed",seed);row.add("positions",positions);
        row.addProperty("collision_calls",collisions[0]);row.addProperty("brightness_calls",brightness[0]);
        row.addProperty("population_next",random.nextLong());row.addProperty("level_next",levelRandom.nextLong());out.println(row);
        if(mode.equals("positive_sheep")) {
            // Emit the complete population instead of an invented expected
            // entity. Full tags include constructor/shared random outcomes.
            var positive=new JsonObject();positive.addProperty("mode","positive_result");positive.addProperty("chunk_x",chunkX);positive.addProperty("chunk_z",chunkZ);
            positive.addProperty("world_seed",42);positive.addProperty("population_seed",seed);positive.add("entities",entities);
            positive.addProperty("entity_next",entityRandom.nextLong());out.println(positive);
        }
    }
    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
        var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
        net.minecraft.server.MinecraftServer.configurePackRepository(packs,new WorldDataConfiguration(new DataPackConfig(List.of("vanilla"),List.of()),FeatureFlags.DEFAULT_FLAGS),true,false);
        try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var out=new PrintStream(args[0])) {
            var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();
            var pending=net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC));
            var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),pending);
            pending.forEach(load->load.apply());
            var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();
            var access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess().freeze();
            int captured=0;
            for(long seed=0;captured<4;seed++) if(new LegacyRandomSource(seed).nextFloat()<biomeProbability(access)) {
                for(String mode:List.of("position_reject","collision_reject","rule_reject","feature_gate"))run(out,access,mode,seed);
                captured++;
            }
            outer: for(int x=-8;x<=8;x++) for(int z=-8;z<=8;z++) {
                var probeRandom=new net.minecraft.world.level.levelgen.WorldgenRandom(new LegacyRandomSource(0));
                long seed=probeRandom.setDecorationSeed(42,x*16,z*16);
                if(probeRandom.nextFloat()>=biomeProbability(access))continue;
                var mobs=access.lookupOrThrow(Registries.BIOME).getOrThrow(ResourceKey.create(Registries.BIOME,Identifier.withDefaultNamespace("plains"))).value().getMobSettings().getMobs(net.minecraft.world.entity.MobCategory.CREATURE);
                if(mobs.getRandom(probeRandom).orElseThrow().type()!=net.minecraft.world.entity.EntityTypes.SHEEP)continue;
                run(out,access,"positive_sheep",seed,x,z);break outer;
            }
        }
    }
    static float biomeProbability(RegistryAccess access) {
        return access.lookupOrThrow(Registries.BIOME).getOrThrow(ResourceKey.create(Registries.BIOME,Identifier.withDefaultNamespace("plains"))).value().getMobSettings().getCreatureProbability();
    }
}
