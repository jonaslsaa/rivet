package rivet.arena;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom;
import it.unimi.dsi.fastutil.objects.ObjectLinkedOpenHashSet;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.DoubleTag;
import net.minecraft.nbt.FloatTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.StringTag;
import net.minecraft.nbt.Tag;
import net.minecraft.nbt.TagParser;
import net.minecraft.resources.Identifier;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.util.ProblemReporter;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.level.BlockEventData;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.levelgen.RandomSupport;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.material.Fluid;
import net.minecraft.world.level.storage.TagValueInput;
import net.minecraft.world.level.storage.TagValueOutput;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.ticks.LevelChunkTicks;
import net.minecraft.world.ticks.LevelTicks;
import net.minecraft.world.ticks.ScheduledTick;
import net.minecraft.world.ticks.TickPriority;
import org.bukkit.Bukkit;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * L4 arena tick-trace probe (docs/VERIFICATION.md section 2.2, item 7).
 *
 * Runs inside a real, freshly booted Paper 26.2 dedicated server (superflat,
 * pinned configs, no players). Every heartbeat of the Paper main thread happens
 * before the server's level ticks (MinecraftServer.tickChildren), so the
 * snapshot taken at heartbeat t is the state after exactly t calls of the real
 * ServerLevel.tick since the arena was placed. Nothing here re-implements game
 * logic: placement and script operations are the production Level.setBlock and
 * LevelTicks.schedule paths, and the trace reads production state.
 *
 * Timeline per arena (the Rust runner uses the same one):
 *   t = 0: reseed the level RNG, install a fresh seeded entity RNG, quietly
 *          place the arena, apply its block entity NBT, run script ops with
 *          tick 0, snapshot 0.
 *   t >= 1: (one ServerLevel.tick has run) run script ops with tick t,
 *          snapshot t.
 * The trace schema is documented in tools/rivet-oracle/arenas/README.md.
 */
public final class ArenaTraceProbe extends JavaPlugin implements Listener {
    static final String SCHEMA = "rivet.arena-trace/1";
    /** UPDATE_KNOWN_SHAPE | UPDATE_SUPPRESS_DROPS | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS | UPDATE_SKIP_ON_PLACE. */
    static final int QUIET = Block.UPDATE_KNOWN_SHAPE | Block.UPDATE_SUPPRESS_DROPS
        | Block.UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS | Block.UPDATE_SKIP_ON_PLACE;
    /** Seed-injection sites (tools/rivet-oracle/arenas/seed-sites.tsv). */
    static final long SITE_LEVEL_RANDOM = 1;
    static final long SITE_ENTITY_RANDOM = 3;
    static final int MAX_WAIT_TICKS = 400;

    /** serializeNulls: schema members whose value is null (`health`) are written, not dropped. */
    static final Gson GSON = new GsonBuilder().disableHtmlEscaping().serializeNulls().create();
    static final Comparator<BlockPos> POS_ORDER = Comparator.<BlockPos>comparingInt(BlockPos::getY)
        .thenComparingInt(BlockPos::getZ).thenComparingInt(BlockPos::getX);

    private ServerLevel level;
    private Path arenaDir;
    private Path outDir;
    private final List<String> names = new ArrayList<>();
    private final Map<String, Set<String>> exclusions = new HashMap<>();
    private int arenaIndex = -1;
    private Arena arena;
    private boolean finished;

    // ---------------------------------------------------------------- arena

    static final class Arena {
        final String name;
        final String sha256;
        final JsonObject spec;
        final BlockPos origin;
        final int sx, sy, sz;
        final int ticks;
        final long seed;
        final List<String> optIn = new ArrayList<>();
        final List<Long> chunks = new ArrayList<>();
        final List<String> lines = new ArrayList<>();
        final Map<BlockPos, Integer> blocks = new HashMap<>();
        final Map<BlockPos, String> blockEntities = new HashMap<>();
        final Map<UUID, Integer> entityNumbers = new HashMap<>();
        final List<JsonArray> blockEvents = new ArrayList<>();
        final List<JsonObject> script = new ArrayList<>();
        ArenaEntityRandom entityRandom;
        String violation;
        int waited;
        int t = -1;

