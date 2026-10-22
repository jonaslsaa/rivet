import com.google.gson.*;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.nbt.*;
import net.minecraft.resources.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.vehicle.minecart.*;
import net.minecraft.world.flag.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.storage.TagValueOutput;
import io.papermc.paper.configuration.WorldConfiguration;

/** Real EntityType factory, complete Entity/Minecart constructor chain and save.
 * Only world getter/identity context uses unconstructed shells; no entity is
 * Unsafe-allocated, and no entity constructor, save or UUID algorithm is mocked.
 * Does not exercise ticking, plugins or world entity-manager admission. */
public final class ChestMinecartProbe {
 static final sun.misc.Unsafe UNSAFE;
 static {try{var f=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);UNSAFE=(sun.misc.Unsafe)f.get(null);}catch(Exception e){throw new ExceptionInInitializerError(e);}}
 static final UUID WORLD_UUID=new UUID(0x123456789abcdef0L,0xfedcba9876543210L);
 static final class WorldShell extends org.bukkit.craftbukkit.CraftWorld {
  private WorldShell(){super(null,null,null,null);}
  @Override public UUID getUID(){return WORLD_UUID;}
 }
 static final class LevelShell extends ServerLevel {
  WorldConfiguration config;WorldShell world;boolean experimental;int nextId;
  private LevelShell(){super(null,null,null,null,null,null,false,0L,List.of(),false,null,null,null,null,null,null);}
  @Override public int getNextEntityId(){return ++nextId;}
  @Override public FeatureFlagSet enabledFeatures(){return experimental?FeatureFlagSet.of(FeatureFlags.VANILLA,FeatureFlags.MINECART_IMPROVEMENTS):FeatureFlags.DEFAULT_FLAGS;}
  @Override public WorldConfiguration paperConfig(){return config;}
  @Override public org.bukkit.craftbukkit.CraftWorld getWorld(){return world;}
  @Override public RegistryAccess registryAccess(){return RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY);}
 }
 static LevelShell level(boolean experimental)throws Exception{
  var level=(LevelShell)UNSAFE.allocateInstance(LevelShell.class);level.experimental=experimental;level.nextId=100;
  level.world=(WorldShell)UNSAFE.allocateInstance(WorldShell.class);
  var spigot=(org.spigotmc.SpigotWorldConfig)UNSAFE.allocateInstance(org.spigotmc.SpigotWorldConfig.class);
  var field=Level.class.getDeclaredField("spigotConfig");field.setAccessible(true);field.set(level,spigot);
  var ctor=WorldConfiguration.class.getDeclaredConstructor(org.spigotmc.SpigotWorldConfig.class,Identifier.class);ctor.setAccessible(true);
  level.config=ctor.newInstance(spigot,Identifier.withDefaultNamespace("overworld"));
  level.config.entities=level.config.new Entities();level.config.entities.spawning=level.config.entities.new Spawning();
  return level;
 }
 static JsonObject run(boolean experimental,long entitySeed,long featureSeed,boolean assignLoot,boolean zeroSeed)throws Exception{
  var level=level(experimental);var entityRandom=new ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom(entitySeed);Entity.SHARED_RANDOM=entityRandom;
  var featureRandom=new LegacyRandomSource(featureSeed);
  var minecart=EntityTypes.CHEST_MINECART.create(level,EntitySpawnReason.CHUNK_GENERATION);
  if(minecart==null)throw new AssertionError("vanilla enabled MISC constructor refused");
  minecart.setInitialPos(-16.5,50.5,32.5);
  if(assignLoot)minecart.setLootTable(net.minecraft.world.level.storage.loot.BuiltInLootTables.ABANDONED_MINESHAFT,zeroSeed?0:featureRandom.nextLong());
  var output=TagValueOutput.createWithContext(net.minecraft.util.ProblemReporter.DISCARDING,level.registryAccess());
  if(!minecart.save(output))throw new AssertionError("fresh entity save refused");
  var row=new JsonObject();row.addProperty("experimental",experimental);row.addProperty("entity_seed",entitySeed);row.addProperty("feature_seed",featureSeed);row.addProperty("assign_loot",assignLoot);row.addProperty("zero_seed",zeroSeed);
  row.addProperty("entity_id",minecart.getId());row.addProperty("entity_rng_next",entityRandom.nextLong());row.addProperty("feature_rng_next",featureRandom.nextLong());
  row.addProperty("behavior",minecart.getBehavior().getClass().getSimpleName());row.addProperty("inventory_size",minecart.getItemStacks().size());
  row.addProperty("inventory_empty",minecart.getItemStacks().stream().allMatch(net.minecraft.world.item.ItemStack::isEmpty));
  var box=minecart.getBoundingBox();row.add("bounds",new Gson().toJsonTree(new double[]{box.minX,box.minY,box.minZ,box.maxX,box.maxY,box.maxZ}));
  row.addProperty("eye_height",minecart.getEyeHeight());row.addProperty("saved",output.buildResult().toString());return row;
 }
 public static void main(String[] args)throws Exception{
  net.minecraft.SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
  try(var output=new PrintStream(args[0])){
   for(boolean experimental:new boolean[]{false,true})for(int mode=0;mode<3;mode++)
    output.println(run(experimental,42L,23L,mode!=0,mode==2));
  }
 }
}
