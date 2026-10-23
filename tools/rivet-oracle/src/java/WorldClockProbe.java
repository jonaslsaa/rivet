import com.google.gson.JsonObject;
import java.io.PrintWriter;
import java.util.Optional;
import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.clock.ClockState;
import net.minecraft.world.clock.ClockNetworkState;
import net.minecraft.world.clock.ClockTimeMarker;

public final class WorldClockProbe {
 public static void main(String[] args) throws Exception {
  SharedConstants.tryDetectVersion(); Bootstrap.bootStrap();
  var type=Class.forName("net.minecraft.world.clock.ServerClockManager$ClockInstance");
  var ctor=type.getDeclaredConstructor(); ctor.setAccessible(true);
  var load=type.getDeclaredMethod("loadFrom",ClockState.class);load.setAccessible(true);
  var tick=type.getDeclaredMethod("tick");tick.setAccessible(true);
  var pack=type.getDeclaredMethod("packState");pack.setAccessible(true);
  var network=type.getDeclaredMethod("packNetworkState",boolean.class);network.setAccessible(true);
  try(var out=new PrintWriter(args[0])) {
   for(long start:new long[]{0,-1,Long.MAX_VALUE}) for(float rate:new float[]{0f,0.25f,1f,1.75f,-0.25f,Float.MAX_VALUE}) for(boolean paused:new boolean[]{false,true}) {
    var instance=ctor.newInstance();float partial=0.75f;
    load.invoke(instance,new ClockState(start,partial,rate,paused));
    for(int step=1;step<=4;step++) {
     tick.invoke(instance);var state=(ClockState)pack.invoke(instance);
     var active=(ClockNetworkState)network.invoke(instance,true);var frozen=(ClockNetworkState)network.invoke(instance,false);
     var row=new JsonObject();row.addProperty("kind","tick");row.addProperty("start",start);row.addProperty("partial_bits",Float.floatToRawIntBits(partial));row.addProperty("rate_bits",Float.floatToRawIntBits(rate));row.addProperty("paused",paused);row.addProperty("steps",step);row.addProperty("total",state.totalTicks());row.addProperty("result_partial_bits",Float.floatToRawIntBits(state.partialTick()));row.addProperty("network_rate_bits",Float.floatToRawIntBits(active.rate()));row.addProperty("frozen_rate_bits",Float.floatToRawIntBits(frozen.rate()));out.println(row);
    }
   }
   for(Integer period:new Integer[]{null,24000,1}) for(int marker:new int[]{0,1000,24000}) for(long total:new long[]{Long.MIN_VALUE,-24001,-1,0,1000,23999,24000,Long.MAX_VALUE}) {
    var m=new ClockTimeMarker(null,marker,Optional.ofNullable(period),true);
    var row=new JsonObject();row.addProperty("kind","marker");row.addProperty("period",period);row.addProperty("ticks",marker);row.addProperty("total",total);row.addProperty("resolve",m.resolveTimeToMoveTo(total));row.addProperty("count",m.getRepetitionCount(total));row.addProperty("occurs",m.occursAt(total));out.println(row);
   }
  }
 }
}
