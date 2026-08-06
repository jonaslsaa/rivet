//! Port of `net.minecraft.util.BitStorage` (MC 26.2).
//!
//! PROVENANCE: `net/minecraft/util/BitStorage.java` in `working/Paper`
//! (vanilla 26.2 + Paper patches). The Paper `BlockCountingBitStorage`
//! (Moonrise) extension is **not** ported — block counting is M2 scope
//! (see #108).
//!
//! The wire format depends on this interface's exact packed-long layout:
//! `PalettedContainer`/`Data.write` emits `getBits()` (one byte) followed by
//! `getRaw()` (the packed `long[]`, big-endian on the wire).

/// `net.minecraft.util.BitStorage` — a packed fixed-width entry array.
///
/// Entries are `getBits()` bits wide, addressed by linear index. The concrete
/// layouts mirror Java exactly:
///
/// - [`SimpleBitStorage`]: entries packed into `ceil(size * bits / 64)`
///   `u64`s. Entry `i` lives in cell `i / valuesPerLong` at bit offset
///   `(i % valuesPerLong) * bits`, where `valuesPerLong = 64 / bits`. This is
///   the layout produced by Java's constructor (`value[cell*vpl + k]` in the
///   `k`-th `bits`-wide slot, slot 0 in the low bits).
/// - [`ZeroBitStorage`]: zero-width entries, no backing storage.
pub trait BitStorage {
    /// `getAndSet(int index, int value)` — writes `value`, returns the prior
    /// entry. Java arithmetic on `int`/`long` is wrapping; the bit masks here
    /// are applied on `u64` with logical shifts.
    fn get_and_set(&mut self, index: usize, value: i32) -> i32;

    /// `set(int index, int value)`.
    fn set(&mut self, index: usize, value: i32);

    /// `get(int index)`.
    fn get(&self, index: usize) -> i32;

    /// `getRaw()` — the packed backing array.
    fn get_raw(&self) -> &[i64];

    /// `getRaw()` on the write path mutates the backing array in place
    /// (`FriendlyByteBuf.readFixedSizeLongArray(storage.getRaw())`), so the
    /// Rust port exposes a `&mut` variant as well.
    fn get_raw_mut(&mut self) -> &mut [i64];

    /// `getSize()`.
    fn get_size(&self) -> usize;

    /// `getBits()` — the entry width in bits (also the wire palette-bits byte
    /// for the owning container).
    fn get_bits(&self) -> i32;

    /// `getAll(IntConsumer)` — visits every entry, index order.
    fn get_all(&self, output: &mut dyn FnMut(i32));

    /// `unpack(int[] output)` — writes every entry into `output[0..size]`.
    fn unpack(&self, output: &mut [i32]);

    /// `copy()` — a fresh storage with identical contents. Java returns `this`
    /// for [`ZeroBitStorage`] and a clone for [`SimpleBitStorage`]; the Rust
    /// port always returns an owned fresh value (Java's shared `this` is an
    /// aliasing optimization that is unobservable on the wire).
    fn copy_box(&self) -> Box<dyn BitStorage>;
}
