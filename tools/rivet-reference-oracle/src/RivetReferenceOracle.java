package dev.rivet.oracle;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import java.io.BufferedReader;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.Base64;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.NbtAccounter;
import net.minecraft.nbt.NbtIo;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.SnbtPrinterTagVisitor;
import net.minecraft.nbt.Tag;
import net.minecraft.nbt.TagParser;
import net.minecraft.SharedConstants;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.ComponentSerialization;
import net.minecraft.server.Bootstrap;

public final class RivetReferenceOracle {
    private static final int PROTOCOL_VERSION = 1;
    private static final int MAX_NBT_BYTES = 16 * 1024 * 1024;
    private static final int MAX_BASE64_CHARS = ((MAX_NBT_BYTES + 2) / 3) * 4;
    private static final Gson GSON = new GsonBuilder().disableHtmlEscaping().create();
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

    public static void main(final String[] args) throws Exception {
        if (args.length > 0) {
            if (args.length == 1 && args[0].equals("--self-test")) {
                selfTest();
                return;
            }
            System.err.println("Usage: run.sh [--self-test]");
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
        try {
            JsonObject request = JsonParser.parseString(line).getAsJsonObject();
            id = request.get("id");
            String operation = requiredString(request, "op");
            JsonObject result = switch (operation) {
                case "ping" -> ping();
                case "snbt.parse" -> parseSnbt(requiredString(request, "input"));
                case "nbt.encode" -> encodeNbt(requiredString(request, "input"));
                case "nbt.decode" -> decodeNbt(requiredString(request, "input_base64"));
                case "component.json" -> componentJson(requiredString(request, "input"));
                default -> throw new IllegalArgumentException("unknown operation: " + operation);
            };
            return success(id, result);
        } catch (Exception error) {
            return failure(id, error);
        }
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
            throw new IllegalArgumentException("base64 NBT input exceeds " + MAX_NBT_BYTES + " decoded bytes");
        }
        byte[] bytes = Base64.getDecoder().decode(input);
        if (bytes.length > MAX_NBT_BYTES) {
            throw new IllegalArgumentException("NBT input exceeds " + MAX_NBT_BYTES + " bytes");
        }
        CompoundTag tag;
        try (DataInputStream data = new DataInputStream(new ByteArrayInputStream(bytes))) {
            tag = NbtIo.read(data, NbtAccounter.create(MAX_NBT_BYTES));
            if (data.available() != 0) {
                throw new IllegalArgumentException("trailing bytes after root compound: " + data.available());
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
        } catch (Exception error) {
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

    private static JsonObject failure(final JsonElement id, final Exception error) {
        JsonObject details = new JsonObject();
        details.addProperty("type", error.getClass().getName());
        details.addProperty("message", error.getMessage());

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

        JsonObject summary = new JsonObject();
        summary.addProperty("ok", true);
        summary.addProperty("protocol", PROTOCOL_VERSION);
        summary.addProperty("tests", 9);
        // Emit on the raw stream, not System.out: Bootstrap.bootStrap() has
        // re-wired System.out through log4j's SysOutOverSLF4J wrapper, so a
        // System.out.println here would surface as a `[HH:mm:ss LEVEL]: [STDOUT]:
        // {...}` log line instead of a bare JSON line. The JSON-Lines protocol
        // (and any consumer that parses the self-test output line by line) rides
        // on the raw stream.
        RAW_STDOUT.println(GSON.toJson(summary));
        RAW_STDOUT.flush();
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
