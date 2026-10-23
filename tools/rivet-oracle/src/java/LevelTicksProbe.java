import com.google.gson.*;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import it.unimi.dsi.fastutil.longs.Long2LongMap;
import net.minecraft.core.*;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.ticks.*;

/** Calls the genuine pinned LevelTicks and LevelChunkTicks methods. Commands
 * are fixture input, not a replacement scheduler. Captures callback queries,
 * exact heap-array/fastutil-index order and interrupted execution state. */
public final class LevelTicksProbe {
 static final Gson GSON=new Gson();
 static final class Stop extends RuntimeException {}
 static JsonArray a(Object...v){return GSON.toJsonTree(v).getAsJsonArray();}
 static final class State {
  Set<Long> blocked=new HashSet<>();
  LevelTicks<String> ticks=new LevelTicks<>(key->!blocked.contains(key));
  LinkedHashMap<ChunkPos,LevelChunkTicks<String>> chunks=new LinkedHashMap<>();
 }
 static Object field(Object value,String name)throws Exception{var f=value.getClass().getDeclaredField(name);f.setAccessible(true);return f.get(value);}
 static JsonArray tick(ScheduledTick<String> t){return a(t.type(),t.pos().getX(),t.pos().getY(),t.pos().getZ(),t.triggerTick(),t.priority().getValue(),t.subTickOrder());}
 static JsonArray iterable(Object ticks){var rows=new JsonArray();for(Object t:(Iterable<?>)ticks)rows.add(tick((ScheduledTick<String>)t));return rows;}
 static JsonObject snapshot(State state)throws Exception{
  var out=new JsonObject();var containers=new JsonArray();
  for(var e:state.chunks.entrySet()){var row=new JsonObject();row.add("pos",a(e.getKey().x(),e.getKey().z()));row.addProperty("count",e.getValue().count());row.add("ticks",GSON.toJsonTree(e.getValue().getAll().map(LevelTicksProbe::tick).toList()));containers.add(row);}
  out.add("containers",containers);var index=new JsonArray();for(var e:((Long2LongMap)field(state.ticks,"nextTickForContainer")).long2LongEntrySet())index.add(a(e.getLongKey(),e.getLongValue()));out.add("index",index);
  out.add("to_run",iterable(field(state.ticks,"toRunThisTick")));out.add("already_run",iterable(field(state.ticks,"alreadyRunThisTick")));out.addProperty("count",state.ticks.count());return out;
 }
 static ScheduledTick<String> scheduled(JsonArray c,int i){return new ScheduledTick<>(c.get(i).getAsString().intern(),new BlockPos(c.get(i+1).getAsInt(),c.get(i+2).getAsInt(),c.get(i+3).getAsInt()),c.get(i+4).getAsLong(),TickPriority.byValue(c.get(i+5).getAsInt()),c.get(i+6).getAsLong());}
 static BoundingBox box(JsonArray c){return new BoundingBox(c.get(0).getAsInt(),c.get(1).getAsInt(),c.get(2).getAsInt(),c.get(3).getAsInt(),c.get(4).getAsInt(),c.get(5).getAsInt());}
 static void run(PrintStream output,String name,List<JsonArray> commands)throws Exception{
  State[] states={new State(),new State()};var snapshots=new JsonArray();var events=new JsonArray();
  for(int commandIndex=0;commandIndex<commands.size();commandIndex++){var c=commands.get(commandIndex);var s=states[c.get(0).getAsInt()];String op=c.get(1).getAsString();boolean interrupted=false;
   switch(op){
    case "add"->{var p=new ChunkPos(c.get(2).getAsInt(),c.get(3).getAsInt());var pending=new ArrayList<SavedTick<String>>();if(c.size()>4)for(var item:c.get(4).getAsJsonArray()){var t=scheduled(item.getAsJsonArray(),0);pending.add(new SavedTick<>(t.type(),t.pos(),(int)t.triggerTick(),t.priority()));}var container=c.size()>4?new LevelChunkTicks<>(pending):new LevelChunkTicks<String>();s.chunks.put(p,container);s.ticks.addContainer(p,container);}
    case "remove"->{s.ticks.removeContainer(new ChunkPos(c.get(2).getAsInt(),c.get(3).getAsInt()));}
    case "unpack"->{s.chunks.get(new ChunkPos(c.get(2).getAsInt(),c.get(3).getAsInt())).unpack(c.get(4).getAsLong());}
    case "schedule"->s.ticks.schedule(scheduled(c,2));
    case "direct"->s.chunks.get(ChunkPos.containing(scheduled(c,2).pos())).schedule(scheduled(c,2));
    case "clear"->s.ticks.clearArea(box(c.get(2).getAsJsonArray()));
    case "copy","copy_from"->{var b=box(c.get(2).getAsJsonArray());var v=c.get(3).getAsJsonArray();var offset=new Vec3i(v.get(0).getAsInt(),v.get(1).getAsInt(),v.get(2).getAsInt());if(op.equals("copy"))s.ticks.copyArea(b,offset);else s.ticks.copyAreaFrom(states[1-c.get(0).getAsInt()].ticks,b,offset);}
    case "tick"->{long now=c.get(2).getAsLong();s.blocked.clear();for(var item:c.get(4).getAsJsonArray()){var p=item.getAsJsonArray();s.blocked.add(ChunkPos.pack(p.get(0).getAsInt(),p.get(1).getAsInt()));}String mode=c.get(5).getAsString();boolean[] first={true};try{s.ticks.tick(now,c.get(3).getAsInt(),(pos,type)->{
      var event=new JsonObject();event.add("tick",a(type,pos.getX(),pos.getY(),pos.getZ()));var queries=new JsonArray();for(int x=0;x<7;x++)queries.add(s.ticks.willTickThisTick(new BlockPos(x,64,0),"t"));event.add("queries",queries);event.addProperty("count",s.ticks.count());events.add(event);
      if(first[0]){first[0]=false;switch(mode){case "reschedule"->s.ticks.schedule(new ScheduledTick<>(type,pos,now,TickPriority.NORMAL,99));case "clear"->s.ticks.clearArea(new BoundingBox(1,0,0,1,100,0));case "copy"->s.ticks.copyArea(new BoundingBox(-32,-64,-32,63,319,31),new Vec3i(0,0,16));case "throw"->throw new Stop();}}
     });}catch(RuntimeException stop){interrupted=true;}}
    default->throw new AssertionError(op);
   }
   if(!Set.of("add","schedule","direct").contains(op)&&!(name.equals("map_churn")&&op.equals("remove"))){var snap=new JsonObject();snap.addProperty("step",commandIndex);snap.add("a",snapshot(states[0]));snap.add("b",snapshot(states[1]));snap.addProperty("interrupted",interrupted);snapshots.add(snap);}
  }
  var row=new JsonObject();row.addProperty("name",name);row.add("commands",GSON.toJsonTree(commands));row.add("events",events);row.add("snapshots",snapshots);output.println(row);
 }
 static List<JsonArray> base(){var cmds=new ArrayList<JsonArray>();for(int[] p:new int[][]{{0,0},{1,0},{-1,0},{0,1},{1,1},{-1,1}})cmds.add(a(0,"add",p[0],p[1]));return cmds;}
 public static void main(String[] args)throws Exception{
  net.minecraft.SharedConstants.tryDetectVersion();
  try(var out=new PrintStream(args[0])){
   for(String mode:new String[]{"normal","reschedule","clear","copy","throw"}){
    var cmds=base();for(int i=0;i<7;i++)cmds.add(a(0,"schedule","t",i,64,0,5,0,i));cmds.add(a(0,"schedule","next",16,64,0,4,-1,2));cmds.add(a(0,"schedule","last",-16,64,0,3,1,-2));cmds.add(a(0,"tick",5,5,a(),mode));cmds.add(a(0,"tick",6,100,a(),"normal"));run(out,"callback_"+mode,cmds);
   }
   {var c=base();c.add(a(0,"add",2,0,a(a("t",32,64,0,-3,0,0),a("t",32,64,0,5,0,0),a("q",33,64,0,0,-3,0))));c.add(a(0,"tick",100,10,a(),"normal"));c.add(a(0,"unpack",2,0,100));c.add(a(0,"tick",100,1,a(a(2,0)),"normal"));c.add(a(0,"tick",100,1,a(),"normal"));c.add(a(0,"tick",105,100,a(),"normal"));run(out,"pending_duplicate_activation",c);}
   {var c=base();c.add(a(0,"schedule","t",0,64,0,10,0,0));c.add(a(0,"remove",0,0));c.add(a(0,"direct","t",1,64,0,1,0,1));c.add(a(0,"schedule","unloaded",0,65,0,0,0,2));c.add(a(0,"tick",100,100,a(),"normal"));c.add(a(0,"add",0,0));c.add(a(0,"schedule","t",2,64,0,100,0,3));c.add(a(0,"tick",100,0,a(),"normal"));c.add(a(0,"tick",100,-1,a(),"normal"));c.add(a(0,"tick",100,100,a(),"normal"));run(out,"unregister_and_limits",c);}
   {var c=base();c.add(a(1,"add",0,0));c.add(a(1,"add",1,0));c.add(a(1,"schedule","a",0,64,0,10,0,Long.MIN_VALUE));c.add(a(1,"schedule","b",1,64,0,10,0,Long.MAX_VALUE));c.add(a(0,"copy_from",a(0,0,0,15,100,15),a(16,0,0)));c.add(a(0,"clear",a(16,0,0,16,100,15)));c.add(a(0,"tick",10,100,a(),"normal"));run(out,"cross_copy_wrap",c);}
   // Equal heap keys reveal actual fastutil table/resizing and iterator removal.
   for(int size:new int[]{3,24,25,80}){var c=new ArrayList<JsonArray>();for(int i=0;i<size;i++){int x=(i*17)%83-41,z=(i*31)%89-44;c.add(a(0,"add",x,z));c.add(a(0,"schedule","e",x*16,64,z*16,0,0,0));}c.add(a(0,"tick",0,size/2,a(),"normal"));c.add(a(0,"tick",1,1000,a(),"normal"));run(out,"ties_"+size,c);}
   {var c=new ArrayList<JsonArray>();for(int i=0;i<160;i++){int x=(i*17)%83-41,z=(i*31)%89-44;c.add(a(0,"add",x,z));c.add(a(0,"schedule","e",x*16,64,z*16,0,0,0));}for(int i=0;i<140;i++)c.add(a(0,"remove",(i*17)%83-41,(i*31)%89-44));for(int i=0;i<80;i++){int x=(i*17)%83-41,z=(i*31)%89-44;c.add(a(0,"add",x,z));c.add(a(0,"schedule","e",x*16,64,z*16,0,0,0));}c.add(a(0,"tick",0,0,a(),"normal"));c.add(a(0,"tick",0,1000,a(),"normal"));run(out,"map_churn",c);}
  }
 }
}
