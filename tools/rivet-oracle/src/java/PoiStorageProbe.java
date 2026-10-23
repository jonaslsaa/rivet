import com.google.gson.*;
import java.io.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.nbt.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.*;
import net.minecraft.world.level.chunk.storage.*;
import ca.spottedleaf.moonrise.common.util.TickThread;
import ca.spottedleaf.moonrise.patches.chunk_system.level.poi.PoiChunk;

/** Genuine Paper POI parse/load/consistency/save through real RegionFileStorage
 * write/flush/close/reopen/read/delete. Reuses controlled world getter shells,
 * never replaces POI algorithms or region IO with fixture expectations. */
public final class PoiStorageProbe {
 static void probe(String output)throws Exception {
  RegionFileVersion.configure("none");var root=Files.createTempDirectory("rivet-poi-storage-oracle-");
  try(var out=new PrintStream(output)){
   int index=0;
   for(String name:new String[]{"fresh_populated","invalid_repaired","valid_occupied_retained"}){
    var folder=root.resolve(Integer.toString(index++));var info=new RegionStorageInfo("probe",Level.OVERWORLD,"poi");var pos=new ChunkPos(-2,3);
    var w=PoiChunkProbe.world();var row=new JsonObject();row.addProperty("name",name);
    String input=switch(name){case "invalid_repaired"->"{Sections:{'0':{Records:[{pos:[I;-31,2,51],type:'meeting',free_tickets:9}]}}}";case "valid_occupied_retained"->"{Sections:{'0':{Valid:1b,Records:[{pos:[I;-31,2,51],type:'home',free_tickets:0}]}}}";default->"{}";};row.addProperty("input",input);
    try(var storage=new RegionFileStorage(info,folder,false)){
     row.addProperty("initial_absent",storage.read(pos)==null);
     if(!name.equals("fresh_populated")){var initial=TagParser.parseCompoundFully(input);initial.putInt("DataVersion",net.minecraft.SharedConstants.getCurrentVersion().dataVersion().version());storage.write(pos,initial);storage.flush();}
     var initial=storage.read(pos);if(initial!=null)initial=ca.spottedleaf.dataconverter.minecraft.MCDataConverter.convertTag(ca.spottedleaf.dataconverter.minecraft.datatypes.MCTypeRegistry.POI_CHUNK,initial,initial.getIntOr("DataVersion",1945),net.minecraft.SharedConstants.getCurrentVersion().dataVersion().version());w.manager.chunk=PoiChunk.parse(w,pos.x(),pos.z(),initial==null?new CompoundTag():initial);var chunk=w.manager.chunk;chunk.load();
     for(int y=-1;y<=1;y++)w.manager.checkConsistencyWithBlocks(SectionPos.of(pos,y),PoiChunkProbe.blocks(y==0?"mixed":"air"));
     var saved=chunk.save();row.addProperty("saved",saved.toString());row.addProperty("save_outcome",chunk.isDirty()?"written":"skipped_clean");if(chunk.isDirty())storage.write(pos,saved);storage.flush();chunk.setDirty(false);row.addProperty("clean_after_ack",!chunk.isDirty());
    }
    try(var storage=new RegionFileStorage(info,folder,false)){
     var loaded=storage.read(pos);row.addProperty("reloaded",loaded.toString());var future=loaded.copy();future.putInt("DataVersion",Integer.MAX_VALUE);row.addProperty("future_conversion_unchanged",future.equals(ca.spottedleaf.dataconverter.minecraft.MCDataConverter.convertTag(ca.spottedleaf.dataconverter.minecraft.datatypes.MCTypeRegistry.POI_CHUNK,future.copy(),Integer.MAX_VALUE,net.minecraft.SharedConstants.getCurrentVersion().dataVersion().version())));w.manager.chunk=PoiChunk.parse(w,pos.x(),pos.z(),loaded);w.manager.chunk.load();row.add("reload_state",PoiChunkProbe.snapshot(w.manager.chunk));
     storage.write(pos,null);storage.flush();row.addProperty("deleted",storage.read(pos)==null);
    }
    out.println(row);
   }
  }finally{try(var paths=Files.walk(root)){for(var path:paths.sorted(Comparator.reverseOrder()).toList())Files.delete(path);}}
 }
 public static void main(String[] args)throws Exception {
  net.minecraft.SharedConstants.tryDetectVersion();Bootstrap.bootStrap();PoiChunkProbe.access=RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY);
  Throwable[] error={null};var thread=new TickThread(()->{try{probe(args[0]);}catch(Throwable e){error[0]=e;}},"poi-storage-oracle");thread.start();thread.join();if(error[0]!=null)throw new RuntimeException(error[0]);
 }
}