        Arena(String name, byte[] bytes) throws Exception {
            this.name = name;
            this.sha256 = hex(MessageDigest.getInstance("SHA-256").digest(bytes));
            this.spec = JsonParser.parseString(new String(bytes, StandardCharsets.UTF_8)).getAsJsonObject();
            if (!name.equals(spec.get("name").getAsString())) {
                throw new IllegalStateException("arena file " + name + " declares name " + spec.get("name"));
            }
            this.origin = pos(spec.getAsJsonArray("origin"));
            JsonArray size = spec.getAsJsonArray("size");
            this.sx = size.get(0).getAsInt();
            this.sy = size.get(1).getAsInt();
            this.sz = size.get(2).getAsInt();
            this.ticks = spec.get("ticks").getAsInt();
            this.seed = spec.get("seed").getAsLong();
            if (spec.has("opt_in")) {
                for (JsonElement e : spec.getAsJsonArray("opt_in")) {
                    optIn.add(e.getAsString());
                }
            }
            for (int cx = (origin.getX() - 1) >> 4; cx <= (origin.getX() + sx) >> 4; cx++) {
                for (int cz = (origin.getZ() - 1) >> 4; cz <= (origin.getZ() + sz) >> 4; cz++) {
                    chunks.add(ChunkPos.pack(cx, cz));
                }
            }
        }

        boolean contains(BlockPos p) {
            int dx = p.getX() - origin.getX(), dy = p.getY() - origin.getY(), dz = p.getZ() - origin.getZ();
            return dx >= 0 && dy >= 0 && dz >= 0 && dx < sx && dy < sy && dz < sz;
        }

        BoundingBox box() {
            return new BoundingBox(origin.getX(), origin.getY(), origin.getZ(),
                origin.getX() + sx - 1, origin.getY() + sy - 1, origin.getZ() + sz - 1);
        }

        AABB aabb() {
            return new AABB(origin.getX(), origin.getY(), origin.getZ(),
                origin.getX() + sx, origin.getY() + sy, origin.getZ() + sz);
        }

        JsonArray rel(BlockPos p) {
            return ints(p.getX() - origin.getX(), p.getY() - origin.getY(), p.getZ() - origin.getZ());
        }

        BlockPos abs(JsonArray rel) {
            return origin.offset(rel.get(0).getAsInt(), rel.get(1).getAsInt(), rel.get(2).getAsInt());
        }
    }

    // --------------------------------------------------------------- plugin

    @Override
    public void onEnable() {
        try {
            if (!Bukkit.isPrimaryThread()) {
                throw new IllegalStateException("onEnable off the Paper main thread");
            }
            this.arenaDir = Path.of(required("rivet.arena.dir"));
            this.outDir = Path.of(required("rivet.arena.out"));
            for (String n : required("rivet.arena.names").split(",")) {
                if (!n.isBlank()) {
                    names.add(n.trim());
                }
            }
            if (names.isEmpty()) {
                throw new IllegalStateException("rivet.arena.names is empty");
            }
            loadExclusions(arenaDir.resolve("exclusions.tsv"));
            org.bukkit.World world = Bukkit.getWorld("world");
            if (world == null) {
                throw new IllegalStateException("world `world` is absent");
            }
            this.level = ((org.bukkit.craftbukkit.CraftWorld) world).getHandle();
            freeze();
            instrumentBlockEvents();
            Bukkit.getPluginManager().registerEvents(this, this);
            Bukkit.getScheduler().runTaskTimer(this, this::heartbeat, 1L, 1L);
            getLogger().info("RIVET_ARENA_PROBE_READY arenas=" + names.size());
        } catch (Throwable error) {
            fail(error);
        }
    }

