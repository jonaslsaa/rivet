//! `java.io.DataInput` / `java.io.DataOutput` ports — the byte-level contract
//! used by `NbtIo` (rivet-nbt) and the network codec.
//!
//! The surface is the minimal slice `NbtIo`'s read/write path needs: big-endian
//! primitives (`writeShort`/`readInt`/...) and modified-UTF-8 strings
//! (`writeUTF`/`readUTF`), the only string form the NBT tags use. The rest of
//! the `java.io.DataOutput`/`DataInput` surface (`writeChar`, `writeBytes`,
//! `readChar`, ...) is deliberately not ported.
//!
//! The write side encodes through the `cesu8` crate's Java variant, which
//! matches `DataOutputStream.writeUTF` byte-for-byte (NUL as `C0 80`, astral
//! characters as CESU-8 surrogate pairs, 2-byte length prefix in big-endian).
//! The read side ports OpenJDK's `DataInputStream.readUTF` decoder directly
//! ([`decode_modified_utf8`]): hostile input must behave exactly like Java —
//! including inputs the `cesu8` Java-variant decoder rejects (a raw NUL byte)
//! or corrupts (the overlong two-byte form `C1 80`).

use std::io::{self, Read, Write};

/// `java.io.DataOutput` — the write side of the byte contract.
pub trait DataOutput {
    /// `DataOutput.write(int)` — low 8 bits.
    fn write(&mut self, b: i32) -> io::Result<()>;

    /// `DataOutput.write(byte[])`.
    fn write_all(&mut self, b: &[u8]) -> io::Result<()>;

    /// `DataOutput.writeUTF(String)` — modified UTF-8 with a 2-byte length
    /// prefix (big-endian), matching `java.io.DataOutputStream.writeUTF`.
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
        let encoded = cesu8::to_java_cesu8(s);
        if encoded.len() > u16::MAX as usize {
            // Modified UTF-8 longer than 65535 bytes cannot be length-prefixed.
            // Java `DataOutput.writeUTF` throws UTFDataFormatException here —
            // before writing anything — and `NbtIo.StringFallbackDataOutput`
            // catches it and writes "" instead. We surface the overflow and let
            // the caller decide (see NbtIo).
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("encoded string too long: {} bytes", encoded.len()),
            ));
        }
        // `as i16` keeps the low 16 bits, i.e. exactly Java's `writeShort(utflen)`
        // which truncates the 0..=65535 byte length to 16 bits.
        self.write_short(encoded.len() as i16)?;
        self.inner.write_all(&encoded)
    }
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
