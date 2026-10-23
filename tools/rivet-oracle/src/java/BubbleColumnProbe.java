import com.google.gson.*;
import java.io.PrintStream;
import java.util.*;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;

/** Executes actual BlockState.tick; only canonical storage/setBlock results
 * are controlled. This proves column traversal, not Level.setBlock effects. */
public final class BubbleColumnProbe {
 static final sun.misc.Unsafe U;
 static {try{var f=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);U=(sun.misc.Unsafe)f.get(null);}catch(Exception e){throw new ExceptionInInitializerError(e);}}
 static final BlockPos POS=new BlockPos(8,64,8);
 static Map<BlockPos,BlockState> cells;
 static JsonArray writes;
 static int rejectAt;
 static final class World extends ServerLevel {
  private World(){super(null,null,null,null,null,null,false,0L,List.of(),false,null,null,null,null,null,null);}
  @Override public BlockState getBlockState(BlockPos p){return cells.getOrDefault(p,Blocks.AIR.defaultBlockState());}
  @Override public boolean setBlock(BlockPos p,BlockState s,int flags){
   boolean accepted=writes.size()+1!=rejectAt && getBlockState(p)!=s;
   var row=new JsonArray();row.add(p.getY());row.add(Block.getId(s));row.add(flags);row.add(accepted);writes.add(row);
   if(accepted)cells.put(p.immutable(),s);return accepted;
  }
 }
 public static void main(String[] args)throws Exception {
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
  var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
  net.minecraft.server.MinecraftServer.configurePackRepository(packs,new net.minecraft.world.level.WorldDataConfiguration(new net.minecraft.world.level.DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var out=new PrintStream(args[0])) {
   var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)).forEach(p->p.apply());
   var water=Blocks.WATER.defaultBlockState();var up=Blocks.BUBBLE_COLUMN.defaultBlockState().setValue(BubbleColumnBlock.DRAG_DOWN,false);var down=Blocks.BUBBLE_COLUMN.defaultBlockState().setValue(BubbleColumnBlock.DRAG_DOWN,true);
   var world=(World)U.allocateInstance(World.class);
   for(var current:List.of(water,water.setValue(BlockStateProperties.LEVEL,1),water.setValue(BlockStateProperties.LEVEL,8),Blocks.LAVA.defaultBlockState(),up,down))
    for(var below:List.of(Blocks.SOUL_SAND.defaultBlockState(),Blocks.MAGMA_BLOCK.defaultBlockState(),Blocks.STONE.defaultBlockState(),up,down))
     for(var middle:List.of(water,up,down,Blocks.OAK_SLAB.defaultBlockState().setValue(BlockStateProperties.WATERLOGGED,true)))
      for(int reject:new int[]{0,1,2}) {
       cells=new HashMap<>();writes=new JsonArray();rejectAt=reject;
       cells.put(POS.below(),below);cells.put(POS,current);cells.put(POS.above(),middle);cells.put(POS.above(2),water);cells.put(POS.above(3),Blocks.STONE.defaultBlockState());
       var row=new JsonObject();row.addProperty("current",Block.getId(current));row.addProperty("below",Block.getId(below));row.addProperty("middle",Block.getId(middle));row.addProperty("reject",reject);
       current.tick(world,POS,new XoroshiroRandomSource(42));
       row.add("writes",writes);var end=new JsonArray();for(int y=64;y<68;y++)end.add(Block.getId(world.getBlockState(new BlockPos(8,y,8))));row.add("end",end);out.println(row);
      }
  }
 }
}
