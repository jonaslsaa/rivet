import com.google.gson.*;
import java.io.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.nbt.*;
import net.minecraft.server.*;
import net.minecraft.server.level.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.entity.*;
import net.minecraft.world.level.block.state.*;
import net.minecraft.world.level.chunk.*;
import net.minecraft.world.level.chunk.status.*;

/** Actual ProtoChunk and WorldGenRegion.setBlock/getBlockEntity transitions.
 * Region spatial lookup and immutable world registry getters are shells;
 * no lifecycle, constructors, save, pending/live maps or promotion is mocked.
 * Does not exercise FULL registration, tickers, game events or entity AI. */
public final class BlockEntityLifecycleProbe {
 static final sun.misc.Unsafe U;
 static {try{var f=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);U=(sun.misc.Unsafe)f.get(null);}catch(Exception e){throw new ExceptionInInitializerError(e);}}
 static RegistryAccess access;
 static final class World extends ServerLevel {
  private World(){super(null,null,null,null,null,null,false,0,List.of(),false,null,null,null,null,null,null);}
  @Override public RegistryAccess registryAccess(){return access;}
 }
 static final class Region extends WorldGenRegion {
  ProtoChunk proto;
  private Region(){super(null,null,null,null);}
  @Override public boolean ensureCanWrite(BlockPos pos){return true;}
  @Override public ChunkAccess getChunk(int x,int z,ChunkStatus status,boolean create){if(x!=0||z!=0)throw new AssertionError("outside oracle chunk");return proto;}
 }
 static void capture(PrintStream out,String family,String phase,Region region,BlockPos pos,BlockEntity original)throws Exception {
  var proto=region.proto;var row=new JsonObject();row.addProperty("family",family);row.addProperty("phase",phase);
  row.addProperty("state",Block.getId(proto.getBlockState(pos)));
  var live=proto.getBlockEntity(pos);row.addProperty("live",live!=null);row.addProperty("same_identity",original!=null&&live==original);if(live!=null)row.addProperty("cached_state",Block.getId(live.getBlockState()));
  var pending=proto.getBlockEntityNbt(pos);if(pending!=null)row.addProperty("pending",pending.toString());
  var saved=proto.getBlockEntityNbtForSaving(pos,access);if(saved!=null)row.addProperty("saved",saved.toString());
  out.println(row);
 }
 static void run(PrintStream out,String family,Block block)throws Exception {
  var region=(Region)U.allocateInstance(Region.class);
  region.proto=new ProtoChunk(new ChunkPos(0,0),UpgradeData.EMPTY,LevelHeightAccessor.create(-64,384),PalettedContainerFactory.create(access),null);
  var f=WorldGenRegion.class.getDeclaredField("level");f.setAccessible(true);f.set(region,U.allocateInstance(World.class));
  var pos=new BlockPos(8,64,8);var state=block.defaultBlockState();int flags=Block.UPDATE_KNOWN_SHAPE|Block.UPDATE_SKIP_POI;
  region.setBlock(pos,state,flags,0);capture(out,family,"pending",region,pos,null);
  var live=region.getBlockEntity(pos);if(live==null)throw new AssertionError("actual native promotion failed");
  if(live instanceof BeehiveBlockEntity hive)hive.storeBee(BeehiveBlockEntity.Occupant.create(77));
  if(live instanceof ChestBlockEntity chest)chest.setLootTable(net.minecraft.world.level.storage.loot.BuiltInLootTables.ABANDONED_MINESHAFT,42);
  capture(out,family,"live",region,pos,live);
  if(state.hasProperty(net.minecraft.world.level.block.state.properties.BlockStateProperties.WATERLOGGED))state=state.setValue(net.minecraft.world.level.block.state.properties.BlockStateProperties.WATERLOGGED,true);
  if(state.hasProperty(BeehiveBlock.HONEY_LEVEL))state=state.setValue(BeehiveBlock.HONEY_LEVEL,2);
  if(state.hasProperty(CreakingHeartBlock.NATURAL))state=state.setValue(CreakingHeartBlock.NATURAL,true);
  region.setBlock(pos,state,flags,0);capture(out,family,"same_block",region,pos,live);
  var ignored=new CompoundTag();ignored.putInt("x",8);ignored.putInt("y",64);ignored.putInt("z",8);ignored.putString("id","DUMMY");ignored.putInt("unexpected",9);
  region.proto.setBlockEntityNbt(ignored);capture(out,family,"pending_write_guard",region,pos,live);
  region.setBlock(pos,Blocks.STONE.defaultBlockState(),flags,0);capture(out,family,"removed",region,pos,live);
 }
 public static void main(String[] args)throws Exception {
  net.minecraft.SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
  if(args.length>1&&args[1].equals("spawner")) {
   access=RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY);
   try(var out=new PrintStream(args[0])) {
    for(boolean weighted:new boolean[]{false,true}) {
     String input="{id:'minecraft:mob_spawner',Paper.Delay:-40000,MinSpawnDelay:-50000,MaxSpawnDelay:90000,SpawnCount:70000,MaxNearbyEntities:80000,RequiredPlayerRange:-40001,SpawnRange:99999,"+(weighted?"SpawnPotentials:[{weight:2,data:{entity:{id:'skeleton',custom:7}}},{weight:3,data:{entity:{id:'creeper',custom:9}}}]":"SpawnData:{entity:{id:'skeleton',custom:11}},SpawnPotentials:[]")+"}";
     var pos=new BlockPos(8,64,8);var entity=new SpawnerBlockEntity(pos,Blocks.SPAWNER.defaultBlockState());
     entity.loadWithComponents(net.minecraft.world.level.storage.TagValueInput.create(net.minecraft.util.ProblemReporter.DISCARDING,access,TagParser.parseCompoundFully(input)));
     var random=new net.minecraft.world.level.levelgen.LegacyRandomSource(42);entity.setEntityId(net.minecraft.world.entity.EntityTypes.ZOMBIE,random);
     var row=new JsonObject();row.addProperty("weighted",weighted);row.addProperty("input",input);row.addProperty("saved",entity.saveWithFullMetadata(access).toString());row.addProperty("spawn_count",entity.getSpawner().spawnCount);row.addProperty("min_delay",entity.getSpawner().minSpawnDelay);row.addProperty("world_next_long",random.nextLong());out.println(row);
    }
   }
   return;
  }
  var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();MinecraftServer.configurePackRepository(packs,WorldDataConfiguration.DEFAULT,true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var out=new PrintStream(args[0])) {
   var layers=RegistryLayer.createRegistryAccess();var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(RegistryLayer.STATIC)));
   var worldgen=net.minecraft.resources.RegistryDataLoader.load(resources,lookups,net.minecraft.resources.RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();access=layers.replaceFrom(RegistryLayer.WORLDGEN,worldgen).compositeAccess().freeze();
   run(out,"chest",Blocks.CHEST);run(out,"beehive",Blocks.BEE_NEST);run(out,"spawner",Blocks.SPAWNER);run(out,"heart",Blocks.CREAKING_HEART);
  }
 }
}
