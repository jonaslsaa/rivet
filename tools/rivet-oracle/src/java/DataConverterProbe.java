import ca.spottedleaf.dataconverter.converters.DataConverter;
import ca.spottedleaf.dataconverter.converters.datatypes.DataHook;
import ca.spottedleaf.dataconverter.converters.datatypes.DataType;
import ca.spottedleaf.dataconverter.converters.datatypes.DataWalker;
import ca.spottedleaf.dataconverter.types.ListType;
import ca.spottedleaf.dataconverter.types.MapType;
import ca.spottedleaf.dataconverter.types.ObjectType;
import ca.spottedleaf.dataconverter.types.TypeUtil;
import ca.spottedleaf.dataconverter.types.Types;
import ca.spottedleaf.dataconverter.types.nbt.NBTListType;
import ca.spottedleaf.dataconverter.types.nbt.NBTMapType;
import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.PrintWriter;
import java.math.BigDecimal;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

/**
 * Samples the pinned Paper 26.2 {@code ca.spottedleaf.dataconverter} foundation
 * classes that issue #535 ports: the version-encoding arithmetic and ordering
 * on {@link DataConverter}, the {@link DataType}/{@link DataHook}/
 * {@link DataWalker} dispatch contract, and the {@code types} abstract-layer
 * defaults ({@code ObjectType.getType}, {@code MapType.setGeneric} /
 * {@code getList(type)} / {@code getOrCreateList} / {@code getOrCreateMap},
 * {@code ListType.setGeneric} / {@code addGeneric}) exercised over the real NBT
 * backing.
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" DataConverterProbe --output dir/
 *
 * Emits a stable golden JSON (issue #535). No registry/version boot is needed:
 * the NBT-backed types and the converter dispatch layer are value-leaf.
 */
public final class DataConverterProbe {
    private DataConverterProbe() {}

