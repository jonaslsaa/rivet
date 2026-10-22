import com.google.gson.*;
import java.io.*;
import java.lang.reflect.Proxy;
import java.security.MessageDigest;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.nbt.*;
import net.minecraft.resources.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.*;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.levelgen.structure.*;
import net.minecraft.world.level.levelgen.structure.structures.MineshaftPieces;

/** Genuine MineshaftPieces postProcess on controlled map-backed world storage.
 * Native ProtoChunks implement marking. No block algorithm is mocked.
 * Entity/Craft-world access refuses: representative cases intentionally do not
 * claim positive entity/spawner authority, which requires a live server backend.
 */
public final class MineshaftPlacementProbe {
 static RegistryAccess access;
 static PalettedContainerFactory factory;
 static final class Ground {
  final String terrain;final Map<BlockPos,BlockState> states=new HashMap<>();
  final ByteArrayOutputStream writes=new ByteArrayOutputStream(),marks=new ByteArrayOutputStream();
  final DataOutputStream writeStream=new DataOutputStream(writes),markStream=new DataOutputStream(marks);
  final List<String> ticks=new ArrayList<>();final Map<ChunkPos,ProtoChunk> chunks=new HashMap<>();
  int writeCount,markCount;
  Ground(String terrain){this.terrain=terrain;}
  BlockState get(BlockPos p){
   var state=states.get(p);if(state!=null)return state;
   if(terrain.equals("void"))return Blocks.AIR.defaultBlockState();
   if(terrain.equals("chain_roof")||terrain.equals("falling_roof"))return (p.getY()==53?(terrain.equals("chain_roof")?Blocks.STONE:Blocks.SAND):Blocks.AIR).defaultBlockState();
   if(terrain.equals("water")&&p.equals(new BlockPos(-1,50,0)))return Blocks.WATER.defaultBlockState();
   return (p.getY()<=70?Blocks.STONE:Blocks.AIR).defaultBlockState();
  }
  int height(Heightmap.Types kind,int x,int z){for(int y=319;y>=-64;y--)if(kind.isOpaque().test(get(new BlockPos(x,y,z))))return y+1;return -64;}
  final class MarkedChunk extends ProtoChunk {
   MarkedChunk(ChunkPos pos){super(pos,UpgradeData.EMPTY,LevelHeightAccessor.create(-64,384),factory,null);}
   @Override public void markPosForPostProcessing(BlockPos pos){
    super.markPosForPostProcessing(pos);
    try {markStream.writeInt(pos.getX());markStream.writeInt(pos.getY());markStream.writeInt(pos.getZ());markCount++;}catch(IOException e){throw new UncheckedIOException(e);}
   }
  }
  WorldGenLevel level(){
   return (WorldGenLevel)Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(),new Class<?>[]{WorldGenLevel.class},(proxy,method,args)->switch(method.getName()){
    case "getBlockState"->get((BlockPos)args[0]);
    case "getFluidState"->get((BlockPos)args[0]).getFluidState();
    case "getMinY"->-64;case "getMaxY"->319;
    case "getHeight"->args==null||args.length==0?384:height((Heightmap.Types)args[0],(int)args[1],(int)args[2]);
    case "getBiome"->access.lookupOrThrow(Registries.BIOME).getOrThrow(terrain.equals("blocked")?net.minecraft.world.level.biome.Biomes.DEEP_DARK:net.minecraft.world.level.biome.Biomes.PLAINS);
    case "registryAccess"->access;
    case "setBlock"->{
     var p=((BlockPos)args[0]).immutable();var state=(BlockState)args[1];states.put(p,state);
     writeStream.writeInt(p.getX());writeStream.writeInt(p.getY());writeStream.writeInt(p.getZ());writeStream.writeInt(Block.getId(state));writeStream.writeInt((int)args[2]);writeCount++;yield true;
    }
    case "getChunk"->{
     var pos=args[0] instanceof BlockPos p?new ChunkPos(p.getX()>>4,p.getZ()>>4):new ChunkPos((int)args[0],(int)args[1]);
     yield chunks.computeIfAbsent(pos,MarkedChunk::new);
    }
    case "scheduleTick"->{var p=(BlockPos)args[0];String kind=args[1] instanceof net.minecraft.world.level.material.Fluid f?BuiltInRegistries.FLUID.getKey(f).toString():BuiltInRegistries.BLOCK.getKey((Block)args[1]).toString();ticks.add(p.getX()+","+p.getY()+","+p.getZ()+","+kind+","+args[2]);yield null;}
    case "getSeed"->42L;
    case "getLevel"->throw new UnsupportedOperationException("genuine Craft/entity authority intentionally unavailable in map-backed placement oracle");
    case "toString"->"MineshaftPlacementStorage";
    default->throw new UnsupportedOperationException(method.toString());
   });
  }
  String statesHash()throws Exception{
   var bytes=new ByteArrayOutputStream();var stream=new DataOutputStream(bytes);
   var entries=new ArrayList<>(states.entrySet());entries.sort(Comparator.comparingInt((Map.Entry<BlockPos,BlockState> e)->e.getKey().getX()).thenComparingInt(e->e.getKey().getY()).thenComparingInt(e->e.getKey().getZ()));
   for(var e:entries){stream.writeInt(e.getKey().getX());stream.writeInt(e.getKey().getY());stream.writeInt(e.getKey().getZ());stream.writeInt(Block.getId(e.getValue()));}
   return hash(bytes.toByteArray());
  }
 }
 static String hash(byte[] bytes)throws Exception{return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));}
 static CompoundTag pieceTag(String id,int[] box,int orientation,int type)throws Exception{
  var t=new CompoundTag();t.putString("id","minecraft:"+id);t.putIntArray("BB",box);t.putInt("O",orientation);t.putInt("GD",3);t.putInt("MST",type);return t;
 }
 static StructurePiece piece(CompoundTag t){
  return switch(t.getStringOr("id","")){
   case "minecraft:msroom"->new MineshaftPieces.MineShaftRoom(t);
   case "minecraft:mscrossing"->new MineshaftPieces.MineShaftCrossing(t);
   case "minecraft:msstairs"->new MineshaftPieces.MineShaftStairs(t);
   case "minecraft:mscorridor"->new MineshaftPieces.MineShaftCorridor(t);
   default->throw new AssertionError(t);
  };
 }
 static void run(PrintStream out,String name,CompoundTag tag,String terrain,long seed,BoundingBox clip)throws Exception{
  var piece=piece(tag);var ground=new Ground(terrain);var random=new LegacyRandomSource(seed);
  piece.postProcess(ground.level(),null,null,random,clip,ChunkPos.ZERO,BlockPos.ZERO);
  var row=new JsonObject();row.addProperty("case",name);row.addProperty("input",piece(tag).createTag(null).toString());row.addProperty("terrain",terrain);row.addProperty("seed",seed);
  row.add("clip",new Gson().toJsonTree(new int[]{clip.minX(),clip.minY(),clip.minZ(),clip.maxX(),clip.maxY(),clip.maxZ()}));
  row.addProperty("writes",ground.writeCount);row.addProperty("writes_sha256",hash(ground.writes.toByteArray()));row.addProperty("states",ground.states.size());row.addProperty("states_sha256",ground.statesHash());
  row.addProperty("marks",ground.markCount);row.addProperty("marks_sha256",hash(ground.marks.toByteArray()));row.addProperty("ticks",ground.ticks.size());row.addProperty("ticks_sha256",hash(String.join("\n",ground.ticks).getBytes(java.nio.charset.StandardCharsets.UTF_8)));
  row.addProperty("rng_next",random.nextLong());row.addProperty("saved",piece.createTag(null).toString());out.println(row);out.flush();
 }
 public static void main(String[] args)throws Exception{
  try(var out=new PrintStream(args[0])){
   SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
   if(args.length>1&&args[1].equals("falling")){for(var b:BuiltInRegistries.BLOCK)if(b instanceof FallingBlock)out.println(BuiltInRegistries.BLOCK.getKey(b));return;}
   var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
   net.minecraft.server.MinecraftServer.configurePackRepository(packs,new WorldDataConfiguration(new DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
   try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected())){
    var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
    var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess();factory=PalettedContainerFactory.create(access);
    var clip=new BoundingBox(-32,-64,-32,32,319,32);
    var room=pieceTag("msroom",new int[]{-3,50,-3,5,58,4},-1,0);room.put("Entrances",TagParser.parseCompoundFully("{x:[[I;-3,51,-3,-2,53,-1]]}").get("x"));
    for(String terrain:new String[]{"stone","void","water","blocked"})run(out,"room_"+terrain,room,terrain,0,clip);
    for(boolean upper:new boolean[]{false,true}){var cross=pieceTag("mscrossing",new int[]{0,50,0,4,upper?56:52,4},-1,upper?1:0);cross.putBoolean("tf",upper);cross.putByte("D",(byte)0);run(out,"cross_"+upper,cross,"stone",42,clip);}
    for(int orientation=0;orientation<4;orientation++){
     int[] box=switch(orientation){case 0->new int[]{0,50,0,2,57,8};case 1->new int[]{-8,50,0,0,57,2};case 2->new int[]{0,50,-8,2,57,0};default->new int[]{0,50,0,8,57,2};};
     run(out,"stairs_"+orientation,pieceTag("msstairs",box,orientation,orientation%2),"stone",42,clip);
    }
    var corridor=pieceTag("mscorridor",new int[]{0,50,0,2,52,9},0,0);corridor.putInt("Num",2);corridor.putBoolean("hr",false);corridor.putBoolean("sc",false);corridor.putBoolean("hps",false);
    for(String terrain:new String[]{"stone","void","water","blocked","chain_roof","falling_roof"})run(out,"corridor_"+terrain,corridor,terrain,0,clip);
    run(out,"corridor_clipped",corridor,"stone",0,new BoundingBox(0,-64,2,1,319,6));
    var rails=corridor.copy();rails.putIntArray("BB",new int[]{0,50,0,9,52,2});rails.putInt("O",3);rails.putInt("MST",1);rails.putBoolean("hr",true);run(out,"corridor_rails",rails,"stone",42,clip);
    var spider=corridor.copy();spider.putBoolean("sc",true);spider.putBoolean("hps",true);run(out,"spider_already_placed",spider,"stone",42,clip);spider.putBoolean("hps",false);run(out,"spider_exterior",spider,"void",42,clip);
   }
  }
 }
}
