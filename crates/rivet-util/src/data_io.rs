//! `java.io.DataInput` / `java.io.DataOutput` ports — the byte-level contract
//! used by `NbtIo` (rivet-nbt) and the network codec.
//!
//! STUB(mc.nbt.io) — minimal faithful surface for the NBT read/write path:
//! big-endian primitives + modified-UTF-8 strings (`writeUTF`/`readUTF`), the
//! only string form `NbtIo` uses. Owned by unit mc.nbt.io.

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
            // Java DataOutput.writeUTF throws UTFDataFormatException here;
            // NbtIo.StringFallbackDataOutput catches it and writes "" instead.
            // We surface the overflow and let the caller decide (see NbtIo).
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("encoded string too long: {} bytes", encoded.len()),
            ));
        }
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
        match cesu8::from_java_cesu8(&buf) {
            Ok(s) => Ok(s.into_owned()),
            Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
        }
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
