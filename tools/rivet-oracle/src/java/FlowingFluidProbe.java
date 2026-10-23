import com.google.gson.*;
import io.papermc.paper.configuration.WorldConfiguration;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.*;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerChunkCache;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraft.tags.BlockTags;
import net.minecraft.util.RandomSource;
import net.minecraft.world.attribute.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.*;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.material.*;
import org.bukkit.event.block.*;

/** Calls genuine FlowingFluid algorithms, Craft event construction/dispatch,
 * liquid-container methods, and real collision shapes. The explicitly bounded
 * storage setter records direct writes without simulating Level neighbour
 * callbacks; that separate canonical mutation algorithm has its own oracle. */
public final class FlowingFluidProbe {
 static org.bukkit.craftbukkit.CraftServer server;
 static World[] active;
 static Object events;
 static Method register;
 static org.bukkit.plugin.Plugin plugin;
 static final class Cache extends ServerChunkCache {
  World world;
  private Cache(){super(null,null,null,null,null,null,0,0,false,null,null,null);}
  @Override public LevelChunk getChunkAtIfLoadedImmediately(int x,int z){
   world.trace.add("cache:"+x+","+z);if(world.unloadedEast&&x>0)return null;
   try{var chunk=(Chunk)ChestMinecartProbe.UNSAFE.allocateInstance(Chunk.class);chunk.world=world;return chunk;}catch(Exception error){throw new AssertionError(error);}
  }
 }
 static final class Chunk extends LevelChunk {
  World world;
  private Chunk(){super(null,null);}
  @Override public BlockState getBlockState(BlockPos pos){world.trace.add("peek:"+position(pos));return world.raw(pos);}
  @Override public FluidState getFluidState(BlockPos pos){world.trace.add("peek:"+position(pos));return world.raw(pos).getFluidState();}
 }
 static final class World extends ServerLevel {
  WorldConfiguration config;
  Map<BlockPos,BlockState> blocks;List<String> trace;GameRules rules;EnvironmentAttributeSystem attributes;net.minecraft.world.level.levelgen.LegacyRandomSource rng;org.bukkit.craftbukkit.CraftServer server;org.bukkit.craftbukkit.CraftWorld craftWorld;Cache cache;int floor;boolean unloadedEast;String policy;
  private World(){super(null,null,null,null,null,null,false,0L,List.of(),false,null,null,null,null,null,null);}
  BlockState raw(BlockPos pos){return blocks.getOrDefault(pos,pos.getY()<floor?Blocks.STONE.defaultBlockState():Blocks.AIR.defaultBlockState());}
  @Override public BlockState getBlockState(BlockPos pos){trace.add("get:"+position(pos));return raw(pos);}
  @Override public ServerChunkCache getChunkSource(){return cache;}
  @Override public FluidState getFluidState(BlockPos pos){return getBlockState(pos).getFluidState();}
  @Override public boolean setBlock(BlockPos pos,BlockState state,int flags){trace.add("set:"+position(pos)+":"+Block.getId(state)+":"+flags);return blocks.put(pos.immutable(),state)!=state;}
  @Override public void scheduleTick(BlockPos pos,Fluid fluid,int delay){trace.add("schedule:"+position(pos)+":"+BuiltInRegistries.FLUID.getId(fluid)+":"+delay);}
  @Override public void scheduleTick(BlockPos pos,Block block,int delay){trace.add("schedule_block:"+position(pos)+":"+BuiltInRegistries.BLOCK.getId(block)+":"+delay);}
  @Override public WorldConfiguration paperConfig(){return config;}
  @Override public RandomSource getRandom(){return rng;}
  @Override public GameRules getGameRules(){return rules;}
  @Override public EnvironmentAttributeSystem environmentAttributes(){return attributes;}
  @Override public org.bukkit.craftbukkit.CraftServer getCraftServer(){return server;}
  @Override public org.bukkit.craftbukkit.CraftWorld getWorld(){return craftWorld;}
  @Override public RegistryAccess registryAccess(){return RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY);}
  @Override public void levelEvent(int type,BlockPos pos,int data){trace.add("event:"+position(pos)+":"+type+":"+data);}
  @Override public int getMinY(){return -64;}
  @Override public int getHeight(){return 384;}
 }
 static String position(BlockPos pos){return pos.getX()+","+pos.getY()+","+pos.getZ();}
 static String fluid(FluidState state){return BuiltInRegistries.FLUID.getId(state.getType())+","+state.getAmount()+","+(state.hasProperty(FlowingFluid.FALLING)&&state.getValue(FlowingFluid.FALLING)?1:0);}
 static void setup()throws Exception {
  SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
  server=(org.bukkit.craftbukkit.CraftServer)ChestMinecartProbe.UNSAFE.allocateInstance(org.bukkit.craftbukkit.CraftServer.class);
  var pluginManager=new org.bukkit.plugin.SimplePluginManager((org.bukkit.Server)Proxy.newProxyInstance(org.bukkit.Server.class.getClassLoader(),new Class[]{org.bukkit.Server.class},(proxy,method,a)->{throw new AssertionError(method);}),null);
  var serverProxy=(org.bukkit.Server)Proxy.newProxyInstance(org.bukkit.Server.class.getClassLoader(),new Class[]{org.bukkit.Server.class},(proxy,method,a)->switch(method.getName()) {case "isPrimaryThread"->true;case "isStopping"->false;case "getPluginManager"->pluginManager;case "getLogger"->java.util.logging.Logger.getLogger("fluid-oracle");case "getName","getVersion","getBukkitVersion"->"fluid-oracle";default->throw new AssertionError(method);});
  var eventType=Class.forName("io.papermc.paper.plugin.manager.PaperEventManager");var eventCtor=eventType.getDeclaredConstructor(org.bukkit.Server.class);eventCtor.setAccessible(true);events=eventCtor.newInstance(serverProxy);
  var callEvent=eventType.getDeclaredMethod("callEvent",org.bukkit.event.Event.class);callEvent.setAccessible(true);
  pluginManager.paperPluginManager=(org.bukkit.plugin.PluginManager)Proxy.newProxyInstance(org.bukkit.plugin.PluginManager.class.getClassLoader(),new Class[]{org.bukkit.plugin.PluginManager.class},(proxy,method,a)->{if(method.getName().equals("callEvent")){try{callEvent.invoke(events,a[0]);}catch(InvocationTargetException error){throw error.getCause();}return null;}throw new AssertionError(method);});
  var pluginField=org.bukkit.craftbukkit.CraftServer.class.getDeclaredField("pluginManager");pluginField.setAccessible(true);pluginField.set(server,pluginManager);
  active=new World[1];
  org.bukkit.plugin.EventExecutor listener=(unused,event)->{
    var world=active[0];
    if(event instanceof FluidLevelChangeEvent change){world.trace.add("level:"+position(((org.bukkit.craftbukkit.block.CraftBlock)change.getBlock()).getPosition())+":"+Block.getId(((org.bukkit.craftbukkit.block.data.CraftBlockData)change.getNewData()).getState()));if(world.policy.equals("cancel_level"))change.setCancelled(true);if(world.policy.equals("alter_level")){var data=((org.bukkit.craftbukkit.block.data.CraftBlockData)change.getNewData()).getState();if(data.hasProperty(LiquidBlock.LEVEL))change.setNewData(data.setValue(LiquidBlock.LEVEL,7).asBlockData());}}
    else if(event instanceof BlockFromToEvent flow){world.trace.add("flow:"+flow.getFace().name());if(world.policy.equals("cancel_down")&&flow.getFace()==org.bukkit.block.BlockFace.DOWN||world.policy.equals("cancel_north")&&flow.getFace()==org.bukkit.block.BlockFace.NORTH)flow.setCancelled(true);}
    else throw new AssertionError(event.getClass());
  };
  register=eventType.getDeclaredMethod("registerEvent",Class.class,org.bukkit.event.Listener.class,org.bukkit.event.EventPriority.class,org.bukkit.plugin.EventExecutor.class,org.bukkit.plugin.Plugin.class,boolean.class);register.setAccessible(true);
  plugin=StructureEventDispatchProbe.plugin("fluids",new java.util.concurrent.atomic.AtomicBoolean(true));
  for(var type:List.of(FluidLevelChangeEvent.class,BlockFromToEvent.class))register.invoke(events,type,new org.bukkit.event.Listener(){},org.bukkit.event.EventPriority.NORMAL,listener,plugin,false);
  org.bukkit.Bukkit.setServer(serverProxy);
 }
 static void registerEvent(Class<? extends org.bukkit.event.Event> type,org.bukkit.plugin.EventExecutor listener)throws Exception {register.invoke(events,type,new org.bukkit.event.Listener(){},org.bukkit.event.EventPriority.NORMAL,listener,plugin,false);}
 static void collisionFaces(String path)throws Exception {
  var states=new ArrayList<BlockState>();
  for(String name:List.of("air","stone","oak_slab","oak_stairs","snow","oak_fence","glass_pane","iron_bars","oak_trapdoor","oak_fence_gate","mud","soul_sand","farmland","dirt_path","cactus","cake","hopper","water","lava")) {
   var possible=BuiltInRegistries.BLOCK.getValue(Identifier.withDefaultNamespace(name)).getStateDefinition().getPossibleStates();
   for(int index:new TreeSet<Integer>(List.of(0,possible.size()/2,possible.size()-1)))states.add(possible.get(index));
  }
  var world=world("allow",64,false,false);var origin=new BlockPos(0,64,0);
  try(var output=new PrintWriter(path)){for(var a:states)for(var b:states)for(var direction:Direction.values()){
   boolean result=net.minecraft.world.phys.shapes.Shapes.mergedFaceOccludes(a.getCollisionShape(world,origin),b.getCollisionShape(world,origin.relative(direction)),direction);
   output.println(Block.getId(a)+"\t"+Block.getId(b)+"\t"+direction.get3DDataValue()+"\t"+(result?1:0));
  }}
 }
 static World world(String policy,int floor,boolean fast,boolean unloaded)throws Exception {
  var world=(World)ChestMinecartProbe.UNSAFE.allocateInstance(World.class);active[0]=world;world.cache=(Cache)ChestMinecartProbe.UNSAFE.allocateInstance(Cache.class);world.cache.world=world;world.blocks=new HashMap<>();world.trace=new ArrayList<>();world.server=server;world.floor=floor;world.unloadedEast=unloaded;world.policy=policy;world.rng=new net.minecraft.world.level.levelgen.LegacyRandomSource(99);
  // Level's final no-load/height methods read these fields directly, not the
  // virtual getters. Install the actual dimension geometry in this shell.
  for(var entry:Map.of("minY",-64,"maxY",319,"height",384,"minSectionY",-4,"maxSectionY",19,"sectionsCount",24).entrySet()){var dimensionField=Level.class.getDeclaredField(entry.getKey());dimensionField.setAccessible(true);dimensionField.setInt(world,entry.getValue());}
  var spigot=(org.spigotmc.SpigotWorldConfig)ChestMinecartProbe.UNSAFE.allocateInstance(org.spigotmc.SpigotWorldConfig.class);
  var configConstructor=WorldConfiguration.class.getDeclaredConstructor(org.spigotmc.SpigotWorldConfig.class,Identifier.class);configConstructor.setAccessible(true);
  world.config=configConstructor.newInstance(spigot,Identifier.withDefaultNamespace("overworld"));world.config.environment=world.config.new Environment();
  world.craftWorld=(org.bukkit.craftbukkit.CraftWorld)ChestMinecartProbe.UNSAFE.allocateInstance(org.bukkit.craftbukkit.CraftWorld.class);var field=org.bukkit.craftbukkit.CraftWorld.class.getDeclaredField("world");field.setAccessible(true);field.set(world.craftWorld,world);
  world.rules=new GameRules(net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS);world.rules.set(GameRules.WATER_SOURCE_CONVERSION,true,null);world.rules.set(GameRules.LAVA_SOURCE_CONVERSION,true,null);world.attributes=EnvironmentAttributeSystem.builder().addConstantLayer(EnvironmentAttributes.FAST_LAVA,base->fast).build();return world;
 }
 public static void main(String[] args)throws Exception {
  setup();
  var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
  MinecraftServer.configurePackRepository(packs,new net.minecraft.world.level.WorldDataConfiguration(new net.minecraft.world.level.DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var properties=new PrintWriter(args[0]);var states=new PrintWriter(args[1]);var output=new PrintWriter(args[2])) {
   net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,RegistryLayer.createRegistryAccess().getLayer(RegistryLayer.STATIC)).forEach(pending->pending.apply());
   for(var block:BuiltInRegistries.BLOCK){boolean container=block instanceof LiquidBlockContainer;var state=block.defaultBlockState();boolean excluded=block instanceof DoorBlock||state.is(BlockTags.SIGNS)||block==Blocks.LADDER||block==Blocks.SUGAR_CANE||block==Blocks.BUBBLE_COLUMN||block==Blocks.NETHER_PORTAL||block==Blocks.END_PORTAL||block==Blocks.END_GATEWAY||block==Blocks.STRUCTURE_VOID;String can="-",place="-";if(container){can=block.getClass().getMethod("canPlaceLiquid",net.minecraft.world.entity.LivingEntity.class,BlockGetter.class,BlockPos.class,BlockState.class,Fluid.class).getDeclaringClass().getSimpleName();place=block.getClass().getMethod("placeLiquid",LevelAccessor.class,BlockPos.class,BlockState.class,FluidState.class).getDeclaringClass().getSimpleName();}properties.println(BuiltInRegistries.BLOCK.getKey(block)+"\t"+can+"\t"+place+"\t"+(excluded?1:0)+"\t"+(block instanceof WaterloggedTransparentBlock?1:0));}
   String previous=null;int start=0,length=0,index=0;for(var state:Block.BLOCK_STATE_REGISTRY){String value=fluid(state.getFluidState());if(previous!=null&&!previous.equals(value)){states.println(start+"\t"+length+"\t"+previous);start=index;length=0;}previous=value;length++;index++;}states.println(start+"\t"+length+"\t"+previous);
   for(String name:List.of("source_down","source_floor","flow_decay","flow_supported","falling","two_sources","waterlogged_slab","slopes","partial_faces","unloaded_east","cancel_level","alter_level","cancel_down","cancel_north","lava_increase"))for(boolean lava:new boolean[]{false,true})for(boolean fast:new boolean[]{false,true}) {
    if(name.equals("waterlogged_slab")&&lava)continue;
    var world=world(name,name.equals("source_down")||name.equals("cancel_down")?62:64,fast,name.equals("unloaded_east"));
    var kind=lava?Fluids.LAVA:Fluids.WATER;var flowing=lava?Fluids.FLOWING_LAVA:Fluids.FLOWING_WATER;var liquid=lava?Blocks.LAVA:Blocks.WATER;var pos=new BlockPos(world.unloadedEast?15:0,64,0);BlockState initial=liquid.defaultBlockState();
    if(List.of("flow_decay","flow_supported","cancel_level","alter_level","lava_increase").contains(name))initial=liquid.defaultBlockState().setValue(LiquidBlock.LEVEL,name.equals("lava_increase")?6:4);
    if(name.equals("falling"))initial=liquid.defaultBlockState().setValue(LiquidBlock.LEVEL,8);
    world.blocks.put(pos,initial);
    if(List.of("flow_supported","lava_increase","two_sources","alter_level").contains(name))world.blocks.put(pos.north(),liquid.defaultBlockState());
    if(name.equals("two_sources")){world.blocks.put(pos.south(),liquid.defaultBlockState());initial=liquid.defaultBlockState().setValue(LiquidBlock.LEVEL,4);world.blocks.put(pos,initial);}
    if(name.equals("waterlogged_slab")){world.blocks.put(pos.north(),Blocks.OAK_SLAB.defaultBlockState());world.blocks.put(pos.north().north(),Blocks.WATER.defaultBlockState());}
    if(name.equals("slopes")){world.blocks.put(pos.north().below(),Blocks.AIR.defaultBlockState());world.blocks.put(pos.east(),Blocks.STONE.defaultBlockState());}
    if(name.equals("partial_faces")){world.blocks.put(pos.north(),Blocks.OAK_SLAB.defaultBlockState());world.blocks.put(pos.south(),Blocks.OAK_STAIRS.defaultBlockState());world.blocks.put(pos.east(),Blocks.OAK_FENCE.defaultBlockState());}
    var inputs=new JsonArray();world.blocks.entrySet().stream().sorted(Comparator.comparing(e->position(e.getKey()))).forEach(e->{var entry=new JsonObject();entry.addProperty("pos",position(e.getKey()));entry.addProperty("state",Block.getId(e.getValue()));inputs.add(entry);});
    initial.getFluidState().tick(world,pos,initial);
    var row=new JsonObject();row.addProperty("name",name);row.addProperty("lava",lava);row.addProperty("fast",fast);row.addProperty("floor",world.floor);row.addProperty("origin",position(pos));row.addProperty("initial",Block.getId(initial));row.add("input",inputs);row.add("trace",new Gson().toJsonTree(world.trace));row.addProperty("next_long",world.rng.nextLong());output.println(row);
   }
  }
  if(args.length>3)collisionFaces(args[3]);
 }
}
