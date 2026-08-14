//! Port of `ca.spottedleaf.moonrise.patches.starlight.light.SWMRNibbleArray`
//! (MC 26.2) — the Single-Writer Multi-Reader nibble array at the heart of
//! Starlight's light storage.
//!
//! Java: `SWMRNibbleArray.java` in `working/Paper`. A 16×16×16 section of
//! 4-bit light levels lives in 2048 bytes (`ARRAY_SIZE`); entry
//! `(x & 15) | ((z & 15) << 4) | ((y & 15) << 8)` sits at byte `index >> 1`
//! (even index in the low nibble, odd index in the high nibble).
//!
//! ## States
//!
//! Every section is in one of four initialisation states
//! (`SWMRNibbleArray.INIT_STATE_*`, [`InitState`]):
//!
//! - `Null` — the nibble does not exist; it is always zero and is never
//!   written to directly. `getSaveState`/`toVanillaNibble` treat it as absent.
//! - `Uninitialised` — all zero, but the backing array is not allocated.
//! - `Initialised` — has real light data.
//! - `Hidden` — initialised, but conversion to Vanilla data is treated as
//!   `Null` (Starlight hides the array from downstream consumers).
//!
//! ## The dual-buffer design
//!
//! Each array carries two snapshots: `storage_updating`/`state_updating`,
//! mutated by the single writer, and `storage_visible`/`state_visible`, read
//! by consumers. Mutators run copy-on-write: the first mutation after a
//! publish clones the shared buffer (`swapUpdatingAndMarkDirty`), so the
//! visible snapshot stays immutable; `update_visible` then publishes the
//! updating snapshot atomically. `isDirty` is
//! `stateUpdating != stateVisible || updatingDirty`.
//!
//! In Java the visible fields are `volatile` and `updateVisible`/
//! `getSaveState`/`toVanillaNibble` are `synchronized` because readers may run
//! on another thread. Rivet confines light storage to the tick thread
//! (OWNERSHIP.md D5 — chunk light is merged into `ChunkMap` on the tick
//! thread, never shared), so those barriers are dropped; the updating/visible
//! split is kept as a data model because it is behaviorally observable through
//! `update_visible`, `get_save_state`, and `to_vanilla_nibble`. All access is
//! exclusive `&mut self`/`&self`.
//!
//! Java pools working byte arrays in a `ThreadLocal<ArrayDeque<byte[]>>`
//! (`allocateBytes`/`freeBytes`) to avoid garbage; that pooling is behaviorally
//! unobservable and is omitted — every "fresh" buffer here is an owned
//! `Box<[u8]>`. Java also stores the state as a raw `int`; [`InitState::Other`]
//! retains unknown persisted values so the save-state constructor matches that
//! raw representation.
//! Java's `toString()` (hex dump of the 4096 nibbles) is debug-only output and
//! is omitted; the port's [`Debug`] shows the state/snapshot shape instead.
//!
//! #184 Phase B: this is the Starlight *data surface*. The propagation engines
//! that consume these arrays (`StarLightEngine`/`BlockStarLightEngine`/
//! `SkyStarLightEngine`) and `StarLightInterface` defer with the
//! `ca.spottedleaf.moonrise.patches.starlight.light` manifest unit.

use std::fmt;

use crate::chunk::data_layer::DataLayer;

/// `SWMRNibbleArray.ARRAY_SIZE` — `16*16*16 / (8/4)` bytes holding 4096
/// 4-bit nibbles.
pub const ARRAY_SIZE: usize = 16 * 16 * 16 / (8 / 4);

/// The per-section initialisation state — `SWMRNibbleArray.INIT_STATE_*`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InitState {
    /// `INIT_STATE_NULL` — the nibble does not exist; always zero, never
    /// written directly, saved/converted as absent.
    Null,
    /// `INIT_STATE_UNINIT` — all zero with no backing array allocated.
    Uninitialised,
    /// `INIT_STATE_INIT` — has light data.
    Initialised,
    /// `INIT_STATE_HIDDEN` — initialised, but conversion to Vanilla data is
    /// treated as `Null`.
    Hidden,
    /// Any other raw state accepted by `SWMRNibbleArray(byte[], int)`.
    Other(i32),
}

impl InitState {
    /// The Java `INIT_STATE_*` int value (`getSaveState`/`SaveUtil` round-trip
    /// the state through the chunk NBT as this int).
    pub const fn to_i32(self) -> i32 {
        match self {
            Self::Null => 0,
            Self::Uninitialised => 1,
            Self::Initialised => 2,
            Self::Hidden => 3,
            Self::Other(state) => state,
        }
    }

    /// Decode a persisted Starlight state without discarding raw values
    /// accepted by Paper's `SWMRNibbleArray(byte[], int)` constructor.
    pub const fn from_i32(state: i32) -> Self {
        match state {
            0 => Self::Null,
            1 => Self::Uninitialised,
            2 => Self::Initialised,
            3 => Self::Hidden,
            _ => Self::Other(state),
        }
    }
}

/// `SWMRNibbleArray.SaveState` — the persistent form `getSaveState` returns,
/// which `SaveUtil` writes into the chunk NBT (`BLOCKLIGHT/SKYLIGHT_STATE_TAG`)
/// and later rebuilds via `new SWMRNibbleArray(bytes, state)`.
pub struct SaveState {
    /// `SaveState.data` — the packed bytes, or `None` for the uninitialised
    /// (all-zero, compressed) form.
    pub data: Option<Vec<u8>>,
    /// `SaveState.state` — the section initialisation state.
    pub state: InitState,
}

/// `ca.spottedleaf.moonrise.patches.starlight.light.SWMRNibbleArray`.
///
/// `Clone` deep-copies the CoW buffers so a caller can hand the engine a
/// snapshot of a section's current light state without aliasing the live
/// array (Java clones the byte array when a chunk's nibble is captured).
#[derive(Clone)]
pub struct SwmrNibbleArray {
    /// `stateUpdating` — the writer-visible state (plain, writer-confined).
    state_updating: InitState,
    /// `stateVisible` — the published state (Java `volatile`; dropped here).
    state_visible: InitState,
    /// `storageUpdating` — the writer's working bytes (CoW from visible).
    storage_updating: Option<Box<[u8]>>,
    /// `updatingDirty` — whether `storage_updating` diverges from visible.
    updating_dirty: bool,
    /// `storageVisible` — the published bytes (Java `volatile`; dropped here).
    storage_visible: Option<Box<[u8]>>,
}

