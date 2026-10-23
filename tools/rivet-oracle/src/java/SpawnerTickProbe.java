import com.google.gson.*;
import java.io.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.nbt.*;
import net.minecraft.server.*;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.util.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.storage.*;
import net.minecraft.world.phys.AABB;
import io.papermc.paper.configuration.WorldConfiguration;

/** Actual BaseSpawner.serverTick with explicit world getter gates. Collision
 * is always blocked, so no entity/event authority is fabricated. Actual load,
 * negative/reset delay, rate throttle, weighted choice, coordinate RNG and save
 * run unchanged. Abstract broadcast and setNext notifications are recorded. */
public final class SpawnerTickProbe {
 static final class World extends ServerLevel {
  WorldConfiguration config; LegacyRandomSource rng; boolean enabled; int collisions;
  private World(){super(null,null,null,null,null,null,false,0L,List.of(),false,null,null,null,null,null,null);}
  @Override public WorldConfiguration paperConfig(){return config;}
  @Override public RandomSource getRandom(){return rng;}
  @Override public boolean isSpawnerBlockEnabled(){return enabled;}
  @Override public boolean noCollision(AABB bounds){collisions++;return false;}
  @Override public RegistryAccess registryAccess(){return RegistryLayer.createRegistryAccess().getLayer(RegistryLayer.STATIC);}
 }
 static final class Spawner extends BaseSpawner {
  boolean near;int nearCalls;List<String> trace=new ArrayList<>();
  @Override public boolean isNearPlayer(Level level,BlockPos pos){nearCalls++;return near;}
  @Override public void broadcastEvent(Level level,BlockPos pos,int id){trace.add("broadcast:"+id);}
  @Override public void setNextSpawnData(Level level,BlockPos pos,SpawnData data){super.setNextSpawnData(level,pos,data);trace.add("next");}
 }
 static World world(int rate,boolean enabled)throws Exception{
  World world=(World)ChestMinecartProbe.UNSAFE.allocateInstance(World.class);
  world.config=ChestMinecartProbe.level(false).paperConfig();world.config.tickRates=world.config.new TickRates();world.config.tickRates.mobSpawner=rate;
  world.rng=new LegacyRandomSource(99);world.enabled=enabled;
  var randomField=Level.class.getDeclaredField("random");randomField.setAccessible(true);randomField.set(world,world.rng);
  return world;
 }
 static void run(PrintWriter output,int rate,boolean near,boolean enabled,String input,int ticks)throws Exception{
  World world=world(rate,enabled);
  var pos=new BlockPos(3,64,-8);var registries=world.registryAccess();var spawner=new Spawner();spawner.near=near;
  spawner.load(null,pos,TagValueInput.create(ProblemReporter.DISCARDING,registries,TagParser.parseCompoundFully(input)));spawner.trace.clear();
  for(int i=0;i<ticks;i++)spawner.serverTick(world,pos);
  var tag=TagValueOutput.createWithContext(ProblemReporter.DISCARDING,registries);spawner.save(tag);
  var counter=BaseSpawner.class.getDeclaredField("tickDelay");counter.setAccessible(true);
  var row=new JsonObject();row.addProperty("rate",rate);row.addProperty("near",near);row.addProperty("enabled",enabled);row.addProperty("ticks",ticks);row.addProperty("input_snbt",input);
  row.addProperty("output_snbt",tag.buildResult().toString());row.addProperty("tick_delay",counter.getInt(spawner));row.addProperty("near_calls",spawner.nearCalls);row.addProperty("collisions",world.collisions);row.add("trace",new Gson().toJsonTree(spawner.trace));row.addProperty("rng_next",world.rng.nextLong());output.println(row);
 }
 static void reload(PrintWriter output,String reload)throws Exception{
  World world=world(4,true);var pos=new BlockPos(3,64,-8);var registries=world.registryAccess();var spawner=new Spawner();spawner.near=false;
  String initial="{Delay:20s,SpawnData:{entity:{id:'minecraft:skeleton',marker:7}}}";
  spawner.load(null,pos,TagValueInput.create(ProblemReporter.DISCARDING,registries,TagParser.parseCompoundFully(initial)));
  spawner.serverTick(world,pos);spawner.trace.clear();
  var counter=BaseSpawner.class.getDeclaredField("tickDelay");counter.setAccessible(true);int before=counter.getInt(spawner);
  spawner.load(null,pos,TagValueInput.create(ProblemReporter.DISCARDING,registries,TagParser.parseCompoundFully(reload)));
  spawner.serverTick(world,pos);
  var tag=TagValueOutput.createWithContext(ProblemReporter.DISCARDING,registries);spawner.save(tag);
  var row=new JsonObject();row.addProperty("initial_snbt",initial);row.addProperty("reload_snbt",reload);row.addProperty("before_counter",before);row.addProperty("after_counter",counter.getInt(spawner));row.addProperty("near_calls",spawner.nearCalls);row.addProperty("rng_next",world.rng.nextLong());row.addProperty("output_snbt",tag.buildResult().toString());row.add("trace",new Gson().toJsonTree(spawner.trace));output.println(row);
 }
 public static void main(String[] args)throws Exception{
  SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
  try(var output=new PrintWriter(args[0])){
   if(args.length>1&&args[1].equals("reload")){
    reload(output,"{Delay:20s,SpawnData:{entity:{id:'minecraft:zombie'}}}");
    reload(output,"{Delay:20s}");
    reload(output,"{Delay:20s,SpawnData:{entity:'malformed'}}");
    return;
   }
   for(int rate:new int[]{-1,0,1,4})for(boolean near:new boolean[]{false,true})for(boolean enabled:new boolean[]{false,true})for(String input:List.of(
    "{Delay:20s,SpawnCount:2s,SpawnData:{entity:{id:'minecraft:pig'}}}",
    "{Delay:-5s,MinSpawnDelay:2s,MaxSpawnDelay:7s,SpawnCount:2s,SpawnData:{entity:{id:'invalid:missing'}}}",
    "{Delay:0s,SpawnCount:2s,SpawnData:{entity:{id:'minecraft:pig'}}}"))run(output,rate,near,enabled,input,8);
   run(output,1,true,true,"{Delay:0s,SpawnCount:0s}",8);
   run(output,1,true,true,"{Delay:0s,MaxNearbyEntities:0s}",8);
   run(output,1,true,true,"{Delay:0s,MinSpawnDelay:7s,MaxSpawnDelay:7s,SpawnData:{entity:{id:'invalid:missing'}},SpawnPotentials:[{weight:1,data:{entity:{id:'invalid:first'}}},{weight:3,data:{entity:{id:'invalid:second'}}}]}",12);
   run(output,1,true,true,"{Delay:0s,SpawnCount:2s,SpawnData:{entity:{id:'minecraft:pig',Pos:[1d,2d,3d]}}}",3);
   run(output,1,true,true,"{Delay:0s,SpawnCount:2s,SpawnData:{entity:{id:'minecraft:pig',Pos:[1d,2d,3d,4d]}}}",3);
  }
 }
}
