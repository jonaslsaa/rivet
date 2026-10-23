import com.google.gson.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.redstone.CollectingNeighborUpdater;

/** Runs genuine private addAndRun/runUpdates with controlled NeighborUpdates
 * callbacks. No block, physics-event or world mutation body is simulated. */
public final class NeighborCollectorProbe {
 static Class<?> updateType;
 static Method add;
 static CollectingNeighborUpdater updater;
 static JsonArray trace;
 static String failure;
 static Object task(int id){
  return Proxy.newProxyInstance(updateType.getClassLoader(),new Class[]{updateType},(proxy,method,args)->{
   if(method.getName().equals("forEachUpdatedPos")){((java.util.function.Consumer<BlockPos>)args[0]).accept(new BlockPos(id,64,0));return null;}
   if(method.getName().equals("runNext")){
    trace.add("run:"+id);
    if(id==1){enqueue(2);enqueue(3);}
    if(id==2){if(failure.equals("callback"))throw new IllegalStateException("callback");enqueue(4);}
    return false;
   }
   if(method.getName().equals("toString"))return "probe:"+id;
   throw new AssertionError(method);
  });
 }
 static void enqueue(int id)throws Throwable {try{add.invoke(updater,new BlockPos(id,64,0),task(id));}catch(InvocationTargetException e){throw e.getCause();}}
 public static void main(String[] args)throws Throwable {
  var out=new java.io.PrintStream(args[0]);
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
  updateType=Class.forName("net.minecraft.world.level.redstone.CollectingNeighborUpdater$NeighborUpdates");
  add=CollectingNeighborUpdater.class.getDeclaredMethod("addAndRun",BlockPos.class,updateType);add.setAccessible(true);
  for(int max:new int[]{-1,0,1,2,3,4,16})for(String mode:List.of("none","callback","debug")){
   failure=mode;trace=new JsonArray();updater=new CollectingNeighborUpdater(null,max);
   updater.setDebugListener(pos->{trace.add("debug:"+pos.getX());if(mode.equals("debug")&&pos.getX()==2)throw new IllegalStateException("debug");});
   boolean failed=false;try{enqueue(1);}catch(Throwable e){if(!(e instanceof IllegalStateException))throw e;failed=true;}
   var row=new JsonObject();row.addProperty("max",max);row.addProperty("failure",mode);row.addProperty("failed",failed);row.add("trace",trace);
   for(String name:List.of("count","stack","addedThisLayer")){var field=CollectingNeighborUpdater.class.getDeclaredField(name);field.setAccessible(true);var value=field.get(updater);row.addProperty(name,value instanceof Number?(Number)value:((Collection<?>)value).size());}
   out.println(row);
  }
  out.close();
 }
}
