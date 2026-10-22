import io.papermc.paper.world.saveddata.PaperWorldMetadata;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtOps;

/** Actual Paper metadata codec, including SavedDataStorage's partial recovery. */
public final class WorldIdentityProbe {
    public static void main(String[] args) {
        for (int[] words : new int[][] {
            {0, -1, Integer.MIN_VALUE, 9},
            {0, -1, Integer.MIN_VALUE, 9, 77},
            {0, -1, Integer.MIN_VALUE}
        }) {
            var input = new CompoundTag();
            input.putIntArray("uuid", words);
            var result = PaperWorldMetadata.CODEC.parse(NbtOps.INSTANCE, input);
            var value = result.resultOrPartial(message -> System.out.println("diagnostic=" + message));
            System.out.println("words=" + words.length + " uuid=" + value.map(metadata -> metadata.uuid().toString()).orElse("absent"));
        }
    }
}
