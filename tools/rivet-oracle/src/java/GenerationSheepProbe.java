import com.google.gson.*;
import java.io.PrintStream;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.resources.*;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.animal.sheep.Sheep;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.storage.TagValueOutput;
import io.papermc.paper.configuration.WorldConfiguration;

/** Real Sheep constructor/finalize/fullsave over actual vanilla biome registry
 * and exact two independent entity/level random sources. World shells expose
 * only constructor metadata; no Entity/Living/Mob algorithms are replaced. */
public final class GenerationSheepProbe {
 static final sun.misc.Unsafe U;
 static {try{var f=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);U=(sun.misc.Unsafe)f.get(null);}catch(Exception e){throw new ExceptionInInitializerError(e);}}
 static RegistryAccess access;
 static final class LevelShell extends ServerLevel {
  WorldConfiguration config;ChestMinecartProbe.WorldShell world;int nextId;net.minecraft.server.MinecraftServer server;
  private LevelShell(){super(null,null,null,null,null,null,false,0L,List.of(),false,null,null,null,null,null,null);}
  @Override public int getNextEntityId(){return ++nextId;}
  @Override public net.minecraft.world.flag.FeatureFlagSet enabledFeatures(){return net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS;}
  @Override public WorldConfiguration paperConfig(){return config;}
  @Override public org.bukkit.craftbukkit.CraftWorld getWorld(){return world;}
  @Override public RegistryAccess registryAccess(){return access;}
  @Override public net.minecraft.server.MinecraftServer getServer(){return server;}
  @Override public String toString(){return "ServerLevel[GenerationCreatureOracle]";}
 }
 static LevelShell level()throws Exception{
  var level=(LevelShell)U.allocateInstance(LevelShell.class);level.nextId=100;level.world=(ChestMinecartProbe.WorldShell)U.allocateInstance(ChestMinecartProbe.WorldShell.class);level.server=(net.minecraft.server.MinecraftServer)U.allocateInstance(net.minecraft.server.dedicated.DedicatedServer.class);
  var spigot=(org.spigotmc.SpigotWorldConfig)U.allocateInstance(org.spigotmc.SpigotWorldConfig.class);var field=Level.class.getDeclaredField("spigotConfig");field.setAccessible(true);field.set(level,spigot);
  var ctor=WorldConfiguration.class.getDeclaredConstructor(org.spigotmc.SpigotWorldConfig.class,Identifier.class);ctor.setAccessible(true);level.config=ctor.newInstance(spigot,Identifier.withDefaultNamespace("overworld"));level.config.entities=level.config.new Entities();level.config.entities.spawning=level.config.entities.new Spawning();level.config.entities.behavior=level.config.entities.new Behavior();return level;
 }
 static void run(PrintStream out,String biomeName,long seed,boolean finalize)throws Exception{
  var level=level();var entityRandom=new ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom(42);Entity.SHARED_RANDOM=entityRandom;
  var random=new LegacyRandomSource(seed);var biome=access.lookupOrThrow(Registries.BIOME).getOrThrow(ResourceKey.create(Registries.BIOME,Identifier.withDefaultNamespace(biomeName)));
  var region=(ServerLevelAccessor)Proxy.newProxyInstance(ServerLevelAccessor.class.getClassLoader(),new Class[]{ServerLevelAccessor.class},(p,m,a)->switch(m.getName()){
    case "getRandom"->random;case "getBiome"->biome;case "getLevel","getMinecraftWorld"->level;case "registryAccess"->access;case "getBlockState"->Blocks.GRASS_BLOCK.defaultBlockState();default->throw new UnsupportedOperationException(m.toString());
  });
  SpawnGroupData group=null;
  for(int i=0;i<(finalize?4:1);i++){
   var sheep=EntityTypes.SHEEP.create(level,EntitySpawnReason.NATURAL);if(sheep==null)throw new AssertionError("ordinary enabled sheep refused");
   if(finalize){sheep.snapTo(8.5,64.0,8.5,123.0F,0.0F);group=sheep.finalizeSpawn(region,new net.minecraft.world.DifficultyInstance(net.minecraft.world.Difficulty.NORMAL,0,0,0),EntitySpawnReason.CHUNK_GENERATION,group);}
   var output=TagValueOutput.createWithContext(net.minecraft.util.ProblemReporter.DISCARDING,access);if(!sheep.save(output))throw new AssertionError("fresh sheep save refused");
   var row=new JsonObject();row.addProperty("biome",biomeName);row.addProperty("seed",seed);row.addProperty("finalize",finalize);row.addProperty("index",i);row.addProperty("id",sheep.getId());row.addProperty("saved",output.buildResult().toString());row.addProperty("baby",sheep.isBaby());
   // Peek independent continuation without changing either stream's next row.
   var entityClone=new ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom(0);entityClone.setSeed(0);var sf=entityRandom.getClass().getDeclaredField("value");sf.setAccessible(true);sf.set(entityClone,sf.get(entityRandom));row.addProperty("entity_rng_next",entityClone.nextLong());
   var levelClone=new LegacyRandomSource(0);levelClone.setSeed(0);var lf=LegacyRandomSource.class.getDeclaredField("seed");lf.setAccessible(true);((java.util.concurrent.atomic.AtomicLong)lf.get(levelClone)).set(((java.util.concurrent.atomic.AtomicLong)lf.get(random)).get());row.addProperty("level_rng_next",levelClone.nextLong());out.println(row);
  }
 }
 public static void main(String[] args)throws Exception{
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
  var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();net.minecraft.server.MinecraftServer.configurePackRepository(packs,new WorldDataConfiguration(new DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var out=new PrintStream(args[0])){
   var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
   var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess().freeze();
   run(out,"plains",0,false);for(String biome:List.of("plains","desert","snowy_plains"))for(long seed:new long[]{0,1,42})run(out,biome,seed,true);
  }
 }
}
