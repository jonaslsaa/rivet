//! `net.minecraft.util.FastBufferedInputStream` — an 8KB buffering `Read` that
//! avoids refills for reads within the buffer. Faithful port of the Java class
//! (DEFAULT_BUFFER_SIZE = 8192).
//!
//! Only the buffered-read shape the compressed NBT path needs is ported.
//! Java's `skip`/`available`/`close` are omitted: the NBT read path only ever
//! pulls bytes through `read`, and dropping the stream releases the inner one.

use std::io::{self, Read};

/// The Java constant.
pub const DEFAULT_BUFFER_SIZE: usize = 8192;

/// `FastBufferedInputStream` — buffers the underlying reader in 8KB chunks.
pub struct FastBufferedInputStream<R: Read> {
    inner: R,
    buffer: Vec<u8>,
    limit: usize,
    position: usize,
}

impl<R: Read> FastBufferedInputStream<R> {
    /// `new FastBufferedInputStream(InputStream)`.
    pub fn new(inner: R) -> Self {
        Self::with_buffer_size(inner, DEFAULT_BUFFER_SIZE)
    }

    /// `new FastBufferedInputStream(InputStream, int bufferSize)`.
    pub fn with_buffer_size(inner: R, buffer_size: usize) -> Self {
        FastBufferedInputStream {
            inner,
            buffer: vec![0u8; buffer_size],
            limit: 0,
            position: 0,
        }
    }

    fn bytes_in_buffer(&self) -> usize {
        self.limit - self.position
    }

    /// Java's `fill()`: reads one buffer-full; on EOF the buffer stays empty
    /// (`limit` 0) so the next read reports EOF.
    fn fill(&mut self) -> io::Result<()> {
        let n = self.inner.read(&mut self.buffer)?;
        self.limit = n;
        self.position = 0;
        Ok(())
    }
}

impl<R: Read> Read for FastBufferedInputStream<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        // Rust's `Read` contract requires a zero-length read to return `Ok(0)`
        // (Java's `read(byte[], 0, 0)` would return -1 on an empty buffer, but
        // that quirk is unobservable through the NBT path and would violate the
        // trait contract).
        if output.is_empty() {
            return Ok(0);
        }
        let mut available = self.bytes_in_buffer();
        if available == 0 {
            // Java: a read at least as large as the buffer bypasses the buffer
            // and reads the underlying stream directly.
            if output.len() >= self.buffer.len() {
                return self.inner.read(output);
            }
            self.fill()?;
            available = self.bytes_in_buffer();
            if available == 0 {
                return Ok(0); // EOF — Java returns -1.
            }
        }
        let to_copy = available.min(output.len());
        output[..to_copy].copy_from_slice(&self.buffer[self.position..self.position + to_copy]);
        self.position += to_copy;
        Ok(to_copy)
    }
}
