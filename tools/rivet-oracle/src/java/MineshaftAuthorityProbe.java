import com.google.gson.*;
import java.io.*;
import java.lang.reflect.*;
import net.minecraft.core.*;
import net.minecraft.nbt.*;
import net.minecraft.resources.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.*;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.levelgen.structure.*;
import net.minecraft.world.level.levelgen.structure.structures.MineshaftPieces;
import net.minecraft.world.level.storage.loot.LootTable;

/** Positive source branch ENTRY evidence only. No fake successful entity or
 * Craft bridge: genuine source stops at first required external authority. */
public final class MineshaftAuthorityProbe {
 static final class Entry extends RuntimeException {
  final String kind;final BlockPos pos;final int flags;
  Entry(String kind,BlockPos pos,int flags){this.kind=kind;this.pos=pos.immutable();this.flags=flags;}
 }
 static BlockPos pendingChest;
 static final class Corridor extends MineshaftPieces.MineShaftCorridor {
  Corridor(CompoundTag tag){super(tag);}
  @Override protected boolean createChest(WorldGenLevel level,BoundingBox clip,net.minecraft.util.RandomSource random,int x,int y,int z,ResourceKey<LootTable> loot){
   pendingChest=this.getWorldPos(x,y,z).immutable();
   try{return super.createChest(level,clip,random,x,y,z,loot);}
   finally{pendingChest=null;}
  }
  @Override protected void placeCraftSpawner(ServerLevelAccessor level,BlockPos pos,org.bukkit.entity.EntityType type,int flags){
   if(type!=org.bukkit.entity.EntityType.CAVE_SPIDER)throw new AssertionError(type);
   throw new Entry("craft_spawner",pos,flags);
  }
 }
 static WorldGenLevel intercepted(MineshaftPlacementProbe.Ground ground){
  var delegate=ground.level();
  return (WorldGenLevel)Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(),new Class<?>[]{WorldGenLevel.class},(proxy,method,args)->{
   if(method.getName().equals("getLevel")&&pendingChest!=null)throw new Entry("chest_minecart",pendingChest,2);
   try{return method.invoke(delegate,args);}catch(InvocationTargetException e){throw e.getCause();}
  });
 }
 static JsonObject run(String target,long seed)throws Exception{
  var input=MineshaftPlacementProbe.pieceTag("mscorridor",new int[]{0,50,0,2,52,9},0,0);
  input.putInt("Num",2);input.putBoolean("hr",false);input.putBoolean("sc",target.equals("craft_spawner"));input.putBoolean("hps",false);
  var piece=new Corridor(input);var ground=new MineshaftPlacementProbe.Ground("stone");var random=new LegacyRandomSource(seed);var clip=new BoundingBox(-32,-64,-32,32,319,32);
  Entry entry;
  try{piece.postProcess(intercepted(ground),null,null,random,clip,ChunkPos.ZERO,BlockPos.ZERO);return null;}
  catch(Entry captured){entry=captured;}
  if(!entry.kind.equals(target))return null;
  var row=new JsonObject();row.addProperty("boundary",entry.kind);row.addProperty("seed",seed);row.addProperty("input",new Corridor(input).createTag(null).toString());row.addProperty("terrain","stone");
  row.add("position",new Gson().toJsonTree(new int[]{entry.pos.getX(),entry.pos.getY(),entry.pos.getZ()}));row.addProperty("flags",entry.flags);
  row.add("clip",new Gson().toJsonTree(new int[]{-32,-64,-32,32,319,32}));
  row.addProperty("writes",ground.writeCount);row.addProperty("writes_sha256",MineshaftPlacementProbe.hash(ground.writes.toByteArray()));
  row.addProperty("states",ground.states.size());row.addProperty("states_sha256",ground.statesHash());
  row.addProperty("marks",ground.markCount);row.addProperty("marks_sha256",MineshaftPlacementProbe.hash(ground.marks.toByteArray()));
  row.addProperty("ticks",ground.ticks.size());row.addProperty("ticks_sha256",MineshaftPlacementProbe.hash(String.join("\n",ground.ticks).getBytes(java.nio.charset.StandardCharsets.UTF_8)));
  row.addProperty("rng_next",random.nextLong());row.addProperty("saved",piece.createTag(null).toString());
  row.addProperty("scope",target.equals("chest_minecart")?"actual createChest preamble+rail placement; intercepted getLevel before EntityType.create; loot/admission NOT executed":"actual corridor branch; intercepted placeCraftSpawner ENTRY; Craft snapshot placement NOT executed");
  return row;
 }
 public static void main(String[] args)throws Exception{
  try(var output=new PrintStream(args[0])){
   net.minecraft.SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
   var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
   net.minecraft.server.MinecraftServer.configurePackRepository(packs,new WorldDataConfiguration(new DataPackConfig(java.util.List.of("vanilla"),java.util.List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
   try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected())){
    var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
    var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();
    MineshaftPlacementProbe.access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess();
    MineshaftPlacementProbe.factory=net.minecraft.world.level.chunk.PalettedContainerFactory.create(MineshaftPlacementProbe.access);
    for(String target:new String[]{"chest_minecart","craft_spawner"}){
     // Fixed positive seeds discovered by one bounded search. Replays execute
     // exactly these two genuine source cases, not a seed matrix.
     var row=run(target,target.equals("chest_minecart")?23L:0L);
     if(row==null)throw new AssertionError("Expected actual positive branch was not reached: "+target);
     output.println(row);output.flush();
    }
   }
  }
 }
}
