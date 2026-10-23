import com.google.gson.*;
import java.io.PrintStream;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.level.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.*;
import net.minecraft.world.level.levelgen.Heightmap;

/** Actual LevelChunk.setBlockState, real section counters/heightmaps. Light
 * callback boundaries are recorded; propagation has its separate real-engine
 * oracle. No entity, packet or neighbour callback is mocked as implemented. */
public final class LiveBlockMutationProbe {
 static final sun.misc.Unsafe U;
 static {try{var f=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);U=(sun.misc.Unsafe)f.get(null);}catch(Exception e){throw new ExceptionInInitializerError(e);}}
 static void put(Object object,Class<?> owner,String name,Object value)throws Exception {var f=owner.getDeclaredField(name);f.setAccessible(true);f.set(object,value);}
 static Object get(Object object,Class<?> owner,String name)throws Exception {var f=owner.getDeclaredField(name);f.setAccessible(true);return f.get(object);}
 static JsonArray effects;
 static LevelChunk chunk;
 static Cache cache;
 static Light light;
 static final BlockPos POS=new BlockPos(8,64,8);
 static final class World extends ServerLevel {
  private World(){super(null,null,null,null,null,null,false,0L,List.of(),false,null,null,null,null,null,null);}
  @Override public BlockState getBlockState(BlockPos p){return chunk.getBlockState(p);}
  @Override public ServerChunkCache getChunkSource(){return cache;}
  @Override public long getGameTime(){return 0;}
 }
 static final class Cache extends ServerChunkCache {
  private Cache(){super(null,null,null,null,null,null,0,0,false,null,null,null);}
  @Override public ThreadedLevelLightEngine getLightEngine(){return light;}
 }
 static final class Light extends ThreadedLevelLightEngine {
  private Light(){super(null,null,false,null,null);}
  @Override public void checkBlock(BlockPos p){effects.add("block");}
  @Override public void updateSectionStatus(SectionPos p,boolean empty){effects.add("section:"+empty);}
 }
 public static void main(String[] args)throws Exception {
  var errors=System.err;var failure=new java.util.concurrent.atomic.AtomicReference<Throwable>();
  var thread=new ca.spottedleaf.moonrise.common.util.TickThread(()->{try{run(args);}catch(Throwable e){failure.set(e);}},"mutation-source");thread.start();thread.join();
  if(failure.get()!=null){failure.get().printStackTrace(errors);System.exit(1);}
 }
 static void run(String[] args)throws Exception {
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
  var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
  net.minecraft.server.MinecraftServer.configurePackRepository(packs,new WorldDataConfiguration(new DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var out=new PrintStream(args[0])){
   var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();
   var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
   var registries=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();
   var factory=PalettedContainerFactory.create(layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,registries).compositeAccess());
   var world=(World)U.allocateInstance(World.class);cache=(Cache)U.allocateInstance(Cache.class);light=(Light)U.allocateInstance(Light.class);
   for(var pair:List.of(new Block[]{Blocks.AIR,Blocks.STONE},new Block[]{Blocks.STONE,Blocks.AIR},new Block[]{Blocks.STONE,Blocks.STONE},new Block[]{Blocks.AIR,Blocks.CAVE_AIR},new Block[]{Blocks.STONE,Blocks.WATER},new Block[]{Blocks.WATER,Blocks.LAVA},new Block[]{Blocks.AIR,Blocks.OAK_LEAVES},new Block[]{Blocks.OAK_LEAVES,Blocks.BAMBOO},new Block[]{Blocks.BAMBOO,Blocks.STONE},new Block[]{Blocks.AIR,Blocks.OAK_FENCE},new Block[]{Blocks.OAK_FENCE,Blocks.STONE})) {
    effects=new JsonArray();chunk=(LevelChunk)U.allocateInstance(LevelChunk.class);
    put(chunk,LevelChunk.class,"level",world);put(chunk,ChunkAccess.class,"chunkPos",ChunkPos.ZERO);put(chunk,ChunkAccess.class,"levelHeightAccessor",LevelHeightAccessor.create(-64,384));
    put(chunk,LevelChunk.class,"blockTicks",new net.minecraft.world.ticks.LevelChunkTicks<>());put(chunk,LevelChunk.class,"fluidTicks",new net.minecraft.world.ticks.LevelChunkTicks<>());
    put(chunk,LevelChunk.class,"unsavedListener",(LevelChunk.UnsavedListener)p->{});
    put(chunk,ChunkAccess.class,"persistentDataContainer",new org.bukkit.craftbukkit.persistence.DirtyCraftPersistentDataContainer(new org.bukkit.craftbukkit.persistence.CraftPersistentDataTypeRegistry()));
    var sections=new LevelChunkSection[24];for(int i=0;i<24;i++)sections[i]=new LevelChunkSection(factory);
    sections[8].setBlockState(8,0,8,pair[0].defaultBlockState());put(chunk,ChunkAccess.class,"sections",sections);
    var maps=new EnumMap<Heightmap.Types,Heightmap>(Heightmap.Types.class);put(chunk,ChunkAccess.class,"heightmaps",maps);
    for(var type:List.of(Heightmap.Types.MOTION_BLOCKING,Heightmap.Types.MOTION_BLOCKING_NO_LEAVES,Heightmap.Types.OCEAN_FLOOR,Heightmap.Types.WORLD_SURFACE))maps.put(type,new Heightmap(chunk,type));
    Heightmap.primeHeightmaps(chunk,maps.keySet());
    int flags=pair[1] instanceof LiquidBlock || pair[1]==Blocks.BAMBOO?276|512:276;
    var old=chunk.setBlockState(POS,pair[1].defaultBlockState(),flags);
    var row=new JsonObject();row.addProperty("old",Block.getId(pair[0].defaultBlockState()));row.addProperty("state",Block.getId(pair[1].defaultBlockState()));row.addProperty("flags",flags);row.addProperty("result",old==null?-1:Block.getId(old));row.add("effects",effects);
    var counts=new JsonArray();for(String name:List.of("nonEmptyBlockCount","tickingBlockCount","tickingFluidCount","fluidCount","specialCollidingBlocks"))counts.add(((Number)get(sections[8],LevelChunkSection.class,name)).intValue());row.add("counts",counts);
    var heights=new JsonArray();for(var type:List.of(Heightmap.Types.MOTION_BLOCKING,Heightmap.Types.MOTION_BLOCKING_NO_LEAVES,Heightmap.Types.OCEAN_FLOOR,Heightmap.Types.WORLD_SURFACE))heights.add(chunk.getHeight(type,8,8));row.add("heights",heights);
    row.addProperty("unsaved",get(chunk,ChunkAccess.class,"unsaved").equals(true));out.println(row);
   }
  }
 }
}
