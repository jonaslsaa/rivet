import com.google.gson.*;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicInteger;
import it.unimi.dsi.fastutil.shorts.Short2ObjectMap;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.nbt.*;
import net.minecraft.resources.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.entity.ai.village.poi.*;

/** Genuine PoiSection methods, shared record objects, codecs and fastutil/JDK
 * iteration. No world/manager algorithms are replaced by expected outputs. */
public final class PoiSectionProbe {
 static final Gson GSON=new Gson();
 static RegistryOps<Tag> OPS;
 static final Method ACQUIRE,RECORDS;
 static final Field RECORD_MAP;
 static {try{ACQUIRE=PoiRecord.class.getDeclaredMethod("acquireTicket");ACQUIRE.setAccessible(true);RECORD_MAP=PoiSection.class.getDeclaredField("records");RECORD_MAP.setAccessible(true);RECORDS=PoiSection.class.getDeclaredMethod("getPoiRecord",BlockPos.class);RECORDS.setAccessible(true);}catch(Exception e){throw new ExceptionInInitializerError(e);}}
 static JsonArray a(Object...items){return GSON.toJsonTree(items).getAsJsonArray();}
 static Holder<PoiType> kind(String name){return BuiltInRegistries.POINT_OF_INTEREST_TYPE.getOrThrow(ResourceKey.create(Registries.POINT_OF_INTEREST_TYPE,Identifier.parse(name)));}
 static final class State {
  AtomicInteger dirty=new AtomicInteger();PoiSection section=new PoiSection(dirty::incrementAndGet);IdentityHashMap<PoiRecord,Integer> ids=new IdentityHashMap<>();Map<String,PoiRecord> retained=new HashMap<>();int updaterCalls;
  int id(PoiRecord record){return ids.computeIfAbsent(record,key->ids.size());}
 }
 static JsonObject record(State s,PoiRecord r){var row=new JsonObject();row.addProperty("id",s.id(r));row.add("pos",a(r.getPos().getX(),r.getPos().getY(),r.getPos().getZ()));row.addProperty("type",r.getPoiType().getRegisteredName());row.addProperty("free",r.getFreeTickets());return row;}
 static JsonObject snapshot(State s)throws Exception {
  var row=new JsonObject();row.addProperty("valid",s.section.isValid());row.addProperty("empty",((ca.spottedleaf.moonrise.patches.chunk_system.level.poi.ChunkSystemPoiSection)s.section).moonrise$isEmpty());row.addProperty("dirty",s.dirty.get());row.addProperty("updater_calls",s.updaterCalls);
  row.addProperty("packed",PoiSection.Packed.CODEC.encodeStart(OPS,s.section.pack()).getOrThrow().toString());
  var records=new JsonArray();for(var r:((Short2ObjectMap<PoiRecord>)RECORD_MAP.get(s.section)).values())records.add(record(s,r));row.add("records",records);
  var types=new JsonArray();var entries=new ArrayList<>(s.section.byType.entrySet());entries.sort(Comparator.comparing(e->e.getKey().getRegisteredName()));for(var e:entries){var type=new JsonObject();type.addProperty("type",e.getKey().getRegisteredName());var values=new JsonArray();for(var r:e.getValue())values.add(record(s,r));type.add("records",values);var space=new JsonArray();s.section.getRecords(k->k.equals(e.getKey()),PoiManager.Occupancy.HAS_SPACE).forEach(r->space.add(s.id(r)));type.add("space",space);var occupied=new JsonArray();s.section.getRecords(k->k.equals(e.getKey()),PoiManager.Occupancy.IS_OCCUPIED).forEach(r->occupied.add(s.id(r)));type.add("occupied",occupied);types.add(type);}row.add("types",types);
  var retained=new JsonObject();s.retained.entrySet().stream().sorted(Map.Entry.comparingByKey()).forEach(e->retained.add(e.getKey(),record(s,e.getValue())));row.add("retained",retained);return row;
 }
 static BlockPos pos(JsonArray c,int i){return new BlockPos(c.get(i).getAsInt(),c.get(i+1).getAsInt(),c.get(i+2).getAsInt());}
 static PoiRecord get(State s,BlockPos p)throws Exception{return ((Optional<PoiRecord>)RECORDS.invoke(s.section,p)).orElse(null);}
 static final class Stop extends RuntimeException {}
 static void run(PrintStream out,String name,List<JsonArray> commands)throws Exception {
  var s=new State();var snapshots=new JsonArray();var results=new JsonArray();
  for(int step=0;step<commands.size();step++){var c=commands.get(step);String op=c.get(0).getAsString();JsonElement result=JsonNull.INSTANCE;
   switch(op){
    case "init"->{s.section=PoiSection.Packed.CODEC.parse(OPS,TagParser.parseCompoundFully(c.get(1).getAsString())).getOrThrow().unpack(s.dirty::incrementAndGet);}
    case "add","add_quiet"->{var record=s.section.add(pos(c,1),kind(c.get(4).getAsString()));if(record!=null)result=new JsonPrimitive(s.id(record));}
    case "remove","remove_quiet"->s.section.remove(pos(c,1));
    case "acquire"->{result=new JsonPrimitive((boolean)ACQUIRE.invoke(get(s,pos(c,1))));}
    case "release"->{try{result=new JsonPrimitive(s.section.release(pos(c,1)));}catch(IllegalStateException absent){result=new JsonPrimitive("missing");}}
    case "retain"->{s.retained.put(c.get(1).getAsString(),get(s,pos(c,2)));}
    case "acquire_retained"->result=new JsonPrimitive((boolean)ACQUIRE.invoke(s.retained.get(c.get(1).getAsString())));
    case "release_retained"->{var r=s.retained.get(c.get(1).getAsString());var release=PoiRecord.class.getDeclaredMethod("releaseTicket");release.setAccessible(true);result=new JsonPrimitive((boolean)release.invoke(r));}
    case "refresh","refresh_throw"->{try{s.section.refresh(consumer->{s.updaterCalls++;for(var scan:c.get(1).getAsJsonArray()){var entry=scan.getAsJsonArray();consumer.accept(pos(entry,0),kind(entry.get(3).getAsString()));if(op.equals("refresh_throw"))throw new Stop();}});}catch(Stop stop){result=new JsonPrimitive("interrupted");}}
    case "query"->{var p=pos(c,1);var query=new JsonObject();query.addProperty("free",s.section.getFreeTickets(p));query.addProperty("exists",s.section.exists(p,k->true));s.section.getType(p).ifPresent(k->query.addProperty("type",k.getRegisteredName()));result=query;}
    case "snapshot"->{}
    default->throw new AssertionError(op);
   }
   results.add(result);
   if(!op.endsWith("_quiet")){var snap=snapshot(s);snap.addProperty("step",step);snapshots.add(snap);}
  }
  var row=new JsonObject();row.addProperty("kind","section");row.addProperty("name",name);row.add("commands",GSON.toJsonTree(commands));row.add("results",results);row.add("snapshots",snapshots);out.println(row);
 }
 public static void main(String[] args)throws Exception {
  net.minecraft.SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
  OPS=RegistryOps.create(NbtOps.INSTANCE,RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY));
  try(var out=new PrintStream(args[0])){
   run(out,"duplicate_mismatch_remove",List.of(a("snapshot"),a("add",0,64,0,"home"),a("retain","old",0,64,0),a("add",0,64,0,"home"),a("acquire",0,64,0),a("add",0,64,0,"meeting"),a("acquire",0,64,0),a("release",0,64,0),a("release",0,64,0),a("acquire_retained","old"),a("remove",0,64,0),a("release",0,64,0),a("refresh",a()),a("query",0,64,0)));
   run(out,"refresh_preserves_old_type_and_identity",List.of(a("init","{Valid:false,Records:[{pos:[I;0,64,0],type:'home',free_tickets:0},{pos:[I;1,64,0],type:'meeting',free_tickets:5}]}"),a("retain","old",0,64,0),a("refresh",a(a(0,64,0,"meeting"),a(2,64,0,"home"),a(2,64,0,"home"),a(1,64,0,"home"))),a("release_retained","old"),a("refresh",a(a(9,64,0,"home"))),a("query",0,64,0)));
   run(out,"refresh_partial_throw",List.of(a("init","{Records:[{pos:[I;0,64,0],type:'home',free_tickets:0},{pos:[I;1,64,0],type:'meeting',free_tickets:5}]}"),a("retain","dropped",1,64,0),a("refresh_throw",a(a(0,64,0,"meeting"),a(1,64,0,"home"))),a("acquire_retained","dropped"),a("refresh",a()),a("snapshot")));
   run(out,"loaded_mismatch_ghost",List.of(a("init","{Valid:true,Records:[{pos:[I;0,64,0],type:'home',free_tickets:0},{pos:[I;0,64,0],type:'meeting',free_tickets:12},{pos:[I;0,64,0],type:'home',free_tickets:1}]}"),a("acquire",0,64,0),a("remove",0,64,0),a("snapshot")));
   run(out,"relative_aliases",List.of(a("add",-1,-1,-1,"home"),a("add",15,15,15,"home"),a("query",31,31,31),a("add",31,31,31,"meeting"),a("remove",-17,-17,-17),a("query",-1,-1,-1)));
   var churn=new ArrayList<JsonArray>();for(int i=0;i<160;i++){int key=(i*43)%4096;churn.add(a("add_quiet",key>>8,64+(key&15),(key>>4)&15,i%2==0?"home":"meeting"));}churn.add(a("snapshot"));for(int i=0;i<140;i++){int key=(i*43)%4096;churn.add(a("remove_quiet",key>>8,64+(key&15),(key>>4)&15));}churn.add(a("snapshot"));for(int i=0;i<80;i++){int key=(i*43)%4096;churn.add(a("add_quiet",key>>8,64+(key&15),(key>>4)&15,i%2==0?"home":"meeting"));}churn.add(a("snapshot"));run(out,"map_and_record_set_churn",churn);
   for(String input:new String[]{"{Records:[]}","{Valid:'wrong',Records:[]}","{Valid:1b,Records:[]}","{Valid:1b}","{Records:[{pos:[I;1,2,3],type:'home'},{}]}","{Records:[{pos:[I;1,2,3],type:'missing'}]}","{Records:[{pos:[I;1,2,3],type:'home',free_tickets:'wrong'}]}"}){
    var decoded=PoiSection.Packed.CODEC.parse(OPS,TagParser.parseCompoundFully(input));var row=new JsonObject();row.addProperty("kind","codec");row.addProperty("input",input);row.addProperty("success",decoded.result().isPresent());var partial=decoded.resultOrPartial(message->{});if(partial.isPresent())row.addProperty("encoded",PoiSection.Packed.CODEC.encodeStart(OPS,partial.get()).getOrThrow().toString());out.println(row);
   }
  }
 }
}
