//! Port of `net.minecraft.world.level.chunk.LevelChunkSection` (MC 26.2) — the
//! wire-visible slice.
//!
//! Java: `LevelChunkSection.java` in `working/Paper`. The section serializes as
//! `[nonEmptyBlockCount i16 BE][fluidCount i16 BE][PalettedContainer<BlockState>]
//! [PalettedContainer<Holder<Biome>>]`, where `PalettedContainer.write` is the
//! `[bits byte][palette][raw longs]` triple from `chunk::paletted_container`.
//! `getSerializedSize()` = `4 + states.getSerializedSize() +
//! biomes.getSerializedSize()`.
//!
//! Ported ahead of the `mc.world.level.chunk` manifest unit because issue #100
//! needs the wire write path to fill the `ClientboundLevelChunkPacketData`
//! opaque sections buffer. The Moonrise block-counting fast path
//! (`moonrise$countEntries`) is ported at the storage level
//! (rivet-util, issue #216), but the section-level lists it feeds — the
//! `FULL_LIST` single-value shortcut, `specialCollidingBlocks`, and the
//! `tickingBlocks` coordinate list — and the set-block/fluid accessors are
//! deferred with the owning unit; the two wire-visible count fields
//! (non-empty blocks, fluids) are ported.
//!
//! RivetTodo(#216): the Moonrise `FULL_LIST` single-value block-counting
//! shortcut, the `specialCollidingBlocks`/`tickingBlocks` lists fed by
//! `countEntries`, the Anti-Xray `chunkPacketInfo`/`chunkSectionIndex` write
//! params, and the set-block/fluid accessors are not ported (deferred to the
//! M2 chunk-storage epic #15); the owning `mc.world.level.chunk.access` unit
//! replaces the superflat-safe predicate defaults (including the separate
//! block- vs fluid-random-tick predicates) with real `BlockBehaviour` flags.

use crate::chunk::paletted_container::PalettedContainer;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;

/// `LevelChunkSection.BIOME_CONTAINER_BITS`.
pub const BIOME_CONTAINER_BITS: i32 = 2;

/// `LevelChunkSection` — the block/biome container pair plus the wire-visible
/// count fields.
pub struct LevelChunkSection<
    T: Clone + PartialEq + Send + 'static,
    B: Clone + PartialEq + Send + 'static,
> {
    /// `nonEmptyBlockCount` — blocks that are not air.
    non_empty_block_count: i16,
    /// `fluidCount` — blocks whose fluid state is non-empty.
    fluid_count: i16,
    /// `tickingBlockCount` — randomly-ticking blocks.
    ticking_block_count: i16,
    /// `tickingFluidCount` — randomly-ticking fluids.
    ticking_fluid_count: i16,
    /// `states` — the 16×16×16 block-state container.
    states: PalettedContainer<T>,
    /// `biomes` — the 4×4×4 biome container.
    biomes: PalettedContainer<B>,
}

