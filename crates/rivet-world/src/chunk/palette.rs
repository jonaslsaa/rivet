//! Port of `net.minecraft.world.level.chunk.Palette<T>` (MC 26.2).
//!
//! The wire contract (via `FriendlyByteBuf`):
//! - `id_for(value, resize)` — the palette-local index to store in the
//!   container's `BitStorage`, calling `resize` when the current width can't
//!   hold another entry (Linear/HashMap) or the value isn't representable
//!   (SingleValue).
//! - `read` / `write` — the palette section of the chunk wire format:
//!   SingleValue writes the global id (varint); Linear/HashMap write the
//!   entry count (varint) then each global id (varint); Global writes nothing.
//! - `get_serialized_size` — exact varint byte counts, used to size the
//!   chunk payload before it is written.
//!
//! Paper's Moonrise `FastPalette`/`FastPaletteData` hooks are optimizations on
//! the read path (materialized `T[]` snapshots) that do not change the wire
//! format or palette semantics; they are not ported.

use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_registry::id_map::DEFAULT_ID;
use rivet_util::mth;

/// The result of `Palette::id_for`. Java calls the container's resize handler
/// synchronously inside `idFor` and returns the handler's index; the Rust port
/// defers the resize to the caller so the palette never holds a `&mut` borrow
/// into the container. The observable behaviour (which index is stored, the
/// final palette contents) is identical.
pub enum IdForResult<T> {
    /// The value fits; this is its palette-local index.
    Id(i32),
    /// The value doesn't fit; the container must grow to `bits` and add
    /// `value`, returning its index in the grown palette.
    Resize { bits: i32, value: T },
}

impl<T: std::fmt::Debug> IdForResult<T> {
    /// The container's internal re-encode paths (`Data.copy_from`,
    /// `reencode_contents`, `on_resize`'s final insert) pass through a
    /// "no resize expected" handler in Java; a resize request there means the
    /// target palette was sized wrong and panics.
    pub fn expect_no_resize(self) -> i32 {
        match self {
            IdForResult::Id(id) => id,
            IdForResult::Resize { bits, value } => panic!(
                "Unexpected palette resize, bits = {}, added value = {:?}",
                bits, value
            ),
        }
    }
}

/// The global id map surface palettes and the container need (Java
/// `IdMap<T>`). `by_id` returns an owned value because the Rust port models
/// block states as copy ids, not references.
pub trait GlobalIdMap<T: Clone + Send + 'static>: Send {
    /// `IdMap.getId(T)` — `-1` when absent.
    fn get_id(&self, value: &T) -> i32;

    /// `IdMap.byIdOrThrow(int)` — panics `"No value with id {id}"` when
    /// absent.
    fn by_id_or_throw(&self, id: i32) -> T;

    /// `IdMap.size()`.
    fn size(&self) -> i32;

    /// `IdMap.byId(int)` — `Option::None` when absent.
    fn by_id(&self, id: i32) -> Option<T>;

    fn clone_box(&self) -> Box<dyn GlobalIdMap<T>>;
}

/// `net.minecraft.world.level.chunk.Palette<T>`.
pub trait Palette<T: Clone + PartialEq + Send + 'static>: Send {
    /// `idFor(T, PaletteResize)` — returns the palette-local index, or a
    /// resize request for the container to handle.
    fn id_for(&mut self, value: &T) -> IdForResult<T>;

    /// `maybeHas(Predicate)`.
    fn maybe_has(&self, predicate: &mut dyn FnMut(&T) -> bool) -> bool;

    /// `valueFor(int)`.
    fn value_for(&self, index: i32) -> T;

    /// `read(FriendlyByteBuf, IdMap)` — rebuild from the wire palette section.
    fn read(&mut self, buffer: &mut FriendlyByteBuf, global_map: &dyn GlobalIdMap<T>);

    /// `write(FriendlyByteBuf, IdMap)`.
    fn write(&self, buffer: &mut FriendlyByteBuf, global_map: &dyn GlobalIdMap<T>);

    /// `getSerializedSize(IdMap)` — exact byte count of the wire palette
    /// section.
    fn get_serialized_size(&self, global_map: &dyn GlobalIdMap<T>) -> i32;

    /// `getSize()` — the number of entries in the palette.
    fn get_size(&self) -> i32;

    /// `copy()` — a fresh palette with identical contents.
    fn copy_palette(&self) -> Box<dyn Palette<T>>;
}

// ---------------------------------------------------------------------------