impl SwmrNibbleArray {
    /// `SWMRNibbleArray()` — lazy init: an uninitialised section with no
    /// backing array.
    pub fn new() -> Self {
        Self::new_with_bytes_and_null(None, false)
    }

    /// `SWMRNibbleArray(byte[] bytes)` — an explicit 2048-byte section,
    /// initialised. Panics like Java's `IllegalArgumentException` on a wrong
    /// length.
    pub fn new_with_bytes(data: Vec<u8>) -> Self {
        Self::new_with_bytes_and_null(Some(data), false)
    }

    /// `SWMRNibbleArray(byte[] bytes, boolean isNullNibble)` — bytes `None`
    /// yields `Null` when `is_null_nibble` and `Uninitialised` otherwise;
    /// `Some` yields `Initialised`. Panics like Java's
    /// `IllegalArgumentException` on a wrong length.
    pub fn new_with_bytes_and_null(bytes: Option<Vec<u8>>, is_null_nibble: bool) -> Self {
        let state = match &bytes {
            None if is_null_nibble => InitState::Null,
            None => InitState::Uninitialised,
            Some(_) => InitState::Initialised,
        };
        if let Some(data) = &bytes {
            assert_eq!(
                data.len(),
                ARRAY_SIZE,
                "Data of wrong length: {}",
                data.len()
            );
        }
        let boxed = bytes.map(Vec::into_boxed_slice);
        // Both views start as the same bytes (Java shares the reference; the
        // port clones so the first CoW never aliases the visible snapshot).
        SwmrNibbleArray {
            state_updating: state,
            state_visible: state,
            storage_updating: boxed.clone(),
            updating_dirty: false,
            storage_visible: boxed,
        }
    }

    /// `SWMRNibbleArray(byte[] bytes, int state)` — the `SaveUtil`/save-state
    /// round-trip constructor. Panics like Java's `IllegalArgumentException`
    /// on a wrong length or on `bytes == None` with an initialised state
    /// (`Initialised`/`Hidden` both require backing bytes).
    pub fn new_with_state(bytes: Option<Vec<u8>>, state: InitState) -> Self {
        if let Some(data) = &bytes {
            assert_eq!(
                data.len(),
                ARRAY_SIZE,
                "Data of wrong length: {}",
                data.len()
            );
        }
        assert!(
            !(bytes.is_none() && (state == InitState::Initialised || state == InitState::Hidden)),
            "Data cannot be null and have state be initialised"
        );
        let boxed = bytes.map(Vec::into_boxed_slice);
        SwmrNibbleArray {
            state_updating: state,
            state_visible: state,
            storage_updating: boxed.clone(),
            updating_dirty: false,
            storage_visible: boxed,
        }
    }

    /// `SWMRNibbleArray.fromVanilla(DataLayer)` — build from a Vanilla light
    /// layer: `None` maps to a `Null` nibble, an empty (uniform-zero) layer to
    /// an uninitialised section, anything else to an initialised copy of the
    /// layer's bytes (never aliased back into the layer).
    pub fn from_vanilla(nibble: Option<&DataLayer>) -> Self {
        match nibble {
            None => Self::new_with_bytes_and_null(None, true),
            Some(nibble) if nibble.is_empty() => Self::new(),
            Some(nibble) => Self::new_with_bytes(nibble.get_data()),
        }
    }

    /// `SWMRNibbleArray.getSaveState()` — the persistent form, with Starlight's
    /// zero compression: an `Initialised` section whose bytes are all zero is
    /// saved as `Uninitialised` with no data; a `Hidden` section that is all
    /// zero is saved as absent (Java `null`), like `Null`. Reads the *visible*
    /// snapshot, never un-published updating work.
    pub fn get_save_state(&self) -> Option<SaveState> {
        let state = self.state_visible;
        let data = &self.storage_visible;
        match state {
            InitState::Null => None,
            InitState::Uninitialised => Some(SaveState { data: None, state }),
            InitState::Initialised | InitState::Hidden => {
                let zero = Self::is_all_zero(data.as_deref().expect("initialised has storage"));
                if zero {
                    match state {
                        InitState::Initialised => Some(SaveState {
                            data: None,
                            state: InitState::Uninitialised,
                        }),
                        _ => None,
                    }
                } else {
                    Some(SaveState {
                        data: data.as_ref().map(|d| d.to_vec()),
                        state,
                    })
                }
            }
            InitState::Other(_) => {
                let zero = Self::is_all_zero(data.as_deref().expect("unknown state has storage"));
                (!zero).then(|| SaveState {
                    data: data.as_ref().map(|d| d.to_vec()),
                    state,
                })
            }
        }
    }

    /// `SWMRNibbleArray.isAllZero(byte[])` — Java reads the array as native
    /// endian `long` words via a byte-array `VarHandle`; `ARRAY_SIZE` (2048) is
    /// divisible by 8. Bit-wise the check is endian-agnostic.
    fn is_all_zero(data: &[u8]) -> bool {
        data.chunks_exact(8)
            .all(|chunk| u64::from_ne_bytes(chunk.try_into().unwrap()) == 0)
    }