impl<
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
> LevelChunkSection<T, B>
{
    /// `LevelChunkSection(PalettedContainer<BlockState> states,
    /// PalettedContainer<Holder<Biome>> biomes)` — runs `recalcBlockCounts()`.
    pub fn new(
        states: PalettedContainer<T>,
        biomes: PalettedContainer<B>,
        is_air: impl Fn(&T) -> bool,
    ) -> Self {
        let mut section = LevelChunkSection {
            non_empty_block_count: 0,
            fluid_count: 0,
            ticking_block_count: 0,
            ticking_fluid_count: 0,
            states,
            biomes,
        };
        section.recalc_block_counts(&is_air, &|_| false, &|_| true);
        section
    }

    /// `getBlockState(int, int, int)`.
    pub fn get_block_state(&self, x: i32, y: i32, z: i32) -> T {
        self.states.get(x, y, z)
    }

    /// `hasOnlyAir()`.
    pub fn has_only_air(&self) -> bool {
        self.non_empty_block_count == 0
    }

    /// `hasFluid()`.
    pub fn has_fluid(&self) -> bool {
        self.fluid_count > 0
    }

    /// `nonEmptyBlockCount`.
    pub fn non_empty_block_count(&self) -> i16 {
        self.non_empty_block_count
    }

    /// `fluidCount`.
    pub fn fluid_count(&self) -> i16 {
        self.fluid_count
    }

    /// `getStates()`.
    pub fn states(&self) -> &PalettedContainer<T> {
        &self.states
    }

    /// `getBiomes()`.
    pub fn biomes(&self) -> &PalettedContainer<B> {
        &self.biomes
    }

    /// `recalcBlockCounts()` — resets the count fields, then tallies them from
    /// the container's palette via [`PalettedContainer::count`] (which mirrors
    /// Java's per-palette-entry summation). Java's Moonrise fast path
    /// (`moonrise$countEntries`, ported at the storage level, issue #216) is
    /// the same summation with coordinate lists the deferred section-level
    /// lists need; it is not wired here. Two simplifications are
    /// superflat-safe and deferred with the owning unit (#216):
    /// `is_randomly_ticking` doubles for both the block and the fluid
    /// random-tick predicates (Paper uses `state.isRandomlyTicking()` for the
    /// block and `fluid.isRandomlyTicking()` for the fluid), and
    /// `fluid_is_empty` replaces real `BlockBehaviour` fluid flags. Neither
    /// affects the wire counts (`nonEmptyBlockCount`/`fluidCount`), which are
    /// exact for the air + stone content.
    ///
    /// The superflat section's stone layer: stone is not air and not randomly
    /// ticking with an empty fluid state, so `nonEmptyBlockCount` is exactly
    /// the number of stone entries and the fluid/ticking counts are 0.
    pub fn recalc_block_counts(
        &mut self,
        is_air: &dyn Fn(&T) -> bool,
        is_randomly_ticking: &dyn Fn(&T) -> bool,
        fluid_is_empty: &dyn Fn(&T) -> bool,
    ) {
        self.non_empty_block_count = 0;
        self.fluid_count = 0;
        self.ticking_block_count = 0;
        self.ticking_fluid_count = 0;
        if self.states.maybe_has(|state| !is_air(state)) {
            // Tally into locals so the `count` closure does not capture `self`
            // while `self.states` is borrowed; the fields are updated after.
            let (mut non_empty, mut fluid) = (0i32, 0i32);
            let (mut ticking_block, mut ticking_fluid) = (0i32, 0i32);
            self.states.count(|state, count| {
                if is_air(&state) {
                    return;
                }
                non_empty += count;
                if is_randomly_ticking(&state) {
                    ticking_block += count;
                }
                if !fluid_is_empty(&state) {
                    fluid += count;
                    if is_randomly_ticking(&state) {
                        ticking_fluid += count;
                    }
                }
            });
            self.non_empty_block_count = non_empty as i16;
            self.fluid_count = fluid as i16;
            self.ticking_block_count = ticking_block as i16;
            self.ticking_fluid_count = ticking_fluid as i16;
        }
    }

    /// `getSerializedSize()` — `4 + states.getSerializedSize() +
    /// biomes.getSerializedSize()`.
    pub fn get_serialized_size(&self) -> i32 {
        4 + self.states.get_serialized_size() + self.biomes.get_serialized_size()
    }

    /// `write(FriendlyByteBuf, ...)` — the wire form. Java's Anti-Xray
    /// `chunkPacketInfo`/`chunkSectionIndex` parameters are a no-op for the
    /// superflat send path (deferred with Anti-Xray).
    pub fn write(&self, buffer: &mut FriendlyByteBuf) {
        buffer.write_short(self.non_empty_block_count);
        buffer.write_short(self.fluid_count);
        self.states.write(buffer);
        self.biomes.write(buffer);
    }

    /// `read(FriendlyByteBuf)` — adopts the wire containers and the wire counts
    /// (Java does not recalc on read). `isClient`/`specialCollidingBlocks` are
    /// client-side Moonrise fields deferred with the owning unit.
    pub fn read(&mut self, buffer: &mut FriendlyByteBuf) {
        self.non_empty_block_count = buffer.read_short();
        self.fluid_count = buffer.read_short();
        self.states.read(buffer);
        let mut biomes = self.biomes.recreate();
        biomes.read(buffer);
        self.biomes = biomes;
    }

    /// `tickingBlockCount`.
    pub fn ticking_block_count(&self) -> i16 {
        self.ticking_block_count
    }

    /// `tickingFluidCount`.
    pub fn ticking_fluid_count(&self) -> i16 {
        self.ticking_fluid_count
    }
}
