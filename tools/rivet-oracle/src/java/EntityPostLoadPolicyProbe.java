import com.google.gson.*;
import java.io.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.nbt.*;
import net.minecraft.server.level.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.chunk.status.ChunkStatusTasks;
import io.papermc.paper.configuration.WorldConfiguration;

/** Actual EntityType LOAD constructors and actual checkDupeUUID. The world
 * existing-entity lookup and explicit config are shells. No policy code is
 * reproduced; secure random UUID values are intentionally not golden bytes. */
public final class EntityPostLoadPolicyProbe {
 static final sun.misc.Unsafe U=ChestMinecartProbe.UNSAFE;
 static final UUID DUPLICATE=new UUID(1,2);
 static JsonArray removeEvents=new JsonArray();
 static final class World extends ServerLevel {
  WorldConfiguration config;ChestMinecartProbe.WorldShell world;Entity existing;int nextId;int lookups;net.minecraft.world.level.levelgen.LegacyRandomSource worldRandom;
  private World(){super(null,null,null,null,null,null,false,0,List.of(),false,null,null,null,null,null,null);}
  @Override public int getNextEntityId(){return ++nextId;}
  @Override public net.minecraft.world.flag.FeatureFlagSet enabledFeatures(){return net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS;}
  @Override public WorldConfiguration paperConfig(){return config;}
  @Override public org.bukkit.craftbukkit.CraftWorld getWorld(){return world;}
  @Override public RegistryAccess registryAccess(){return RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY);}
  @Override public net.minecraft.util.RandomSource getRandom(){return worldRandom;}
  @Override public Entity getEntity(UUID uuid){lookups++;return existing!=null&&existing.getUUID().equals(uuid)?existing:null;}
 }
 static void run(PrintStream out,String mode,String situation,int count)throws Exception {
  var base=ChestMinecartProbe.level(false);var w=(World)U.allocateInstance(World.class);w.nextId=100;w.world=base.world;w.config=base.config;w.worldRandom=new net.minecraft.world.level.levelgen.LegacyRandomSource(99);
  var spigot=Level.class.getDeclaredField("spigotConfig");spigot.setAccessible(true);spigot.set(w,spigot.get(base));
  w.config.entities.spawning.duplicateUuid=w.config.entities.spawning.new DuplicateUUID();w.config.entities.spawning.duplicateUuid.mode=WorldConfiguration.Entities.Spawning.DuplicateUUID.DuplicateUUIDMode.valueOf(mode);w.config.entities.spawning.duplicateUuid.safeRegenDeleteRange=32;
  w.moonrise$setEntityLookup(new EntityLookupProbe.Lookup(w,(net.minecraft.world.level.entity.LevelCallback<Entity>)java.lang.reflect.Proxy.newProxyInstance(net.minecraft.world.level.entity.LevelCallback.class.getClassLoader(),new Class[]{net.minecraft.world.level.entity.LevelCallback.class},(p,m,a)->{throw new AssertionError("detached removal unexpectedly indexed: "+m);})));
  if(!situation.equals("absent")) {
   w.existing=(situation.equals("different_type")?EntityTypes.MINECART:EntityTypes.CHEST_MINECART).create(w,EntitySpawnReason.LOAD);
   w.existing.setUUID(DUPLICATE);w.existing.setPos(situation.equals("far")?40:situation.equals("boundary")?32:1,64,0);
   if(situation.equals("removed"))w.existing.discard(null);
  }
  w.nextId=100;removeEvents=new JsonArray();w.worldRandom=new net.minecraft.world.level.levelgen.LegacyRandomSource(99);var random=new ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom(42);Entity.SHARED_RANDOM=random;
  var decisions=new JsonArray();var constructed=new ArrayList<Entity>();
  var accepted=java.util.stream.IntStream.range(0,count).mapToObj(i->{
   var tag=new CompoundTag();tag.putString("id","minecraft:chest_minecart");tag.putIntArray("UUID",new int[]{0,1,0,2});var pos=new ListTag();pos.add(DoubleTag.valueOf(0));pos.add(DoubleTag.valueOf(64));pos.add(DoubleTag.valueOf(0));tag.put("Pos",pos);
   var entity=EntityType.loadEntityRecursive(tag,w,new EntitySpawnRequest(EntitySpawnReason.LOAD,false),e->e);if(entity==null)throw new AssertionError("native load refused");constructed.add(entity);return entity;
  }).filter(entity->{boolean discard=ChunkStatusTasks.checkDupeUUID(w,entity);var d=new JsonObject();d.addProperty("discard",discard);d.addProperty("removed",entity.isRemoved());d.addProperty("uuid_changed",!entity.getUUID().equals(DUPLICATE));decisions.add(d);return !discard;}).toList();
  var row=new JsonObject();row.addProperty("mode",mode);row.addProperty("situation",situation);row.addProperty("count",count);row.add("decisions",decisions);row.addProperty("accepted",accepted.size());row.addProperty("next_id",w.nextId);row.addProperty("lookups",w.lookups);row.add("remove_events",removeEvents);row.addProperty("entity_rng_next",random.nextLong());row.addProperty("world_rng_next",w.worldRandom.nextLong());out.println(row);
 }
 public static void main(String[] args)throws Exception {
  System.setProperty("paper.debugEntitiesWithInvalidIds","true");
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
  org.bukkit.craftbukkit.CraftRegistry.setMinecraftRegistry(RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY));
  var server=(org.bukkit.craftbukkit.CraftServer)U.allocateInstance(org.bukkit.craftbukkit.CraftServer.class);
  var manager=new org.bukkit.plugin.SimplePluginManager(server,null);
  manager.paperPluginManager=(org.bukkit.plugin.PluginManager)java.lang.reflect.Proxy.newProxyInstance(org.bukkit.plugin.PluginManager.class.getClassLoader(),new Class[]{org.bukkit.plugin.PluginManager.class},(p,m,a)->{if(!m.getName().equals("callEvent"))throw new AssertionError(m);var event=(org.bukkit.event.entity.EntityRemoveEvent)a[0];removeEvents.add(event.getCause().name());return null;});
  var field=org.bukkit.craftbukkit.CraftServer.class.getDeclaredField("pluginManager");field.setAccessible(true);field.set(server,manager);
  field=org.bukkit.Bukkit.class.getDeclaredField("server");field.setAccessible(true);field.set(null,server);
  try(var out=new PrintStream(args[0])){for(String mode:List.of("SAFE_REGEN","DELETE","WARN","NOTHING")){for(String situation:List.of("absent","near","far","boundary","different_type","removed"))run(out,mode,situation,1);run(out,mode,"absent",2);}}
  catch(Throwable failure){failure.printStackTrace(new PrintStream(new FileOutputStream(FileDescriptor.err)));throw failure;}
 }
}
