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
import java.util.Comparator;
import java.util.HashMap;
import java.util.HashSet;
import java.util.HexFormat;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.function.Consumer;
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
    static final Gson GSON = new GsonBuilder().disableHtmlEscaping().create();
    private static final Gson PRETTY = new GsonBuilder().disableHtmlEscaping().setPrettyPrinting().create();
    /**
     * Repo-relative directory of the oracle's Java sources. The op source hash is the SHA-256 of
     * the listing {@code <sha256>  <file name>\n} of its {@code *.java} files in name order
     * (what {@code sha256sum src/*.java | sha256sum} prints), so an op may live in its own file.
     */
    private static final String OP_SOURCE = "tools/rivet-reference-oracle/src";
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

    /** One handler call that may throw. */
    @FunctionalInterface
    private interface Call {
        JsonObject run() throws Exception;
    }

    /**
     * Runs one worldgen op call with logging off. The first call opens the
     * vanilla pack and the DataConverter, which log to stdout ("Found new
     * data pack ...", "Initialising converters ..."); live mode speaks JSON
     * Lines there.
     */
    private static JsonObject quiet(final Call call) throws Exception {
        var level = org.apache.logging.log4j.LogManager.getRootLogger().getLevel();
        org.apache.logging.log4j.core.config.Configurator.setRootLevel(org.apache.logging.log4j.Level.OFF);
        try {
            return call.run();
        } finally {
            org.apache.logging.log4j.core.config.Configurator.setRootLevel(level);
        }
    }

    /** Runs an op on one request. {@code dec} receives optional human-readable decimals for float fields. */
    @FunctionalInterface
    interface Handler {
        JsonObject run(JsonObject request, JsonObject dec) throws Exception;
    }

    /**
     * {@code corpusCheck}, when present, runs over the parsed requests of an op's
     * corpus at record time and throws when the corpus does not close over the
     * registry the op sweeps (declared == registry size x seeds).
     */
    record Op(String name, List<String> resultKeys, Handler handler, Predicate<Exception> rejection,
              Consumer<List<JsonObject>> corpusCheck) {
        Op(String name, List<String> resultKeys, Handler handler, Predicate<Exception> rejection) {
            this(name, resultKeys, handler, rejection, null);
        }
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
        // The worldgen sweeps (VERIFICATION section 5 item 5). A Java exception
        // thrown by Paper's own feature or structure code is part of the
        // answer (`threw`); only a gap of the synthetic region (RegionFault)
        // or an unknown registry entry is unavailable evidence.
        register(ops, new Op(
            "feature.place",
            List.of("placed", "rng_next", "scheduled", "blocks", "threw"),
            (request, dec) -> quiet(() -> featurePlace(request, dec)),
            error -> false,
            RivetReferenceOracle::featurePlaceCorpusCheck
        ));
        register(ops, new Op(
            "carver.carve",
            List.of("chunks"),
            (request, dec) -> quiet(() -> carverCarve(request, dec)),
            error -> false,
            RivetReferenceOracle::carverCarveCorpusCheck
        ));
        register(ops, new Op(
            "structure.start",
            List.of("candidates", "starts"),
            (request, dec) -> quiet(() -> structureStart(request, dec)),
            error -> false,
            RivetReferenceOracle::structureStartCorpusCheck
        ));
        // `brigadier.dispatch`: a declarative dispatcher spec, one command
        // string, facets parse/execute, suggestions, usage, ambiguities.
        // Brigadier's own exceptions are data inside the result.
        register(ops, new Op(
            "brigadier.dispatch",
            BrigadierDispatchOp.RESULT_KEYS,
            (request, dec) -> BrigadierDispatchOp.run(request),
            BrigadierDispatchOp::isRejection
        ));
        for (Op op : EntityOps.ops()) {
            register(ops, op);
        }
        GameplayOps.register(ops);
        return Collections.unmodifiableMap(ops);
    }

    static void register(final Map<String, Op> ops, final Op op) {
        if (ops.putIfAbsent(op.name(), op) != null) {
            throw new IllegalStateException("duplicate op " + op.name());
        }
    }

    static List<String> keys(final List<String> head, final String... tail) {
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
            System.err.println("Usage: run.sh [--self-test | --record <corpus-dir> <out-dir>]");
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
            if (op.corpusCheck() != null) {
                List<JsonObject> requests = new ArrayList<>();
                for (String line : lines) {
                    requests.add(JsonParser.parseString(line).getAsJsonObject());
                }
                op.corpusCheck().accept(requests);
            }
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
        manifest.addProperty("op_source_sha256", sourceDigest(Path.of(requiredProperty("rivet.oracle.source"))));
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

    private static String sourceDigest(final Path sourceDir) throws Exception {
        List<Path> files;
        try (var listing = Files.list(sourceDir)) {
            files = listing.filter(path -> path.getFileName().toString().endsWith(".java")).sorted().toList();
        }
        StringBuilder entries = new StringBuilder();
        for (Path file : files) {
            entries.append(sha256(Files.readAllBytes(file))).append("  ").append(file.getFileName()).append('\n');
        }
        return sha256(entries.toString().getBytes(StandardCharsets.UTF_8));
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
    static final class ServerRegistries {
        /**
         * Loading logs to the process's stdout ("Found new data pack ...");
         * live mode speaks JSON Lines on that stream, so logging is switched
         * off until the registries and Paper's configuration are ready.
         */
        private static final org.apache.logging.log4j.Level LOG_LEVEL = org.apache.logging.log4j.LogManager.getRootLogger().getLevel();

        static {
            org.apache.logging.log4j.core.config.Configurator.setRootLevel(org.apache.logging.log4j.Level.OFF);
        }

        /** The layered registries behind {@link #ACCESS}, for ops that reload the data-pack layer on top. */
        static net.minecraft.core.LayeredRegistryAccess<net.minecraft.server.RegistryLayer> LAYERS;

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
                    LAYERS = layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN, worldgen);
                    var access = LAYERS.compositeAccess().freeze();
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

    // ---- worldgen sweep: feature.place / carver.carve / structure.start ----
    //
    // VERIFICATION section 5 item 5. Every op is iterated over the whole
    // vanilla registry it sweeps (`corpusCheck` makes `--record` fail on a
    // corpus that omits an entry) for the same eight world seeds, and answers
    // with the geometry Rust must reproduce, never a hash: the changed blocks of
    // a feature placed into a fixed synthetic region, the carving mask and
    // changed blocks of a carver over a chunk grid, the structure starts (pieces
    // with type, bounding box, rotation, mirror and generation depth) of a
    // structure set over a chunk grid.

    /** The eight fixed world seeds every worldgen sweep op runs for. */
    private static final long[] WORLDGEN_SEEDS = {
        0L, 1L, 42L, 1337L, -1L, 8675309L, 4611686018427387905L, Long.MIN_VALUE
    };

    /** The proxy region does not implement a method a feature called: a harness gap, never a Paper verdict. */
    private static final class RegionFault extends RuntimeException {
        RegionFault(final String message) {
            super(message);
        }
    }

    private static long requiredLong(final JsonObject request, final String property) {
        JsonElement value = request.get(property);
        if (value == null || !value.isJsonPrimitive() || !value.getAsJsonPrimitive().isNumber()) {
            throw new IllegalArgumentException("property '" + property + "' must be an integer");
        }
        return value.getAsLong();
    }

    private static <T> net.minecraft.core.Holder.Reference<T> registryEntry(
        final net.minecraft.resources.ResourceKey<? extends net.minecraft.core.Registry<T>> registry, final String key
    ) {
        return ServerRegistries.ACCESS.lookupOrThrow(registry)
            .get(net.minecraft.resources.ResourceKey.create(registry, net.minecraft.resources.Identifier.parse(key)))
            .orElseThrow(() -> new IllegalArgumentException("unknown " + registry.identifier() + " entry " + key));
    }

    /** Every key of a registry, sorted, as the corpus must cover them. */
    private static <T> List<String> registryKeys(final net.minecraft.resources.ResourceKey<? extends net.minecraft.core.Registry<T>> registry) {
        List<String> keys = new ArrayList<>();
        ServerRegistries.ACCESS.lookupOrThrow(registry).listElementIds().forEach(key -> keys.add(key.identifier().toString()));
        Collections.sort(keys);
        return keys;
    }

    /**
     * Record-time closure: the corpus must hold exactly one input for every
     * (registry key, seed) pair of the registry the op sweeps, so the declared
     * count is the registry size times the eight seeds on Paper's own registry.
     */
    private static void requireSweep(final String op, final List<JsonObject> corpus, final String keyProperty, final List<String> registryKeys) {
        Set<String> seen = new HashSet<>();
        for (JsonObject request : corpus) {
            String key = requiredString(request, keyProperty);
            long seed = requiredLong(request, "seed");
            if (!seen.add(key + "\0" + seed)) {
                throw new IllegalStateException(op + ": duplicate (" + key + ", " + seed + ")");
            }
        }
        for (String key : registryKeys) {
            for (long seed : WORLDGEN_SEEDS) {
                if (!seen.contains(key + "\0" + seed)) {
                    throw new IllegalStateException(op + ": the corpus omits registry entry " + key + " for seed " + seed);
                }
            }
        }
        if (seen.size() != registryKeys.size() * WORLDGEN_SEEDS.length) {
            throw new IllegalStateException(op + ": the corpus names entries outside the registry (" + seen.size() + " inputs for "
                + registryKeys.size() + " entries x " + WORLDGEN_SEEDS.length + " seeds)");
        }
    }

    private static void featurePlaceCorpusCheck(final List<JsonObject> corpus) {
        requireSweep("feature.place", corpus, "feature", registryKeys(net.minecraft.core.registries.Registries.CONFIGURED_FEATURE));
    }

    private static void carverCarveCorpusCheck(final List<JsonObject> corpus) {
        requireSweep("carver.carve", corpus, "carver", registryKeys(net.minecraft.core.registries.Registries.CONFIGURED_CARVER));
    }

    private static void structureStartCorpusCheck(final List<JsonObject> corpus) {
        requireSweep("structure.start", corpus, "structure_set", registryKeys(net.minecraft.core.registries.Registries.STRUCTURE_SET));
    }

    /** Human-readable account of a Throwable for the recorded `dec` object (never compared). */
    private static String throwableSummary(final Throwable error) {
        StringBuilder text = new StringBuilder(error.toString());
        StackTraceElement[] frames = error.getStackTrace();
        for (int index = 0; index < Math.min(frames.length, 6); index++) {
            text.append(" <- ").append(frames[index]);
        }
        return text.toString();
    }

    private static net.minecraft.world.level.block.state.BlockState defaultState(final net.minecraft.world.level.block.Block block) {
        return block.defaultBlockState();
    }

    // The frame every sweep op shares: the build-height window and the
    // `Block.getId` identity of a state (what the Rust `BlockState::id` returns).
    private static final int MIN_Y = -64;
    private static final int HEIGHT = 384;

    /**
     * The synthetic region profile a `feature.place` request names. The region
     * is an unbounded plane: columns x, z in any range read the same base fill,
     * y outside [-64, 319] is void air and cannot be written. Profiles (base fill
     * by y; every profile is horizontally uniform except `cave`):
     * <ul>
     * <li>`stone` — stone for y <= 63, air above. Origin (8, 40, 8).
     * <li>`surface` — stone y <= 60, dirt 61..62, grass_block 63, air above. Origin (8, 64, 8).
     * <li>`cave` — as `stone`, with an air cavity x, z in [-8, 24], y in [30, 44]. Origin (8, 30, 8).
     * <li>`ocean` — stone y <= 43, sand 44, water 45..62, air above. Origin (8, 45, 8).
     * <li>`nether` — netherrack for y <= 40 and y >= 120, air between; sea level 32. Origin (8, 41, 8).
     * <li>`nether_ceiling` — as `nether`, origin (8, 119, 8) (under the ceiling).
     * <li>`end` — end_stone for y <= 62, air above; sea level 0. Origin (8, 63, 8).
     * <li>`void` — air everywhere. Origin (8, 64, 8).
     * </ul>
     * The biome is plains (`nether_wastes` in the nether profiles, `the_end` in
     * `end`, `the_void` in `void`) at every position; block and sky light are 0.
     */
    private record Profile(String name, int ox, int oy, int oz, String biome, int seaLevel, String settings) {
        net.minecraft.world.level.block.state.BlockState base(final int x, final int y, final int z) {
            if (y < MIN_Y || y >= MIN_Y + HEIGHT) {
                return defaultState(net.minecraft.world.level.block.Blocks.VOID_AIR);
            }
            return defaultState(switch (name) {
                case "stone" -> y <= 63 ? net.minecraft.world.level.block.Blocks.STONE : net.minecraft.world.level.block.Blocks.AIR;
                case "surface" -> y <= 60 ? net.minecraft.world.level.block.Blocks.STONE
                    : y <= 62 ? net.minecraft.world.level.block.Blocks.DIRT
                    : y == 63 ? net.minecraft.world.level.block.Blocks.GRASS_BLOCK : net.minecraft.world.level.block.Blocks.AIR;
                case "cave" -> x >= -8 && x <= 24 && z >= -8 && z <= 24 && y >= 30 && y <= 44
                    ? net.minecraft.world.level.block.Blocks.AIR
                    : y <= 63 ? net.minecraft.world.level.block.Blocks.STONE : net.minecraft.world.level.block.Blocks.AIR;
                case "ocean" -> y <= 43 ? net.minecraft.world.level.block.Blocks.STONE
                    : y == 44 ? net.minecraft.world.level.block.Blocks.SAND
                    : y <= 62 ? net.minecraft.world.level.block.Blocks.WATER : net.minecraft.world.level.block.Blocks.AIR;
                case "nether", "nether_ceiling" -> y <= 40 || y >= 120
                    ? net.minecraft.world.level.block.Blocks.NETHERRACK : net.minecraft.world.level.block.Blocks.AIR;
                case "end" -> y <= 62 ? net.minecraft.world.level.block.Blocks.END_STONE : net.minecraft.world.level.block.Blocks.AIR;
                case "void" -> net.minecraft.world.level.block.Blocks.AIR;
                default -> throw new IllegalArgumentException("unknown region profile " + name);
            });
        }
    }

    private static final Map<String, Profile> PROFILES = profiles();

    private static Map<String, Profile> profiles() {
        Map<String, Profile> all = new LinkedHashMap<>();
        for (Profile profile : List.of(
            new Profile("stone", 8, 40, 8, "minecraft:plains", 63, "minecraft:overworld"),
            new Profile("surface", 8, 64, 8, "minecraft:plains", 63, "minecraft:overworld"),
            new Profile("cave", 8, 30, 8, "minecraft:plains", 63, "minecraft:overworld"),
            new Profile("ocean", 8, 45, 8, "minecraft:plains", 63, "minecraft:overworld"),
            new Profile("nether", 8, 41, 8, "minecraft:nether_wastes", 32, "minecraft:nether"),
            new Profile("nether_ceiling", 8, 119, 8, "minecraft:nether_wastes", 32, "minecraft:nether"),
            new Profile("end", 8, 63, 8, "minecraft:the_end", 0, "minecraft:end"),
            new Profile("void", 8, 64, 8, "minecraft:the_void", 63, "minecraft:overworld")
        )) {
            all.put(profile.name(), profile);
        }
        return Collections.unmodifiableMap(all);
    }

    private static Profile profile(final String name) {
        Profile profile = PROFILES.get(name);
        if (profile == null) {
            throw new IllegalArgumentException("unknown region profile " + name);
        }
        return profile;
    }

    /** A fresh generator over the profile's noise settings: the sea level and build window features read through it. */
    private static net.minecraft.world.level.chunk.ChunkGenerator generatorFor(final Profile profile) {
        var biomes = ServerRegistries.ACCESS.lookupOrThrow(net.minecraft.core.registries.Registries.BIOME);
        var settings = registryEntry(net.minecraft.core.registries.Registries.NOISE_SETTINGS, profile.settings());
        return new net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator(
            new net.minecraft.world.level.biome.FixedBiomeSource(biomes.getOrThrow(
                net.minecraft.resources.ResourceKey.create(net.minecraft.core.registries.Registries.BIOME, net.minecraft.resources.Identifier.parse(profile.biome())))),
            settings
        );
    }

    private static void setField(final Object target, final String name, final Object value) throws ReflectiveOperationException {
        setField(target.getClass(), target, name, value);
    }

    private static void setField(final Class<?> owner, final Object target, final String name, final Object value) throws ReflectiveOperationException {
        var field = owner.getDeclaredField(name);
        field.setAccessible(true);
        field.set(target, value);
    }

    /**
     * Getter-only class shells (never constructed, no simulation method runs on
     * them) for what features reach through {@code level.getLevel()}: the
     * server's {@code StructureTemplateManager} (fossils, sulfur springs) and the
     * chunk generator behind the chunk source (the pale moss decorator).
     */
    private static net.minecraft.server.level.ServerLevel levelShell(final net.minecraft.world.level.chunk.ChunkGenerator generator) {
        try {
            var unsafeField = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
            unsafeField.setAccessible(true);
            var unsafe = (sun.misc.Unsafe) unsafeField.get(null);
            var level = (net.minecraft.server.level.ServerLevel) unsafe.allocateInstance(net.minecraft.server.level.ServerLevel.class);
            var server = (net.minecraft.server.MinecraftServer) unsafe.allocateInstance(net.minecraft.server.dedicated.DedicatedServer.class);
            var cache = (net.minecraft.server.level.ServerChunkCache) unsafe.allocateInstance(net.minecraft.server.level.ServerChunkCache.class);
            var chunkMap = (net.minecraft.server.level.ChunkMap) unsafe.allocateInstance(net.minecraft.server.level.ChunkMap.class);
            setField(net.minecraft.server.MinecraftServer.class, server, "structureTemplateManager", StructureWorld.TEMPLATES);
            setField(level, "server", server);
            setField(level, "chunkSource", cache);
            setField(cache, "chunkMap", chunkMap);
            setField(chunkMap, "worldGenContext", new net.minecraft.world.level.chunk.status.WorldGenContext(level, generator, null, null, Runnable::run, null));
            return level;
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("cannot allocate the ServerLevel shell", error);
        }
    }

    /** The generated base fill of one chunk of a profile, cached: ProtoChunks are copied from it, never rebuilt. */
    private static final Map<String, net.minecraft.world.level.chunk.ProtoChunk> TEMPLATES = new HashMap<>();

    private static net.minecraft.world.level.chunk.ProtoChunk template(final Profile profile, final int chunkX, final int chunkZ) {
        return TEMPLATES.computeIfAbsent(profile.name() + ":" + chunkX + ":" + chunkZ, key -> {
            var pos = new net.minecraft.world.level.ChunkPos(chunkX, chunkZ);
            var chunk = new net.minecraft.world.level.chunk.ProtoChunk(pos, net.minecraft.world.level.chunk.UpgradeData.EMPTY,
                net.minecraft.world.level.LevelHeightAccessor.create(MIN_Y, HEIGHT),
                net.minecraft.world.level.chunk.PalettedContainerFactory.create(ServerRegistries.ACCESS), null);
            var cursor = new net.minecraft.core.BlockPos.MutableBlockPos();
            var air = defaultState(net.minecraft.world.level.block.Blocks.AIR);
            for (int y = MIN_Y; y < MIN_Y + HEIGHT; y++) {
                for (int x = 0; x < 16; x++) {
                    for (int z = 0; z < 16; z++) {
                        var state = profile.base(pos.getMinBlockX() + x, y, pos.getMinBlockZ() + z);
                        if (state != air) {
                            chunk.setBlockState(cursor.set(pos.getMinBlockX() + x, y, pos.getMinBlockZ() + z), state, 0);
                        }
                    }
                }
            }
            var biome = ServerRegistries.ACCESS.lookupOrThrow(net.minecraft.core.registries.Registries.BIOME).getOrThrow(
                net.minecraft.resources.ResourceKey.create(net.minecraft.core.registries.Registries.BIOME, net.minecraft.resources.Identifier.parse(profile.biome())));
            chunk.fillBiomesFromNoise((quartX, quartY, quartZ, sampler) -> biome, net.minecraft.world.level.biome.Climate.empty());
            return chunk;
        });
    }

    /**
     * The region a feature is placed into: a 3 x 3 window of {@code ProtoChunk}s
     * (status CARVERS, the centre one with the final heightmaps primed) around
     * chunk (0, 0), each a copy of the profile's base fill, behind a
     * {@code WorldGenRegion}-like facade. Blocks, heights and bulk section access
     * go through the real chunks; {@code ensureCanWrite} is the write radius 1
     * of the FEATURES step (a write more than one chunk from (0, 0) returns
     * false) and a read outside the window is the crash `Requested chunk
     * unavailable during world generation`. A write is a plain store: no
     * neighbour updates, no light, no POI, no entities; the block entity a write
     * implies is created on the spot (the region materializes Paper's DUMMY
     * entity lazily, which is unobservable). Scheduled ticks are recorded.
     */
    private static final class Region {
        final Profile profile;
        final long seed;
        final net.minecraft.world.level.chunk.ProtoChunk[] chunks = new net.minecraft.world.level.chunk.ProtoChunk[9];
        final Map<net.minecraft.core.BlockPos, net.minecraft.world.level.block.entity.BlockEntity> blockEntities = new HashMap<>();
        long subTick;
        final net.minecraft.util.RandomSource worldRandom = new net.minecraft.world.level.levelgen.XoroshiroRandomSource(99L);
        final net.minecraft.core.Holder<net.minecraft.world.level.biome.Biome> biome;
        final net.minecraft.server.level.ServerLevel shell;

        Region(final Profile profile, final long seed, final net.minecraft.world.level.chunk.ChunkGenerator generator) {
            this.profile = profile;
            this.seed = seed;
            this.shell = levelShell(generator);
            this.biome = ServerRegistries.ACCESS.lookupOrThrow(net.minecraft.core.registries.Registries.BIOME).getOrThrow(
                net.minecraft.resources.ResourceKey.create(net.minecraft.core.registries.Registries.BIOME, net.minecraft.resources.Identifier.parse(profile.biome())));
            for (int cx = -1; cx <= 1; cx++) {
                for (int cz = -1; cz <= 1; cz++) {
                    var source = template(profile, cx, cz);
                    var sections = new net.minecraft.world.level.chunk.LevelChunkSection[source.getSections().length];
                    for (int index = 0; index < sections.length; index++) {
                        sections[index] = source.getSections()[index].copy();
                    }
                    chunks[(cx + 1) * 3 + cz + 1] = new net.minecraft.world.level.chunk.ProtoChunk(source.getPos(),
                        net.minecraft.world.level.chunk.UpgradeData.EMPTY, sections,
                        new net.minecraft.world.ticks.ProtoChunkTicks<>(), new net.minecraft.world.ticks.ProtoChunkTicks<>(),
                        net.minecraft.world.level.LevelHeightAccessor.create(MIN_Y, HEIGHT),
                        net.minecraft.world.level.chunk.PalettedContainerFactory.create(ServerRegistries.ACCESS), null);
                    chunks[(cx + 1) * 3 + cz + 1].setPersistedStatus(net.minecraft.world.level.chunk.status.ChunkStatus.CARVERS);
                }
            }
            // `ChunkStatusTasks.generateFeatures` primes the final heightmaps of the generating chunk before decoration.
            net.minecraft.world.level.levelgen.Heightmap.primeHeightmaps(chunks[4], java.util.EnumSet.of(
                net.minecraft.world.level.levelgen.Heightmap.Types.MOTION_BLOCKING,
                net.minecraft.world.level.levelgen.Heightmap.Types.MOTION_BLOCKING_NO_LEAVES,
                net.minecraft.world.level.levelgen.Heightmap.Types.OCEAN_FLOOR,
                net.minecraft.world.level.levelgen.Heightmap.Types.WORLD_SURFACE));
        }

        net.minecraft.world.level.chunk.ProtoChunk chunk(final int chunkX, final int chunkZ) {
            if (Math.abs(chunkX) > 1 || Math.abs(chunkZ) > 1) {
                var failure = new IllegalStateException("Requested chunk unavailable during world generation");
                throw new net.minecraft.ReportedException(net.minecraft.CrashReport.forThrowable(failure, "Exception generating new chunk"));
            }
            return chunks[(chunkX + 1) * 3 + chunkZ + 1];
        }

        net.minecraft.world.level.chunk.ProtoChunk chunk(final net.minecraft.core.BlockPos pos) {
            return chunk(pos.getX() >> 4, pos.getZ() >> 4);
        }

        net.minecraft.world.level.block.state.BlockState get(final net.minecraft.core.BlockPos pos) {
            return chunk(pos).getBlockState(pos);
        }

        boolean canWrite(final net.minecraft.core.BlockPos pos) {
            return Math.abs(pos.getX() >> 4) <= 1 && Math.abs(pos.getZ() >> 4) <= 1;
        }

        boolean set(final net.minecraft.core.BlockPos position, final net.minecraft.world.level.block.state.BlockState state, final int flags) {
            net.minecraft.core.BlockPos pos = position.immutable();
            if (!canWrite(pos)) {
                return false;
            }
            var chunk = chunk(pos);
            var old = chunk.setBlockState(pos, state, flags);
            if (state.hasBlockEntity()) {
                if (old != null && old.getBlock() != state.getBlock()) {
                    blockEntities.remove(pos);
                }
                if (!blockEntities.containsKey(pos)) {
                    var entity = ((net.minecraft.world.level.block.EntityBlock) state.getBlock()).newBlockEntity(pos, state);
                    if (entity != null) {
                        blockEntities.put(pos, entity);
                    }
                }
            } else if (old != null && old.hasBlockEntity()) {
                blockEntities.remove(pos);
            }
            return true;
        }

        @SuppressWarnings("unchecked")
        net.minecraft.world.level.WorldGenLevel level() {
            return (net.minecraft.world.level.WorldGenLevel) java.lang.reflect.Proxy.newProxyInstance(
                net.minecraft.world.level.WorldGenLevel.class.getClassLoader(),
                new Class<?>[] {net.minecraft.world.level.WorldGenLevel.class},
                (proxy, method, args) -> {
                    switch (method.getName()) {
                        case "getBlockState": return get((net.minecraft.core.BlockPos) args[0]);
                        case "getFluidState": return get((net.minecraft.core.BlockPos) args[0]).getFluidState();
                        case "isWaterAt": return get((net.minecraft.core.BlockPos) args[0]).getFluidState().is(net.minecraft.tags.FluidTags.WATER);
                        case "isEmptyBlock": return get((net.minecraft.core.BlockPos) args[0]).isAir();
                        case "ensureCanWrite": return canWrite((net.minecraft.core.BlockPos) args[0]);
                        case "isStateAtPosition":
                            return ((Predicate<net.minecraft.world.level.block.state.BlockState>) args[1]).test(get((net.minecraft.core.BlockPos) args[0]));
                        case "isFluidAtPosition":
                            return ((Predicate<net.minecraft.world.level.material.FluidState>) args[1]).test(get((net.minecraft.core.BlockPos) args[0]).getFluidState());
                        case "setBlock": return set((net.minecraft.core.BlockPos) args[0], (net.minecraft.world.level.block.state.BlockState) args[1], (int) args[2]);
                        // WorldGenRegion.removeBlock: setBlock(pos, AIR, UPDATE_ALL), whatever the old state held.
                        case "removeBlock":
                            return set((net.minecraft.core.BlockPos) args[0],
                                defaultState(net.minecraft.world.level.block.Blocks.AIR),
                                net.minecraft.world.level.block.Block.UPDATE_ALL);
                        case "destroyBlock": throw new RegionFault("destroyBlock");
                        case "getBlockEntity": {
                            var entity = blockEntities.get((net.minecraft.core.BlockPos) args[0]);
                            return args.length == 1 ? entity
                                : entity != null && entity.getType() == args[1] ? Optional.of(entity) : Optional.empty();
                        }
                        case "scheduleTick": {
                            net.minecraft.core.BlockPos p = ((net.minecraft.core.BlockPos) args[0]).immutable();
                            if (args[1] instanceof net.minecraft.world.level.block.Block block) {
                                chunk(p).getBlockTicks().schedule(new net.minecraft.world.ticks.ScheduledTick<>(block, p, (int) args[2], subTick++));
                            } else {
                                chunk(p).getFluidTicks().schedule(new net.minecraft.world.ticks.ScheduledTick<>(
                                    (net.minecraft.world.level.material.Fluid) args[1], p, (int) args[2], subTick++));
                            }
                            return null;
                        }
                        case "getRandom": return worldRandom;
                        case "getSeed": return seed;
                        case "registryAccess": return ServerRegistries.ACCESS;
                        case "getLevel": case "getMinecraftWorld": return shell;
                        case "getHeight":
                            if (args == null || args.length == 0) {
                                return HEIGHT;
                            }
                            if (args.length == 2) {
                                var at = (net.minecraft.core.BlockPos) args[1];
                                return chunk(at).getHeight((net.minecraft.world.level.levelgen.Heightmap.Types) args[0], at.getX() & 15, at.getZ() & 15) + 1;
                            }
                            return chunk((int) args[1] >> 4, (int) args[2] >> 4)
                                .getHeight((net.minecraft.world.level.levelgen.Heightmap.Types) args[0], (int) args[1] & 15, (int) args[2] & 15) + 1;
                        case "getMinY": return MIN_Y;
                        case "getSeaLevel": return profile.seaLevel();
                        case "getBiome": case "getNoiseBiome": case "getUncachedNoiseBiome": return biome;
                        case "getBrightness": case "getRawBrightness": case "getMaxLocalRawBrightness": return 0;
                        case "canSeeSky": return false;
                        case "getSkyDarken": return 0;
                        case "isClientSide": return false;
                        case "hasChunk": return Math.abs((int) args[0]) <= 1 && Math.abs((int) args[1]) <= 1;
                        case "getChunk": {
                            if (args[0] instanceof net.minecraft.core.BlockPos p) {
                                return chunk(p);
                            }
                            return chunk((int) args[0], (int) args[1]);
                        }
                        case "getEntities": case "getEntitiesOfClass": return List.of();
                        case "nextSubTickCount": return 0L;
                        case "setCurrentlyGenerating": return null;
                        case "levelEvent": case "gameEvent": case "playSound": case "addParticle": case "blockUpdated": return null;
                        case "toString": return "RivetRegion[" + profile.name() + "]";
                        case "hashCode": return System.identityHashCode(proxy);
                        case "equals": return proxy == args[0];
                        default:
                            if (method.isDefault()) {
                                return java.lang.reflect.InvocationHandler.invokeDefault(proxy, method, args);
                            }
                            throw new RegionFault(method.toString());
                    }
                });
        }
    }

    /** `feature.place`: place one configured feature into the profile's region with the seed's random source. */
    private static JsonObject featurePlace(final JsonObject request, final JsonObject dec) {
        String key = requiredString(request, "feature");
        long seed = requiredLong(request, "seed");
        Profile profile = profile(requiredString(request, "region"));
        var configured = registryEntry(net.minecraft.core.registries.Registries.CONFIGURED_FEATURE, key).value();
        var generator = generatorFor(profile);
        Region region = new Region(profile, seed, generator);
        var origin = new net.minecraft.core.BlockPos(profile.ox(), profile.oy(), profile.oz());
        var random = new net.minecraft.world.level.levelgen.XoroshiroRandomSource(seed);
        boolean placed = false;
        String threw = null;
        try {
            placed = configured.place(region.level(), generator, random, origin);
        } catch (RegionFault fault) {
            throw fault;
        } catch (RuntimeException | StackOverflowError error) {
            threw = error.getClass().getName();
            dec.addProperty("threw", throwableSummary(error));
        }
        JsonObject result = new JsonObject();
        result.addProperty("placed", placed);
        result.addProperty("rng_next", random.nextLong());
        List<Object[]> ticks = new ArrayList<>();
        for (var chunk : region.chunks) {
            for (var tick : ((net.minecraft.world.ticks.ProtoChunkTicks<net.minecraft.world.level.block.Block>) chunk.getBlockTicks()).scheduledTicks()) {
                ticks.add(new Object[] {tick.pos().getX(), tick.pos().getY(), tick.pos().getZ(), "block",
                    net.minecraft.core.registries.BuiltInRegistries.BLOCK.getKey(tick.type()).toString()});
            }
            for (var tick : ((net.minecraft.world.ticks.ProtoChunkTicks<net.minecraft.world.level.material.Fluid>) chunk.getFluidTicks()).scheduledTicks()) {
                ticks.add(new Object[] {tick.pos().getX(), tick.pos().getY(), tick.pos().getZ(), "fluid",
                    net.minecraft.core.registries.BuiltInRegistries.FLUID.getKey(tick.type()).toString()});
            }
        }
        ticks.sort(Comparator.<Object[]>comparingInt(t -> (int) t[0]).thenComparingInt(t -> (int) t[1]).thenComparingInt(t -> (int) t[2])
            .thenComparing(t -> (String) t[3]).thenComparing(t -> (String) t[4]));
        JsonArray scheduled = new JsonArray();
        for (Object[] tick : ticks) {
            JsonArray entry = new JsonArray();
            entry.add((int) tick[0]);
            entry.add((int) tick[1]);
            entry.add((int) tick[2]);
            entry.add((String) tick[3]);
            entry.add((String) tick[4]);
            scheduled.add(entry);
        }
        result.add("scheduled", scheduled);
        JsonArray blocks = new JsonArray();
        JsonObject names = new JsonObject();
        var cursor = new net.minecraft.core.BlockPos.MutableBlockPos();
        for (int x = -16; x < 32; x++) {
            for (int y = MIN_Y; y < MIN_Y + HEIGHT; y++) {
                for (int z = -16; z < 32; z++) {
                    cursor.set(x, y, z);
                    var state = region.get(cursor);
                    if (state != profile.base(x, y, z)) {
                        JsonArray entry = new JsonArray();
                        entry.add(x);
                        entry.add(y);
                        entry.add(z);
                        int id = net.minecraft.world.level.block.Block.getId(state);
                        entry.add(id);
                        blocks.add(entry);
                        names.addProperty(Integer.toString(id), net.minecraft.commands.arguments.blocks.BlockStateParser.serialize(state));
                    }
                }
            }
        }
        result.add("blocks", blocks);
        if (threw != null) {
            result.addProperty("threw", true);
        }
        dec.add("states", names);
        return result;
    }

    // ---- carver.carve ----

    /** The 3 x 3 grid of target chunks `carver.carve` carves, centred on the origin chunk. */
    private static final int CARVER_GRID = 1;

    /** `CarvingContext` with the surface-system replacement switched off (the region has no surface rule). */
    private static final class PlainCarvingContext extends net.minecraft.world.level.levelgen.carver.CarvingContext {
        PlainCarvingContext(final net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator generator, final net.minecraft.world.level.LevelHeightAccessor height) {
            super(generator, ServerRegistries.ACCESS, height, null, null, null, null);
        }

        @Override
        public Optional<net.minecraft.world.level.block.state.BlockState> topMaterial(
            final java.util.function.Function<net.minecraft.core.BlockPos, net.minecraft.core.Holder<net.minecraft.world.level.biome.Biome>> biomeGetter,
            final net.minecraft.world.level.chunk.ChunkAccess chunk, final net.minecraft.core.BlockPos pos, final boolean underFluid
        ) {
            return Optional.empty();
        }
    }

    private static JsonArray runs(final java.util.BitSet bits) {
        JsonArray runs = new JsonArray();
        int start = bits.nextSetBit(0);
        while (start >= 0) {
            int end = bits.nextClearBit(start);
            JsonArray run = new JsonArray();
            run.add(start);
            run.add(end - start);
            runs.add(run);
            start = bits.nextSetBit(end);
        }
        return runs;
    }

    /**
     * `carver.carve`: carve every chunk of a 3 x 3 grid with one configured
     * carver, the way `NoiseBasedChunkGenerator.applyCarvers` loops: for each
     * source chunk within 8 of the target, `setLargeFeatureSeed(seed + index,
     * x, z)` on a `WorldgenRandom(LegacyRandomSource)`, then `isStartChunk`
     * and `carve`. The target chunk starts as stone for y <= 63 and air above;
     * the aquifer is `Aquifer.createDisabled` over the global fluid picker
     * (lava below y -54, the overworld default fluid below sea level 63), so
     * carved blocks below the sea level fill with water; no surface
     * replacement. Result per target chunk: starts (source chunks that were
     * start chunks), carved (carve returned true), mask (set bits of the
     * `CarvingMask`, index x | z << 4 | (y + 64) << 8, as [start, length]
     * runs) and changed ([start, length, state id] runs of blocks that differ
     * from the base fill).
     */
    private static JsonObject carverCarve(final JsonObject request, final JsonObject dec) {
        String key = requiredString(request, "carver");
        long seed = requiredLong(request, "seed");
        int index = (int) requiredLong(request, "index");
        var configured = registryEntry(net.minecraft.core.registries.Registries.CONFIGURED_CARVER, key).value();
        var access = ServerRegistries.ACCESS;
        var biomes = access.lookupOrThrow(net.minecraft.core.registries.Registries.BIOME);
        var plains = biomes.getOrThrow(net.minecraft.resources.ResourceKey.create(net.minecraft.core.registries.Registries.BIOME, net.minecraft.resources.Identifier.parse("minecraft:plains")));
        var settings = registryEntry(net.minecraft.core.registries.Registries.NOISE_SETTINGS, "minecraft:overworld");
        var generator = new net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator(new net.minecraft.world.level.biome.FixedBiomeSource(plains), settings);
        var height = net.minecraft.world.level.LevelHeightAccessor.create(MIN_Y, HEIGHT);
        var factory = net.minecraft.world.level.chunk.PalettedContainerFactory.create(access);
        var stone = defaultState(net.minecraft.world.level.block.Blocks.STONE);
        var lava = defaultState(net.minecraft.world.level.block.Blocks.LAVA);
        var water = defaultState(net.minecraft.world.level.block.Blocks.WATER);
        int seaLevel = settings.value().seaLevel();
        var lavaStatus = new net.minecraft.world.level.levelgen.Aquifer.FluidStatus(-54, lava);
        var seaStatus = new net.minecraft.world.level.levelgen.Aquifer.FluidStatus(seaLevel, settings.value().defaultFluid());
        var aquifer = net.minecraft.world.level.levelgen.Aquifer.createDisabled((x, y, z) -> y < Math.min(-54, seaLevel) ? lavaStatus : seaStatus);
        var context = new PlainCarvingContext(generator, height);
        var random = new net.minecraft.world.level.levelgen.WorldgenRandom(new net.minecraft.world.level.levelgen.LegacyRandomSource(0L));
        JsonArray chunks = new JsonArray();
        for (int cx = -CARVER_GRID; cx <= CARVER_GRID; cx++) {
            for (int cz = -CARVER_GRID; cz <= CARVER_GRID; cz++) {
                var chunkPos = new net.minecraft.world.level.ChunkPos(cx, cz);
                var chunk = new net.minecraft.world.level.chunk.ProtoChunk(chunkPos, net.minecraft.world.level.chunk.UpgradeData.EMPTY, height, factory, null);
                var cursor = new net.minecraft.core.BlockPos.MutableBlockPos();
                for (int y = MIN_Y; y <= 63; y++) {
                    for (int x = 0; x < 16; x++) {
                        for (int z = 0; z < 16; z++) {
                            chunk.setBlockState(cursor.set(chunkPos.getMinBlockX() + x, y, chunkPos.getMinBlockZ() + z), stone, 0);
                        }
                    }
                }
                var mask = new net.minecraft.world.level.chunk.CarvingMask(HEIGHT, MIN_Y);
                int starts = 0;
                int carved = 0;
                for (int dx = -8; dx <= 8; dx++) {
                    for (int dz = -8; dz <= 8; dz++) {
                        var source = new net.minecraft.world.level.ChunkPos(cx + dx, cz + dz);
                        random.setLargeFeatureSeed(seed + index, source.x(), source.z());
                        if (configured.isStartChunk(random)) {
                            starts++;
                            if (configured.carve(context, chunk, pos -> plains, random, aquifer, source, mask)) {
                                carved++;
                            }
                        }
                    }
                }
                java.util.BitSet maskBits = java.util.BitSet.valueOf(mask.toArray());
                JsonArray changed = new JsonArray();
                int runStart = -1;
                int runLength = 0;
                int runState = 0;
                for (int at = 0; at <= 256 * HEIGHT; at++) {
                    int id = -1;
                    if (at < 256 * HEIGHT) {
                        var state = chunk.getBlockState(cursor.set(chunkPos.getMinBlockX() + (at & 15), MIN_Y + (at >> 8), chunkPos.getMinBlockZ() + (at >> 4 & 15)));
                        var base = MIN_Y + (at >> 8) <= 63 ? stone : defaultState(net.minecraft.world.level.block.Blocks.AIR);
                        id = state == base ? -1 : net.minecraft.world.level.block.Block.getId(state);
                    }
                    if (runLength > 0 && id != runState) {
                        JsonArray run = new JsonArray();
                        run.add(runStart);
                        run.add(runLength);
                        run.add(runState);
                        changed.add(run);
                        runLength = 0;
                    }
                    if (id >= 0) {
                        if (runLength == 0) {
                            runStart = at;
                            runState = id;
                        }
                        runLength++;
                    }
                }
                JsonObject entry = new JsonObject();
                JsonArray at = new JsonArray();
                at.add(cx);
                at.add(cz);
                entry.add("chunk", at);
                entry.addProperty("starts", starts);
                entry.addProperty("carved", carved);
                entry.add("mask", runs(maskBits));
                entry.add("changed", changed);
                chunks.add(entry);
            }
        }
        JsonObject result = new JsonObject();
        result.add("chunks", chunks);
        return result;
    }

    // ---- structure.start ----

    /** Half-width of the chunk grid `structure.start` scans: chunks x, z in [-48, 47]. */
    private static final int STRUCTURE_GRID = 48;

    private static final class StructureWorld {
        static final net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager TEMPLATES = templates();

        private static net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager templates() {
            try {
                var packs = net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
                net.minecraft.server.MinecraftServer.configurePackRepository(
                    packs, net.minecraft.world.level.WorldDataConfiguration.DEFAULT, true, false
                );
                var resources = new net.minecraft.server.packs.resources.MultiPackResourceManager(
                    net.minecraft.server.packs.PackType.SERVER_DATA, packs.openAllSelected()
                );
                java.nio.file.Path root = Files.createTempDirectory("rivet-structure-world");
                var storage = net.minecraft.world.level.storage.LevelStorageSource.createDefault(root).createAccess("world");
                return new net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager(
                    resources, storage, net.minecraft.util.datafix.DataFixers.getDataFixer(),
                    ServerRegistries.ACCESS.lookupOrThrow(net.minecraft.core.registries.Registries.BLOCK)
                );
            } catch (Exception error) {
                throw new IllegalStateException("cannot open the structure template manager", error);
            }
        }
    }

    private static JsonArray chunkJson(final int x, final int z) {
        JsonArray at = new JsonArray();
        at.add(x);
        at.add(z);
        return at;
    }

    /**
     * `structure.start`: for one structure set and world seed, scan the chunk
     * grid [-48, 47]^2 of the set's dimension (`overworld`, `nether` or `end`;
     * the real multi-noise / end biome source and noise settings) the way
     * `ChunkGenerator.createStructures` does, but without a chunk: every
     * chunk where `StructurePlacement.isStructureChunk` holds is a candidate;
     * a one-structure set generates it, a larger set draws weighted options
     * from `setLargeFeatureSeed(seed, x, z)` and tries them in turn until one
     * is valid. Pieces are the `StructureStart` pieces in order; no block is
     * placed. A generation that throws is the verdict `threw` on that chunk.
     */
    private static JsonObject structureStart(final JsonObject request, final JsonObject dec) throws Exception {
        String key = requiredString(request, "structure_set");
        long seed = requiredLong(request, "seed");
        String dimension = requiredString(request, "dimension");
        var access = ServerRegistries.ACCESS;
        var biomeLookup = access.lookupOrThrow(net.minecraft.core.registries.Registries.BIOME);
        var set = registryEntry(net.minecraft.core.registries.Registries.STRUCTURE_SET, key);
        String settingsKey;
        net.minecraft.world.level.biome.BiomeSource biomeSource;
        net.minecraft.resources.ResourceKey<net.minecraft.world.level.Level> levelKey;
        switch (dimension) {
            case "overworld" -> {
                settingsKey = "minecraft:overworld";
                levelKey = net.minecraft.world.level.Level.OVERWORLD;
                biomeSource = net.minecraft.world.level.biome.MultiNoiseBiomeSource.createFromPreset(
                    access.lookupOrThrow(net.minecraft.core.registries.Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                        .getOrThrow(net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists.OVERWORLD));
            }
            case "nether" -> {
                settingsKey = "minecraft:nether";
                levelKey = net.minecraft.world.level.Level.NETHER;
                biomeSource = net.minecraft.world.level.biome.MultiNoiseBiomeSource.createFromPreset(
                    access.lookupOrThrow(net.minecraft.core.registries.Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST)
                        .getOrThrow(net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists.NETHER));
            }
            case "end" -> {
                settingsKey = "minecraft:end";
                levelKey = net.minecraft.world.level.Level.END;
                biomeSource = net.minecraft.world.level.biome.TheEndBiomeSource.create(biomeLookup);
            }
            default -> throw new IllegalArgumentException("unknown dimension " + dimension);
        }
        var settingsHolder = registryEntry(net.minecraft.core.registries.Registries.NOISE_SETTINGS, settingsKey);
        var generator = new net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator(biomeSource, settingsHolder);
        var settingsKeyObject = net.minecraft.resources.ResourceKey.create(net.minecraft.core.registries.Registries.NOISE_SETTINGS, net.minecraft.resources.Identifier.parse(settingsKey));
        var randomState = net.minecraft.world.level.levelgen.RandomState.create(access, settingsKeyObject, seed);
        var height = net.minecraft.world.level.LevelHeightAccessor.create(settingsHolder.value().noiseSettings().minY(), settingsHolder.value().noiseSettings().height());
        var structureState = structureState(generator, randomState, seed, access);
        // Paper's own set: `possibleStructureSets` holds the placement `injectSpigot` rebuilt
        // (the spigot.yml salts, keyed for the frequency overrides), and only the sets whose
        // structures have a biome in the dimension's biome source. A set absent from it has no
        // chunk to scan.
        net.minecraft.world.level.levelgen.structure.StructureSet possible = null;
        for (var candidate : structureState.possibleStructureSets()) {
            // `injectSpigot` re-wraps a random-spread set in a direct holder; the key then travels on the keyed placement.
            String candidateKey = candidate.unwrapKey().map(k -> k.identifier().toString()).orElseGet(() ->
                ((net.minecraft.world.level.chunk.ChunkGeneratorStructureState.KeyedRandomSpreadStructurePlacement) candidate.value().placement())
                    .key.identifier().toString());
            if (candidateKey.equals(key)) {
                possible = candidate.value();
            }
        }
        JsonArray candidates = new JsonArray();
        JsonArray starts = new JsonArray();
        if (possible == null) {
            JsonObject empty = new JsonObject();
            empty.add("candidates", candidates);
            empty.add("starts", starts);
            return empty;
        }
        var placement = possible.placement();
        net.minecraft.resources.ResourceKey<net.minecraft.world.level.levelgen.structure.StructureSet> placementKey = placement instanceof net.minecraft.world.level.chunk.ChunkGeneratorStructureState.KeyedRandomSpreadStructurePlacement keyed
            ? keyed.key : null;
        var entries = possible.structures();
        for (int x = -STRUCTURE_GRID; x < STRUCTURE_GRID; x++) {
            for (int z = -STRUCTURE_GRID; z < STRUCTURE_GRID; z++) {
                if (!placement.isStructureChunk(structureState, x, z, placementKey)) {
                    continue;
                }
                candidates.add(chunkJson(x, z));
                var chunkPos = new net.minecraft.world.level.ChunkPos(x, z);
                List<net.minecraft.world.level.levelgen.structure.StructureSet.StructureSelectionEntry> options = new ArrayList<>(entries);
                var random = new net.minecraft.world.level.levelgen.WorldgenRandom(new net.minecraft.world.level.levelgen.LegacyRandomSource(0L));
                random.setLargeFeatureSeed(seed, x, z);
                int total = 0;
                for (var option : options) {
                    total += option.weight();
                }
                while (!options.isEmpty()) {
                    int index = 0;
                    if (entries.size() > 1) {
                        int choice = random.nextInt(total);
                        for (var option : options) {
                            choice -= option.weight();
                            if (choice < 0) {
                                break;
                            }
                            index++;
                        }
                    }
                    var selected = options.get(index);
                    var structure = selected.structure().value();
                    net.minecraft.world.level.levelgen.structure.StructureStart start;
                    String threw = null;
                    try {
                        start = structure.generate(selected.structure(), levelKey, access, generator, biomeSource, randomState,
                            StructureWorld.TEMPLATES, seed, chunkPos, 0, height, structure.biomes()::contains);
                    } catch (RuntimeException | StackOverflowError error) {
                        start = net.minecraft.world.level.levelgen.structure.StructureStart.INVALID_START;
                        threw = error.getClass().getName();
                        dec.addProperty("threw " + x + "," + z, throwableSummary(error));
                    }
                    if (start.isValid() || threw != null) {
                        JsonObject entry = new JsonObject();
                        entry.add("chunk", chunkJson(x, z));
                        entry.addProperty("structure", selected.structure().unwrapKey().orElseThrow().identifier().toString());
                        JsonArray pieces = new JsonArray();
                        for (var piece : start.getPieces()) {
                            JsonObject json = new JsonObject();
                            json.addProperty("type", net.minecraft.core.registries.BuiltInRegistries.STRUCTURE_PIECE.getKey(piece.getType()).toString());
                            var box = piece.getBoundingBox();
                            JsonArray bounds = new JsonArray();
                            for (int value : new int[] {box.minX(), box.minY(), box.minZ(), box.maxX(), box.maxY(), box.maxZ()}) {
                                bounds.add(value);
                            }
                            json.add("box", bounds);
                            json.add("rotation", piece.getRotation() == null ? com.google.gson.JsonNull.INSTANCE : new com.google.gson.JsonPrimitive(piece.getRotation().name()));
                            json.add("mirror", piece.getMirror() == null ? com.google.gson.JsonNull.INSTANCE : new com.google.gson.JsonPrimitive(piece.getMirror().name()));
                            json.addProperty("depth", piece.getGenDepth());
                            pieces.add(json);
                        }
                        entry.add("pieces", pieces);
                        if (threw != null) {
                            entry.addProperty("threw", true);
                        }
                        starts.add(entry);
                        break;
                    }
                    options.remove(index);
                    total -= selected.weight();
                    if (entries.size() == 1) {
                        break;
                    }
                }
            }
        }
        JsonObject result = new JsonObject();
        result.add("candidates", candidates);
        result.add("starts", starts);
        return result;
    }

    /**
     * Paper's `SpigotWorldConfig` at its defaults. Its constructor reads a
     * spigot.yml and a live Bukkit server, neither of which exists here, so the
     * shell is allocated and its seed fields (the only ones structure placement
     * reads) are set to the defaults `initWorldGenSeeds` would read.
     */
    private static net.minecraft.world.level.chunk.ChunkGeneratorStructureState structureState(
        final net.minecraft.world.level.chunk.ChunkGenerator generator, final net.minecraft.world.level.levelgen.RandomState randomState,
        final long seed, final net.minecraft.core.RegistryAccess access
    ) throws Exception {
        var unsafeField = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
        unsafeField.setAccessible(true);
        var conf = (org.spigotmc.SpigotWorldConfig) ((sun.misc.Unsafe) unsafeField.get(null)).allocateInstance(org.spigotmc.SpigotWorldConfig.class);
        Object[][] seeds = {
            {"villageSeed", 10387312}, {"desertSeed", 14357617}, {"iglooSeed", 14357618}, {"jungleSeed", 14357619},
            {"swampSeed", 14357620}, {"monumentSeed", 10387313}, {"shipwreckSeed", 165745295}, {"oceanSeed", 14357621},
            {"outpostSeed", 165745296}, {"endCitySeed", 10387313}, {"slimeSeed", 987234911}, {"netherSeed", 30084232},
            {"mansionSeed", 10387319}, {"fossilSeed", 14357921}, {"portalSeed", 34222645}, {"ancientCitySeed", 20083232},
            {"trailRuinsSeed", 83469867}, {"trialChambersSeed", 94251327}, {"buriedTreasureSeed", 10387320},
        };
        for (Object[] entry : seeds) {
            setField(conf, (String) entry[0], entry[1]);
        }
        return generator.createState(access.lookupOrThrow(net.minecraft.core.registries.Registries.STRUCTURE_SET), randomState, seed, conf);
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

    static String requiredString(final JsonObject request, final String property) {
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
}
