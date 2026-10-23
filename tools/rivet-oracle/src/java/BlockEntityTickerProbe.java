import com.google.gson.*;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.nbt.*;
import net.minecraft.server.*;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.sounds.*;
import net.minecraft.util.*;
import net.minecraft.world.attribute.*;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.TickRateManager;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.entity.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.CreakingHeartState;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.storage.TagValueInput;
import io.papermc.paper.configuration.WorldConfiguration;

/** Actual server getTicker classifications and actual hive serverTick. The
 * world supplies a real constant BEES_STAY_IN_HIVE environment layer so the
 * release gate really refuses before entity creation. Sound requests are
 * captured, not broadcast; bee release/AI is not simulated by this probe. */
public final class BlockEntityTickerProbe {
 static final class World extends ServerLevel {
  WorldConfiguration config; EnvironmentAttributeSystem attributes;
  LegacyRandomSource rng; int sounds;
  TickRateManager rate;
  private World(){super(null,null,null,null,null,null,false,0L,List.of(),false,null,null,null,null,null,null);}
  @Override public WorldConfiguration paperConfig(){return config;}
  @Override public EnvironmentAttributeSystem environmentAttributes(){return attributes;}
  @Override public RandomSource getRandom(){return rng;}
  @Override public TickRateManager tickRateManager(){return rate;}
  @Override public boolean shouldTickBlocksAt(BlockPos pos){return true;}
  @Override public void playSound(Entity except,double x,double y,double z,SoundEvent sound,SoundSource source,float volume,float pitch){sounds++;}
 }
 static World world(boolean cooldown,long seed)throws Exception{
  World w=(World)ChestMinecartProbe.UNSAFE.allocateInstance(World.class);
  w.config=ChestMinecartProbe.level(false).paperConfig();
  w.config.entities.behavior=w.config.entities.new Behavior();
  w.config.entities.behavior.cooldownFailedBeehiveReleases=cooldown;
  w.config.unsupportedSettings=w.config.new UnsupportedSettings();
  w.config.unsupportedSettings.ticking=w.config.unsupportedSettings.new Ticking();
  w.attributes=EnvironmentAttributeSystem.builder().addConstantLayer(EnvironmentAttributes.BEES_STAY_IN_HIVE,old->true).build();
  w.rng=new LegacyRandomSource(seed);w.rate=new TickRateManager();
  for(String name:List.of("blockEntityTickers","pendingBlockEntityTickers")){
   var f=Level.class.getDeclaredField(name);f.setAccessible(true);f.set(w,new ArrayList<>());
  }
  return w;
 }
 @SuppressWarnings({"unchecked","rawtypes"})
 static boolean ticker(World world,BlockState state,BlockEntity entity){
  return ((EntityBlock)state.getBlock()).getTicker(world,state,entity.getType())!=null;
 }
 static TickingBlockEntity recording(String name,List<String> trace,Runnable action){
  return new TickingBlockEntity(){
   public void tick(){trace.add(name);if(action!=null)action.run();}
   public boolean isRemoved(){return false;}
   public BlockPos getPos(){return BlockPos.ZERO;}
   public String getType(){return "probe:"+name;}
  };
 }
 public static void main(String[] args)throws Exception{
  SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
  var registries=RegistryLayer.createRegistryAccess().getLayer(RegistryLayer.STATIC);
  var pos=new BlockPos(3,64,-8);var gson=new Gson();
  try(var output=new PrintWriter(args[0])){
   for(boolean cooldown:new boolean[]{false,true})for(int age:new int[]{0,600,601,-3,Integer.MAX_VALUE})for(int ticks:new int[]{1,4,605}){
    var w=world(cooldown,99);var hive=new BeehiveBlockEntity(pos,Blocks.BEE_NEST.defaultBlockState());
    String input="{bees:[{entity_data:{id:'minecraft:bee',HasNectar:1b},ticks_in_hive:"+age+",min_ticks_in_hive:600}]}";
    hive.loadWithComponents(TagValueInput.create(ProblemReporter.DISCARDING,registries,TagParser.parseCompoundFully(input)));
    for(int i=0;i<ticks;i++)BeehiveBlockEntity.serverTick(w,pos,hive.getBlockState(),hive);
    var storedField=BeehiveBlockEntity.class.getDeclaredField("stored");storedField.setAccessible(true);
    Object bee=((List<?>)storedField.get(hive)).getFirst();
    var counter=bee.getClass().getDeclaredField("exitTickCounter");counter.setAccessible(true);
    var row=new JsonObject();row.addProperty("kind","hive");row.addProperty("cooldown",cooldown);row.addProperty("ticks",ticks);row.addProperty("input_snbt",input);
    row.addProperty("output_snbt",hive.saveWithFullMetadata(registries).toString());row.addProperty("exit_counter",counter.getInt(bee));row.addProperty("sounds",w.sounds);row.addProperty("rng_next",w.rng.nextLong());output.println(gson.toJson(row));
   }
   var w=world(true,99);
   for(BlockState state:List.of(Blocks.CHEST.defaultBlockState(),Blocks.SPAWNER.defaultBlockState(),Blocks.BEE_NEST.defaultBlockState(),Blocks.BEEHIVE.defaultBlockState(),Blocks.CREAKING_HEART.defaultBlockState(),Blocks.CREAKING_HEART.defaultBlockState().setValue(CreakingHeartBlock.STATE,CreakingHeartState.DORMANT),Blocks.CREAKING_HEART.defaultBlockState().setValue(CreakingHeartBlock.STATE,CreakingHeartState.AWAKE))){
    BlockEntity entity=((EntityBlock)state.getBlock()).newBlockEntity(pos,state);
    var row=new JsonObject();row.addProperty("kind","classification");row.addProperty("state",state.toString());row.addProperty("ticker",ticker(w,state,entity));output.println(gson.toJson(row));
   }
   // Real Level loop + the actual private LevelChunk rebindable wrapper,
   // exercising callback-time pending insertion, replacement, freeze and NULL.
   var loop=world(true,99);var trace=new ArrayList<String>();
   var cls=Class.forName("net.minecraft.world.level.chunk.LevelChunk$RebindableTickingBlockEntityWrapper");
   var ctor=cls.getDeclaredConstructor(TickingBlockEntity.class);ctor.setAccessible(true);
   var rebind=cls.getDeclaredMethod("rebind",TickingBlockEntity.class);rebind.setAccessible(true);
   var wrapperB=(TickingBlockEntity)ctor.newInstance(recording("B",trace,null));
   final TickingBlockEntity[] wrapperA=new TickingBlockEntity[1];
   wrapperA[0]=(TickingBlockEntity)ctor.newInstance(recording("A1",trace,()->{
    loop.addBlockEntityTicker(wrapperB);
    try{rebind.invoke(wrapperA[0],recording("A2",trace,null));}catch(Exception e){throw new RuntimeException(e);}
   }));
   loop.addBlockEntityTicker(wrapperA[0]);loop.tickBlockEntities();trace.add("after1");
   loop.tickBlockEntities();trace.add("after2");
   loop.rate.setFrozen(true);loop.rate.tick();loop.tickBlockEntities();trace.add("frozen");
   loop.rate.setFrozen(false);loop.rate.tick();
   var nullField=Class.forName("net.minecraft.world.level.chunk.LevelChunk").getDeclaredField("NULL_TICKER");nullField.setAccessible(true);
   rebind.invoke(wrapperA[0],nullField.get(null));loop.tickBlockEntities();trace.add("after_remove");
   loop.config.unsupportedSettings.ticking.blockEntities=false;
   loop.addBlockEntityTicker((TickingBlockEntity)ctor.newInstance(recording("C",trace,null)));
   loop.config.unsupportedSettings.ticking.blockEntities=true;loop.tickBlockEntities();
   var row=new JsonObject();row.addProperty("kind","lifecycle");row.add("trace",gson.toJsonTree(trace));output.println(gson.toJson(row));
  }
 }
}
