import com.google.gson.*;
import java.io.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.resources.*;
import net.minecraft.server.level.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.entity.*;
import ca.spottedleaf.moonrise.patches.chunk_system.entity.ChunkSystemEntity;
import ca.spottedleaf.moonrise.patches.chunk_system.level.entity.*;
import ca.spottedleaf.moonrise.patches.chunk_system.level.chunk.ChunkData;

/** Genuine Moonrise EntityLookup and ChunkEntitySlices, genuine native mobs.
 * Adapter supplies getter world/thread/ticket context and observable required
 * LevelCallbacks; it does not replace map admission or status algorithms. */
public final class EntityLookupProbe {
 static final Gson G=new Gson();
 static final class Stop extends RuntimeException{}
 static final class Lookup extends ca.spottedleaf.moonrise.patches.chunk_system.level.entity.EntityLookup {
  final Thread thread=Thread.currentThread();Boolean blocked;final LinkedHashMap<ChunkPos,ChunkEntitySlices> chunks=new LinkedHashMap<>();final ca.spottedleaf.moonrise.common.list.ReferenceList<Entity> tracked=new ca.spottedleaf.moonrise.common.list.ReferenceList<>();
  Lookup(Level level,LevelCallback<Entity> callback){super(level,callback);}
  protected Boolean blockTicketUpdates(){var old=blocked;blocked=true;return old;}protected void setBlockTicketUpdates(Boolean value){blocked=value;}
  protected void checkThread(int x,int z,String message){if(Thread.currentThread()!=thread)throw new AssertionError(message);}protected void checkThread(Entity e,String message){checkThread(0,0,message);}
  protected ChunkEntitySlices createEntityChunk(int x,int z,boolean transientChunk){var key=new ChunkPos(x,z);var existing=chunks.get(key);if(existing!=null)return existing;var chunk=new ChunkEntitySlices(world,x,z,FullChunkStatus.INACCESSIBLE,new ChunkData(),-4,19);chunk.setTransient(transientChunk);chunks.put(key,chunk);entitySectionLoad(x,z,chunk);return chunk;}
  protected void onEmptySlices(int x,int z){}protected void entitySectionChangeCallback(Entity e,int a,int b,int c,int d,int f,int g){}protected void addEntityCallback(Entity e){}protected void removeEntityCallback(Entity e){}
  protected void entityStartLoaded(Entity e){tracked.add(e);}protected void entityEndLoaded(Entity e){tracked.remove(e);}protected void entityStartTicking(Entity e){}protected void entityEndTicking(Entity e){}
  protected boolean screenEntity(Entity e,boolean disk,boolean event){return true;} // Actual PaperHooks.screenEntity is unconditional true.
  boolean admit(Entity e,boolean disk){return addEntity(e,disk,true);}Entity raw(int id){return entityById.get(id);}
 }
 static JsonArray a(Object... values){return G.toJsonTree(values).getAsJsonArray();}
 static JsonObject entity(String family,int id,int uuid,int x,int y,int z,boolean removed,boolean dead){var data=new JsonObject();data.addProperty("family",family);data.addProperty("id",id);data.addProperty("uuid",new UUID(0,uuid).toString());data.add("pos",a(x,y,z));data.addProperty("removed",removed);data.addProperty("dead",dead);return data;}
 static void run(PrintStream out,String name,List<JsonObject> entities,List<JsonArray> commands)throws Exception{
  var level=GenerationSheepProbe.level();
  var waypoint=ServerLevel.class.getDeclaredField("waypointManager");waypoint.setAccessible(true);waypoint.set(level,new net.minecraft.server.waypoints.ServerWaypointManager(level));
  for(var data:new Object[][]{{"minY",-64},{"height",384},{"maxY",319},{"minSectionY",-4},{"maxSectionY",19},{"sectionsCount",24}}){var f=Level.class.getDeclaredField((String)data[0]);f.setAccessible(true);f.set(level,data[1]);}
  var events=new JsonArray();String[] throwing={""};
  var callback=(LevelCallback<Entity>)java.lang.reflect.Proxy.newProxyInstance(LevelCallback.class.getClassLoader(),new Class[]{LevelCallback.class},(p,m,args)->{String event=switch(m.getName()){case "onCreated"->"Created";case "onDestroyed"->"Destroyed";case "onTickingStart"->"TickingStart";case "onTickingEnd"->"TickingEnd";case "onTrackingStart"->"TrackingStart";case "onTrackingEnd"->"TrackingEnd";case "onSectionChange"->"SectionChange";default->throw new AssertionError(m);};events.add(a(event,((Entity)args[0]).getId()));if(event.equals(throwing[0]))throw new Stop();return null;});
  var lookup=new Lookup(level,callback);((ca.spottedleaf.moonrise.patches.chunk_system.level.ChunkSystemLevel)level).moonrise$setEntityLookup(lookup);var values=new ArrayList<Entity>();
  for(var data:entities){var type=switch(data.get("family").getAsString()){case "sheep"->EntityTypes.SHEEP;case "cow"->EntityTypes.COW;case "pig"->EntityTypes.PIG;default->EntityTypes.CHICKEN;};var e=(Mob)type.create(level,EntitySpawnReason.LOAD);e.setId(data.get("id").getAsInt());e.setUUID(UUID.fromString(data.get("uuid").getAsString()));var pos=data.getAsJsonArray("pos");e.snapTo(pos.get(0).getAsDouble(),pos.get(1).getAsDouble(),pos.get(2).getAsDouble(),0,0);if(data.get("dead").getAsBoolean())e.setHealth(0);if(data.get("removed").getAsBoolean())e.discard();values.add(e);}
  var snapshots=new JsonArray();
  for(var c:commands){String op=c.get(0).getAsString();Object result=null;try{switch(op){
   case "add"->result=lookup.admit(values.get(c.get(1).getAsInt()),c.get(2).getAsBoolean());
   case "status"->{int x=c.get(1).getAsInt(),z=c.get(2).getAsInt();lookup.getOrCreateChunk(x,z);lookup.chunkStatusChange(x,z,FullChunkStatus.valueOf(c.get(3).getAsString()));}
   case "worldgen"->{var selected=new ArrayList<Entity>();for(var index:c.get(1).getAsJsonArray())selected.add(values.get(index.getAsInt()));lookup.addWorldGenChunkEntities(selected,new ChunkPos(c.get(2).getAsInt(),c.get(3).getAsInt()));}
   case "remove"->values.get(c.get(1).getAsInt()).setRemoved(c.get(2).getAsBoolean()?Entity.RemovalReason.DISCARDED:Entity.RemovalReason.UNLOADED_TO_CHUNK);
   case "throw"->throwing[0]=c.get(1).getAsString();default->throw new AssertionError(op);
  }}catch(Stop e){result="error";}
   var snapshot=new JsonObject();snapshot.add("result",G.toJsonTree(result));var raw=new JsonArray();var seen=new HashSet<Integer>();for(var value:values){int id=value.getId();if(!seen.add(id))continue;var mapped=lookup.raw(id);var position=mapped==null?null:(ChunkSystemEntity)mapped;raw.add(a(id,lookup.hasEntity(id),lookup.get(id)!=null,mapped==null?null:mapped.getUUID().toString(),position==null?null:position.moonrise$getSectionX(),position==null?null:position.moonrise$getSectionY(),position==null?null:position.moonrise$getSectionZ()));}snapshot.add("raw",raw);
   var all=new JsonArray();for(var e:lookup.getAll())all.add(e.getId());snapshot.add("accessible",all);var tracked=new JsonArray();for(int i=0;i<lookup.tracked.size();i++)tracked.add(((Entity)lookup.tracked.getRawData()[i]).getId());snapshot.add("tracked",tracked);snapshots.add(snapshot);
  }
  var row=new JsonObject();row.addProperty("name",name);row.add("entities",G.toJsonTree(entities));row.add("commands",G.toJsonTree(commands));row.add("events",events);row.add("snapshots",snapshots);out.println(row);
 }
 public static void main(String[] args)throws Exception{
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();net.minecraft.server.MinecraftServer.configurePackRepository(packs,WorldDataConfiguration.DEFAULT,true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var out=new PrintStream(args[0])){
   var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();GenerationSheepProbe.access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess().freeze();
   run(out,"visibility",List.of(entity("cow",101,1,0,64,0,false,false)),List.of(a("add",0,false),a("status",0,0,"FULL"),a("status",0,0,"BLOCK_TICKING"),a("status",0,0,"ENTITY_TICKING"),a("status",0,0,"FULL"),a("status",0,0,"INACCESSIBLE"),a("remove",0,true)));
   run(out,"duplicates",List.of(entity("cow",101,1,0,64,0,false,false),entity("pig",101,2,1,64,0,false,false),entity("chicken",102,1,2,64,0,false,false)),List.of(a("add",0,false),a("add",1,false),a("add",2,false),a("status",0,0,"FULL")));
   run(out,"removed_vs_dead",List.of(entity("sheep",101,1,0,64,0,true,false),entity("pig",102,2,1,64,0,false,true)),List.of(a("status",0,0,"FULL"),a("add",0,false),a("add",1,false),a("remove",1,true)));
   run(out,"clamped_sections",List.of(entity("cow",101,1,-1,-10000,32,false,false),entity("pig",102,2,-1,10000,32,false,false)),List.of(a("add",0,false),a("add",1,false),a("status",-1,2,"FULL")));
   run(out,"worldgen_wrong_chunk",List.of(entity("cow",101,1,0,64,0,false,false),entity("pig",102,2,16,64,0,false,false)),List.of(a("worldgen",a(0,1),0,0),a("status",0,0,"FULL")));
   run(out,"disk_no_created",List.of(entity("chicken",101,1,0,64,0,false,false)),List.of(a("status",0,0,"FULL"),a("add",0,true),a("remove",0,false)));
   run(out,"callback_failure",List.of(entity("cow",101,1,0,64,0,false,false)),List.of(a("status",0,0,"FULL"),a("throw","TrackingStart"),a("add",0,false)));
  }
 }
}
