package dev.rivet.oracle;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.google.gson.JsonParseException;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import com.mojang.serialization.Codec;
import com.mojang.serialization.DataResult;
import com.mojang.serialization.DynamicOps;
import com.mojang.serialization.JsonOps;
import java.io.BufferedReader;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.InputStreamReader;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Base64;
import java.util.Collections;
import java.util.HashSet;
import java.util.HexFormat;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.function.Predicate;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtAccounter;
import net.minecraft.nbt.NbtIo;
import net.minecraft.nbt.NbtAccounterException;
import net.minecraft.nbt.NbtFormatException;
import net.minecraft.nbt.ReportedNbtException;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.SnbtPrinterTagVisitor;
import net.minecraft.nbt.Tag;
import net.minecraft.nbt.TagParser;
import io.netty.buffer.ByteBuf;
import io.netty.buffer.ByteBufUtil;
import io.netty.buffer.Unpooled;
import net.minecraft.SharedConstants;
import net.minecraft.core.RegistryAccess;
import net.minecraft.network.ProtocolInfo;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.VarInt;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.configuration.ConfigurationProtocols;
import net.minecraft.network.protocol.game.GameProtocols;
import net.minecraft.network.protocol.handshake.HandshakeProtocols;
import net.minecraft.network.protocol.login.LoginProtocols;
import net.minecraft.network.protocol.status.StatusProtocols;
import net.minecraft.resources.RegistryOps;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.ComponentSerialization;
import net.minecraft.server.Bootstrap;

public final class RivetReferenceOracle {
    private static final int PROTOCOL_VERSION = 1;
    private static final int MAX_NBT_BYTES = 16 * 1024 * 1024;
    private static final int MAX_BASE64_CHARS = ((MAX_NBT_BYTES + 2) / 3) * 4;
    private static final Gson GSON = new GsonBuilder().disableHtmlEscaping().create();
    private static final Gson PRETTY = new GsonBuilder().disableHtmlEscaping().setPrettyPrinting().create();
    /** Repo-relative path of this file: every op's Java source lives here, so its SHA-256 is the op source hash. */
    private static final String OP_SOURCE = "tools/rivet-reference-oracle/src/RivetReferenceOracle.java";
    private static final TagParser<Tag> SNBT_PARSER = TagParser.create(NbtOps.INSTANCE);

    /**
     * The untouched stdout, captured before {@code Bootstrap.bootStrap()}
     * installs Paper's log4j SysOutOverSLF4J wrapper (which re-logs every
     * {@code System.out.println} as a {@code [STDOUT]: ...} line). The
     * JSON-Lines protocol rides on the raw stream so responses stay parseable.
     */
    private static final java.io.PrintStream RAW_STDOUT = System.out;

    static {
        // `component.json` decodes through `ComponentSerialization.CODEC`, whose
        // contents codecs touch the vanilla registries (score/selector/etc.).
        // Bootstrap the registries once at startup, the same way WorldGenSampler
        // does — no full server boot required. The NBT ops don't use registries
        // and are unaffected. `SharedConstants.tryDetectVersion()` must run
        // first (Bootstrap reads the current game version).
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
    }

    private RivetReferenceOracle() {
    }

    // ---- op registry ------------------------------------------------------
    //
    // An oracle op is a named handler that runs real Paper classes on one
    // request object. Every op declares the fixed order of its result keys
    // (the canonical-JSON contract both sides honour) and which exception
    // classes are input-rejection verdicts; every other exception is
    // `unavailable` evidence, never a verdict. The same handler serves live
    // mode (stdin/stdout JSON Lines, one JVM for any number of ops) and record
    // mode (`--record`, corpus -> committed JSONL + manifest), so a recorded
    // golden and a live answer can never come from different code.

    /** Runs an op on one request. {@code dec} receives optional human-readable decimals for float fields. */
    @FunctionalInterface
    private interface Handler {
        JsonObject run(JsonObject request, JsonObject dec) throws Exception;
    }

    private record Op(String name, List<String> resultKeys, Handler handler, Predicate<Exception> rejection) {
    }

    private static final List<String> TAG_KEYS = List.of("tag_id", "tag_type", "snbt", "pretty_snbt");

    private static final Map<String, Op> OPS = registry();

    private static Map<String, Op> registry() {
        Map<String, Op> ops = new LinkedHashMap<>();
        register(ops, new Op(
            "ping",
            List.of("paper_specification", "paper_implementation", "paper_commit", "paper_sha256", "java_version"),
            (request, dec) -> ping(),
            error -> false
        ));
        register(ops, new Op(
            "snbt.parse",
            TAG_KEYS,
            (request, dec) -> parseSnbt(requiredString(request, "input")),
            error -> error instanceof CommandSyntaxException
        ));
        register(ops, new Op(
            "nbt.encode",
            keys(TAG_KEYS, "bytes", "output_base64"),
            (request, dec) -> encodeNbt(requiredString(request, "input")),
            error -> error instanceof CommandSyntaxException
        ));
        register(ops, new Op(
            "nbt.decode",
            keys(TAG_KEYS, "bytes"),
            (request, dec) -> decodeNbt(requiredString(request, "input_base64")),
            RivetReferenceOracle::isNbtDecodeRejection
        ));
        // Component codec rejection is a successful `accept:false` result, so
        // no exception is ever a rejection verdict for this op.
        register(ops, new Op(
            "component.json",
            List.of("accept", "canonical"),
            (request, dec) -> componentJson(requiredString(request, "input")),
            error -> false
        ));
        // `codec.reencode`: decode a JSON value (or SNBT) with a named vanilla
        // codec under RegistryOps over the server's frozen registries, then
        // re-encode. A decline is a successful `accept:false` result naming the
        // phase, like `component.json`; only a malformed SNBT input is a
        // rejection verdict.
        register(ops, new Op(
            "codec.reencode",
            List.of("accept", "phase", "canonical"),
            (request, dec) -> codecReencode(request, dec),
            error -> error instanceof CommandSyntaxException
        ));
        // `packet.reencode`: look the packet up by protocol phase, direction
        // and id in the real ProtocolInfo, decode the body, re-encode it.
        // Any exception while decoding or encoding the supplied bytes is the
        // verdict "Paper rejects this packet"; a bad request is unavailable.
        register(ops, new Op(
            "packet.reencode",
            List.of("name", "body"),
            (request, dec) -> packetReencode(request),
            error -> error instanceof PacketRejection
        ));
        // `shapes.eval`: Vec3 / AABB / VoxelShape / Shapes, as Paper runs them.
        register(ops, new Op(
            "shapes.eval",
            List.of("out"),
            (request, dec) -> shapesEval(request),
            error -> error instanceof IllegalArgumentException || error instanceof UnsupportedOperationException
        ));
        return Collections.unmodifiableMap(ops);
    }

    private static void register(final Map<String, Op> ops, final Op op) {
        if (ops.putIfAbsent(op.name(), op) != null) {
            throw new IllegalStateException("duplicate op " + op.name());
        }
    }

    private static List<String> keys(final List<String> head, final String... tail) {
        List<String> all = new ArrayList<>(head);
        all.addAll(List.of(tail));
        return List.copyOf(all);
    }

    /** Run one op and return its result in the op's canonical key order. */
    private static JsonObject execute(final Op op, final JsonObject request, final JsonObject dec) throws Exception {
        return canonical(op, op.handler().run(request, dec));
    }

    /**
     * Rebuild a handler result in the op's declared key order. An undeclared
     * key, or a raw floating-point number anywhere in the result, is an
     * infrastructure fault (unavailable), never a verdict: floats must travel
     * as IEEE-754 bit hex (`f32Bits`/`f64Bits`) so both sides compare bytes.
     */
    private static JsonObject canonical(final Op op, final JsonObject result) {
        JsonObject ordered = new JsonObject();
        for (String key : op.resultKeys()) {
            JsonElement value = result.get(key);
            if (value != null) {
                requireCanonicalValue(op.name() + "." + key, value);
                ordered.add(key, value);
            }
        }
        if (ordered.size() != result.size()) {
            throw new IllegalStateException("op " + op.name() + " emitted undeclared result keys " + result.keySet()
                + "; declared " + op.resultKeys());
        }
        return ordered;
    }

    private static void requireCanonicalValue(final String path, final JsonElement value) {
        if (value.isJsonPrimitive() && value.getAsJsonPrimitive().isNumber()) {
            Number number = value.getAsNumber();
            if (number instanceof Double || number instanceof Float || number instanceof java.math.BigDecimal) {
                throw new IllegalStateException(path + ": floating-point values must be emitted as IEEE-754 bit hex, got " + number);
            }
        } else if (value.isJsonArray()) {
            int index = 0;
            for (JsonElement element : value.getAsJsonArray()) {
                requireCanonicalValue(path + "[" + index++ + "]", element);
            }
        } else if (value.isJsonObject()) {
            for (Map.Entry<String, JsonElement> entry : value.getAsJsonObject().entrySet()) {
                requireCanonicalValue(path + "." + entry.getKey(), entry.getValue());
            }
        }
    }

    /** Canonical f32: {@link Float#floatToIntBits} (NaN canonicalized, D14) as {@code 0x} + 8 lowercase hex digits. */
    static String f32Bits(final float value) {
        return "0x" + HexFormat.of().toHexDigits(Float.floatToIntBits(value));
    }

    /** Canonical f64: {@link Double#doubleToLongBits} (NaN canonicalized, D14) as {@code 0x} + 16 lowercase hex digits. */
    static String f64Bits(final double value) {
        return "0x" + HexFormat.of().toHexDigits(Double.doubleToLongBits(value));
    }

    /** Emit an f32 result field as bit hex, with its human decimal in {@code dec} (recorded, never compared). */
    static void putF32(final JsonObject result, final JsonObject dec, final String key, final float value) {
        result.addProperty(key, f32Bits(value));
        dec.addProperty(key, Float.toString(value));
    }

    /** Emit an f64 result field as bit hex, with its human decimal in {@code dec} (recorded, never compared). */
    static void putF64(final JsonObject result, final JsonObject dec, final String key, final double value) {
        result.addProperty(key, f64Bits(value));
        dec.addProperty(key, Double.toString(value));
    }

    public static void main(final String[] args) throws Exception {
        if (args.length > 0) {
            if (args.length == 1 && args[0].equals("--self-test")) {
                selfTest();
                return;
            }
            if (args.length == 3 && args[0].equals("--record")) {
                try {
                    record(Path.of(args[1]), Path.of(args[2]));
                } catch (Exception error) {
                    System.err.println("record failed: " + error);
                    System.err.flush();
                    // Paper class-init starts non-daemon threads; exit explicitly.
                    System.exit(1);
                }
                RAW_STDOUT.flush();
                System.exit(0);
            }
            if (args.length == 2 && args[0].equals("--dump-vanilla-shapes")) {
                try {
                    dumpVanillaShapes(Path.of(args[1]));
                } catch (Exception error) {
                    System.err.println("dump failed: " + error);
                    System.err.flush();
                    System.exit(1);
                }
                RAW_STDOUT.flush();
                System.exit(0);
            }
            System.err.println("Usage: run.sh [--self-test | --record <corpus-dir> <out-dir> | --dump-vanilla-shapes <out.jsonl>]");
            System.exit(64);
        }

        try (BufferedReader input = new BufferedReader(
            new InputStreamReader(System.in, StandardCharsets.UTF_8)
        )) {
            String line;
            while ((line = input.readLine()) != null) {
                if (!line.isBlank()) {
                    RAW_STDOUT.println(GSON.toJson(processLine(line)));
                    RAW_STDOUT.flush();
                }
            }
        }
    }

