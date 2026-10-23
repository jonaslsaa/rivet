import com.google.gson.*;
import io.netty.buffer.Unpooled;
import it.unimi.dsi.fastutil.shorts.ShortLinkedOpenHashSet;
import java.io.PrintWriter;
import java.util.HexFormat;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.network.*;
import net.minecraft.network.protocol.game.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.BlockState;

/** Actual packet constructors/codecs; no handwritten Java wire encoder. */
public final class LiveBlockPacketsProbe {
 static String hex(FriendlyByteBuf buf) {
  byte[] bytes=new byte[buf.readableBytes()];buf.getBytes(0,bytes);buf.release();
  return HexFormat.of().formatHex(bytes);
 }
 public static void main(String[] args)throws Exception {
  SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
  try(var out=new PrintWriter(args[0])) {
   for(int[] xyz:new int[][]{{0,0,0},{-1,-64,15},{30000000,319,-30000000}})
    for(Block block:new Block[]{Blocks.AIR,Blocks.WATER,Blocks.LAVA,Blocks.OBSIDIAN}) {
     var pos=new BlockPos(xyz[0],xyz[1],xyz[2]);var state=block.defaultBlockState();
     var row=new JsonObject();row.add("pos",new Gson().toJsonTree(xyz));row.addProperty("state",Block.getId(state));
     var buffer=new RegistryFriendlyByteBuf(Unpooled.buffer(),RegistryAccess.EMPTY);
     ClientboundBlockUpdatePacket.STREAM_CODEC.encode(buffer,new ClientboundBlockUpdatePacket(pos,state));row.addProperty("block",hex(buffer));
     var section=SectionPos.of(pos);var positions=new ShortLinkedOpenHashSet(new short[]{0,4095,256,16,1});
     BlockState[] states={state,Blocks.STONE.defaultBlockState(),Blocks.WATER.defaultBlockState(),state,Blocks.AIR.defaultBlockState()};
     var entries=new JsonArray();int i=0;for(short local:positions){var entry=new JsonArray();entry.add(local);entry.add(Block.getId(states[i++]));entries.add(entry);}row.add("changes",entries);
     var sectionBuf=new FriendlyByteBuf(Unpooled.buffer());
     ClientboundSectionBlocksUpdatePacket.STREAM_CODEC.encode(sectionBuf,new ClientboundSectionBlocksUpdatePacket(section,positions,states));row.addProperty("section",hex(sectionBuf));
     var events=new JsonArray();for(boolean global:new boolean[]{false,true}) {
      var eventBuf=new FriendlyByteBuf(Unpooled.buffer());
      ClientboundLevelEventPacket.STREAM_CODEC.encode(eventBuf,new ClientboundLevelEventPacket(1501,pos,Block.getId(state),global));events.add(hex(eventBuf));
     }row.add("events",events);out.println(row);
    }
  }
 }
}
