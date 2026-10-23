import com.google.gson.*;
import java.io.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.nbt.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.level.*;
import net.minecraft.world.entity.ai.village.poi.*;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.*;
import net.minecraft.world.level.block.state.properties.*;
import net.minecraft.world.level.chunk.*;
import ca.spottedleaf.moonrise.common.util.TickThread;
import ca.spottedleaf.moonrise.patches.chunk_system.level.poi.PoiChunk;

/** Genuine PoiChunk parse/save/load + PoiManager.checkConsistencyWithBlocks.
 * Only world/manager ownership getters and notification sinks are shells;
 * distance propagation is not part of this oracle's claimed boundary. */
public final class PoiChunkProbe {
 static final sun.misc.Unsafe U;
 static {try{var f=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);U=(sun.misc.Unsafe)f.get(null);}catch(Exception e){throw new ExceptionInInitializerError(e);}}
 static RegistryAccess access;
 static void put(Object object,Class<?> owner,String name,Object value)throws Exception{var f=owner.getDeclaredField(name);f.setAccessible(true);f.set(object,value);}
 static final class World extends ServerLevel {
  Manager manager;Cache cache;
  private World(){super(null,null,null,null,null,null,false,0,List.of(),false,null,null,null,null,null,null);}
  @Override public RegistryAccess registryAccess(){return access;}
  @Override public PoiManager getPoiManager(){return manager;}
  @Override public ServerChunkCache getChunkSource(){return cache;}
 }
 static final class Cache extends ServerChunkCache {
  Manager manager;
  private Cache(){super(null,null,null,null,null,null,0,0,false,null,null,null);}
  @Override public PoiManager getPoiManager(){return manager;}
 }
 static class Manager extends PoiManager {
  PoiChunk chunk;JsonArray events;
  Manager(){super(null,null,null,false,null,null,null);}
  @Override public Optional<PoiSection> getOrLoad(long pos){return chunk.getSectionForVanilla(SectionPos.y(pos));}
  @Override protected PoiSection getOrCreate(long pos){return chunk.getOrCreateSection(SectionPos.y(pos));}
  @Override public void setDirty(long pos){chunk.setDirty(true);event("dirty",pos);}
  @Override protected void onSectionLoad(long pos){event("load",pos);}
  void event(String name,long pos){var e=new JsonArray();e.add(name);e.add(SectionPos.y(pos));e.add(chunk.isLoaded());events.add(e);}
 }
 static World world()throws Exception {
  var w=(World)U.allocateInstance(World.class);w.manager=(Manager)U.allocateInstance(Manager.class);w.manager.events=new JsonArray();w.cache=(Cache)U.allocateInstance(Cache.class);w.cache.manager=w.manager;
  put(w,Level.class,"dimension",Level.OVERWORLD);put(w,Level.class,"minY",-16);put(w,Level.class,"maxY",31);put(w,Level.class,"height",48);put(w,Level.class,"minSectionY",-1);put(w,Level.class,"maxSectionY",1);put(w,Level.class,"sectionsCount",3);put(w.manager,PoiManager.class,"world",w);return w;
 }
 static JsonObject snapshot(PoiChunk chunk){var r=new JsonObject();r.addProperty("loaded",chunk.isLoaded());r.addProperty("dirty",chunk.isDirty());r.addProperty("empty",chunk.isEmpty());var tag=chunk.save();if(tag!=null)r.addProperty("saved",tag.toString());return r;}
 static LevelChunkSection blocks(String pattern){
  var section=new LevelChunkSection(new PalettedContainer<BlockState>(Blocks.AIR.defaultBlockState(),Strategy.createForBlockStates(Block.BLOCK_STATE_REGISTRY),null),null);
  if(!pattern.equals("air")){
   section.setBlockState(1,2,3,Blocks.BED.red().defaultBlockState().setValue(BedBlock.PART,BedPart.HEAD),false);
   section.setBlockState(2,1,3,Blocks.BELL.defaultBlockState(),false);
   section.setBlockState(2,2,1,Blocks.COMPOSTER.defaultBlockState(),false);
   section.setBlockState(1,1,2,Blocks.BED.red().defaultBlockState(),false); // foot is not a POI
  }return section;
 }
 static void run(PrintStream out,String name,String input,String pattern)throws Exception{
  var w=world();w.manager.chunk=PoiChunk.parse(w,-2,3,TagParser.parseCompoundFully(input));var chunk=w.manager.chunk;
  var row=new JsonObject();row.addProperty("name",name);row.addProperty("input",input);row.addProperty("pattern",pattern);row.add("before",snapshot(chunk));chunk.load();chunk.load();row.add("loaded",snapshot(chunk));
  for(int y=-1;y<=1;y++)w.manager.checkConsistencyWithBlocks(SectionPos.of(-2,y,3),blocks(y==0?pattern:"air"));
  row.add("after",snapshot(chunk));row.add("events",w.manager.events);out.println(row);
 }
 static void probe(String output)throws Exception{
  try(var out=new PrintStream(output)){
   run(out,"absent_air","{}","air");run(out,"absent_populated","{}","mixed");
   run(out,"valid_empty","{Sections:{'0':{Valid:1b,Records:[]}}}","mixed");
   run(out,"invalid_empty","{Sections:{'0':{Records:[]}}}","mixed");
   run(out,"valid_retained","{Sections:{'0':{Valid:1b,Records:[{pos:[I;-31,2,51],type:'home',free_tickets:0}]}}}","air");
   run(out,"invalid_retained","{Sections:{'0':{Records:[{pos:[I;-31,2,51],type:'meeting',free_tickets:9}]}}}","mixed");
   run(out,"invalid_cleared","{Sections:{'0':{Records:[{pos:[I;-31,2,51],type:'home'}]}}}","air");
   run(out,"malformed_valid","{Sections:{'0':{Valid:'bad',Records:[]}}}","air");
   run(out,"partial_record","{Sections:{'0':{Valid:1b,Records:[{pos:[I;-31,2,51],type:'home'},{pos:[I;0,0,0],type:'missing'}]}}}","mixed");
   run(out,"wrong_section_type","{Sections:{'0':42,'90':{Records:[]}}}","air");
   run(out,"section_order","{Sections:{'1':{Records:[]},'-1':{Records:[]},'0':{Records:[]}}}","air");
  }
 }
 public static void main(String[] args)throws Exception {
  net.minecraft.SharedConstants.tryDetectVersion();Bootstrap.bootStrap();access=RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY);
  Throwable[] error={null};var thread=new TickThread(()->{try{probe(args[0]);}catch(Throwable e){error[0]=e;}},"poi-chunk-oracle");thread.start();thread.join();if(error[0]!=null)throw new RuntimeException(error[0]);
 }
}
