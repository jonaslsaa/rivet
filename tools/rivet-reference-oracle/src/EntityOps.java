package dev.rivet.oracle;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.mojang.authlib.GameProfile;
import java.lang.reflect.Field;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.List;
import java.util.TreeSet;
import java.util.UUID;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.protocol.Packet;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.network.ServerGamePacketListenerImpl;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundSource;
import net.minecraft.world.Difficulty;
import net.minecraft.world.damagesource.CombatRules;
import net.minecraft.world.damagesource.DamageScaling;
import net.minecraft.world.damagesource.DamageSource;
import net.minecraft.world.damagesource.DamageType;
import net.minecraft.world.effect.MobEffect;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.monster.zombie.Zombie;
import net.minecraft.world.entity.player.Abilities;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.food.FoodData;
import net.minecraft.world.food.FoodProperties;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.phys.Vec3;
import org.bukkit.craftbukkit.entity.CraftPlayer;

/**
 * The pure entity-verification ops (VERIFICATION item 9): {@code damage.calc},
 * {@code damage_source.scaling}, {@code food.exhaustion}, {@code xp.curves}.
 * They live beside {@link RivetReferenceOracle} so the shared registry file only
 * carries the one-line registration; the op source pin in the recording manifest
 * covers this file too.
 *
 * <h2>Floating-point contract</h2>
 * Every float travels as {@code 0x} + 8 lowercase hex digits of
 * {@link Float#floatToIntBits} in requests and results (D14: NaN canonical), so
 * both sides compare IEEE-754 bits.
 *
 * <h2>Live-entity harness contract</h2>
 * Surfaces that need a live entity run the real Paper method on an entity that
 * was allocated without its constructor (the {@code sun.misc.Unsafe} technique
 * the arena and tracking probes already use). The harness supplies only what
 * the method under test reads; the contract the Rust side reproduces is:
 * <ul>
 * <li><b>Level</b> ({@link HarnessLevel}, an unconstructed {@code ServerLevel}):
 *   {@code getDifficulty()} answers the request's difficulty,
 *   {@code getGameRules()} a real {@code GameRules} whose only touched rule is
 *   {@code natural_health_regeneration}, {@code spigotConfig.regenExhaustion}
 *   the request's value, {@code isClientSide()} false, and {@code playSound}
 *   records {@code [volume, pitch]}.</li>
 * <li><b>Player health</b> (food.exhaustion): {@code getHealth()} is the
 *   harness field, {@code getMaxHealth()} is final in Paper so {@code isHurt()}
 *   is overridden with Paper's own expression over the harness maximum,
 *   {@code heal(amount, ...)} sets {@code health = clamp(health + amount, 0,
 *   max)} (the {@code setHealth} clamp) and
 *   {@code hurtServer(level, starve, amount)} sets {@code health =
 *   max(health - amount, 0)}: the armor, absorption and event pipeline of
 *   {@code LivingEntity.hurtServer} is not part of this op.</li>
 * <li><b>Events</b>: the Bukkit server is a proxy (installed once, process-wide, into
 *   {@code Bukkit.server} on the first food or xp request; it is never removed) whose
 *   plugin manager drops every event, so {@code FoodLevelChangeEvent} and
 *   {@code EntityExhaustionEvent} are never cancelled or modified.</li>
 * <li><b>damage_source.scaling</b> runs the real {@code Player.hurtServer}: the
 *   harness answers not-invulnerable, not-dead, health 20, no fire resistance,
 *   not sleeping, and stops the real {@code LivingEntity.hurtServer} at its
 *   {@code applyItemBlocking(level, source, damage)} call, which is the first
 *   place the scaled and non-negative-clamped damage is visible.</li>
 * <li><b>xp.curves</b> runs the real {@code ServerPlayer} experience methods;
 *   {@code increaseScore} is a no-op (the score lives in entity data, which an
 *   unconstructed entity does not have).</li>
 * </ul>
 */
final class EntityOps {
    private EntityOps() {
    }