    private static String required(String key) {
        String value = System.getProperty(key);
        if (value == null || value.isBlank()) {
            throw new IllegalStateException("missing system property " + key);
        }
        return value;
    }

    private void loadExclusions(Path path) throws IOException {
        for (String line : Files.readAllLines(path, StandardCharsets.UTF_8)) {
            if (line.isBlank() || line.startsWith("#") || line.startsWith("scope\t")) {
                continue;
            }
            String[] cells = line.split("\t");
            exclusions.computeIfAbsent(cells[0], k -> new HashSet<>()).add(cells[1]);
        }
    }

    /** Random ticks, spawning, weather and daylight off (VERIFICATION 2.5). */
    private void freeze() {
        GameRules rules = level.getGameRules();
        rules.set(GameRules.RANDOM_TICK_SPEED, 0, level);
        rules.set(GameRules.ADVANCE_TIME, false, level);
        rules.set(GameRules.ADVANCE_WEATHER, false, level);
        rules.set(GameRules.SPAWN_MOBS, false, level);
        level.getWorld().setStorm(false);
        level.getWorld().setThundering(false);
        if (rules.get(GameRules.RANDOM_TICK_SPEED) != 0 || rules.get(GameRules.ADVANCE_TIME)
            || rules.get(GameRules.ADVANCE_WEATHER) || rules.get(GameRules.SPAWN_MOBS)) {
            throw new IllegalStateException("frozen-simulation gamerules did not stick");
        }
        getLogger().info("RIVET_ARENA_FROZEN randomTickSpeed=0 advanceTime=false advanceWeather=false spawnMobs=false");
    }

    /**
     * ServerLevel.blockEvent appends to the private final blockEvents set and
     * runBlockEvents drains it within the same tick, so the queue is always
     * empty at a heartbeat. The set is replaced by a recording subclass that
     * keeps the production add/drain behaviour and logs every insertion that
     * targets the active arena box, in insertion order.
     */
    private void instrumentBlockEvents() throws Exception {
        var field = ServerLevel.class.getDeclaredField("blockEvents");
        var unsafeField = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
        unsafeField.setAccessible(true);
        var unsafe = (sun.misc.Unsafe) unsafeField.get(null);
        @SuppressWarnings("unchecked")
        var existing = (ObjectLinkedOpenHashSet<BlockEventData>) unsafe.getObject(level, unsafe.objectFieldOffset(field));
        var recording = new ObjectLinkedOpenHashSet<BlockEventData>() {
            @Override
            public boolean add(BlockEventData data) {
                boolean added = super.add(data);
                Arena active = arena;
                if (active != null && active.t >= 0 && active.contains(data.pos())) {
                    JsonArray row = active.rel(data.pos());
                    row.add(BuiltInRegistries.BLOCK.getKey(data.block()).toString());
                    row.add(data.paramA());
                    row.add(data.paramB());
                    row.add(added);
                    active.blockEvents.add(row);
                }
                return added;
            }
        };
        recording.addAll(existing);
        unsafe.putObject(level, unsafe.objectFieldOffset(field), recording);
    }

    /**
     * Seed-injection site 3. Paper 26.2 gives every entity the one static
     * Entity.SHARED_RANDOM (`Entity.random` is a reference to it, captured at
     * construction), and that source honours only the first setSeed per JVM.
     * Reseeding it per entity therefore cannot work. Instead each arena installs
     * a fresh seeded source as Entity.SHARED_RANDOM before placement, so every
     * entity constructed while the arena runs shares it, in Paper's own draw
     * order. setSeed on it is not modelled and fails the probe.
     */
    static final class ArenaEntityRandom extends ThreadUnsafeRandom {
        private boolean sealed;

        ArenaEntityRandom(long seed) {
            super(seed);
            this.sealed = true;
        }

        @Override
        public synchronized void setSeed(long seed) {
            if (sealed) {
                throw new IllegalStateException("setSeed on the arena entity RNG (Entity.SHARED_RANDOM)");
            }
            super.setSeed(seed);
        }
    }

