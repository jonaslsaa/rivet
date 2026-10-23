import com.google.gson.*;
import java.io.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.nbt.*;
import net.minecraft.server.*;
import net.minecraft.tags.*;
import net.minecraft.world.entity.ai.village.poi.*;
import net.minecraft.world.level.*;
import ca.spottedleaf.moonrise.common.util.TickThread;
import ca.spottedleaf.moonrise.common.misc.Delayed26WayDistancePropagator3D;
import ca.spottedleaf.moonrise.patches.chunk_system.level.poi.PoiChunk;

/** Real manager source updates/isVillageCenter/sectionsToVillage/tick/unload.
 * The single-chunk holder lookup and dirty bit are controlled ownership sinks;
 * all POI mutations and the distance algorithm are the actual Paper bodies. */
public final class PoiManagerProbe {
 static class Manager extends PoiChunkProbe.Manager {
  @Override public Optional<PoiSection> get(long pos){return chunk!=null&&chunk.isLoaded()&&SectionPos.x(pos)==-2&&SectionPos.z(pos)==3?chunk.getSectionForVanilla(SectionPos.y(pos)):Optional.empty();}
  void update(long pos){try{var m=PoiManager.class.getDeclaredMethod("updateDistanceTracking",long.class);m.setAccessible(true);m.invoke(this,pos);}catch(Exception e){throw new RuntimeException(e);}}
  @Override public void setDirty(long pos){super.setDirty(pos);update(pos);}
  @Override protected void onSectionLoad(long pos){super.onSectionLoad(pos);update(pos);}
 }
 static void snapshot(PrintStream out,String phase,Manager m){
  var row=new JsonObject();row.addProperty("phase",phase);var values=new JsonArray();
  for(int[] d:new int[][]{{0,0,0},{1,0,0},{1,1,1},{6,0,0},{7,0,0},{0,-6,0}})values.add(m.sectionsToVillage(SectionPos.of(-2+d[0],d[1],3+d[2])));
  row.add("distances",values);if(m.chunk!=null)row.add("chunk",PoiChunkProbe.snapshot(m.chunk));out.println(row);
 }
 static void acquire(PoiSection section,BlockPos pos)throws Exception{
  var lookup=PoiSection.class.getDeclaredMethod("getPoiRecord",BlockPos.class);lookup.setAccessible(true);var record=((Optional<?>)lookup.invoke(section,pos)).orElseThrow();
  var acquire=PoiRecord.class.getDeclaredMethod("acquireTicket");acquire.setAccessible(true);acquire.invoke(record);
 }
 static void probe(String path)throws Exception{
  var w=PoiChunkProbe.world();var m=(Manager)PoiChunkProbe.U.allocateInstance(Manager.class);m.events=new JsonArray();w.manager=m;w.cache.manager=m;PoiChunkProbe.put(m,PoiManager.class,"world",w);PoiChunkProbe.put(m,PoiManager.class,"villageDistanceTracker",new Delayed26WayDistancePropagator3D());
  try(var out=new PrintStream(path)){
   var tagrow=new JsonObject();tagrow.addProperty("phase","tag");var tags=new JsonArray();BuiltInRegistries.POINT_OF_INTEREST_TYPE.getOrThrow(PoiTypeTags.VILLAGE).stream().forEach(h->tags.add(h.getRegisteredName()));tagrow.add("types",tags);out.println(tagrow);
   m.chunk=PoiChunk.parse(w,-2,3,TagParser.parseCompoundFully("{Sections:{'0':{Valid:1b,Records:[{pos:[I;-31,2,51],type:'home'}]}}}"));m.chunk.load();snapshot(out,"loaded_occupied",m);
   var home=new BlockPos(-31,2,51);var meeting=new BlockPos(-30,2,51);
   m.release(home);snapshot(out,"released",m);
   acquire(m.chunk.getSection(0),home);snapshot(out,"reacquired",m);
   m.add(meeting,BuiltInRegistries.POINT_OF_INTEREST_TYPE.getOrThrow(PoiTypes.MEETING));acquire(m.chunk.getSection(0),meeting);m.remove(home);snapshot(out,"meeting_remains",m);
   m.remove(meeting);snapshot(out,"all_removed",m);
   m.add(home,BuiltInRegistries.POINT_OF_INTEREST_TYPE.getOrThrow(PoiTypes.HOME));acquire(m.chunk.getSection(0),home);snapshot(out,"occupied_again",m);
   m.chunk=null;m.moonrise$onUnload(ChunkPos.pack(-2,3));snapshot(out,"unloaded",m);
  }
 }
 public static void main(String[] args)throws Exception{
  net.minecraft.SharedConstants.tryDetectVersion();Bootstrap.bootStrap();PoiChunkProbe.access=RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY);
  var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();MinecraftServer.configurePackRepository(packs,WorldDataConfiguration.DEFAULT,true,false);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected())){
   for(var pending:TagLoader.loadTagsForExistingRegistries(resources,PoiChunkProbe.access))pending.apply();
  }
  Throwable[] error={null};var thread=new TickThread(()->{try{probe(args[0]);}catch(Throwable e){error[0]=e;}},"poi-manager-oracle");thread.start();thread.join();if(error[0]!=null)throw new RuntimeException(error[0]);
 }
}
