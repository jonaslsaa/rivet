import com.google.gson.*;
import java.io.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.*;
import net.minecraft.resources.*;
import net.minecraft.server.*;
import net.minecraft.world.attribute.*;
import net.minecraft.world.clock.ClockManager;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.timeline.Timeline;
import net.minecraft.world.phys.Vec3;

/** Full server-registry projection, NOT RegistrySynchronization/NETWORK_CODEC.
 * BeesStayInHive is not syncable and is absent from the network timeline. */
public final class EnvironmentAttributesProbe {
    static final String[] NAMES={"minecraft:gameplay/bees_stay_in_hive","minecraft:gameplay/creaking_active"};
    static CompoundTag project(EnvironmentAttributeMap map) {
        var full=(CompoundTag)EnvironmentAttributeMap.CODEC.encodeStart(NbtOps.INSTANCE,map).getOrThrow();
        var result=new CompoundTag();
        for(var key:NAMES) if(full.contains(key)) result.put(key,full.get(key).copy());
        return result;
    }
    static void record(PrintWriter out,String registry,String key,CompoundTag data) {
        var row=new JsonObject();row.addProperty("registry",registry);row.addProperty("key",key);row.addProperty("snbt",data.toString());out.println(row);
    }
    public static void main(String[] args)throws Exception {
        SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
        var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
        MinecraftServer.configurePackRepository(packs,new net.minecraft.world.level.WorldDataConfiguration(new net.minecraft.world.level.DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
        try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var corpus=new PrintWriter(args[0]);var samples=new PrintWriter(args[1]);var maps=new PrintWriter(args[2])) {
            var layers=RegistryLayer.createRegistryAccess();
            var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(RegistryLayer.STATIC)));
            var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();
            layers=layers.replaceFrom(RegistryLayer.WORLDGEN,worldgen);
            var all=new ArrayList<HolderLookup.RegistryLookup<?>>(lookups);worldgen.listRegistries().forEach(all::add);
            var dimensions=RegistryDataLoader.load(resources,all,RegistryDataLoader.DIMENSION_REGISTRIES,Runnable::run).join();
            var access=layers.replaceFrom(RegistryLayer.DIMENSIONS,dimensions).compositeAccess().freeze();
            var biomes=access.lookupOrThrow(Registries.BIOME);var types=access.lookupOrThrow(Registries.DIMENSION_TYPE);var timelines=access.lookupOrThrow(Registries.TIMELINE);
            var ops=RegistryOps.create(NbtOps.INSTANCE,access);
            for(var holder:biomes.listElements().toList()) {
                var data=new CompoundTag();data.put("attributes",project(holder.value().getAttributes()));record(corpus,"minecraft:worldgen/biome",holder.getRegisteredName(),data);
            }
            for(var holder:types.listElements().toList()) {
                var type=holder.value();var data=new CompoundTag();data.put("attributes",project(type.attributes()));data.putBoolean("has_skylight",type.hasSkyLight());data.putBoolean("has_ceiling",type.hasCeiling());
                var names=new ListTag();for(var timeline:type.timelines())names.add(StringTag.valueOf(timeline.getRegisteredName()));data.put("timelines",names);record(corpus,"minecraft:dimension_type",holder.getRegisteredName(),data);
            }
            for(var holder:timelines.listElements().toList()) {
                var full=(CompoundTag)Timeline.DIRECT_CODEC.encodeStart(ops,holder.value()).getOrThrow();var data=new CompoundTag();data.put("clock",full.get("clock").copy());if(full.contains("period_ticks"))data.put("period_ticks",full.get("period_ticks").copy());
                var tracks=new CompoundTag();var original=full.getCompoundOrEmpty("tracks");for(var name:NAMES)if(original.contains(name))tracks.put(name,original.get(name).copy());data.put("tracks",tracks);record(corpus,"minecraft:timeline",holder.getRegisteredName(),data);
            }
            for(var modifier:List.of("override","and","nand","or","nor","xor","xnor"))for(boolean base:new boolean[]{false,true})for(var argument:List.of("0b","1b","256","0.5f","-256.5d")) {
                String source="{\"minecraft:gameplay/bees_stay_in_hive\":{modifier:\""+modifier+"\",argument:"+argument+"}}";
                var decoded=EnvironmentAttributeMap.CODEC.parse(NbtOps.INSTANCE,TagParser.parseCompoundFully(source)).getOrThrow();
                var row=new JsonObject();row.addProperty("input_snbt",source);row.addProperty("base",base);row.addProperty("value",decoded.applyModifier(EnvironmentAttributes.BEES_STAY_IN_HIVE,base));row.addProperty("output_snbt",project(decoded).toString());maps.println(row);
            }
            long[] ticks={Long.MIN_VALUE,-24001,-1,0,12541,12542,12543,12599,12600,12601,23400,23401,23402,23459,23460,23461,23999,24000,Long.MAX_VALUE};
            float[][] weather={{0,0},{0.49999997F,0},{0.5F,0},{1,0.5F},{0.9F,0.45F},{1,1}};
            for(var type:types.listElements().toList())for(var biomeName:List.of("plains","pale_garden","nether_wastes"))for(long time:ticks)for(float[] strengths:weather) {
                var biome=biomes.getOrThrow(ResourceKey.create(Registries.BIOME,Identifier.withDefaultNamespace(biomeName))).value();
                var builder=EnvironmentAttributeSystem.builder().addConstantLayer(type.value().attributes());
                builder.addPositionalLayer(EnvironmentAttributes.BEES_STAY_IN_HIVE,(base,pos,interpolator)->biome.getAttributes().applyModifier(EnvironmentAttributes.BEES_STAY_IN_HIVE,base));
                builder.addPositionalLayer(EnvironmentAttributes.CREAKING_ACTIVE,(base,pos,interpolator)->biome.getAttributes().applyModifier(EnvironmentAttributes.CREAKING_ACTIVE,base));
                ClockManager clock=definition->{if(!definition.getRegisteredName().equals("minecraft:overworld"))throw new AssertionError(definition);return time;};
                for(var timeline:type.value().timelines())builder.addTimelineLayer(timeline,clock);
                // Here world dimension identity matches the type's builtin key.
                if(type.value().hasSkyLight()&&!type.value().hasCeiling()&&!type.getRegisteredName().equals("minecraft:the_end"))WeatherAttributes.addBuiltinLayers(builder,new WeatherAttributes.WeatherAccess(){public float rainLevel(){return strengths[0];}public float thunderLevel(){return strengths[1];}});
                var system=builder.build();var row=new JsonObject();row.addProperty("dimension",type.getRegisteredName());row.addProperty("biome","minecraft:"+biomeName);row.addProperty("clock",time);row.addProperty("rain",strengths[0]);row.addProperty("thunder",strengths[1]);row.addProperty("bees",system.getValue(EnvironmentAttributes.BEES_STAY_IN_HIVE,new Vec3(-0.5,64.5,3.5)));row.addProperty("creaking",system.getValue(EnvironmentAttributes.CREAKING_ACTIVE,new Vec3(-0.5,64.5,3.5)));samples.println(row);
            }
        }
    }
}
