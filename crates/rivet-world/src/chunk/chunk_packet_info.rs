//! `io.papermc.paper.antixray.ChunkPacketInfo<T>` — per-section metadata
//! populated while serializing a chunk's block-state sections into the wire
//! buffer for Anti-Xray obfuscation.
//!
//! Mirrors Paper's `ChunkPacketInfo<T>` fields and accessors:
//! - `bits`: per-section `configuration.bitsInMemory()`
//! - `palettes`: per-section `Palette<T>` snapshot captured at wire-write time
//! - `indexes`: per-section byte offset (`buffer.writerIndex()`) where the
//!   raw `long[]` storage begins in the serialized chunk buffer
//! - `preset_values`: per-section Anti-Xray preset values from
//!   `PalettedContainer`
//! - `buffer`: optional serialized chunk-sections byte buffer
//! - `is_written(chunk_section_index)`: `bits[chunk_section_index] != 0`

use crate::chunk::palette::Palette;

/// `io.papermc.paper.antixray.ChunkPacketInfo<T>`.
pub struct ChunkPacketInfo<T: Clone + PartialEq + Send + Sync + 'static> {
    bits: Vec<i32>,
    palettes: Vec<Option<Box<dyn Palette<T> + Send + Sync>>>,
    indexes: Vec<i32>,
    preset_values: Vec<Option<Vec<T>>>,
    buffer: Option<Vec<u8>>,
}

impl<T: Clone + PartialEq + Send + Sync + 'static> ChunkPacketInfo<T> {
    /// Allocates empty per-section metadata slots for `section_count` chunk
    /// sections (Java `new ChunkPacketInfo<>(chunkPacket, chunk)`).
    pub fn new(section_count: usize) -> Self {
        let mut palettes = Vec::with_capacity(section_count);
        for _ in 0..section_count {
            palettes.push(None);
        }
        Self {
            bits: vec![0; section_count],
            palettes,
            indexes: vec![0; section_count],
            preset_values: vec![None; section_count],
            buffer: None,
        }
    }

    /// Number of section slots in this metadata table.
    pub fn section_count(&self) -> usize {
        self.bits.len()
    }

    /// `getBuffer()`.
    pub fn buffer(&self) -> Option<&[u8]> {
        self.buffer.as_deref()
    }

    /// `setBuffer(byte[])`.
    pub fn set_buffer(&mut self, buffer: Vec<u8>) {
        self.buffer = Some(buffer);
    }

    /// `getBits(int chunkSectionIndex)`.
    pub fn get_bits(&self, chunk_section_index: usize) -> i32 {
        self.bits[chunk_section_index]
    }

    /// `setBits(int chunkSectionIndex, int bits)`.
    pub fn set_bits(&mut self, chunk_section_index: usize, bits: i32) {
        self.bits[chunk_section_index] = bits;
    }

    /// `getPalette(int chunkSectionIndex)`.
    pub fn get_palette(
        &self,
        chunk_section_index: usize,
    ) -> Option<&(dyn Palette<T> + Send + Sync)> {
        self.palettes[chunk_section_index].as_deref()
    }

    /// `setPalette(int chunkSectionIndex, Palette<T>)`.
    pub fn set_palette(
        &mut self,
        chunk_section_index: usize,
        palette: Box<dyn Palette<T> + Send + Sync>,
    ) {
        self.palettes[chunk_section_index] = Some(palette);
    }

    /// `getIndex(int chunkSectionIndex)`.
    pub fn get_index(&self, chunk_section_index: usize) -> i32 {
        self.indexes[chunk_section_index]
    }

    /// `setIndex(int chunkSectionIndex, int index)`.
    pub fn set_index(&mut self, chunk_section_index: usize, index: i32) {
        self.indexes[chunk_section_index] = index;
    }

    /// `getPresetValues(int chunkSectionIndex)`.
    pub fn get_preset_values(&self, chunk_section_index: usize) -> Option<&[T]> {
        self.preset_values[chunk_section_index].as_deref()
    }

    /// `setPresetValues(int chunkSectionIndex, T[])`.
    pub fn set_preset_values(&mut self, chunk_section_index: usize, preset_values: Option<Vec<T>>) {
        self.preset_values[chunk_section_index] = preset_values;
    }

    /// `isWritten(int chunkSectionIndex)` — `bits[chunkSectionIndex] != 0`.
    pub fn is_written(&self, chunk_section_index: usize) -> bool {
        self.bits[chunk_section_index] != 0
    }
}
