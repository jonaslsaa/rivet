//! `java.io.DataInput` / `java.io.DataOutput` ports — the byte-level contract
//! used by `NbtIo` (rivet-nbt) and the network codec.
//!
//! The surface is the minimal slice `NbtIo`'s read/write path needs: big-endian
//! primitives (`writeShort`/`readInt`/...) and modified-UTF-8 strings
//! (`writeUTF`/`readUTF`), the only string form the NBT tags use. The rest of
//! the `java.io.DataOutput`/`DataInput` surface (`writeChar`, `writeBytes`,
//! `readChar`, ...) is deliberately not ported.
//!
//! Both directions of modified UTF-8 are in-repo ports, so `rivet-protocol`'s
//! NBT bridge and `rivet-nbt`'s `StringFallbackDataOutput` can share them
//! without a dependency cycle (both already depend on `rivet-util`). The write
//! side ([`write_utf_body`]) matches `DataOutputStream.writeUTF` byte-for-byte
//! (NUL as `C0 80`, astral characters as CESU-8 surrogate pairs, 2-byte length
//! prefix in big-endian). The read side ([`decode_modified_utf8`]) is a direct
//! port of OpenJDK's `DataInputStream.readUTF` decoder: Java accepts overlong
//! forms (`C1 80` -> `U+0040`, `E0 80 80` -> NUL) that the `cesu8` crate's
//! Java-variant decoder rejects, and Java's diagnostics name the exact byte
//! offset, which `cesu8`'s generic error does not.

use std::io::{self, Read, Write};

/// `java.io.DataOutput` — the write side of the byte contract.
pub trait DataOutput {
    /// `DataOutput.write(int)` — low 8 bits.
    fn write(&mut self, b: i32) -> io::Result<()>;

    /// `DataOutput.write(byte[])`.
    fn write_all(&mut self, b: &[u8]) -> io::Result<()>;

    /// `DataOutput.writeUTF(String)` — modified UTF-8 with a 2-byte length
    /// prefix (big-endian), matching `java.io.DataOutputStream.writeUTF`.
    ///
    /// Errors with `InvalidData` (Java's `UTFDataFormatException`) before
    /// writing anything when the encoded body exceeds 65535 bytes; the error
    /// message matches OpenJDK 25's, except the head/tail display keeps whole
    /// code points where Java's UTF-16 slicing would split a surrogate pair.
    fn write_utf(&mut self, s: &str) -> io::Result<()>;

    /// `DataOutput.writeBoolean(boolean)`.
    fn write_boolean(&mut self, v: bool) -> io::Result<()> {
        self.write(v as i32)
    }

    /// `DataOutput.writeByte(int)`.
    fn write_byte(&mut self, v: i32) -> io::Result<()> {
        self.write(v)
    }

    /// `DataOutput.writeShort(int)` — big-endian 2 bytes.
    fn write_short(&mut self, v: i16) -> io::Result<()> {
        self.write(((v as u16 >> 8) & 0xFF) as i32)?;
        self.write((v & 0xFF) as i32)
    }

    /// `DataOutput.writeInt(int)` — big-endian 4 bytes.
    fn write_int(&mut self, v: i32) -> io::Result<()> {
        self.write(((v as u32 >> 24) & 0xFF) as i32)?;
        self.write(((v as u32 >> 16) & 0xFF) as i32)?;
        self.write(((v as u32 >> 8) & 0xFF) as i32)?;
        self.write(v & 0xFF)
    }

    /// `DataOutput.writeLong(long)` — big-endian 8 bytes.
    fn write_long(&mut self, v: i64) -> io::Result<()> {
        self.write(((v as u64 >> 56) & 0xFF) as i32)?;
        self.write(((v as u64 >> 48) & 0xFF) as i32)?;
        self.write(((v as u64 >> 40) & 0xFF) as i32)?;
        self.write(((v as u64 >> 32) & 0xFF) as i32)?;
        self.write(((v as u64 >> 24) & 0xFF) as i32)?;
        self.write(((v as u64 >> 16) & 0xFF) as i32)?;
        self.write(((v as u64 >> 8) & 0xFF) as i32)?;
        self.write((v & 0xFF) as i32)
    }

