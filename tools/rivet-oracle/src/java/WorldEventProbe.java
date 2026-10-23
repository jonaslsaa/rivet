import com.google.gson.*;
import java.io.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicBoolean;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.block.*;
import org.bukkit.event.*;
import org.bukkit.event.block.*;
import org.bukkit.plugin.*;
import org.bukkit.craftbukkit.block.*;
import com.destroystokyo.paper.event.server.ServerExceptionEvent;

/** Genuine PaperEventManager + real synchronous Bukkit block event classes. */
public final class WorldEventProbe {
 static void add(Class<? extends Event> type,Plugin plugin,EventPriority priority,boolean ignore,EventExecutor callback)throws Exception{FlowingFluidProbe.register.invoke(FlowingFluidProbe.events,type,new Listener(){},priority,callback,plugin,ignore);}
 static Event event(String kind,FlowingFluidProbe.World world)throws Exception{
  var pos=new BlockPos(0,64,0);var block=CraftBlock.at(world,pos);var data=Blocks.WATER.defaultBlockState().asBlockData();
  return switch(kind){case "physics"->new BlockPhysicsEvent(block,data,CraftBlock.at(world,pos.west()));case "fluid"->new FluidLevelChangeEvent(block,data);case "from_to"->new BlockFromToEvent(block,org.bukkit.block.BlockFace.EAST);case "form"->{var snapshot=CraftBlockStates.getBlockState(world,pos);snapshot.setBlock(Blocks.STONE.defaultBlockState());yield new BlockFormEvent(block,snapshot);}default->throw new AssertionError(kind);};
 }
 static void alter(Event event){if(event instanceof FluidLevelChangeEvent fluid)fluid.setNewData(Blocks.WATER.defaultBlockState().setValue(LiquidBlock.LEVEL,7).asBlockData());else if(event instanceof BlockFormEvent form)form.getNewState().setType(org.bukkit.Material.GLASS);}
 public static void main(String[] args)throws Exception{
  FlowingFluidProbe.setup();var call=FlowingFluidProbe.events.getClass().getDeclaredMethod("callEvent",Event.class);call.setAccessible(true);
  try(var out=new PrintWriter(args[0])){for(String kind:List.of("physics","fluid","from_to","form"))for(String scenario:List.of("priorities","failure","disable","wrong_material")){
   if(scenario.equals("wrong_material")&&!kind.equals("fluid"))continue;
   HandlerList.unregisterAll();var world=FlowingFluidProbe.world("allow",64,false,false);world.blocks.put(new BlockPos(0,64,0),Blocks.WATER.defaultBlockState());
   var event=event(kind,world);var type=event.getClass();var trace=new ArrayList<String>();var enabled=new AtomicBoolean(true);var plugin=StructureEventDispatchProbe.plugin("native",enabled);var other=StructureEventDispatchProbe.plugin("other",new AtomicBoolean(true));
   add(ServerExceptionEvent.class,other,EventPriority.NORMAL,false,(unused,e)->{trace.add("exception");var exception=(com.destroystokyo.paper.exception.ServerEventException)((ServerExceptionEvent)e).getException();((Cancellable)exception.getEvent()).setCancelled(true);});
   if(scenario.equals("priorities")){
    add(type,plugin,EventPriority.LOWEST,false,(unused,e)->{trace.add("cancel");((Cancellable)e).setCancelled(true);});
    add(type,plugin,EventPriority.NORMAL,true,(unused,e)->trace.add("ignored"));
    add(type,plugin,EventPriority.HIGHEST,false,(unused,e)->{trace.add("uncancel");alter(e);((Cancellable)e).setCancelled(false);});
    add(type,other,EventPriority.MONITOR,false,(unused,e)->trace.add("monitor"));
   }else if(scenario.equals("disable")){
    add(type,plugin,EventPriority.LOWEST,false,(unused,e)->{trace.add("disable");enabled.set(false);});add(type,plugin,EventPriority.NORMAL,false,(unused,e)->trace.add("disabled"));add(type,other,EventPriority.MONITOR,false,(unused,e)->trace.add("other"));
   }else{
    add(type,plugin,EventPriority.LOWEST,false,(unused,e)->{trace.add("failure");if(scenario.equals("wrong_material"))((FluidLevelChangeEvent)e).setNewData(Blocks.GLASS.defaultBlockState().asBlockData());else{alter(e);throw new IllegalStateException("fixture listener failure");}});
    add(type,other,EventPriority.MONITOR,true,(unused,e)->trace.add("ignored"));
   }
   call.invoke(FlowingFluidProbe.events,event);
   var row=new JsonObject();row.addProperty("kind",kind);row.addProperty("scenario",scenario);row.add("trace",new Gson().toJsonTree(trace));row.addProperty("cancelled",((Cancellable)event).isCancelled());
   if(event instanceof FluidLevelChangeEvent fluid)row.addProperty("state",Block.getId(((org.bukkit.craftbukkit.block.data.CraftBlockData)fluid.getNewData()).getState()));
   if(event instanceof BlockFormEvent form)row.addProperty("state",Block.getId(((CraftBlockState)form.getNewState()).getHandle()));
   out.println(row);
  }}
 }
}
