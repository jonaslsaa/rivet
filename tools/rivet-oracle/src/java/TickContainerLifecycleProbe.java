import com.google.gson.*;
import java.io.PrintStream;
import java.util.*;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.ticks.*;
import ca.spottedleaf.moonrise.patches.chunk_system.ticks.ChunkSystemLevelChunkTicks;

/** Actual pending/registered/activated/saved/removed container lifecycle.
 * In particular pack changes lastSaved before LevelChunk.tryMarkSaved reads
 * the dirty state; no copied/frozen saved-list authority is substituted. */
public final class TickContainerLifecycleProbe {
    static void capture(PrintStream out,String phase,LevelChunkTicks<String> ticks,long time) {
        var dirty=(ChunkSystemLevelChunkTicks)ticks;
        var row=new JsonObject();row.addProperty("phase",phase);row.addProperty("time",time);
        row.addProperty("before_pack_dirty",dirty.moonrise$isDirty(time));row.addProperty("count",ticks.count());
        var saved=new JsonArray();for(var tick:ticks.pack(time)) {
            var value=new JsonArray();value.add(tick.type());value.add(tick.pos().getX());value.add(tick.pos().getY());value.add(tick.pos().getZ());value.add(tick.delay());value.add(tick.priority().getValue());saved.add(value);
        }
        row.add("saved",saved);row.addProperty("after_pack_dirty",dirty.moonrise$isDirty(time));out.println(row);
    }
    public static void main(String[] args)throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        try(var out=new PrintStream(args[0])) {
            var ticks=new LevelChunkTicks<String>(List.of(
                new SavedTick<>("t",new BlockPos(1,64,1),5,TickPriority.NORMAL),
                new SavedTick<>("q",new BlockPos(2,64,1),0,TickPriority.HIGH)));
            var scheduler=new LevelTicks<String>(key->true);
            capture(out,"pending",ticks,100);
            scheduler.addContainer(new ChunkPos(0,0),ticks);capture(out,"registered",ticks,100);
            ticks.unpack(100);capture(out,"activated",ticks,100);
            capture(out,"aged",ticks,101);
            ticks.poll();capture(out,"polled",ticks,101);
            ((ChunkSystemLevelChunkTicks)ticks).moonrise$clearDirty();capture(out,"cleaned",ticks,101);
            scheduler.schedule(new ScheduledTick<>("q",new BlockPos(2,64,1),108,TickPriority.LOW,33));capture(out,"scheduled",ticks,101);
            scheduler.removeContainer(new ChunkPos(0,0));
            ticks.schedule(new ScheduledTick<>("t",new BlockPos(3,64,1),102,TickPriority.NORMAL,34));capture(out,"detached",ticks,101);
            scheduler.tick(110,100,(pos,kind)->{throw new AssertionError("removed container callback leaked");});
        }
    }
}
