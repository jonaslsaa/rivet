import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.TreeMap;
import net.minecraft.world.level.chunk.DataLayer;
import net.minecraft.world.level.lighting.DataLayerStorageMap;
import net.minecraft.world.level.lighting.DynamicGraphMinFixedPoint;
import net.minecraft.world.level.lighting.LeveledPriorityQueue;
import net.minecraft.world.level.lighting.SpatialLongSet;
import net.minecraft.util.Mth;

/**
 * Captures deterministic Paper 26.2 golden traces for the
 * {@code mc.world.level.lighting.core} primitives (issue #184): queue pop
 * orders, spatial-set packing/pop order, the min-fixed-point flood, and the
 * storage-map semantics. Emits a flat {@code key=value} text file so the Rust
 * port can pin exact values against the real Paper runtime.
 *
 * Run inside the full bundler classpath (server jar + all libraries), e.g.:
 *   java -cp "<server.jar>:<all lib jars>" LightingCoreProbe --output trace.txt
 */
public final class LightingCoreProbe {
    private LightingCoreProbe() {}

    private static PrintWriter out;

    private static void emit(String line) {
        out.println(line);
    }

    /** A concrete {@link DataLayerStorageMap} (its only abstract method is {@code copy}).
     *  The copy copies every stored layer, like the real section-storage subclasses. */
    private static final class TestStorageMap extends DataLayerStorageMap<TestStorageMap> {
        TestStorageMap() {
            this(new it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<>());
        }

        TestStorageMap(it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<DataLayer> map) {
            super(map);
        }

        @Override
        public TestStorageMap copy() {
            // Shallow map clone, exactly like BlockDataLayerStorageMap/Sky
            // DataLayerStorageMap (`this.map.clone()`): the DataLayer references
            // are shared, so a fill through the copy is visible in the original.
            return new TestStorageMap(this.map.clone());
        }
    }

    /** A 9-node line graph; a node adopts its neighbor's level unchanged. */
    private static final class LineGraph extends DynamicGraphMinFixedPoint {
        final int[] levels;

        LineGraph(int size) {
            super(16, 4, 64);
            this.levels = new int[size];
        }

        @Override
        protected int getComputedLevel(long node, long knownParent, int knownLevelFromParent) {
            return knownLevelFromParent;
        }

        @Override
        protected void checkNeighborsAfterUpdate(long node, int level, boolean onlyDecrease) {
            if (node > 0) {
                this.checkNeighbor(node, node - 1, level, onlyDecrease);
            }
            if (node + 1 < this.levels.length) {
                this.checkNeighbor(node, node + 1, level, onlyDecrease);
            }
        }

        @Override
        protected int getLevel(long node) {
            return this.levels[(int) node];
        }

        @Override
        protected void setLevel(long node, int level) {
            this.levels[(int) node] = level;
        }

        @Override
        protected int computeLevelFromNeighbor(long from, long to, int fromLevel) {
            return fromLevel;
        }

        /** Drive the graph ops from inside the subclass, where the protected
         *  base methods are accessible (they are `protected` in Java). */
        void drive(String tag) {
            if (tag.equals("increase")) {
                emit("graph.hasWork.initial=" + this.hasWork());
                emit("graph.queueSize.initial=" + this.getQueueSize());
                this.setLevel(0, 15);
                emit("graph.checkNode0.hasWork=" + this.hasWork());
                emit("graph.queueSize.after.checkNode0=" + this.getQueueSize());
                this.checkEdge(0, 1, 15, false);
                emit("graph.queueSize.after.checkEdge.0-1=" + this.getQueueSize());
                emit("graph.runUpdates.ret=" + this.runUpdates(100));
                emit("graph.hasWork.final=" + this.hasWork());
                emitLevels("increase");
            } else if (tag.equals("decrease")) {
                for (int i = 0; i < 9; i++) {
                    this.setLevel(i, 15);
                }
                this.checkEdge(0, 0, 3, true);
                emit("graph.decrease.hasWork=" + this.hasWork());
                emit("graph.decrease.runUpdates.ret=" + this.runUpdates(100));
                emit("graph.decrease.hasWork.final=" + this.hasWork());
                emitLevels("decrease");
            } else if (tag.equals("removeIf")) {
                this.setLevel(0, 15);
                this.checkEdge(0, 1, 15, false);
                this.checkEdge(0, 2, 15, false);
                emit("graph.removeIf.queueSize.initial=" + this.getQueueSize());
                this.removeFromQueue(1);
                emit("graph.removeIf.queueSize.after.removeFromQueue=" + this.getQueueSize());
                emit("graph.removeIf.hasWork.after.removeFromQueue=" + this.hasWork());
                this.removeIf(n -> n == 2);
                emit("graph.removeIf.queueSize.after.removeIf=" + this.getQueueSize());
                emit("graph.removeIf.hasWork.final=" + this.hasWork());
                emitLevels("removeIf");
            }
        }