    /** Every entity near the arena must draw from the arena's entity RNG (site 3). */
    @EventHandler
    public void onEntityAdd(com.destroystokyo.paper.event.entity.EntityAddToWorldEvent event) {
        Arena active = arena;
        if (active == null || active.t < 0) {
            return;
        }
        Entity entity = ((org.bukkit.craftbukkit.entity.CraftEntity) event.getEntity()).getHandle();
        if (entity.level() == level && active.aabb().inflate(16).contains(entity.position())
            && entity.getRandom() != active.entityRandom && active.violation == null) {
            active.violation = active.name + ": " + BuiltInRegistries.ENTITY_TYPE.getKey(entity.getType())
                + " was not constructed with the arena entity RNG (seed site 3)";
        }
    }

    static long mix(long seed, long site, long ordinal) {
        return RandomSupport.mixStafford13(seed + site * 0x9E3779B97F4A7C15L + ordinal);
    }

    private void heartbeat() {
        if (finished) {
            return;
        }
        try {
            if (arena == null) {
                if (++arenaIndex >= names.size()) {
                    finish();
                    return;
                }
                String name = names.get(arenaIndex);
                arena = new Arena(name, Files.readAllBytes(arenaDir.resolve(name + ".json")));
                for (long chunk : arena.chunks) {
                    level.getWorld().setChunkForceLoaded(ChunkPos.getX(chunk), ChunkPos.getZ(chunk), true);
                }
            }
            if (arena.t < 0) {
                for (long chunk : arena.chunks) {
                    if (!level.shouldTickBlocksAt(chunk) || !level.areEntitiesLoaded(chunk)) {
                        if (++arena.waited > MAX_WAIT_TICKS) {
                            throw new IllegalStateException("arena chunks never became ticking: " + arena.name);
                        }
                        return;
                    }
                }
                start(arena);
            } else {
                arena.t++;
                runScript(arena, arena.t);
                snapshot(arena);
            }
            if (arena.violation != null) {
                throw new IllegalStateException(arena.violation);
            }
            if (arena.t == arena.ticks) {
                complete(arena);
                arena = null;
            }
        } catch (Throwable error) {
            fail(error);
        }
    }

    // ------------------------------------------------------------ placement

