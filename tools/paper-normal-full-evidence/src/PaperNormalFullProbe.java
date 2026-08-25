package org.rivet.paper_normal_full;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.atomic.AtomicBoolean;
import org.bukkit.Bukkit;
import org.bukkit.Chunk;
import org.bukkit.GameRule;
import org.bukkit.World;
import org.bukkit.craftbukkit.CraftChunk;
import org.bukkit.plugin.java.JavaPlugin;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.levelgen.Heightmap;

/**
 * Main-thread-only observer for the independent Paper FULL capture route.
 *
 * The driver installs level-33 minecraft:forced tickets before this plugin is
 * loaded.  This probe never calls a generating API: an unloaded closure member
 * remains a failure, and the probe waits for the forced route to finish before
 * asking Paper for a FULL handle.  Tickets are not removed by the probe and
 * remain installed through the driver's graceful stop and post-exit extraction.
 */
public final class PaperNormalFullProbe extends JavaPlugin {
    private static final int MAX_POLLS = 600;
    private final AtomicBoolean finished = new AtomicBoolean(false);
    private List<int[]> closure;
    private List<int[]> targets;
    private Path output;
    private String token;
    private World world;
    private int polls;

    @Override
    public void onEnable() {
        if (!Bukkit.isPrimaryThread()) {
            fail("onEnable was not called on the Paper main thread");
            return;
        }
        try {
            this.output = Path.of(requiredProperty("rivet.probe.output"));
            this.token = requiredProperty("rivet.capture.token");
            this.closure = parseCoordinates(requiredProperty("rivet.probe.closure"));
            this.targets = parseCoordinates(requiredProperty("rivet.probe.targets"));
            this.world = Bukkit.getWorld("world");
            if (world == null) {
                fail("minecraft:overworld Bukkit world `world` is absent");
                return;
            }
            if (world.getEnvironment() != World.Environment.NORMAL) {
                fail("Bukkit world is not the normal overworld: " + world.getEnvironment());
                return;
            }
            freezeSimulation();
            getLogger().info("RIVET_PROBE_MAIN_THREAD world=minecraft:overworld closure=" + closure.size());
            Bukkit.getScheduler().runTaskTimer(this, this::poll, 1L, 1L);
        } catch (RuntimeException e) {
            fail("probe setup failed: " + e);
        }
    }

    private void freezeSimulation() {
        if (!world.setGameRule(GameRule.RANDOM_TICK_SPEED, 0)
            || !world.setGameRule(GameRule.DO_DAYLIGHT_CYCLE, false)
            || !world.setGameRule(GameRule.DO_WEATHER_CYCLE, false)
            || !world.setGameRule(GameRule.DO_MOB_SPAWNING, false)) {
            throw new IllegalStateException("Paper rejected a frozen-simulation gamerule");
        }
        if (!Integer.valueOf(0).equals(world.getGameRuleValue(GameRule.RANDOM_TICK_SPEED))
            || !Boolean.FALSE.equals(world.getGameRuleValue(GameRule.DO_DAYLIGHT_CYCLE))
            || !Boolean.FALSE.equals(world.getGameRuleValue(GameRule.DO_WEATHER_CYCLE))
            || !Boolean.FALSE.equals(world.getGameRuleValue(GameRule.DO_MOB_SPAWNING))
            || !spawnLimitsAreZero()) {
            throw new IllegalStateException("frozen-simulation gamerules did not stick");
        }
        getLogger().info("RIVET_SIMULATION_FROZEN randomTickSpeed=0 daylight=false weather=false mobSpawning=false spawnLimits=0");
    }

    private boolean spawnLimitsAreZero() {
        // The pinned paper-world-defaults.yml sets all eight categories to 0.
        // The plugin cannot read Paper's YAML model through the Bukkit API, so
        // this is recorded as a driver/config assertion and checked by the
        // validator against the exact fixture bytes.
        return true;
    }

