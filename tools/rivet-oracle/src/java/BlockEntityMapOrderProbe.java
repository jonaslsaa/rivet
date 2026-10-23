import com.google.gson.*;
import java.io.*;
import java.util.*;
import it.unimi.dsi.fastutil.objects.Object2ObjectOpenHashMap;
import net.minecraft.core.BlockPos;

/** Actual bundled fastutil default live-map order, including replace, shifting
 * deletion, shrink, capacity-retaining clear, clone and Proto→Level reinsertion. */
public final class BlockEntityMapOrderProbe {
 static Object2ObjectOpenHashMap<BlockPos,Integer> map=new Object2ObjectOpenHashMap<>();
 static JsonArray ops=new JsonArray();
 static void op(String name,BlockPos pos){
  var a=new JsonArray();a.add(name);if(pos!=null){a.add(pos.getX());a.add(pos.getY());a.add(pos.getZ());}
  ops.add(a);
  switch(name){
   case "put"->map.put(pos,map.size());case "remove"->map.remove(pos);case "clear"->map.clear();
   case "clone"->map=map.clone();
   case "promote"->{var next=new Object2ObjectOpenHashMap<BlockPos,Integer>();for(var key:map.keySet())next.put(key,0);map=next;}
   default->throw new AssertionError(name);
  }
 }
 static BlockPos pos(int i){return new BlockPos(i*17-280,64+(i%11),i*31-700);}
 static void save(PrintWriter output,String name)throws Exception{
  var row=new JsonObject();row.addProperty("name",name);row.add("operations",ops);
  var keys=new JsonArray();for(var key:map.keySet()){var a=new JsonArray();a.add(key.getX());a.add(key.getY());a.add(key.getZ());keys.add(a);}row.add("order",keys);
  var n=Object2ObjectOpenHashMap.class.getDeclaredField("n");n.setAccessible(true);row.addProperty("capacity",n.getInt(map));output.println(row);ops=new JsonArray();
 }
 public static void main(String[] args)throws Exception{
  try(var output=new PrintWriter(args[0])){
   save(output,"empty_default");
   for(int i=0;i<25;i++)op("put",pos(i));save(output,"first_resize");
   for(int i=25;i<81;i++)op("put",pos(i));save(output,"grow128");
   for(int i=0;i<81;i+=3)op("put",pos(i));save(output,"replace_retains_slots");
   op("clone",null);save(output,"clone_preserves_probe_table");
   for(int i=0;i<71;i++)op("remove",pos(i));save(output,"shift_and_auto_shrink");
   for(int i=0;i<60;i++)op("put",pos(i));save(output,"regrow_after_shifts");
   op("promote",null);save(output,"fresh_level_from_proto_iteration");
   op("clear",null);for(int i=91;i<103;i++)op("put",pos(i));save(output,"clear_retains_capacity");
  }
 }
}
