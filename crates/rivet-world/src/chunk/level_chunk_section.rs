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
//! The Paper block-counting (`moonrise$countEntries`) fields are ported
//! (issue #216): `specialCollidingBlocks` (a predicate-count shortcut) and the
//! `tickingBlocks` coordinate list are fed by `recalcBlockCounts`'s storage
//! walk, and `read()` adopts Java's client-side forced-special-colliding
//! behavior. `specialCollidingBlocks` is never observable from the wire — it
//! only drives the Moonrise collision fast path — so the `is_special_colliding`
//! predicate is parameterized like `is_air` until the real `BlockBehaviour`
//! collision flags land (the `mc.world.level.block` slice replaces the
//! superflat-safe defaults, mirroring `is_randomly_ticking`/`fluid_is_empty`).
//!
//! Deferred with the owning unit: the Anti-Xray `chunkPacketInfo`/
//! `chunkSectionIndex` write params (with `paper.antixray`), and the
//! set-block/fluid accessors (with the mutator unit; the section read/write
//! paths do not need them).

use crate::chunk::moonrise_short_list::ShortList;
use crate::chunk::paletted_container::PalettedContainer;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;

/// `LevelChunkSection.BIOME_CONTAINER_BITS`.
pub const BIOME_CONTAINER_BITS: i32 = 2;

/// `LevelChunkSection.CLIENT_FORCED_SPECIAL_COLLIDING_BLOCKS` — the sentinel a
/// client-side (read-in) section's `specialCollidingBlocks` is forced to when
/// the section has any non-air block that is special-colliding.
const CLIENT_FORCED_SPECIAL_COLLIDING_BLOCKS: i16 = 9999;

/// The `16*16*16` ascending index list Java's `recalcBlockCounts` reuses as
/// the single-value palette's coordinate list (`FULL_LIST`). A fresh allocation
/// is equivalent: it is consumed once per recalc and never mutated.
const SECTION_SIZE: usize = 16 * 16 * 16;

/// `LevelChunkSection` — the block/biome container pair plus the wire-visible
/// count fields and the Moonrise block-counting lists.
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
    /// `specialCollidingBlocks` — the count of special-colliding blocks in the
    /// section (a cached `hasLargeCollisionShape`/`MOVING_PISTON` tally).
    special_colliding_blocks: i16,
    /// `tickingBlocks` — the Moonrise insertion-ordered list of packed
    /// positions (`x | z<<4 | y<<8`) of randomly-ticking blocks.
    ticking_blocks: ShortList,
}

