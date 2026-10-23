import com.google.gson.*;
import java.io.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.*;
import net.minecraft.resources.*;
import net.minecraft.server.*;
import net.minecraft.server.level.*;
import net.minecraft.world.clock.*;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.gamerules.GameRules;

/** Actual PackedClockStates codec and ServerClockManager.createFullSyncPacket.
 * World/player getter shells supply explicit state only; no player construction,
 * simulation, packet transport or plugin callback is claimed by this probe. */
public final class WorldClockBootstrapProbe {
    static final class World extends ServerLevel {
        DimensionType type;GameRules rules;long time,gameTime;
        private World(){super(null,null,null,null,null,null,false,0L,List.of(),false,null,null,null,null,null,null);}
        @Override public DimensionType dimensionType(){return type;}
        @Override public GameRules getGameRules(){return rules;}
        @Override public long getDefaultClockTime(){return time;}
        @Override public long getGameTime(){return gameTime;}
    }
    static void field(Object target,String name,Object value)throws Exception {
        var f=ServerClockManager.class.getDeclaredField(name);f.setAccessible(true);f.set(target,value);
    }
    public static void main(String[] args)throws Exception {
        SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
        var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
        MinecraftServer.configurePackRepository(packs,new net.minecraft.world.level.WorldDataConfiguration(new net.minecraft.world.level.DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
        try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var output=new PrintWriter(args[0])) {
            var layers=RegistryLayer.createRegistryAccess();
            var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(RegistryLayer.STATIC)));
            var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();layers=layers.replaceFrom(RegistryLayer.WORLDGEN,worldgen);
            var all=new ArrayList<HolderLookup.RegistryLookup<?>>(lookups);worldgen.listRegistries().forEach(all::add);
            var dimensions=RegistryDataLoader.load(resources,all,RegistryDataLoader.DIMENSION_REGISTRIES,Runnable::run).join();
            var access=layers.replaceFrom(RegistryLayer.DIMENSIONS,dimensions).compositeAccess().freeze();var ops=RegistryOps.create(NbtOps.INSTANCE,access);
            var inputs=List.of("{}","{overworld:{total_ticks:13000}}","{\"minecraft:overworld\":{total_ticks:-1L,partial_tick:0.75f,rate:0.25f,paused:1b},\"minecraft:the_end\":{total_ticks:42}}","{overworld:{total_ticks:256.75d,paused:256,partial_tick:2.5d}}","{overworld:{total_ticks:3,rate:0f}}","{overworld:{total_ticks:3,rate:-1f}}","{overworld:{rate:1f}}","{overworld:{total_ticks:3,paused:\"bad\"}}","{overworld:{total_ticks:3,partial_tick:\"bad\"}}","{overworld:{total_ticks:3,rate:\"bad\"}}","{overworld:{total_ticks:3},missing:{total_ticks:5}}","{overworld:{total_ticks:3},the_end:{total_ticks:9,rate:0}}","{overworld:{total_ticks:3},\"minecraft:overworld\":{total_ticks:7}}","{overworld:\"wrongshape\"}");
            for(String input:inputs) {
                var parsed=PackedClockStates.CODEC.parse(ops,TagParser.parseCompoundFully(input));
                var partial=parsed.resultOrPartial(error->{});var row=new JsonObject();row.addProperty("kind","codec");row.addProperty("input_snbt",input);row.addProperty("error",parsed.error().isPresent());
                if(partial.isPresent())row.addProperty("output_snbt",PackedClockStates.CODEC.encodeStart(ops,partial.get()).getOrThrow().toString());else row.add("output_snbt",JsonNull.INSTANCE);output.println(row);
            }
            var managerConstructor=ServerClockManager.class.getDeclaredConstructor(PackedClockStates.class);managerConstructor.setAccessible(true);
            var instanceClass=Class.forName("net.minecraft.world.clock.ServerClockManager$ClockInstance");var ctor=instanceClass.getDeclaredConstructor();ctor.setAccessible(true);
            var load=instanceClass.getDeclaredMethod("loadFrom",ClockState.class);load.setAccessible(true);
            var clocksField=ServerClockManager.class.getDeclaredField("clocks");clocksField.setAccessible(true);
            for(var dimension:List.of("overworld","the_end","the_nether"))for(long ticks:new long[]{-1,0,23999,Long.MAX_VALUE})for(boolean paused:new boolean[]{false,true})for(boolean advance:new boolean[]{false,true})for(boolean relative:new boolean[]{false,true})for(long offset:new long[]{-24001,0,1000}) {
                var world=(World)ChestMinecartProbe.UNSAFE.allocateInstance(World.class);world.type=access.lookupOrThrow(Registries.DIMENSION_TYPE).getOrThrow(ResourceKey.create(Registries.DIMENSION_TYPE,Identifier.withDefaultNamespace(dimension))).value();world.time=ticks;world.gameTime=12345;
                world.rules=new GameRules(net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS);world.rules.set(GameRules.ADVANCE_TIME,advance,null);
                var player=(ServerPlayer)ChestMinecartProbe.UNSAFE.allocateInstance(ServerPlayer.class);player.setLevel(world);player.relativeTime=relative;player.timeOffset=offset;
                var manager=managerConstructor.newInstance(PackedClockStates.EMPTY);field(manager,"level",world);
                @SuppressWarnings("unchecked") var instances=(Map<Holder<WorldClock>,Object>)clocksField.get(manager);
                for(var clock:access.lookupOrThrow(Registries.WORLD_CLOCK).listElements().toList()) {var instance=ctor.newInstance();load.invoke(instance,new ClockState(ticks,0.75f,0.25f,paused));instances.put(clock,instance);}
                // Compare the packet's keyed values, not JVM identity-hash
                // HashMap entry order (which has no protocol semantic effect).
                var packet=manager.createFullSyncPacket(player);var updates=new JsonObject();var ordered=new TreeMap<String,ClockNetworkState>();packet.clockUpdates().forEach((key,value)->ordered.put(key.getRegisteredName(),value));ordered.forEach((key,value)->{var state=new JsonObject();state.addProperty("total",value.totalTicks());state.addProperty("partial_bits",Float.floatToRawIntBits(value.partialTick()));state.addProperty("rate_bits",Float.floatToRawIntBits(value.rate()));updates.add(key,state);});
                var row=new JsonObject();row.addProperty("kind","projection");row.addProperty("dimension","minecraft:"+dimension);row.addProperty("ticks",ticks);row.addProperty("paused",paused);row.addProperty("advance",advance);row.addProperty("relative",relative);row.addProperty("offset",offset);row.addProperty("game_time",packet.gameTime());row.add("updates",updates);output.println(row);
            }
        }
    }
}
