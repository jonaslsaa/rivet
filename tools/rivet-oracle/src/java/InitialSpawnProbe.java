import com.google.gson.*;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.resources.*;
import net.minecraft.server.*;
import net.minecraft.server.level.*;
import net.minecraft.server.level.progress.LevelLoadListener;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.properties.*;
import net.minecraft.world.level.chunk.*;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.dimension.*;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.storage.*;

/** Genuine private MinecraftServer.setInitialSpawn and PlayerSpawnFinder on
 * actual native ProtoChunk/LevelChunk heightmaps and collision algorithms.
 * Only world/cache getters are shells; controlled terrain is fixture DATA,
 * not a replacement spawn, heightmap or collision algorithm. */
public final class InitialSpawnProbe {
 static final sun.misc.Unsafe UNSAFE;
 static {try{var f=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);UNSAFE=(sun.misc.Unsafe)f.get(null);}catch(Exception e){throw new ExceptionInInitializerError(e);}}
 static RegistryAccess.Frozen access;static net.minecraft.core.LayeredRegistryAccess<RegistryLayer> layers;static NoiseBasedChunkGenerator generator;static PalettedContainerFactory factory;
 static void put(Object target,Class<?> owner,String name,Object value)throws Exception{var f=owner.getDeclaredField(name);f.setAccessible(true);f.set(target,value);}
 static final class Cache extends ServerChunkCache {
  World world;RandomState random;
  private Cache(){super(null,null,null,null,null,null,0,0,false,null,null,null);}
  @Override public ChunkGenerator getGenerator(){return generator;}
  @Override public RandomState randomState(){return random;}
  @Override public LevelChunk getChunkAtIfLoadedImmediately(int x,int z){return world.chunk(new ChunkPos(x,z));}
  @Override public ChunkAccess getChunk(int x,int z,ChunkStatus status,boolean load){return world.chunk(new ChunkPos(x,z));}
 }
 static final class World extends ServerLevel {
  Cache cache;String terrain;ChunkPos climate;long seed;DimensionType type;ServerLevelData data;Map<ChunkPos,LevelChunk> chunks;Set<ChunkPos> queried;
  private World(){super(null,null,null,null,null,null,false,0,List.of(),false,null,null,null,null,null,null);}
  @Override public ServerChunkCache getChunkSource(){return cache;}
  @Override public RegistryAccess registryAccess(){return access;}
  @Override public DimensionType dimensionType(){return type;}
  @Override public int getMinY(){return type.minY();}
  @Override public int getHeight(){return type.height();}
  @Override public long getSeed(){return seed;}
  @Override public LevelData getLevelData(){return data;}
  LevelChunk chunk(ChunkPos pos){
   queried.add(pos);
   return chunks.computeIfAbsent(pos,key->{
    var proto=new ProtoChunk(key,UpgradeData.EMPTY,this,factory,null);
    boolean skipped=terrain.equals("void") || terrain.equals("skip_two") && (key.equals(climate)||key.equals(new ChunkPos(climate.x()+1,climate.z())));
    if(!skipped)for(int x=0;x<16;x++)for(int z=0;z<16;z++){
     if(terrain.equals("late_column")&&!((x==2&&z==3)||(x==3&&z==0)))continue;
     var state=switch(terrain){case "water"->Blocks.WATER.defaultBlockState();case "bottom_slab"->Blocks.STONE_SLAB.defaultBlockState();case "top_slab"->Blocks.STONE_SLAB.defaultBlockState().setValue(SlabBlock.TYPE,SlabType.TOP);default->Blocks.STONE.defaultBlockState();};
     proto.setBlockState(new BlockPos(key.getMinBlockX()+x,62,key.getMinBlockZ()+z),Blocks.STONE.defaultBlockState(),2);
     proto.setBlockState(new BlockPos(key.getMinBlockX()+x,63,key.getMinBlockZ()+z),state,2);
    }
    Heightmap.primeHeightmaps(proto,ChunkStatus.FULL.heightmapsAfter());
    return new LevelChunk(this,proto,null);
   });
  }
 }
 static JsonObject run(long seed,String terrain)throws Exception{
  var random=RandomState.create(access,NoiseGeneratorSettings.OVERWORLD,seed);var climate=random.sampler().findSpawnPosition();
  var world=(World)UNSAFE.allocateInstance(World.class);world.seed=seed;world.terrain=terrain;world.climate=ChunkPos.containing(climate);world.type=access.lookupOrThrow(Registries.DIMENSION_TYPE).getOrThrow(ResourceKey.create(Registries.DIMENSION_TYPE,Identifier.withDefaultNamespace("overworld"))).value();world.chunks=new HashMap<>();world.queried=new LinkedHashSet<>();
  world.cache=(Cache)UNSAFE.allocateInstance(Cache.class);world.cache.world=world;world.cache.random=random;put(world,Level.class,"dimension",Level.OVERWORLD);
  // Level's cached height getters are constructor-owned world context. Supply
  // the actual registered dimension values, not zeroed Unsafe shell fields.
  put(world,Level.class,"minY",world.type.minY());put(world,Level.class,"height",world.type.height());put(world,Level.class,"maxY",world.type.minY()+world.type.height()-1);
  put(world,Level.class,"minSectionY",world.type.minY()>>4);put(world,Level.class,"maxSectionY",(world.type.minY()+world.type.height()-1)>>4);put(world,Level.class,"sectionsCount",world.type.height()>>4);
  LevelData.RespawnData[] spawn={null};var writes=new JsonArray();
  var data=(ServerLevelData)Proxy.newProxyInstance(ServerLevelData.class.getClassLoader(),new Class<?>[]{ServerLevelData.class},(proxy,method,args)->switch(method.getName()){
   case "setSpawn"->{spawn[0]=(LevelData.RespawnData)args[0];var p=spawn[0].pos();writes.add(new Gson().toJsonTree(new int[]{p.getX(),p.getY(),p.getZ()}));yield null;}
   case "getRespawnData"->spawn[0];case "getLevelName"->"InitialSpawnOracle";case "getGameTime"->0L;default->throw new UnsupportedOperationException(method.toString());
  });
  world.data=data;
  var method=MinecraftServer.class.getDeclaredMethod("setInitialSpawn",ServerLevel.class,ServerLevelData.class,boolean.class,boolean.class,LevelLoadListener.class);method.setAccessible(true);
  try{method.invoke(null,world,data,false,false,LevelLoadListener.noop());}catch(InvocationTargetException e){throw new RuntimeException(e.getCause());}
  var row=new JsonObject();row.addProperty("seed",seed);row.addProperty("terrain",terrain);row.add("climate",new Gson().toJsonTree(new int[]{climate.getX(),climate.getY(),climate.getZ()}));row.add("writes",writes);row.addProperty("queried_chunks",world.queried.size());var p=spawn[0].pos();row.add("spawn",new Gson().toJsonTree(new int[]{p.getX(),p.getY(),p.getZ()}));return row;
 }
 public static void main(String[] args)throws Exception{
  net.minecraft.SharedConstants.tryDetectVersion();Bootstrap.bootStrap();var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();MinecraftServer.configurePackRepository(packs,WorldDataConfiguration.DEFAULT,true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var output=new PrintStream(args[0])){
   layers=RegistryLayer.createRegistryAccess();var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(RegistryLayer.STATIC)));var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();layers=layers.replaceFrom(RegistryLayer.WORLDGEN,worldgen);
   var all=new ArrayList<HolderLookup.RegistryLookup<?>>(lookups);worldgen.listRegistries().forEach(all::add);var dimensions=RegistryDataLoader.load(resources,all,RegistryDataLoader.DIMENSION_REGISTRIES,Runnable::run).join();layers=layers.replaceFrom(RegistryLayer.DIMENSIONS,dimensions);access=layers.compositeAccess().freeze();generator=(NoiseBasedChunkGenerator)access.lookupOrThrow(Registries.WORLD_PRESET).getOrThrow(net.minecraft.world.level.levelgen.presets.WorldPresets.NORMAL).value().createWorldDimensions().overworld();factory=PalettedContainerFactory.create(access);
   var server=(net.minecraft.server.dedicated.DedicatedServer)UNSAFE.allocateInstance(net.minecraft.server.dedicated.DedicatedServer.class);put(server,MinecraftServer.class,"registries",layers);put(null,MinecraftServer.class,"SERVER",server);
   for(String terrain:new String[]{"stone","late_column","skip_two","water","void","bottom_slab","top_slab"})output.println(run(42,terrain));output.println(run(0,"stone"));
  }
 }
}
