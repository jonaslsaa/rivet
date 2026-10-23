import java.io.PrintStream;
import java.lang.reflect.Method;
import java.util.*;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import ca.spottedleaf.moonrise.patches.collisions.CollisionUtil;
/** Real declaring bodies and cached section bookkeeping flags. No live
 * mutation algorithm is synthesized here. */
public final class BlockMutationMetadataProbe {
 static String owner(Class<?> type,String name) {
  for(Class<?> c=type;c!=null;c=c.getSuperclass())for(Method m:c.getDeclaredMethods())if(m.getName().equals(name)&&!m.isBridge())return c.getSimpleName();
  throw new AssertionError(type+" "+name);
 }
 public static void main(String[] args)throws Exception {
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
  try(var blocks=new PrintStream(args[0]);var states=new PrintStream(args[1])) {
   for(Block b:BuiltInRegistries.BLOCK)blocks.println(BuiltInRegistries.BLOCK.getKey(b)+"\t"+owner(b.getClass(),"onPlace")+"\t"+owner(b.getClass(),"affectNeighborsAfterRemoval")+"\t"+owner(b.getClass(),"shouldChangedStateKeepBlockEntity")+"\t"+owner(b.getClass(),"updateIndirectNeighbourShapes")+"\t"+owner(b.getClass(),"neighborChanged"));
   int start=0,last=-1;
   for(int id=0;id<=Block.BLOCK_STATE_REGISTRY.size();id++){
    int flags=-2;if(id<Block.BLOCK_STATE_REGISTRY.size()){BlockState s=Block.BLOCK_STATE_REGISTRY.byId(id);flags=(CollisionUtil.isSpecialCollidingBlock(s)?1:0)|(s.getFluidState().isRandomlyTicking()?2:0)|(s.hasAnalogOutputSignal()?4:0);}
    if(last!=-1&&flags!=last){states.println(start+"\t"+(id-start)+"\t"+last);start=id;}
    last=flags;
   }
  }
 }
}
