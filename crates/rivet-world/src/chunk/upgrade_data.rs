//! Port of `net.minecraft.world.level.chunk.UpgradeData` (MC 26.2) — the
//! carrier the `ChunkAccess` constructor takes for old-version chunk upgrades.
//!
//! Java: `UpgradeData.java` in `working/Paper`. The class holds per-section
//! block-upgrade indices, a `Direction8` `sides` set, and two neighbor-tick
//! lists, and serializes them as NBT (`Indices`/`Sides`/`neighbor_block_ticks`/
//! `neighbor_fluid_ticks`).
//!
//! This slice ports the *carrier* only — `EMPTY`, the `index` array, the
//! `sides` bitmask over the already-ported `Direction8` (issue #212), and the
//! NBT `write`/`from_tag` round-trip for `Indices` + `Sides`. The block
//! fixers, the neighbor-tick `SavedTick` codecs, and the `upgrade` walk all
//! defer with the block/block-state units, which is why `UpgradeData::new`
//! takes the section count directly (Java's `LevelHeightAccessor` constructor
//! arg is only used to size `index`).
//!
//! The `SavedTick<Block>`/`SavedTick<Fluid>` neighbor-tick
//! codecs (`BLOCK_TICKS_CODEC`/`FLUID_TICKS_CODEC`) are not ported —
//! `SavedTick` lives with the `world.ticks` unit.
//! RivetTodo(#228): `BlockFixers`/`BlockFixer` and the `upgrade`/
//! `upgradeSides`/`upgradeInside` walk are not ported — they need real
//! `BlockBehaviour` shape-update flags; the `mc.world.level.block` slice
//! re-adds them when it lands. `UpgradeData.EMPTY` keeps the
//! `EmptyBlockGetter` level accessor arg in Java; the port's `EMPTY` is a
//! plain empty value (the accessor is only consulted by the deferred fixers).

use rivet_registry::core::Direction8;

use rivet_nbt::compound_tag::CompoundTag;

/// `UpgradeData.TAG_INDICES` — `"Indices"`.
const TAG_INDICES: &str = "Indices";
/// `UpgradeData.TAG_SIDES` — `"Sides"`.
const TAG_SIDES: &str = "Sides";

/// `net.minecraft.world.level.chunk.UpgradeData`.
#[derive(Clone)]
pub struct UpgradeData {
    /// `index` — per-section arrays of packed block coordinates to re-upgrade
    /// (`coordinate >> 8 & 15` is y, `>> 4 & 15` z, `& 15` x; see
    /// `ProtoChunk.packOffsetCoordinates`'s sibling layout). `None` where Java
    /// holds `null`.
    index: Vec<Option<Vec<i32>>>,
    /// `sides` — the `Direction8` set (an `EnumSet`); NBT is a bitmask over
    /// `direction8.ordinal()`.
    sides: Vec<Direction8>,
}

impl UpgradeData {
    /// `UpgradeData.EMPTY` — `new UpgradeData(EmptyBlockGetter.INSTANCE)`, the
    /// empty carrier (every section `null`, no sides).
    pub fn empty(section_count: usize) -> Self {
        UpgradeData {
            index: vec![None; section_count],
            sides: Vec::new(),
        }
    }

    /// `UpgradeData(CompoundTag, LevelHeightAccessor)` — the from-NBT carrier.
    /// Reads `Indices` (a compound of `String(sectionIndex)` → int-array),
    /// then `Sides` (a byte bitmask over `Direction8.values()`). The neighbor
    /// tick lists are skipped (deferred codecs).
    ///
    /// The `Sides` byte is widened to `i32` exactly like Java's `getIntOr`
    /// reads a byte tag (sign-extended, keeping the `0x80` bit of ordinal 7
    /// set); the mask test then matches `(sideInt & 1 << ordinal) != 0`.
    pub fn from_tag(tag: &CompoundTag, section_count: usize) -> Self {
        let mut index = vec![None; section_count];
        if let Some(indices) = tag.get_compound(TAG_INDICES) {
            for (i, entry) in index.iter_mut().enumerate() {
                *entry = indices.get_int_array(&i.to_string()).cloned();
            }
        }
        let side_bits = tag.get_byte_or(TAG_SIDES, 0) as i32;
        let mut sides = Vec::new();
        for (ordinal, direction) in Direction8::all().iter().enumerate() {
            if side_bits & (1 << ordinal) != 0 {
                sides.push(*direction);
            }
        }
        UpgradeData { index, sides }
    }

