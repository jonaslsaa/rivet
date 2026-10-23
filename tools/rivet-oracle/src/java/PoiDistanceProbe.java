import com.google.gson.*;
import java.io.*;
import java.lang.reflect.*;
import java.nio.ByteBuffer;
import java.security.MessageDigest;
import java.util.*;
import it.unimi.dsi.fastutil.longs.Long2ByteMap;
import ca.spottedleaf.moonrise.common.misc.Delayed26WayDistancePropagator3D;

/** Executes the actual pinned Moonrise propagator. Digests include EVERY
 * propagated level and EVERY ordered intermediate callback, not sampled or
 * independently recomputed expected distances. */
public final class PoiDistanceProbe {
 static final Gson GSON=new Gson();
 static JsonArray a(Object...items){return GSON.toJsonTree(items).getAsJsonArray();}
 static MessageDigest digest(){try{return MessageDigest.getInstance("SHA-256");}catch(Exception e){throw new AssertionError(e);}}
 static final class Events {
  MessageDigest hash=digest();int count,equal;JsonArray head=new JsonArray();ArrayDeque<JsonArray> tail=new ArrayDeque<>();
  void accept(long key,byte old,byte next){hash.update(ByteBuffer.allocate(10).putLong(key).put(old).put(next).array());var event=a(key,old,next);if(head.size()<4)head.add(event);tail.addLast(event);if(tail.size()>4)tail.removeFirst();count++;if(old==next)equal++;}
 }
 static JsonObject levels(Delayed26WayDistancePropagator3D tracker)throws Exception {
  var field=Delayed26WayDistancePropagator3D.class.getDeclaredField("levels");field.setAccessible(true);var map=(Long2ByteMap)field.get(tracker);var keys=map.keySet().toLongArray();Arrays.sort(keys);var hash=digest();for(long key:keys)hash.update(ByteBuffer.allocate(9).putLong(key).put(map.get(key)).array());var result=new JsonObject();result.addProperty("size",map.size());result.addProperty("hash",HexFormat.of().formatHex(hash.digest()));return result;
 }
 static void run(PrintStream output,String name,List<JsonArray> commands)throws Exception {
  Events[] events={new Events()};var tracker=new Delayed26WayDistancePropagator3D((key,old,next)->events[0].accept(key,old,next));var steps=new JsonArray();
  for(var c:commands){events[0]=new Events();String op=c.get(0).getAsString();JsonElement result=JsonNull.INSTANCE;try{switch(op){
   case "set"->tracker.setSource(c.get(1).getAsInt(),c.get(2).getAsInt(),c.get(3).getAsInt(),c.get(4).getAsInt());
   case "set_packed"->tracker.setSource(c.get(1).getAsLong(),c.get(2).getAsInt());
   case "remove"->tracker.removeSource(c.get(1).getAsInt(),c.get(2).getAsInt(),c.get(3).getAsInt());
   case "remove_packed"->tracker.removeSource(c.get(1).getAsLong());
   case "get"->result=new JsonPrimitive(tracker.getLevel(c.get(1).getAsInt(),c.get(2).getAsInt(),c.get(3).getAsInt()));
   case "get_packed"->result=new JsonPrimitive(tracker.getLevel(c.get(1).getAsLong()));
   case "propagate"->result=new JsonPrimitive(tracker.propagateUpdates());
   default->throw new AssertionError(op);
  }}catch(IllegalArgumentException invalid){result=new JsonPrimitive(invalid.getMessage());}
  var step=new JsonObject();step.add("result",result);step.add("levels",levels(tracker));step.addProperty("events",events[0].count);step.addProperty("equal_events",events[0].equal);step.addProperty("events_hash",HexFormat.of().formatHex(events[0].hash.digest()));step.add("head",events[0].head);step.add("tail",GSON.toJsonTree(events[0].tail));steps.add(step);
  }
  var row=new JsonObject();row.addProperty("name",name);row.add("commands",GSON.toJsonTree(commands));row.add("steps",steps);output.println(row);
 }
 public static void main(String[] args)throws Exception {
  try(var output=new PrintStream(args[0])){
   run(output,"single_stale_and_redundant",List.of(a("propagate"),a("set",0,0,0,3),a("get",0,0,0),a("propagate"),a("get",0,0,0),a("get",1,1,1),a("set",0,0,0,3),a("propagate"),a("set",0,0,0,1),a("get",0,0,0),a("propagate"),a("remove",0,0,0),a("get",0,0,0),a("propagate"),a("remove",0,0,0),a("propagate")));
   run(output,"coalesced_linked_updates",List.of(a("set",0,0,0,4),a("set",2,0,0,3),a("set",0,0,0,2),a("set",-2,1,0,3),a("set",2,0,0,4),a("propagate"),a("remove",2,0,0),a("set",0,0,0,4),a("propagate"),a("set",0,0,0,1),a("set",-2,1,0,1),a("propagate"),a("remove",0,0,0),a("remove",-2,1,0),a("propagate")));
   run(output,"cancelled_before_propagation",List.of(a("set",0,0,0,7),a("remove",0,0,0),a("propagate"),a("set",0,0,0,2),a("propagate"),a("set",0,0,0,5),a("set",0,0,0,2),a("propagate"),a("remove",0,0,0),a("set",0,0,0,2),a("propagate")));
   run(output,"poi_level7_overlap_unload",List.of(a("set",-1,-4,-1,7),a("set",2,-4,2,7),a("propagate"),a("get",0,-4,0),a("remove",-1,-4,-1),a("propagate"),a("get",0,-4,0),a("remove",2,-4,2),a("propagate")));
   run(output,"packed_coordinate_boundaries",List.of(a("set",2097151,524287,-2097152,3),a("propagate"),a("get",-2097152,-524288,2097151),a("set_packed",Long.MIN_VALUE,2),a("set_packed",Long.MAX_VALUE,2),a("propagate"),a("get_packed",Long.MIN_VALUE),a("remove",2097151,524287,-2097152),a("remove_packed",Long.MIN_VALUE),a("remove_packed",Long.MAX_VALUE),a("propagate")));
   run(output,"invalid_levels",List.of(a("set",0,0,0,0),a("set",0,0,0,-1),a("set",0,0,0,64),a("set",0,0,0,Integer.MIN_VALUE),a("set",0,0,0,Integer.MAX_VALUE),a("propagate")));
   for(int order=0;order<2;order++){var commands=new ArrayList<JsonArray>();for(int i=0;i<30;i++){int j=order==0?i:29-i;commands.add(a("set",j%5-2,j/5%3-1,j/15,2+j%3));}commands.add(a("propagate"));for(int i=0;i<30;i+=2)commands.add(a("remove",i%5-2,i/5%3-1,i/15));commands.add(a("propagate"));for(int i=1;i<30;i+=2)commands.add(a("set",i%5-2,i/5%3-1,i/15,1));commands.add(a("propagate"));run(output,"dense_source_order_"+order,commands);}
  }
 }
}
