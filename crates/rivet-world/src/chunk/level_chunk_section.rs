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
//! predicate is parameterized like `is_air`: every constructor takes the
//! caller's real `BlockBehaviour` predicates (there is no placeholder default
//! that silently zeroes the ticking/special-colliding fields). The superflat
//! callers wire the generated behavior table where it covers a flag and
//! exact-for-content stand-ins where it does not.
//!
//! Deferred with the owning unit: the Anti-Xray `chunkPacketInfo`/
//! `chunkSectionIndex` write params (with `paper.antixray`), the fluid
//! set/accessors (with the fluid-state unit), and `setBlockState`'s client-side
//! `isClient` shortcut (the section has no client flag; the worldgen `doFill`
//! write path uses `checkThreading = false`, so the non-client branch is the
//! only one this port's [`set_block_state`](Self::set_block_state) reaches).

use crate::biome::biome_resolver::BiomeResolver;
use crate::biome::climate::Sampler;
use crate::chunk::moonrise_short_list::ShortList;
use crate::chunk::paletted_container::PalettedContainer;
use crate::chunk::strategy::Strategy;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::holder::Holder;
use std::sync::LazyLock;

/// `LevelChunkSection.BIOME_CONTAINER_BITS`.
pub const BIOME_CONTAINER_BITS: i32 = 2;

/// `LevelChunkSection.CLIENT_FORCED_SPECIAL_COLLIDING_BLOCKS` — the sentinel a
/// client-side (read-in) section's `specialCollidingBlocks` is forced to when
/// the section has any non-air block that is special-colliding.
const CLIENT_FORCED_SPECIAL_COLLIDING_BLOCKS: i16 = 9999;

/// `16*16*16` — the section cell count (the length of Java's `FULL_LIST`).
const SECTION_SIZE: usize = 16 * 16 * 16;