        private void emitLevels(String tag) {
            StringBuilder sb = new StringBuilder("graph.levels." + tag + "=");
            for (int i = 0; i < this.levels.length; i++) {
                if (i > 0) {
                    sb.append(',');
                }
                sb.append(this.levels[i]);
            }
            emit(sb.toString());
        }
    }

    public static void main(String[] args) throws Exception {
        String output = null;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--output" -> output = args[++i];
                default -> throw new IllegalArgumentException("Unknown arg: " + args[i]);
            }
        }
        if (output == null) {
            throw new IllegalArgumentException("Usage: LightingCoreProbe --output <file>");
        }
        out = new PrintWriter(Files.newBufferedWriter(Path.of(output)), true); // autoflush

        probeLeveledPriorityQueue();
        probeSpatialLongSet();
        probeDynamicGraphIncrease();
        probeDynamicGraphDecrease();
        probeDynamicGraphRemoveIf();
        probeStorageMap();

        out.close();
    }

    private static void probeLeveledPriorityQueue() {
        // The queue-level golden: enqueue a known sequence and record the pop
        // order plus the isEmpty/firstQueuedLevel transitions.
        LeveledPriorityQueue q = new LeveledPriorityQueue(6, 4);
        emit("queue.empty.initial=" + q.isEmpty());
        emit("queue.enqueue=11:2,12:0,13:2,14:3,15:1");
        q.enqueue(11L, 2);
        q.enqueue(12L, 0);
        q.enqueue(13L, 2);
        q.enqueue(14L, 3);
        q.enqueue(15L, 1);
        emit("queue.empty.after.enqueue=" + q.isEmpty());
        emit("queue.pop=" + q.removeFirstLong());
        emit("queue.pop=" + q.removeFirstLong());
        emit("queue.pop=" + q.removeFirstLong());
        // Re-prioritize 14 from level 3 to level 0 (dequeue with upperBound =
        // newPriority 0: an emptied level below must not re-claim the lead).
        emit("queue.dequeue=14,3,0");
        q.dequeue(14L, 3, 0);
        emit("queue.empty.after.dequeue.14=" + q.isEmpty());
        emit("queue.enqueue=14:0");
        q.enqueue(14L, 0);
        emit("queue.pop=" + q.removeFirstLong());
        emit("queue.pop=" + q.removeFirstLong());
        emit("queue.empty.final=" + q.isEmpty());
        // Dedup: enqueue a node already on a level keeps its FIFO position.
        LeveledPriorityQueue d = new LeveledPriorityQueue(4, 4);
        d.enqueue(1L, 1);
        d.enqueue(2L, 1);
        d.enqueue(1L, 1); // set-add: no-op, keeps original position
        emit("queue.dedup.pop=" + d.removeFirstLong());
        emit("queue.dedup.pop=" + d.removeFirstLong());
        emit("queue.dedup.empty=" + d.isEmpty());
        // Duplicate add to the lowest level does not re-claim the lead.
        LeveledPriorityQueue l = new LeveledPriorityQueue(4, 4);
        l.enqueue(5L, 2);
        l.enqueue(5L, 2);
        l.dequeue(5L, 2, 4);
        emit("queue.lowest.dupe.empty=" + l.isEmpty());
    }

    /** Paper's `InternalMap` packing, computed from real `Mth` — the bit ops are
     *  transcribed verbatim from `SpatialLongSet.java`. */
    private static final int X_BITS = Mth.log2(60000000);
    private static final int Z_BITS = Mth.log2(60000000);
    private static final int Y_BITS = 64 - X_BITS - Z_BITS;
    private static final int Y_OFFSET = 0;
    private static final int Z_OFFSET = Y_BITS;
    private static final int X_OFFSET = Y_BITS + Z_BITS;
    private static final long OUTER_MASK = 3L << X_OFFSET | 3L | 3L << Z_OFFSET;

    private static long getOuterKey(long key) {
        return key & ~OUTER_MASK;
    }

    private static int getInnerKey(long key) {
        int innerX = (int) (key >>> X_OFFSET & 3L);
        int innerY = (int) (key >>> 0 & 3L);
        int innerZ = (int) (key >>> Z_OFFSET & 3L);
        return innerX << 4 | innerZ << 2 | innerY;
    }

    private static long getFullKey(long outerKey, int innerKey) {
        outerKey |= (long) (innerKey >>> 4 & 3) << X_OFFSET;
        outerKey |= (long) (innerKey >>> 2 & 3) << Z_OFFSET;
        return outerKey | (long) (innerKey >>> 0 & 3) << 0;
    }

    private static void probeSpatialLongSet() {
        emit("spatial.xbits=" + X_BITS);
        emit("spatial.zbits=" + Z_BITS);
        emit("spatial.ybits=" + Y_BITS);
        emit("spatial.yoffset=" + Y_OFFSET);
        emit("spatial.zoffset=" + Z_OFFSET);
        emit("spatial.xoffset=" + X_OFFSET);
        emit("spatial.outerMask=" + OUTER_MASK);

        long a = (1L << 39) | (2L << 14) | 3L;
        long b = (1L << 39) | (5L << 14) | 6L; // different outer group from a
        long c = (9L << 39) | (2L << 14) | 3L; // different outer group from a
        emit("spatial.key.a=" + a);
        emit("spatial.key.b=" + b);
        emit("spatial.key.c=" + c);
        emit("spatial.outer.a=" + getOuterKey(a));
        emit("spatial.inner.a=" + getInnerKey(a));
        emit("spatial.full.a=" + getFullKey(getOuterKey(a), getInnerKey(a)));

        SpatialLongSet set = new SpatialLongSet(256, 0.5f);
        emit("spatial.empty.initial=" + set.isEmpty());
        emit("spatial.add.a=" + set.add(a));
        emit("spatial.add.b=" + set.add(b));
        emit("spatial.add.c=" + set.add(c));
        emit("spatial.add.b.dupe=" + set.add(b));
        emit("spatial.empty.after.adds=" + set.isEmpty());
        emit("spatial.pop=" + set.removeFirstLong());
        emit("spatial.pop=" + set.removeFirstLong());
        emit("spatial.pop=" + set.removeFirstLong());
        emit("spatial.empty.final=" + set.isEmpty());

        // Removal: drop the middle group, then pop order; re-add appends.
        SpatialLongSet rem = new SpatialLongSet(256, 0.5f);
        long x = (1L << 39) | 1L;
        long y = (2L << 39) | 1L;
        long z = (3L << 39) | 1L;
        rem.add(x);
        rem.add(y);
        rem.add(z);
        emit("spatial.rem.y=" + rem.rem(y));
        emit("spatial.rem.y.twice=" + rem.rem(y));
        emit("spatial.pop.after.rem=" + rem.removeFirstLong());
        emit("spatial.pop.after.rem=" + rem.removeFirstLong());
        emit("spatial.empty.after.rem=" + rem.isEmpty());
        rem.add(x);
        rem.add(z);
        rem.rem(x);
        emit("spatial.pop.after.readd=" + rem.removeFirstLong());
        emit("spatial.empty.after.readd=" + rem.isEmpty());
    }

    private static void probeDynamicGraphIncrease() {
        LineGraph g = new LineGraph(9);
        g.drive("increase");
    }

    private static void probeDynamicGraphDecrease() {
        LineGraph g = new LineGraph(9);
        g.drive("decrease");
    }

    private static void probeDynamicGraphRemoveIf() {
        LineGraph g = new LineGraph(4);
        g.drive("removeIf");
    }

    private static void probeStorageMap() {
        TestStorageMap m = new TestStorageMap();
        long key = 0x123456789L;
        DataLayer layer = new DataLayer(15);
        emit("storage.has.initial=" + m.hasLayer(key));
        emit("storage.get.initial=" + (m.getLayer(key) == null ? "null" : "nonnull"));
        m.setLayer(key, layer);
        emit("storage.has.after.set=" + m.hasLayer(key));
        emit("storage.get.after.set.filled=" + m.getLayer(key).isDefinitelyFilledWith(15));
        DataLayer copied = m.copyDataLayer(key);
        emit("storage.copyDataLayer.filled=" + copied.isDefinitelyFilledWith(15));
        // Mutate the copy through the stored reference and re-read.
        copied.fill(3);
        emit("storage.get.after.mutate.filled=" + m.getLayer(key).isDefinitelyFilledWith(3));
        try {
            m.copyDataLayer(99L);
            emit("storage.copyDataLayer.absent=no-throw");
        } catch (NullPointerException npe) {
            emit("storage.copyDataLayer.absent=throws:NPE");
        }
        m.removeLayer(key);
        emit("storage.has.after.remove=" + m.hasLayer(key));
        // copy() reference semantics: `removeLayer`/`setLayer` leave the LRU
        // cache holding the stale pre-`copyDataLayer` layer, so clear the cache
        // first (fastutil `clone()` copies the value array but shares the
        // `DataLayer` objects — getLayer through the original and the copy must
        // return the SAME object, and a fill through the copy is visible in the
        // original). The Rust port drops the cache (pure read optimization) and
        // shares the layers with `Rc<RefCell>`, reproducing these exact lines.
        m.setLayer(key, new DataLayer(0));
        m.clearCache();
        TestStorageMap c = m.copy();
        emit("storage.copy.same.reference=" + (m.getLayer(key) == c.getLayer(key)));
        c.getLayer(key).fill(9);
        emit("storage.copy.original.filled=" + m.getLayer(key).isDefinitelyFilledWith(9));
        emit("storage.copy.copied.filled=" + c.getLayer(key).isDefinitelyFilledWith(9));
    }
}
