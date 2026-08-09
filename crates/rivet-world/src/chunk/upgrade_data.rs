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
//! RivetTodo(#216): the `SavedTick<Block>`/`SavedTick<Fluid>` neighbor-tick
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
    pub fn from_tag(tag: &CompoundTag, section_count: usize) -> Self {
        let mut index = vec![None; section_count];
        if let Some(indices) = tag.get_compound(TAG_INDICES) {
            for (i, entry) in index.iter_mut().enumerate() {
                *entry = indices.get_int_array(&i.to_string()).cloned();
            }
        }
        let side_bits = tag.get_byte_or(TAG_SIDES, 0);
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
        let mut side_bits = 0i8;
        for direction in &self.sides {
            side_bits |= 1 << *direction as i8;
        }
        tag.put_byte(TAG_SIDES, side_bits);
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