    private void start(Arena a) throws Exception {
        for (int dy = 0; dy < a.sy; dy++) {
            for (int dz = 0; dz < a.sz; dz++) {
                for (int dx = 0; dx < a.sx; dx++) {
                    BlockPos p = a.origin.offset(dx, dy, dz);
                    if (!level.getBlockState(p).isAir()) {
                        throw new IllegalStateException(a.name + ": arena box is not empty at " + p);
                    }
                }
            }
        }
        if (!level.getEntities((Entity) null, a.aabb().inflate(16), e -> true).isEmpty()) {
            throw new IllegalStateException(a.name + ": entities near the arena before placement");
        }
        for (String opt : a.optIn) {
            throw new IllegalStateException(a.name + ": opt-in `" + opt + "` is not supported by this probe version");
        }
        // Seed-injection site 1, before anything in the arena can draw from it.
        level.getRandom().setSeed(mix(a.seed, SITE_LEVEL_RANDOM, 0));
        // Seed-injection site 3: entities constructed from here on capture it.
        a.entityRandom = new ArenaEntityRandom(mix(a.seed, SITE_ENTITY_RANDOM, 0));
        Entity.SHARED_RANDOM = a.entityRandom;
        a.t = 0;

        JsonArray placements = new JsonArray();
        for (JsonElement element : a.spec.getAsJsonArray("fill")) {
            JsonObject fill = element.getAsJsonObject();
            BlockState state = parseState(fill.get("state").getAsString());
            JsonArray from = fill.getAsJsonArray("from");
            JsonArray to = fill.has("to") ? fill.getAsJsonArray("to") : from;
            for (int y = from.get(1).getAsInt(); y <= to.get(1).getAsInt(); y++) {
                for (int z = from.get(2).getAsInt(); z <= to.get(2).getAsInt(); z++) {
                    for (int x = from.get(0).getAsInt(); x <= to.get(0).getAsInt(); x++) {
                        BlockPos p = a.origin.offset(x, y, z);
                        if (!a.contains(p)) {
                            throw new IllegalStateException(a.name + ": fill outside the arena box at " + p);
                        }
                        level.setBlock(p, state, QUIET);
                        if (level.getBlockState(p) != state) {
                            throw new IllegalStateException(a.name + ": quiet placement rejected at " + p);
                        }
                        JsonArray row = a.rel(p);
                        row.add(Block.getId(state));
                        placements.add(row);
                    }
                }
            }
        }
        JsonArray blockEntities = new JsonArray();
        if (a.spec.has("block_entities")) {
            for (JsonElement element : a.spec.getAsJsonArray("block_entities")) {
                JsonObject spec = element.getAsJsonObject();
                BlockPos p = a.abs(spec.getAsJsonArray("pos"));
                BlockEntity entity = level.getBlockEntity(p);
                if (entity == null) {
                    throw new IllegalStateException(a.name + ": no block entity at " + p);
                }
                CompoundTag tag = TagParser.parseCompoundFully(spec.get("snbt").getAsString());
                entity.loadWithComponents(TagValueInput.create(ProblemReporter.DISCARDING, level.registryAccess(), tag));
                entity.setChanged();
                JsonArray row = a.rel(p);
                row.add(entity.saveWithFullMetadata(level.registryAccess()).toString());
                blockEntities.add(row);
            }
        }
        if (a.spec.has("script")) {
            for (JsonElement element : a.spec.getAsJsonArray("script")) {
                a.script.add(resolve(a, element.getAsJsonObject()));
            }
        }

        JsonObject header = new JsonObject();
        header.addProperty("kind", "header");
        header.addProperty("schema", SCHEMA);
        header.addProperty("arena", a.name);
        header.addProperty("arena_sha256", a.sha256);
        header.add("origin", ints(a.origin.getX(), a.origin.getY(), a.origin.getZ()));
        header.add("size", ints(a.sx, a.sy, a.sz));
        header.addProperty("ticks", a.ticks);
        header.addProperty("seed", Long.toString(a.seed));
        header.add("opt_in", GSON.toJsonTree(a.optIn));
        header.add("placements", placements);
        header.add("block_entities", blockEntities);
        JsonArray script = new JsonArray();
        a.script.forEach(script::add);
        header.add("script", script);
        a.lines.add(GSON.toJson(header));

        runScript(a, 0);
        snapshot(a);
    }

    static BlockState parseState(String text) throws Exception {
        return BlockStateParser.parseForBlock(BuiltInRegistries.BLOCK, text, false).blockState();
    }

    /** Script ops resolved to registry ids; the header carries this form. */
    private JsonObject resolve(Arena a, JsonObject op) throws Exception {
        JsonObject out = new JsonObject();
        out.addProperty("tick", op.get("tick").getAsInt());
        String kind = op.get("op").getAsString();
        out.addProperty("op", kind);
        JsonArray rel = op.getAsJsonArray("pos");
        if (!a.contains(a.abs(rel))) {
            throw new IllegalStateException(a.name + ": script op outside the arena box: " + op);
        }
        out.add("pos", rel);
        switch (kind) {
            case "set" -> {
                out.addProperty("state", Block.getId(parseState(op.get("state").getAsString())));
                out.addProperty("flags", op.get("flags").getAsInt());
            }
            case "schedule_block", "schedule_fluid" -> {
                String id = op.get(kind.equals("schedule_block") ? "block" : "fluid").getAsString();
                Identifier key = Identifier.parse(id);
                if (kind.equals("schedule_block") ? !BuiltInRegistries.BLOCK.containsKey(key) : !BuiltInRegistries.FLUID.containsKey(key)) {
                    throw new IllegalStateException(a.name + ": unknown " + kind + " type " + id);
                }
                out.addProperty(kind.equals("schedule_block") ? "block" : "fluid", key.toString());
                out.addProperty("delay", op.get("delay").getAsInt());
                out.addProperty("priority", op.has("priority") ? op.get("priority").getAsInt() : 0);
            }
            default -> throw new IllegalStateException(a.name + ": unknown script op " + kind);
        }
        return out;
    }

