import com.google.gson.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.nbt.TagParser;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.util.ProblemReporter;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BeehiveBlockEntity;
import net.minecraft.world.level.storage.TagValueInput;

/** Real Paper hive load/store/save, without a server world or entity simulation. */
public final class HiveStorageProbe {
    record Case(String name, String input, int[] append) {}
    public static void main(String[] args) {
        var output = System.out;
        var errors = System.err;
        try {
            SharedConstants.tryDetectVersion(); Bootstrap.bootStrap();
            var registries = RegistryLayer.createRegistryAccess().getLayer(RegistryLayer.STATIC);
            String bee = "{entity_data:{id:'minecraft:bee'},ticks_in_hive:42,min_ticks_in_hive:600}";
            for (var test : List.of(
                new Case("fresh", null, new int[]{}),
                new Case("three_worldgen", null, new int[]{0, 598, 123}),
                new Case("store_above_capacity", null, new int[]{1,2,3,4,5}),
                new Case("load_empty", "{}", new int[]{7}),
                new Case("load_existing", "{bees:["+bee+"],flower_pos:[I;-4,70,29],'Bukkit.MaxEntities':8}", new int[]{99}),
                new Case("load_extra", "{bees:["+bee+"],unknown:5,PublicBukkitValues:{'probe:key':1}}", new int[]{}),
                new Case("numeric_coercion", "{bees:[{entity_data:{id:'minecraft:bee',Age:-30},ticks_in_hive:42s,min_ticks_in_hive:600L}],'Bukkit.MaxEntities':7b}", new int[]{13}),
                new Case("negative_ticks", "{bees:[{entity_data:{id:'minecraft:bee'},ticks_in_hive:-8,min_ticks_in_hive:-2}]}", new int[]{}),
                new Case("invalid_bees_shape", "{bees:{bad:1},flower_pos:[I;1,2]}", new int[]{31}),
                new Case("invalid_occupant", "{bees:[{entity_data:{id:'minecraft:bee'},ticks_in_hive:1}]}", new int[]{}),
                new Case("partially_invalid_list", "{bees:["+bee+",{entity_data:{id:'minecraft:bee'},ticks_in_hive:1}]}", new int[]{}),
                new Case("unknown_entity", "{bees:[{entity_data:{id:'invalid:missing'},ticks_in_hive:1,min_ticks_in_hive:600}]}", new int[]{}),
                new Case("non_bee_entity", "{bees:[{entity_data:{id:'minecraft:pig',Age:12},ticks_in_hive:1,min_ticks_in_hive:600}]}", new int[]{}),
                new Case("wrong_max_type", "{'Bukkit.MaxEntities':'bad'}", new int[]{}),
                new Case("valid_components", "{components:{'minecraft:custom_data':{probe:1},'minecraft:damage':3}}", new int[]{}),
                new Case("invalid_components", "{components:{'invalid:unknown':3,'minecraft:damage':'bad','minecraft:custom_data':{probe:2}}}", new int[]{}),
                new Case("fractional_numbers", "{bees:[{entity_data:{id:'bee'},ticks_in_hive:-1.7f,min_ticks_in_hive:600.8d}],'Bukkit.MaxEntities':-1.7f}", new int[]{}),
                new Case("oversized_flower", "{flower_pos:[I;-4,70,29,123]}", new int[]{}))) {
                var hive = new BeehiveBlockEntity(new BlockPos(-17,70,31), Blocks.BEE_NEST.defaultBlockState());
                if (test.input() != null) hive.loadWithComponents(TagValueInput.create(
                    ProblemReporter.DISCARDING, registries, TagParser.parseCompoundFully(test.input())));
                for (int ticks : test.append()) hive.storeBee(BeehiveBlockEntity.Occupant.create(ticks));
                var row = new JsonObject();
                row.addProperty("name", test.name());
                row.addProperty("input_snbt", test.input());
                row.add("append", new Gson().toJsonTree(test.append()));
                row.addProperty("output_snbt", hive.saveWithFullMetadata(registries).toString());
                output.println("HIVE_STORAGE\t" + row);
            }
        } catch (Throwable error) { error.printStackTrace(errors); System.exit(1); }
    }
}