/// `Palette.Factory` — `create(bits, entries)`.
pub trait PaletteFactory {
    fn create<T: Clone + PartialEq + Send + 'static>(
        &self,
        bits: i32,
        entries: Vec<T>,
    ) -> Box<dyn Palette<T>>;
}

/// `Strategy.SINGLE_VALUE_PALETTE_FACTORY`.
pub struct SingleValueFactory;

impl PaletteFactory for SingleValueFactory {
    fn create<T: Clone + PartialEq + Send + 'static>(
        &self,
        _bits: i32,
        entries: Vec<T>,
    ) -> Box<dyn Palette<T>> {
        Box::new(SingleValuePalette::new(entries))
    }
}

/// `Strategy`'s private `LINEAR_PALETTE_FACTORY`.
pub struct LinearFactory;

impl PaletteFactory for LinearFactory {
    fn create<T: Clone + PartialEq + Send + 'static>(
        &self,
        bits: i32,
        entries: Vec<T>,
    ) -> Box<dyn Palette<T>> {
        Box::new(LinearPalette::new(bits, entries))
    }
}

/// `Strategy`'s private `HASHMAP_PALETTE_FACTORY`.
pub struct HashMapFactory;

impl PaletteFactory for HashMapFactory {
    fn create<T: Clone + PartialEq + Send + 'static>(
        &self,
        bits: i32,
        entries: Vec<T>,
    ) -> Box<dyn Palette<T>> {
        Box::new(HashMapPalette::new(bits, entries))
    }
}

/// `Mth.ceillog2` — the palette-width function (`minimumBitsRequiredForDistinctValues`).
pub fn ceillog2(count: i32) -> i32 {
    mth::ceillog2(count)
}

/// `MissingPaletteEntryException` — `value_for` with an index the palette
/// doesn't hold. Java's `RuntimeException` maps to `panic!` (PORTING.md).
pub fn missing_palette_entry(index: i32) -> ! {
    panic!("Missing Palette entry for index {}.", index)
}

// ---------------------------------------------------------------------------
// SingleValuePalette
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.chunk.SingleValuePalette<T>`.
pub struct SingleValuePalette<T> {
    value: Option<T>,
}

impl<T> SingleValuePalette<T> {
    /// `SingleValuePalette(List<T> paletteEntries)` — panics if more than one
    /// entry is given (Java `Validate.isTrue`).
    pub fn new(mut entries: Vec<T>) -> Self {
        assert!(
            entries.len() <= 1,
            "Can't initialize SingleValuePalette with {} values.",
            entries.len()
        );
        SingleValuePalette {
            value: if entries.is_empty() {
                None
            } else {
                Some(entries.remove(0))
            },
        }
    }
}

impl<T: Clone + PartialEq + Send + 'static> SingleValuePalette<T> {
    /// `SingleValuePalette.create`.
    pub fn create(bits: i32, entries: Vec<T>) -> Box<dyn Palette<T>> {
        let _ = bits;
        Box::new(SingleValuePalette::new(entries))
    }
}

impl<T: Clone + PartialEq + Send + 'static> Palette<T> for SingleValuePalette<T> {
    fn id_for(&mut self, value: &T) -> IdForResult<T> {
        if self.value.is_some() && self.value.as_ref() != Some(value) {
            return IdForResult::Resize {
                bits: 1,
                value: value.clone(),
            };
        }
        self.value = Some(value.clone());
        IdForResult::Id(0)
    }

    fn maybe_has(&self, predicate: &mut dyn FnMut(&T) -> bool) -> bool {
        match &self.value {
            None => panic!("Use of an uninitialized palette"),
            Some(v) => predicate(v),
        }
    }

    fn value_for(&self, index: i32) -> T {
        match &self.value {
            Some(v) if index == 0 => v.clone(),
            _ => missing_palette_entry(index),
        }
    }

    fn read(&mut self, buffer: &mut FriendlyByteBuf, global_map: &dyn GlobalIdMap<T>) {
        let id = buffer.read_var_int();
        self.value = Some(global_map.by_id_or_throw(id));
    }

    fn write(&self, buffer: &mut FriendlyByteBuf, global_map: &dyn GlobalIdMap<T>) {
        match &self.value {
            None => panic!("Use of an uninitialized palette"),
            Some(v) => {
                buffer.write_var_int(global_map.get_id(v));
            }
        }
    }

    fn get_serialized_size(&self, global_map: &dyn GlobalIdMap<T>) -> i32 {
        match &self.value {
            None => panic!("Use of an uninitialized palette"),
            Some(v) => rivet_protocol::var_int::get_byte_size(global_map.get_id(v)),
        }
    }

    fn get_size(&self) -> i32 {
        1
    }

    fn copy_palette(&self) -> Box<dyn Palette<T>> {
        if self.value.is_none() {
            panic!("Use of an uninitialized palette");
        }
        Box::new(SingleValuePalette {
            value: self.value.clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// LinearPalette
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.chunk.LinearPalette<T>`.
pub struct LinearPalette<T> {
    values: Vec<Option<T>>,
    bits: i32,
    size: i32,
}

