import com.google.gson.*;
import java.io.PrintWriter;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.server.MinecraftServer;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.redstone.Orientation;
import org.bukkit.event.block.BlockFormEvent;

/** Actual LiquidBlock callbacks and genuine Craft BlockForm event dispatch.
 * Storage writes are recorded at the Level boundary, not a live-physics claim. */
public final class LiquidBlockProbe {
 public static void main(String[] args)throws Exception {
  FlowingFluidProbe.setup();
  FlowingFluidProbe.registerEvent(BlockFormEvent.class,(unused,event)->{
   var form=(BlockFormEvent)event;var world=FlowingFluidProbe.active[0];
   world.trace.add("form:"+Block.getId(((org.bukkit.craftbukkit.block.CraftBlockState)form.getNewState()).getHandle()));
   if(world.policy.equals("cancel_form"))form.setCancelled(true);
   if(world.policy.equals("alter_form"))form.getNewState().setType(org.bukkit.Material.GLASS);
  });
  var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
  MinecraftServer.configurePackRepository(packs,new net.minecraft.world.level.WorldDataConfiguration(new net.minecraft.world.level.DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
  var neighbor=LiquidBlock.class.getDeclaredMethod("neighborChanged",BlockState.class,Level.class,BlockPos.class,Block.class,Orientation.class,boolean.class);neighbor.setAccessible(true);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var out=new PrintWriter(args[0])) {
   net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,net.minecraft.server.RegistryLayer.createRegistryAccess().getLayer(net.minecraft.server.RegistryLayer.STATIC)).forEach(p->p.apply());
   for(String name:List.of("water","water_lava","water_unloaded","source_water_mix","flow_water_mix","basalt","water_priority","bubble_soul","bubble_magma"))
    for(boolean fast:new boolean[]{false,true})for(String policy:List.of("allow","cancel_form","alter_form"))for(boolean notification:new boolean[]{false,true}) {
     var world=FlowingFluidProbe.world(policy,64,fast,name.equals("water_unloaded"));world.config.environment.waterOverLavaFlowSpeed=17;var pos=new BlockPos(name.equals("water_unloaded")?15:0,64,0);
     boolean lava=name.contains("mix")||name.equals("basalt")||name.equals("water_priority");var initial=(lava?Blocks.LAVA:Blocks.WATER).defaultBlockState();
     if(name.equals("flow_water_mix"))initial=initial.setValue(LiquidBlock.LEVEL,4);
     world.blocks.put(pos,initial);
     if(name.equals("water_lava"))world.blocks.put(pos.north(),Blocks.LAVA.defaultBlockState());
     if(name.equals("water_unloaded"))world.blocks.put(pos.east(),Blocks.LAVA.defaultBlockState());
     if(name.contains("mix"))world.blocks.put(pos.above(),Blocks.WATER.defaultBlockState());
     if(name.equals("basalt")||name.equals("water_priority")){world.blocks.put(pos.below(),Blocks.SOUL_SOIL.defaultBlockState());world.blocks.put(pos.north(),Blocks.BLUE_ICE.defaultBlockState());}
     if(name.equals("water_priority"))world.blocks.put(pos.above(),Blocks.WATER.defaultBlockState());
     if(name.equals("bubble_soul"))world.blocks.put(pos.below(),Blocks.SOUL_SAND.defaultBlockState());
     if(name.equals("bubble_magma"))world.blocks.put(pos.below(),Blocks.MAGMA_BLOCK.defaultBlockState());
     var inputs=new JsonArray();world.blocks.entrySet().stream().sorted(Comparator.comparing(e->FlowingFluidProbe.position(e.getKey()))).forEach(e->{var item=new JsonObject();item.addProperty("pos",FlowingFluidProbe.position(e.getKey()));item.addProperty("state",Block.getId(e.getValue()));inputs.add(item);});
     if(notification)neighbor.invoke(initial.getBlock(),initial,world,pos,Blocks.STONE,null,false);else initial.onPlace(world,pos,Blocks.STONE.defaultBlockState(),false);
     var row=new JsonObject();row.addProperty("name",name);row.addProperty("fast",fast);row.addProperty("policy",policy);row.addProperty("notification",notification);row.addProperty("origin",FlowingFluidProbe.position(pos));row.addProperty("initial",Block.getId(initial));row.add("input",inputs);
     row.add("effects",new Gson().toJsonTree(world.trace.stream().filter(s->!s.startsWith("get:")&&!s.startsWith("cache:")&&!s.startsWith("peek:")).toList()));row.addProperty("next_long",world.rng.nextLong());out.println(row);
    }
  }
 }
}
