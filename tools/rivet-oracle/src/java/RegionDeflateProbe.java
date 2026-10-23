import com.google.gson.JsonObject;
import java.io.*;
import java.util.HexFormat;
import net.minecraft.world.level.chunk.storage.RegionFileVersion;

/** Actual registered Paper stream wrappers, not a replacement compressor. */
public final class RegionDeflateProbe {
    static final HexFormat HEX = HexFormat.of();
    static byte[] read(byte[] compressed) throws Exception {
        try (var input = RegionFileVersion.VERSION_DEFLATE.wrap(new ByteArrayInputStream(compressed))) {
            return input.readAllBytes();
        }
    }
    public static void main(String[] args) throws Exception {
        if (args.length > 0 && args[0].equals("verify")) {
            try (var input = new BufferedReader(new InputStreamReader(System.in))) {
                for (String line; (line = input.readLine()) != null;) {
                    System.out.println(HEX.formatHex(read(HEX.parseHex(line))));
                }
            }
            return;
        }
        for (int size : new int[]{0, 15, 256, 4096, 8192}) {
            byte[] payload = new byte[size];
            for (int i = 0; i < size; ++i) payload[i] = (byte)(size == 4096 ? 65 : (i * 31 + i / 7));
            var bytes = new ByteArrayOutputStream();
            try (var output = RegionFileVersion.VERSION_DEFLATE.wrap(bytes)) { output.write(payload); }
            if (!java.util.Arrays.equals(payload, read(bytes.toByteArray()))) throw new AssertionError();
            var row = new JsonObject();
            row.addProperty("size", size);
            row.addProperty("compressed", HEX.formatHex(bytes.toByteArray()));
            System.out.println(row);
        }
    }
}
