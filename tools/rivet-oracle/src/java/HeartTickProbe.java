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
import net.minecraft.world.Difficulty;
import net.minecraft.world.attribute.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.entity.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.CreakingHeartState;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.storage.*;
import net.minecraft.world.phys.Vec3;

/** Actual CreakingHeartBlockEntity.serverTick, persistence and transient state.
 * The explicit world has no protector and no nearby player. This exercises
 * real UUID grace expiry, axis/log gates, state writes, schedule RNG and orphan
 * hurt-emitter sounds without pretending Creaking AI/spawning is implemented. */
public final class HeartTickProbe {
 static final BlockPos POS=new BlockPos(3,64,-8);
 static final class World extends ServerLevel {
  EnvironmentAttributeSystem attributes;LegacyRandomSource rng;boolean logs;
  BlockState state;List<String> trace;int lookups;List<String> reads;
  private World(){super(null,null,null,null,null,null,false,0L,List.of(),false,null,null,null,null,null,null);}
  @Override public RandomSource getRandom(){return rng;}
  @Override public EnvironmentAttributeSystem environmentAttributes(){return attributes;}
  @Override public BlockState getBlockState(BlockPos pos){reads.add(pos.getX()+","+pos.getY()+","+pos.getZ());return logs?Blocks.PALE_OAK_LOG.defaultBlockState():Blocks.AIR.defaultBlockState();}
  @Override public Entity getEntity(UUID uuid){lookups++;trace.add("lookup");return null;}
  @Override public void blockEntityChanged(BlockPos pos){trace.add("changed");}
  @Override public void updateNeighbourForOutputSignal(BlockPos pos,Block block){trace.add("neighbors");}
  @Override public boolean setBlock(BlockPos pos,BlockState state,int flags){if(flags!=3)throw new AssertionError(flags);this.state=state;trace.add("state:"+state.getValue(CreakingHeartBlock.STATE).getSerializedName());return true;}
  @Override public boolean isSpawningMonsters(){return true;}
  @Override public LevelData getLevelData(){return (LevelData)Proxy.newProxyInstance(LevelData.class.getClassLoader(),new Class[]{LevelData.class},(p,m,a)->{if(m.getName().equals("getDifficulty"))return Difficulty.NORMAL;throw new AssertionError(m);});}
  @Override public Player getNearestPlayerThatAffectsSpawning(double x,double y,double z,double range,boolean creative){if(x!=3||y!=64||z!=-8||range!=32||creative)throw new AssertionError();trace.add("nearest");return null;}
  @Override public void playSound(Entity except,BlockPos pos,SoundEvent sound,SoundSource source,float volume,float pitch){trace.add("sound:"+pos.getX()+","+pos.getY()+","+pos.getZ()+":"+Float.floatToRawIntBits(volume));}
 }
 static void field(Object target,String name,Object value)throws Exception{
  var f=CreakingHeartBlockEntity.class.getDeclaredField(name);f.setAccessible(true);f.set(target,value);
 }
 static Object field(Object target,String name)throws Exception{
  var f=CreakingHeartBlockEntity.class.getDeclaredField(name);f.setAccessible(true);return f.get(target);
 }
 public static void main(String[] args)throws Exception{
  SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
  var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
  MinecraftServer.configurePackRepository(packs,new net.minecraft.world.level.WorldDataConfiguration(new net.minecraft.world.level.DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected())){
   net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,RegistryLayer.createRegistryAccess().getLayer(RegistryLayer.STATIC)).forEach(pending->pending.apply());
  }
  var registries=RegistryLayer.createRegistryAccess().getLayer(RegistryLayer.STATIC);
  try(var output=new PrintWriter(args[0])){
   for(boolean uuid:new boolean[]{false,true})for(boolean active:new boolean[]{false,true})for(boolean logs:new boolean[]{false,true})for(int ticks:new int[]{1,2,30,55})for(int emitter:new int[]{0,60}){
    var world=(World)ChestMinecartProbe.UNSAFE.allocateInstance(World.class);world.logs=logs;world.rng=new LegacyRandomSource(99);world.trace=new ArrayList<>();world.reads=new ArrayList<>();
    world.attributes=EnvironmentAttributeSystem.builder().addConstantLayer(EnvironmentAttributes.CREAKING_ACTIVE,old->active).build();
    world.state=Blocks.CREAKING_HEART.defaultBlockState().setValue(CreakingHeartBlock.STATE,CreakingHeartState.DORMANT);
    var heart=new CreakingHeartBlockEntity(POS,world.state);String input=uuid?"{creaking:[I;0,1,0,2]}":"{}";
    heart.loadWithComponents(TagValueInput.create(ProblemReporter.DISCARDING,registries,TagParser.parseCompoundFully(input)));heart.setLevel(world);
    field(heart,"emitter",emitter);if(emitter!=0)field(heart,"emitterTarget",new Vec3(10,65,-4));
    for(int i=0;i<ticks;i++){CreakingHeartBlockEntity.serverTick(world,POS,world.state,heart);heart.setBlockState(world.state);}
    var row=new JsonObject();row.addProperty("input_snbt",input);row.addProperty("active",active);row.addProperty("logs",logs);row.addProperty("ticks",ticks);row.addProperty("initial_emitter",emitter);row.addProperty("state",world.state.getValue(CreakingHeartBlock.STATE).getSerializedName());
    row.addProperty("output_snbt",heart.saveWithFullMetadata(registries).toString());row.addProperty("ticks_existed",(long)field(heart,"ticksExisted"));row.addProperty("ticker",(int)field(heart,"ticker"));row.addProperty("emitter",(int)field(heart,"emitter"));row.addProperty("output_signal",heart.getAnalogOutputSignal());row.addProperty("rng_next",world.rng.nextLong());row.add("trace",new Gson().toJsonTree(world.trace));row.add("reads",new Gson().toJsonTree(world.reads));output.println(row);
   }
  }
 }
}