    /// `DataOutput.writeFloat(float)` — int bits big-endian.
    ///
    /// `java.io.DataOutputStream.writeFloat` uses `Float.floatToIntBits`, which
    /// canonicalizes every NaN payload to `0x7fc00000`. `to_bits` alone would
    /// preserve a non-canonical payload, so canonicalize NaN before encoding.
    fn write_float(&mut self, v: f32) -> io::Result<()> {
        let bits = if v.is_nan() {
            0x7fc0_0000u32
        } else {
            v.to_bits()
        };
        self.write_int(bits as i32)
    }

    /// `DataOutput.writeDouble(double)` — long bits big-endian.
    ///
    /// `java.io.DataOutputStream.writeDouble` uses `Double.doubleToLongBits`,
    /// which canonicalizes every NaN payload to `0x7ff8_0000_0000_0000`.
    fn write_double(&mut self, v: f64) -> io::Result<()> {
        let bits = if v.is_nan() {
            0x7ff8_0000_0000_0000u64
        } else {
            v.to_bits()
        };
        self.write_long(bits as i64)
    }
}

/// `java.io.DataInput` — the read side of the byte contract.
pub trait DataInput {
    /// `DataInput.readUnsignedByte()`.
    fn read_unsigned_byte(&mut self) -> io::Result<i32>;

    /// `DataInput.readUnsignedShort()`.
    fn read_unsigned_short(&mut self) -> io::Result<i32>;

    /// `DataInput.readInt()`.
    fn read_int(&mut self) -> io::Result<i32>;

    /// `DataInput.readLong()`.
    fn read_long(&mut self) -> io::Result<i64>;

    /// `DataInput.readFloat()`.
    fn read_float(&mut self) -> io::Result<f32>;

    /// `DataInput.readDouble()`.
    fn read_double(&mut self) -> io::Result<f64>;

    /// `DataInput.readUTF()` — modified UTF-8 with a 2-byte length prefix,
    /// matching `java.io.DataInputStream.readUTF`.
    fn read_utf(&mut self) -> io::Result<String>;

    /// `DataInput.readFully(byte[])` — read exactly `n` bytes.
    fn read_fully(&mut self, n: usize) -> io::Result<Vec<u8>>;

    /// `DataInput.skipBytes(int)` — skips up to `n` bytes; on EOF silently
    /// returns the count actually skipped (never an error).
    fn skip_bytes(&mut self, n: usize) -> io::Result<usize>;
}

/// Blanket forwarding impl so `&mut T` can be passed wherever `DataOutput` is
/// required (mirrors Java passing the same `DataOutput` reference around, e.g.
/// `NbtIo.writeUnnamedTag(tag, new StringFallbackDataOutput(output))`).
impl<T: DataOutput + ?Sized> DataOutput for &mut T {
    fn write(&mut self, b: i32) -> io::Result<()> {
        (**self).write(b)
    }

    fn write_all(&mut self, b: &[u8]) -> io::Result<()> {
        (**self).write_all(b)
    }

    fn write_utf(&mut self, s: &str) -> io::Result<()> {
        (**self).write_utf(s)
    }

    fn write_boolean(&mut self, v: bool) -> io::Result<()> {
        (**self).write_boolean(v)
    }

    fn write_byte(&mut self, v: i32) -> io::Result<()> {
        (**self).write_byte(v)
    }

    fn write_short(&mut self, v: i16) -> io::Result<()> {
        (**self).write_short(v)
    }

    fn write_int(&mut self, v: i32) -> io::Result<()> {
        (**self).write_int(v)
    }

    fn write_long(&mut self, v: i64) -> io::Result<()> {
        (**self).write_long(v)
    }

    fn write_float(&mut self, v: f32) -> io::Result<()> {
        (**self).write_float(v)
    }

    fn write_double(&mut self, v: f64) -> io::Result<()> {
        (**self).write_double(v)
    }
}

