//! Port of `net.minecraft.world.level.chunk.PalettedContainer<T>` (MC 26.2).
//!
//! The container is a `BitStorage` of palette-local indices plus a `Palette`
//! mapping those indices to global ids. This module ports the pure value/wire
//! surface needed by the superflat chunk wire format (#108): construction
//! (`new`, `recreate`, `copy`), mutation (`set`, `get_and_set`), reads
//! (`get`, `maybe_has`, `count`, `get_all`, `forEachInPalette`), the wire
//! `read`/`write`/`get_serialized_size`, and the NBT `pack`/`unpack` paths
//! (which share the exact bits-per-entry transition logic).
//!
//! Deferred (not part of M1): `acquire`/`release` threading guards (the
//! container is tick-thread-confined game state — OWNERSHIP.md — and Java's
//! `synchronized` is dropped with a note, as PORTING.md prescribes for
//! tick-confined state) and the Anti-Xray `write`/`ChunkPacketInfo` params.
//!
//! The Anti-Xray `presetValues` constructor surface and the Moonrise
//! `FastPalette` read-path snapshot are ported (issue #216).
//!
//! The `onResize` reentrancy is ported by deferring the resize: `Palette::id_for`
//! returns `IdForResult` and the container grows, then inserts the value. The
//! Java `PaletteResize.noResizeExpected()` internal path maps to
//! `IdForResult::expect_no_resize`.

use std::collections::{HashMap, HashSet};

use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_util::bit_storage::BitStorage;
use rivet_util::simple_bit_storage::{InitializationException, SimpleBitStorage};
use rivet_util::zero_bit_storage::ZeroBitStorage;

use crate::chunk::configuration::{Configuration, PaletteFactoryKind};
use crate::chunk::palette::{GlobalIdMap, HashMapPalette, IdForResult, Palette, ceillog2};
use crate::chunk::strategy::Strategy;

/// `PalettedContainerRO.PackedData<T>` — the NBT codec's packed form.
///
/// Java's `Optional<LongStream>` maps to `Option<Vec<i64>>`. `bits_per_entry`
/// is the on-disc width (`UNKNOWN_BITS_PER_ENTRY` = -1 when not declared).
pub struct PackedData<T> {
    pub palette_entries: Vec<T>,
    pub storage: Option<Vec<i64>>,
    pub bits_per_entry: i32,
}

impl<T> PackedData<T> {
    /// `PackedData.UNKNOWN_BITS_PER_ENTRY`.
    pub const UNKNOWN_BITS_PER_ENTRY: i32 = -1;

    /// `PackedData(List<T>, Optional<LongStream>)` — declares no bit count.
    pub fn new(palette_entries: Vec<T>, storage: Option<Vec<i64>>) -> Self {
        PackedData {
            palette_entries,
            storage,
            bits_per_entry: Self::UNKNOWN_BITS_PER_ENTRY,
        }
    }

    /// `PackedData(List<T>, Optional<LongStream>, int bitsPerEntry)`.
    pub fn with_bits(
        palette_entries: Vec<T>,
        storage: Option<Vec<i64>>,
        bits_per_entry: i32,
    ) -> Self {
        PackedData {
            palette_entries,
            storage,
            bits_per_entry,
        }
    }
}

/// `PalettedContainer<T>`.
pub struct PalettedContainer<T: Clone + PartialEq + Send + 'static> {
    strategy: Strategy<T>,
    data: Data<T>,
    /// Paper Anti-Xray `presetValues` — a fixed set of values kept in the
    /// palette (re-inserted on every resize/read) so the wire palette always
    /// contains them. `None` when no preset values are configured.
    preset_values: Option<Vec<T>>,
}