    static List<RivetReferenceOracle.Op> ops() {
        return List.of(
            new RivetReferenceOracle.Op(
                "damage.calc",
                List.of("damage", "msg_id", "scaling", "exhaustion", "effects", "death_message", "tags"),
                (request, dec) -> damageCalc(request, dec),
                error -> false
            ),
            new RivetReferenceOracle.Op(
                "damage_source.scaling",
                List.of("scales", "hurt", "damage"),
                (request, dec) -> damageSourceScaling(request, dec),
                error -> false
            ),
            new RivetReferenceOracle.Op(
                "food.exhaustion",
                List.of("states"),
                (request, dec) -> foodExhaustion(request),
                error -> false
            ),
            new RivetReferenceOracle.Op(
                "xp.curves",
                List.of("needed", "states"),
                (request, dec) -> xpCurves(request),
                error -> false
            )
        );
    }

    // ---- request helpers --------------------------------------------------

    private static String str(final JsonObject request, final String key) {
        JsonElement element = request.get(key);
        if (element == null || !element.isJsonPrimitive() || !element.getAsJsonPrimitive().isString()) {
            throw new IllegalArgumentException("request property '" + key + "' must be a string");
        }
        return element.getAsString();
    }

    private static int integer(final JsonObject request, final String key) {
        JsonElement element = request.get(key);
        if (element == null || !element.isJsonPrimitive() || !element.getAsJsonPrimitive().isNumber()) {
            throw new IllegalArgumentException("request property '" + key + "' must be an integer");
        }
        return element.getAsInt();
    }

    private static boolean bool(final JsonObject request, final String key) {
        JsonElement element = request.get(key);
        if (element == null || !element.isJsonPrimitive() || !element.getAsJsonPrimitive().isBoolean()) {
            throw new IllegalArgumentException("request property '" + key + "' must be a boolean");
        }
        return element.getAsBoolean();
    }

    private static float f32(final JsonObject request, final String key) {
        return parseF32(str(request, key));
    }

    private static float parseF32(final String text) {
        if (text.length() != 10 || !text.startsWith("0x")) {
            throw new IllegalArgumentException("float must be 0x + 8 hex digits, got " + text);
        }
        return Float.intBitsToFloat((int) Long.parseLong(text.substring(2), 16));
    }

    private static JsonArray array(final JsonObject request, final String key) {
        JsonElement element = request.get(key);
        if (element == null || !element.isJsonArray()) {
            throw new IllegalArgumentException("request property '" + key + "' must be an array");
        }
        return element.getAsJsonArray();
    }

    // ---- reflection / allocation (harness plumbing) ----------------------

    private static final sun.misc.Unsafe UNSAFE = unsafe();

    private static sun.misc.Unsafe unsafe() {
        try {
            Field field = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
            field.setAccessible(true);
            return (sun.misc.Unsafe) field.get(null);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("no Unsafe", error);
        }
    }

    @SuppressWarnings("unchecked")
    private static <T> T allocate(final Class<T> type) {
        try {
            return (T) UNSAFE.allocateInstance(type);
        } catch (InstantiationException error) {
            throw new IllegalStateException("cannot allocate " + type, error);
        }
    }

    private static Field field(final Class<?> owner, final String name) {
        try {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            return field;
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(owner + "." + name, error);
        }
    }

    private static void set(final Object target, final Class<?> owner, final String name, final Object value) {
        try {
            field(owner, name).set(target, value);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(owner + "." + name, error);
        }
    }

    private static int getInt(final Object target, final Class<?> owner, final String name) {
        try {
            return field(owner, name).getInt(target);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(owner + "." + name, error);
        }
    }

    // ---- damage.calc ------------------------------------------------------

    /** A weapon-less, entity-less source: {@code CombatRules.getDamageAfterAbsorb} then never touches the victim. */
    private static final DamageSource PLAIN_SOURCE =
        new DamageSource(Holder.direct(new DamageType("generic", DamageScaling.NEVER, 0.0F)));

