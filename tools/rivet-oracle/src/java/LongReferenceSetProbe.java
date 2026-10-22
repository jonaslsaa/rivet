import it.unimi.dsi.fastutil.longs.LongOpenHashSet;
import java.util.Random;
import java.util.StringJoiner;

/** Actual bundled fastutil, hexadecimal longs avoid any JSON numeric coercion. */
public final class LongReferenceSetProbe {
    static String digest(long[] values) {
        try {
            var hash = java.security.MessageDigest.getInstance("SHA-256");
            for (long value : values) hash.update(java.nio.ByteBuffer.allocate(8).putLong(value).array());
            return java.util.HexFormat.of().formatHex(hash.digest());
        } catch (java.security.NoSuchAlgorithmException error) { throw new AssertionError(error); }
    }
    static String values(long[] values) {
        var text = new StringJoiner(",");
        for (long value : values) text.add(Long.toUnsignedString(value, 16));
        return values.length == 0 ? "-" : text.toString();
    }
    static void emit(String name, String mode, long[] input) {
        LongOpenHashSet set;
        if (mode.equals("array")) set = new LongOpenHashSet(input);
        else { set = new LongOpenHashSet(); for (long value : input) set.add(value); }
        long[] extra = {0, -1, Long.MIN_VALUE, Long.MAX_VALUE, 123, 456, 789};
        var clone = set.clone();
        for (long value : extra) clone.add(value);
        System.out.println(name + "\t" + mode + "\t" + values(input) + "\t" + digest(set.toLongArray())
            + "\t" + values(extra) + "\t" + digest(clone.toLongArray()));
    }
    public static void main(String[] args) {
        emit("monument", "default", new long[]{0x1234,0xabcd});
        emit("replacement", "array", new long[]{9,8});
        emit("dedup", "array", new long[]{1,2,1,3,2,3});
        for (int count : new int[]{0,1,2,3,23,24,25,48,49,96,97,193,289,512}) {
            long[] random = new Random(42).longs(count).toArray();
            long[] grid = new long[count];
            long[] duplicates = new long[count];
            for (int i = 0; i < count; i++) {
                int x = i / 17 - 8, z = i % 17 - 8;
                grid[i] = (x & 0xffffffffL) | ((z & 0xffffffffL) << 32);
                duplicates[i] = i % 7 == 0 ? Long.MIN_VALUE : i % 5;
            }
            for (String mode : new String[]{"default","array"}) {
                emit("random" + count, mode, random);
                emit("grid" + count, mode, grid);
                emit("duplicates" + count, mode, duplicates);
            }
        }
    }
}
