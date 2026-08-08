import io.netty.buffer.ByteBuf;
import io.netty.buffer.ByteBufUtil;
import io.netty.buffer.Unpooled;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.HexFormat;

/**
 * Ground-truth probe for Java string boundaries that can hold isolated UTF-16
 * surrogates (GitHub #264). Compiles and runs against the pinned JDK (25.0.2)
 * and netty 4.2.15 (the versions Paper 26.2 uses). Each probe prints one
 * machine-readable JSON line on stdout; nothing on stderr.
 *
 * Run: see run.sh in this directory.
 */
public final class SurrogateProbe {
    private static final HexFormat HEX = HexFormat.of();

    /** Minimal VarInt read/write (netty VarInt). */
    private static final class VarInt {
        static void write(final ByteBuf out, final int value) {
            int v = value;
            while ((v & 0xFFFFFF80) != 0) {
                out.writeByte((v & 0x7F) | 0x80);
                v >>>= 7;
            }
            out.writeByte(v);
        }

        static int read(final ByteBuf in) {
            int value = 0;
            int i = 0;
            int b;
            while (((b = in.readByte()) & 0x80) != 0) {
                value |= (b & 0x7F) << i;
                i += 7;
            }
            return value | (b << i);
        }
    }

    private static void out(final String key, final Object value) {
        System.out.println("{\"probe\":\"" + key + "\",\"value\":" + JSON.quote(String.valueOf(value)) + "}");
    }

    /** Minimal JSON string quoting (control chars and quote escaped). */
    private static final class JSON {
        static String quote(final String s) {
            StringBuilder b = new StringBuilder("\"");
            for (int i = 0; i < s.length(); i++) {
                char c = s.charAt(i);
                switch (c) {
                    case '"' -> b.append("\\\"");
                    case '\\' -> b.append("\\\\");
                    case '\n' -> b.append("\\n");
                    case '\r' -> b.append("\\r");
                    case '\t' -> b.append("\\t");
                    default -> {
                        if (c < 0x20) {
                            b.append(String.format("\\u%04x", (int) c));
                        } else {
                            b.append(c);
                        }
                    }
                }
            }
            return b.append("\"").toString();
        }
    }

    private static String hex(final byte[] bytes) {
        return HEX.formatHex(bytes);
    }

    public static void main(final String[] args) throws Exception {
        // The lone-surrogate test strings.
        final String high = "\uD800"; // lone high surrogate
        final String low = "\uDC00"; // lone low surrogate
        final String pair = "\uD83D\uDCA9"; // 💩 (valid pair)

        // ---- 1. DataOutputStream.writeUTF / DataInputStream.readUTF ---------
        probeWriteUTF("writeUTF_high", high);
        probeWriteUTF("writeUTF_low", low);
        probeWriteUTF("writeUTF_pair", pair);
        probeReadUTF("readUTF_high", new byte[]{(byte) 0xED, (byte) 0xA0, (byte) 0x80}); // ED A0 80 = D800
        probeReadUTF("readUTF_low", new byte[]{(byte) 0xED, (byte) 0xB0, (byte) 0x80}); // ED B0 80 = DC00
        probeReadUTF("readUTF_pair", new byte[]{(byte) 0xED, (byte) 0xA0, (byte) 0x80, (byte) 0xED, (byte) 0xB0, (byte) 0x80});

        // ---- 2. netty ByteBufUtil.writeUtf8 (Utf8String.write path) ---------
        probeNettyWriteUtf8("netty_writeUtf8_high", high);
        probeNettyWriteUtf8("netty_writeUtf8_low", low);
        probeNettyWriteUtf8("netty_writeUtf8_pair", pair);

        // ---- 3. new String(bytes, UTF_8) WHATWG decode (Utf8String.read) ----
        probeStringUtf8("jdk_decode_ed_a0_80", new byte[]{(byte) 0xED, (byte) 0xA0, (byte) 0x80});
        probeStringUtf8("jdk_decode_ed_a0_80_41", new byte[]{(byte) 0xED, (byte) 0xA0, (byte) 0x80, 0x41});
        probeStringUtf8("jdk_decode_pair", new byte[]{(byte) 0xF0, (byte) 0x9F, (byte) 0x92, (byte) 0xA9});

        // ---- 4. Utf8String.read on a wire string with a lone surrogate ------
        // netty: `input.toString(readerIndex, bufferLength, UTF_8)` then
        // `result.length()` (UTF-16 units) vs maxLength. `ED A0 80` decodes to
        // one U+FFFD under WHATWG; a Rust WHATWG port must produce the same.
        probeNettyToString("netty_toString_ed_a0_80", new byte[]{(byte) 0xED, (byte) 0xA0, (byte) 0x80});
        probeNettyToString("netty_toString_pair", new byte[]{(byte) 0xF0, (byte) 0x9F, (byte) 0x92, (byte) 0xA9});

        // ---- 5. Gson JsonPrimitive round-trip of a lone surrogate -----------
        probeGson("gson_high_escaped", "\"\\ud800\"");
        probeGson("gson_high_raw", high);
        probeGson("gson_low_escaped", "\"\\udc00\"");
        probeGson("gson_pair_escaped", "\"\\ud83d\\udca9\"");

        // ---- 6. UTF-8 encoder of a lone surrogate (Java String -> bytes) ----
        probeUtf8Encode("jdk_utf8_encode_high", high);
        probeUtf8Encode("jdk_utf8_encode_low", low);
        probeUtf8Encode("jdk_utf8_encode_pair", pair);

        // ---- 7. Character.toString / isValidCodePoint for SNBT escapes ------
        // ---- 8. SNBT quoteAndEscape of a lone surrogate (StringTag/SNBT printer)
        out("quoteAndEscape_high", codeUnits(quoteAndEscape(high)));
        out("quoteAndEscape_low", codeUnits(quoteAndEscape(low)));

        // ---- 9. Full Utf8String.write/read round trip through netty VarInt ----
        probeNettyUtf8StringRoundTrip("netty_utf8string_roundtrip_ed_a0_80",
            new byte[]{(byte) 0xED, (byte) 0xA0, (byte) 0x80});

        out("char_isValidCodePoint_d800", Character.isValidCodePoint(0xD800));
        out("char_isValidCodePoint_dfff", Character.isValidCodePoint(0xDFFF));
        out("char_isValidCodePoint_10000", Character.isValidCodePoint(0x10000));
        out("char_toString_d800_units", codeUnits(Character.toString(0xD800)));
        out("char_toString_10000_units", codeUnits(Character.toString(0x10000)));
        out("codePointOf_high_surrogates_d800", Character.codePointOf("HIGH SURROGATES D800") == 0xD800);
    }

