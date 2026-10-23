import com.google.gson.*;
import java.io.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.resources.*;
import net.minecraft.server.level.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.level.*;
import net.minecraft.nbt.*;
import ca.spottedleaf.moonrise.patches.chunk_system.level.entity.ChunkEntitySlices;

/** Genuine Moonrise saveEntityChunk0/readEntities/copyEntities,
 * with genuine entity constructors, load/save and complete runtime registries.
 * Getter-only level context does not emulate serialization or admission. */
public final class EntityRegionProbe {
 static final Gson G=new Gson();
 static final ChunkPos POS=new ChunkPos(-3,7);
 static String input(String family,int uuid,boolean persist){return "{id:'minecraft:"+family+"',UUID:[I;0,0,0,"+uuid+"],Pos:[-47.5d,64d,112.5d],Rotation:[90f,0f],EggLayTime:7000,Bukkit.persist:"+(persist?"1b":"0b")+"}";}
 static void save(PrintStream out,String name,List<String> inputs,Map<String,Integer> limits,boolean force)throws Exception{
  var world=GenerationSheepProbe.level();world.config.chunks=world.config.new Chunks();
  world.config.chunks.entityPerChunkSaveLimit.clear();for(var entry:limits.entrySet())world.config.chunks.entityPerChunkSaveLimit.put(BuiltInRegistries.ENTITY_TYPE.getValue(Identifier.parse(entry.getKey())),entry.getValue().intValue());
  Entity.SHARED_RANDOM=new ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom(42);
  var values=new ArrayList<Entity>();for(String snbt:inputs){var tag=TagParser.parseCompoundFully(snbt);var root=new CompoundTag();root.put("Entities",new ListTag(List.of(tag)));values.addAll(ChunkEntitySlices.readEntities(world,POS,root));}
  var saved=ChunkEntitySlices.saveEntityChunk0(values,POS,world,force);
  var row=new JsonObject();row.addProperty("mode","save");row.addProperty("name",name);row.add("inputs",G.toJsonTree(inputs));row.add("limits",G.toJsonTree(new TreeMap<>(limits)));row.addProperty("force",force);row.addProperty("saved",saved==null?null:saved.toString());out.println(row);
 }
 static void read(PrintStream out,String name,String input)throws Exception{
  read(out,name,input,Map.of());
 }
 static void read(PrintStream out,String name,String input,Map<String,Integer> limits)throws Exception{
  var world=GenerationSheepProbe.level();world.config.chunks=world.config.new Chunks();world.config.chunks.entityPerChunkSaveLimit.clear();for(var entry:limits.entrySet())world.config.chunks.entityPerChunkSaveLimit.put(BuiltInRegistries.ENTITY_TYPE.getValue(Identifier.parse(entry.getKey())),entry.getValue().intValue());Entity.SHARED_RANDOM=new ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom(42);
  var values=ChunkEntitySlices.readEntities(world,POS,TagParser.parseCompoundFully(input));var saved=new JsonArray();for(var entity:values){var tag=net.minecraft.world.level.storage.TagValueOutput.createWithContext(net.minecraft.util.ProblemReporter.DISCARDING,GenerationSheepProbe.access);if(!entity.save(tag))throw new AssertionError("unexpected save decline");saved.add(tag.buildResult().toString());}
  var row=new JsonObject();row.addProperty("mode","read");row.addProperty("name",name);row.addProperty("input",input);row.add("limits",G.toJsonTree(new TreeMap<>(limits)));row.addProperty("next_id",world.nextId);row.add("saved",saved);if(name.startsWith("root_limit_"))row.addProperty("rng_next",Entity.SHARED_RANDOM.nextLong());out.println(row);
 }
 static void copy(PrintStream out,String from,String into)throws Exception{
  var source=from==null?null:TagParser.parseCompoundFully(from);var target=TagParser.parseCompoundFully(into);ChunkEntitySlices.copyEntities(source,target);
  var row=new JsonObject();row.addProperty("mode","copy");row.addProperty("from",from);row.addProperty("into",into);row.addProperty("saved",target.toString());out.println(row);
 }
 public static void main(String[] args)throws Exception{
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();net.minecraft.server.MinecraftServer.configurePackRepository(packs,WorldDataConfiguration.DEFAULT,true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var out=new PrintStream(args[0])){
   var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();GenerationSheepProbe.access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess().freeze();
   var all=List.of(input("sheep",1,true),input("cow",2,true),input("pig",3,true),input("chicken",4,true),input("chest_minecart",5,true));
   save(out,"five_native_families",all,Map.of(),false);save(out,"forced_empty",List.of(),Map.of(),true);save(out,"empty_delete",List.of(),Map.of(),false);
   save(out,"nonpersistent_consumes_limit",List.of(input("sheep",1,false),input("sheep",2,true),input("cow",3,true)),Map.of("minecraft:sheep",1),false);
   save(out,"per_type_limits",List.of(all.get(0),input("sheep",6,true),all.get(1),all.get(2)),Map.of("minecraft:sheep",1,"minecraft:cow",0,"minecraft:pig",-2),false);
   save(out,"all_decline",List.of(input("sheep",1,false)),Map.of(),false);
   read(out,"position_mismatch_is_not_checked","{Position:[I;99,99],Entities:["+all.get(0)+","+all.get(4)+"]}");
   read(out,"duplicate_roots_retained_until_admission","{Entities:["+all.get(0)+","+all.get(0)+"]}");
   read(out,"unknown_entity_skipped","{Entities:[{id:'minecraft:not_registered'},"+all.get(1)+"]}");
   read(out,"wrong_entities_type_empty","{Entities:17}");
   read(out,"load_limit_still_constructs","{Entities:["+all.get(0)+","+all.get(0)+","+all.get(1)+"]}",Map.of("minecraft:sheep",0));
   copy(out,"{Entities:[{id:'old:unknown',custom:42}]}","{DataVersion:4903,Entities:[{id:'new:native'}],Position:[I;-3,7]}");
   copy(out,"{Entities:[{old:1}]}","{Entities:3}");copy(out,null,"{Entities:[{id:'kept'}]}");
   String passengerRoot=all.get(0).substring(0,all.get(0).length()-1)+",Passengers:[{id:'minecraft:zombie'}]}";
   read(out,"root_limit_zero_skips_passengers","{Entities:["+passengerRoot+"]}",Map.of("minecraft:sheep",0));
   read(out,"root_limit_exhausted_skips_passengers","{Entities:["+all.get(0)+","+passengerRoot+"]}",Map.of("minecraft:sheep",1));
  }
 }
}
