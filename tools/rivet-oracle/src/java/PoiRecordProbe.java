import com.google.gson.*;
import java.io.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicInteger;
import net.minecraft.core.*;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.entity.ai.village.poi.*;
import net.minecraft.world.level.block.Block;

/** Actual complete builtin POI state catalog and native ticket transitions. */
public final class PoiRecordProbe {
    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion(); Bootstrap.bootStrap();
        var acquire = PoiRecord.class.getDeclaredMethod("acquireTicket"); acquire.setAccessible(true);
        var release = PoiRecord.class.getDeclaredMethod("releaseTicket"); release.setAccessible(true);
        try (var out = new PrintStream(args[0])) {
            for (var type : BuiltInRegistries.POINT_OF_INTEREST_TYPE) {
                var id = BuiltInRegistries.POINT_OF_INTEREST_TYPE.getKey(type);
                var holder = BuiltInRegistries.POINT_OF_INTEREST_TYPE.wrapAsHolder(type);
                var catalog = new JsonObject(); catalog.addProperty("kind", "type");
                catalog.addProperty("name", id.toString()); catalog.addProperty("max_tickets", type.maxTickets()); catalog.addProperty("valid_range", type.validRange());
                catalog.add("states", new Gson().toJsonTree(type.matchingStates().stream().mapToInt(Block::getId).sorted().toArray())); out.println(catalog);
                for (int initial : new int[]{Integer.MIN_VALUE, -2, 0, type.maxTickets(), type.maxTickets()+1, Integer.MAX_VALUE}) {
                    var dirty = new AtomicInteger();
                    var record = new PoiRecord.Packed(new BlockPos(-17, 63, 32), holder, initial).unpack(dirty::incrementAndGet);
                    var row = new JsonObject(); row.addProperty("kind", "record"); row.addProperty("name", id.toString()); row.addProperty("initial", initial);
                    var steps = new JsonArray();
                    for (String action : new String[]{"acquire", "release", "release", "acquire"}) {
                        boolean result = (boolean)(action.equals("acquire") ? acquire : release).invoke(record);
                        var step = new JsonObject(); step.addProperty("action",action); step.addProperty("result",result); step.addProperty("free",record.getFreeTickets()); step.addProperty("dirty",dirty.get()); step.addProperty("space",record.hasSpace()); step.addProperty("occupied",record.isOccupied()); steps.add(step);
                    }
                    row.add("steps",steps); out.println(row);
                }
            }
            var ops = net.minecraft.resources.RegistryOps.create(net.minecraft.nbt.NbtOps.INSTANCE, RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY));
            for (String input : new String[]{
                "{pos:[I;1,2,3],type:'home'}", "{pos:[I;-17,63,32],type:'minecraft:meeting',free_tickets:32}",
                "{pos:[I;1,2,3],type:'beehive',free_tickets:-9}", "{pos:[I;1,2,3],type:'missing'}",
                "{pos:[I;1,2,3],type:'home',free_tickets:'wrong'}", "{type:'home'}",
                "{pos:[I;1,2,3,4],type:'home'}", "{pos:[I;1,2],type:'home'}"
            }) {
                var decoded = PoiRecord.Packed.CODEC.parse(ops, net.minecraft.nbt.TagParser.parseCompoundFully(input));
                var value = decoded.resultOrPartial(message -> {});
                var row = new JsonObject(); row.addProperty("kind","codec"); row.addProperty("input",input); row.addProperty("success",decoded.result().isPresent());
                if (value.isPresent()) row.addProperty("encoded",PoiRecord.Packed.CODEC.encodeStart(ops,value.get()).getOrThrow().toString());
                out.println(row);
            }
            var home = BuiltInRegistries.POINT_OF_INTEREST_TYPE.getOrThrow(PoiTypes.HOME);
            var original = new PoiRecord.Packed(new BlockPos(1,2,3),home,0);
            var other = new MappedRegistry<PoiType>(net.minecraft.core.registries.Registries.POINT_OF_INTEREST_TYPE,com.mojang.serialization.Lifecycle.stable());
            Registry.register(other,PoiTypes.HOME,home.value());other.freeze();
            for(String context:new String[]{"plain","empty","foreign","direct"}) {
                com.mojang.serialization.DynamicOps<net.minecraft.nbt.Tag> contextOps=switch(context){
                    case "plain"->net.minecraft.nbt.NbtOps.INSTANCE;
                    case "empty"->net.minecraft.resources.RegistryOps.create(net.minecraft.nbt.NbtOps.INSTANCE,RegistryAccess.EMPTY);
                    case "foreign"->net.minecraft.resources.RegistryOps.create(net.minecraft.nbt.NbtOps.INSTANCE,new RegistryAccess.ImmutableRegistryAccess(List.of(other)));
                    default->ops;
                };
                var input=net.minecraft.nbt.TagParser.parseCompoundFully("{pos:[I;1,2,3],type:'home'}");
                var decoded=PoiRecord.Packed.CODEC.parse(contextOps,input);
                var value=context.equals("direct")?new PoiRecord.Packed(original.pos(),Holder.direct(home.value()),0):original;
                var encoded=PoiRecord.Packed.CODEC.encodeStart(contextOps,value);
                var row=new JsonObject();row.addProperty("kind","authority");row.addProperty("context",context);row.addProperty("decode_success",decoded.result().isPresent());row.addProperty("encode_success",encoded.result().isPresent());
                if(context.equals("plain")||context.equals("empty")){row.addProperty("decode_error",decoded.error().orElseThrow().message());row.addProperty("encode_error",encoded.error().orElseThrow().message());}
                if(context.equals("direct"))row.addProperty("encode_error",encoded.error().orElseThrow().message());
                out.println(row);
            }
        }
    }
}