impl<T: Clone + PartialEq + Send + std::fmt::Debug + 'static> PalettedContainer<T> {
    /// `PalettedContainer(T initialValue, Strategy<T>)`.
    ///
    /// Starts at the zero-bit configuration (single-value palette,
    /// `ZeroBitStorage`), then inserts `initial_value`.
    pub fn new(initial_value: T, strategy: Strategy<T>) -> Self {
        Self::new_with_preset_values(initial_value, strategy, None)
    }

    /// `PalettedContainer(T initialValue, Strategy<T>, T[] presetValues)`.
    ///
    /// As [`new`](Self::new), but records the Anti-Xray `preset_values`. Like
    /// Java, the preset values are not inserted here — they take effect on the
    /// next resize (`on_resize` widens for them) or `read`.
    pub fn new_with_preset_values(
        initial_value: T,
        strategy: Strategy<T>,
        preset_values: Option<Vec<T>>,
    ) -> Self {
        let mut data = Self::create_or_reuse_data(&strategy, None, 0);
        let _ = data.palette.id_for(&initial_value);
        let mut container = PalettedContainer {
            strategy,
            data,
            preset_values,
        };
        container.update_data();
        container
    }

    /// Java's private constructor
    /// `PalettedContainer(Strategy, Configuration, BitStorage, Palette, List<T>
    /// values, T defaultValue, T[] presetValues)` — used by `unpack`. Adopts
    /// `storage`/`palette` (built from the wire), then runs the Anti-Xray
    /// preset insertion block (Paper re-adds the resize handling Mojang
    /// removed for reads in 1.18).
    pub(crate) fn from_data(
        strategy: Strategy<T>,
        configuration: Configuration,
        storage: Box<dyn BitStorage>,
        palette: Box<dyn Palette<T>>,
        values: Vec<T>,
        default_value: Option<T>,
        preset_values: Option<Vec<T>>,
    ) -> Self {
        let mut container = PalettedContainer {
            strategy,
            data: Data {
                configuration,
                storage,
                palette,
                snapshot: None,
            },
            preset_values,
        };

        // Paper Anti-Xray: if preset values are configured and the stored
        // configuration can hold them, insert them so the wire palette always
        // contains them, growing the palette when it fills up.
        if let Some(presets) = container.preset_values.clone() {
            let should_insert = match &container.data.configuration {
                // A single-value palette only gets the presets when its single
                // entry differs from the codec default (Java `defaultValue`).
                Configuration::Simple {
                    factory: PaletteFactoryKind::SingleValue,
                    ..
                } => {
                    container.data.palette.value_for(0)
                        != default_value.expect("preset values require a codec default value")
                }
                Configuration::Global { .. } => false,
                Configuration::Simple { .. } => true,
            };
            if should_insert {
                let max_size = 1i32 << container.data.configuration.bits_in_memory();
                for preset in &presets {
                    if container.data.palette.get_size() >= max_size {
                        // The palette is full: widen once to fit every distinct
                        // value (wire palette entries + all presets), then stop.
                        let mut all_values: Vec<T> = values.clone();
                        for p in &presets {
                            if !all_values.contains(p) {
                                all_values.push(p.clone());
                            }
                        }
                        let new_bits = ceillog2(all_values.len() as i32);
                        if new_bits > container.data.configuration.bits_in_memory() {
                            container.on_resize(new_bits, None);
                        }
                        break;
                    }
                    container.insert_index(preset.clone());
                }
            }
        }
        // The snapshot must reflect the preset inserts (Java's is live).
        container.update_data();

        container
    }

    /// `createOrReuseData(Data oldData, int targetBits)` — Java returns
    /// `oldData` itself when the configuration matches; the port returns a
    /// copy (same contents, and callers overwrite the storage from the wire).
    fn create_or_reuse_data(
        strategy: &Strategy<T>,
        old_data: Option<&Data<T>>,
        target_bits: i32,
    ) -> Data<T> {
        let data_configuration = strategy.configuration_for_bit_count(target_bits);
        if let Some(old) = old_data
            && old.configuration == data_configuration
        {
            return old.copy();
        }
        Data::new(data_configuration, strategy)
    }

    /// `onResize(int bits, T lastAddedValue)` — the container's resize handler
    /// (Java `PaletteResize`). Rebuilds `data` at `bits`, copies the old
    /// contents across, then re-adds the preset values and inserts
    /// `last_added_value`, returning its index (`-1` when `last_added_value`
    /// is absent — Java's `null` from the unpack resize path).
    fn on_resize(&mut self, bits: i32, last_added_value: Option<T>) -> i32 {
        // Paper Anti-Xray: when growing from a single-value palette with
        // preset values configured, widen up front so every preset fits
        // without a cascade of further resizes.
        let mut bits = bits;
        if let Some(added) = last_added_value.as_ref()
            && let Some(presets) = self.preset_values.as_ref()
            && matches!(
                &self.data.configuration,
                Configuration::Simple {
                    factory: PaletteFactoryKind::SingleValue,
                    ..
                }
            )
        {
            let mut duplicates = 0;
            if presets.contains(added) {
                duplicates += 1;
            }
            if presets.contains(&self.data.palette.value_for(0)) {
                duplicates += 1;
            }
            let size = 1i32
                << self
                    .strategy
                    .configuration_for_bit_count(bits)
                    .bits_in_memory();
            bits = ceillog2(size + presets.len() as i32 - duplicates);
        }
        let configuration = self.strategy.configuration_for_bit_count(bits);
        if self.data.configuration == configuration {
            // Java's createOrReuseData reuses the same Data; copyFrom(old, old)
            // is a self-copy no-op. Only the final insert (or `-1` for the
            // unpack `onResize(newBits, null)` widening) is observable.
            return match last_added_value {
                None => -1,
                Some(value) => self.data.palette.id_for(&value).expect_no_resize(),
            };
        }
        let mut new_data = Data::new(configuration, &self.strategy);
        new_data.copy_from(&*self.data.palette, &*self.data.storage);
        self.data = new_data;
        self.add_preset_values();
        match last_added_value {
            None => -1,
            Some(value) => self.data.palette.id_for(&value).expect_no_resize(),
        }
        // Every caller re-materializes the snapshot after this helper returns:
        // `insert_index` unconditionally, `from_data` after the preset block.
    }

    /// `updateData(Data)` — recomputes the Moonrise `FastPaletteData`
    /// read-path snapshot from the current palette.
    fn update_data(&mut self) {
        self.data.snapshot = self.data.palette.raw_palette();
    }

    /// `addPresetValues()` — re-inserts the preset values into the current
    /// palette (Java's Anti-Xray hook on resize and read). Java's snapshot is
    /// a live reference, so it must be re-materialized after the inserts.
    fn add_preset_values(&mut self) {
        if self.preset_values.is_some()
            && !matches!(&self.data.configuration, Configuration::Global { .. })
        {
            let presets = self.preset_values.clone().unwrap();
            for preset in presets {
                let _ = self.insert_index(preset);
            }
        }
        self.update_data();
    }

    /// `readPalette(Data, int)` — the Moonrise read path: consult the
    /// `FastPaletteData` snapshot when present (Java `palette[paletteIdx]`,
    /// panicking when the entry is null), else the palette's `value_for`.
    fn read_palette(&self, data: &Data<T>, palette_idx: i32) -> T {
        if let Some(snapshot) = &data.snapshot {
            match snapshot.get(palette_idx as usize) {
                Some(value) => value.clone(),
                None => panic!("Palette index out of bounds"),
            }
        } else {
            data.palette.value_for(palette_idx)
        }
    }

    /// `getAndSet(int x, int y, int z, T)` — returns the previous value.
    pub fn get_and_set(&mut self, x: i32, y: i32, z: i32, value: T) -> T {
        let index = self.strategy.get_index(x, y, z);
        self.get_and_set_index(index, value)
    }

    /// `getAndSet(int index, T)`.
    pub fn get_and_set_index(&mut self, index: usize, value: T) -> T {
        let palette_idx = self.insert_index(value);
        // Java re-reads `this.data` after idFor because the resize may have
        // replaced it; the port reads the (possibly replaced) data here.
        let prev = self.data.storage.get_and_set(index, palette_idx);
        self.read_palette(&self.data, prev)
    }

    /// `set(int x, int y, int z, T)`.
    pub fn set(&mut self, x: i32, y: i32, z: i32, value: T) {
        let index = self.strategy.get_index(x, y, z);
        let id = self.insert_index(value);
        self.data.storage.set(index, id);
    }

    /// `set(int index, T)`.
    pub fn set_index(&mut self, index: usize, value: T) {
        let id = self.insert_index(value);
        self.data.storage.set(index, id);
    }

    /// The shared `idFor` + deferred resize path.
    fn insert_index(&mut self, value: T) -> i32 {
        let result = match self.data.palette.id_for(&value) {
            IdForResult::Id(id) => id,
            IdForResult::Resize { bits, value } => self.on_resize(bits, Some(value)),
        };
        // Java's snapshot is a live reference into the palette backing array,
        // so it stays current through every palette mutation — including
        // non-resize inserts that never reach `on_resize`. The owned `Vec`
        // snapshot must be re-materialized after each `id_for` (the only
        // palette-mutating call) to match.
        self.update_data();
        result
    }

    /// `get(int x, int y, int z)`.
    pub fn get(&self, x: i32, y: i32, z: i32) -> T {
        let index = self.strategy.get_index(x, y, z);
        let state = self.data.storage.get(index);
        self.read_palette(&self.data, state)
    }

    /// `get(int index)`.
    pub fn get_index(&self, index: usize) -> T {
        let state = self.data.storage.get(index);
        self.read_palette(&self.data, state)
    }

    /// `getAll(Consumer<T>)` — every distinct value present, each once.
    ///
    /// Java's `IntArraySet` preserves first-appearance order (dedupes while
    /// keeping insertion order), so the port uses a `HashSet` for membership
    /// plus a `Vec` for order rather than a sorted set.
    pub fn get_all(&self, mut consumer: impl FnMut(T)) {
        let mut seen = HashSet::new();
        let mut order = Vec::new();
        self.data.storage.get_all(&mut |state| {
            if seen.insert(state) {
                order.push(state);
            }
        });
        for state in order {
            consumer(self.data.palette.value_for(state));
        }
    }

    /// `read(FriendlyByteBuf)`.
    pub fn read(&mut self, buffer: &mut FriendlyByteBuf) {
        let new_bits = buffer.read_byte() as i32;
        let mut new_data = Self::create_or_reuse_data(&self.strategy, Some(&self.data), new_bits);
        new_data.palette.read(buffer, self.strategy.global_map());
        buffer.read_fixed_size_long_array(new_data.storage.get_raw_mut());
        self.data = new_data;
        // Paper Anti-Xray: the server re-inserts the preset values after a
        // read so the wire palette keeps containing them (Java notes this is
        // "inefficient, but this isn't used by the server"). Refreshes the
        // read-path snapshot whether or not presets are configured.
        self.add_preset_values();
    }

    /// `write(FriendlyByteBuf)`.
    pub fn write(&self, buffer: &mut FriendlyByteBuf) {
        self.data.write(buffer, self.strategy.global_map());
    }

    /// `getSerializedSize()` — `1 + palette + raw.len() * 8`.
    pub fn get_serialized_size(&self) -> i32 {
        self.data.get_serialized_size(self.strategy.global_map())
    }

    /// `bitsPerEntry()` — the in-memory storage width (`Data.storage.getBits`).
    pub fn bits_per_entry(&self) -> i32 {
        self.data.storage.get_bits()
    }

    /// `maybeHas(Predicate)` — consults the palette only.
    pub fn maybe_has(&self, mut predicate: impl FnMut(&T) -> bool) -> bool {
        self.data.palette.maybe_has(&mut predicate)
    }

    /// `forEachInPalette(Consumer<T>)`.
    pub fn for_each_in_palette(&self, mut consumer: impl FnMut(T)) {
        for i in 0..self.data.palette.get_size() {
            consumer(self.data.palette.value_for(i));
        }
    }

    /// `count(CountConsumer<T>)`.
    pub fn count(&self, mut output: impl FnMut(T, i32)) {
        if self.data.palette.get_size() == 1 {
            output(
                self.data.palette.value_for(0),
                self.data.storage.get_size() as i32,
            );
        } else {
            // Java `PalettedContainer.count` tallies with an
            // `Int2IntOpenHashMap` over `getAll`; the `HashMap` mirrors it,
            // including the nondeterministic iteration order (unobservable:
            // the consumer sums). Moonrise's `moonrise$countEntries` fast path
            // is a separate surface ported on the storages (issue #216); it
            // feeds the section-level `recalcBlockCounts`, not this method.
            let mut counts = HashMap::new();
            self.data.storage.get_all(&mut |state| {
                *counts.entry(state).or_insert(0) += 1;
            });
            for (state, count) in counts {
                output(self.data.palette.value_for(state), count);
            }
        }
    }

    /// `copy()`.
    pub fn copy(&self) -> Self {
        PalettedContainer {
            strategy: self.strategy.clone(),
            data: self.data.copy(),
            preset_values: self.preset_values.clone(),
        }
    }

    /// `recreate()` — a fresh single-value container holding the palette's
    /// first entry (and, on the Anti-Xray path, the preset values).
    pub fn recreate(&self) -> Self {
        PalettedContainer::new_with_preset_values(
            self.data.palette.value_for(0),
            self.strategy.clone(),
            self.preset_values.clone(),
        )
    }

    /// `pack(Strategy<T>)` — the NBT codec path. Re-encodes the storage against
    /// a fresh `HashMapPalette`, then picks the on-disc configuration by the
    /// palette size. Takes the strategy explicitly (Java's signature — the
    /// `PalettedContainerRO` read-view takes it as an argument); the container's
    /// own strategy is the value every caller passes.
    pub fn pack_with_strategy(&self, strategy: &Strategy<T>) -> PackedData<T> {
        let current_storage = &*self.data.storage;
        let current_palette = &*self.data.palette;
        let mut new_palette = HashMapPalette::new(current_storage.get_bits(), Vec::new());
        let new_contents = reencode_contents(current_storage, current_palette, &mut new_palette);
        let stored_configuration = strategy.configuration_for_palette_size(new_palette.get_size());
        let bits_on_disc = stored_configuration.bits_in_storage();
        let values = if bits_on_disc != 0 {
            let storage = SimpleBitStorage::from_values(
                bits_on_disc,
                strategy.entry_count() as usize,
                &new_contents,
            );
            Some(storage.get_raw().to_vec())
        } else {
            None
        };
        PackedData::with_bits(new_palette.get_entries(), values, bits_on_disc)
    }

    /// `pack()` — [`pack_with_strategy`](Self::pack_with_strategy) with the
    /// container's own strategy (the common call; Java always passes the same
    /// strategy the container was built with).
    pub fn pack(&self) -> PackedData<T> {
        self.pack_with_strategy(&self.strategy)
    }

    /// `unpack(Strategy<T>, PackedData<T>)` — Anti-Xray disabled
    /// (`presetValues == null` in Java).
    pub fn unpack(strategy: &Strategy<T>, disc_data: PackedData<T>) -> Result<Self, String> {
        Self::unpack_impl(strategy, disc_data, None, None)
    }

    /// `unpack(Strategy<T>, PackedData<T>, T defaultValue, T[] presetValues)`.
    ///
    /// Reconstructs a container from the NBT packed form, re-encoding the
    /// storage whenever the on-disc bits differ from the in-memory width (the
    /// `alwaysRepack` / width-mismatch path). `default_value`/`preset_values`
    /// feed the Anti-Xray constructor block (null in the server). Errors mirror
    /// Java's `DataResult.error` messages.
    pub fn unpack_with_preset_values(
        strategy: &Strategy<T>,
        disc_data: PackedData<T>,
        default_value: T,
        preset_values: Option<Vec<T>>,
    ) -> Result<Self, String> {
        Self::unpack_impl(strategy, disc_data, Some(default_value), preset_values)
    }

    /// The shared decode; `default_value` is only consulted by the Anti-Xray
    /// block (absent when no preset values are configured).
    fn unpack_impl(
        strategy: &Strategy<T>,
        disc_data: PackedData<T>,
        default_value: Option<T>,
        preset_values: Option<Vec<T>>,
    ) -> Result<Self, String> {
        let palette_entries = disc_data.palette_entries;
        let entry_count = strategy.entry_count();
        let stored_configuration =
            strategy.configuration_for_palette_size(palette_entries.len() as i32);
        let bits_on_disc = stored_configuration.bits_in_storage();
        if disc_data.bits_per_entry != PackedData::<T>::UNKNOWN_BITS_PER_ENTRY
            && bits_on_disc != disc_data.bits_per_entry
        {
            return Err(format!(
                "Invalid bit count, calculated {}, but container declared {}",
                bits_on_disc, disc_data.bits_per_entry
            ));
        }

        let (storage, palette): (Box<dyn BitStorage>, Box<dyn Palette<T>>) = if stored_configuration
            .bits_in_memory()
            == 0
        {
            let palette = stored_configuration.create_palette(strategy, palette_entries.clone());
            let storage: Box<dyn BitStorage> = Box::new(ZeroBitStorage::new(entry_count as usize));
            (storage, palette)
        } else {
            let data = disc_data
                .storage
                .ok_or_else(|| "Missing values for non-zero storage".to_string())?;
            if !stored_configuration.always_repack()
                && stored_configuration.bits_in_memory() == bits_on_disc
            {
                let palette =
                    stored_configuration.create_palette(strategy, palette_entries.clone());
                let storage = SimpleBitStorage::from_raw(bits_on_disc, entry_count as usize, &data)
                    .map_err(|e: InitializationException| {
                        format!("Failed to read PalettedContainer: {}", e)
                    })?;
                (Box::new(storage) as Box<dyn BitStorage>, palette)
            } else {
                let old_palette = HashMapPalette::new(bits_on_disc, palette_entries.clone());
                let old_storage =
                    SimpleBitStorage::from_raw(bits_on_disc, entry_count as usize, &data).map_err(
                        |e: InitializationException| {
                            format!("Failed to read PalettedContainer: {}", e)
                        },
                    )?;
                // Java passes `paletteEntries` to the new palette's factory too
                // (for a Global config the factory ignores them; kept for exact
                // fidelity).
                let mut new_palette =
                    stored_configuration.create_palette(strategy, palette_entries.clone());
                let new_contents = reencode_contents(&old_storage, &old_palette, &mut *new_palette);
                let storage = SimpleBitStorage::from_values(
                    stored_configuration.bits_in_memory(),
                    entry_count as usize,
                    &new_contents,
                );
                (Box::new(storage) as Box<dyn BitStorage>, new_palette)
            }
        };

        Ok(PalettedContainer::from_data(
            strategy.clone(),
            stored_configuration,
            storage,
            palette,
            palette_entries,
            default_value,
            preset_values,
        ))
    }

    /// The global map (exposed for tests / callers needing the wire global ids).
    pub fn global_map(&self) -> &dyn GlobalIdMap<T> {
        self.strategy.global_map()
    }

    /// The strategy's global-palette in-memory width.
    pub fn global_palette_bits_in_memory(&self) -> i32 {
        self.strategy.global_palette_bits_in_memory()
    }
}