/// `DataOutputStream` over any `Write` — implements `DataOutput`.
pub struct DataOutputStream<W: Write> {
    inner: W,
}

impl<W: Write> DataOutputStream<W> {
    pub fn new(inner: W) -> Self {
        DataOutputStream { inner }
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> DataOutput for DataOutputStream<W> {
    fn write(&mut self, b: i32) -> io::Result<()> {
        self.inner.write_all(&[b as u8])
    }

    fn write_all(&mut self, b: &[u8]) -> io::Result<()> {
        self.inner.write_all(b)
    }

    fn write_utf(&mut self, s: &str) -> io::Result<()> {
        let encoded = write_utf_body(s)?;
        // `as i16` keeps the low 16 bits, i.e. exactly Java's `writeShort(utflen)`
        // which truncates the 0..=65535 byte length to 16 bits.
        self.write_short(encoded.len() as i16)?;
        self.inner.write_all(&encoded)
    }
}

/// `DataOutputStream.writeUTF` body — encode `s` as modified UTF-8 (no length
/// prefix; the caller writes the `u16`) and enforce the 2-byte prefix limit.
///
/// A body longer than 65535 bytes is an `InvalidData` error (Java's
/// `UTFDataFormatException`) with OpenJDK 25's exact `tooLongMsg`, returned
/// before any bytes are produced. `NbtIo.StringFallbackDataOutput` catches that
/// specific error and writes `""` instead. The network NBT bridge
/// (`rivet-protocol::friendly_byte_buf`) shares this helper so its overflow
/// wording and byte-for-byte encoding are identical to `DataOutputStream`'s.
pub fn write_utf_body(s: &str) -> io::Result<Vec<u8>> {
    let encoded = encode_modified_utf8(s);
    if encoded.len() > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            too_long_message(s, encoded.len()),
        ));
    }
    Ok(encoded)
}

/// `DataOutputStream.writeUTF` — encode `s` as modified UTF-8 (no length
/// prefix; the caller writes the `u16`). Every `&str` is encodable: NUL →
/// `C0 80`, BMP non-ASCII → two bytes, supplementary scalars → two surrogate
/// halves (three bytes each). Mirrors Java exactly, including that a string
/// longer than 65535 encoded bytes fails via [`too_long_message`].
fn encode_modified_utf8(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for unit in s.encode_utf16() {
        match unit {
            // The 2-byte form covers 0x0000 and 0x0080..=0x07FF: NUL is never
            // written as a raw 0x00 byte.
            0x0000 | 0x0080..=0x07FF => {
                out.push(0xC0 | ((unit >> 6) & 0x1F) as u8);
                out.push(0x80 | (unit & 0x3F) as u8);
            }
            0x0001..=0x007F => out.push(unit as u8),
            _ => {
                out.push(0xE0 | ((unit >> 12) & 0x0F) as u8);
                out.push(0x80 | ((unit >> 6) & 0x3F) as u8);
                out.push(0x80 | (unit & 0x3F) as u8);
            }
        }
    }
    out
}

/// The modified-UTF-8 byte length of `s` (the `writeUTF` payload length), used
/// by `NbtIo.StringFallbackDataOutput` to pre-check the prefix limit without
/// allocating the encoded body.
pub fn encoded_len(s: &str) -> usize {
    let mut len = 0usize;
    for unit in s.encode_utf16() {
        len += match unit {
            0x0001..=0x007F => 1,
            0x0000 | 0x0080..=0x07FF => 2,
            _ => 3,
        };
    }
    len
}