impl<T> LinearPalette<T> {
    /// `LinearPalette(int bits, List<T> paletteEntries)` — panics when the
    /// entries exceed the capacity `1 << bits` (Java `Validate.isTrue`).
    pub fn new(bits: i32, palette_entries: Vec<T>) -> Self {
        let capacity = 1usize << bits;
        assert!(
            palette_entries.len() <= capacity,
            "Can't initialize LinearPalette of size {} with {} entries",
            capacity,
            palette_entries.len()
        );
        let size = palette_entries.len() as i32;
        let mut values = Vec::with_capacity(capacity);
        values.extend(palette_entries.into_iter().map(Some));
        values.resize_with(capacity, || None);
        LinearPalette { values, bits, size }
    }
}

impl<T: Clone + PartialEq + Send + 'static> LinearPalette<T> {
    /// `LinearPalette.create`.
    pub fn create(bits: i32, entries: Vec<T>) -> Box<dyn Palette<T>> {
        Box::new(LinearPalette::new(bits, entries))
    }
}

impl<T: Clone + PartialEq + Send + 'static> Palette<T> for LinearPalette<T> {
    fn id_for(&mut self, value: &T) -> IdForResult<T> {
        for i in 0..self.size as usize {
            if self.values[i].as_ref() == Some(value) {
                return IdForResult::Id(i as i32);
            }
        }

        let index = self.size as usize;
        if index < self.values.len() {
            self.values[index] = Some(value.clone());
            self.size += 1;
            IdForResult::Id(index as i32)
        } else {
            IdForResult::Resize {
                bits: self.bits + 1,
                value: value.clone(),
            }
        }
    }

    fn maybe_has(&self, predicate: &mut dyn FnMut(&T) -> bool) -> bool {
        for i in 0..self.size as usize {
            if predicate(self.values[i].as_ref().unwrap()) {
                return true;
            }
        }
        false
    }

    fn value_for(&self, index: i32) -> T {
        if index >= 0 && (index as usize) < self.size as usize {
            self.values[index as usize].clone().unwrap()
        } else {
            missing_palette_entry(index)
        }
    }

    fn read(&mut self, buffer: &mut FriendlyByteBuf, global_map: &dyn GlobalIdMap<T>) {
        self.size = buffer.read_var_int();
        for i in 0..self.size as usize {
            let id = buffer.read_var_int();
            self.values[i] = Some(global_map.by_id_or_throw(id));
        }
    }

    fn write(&self, buffer: &mut FriendlyByteBuf, global_map: &dyn GlobalIdMap<T>) {
        buffer.write_var_int(self.size);
        for i in 0..self.size as usize {
            buffer.write_var_int(global_map.get_id(self.values[i].as_ref().unwrap()));
        }
    }

    fn get_serialized_size(&self, global_map: &dyn GlobalIdMap<T>) -> i32 {
        let mut result = rivet_protocol::var_int::get_byte_size(self.size);
        for i in 0..self.size as usize {
            result += rivet_protocol::var_int::get_byte_size(
                global_map.get_id(self.values[i].as_ref().unwrap()),
            );
        }
        result
    }

    fn get_size(&self) -> i32 {
        self.size
    }

    fn copy_palette(&self) -> Box<dyn Palette<T>> {
        Box::new(LinearPalette {
            values: self.values.clone(),
            bits: self.bits,
            size: self.size,
        })
    }
}