    /// `SWMRNibbleArray.extrudeLower(SWMRNibbleArray other)` — copy `other`'s
    /// `y == 0` layer (bytes 0..128) into every y-layer of `self`, so skylight
    /// propagates straight down through a homogeneous section. Reads `other`'s
    /// *updating* storage. Panics like Java's `IllegalArgumentException` when
    /// `other` is `Null`; when `other` has no storage, `self` is uninitialised
    /// instead.
    pub fn extrude_lower(&mut self, other: &Self) {
        assert!(
            other.state_updating != InitState::Null,
            "cannot extrudeLower from a null nibble"
        );
        if other.storage_updating.is_none() {
            self.set_uninitialised();
            return;
        }

        let src = other.storage_updating.as_deref().unwrap();
        if !self.updating_dirty {
            // Fresh buffer (never the shared-with-visible one); overwritten
            // wholesale below, so no need to preserve prior content.
            if self.storage_updating.is_none() {
                self.state_updating = InitState::Initialised;
            }
            self.storage_updating = Some(vec![0u8; ARRAY_SIZE].into_boxed_slice());
            self.updating_dirty = true;
        }
        let into = self.storage_updating.as_deref_mut().unwrap();

        let start = 0;
        // `(15 | (15 << 4)) >>> 1` — the y=0 xz-plane occupies bytes 0..=127.
        let end = (15 | (15 << 4)) >> 1;
        /* x | (z << 4) | (y << 8) */
        for y in 0..16 {
            let dest = (y << (8 - 1)) as usize;
            into[dest..dest + (end - start + 1)].copy_from_slice(&src[start..(end - start + 1)]);
        }
    }

    /// `SWMRNibbleArray.setFull()` — fill the updating buffer with `0xFF`
    /// (every level 15); the state becomes `Initialised` unless already
    /// `Hidden`.
    pub fn set_full(&mut self) {
        if self.state_updating != InitState::Hidden {
            self.state_updating = InitState::Initialised;
        }
        if self.storage_updating.is_none() || !self.updating_dirty {
            self.storage_updating = Some(vec![0xFF; ARRAY_SIZE].into_boxed_slice());
        } else {
            self.storage_updating.as_deref_mut().unwrap().fill(0xFF);
        }
        self.updating_dirty = true;
    }

    /// `SWMRNibbleArray.setZero()` — fill the updating buffer with `0x00`; the
    /// state becomes `Initialised` unless already `Hidden`.
    pub fn set_zero(&mut self) {
        if self.state_updating != InitState::Hidden {
            self.state_updating = InitState::Initialised;
        }
        if self.storage_updating.is_none() || !self.updating_dirty {
            self.storage_updating = Some(vec![0u8; ARRAY_SIZE].into_boxed_slice());
        } else {
            self.storage_updating.as_deref_mut().unwrap().fill(0x00);
        }
        self.updating_dirty = true;
    }

    /// `SWMRNibbleArray.setNonNull()` — `Hidden` → `Initialised`, `Null` →
    /// `Uninitialised`, otherwise a no-op.
    pub fn set_non_null(&mut self) {
        if self.state_updating == InitState::Hidden {
            self.state_updating = InitState::Initialised;
            return;
        }
        if self.state_updating != InitState::Null {
            return;
        }
        self.state_updating = InitState::Uninitialised;
    }

    /// `SWMRNibbleArray.setNull()` — drop the updating state and storage and
    /// clear `updating_dirty`. The section stays dirty until published (the
    /// updating state now differs from the visible one).
    pub fn set_null(&mut self) {
        self.state_updating = InitState::Null;
        self.storage_updating = None;
        self.updating_dirty = false;
    }

    /// `SWMRNibbleArray.setUninitialised()` — drop the updating storage and
    /// become `Uninitialised`; like `setNull` this leaves the section dirty
    /// until `update_visible` publishes the new state.
    pub fn set_uninitialised(&mut self) {
        self.state_updating = InitState::Uninitialised;
        self.storage_updating = None;
        self.updating_dirty = false;
    }

    /// `SWMRNibbleArray.setHidden()` — `Hidden` is sticky; from `Initialised`
    /// it hides the section while preserving the bytes, from any other state it
    /// drops to `Null`.
    pub fn set_hidden(&mut self) {
        if self.state_updating == InitState::Hidden {
            return;
        }
        if self.state_updating != InitState::Initialised {
            self.set_null();
        } else {
            self.state_updating = InitState::Hidden;
        }
    }

    /// `SWMRNibbleArray.isDirty()` — the updating snapshot diverges from the
    /// visible one.
    pub fn is_dirty(&self) -> bool {
        self.state_updating != self.state_visible || self.updating_dirty
    }

    /// `SWMRNibbleArray.isNullNibbleUpdating()`.
    pub fn is_null_nibble_updating(&self) -> bool {
        self.state_updating == InitState::Null
    }

    /// `SWMRNibbleArray.isNullNibbleVisible()`.
    pub fn is_null_nibble_visible(&self) -> bool {
        self.state_visible == InitState::Null
    }

    /// `SWMRNibbleArray.isUninitialisedUpdating()`.
    pub fn is_uninitialised_updating(&self) -> bool {
        self.state_updating == InitState::Uninitialised
    }

    /// `SWMRNibbleArray.isUninitialisedVisible()`.
    pub fn is_uninitialised_visible(&self) -> bool {
        self.state_visible == InitState::Uninitialised
    }

    /// `SWMRNibbleArray.isInitialisedUpdating()`.
    pub fn is_initialised_updating(&self) -> bool {
        self.state_updating == InitState::Initialised
    }

    /// `SWMRNibbleArray.isInitialisedVisible()`.
    pub fn is_initialised_visible(&self) -> bool {
        self.state_visible == InitState::Initialised
    }

    /// Whether the *visible* state is an unsupported persisted value (`Other`).
    /// `to_vanilla_nibble` panics on that state (Java would carry it through
    /// the same `toVanillaNibble` and keep the raw int), so a caller that
    /// converts a reconstructed chunk into packet form must reject it first —
    /// `toVanillaNibble`'s panic is not a typed error surface.
    pub fn has_unknown_state_visible(&self) -> bool {
        matches!(self.state_visible, InitState::Other(_))
    }

    /// `SWMRNibbleArray.isHiddenUpdating()`.
    pub fn is_hidden_updating(&self) -> bool {
        self.state_updating == InitState::Hidden
    }

    /// `SWMRNibbleArray.isHiddenVisible()`.
    pub fn is_hidden_visible(&self) -> bool {
        self.state_visible == InitState::Hidden
    }

    /// `SWMRNibbleArray.swapUpdatingAndMarkDirty()` — the copy-on-write
    /// trigger: clone the current updating buffer (or allocate a zeroed one)
    /// so the writer can diverge without touching the published snapshot; the
    /// state becomes `Initialised` unless already `Hidden`.
    fn swap_updating_and_mark_dirty(&mut self) {
        if self.updating_dirty {
            return;
        }
        if self.storage_updating.is_some() {
            self.storage_updating = self.storage_updating.clone();
        } else {
            self.storage_updating = Some(vec![0u8; ARRAY_SIZE].into_boxed_slice());
        }
        if self.state_updating != InitState::Hidden {
            self.state_updating = InitState::Initialised;
        }
        self.updating_dirty = true;
    }

