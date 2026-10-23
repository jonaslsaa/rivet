import com.google.gson.*;
import java.io.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.nbt.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.level.storage.TagValueOutput;

/** Actual loadEntityRecursive constructors + Entity/Living/Mob/Animal and
 * registered variant load chains, not NBT echoing or replacement algorithms. */
public final class GenerationCreatureLoadProbe {
 static void run(PrintStream out,String name,String input)throws Exception {
  var level=GenerationSheepProbe.level();var random=new ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom(42);Entity.SHARED_RANDOM=random;
  var callbacks=new JsonArray();
  var entity=EntityType.loadEntityRecursive(TagParser.parseCompoundFully(input),level,new EntitySpawnRequest(EntitySpawnReason.LOAD,false),loaded->{var c=new JsonObject();c.addProperty("id",loaded.getId());c.addProperty("uuid",loaded.getUUID().toString());c.addProperty("generation",loaded.generation);callbacks.add(c);return loaded;});
  var row=new JsonObject();row.addProperty("name",name);row.addProperty("input",input);row.add("callbacks",callbacks);row.addProperty("next_id",level.nextId);row.addProperty("rng_next",random.nextLong());
  if(entity!=null){var output=TagValueOutput.createWithContext(net.minecraft.util.ProblemReporter.DISCARDING,level.registryAccess());row.addProperty("save_result",entity.save(output));row.addProperty("saved",output.buildResult().toString());row.add("old_pos",new Gson().toJsonTree(new double[]{entity.xo,entity.yo,entity.zo}));}
  out.println(row);
 }
 public static void main(String[] args)throws Exception {
  System.setProperty("paper.debugEntitiesWithInvalidIds","true");
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
  var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();net.minecraft.server.MinecraftServer.configurePackRepository(packs,net.minecraft.world.level.WorldDataConfiguration.DEFAULT,true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var out=new PrintStream(args[0])){
   var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
   var worldgen=net.minecraft.resources.RegistryDataLoader.load(resources,lookups,net.minecraft.resources.RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();GenerationSheepProbe.access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess().freeze();
   for(String file:List.of("generation-sheep.jsonl","generation-farm-animals.jsonl")){int index=0;for(String line:Files.readAllLines(Path.of(args[1],file))){var data=JsonParser.parseString(line).getAsJsonObject();run(out,file+":"+index++,data.get("saved").getAsString());}}
   for(String family:List.of("sheep","cow","pig","chicken")){
    String id="id:\"minecraft:"+family+"\"";
    run(out,family+"_empty","{"+id+"}");
    run(out,family+"_primitives","{"+id+",Pos:[-4.25d,64.5d,2.75d],Motion:[10.1d,-10d,1.25d],Rotation:[725f,275f],UUID:[I;1,2,3,4],Health:999f,HurtTime:2s,DeathTime:3s,AbsorptionAmount:4f,Age:-100,ForcedAge:17,AgeLocked:1b,InLove:41,CanPickUpLoot:1b,PersistenceRequired:1b,LeftHanded:1b,NoAI:1b,Bukkit.Aware:0b,Sheared:1b,Color:14b,IsChickenJockey:1b,EggLayTime:77,variant:\"minecraft:warm\",sound_variant:\"minecraft:classic\"}");
    run(out,family+"_malformed","{"+id+",Pos:[1d,\"bad\",2d,3d,4d],Rotation:[\"bad\",-725f,-275f],UUID:[I;1,2,3],Health:-9f,Age:\"bad\",variant:\"minecraft:missing\",sound_variant:\"missing\",Color:99b,Paper.SpawnReason:\"nonsense\"}");
    run(out,family+"_metadata","{"+id+",Silent:1b,NoGravity:1b,Glowing:1b,CustomNameVisible:1b,Air:40000,Bukkit.MaxAirSupply:400,Bukkit.visibleByDefault:0b,Bukkit.invisible:1b,Paper.FireOverride:\"FALSE\",Paper.FrictionState:\"TRUE\",TicksFrozen:17,Spigot.ticksLived:19,data:{x:1},BukkitValues:{\"test:x\":2},Paper.Origin:[1d,2d,3d],Paper.FromMobSpawner:1b,Paper.FromNetherPortal:1b,Paper.FreezeLock:1b}");
    run(out,family+"_persist_false","{"+id+",Bukkit.persist:0b}");
   }
   run(out,"invalid_id","{id:\"minecraft:missing\"}");
   run(out,"sheep_fractional_color","{id:\"minecraft:sheep\",Color:-254.5f}");
  }
 }
}
