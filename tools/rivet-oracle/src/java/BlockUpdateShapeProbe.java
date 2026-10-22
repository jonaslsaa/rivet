import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;

/** Declaring implementation, not guessed block categories, for updateShape. */
public final class BlockUpdateShapeProbe {
    public static void main(String[] args) {
        var output = System.out;
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        for (var block : BuiltInRegistries.BLOCK) {
            Class<?> owner = block.getClass();
            search: while (owner != null) {
                for (var method : owner.getDeclaredMethods()) {
                    if (method.getName().equals("updateShape") && method.getParameterCount() == 8) break search;
                }
                owner = owner.getSuperclass();
            }
            if (owner == null) throw new IllegalStateException("Missing updateShape: " + block);
            output.println("UPDATE_OWNER\t" + BuiltInRegistries.BLOCK.getKey(block) + "\t" + owner.getSimpleName());
        }
        var shape = new net.minecraft.world.phys.shapes.BitSetDiscreteVoxelShape(3, 2, 3);
        shape.fill(0, 0, 0);
        shape.fill(0, 0, 1);
        shape.fill(2, 1, 2);
        shape.forAllFaces((direction, x, y, z) -> output.println("EDGE_FACE\t" + direction.getSerializedName() + "\t" + x + "\t" + y + "\t" + z));
    }
}
