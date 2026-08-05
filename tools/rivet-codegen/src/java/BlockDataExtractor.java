import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.properties.Property;

/**
 * Extracts the vanilla block registry + per-block state properties from the
 * real Paper jar and writes them to a JSON file.
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" BlockDataExtractor --output block_states.json --version 26.2
 *
 * This is the "read from the real data" half of tools/rivet-codegen extract.
 */
public final class BlockDataExtractor {
    private BlockDataExtractor() {}

    public static void main(String[] args) throws Exception {
        String output = null;
        String version = null;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--output" -> output = args[++i];
                case "--version" -> version = args[++i];
                default -> throw new IllegalArgumentException("Unknown arg: " + args[i]);
            }
        }
        if (output == null || version == null) {
            throw new IllegalArgumentException("Usage: BlockDataExtractor --output <file.json> --version <mc>");
        }

        net.minecraft.SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        JsonObject root = new JsonObject();
        root.addProperty("minecraft_version", version);
        JsonArray blocks = new JsonArray();
        for (Block block : BuiltInRegistries.BLOCK) {
            JsonObject entry = new JsonObject();
            entry.addProperty("id", BuiltInRegistries.BLOCK.getId(block));
            Identifier key = BuiltInRegistries.BLOCK.getKey(block);
            entry.addProperty("name", key == null ? "minecraft:air" : key.toString());

            @SuppressWarnings({"rawtypes", "unchecked"})
            JsonArray props = new JsonArray();
            for (Property property : block.getStateDefinition().getProperties()) {
                JsonObject prop = new JsonObject();
                prop.addProperty("name", property.getName());
                JsonArray values = new JsonArray();
                // Use Property.getName(T) — for EnumProperty this is
                // value.getSerializedName() (the SNBT/blockstate string), not
                // enum name(). Order matches getPossibleValues().
                for (Object value : property.getPossibleValues()) {
                    values.add(property.getName((Comparable) value));
                }
                prop.add("values", values);
                props.add(prop);
            }
            entry.add("properties", props);
            blocks.add(entry);
        }
        root.add("blocks", blocks);

        Gson gson = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();
        try (PrintWriter writer = new PrintWriter(output, "UTF-8")) {
            gson.toJson(root, writer);
        }
        System.out.println("extracted " + blocks.size() + " blocks to " + output);
    }
}