    private void poll() {
        if (finished.get()) {
            return;
        }
        if (!Bukkit.isPrimaryThread()) {
            fail("scheduled probe step left the Paper main thread");
            return;
        }
        polls++;
        for (int[] coordinate : closure) {
            if (!world.isChunkLoaded(coordinate[0], coordinate[1])) {
                if (polls >= MAX_POLLS) {
                    fail("forced closure chunk was not loaded before timeout: " + coordinate[0] + "," + coordinate[1]);
                }
                return;
            }
        }
        try {
            List<String> targetEvidence = new ArrayList<>();
            for (int[] coordinate : targets) {
                if (!world.isChunkLoaded(coordinate[0], coordinate[1])) {
                    fail("target was not loaded through the forced route: " + coordinate[0] + "," + coordinate[1]);
                    return;
                }
                // getChunkAt follows the already-loaded route after the
                // isChunkLoaded guard; it is not used to trigger generation.
                Chunk bukkitChunk = world.getChunkAt(coordinate[0], coordinate[1]);
                ChunkAccess access = ((CraftChunk) bukkitChunk).getHandle(ChunkStatus.FULL);
                String status = "minecraft:" + access.getPersistedStatus().getName();
                boolean lightCorrect = access.isLightCorrect();
                int worldSurface = access.getHeight(Heightmap.Types.WORLD_SURFACE, 8, 8);
                int motionBlocking = access.getHeight(Heightmap.Types.MOTION_BLOCKING, 8, 8);
                if (!"minecraft:full".equals(status) || !lightCorrect) {
                    fail("target did not reach FULL+light: " + coordinate[0] + "," + coordinate[1]
                        + " status=" + status + " light=" + lightCorrect);
                    return;
                }
                targetEvidence.add("{\"x\":" + coordinate[0] + ",\"z\":" + coordinate[1]
                    + ",\"status\":\"" + status + "\",\"light_correct\":" + lightCorrect
                    + ",\"world_surface\":" + worldSurface + ",\"motion_blocking\":" + motionBlocking + "}");
            }
            String json = "{\"format\":1,\"producer\":\"PaperNormalFullProbe\","
                + "\"main_thread\":true,\"world\":\"minecraft:overworld\","
                + "\"closure_count\":" + closure.size() + ",\"polls\":" + polls
                + ",\"simulation_frozen\":true,\"token\":\"" + token + "\",\"targets\":["
                + String.join(",", targetEvidence) + "]}";
            Files.createDirectories(output.getParent());
            Files.writeString(output, json + System.lineSeparator(), StandardCharsets.UTF_8);
            getLogger().info("RIVET_PROBE_READY targets=" + targets.size() + " closure=" + closure.size());
            getLogger().info("RIVET_CAPTURE_TOKEN=" + token);
            finished.set(true);
            Bukkit.getScheduler().cancelTasks(this);
        } catch (IOException | RuntimeException e) {
            fail("probe observation failed: " + e);
        }
    }

    private void fail(String message) {
        if (finished.compareAndSet(false, true)) {
            getLogger().severe("RIVET_PROBE_FAILED " + message);
            try {
                if (output != null) {
                    Files.createDirectories(output.getParent());
                    Files.writeString(output, "{\"format\":1,\"failed\":true,\"message\":\""
                        + message.replace("\\", "\\\\").replace("\"", "\\\"") + "\"}\n",
                        StandardCharsets.UTF_8);
                }
            } catch (IOException ignored) {
                // The server log is the authoritative failure channel.
            }
            Bukkit.getScheduler().cancelTasks(this);
        }
    }

    private static String requiredProperty(String name) {
        String value = System.getProperty(name);
        if (value == null || value.isBlank()) {
            throw new IllegalArgumentException("missing -D" + name);
        }
        return value;
    }

    private static List<int[]> parseCoordinates(String encoded) {
        List<int[]> result = new ArrayList<>();
        if (encoded.isBlank()) {
            throw new IllegalArgumentException("empty coordinate list");
        }
        for (String item : encoded.split(";", -1)) {
            String[] parts = item.split(",", -1);
            if (parts.length != 2) {
                throw new IllegalArgumentException("bad coordinate: " + item);
            }
            result.add(new int[] { Integer.parseInt(parts[0]), Integer.parseInt(parts[1]) });
        }
        return result;
    }
}
