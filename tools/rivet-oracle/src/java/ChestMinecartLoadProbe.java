import com.google.gson.*;
import java.io.PrintStream;
import java.util.*;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.nbt.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.level.storage.TagValueOutput;

/** Actual EntityType.loadEntityRecursive LOAD,false; real native constructor,
 * Entity.load, minecart/container loaders, Craft wrapper and full save. */
public final class ChestMinecartLoadProbe {
    static void run(PrintStream out,String name,String input,boolean experimental)throws Exception {
        var level=ChestMinecartProbe.level(experimental);
        var random=new ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom(42L);Entity.SHARED_RANDOM=random;
        var callbacks=new JsonArray();
        var entity=EntityType.loadEntityRecursive(TagParser.parseCompoundFully(input),level,new EntitySpawnRequest(EntitySpawnReason.LOAD,false),loaded->{
            var callback=new JsonObject();callback.addProperty("id",loaded.getId());callback.addProperty("uuid",loaded.getUUID().toString());callback.addProperty("generation",loaded.generation);callbacks.add(callback);return loaded;
        });
        var row=new JsonObject();row.addProperty("name",name);row.addProperty("input",input);row.addProperty("experimental",experimental);row.add("callbacks",callbacks);row.addProperty("next_id",level.nextId);row.addProperty("rng_next",random.nextLong());
        if(entity!=null){var output=TagValueOutput.createWithContext(net.minecraft.util.ProblemReporter.DISCARDING,level.registryAccess());row.addProperty("save_result",entity.save(output));row.addProperty("saved",output.buildResult().toString());row.addProperty("old_x",entity.xo);row.addProperty("old_y",entity.yo);row.addProperty("old_z",entity.zo);}
        out.println(row);
    }
    public static void main(String[] args)throws Exception {
        System.setProperty("paper.debugEntitiesWithInvalidIds","true");
        net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
        org.bukkit.craftbukkit.CraftRegistry.setMinecraftRegistry(RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY));
        try(var out=new PrintStream(args[0])){
            for(boolean experimental:new boolean[]{false,true}){
                run(out,"minimal","{id:\"minecraft:chest_minecart\"}",experimental);
                run(out,"persisted_generated","{id:\"minecraft:chest_minecart\",Pos:[-16.5d,50.5d,32.5d],UUID:[I;1,2,3,4],HasTicked:1b,Items:[],LootTable:\"minecraft:chests/abandoned_mineshaft\",LootTableSeed:23L}",experimental);
                run(out,"primitive_fields","{id:\"minecraft:chest_minecart\",Pos:[-4.25d,64.5d,2.75d],Motion:[10.1d,-10.0d,1.25d],Rotation:[725.0f,275.0f],fall_distance:3.5d,Fire:12s,Air:40000,OnGround:1b,Invulnerable:1b,PortalCooldown:7,Spigot.ticksLived:11,Bukkit.visibleByDefault:0b,Bukkit.invisible:1b,Bukkit.MaxAirSupply:400,CustomNameVisible:1b,Silent:1b,NoGravity:1b,Glowing:1b,TicksFrozen:17,Paper.FireOverride:\"FALSE\",data:{x:1},BukkitValues:{\"test:x\":2},Paper.Origin:[1.0d,2.0d,3.0d],Paper.FromMobSpawner:1b,Paper.FromNetherPortal:1b,Paper.FreezeLock:1b,DisplayState:{Name:\"minecraft:oak_log\",Properties:{axis:\"x\"}},DisplayOffset:-5,FlippedRotation:1b,Paper.FrictionState:\"TRUE\"}",experimental);
                run(out,"clamp_and_invalid_uuid","{id:\"chest_minecart\",Pos:[1.0e30d,-1.0e30d,-1.0e30d],Motion:[11.0d,9.5d,-11.0d],Rotation:[-725.0f,-275.0f],UUID:[I;1,2,3],Paper.SpawnReason:\"CUSTOM\"}",experimental);
                run(out,"persist_false","{id:\"minecraft:chest_minecart\",Bukkit.persist:0b}",experimental);
                run(out,"invalid_id","{id:\"minecraft:not_an_entity\"}",experimental);
            }
        }
    }
}