/// `PalettedContainerRO<T>` — the read-view surface of a paletted container
/// (Java's interface of the same name; `PalettedContainer` implements it).
///
/// `copy`/`recreate`/`pack` return the concrete `PalettedContainer` (Java's
/// covariant returns). The mutating surface (`set`, `getAndSet`, `read`) is
/// deliberately absent — it is not part of the read view.
///
/// The methods delegate to the inherent [`PalettedContainer`] surface; this
/// trait exists so a value that only needs reads can be typed by capability
/// (e.g. the factory's `biomeContainerCodec` in Java is `Codec<
/// PalettedContainerRO<Holder<Biome>>>`).
pub trait PalettedContainerRO<T: Clone + PartialEq + Send + std::fmt::Debug + 'static> {
    /// `get(int, int, int)`.
    fn get(&self, x: i32, y: i32, z: i32) -> T;
    /// `getAll(Consumer<T>)`.
    fn get_all(&self, consumer: &mut dyn FnMut(T));
    /// `write(FriendlyByteBuf)` — the deprecated no-Anti-Xray-info variant.
    ///
    /// RivetTodo(#216): the Anti-Xray overload `write(FriendlyByteBuf,
    /// @Nullable ChunkPacketInfo<T>, int chunkSectionIndex)` is omitted —
    /// `ChunkPacketInfo` is deferred with the `paper.antixray` chunk-storage
    /// unit; re-add it when that type lands.
    fn write(&self, buffer: &mut FriendlyByteBuf);
    /// `getSerializedSize()`.
    fn get_serialized_size(&self) -> i32;
    /// `bitsPerEntry()` — the in-memory storage width.
    fn bits_per_entry(&self) -> i32;
    /// `maybeHas(Predicate<T>)`.
    fn maybe_has(&self, predicate: &mut dyn FnMut(&T) -> bool) -> bool;
    /// `forEachInPalette(Consumer<T>)`.
    fn for_each_in_palette(&self, consumer: &mut dyn FnMut(T));
    /// `count(PalettedContainer.CountConsumer<T>)`.
    fn count(&self, output: &mut dyn FnMut(T, i32));
    /// `copy()`.
    fn copy(&self) -> PalettedContainer<T>;
    /// `recreate()`.
    fn recreate(&self) -> PalettedContainer<T>;
    /// `pack(Strategy<T>)`.
    fn pack(&self, strategy: &Strategy<T>) -> PackedData<T>;
}

