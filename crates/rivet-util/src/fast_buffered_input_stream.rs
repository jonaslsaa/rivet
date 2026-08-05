//! `net.minecraft.util.FastBufferedInputStream` — an 8KB buffering `Read` that
//! avoids refills for reads within the buffer. Faithful port of the Java class
//! (DEFAULT_BUFFER_SIZE = 8192).
//!
//! STUB(mc.nbt.io) — minimal faithful surface for the compressed read path.

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

    fn fill(&mut self) -> io::Result<()> {
        let n = self.inner.read(&mut self.buffer)?;
        self.limit = n;
        self.position = 0;
        Ok(())
    }
}

impl<R: Read> Read for FastBufferedInputStream<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if self.position >= self.limit {
            self.fill()?;
            if self.position >= self.limit {
                return Ok(0);
            }
        }
        let available = self.bytes_in_buffer();
        let to_copy = available.min(output.len());
        output[..to_copy].copy_from_slice(&self.buffer[self.position..self.position + to_copy]);
        self.position += to_copy;
        Ok(to_copy)
    }
}