    private static JsonObject processLine(final String line) {
        JsonElement id = null;
        String operation = null;
        Op op = null;
        try {
            JsonObject request = JsonParser.parseString(line).getAsJsonObject();
            id = request.get("id");
            operation = requiredString(request, "op");
            op = OPS.get(operation);
            if (op == null) {
                throw new IllegalArgumentException("unknown operation: " + operation);
            }
            return success(id, execute(op, request, new JsonObject()));
        } catch (Exception error) {
            return failure(id, operation, op != null && op.rejection().test(error), error);
        }
    }

    // ---- record mode ------------------------------------------------------

    /**
     * Record every op corpus under {@code corpusDir} ({@code <op>.jsonl}, one
     * {@code {"id":...,<request fields>}} object per line) through the same
     * handlers live mode serves, writing {@code <outDir>/<op>.jsonl} plus a
     * deterministic {@code manifest.json} (Paper pin + commit, op source
     * SHA-256, corpus SHA-256 and count, output SHA-256 and count). Each
     * recorded line is {@code {"id","ok":true,"result",["dec"]}} or
     * {@code {"id","ok":false,"kind":"input_rejected","detail"}}; an input with
     * no verdict (unavailable) aborts the recording, as does an empty corpus or
     * a duplicate id. Re-running on an unchanged Paper must reproduce every
     * byte; `rivet-oracle regen --check` enforces that.
     */
    private static void record(final Path corpusDir, final Path outDir) throws Exception {
        List<Path> corpora;
        try (var listing = Files.list(corpusDir)) {
            corpora = listing.filter(path -> path.getFileName().toString().endsWith(".jsonl")).sorted().toList();
        }
        if (corpora.isEmpty()) {
            throw new IllegalStateException("no <op>.jsonl corpus under " + corpusDir + "; recording nothing is not a recording");
        }
        Files.createDirectories(outDir);
        JsonArray entries = new JsonArray();
        int total = 0;
        for (Path corpus : corpora) {
            String fileName = corpus.getFileName().toString();
            String opName = fileName.substring(0, fileName.length() - ".jsonl".length());
            Op op = OPS.get(opName);
            if (op == null) {
                throw new IllegalStateException("corpus " + fileName + " names no registered op");
            }
            if (opName.equals("ping")) {
                throw new IllegalStateException("ping is the provenance handshake, not a recordable op");
            }
            byte[] corpusBytes = Files.readAllBytes(corpus);
            List<String> lines = new String(corpusBytes, StandardCharsets.UTF_8).lines().toList();
            StringBuilder recorded = new StringBuilder();
            Set<String> ids = new HashSet<>();
            int count = 0;
            for (int index = 0; index < lines.size(); index++) {
                String line = lines.get(index);
                String where = fileName + ":" + (index + 1);
                if (line.isBlank()) {
                    throw new IllegalStateException(where + ": blank corpus line");
                }
                JsonObject entry = JsonParser.parseString(line).getAsJsonObject();
                String id = requiredString(entry, "id");
                if (!ids.add(id)) {
                    throw new IllegalStateException(where + ": duplicate input id " + id);
                }
                if (entry.has("op")) {
                    throw new IllegalStateException(where + ": corpus entries must not carry `op` (the file name is the op)");
                }
                JsonObject request = entry.deepCopy();
                request.addProperty("op", opName);
                recorded.append(GSON.toJson(recordOne(op, id, request))).append('\n');
                count++;
            }
            if (count == 0) {
                throw new IllegalStateException(fileName + ": empty corpus records nothing");
            }
            byte[] output = recorded.toString().getBytes(StandardCharsets.UTF_8);
            Files.write(outDir.resolve(opName + ".jsonl"), output);
            JsonObject entry = new JsonObject();
            entry.addProperty("op", opName);
            entry.addProperty("corpus", "corpus/" + fileName);
            entry.addProperty("corpus_sha256", sha256(corpusBytes));
            entry.addProperty("corpus_n", count);
            entry.addProperty("output", opName + ".jsonl");
            entry.addProperty("output_sha256", sha256(output));
            entry.addProperty("output_n", count);
            entries.add(entry);
            total += count;
        }
        JsonObject manifest = new JsonObject();
        manifest.addProperty("format", 1);
        manifest.addProperty("kind", "oracle-ops");
        manifest.addProperty("paper", requiredProperty("rivet.paper.pin"));
        manifest.addProperty("paper_commit", requiredProperty("rivet.paper.commit"));
        manifest.addProperty("op_source", OP_SOURCE);
        manifest.addProperty("op_source_sha256", sha256(Files.readAllBytes(Path.of(requiredProperty("rivet.oracle.source")))));
        manifest.add("ops", entries);
        Files.writeString(outDir.resolve("manifest.json"), PRETTY.toJson(manifest) + "\n", StandardCharsets.UTF_8);

        JsonObject summary = new JsonObject();
        summary.addProperty("ok", true);
        summary.addProperty("ops", entries.size());
        summary.addProperty("recorded", total);
        RAW_STDOUT.println(GSON.toJson(summary));
    }

    private static JsonObject recordOne(final Op op, final String id, final JsonObject request) {
        JsonObject line = new JsonObject();
        line.addProperty("id", id);
        JsonObject dec = new JsonObject();
        try {
            JsonObject result = execute(op, request, dec);
            line.addProperty("ok", true);
            line.add("result", result);
            if (dec.size() > 0) {
                line.add("dec", dec);
            }
        } catch (Exception error) {
            if (!op.rejection().test(error)) {
                throw new IllegalStateException(op.name() + " input " + id + " produced no verdict (unavailable): " + error, error);
            }
            line.addProperty("ok", false);
            line.addProperty("kind", "input_rejected");
            JsonObject detail = new JsonObject();
            detail.addProperty("type", error.getClass().getName());
            detail.addProperty("message", error.getMessage());
            line.add("detail", detail);
        }
        return line;
    }

    private static String sha256(final byte[] bytes) throws Exception {
        return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
    }

    private static JsonObject ping() {
        JsonObject result = new JsonObject();
        result.addProperty("paper_specification", requiredProperty("rivet.paper.specification"));
        result.addProperty("paper_implementation", requiredProperty("rivet.paper.implementation"));
        result.addProperty("paper_commit", requiredProperty("rivet.paper.commit"));
        result.addProperty("paper_sha256", requiredProperty("rivet.paper.sha256"));
        result.addProperty("java_version", Runtime.version().toString());
        return result;
    }

    private static JsonObject parseSnbt(final String input) throws Exception {
        return describeTag(SNBT_PARSER.parseFully(input));
    }

    private static JsonObject encodeNbt(final String input) throws Exception {
        CompoundTag tag = TagParser.parseCompoundFully(input);
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        try (DataOutputStream output = new DataOutputStream(bytes)) {
            NbtIo.write(tag, output);
        }

        JsonObject result = describeTag(tag);
        result.addProperty("bytes", bytes.size());
        result.addProperty("output_base64", Base64.getEncoder().encodeToString(bytes.toByteArray()));
        return result;
    }

    private static JsonObject decodeNbt(final String input) throws Exception {
        if (input.length() > MAX_BASE64_CHARS) {
            throw new RejectedInputException("base64 NBT input exceeds " + MAX_NBT_BYTES + " decoded bytes");
        }
        byte[] bytes;
        try {
            bytes = Base64.getDecoder().decode(input);
        } catch (IllegalArgumentException error) {
            throw new RejectedInputException(error.getMessage());
        }
        if (bytes.length > MAX_NBT_BYTES) {
            throw new RejectedInputException("NBT input exceeds " + MAX_NBT_BYTES + " bytes");
        }
        CompoundTag tag;
        try (DataInputStream data = new DataInputStream(new ByteArrayInputStream(bytes))) {
            tag = NbtIo.read(data, NbtAccounter.create(MAX_NBT_BYTES));
            if (data.available() != 0) {
                throw new RejectedInputException("trailing bytes after root compound: " + data.available());
            }
        }

        JsonObject result = describeTag(tag);
        result.addProperty("bytes", bytes.length);
        return result;
    }

    /**
     * `component.json`: decode a component JSON through
     * {@link ComponentSerialization#CODEC} under non-compressed {@link JsonOps}
     * and re-encode it. This is Paper's canonical round-trip identity for the
     * wire form a chat/title/player-info/scoreboard packet carries: `accept` is
     * whether the input decodes at all, and `canonical` is the exact JSON the
     * codec re-emits (byte-comparable with the Rust side, since both serialize
     * compactly with insertion order and no HTML escaping). A malformed input
     * (invalid JSON or an undecodable component) returns `accept:false` — the
     * accept/reject parity contract for the strict-malformed fixtures.
     *
     * The reject path is a VERDICT, not a failure: Paper's codec declining to
     * decode is a `DataResult.error`, returned as `accept:false`. A thrown
     * exception inside the codec (e.g. an NPE on an unexpected shape) is an
     * infrastructure crash and MUST propagate — the response then carries
     * `ok:false` + an `error`, and the parity tool hard-fails instead of
     * recording a spurious "both rejected" match.
     */
    private static JsonObject componentJson(final String input) {
        JsonObject result = new JsonObject();
        JsonElement element;
        try {
            element = JsonParser.parseString(input);
        } catch (JsonParseException error) {
            result.addProperty("accept", false);
            return result;
        }
        var decoded = ComponentSerialization.CODEC.decode(JsonOps.INSTANCE, element);
        if (decoded.error().isPresent()) {
            // Paper's codec declined to decode the input — a genuine reject.
            result.addProperty("accept", false);
            return result;
        }
        Component component = decoded.getOrThrow().getFirst();
        JsonElement encoded = ComponentSerialization.CODEC
            .encodeStart(JsonOps.INSTANCE, component)
            .getOrThrow();
        result.addProperty("accept", true);
        // Serialize with the same non-html-escaping Gson used for responses
        // so the canonical is byte-identical to the Rust encoder's output.
        result.addProperty("canonical", GSON.toJson(encoded));
        return result;
    }

    // ---- the named codec table ----------------------------------------------