impl<T: Clone + PartialEq + Send + std::fmt::Debug + 'static> PalettedContainerRO<T>
    for PalettedContainer<T>
{
    fn get(&self, x: i32, y: i32, z: i32) -> T {
        PalettedContainer::get(self, x, y, z)
    }

    fn get_all(&self, consumer: &mut dyn FnMut(T)) {
        PalettedContainer::get_all(self, consumer)
    }

    fn write(&self, buffer: &mut FriendlyByteBuf) {
        PalettedContainer::write(self, buffer)
    }

    fn get_serialized_size(&self) -> i32 {
        PalettedContainer::get_serialized_size(self)
    }

    fn bits_per_entry(&self) -> i32 {
        PalettedContainer::bits_per_entry(self)
    }

    fn maybe_has(&self, predicate: &mut dyn FnMut(&T) -> bool) -> bool {
        PalettedContainer::maybe_has(self, predicate)
    }

    fn for_each_in_palette(&self, consumer: &mut dyn FnMut(T)) {
        PalettedContainer::for_each_in_palette(self, consumer)
    }

    fn count(&self, output: &mut dyn FnMut(T, i32)) {
        PalettedContainer::count(self, output)
    }

    fn copy(&self) -> PalettedContainer<T> {
        PalettedContainer::copy(self)
    }

    fn recreate(&self) -> PalettedContainer<T> {
        PalettedContainer::recreate(self)
    }

    fn pack(&self, strategy: &Strategy<T>) -> PackedData<T> {
        PalettedContainer::pack_with_strategy(self, strategy)
    }
}