impl<
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
> LevelChunkSection<T, B>
{
    /// `LevelChunkSection(PalettedContainer<BlockState> states,
    /// PalettedContainer<Holder<Biome>> biomes)` — runs `recalcBlockCounts()`.
    ///
    /// `is_air` classifies states for the recalc; the `is_randomly_ticking`,
    /// `fluid_is_empty`, and `is_special_colliding` defaults are the
    /// superflat-safe stand-ins documented on [`recalc_block_counts`].
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
            special_colliding_blocks: 0,
            ticking_blocks: ShortList::new(),
        };
        section.recalc_block_counts(&is_air, &|_| false, &|_| true, &|_| false);
        section
    }

    /// `getBlockState(int, int, int)`.
    pub fn get_block_state(&self, x: i32, y: i32, z: i32) -> T {
        self.states.get(x, y, z)
    }

    /// `getNoiseBiome(int, int, int)` — the 4×4×4 biome container read
    /// (section-local quart coords). Java passes the already-masked quart
    /// coords through to `PalettedContainer.get` unmasked; the callers (the
    /// chunk `getNoiseBiome`) mask to `& 3` first.
    pub fn get_noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> B {
        self.biomes.get(quart_x, quart_y, quart_z)
    }

    /// `hasOnlyAir()`.
    pub fn has_only_air(&self) -> bool {
        self.non_empty_block_count == 0
    }

    /// `hasFluid()`.
    pub fn has_fluid(&self) -> bool {
        self.fluid_count > 0
    }

    /// `isRandomlyTicking()` — `isRandomlyTickingBlocks() ||
    /// isRandomlyTickingFluids()`.
    pub fn is_randomly_ticking(&self) -> bool {
        self.ticking_block_count > 0 || self.ticking_fluid_count > 0
    }

    /// `isRandomlyTickingBlocks()`.
    pub fn is_randomly_ticking_blocks(&self) -> bool {
        self.ticking_block_count > 0
    }

    /// `isRandomlyTickingFluids()`.
    pub fn is_randomly_ticking_fluids(&self) -> bool {
        self.ticking_fluid_count > 0
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

    /// `moonrise$hasSpecialCollidingBlocks()`.
    pub fn has_special_colliding_blocks(&self) -> bool {
        self.special_colliding_blocks != 0
    }

    /// `moonrise$getTickingBlockList()`.
    pub fn ticking_blocks(&self) -> &ShortList {
        &self.ticking_blocks
    }

    /// `recalcBlockCounts()` — resets the count fields and the Moonrise lists,
    /// then tallies them from the container's palette via the storage walk.
    ///
    /// Java's Moonrise fast path (issue #216) is ported faithfully: when the
    /// palette holds a single value, `FULL_LIST` (the ascending `0..4096`
    /// index list) is used instead of `moonrise$countEntries()`; otherwise the
    /// per-palette-id coordinate groups are read from the storage directly.
    /// Each group contributes the palette value's counts; the packed positions
    /// of randomly-ticking values are appended to `tickingBlocks` (Java's
    /// `setMinCapacity` + `add` loop — the `ShortList` dedupes, so the
    /// coordinate list ends up with exactly the distinct ticking positions).
    ///
    /// Three simplifications are superflat-safe and deferred with the owning
    /// unit (#216): `is_randomly_ticking` doubles for both the block and the
    /// fluid random-tick predicates (Paper uses `state.isRandomlyTicking()` for
    /// the block and `fluid.isRandomlyTicking()` for the fluid),
    /// `fluid_is_empty` replaces real `BlockBehaviour` fluid flags, and
    /// `is_special_colliding` replaces `CollisionUtil.isSpecialCollidingBlock`
    /// (the `shapeExceedsCube` cache flag / `MOVING_PISTON` — not in the
    /// generated `block_behaviors` table). None of these affect the wire
    /// counts (`nonEmptyBlockCount`/`fluidCount`), which are exact for the air
    /// + stone content.
    pub fn recalc_block_counts(
        &mut self,
        is_air: &dyn Fn(&T) -> bool,
        is_randomly_ticking: &dyn Fn(&T) -> bool,
        fluid_is_empty: &dyn Fn(&T) -> bool,
        is_special_colliding: &dyn Fn(&T) -> bool,
    ) {
        self.non_empty_block_count = 0;
        self.fluid_count = 0;
        self.ticking_block_count = 0;
        self.ticking_fluid_count = 0;
        self.special_colliding_blocks = 0;
        self.ticking_blocks.clear();
        if self.states.maybe_has(|state| !is_air(state)) {
            // Tally into locals so the closures do not capture `self` while
            // `self.states` is borrowed; the fields are updated after.
            let mut non_empty = 0i32;
            let mut fluid = 0i32;
            let mut ticking_block = 0i32;
            let mut ticking_fluid = 0i32;
            let mut special_colliding = 0i32;

            let palette_size = self.states.palette_size();
            let counts: Vec<(i32, Vec<i16>)> = if palette_size == 1 {
                vec![(0, (0..SECTION_SIZE as i16).collect())]
            } else {
                self.states.count_entries()
            };

            for (palette_idx, coordinates) in counts {
                let palette_count = coordinates.len() as i32;
                let state = self.states.value_for_palette(palette_idx);
                if is_air(&state) {
                    continue;
                }
                non_empty += palette_count;
                if is_special_colliding(&state) {
                    special_colliding += palette_count;
                }
                if is_randomly_ticking(&state) {
                    ticking_block += palette_count;
                    // Java's setMinCapacity(Math.min((rawLen + size) * 3 / 2,
                    // 16*16*16)) — a capacity-only allocation hint; its exact
                    // value is unobservable (`ShortList` contents never change).
                    // `rawLen` is Java's raw backing-array length (>= the
                    // logical size); `coordinates.len()` is the logical length,
                    // so the hint value may differ, but the effect is identical.
                    self.ticking_blocks.set_min_capacity(std::cmp::min(
                        (coordinates.len() + self.ticking_blocks.size()) * 3 / 2,
                        SECTION_SIZE,
                    ));
                    for packed in coordinates {
                        self.ticking_blocks.add(packed);
                    }
                }
                if !fluid_is_empty(&state) {
                    fluid += palette_count;
                    if is_randomly_ticking(&state) {
                        ticking_fluid += palette_count;
                    }
                }
            }

            self.non_empty_block_count = non_empty as i16;
            self.fluid_count = fluid as i16;
            self.ticking_block_count = ticking_block as i16;
            self.ticking_fluid_count = ticking_fluid as i16;
            self.special_colliding_blocks = special_colliding as i16;
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
    /// (Java does not recalc on read). Java's read sets `isClient = true`
    /// (stored only for its `setBlockState` client-side shortcut, which the
    /// port defers) and forces `specialCollidingBlocks` to the
    /// `CLIENT_FORCED_SPECIAL_COLLIDING_BLOCKS` sentinel when the section has
    /// any non-empty block and `maybeHas` a special-colliding state, else 0.
    /// Java leaves `tickingBlocks` untouched here; the port's read-in sections
    /// are freshly constructed, so the list stays empty (client bookkeeping
    /// starts at the first `setBlockState`).
    ///
    /// `is_special_colliding` is the caller's `CollisionUtil.isSpecialCollidingBlock`
    /// equivalent — the section is generic over `T`, so the real `BlockBehaviour`
    /// collision flags are threaded in like `is_air` on [`new`](Self::new).
    pub fn read(
        &mut self,
        buffer: &mut FriendlyByteBuf,
        is_special_colliding: &dyn Fn(&T) -> bool,
    ) {
        self.non_empty_block_count = buffer.read_short();
        self.fluid_count = buffer.read_short();
        self.states.read(buffer);
        let mut biomes = self.biomes.recreate();
        biomes.read(buffer);
        self.biomes = biomes;
        self.special_colliding_blocks = if self.non_empty_block_count != 0
            && self.states.maybe_has(|state| is_special_colliding(state))
        {
            CLIENT_FORCED_SPECIAL_COLLIDING_BLOCKS
        } else {
            0
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::palette::GlobalIdMap;
    use crate::chunk::paletted_container::PalettedContainer;
    use crate::chunk::strategy::Strategy;
    use bytes::BytesMut;

    #[derive(Clone, Copy)]
    struct TestGlobalMap;
    impl GlobalIdMap<u8> for TestGlobalMap {
        fn get_id(&self, value: &u8) -> i32 {
            *value as i32
        }
        fn by_id_or_throw(&self, id: i32) -> u8 {
            id as u8
        }
        fn size(&self) -> i32 {
            256
        }
        fn by_id(&self, id: i32) -> Option<u8> {
            Some(id as u8)
        }
        fn clone_box(&self) -> Box<dyn GlobalIdMap<u8>> {
            Box::new(*self)
        }
    }

    fn block_strategy() -> Strategy<u8> {
        Strategy::create_for_block_states(Box::new(TestGlobalMap))
    }
    fn biome_strategy() -> Strategy<u8> {
        Strategy::create_for_biomes(Box::new(TestGlobalMap))
    }
    fn is_air(state: &u8) -> bool {
        *state == 0
    }

    /// A fresh all-air section (`LevelChunkSection(PalettedContainer.of(0),
    /// PalettedContainer.of(0))`, which runs `recalcBlockCounts`).
    fn all_air_section() -> LevelChunkSection<u8, u8> {
        LevelChunkSection::new(
            PalettedContainer::new(0u8, block_strategy()),
            PalettedContainer::new(0u8, biome_strategy()),
            is_air,
        )
    }

    /// `recalcBlockCounts` on a single-value palette uses Java's `FULL_LIST`
    /// fast path: the whole `0..4096` index range is one group, so a section
    /// that is entirely one non-air, randomly-ticking value gets
    /// `nonEmptyBlockCount = 4096` and a `tickingBlocks` list of every distinct
    /// packed position.
    #[test]
    fn single_palette_uses_full_list_shortcut() {
        // `PalettedContainer.of(1)` fills all 16*16*16 cells with value 1
        // (non-air); the palette holds exactly one entry.
        let mut section = LevelChunkSection::new(
            PalettedContainer::new(1u8, block_strategy()),
            PalettedContainer::new(0u8, biome_strategy()),
            is_air,
        );
        assert_eq!(section.non_empty_block_count(), 4096);
        assert_eq!(section.fluid_count(), 0);
        assert!(!section.has_fluid());

        // Every packed position 0..4096 is distinct, so all are appended.
        section.recalc_block_counts(&is_air, &|_| true, &|_| true, &|_| false);
        assert_eq!(section.ticking_block_count(), 4096);
        assert_eq!(section.ticking_blocks().size(), 4096);
        for index in 0..4096 {
            assert_eq!(section.ticking_blocks().get_raw(index), index as i16);
        }
        assert!(section.is_randomly_ticking());
        assert!(section.is_randomly_ticking_blocks());
        assert!(!section.is_randomly_ticking_fluids());
        // An all-special-colliding single-value section counts every cell.
        section.recalc_block_counts(&is_air, &|_| false, &|_| true, &|_| true);
        assert_eq!(section.non_empty_block_count(), 4096);
        assert!(section.has_special_colliding_blocks());
    }

    /// A multi-value palette walks `moonrise$countEntries`: each palette id
    /// yields its distinct storage positions, and only the randomly-ticking
    /// values feed `tickingBlocks` (in first-appearance order, packed
    /// `x | z<<4 | y<<8`).
    #[test]
    fn multi_palette_counts_entries_and_packs_ticking_positions() {
        let mut states = PalettedContainer::new(0u8, block_strategy());
        // Value 1 at (0,0,0) → index 0; value 2 at (1,0,0) and (2,0,0) →
        // indices 1, 2; value 3 at (3,0,0) and (4,0,0) → indices 3, 4;
        // value 4 at (0,1,0) → index 1<<8 = 256.
        states.set(0, 0, 0, 1);
        states.set(1, 0, 0, 2);
        states.set(2, 0, 0, 2);
        states.set(3, 0, 0, 3);
        states.set(4, 0, 0, 3);
        states.set(0, 1, 0, 4);
        let mut section = LevelChunkSection::new(
            states,
            PalettedContainer::new(0u8, biome_strategy()),
            is_air,
        );

        // Values 2 and 4 randomly tick; value 3 is special-colliding.
        section.recalc_block_counts(&is_air, &|s| *s == 2 || *s == 4, &|_| true, &|s| *s == 3);
        assert_eq!(section.non_empty_block_count(), 6);
        assert_eq!(section.fluid_count(), 0);
        assert_eq!(section.ticking_block_count(), 3);
        assert_eq!(section.ticking_blocks().size(), 3);
        // First-appearance order: value 2's (1,0,0), (2,0,0), then value 4's
        // (0,1,0) packed as x | z<<4 | y<<8.
        assert_eq!(section.ticking_blocks().get_raw(0), 1);
        assert_eq!(section.ticking_blocks().get_raw(1), 2);
        assert_eq!(section.ticking_blocks().get_raw(2), 256);
        assert!(section.has_special_colliding_blocks());
        assert_eq!(section.ticking_block_count(), 3);
    }

    /// `specialCollidingBlocks` is tallied per palette group from the caller's
    /// `is_special_colliding` predicate; the getter is the `!= 0` sentinel
    /// test Java uses.
    #[test]
    fn special_colliding_blocks_counts_matching_values() {
        let mut states = PalettedContainer::new(0u8, block_strategy());
        states.set(0, 0, 0, 2);
        states.set(1, 0, 0, 3);
        states.set(2, 0, 0, 3);
        let mut section = LevelChunkSection::new(
            states,
            PalettedContainer::new(0u8, biome_strategy()),
            is_air,
        );
        section.recalc_block_counts(&is_air, &|_| false, &|_| true, &|s| *s == 3);
        assert_eq!(section.non_empty_block_count(), 3);
        assert!(section.has_special_colliding_blocks());

        // A predicate matching nothing leaves the count at zero.
        section.recalc_block_counts(&is_air, &|_| false, &|_| true, &|_| false);
        assert!(!section.has_special_colliding_blocks());
        assert_eq!(section.non_empty_block_count(), 3);
    }

    /// A wire round-trip mirrors Java's `read`: the wire counts are adopted
    /// as-is and a client-side (`isClient = true`) section forces
    /// `specialCollidingBlocks` to the `9999` sentinel when it holds any
    /// non-air block that `maybeHas` a special-colliding state.
    #[test]
    fn read_forces_client_special_colliding_sentinel() {
        let mut states = PalettedContainer::new(0u8, block_strategy());
        states.set(0, 0, 0, 1);
        let section = LevelChunkSection::new(
            states,
            PalettedContainer::new(0u8, biome_strategy()),
            is_air,
        );
        assert_eq!(section.non_empty_block_count(), 1);

        let mut buf = FriendlyByteBuf::new(BytesMut::new());
        section.write(&mut buf);
        let bytes = buf.into_inner().to_vec();

        // Matching predicate → sentinel forced; the wire contents are adopted
        // (no recalc — `tickingBlocks` stays empty like Java's client).
        let mut read_section = all_air_section();
        let mut buf = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        read_section.read(&mut buf, &|s| *s == 1);
        assert_eq!(read_section.non_empty_block_count(), 1);
        assert_eq!(read_section.get_block_state(0, 0, 0), 1);
        assert!(read_section.has_special_colliding_blocks());
        assert_eq!(read_section.ticking_blocks().size(), 0);

        // Non-matching predicate → sentinel not forced.
        let mut read_section = all_air_section();
        let mut buf = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        read_section.read(&mut buf, &|_| false);
        assert!(!read_section.has_special_colliding_blocks());

        // An all-air wire section never forces the sentinel.
        let all_air = all_air_section();
        let mut buf = FriendlyByteBuf::new(BytesMut::new());
        all_air.write(&mut buf);
        let bytes = buf.into_inner().to_vec();
        let mut read_section = all_air_section();
        let mut buf = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        read_section.read(&mut buf, &|s| *s == 1);
        assert_eq!(read_section.non_empty_block_count(), 0);
        assert!(!read_section.has_special_colliding_blocks());
    }

    /// `recalcBlockCounts` first resets the Moonrise bookkeeping, so a second
    /// recalc with different predicates clears `tickingBlocks` and the count
    /// fields rather than accumulating.
    #[test]
    fn recalc_resets_then_retallies() {
        let mut states = PalettedContainer::new(0u8, block_strategy());
        states.set(0, 0, 0, 1);
        states.set(0, 1, 0, 1);
        let mut section = LevelChunkSection::new(
            states,
            PalettedContainer::new(0u8, biome_strategy()),
            is_air,
        );
        section.recalc_block_counts(&is_air, &|_| true, &|_| true, &|_| false);
        assert_eq!(section.ticking_block_count(), 2);
        assert_eq!(section.ticking_blocks().size(), 2);

        // Now nothing ticks: the earlier list must be dropped, not appended to.
        section.recalc_block_counts(&is_air, &|_| false, &|_| true, &|_| false);
        assert_eq!(section.ticking_block_count(), 0);
        assert_eq!(section.ticking_blocks().size(), 0);
        assert_eq!(section.non_empty_block_count(), 2);
        assert!(!section.is_randomly_ticking());

        // The fluid branch: both states are non-empty fluid, so the (shared)
        // random-ticking predicate counts them as ticking fluids too. The
        // `is_randomly_ticking_fluids` getter reflects the fluid list, distinct
        // from the block list.
        section.recalc_block_counts(&is_air, &|_| true, &|s| *s != 1, &|_| false);
        assert_eq!(section.fluid_count(), 2);
        assert_eq!(section.ticking_fluid_count(), 2);
        assert!(section.is_randomly_ticking());
        assert!(section.is_randomly_ticking_fluids());
        assert_eq!(section.ticking_block_count(), 2);
        assert!(section.is_randomly_ticking_blocks());
    }
}
