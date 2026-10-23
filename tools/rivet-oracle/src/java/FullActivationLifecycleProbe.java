import com.google.gson.JsonObject;
import java.io.PrintStream;
import java.lang.reflect.Field;
import java.util.concurrent.atomic.AtomicReference;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.FullChunkStatus;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.chunk.*;

/** Genuine LevelChunk helper bodies. The chunk is a deliberately unconstructed
 * carrier: these methods touch only initialized fields shown below. This is
 * NOT a claim to execute ChunkFullTask, entity loading, BE registration or IO. */
public final class FullActivationLifecycleProbe {
    static final class ClockLevel extends net.minecraft.server.level.ServerLevel{
        private ClockLevel(){super(null,null,null,null,null,null,false,0L,java.util.List.of(),false,null,null,null,null,null,null);}
        @Override public long getGameTime(){return 123L;}
    }
    static final sun.misc.Unsafe U;
    static {try{var field=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");field.setAccessible(true);U=(sun.misc.Unsafe)field.get(null);}catch(Exception e){throw new ExceptionInInitializerError(e);}}
    static Field field(Class<?> owner,String name)throws Exception{var field=owner.getDeclaredField(name);field.setAccessible(true);return field;}
    public static void main(String[] args)throws Exception{
        var errors=System.err;try{run(args);}catch(Throwable error){error.printStackTrace(errors);System.exit(1);}
    }
    static void run(String[] args)throws Exception{
        net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
        try(var out=new PrintStream(args[0])){
            for(boolean fail:new boolean[]{false,true}){
                var chunk=(LevelChunk)U.allocateInstance(LevelChunk.class);
                field(LevelChunk.class,"level").set(chunk,U.allocateInstance(ClockLevel.class));
                field(LevelChunk.class,"blockTicks").set(chunk,new net.minecraft.world.ticks.LevelChunkTicks<>());
                field(LevelChunk.class,"fluidTicks").set(chunk,new net.minecraft.world.ticks.LevelChunkTicks<>());
                int[] calls={0};
                LevelChunk.PostLoadProcessor callback=value->{calls[0]++;if(fail)throw new IllegalStateException("probe callback failure");};
                field(LevelChunk.class,"postLoad").set(chunk,callback);
                var row=new JsonObject();row.addProperty("name",fail?"callback_throw":"normal");
                for(int i=0;i<2;i++){try{chunk.runPostLoad();}catch(IllegalStateException expected){if(!fail)throw expected;}}
                row.addProperty("callback_calls",calls[0]);row.addProperty("retained",field(LevelChunk.class,"postLoad").get(chunk)==callback);
                row.addProperty("loaded_before",field(LevelChunk.class,"loaded").getBoolean(chunk));
                chunk.setLoaded(true);row.addProperty("loaded_after",field(LevelChunk.class,"loaded").getBoolean(chunk));
                var status=new AtomicReference<>(FullChunkStatus.INACCESSIBLE);chunk.setFullStatus(status::get);
                row.addProperty("status_before",chunk.getFullStatus().name());status.set(FullChunkStatus.BLOCK_TICKING);row.addProperty("status_after",chunk.getFullStatus().name());
                // Actual dirty listener attachment and actual pinned empty
                // ChunkMap listener target, without replacing either body.
                field(ChunkAccess.class,"unsaved").setBoolean(chunk,true);
                field(ChunkAccess.class,"chunkPos").set(chunk,new ChunkPos(2,-3));
                var map=(net.minecraft.server.level.ChunkMap)U.allocateInstance(net.minecraft.server.level.ChunkMap.class);
                var unsaved=net.minecraft.server.level.ChunkMap.class.getDeclaredMethod("setChunkUnsaved",ChunkPos.class);unsaved.setAccessible(true);
                int[] notifications={0};chunk.setUnsavedListener(pos->{notifications[0]++;try{unsaved.invoke(map,pos);}catch(Exception e){throw new RuntimeException(e);}});
                row.addProperty("dirty_attach_notifications",notifications[0]);
                out.println(row);
            }
            // PlatformHooks is real Paper: both helper bodies accept these
            // absent arguments because they intentionally do no work.
            var hooks=ca.spottedleaf.moonrise.common.PlatformHooks.get();
            hooks.setCurrentlyLoading(null,null);hooks.chunkFullStatusComplete(null,null);
            var row=new JsonObject();row.addProperty("name","paper_platform_noops");row.addProperty("accepted_null",true);out.println(row);
        }
    }
}