    /// `isEmpty()` — every `index` entry is null and `sides` is empty.
    pub fn is_empty(&self) -> bool {
        self.index.iter().all(|entry| entry.is_none()) && self.sides.is_empty()
    }

    /// `write()` — the NBT form. `Indices` collects the non-empty per-section
    /// arrays under `String.valueOf(sectionIndex)`, `Sides` is the packed byte
    /// (always written, even when 0, matching Java).
    pub fn write(&self) -> CompoundTag {
        let mut tag = CompoundTag::new();
        let mut indices = CompoundTag::new();
        for (i, entry) in self.index.iter().enumerate() {
            if let Some(values) = entry.as_ref().filter(|values| !values.is_empty()) {
                indices.put_int_array(&i.to_string(), values.clone());
            }
        }
        if !indices.is_empty() {
            tag.put(TAG_INDICES.into(), rivet_nbt::tag::Tag::Compound(indices));
        }
        let mut side_bits = 0i32;
        for direction in &self.sides {
            side_bits |= 1 << *direction as i32;
        }
        tag.put_byte(TAG_SIDES, side_bits as i8);
        tag
    }

    /// `copy()` — `this == EMPTY ? EMPTY : new UpgradeData(this)` — a deep
    /// copy (Java's `IntArrays.copy` per section). The port has no identity
    /// comparison, so it always copies.
    pub fn copy(&self) -> Self {
        UpgradeData {
            index: self.index.clone(),
            sides: self.sides.clone(),
        }
    }

    /// The per-section index arrays (read accessor for tests and the deferred
    /// upgrade walk).
    pub fn index(&self) -> &[Option<Vec<i32>>] {
        &self.index
    }

    /// The `sides` set, in `Direction8.values()` order.
    pub fn sides(&self) -> &[Direction8] {
        &self.sides
    }
}

