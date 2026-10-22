import com.google.gson.*;
import com.mojang.serialization.JsonOps;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.levelgen.structure.*;

/** Pinned live registry IDs/order and original datapack JSON, not directory order. */
public final class StructureRegistryProbe {
    public static void main(String[] args)throws Exception {
        var out=args.length==0?System.out:new java.io.PrintStream(args[0]);
        SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
        if(args.length>1 && args[1].equals("entities")){
            var registry=net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE;
            var codec=registry.byNameCodec();
            if(!registry.getKey(codec.parse(JsonOps.INSTANCE,new JsonPrimitive("pig")).getOrThrow()).toString().equals("minecraft:pig")
                || codec.parse(JsonOps.INSTANCE,new JsonPrimitive("Minecraft:Pig")).error().isEmpty()
                || codec.parse(JsonOps.INSTANCE,new JsonPrimitive("minecraft:not_an_entity")).error().isEmpty())
                throw new AssertionError("Actual built-in entity Identifier codec contract changed");
            for(var entity:registry){
                var row=new JsonObject();row.addProperty("id",registry.getId(entity));row.addProperty("name",registry.getKey(entity).toString());row.addProperty("category",entity.getCategory().getName());
                out.println("STRUCTURE_REGISTRY\t"+row);
            }
            out.flush();if(args.length>0)out.close();return;
        }
        var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
        net.minecraft.server.MinecraftServer.configurePackRepository(packs,new net.minecraft.world.level.WorldDataConfiguration(new net.minecraft.world.level.DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
        try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected())){
            var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();
            var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
            var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();
            var access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess();var ops=RegistryOps.create(JsonOps.INSTANCE,access);
            var structures=access.lookupOrThrow(Registries.STRUCTURE);
            for(var holder:structures.listElements().toList()){
                var key=holder.key().identifier();var row=new JsonObject();row.addProperty("kind","structure");row.addProperty("key",key.toString());row.addProperty("id",structures.getId(holder.value()));
                try(var reader=resources.getResourceOrThrow(Identifier.fromNamespaceAndPath(key.getNamespace(),"worldgen/structure/"+key.getPath()+".json")).openAsReader()){row.add("data",JsonParser.parseReader(reader));}
                row.add("encoded",Structure.DIRECT_CODEC.encodeStart(ops,holder.value()).getOrThrow());out.println("STRUCTURE_REGISTRY\t"+row);
            }
            var sets=access.lookupOrThrow(Registries.STRUCTURE_SET);
            for(var holder:sets.listElements().toList()){
                var key=holder.key().identifier();var row=new JsonObject();row.addProperty("kind","structure_set");row.addProperty("key",key.toString());row.addProperty("id",sets.getId(holder.value()));
                try(var reader=resources.getResourceOrThrow(Identifier.fromNamespaceAndPath(key.getNamespace(),"worldgen/structure_set/"+key.getPath()+".json")).openAsReader()){row.add("data",JsonParser.parseReader(reader));}
                row.add("encoded",StructureSet.DIRECT_CODEC.encodeStart(ops,holder.value()).getOrThrow());out.println("STRUCTURE_REGISTRY\t"+row);
            }
            for(var tag:structures.listTags().sorted(java.util.Comparator.comparing(t->t.key().location().toString())).toList()){
                var row=new JsonObject();row.addProperty("kind","structure_tag");row.addProperty("key",tag.key().location().toString());var members=new JsonArray();tag.stream().forEach(holder->members.add(holder.unwrapKey().orElseThrow().identifier().toString()));row.add("members",members);out.println("STRUCTURE_REGISTRY\t"+row);
            }
        }
        out.flush();if(args.length>0)out.close();
    }
}
