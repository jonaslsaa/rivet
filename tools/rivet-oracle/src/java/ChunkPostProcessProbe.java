import com.google.gson.*;
import java.io.PrintStream;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.level.chunk.*;
import net.minecraft.nbt.CompoundTag;
import it.unimi.dsi.fastutil.shorts.*;

/** Executes actual LevelChunk.postProcessGeneration. Only world storage and
 * the pending lookup boundary are controlled; no fluid cases claim FlowingFluid
 * parity and pending rows establish call order, not native BE codec fidelity. */
public final class ChunkPostProcessProbe {
 static final sun.misc.Unsafe U;
 static {try{var f=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);U=(sun.misc.Unsafe)f.get(null);}catch(Exception e){throw new ExceptionInInitializerError(e);}}
 static Field field(Class<?> owner,String name)throws Exception{var f=owner.getDeclaredField(name);f.setAccessible(true);return f;}
 static Store store;
 static final class Store {
  final Map<BlockPos,BlockState> states=new HashMap<>();final JsonArray writes=new JsonArray(),lookups=new JsonArray();int failAt;
  BlockState get(BlockPos p){return states.getOrDefault(p,Blocks.AIR.defaultBlockState());}
 }
 static final class World extends ServerLevel {
  private World(){super(null,null,null,null,null,null,false,0L,List.of(),false,null,null,null,null,null,null);}
  @Override public BlockState getBlockState(BlockPos p){return store.get(p);}
  @Override public boolean setBlock(BlockPos p,BlockState s,int flags){
   if(store.failAt>0&&store.writes.size()+1==store.failAt)throw new IllegalStateException("source write sentinel");
   store.states.put(p.immutable(),s);var row=new JsonArray();row.add(p.getX());row.add(p.getY());row.add(p.getZ());row.add(Block.getId(s));row.add(flags);store.writes.add(row);return true;
  }
 }
 static final class Chunk extends LevelChunk {
  private Chunk(){super(null,ChunkPos.ZERO);}
  @Override public BlockState getBlockState(BlockPos p){return store.get(p);}
  @Override public BlockEntity getBlockEntity(BlockPos p){var row=new JsonArray();row.add(p.getX());row.add(p.getY());row.add(p.getZ());store.lookups.add(row);return null;}
 }
 static Chunk chunk(int pending)throws Exception {
  var chunk=(Chunk)U.allocateInstance(Chunk.class);
  field(LevelChunk.class,"level").set(chunk,U.allocateInstance(World.class));
  field(ChunkAccess.class,"chunkPos").set(chunk,ChunkPos.ZERO);
  field(ChunkAccess.class,"levelHeightAccessor").set(chunk,LevelHeightAccessor.create(-64,384));
  field(ChunkAccess.class,"upgradeData").set(chunk,UpgradeData.EMPTY);
  field(ChunkAccess.class,"postProcessing").set(chunk,new ShortList[24]);
  Map<BlockPos,CompoundTag> source=new HashMap<>();for(int i=0;i<pending;i++)source.put(new BlockPos((i*7)&15,64+(i>>4),((i*11)>>4)&15),new CompoundTag());
  Map<BlockPos,CompoundTag> target=new HashMap<>();target.putAll(source);
  field(ChunkAccess.class,"pendingBlockEntities").set(chunk,target);
  return chunk;
 }
 public static void main(String[] args)throws Exception {
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
  try(var out=new PrintStream(args[0])) {
   for(String name:new String[]{"fence","cocoa","duplicate","write_failure","pending1","pending12","pending13","pending30"}) {
    store=new Store();int pending=name.startsWith("pending")?Integer.parseInt(name.substring(7)):0;var chunk=chunk(pending);
    if(pending==0){
     var a=new BlockPos(8,64,8);var b=new BlockPos(9,64,8);
     if(name.equals("fence")){store.states.put(a,Blocks.OAK_FENCE.defaultBlockState());store.states.put(a.west(),Blocks.STONE.defaultBlockState());}
     else {store.states.put(a,Blocks.COCOA.defaultBlockState().setValue(BlockStateProperties.HORIZONTAL_FACING,Direction.NORTH));}
     chunk.getPostProcessing()[8]=new ShortArrayList(new short[]{ProtoChunk.packOffsetCoordinates(a)});
     if(name.equals("duplicate"))chunk.getPostProcessing()[8].add(ProtoChunk.packOffsetCoordinates(a));
     if(name.equals("write_failure")){store.states.put(b,Blocks.COCOA.defaultBlockState().setValue(BlockStateProperties.HORIZONTAL_FACING,Direction.NORTH));chunk.getPostProcessing()[8].add(ProtoChunk.packOffsetCoordinates(b));store.failAt=2;}
    }
    boolean failed=false;try{chunk.postProcessGeneration((ServerLevel)chunk.getLevel());}catch(IllegalStateException e){if(!name.equals("write_failure"))throw e;failed=true;}
    var row=new JsonObject();row.addProperty("name",name);row.addProperty("pending_count",pending);row.add("writes",store.writes);row.add("lookups",store.lookups);
    row.addProperty("failed",failed);row.addProperty("done",chunk.moonrise$isPostProcessingDone());row.addProperty("remaining_pending",((Map<?,?>)field(ChunkAccess.class,"pendingBlockEntities").get(chunk)).size());
    var counts=new JsonArray();for(var list:chunk.getPostProcessing())counts.add(list==null?0:list.size());row.add("sections",counts);out.println(row);
   }
  }
 }
}