impl Default for UpgradeData {
    fn default() -> Self {
        Self::empty(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::core::Direction8;

    /// An `Indices`/`Sides` NBT tag exercising every carrier field Java
    /// serializes: two per-section int arrays (one empty, one populated) and a
    /// `Sides` bitmask with the highest bit (`0x80`, `Direction8::NorthWest`
    /// ordinal 7) plus a low bit (`North`, ordinal 0).
    fn carrier_tag() -> CompoundTag {
        let mut tag = CompoundTag::new();
        let mut indices = CompoundTag::new();
        indices.put_int_array("0", vec![1, 2, 3]);
        indices.put_int_array("1", vec![]); // Java skips empty arrays on write
        indices.put_int_array("2", vec![0x1234]);
        tag.put(TAG_INDICES.into(), rivet_nbt::tag::Tag::Compound(indices));
        // `North` | `NorthWest` = 1 | 0x80 = 0x81, stored as a signed byte -127.
        tag.put_byte(TAG_SIDES, 0x81u8 as i8);
        tag
    }

    #[test]
    fn from_tag_reads_indices_and_sides_like_java() {
        let data = UpgradeData::from_tag(&carrier_tag(), 4);
        // Per-section arrays, keyed `String.valueOf(i)`, absent entries stay
        // `None`. The empty array at "1" is stored (the NBT carries it) even
        // though `write` omits empties — Java reads whatever the tag holds.
        assert_eq!(
            data.index(),
            &[Some(vec![1, 2, 3]), Some(vec![]), Some(vec![0x1234]), None,]
        );
        assert_eq!(data.sides(), &[Direction8::North, Direction8::NorthWest]);
    }

    #[test]
    fn write_round_trips_indices_and_sides_byte_identically() {
        let data = UpgradeData::from_tag(&carrier_tag(), 4);
        let written = data.write();
        // `Indices` omits the empty array (Java's `length != 0` guard).
        assert_eq!(
            written
                .get_compound(TAG_INDICES)
                .unwrap()
                .get_int_array("1"),
            None
        );
        assert_eq!(
            written
                .get_compound(TAG_INDICES)
                .unwrap()
                .get_int_array("0"),
            Some(&vec![1, 2, 3])
        );
        // `Sides` is always written, even when 0 (Java always `putByte`).
        assert_eq!(written.get_byte_or(TAG_SIDES, -1), 0x81u8 as i8);
    }

    #[test]
    fn sides_bitmask_0x80_round_trips_northwest() {
        // The 8th `Direction8` (NorthWest, ordinal 7) sets bit 7 = 0x80, which
        // as a Java byte is negative (-128). Java reads the byte back as an int
        // (`getIntOr` sign-extends) and tests `& (1 << 7) != 0`; the port's
        // `i32` widening must keep the high bit set.
        let mut tag = CompoundTag::new();
        tag.put_byte(TAG_SIDES, -128); // 0x80 as i8
        let data = UpgradeData::from_tag(&tag, 1);
        assert_eq!(data.sides(), &[Direction8::NorthWest]);

        // Write back: `[NorthWest]` → `0x80`, truncated to the signed byte -128.
        let mut single = UpgradeData::empty(1);
        single.sides.push(Direction8::NorthWest);
        assert_eq!(single.write().get_byte_or(TAG_SIDES, 0), -128);
    }

    #[test]
    fn missing_sides_defaults_to_zero_and_empty_data_stays_empty() {
        // A tag with no `Sides` reads as 0 (Java `getIntOr("Sides", 0)`).
        let empty = UpgradeData::from_tag(&CompoundTag::new(), 2);
        assert!(empty.is_empty());
        assert_eq!(empty.sides(), &[] as &[Direction8]);

        // An all-`None` index with sides present is not empty.
        let mut tag = CompoundTag::new();
        tag.put_byte(TAG_SIDES, 0x01);
        let with_side = UpgradeData::from_tag(&tag, 2);
        assert!(!with_side.is_empty());
        assert_eq!(with_side.sides(), &[Direction8::North]);
    }

    #[test]
    fn copy_is_a_deep_copy_independent_of_the_source() {
        let a = UpgradeData::from_tag(&carrier_tag(), 4);
        let b = a.copy();
        assert_eq!(a.index(), b.index());
        assert_eq!(a.sides(), b.sides());
        // `IntArrays.copy` per section: a source read from different NBT must
        // not disturb the earlier copy's arrays (Java copies, never aliases).
        let mut other = CompoundTag::new();
        let mut indices = CompoundTag::new();
        indices.put_int_array("0", vec![9, 9]);
        other.put(TAG_INDICES.into(), rivet_nbt::tag::Tag::Compound(indices));
        let a2 = UpgradeData::from_tag(&other, 4);
        assert_ne!(a2.index()[0].as_ref().unwrap(), &vec![1, 2, 3]);
        assert_eq!(b.index()[0].as_ref().unwrap(), &vec![1, 2, 3]);
        assert_eq!(b.index()[2].as_ref().unwrap(), &vec![0x1234]);
    }

    #[test]
    fn is_empty_requires_all_indexes_null_and_no_sides() {
        assert!(UpgradeData::empty(24).is_empty());
        // Any index entry makes it non-empty.
        let data = UpgradeData::from_tag(&carrier_tag(), 4);
        assert!(!data.is_empty());
    }
}