    /// `SWMRNibbleArray.updateVisible()` — publish the updating snapshot to the
    /// visible one; returns `false` (Java's early return) when not dirty. A
    /// `Null`/`Uninitialised` updating state drops the visible storage; an
    /// initialised one copies the updating bytes into the existing visible
    /// buffer (preserving its identity in Java; the port copies content). After
    /// the publish the updating buffer's content equals the visible snapshot —
    /// Java re-aliases the arrays, while the port keeps its `Box`es distinct
    /// and the next write CoWs `storage_updating` regardless.
    pub fn update_visible(&mut self) -> bool {
        if !self.is_dirty() {
            return false;
        }
        match self.state_updating {
            InitState::Null | InitState::Uninitialised => {
                self.storage_visible = None;
            }
            InitState::Initialised | InitState::Hidden | InitState::Other(_) => {
                if self.storage_visible.is_none() {
                    self.storage_visible = self.storage_updating.clone();
                } else if self.storage_updating != self.storage_visible {
                    let src = self
                        .storage_updating
                        .as_deref()
                        .expect("initialised has updating storage");
                    self.storage_visible
                        .as_deref_mut()
                        .unwrap()
                        .copy_from_slice(src);
                }
            }
        }
        self.updating_dirty = false;
        self.state_visible = self.state_updating;
        true
    }

    /// `SWMRNibbleArray.toVanillaNibble()` — convert the *visible* state to a
    /// Vanilla `DataLayer`: `Null`/`Hidden` → `None` (Java `null`),
    /// `Uninitialised` → an empty layer, `Initialised` → a copy of the bytes.
    pub fn to_vanilla_nibble(&self) -> Option<DataLayer> {
        match self.state_visible {
            InitState::Hidden | InitState::Null => None,
            InitState::Uninitialised => Some(DataLayer::new(0)),
            InitState::Initialised => Some(DataLayer::with_data(
                self.storage_visible
                    .as_deref()
                    .expect("initialised has storage")
                    .to_vec(),
            )),
            InitState::Other(_) => panic!("unknown Starlight light state"),
        }
    }

    /// `SWMRNibbleArray.getUpdating(x, y, z)` — the writer-side light at the
    /// masked local coordinate (0 when the updating storage is absent).
    pub fn get_updating(&self, x: i32, y: i32, z: i32) -> i32 {
        self.get_updating_index(((x & 15) | ((z & 15) << 4) | ((y & 15) << 8)) as usize)
    }

    /// `SWMRNibbleArray.getUpdating(int index)` — writer-side light at the
    /// raw `x | (z << 4) | (y << 8)` index.
    pub fn get_updating_index(&self, index: usize) -> i32 {
        match &self.storage_updating {
            None => 0,
            Some(bytes) => nibble_at(bytes, index),
        }
    }

    /// `SWMRNibbleArray.getVisible(x, y, z)` — the published light at the
    /// masked local coordinate (0 when the visible storage is absent).
    pub fn get_visible(&self, x: i32, y: i32, z: i32) -> i32 {
        self.get_visible_index(((x & 15) | ((z & 15) << 4) | ((y & 15) << 8)) as usize)
    }

    /// `SWMRNibbleArray.getVisible(int index)` — published light at the raw
    /// index.
    pub fn get_visible_index(&self, index: usize) -> i32 {
        match &self.storage_visible {
            None => 0,
            Some(bytes) => nibble_at(bytes, index),
        }
    }

    /// `SWMRNibbleArray.set(x, y, z, value)` — write the writer-side light at
    /// the masked local coordinate, forcing copy-on-write first.
    pub fn set(&mut self, x: i32, y: i32, z: i32, value: i32) {
        self.set_index(
            ((x & 15) | ((z & 15) << 4) | ((y & 15) << 8)) as usize,
            value,
        );
    }

    /// `SWMRNibbleArray.set(int index, int value)` — write the writer-side
    /// light at the raw index, forcing copy-on-write first.
    pub fn set_index(&mut self, index: usize, value: i32) {
        if !self.updating_dirty {
            self.swap_updating_and_mark_dirty();
        }
        let shift = (index & 1) << 2;
        let i = index >> 1;
        let bytes = self.storage_updating.as_deref_mut().expect("dirty storage");
        bytes[i] = (bytes[i] & (0xF0u8 >> shift)) | (value.wrapping_shl(shift as u32) as u8);
    }
}

impl Default for SwmrNibbleArray {
    /// `SWMRNibbleArray()`.
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SwmrNibbleArray {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SwmrNibbleArray")
            .field("state_updating", &self.state_updating)
            .field("state_visible", &self.state_visible)
            .field("updating_dirty", &self.updating_dirty)
            .field("has_updating_storage", &self.storage_updating.is_some())
            .field("has_visible_storage", &self.storage_visible.is_some())
            .finish()
    }
}