    private void runScript(Arena a, int tick) {
        for (JsonObject op : a.script) {
            if (op.get("tick").getAsInt() != tick) {
                continue;
            }
            BlockPos p = a.abs(op.getAsJsonArray("pos"));
            switch (op.get("op").getAsString()) {
                case "set" -> level.setBlock(p, Block.stateById(op.get("state").getAsInt()), op.get("flags").getAsInt());
                case "schedule_block" -> {
                    Block block = BuiltInRegistries.BLOCK.getValue(Identifier.parse(op.get("block").getAsString()));
                    level.scheduleTick(p, block, op.get("delay").getAsInt(), TickPriority.byValue(op.get("priority").getAsInt()));
                }
                case "schedule_fluid" -> {
                    Fluid fluid = BuiltInRegistries.FLUID.getValue(Identifier.parse(op.get("fluid").getAsString()));
                    level.scheduleTick(p, fluid, op.get("delay").getAsInt(), TickPriority.byValue(op.get("priority").getAsInt()));
                }
                default -> throw new IllegalStateException("unreachable");
            }
        }
    }

    // ------------------------------------------------------------- snapshot

    private void snapshot(Arena a) throws Exception {
        JsonObject line = new JsonObject();
        line.addProperty("kind", "tick");
        line.addProperty("t", a.t);

        JsonArray blocks = new JsonArray();
        for (int dy = 0; dy < a.sy; dy++) {
            for (int dz = 0; dz < a.sz; dz++) {
                for (int dx = 0; dx < a.sx; dx++) {
                    BlockPos p = a.origin.offset(dx, dy, dz);
                    int id = Block.getId(level.getBlockState(p));
                    if (a.blocks.getOrDefault(p, 0) != id) {
                        a.blocks.put(p, id);
                        JsonArray row = ints(dx, dy, dz);
                        row.add(id);
                        blocks.add(row);
                    }
                }
            }
        }
        line.add("blocks", blocks);
        line.add("block_ticks", ticks(a, level.getBlockTicks(), b -> BuiltInRegistries.BLOCK.getKey(b).toString()));
        line.add("fluid_ticks", ticks(a, level.getFluidTicks(), f -> BuiltInRegistries.FLUID.getKey(f).toString()));

        JsonArray events = new JsonArray();
        a.blockEvents.forEach(events::add);
        a.blockEvents.clear();
        line.add("block_events", events);

        line.add("block_entities", blockEntities(a));
        line.add("entities", entities(a));
        a.lines.add(GSON.toJson(line));
    }

    private <T> JsonArray ticks(Arena a, LevelTicks<T> ticks, java.util.function.Function<T, String> name) throws Exception {
        var field = LevelTicks.class.getDeclaredField("allContainers");
        field.setAccessible(true);
        @SuppressWarnings("unchecked")
        var containers = (it.unimi.dsi.fastutil.longs.Long2ObjectMap<LevelChunkTicks<T>>) field.get(ticks);
        List<ScheduledTick<T>> inBox = new ArrayList<>();
        for (long chunk : a.chunks) {
            LevelChunkTicks<T> container = containers.get(chunk);
            if (container != null) {
                container.getAll().filter(t -> a.contains(t.pos())).forEach(inBox::add);
            }
        }
        inBox.sort(ScheduledTick.DRAIN_ORDER);
        long now = level.getGameTime();
        JsonArray rows = new JsonArray();
        for (ScheduledTick<T> t : inBox) {
            JsonArray row = a.rel(t.pos());
            row.add(name.apply(t.type()));
            row.add(t.triggerTick() - now);
            row.add(t.priority().getValue());
            rows.add(row);
        }
        return rows;
    }