/// OpenJDK `DataOutputStream.tooLongMsg` — the `UTFDataFormatException`
/// message for a modified-UTF-8 body longer than 65535 bytes.
///
/// The message quotes the first and last 8 UTF-16 code units of `s`
/// (`String.substring(0, 8)` / `substring(slen - 8, slen)` in Java), so an
/// astral character counts as two units. Java's UTF-16 slicing can cut between
/// the halves of a surrogate pair, leaving a lone surrogate in the message; a
/// Rust `String` cannot hold one, so when a cut lands inside an astral
/// character the whole character is kept instead. This display-side preservation
/// is not the decoder's behavior — [`decode_modified_utf8`] rejects an unpaired
/// surrogate with an error.
fn too_long_message(s: &str, utflen: usize) -> String {
    /// First `n` UTF-16 code units of `s` as whole characters, from the start
    /// or the end; a cut inside an astral character keeps the whole character
    /// (see [`too_long_message`]).
    fn take_utf16(s: &str, n: usize, from_end: bool) -> String {
        let mut units = 0usize;
        let mut out = String::new();
        if from_end {
            for ch in s.chars().rev() {
                if units >= n {
                    break;
                }
                out.push(ch);
                units += ch.len_utf16();
            }
            out.chars().rev().collect()
        } else {
            for ch in s.chars() {
                if units >= n {
                    break;
                }
                out.push(ch);
                units += ch.len_utf16();
            }
            out
        }
    }
    let head = take_utf16(s, 8, false);
    let tail = take_utf16(s, 8, true);
    format!("encoded string ({head}...{tail}) too long: {utflen} bytes")
}

/// `DataInputStream` over any `Read` — implements `DataInput`.
pub struct DataInputStream<R: Read> {
    inner: R,
}

