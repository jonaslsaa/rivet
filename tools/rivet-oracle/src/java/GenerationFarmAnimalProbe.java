import com.google.gson.*;
import com.mojang.serialization.*;
import java.io.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.resources.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.animal.cow.*;
import net.minecraft.world.entity.animal.pig.*;
import net.minecraft.world.entity.animal.chicken.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.storage.TagValueOutput;

/** Actual registered variant values/order and real EntityType constructors,
 * finalizeSpawn and complete saves. Only getter world context is shelled. */
public final class GenerationFarmAnimalProbe {
 static RegistryAccess access;
 static <T> void registry(PrintStream out,ResourceKey<Registry<T>> key,Codec<T> codec) {
  var row=new JsonObject();row.addProperty("registry",key.identifier().toString());var entries=new JsonArray();
  access.lookupOrThrow(key).listElements().forEach(holder->{var entry=new JsonObject();entry.addProperty("key",holder.key().identifier().toString());entry.add("value",codec.encodeStart(RegistryOps.create(JsonOps.INSTANCE,access),holder.value()).getOrThrow());entries.add(entry);});row.add("entries",entries);out.println(row);
 }
 static void run(PrintStream out,String family,String biomeName,long seed,boolean finalize)throws Exception {
  var level=GenerationSheepProbe.level();var entityRandom=new ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom(42);Entity.SHARED_RANDOM=entityRandom;
  var random=new LegacyRandomSource(seed);var biome=access.lookupOrThrow(Registries.BIOME).getOrThrow(ResourceKey.create(Registries.BIOME,Identifier.withDefaultNamespace(biomeName)));
  var region=(ServerLevelAccessor)java.lang.reflect.Proxy.newProxyInstance(ServerLevelAccessor.class.getClassLoader(),new Class[]{ServerLevelAccessor.class},(p,m,a)->switch(m.getName()){
   case "getRandom"->random;case "getBiome"->biome;case "getLevel","getMinecraftWorld"->level;case "registryAccess"->access;case "getBlockState"->Blocks.GRASS_BLOCK.defaultBlockState();
   // Pinned farm selectors only read biomes; fail if that ceases to be true.
   case "environmentAttributes"->java.lang.reflect.Proxy.newProxyInstance(net.minecraft.world.attribute.EnvironmentAttributeReader.class.getClassLoader(),new Class[]{net.minecraft.world.attribute.EnvironmentAttributeReader.class},(ep,em,ea)->{throw new UnsupportedOperationException(em.toString());});
   default->throw new UnsupportedOperationException(m.toString());
  });
  SpawnGroupData group=null;
  for(int i=0;i<(finalize?4:1);i++) {
   var type=switch(family){case "cow"->EntityTypes.COW;case "pig"->EntityTypes.PIG;case "chicken"->EntityTypes.CHICKEN;default->throw new AssertionError(family);};
   var mob=(Mob)type.create(level,EntitySpawnReason.NATURAL);if(mob==null)throw new AssertionError("enabled animal refused");
   if(finalize){mob.snapTo(8.5,64,8.5,123,0);group=mob.finalizeSpawn(region,new net.minecraft.world.DifficultyInstance(net.minecraft.world.Difficulty.NORMAL,0,0,0),EntitySpawnReason.CHUNK_GENERATION,group);}
   var output=TagValueOutput.createWithContext(net.minecraft.util.ProblemReporter.DISCARDING,access);if(!mob.save(output))throw new AssertionError("save refused");
   var row=new JsonObject();row.addProperty("family",family);row.addProperty("biome",biomeName);row.addProperty("seed",seed);row.addProperty("finalize",finalize);row.addProperty("index",i);row.addProperty("saved",output.buildResult().toString());
   var ec=new ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom(0);var ef=entityRandom.getClass().getDeclaredField("value");ef.setAccessible(true);ef.set(ec,ef.get(entityRandom));row.addProperty("entity_rng_next",ec.nextLong());
   var lc=new LegacyRandomSource(0);var lf=LegacyRandomSource.class.getDeclaredField("seed");lf.setAccessible(true);((java.util.concurrent.atomic.AtomicLong)lf.get(lc)).set(((java.util.concurrent.atomic.AtomicLong)lf.get(random)).get());row.addProperty("level_rng_next",lc.nextLong());out.println(row);
  }
 }
 public static void main(String[] args)throws Exception {
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
  var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();net.minecraft.server.MinecraftServer.configurePackRepository(packs,net.minecraft.world.level.WorldDataConfiguration.DEFAULT,true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var out=new PrintStream(args[0]);var registries=new PrintStream(args[1])) {
   var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
   var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess().freeze();GenerationSheepProbe.access=access;
   registry(registries,Registries.COW_VARIANT,CowVariant.DIRECT_CODEC);registry(registries,Registries.PIG_VARIANT,PigVariant.DIRECT_CODEC);registry(registries,Registries.CHICKEN_VARIANT,ChickenVariant.DIRECT_CODEC);
   registry(registries,Registries.COW_SOUND_VARIANT,CowSoundVariant.DIRECT_CODEC);registry(registries,Registries.PIG_SOUND_VARIANT,PigSoundVariant.DIRECT_CODEC);registry(registries,Registries.CHICKEN_SOUND_VARIANT,ChickenSoundVariant.DIRECT_CODEC);
   for(String family:List.of("cow","pig","chicken")){run(out,family,"plains",0,false);for(String biome:List.of("plains","desert","snowy_plains"))for(long seed:new long[]{0,1,42})run(out,family,biome,seed,true);}
  }
 }
}