    private JsonArray blockEntities(Arena a) {
        Map<BlockPos, String> now = new LinkedHashMap<>();
        List<BlockPos> positions = new ArrayList<>();
        for (long chunk : a.chunks) {
            LevelChunk c = level.getChunkSource().getChunkNow(ChunkPos.getX(chunk), ChunkPos.getZ(chunk));
            if (c == null) {
                throw new IllegalStateException(a.name + ": arena chunk unloaded");
            }
            for (BlockPos p : c.getBlockEntities().keySet()) {
                if (a.contains(p)) {
                    positions.add(p);
                }
            }
        }
        positions.sort(POS_ORDER);
        for (BlockPos p : positions) {
            CompoundTag tag = level.getBlockEntity(p).saveWithFullMetadata(level.registryAccess());
            for (String key : exclusions.getOrDefault("block_entity_nbt", Set.of())) {
                tag.remove(key);
            }
            now.put(p, normalizeFloats(tag).toString());
        }
        JsonArray rows = new JsonArray();
        List<BlockPos> changed = new ArrayList<>();
        for (var e : now.entrySet()) {
            if (!e.getValue().equals(a.blockEntities.get(e.getKey()))) {
                changed.add(e.getKey());
            }
        }
        for (BlockPos p : a.blockEntities.keySet()) {
            if (!now.containsKey(p)) {
                changed.add(p);
            }
        }
        changed.sort(POS_ORDER);
        for (BlockPos p : changed) {
            JsonArray row = a.rel(p);
            String snbt = now.get(p);
            if (snbt == null) {
                row.add(JsonNull.INSTANCE);
                a.blockEntities.remove(p);
            } else {
                row.add(snbt);
                a.blockEntities.put(p, snbt);
            }
            rows.add(row);
        }
        return rows;
    }

    private JsonArray entities(Arena a) {
        List<Entity> found = level.getEntities((Entity) null, a.aabb(), e -> true);
        List<JsonObject> rows = new ArrayList<>();
        for (Entity e : found) {
            Integer n = a.entityNumbers.get(e.getUUID());
            if (n == null) {
                n = a.entityNumbers.size() + 1;
                a.entityNumbers.put(e.getUUID(), n);
            }
            JsonObject row = new JsonObject();
            row.addProperty("n", n);
            row.addProperty("type", BuiltInRegistries.ENTITY_TYPE.getKey(e.getType()).toString());
            row.add("pos", doubles(e.getX(), e.getY(), e.getZ()));
            var delta = e.getDeltaMovement();
            row.add("delta", doubles(delta.x, delta.y, delta.z));
            if (e instanceof LivingEntity living) {
                row.addProperty("health", String.format("0x%08x", Float.floatToIntBits(living.getHealth())));
            } else {
                row.add("health", JsonNull.INSTANCE);
            }
            TagValueOutput output = TagValueOutput.createWithContext(ProblemReporter.DISCARDING, level.registryAccess());
            e.saveWithoutId(output);
            CompoundTag tag = output.buildResult();
            tag.remove("UUID"); // replaced by the first-appearance number `n`
            for (String key : exclusions.getOrDefault("entity_nbt", Set.of())) {
                tag.remove(key);
            }
            row.addProperty("nbt", normalizeFloats(tag).toString());
            rows.add(row);
        }
        rows.sort(Comparator.comparingInt(r -> r.get("n").getAsInt()));
        JsonArray out = new JsonArray();
        rows.forEach(out::add);
        return out;
    }

    // ------------------------------------------------------------- teardown

