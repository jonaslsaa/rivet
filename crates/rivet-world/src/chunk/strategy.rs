//! Port of `net.minecraft.world.level.chunk.Strategy<T>` (MC 26.2).
//!
//! Encapsulates the per-container type (block states vs biomes): the global
//! id map, the palette-width transitions (the exact per-bit-count
//! `Configuration` ladder), the bits-per-axis index packing, and the entry
//! count. Java's abstract `getConfigurationForBitCount` dispatch (two anonymous
//! `Strategy` subclasses) is mirrored with a `StrategyKind` discriminant.

use crate::chunk::configuration::{Configuration, PaletteFactoryKind};
use crate::chunk::palette::{GlobalIdMap, GlobalPalette, Palette, ceillog2};

/// The two Java `Strategy` subclasses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrategyKind {
    /// `Strategy.createForBlockStates` — 4 bits per axis (16×16×16).
    BlockStates,
    /// `Strategy.createForBiomes` — 2 bits per axis (4×4×4).
    Biomes,
}

/// `net.minecraft.world.level.chunk.Strategy<T>`.
pub struct Strategy<T: Clone + Send + 'static> {
    global_map: Box<dyn GlobalIdMap<T>>,
    kind: StrategyKind,
    global_palette_bits_in_memory: i32,
    bits_per_axis: i32,
    entry_count: i32,
}

impl<T: Clone + Send + 'static> Clone for Strategy<T> {
    fn clone(&self) -> Self {
        Strategy {
            global_map: self.global_map.clone_box(),
            kind: self.kind,
            global_palette_bits_in_memory: self.global_palette_bits_in_memory,
            bits_per_axis: self.bits_per_axis,
            entry_count: self.entry_count,
        }
    }
}

impl<T: Clone + Send + 'static> Strategy<T> {
    /// Java's private `Strategy(IdMap, bitsPerAxis)`.
    fn new(global_map: Box<dyn GlobalIdMap<T>>, kind: StrategyKind, bits_per_axis: i32) -> Self {
        let size = global_map.size();
        let global_palette_bits_in_memory = ceillog2(size);
        let entry_count = 1i32 << (bits_per_axis * 3);
        Strategy {
            global_map,
            kind,
            global_palette_bits_in_memory,
            bits_per_axis,
            entry_count,
        }
    }

    /// `Strategy.createForBlockStates(IdMap<T>)` — the strategy used for a
    /// `PalettedContainer<BlockState>`: bits-per-axis 4, so a 16×16×16 section
    /// has 4096 entries.
    pub fn create_for_block_states(registry: Box<dyn GlobalIdMap<T>>) -> Self {
        Self::new(registry, StrategyKind::BlockStates, 4)
    }

    /// `Strategy.createForBiomes(IdMap<T>)` — bits-per-axis 2 (4×4×4 = 64
    /// entries). Included for the shared `PalettedContainer`/`Palette` wire
    /// format even though biome containers are not part of the M1 #108 scope.
    pub fn create_for_biomes(registry: Box<dyn GlobalIdMap<T>>) -> Self {
        Self::new(registry, StrategyKind::Biomes, 2)
    }

    /// `entryCount()`.
    pub fn entry_count(&self) -> i32 {
        self.entry_count
    }

    /// `getIndex(int x, int y, int z)` — `(y << bitsPerAxis | z) << bitsPerAxis | x`.
    pub fn get_index(&self, x: i32, y: i32, z: i32) -> usize {
        ((y << self.bits_per_axis | z) << self.bits_per_axis | x) as usize
    }

    /// `globalMap()`.
    pub fn global_map(&self) -> &dyn GlobalIdMap<T> {
        self.global_map.as_ref()
    }

    /// `getConfigurationForBitCount(int entryBits)` — the abstract Java
    /// dispatch over the two strategy kinds.
    pub fn configuration_for_bit_count(&self, entry_bits: i32) -> Configuration {
        match self.kind {
            StrategyKind::BlockStates => match entry_bits {
                0 => Self::zero_bits(),
                1..=4 => Self::four_bits_linear(),
                5 => Self::five_bits_hashmap(),
                6 => Self::six_bits_hashmap(),
                7 => Self::seven_bits_hashmap(),
                8 => Self::eight_bits_hashmap(),
                _ => Configuration::global(self.global_palette_bits_in_memory, entry_bits),
            },
            StrategyKind::Biomes => match entry_bits {
                0 => Self::zero_bits(),
                1 => Self::one_bit_linear(),
                2 => Self::two_bits_linear(),
                3 => Self::three_bits_linear(),
                _ => Configuration::global(self.global_palette_bits_in_memory, entry_bits),
            },
        }
    }

    /// `getConfigurationForPaletteSize(int paletteSize)` —
    /// `getConfigurationForBitCount(ceillog2(paletteSize))`.
    pub fn configuration_for_palette_size(&self, palette_size: i32) -> Configuration {
        let bits = ceillog2(palette_size);
        self.configuration_for_bit_count(bits)
    }

    // The Java static `Configuration` constants.
    pub fn zero_bits() -> Configuration {
        Configuration::simple(PaletteFactoryKind::SingleValue, 0)
    }
    pub fn one_bit_linear() -> Configuration {
        Configuration::simple(PaletteFactoryKind::Linear, 1)
    }
    pub fn two_bits_linear() -> Configuration {
        Configuration::simple(PaletteFactoryKind::Linear, 2)
    }
    pub fn three_bits_linear() -> Configuration {
        Configuration::simple(PaletteFactoryKind::Linear, 3)
    }
    pub fn four_bits_linear() -> Configuration {
        Configuration::simple(PaletteFactoryKind::Linear, 4)
    }
    pub fn five_bits_hashmap() -> Configuration {
        Configuration::simple(PaletteFactoryKind::HashMap, 5)
    }
    pub fn six_bits_hashmap() -> Configuration {
        Configuration::simple(PaletteFactoryKind::HashMap, 6)
    }
    pub fn seven_bits_hashmap() -> Configuration {
        Configuration::simple(PaletteFactoryKind::HashMap, 7)
    }
    pub fn eight_bits_hashmap() -> Configuration {
        Configuration::simple(PaletteFactoryKind::HashMap, 8)
    }

    /// The global palette's in-memory width (`ceillog2(globalMap.size())`).
    pub fn global_palette_bits_in_memory(&self) -> i32 {
        self.global_palette_bits_in_memory
    }
}

impl<T: Clone + PartialEq + Send + 'static> Strategy<T> {
    /// `globalPalette()` — a `GlobalPalette` over this strategy's global map
    /// (Java shares one instance; the container re-creates it on demand, so
    /// freshness is unobservable).
    pub fn global_palette(&self) -> Box<dyn Palette<T>> {
        Box::new(GlobalPalette::new(self.global_map.clone_box()))
    }
}