/// `PalettedContainer.Data<T>` — the configuration/storage/palette triple
/// plus the Moonrise `FastPaletteData` read-path snapshot (issue #216).
pub struct Data<T: Clone + PartialEq + Send + 'static> {
    configuration: Configuration,
    storage: Box<dyn BitStorage>,
    palette: Box<dyn Palette<T>>,
    /// `moonrise$palette` — the materialized palette snapshot used by the
    /// `read_palette` fast path (`None` when no palette materializes one).
    snapshot: Option<Vec<T>>,
}

impl<T: Clone + PartialEq + Send + std::fmt::Debug + 'static> Data<T> {
    /// `createOrReuseData`'s fresh-data branch: storage sized by
    /// `configuration.bits_in_memory()` (zero-width -> `ZeroBitStorage`),
    /// palette from the configuration with no entries.
    fn new(configuration: Configuration, strategy: &Strategy<T>) -> Self {
        let entry_count = strategy.entry_count();
        let storage: Box<dyn BitStorage> = if configuration.bits_in_memory() == 0 {
            Box::new(ZeroBitStorage::new(entry_count as usize))
        } else {
            Box::new(SimpleBitStorage::new(
                configuration.bits_in_memory(),
                entry_count as usize,
            ))
        };
        let palette = configuration.create_palette(strategy, Vec::new());
        Data {
            configuration,
            storage,
            palette,
            snapshot: None,
        }
    }

    /// `copyFrom(Palette oldPalette, BitStorage oldStorage)`.
    fn copy_from(&mut self, old_palette: &dyn Palette<T>, old_storage: &dyn BitStorage) {
        for i in 0..old_storage.get_size() {
            let value = old_palette.value_for(old_storage.get(i));
            self.storage
                .set(i, self.palette.id_for(&value).expect_no_resize());
        }
    }

    /// `Data.copy()`.
    fn copy(&self) -> Self {
        Data {
            configuration: self.configuration.clone(),
            storage: self.storage.copy_box(),
            palette: self.palette.copy_palette(),
            snapshot: self.snapshot.clone(),
        }
    }

    /// `Data.write(FriendlyByteBuf, IdMap)`.
    fn write(&self, buffer: &mut FriendlyByteBuf, global_map: &dyn GlobalIdMap<T>) {
        buffer.write_byte(self.storage.get_bits() as i8);
        self.palette.write(buffer, global_map);
        buffer.write_fixed_size_long_array(self.storage.get_raw());
    }

    /// `Data.getSerializedSize(IdMap)`.
    fn get_serialized_size(&self, global_map: &dyn GlobalIdMap<T>) -> i32 {
        1 + self.palette.get_serialized_size(global_map) + self.storage.get_raw().len() as i32 * 8
    }
}

/// `reencodeContents(BitStorage, Palette, Palette)` — re-maps every stored
/// entry through `old_palette.valueFor` then `new_palette.idFor`, skipping
/// re-mapping runs of identical ids (Java's `lastReadId`/`lastWrittenId`
/// cache).
pub fn reencode_contents<T: Clone + PartialEq + Send + std::fmt::Debug + 'static>(
    storage: &dyn BitStorage,
    old_palette: &dyn Palette<T>,
    new_palette: &mut dyn Palette<T>,
) -> Vec<i32> {
    let size = storage.get_size();
    let mut buffer = vec![0i32; size];
    storage.unpack(&mut buffer);

    let mut last_read_id = -1;
    let mut last_written_id = -1;
    for slot in buffer.iter_mut() {
        let id = *slot;
        if id != last_read_id {
            last_read_id = id;
            last_written_id = new_palette
                .id_for(&old_palette.value_for(id))
                .expect_no_resize();
        }
        *slot = last_written_id;
    }
    buffer
}
