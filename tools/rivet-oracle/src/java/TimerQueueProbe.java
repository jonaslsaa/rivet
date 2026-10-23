import com.google.gson.*;
import com.mojang.serialization.MapCodec;
import java.util.*;
import net.minecraft.world.level.timers.*;

/** Execute real TimerQueue ordering, stale indexes, reentrant work and errors. */
public final class TimerQueueProbe {
    record Callback(String name) implements TimerCallback<JsonArray> {
        public MapCodec<? extends TimerCallback<JsonArray>> codec(){throw new UnsupportedOperationException("execution-only callback");}
        public void handle(JsonArray log,TimerQueue<JsonArray> queue,long time) {
            var event=new JsonArray();event.add(name);event.add(time);log.add(event);
            switch(name) {
                case "reschedule" -> queue.schedule("child",time,new Callback("child"));
                case "remove" -> {var removed=new JsonArray();removed.add("removed");removed.add(queue.remove("same"));log.add(removed);}
                case "throw" -> throw new IllegalStateException("controlled callback failure");
            }
        }
    }
    public static void main(String[] args)throws Exception {
        var out=System.out;
        net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
        for(var commands:List.of(
            "[[\"schedule\",\"a\",2,\"a\"],[\"schedule\",\"b\",1,\"b\"],[\"tick\",2]]",
            "[[\"schedule\",\"a\",1,\"first\"],[\"schedule\",\"a\",1,\"duplicate\"],[\"schedule\",\"b\",1,\"second\"],[\"tick\",1]]",
            "[[\"schedule\",\"same\",1,\"old\"],[\"tick\",2],[\"schedule\",\"same\",1,\"duplicate\"],[\"remove\",\"same\"],[\"schedule\",\"same\",1,\"new\"],[\"tick\",2]]",
            "[[\"schedule\",\"same\",1,\"remove\"],[\"schedule\",\"same\",2,\"second\"],[\"tick\",2]]",
            "[[\"schedule\",\"a\",1,\"reschedule\"],[\"tick\",1]]",
            "[[\"schedule\",\"a\",1,\"throw\"],[\"schedule\",\"b\",1,\"after\"],[\"tick\",1],[\"tick\",1]]",
            "[[\"schedule\",\"a\",-1,\"negative\"],[\"schedule\",\"b\",9223372036854775807,\"max\"],[\"tick\",0],[\"remove\",\"missing\"],[\"tick\",9223372036854775807]]"
        )) {
            var queue=new TimerQueue<JsonArray>();var log=new JsonArray();var snapshots=new JsonArray();
            var input=JsonParser.parseString(commands).getAsJsonArray();
            for(var value:input) {
                var command=value.getAsJsonArray();String op=command.get(0).getAsString();
                switch(op) {
                    case "schedule" -> queue.schedule(command.get(1).getAsString(),command.get(2).getAsLong(),new Callback(command.get(3).getAsString()));
                    case "remove" -> {var removed=new JsonArray();removed.add("removed");removed.add(queue.remove(command.get(1).getAsString()));log.add(removed);}
                    case "tick" -> {try{queue.tick(log,command.get(1).getAsLong());}catch(IllegalStateException failure){var event=new JsonArray();event.add("error");log.add(event);}}
                }
                var snapshot=new JsonObject();snapshot.add("ids",new Gson().toJsonTree(new TreeSet<>(queue.getEventsIds())));snapshot.addProperty("dirty",queue.isDirty());snapshots.add(snapshot);
            }
            var row=new JsonObject();row.add("commands",input);row.add("log",log);row.add("snapshots",snapshots);
            out.println("TIMER_QUEUE\t"+row);
        }
        for(String input:List.of("{}","{events:[]}","{events:[B;]}","{events:[I;]}","{events:[L;]}","{events:[B;1b]}","{events:[I;2]}","{events:[L;3L]}",
            "{events:[{id:\"a\",trigger_time:4L,callback:{type:\"function\",id:\"test:a\"}},{id:\"b\",trigger_time:2L,callback:{type:\"function_tag\",id:\"test:b\"}}]}",
            "{events:[{}, {id:\"a\",trigger_time:3.75d,callback:{type:\"function\",id:\"test:a\"}}]}",
            "{events:[{id:\"bad\",trigger_time:1,callback:{type:\"missing\",id:\"test:a\"}}]}",
            "{events:[{id:\"a\",trigger_time:2,callback:{type:\"function\",id:\"test:first\"}},{id:\"a\",trigger_time:2,callback:{type:\"function\",id:\"test:duplicate\"}}]}")) {
            var result=TimerQueue.CODEC.parse(net.minecraft.nbt.NbtOps.INSTANCE,net.minecraft.nbt.TagParser.parseCompoundFully(input)).resultOrPartial(error->System.err.println("expected codec diagnostic: "+error));
            var row=new JsonObject();row.addProperty("input",input);row.addProperty("present",result.isPresent());
            if(result.isPresent()) {
                row.addProperty("dirty",result.get().isDirty());
                row.addProperty("encoded",TimerQueue.CODEC.encodeStart(net.minecraft.nbt.NbtOps.INSTANCE,result.get()).getOrThrow().toString());
            }
            out.println("TIMER_CODEC\t"+row);
        }
    }
}