/// Read the 4-bit value at a raw `x | (z << 4) | (y << 8)` index: byte
/// `index >> 1`, low nibble for even index, high nibble for odd index.
fn nibble_at(bytes: &[u8], index: usize) -> i32 {
    let value = bytes[index >> 1];
    ((value >> ((index & 1) << 2)) & 0xF) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `getIndex(x, y, z) = y << 8 | z << 4 | x`; byte `index >> 1`; even x in
    /// the low nibble, odd x in the high nibble. y advances a 128-byte plane,
    /// z a 16-byte column. Mirrors `DataLayer`'s layout test.
    #[test]
    fn layout_is_y_then_z_then_x_with_nibble_packing() {
        let mut array = SwmrNibbleArray::new();
        array.set(0, 0, 0, 5); // index 0 -> byte 0 low nibble
        array.set(1, 0, 0, 10); // index 1 -> byte 0 high nibble
        array.set(0, 0, 1, 3); // index 16 -> byte 8 low nibble
        array.set(0, 1, 0, 7); // index 256 -> byte 128 low nibble
        assert_eq!(array.get_updating(0, 0, 0), 5);
        assert_eq!(array.get_updating(1, 0, 0), 10);
        assert_eq!(array.get_updating(0, 0, 1), 3);
        assert_eq!(array.get_updating(0, 1, 0), 7);
        array.update_visible();
        let data = array.to_vanilla_nibble().unwrap().get_data();
        assert_eq!(data[0], 0xA5);
        assert_eq!(data[8], 0x03);
        assert_eq!(data[128], 0x07);
    }

    /// `(x & 15) | ((z & 15) << 4) | ((y & 15) << 8)` — coordinates outside
    /// 0..15 wrap, including negatives (`-1 & 15 == 15` in two's complement).
    #[test]
    fn coordinate_masking_wraps_negative_and_large_coordinates() {
        let mut array = SwmrNibbleArray::new();
        array.set(16, 0, 0, 8); // 16 & 15 = 0 -> byte 0 low nibble
        array.set(-1, 0, 0, 9); // -1 & 15 = 15 (odd) -> byte 7 high nibble
        array.set(0, -1, 0, 7); // y 15 -> byte 1920 low nibble
        array.set(0, 0, -1, 6); // z 15 -> index 240 -> byte 120 low nibble
        assert_eq!(array.get_updating(0, 0, 0), 8);
        assert_eq!(array.get_updating(15, 0, 0), 9);
        assert_eq!(array.get_updating(0, 15, 0), 7);
        assert_eq!(array.get_updating(0, 0, 15), 6);
        array.update_visible();
        let data = array.to_vanilla_nibble().unwrap().get_data();
        assert_eq!(data[0] & 0x0F, 8);
        assert_eq!((data[7] >> 4) & 0x0F, 9); // odd index 15 -> high nibble
        assert_eq!(data[1920] & 0x0F, 7);
        assert_eq!(data[120] & 0x0F, 6);
    }

    /// `getUpdating`/`getVisible` on an absent buffer return 0 (Java's null
    /// storage), never panic.
    #[test]
    fn reads_on_absent_storage_return_zero() {
        let array = SwmrNibbleArray::new(); // UNINIT, no storage
        assert_eq!(array.get_updating(0, 0, 0), 0);
        assert_eq!(array.get_updating_index(4095), 0);
        assert_eq!(array.get_visible(5, 5, 5), 0);
        assert_eq!(array.get_visible_index(2048), 0);
    }

    /// The byte cast truncates out-of-range values to the low 8 bits, so only
    /// the low nibble survives: `set(..., 16)` writes 0, `set(..., 0x1F)` on an
    /// odd index writes 15.
    #[test]
    fn set_truncates_out_of_range_values_like_byte_cast() {
        let mut array = SwmrNibbleArray::new();
        array.set(0, 0, 0, 16); // low nibble: 16 & 0xF == 0
        assert_eq!(array.get_updating(0, 0, 0), 0);
        array.set(1, 0, 0, 0x1F); // high nibble: 0x1F0 as byte = 0xF0
        assert_eq!(array.get_updating(1, 0, 0), 15);
    }

    /// `SWMRNibbleArray()` is an uninitialised, clean section with no storage.
    #[test]
    fn default_constructor_is_uninitialised() {
        let array = SwmrNibbleArray::new();
        assert!(array.is_uninitialised_updating());
        assert!(array.is_uninitialised_visible());
        assert!(!array.is_initialised_updating());
        assert!(!array.is_dirty());
        assert_eq!(array.get_updating_index(0), 0);
    }

    /// `SWMRNibbleArray(byte[])` is an initialised section sharing the bytes
    /// across both views.
    #[test]
    fn bytes_constructor_is_initialised() {
        let array = SwmrNibbleArray::new_with_bytes(vec![0xAB; ARRAY_SIZE]);
        assert!(array.is_initialised_updating());
        assert!(array.is_initialised_visible());
        assert!(!array.is_dirty());
        assert_eq!(array.get_updating_index(0), 0xB);
        assert_eq!(array.get_visible_index(0), 0xB);
    }

    /// Wrong-length bytes panic like Java's `IllegalArgumentException`.
    #[test]
    fn wrong_length_constructors_panic() {
        let caught = std::panic::catch_unwind(|| SwmrNibbleArray::new_with_bytes(vec![0; 100]));
        assert!(caught.is_err());
        let caught = std::panic::catch_unwind(|| {
            SwmrNibbleArray::new_with_bytes_and_null(Some(vec![0; 10]), true)
        });
        assert!(caught.is_err());
        let caught = std::panic::catch_unwind(|| {
            SwmrNibbleArray::new_with_state(Some(vec![0; 10]), InitState::Initialised)
        });
        assert!(caught.is_err());
    }

    /// `SWMRNibbleArray(byte[], boolean)`: `null`+`true` is `Null`, `null`+
    /// `false` is `Uninitialised`, non-null is `Initialised` regardless.
    #[test]
    fn null_nibble_constructor_states() {
        assert!(SwmrNibbleArray::new_with_bytes_and_null(None, true).is_null_nibble_updating());
        assert!(SwmrNibbleArray::new_with_bytes_and_null(None, false).is_uninitialised_updating());
        assert!(
            SwmrNibbleArray::new_with_bytes_and_null(Some(vec![1; ARRAY_SIZE]), true)
                .is_initialised_updating()
        );
    }

    /// `SWMRNibbleArray(byte[], int)` invariant: null bytes with an initialised
    /// state (`Initialised`/`Hidden`) is rejected.
    #[test]
    fn state_constructor_invariants_panic() {
        let caught = std::panic::catch_unwind(|| {
            SwmrNibbleArray::new_with_state(None, InitState::Initialised)
        });
        assert!(caught.is_err());
        let caught =
            std::panic::catch_unwind(|| SwmrNibbleArray::new_with_state(None, InitState::Hidden));
        assert!(caught.is_err());
        // Null / Uninitialised and raw states with no bytes are fine, as is
        // any state with bytes.
        assert!(SwmrNibbleArray::new_with_state(None, InitState::Null).is_null_nibble_updating());
        assert!(
            SwmrNibbleArray::new_with_state(None, InitState::Uninitialised)
                .is_uninitialised_updating()
        );
        assert!(
            SwmrNibbleArray::new_with_state(Some(vec![2; ARRAY_SIZE]), InitState::Hidden)
                .is_hidden_updating()
        );
        let raw = SwmrNibbleArray::new_with_state(None, InitState::Other(4));
        assert!(!raw.is_null_nibble_updating());
        assert!(!raw.is_uninitialised_updating());
    }

    /// `InitState.to_i32()` mirrors the Java `INIT_STATE_*` constants.
    #[test]
    fn init_state_java_values() {
        assert_eq!(InitState::Null.to_i32(), 0);
        assert_eq!(InitState::Uninitialised.to_i32(), 1);
        assert_eq!(InitState::Initialised.to_i32(), 2);
        assert_eq!(InitState::Hidden.to_i32(), 3);
        assert_eq!(InitState::from_i32(4), InitState::Other(4));
        assert_eq!(InitState::Other(99).to_i32(), 99);
    }

    /// `setNull` drops updating state and storage and clears updating dirtiness;
    /// the section stays dirty until the null state is published (Java: the
    /// setter touches only `stateUpdating`, so `isDirty()` reads
    /// `stateUpdating != stateVisible`).
    #[test]
    fn set_null_resets_state_and_storage() {
        let mut array = SwmrNibbleArray::new_with_bytes(vec![0x11; ARRAY_SIZE]);
        array.set(0, 0, 0, 7); // dirty
        array.set_null();
        assert!(array.is_null_nibble_updating());
        assert!(array.is_dirty()); // NULL updating != INIT visible
        assert_eq!(array.get_updating_index(0), 0);
        array.update_visible();
        assert!(!array.is_dirty());
        assert!(array.is_null_nibble_visible());
        assert_eq!(array.get_visible_index(0), 0);
    }

    /// `setNonNull`: Hidden -> Initialised, Null -> Uninitialised, already
    /// initialised is a no-op (storage and state preserved).
    #[test]
    fn set_non_null_transitions() {
        let mut null_array = SwmrNibbleArray::new_with_bytes_and_null(None, true);
        null_array.set_non_null();
        assert!(null_array.is_uninitialised_updating());

        let mut init = SwmrNibbleArray::new_with_bytes(vec![0x33; ARRAY_SIZE]);
        init.set_non_null();
        assert!(init.is_initialised_updating());
        assert_eq!(init.get_updating_index(0), 3);

        let mut hidden = SwmrNibbleArray::new_with_bytes(vec![0x44; ARRAY_SIZE]);
        hidden.set_hidden();
        hidden.set_non_null();
        assert!(hidden.is_initialised_updating());
        assert_eq!(hidden.get_updating_index(0), 4); // bytes preserved
    }

    /// `setHidden`: sticky; from Initialised it hides while preserving bytes,
    /// from Null/Uninitialised it drops to Null.
    #[test]
    fn set_hidden_transitions() {
        let mut init = SwmrNibbleArray::new_with_bytes(vec![0x55; ARRAY_SIZE]);
        init.set_hidden();
        assert!(init.is_hidden_updating());
        assert_eq!(init.get_updating_index(0), 5); // storage preserved

        let mut from_null = SwmrNibbleArray::new_with_bytes_and_null(None, true);
        from_null.set_hidden();
        assert!(from_null.is_null_nibble_updating());

        let mut from_uninit = SwmrNibbleArray::new();
        from_uninit.set_hidden();
        assert!(from_uninit.is_null_nibble_updating());

        init.set_hidden(); // already hidden: no-op
        assert!(init.is_hidden_updating());
    }

    /// `setFull`/`setZero` fill every byte and make the state `Initialised`.
    #[test]
    fn set_full_and_zero_fill() {
        let mut full = SwmrNibbleArray::new();
        full.set_full();
        assert!(full.is_initialised_updating());
        assert!(full.is_dirty());
        assert_eq!(full.get_updating_index(0), 15);
        assert_eq!(full.get_updating_index(4095), 15);

        let mut zero = SwmrNibbleArray::new();
        zero.set_zero();
        assert!(zero.is_initialised_updating());
        assert_eq!(zero.get_updating_index(0), 0);
        assert_eq!(zero.get_updating_index(4095), 0);
    }

    /// `setFull` from `Hidden` keeps the hidden state but still fills.
    #[test]
    fn set_full_keeps_hidden_state() {
        let mut hidden = SwmrNibbleArray::new_with_bytes(vec![0; ARRAY_SIZE]);
        hidden.set_hidden();
        hidden.set_full();
        assert!(hidden.is_hidden_updating());
        assert_eq!(hidden.get_updating_index(0), 15);
    }

    /// The updating/visible predicates track each snapshot independently
    /// through a set + publish cycle.
    #[test]
    fn predicates_reflect_updating_and_visible_independently() {
        let mut array = SwmrNibbleArray::new();
        assert!(array.is_uninitialised_updating() && array.is_uninitialised_visible());
        array.set(0, 0, 0, 1);
        assert!(array.is_initialised_updating());
        assert!(array.is_uninitialised_visible());
        assert!(!array.is_initialised_visible());
        array.update_visible();
        assert!(array.is_initialised_updating() && array.is_initialised_visible());
        assert!(!array.is_dirty());
    }

    /// `getSaveState` on every state, including the zero compression:
    /// Initialised-all-zero saves as Uninitialised with no data, and a
    /// Hidden-all-zero section saves as absent (Java null).
    #[test]
    fn save_matrix_all_states() {
        // NULL -> absent
        let null = SwmrNibbleArray::new_with_bytes_and_null(None, true);
        assert!(null.get_save_state().is_none());

        // UNINIT -> absent data, UNINIT state
        let uninit = SwmrNibbleArray::new();
        let save = uninit.get_save_state().unwrap();
        assert_eq!(save.state, InitState::Uninitialised);
        assert!(save.data.is_none());

        // INIT all-zero -> zero-compressed to UNINIT with no data
        let zero_init = SwmrNibbleArray::new_with_bytes(vec![0; ARRAY_SIZE]);
        let save = zero_init.get_save_state().unwrap();
        assert_eq!(save.state, InitState::Uninitialised);
        assert!(save.data.is_none());

        // INIT non-zero -> data + INIT state
        let bytes = vec![0xAB; ARRAY_SIZE];
        let init = SwmrNibbleArray::new_with_bytes(bytes.clone());
        let save = init.get_save_state().unwrap();
        assert_eq!(save.state, InitState::Initialised);
        assert_eq!(save.data.as_deref(), Some(bytes.as_slice()));

        // HIDDEN all-zero -> absent (treated like NULL on save). `setHidden`
        // only touches the updating state, so publish it first (Java reads the
        // visible snapshot here).
        let mut hidden_zero = SwmrNibbleArray::new_with_bytes(vec![0; ARRAY_SIZE]);
        hidden_zero.set_hidden();
        assert!(hidden_zero.is_dirty()); // HIDDEN updating != INIT visible
        hidden_zero.update_visible();
        assert!(hidden_zero.is_hidden_visible());
        assert!(hidden_zero.get_save_state().is_none());

        // HIDDEN non-zero -> data + HIDDEN state
        let mut hidden = SwmrNibbleArray::new_with_bytes(vec![0xCD; ARRAY_SIZE]);
        hidden.set_hidden();
        hidden.update_visible();
        let save = hidden.get_save_state().unwrap();
        assert_eq!(save.state, InitState::Hidden);
        assert_eq!(
            save.data.as_deref(),
            Some(vec![0xCD; ARRAY_SIZE].as_slice())
        );
    }

    /// `getSaveState` reads the *visible* snapshot — un-published updating work
    /// must not leak into the save.
    #[test]
    fn save_uses_visible_not_updating() {
        let mut array = SwmrNibbleArray::new_with_bytes(vec![0x11; ARRAY_SIZE]);
        array.update_visible();
        array.set(0, 0, 0, 7); // dirties updating only
        let save = array.get_save_state().unwrap();
        assert_eq!(save.state, InitState::Initialised);
        assert_eq!(save.data.as_ref().unwrap()[0], 0x11); // visible unchanged
    }

    /// `new SWMRNibbleArray(bytes, state)` rebuilds a section identical to the
    /// one `getSaveState` captured.
    #[test]
    fn save_state_round_trips_via_new_with_state() {
        let original = SwmrNibbleArray::new_with_bytes(vec![0x33; ARRAY_SIZE]);
        let save = original.get_save_state().unwrap();
        let rebuilt = SwmrNibbleArray::new_with_state(save.data, save.state);
        assert!(rebuilt.is_initialised_updating());
        assert_eq!(rebuilt.get_updating_index(0), 3);
        assert_eq!(rebuilt.get_updating_index(4095), 3);

        let empty = SwmrNibbleArray::new();
        let save = empty.get_save_state().unwrap();
        let rebuilt = SwmrNibbleArray::new_with_state(save.data, save.state);
        assert!(rebuilt.is_uninitialised_updating());
    }

    /// `updateVisible` is a no-op (returns false) on a clean section.
    #[test]
    fn update_visible_returns_false_when_clean() {
        let mut array = SwmrNibbleArray::new();
        assert!(!array.update_visible());
        let mut array = SwmrNibbleArray::new_with_bytes(vec![0x11; ARRAY_SIZE]);
        assert!(!array.update_visible());
    }

    /// `updateVisible` publishes state, storage, and clears the dirty flag.
    #[test]
    fn update_visible_publishes_state_and_storage() {
        let mut array = SwmrNibbleArray::new();
        array.set(0, 0, 0, 9);
        assert!(array.is_dirty());
        assert!(array.update_visible());
        assert!(!array.is_dirty());
        assert!(array.is_initialised_visible());
        assert_eq!(array.get_visible(0, 0, 0), 9);
        assert!(!array.update_visible());
    }

    /// Publishing a `Null`/`Uninitialised` updating state drops the visible
    /// storage.
    #[test]
    fn update_visible_null_and_uninit_drop_visible_storage() {
        let mut array = SwmrNibbleArray::new_with_bytes(vec![0x22; ARRAY_SIZE]);
        array.update_visible();
        assert!(array.is_initialised_visible());
        array.set_null();
        assert!(array.update_visible());
        assert!(array.is_null_nibble_visible());
        assert_eq!(array.get_visible(0, 0, 0), 0);
        assert!(array.to_vanilla_nibble().is_none());
    }

    /// Copy-on-write: after a publish, the visible snapshot stays frozen even
    /// as the writer mutates the updating buffer.
    #[test]
    fn copy_on_write_isolates_visible_snapshot() {
        let mut array = SwmrNibbleArray::new();
        array.set(0, 0, 0, 1);
        array.update_visible();
        assert_eq!(array.get_visible(0, 0, 0), 1);
        array.set(0, 0, 0, 2);
        assert_eq!(array.get_updating(0, 0, 0), 2);
        assert_eq!(array.get_visible(0, 0, 0), 1); // not yet published
        array.update_visible();
        assert_eq!(array.get_visible(0, 0, 0), 2);
    }

    /// `extrudeLower` copies `other`'s y=0 xz-plane into every y-layer of
    /// `self`.
    #[test]
    fn extrude_lower_replicates_bottom_layer() {
        let mut above = SwmrNibbleArray::new();
        above.set(0, 0, 0, 1);
        above.set(1, 0, 0, 2);
        above.set(0, 0, 1, 3);
        let mut below = SwmrNibbleArray::new();
        below.extrude_lower(&above);
        assert!(below.is_initialised_updating());
        assert!(below.is_dirty());
        below.update_visible();
        let data = below.to_vanilla_nibble().unwrap().get_data();
        for y in 0..16usize {
            let plane = y << 7;
            assert_eq!(data[plane], 0x21); // x=0 low 1, x=1 high 2
            assert_eq!(data[plane + 8], 0x03); // z=1
        }
    }

    /// `extrudeLower` from a `Null` other panics like Java's
    /// `IllegalArgumentException`.
    #[test]
    fn extrude_lower_from_null_panics() {
        let null = SwmrNibbleArray::new_with_bytes_and_null(None, true);
        let mut target = SwmrNibbleArray::new();
        let caught =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| target.extrude_lower(&null)));
        assert!(caught.is_err());
    }

    /// `extrudeLower` from a `Null`/`Uninitialised` other with no storage
    /// uninitialises `self` instead of writing.
    #[test]
    fn extrude_lower_from_uninitialised_uninitialises_self() {
        let uninit = SwmrNibbleArray::new();
        let mut target = SwmrNibbleArray::new_with_bytes(vec![0x11; ARRAY_SIZE]);
        target.extrude_lower(&uninit);
        assert!(target.is_uninitialised_updating());
        assert!(target.is_dirty()); // updating != visible now
        assert_eq!(target.get_updating_index(0), 0);
    }

    /// `extrudeLower` on an already-dirty section fills in place and keeps the
    /// section initialised.
    #[test]
    fn extrude_lower_on_dirty_reuses_storage() {
        let mut above = SwmrNibbleArray::new();
        above.set(0, 0, 0, 9);
        let mut target = SwmrNibbleArray::new();
        target.set(3, 3, 3, 5); // dirty the updating buffer
        target.extrude_lower(&above);
        assert!(target.is_initialised_updating());
        assert!(target.is_dirty());
        assert_eq!(target.get_updating(0, 0, 0), 9);
        assert_eq!(target.get_updating(0, 1, 0), 9);
        assert_eq!(target.get_updating(0, 15, 0), 9);
    }

    /// `extrudeLower` never mutates `other` (reads only).
    #[test]
    fn extrude_lower_does_not_mutate_other() {
        let mut above = SwmrNibbleArray::new();
        above.set(0, 0, 0, 6);
        above.update_visible();
        let mut below = SwmrNibbleArray::new();
        below.extrude_lower(&above);
        assert_eq!(above.get_updating(0, 0, 0), 6);
        assert!(above.is_initialised_visible());
        assert!(!above.is_dirty());
        assert_eq!(below.get_updating(0, 0, 0), 6);
    }

    /// `fromVanilla(null)` yields a `Null` nibble.
    #[test]
    fn from_vanilla_null_data_layer() {
        let array = SwmrNibbleArray::from_vanilla(None);
        assert!(array.is_null_nibble_updating());
    }

    /// `fromVanilla`: an empty layer is uninitialised, a filled layer is an
    /// initialised copy of the layer's bytes.
    #[test]
    fn from_vanilla_empty_and_filled() {
        let empty = DataLayer::new(0);
        let array = SwmrNibbleArray::from_vanilla(Some(&empty));
        assert!(array.is_uninitialised_updating());

        let mut filled = DataLayer::new(0);
        filled.set(2, 3, 4, 7);
        let array = SwmrNibbleArray::from_vanilla(Some(&filled));
        assert!(array.is_initialised_updating());
        assert_eq!(array.get_updating(2, 3, 4), 7);
        assert_eq!(array.get_visible(2, 3, 4), 7);
    }

    /// `toVanillaNibble` per state: `Null`/`Hidden` -> `None`, `Uninitialised`
    /// -> an empty layer, `Initialised` -> the bytes.
    #[test]
    fn to_vanilla_nibble_all_states() {
        let null = SwmrNibbleArray::new_with_bytes_and_null(None, true);
        assert!(null.to_vanilla_nibble().is_none());

        let mut hidden = SwmrNibbleArray::new_with_bytes(vec![0x11; ARRAY_SIZE]);
        hidden.set_hidden();
        hidden.update_visible(); // publish the HIDDEN updating state
        assert!(hidden.is_hidden_visible());
        assert!(hidden.to_vanilla_nibble().is_none());

        let uninit = SwmrNibbleArray::new();
        assert!(uninit.to_vanilla_nibble().unwrap().is_empty());

        let init = SwmrNibbleArray::new_with_bytes(vec![0xAB; ARRAY_SIZE]);
        assert_eq!(
            init.to_vanilla_nibble().unwrap().get_data(),
            vec![0xAB; ARRAY_SIZE]
        );
    }

    /// `fromVanilla` then `toVanillaNibble` round-trips the light layer
    /// (semantic equality through `DataLayer`).
    #[test]
    fn from_vanilla_to_vanilla_round_trip() {
        let mut layer = DataLayer::new(0);
        layer.set(1, 2, 3, 14);
        layer.set(15, 15, 15, 1);
        let array = SwmrNibbleArray::from_vanilla(Some(&layer));
        let back = array.to_vanilla_nibble().unwrap();
        assert_eq!(back, layer);
        assert_eq!(back.get(1, 2, 3), 14);
        assert_eq!(back.get(15, 15, 15), 1);
    }

    /// `set` dirties the section even when the written nibble is unchanged
    /// (Java sets `updatingDirty` unconditionally).
    #[test]
    fn set_marks_dirty_even_for_same_value() {
        let mut array = SwmrNibbleArray::new();
        array.set(0, 0, 0, 5);
        array.update_visible();
        assert!(!array.is_dirty());
        array.set(0, 0, 0, 5);
        assert!(array.is_dirty());
    }

    /// A hostile sequence: hiding then re-vealing via `setNonNull` preserves
    /// the storage, and the whole lifecycle round-trips through save/publish.
    #[test]
    fn hostile_hidden_round_trip_and_republish() {
        let mut array = SwmrNibbleArray::new_with_bytes(vec![0x11; ARRAY_SIZE]);
        array.set_hidden();
        array.update_visible();
        assert!(array.is_hidden_visible());
        assert!(array.to_vanilla_nibble().is_none()); // hidden never converts
        let save = array.get_save_state().unwrap();
        assert_eq!(save.state, InitState::Hidden);
        assert_eq!(
            save.data.as_deref(),
            Some(vec![0x11; ARRAY_SIZE].as_slice())
        );

        let mut rebuilt = SwmrNibbleArray::new_with_state(save.data, save.state);
        assert!(rebuilt.is_hidden_updating());
        rebuilt.set_non_null(); // reveal
        assert!(rebuilt.is_initialised_updating());
        assert_eq!(rebuilt.get_updating_index(0), 1);
        rebuilt.update_visible();
        assert_eq!(
            rebuilt.to_vanilla_nibble().unwrap().get_data(),
            vec![0x11; ARRAY_SIZE]
        );
    }
}