// ---------------------------------------------------------------------------
// HashMapPalette
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.chunk.HashMapPalette<T>`.
///
/// Uses an insertion-order `Vec` as the identity map, which preserves Java's
/// `CrudeIncrementalIntIdentityHashBiMap` observable wire order (ids are
/// insertion order, compact). The Java hash-slots machinery is a performance
/// detail that does not affect `idFor`/`getEntries` order.
pub struct HashMapPalette<T> {
    values: Vec<T>,
    bits: i32,
}

impl<T> HashMapPalette<T> {
    /// `HashMapPalette(int bits, List<T> values)`.
    pub fn new(bits: i32, values: Vec<T>) -> Self {
        HashMapPalette { values, bits }
    }
}

impl<T: Clone + PartialEq + Send + 'static> HashMapPalette<T> {
    /// `HashMapPalette.create`.
    pub fn create(bits: i32, entries: Vec<T>) -> Box<dyn Palette<T>> {
        Box::new(HashMapPalette::new(bits, entries))
    }

    /// `getEntries()` — the palette entries in insertion order (Java's
    /// `CrudeIncrementalIntIdentityHashBiMap` iterator is `byId` order, which
    /// is exactly insertion order).
    pub fn get_entries(&self) -> Vec<T> {
        self.values.clone()
    }
}

impl<T: Clone + PartialEq + Send + 'static> Palette<T> for HashMapPalette<T> {
    fn id_for(&mut self, value: &T) -> IdForResult<T> {
        let id = self
            .values
            .iter()
            .position(|v| v == value)
            .map(|p| p as i32)
            .unwrap_or(DEFAULT_ID);
        if id == DEFAULT_ID {
            if self.values.len() >= 1usize << self.bits {
                IdForResult::Resize {
                    bits: self.bits + 1,
                    value: value.clone(),
                }
            } else {
                self.values.push(value.clone());
                IdForResult::Id(self.values.len() as i32 - 1)
            }
        } else {
            IdForResult::Id(id)
        }
    }

    fn maybe_has(&self, predicate: &mut dyn FnMut(&T) -> bool) -> bool {
        for v in &self.values {
            if predicate(v) {
                return true;
            }
        }
        false
    }

    fn value_for(&self, index: i32) -> T {
        match self.values.get(index as usize) {
            Some(v) => v.clone(),
            None => missing_palette_entry(index),
        }
    }

    fn read(&mut self, buffer: &mut FriendlyByteBuf, global_map: &dyn GlobalIdMap<T>) {
        self.values.clear();
        let size = buffer.read_var_int();
        for _ in 0..size {
            let id = buffer.read_var_int();
            self.values.push(global_map.by_id_or_throw(id));
        }
    }

    fn write(&self, buffer: &mut FriendlyByteBuf, global_map: &dyn GlobalIdMap<T>) {
        buffer.write_var_int(self.values.len() as i32);
        for v in &self.values {
            buffer.write_var_int(global_map.get_id(v));
        }
    }

    fn get_serialized_size(&self, global_map: &dyn GlobalIdMap<T>) -> i32 {
        let mut size = rivet_protocol::var_int::get_byte_size(self.values.len() as i32);
        for v in &self.values {
            size += rivet_protocol::var_int::get_byte_size(global_map.get_id(v));
        }
        size
    }

    fn get_size(&self) -> i32 {
        self.values.len() as i32
    }

    fn copy_palette(&self) -> Box<dyn Palette<T>> {
        Box::new(HashMapPalette {
            values: self.values.clone(),
            bits: self.bits,
        })
    }
}

// ---------------------------------------------------------------------------
// GlobalPalette
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.chunk.GlobalPalette<T>` — the 15-bit (block) /
/// global-id palette. The wire palette section is empty (the entries are the
/// global ids themselves).
pub struct GlobalPalette<T> {
    registry: Box<dyn GlobalIdMap<T>>,
}

impl<T> GlobalPalette<T> {
    /// `GlobalPalette(IdMap<T> registry)`.
    pub fn new(registry: Box<dyn GlobalIdMap<T>>) -> Self {
        GlobalPalette { registry }
    }
}

impl<T: Clone + PartialEq + Send + 'static> Palette<T> for GlobalPalette<T> {
    fn id_for(&mut self, value: &T) -> IdForResult<T> {
        let id = self.registry.get_id(value);
        if id == DEFAULT_ID {
            IdForResult::Id(0)
        } else {
            IdForResult::Id(id)
        }
    }

    fn maybe_has(&self, _predicate: &mut dyn FnMut(&T) -> bool) -> bool {
        true
    }

    fn value_for(&self, index: i32) -> T {
        match self.registry.by_id(index) {
            Some(v) => v,
            None => missing_palette_entry(index),
        }
    }

    fn read(&mut self, _buffer: &mut FriendlyByteBuf, _global_map: &dyn GlobalIdMap<T>) {}

    fn write(&self, _buffer: &mut FriendlyByteBuf, _global_map: &dyn GlobalIdMap<T>) {}

    fn get_serialized_size(&self, _global_map: &dyn GlobalIdMap<T>) -> i32 {
        0
    }

    fn get_size(&self) -> i32 {
        self.registry.size()
    }

    fn copy_palette(&self) -> Box<dyn Palette<T>> {
        Box::new(GlobalPalette {
            registry: self.registry.clone_box(),
        })
    }
}
