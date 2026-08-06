//! Port of `net.minecraft.world.level.chunk.Configuration` (MC 26.2).
//!
//! The strategy's per-bit-count configuration: which palette factory to use
//! and how many bits the in-memory storage and the wire (on-storage) layout
//! use. Java models this as a sealed interface with two records (`Global` and
//! `Simple`); the Rust port mirrors that with an enum, preserving the
//! `equals` semantics (value equality over the fields).

use crate::chunk::palette::Palette;
use crate::chunk::strategy::Strategy;

/// `Palette.Factory` — the concrete factory, mirrored as an enum because the
/// three Java factories are singletons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaletteFactoryKind {
    /// `Strategy.SINGLE_VALUE_PALETTE_FACTORY` — `SingleValuePalette::create`.
    SingleValue,
    /// `LINEAR_PALETTE_FACTORY` — `LinearPalette::create`.
    Linear,
    /// `HASHMAP_PALETTE_FACTORY` — `HashMapPalette::create`.
    HashMap,
}

impl PaletteFactoryKind {
    /// `Factory.create(int bits, List<T> entries)`.
    pub fn create<T: Clone + PartialEq + Send + 'static>(
        self,
        bits: i32,
        palette_entries: Vec<T>,
    ) -> Box<dyn Palette<T>> {
        match self {
            PaletteFactoryKind::SingleValue => {
                crate::chunk::palette::SingleValuePalette::create(bits, palette_entries)
            }
            PaletteFactoryKind::Linear => {
                crate::chunk::palette::LinearPalette::create(bits, palette_entries)
            }
            PaletteFactoryKind::HashMap => {
                crate::chunk::palette::HashMapPalette::create(bits, palette_entries)
            }
        }
    }
}

/// `net.minecraft.world.level.chunk.Configuration` (the two Java records).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Configuration {
    /// `Configuration.Global(int bitsInMemory, int bitsInStorage)`.
    Global {
        bits_in_memory: i32,
        bits_in_storage: i32,
    },
    /// `Configuration.Simple(Palette.Factory factory, int bits)`.
    Simple {
        factory: PaletteFactoryKind,
        bits: i32,
    },
}

impl Configuration {
    /// `alwaysRepack()` — true only for `Global`.
    pub fn always_repack(&self) -> bool {
        matches!(self, Configuration::Global { .. })
    }

    /// `bitsInMemory()`.
    pub fn bits_in_memory(&self) -> i32 {
        match self {
            Configuration::Global { bits_in_memory, .. } => *bits_in_memory,
            Configuration::Simple { bits, .. } => *bits,
        }
    }

    /// `bitsInStorage()`.
    pub fn bits_in_storage(&self) -> i32 {
        match self {
            Configuration::Global {
                bits_in_storage, ..
            } => *bits_in_storage,
            Configuration::Simple { bits, .. } => *bits,
        }
    }

    /// `createPalette(Strategy, List<T>)`.
    pub fn create_palette<T: Clone + PartialEq + Send + 'static>(
        &self,
        strategy: &Strategy<T>,
        palette_entries: Vec<T>,
    ) -> Box<dyn Palette<T>> {
        match self {
            Configuration::Global { .. } => strategy.global_palette(),
            Configuration::Simple { factory, bits } => factory.create(*bits, palette_entries),
        }
    }
}

/// Convenience constructors matching the Java static constants' shape.
impl Configuration {
    pub fn global(bits_in_memory: i32, bits_in_storage: i32) -> Self {
        Configuration::Global {
            bits_in_memory,
            bits_in_storage,
        }
    }

    pub fn simple(factory: PaletteFactoryKind, bits: i32) -> Self {
        Configuration::Simple { factory, bits }
    }
}