    public static void main(String[] args) throws Exception {
        String output = null;
        String paper = "26.2-DEV-main@0a99345";
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--output" -> output = args[++i];
                case "--paper" -> paper = args[++i];
                default -> throw new IllegalArgumentException("Unknown arg: " + args[i]);
            }
        }
        if (output == null) {
            throw new IllegalArgumentException("Usage: DataConverterProbe --output <dir> [--paper <pin>]");
        }

        JsonObject root = new JsonObject();
        root.addProperty("paper", paper);

        // ---- DataConverter.encodeVersions / getVersion / getStep / encodedToString ----
        JsonArray enc = new JsonArray();
        int[][] pairs = {
            {0, 0}, {1, 0}, {1, 1}, {2, 0}, {-1, 0}, {-1, -1}, {99, 0}, {1344, 0},
            {1344, 1}, {1344, 2147483647}, {1344, -2147483648}, {268435456, 1234},
            {-2147483648, 0}, {-2147483648, -2147483648},
        };
        for (int[] p : pairs) {
            long encoded = DataConverter.encodeVersions(p[0], p[1]);
            JsonObject row = new JsonObject();
            row.addProperty("version", p[0]);
            row.addProperty("step", p[1]);
            row.addProperty("encoded", Long.toString(encoded));
            row.addProperty("roundTripVersion", DataConverter.getVersion(encoded));
            row.addProperty("roundTripStep", DataConverter.getStep(encoded));
            row.addProperty("encodedToString", DataConverter.encodedToString(encoded));
            enc.add(row);
        }
        root.add("encodeVersions", enc);

        // encoded < encodeVersions(version, step+1) for a step at the INT max edge
        {
            JsonObject monotonic = new JsonObject();
            long a = DataConverter.encodeVersions(5, 100);
            long b = DataConverter.encodeVersions(5, 101);
            monotonic.addProperty("a", Long.toString(a));
            monotonic.addProperty("b", Long.toString(b));
            monotonic.addProperty("aLessThanB", a < b);
            root.add("stepMonotonic", monotonic);
        }

        // ---- LOWEST_VERSION_COMPARATOR ordering ----
        {
            JsonArray order = new JsonArray();
            List<DataConverter<Object, Object>> convs = new ArrayList<>();
            convs.add(conv(5, 0, "c50"));
            convs.add(conv(5, 2, "c52"));
            convs.add(conv(5, 1, "c51"));
            convs.add(conv(3, 0, "c30"));
            convs.add(conv(100, 0, "c100"));
            convs.add(conv(5, -1, "c5-1"));
            convs.add(conv(3, 2147483647, "c3max"));
            convs.sort(DataConverter.LOWEST_VERSION_COMPARATOR);
            for (DataConverter<Object, Object> c : convs) {
                JsonObject row = new JsonObject();
                row.addProperty("name", c.getEncodedVersion() == Long.MIN_VALUE ? "min" : c.toString());
                row.addProperty("toVersion", c.getToVersion());
                row.addProperty("versionStep", c.getVersionStep());
                row.addProperty("encoded", Long.toString(c.getEncodedVersion()));
                order.add(row);
            }
            root.add("comparatorOrder", order);
        }

        // ---- DataType.convertOrOriginal ----
        {
            JsonObject dt = new JsonObject();
            DataType<Object, Object> nullConv = new DataType<Object, Object>() {
                @Override
                public Object convert(final Object data, final long fromVersion, final long toVersion) {
                    return null; // "no replacement"
                }
            };
            DataType<Object, Object> replaceConv = new DataType<Object, Object>() {
                @Override
                public Object convert(final Object data, final long fromVersion, final long toVersion) {
                    return "replaced";
                }
            };
            dt.addProperty("nullConverterKeepsOriginal", String.valueOf(nullConv.convertOrOriginal("orig", 1, 2)));
            dt.addProperty("replacingConverterReplaces", String.valueOf(replaceConv.convertOrOriginal("orig", 1, 2)));
            root.add("dataTypeConvertOrOriginal", dt);
        }

        // ---- DataHook + DataWalker contract ----
        {
            JsonObject hw = new JsonObject();
            DataHook<Object, Object> hook = new DataHook<Object, Object>() {
                @Override
                public Object preHook(final Object data, final long fromVersion, final long toVersion) {
                    return data;
                }
                @Override
                public Object postHook(final Object data, final long fromVersion, final long toVersion) {
                    return null;
                }
            };
            hw.addProperty("preHookPassthrough", String.valueOf(hook.preHook("d", 1, 2)));
            hw.addProperty("postHookNull", hook.postHook("d", 1, 2) == null);
            hw.addProperty("noOpWalkNull", DataWalker.noOp().walk("d", 1, 2) == null);
            root.add("hookWalker", hw);
        }

        // ---- ObjectType.getType boundaries ----
        {
            JsonObject ot = new JsonObject();
            addType(ot, "byte", Byte.valueOf((byte)3));
            addType(ot, "short", Short.valueOf((short)3));
            addType(ot, "int", Integer.valueOf(3));
            addType(ot, "long", Long.valueOf(3L));
            addType(ot, "float", Float.valueOf(3.0f));
            addType(ot, "double", Double.valueOf(3.0));
            addType(ot, "bigDecimal", new BigDecimal("3"));
            addType(ot, "string", "abc");
            addType(ot, "boolean", Boolean.TRUE);
            addType(ot, "byteArray", new byte[]{1, 2});
            addType(ot, "shortArray", new short[]{1, 2});
            addType(ot, "intArray", new int[]{1, 2});
            addType(ot, "longArray", new long[]{1, 2});
            addType(ot, "map", new NBTMapType());
            addType(ot, "list", new NBTListType());
            // ObjectType.getType(null) reaches `object.getClass()` unguarded and
            // throws NPE (ObjectType.java:59) — ported as a panic in Rust, not
            // an `Option::None`.
            boolean nullNpe = false;
            try {
                ObjectType.getType(null);
            } catch (final NullPointerException ex) {
                nullNpe = true;
            }
            ot.addProperty("null_npe", nullNpe);
            root.add("objectType", ot);
        }

        // ---- MapType default methods over NBTMapType ----
        {
            JsonObject mt = new JsonObject();
            NBTMapType map = new NBTMapType();
            // setGeneric dispatch
            map.setGeneric("b", Byte.valueOf((byte)1));
            map.setGeneric("s", Short.valueOf((short)2));
            map.setGeneric("i", Integer.valueOf(3));
            map.setGeneric("l", Long.valueOf(4L));
            map.setGeneric("f", Float.valueOf(5.5f));
            map.setGeneric("d", Double.valueOf(6.5));
            map.setGeneric("bool", Boolean.TRUE);
            map.setGeneric("str", "seven");
            map.setGeneric("map", new NBTMapType());
            map.setGeneric("list", new NBTListType());
            map.setGeneric("bytes", new byte[]{8, 9});
            map.setGeneric("ints", new int[]{10, 11});
            map.setGeneric("longs", new long[]{12, 13});
            mt.addProperty("b", String.valueOf(map.getGeneric("b")));
            mt.addProperty("s", String.valueOf(map.getGeneric("s")));
            mt.addProperty("i", String.valueOf(map.getGeneric("i")));
            mt.addProperty("l", String.valueOf(map.getGeneric("l")));
            mt.addProperty("f", String.valueOf(map.getGeneric("f")));
            mt.addProperty("d", String.valueOf(map.getGeneric("d")));
            mt.addProperty("bool", String.valueOf(map.getGeneric("bool")));
            mt.addProperty("str", String.valueOf(map.getGeneric("str")));
            mt.addProperty("map", map.getGeneric("map").getClass().getName());
            mt.addProperty("list", map.getGeneric("list").getClass().getName());
            mt.addProperty("bytes", java.util.Arrays.toString((byte[])map.getGeneric("bytes")));
            mt.addProperty("ints", java.util.Arrays.toString((int[])map.getGeneric("ints")));
            mt.addProperty("longs", java.util.Arrays.toString((long[])map.getGeneric("longs")));

            // getNumber / boolean coercion from a byte
            mt.addProperty("getBooleanFromByte", map.getBoolean("b"));
            mt.addProperty("getNumberFromByte", String.valueOf(map.getNumber("b")));
            mt.addProperty("getIntCoerced", map.getInt("b"));

            // hasKey type filtering
            mt.addProperty("hasKey_int_AS_INT", map.hasKey("i", ObjectType.INT));
            mt.addProperty("hasKey_int_AS_BYTE", map.hasKey("i", ObjectType.BYTE));
            mt.addProperty("hasKey_int_AS_NUMBER", map.hasKey("i", ObjectType.NUMBER));
            mt.addProperty("hasKey_str_AS_STRING", map.hasKey("str", ObjectType.STRING));
            mt.addProperty("hasKey_unknown", map.hasKey("nope", ObjectType.INT));

            // getList type filter
            NBTMapType lists = new NBTMapType();
            NBTListType intList = new NBTListType();
            intList.addInt(1);
            intList.addInt(2);
            lists.setList("ints", intList);
            NBTListType emptyList = new NBTListType();
            lists.setList("empty", emptyList);
            mt.addProperty("getList_int_as_INT_notNull", lists.getList("ints", ObjectType.INT) != null);
            mt.addProperty("getList_int_as_STRING_null", lists.getList("ints", ObjectType.STRING) == null);
            mt.addProperty("getList_empty_as_any_notNull", lists.getList("empty", ObjectType.MAP) != null);
            mt.addProperty("getList_missing_null", lists.getList("nope", ObjectType.INT) == null);
            mt.addProperty("getListUnchecked_int_notNull", lists.getListUnchecked("ints") != null);

            // getOrCreateList / getOrCreateMap
            NBTMapType create = new NBTMapType();
            ListType createdList = create.getOrCreateList("k", ObjectType.INT);
            createdList.addInt(42);
            mt.addProperty("getOrCreateList_created", create.hasKey("k"));
            mt.addProperty("getOrCreateList_size", create.getListUnchecked("k").size());
            MapType createdMap = create.getOrCreateMap("m");
            createdMap.setInt("inner", 7);
            mt.addProperty("getOrCreateMap_created", create.hasKey("m"));
            mt.addProperty("getOrCreateMap_inner", create.getMap("m").getInt("inner"));

            root.add("mapTypeDefaults", mt);
        }

        // ---- ListType default setGeneric/addGeneric over NBTListType ----
        {
            JsonObject lt = new JsonObject();
            NBTListType list = new NBTListType();
            // setGeneric is a *set* (ListTag.set -> ArrayList.set): the index
            // must already be populated or it throws IndexOutOfBoundsException.
            list.addInt(0);
            list.setGeneric(0, Integer.valueOf(3));
            list.addGeneric(Short.valueOf((short)4));
            list.addGeneric("five");
            list.addGeneric(new NBTMapType());
            list.addGeneric(new int[]{6, 7});
            lt.addProperty("setGeneric_int", list.getInt(0));
            lt.addProperty("addGeneric_short", list.getShort(1));
            lt.addProperty("addGeneric_string", list.getString(2));
            lt.addProperty("addGeneric_map", list.getMap(3).getClass().getName());
            lt.addProperty("addGeneric_ints", java.util.Arrays.toString(list.getInts(4)));
            boolean setGenericOob = false;
            try {
                new NBTListType().setGeneric(0, Integer.valueOf(1));
            } catch (final IndexOutOfBoundsException ex) {
                setGenericOob = true;
            }
            lt.addProperty("setGeneric_empty_oob", setGenericOob);
            root.add("listTypeDefaults", lt);
        }

        Gson gson = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();
        try (PrintWriter pw = new PrintWriter(Path.of(output, "dataconverter-foundation.json").toFile(), "UTF-8")) {
            pw.print(gson.toJson(root));
        }
        System.out.println("wrote " + output + "/dataconverter-foundation.json");
    }

    private static void addType(final JsonObject ot, final String name, final Object value) {
        final ObjectType type = ObjectType.getType(value);
        // Null results (unhandled `Number` subtypes like `BigDecimal`, and
        // `Boolean`) are not recorded: Java's `getType` returns null for them
        // and the golden omits the key, keeping the output stable across runs.
        if (type != null) {
            ot.addProperty(name, type.name());
        }
    }

    private static DataConverter<Object, Object> conv(final int toVersion, final int versionStep, final String tag) {
        return new DataConverter<Object, Object>(toVersion, versionStep) {
            @Override
            public Object convert(final Object data, final long sourceVersion, final long toVersion) {
                return data;
            }
            // `toString` feeds the `name` field of the comparator-order golden.
            // The default `Object.toString` embeds the JVM identity hash, which
            // is not stable across runs; the tag makes regeneration byte-for-byte
            // deterministic (issue #535).
            @Override
            public String toString() {
                return tag;
            }
        };
    }
}
