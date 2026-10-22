import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.PrintStream;
import java.util.Optional;
import net.minecraft.SharedConstants;
import net.minecraft.core.Vec3i;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.levelgen.structure.placement.RandomSpreadStructurePlacement;
import net.minecraft.world.level.levelgen.structure.placement.RandomSpreadType;
import net.minecraft.world.level.levelgen.structure.placement.StructurePlacement.FrequencyReductionMethod;

/** Executes genuine Paper placement methods; no placement arithmetic is mocked. */
public final class StructurePlacementProbe {
    public static void main(String[] args) throws Exception {
        try (var output = new PrintStream(args[0])) {
            SharedConstants.tryDetectVersion();
            Bootstrap.bootStrap();
            long[] seeds = {0L, 42L, Long.MIN_VALUE};
            int[][] positions = {{0, 0}, {-1, -1}, {-33, 65}, {12345, -54321}, {Integer.MIN_VALUE, Integer.MAX_VALUE}};
            int[][] grids = {{1, 0, 0}, {32, 8, 10387312}, {34, 8, 165745295}, {4096, 4095, Integer.MAX_VALUE}};
            for (int[] grid : grids) for (var spread : RandomSpreadType.values())
                for (long seed : seeds) for (int[] pos : positions) {
                    var placement = new RandomSpreadStructurePlacement(new Vec3i(9, -4, 16),
                        FrequencyReductionMethod.DEFAULT, 1.0F, grid[2], Optional.empty(), grid[0], grid[1], spread);
                    var candidate = placement.getPotentialStructureChunk(seed, pos[0], pos[1]);
                    var row = new JsonObject();
                    row.addProperty("kind", "spread");
                    row.addProperty("seed", seed);
                    row.addProperty("spacing", grid[0]);
                    row.addProperty("separation", grid[1]);
                    row.addProperty("salt", grid[2]);
                    row.addProperty("spread_type", spread.getSerializedName());
                    row.addProperty("x", pos[0]); row.addProperty("z", pos[1]);
                    var potential = new JsonArray(); potential.add(candidate.x()); potential.add(candidate.z());
                    row.add("potential", potential);
                    var locate = placement.getLocatePos(candidate);
                    var location = new JsonArray(); location.add(locate.getX()); location.add(locate.getY()); location.add(locate.getZ());
                    row.add("locate", location);
                    output.println(row);
                }
            for (var method : FrequencyReductionMethod.values()) for (long seed : seeds)
                for (int[] pos : positions) for (float frequency : new float[]{0.0F, 0.01F, 0.25F, 1.0F})
                    for (Integer override : new Integer[]{null, 10387320}) {
                        var placement = new RandomSpreadStructurePlacement(Vec3i.ZERO, method, frequency,
                            165745295, Optional.empty(), 32, 8, RandomSpreadType.LINEAR);
                        var row = new JsonObject();
                        row.addProperty("kind", "frequency"); row.addProperty("method", method.getSerializedName());
                        row.addProperty("seed", seed); row.addProperty("salt", 165745295);
                        row.addProperty("x", pos[0]); row.addProperty("z", pos[1]);
                        row.addProperty("frequency", frequency); row.addProperty("override", override);
                        row.addProperty("result", placement.applyAdditionalChunkRestrictions(pos[0], pos[1], seed, override));
                        output.println(row);
                    }
        }
    }
}