    /** StringTag.quoteAndEscape equivalent (the SNBT printer's string form). */
    private static String quoteAndEscape(final String s) {
        StringBuilder result = new StringBuilder();
        char quote = '\0';
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c == '\\') {
                result.append("\\\\");
            } else if (c != '"' && c != '\'') {
                result.append(c);
            } else {
                if (quote == '\0') {
                    quote = (c == '"') ? '\'' : '"';
                }
                if (quote == c) {
                    result.append('\\');
                }
                result.append(c);
            }
        }
        if (quote == '\0') {
            quote = '"';
        }
        return quote + result.toString() + quote;
    }

    private static String codeUnits(final String s) {
        StringBuilder b = new StringBuilder();
        for (int i = 0; i < s.length(); i++) {
            if (i > 0) {
                b.append(' ');
            }
            b.append(String.format("U+%04X", (int) s.charAt(i)));
        }
        return b.toString();
    }

    private static void probeWriteUTF(final String key, final String s) {
        try {
            ByteArrayOutputStream bos = new ByteArrayOutputStream();
            DataOutputStream dos = new DataOutputStream(bos);
            dos.writeUTF(s);
            dos.flush();
            out(key, "utf16_units=" + s.length() + " bytes=" + hex(bos.toByteArray()));
        } catch (IOException e) {
            out(key, "EXCEPTION " + e.getClass().getSimpleName() + ": " + e.getMessage());
        }
    }

    private static void probeReadUTF(final String key, final byte[] payload) {
        try {
            byte[] framed = new byte[2 + payload.length];
            framed[0] = (byte) ((payload.length >> 8) & 0xFF);
            framed[1] = (byte) (payload.length & 0xFF);
            System.arraycopy(payload, 0, framed, 2, payload.length);
            DataInputStream dis = new DataInputStream(new ByteArrayInputStream(framed));
            String s = dis.readUTF();
            out(key, "units=" + s.length() + " codepoints=" + s.codePointCount(0, s.length())
                + " hex=" + hex(s.getBytes(StandardCharsets.UTF_8)) + " chars=" + codeUnits(s));
        } catch (IOException e) {
            out(key, "EXCEPTION " + e.getClass().getSimpleName() + ": " + e.getMessage());
        }
    }

    private static void probeNettyWriteUtf8(final String key, final String s) {
        ByteBuf tmp = Unpooled.buffer();
        try {
            int written = ByteBufUtil.writeUtf8(tmp, s);
            byte[] bytes = new byte[written];
            tmp.getBytes(0, bytes);
            out(key, "utf16_units=" + s.length() + " bytesWritten=" + written + " bytes=" + hex(bytes));
        } finally {
            tmp.release();
        }
    }

    private static void probeStringUtf8(final String key, final byte[] bytes) {
        String s = new String(bytes, StandardCharsets.UTF_8);
        out(key, "units=" + s.length() + " chars=" + codeUnits(s));
    }

    private static void probeNettyToString(final String key, final byte[] bytes) {
        ByteBuf buf = Unpooled.wrappedBuffer(bytes);
        try {
            String s = buf.toString(buf.readerIndex(), buf.readableBytes(), StandardCharsets.UTF_8);
            out(key, "units=" + s.length() + " chars=" + codeUnits(s));
        } finally {
            buf.release();
        }
    }

    private static void probeGson(final String key, final String json) {
        try {
            com.google.gson.JsonElement el = com.google.gson.JsonParser.parseString(json);
            String serialized = el.toString();
            out(key, "value_units=" + el.getAsString().length()
                + " value_chars=" + codeUnits(el.getAsString())
                + " serialized=" + serialized);
        } catch (Exception e) {
            out(key, "EXCEPTION " + e.getClass().getSimpleName() + ": " + e.getMessage());
        }
    }

    /** netty Utf8String.read equivalent on a payload (varint len + bytes). */
    private static void probeNettyUtf8StringRoundTrip(final String key, final byte[] payload) {
        ByteBuf wire = Unpooled.buffer();
        try {
            VarInt.write(wire, payload.length);
            wire.writeBytes(payload);
            int len = VarInt.read(wire);
            String s = wire.toString(wire.readerIndex(), len, StandardCharsets.UTF_8);
            wire.readerIndex(wire.readerIndex() + len);
            out(key, "decoded_units=" + s.length() + " decoded_chars=" + codeUnits(s)
                + " vs_maxLength=32767 pass=" + (s.length() <= 32767));
        } finally {
            wire.release();
        }
    }

    private static void probeUtf8Encode(final String key, final String s) {
        byte[] bytes = s.getBytes(StandardCharsets.UTF_8);
        out(key, "input_units=" + s.length() + " encoded=" + hex(bytes));
    }
}
