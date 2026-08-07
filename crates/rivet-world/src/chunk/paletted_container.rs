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
//! Deferred (not part of M1): the Anti-Xray `presetValues` surface, the
//! Moonrise `FastPalette` read-path snapshot, and `acquire`/`release`
//! threading guards (the container is tick-thread-confined game state —
//! OWNERSHIP.md — and Java's `synchronized` is dropped with a note, as
//! PORTING.md prescribes for tick-confined state).
//!
//! RivetTodo(#216): the Anti-Xray `presetValues` surface and Moonrise
//! `FastPalette` read-path snapshot are not ported (deferred to the M2 chunk
//! storage epic #15); the threading guards are intentionally dropped
//! (tick-thread-confined state).
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

use crate::chunk::configuration::Configuration;
use crate::chunk::palette::{GlobalIdMap, HashMapPalette, IdForResult, Palette};
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
}

impl<T: Clone + PartialEq + Send + std::fmt::Debug + 'static> PalettedContainer<T> {
    /// `PalettedContainer(T initialValue, Strategy<T>)`.
    ///
    /// Starts at the zero-bit configuration (single-value palette,
    /// `ZeroBitStorage`), then inserts `initial_value`.
    pub fn new(initial_value: T, strategy: Strategy<T>) -> Self {
        let mut data = Self::create_or_reuse_data(&strategy, None, 0);
        let _ = data.palette.id_for(&initial_value);
        PalettedContainer { strategy, data }
    }

    /// The private constructor used by `unpack`.
    pub(crate) fn from_data(
        strategy: Strategy<T>,
        configuration: Configuration,
        storage: Box<dyn BitStorage>,
        palette: Box<dyn Palette<T>>,
    ) -> Self {
        PalettedContainer {
            strategy,
            data: Data {
                configuration,
                storage,
                palette,
            },
        }
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
    /// contents across, then inserts `last_added_value`, returning its index.
    fn on_resize(&mut self, bits: i32, last_added_value: T) -> i32 {
        let configuration = self.strategy.configuration_for_bit_count(bits);
        if self.data.configuration == configuration {
            // Java's createOrReuseData reuses the same Data; copyFrom(old, old)
            // is a self-copy no-op. Only the final insert is observable.
            return self
                .data
                .palette
                .id_for(&last_added_value)
                .expect_no_resize();
        }
        let mut new_data = Data::new(configuration, &self.strategy);
        new_data.copy_from(&*self.data.palette, &*self.data.storage);
        self.data = new_data;
        self.data
            .palette
            .id_for(&last_added_value)
            .expect_no_resize()
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
        self.data.palette.value_for(prev)
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
        match self.data.palette.id_for(&value) {
            IdForResult::Id(id) => id,
            IdForResult::Resize { bits, value } => self.on_resize(bits, value),
        }
    }

    /// `get(int x, int y, int z)`.
    pub fn get(&self, x: i32, y: i32, z: i32) -> T {
        let index = self.strategy.get_index(x, y, z);
        let state = self.data.storage.get(index);
        self.data.palette.value_for(state)
    }

    /// `get(int index)`.
    pub fn get_index(&self, index: usize) -> T {
        let state = self.data.storage.get(index);
        self.data.palette.value_for(state)
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
        }
    }

    /// `recreate()` — a fresh single-value container holding the palette's
    /// first entry.
    pub fn recreate(&self) -> Self {
        PalettedContainer::new(self.data.palette.value_for(0), self.strategy.clone())
    }

    /// `pack(Strategy<T>)` — the NBT codec path. Re-encodes the storage against
    /// a fresh `HashMapPalette`, then picks the on-disc configuration by the
    /// palette size.
    pub fn pack(&self) -> PackedData<T> {
        let current_storage = &*self.data.storage;
        let current_palette = &*self.data.palette;
        let mut new_palette = HashMapPalette::new(current_storage.get_bits(), Vec::new());
        let new_contents = reencode_contents(current_storage, current_palette, &mut new_palette);
        let stored_configuration = self
            .strategy
            .configuration_for_palette_size(new_palette.get_size());
        let bits_on_disc = stored_configuration.bits_in_storage();
        let values = if bits_on_disc != 0 {
            let storage = SimpleBitStorage::from_values(
                bits_on_disc,
                self.strategy.entry_count() as usize,
                &new_contents,
            );
            Some(storage.get_raw().to_vec())
        } else {
            None
        };
        PackedData::with_bits(new_palette.get_entries(), values, bits_on_disc)
    }

    /// `unpack(Strategy<T>, PackedData<T>, T defaultValue, T[] presetValues)`.
    ///
    /// Reconstructs a container from the NBT packed form, re-encoding the
    /// storage whenever the on-disc bits differ from the in-memory width (the
    /// `alwaysRepack` / width-mismatch path). `defaultValue`/`presetValues`
    /// feed only the Anti-Xray constructor block (null in the server), so they
    /// are dropped. Errors mirror Java's `DataResult.error` messages.
    pub fn unpack(strategy: &Strategy<T>, disc_data: PackedData<T>) -> Result<Self, String> {
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
            let palette = stored_configuration.create_palette(strategy, palette_entries);
            let storage: Box<dyn BitStorage> = Box::new(ZeroBitStorage::new(entry_count as usize));
            (storage, palette)
        } else {
            let data = disc_data
                .storage
                .ok_or_else(|| "Missing values for non-zero storage".to_string())?;
            if !stored_configuration.always_repack()
                && stored_configuration.bits_in_memory() == bits_on_disc
            {
                let palette = stored_configuration.create_palette(strategy, palette_entries);
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
                    stored_configuration.create_palette(strategy, palette_entries);
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

/// `PalettedContainer.Data<T>` — the configuration/storage/palette triple.
pub struct Data<T: Clone + PartialEq + Send + 'static> {
    configuration: Configuration,
    storage: Box<dyn BitStorage>,
    palette: Box<dyn Palette<T>>,
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
