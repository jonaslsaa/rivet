import com.google.gson.JsonObject;
import java.io.PrintStream;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.storage.loot.LootTable;
import org.bukkit.NamespacedKey;
import org.bukkit.craftbukkit.CraftLootTable;
import org.bukkit.craftbukkit.CraftRegistry;
import org.bukkit.craftbukkit.block.*;

/** Real CraftBlockStates snapshots and CraftChest/CreatureSpawner setters.
 * No block-entity serialization or defaults are reimplemented in this probe.
 * Fresh spawner has no weighted potentials and therefore consumes no RNG.
 */
public final class StructureSnapshotProbe {
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion(); Bootstrap.bootStrap();
        var access = RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY);
        CraftRegistry.setMinecraftRegistry(access);
        org.bukkit.Bukkit.setServer((org.bukkit.Server)java.lang.reflect.Proxy.newProxyInstance(org.bukkit.Server.class.getClassLoader(),new Class<?>[]{org.bukkit.Server.class},(self,method,a)->switch(method.getName()) {
            case "getName","getVersion","getBukkitVersion" -> "snapshot-oracle";
            case "getLogger" -> java.util.logging.Logger.getLogger("snapshot-oracle");
            case "getLootTable" -> new CraftLootTable((NamespacedKey)a[0],LootTable.EMPTY);
            default -> throw new UnsupportedOperationException(method.toString());
        }));
        var pos = new BlockPos(8,64,8);
        try (var out = new PrintStream(args[0])) {
            var types = new com.google.gson.JsonArray();
            for (var block : BuiltInRegistries.BLOCK) {
                boolean has = block.defaultBlockState().hasBlockEntity();
                for (var state : block.getStateDefinition().getPossibleStates()) if(state.hasBlockEntity()!=has) throw new AssertionError("state-dependent block entity classification");
                if(has) {
                    types.add(BuiltInRegistries.BLOCK.getKey(block).toString());
                }
            }
            var classification = new JsonObject(); classification.addProperty("name","block_entity_types"); classification.add("types",types);out.println(classification);
            var chest = (CraftChest) CraftBlockStates.getBlockState(access,pos,Blocks.CHEST.defaultBlockState(),null);
            row(out,"fresh_chest",chest);
            chest.setLootTable(new CraftLootTable(NamespacedKey.minecraft("chests/buried_treasure"),LootTable.EMPTY));
            chest.setSeed(42);
            row(out,"loot_chest",chest);
            var spawner = (CraftCreatureSpawner) CraftBlockStates.getBlockState(access,pos,Blocks.SPAWNER.defaultBlockState(),null);
            row(out,"fresh_spawner",spawner);
            spawner.setSpawnedType(org.bukkit.entity.EntityType.CAVE_SPIDER);
            row(out,"cave_spider_spawner",spawner);
            loaded(out,access,pos,"load_chest_zero_seed",Blocks.CHEST.defaultBlockState(),"{id:\"minecraft:chest\",LootTable:\"chests/buried_treasure\",LootTableSeed:0L,Items:[],PublicBukkitValues:{\"test:value\":7}}");
            loaded(out,access,pos,"load_spawner_missing",Blocks.SPAWNER.defaultBlockState(),"{id:\"minecraft:mob_spawner\",unknown:1}");
            loaded(out,access,pos,"load_spawner_numeric_and_potentials",Blocks.SPAWNER.defaultBlockState(),"{id:\"minecraft:mob_spawner\",Delay:40000,Paper.Delay:40001,MinSpawnDelay:40001,MaxSpawnDelay:90000,SpawnCount:70000,SpawnRange:-40000,SpawnData:{entity:{id:\"cave_spider\",custom:7},unknown:9},SpawnPotentials:[{weight:2,data:{entity:{id:\"zombie\"}}},{weight:0,data:{entity:{foo:9}}}],PublicBukkitValues:{\"test:value\":7}}");
        }
    }
    static void row(PrintStream out,String name,CraftBlockEntityState<?> snapshot) {
        var row = new JsonObject(); row.addProperty("name",name);
        row.addProperty("state",net.minecraft.world.level.block.Block.getId(snapshot.getHandle()));
        row.addProperty("nbt",snapshot.getSnapshotNBT().toString()); out.println(row);
    }
    static void loaded(PrintStream out,RegistryAccess access,BlockPos pos,String name,net.minecraft.world.level.block.state.BlockState state,String input) throws Exception {
        var snapshot=(CraftBlockEntityState<?>)CraftBlockStates.getBlockState(access,pos,state,net.minecraft.nbt.TagParser.parseCompoundFully(input));
        var row=new JsonObject();row.addProperty("name",name);row.addProperty("state",net.minecraft.world.level.block.Block.getId(state));row.addProperty("input",input);row.addProperty("nbt",snapshot.getSnapshotNBT().toString());out.println(row);
    }
}