    private void complete(Arena a) throws Exception {
        JsonObject end = new JsonObject();
        end.addProperty("kind", "end");
        end.addProperty("ticks", a.ticks);
        a.lines.add(GSON.toJson(end));
        Files.createDirectories(outDir);
        Files.writeString(outDir.resolve(a.name + ".trace.jsonl"), String.join("\n", a.lines) + "\n", StandardCharsets.UTF_8);

        for (Entity e : level.getEntities((Entity) null, a.aabb().inflate(16), x -> true)) {
            e.discard();
        }
        for (int dy = a.sy - 1; dy >= 0; dy--) {
            for (int dz = 0; dz < a.sz; dz++) {
                for (int dx = 0; dx < a.sx; dx++) {
                    level.setBlock(a.origin.offset(dx, dy, dz), net.minecraft.world.level.block.Blocks.AIR.defaultBlockState(), QUIET);
                }
            }
        }
        level.getBlockTicks().clearArea(a.box());
        level.getFluidTicks().clearArea(a.box());
        for (long chunk : a.chunks) {
            level.getWorld().setChunkForceLoaded(ChunkPos.getX(chunk), ChunkPos.getZ(chunk), false);
        }
        getLogger().info("RIVET_ARENA_DONE " + a.name + " ticks=" + a.ticks);
    }

    private void finish() throws IOException {
        finished = true;
        Files.createDirectories(outDir);
        Files.writeString(outDir.resolve("DONE"), String.join("\n", names) + "\n", StandardCharsets.UTF_8);
        getLogger().info("RIVET_ARENA_ALL_DONE");
        Bukkit.shutdown();
    }

    private void fail(Throwable error) {
        finished = true;
        getLogger().severe("RIVET_ARENA_FAILED " + error);
        error.printStackTrace();
        try {
            Files.createDirectories(outDir);
            Files.writeString(outDir.resolve("FAILED"), String.valueOf(error) + "\n", StandardCharsets.UTF_8);
        } catch (Exception ignored) {
            // The missing DONE marker still fails the driver.
        }
        Bukkit.shutdown();
    }

    // -------------------------------------------------------------- helpers

    static BlockPos pos(JsonArray a) {
        return new BlockPos(a.get(0).getAsInt(), a.get(1).getAsInt(), a.get(2).getAsInt());
    }

    static JsonArray ints(int... values) {
        JsonArray out = new JsonArray();
        for (int v : values) {
            out.add(v);
        }
        return out;
    }

    /** Doubles as raw IEEE-754 bits (Double.doubleToLongBits canonicalizes NaN, D14). */
    static JsonArray doubles(double... values) {
        JsonArray out = new JsonArray();
        for (double v : values) {
            out.add(String.format("0x%016x", Double.doubleToLongBits(v)));
        }
        return out;
    }

    /**
     * NBT floats and doubles as IEEE-754 bits (VERIFICATION 2.4), so parity
     * never rests on decimal float formatting: a FloatTag becomes the string
     * `#f:0x<floatToIntBits>`, a DoubleTag `#d:0x<doubleToLongBits>` (NaN
     * canonicalized, D14). A real string starting with `#` gains one more `#`,
     * which keeps the mapping injective. Mutates and returns `tag`.
     */
    static Tag normalizeFloats(Tag tag) {
        if (tag instanceof FloatTag f) {
            return StringTag.valueOf(String.format("#f:0x%08x", Float.floatToIntBits(f.value())));
        }
        if (tag instanceof DoubleTag d) {
            return StringTag.valueOf(String.format("#d:0x%016x", Double.doubleToLongBits(d.value())));
        }
        if (tag instanceof StringTag s) {
            return s.value().startsWith("#") ? StringTag.valueOf("#" + s.value()) : s;
        }
        if (tag instanceof CompoundTag c) {
            for (String key : List.copyOf(c.keySet())) {
                c.put(key, normalizeFloats(c.get(key)));
            }
        } else if (tag instanceof ListTag l) {
            for (int i = 0; i < l.size(); i++) {
                l.set(i, normalizeFloats(l.get(i)));
            }
        }
        return tag;
    }

    static String hex(byte[] bytes) {
        StringBuilder out = new StringBuilder();
        for (byte b : bytes) {
            out.append(String.format("%02x", b));
        }
        return out.toString();
    }
}
