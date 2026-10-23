import com.google.gson.*;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.nbt.*;
import net.minecraft.server.*;
import net.minecraft.server.level.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.entity.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.*;
import io.papermc.paper.configuration.WorldConfiguration;

/** Actual LevelChunk.getBlockEntity/promotion/addAndRegister. Only terrain and
 * world-effect getters are shells. The chunk's real native maps, constructors,
 * pending consumption, wrapper binding and save execute unmodified. No ticker
 * body, game-event listener or entity AI is replaced with success. */
public final class LazyBlockEntityProbe {
 static final BlockPos POS=new BlockPos(8,64,8);
 static final class World extends ServerLevel {
  WorldConfiguration config;int added,tickers;
  private World(){super(null,null,null,null,null,null,false,0L,List.of(),false,null,null,null,null,null,null);}
  @Override public RegistryAccess registryAccess(){return RegistryLayer.createRegistryAccess().getLayer(RegistryLayer.STATIC);}
  @Override public WorldConfiguration paperConfig(){return config;}
  @Override public void onBlockEntityAdded(BlockEntity entity){added++;}
  @Override public void addBlockEntityTicker(TickingBlockEntity ticker){tickers++;}
 }
 static final class Chunk extends LevelChunk {
  BlockState state;
  private Chunk(){super(null,null);}
  @Override public BlockState getBlockState(BlockPos pos){return state;}
 }
 static void put(Object target,Class<?> owner,String name,Object value)throws Exception{var f=owner.getDeclaredField(name);f.setAccessible(true);f.set(target,value);}
 static Chunk chunk(World world,BlockState state,boolean loaded)throws Exception{
  var chunk=(Chunk)ChestMinecartProbe.UNSAFE.allocateInstance(Chunk.class);chunk.state=state;
  put(chunk,LevelChunk.class,"level",world);put(chunk,LevelChunk.class,"tickersInLevel",new HashMap<>());
  put(chunk,ChunkAccess.class,"blockEntities",new it.unimi.dsi.fastutil.objects.Object2ObjectOpenHashMap<BlockPos,BlockEntity>());
  put(chunk,ChunkAccess.class,"pendingBlockEntities",new HashMap<BlockPos,CompoundTag>());chunk.setLoaded(loaded);return chunk;
 }
 static void run(PrintWriter output,String family,BlockState state,String kind,boolean loaded,boolean immediate)throws Exception{
  var world=(World)ChestMinecartProbe.UNSAFE.allocateInstance(World.class);world.config=ChestMinecartProbe.level(false).paperConfig();
  world.config.unsupportedSettings=world.config.new UnsupportedSettings();world.config.unsupportedSettings.ticking=world.config.unsupportedSettings.new Ticking();
  put(world,Level.class,"capturedBlockEntities",new LinkedHashMap<BlockPos,BlockEntity>());
  var chunk=chunk(world,state,loaded);String id=switch(family){case "chest"->"minecraft:chest";case "hive"->"minecraft:beehive";case "spawner"->"minecraft:mob_spawner";case "heart"->"minecraft:creaking_heart";default->throw new AssertionError();};
  String input=switch(kind){case "missing"->null;case "dummy"->"{id:'DUMMY',x:8,y:64,z:8}";case "valid"->"{id:'"+id+"',x:8,y:64,z:8,bees:[{entity_data:{id:'minecraft:bee'},ticks_in_hive:77,min_ticks_in_hive:600}]}";case "wrong"->"{id:'minecraft:beehive',x:8,y:64,z:8}";case "unknown"->"{id:'invalid:unknown',x:8,y:64,z:8}";default->throw new AssertionError();};
  if(input!=null)chunk.setBlockEntityNbt(TagParser.parseCompoundFully(input));
  var creation=immediate?LevelChunk.EntityCreationType.IMMEDIATE:LevelChunk.EntityCreationType.CHECK;
  BlockEntity first=chunk.getBlockEntity(POS,creation),second=chunk.getBlockEntity(POS,creation);
  var row=new JsonObject();row.addProperty("family",family);row.addProperty("kind",kind);row.addProperty("loaded",loaded);row.addProperty("immediate",immediate);row.addProperty("input_snbt",input);row.addProperty("present",first!=null);row.addProperty("same_identity",first!=null&&first==second);row.addProperty("pending",chunk.getBlockEntityNbt(POS)!=null);row.addProperty("added",world.added);row.addProperty("tickers",world.tickers);
  if(first!=null){row.addProperty("has_level",first.hasLevel());row.addProperty("saved",first.saveWithFullMetadata(world.registryAccess()).toString());}
  output.println(row);
 }
 public static void main(String[] args)throws Exception{
  SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
  try(var output=new PrintWriter(args[0])){
   for(String family:List.of("chest","hive","spawner","heart"))for(String kind:List.of("missing","dummy","valid","wrong","unknown"))for(boolean loaded:new boolean[]{false,true})for(boolean immediate:new boolean[]{false,true}){
    Block block=switch(family){case "chest"->Blocks.CHEST;case "hive"->Blocks.BEE_NEST;case "spawner"->Blocks.SPAWNER;case "heart"->Blocks.CREAKING_HEART;default->throw new AssertionError();};
    run(output,family,block.defaultBlockState(),kind,loaded,immediate);
   }
  }
 }
}
