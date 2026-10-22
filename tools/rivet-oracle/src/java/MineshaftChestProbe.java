import com.google.gson.*;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.nbt.*;
import net.minecraft.resources.*;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.level.*;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.levelgen.structure.*;

/** Genuine positive corridor branch, real chest factory/constructors/loot/save,
 * map-backed world storage and actual ProtoChunk.addEntity. The level getter
 * shell supplies identity/config only; plugins and live ticking are not tested. */
public final class MineshaftChestProbe {
 static JsonObject run()throws Exception{
  var input=MineshaftPlacementProbe.pieceTag("mscorridor",new int[]{0,50,0,2,52,9},0,0);
  input.putInt("Num",2);input.putBoolean("hr",false);input.putBoolean("sc",false);input.putBoolean("hps",false);
  var piece=MineshaftPlacementProbe.piece(input);var ground=new MineshaftPlacementProbe.Ground("stone");
  var level=ChestMinecartProbe.level(false);var entityRandom=new ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom(42);Entity.SHARED_RANDOM=entityRandom;
  var delegate=ground.level();var admitted=new ArrayList<CompoundTag>();
  var storage=(WorldGenLevel)Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(),new Class<?>[]{WorldGenLevel.class},(proxy,method,args)->{
   if(method.getName().equals("getLevel"))return level;
   if(method.getName().equals("addFreshEntity")){
    var entity=(Entity)args[0];var chunk=(ProtoChunk)delegate.getChunk(entity.getBlockX()>>4,entity.getBlockZ()>>4);
    chunk.addEntity(entity);admitted.add(chunk.getEntities().getLast().copy());return true;
   }
   try{return method.invoke(delegate,args);}catch(InvocationTargetException e){throw e.getCause();}
  });
  var random=new LegacyRandomSource(23);var clip=new BoundingBox(-32,-64,-32,32,319,32);
  piece.postProcess(storage,null,null,random,clip,ChunkPos.ZERO,BlockPos.ZERO);
  if(admitted.isEmpty())throw new AssertionError("positive chest branch not reached");
  var row=new JsonObject();row.addProperty("seed",23);row.addProperty("entity_seed",42);row.addProperty("input",MineshaftPlacementProbe.piece(input).createTag(null).toString());row.addProperty("terrain","stone");
  row.add("clip",new Gson().toJsonTree(new int[]{-32,-64,-32,32,319,32}));
  row.addProperty("writes",ground.writeCount);row.addProperty("writes_sha256",MineshaftPlacementProbe.hash(ground.writes.toByteArray()));row.addProperty("states",ground.states.size());row.addProperty("states_sha256",ground.statesHash());
  row.addProperty("marks",ground.markCount);row.addProperty("marks_sha256",MineshaftPlacementProbe.hash(ground.marks.toByteArray()));row.addProperty("ticks",ground.ticks.size());row.addProperty("ticks_sha256",MineshaftPlacementProbe.hash(String.join("\n",ground.ticks).getBytes(java.nio.charset.StandardCharsets.UTF_8)));
  row.addProperty("rng_next",random.nextLong());row.addProperty("entity_rng_next",entityRandom.nextLong());row.addProperty("saved",piece.createTag(null).toString());
  var entities=new JsonArray();for(var tag:admitted)entities.add(tag.toString());row.add("entities",entities);return row;
 }
 public static void main(String[] args)throws Exception{
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
  var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
  net.minecraft.server.MinecraftServer.configurePackRepository(packs,new WorldDataConfiguration(new DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var output=new PrintStream(args[0])){
   var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
   var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();
   MineshaftPlacementProbe.access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess();MineshaftPlacementProbe.factory=net.minecraft.world.level.chunk.PalettedContainerFactory.create(MineshaftPlacementProbe.access);
   output.println(run());
  }
 }
}