/// `LevelChunkSection.FULL_LIST` — the ascending `0..4096` index list Java
/// builds once as a static and reuses for the single-value palette's
/// coordinate list (issue #216). The port mirrors that static reuse: a
/// process-wide shared list the single-palette recalc reads without
/// allocating a fresh 4096-short list per call.
static FULL_LIST: LazyLock<Vec<i16>> = LazyLock::new(|| (0..SECTION_SIZE as i16).collect());

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
    /// PalettedContainer<Holder<Biome>> biomes)` — runs `recalcBlockCounts()`
    /// with the caller's real `BlockBehaviour` predicates.
    ///
    /// All five predicates are required — there is no placeholder default that
    /// silently zeroes the ticking/special-colliding fields. The caller passes
    /// its `state.isAir()` / `state.isRandomlyTicking()` /
    /// `state.getFluidState().isEmpty()` /
    /// `state.getFluidState().isRandomlyTicking()` /
    /// `CollisionUtil.isSpecialCollidingBlock(state)` equivalents; the
    /// superflat callers wire the generated behavior table where it covers a
    /// flag (see [`recalc_block_counts`] for the two flags it does not yet).
    pub fn new(
        states: PalettedContainer<T>,
        biomes: PalettedContainer<B>,
        is_air: impl Fn(&T) -> bool,
        is_randomly_ticking: impl Fn(&T) -> bool,
        fluid_is_empty: impl Fn(&T) -> bool,
        fluid_is_randomly_ticking: impl Fn(&T) -> bool,
        is_special_colliding: impl Fn(&T) -> bool,
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
        section.recalc_block_counts(
            &is_air,
            &is_randomly_ticking,
            &fluid_is_empty,
            &fluid_is_randomly_ticking,
            &is_special_colliding,
        );
        section
    }

    /// The default all-air section Java's `LevelChunkSection(containerFactory,
    /// ...)` path builds (issue #216): a section whose container is the
    /// factory's air default. `recalcBlockCounts()` on all-air content yields
    /// exactly this zero state (no non-empty blocks, no fluids, no ticking, no
    /// special-colliding), so the block predicates are unnecessary here — this
    /// constructor is the honest equivalent and is only valid for guaranteed
    /// all-air content (the chunk accessors' `replaceMissingSections` defaults).
    pub(crate) fn new_all_air(states: PalettedContainer<T>, biomes: PalettedContainer<B>) -> Self {
        LevelChunkSection {
            non_empty_block_count: 0,
            fluid_count: 0,
            ticking_block_count: 0,
            ticking_fluid_count: 0,
            states,
            biomes,
            special_colliding_blocks: 0,
            ticking_blocks: ShortList::new(),
        }
    }

    /// Value-transform the states and biomes containers into `T2`/`B2` with the
    /// target strategies, mapping each palette entry through the caller's
    /// `map_block`/`map_biome` closures — the wire-identical re-encode the #516
    /// server bridge needs (`BlockState::id()` IS the server `StateId`, and
    /// both biome newtypes are dense registry ids). All count fields, the
    /// ticking list, and the special-colliding tally are preserved as-is: the
    /// mapped values occupy the same dense id space, so the counts remain exact
    /// without a `recalcBlockCounts` pass.
    pub fn map_values<T2, B2>(
        self,
        block_strategy: &Strategy<T2>,
        biome_strategy: &Strategy<B2>,
        map_block: &impl Fn(&T) -> T2,
        map_biome: &impl Fn(&B) -> B2,
    ) -> Result<LevelChunkSection<T2, B2>, String>
    where
        T2: Clone + PartialEq + Send + std::fmt::Debug + 'static,
        B2: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    {
        let LevelChunkSection {
            non_empty_block_count,
            fluid_count,
            ticking_block_count,
            ticking_fluid_count,
            states,
            biomes,
            special_colliding_blocks,
            ticking_blocks,
        } = self;
        let states: PalettedContainer<T2> = states.map_values(block_strategy, map_block)?;
        let biomes: PalettedContainer<B2> = biomes.map_values(biome_strategy, map_biome)?;
        Ok(LevelChunkSection {
            non_empty_block_count,
            fluid_count,
            ticking_block_count,
            ticking_fluid_count,
            states,
            biomes,
            special_colliding_blocks,
            ticking_blocks,
        })
    }

    /// `getBlockState(int, int, int)`.
    pub fn get_block_state(&self, x: i32, y: i32, z: i32) -> T {
        self.states.get(x, y, z)
    }

    /// `acquire()` — the section's `PalettedContainer.acquire()`.
    ///
    /// Paper disables the `ThreadingDetector` (`// Paper - disable this - use
    /// proper synchronization`), so Java's acquire/release are NO-OPs; the
    /// port mirrors them as documented no-ops. The `NoiseBasedChunkGenerator`
    /// `fillFromNoise` lifecycle acquires every section it will write and
    /// releases them in `finally`, so the no-ops keep the call structure
    /// faithful (the section set writes are already `&mut self`-exclusive here).
    pub fn acquire(&self) {}

    /// `release()` — the section's `PalettedContainer.release()` (a NO-OP; see
    /// [`acquire`](Self::acquire)).
    pub fn release(&self) {}

    /// `setBlockState(int sectionX, int sectionY, int sectionZ, BlockState
    /// state, boolean checkThreading)` — the `checkThreading = false` path
    /// (`getAndSetUnchecked`).
    ///
    /// The count bookkeeping is ported exactly: Java decrements the previous
    /// state's counts (non-empty, randomly-ticking block, fluid, randomly-
    /// ticking fluid) and increments the new state's, then runs Paper's
    /// `updateBlockCallback` (special-colliding tally + the `tickingBlocks`
    /// list). The `isClient` branch of `updateBlockCallback` is unreachable
    /// here (no client flag — see the module doc); the special-colliding and
    /// randomly-ticking branches use the same caller-supplied predicates
    /// [`new`](Self::new)/`recalc_block_counts` take, so the write stays
    /// faithful to the caller's real `BlockBehaviour` flags. `checkThreading`
    /// itself is omitted (Paper's `getAndSet`/`getAndSetUnchecked` differ only
    /// by the acquire/release no-ops, which [`get_and_set`] already omits).
    ///
    /// Returns the previous state, matching Java.
    #[allow(clippy::too_many_arguments)] // Java's `setBlockState` has 7 params + the `BlockBehaviour` predicate set.
    pub fn set_block_state(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        state: T,
        is_air: &dyn Fn(&T) -> bool,
        is_randomly_ticking: &dyn Fn(&T) -> bool,
        fluid_is_empty: &dyn Fn(&T) -> bool,
        fluid_is_randomly_ticking: &dyn Fn(&T) -> bool,
        is_special_colliding: &dyn Fn(&T) -> bool,
    ) -> T {
        // `getAndSet` moves the new state into the container; the increment
        // branch below still reads the caller's `state` (`T: Clone`), so clone
        // here rather than re-reading the container.
        let previous = self.states.get_and_set(x, y, z, state.clone());
        // All counts are Java `short` fields; Java's compound assignment narrows
        // the `int` result back to `short` (wrapping on overflow), so the port
        // uses wrapping arithmetic (PORTING.md) rather than debug-build panics.
        if !is_air(&previous) {
            self.non_empty_block_count = self.non_empty_block_count.wrapping_sub(1);
            if is_randomly_ticking(&previous) {
                self.ticking_block_count = self.ticking_block_count.wrapping_sub(1);
            }
            if !fluid_is_empty(&previous) {
                self.fluid_count = self.fluid_count.wrapping_sub(1);
                if fluid_is_randomly_ticking(&previous) {
                    self.ticking_fluid_count = self.ticking_fluid_count.wrapping_sub(1);
                }
            }
        }
        if !is_air(&state) {
            self.non_empty_block_count = self.non_empty_block_count.wrapping_add(1);
            if is_randomly_ticking(&state) {
                self.ticking_block_count = self.ticking_block_count.wrapping_add(1);
            }
            if !fluid_is_empty(&state) {
                self.fluid_count = self.fluid_count.wrapping_add(1);
                if fluid_is_randomly_ticking(&state) {
                    self.ticking_fluid_count = self.ticking_fluid_count.wrapping_add(1);
                }
            }
        }
        // `updateBlockCallback(x, y, z, state, previous)`.
        if previous != state {
            let is_special_old = is_special_colliding(&previous);
            let is_special_new = is_special_colliding(&state);
            if is_special_old != is_special_new {
                if is_special_old {
                    self.special_colliding_blocks = self.special_colliding_blocks.wrapping_sub(1);
                } else {
                    self.special_colliding_blocks = self.special_colliding_blocks.wrapping_add(1);
                }
            }
            let old_ticking = is_randomly_ticking(&previous);
            let new_ticking = is_randomly_ticking(&state);
            if old_ticking != new_ticking {
                let position = (x | (z << 4) | (y << 8)) as i16;
                if old_ticking {
                    self.ticking_blocks.remove(position);
                } else {
                    self.ticking_blocks.add(position);
                }
            }
        }
        previous
    }

    /// `getNoiseBiome(int, int, int)` — the 4×4×4 biome container read
    /// (section-local quart coords). Java passes the already-masked quart
    /// coords through to `PalettedContainer.get` unmasked; the callers (the
    /// chunk `getNoiseBiome`) mask to `& 3` first.
    pub fn get_noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> B {
        self.biomes.get(quart_x, quart_y, quart_z)
    }

    /// `setNoiseBiome(int, int, int, Holder<Biome>)` (CraftBukkit) — the
    /// single-cell biome write (section-local quart coords).
    pub fn set_noise_biome(&mut self, quart_x: i32, quart_y: i32, quart_z: i32, biome: B) {
        self.biomes.set(quart_x, quart_y, quart_z, biome);
    }

    /// `fillBiomesFromNoise(BiomeResolver, Climate.Sampler, int quartMinX,
    /// int quartMinY, int quartMinZ)` — recreates the biome container and fills
    /// the 4×4×4 cells from the resolver at `quartMin + {0..3}` per axis.
    ///
    /// Java stores the resolved `Holder<Biome>` directly; the port's section is
    /// generic over the stored element `B` (the worldgen chunk's dense `BiomeId`
    /// or a test's u8), so the caller's `map_biome` converts the resolved
    /// `Holder<BiomeId>` handle into `B` (the `holder_biome_id` seam). The
    /// recreate-then-fill is Java verbatim: a fresh single-value container
    /// holding the old palette's first entry, each cell written with
    /// `getAndSetUnchecked` (the previous value is discarded), then installed.
    /// The `x → y → z` loop order matches Java, so the palette insertion order
    /// is identical.
    pub fn fill_biomes_from_noise(
        &mut self,
        biome_resolver: &dyn BiomeResolver,
        sampler: &Sampler,
        quart_min_x: i32,
        quart_min_y: i32,
        quart_min_z: i32,
        map_biome: &impl Fn(&Holder<BiomeId>) -> B,
    ) {
        let mut new_biomes = self.biomes.recreate();
        let size = 4;
        for x in 0..size {
            for y in 0..size {
                for z in 0..size {
                    let biome = map_biome(&biome_resolver.get_noise_biome(
                        quart_min_x + x,
                        quart_min_y + y,
                        quart_min_z + z,
                        sampler,
                    ));
                    new_biomes.get_and_set(x, y, z, biome);
                }
            }
        }
        self.biomes = new_biomes;
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
    /// Java's Moonrise fast path (issue #216) is ported: when the palette
    /// holds a single value, the static `FULL_LIST` (the ascending `0..4096`
    /// index list, allocated once like Java's) is used instead of
    /// `moonrise$countEntries()`; otherwise the per-palette-id coordinate
    /// groups are read from the storage directly. Each group contributes the
    /// palette value's counts; the packed positions of randomly-ticking values
    /// are appended to `tickingBlocks` (Java's `setMinCapacity` + `add` loop —
    /// the `ShortList` dedupes, so the coordinate list ends up with exactly
    /// the distinct ticking positions).
    ///
    /// All five predicates are the caller's real `BlockBehaviour` equivalents —
    /// there is no placeholder default. Paper uses `state.isRandomlyTicking()`
    /// for the block count and `fluid.isRandomlyTicking()` for the fluid
    /// count, so `is_randomly_ticking` and `fluid_is_randomly_ticking` are
    /// threaded separately. The superflat callers wire the generated behavior
    /// table for `is_randomly_ticking`/`fluid_is_empty`, and the two flags the
    /// table does not yet carry are tracked below.
    ///
    /// The generated `block_behaviors` table has no fluid-random-tick or
    /// special-colliding flags; the superflat callers
    /// pass exact-for-content stand-ins (the air + stone content has no fluid
    /// and no special-colliding block) and the real `CollisionUtil`
    /// `isSpecialCollidingBlock` / `fluid.isRandomlyTicking()` equivalents
    /// must be wired when the owning block slice adds them.
    ///
    /// `tickingBlocks`'s element order follows `count_entries`' first-appearance
    /// order (Java's hash-bucket order is not portable); the list never reaches
    /// the wire and no ported consumer reads its order yet.
    ///
    /// `count_entries`' first-appearance outer order differs from Java's
    /// hash-bucket `tickingBlocks` order. No current consumer observes the
    /// list order; a future random-tick scheduler must define that boundary
    /// before consuming it.
    pub fn recalc_block_counts(
        &mut self,
        is_air: &dyn Fn(&T) -> bool,
        is_randomly_ticking: &dyn Fn(&T) -> bool,
        fluid_is_empty: &dyn Fn(&T) -> bool,
        fluid_is_randomly_ticking: &dyn Fn(&T) -> bool,
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
            // Java's `paletteSize == 1` fast path reuses the static `FULL_LIST`
            // (allocated once) instead of `moonrise$countEntries()`; the port
            // mirrors that static reuse so the single-value recalc does not
            // allocate a fresh 4096-short list each call. The multi-value path
            // materializes the per-id lists exactly like Java's map.
            let entries: Vec<(i32, Vec<i16>)>;
            let counts: Vec<(i32, &[i16])> = if palette_size == 1 {
                vec![(0, FULL_LIST.as_slice())]
            } else {
                entries = self.states.count_entries();
                entries
                    .iter()
                    .map(|(id, coords)| (*id, coords.as_slice()))
                    .collect()
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
                    for &packed in coordinates {
                        self.ticking_blocks.add(packed);
                    }
                }
                if !fluid_is_empty(&state) {
                    fluid += palette_count;
                    // Paper uses the fluid state's own random-tick predicate
                    // (`fluid.isRandomlyTicking()`), distinct from the block
                    // state's — threaded separately.
                    if fluid_is_randomly_ticking(&state) {
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
    /// Java leaves `tickingBlocks` untouched on read — a read-in client
    /// section's list is repopulated only by a later `recalcBlockCounts` or a
    /// block mutation. The port's read-in sections are freshly constructed, so
    /// the list stays empty, matching Java's read-in state.
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
    use crate::biome::Climate;
    use crate::chunk::palette::GlobalIdMap;
    use crate::chunk::paletted_container::PalettedContainer;
    use crate::chunk::strategy::Strategy;
    use bytes::BytesMut;
    use std::cell::RefCell;

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

    /// A fresh all-air section — the `new_all_air` default (equivalent to
    /// `recalcBlockCounts` on all-air content: zero counts, empty ticking).
    fn all_air_section() -> LevelChunkSection<u8, u8> {
        LevelChunkSection::new_all_air(
            PalettedContainer::new(0u8, block_strategy()),
            PalettedContainer::new(0u8, biome_strategy()),
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
            |_| true,  // block randomly ticking
            |_| true,  // fluid empty
            |_| true,  // fluid randomly ticking (unused: fluid empty)
            |_| false, // special colliding
        );
        assert_eq!(section.non_empty_block_count(), 4096);
        assert_eq!(section.fluid_count(), 0);
        assert!(!section.has_fluid());

        // Every packed position 0..4096 is distinct, so all are appended.
        section.recalc_block_counts(&is_air, &|_| true, &|_| true, &|_| true, &|_| false);
        assert_eq!(section.ticking_block_count(), 4096);
        assert_eq!(section.ticking_blocks().size(), 4096);
        for index in 0..4096 {
            assert_eq!(section.ticking_blocks().get_raw(index), index as i16);
        }
        assert!(section.is_randomly_ticking());
        assert!(section.is_randomly_ticking_blocks());
        assert!(!section.is_randomly_ticking_fluids());
        // An all-special-colliding single-value section counts every cell.
        section.recalc_block_counts(&is_air, &|_| false, &|_| true, &|_| true, &|_| true);
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
            |s| *s == 2 || *s == 4, // block randomly ticking
            |_| true,               // fluid empty
            |_| true,               // fluid randomly ticking (unused: fluid empty)
            |s| *s == 3,            // special colliding
        );

        // Values 2 and 4 randomly tick; value 3 is special-colliding.
        section.recalc_block_counts(
            &is_air,
            &|s| *s == 2 || *s == 4,
            &|_| true,
            &|_| true,
            &|s| *s == 3,
        );
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
            |_| false,   // block randomly ticking
            |_| true,    // fluid empty
            |_| true,    // fluid randomly ticking (unused: fluid empty)
            |s| *s == 3, // special colliding
        );
        section.recalc_block_counts(&is_air, &|_| false, &|_| true, &|_| true, &|s| *s == 3);
        assert_eq!(section.non_empty_block_count(), 3);
        assert!(section.has_special_colliding_blocks());

        // A predicate matching nothing leaves the count at zero.
        section.recalc_block_counts(&is_air, &|_| false, &|_| true, &|_| true, &|_| false);
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
            |_| false, // block randomly ticking
            |_| true,  // fluid empty
            |_| true,  // fluid randomly ticking (unused: fluid empty)
            |_| false, // special colliding
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
            |_| false, // block randomly ticking
            |_| true,  // fluid empty
            |_| true,  // fluid randomly ticking (unused: fluid empty)
            |_| false, // special colliding
        );
        section.recalc_block_counts(&is_air, &|_| true, &|_| true, &|_| true, &|_| false);
        assert_eq!(section.ticking_block_count(), 2);
        assert_eq!(section.ticking_blocks().size(), 2);

        // Now nothing ticks: the earlier list must be dropped, not appended to.
        section.recalc_block_counts(&is_air, &|_| false, &|_| true, &|_| true, &|_| false);
        assert_eq!(section.ticking_block_count(), 0);
        assert_eq!(section.ticking_blocks().size(), 0);
        assert_eq!(section.non_empty_block_count(), 2);
        assert!(!section.is_randomly_ticking());

        // The fluid branch: both states are non-empty fluid, and the fluid
        // random-tick predicate (distinct from the block predicate) counts
        // them as ticking fluids. The `is_randomly_ticking_fluids` getter
        // reflects the fluid list, distinct from the block list.
        section.recalc_block_counts(&is_air, &|_| true, &|s| *s != 1, &|_| true, &|_| false);
        assert_eq!(section.fluid_count(), 2);
        assert_eq!(section.ticking_fluid_count(), 2);
        assert!(section.is_randomly_ticking());
        assert!(section.is_randomly_ticking_fluids());
        assert_eq!(section.ticking_block_count(), 2);
        assert!(section.is_randomly_ticking_blocks());
    }

    /// The fluid random-tick predicate is independent of the block one (Paper:
    /// `state.isRandomlyTicking()` vs `fluid.isRandomlyTicking()`): a state can
    /// tick as a block without ticking as a fluid, and vice versa.
    #[test]
    fn fluid_ticking_uses_its_own_predicate() {
        let mut states = PalettedContainer::new(0u8, block_strategy());
        states.set(0, 0, 0, 1); // block-ticking only
        states.set(1, 0, 0, 2); // fluid-ticking only
        let mut section = LevelChunkSection::new(
            states,
            PalettedContainer::new(0u8, biome_strategy()),
            is_air,
            |_| false, // block randomly ticking (overridden below)
            |_| true,  // fluid empty (overridden below)
            |_| true,  // fluid randomly ticking (overridden below)
            |_| false, // special colliding
        );
        // Block ticks only value 1; fluid branch is entered only for value 2,
        // and among those only value 2 ticks as a fluid.
        section.recalc_block_counts(
            &is_air,
            &|s| *s == 1, // block randomly ticking
            &|s| *s != 2, // fluid empty (false for value 2 → fluid present)
            &|s| *s == 2, // fluid randomly ticking
            &|_| false,   // special colliding
        );
        assert_eq!(section.ticking_block_count(), 1);
        assert_eq!(section.fluid_count(), 1);
        assert_eq!(section.ticking_fluid_count(), 1);
        assert!(section.is_randomly_ticking_blocks());
        assert!(section.is_randomly_ticking_fluids());
        assert_eq!(section.ticking_blocks().size(), 1);
    }

    /// `setBlockState`'s count bookkeeping — the Paper port:
    /// `getAndSetUnchecked` then decrement/increment the previous/new counts.
    #[test]
    fn set_block_state_adjusts_counts_and_returns_previous() {
        let mut section = all_air_section();
        // Value 1: non-air (`*s != 0`), block-ticking (`*s == 1`), non-empty
        // fluid (`!fluid_is_empty`, i.e. `*s != 0`), ticking fluid (`*s == 1`).
        // Value 0 (air): none of those — the same predicates Java's real
        // `BlockBehaviour` flags give air.
        let block_ticks = |s: &u8| *s == 1;
        let fluid_is_empty = |s: &u8| *s == 0; // value 0 (air) has no fluid
        let fluid_ticks = |s: &u8| *s == 1;
        let special = |s: &u8| *s == 2;
        let previous = section.set_block_state(
            0,
            0,
            0,
            1,
            &is_air,
            &block_ticks,
            &fluid_is_empty,
            &fluid_ticks,
            &special,
        );
        assert_eq!(previous, 0);
        assert_eq!(section.get_block_state(0, 0, 0), 1);
        assert_eq!(section.non_empty_block_count(), 1);
        assert_eq!(section.ticking_block_count(), 1);
        assert_eq!(section.fluid_count(), 1);
        assert_eq!(section.ticking_fluid_count(), 1);
        assert!(section.is_randomly_ticking());
        // The ticking position is `x | z<<4 | y<<8` (0 here).
        assert_eq!(section.ticking_blocks().size(), 1);
        assert_eq!(section.ticking_blocks().get_raw(0), 0);

        // Overwrite with air: previous counts are decremented; the ticking
        // position is removed; previous state is returned.
        let previous = section.set_block_state(
            0,
            0,
            0,
            0,
            &is_air,
            &block_ticks,
            &fluid_is_empty,
            &fluid_ticks,
            &special,
        );
        assert_eq!(previous, 1);
        assert_eq!(section.get_block_state(0, 0, 0), 0);
        assert_eq!(section.non_empty_block_count(), 0);
        assert_eq!(section.ticking_block_count(), 0);
        assert_eq!(section.fluid_count(), 0);
        assert_eq!(section.ticking_fluid_count(), 0);
        assert_eq!(section.ticking_blocks().size(), 0);
    }

    /// The packed ticking position matches Java's `(short)(x | z << 4 |
    /// y << 8)` for a non-zero cell.
    #[test]
    fn set_block_state_packs_ticking_position_like_java() {
        let mut section = all_air_section();
        // Cell (x=3, y=5, z=7) → `3 | 7<<4 | 5<<8 = 3 | 112 | 1280 = 1395`.
        section.set_block_state(
            3,
            5,
            7,
            1,
            &is_air,
            &|s| *s == 1, // block randomly ticking
            &|_| true,    // fluid empty — no fluid counts
            &|_| true,
            &|_| false,
        );
        assert_eq!(section.ticking_blocks().size(), 1);
        assert_eq!(
            section.ticking_blocks().get_raw(0),
            (3 | (7 << 4) | (5 << 8)) as i16
        );
        assert_eq!(section.fluid_count(), 0);
        assert_eq!(section.non_empty_block_count(), 1);
    }

    /// A non-ticking block never enters the ticking list even though the cell
    /// is written; an overwrite that drops the special-colliding flag adjusts
    /// the special-colliding tally (Paper's `updateBlockCallback`).
    #[test]
    fn set_block_state_tracks_special_colliding_swap() {
        let mut section = all_air_section();
        // Value 2: non-air, non-ticking, special-colliding.
        section.set_block_state(
            0,
            0,
            0,
            2,
            &is_air,
            &|_| false, // not randomly ticking
            &|_| true,  // fluid empty
            &|_| true,
            &|s| *s == 2, // special colliding
        );
        assert_eq!(section.non_empty_block_count(), 1);
        assert!(section.has_special_colliding_blocks());
        assert_eq!(section.ticking_blocks().size(), 0);

        // Swap to value 1 (non-ticking, non-special): the special-colliding
        // tally drops and the ticking list stays empty.
        section.set_block_state(
            0,
            0,
            0,
            1,
            &is_air,
            &|_| false,
            &|_| true,
            &|_| true,
            &|s| *s == 2,
        );
        assert_eq!(section.non_empty_block_count(), 1);
        assert!(!section.has_special_colliding_blocks());
        assert_eq!(section.ticking_blocks().size(), 0);
    }

    /// `acquire`/`release` are documented no-ops (Paper disables the
    /// `ThreadingDetector`); they must be callable and leave the section
    /// unchanged.
    #[test]
    fn acquire_release_are_noops() {
        let section = all_air_section();
        let counts = (
            section.non_empty_block_count(),
            section.fluid_count(),
            section.ticking_block_count(),
            section.ticking_fluid_count(),
        );
        section.acquire();
        section.release();
        section.acquire();
        assert_eq!(
            (
                section.non_empty_block_count(),
                section.fluid_count(),
                section.ticking_block_count(),
                section.ticking_fluid_count(),
            ),
            counts
        );
    }

    /// A `BiomeResolver` that records every quart request in order and returns
    /// a deterministic holder derived from the coordinates.
    struct RecordingResolver(RefCell<Vec<(i32, i32, i32)>>);

    impl BiomeResolver for RecordingResolver {
        fn get_noise_biome(
            &self,
            quart_x: i32,
            quart_y: i32,
            quart_z: i32,
            _sampler: &Sampler,
        ) -> Holder<BiomeId> {
            self.0.borrow_mut().push((quart_x, quart_y, quart_z));
            let id = (quart_x
                .wrapping_mul(31)
                .wrapping_add(quart_y.wrapping_mul(7))
                .wrapping_add(quart_z.wrapping_mul(13)))
                & 0xff;
            Holder::direct(BiomeId::from_id(id as u16))
        }
    }

    /// `dense_biome_id` for the u8 test element: a `Direct` holder reads its id,
    /// a `Reference` holder its registry id.
    fn map_biome(holder: &Holder<BiomeId>) -> u8 {
        match holder {
            Holder::Direct(biome) => biome.id() as u8,
            Holder::Reference { id, .. } => *id as u8,
        }
    }

    /// `fillBiomesFromNoise` resolves the 4×4×4 cells at
    /// `quartMin + {0..3}` per axis in Java's `x → y → z` order, installs a
    /// freshly recreated container, and each cell holds the mapped resolution.
    #[test]
    fn fill_biomes_from_noise_resolves_cells_at_quart_min_plus_offset() {
        let resolver = RecordingResolver(RefCell::new(Vec::new()));
        let mut section = all_air_section();
        let sampler = Climate::empty();
        section.fill_biomes_from_noise(&resolver, &sampler, -4, 8, 0, &map_biome);

        let calls = resolver.0.into_inner();
        assert_eq!(calls.len(), 64);
        // Java iterates x, then y, then z — the resolver is driven in that
        // exact order with `quartMin + {0..3}` per axis.
        let expected: Vec<(i32, i32, i32)> = (0..4)
            .flat_map(|x| (0..4).flat_map(move |y| (0..4).map(move |z| (-4 + x, 8 + y, z))))
            .collect();
        assert_eq!(calls, expected);
        // Each written cell holds the mapped resolution of the absolute quart —
        // the read-back positions line up with the x→y→z write order.
        for (index, (x, y, z)) in calls.iter().enumerate() {
            // index = x*16 + y*4 + z (the x→y→z write order), so the section-local
            // cell is (x, y, z) with x = index>>4, y = (index>>2)&3, z = index&3.
            let (local_x, local_y, local_z) = ((index >> 4) & 3, (index >> 2) & 3, index & 3);
            let id = (x
                .wrapping_mul(31)
                .wrapping_add(y.wrapping_mul(7))
                .wrapping_add(z.wrapping_mul(13)))
                & 0xff;
            assert_eq!(
                section.get_noise_biome(local_x as i32, local_y as i32, local_z as i32),
                id as u8
            );
        }
    }

    /// `setNoiseBiome` is the single-cell biome write (CraftBukkit
    /// `setNoiseBiome`): it writes the section-local quart cell and reads back.
    #[test]
    fn set_noise_biome_writes_the_single_cell() {
        let mut section = all_air_section();
        section.set_noise_biome(1, 2, 3, 7u8);
        assert_eq!(section.get_noise_biome(1, 2, 3), 7);
        // Neighboring cells are untouched.
        assert_eq!(section.get_noise_biome(0, 2, 3), 0);
        assert_eq!(section.get_noise_biome(1, 1, 3), 0);
    }
}
