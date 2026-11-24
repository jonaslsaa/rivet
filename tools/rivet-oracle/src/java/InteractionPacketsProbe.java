import com.google.gson.*;
import io.netty.buffer.Unpooled;
import java.io.PrintWriter;
import java.util.HexFormat;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.network.*;
import net.minecraft.network.protocol.game.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.phys.*;

/** Actual #448 interaction packet codecs; no handwritten Java wire encoder. */
public final class InteractionPacketsProbe {
 static String hex(FriendlyByteBuf buf) {
  byte[] bytes=new byte[buf.readableBytes()];buf.getBytes(buf.readerIndex(),bytes);
  return HexFormat.of().formatHex(bytes);
 }
 static JsonArray ints(int... values){var a=new JsonArray();for(int v:values)a.add(v);return a;}
 static JsonArray bitsNumeric(Vec3 v){var a=new JsonArray();
  a.add(new java.math.BigInteger(Long.toUnsignedString(Double.doubleToRawLongBits(v.x))));
  a.add(new java.math.BigInteger(Long.toUnsignedString(Double.doubleToRawLongBits(v.y))));
  a.add(new java.math.BigInteger(Long.toUnsignedString(Double.doubleToRawLongBits(v.z))));return a;}
 static String error(Runnable decode){
  try{decode.run();}catch(RuntimeException e){return e.getMessage();}
  throw new AssertionError("decode unexpectedly succeeded");
 }
 public static void main(String[] args)throws Exception {
  SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
  try(var out=new PrintWriter(args[0])) {
   Object[][] useItemOn={
    {InteractionHand.MAIN_HAND,new int[]{0,0,0},Direction.UP,new Vec3(0.5,1.0,0.5),false,false,0},
    {InteractionHand.OFF_HAND,new int[]{-1,-64,15},Direction.NORTH,new Vec3(-0.25,-63.9,15.0),true,false,300},
    {InteractionHand.MAIN_HAND,new int[]{29999999,319,-29999999},Direction.EAST,new Vec3(30000000.0,319.5,-29999998.7),false,true,2147483647},
    {InteractionHand.MAIN_HAND,new int[]{1,2,3},Direction.DOWN,new Vec3(1.1,2.0000001,3.9999),false,false,5},
    {InteractionHand.OFF_HAND,new int[]{10,64,10},Direction.WEST,new Vec3(12.5,66.0,8.0),true,true,128},
    {InteractionHand.MAIN_HAND,new int[]{-7,-1,-9},Direction.SOUTH,new Vec3(Double.POSITIVE_INFINITY,-0.0,-9.0),false,false,1},
   };
   for(Object[] c:useItemOn){
    int[] xyz=(int[])c[1];var pos=new BlockPos(xyz[0],xyz[1],xyz[2]);
    var hit=new BlockHitResult((Vec3)c[3],(Direction)c[2],pos,(boolean)c[4],(boolean)c[5]);
    var packet=new ServerboundUseItemOnPacket((InteractionHand)c[0],hit,(int)c[6]);
    var buf=new FriendlyByteBuf(Unpooled.buffer());ServerboundUseItemOnPacket.STREAM_CODEC.encode(buf,packet);
    var row=new JsonObject();row.addProperty("packet","use_item_on");
    row.addProperty("hand",((InteractionHand)c[0]).ordinal());row.add("pos",ints(xyz));
    row.addProperty("direction",((Direction)c[2]).ordinal());row.add("location_bits",bitsNumeric((Vec3)c[3]));
    row.addProperty("inside",(boolean)c[4]);row.addProperty("world_border",(boolean)c[5]);row.addProperty("sequence",(int)c[6]);
    row.addProperty("body",hex(buf));
    var decoded=ServerboundUseItemOnPacket.STREAM_CODEC.decode(buf);
    if(buf.readableBytes()!=0)throw new AssertionError("trailing");
    row.add("decoded_location_bits",bitsNumeric(decoded.getHitResult().getLocation()));
    out.println(row);
   }
   // Controlled negatives: readEnum(InteractionHand / Direction) out of range.
   int[][] badUseItemOn={{2,1},{-1,1},{0,6}};
   for(int[] bad:badUseItemOn){
    var buf=new FriendlyByteBuf(Unpooled.buffer());
    buf.writeVarInt(bad[0]);buf.writeLong(BlockPos.asLong(1,2,3));buf.writeVarInt(bad[1]);
    buf.writeFloat(0.5f);buf.writeFloat(0.5f);buf.writeFloat(0.5f);buf.writeBoolean(false);buf.writeBoolean(false);buf.writeVarInt(0);
    var row=new JsonObject();row.addProperty("packet","use_item_on_malformed");row.addProperty("body",hex(buf));
    row.addProperty("error",error(()->ServerboundUseItemOnPacket.STREAM_CODEC.decode(buf)));out.println(row);
   }
   Object[][] useItem={
    {InteractionHand.MAIN_HAND,0,0.0f,0.0f},
    {InteractionHand.OFF_HAND,2147483647,-180.0f,90.0f},
    {InteractionHand.MAIN_HAND,1,Float.intBitsToFloat(0x7fc00000),Float.POSITIVE_INFINITY},
    {InteractionHand.MAIN_HAND,128,370.5f,-95.25f},
   };
   for(Object[] c:useItem){
    var packet=new ServerboundUseItemPacket((InteractionHand)c[0],(int)c[1],(float)c[2],(float)c[3]);
    var buf=new FriendlyByteBuf(Unpooled.buffer());ServerboundUseItemPacket.STREAM_CODEC.encode(buf,packet);
    var row=new JsonObject();row.addProperty("packet","use_item");row.addProperty("hand",((InteractionHand)c[0]).ordinal());
    row.addProperty("sequence",(int)c[1]);
    row.addProperty("y_rot_bits",Integer.toUnsignedLong(Float.floatToRawIntBits((float)c[2])));
    row.addProperty("x_rot_bits",Integer.toUnsignedLong(Float.floatToRawIntBits((float)c[3])));
    row.addProperty("body",hex(buf));out.println(row);
   }
   for(int hand:new int[]{2,5}){
    var buf=new FriendlyByteBuf(Unpooled.buffer());buf.writeVarInt(hand);buf.writeVarInt(0);buf.writeFloat(0f);buf.writeFloat(0f);
    var row=new JsonObject();row.addProperty("packet","use_item_malformed");row.addProperty("body",hex(buf));
    row.addProperty("error",error(()->ServerboundUseItemPacket.STREAM_CODEC.decode(buf)));out.println(row);
   }
   for(int sequence:new int[]{0,1,127,128,2147483647,-1}){
    var buf=new FriendlyByteBuf(Unpooled.buffer());ClientboundBlockChangedAckPacket.STREAM_CODEC.encode(buf,new ClientboundBlockChangedAckPacket(sequence));
    var row=new JsonObject();row.addProperty("packet","block_changed_ack");row.addProperty("sequence",sequence);row.addProperty("body",hex(buf));out.println(row);
   }
   Object[][] destruction={
    {1,new int[]{0,0,0},0},{42,new int[]{-1,-64,15},-1},{2147483647,new int[]{29999999,319,-29999999},9},
    {-5,new int[]{1,2,3},255},{7,new int[]{1,2,3},300},{7,new int[]{1,2,3},10},
   };
   for(Object[] c:destruction){
    int[] xyz=(int[])c[1];
    var packet=new ClientboundBlockDestructionPacket((int)c[0],new BlockPos(xyz[0],xyz[1],xyz[2]),(int)c[2]);
    var buf=new FriendlyByteBuf(Unpooled.buffer());ClientboundBlockDestructionPacket.STREAM_CODEC.encode(buf,packet);
    var row=new JsonObject();row.addProperty("packet","block_destruction");row.addProperty("id",(int)c[0]);row.add("pos",ints(xyz));
    row.addProperty("progress",(int)c[2]);row.addProperty("body",hex(buf));
    row.addProperty("decoded_progress",ClientboundBlockDestructionPacket.STREAM_CODEC.decode(buf).getProgress());out.println(row);
   }
  }
 }
}
