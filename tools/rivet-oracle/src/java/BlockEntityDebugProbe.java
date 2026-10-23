import com.google.gson.*;
import io.netty.buffer.*;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.network.*;
import net.minecraft.network.protocol.game.*;
import net.minecraft.server.*;
import net.minecraft.server.level.*;
import net.minecraft.util.debug.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.entity.*;
import net.minecraft.world.level.block.state.*;

/** Actual LevelDebugSynchronizers registration, DebugHiveInfo.pack (including
 * actual CampfireBlock collision smoke tests), and actual packet codec.
 * Getter-only Level shell supplies a controlled block map. Awake is installed
 * reflectively: this does NOT claim wakeUp's chunk/entity/POI traversal or
 * multiplayer fanout is exercised. No debug producer/codec is mocked. */
public final class BlockEntityDebugProbe {
 static final sun.misc.Unsafe U;
 static {try{var f=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);U=(sun.misc.Unsafe)f.get(null);}catch(Exception e){throw new ExceptionInInitializerError(e);}}
 static RegistryAccess access;
 static final BlockPos POS=new BlockPos(8,64,8);
 static final class World extends ServerLevel {
  Map<BlockPos,BlockState> blocks;
  private World(){super(null,null,null,null,null,null,false,0,List.of(),false,null,null,null,null,null,null);}
  @Override public RegistryAccess registryAccess(){return access;}
  @Override public BlockState getBlockState(BlockPos pos){return blocks.getOrDefault(pos,Blocks.AIR.defaultBlockState());}
 }
 static Field field(Class<?> type,String name)throws Exception{var f=type.getDeclaredField(name);f.setAccessible(true);return f;}
 static int hiveCount(LevelDebugSynchronizers sync)throws Exception{
  var sources=(Map<?,?>)field(LevelDebugSynchronizers.class,"sourceSynchronizers").get(sync);
  Object source=sources.get(DebugSubscriptions.BEE_HIVES);
  return ((Map<?,?>)field(source.getClass(),"blockEntitySources").get(source)).size();
 }
 static void registration(PrintStream out,World world,BlockEntity entity,String family)throws Exception{
  var sync=new LevelDebugSynchronizers(world);
  sync.registerBlockEntity(entity);
  var row=new JsonObject();row.addProperty("mode","registration");row.addProperty("family",family);row.addProperty("sleeping_count",hiveCount(sync));
  field(LevelDebugSynchronizers.class,"sleeping").setBoolean(sync,false);
  sync.registerBlockEntity(entity);row.addProperty("awake_count",hiveCount(sync));
  sync.registerBlockEntity(entity);row.addProperty("repeat_count",hiveCount(sync));
  sync.tick(new ServerDebugSubscribers(null));row.addProperty("sleep_after_tick",field(LevelDebugSynchronizers.class,"sleeping").getBoolean(sync));row.addProperty("count_after_sleep_tick",hiveCount(sync));out.println(row);
 }
 static void hive(PrintStream out,World world,String name,BlockState obstacle,int distance,boolean fireBelow,int bees)throws Exception{
  world.blocks.clear();
  if(obstacle!=null)world.blocks.put(POS.below(distance),obstacle);
  if(fireBelow)world.blocks.put(POS.below(distance+1),Blocks.CAMPFIRE.defaultBlockState());
  var state=Blocks.BEE_NEST.defaultBlockState().setValue(BeehiveBlock.HONEY_LEVEL,3);
  var entity=new BeehiveBlockEntity(POS,state);entity.setLevel(world);
  for(int i=0;i<bees;i++)entity.storeBee(BeehiveBlockEntity.Occupant.create(7+i));
  var value=DebugHiveInfo.pack(entity);
  var row=new JsonObject();row.addProperty("mode","hive");row.addProperty("name",name);row.addProperty("distance",distance);row.addProperty("fire_below",fireBelow);row.addProperty("bees",bees);
  row.addProperty("obstacle",obstacle==null?-1:Block.getId(obstacle));row.addProperty("block",BuiltInRegistries.BLOCK.getId(value.type()));row.addProperty("occupants",value.occupantCount());row.addProperty("honey",value.honeyLevel());row.addProperty("sedated",value.sedated());
  var buf=new RegistryFriendlyByteBuf(Unpooled.buffer(),access);
  ClientboundDebugBlockValuePacket.STREAM_CODEC.encode(buf,new ClientboundDebugBlockValuePacket(POS,DebugSubscriptions.BEE_HIVES.packUpdate(value)));
  row.addProperty("packet",ByteBufUtil.hexDump(buf));buf.release();
  buf=new RegistryFriendlyByteBuf(Unpooled.buffer(),access);
  ClientboundDebugBlockValuePacket.STREAM_CODEC.encode(buf,new ClientboundDebugBlockValuePacket(POS,DebugSubscriptions.BEE_HIVES.emptyUpdate()));
  row.addProperty("clear_packet",ByteBufUtil.hexDump(buf));buf.release();out.println(row);
 }
 public static void main(String[] args)throws Exception{
  net.minecraft.SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
  var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();MinecraftServer.configurePackRepository(packs,WorldDataConfiguration.DEFAULT,true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var out=new PrintStream(args[0])){
   var layers=RegistryLayer.createRegistryAccess();var pending=net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(RegistryLayer.STATIC));var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(RegistryLayer.WORLDGEN),pending);pending.forEach(tags -> tags.apply());
   var worldgen=net.minecraft.resources.RegistryDataLoader.load(resources,lookups,net.minecraft.resources.RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();access=layers.replaceFrom(RegistryLayer.WORLDGEN,worldgen).compositeAccess().freeze();
   var world=(World)U.allocateInstance(World.class);world.blocks=new HashMap<>();
   var registry=new JsonObject();registry.addProperty("mode","registry");var names=new JsonArray();for(var subscription:BuiltInRegistries.DEBUG_SUBSCRIPTION)names.add(BuiltInRegistries.DEBUG_SUBSCRIPTION.getKey(subscription).toString());registry.add("names",names);out.println(registry);
   registration(out,world,new ChestBlockEntity(POS,Blocks.CHEST.defaultBlockState()),"chest");registration(out,world,new SpawnerBlockEntity(POS,Blocks.SPAWNER.defaultBlockState()),"spawner");registration(out,world,new CreakingHeartBlockEntity(POS,Blocks.CREAKING_HEART.defaultBlockState()),"heart");registration(out,world,new BeehiveBlockEntity(POS,Blocks.BEE_NEST.defaultBlockState()),"hive");
   hive(out,world,"air",null,1,false,0);hive(out,world,"lit",Blocks.CAMPFIRE.defaultBlockState(),1,false,2);hive(out,world,"unlit",Blocks.CAMPFIRE.defaultBlockState().setValue(CampfireBlock.LIT,false),1,false,1);hive(out,world,"blocked",Blocks.STONE.defaultBlockState(),1,false,1);hive(out,world,"block_with_fire",Blocks.STONE.defaultBlockState(),1,true,1);hive(out,world,"fifth_block_with_sixth_fire",Blocks.STONE.defaultBlockState(),5,true,1);hive(out,world,"sixth_fire_out_of_range",Blocks.CAMPFIRE.defaultBlockState(),6,false,1);hive(out,world,"slab_with_fire",Blocks.OAK_SLAB.defaultBlockState(),2,true,1);
  }
 }
}