impl<R: Read> DataInputStream<R> {
    pub fn new(inner: R) -> Self {
        DataInputStream { inner }
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> DataInput for DataInputStream<R> {
    fn read_unsigned_byte(&mut self) -> io::Result<i32> {
        let mut buf = [0u8; 1];
        self.inner.read_exact(&mut buf)?;
        Ok(buf[0] as i32)
    }

    fn read_unsigned_short(&mut self) -> io::Result<i32> {
        let mut buf = [0u8; 2];
        self.inner.read_exact(&mut buf)?;
        Ok(((buf[0] as i32) << 8) | (buf[1] as i32))
    }

    fn read_int(&mut self) -> io::Result<i32> {
        let mut buf = [0u8; 4];
        self.inner.read_exact(&mut buf)?;
        Ok(i32::from_be_bytes(buf))
    }

    fn read_long(&mut self) -> io::Result<i64> {
        let mut buf = [0u8; 8];
        self.inner.read_exact(&mut buf)?;
        Ok(i64::from_be_bytes(buf))
    }

    fn read_float(&mut self) -> io::Result<f32> {
        Ok(f32::from_bits(self.read_int()? as u32))
    }

    fn read_double(&mut self) -> io::Result<f64> {
        Ok(f64::from_bits(self.read_long()? as u64))
    }

    fn read_utf(&mut self) -> io::Result<String> {
        let len = self.read_unsigned_short()? as usize;
        let mut buf = vec![0u8; len];
        self.inner.read_exact(&mut buf)?;
        decode_modified_utf8(&buf)
    }

    fn read_fully(&mut self, n: usize) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; n];
        self.inner.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn skip_bytes(&mut self, n: usize) -> io::Result<usize> {
        let mut skipped = 0usize;
        let mut buf = [0u8; 8192];
        while skipped < n {
            let want = buf.len().min(n - skipped);
            let read = self.inner.read(&mut buf[..want])?;
            if read == 0 {
                break; // EOF — return the count actually skipped (Java semantics)
            }
            skipped += read;
        }
        Ok(skipped)
    }
}

/// OpenJDK `DataInputStream.readUTF` body decoder — turns the bytes after the
/// 2-byte length prefix into the equivalent Rust `String`.
///
/// This is a faithful port of the current OpenJDK body (the loop after
/// `readFully(bytearr, 0, utflen)`), including its error handling:
/// - the top nibble of each byte selects the 1/2/3-byte form, and the
///   leading-ASCII run is decoded first;
/// - a two-byte form whose second byte is not `10xxxxxx`, or a three-byte
///   form whose second or third byte is not `10xxxxxx`, throws
///   `UTFDataFormatException` with the same byte offset message;
/// - a truncated lead byte at the end throws "malformed input: partial
///   character at end";
/// - top nibbles 8-11 and 15 throw "malformed input around byte N";
/// - a raw `0x00` byte is a valid one-byte character, and the overlong
///   two-byte form `C1 80` decodes to `U+0040` (only the continuation bytes
///   are validated, not the lead byte's non-overlong bound).
///
/// The deviations are forced by the return type. Java `readUTF` returns a
/// UTF-16 `String` with no validation, so it never fails on an unpaired
/// surrogate (`0xD800..=0xDFFF`); Rust `String` must be valid UTF-8, so an
/// unpaired surrogate here is an error. A high+low surrogate pair — how the
/// encoder wrote an astral character — is returned by Java as two code units
/// representing that one astral scalar, which Rust materializes as the single
/// scalar.
pub fn decode_modified_utf8(bytes: &[u8]) -> io::Result<String> {
    let utflen = bytes.len();
    // Java's first loop: fast-forward over the leading ASCII run.
    let mut units = Vec::with_capacity(utflen);
    let mut count = 0usize;
    while count < utflen && bytes[count] <= 0x7F {
        units.push(bytes[count] as u16);
        count += 1;
    }

    // Java's switch (c >> 4) loop. `count` tracks bytes consumed exactly like
    // the Java `count`, so the error messages report the same byte offsets.
    while count < utflen {
        let c = bytes[count] as i32;
        match c >> 4 {
            // 0xxxxxxx.
            0..=7 => {
                count += 1;
                units.push(c as u16);
            }
            // 110x xxxx   10xx xxxx.
            12 | 13 => {
                count += 2;
                if count > utflen {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "malformed input: partial character at end",
                    ));
                }
                let char2 = bytes[count - 1] as i32;
                if (char2 & 0xC0) != 0x80 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("malformed input around byte {count}"),
                    ));
                }
                units.push((((c & 0x1F) << 6) | (char2 & 0x3F)) as u16);
            }
            // 1110 xxxx  10xx xxxx  10xx xxxx.
            14 => {
                count += 3;
                if count > utflen {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "malformed input: partial character at end",
                    ));
                }
                let char2 = bytes[count - 2] as i32;
                let char3 = bytes[count - 1] as i32;
                if (char2 & 0xC0) != 0x80 || (char3 & 0xC0) != 0x80 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("malformed input around byte {}", count - 1),
                    ));
                }
                units.push((((c & 0x0F) << 12) | ((char2 & 0x3F) << 6) | (char3 & 0x3F)) as u16);
            }
            // 10xx xxxx, 1111 xxxx.
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("malformed input around byte {count}"),
                ));
            }
        }
    }

    // Java `return new String(chararr, 0, chararr_count)`: code units to a Rust
    // String. Adjacent high+low surrogates combine into the astral scalar the
    // encoder wrote; an unpaired surrogate is unrepresentable and errors.
    // `char::from_u32` is guarded so the hostile-input path can never panic.
    let mut out = String::with_capacity(units.len());
    let mut j = 0;
    while j < units.len() {
        let c = units[j];
        match c {
            0xD800..=0xDBFF => {
                let Some(&next) = units.get(j + 1) else {
                    return Err(unpaired_surrogate());
                };
                if !(0xDC00..=0xDFFF).contains(&next) {
                    return Err(unpaired_surrogate());
                }
                let cp = 0x1_0000 + (((c as u32 - 0xD800) << 10) | (next as u32 - 0xDC00));
                let Some(ch) = char::from_u32(cp) else {
                    return Err(unpaired_surrogate());
                };
                out.push(ch);
                j += 2;
            }
            0xDC00..=0xDFFF => {
                return Err(unpaired_surrogate());
            }
            _ => {
                let Some(ch) = char::from_u32(c as u32) else {
                    return Err(unpaired_surrogate());
                };
                out.push(ch);
                j += 1;
            }
        }
    }
    Ok(out)
}

/// A decoded `0xD800..=0xDFFF` code unit not paired with its counterpart.
fn unpaired_surrogate() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "unpaired surrogate in modified UTF-8 (Java String can hold it, Rust String cannot)",
    )
}
