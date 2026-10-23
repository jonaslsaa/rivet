import com.google.gson.JsonObject;
import java.io.PrintStream;
import java.lang.reflect.Field;
import ca.spottedleaf.moonrise.patches.chunk_system.scheduling.NewChunkHolder;
import net.minecraft.server.level.FullChunkStatus;
import net.minecraft.world.level.chunk.status.ChunkStatus;

/** Executes actual private updatePendingStatus, not a copied mask algorithm. */
public final class HolderReadinessProbe {
    static final sun.misc.Unsafe U;
    static {try {var f=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);U=(sun.misc.Unsafe)f.get(null);}catch(Exception e){throw new ExceptionInInitializerError(e);}}
    static Field field(String name)throws Exception {var f=NewChunkHolder.class.getDeclaredField(name);f.setAccessible(true);return f;}
    public static void main(String[] args)throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
        var update=NewChunkHolder.class.getDeclaredMethod("updatePendingStatus");update.setAccessible(true);
        var pending=field("pendingFullChunkStatus");
        // Genuine source masks plus gaps, unrelated high bits and self-absent
        // neighbours. Each case varies ticket clamping and generation fallback.
        long[] masks={0,1L<<12,0x739c0L,0x1ffffffL,0x1ffffffL^(1L<<12),0x1ffffffL^1L,
            0x739c0L^(1L<<6),1L<<63,0x1ffffffL|(1L<<63),0x1ffffffL^(1L<<24)};
        try(var out=new PrintStream(args[0])) {
            for(long bits:masks)for(int ticket:new int[]{30,31,32,33,34,45})for(boolean full:new boolean[]{false,true}) {
                var holder=(NewChunkHolder)U.allocateInstance(NewChunkHolder.class);
                field("oldTicketLevel").setInt(holder,ticket);
                field("currentGenStatus").set(holder,full?ChunkStatus.FULL:ChunkStatus.SPAWN);
                field("fullNeighbourChunksLoadedBitset").setLong(holder,bits);
                pending.set(holder,FullChunkStatus.INACCESSIBLE);
                update.invoke(holder);
                var row=new JsonObject();row.addProperty("ticket",ticket);row.addProperty("generation_full",full);
                // JSON unsigned representation avoids losing high bit input.
                row.add("bits",com.google.gson.JsonParser.parseString(Long.toUnsignedString(bits)));
                row.addProperty("pending",((FullChunkStatus)pending.get(holder)).ordinal());
                out.println(row);
            }
        }
    }
}
