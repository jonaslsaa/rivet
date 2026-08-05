package dev.rivet.oracle;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
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

public final class RivetReferenceOracle {
    private static final int PROTOCOL_VERSION = 1;
    private static final Gson GSON = new GsonBuilder().disableHtmlEscaping().create();
    private static final TagParser<Tag> SNBT_PARSER = TagParser.create(NbtOps.INSTANCE);

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
                    System.out.println(GSON.toJson(processLine(line)));
                    System.out.flush();
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
                default -> throw new IllegalArgumentException("unknown operation: " + operation);
            };
            return success(id, result);
        } catch (Exception error) {
            return failure(id, error);
        }
    }

    private static JsonObject ping() {
        JsonObject result = new JsonObject();
        result.addProperty("paper_minecraft_version", "26.2");
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
        byte[] bytes = Base64.getDecoder().decode(input);
        CompoundTag tag;
        try (DataInputStream data = new DataInputStream(new ByteArrayInputStream(bytes))) {
            tag = NbtIo.read(data, NbtAccounter.unlimitedHeap());
            if (data.available() != 0) {
                throw new IllegalArgumentException("trailing bytes after root compound: " + data.available());
            }
        }

        JsonObject result = describeTag(tag);
        result.addProperty("bytes", bytes.length);
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

        String encodedRequest = "{\"id\":\"encode\",\"op\":\"nbt.encode\","
            + "\"input\":\"{name:\\\"Rivet\\\",values:[I;1,2,3]}\"}";
        JsonObject encoded = processLine(encodedRequest);
        requireSuccess(encoded, "nbt.encode");

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

        JsonObject summary = new JsonObject();
        summary.addProperty("ok", true);
        summary.addProperty("protocol", PROTOCOL_VERSION);
        summary.addProperty("tests", 4);
        System.out.println(GSON.toJson(summary));
    }

    private static void requireSuccess(final JsonObject response, final String operation) {
        if (!response.get("ok").getAsBoolean()) {
            throw new IllegalStateException(operation + " self-test failed: " + response);
        }
    }
}