    /** Every codec Rivet has ported, keyed by the name `rivet-parity`'s Rust handler table uses. */
    private static final class CodecTable {
        private static Map<String, Codec<?>> build() {
            Map<String, Codec<?>> codecs = new LinkedHashMap<>();
            codecs.put("ItemStack.CODEC", net.minecraft.world.item.ItemStack.CODEC);
            codecs.put("ItemStack.OPTIONAL_CODEC", net.minecraft.world.item.ItemStack.OPTIONAL_CODEC);
            codecs.put("DataComponentPatch.CODEC", net.minecraft.core.component.DataComponentPatch.CODEC);
            codecs.put("DataComponentMap.CODEC", net.minecraft.core.component.DataComponentMap.CODEC);
            codecs.put("ItemStackTemplate.CODEC", net.minecraft.world.item.ItemStackTemplate.CODEC);
            codecs.put("ItemContainerContents.CODEC", net.minecraft.world.item.component.ItemContainerContents.CODEC);
            codecs.put("BundleContents.CODEC", net.minecraft.world.item.component.BundleContents.CODEC);
            codecs.put("ChargedProjectiles.CODEC", net.minecraft.world.item.component.ChargedProjectiles.CODEC);
            codecs.put("FoodProperties.DIRECT_CODEC", net.minecraft.world.food.FoodProperties.DIRECT_CODEC);
            codecs.put("ItemUseAnimation.CODEC", net.minecraft.world.item.ItemUseAnimation.CODEC);
            codecs.put("ConsumeEffect.CODEC", net.minecraft.world.item.consume_effects.ConsumeEffect.CODEC);
            codecs.put("Consumable.CODEC", net.minecraft.world.item.component.Consumable.CODEC);
            codecs.put("DeathProtection.CODEC", net.minecraft.world.item.component.DeathProtection.CODEC);
            codecs.put("SuspiciousStewEffects.CODEC", net.minecraft.world.item.component.SuspiciousStewEffects.CODEC);
            codecs.put("UseCooldown.CODEC", net.minecraft.world.item.component.UseCooldown.CODEC);
            codecs.put("UseEffects.CODEC", net.minecraft.world.item.component.UseEffects.CODEC);
            codecs.put("UseRemainder.CODEC", net.minecraft.world.item.component.UseRemainder.CODEC);
            codecs.put("OminousBottleAmplifier.CODEC", net.minecraft.world.item.component.OminousBottleAmplifier.CODEC);
            codecs.put("BlockState.CODEC", net.minecraft.world.level.block.state.BlockState.CODEC);
            codecs.put("IntProviders.CODEC", net.minecraft.util.valueproviders.IntProviders.CODEC);
            codecs.put("FloatProviders.CODEC", net.minecraft.util.valueproviders.FloatProviders.CODEC);
            codecs.put("TextColor.CODEC", net.minecraft.network.chat.TextColor.CODEC);
            codecs.put("ComponentSerialization.CODEC", net.minecraft.network.chat.ComponentSerialization.CODEC);
            codecs.put("Xoroshiro128PlusPlus.CODEC", net.minecraft.world.level.levelgen.Xoroshiro128PlusPlus.CODEC);
            codecs.put("UUIDUtil.CODEC", net.minecraft.core.UUIDUtil.CODEC);
            codecs.put("Identifier.CODEC", net.minecraft.resources.Identifier.CODEC);
            codecs.put("BlockPos.CODEC", net.minecraft.core.BlockPos.CODEC);
            codecs.put("GlobalPos.CODEC", net.minecraft.core.GlobalPos.CODEC);
            codecs.put("Rotations.CODEC", net.minecraft.core.Rotations.CODEC);
            codecs.put("Direction.CODEC", net.minecraft.core.Direction.CODEC);
            codecs.put("Difficulty.CODEC", net.minecraft.world.Difficulty.CODEC);
            codecs.put("GameType.CODEC", net.minecraft.world.level.GameType.CODEC);
            codecs.put("Heightmap.Types.CODEC", net.minecraft.world.level.levelgen.Heightmap.Types.CODEC);
            codecs.put("LiquidSettings.CODEC", net.minecraft.world.level.levelgen.structure.templatesystem.LiquidSettings.CODEC);
            codecs.put("HeightProvider.CODEC", net.minecraft.world.level.levelgen.heightproviders.HeightProvider.CODEC);
            codecs.put("VerticalAnchor.CODEC", net.minecraft.world.level.levelgen.VerticalAnchor.CODEC);
            codecs.put("GeodeCrackSettings.CODEC", net.minecraft.world.level.levelgen.GeodeCrackSettings.CODEC);
            codecs.put("GeodeLayerSettings.CODEC", net.minecraft.world.level.levelgen.GeodeLayerSettings.CODEC);
            codecs.put("CarverDebugSettings.CODEC", net.minecraft.world.level.levelgen.carver.CarverDebugSettings.CODEC);
            codecs.put("MobEffectInstance.CODEC", net.minecraft.world.effect.MobEffectInstance.CODEC);
            codecs.put("FeatureSize.CODEC", net.minecraft.world.level.levelgen.feature.featuresize.FeatureSize.CODEC);
            codecs.put("RuleTest.CODEC", net.minecraft.world.level.levelgen.structure.templatesystem.RuleTest.CODEC);
            codecs.put("PosRuleTest.CODEC", net.minecraft.world.level.levelgen.structure.templatesystem.PosRuleTest.CODEC);
            codecs.put("RuleBlockEntityModifier.CODEC", net.minecraft.world.level.levelgen.structure.templatesystem.rule.blockentity.RuleBlockEntityModifier.CODEC);
            codecs.put("ProcessorRule.CODEC", net.minecraft.world.level.levelgen.structure.templatesystem.ProcessorRule.CODEC);
            codecs.put("NormalNoise.NoiseParameters.DIRECT_CODEC", net.minecraft.world.level.levelgen.synth.NormalNoise.NoiseParameters.DIRECT_CODEC);
            codecs.put("WorldPreset.DIRECT_CODEC", net.minecraft.world.level.levelgen.presets.WorldPreset.DIRECT_CODEC);
            codecs.put("DataPackConfig.CODEC", net.minecraft.world.level.DataPackConfig.CODEC);
            codecs.put("WorldDataConfiguration.CODEC", net.minecraft.world.level.WorldDataConfiguration.CODEC);
            codecs.put("FeatureFlags.CODEC", net.minecraft.world.flag.FeatureFlags.CODEC);
            codecs.put("WorldBorder.CODEC", net.minecraft.world.level.border.WorldBorder.CODEC);
            codecs.put("WorldBorder.Settings.CODEC", net.minecraft.world.level.border.WorldBorder.Settings.CODEC);
            codecs.put("WeatherData.CODEC", net.minecraft.world.level.saveddata.WeatherData.CODEC);
            codecs.put("WanderingTraderData.CODEC", net.minecraft.world.level.saveddata.WanderingTraderData.CODEC);
            codecs.put("LevelSettings.DifficultySettings.CODEC", net.minecraft.world.level.LevelSettings.DifficultySettings.CODEC);
            codecs.put("LevelData.RespawnData.CODEC", net.minecraft.world.level.storage.LevelData.RespawnData.CODEC);
            codecs.put("BlendingData.Packed.CODEC", net.minecraft.world.level.levelgen.blending.BlendingData.Packed.CODEC);
            codecs.put("NoiseSettings.CODEC", net.minecraft.world.level.levelgen.NoiseSettings.CODEC);
            codecs.put("ObjectiveCriteria.RenderType.CODEC", net.minecraft.world.scores.criteria.ObjectiveCriteria.RenderType.CODEC);
            codecs.put("ObjectiveCriteria.CODEC", net.minecraft.world.scores.criteria.ObjectiveCriteria.CODEC);
            codecs.put("DisplaySlot.CODEC", net.minecraft.world.scores.DisplaySlot.CODEC);
            codecs.put("NumberFormatTypes.CODEC", net.minecraft.network.chat.numbers.NumberFormatTypes.CODEC);
            codecs.put("Objective.Packed.CODEC", net.minecraft.world.scores.Objective.Packed.CODEC);
            codecs.put("ScoreboardSaveData.Packed.CODEC", net.minecraft.world.scores.ScoreboardSaveData.Packed.CODEC);
            codecs.put("Scoreboard.PackedScore.CODEC", net.minecraft.world.scores.Scoreboard.PackedScore.CODEC);
            codecs.put("Team.Visibility.CODEC", net.minecraft.world.scores.Team.Visibility.CODEC);
            codecs.put("Team.CollisionRule.CODEC", net.minecraft.world.scores.Team.CollisionRule.CODEC);
            codecs.put("PlayerTeam.Packed.CODEC", net.minecraft.world.scores.PlayerTeam.Packed.CODEC);
            codecs.put("TeamColor.CODEC", net.minecraft.world.scores.TeamColor.CODEC);
            codecs.put("Biome.Precipitation.CODEC", net.minecraft.world.level.biome.Biome.Precipitation.CODEC);
            codecs.put("Biome.TemperatureModifier.CODEC", net.minecraft.world.level.biome.Biome.TemperatureModifier.CODEC);
            codecs.put("BiomeSpecialEffects.CODEC", net.minecraft.world.level.biome.BiomeSpecialEffects.CODEC);
            codecs.put("BiomeSpecialEffects.GrassColorModifier.CODEC", net.minecraft.world.level.biome.BiomeSpecialEffects.GrassColorModifier.CODEC);
            codecs.put("Climate.Parameter.CODEC", net.minecraft.world.level.biome.Climate.Parameter.CODEC);
            codecs.put("Climate.ParameterPoint.CODEC", net.minecraft.world.level.biome.Climate.ParameterPoint.CODEC);
            codecs.put("MobSpawnSettings.MobSpawnCost.CODEC", net.minecraft.world.level.biome.MobSpawnSettings.MobSpawnCost.CODEC);
            codecs.put("MultiNoiseBiomeSourceParameterList.Preset.CODEC", net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterList.Preset.CODEC);
            codecs.put("TickPriority.CODEC", net.minecraft.world.ticks.TickPriority.CODEC);
            codecs.put("MobCategory.CODEC", net.minecraft.world.entity.MobCategory.CODEC);
            codecs.put("GenerationStep.Decoration.CODEC", net.minecraft.world.level.levelgen.GenerationStep.Decoration.CODEC);
            codecs.put("RarityFilter.CODEC", net.minecraft.world.level.levelgen.placement.RarityFilter.CODEC.codec());
            codecs.put("HeightRangePlacement.CODEC", net.minecraft.world.level.levelgen.placement.HeightRangePlacement.CODEC.codec());
            codecs.put("CountOnEveryLayerPlacement.CODEC", net.minecraft.world.level.levelgen.placement.CountOnEveryLayerPlacement.CODEC.codec());
            codecs.put("FixedPlacement.CODEC", net.minecraft.world.level.levelgen.placement.FixedPlacement.CODEC.codec());
            codecs.put("SurfaceWaterDepthFilter.CODEC", net.minecraft.world.level.levelgen.placement.SurfaceWaterDepthFilter.CODEC.codec());
            codecs.put("CaveSurface.CODEC", net.minecraft.world.level.levelgen.placement.CaveSurface.CODEC);
            codecs.put("InSquarePlacement.CODEC", net.minecraft.world.level.levelgen.placement.InSquarePlacement.CODEC.codec());
            codecs.put("HeightmapPlacement.CODEC", net.minecraft.world.level.levelgen.placement.HeightmapPlacement.CODEC.codec());
            codecs.put("RandomOffsetPlacement.CODEC", net.minecraft.world.level.levelgen.placement.RandomOffsetPlacement.CODEC.codec());
            codecs.put("SurfaceRelativeThresholdFilter.CODEC", net.minecraft.world.level.levelgen.placement.SurfaceRelativeThresholdFilter.CODEC.codec());
            codecs.put("CanyonCarverConfiguration.CanyonShapeConfiguration.CODEC", net.minecraft.world.level.levelgen.carver.CanyonCarverConfiguration.CanyonShapeConfiguration.CODEC);
            codecs.put("FoliagePlacer.CODEC", net.minecraft.world.level.levelgen.feature.foliageplacers.FoliagePlacer.CODEC);
            codecs.put("CountConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.CountConfiguration.CODEC);
            codecs.put("BlockStateConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.BlockStateConfiguration.CODEC);
            codecs.put("LayerConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.LayerConfiguration.CODEC);
            codecs.put("DeltaFeatureConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.DeltaFeatureConfiguration.CODEC);
            codecs.put("ReplaceSphereConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.ReplaceSphereConfiguration.CODEC);
            codecs.put("NoneFeatureConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.NoneFeatureConfiguration.CODEC);
            codecs.put("ColumnFeatureConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.ColumnFeatureConfiguration.CODEC);
            codecs.put("UnderwaterMagmaConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.UnderwaterMagmaConfiguration.CODEC);
            codecs.put("ItemLore.CODEC", net.minecraft.world.item.component.ItemLore.CODEC);
            codecs.put("CustomModelData.CODEC", net.minecraft.world.item.component.CustomModelData.CODEC);
            codecs.put("DyedItemColor.CODEC", net.minecraft.world.item.component.DyedItemColor.CODEC);
            codecs.put("MapItemColor.CODEC", net.minecraft.world.item.component.MapItemColor.CODEC);
            codecs.put("FireworkExplosion.CODEC", net.minecraft.world.item.component.FireworkExplosion.CODEC);
            codecs.put("Fireworks.CODEC", net.minecraft.world.item.component.Fireworks.CODEC);
            codecs.put("WritableBookContent.CODEC", net.minecraft.world.item.component.WritableBookContent.CODEC);
            codecs.put("BlockItemStateProperties.CODEC", net.minecraft.world.item.component.BlockItemStateProperties.CODEC);
            codecs.put("CustomData.CODEC", net.minecraft.world.item.component.CustomData.CODEC);
            codecs.put("AttackRange.CODEC", net.minecraft.world.item.component.AttackRange.CODEC);
            codecs.put("SwingAnimation.CODEC", net.minecraft.world.item.component.SwingAnimation.CODEC);
            codecs.put("Weapon.CODEC", net.minecraft.world.item.component.Weapon.CODEC);
            codecs.put("OreConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.OreConfiguration.CODEC);
            codecs.put("ReplaceBlockConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.ReplaceBlockConfiguration.CODEC);
            codecs.put("EndSpikeConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.EndSpikeConfiguration.CODEC);
            codecs.put("EndSpikeFeature.EndSpike.CODEC", net.minecraft.world.level.levelgen.feature.EndSpikeFeature.EndSpike.CODEC);
            codecs.put("TwistingVinesConfig.CODEC", net.minecraft.world.level.levelgen.feature.configurations.TwistingVinesConfig.CODEC);
            codecs.put("SculkPatchConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.SculkPatchConfiguration.CODEC);
            codecs.put("ProbabilityFeatureConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.ProbabilityFeatureConfiguration.CODEC);
            codecs.put("EndGatewayConfiguration.CODEC", net.minecraft.world.level.levelgen.feature.configurations.EndGatewayConfiguration.CODEC);
            codecs.put("WorldOptions.CODEC", net.minecraft.world.level.levelgen.WorldOptions.CODEC.codec());
            codecs.put("BelowZeroRetrogen.CODEC", net.minecraft.world.level.levelgen.BelowZeroRetrogen.CODEC);
            codecs.put("WrittenBookContent.CODEC", net.minecraft.world.item.component.WrittenBookContent.CODEC);
            codecs.put("LodestoneTracker.CODEC", net.minecraft.world.item.component.LodestoneTracker.CODEC);
            codecs.put("SeededContainerLoot.CODEC", net.minecraft.world.item.component.SeededContainerLoot.CODEC);
            codecs.put("TooltipDisplay.CODEC", net.minecraft.world.item.component.TooltipDisplay.CODEC);
            codecs.put("Enchantable.CODEC", net.minecraft.world.item.enchantment.Enchantable.CODEC);
            codecs.put("BlocksAttacks.ItemDamageFunction.CODEC", net.minecraft.world.item.component.BlocksAttacks.ItemDamageFunction.CODEC);
            codecs.put("KineticWeapon.Condition.CODEC", net.minecraft.world.item.component.KineticWeapon.Condition.CODEC);
            codecs.put("CountPlacement.CODEC", net.minecraft.world.level.levelgen.placement.CountPlacement.CODEC.codec());
            codecs.put("NoiseBasedCountPlacement.CODEC", net.minecraft.world.level.levelgen.placement.NoiseBasedCountPlacement.CODEC.codec());
            codecs.put("NoiseThresholdCountPlacement.CODEC", net.minecraft.world.level.levelgen.placement.NoiseThresholdCountPlacement.CODEC.codec());
            codecs.put("BiomeFilter.CODEC", net.minecraft.world.level.levelgen.placement.BiomeFilter.CODEC.codec());
            codecs.put("FontDescription.CODEC", net.minecraft.network.chat.FontDescription.CODEC);
            codecs.put("Tool.CODEC", net.minecraft.world.item.component.Tool.CODEC);
            codecs.put("Tool.Rule.CODEC", net.minecraft.world.item.component.Tool.Rule.CODEC);
            codecs.put("Repairable.CODEC", net.minecraft.world.item.enchantment.Repairable.CODEC);
            codecs.put("BlocksAttacks.CODEC", net.minecraft.world.item.component.BlocksAttacks.CODEC);
            codecs.put("BlocksAttacks.DamageReduction.CODEC", net.minecraft.world.item.component.BlocksAttacks.DamageReduction.CODEC);
            codecs.put("DamageResistant.CODEC", net.minecraft.world.item.component.DamageResistant.CODEC);
            codecs.put("KineticWeapon.CODEC", net.minecraft.world.item.component.KineticWeapon.CODEC);
            codecs.put("PiercingWeapon.CODEC", net.minecraft.world.item.component.PiercingWeapon.CODEC);
            codecs.put("SwingAnimationType.CODEC", net.minecraft.world.item.SwingAnimationType.CODEC);
            codecs.put("MapDecorations.CODEC", net.minecraft.world.item.component.MapDecorations.CODEC);
            codecs.put("MapDecorations.Entry.CODEC", net.minecraft.world.item.component.MapDecorations.Entry.CODEC);
            codecs.put("ItemAttributeModifiers.CODEC", net.minecraft.world.item.component.ItemAttributeModifiers.CODEC);
            codecs.put("ItemAttributeModifiers.Entry.CODEC", net.minecraft.world.item.component.ItemAttributeModifiers.Entry.CODEC);
            codecs.put("FireworkExplosion.Shape.CODEC", net.minecraft.world.item.component.FireworkExplosion.Shape.CODEC);
            codecs.put("InstrumentComponent.CODEC", net.minecraft.world.item.component.InstrumentComponent.CODEC);
            codecs.put("Instrument.DIRECT_CODEC", net.minecraft.world.item.Instrument.DIRECT_CODEC);
            codecs.put("TrimMaterial.DIRECT_CODEC", net.minecraft.world.item.equipment.trim.TrimMaterial.DIRECT_CODEC);
            codecs.put("ProvidesTrimMaterial.CODEC", net.minecraft.world.item.component.ProvidesTrimMaterial.CODEC);
            codecs.put("SulfurCubeContent.CODEC", net.minecraft.world.item.component.SulfurCubeContent.CODEC);
            codecs.put("IntProviders.NON_NEGATIVE_CODEC", net.minecraft.util.valueproviders.IntProviders.NON_NEGATIVE_CODEC);
            codecs.put("IntProviders.POSITIVE_CODEC", net.minecraft.util.valueproviders.IntProviders.POSITIVE_CODEC);
            codecs.put("Direction.Axis.CODEC", net.minecraft.core.Direction.Axis.CODEC);
            codecs.put("Biome.NETWORK_CODEC", net.minecraft.world.level.biome.Biome.NETWORK_CODEC);
            return Collections.unmodifiableMap(codecs);
        }
    }

    // ---- registries ------------------------------------------------------

    /**
     * The server's frozen registries, loaded the way {@code WorldLoader} does it
     * from the vanilla data pack: static registries plus the worldgen layer.
     * Initialized on first use so ops that need no registry never pay for it.
     */
    private static final class ServerRegistries {
        /**
         * Loading logs to the process's stdout ("Found new data pack ...");
         * live mode speaks JSON Lines on that stream, so logging is switched
         * off until the registries and Paper's configuration are ready.
         */
        private static final org.apache.logging.log4j.Level LOG_LEVEL = org.apache.logging.log4j.LogManager.getRootLogger().getLevel();

        static {
            org.apache.logging.log4j.core.config.Configurator.setRootLevel(org.apache.logging.log4j.Level.OFF);
        }

        static final RegistryAccess ACCESS = load();

        static {
            // Paper's packets read GlobalConfiguration (data sanitization,
            // packet limiter): initialize it with its defaults, as a first
            // start writes them, into a scratch directory.
            try {
                java.nio.file.Path scratch = Files.createTempDirectory("rivet-paper-config");
                new io.papermc.paper.configuration.PaperConfigurations(scratch).initializeGlobalConfiguration(ACCESS);
                // `anticheat.obfuscation.items` is only built by a running
                // MinecraftServer (its binding reads the server's registries),
                // and the packets read it for data sanitization. The op wants
                // the pure codec behaviour, so install the section with an
                // allocated binding whose level is NONE: nothing is obfuscated.
                var obfuscation = io.papermc.paper.configuration.GlobalConfiguration.get().anticheat.obfuscation;
                if (obfuscation.items == null) {
                    obfuscation.items = obfuscation.new Items();
                    var unsafeField = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
                    unsafeField.setAccessible(true);
                    var unsafe = (sun.misc.Unsafe) unsafeField.get(null);
                    var bindingClass = io.papermc.paper.util.sanitizer.ItemObfuscationBinding.class;
                    var binding = unsafe.allocateInstance(bindingClass);
                    unsafe.putObject(binding, unsafe.objectFieldOffset(bindingClass.getDeclaredField("level")),
                        io.papermc.paper.util.sanitizer.ItemObfuscationSession.ObfuscationLevel.NONE);
                    obfuscation.items.binding = (io.papermc.paper.util.sanitizer.ItemObfuscationBinding) binding;
                }
            } catch (Exception error) {
                throw new IllegalStateException("cannot initialize Paper's global configuration", error);
            } finally {
                org.apache.logging.log4j.core.config.Configurator.setRootLevel(LOG_LEVEL);
            }
        }

        private static RegistryAccess load() {
            try {
                var packs = net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
                net.minecraft.server.MinecraftServer.configurePackRepository(
                    packs, net.minecraft.world.level.WorldDataConfiguration.DEFAULT, true, false
                );
                try (var resources = new net.minecraft.server.packs.resources.MultiPackResourceManager(
                    net.minecraft.server.packs.PackType.SERVER_DATA, packs.openAllSelected()
                )) {
                    var layers = net.minecraft.server.RegistryLayer.createRegistryAccess();
                    var pending = net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(
                        resources, layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)
                    );
                    var lookups = net.minecraft.tags.TagLoader.buildUpdatedLookups(
                        layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN), pending
                    );
                    pending.forEach(tags -> tags.apply());
                    var worldgen = net.minecraft.resources.RegistryDataLoader.load(
                        resources, lookups, net.minecraft.resources.RegistryDataLoader.WORLDGEN_REGISTRIES, Runnable::run
                    ).join();
                    var access = layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN, worldgen).compositeAccess().freeze();
                    // ReloadableServerResources: bind every item's prototype components.
                    net.minecraft.core.registries.BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(access)
                        .forEach(net.minecraft.core.component.DataComponentInitializers.PendingComponents::apply);
                    return access;
                }
            } catch (Exception error) {
                throw new IllegalStateException("cannot load the vanilla registries", error);
            }
        }
    }

    // ---- codec.reencode ---------------------------------------------------

    /** The named codec table: every codec Rivet has ported, by the name the Rust handler table uses. */
    private static final Map<String, Codec<?>> CODECS = CodecTable.build();

    @SuppressWarnings("unchecked")
    private static JsonObject codecReencode(final JsonObject request, final JsonObject dec) throws Exception {
        String name = requiredString(request, "codec");
        String opsName = requiredString(request, "ops");
        Codec<Object> codec = (Codec<Object>) CODECS.get(name);
        if (codec == null) {
            throw new IllegalArgumentException("unknown codec: " + name);
        }
        JsonObject result = new JsonObject();
        switch (opsName) {
            case "json" -> {
                JsonElement input = request.get("input");
                if (input == null) {
                    throw new IllegalArgumentException("property 'input' is required");
                }
                DynamicOps<JsonElement> ops = RegistryOps.create(JsonOps.INSTANCE, ServerRegistries.ACCESS);
                reencode(codec, ops, input, GSON::toJson, result, dec);
            }
            case "nbt" -> {
                Tag input = SNBT_PARSER.parseFully(requiredString(request, "input"));
                DynamicOps<Tag> ops = RegistryOps.create(NbtOps.INSTANCE, ServerRegistries.ACCESS);
                reencode(codec, ops, input, Tag::toString, result, dec);
            }
            default -> throw new IllegalArgumentException("unknown ops: " + opsName);
        }
        return result;
    }

    /**
     * Decode then re-encode. A DataResult error, or an exception thrown by the
     * codec's own code, declines the input at that phase; the canonical form is
     * the encoder's output rendered compactly.
     */
    private static <T> void reencode(
        final Codec<Object> codec, final DynamicOps<T> ops, final T input,
        final java.util.function.Function<T, String> render, final JsonObject result, final JsonObject dec
    ) {
        Object value;
        try {
            DataResult<Object> decoded = codec.parse(ops, input);
            if (decoded.result().isEmpty()) {
                decline(result, "decode");
                dec.addProperty("error", decoded.error().map(DataResult.Error::message).orElse(""));
                return;
            }
            value = decoded.result().get();
        } catch (RuntimeException error) {
            decline(result, "decode");
            dec.addProperty("thrown", error.toString());
            return;
        }
        try {
            DataResult<T> encoded = codec.encodeStart(ops, value);
            if (encoded.result().isEmpty()) {
                decline(result, "encode");
                dec.addProperty("error", encoded.error().map(DataResult.Error::message).orElse(""));
                return;
            }
            result.addProperty("accept", true);
            result.addProperty("canonical", render.apply(encoded.result().get()));
        } catch (RuntimeException error) {
            decline(result, "encode");
            dec.addProperty("thrown", error.toString());
        }
    }

    private static void decline(final JsonObject result, final String phase) {
        result.addProperty("accept", false);
        result.addProperty("phase", phase);
    }

    // ---- packet.reencode --------------------------------------------------

    /** Paper rejected the supplied packet bytes (any exception while decoding or encoding them). */
    private static final class PacketRejection extends Exception {
        PacketRejection(final Throwable cause) {
            super(chain(cause), cause);
        }

        /** The exception and its causes, outermost first, for the recorded (never compared) detail. */
        private static String chain(final Throwable error) {
            StringBuilder text = new StringBuilder();
            for (Throwable current = error; current != null && text.length() < 600; current = current.getCause()) {
                if (text.length() > 0) {
                    text.append(" <- ");
                }
                text.append(current.getClass().getName()).append(": ").append(current.getMessage());
            }
            return text.toString();
        }
    }

    private static final Map<String, ProtocolInfo<?>> PROTOCOLS = new java.util.HashMap<>();

    private static synchronized ProtocolInfo<?> protocolInfo(final String state, final String direction) {
        String key = state + "/" + direction;
        ProtocolInfo<?> cached = PROTOCOLS.get(key);
        if (cached != null) {
            return cached;
        }
        boolean clientbound = switch (direction) {
            case "clientbound" -> true;
            case "serverbound" -> false;
            default -> throw new IllegalArgumentException("unknown direction: " + direction);
        };
        ProtocolInfo<?> info = switch (state) {
            case "handshake" -> {
                if (clientbound) {
                    throw new IllegalArgumentException("handshake has no clientbound protocol");
                }
                yield HandshakeProtocols.SERVERBOUND;
            }
            case "status" -> clientbound ? StatusProtocols.CLIENTBOUND : StatusProtocols.SERVERBOUND;
            case "login" -> clientbound ? LoginProtocols.CLIENTBOUND : LoginProtocols.SERVERBOUND;
            case "configuration" -> clientbound ? ConfigurationProtocols.CLIENTBOUND : ConfigurationProtocols.SERVERBOUND;
            case "play" -> {
                var decorator = RegistryFriendlyByteBuf.decorator(ServerRegistries.ACCESS);
                yield clientbound
                    ? GameProtocols.CLIENTBOUND_TEMPLATE.bind(decorator)
                    : GameProtocols.SERVERBOUND_TEMPLATE.bind(decorator, () -> true);
            }
            default -> throw new IllegalArgumentException("unknown state: " + state);
        };
        PROTOCOLS.put(key, info);
        return info;
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static JsonObject packetReencode(final JsonObject request) throws Exception {
        String state = requiredString(request, "state");
        String direction = requiredString(request, "direction");
        JsonElement idElement = request.get("packet_id");
        if (idElement == null || !idElement.isJsonPrimitive() || !idElement.getAsJsonPrimitive().isNumber()) {
            throw new IllegalArgumentException("property 'packet_id' must be a number");
        }
        int id = idElement.getAsInt();
        byte[] body;
        try {
            body = HexFormat.of().parseHex(requiredString(request, "body"));
        } catch (IllegalArgumentException error) {
            throw new IllegalArgumentException("property 'body' must be hex: " + error.getMessage());
        }
        ProtocolInfo info = protocolInfo(state, direction);

        ByteBuf input = Unpooled.buffer();
        VarInt.write(input, id);
        input.writeBytes(body);
        int prefix = input.readableBytes() - body.length;
        Packet<?> packet;
        byte[] encoded;
        try {
            packet = (Packet<?>) info.codec().decode(input);
            if (input.isReadable()) {
                // PacketDecoder: "was larger than I expected, found N bytes extra"
                throw new IOException("packet " + packet.type() + " has " + input.readableBytes() + " bytes extra");
            }
            ByteBuf output = Unpooled.buffer();
            info.codec().encode(output, packet);
            encoded = ByteBufUtil.getBytes(output);
        } catch (Exception | OutOfMemoryError error) {
            // A hostile length prefix can make Paper's trusted (unlimited) NBT
            // reader allocate past the heap: Paper cannot decode that packet.
            // A NullPointerException that names Paper's global configuration or
            // the (absent) MinecraftServer is a fault in the oracle's own setup,
            // never a verdict; any other NPE is Paper failing on the packet.
            for (Throwable cause = error; cause != null; cause = cause.getCause()) {
                if (cause instanceof NullPointerException
                    && cause.getMessage() != null
                    && (cause.getMessage().contains("GlobalConfiguration") || cause.getMessage().contains("MinecraftServer"))) {
                    throw error instanceof Exception exception ? exception : new IllegalStateException(error);
                }
            }
            throw new PacketRejection(error);
        }
        ByteBuf expectedPrefix = Unpooled.buffer();
        VarInt.write(expectedPrefix, id);
        if (encoded.length < prefix || !java.util.Arrays.equals(
            java.util.Arrays.copyOf(encoded, prefix), ByteBufUtil.getBytes(expectedPrefix)
        )) {
            throw new IllegalStateException("encoded packet does not carry packet id " + id);
        }
        JsonObject result = new JsonObject();
        result.addProperty("name", packet.type().id().toString());
        result.addProperty("body", HexFormat.of().formatHex(encoded, prefix, encoded.length));
        return result;
    }

    private static JsonObject describeTag(final Tag tag) {
        JsonObject result = new JsonObject();
        result.addProperty("tag_id", tag.getId());
        result.addProperty("tag_type", tag.getType().getName());
        result.addProperty("snbt", tag.toString());
        result.addProperty("pretty_snbt", new SnbtPrinterTagVisitor().visit(tag));
        return result;
    }

    private static String requiredString(final JsonObject request, final String property) {
        JsonElement value = request.get(property);
        if (value == null || !value.isJsonPrimitive() || !value.getAsJsonPrimitive().isString()) {
            throw new IllegalArgumentException("property '" + property + "' must be a string");
        }
        return value.getAsString();
    }

    private static String requiredProperty(final String property) {
        String value = System.getProperty(property);
        if (value == null || value.isBlank()) {
            throw new IllegalStateException("launcher did not provide " + property);
        }
        return value;
    }

    private static JsonObject success(final JsonElement id, final JsonObject result) {
        JsonObject response = response(id);
        response.addProperty("ok", true);
        response.add("result", result);
        return response;
    }

    private static final class RejectedInputException extends Exception {
        RejectedInputException(String message) {
            super(message);
        }
    }

    private static boolean isNbtDecodeRejection(final Exception error) {
        // This operation reads only an in-memory byte array. IOException
        // (including Paper's wrapping crash report) means malformed input,
        // not unavailable disk/network I/O. Do not classify arbitrary
        // runtime exceptions or arbitrary IllegalArgumentException here.
        return error instanceof RejectedInputException
            || error instanceof IOException
            || error instanceof NbtAccounterException
            || error instanceof NbtFormatException
            || (error instanceof ReportedNbtException && error.getCause() instanceof IOException);
    }

    private static JsonObject failure(final JsonElement id, final String operation, final boolean rejected, final Exception error) {
        JsonObject details = new JsonObject();
        details.addProperty("kind", rejected ? "input_rejected" : "unavailable");
        details.addProperty("operation", operation);
        details.addProperty("type", error.getClass().getName());
        details.addProperty("message", error.getMessage());
        if (error.getCause() != null) {
            // Diagnostics only: the causes behind a wrapped exception.
            StringBuilder causes = new StringBuilder();
            for (Throwable cause = error.getCause(); cause != null && causes.length() < 600; cause = cause.getCause()) {
                causes.append(causes.length() > 0 ? " <- " : "").append(cause.getClass().getName()).append(": ").append(cause.getMessage());
            }
            details.addProperty("cause", causes.toString());
        }

        JsonObject response = response(id);
        response.addProperty("ok", false);
        response.add("error", details);
        return response;
    }

    private static JsonObject response(final JsonElement id) {
        JsonObject response = new JsonObject();
        if (id != null) {
            response.add("id", id.deepCopy());
        }
        response.addProperty("protocol", PROTOCOL_VERSION);
        return response;
    }

    private static void selfTest() {
        JsonObject ping = processLine("{\"id\":\"ping\",\"op\":\"ping\"}");
        requireSuccess(ping, "ping");

        JsonObject parsed = processLine(
            "{\"id\":\"parse\",\"op\":\"snbt.parse\",\"input\":\"{answer:42,ok:true}\"}"
        );
        requireSuccess(parsed, "snbt.parse");
        JsonObject parsedResult = parsed.getAsJsonObject("result");
        requireEquals(10, parsedResult.get("tag_id").getAsInt(), "compound tag id");
        requireEquals("{answer:42,ok:1b}", parsedResult.get("snbt").getAsString(), "canonical SNBT");

        String encodedRequest = "{\"id\":\"encode\",\"op\":\"nbt.encode\","
            + "\"input\":\"{name:\\\"Rivet\\\",values:[I;1,2,3]}\"}";
        JsonObject encoded = processLine(encodedRequest);
        requireSuccess(encoded, "nbt.encode");
        requireEquals(
            "CgAACAAEbmFtZQAFUml2ZXQLAAZ2YWx1ZXMAAAADAAAAAQAAAAIAAAADAA==",
            encoded.getAsJsonObject("result").get("output_base64").getAsString(),
            "binary NBT fixture"
        );

        String base64 = encoded.getAsJsonObject("result").get("output_base64").getAsString();
        JsonObject decodeRequest = new JsonObject();
        decodeRequest.addProperty("id", "decode");
        decodeRequest.addProperty("op", "nbt.decode");
        decodeRequest.addProperty("input_base64", base64);
        JsonObject decoded = processLine(GSON.toJson(decodeRequest));
        requireSuccess(decoded, "nbt.decode");

        String encodedSnbt = encoded.getAsJsonObject("result").get("snbt").getAsString();
        String decodedSnbt = decoded.getAsJsonObject("result").get("snbt").getAsString();
        if (!encodedSnbt.equals(decodedSnbt)) {
            throw new IllegalStateException("NBT round trip changed SNBT");
        }

        byte[] oversizedArray = new byte[]{10, 0, 0, 7, 0, 1, 'x', 127, -1, -1, -1};
        JsonObject hostileRequest = new JsonObject();
        hostileRequest.addProperty("op", "nbt.decode");
        hostileRequest.addProperty("input_base64", Base64.getEncoder().encodeToString(oversizedArray));
        JsonObject hostile = processLine(GSON.toJson(hostileRequest));
        if (hostile.get("ok").getAsBoolean()) {
            throw new IllegalStateException("oversized NBT declaration was accepted");
        }
        requireSuccess(processLine("{\"op\":\"ping\"}"), "ping after malformed NBT");

        JsonObject malformed = processLine("{\"op\":\"snbt.parse\",\"input\":\"{broken:\"}");
        if (malformed.get("ok").getAsBoolean()) {
            throw new IllegalStateException("malformed SNBT was accepted");
        }
        requireEquals("input_rejected", malformed.getAsJsonObject("error").get("kind").getAsString(), "syntax error verdict");
        requireEquals("snbt.parse", malformed.getAsJsonObject("error").get("operation").getAsString(), "syntax error operation");
        NullPointerException injected = new NullPointerException("injected internal failure");
        JsonObject internal = failure(null, "snbt.parse", OPS.get("snbt.parse").rejection().test(injected), injected);
        requireEquals("unavailable", internal.getAsJsonObject("error").get("kind").getAsString(), "internal error is not rejection");
        JsonObject unknown = processLine("{\"op\":\"not_an_operation\"}");
        requireEquals("unavailable", unknown.getAsJsonObject("error").get("kind").getAsString(), "unknown operation is not rejection");
        JsonObject missingInput = processLine("{\"op\":\"snbt.parse\"}");
        requireEquals("unavailable", missingInput.getAsJsonObject("error").get("kind").getAsString(), "malformed request is not rejection");
        JsonObject badBase64 = processLine("{\"op\":\"nbt.decode\",\"input_base64\":\"!!!\"}");
        requireEquals("input_rejected", badBase64.getAsJsonObject("error").get("kind").getAsString(), "base64 input rejection");
        JsonObject truncated = processLine("{\"op\":\"nbt.decode\",\"input_base64\":\"\"}");
        requireEquals("input_rejected", truncated.getAsJsonObject("error").get("kind").getAsString(), "truncated NBT rejection");
        // Spigot's oversized-array guard throws a bare IllegalArgumentException.
        // Without an unambiguous input-rejection type, preserve the failure as
        // unavailable evidence rather than accepting arbitrary runtime faults.
        requireEquals("unavailable", hostile.getAsJsonObject("error").get("kind").getAsString(), "ambiguous runtime failure is unavailable");

        // Registry contract: every op declares a non-empty, duplicate-free
        // result-key order, and the canonicalizer refuses raw floats and
        // undeclared keys instead of passing them through.
        for (Op op : OPS.values()) {
            if (op.resultKeys().isEmpty() || Set.copyOf(op.resultKeys()).size() != op.resultKeys().size()) {
                throw new IllegalStateException("op " + op.name() + " has an invalid result-key order");
            }
        }
        JsonObject rawFloat = new JsonObject();
        rawFloat.addProperty("accept", 1.5d);
        requireThrows(() -> canonical(OPS.get("component.json"), rawFloat), "raw float result");
        JsonObject undeclared = new JsonObject();
        undeclared.addProperty("accept", true);
        undeclared.addProperty("extra", 1);
        requireThrows(() -> canonical(OPS.get("component.json"), undeclared), "undeclared result key");
        // IEEE-754 bit hex with Java's NaN canonicalization (D14).
        requireEquals("0x3f800000", f32Bits(1.0f), "f32 bits");
        requireEquals("0x80000000", f32Bits(-0.0f), "f32 negative zero");
        requireEquals("0x7fc00000", f32Bits(Float.intBitsToFloat(0x7fc00001)), "f32 NaN canonical");
        requireEquals("0x3ff0000000000000", f64Bits(1.0d), "f64 bits");
        requireEquals("0x7ff8000000000000", f64Bits(Double.longBitsToDouble(0x7ff0000000000001L)), "f64 NaN canonical");

        JsonObject summary = new JsonObject();
        summary.addProperty("ok", true);
        summary.addProperty("protocol", PROTOCOL_VERSION);
        summary.addProperty("tests", 22);
        // Emit on the raw stream, not System.out: Bootstrap.bootStrap() has
        // re-wired System.out through log4j's SysOutOverSLF4J wrapper, so a
        // System.out.println here would surface as a `[HH:mm:ss LEVEL]: [STDOUT]:
        // {...}` log line instead of a bare JSON line. The JSON-Lines protocol
        // (and any consumer that parses the self-test output line by line) rides
        // on the raw stream.
        RAW_STDOUT.println(GSON.toJson(summary));
        RAW_STDOUT.flush();
    }

    private static void requireThrows(final Runnable action, final String label) {
        try {
            action.run();
        } catch (IllegalStateException expected) {
            return;
        }
        throw new IllegalStateException(label + ": expected the canonicalizer to refuse it");
    }

    private static void requireSuccess(final JsonObject response, final String operation) {
        if (!response.get("ok").getAsBoolean()) {
            throw new IllegalStateException(operation + " self-test failed: " + response);
        }
    }

    private static void requireEquals(final Object expected, final Object actual, final String label) {
        if (!expected.equals(actual)) {
            throw new IllegalStateException(label + ": expected " + expected + ", got " + actual);
        }
    }

    /**
     * {@code --dump-vanilla-shapes <out.jsonl>}: every distinct collision,
     * outline, occlusion and visual shape of every vanilla block state, as its
     * {@code toAabbs()} box list. One line per distinct box list, in block-state
     * registry order, naming the first state and shape kind that produced it.
     * This is the seed list of the {@code shapes.eval} vanilla corpus; the
     * Rust corpus generator rebuilds each shape from its boxes.
     */
    private static void dumpVanillaShapes(final Path out) throws Exception {
        java.util.Map<String, JsonObject> distinct = new LinkedHashMap<>();
        for (net.minecraft.world.level.block.state.BlockState state : net.minecraft.world.level.block.Block.BLOCK_STATE_REGISTRY) {
            net.minecraft.world.phys.shapes.VoxelShape[] shapes = {
                state.getCollisionShape(net.minecraft.world.level.EmptyBlockGetter.INSTANCE, net.minecraft.core.BlockPos.ZERO),
                state.getShape(net.minecraft.world.level.EmptyBlockGetter.INSTANCE, net.minecraft.core.BlockPos.ZERO),
                state.getOcclusionShape(),
                state.getVisualShape(net.minecraft.world.level.EmptyBlockGetter.INSTANCE, net.minecraft.core.BlockPos.ZERO,
                    net.minecraft.world.phys.shapes.CollisionContext.empty()),
            };
            String[] kinds = {"collision", "shape", "occlusion", "visual"};
            for (int i = 0; i < shapes.length; i++) {
                JsonArray boxes = new JsonArray();
                for (net.minecraft.world.phys.AABB box : shapes[i].toAabbs()) {
                    boxes.add(aabbBits(box));
                }
                String key = boxes.toString();
                if (!distinct.containsKey(key)) {
                    JsonObject line = new JsonObject();
                    line.addProperty("from", state.toString() + ":" + kinds[i]);
                    line.add("boxes", boxes);
                    distinct.put(key, line);
                }
            }
        }
        StringBuilder text = new StringBuilder();
        for (JsonObject line : distinct.values()) {
            text.append(GSON.toJson(line)).append('\n');
        }
        Files.writeString(out, text.toString(), StandardCharsets.UTF_8);
        JsonObject summary = new JsonObject();
        summary.addProperty("ok", true);
        summary.addProperty("distinct", distinct.size());
        RAW_STDOUT.println(GSON.toJson(summary));
    }

    // ---- shapes.eval ------------------------------------------------------
    //
    // `shapes.eval`: net.minecraft.world.phys (Vec3, AABB) and
    // net.minecraft.world.phys.shapes (Shapes, VoxelShape, BooleanOp) as run by
    // Paper (with the Moonrise collision patches). A request names a `kind`:
    //
    //   vec3   {fn, a, b, s, f}      one Vec3 function
    //   aabb   {fn, box, box2, v, v2, d, s, boxes, pos}   one AABB function
    //   shape  {spec, query}         build a shape from a `spec` tree and
    //                                answer its summary plus one `query`
    //
    // Every double/float travels as IEEE-754 bit hex (`0x...`), so a request and
    // an answer are exact; the answer is `{"out": ...}` with every double again
    // as bit hex. IllegalArgumentException (Shapes.box with min > max) and
    // UnsupportedOperationException (a join operator with (false,false) -> true)
    // are input-rejection verdicts.

    private static double hexF64(final JsonElement element) {
        String text = element.getAsString();
        if (!text.startsWith("0x") || text.length() != 18) {
            throw new IllegalStateException("bad f64 bits " + text);
        }
        return Double.longBitsToDouble(Long.parseUnsignedLong(text.substring(2), 16));
    }

    private static float hexF32(final JsonElement element) {
        String text = element.getAsString();
        if (!text.startsWith("0x") || text.length() != 10) {
            throw new IllegalStateException("bad f32 bits " + text);
        }
        return Float.intBitsToFloat(Integer.parseUnsignedInt(text.substring(2), 16));
    }

    private static net.minecraft.world.phys.Vec3 hexVec3(final JsonElement element) {
        JsonArray a = element.getAsJsonArray();
        return new net.minecraft.world.phys.Vec3(hexF64(a.get(0)), hexF64(a.get(1)), hexF64(a.get(2)));
    }

    private static net.minecraft.world.phys.AABB hexAabb(final JsonElement element) {
        JsonArray a = element.getAsJsonArray();
        return new net.minecraft.world.phys.AABB(
            hexF64(a.get(0)), hexF64(a.get(1)), hexF64(a.get(2)), hexF64(a.get(3)), hexF64(a.get(4)), hexF64(a.get(5)));
    }

    private static net.minecraft.core.BlockPos intPos(final JsonElement element) {
        JsonArray a = element.getAsJsonArray();
        return new net.minecraft.core.BlockPos(a.get(0).getAsInt(), a.get(1).getAsInt(), a.get(2).getAsInt());
    }

    private static JsonArray bitsOf(final double... values) {
        JsonArray array = new JsonArray();
        for (double value : values) {
            array.add(f64Bits(value));
        }
        return array;
    }

    private static JsonArray vec3Bits(final net.minecraft.world.phys.Vec3 v) {
        return bitsOf(v.x, v.y, v.z);
    }

    private static JsonArray aabbBits(final net.minecraft.world.phys.AABB b) {
        return bitsOf(b.minX, b.minY, b.minZ, b.maxX, b.maxY, b.maxZ);
    }

    private static net.minecraft.core.Direction directionOf(final JsonElement element) {
        return net.minecraft.core.Direction.valueOf(element.getAsString());
    }

    private static net.minecraft.core.Direction.Axis axisOf(final JsonElement element) {
        return net.minecraft.core.Direction.Axis.valueOf(element.getAsString());
    }

    private static JsonObject shapesEval(final JsonObject request) throws Exception {
        String kind = requiredString(request, "kind");
        JsonObject out = new JsonObject();
        switch (kind) {
            case "vec3" -> shapesVec3(request, out);
            case "aabb" -> shapesAabb(request, out);
            case "shape" -> shapesShape(request, out);
            default -> throw new IllegalStateException("unknown shapes.eval kind " + kind);
        }
        JsonObject result = new JsonObject();
        result.add("out", out);
        return result;
    }

    private static void putVec3(final JsonObject out, final net.minecraft.world.phys.Vec3 v) {
        out.add("v", vec3Bits(v));
    }

    private static void shapesVec3(final JsonObject request, final JsonObject out) {
        String fn = requiredString(request, "fn");
        net.minecraft.world.phys.Vec3 a = request.has("a") ? hexVec3(request.get("a")) : null;
        net.minecraft.world.phys.Vec3 b = request.has("b") ? hexVec3(request.get("b")) : null;
        double s = request.has("s") ? hexF64(request.get("s")) : 0.0;
        float f = request.has("f") ? hexF32(request.get("f")) : 0.0f;
        float g = request.has("g") ? hexF32(request.get("g")) : 0.0f;
        switch (fn) {
            case "add" -> putVec3(out, a.add(b));
            case "addScalar" -> putVec3(out, a.add(s));
            case "subtract" -> putVec3(out, a.subtract(b));
            case "subtractScalar" -> putVec3(out, a.subtract(s));
            case "multiply" -> putVec3(out, a.multiply(b));
            case "scale" -> putVec3(out, a.scale(s));
            case "cross" -> putVec3(out, a.cross(b));
            case "vectorTo" -> putVec3(out, a.vectorTo(b));
            case "normalize" -> putVec3(out, a.normalize());
            case "reverse" -> putVec3(out, a.reverse());
            case "horizontal" -> putVec3(out, a.horizontal());
            case "lerp" -> putVec3(out, a.lerp(b, s));
            case "xRot" -> putVec3(out, a.xRot(f));
            case "yRot" -> putVec3(out, a.yRot(f));
            case "zRot" -> putVec3(out, a.zRot(f));
            case "rotateClockwise90" -> putVec3(out, a.rotateClockwise90());
            case "projectedOn" -> putVec3(out, a.projectedOn(b));
            case "addLocalCoordinates" -> putVec3(out, a.addLocalCoordinates(b));
            case "directionFromRotation" -> putVec3(out, net.minecraft.world.phys.Vec3.directionFromRotation(f, g));
            case "dot" -> out.addProperty("n", f64Bits(a.dot(b)));
            case "length" -> out.addProperty("n", f64Bits(a.length()));
            case "lengthSqr" -> out.addProperty("n", f64Bits(a.lengthSqr()));
            case "distanceTo" -> out.addProperty("n", f64Bits(a.distanceTo(b)));
            case "distanceToSqr" -> out.addProperty("n", f64Bits(a.distanceToSqr(b)));
            case "horizontalDistance" -> out.addProperty("n", f64Bits(a.horizontalDistance()));
            case "horizontalDistanceSqr" -> out.addProperty("n", f64Bits(a.horizontalDistanceSqr()));
            case "closerThan" -> out.addProperty("b", a.closerThan(b, s));
            case "closerThanXzY" -> out.addProperty("b", a.closerThan(b, s, hexF64(request.get("t"))));
            case "isFinite" -> out.addProperty("b", a.isFinite());
            case "equals" -> out.addProperty("b", a.equals(b));
            case "hashCode" -> out.addProperty("i", a.hashCode());
            case "toString" -> out.addProperty("s", a.toString());
            case "rotation" -> {
                net.minecraft.world.phys.Vec2 rotation = a.rotation();
                JsonArray bits = new JsonArray();
                bits.add(f32Bits(rotation.x));
                bits.add(f32Bits(rotation.y));
                out.add("f", bits);
            }
            default -> throw new IllegalStateException("unknown vec3 fn " + fn);
        }
    }

    private static void shapesAabb(final JsonObject request, final JsonObject out) {
        String fn = requiredString(request, "fn");
        net.minecraft.world.phys.AABB box = hexAabb(request.get("box"));
        net.minecraft.world.phys.AABB box2 = request.has("box2") ? hexAabb(request.get("box2")) : null;
        net.minecraft.world.phys.Vec3 v = request.has("v") ? hexVec3(request.get("v")) : null;
        net.minecraft.world.phys.Vec3 v2 = request.has("v2") ? hexVec3(request.get("v2")) : null;
        net.minecraft.world.phys.Vec3 d = request.has("d") ? hexVec3(request.get("d")) : null;
        double s = request.has("s") ? hexF64(request.get("s")) : 0.0;
        switch (fn) {
            case "new" -> out.add("box", aabbBits(box));
            case "move" -> out.add("box", aabbBits(box.move(d.x, d.y, d.z)));
            case "moveVec" -> out.add("box", aabbBits(box.move(d)));
            case "moveBlockPos" -> out.add("box", aabbBits(box.move(intPos(request.get("pos")))));
            case "inflate3" -> out.add("box", aabbBits(box.inflate(d.x, d.y, d.z)));
            case "inflate" -> out.add("box", aabbBits(box.inflate(s)));
            case "deflate3" -> out.add("box", aabbBits(box.deflate(d.x, d.y, d.z)));
            case "deflate" -> out.add("box", aabbBits(box.deflate(s)));
            case "contract" -> out.add("box", aabbBits(box.contract(d.x, d.y, d.z)));
            case "expandTowards" -> out.add("box", aabbBits(box.expandTowards(d)));
            case "expandTowards3" -> out.add("box", aabbBits(box.expandTowards(d.x, d.y, d.z)));
            case "intersect" -> out.add("box", aabbBits(box.intersect(box2)));
            case "minmax" -> out.add("box", aabbBits(box.minmax(box2)));
            case "setMinX" -> out.add("box", aabbBits(box.setMinX(s)));
            case "setMaxY" -> out.add("box", aabbBits(box.setMaxY(s)));
            case "setMinZ" -> out.add("box", aabbBits(box.setMinZ(s)));
            case "intersects" -> out.addProperty("b", box.intersects(box2));
            case "intersectsVecs" -> out.addProperty("b", box.intersects(v, v2));
            case "intersectsBlockPos" -> out.addProperty("b", box.intersects(intPos(request.get("pos"))));
            case "contains" -> out.addProperty("b", box.contains(v));
            case "hasNaN" -> out.addProperty("b", box.hasNaN());
            case "equals" -> out.addProperty("b", box.equals(box2));
            case "hashCode" -> out.addProperty("i", box.hashCode());
            case "toString" -> out.addProperty("s", box.toString());
            case "getSize" -> out.addProperty("n", f64Bits(box.getSize()));
            case "getXsize" -> out.addProperty("n", f64Bits(box.getXsize()));
            case "getYsize" -> out.addProperty("n", f64Bits(box.getYsize()));
            case "getZsize" -> out.addProperty("n", f64Bits(box.getZsize()));
            case "distanceToSqrVec" -> out.addProperty("n", f64Bits(box.distanceToSqr(v)));
            case "distanceToSqrBox" -> out.addProperty("n", f64Bits(box.distanceToSqr(box2)));
            case "getCenter" -> putVec3(out, box.getCenter());
            case "getBottomCenter" -> putVec3(out, box.getBottomCenter());
            case "getMinPosition" -> putVec3(out, box.getMinPosition());
            case "getMaxPosition" -> putVec3(out, box.getMaxPosition());
            case "clip" -> {
                java.util.Optional<net.minecraft.world.phys.Vec3> hit = box.clip(v, v2);
                out.add("hit", hit.isPresent() ? vec3Bits(hit.get()) : com.google.gson.JsonNull.INSTANCE);
            }
            case "clipAll" -> {
                List<net.minecraft.world.phys.AABB> boxes = new ArrayList<>();
                for (JsonElement element : request.getAsJsonArray("boxes")) {
                    boxes.add(hexAabb(element));
                }
                putBlockHit(out, net.minecraft.world.phys.AABB.clip(boxes, v, v2, intPos(request.get("pos"))));
            }
            case "collidedAlongVector" -> {
                List<net.minecraft.world.phys.AABB> boxes = new ArrayList<>();
                for (JsonElement element : request.getAsJsonArray("boxes")) {
                    boxes.add(hexAabb(element));
                }
                out.addProperty("b", box.collidedAlongVector(v, boxes));
            }
            case "ofSize" -> out.add("box", aabbBits(net.minecraft.world.phys.AABB.ofSize(v, d.x, d.y, d.z)));
            default -> throw new IllegalStateException("unknown aabb fn " + fn);
        }
    }

    private static void putBlockHit(final JsonObject out, final net.minecraft.world.phys.BlockHitResult hit) {
        if (hit == null) {
            out.add("hit", com.google.gson.JsonNull.INSTANCE);
            return;
        }
        JsonObject object = new JsonObject();
        object.add("loc", vec3Bits(hit.getLocation()));
        object.addProperty("dir", hit.getDirection().name());
        object.addProperty("inside", hit.isInside());
        net.minecraft.core.BlockPos pos = hit.getBlockPos();
        JsonArray at = new JsonArray();
        at.add(pos.getX());
        at.add(pos.getY());
        at.add(pos.getZ());
        object.add("pos", at);
        out.add("hit", object);
    }

    private static net.minecraft.world.phys.shapes.BooleanOp booleanOp(final String name) throws Exception {
        return (net.minecraft.world.phys.shapes.BooleanOp) net.minecraft.world.phys.shapes.BooleanOp.class
            .getField(name).get(null);
    }

    private static net.minecraft.world.phys.shapes.VoxelShape buildShape(final JsonObject spec) throws Exception {
        String type = requiredString(spec, "t");
        switch (type) {
            case "box": {
                JsonArray b = spec.getAsJsonArray("b");
                return net.minecraft.world.phys.shapes.Shapes.box(
                    hexF64(b.get(0)), hexF64(b.get(1)), hexF64(b.get(2)), hexF64(b.get(3)), hexF64(b.get(4)), hexF64(b.get(5)));
            }
            case "create": {
                JsonArray b = spec.getAsJsonArray("b");
                return net.minecraft.world.phys.shapes.Shapes.create(
                    hexF64(b.get(0)), hexF64(b.get(1)), hexF64(b.get(2)), hexF64(b.get(3)), hexF64(b.get(4)), hexF64(b.get(5)));
            }
            case "block":
                return net.minecraft.world.phys.shapes.Shapes.block();
            case "empty":
                return net.minecraft.world.phys.shapes.Shapes.empty();
            case "infinity":
                return net.minecraft.world.phys.shapes.Shapes.INFINITY;
            case "join": {
                net.minecraft.world.phys.shapes.VoxelShape a = buildShape(spec.getAsJsonObject("a"));
                net.minecraft.world.phys.shapes.VoxelShape b = buildShape(spec.getAsJsonObject("b"));
                net.minecraft.world.phys.shapes.BooleanOp op = booleanOp(requiredString(spec, "op"));
                return spec.get("opt").getAsBoolean()
                    ? net.minecraft.world.phys.shapes.Shapes.join(a, b, op)
                    : net.minecraft.world.phys.shapes.Shapes.joinUnoptimized(a, b, op);
            }
            case "or": {
                net.minecraft.world.phys.shapes.VoxelShape first = buildShape(spec.getAsJsonObject("a"));
                JsonArray rest = spec.getAsJsonArray("rest");
                net.minecraft.world.phys.shapes.VoxelShape[] others = new net.minecraft.world.phys.shapes.VoxelShape[rest.size()];
                for (int i = 0; i < others.length; i++) {
                    others[i] = buildShape(rest.get(i).getAsJsonObject());
                }
                return net.minecraft.world.phys.shapes.Shapes.or(first, others);
            }
            case "move": {
                net.minecraft.world.phys.Vec3 d = hexVec3(spec.get("d"));
                return buildShape(spec.getAsJsonObject("s")).move(d.x, d.y, d.z);
            }
            case "optimize":
                return buildShape(spec.getAsJsonObject("s")).optimize();
            case "face":
                return buildShape(spec.getAsJsonObject("s")).getFaceShape(directionOf(spec.get("dir")));
            case "clamped":
                return ((ca.spottedleaf.moonrise.patches.collisions.shape.CollisionVoxelShape) buildShape(spec.getAsJsonObject("s")))
                    .moonrise$getFaceShapeClamped(directionOf(spec.get("dir")));
            case "single":
                return buildShape(spec.getAsJsonObject("s")).singleEncompassing();
            default:
                throw new IllegalStateException("unknown shape spec " + type);
        }
    }

    /** The cheap per-shape digest: emptiness, caches' booleans and the box list. */
    private static JsonObject shapeLite(final net.minecraft.world.phys.shapes.VoxelShape shape) {
        JsonObject lite = new JsonObject();
        lite.addProperty("empty", shape.isEmpty());
        ca.spottedleaf.moonrise.patches.collisions.shape.CollisionVoxelShape collision =
            (ca.spottedleaf.moonrise.patches.collisions.shape.CollisionVoxelShape) shape;
        lite.addProperty("full", collision.moonrise$isFullBlock());
        lite.addProperty("occludes", collision.moonrise$occludesFullBlock());
        JsonArray aabbs = new JsonArray();
        for (net.minecraft.world.phys.AABB box : shape.toAabbs()) {
            aabbs.add(aabbBits(box));
        }
        lite.add("aabbs", aabbs);
        return lite;
    }

    private static JsonObject shapeSummary(final net.minecraft.world.phys.shapes.VoxelShape shape) {
        JsonObject summary = shapeLite(shape);
        summary.add("bounds", shape.isEmpty() ? com.google.gson.JsonNull.INSTANCE : aabbBits(shape.bounds()));
        net.minecraft.world.phys.shapes.DiscreteVoxelShape grid = shape.shape;
        JsonArray size = new JsonArray();
        size.add(grid.getXSize());
        size.add(grid.getYSize());
        size.add(grid.getZSize());
        summary.add("size", size);
        JsonObject coords = new JsonObject();
        for (net.minecraft.core.Direction.Axis axis : net.minecraft.core.Direction.Axis.VALUES) {
            coords.add(axis.name(), bitsOf(shape.getCoords(axis).toDoubleArray()));
        }
        summary.add("coords", coords);
        StringBuilder voxels = new StringBuilder();
        for (int x = 0; x < grid.getXSize(); x++) {
            for (int y = 0; y < grid.getYSize(); y++) {
                for (int z = 0; z < grid.getZSize(); z++) {
                    voxels.append(grid.isFull(x, y, z) ? '1' : '0');
                }
            }
        }
        summary.addProperty("voxels", voxels.toString());
        JsonArray min = new JsonArray();
        JsonArray max = new JsonArray();
        for (net.minecraft.core.Direction.Axis axis : net.minecraft.core.Direction.Axis.VALUES) {
            min.add(f64Bits(shape.min(axis)));
            max.add(f64Bits(shape.max(axis)));
        }
        summary.add("min", min);
        summary.add("max", max);
        JsonObject faces = new JsonObject();
        JsonObject clamped = new JsonObject();
        for (net.minecraft.core.Direction direction : net.minecraft.core.Direction.values()) {
            faces.add(direction.name(), shapeLite(shape.getFaceShape(direction)));
            clamped.add(direction.name(), shapeLite(
                ((ca.spottedleaf.moonrise.patches.collisions.shape.CollisionVoxelShape) shape).moonrise$getFaceShapeClamped(direction)));
        }
        summary.add("faces", faces);
        summary.add("clamped", clamped);
        return summary;
    }

    private static void shapesShape(final JsonObject request, final JsonObject out) throws Exception {
        net.minecraft.world.phys.shapes.VoxelShape shape = buildShape(request.getAsJsonObject("spec"));
        out.add("summary", shapeSummary(shape));
        JsonObject query = request.getAsJsonObject("query");
        JsonObject answer = new JsonObject();
        switch (requiredString(query, "t")) {
            case "none" -> {
            }
            case "collide" -> answer.addProperty("v", f64Bits(shape.collide(
                axisOf(query.get("axis")), hexAabb(query.get("box")), hexF64(query.get("move")))));
            case "clip" -> putBlockHit(answer, shape.clip(hexVec3(query.get("from")), hexVec3(query.get("to")), intPos(query.get("pos"))));
            case "closest" -> {
                java.util.Optional<net.minecraft.world.phys.Vec3> point = shape.closestPointTo(hexVec3(query.get("p")));
                answer.add("p", point.isPresent() ? vec3Bits(point.get()) : com.google.gson.JsonNull.INSTANCE);
            }
            case "minmax" -> {
                net.minecraft.core.Direction.Axis axis = axisOf(query.get("axis"));
                double b = hexF64(query.get("b"));
                double c = hexF64(query.get("c"));
                answer.addProperty("min", f64Bits(shape.min(axis, b, c)));
                answer.addProperty("max", f64Bits(shape.max(axis, b, c)));
            }
            case "join" -> {
                net.minecraft.world.phys.shapes.VoxelShape other = buildShape(query.getAsJsonObject("spec2"));
                net.minecraft.world.phys.shapes.BooleanOp op = booleanOp(requiredString(query, "op"));
                answer.addProperty("nonEmpty", net.minecraft.world.phys.shapes.Shapes.joinIsNotEmpty(shape, other, op));
                answer.add("joined", shapeLite(net.minecraft.world.phys.shapes.Shapes.join(shape, other, op)));
                answer.add("unoptimized", shapeLite(net.minecraft.world.phys.shapes.Shapes.joinUnoptimized(shape, other, op)));
            }
            case "equal" -> {
                net.minecraft.world.phys.shapes.VoxelShape other = buildShape(query.getAsJsonObject("spec2"));
                answer.addProperty("equal", net.minecraft.world.phys.shapes.Shapes.equal(shape, other));
            }
            case "occludes" -> {
                net.minecraft.world.phys.shapes.VoxelShape other = buildShape(query.getAsJsonObject("spec2"));
                net.minecraft.core.Direction direction = directionOf(query.get("dir"));
                answer.addProperty("block", net.minecraft.world.phys.shapes.Shapes.blockOccludes(shape, other, direction));
                answer.addProperty("merged", net.minecraft.world.phys.shapes.Shapes.mergedFaceOccludes(shape, other, direction));
                answer.addProperty("face", net.minecraft.world.phys.shapes.Shapes.faceShapeOccludes(shape, other));
                answer.add("or", shapeLite(((ca.spottedleaf.moonrise.patches.collisions.shape.CollisionVoxelShape) shape)
                    .moonrise$orUnoptimized(other)));
            }
            default -> throw new IllegalStateException("unknown shape query " + requiredString(query, "t"));
        }
        out.add("query", answer);
    }
}