    private static JsonObject damageCalc(final JsonObject request, final JsonObject dec) {
        JsonObject result = new JsonObject();
        switch (str(request, "fn")) {
            case "after_absorb" -> {
                // getDamageAfterAbsorb(victim, damage, source, armor, toughness): the victim is read only
                // when source.getWeaponItem() is non-null (the Breach path, which needs a live ServerLevel
                // and victim and is therefore not part of this op).
                float damage = CombatRules.getDamageAfterAbsorb(
                    null, f32(request, "damage"), PLAIN_SOURCE, f32(request, "armor"), f32(request, "toughness"));
                RivetReferenceOracle.putF32(result, dec, "damage", damage);
            }
            case "after_magic_absorb" -> {
                float damage = CombatRules.getDamageAfterMagicAbsorb(f32(request, "damage"), f32(request, "protection"));
                RivetReferenceOracle.putF32(result, dec, "damage", damage);
            }
            case "type" -> damageTypeDefinition(str(request, "damage_type"), result, dec);
            default -> throw new IllegalArgumentException("unknown damage.calc fn " + request.get("fn"));
        }
        return result;
    }

    private static Holder<DamageType> damageType(final String id) {
        var lookup = RivetReferenceOracle.ServerRegistries.ACCESS.lookupOrThrow(Registries.DAMAGE_TYPE);
        return lookup.getOrThrow(ResourceKey.create(Registries.DAMAGE_TYPE, Identifier.parse(id)));
    }

    /** The data-driven definition of a vanilla damage type and every damage-type tag it is in. */
    private static void damageTypeDefinition(final String id, final JsonObject result, final JsonObject dec) {
        Holder<DamageType> holder = damageType(id);
        DamageType type = holder.value();
        result.addProperty("msg_id", type.msgId());
        result.addProperty("scaling", type.scaling().getSerializedName());
        RivetReferenceOracle.putF32(result, dec, "exhaustion", type.exhaustion());
        result.addProperty("effects", type.effects().getSerializedName());
        result.addProperty("death_message", type.deathMessageType().getSerializedName());
        TreeSet<String> tags = new TreeSet<>();
        holder.tags().forEach(tag -> tags.add(tag.location().toString()));
        JsonArray array = new JsonArray();
        tags.forEach(array::add);
        result.add("tags", array);
    }

    // ---- shared harness level --------------------------------------------

    /** An unconstructed {@code ServerLevel} answering only what the entity methods under test read. */
    private static final class HarnessLevel extends ServerLevel {
        Difficulty difficulty = Difficulty.NORMAL;
        GameRules rules;
        final List<float[]> sounds = new ArrayList<>();

        HarnessLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false, null, null, null, null, null, null);
        }

        static HarnessLevel create(final Difficulty difficulty, final boolean naturalRegen, final float regenExhaustion) {
            HarnessLevel level = allocate(HarnessLevel.class);
            level.difficulty = difficulty;
            // The allocated subclass has no field initialisers: build its own state.
            set(level, HarnessLevel.class, "sounds", new ArrayList<float[]>());
            level.rules = new GameRules(net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS);
            level.rules.set(GameRules.NATURAL_HEALTH_REGENERATION, naturalRegen, null);
            org.spigotmc.SpigotWorldConfig spigot = allocate(org.spigotmc.SpigotWorldConfig.class);
            set(spigot, org.spigotmc.SpigotWorldConfig.class, "regenExhaustion", regenExhaustion);
            set(level, net.minecraft.world.level.Level.class, "spigotConfig", spigot);
            return level;
        }

        @Override
        public Difficulty getDifficulty() {
            return this.difficulty;
        }

        @Override
        public GameRules getGameRules() {
            return this.rules;
        }

        @Override
        public void playSound(
            final Entity except, final double x, final double y, final double z, final SoundEvent sound,
            final SoundSource source, final float volume, final float pitch
        ) {
            this.sounds.add(new float[] {volume, pitch});
        }
    }

    private static Difficulty difficulty(final String name) {
        return switch (name) {
            case "peaceful" -> Difficulty.PEACEFUL;
            case "easy" -> Difficulty.EASY;
            case "normal" -> Difficulty.NORMAL;
            case "hard" -> Difficulty.HARD;
            default -> throw new IllegalArgumentException("unknown difficulty " + name);
        };
    }

    // ---- damage_source.scaling -------------------------------------------

    /** Stops {@code LivingEntity.hurtServer} where the damage first becomes observable. */
    private static final class Reached extends RuntimeException {
        final float damage;

        Reached(final float damage) {
            super(null, null, false, false);
            this.damage = damage;
        }
    }

    /** A {@code Player} whose {@code hurtServer} is Paper's own; see the class contract. */
    private static final class ScalingPlayer extends Player {
        ScalingPlayer() {
            super(null, new GameProfile(new UUID(0L, 0L), "harness"));
        }

        @Override
        public net.minecraft.world.level.GameType gameMode() {
            return null;
        }

        @Override
        public boolean isInvulnerableTo(final ServerLevel level, final DamageSource source) {
            return false;
        }

        @Override
        public float getHealth() {
            return 20.0F;
        }

        @Override
        public boolean isDeadOrDying() {
            return false;
        }

        @Override
        public boolean isSleeping() {
            return false;
        }

        @Override
        public boolean hasEffect(final Holder<MobEffect> effect) {
            return false;
        }

        @Override
        public float applyItemBlocking(final ServerLevel level, final DamageSource source, final float damage, final boolean dryRun) {
            throw new Reached(damage);
        }
    }

    private static Entity victimOf(final String kind) {
        return switch (kind) {
            case "none" -> null;
            case "living" -> allocate(Zombie.class);
            case "player" -> allocate(ServerPlayer.class);
            case "non_living" -> allocate(net.minecraft.world.entity.projectile.arrow.Arrow.class);
            default -> throw new IllegalArgumentException("unknown entity kind " + kind);
        };
    }

    private static JsonObject damageSourceScaling(final JsonObject request, final JsonObject dec) {
        Holder<DamageType> type = damageType(str(request, "damage_type"));
        DamageSource source = new DamageSource(type, victimOf(str(request, "direct")), victimOf(str(request, "causing")));
        float damage = f32(request, "damage");

        ScalingPlayer player = allocate(ScalingPlayer.class);
        set(player, Player.class, "abilities", new Abilities());
        HarnessLevel level = HarnessLevel.create(difficulty(str(request, "difficulty")), true, 6.0F);

        JsonObject result = new JsonObject();
        result.addProperty("scales", source.scalesWithDifficulty());
        try {
            player.hurtServer(level, source, damage);
            // Paper cancelled before LivingEntity.hurtServer (peaceful scaling).
            result.addProperty("hurt", false);
        } catch (Reached reached) {
            result.addProperty("hurt", true);
            RivetReferenceOracle.putF32(result, dec, "damage", reached.damage);
        }
        return result;
    }

    // ---- ServerPlayer harness (food.exhaustion, xp.curves) ---------------

    private static final class HarnessConnection extends ServerGamePacketListenerImpl {
        HarnessConnection() {
            super(null, null, null, null);
        }

        @Override
        public void send(final Packet<?> packet) {
        }
    }

    private static final class HarnessCraftPlayer extends CraftPlayer {
        float health;

        HarnessCraftPlayer() {
            super(null, null);
        }

        @Override
        public float getScaledHealth() {
            return this.health;
        }

        @Override
        public void sendHealthUpdate() {
        }

        @Override
        public org.bukkit.Server getServer() {
            return Lazy.BUKKIT_SERVER;
        }
    }

    /** A {@code ServerPlayer} without constructor: food, experience and health are the harness's own state. */
    private static final class HarnessPlayer extends ServerPlayer {
        HarnessLevel level;
        HarnessCraftPlayer bukkit;
        float health;
        float maxHealth;

        HarnessPlayer() {
            super(null, null, null, null);
        }

        static HarnessPlayer create(final HarnessLevel level, final float health, final float maxHealth, final boolean invulnerable) {
            HarnessPlayer player = allocate(HarnessPlayer.class);
            player.level = level;
            player.health = health;
            player.maxHealth = maxHealth;
            // Installs the Bukkit server proxy (Bukkit.getPluginManager() is read statically).
            java.util.Objects.requireNonNull(Lazy.BUKKIT_SERVER);
            player.bukkit = allocate(HarnessCraftPlayer.class);
            player.bukkit.health = health;
            Abilities abilities = new Abilities();
            abilities.invulnerable = invulnerable;
            set(player, Player.class, "abilities", abilities);
            set(player, Player.class, "foodData", new FoodData());
            set(player, Entity.class, "random", net.minecraft.util.RandomSource.create(0L));
            set(player, Entity.class, "position", Vec3.ZERO);
            player.connection = allocate(HarnessConnection.class);
            return player;
        }

        @Override
        public ServerLevel level() {
            return this.level;
        }

        @Override
        public CraftPlayer getBukkitEntity() {
            this.bukkit.health = this.health;
            return this.bukkit;
        }

        @Override
        public float getHealth() {
            return this.health;
        }

        @Override
        public boolean isHurt() {
            return this.getHealth() > 0.0F && this.getHealth() < this.maxHealth;
        }

        @Override
        public void heal(final float heal, final org.bukkit.event.entity.EntityRegainHealthEvent.RegainReason reason) {
            this.health = clamp(this.health + heal, 0.0F, this.maxHealth);
        }

        @Override
        public void heal(
            final float heal, final org.bukkit.event.entity.EntityRegainHealthEvent.RegainReason reason, final boolean fastRegen
        ) {
            this.health = clamp(this.health + heal, 0.0F, this.maxHealth);
        }

        @Override
        public boolean hurtServer(final ServerLevel level, final DamageSource source, final float damage) {
            this.health = Math.max(this.health - damage, 0.0F);
            return true;
        }

        @Override
        public net.minecraft.world.damagesource.DamageSources damageSources() {
            return Lazy.DAMAGE_SOURCES;
        }

        @Override
        public void increaseScore(final int amount) {
        }

        private static float clamp(final float value, final float min, final float max) {
            return net.minecraft.util.Mth.clamp(value, min, max);
        }
    }

    /** Built on first use so ops that need no registry or Bukkit server never pay for them. */
    private static final class Lazy {
        static final net.minecraft.world.damagesource.DamageSources DAMAGE_SOURCES =
            new net.minecraft.world.damagesource.DamageSources(RivetReferenceOracle.ServerRegistries.ACCESS);
        static final org.bukkit.Server BUKKIT_SERVER = installBukkitServer();
    }

    /** A Bukkit server whose plugin manager swallows every event (nothing is ever cancelled). */
    private static org.bukkit.Server installBukkitServer() {
        org.bukkit.plugin.PluginManager plugins = (org.bukkit.plugin.PluginManager) Proxy.newProxyInstance(
            EntityOps.class.getClassLoader(), new Class<?>[] {org.bukkit.plugin.PluginManager.class}, (proxy, method, args) -> {
                if (method.getName().equals("callEvent")) {
                    return null;
                }
                throw new UnsupportedOperationException("PluginManager." + method.getName());
            });
        org.bukkit.Server server = (org.bukkit.Server) Proxy.newProxyInstance(
            EntityOps.class.getClassLoader(), new Class<?>[] {org.bukkit.Server.class}, (proxy, method, args) -> {
                if (method.getName().equals("getPluginManager")) {
                    return plugins;
                }
                throw new UnsupportedOperationException("Server." + method.getName());
            });
        try {
            Field bukkit = org.bukkit.Bukkit.class.getDeclaredField("server");
            bukkit.setAccessible(true);
            bukkit.set(null, server);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("cannot install the harness Bukkit server", error);
        }
        return server;
    }

    // ---- food.exhaustion --------------------------------------------------

    private static JsonObject foodState(final HarnessPlayer player) {
        FoodData food = player.getFoodData();
        JsonObject state = new JsonObject();
        state.addProperty("food", food.getFoodLevel());
        state.addProperty("saturation", RivetReferenceOracle.f32Bits(food.getSaturationLevel()));
        state.addProperty("exhaustion", RivetReferenceOracle.f32Bits(food.exhaustionLevel));
        state.addProperty("tick_timer", getInt(food, FoodData.class, "tickTimer"));
        state.addProperty("health", RivetReferenceOracle.f32Bits(player.health));
        state.addProperty("needs_food", food.needsFood());
        state.addProperty("has_enough_food", food.hasEnoughFood());
        return state;
    }

    private static JsonObject foodExhaustion(final JsonObject request) {
        HarnessLevel level = HarnessLevel.create(
            difficulty(str(request, "difficulty")), bool(request, "natural_regen"), f32(request, "regen_exhaustion"));
        HarnessPlayer player = HarnessPlayer.create(
            level, f32(request, "health"), f32(request, "max_health"), bool(request, "invulnerable"));
        FoodData food = player.getFoodData();
        food.setFoodLevel(integer(request, "food"));
        food.setSaturation(f32(request, "saturation"));
        food.exhaustionLevel = f32(request, "exhaustion");
        set(food, FoodData.class, "tickTimer", integer(request, "tick_timer"));

        JsonArray states = new JsonArray();
        for (JsonElement element : array(request, "steps")) {
            JsonArray step = element.getAsJsonArray();
            switch (step.get(0).getAsString()) {
                case "tick" -> food.tick(player);
                case "eat" -> food.eat(step.get(1).getAsInt(), parseF32(step.get(2).getAsString()));
                case "eat_props" -> food.eat(new FoodProperties(step.get(1).getAsInt(), parseF32(step.get(2).getAsString()), false));
                case "eat_event" -> food.eat(
                    new FoodProperties(step.get(1).getAsInt(), parseF32(step.get(2).getAsString()), false), null, player);
                case "exhaust" -> food.addExhaustion(parseF32(step.get(1).getAsString()));
                case "set_food" -> food.setFoodLevel(step.get(1).getAsInt());
                case "set_saturation" -> food.setSaturation(parseF32(step.get(1).getAsString()));
                default -> throw new IllegalArgumentException("unknown food step " + step);
            }
            states.add(foodState(player));
        }
        JsonObject result = new JsonObject();
        result.add("states", states);
        return result;
    }

    // ---- xp.curves --------------------------------------------------------

    private static JsonObject xpState(final HarnessPlayer player, final HarnessLevel level) {
        JsonObject state = new JsonObject();
        state.addProperty("level", player.experienceLevel);
        state.addProperty("progress", RivetReferenceOracle.f32Bits(player.experienceProgress));
        state.addProperty("total", player.totalExperience);
        state.addProperty("needed", player.getXpNeededForNextLevel());
        JsonArray sounds = new JsonArray();
        for (float[] sound : level.sounds) {
            JsonArray entry = new JsonArray();
            entry.add(RivetReferenceOracle.f32Bits(sound[0]));
            entry.add(RivetReferenceOracle.f32Bits(sound[1]));
            sounds.add(entry);
        }
        level.sounds.clear();
        state.add("sounds", sounds);
        return state;
    }

    private static JsonObject xpCurves(final JsonObject request) {
        HarnessLevel level = HarnessLevel.create(Difficulty.NORMAL, true, 6.0F);
        HarnessPlayer player = HarnessPlayer.create(level, 20.0F, 20.0F, false);
        player.experienceLevel = integer(request, "level");
        player.experienceProgress = f32(request, "progress");
        player.totalExperience = integer(request, "total");
        player.tickCount = integer(request, "tick_count");
        set(player, Player.class, "lastLevelUpTime", integer(request, "last_level_up_time"));

        JsonObject result = new JsonObject();
        result.addProperty("needed", player.getXpNeededForNextLevel());
        JsonArray states = new JsonArray();
        for (JsonElement element : array(request, "steps")) {
            JsonArray step = element.getAsJsonArray();
            int amount = step.get(1).getAsInt();
            switch (step.get(0).getAsString()) {
                case "points" -> player.giveExperiencePoints(amount);
                case "levels" -> player.giveExperienceLevels(amount);
                case "set_points" -> player.setExperiencePoints(amount);
                case "set_levels" -> player.setExperienceLevels(amount);
                case "enchant" -> player.onEnchantmentPerformed(ItemStack.EMPTY, amount);
                default -> throw new IllegalArgumentException("unknown xp step " + step);
            }
            states.add(xpState(player, level));
        }
        result.add("states", states);
        return result;
    }
}
