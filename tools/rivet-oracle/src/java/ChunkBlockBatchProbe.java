import com.google.gson.*;
import it.unimi.dsi.fastutil.shorts.ShortOpenHashSet;
/** Actual ChunkHolder per-section collection; order is not insertion/sorted. */
public final class ChunkBlockBatchProbe {
    public static void main(String[] args) {
        for(int count:new int[]{1,2,5,24,25,64}) for(boolean reversed:new boolean[]{false,true}) {
            var input=new JsonArray();var set=new ShortOpenHashSet();
            for(int step=0;step<count;step++) {
                short value=(short)(((reversed?count-1-step:step)*257)&4095);
                input.add(value);set.add(value);
                if(step%3==0){input.add(value);set.add(value);}
            }
            var order=new JsonArray();for(short pos:set)order.add(pos);
            var row=new JsonObject();row.add("insertions",input);row.add("iteration",order);System.out.println(row);
        }
    }
}
