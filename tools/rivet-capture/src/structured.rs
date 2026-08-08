//! Canonical re-serialization of wire-format payloads whose byte order is
//! nondeterministic across Paper boots.
//!
//! Java serializes several join-path payloads from `HashMap`s (or fastutil
//! `Object2ObjectOpenHashMap`s) whose iteration order is not stable across JVM
//! processes:
//!
//! - `update_tags` (13): the registries, the tags within each registry, and the
//!   entry ids within each tag can each appear in a different order;
//! - `registry_data` (7): the entries and the fields of every NBT compound value
//!   (fastutil-map backed `CompoundTag`) can be in a different order;
//! - `level_chunk_with_light` (45): the heightmap map (types → long arrays) can
//!   be in a different order.
//!
//! In every case the *content* is identical across boots (verified by the
//! determinism harness); only the order varies. These functions parse the
//! payload into its logical structure, sort the order-insensitive levels, and
//! re-serialize byte-for-byte. Everything else in the packet is left untouched.
//!
//! `update_advancements` (130) used to be canonicalized here; its display
//! canonicalizer now lives in `rivet-decode::advancement` (see `normalize.rs`),
//! which structurally sorts advancement display NBT compound fields and
//! `DataComponentPatch` entries (issue #221).
//!
//! All multi-byte integers are big-endian (Java `DataOutput`); VarInts and
//! strings follow the Minecraft protocol.

use crate::frame;

// NBT parse/serialize is shared with `rivet-decode::advancement` (the
// update_advancements canonicalizer): one strict parser + writer so the two
// join-path canonicalizers cannot drift on the wire bytes they accept/reject
// (negative array/list lengths, ListTag elem End, modified-UTF-8 strings).
use rivet_decode::nbt::{read_nbt, write_nbt};
// The `Nbt` variant constructors are used by the test module (`super::*`).
#[cfg(test)]
use rivet_decode::nbt::Nbt;

fn read_string(body: &[u8], off: &mut usize) -> Option<String> {
    let len = frame::read_varint(body, off)?;
    if len < 0 {
        return None;
    }
    let s = std::str::from_utf8(body.get(*off..*off + len as usize)?)
        .ok()?
        .to_owned();
    *off += len as usize;
    Some(s)
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    frame::write_varint(out, s.len() as i32);
    out.extend_from_slice(s.as_bytes());
}

/// Canonicalize a `registry_data` (7) body: sort entries by entry id and sort
/// the fields of every NBT compound value.
pub fn canon_registry_data(body: &[u8]) -> Option<Vec<u8>> {
    let mut off = 0;
    let registry = read_string(body, &mut off)?;
    let entry_count = frame::read_varint(body, &mut off)?;
    let mut entries = Vec::with_capacity(entry_count.max(0) as usize);
    for _ in 0..entry_count.max(0) {
        let id = read_string(body, &mut off)?;
        let present = *body.get(off)?;
        off += 1;
        let value = if present != 0 {
            Some(read_nbt(body, &mut off)?)
        } else {
            None
        };
        entries.push((id, value));
    }
    // A trailing partial read would mean the body has bytes we did not account
    // for — fail loudly rather than emit a corrupt canonical form.
    if off != body.len() {
        return None;
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = Vec::with_capacity(body.len());
    write_string(&mut out, &registry);
    frame::write_varint(&mut out, entries.len() as i32);
    for (id, value) in &entries {
        write_string(&mut out, id);
        match value {
            Some(v) => {
                out.push(1);
                write_nbt(&mut out, v);
            }
            None => out.push(0),
        }
    }
    Some(out)
}

/// Canonicalize an `update_attributes` (131) body (after the entity id is
/// rewritten to 1): sort the attribute snapshots by attribute id and each
/// snapshot's modifiers by modifier id.
pub fn canon_update_attributes(body: &[u8]) -> Option<Vec<u8>> {
    let mut off = 0;
    let entity_id = frame::read_varint(body, &mut off)?;
    let count = frame::read_varint(body, &mut off)?;
    let mut snapshots = Vec::with_capacity(count.max(0) as usize);
    for _ in 0..count.max(0) {
        let attr_id = frame::read_varint(body, &mut off)?;
        let base_off = off;
        frame::read_bytes(body, &mut off, 8)?; // base value (double)
        let mod_count = frame::read_varint(body, &mut off)?;
        let mut modifiers = Vec::with_capacity(mod_count.max(0) as usize);
        for _ in 0..mod_count.max(0) {
            let start = off;
            let id = read_string(body, &mut off)?;
            frame::read_bytes(body, &mut off, 8)?; // amount
            frame::read_varint(body, &mut off)?; // operation
            modifiers.push((id, body[start..off].to_vec()));
        }
        modifiers.sort_by(|a, b| a.0.cmp(&b.0));
        // Rebuild: attr_id + base + mod count + sorted modifiers.
        let mut raw = Vec::with_capacity(modifiers.len() * 8 + 24);
        frame::write_varint(&mut raw, attr_id);
        raw.extend_from_slice(&body[base_off..base_off + 8]);
        frame::write_varint(&mut raw, modifiers.len() as i32);
        for (_, m) in &modifiers {
            raw.extend_from_slice(m);
        }
        snapshots.push((attr_id, raw));
    }
    if off != body.len() {
        return None;
    }
    snapshots.sort_by_key(|(id, _)| *id);

    let mut out = Vec::with_capacity(body.len());
    frame::write_varint(&mut out, entity_id);
    frame::write_varint(&mut out, snapshots.len() as i32);
    for (_, raw) in &snapshots {
        out.extend_from_slice(raw);
    }
    Some(out)
}

/// Canonicalize an `update_recipes` (133) body: the recipe-property-set map
/// (HashMap) and each set's item list (HashSet) iterate per-boot; sort both.
/// The trailing stonecutter `SelectableRecipe.SingleInputSet` is deterministic
/// on the join path (verified across boots) and preserved verbatim.
///
/// Wire format: `[VarInt map count][count × ([ResourceKey id][RecipePropertySet])]`
/// followed by the stonecutter list, where a `RecipePropertySet` is
/// `[VarInt item count][count × item VarInt]`.
pub fn canon_update_recipes(body: &[u8]) -> Option<Vec<u8>> {
    let mut off = 0;
    let count = frame::read_varint(body, &mut off)?;
    let mut sets = Vec::with_capacity(count.max(0) as usize);
    for _ in 0..count.max(0) {
        let id = read_string(body, &mut off)?;
        let item_count = frame::read_varint(body, &mut off)?;
        let mut items = Vec::with_capacity(item_count.max(0) as usize);
        for _ in 0..item_count.max(0) {
            items.push(frame::read_varint(body, &mut off)?);
        }
        items.sort_unstable();
        sets.push((id, items));
    }
    let tail = body.get(off..)?; // stonecutter list (deterministic)
    sets.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = Vec::with_capacity(body.len());
    frame::write_varint(&mut out, sets.len() as i32);
    for (id, items) in &sets {
        write_string(&mut out, id);
        frame::write_varint(&mut out, items.len() as i32);
        for item in items {
            frame::write_varint(&mut out, *item);
        }
    }
    out.extend_from_slice(tail);
    Some(out)
}

/// Canonicalize an `update_tags` (13) body: sort registries by name, tags by
/// name, and entry ids ascending.
pub fn canon_update_tags(body: &[u8]) -> Option<Vec<u8>> {
    let mut off = 0;
    let registry_count = frame::read_varint(body, &mut off)?;
    let mut registries = Vec::with_capacity(registry_count.max(0) as usize);
    for _ in 0..registry_count.max(0) {
        let name = read_string(body, &mut off)?;
        let tag_count = frame::read_varint(body, &mut off)?;
        let mut tags = Vec::with_capacity(tag_count.max(0) as usize);
        for _ in 0..tag_count.max(0) {
            let tag_name = read_string(body, &mut off)?;
            let id_count = frame::read_varint(body, &mut off)?;
            let mut ids = Vec::with_capacity(id_count.max(0) as usize);
            for _ in 0..id_count.max(0) {
                ids.push(frame::read_varint(body, &mut off)?);
            }
            ids.sort_unstable();
            tags.push((tag_name, ids));
        }
        tags.sort_by(|a, b| a.0.cmp(&b.0));
        registries.push((name, tags));
    }
    if off != body.len() {
        return None;
    }
    registries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = Vec::with_capacity(body.len());
    frame::write_varint(&mut out, registries.len() as i32);
    for (name, tags) in &registries {
        write_string(&mut out, name);
        frame::write_varint(&mut out, tags.len() as i32);
        for (tag_name, ids) in tags {
            write_string(&mut out, tag_name);
            frame::write_varint(&mut out, ids.len() as i32);
            for id in ids {
                frame::write_varint(&mut out, *id);
            }
        }
    }
    Some(out)
}

/// Number of block positions in a 16×16×16 section (block-states container).
const BLOCK_ENTRY_COUNT: usize = 4096;
/// Number of biome entries in a 4×4×4 section (biome container).
const BIOME_ENTRY_COUNT: usize = 64;
/// The wire threshold above which a `PalettedContainer` is the global palette
/// (no palette on the wire; the packed indices are already global state ids).
/// This must equal the generated `GLOBAL_PALETTE_BITS` (`ceillog2(BLOCK_STATE_COUNT)`
/// = 15) — a local palette can hold up to 2^bits local ids, so a section whose
/// live state count exceeds 2^8 = 256 distinct states (e.g. any real world with
/// more than one biome's blocks in a section) would otherwise be misread as a
/// global palette by a threshold of 9.
const GLOBAL_BITS: usize = rivet_registry::generated::block_states::GLOBAL_PALETTE_BITS as usize;

/// Canonicalize one `PalettedContainer` (block states or biomes) starting at
/// `off`: sort the local palette by state id ascending and re-pack the data so
/// the indices follow the sorted palette. Returns the canonical bytes and the
/// offset just past the container.
///
/// Wire format: `[byte bits]`, then the palette, then the fixed-size packed
/// long array (no length prefix — the reader derives the count from `bits` and
/// the container's entry count).
///
/// - `bits == 0` — `SingleValuePalette`: exactly one id VarInt, no count prefix
///   (Paper's `SingleValuePalette.write` = `writeVarInt(globalMap.getId(value))`),
///   and no packed data. Sorting is a no-op.
/// - `1 <= bits < GLOBAL_BITS` — `LinearPalette`/`HashMapPalette`
///   (`[VarInt count][count × VarInt id]`). For the multi-entry palettes the
///   per-boot iteration order is order-insensitive; sort + remap.
/// - `bits >= GLOBAL_BITS` — `GlobalPalette`, no palette; indices are already
///   canonical, leave the data untouched.
fn canon_paletted(body: &[u8], off: usize, entry_count: usize) -> Option<(Vec<u8>, usize)> {
    let bits = *body.get(off)? as usize;
    let mut o = off + 1;
    let mut palette = Vec::new();
    if bits == 0 {
        // SingleValuePalette: exactly one id VarInt, no count prefix (Paper's
        // `SingleValuePalette.write` = `writeVarInt(globalMap.getId(value))`).
        palette.push(frame::read_varint(body, &mut o)?);
    } else if bits < GLOBAL_BITS {
        // LinearPalette/HashMapPalette: `[VarInt count][count × VarInt id]`.
        let count = frame::read_varint(body, &mut o)?;
        for _ in 0..count.max(0) {
            palette.push(frame::read_varint(body, &mut o)?);
        }
    }
    let long_count = (entry_count * bits).div_ceil(64);
    let data = frame::read_bytes(body, &mut o, long_count * 8)?;

    if bits == 0 || palette.len() <= 1 || bits >= GLOBAL_BITS {
        // Single-value or empty or global: nothing order-dependent to fix.
        return Some((body[off..o].to_vec(), o));
    }

    // Sort palette entries by state id and map old index -> new index.
    let mut indexed: Vec<(i32, usize)> = palette
        .iter()
        .copied()
        .enumerate()
        .map(|(i, s)| (s, i))
        .collect();
    indexed.sort_by_key(|(s, _)| *s);
    let sorted_palette: Vec<i32> = indexed.iter().map(|(s, _)| *s).collect();
    let mut old_to_new = vec![0usize; palette.len()];
    for (new_i, (_, old_i)) in indexed.iter().enumerate() {
        old_to_new[*old_i] = new_i;
    }

    // Re-pack: decode each entry's old index, remap, write into a fresh long
    // array (LSB-first packing, exactly like Java's SimpleBitStorage).
    let data_words: Vec<u64> = data
        .chunks_exact(8)
        .map(|c| u64::from_be_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
        .collect();
    let mask = if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let mut repacked = vec![0u64; long_count];
    for i in 0..entry_count {
        let bit = i * bits;
        let word = bit / 64;
        let shift = bit % 64;
        let old = ((data_words[word] >> shift) & mask) as usize;
        if let Some(&new) = old_to_new.get(old) {
            repacked[word] |= (new as u64) << shift;
        }
    }

    let mut out = Vec::with_capacity((long_count * 8) + 16);
    out.push(bits as u8);
    frame::write_varint(&mut out, sorted_palette.len() as i32);
    for state_id in &sorted_palette {
        frame::write_varint(&mut out, *state_id);
    }
    for word in &repacked {
        out.extend_from_slice(&word.to_be_bytes());
    }
    Some((out, o))
}

/// Canonicalize a `level_chunk_with_light` (45) body:
///
/// `[Int chunkX][Int chunkZ]`
/// `[VarInt heightmapCount][count × ([VarInt type][LongArray])]`
/// `[VarInt bufferLen][sections…]`
/// `<block-entity list>`.
///
/// The heightmap map iterates per-boot (sorted by type id) and each section's
/// block-state/biome palettes iterate per-boot (sorted + indices re-packed).
/// The block-entity list is untouched.
pub fn canon_chunk(body: &[u8]) -> Option<Vec<u8>> {
    if body.len() < 8 {
        return None;
    }
    let mut off = 8;
    let count = frame::read_varint(body, &mut off)?;
    let mut entries = Vec::with_capacity(count.max(0) as usize);
    for _ in 0..count.max(0) {
        let type_id = frame::read_varint(body, &mut off)?;
        let len = frame::read_varint(body, &mut off)?;
        let longs = frame::read_bytes(body, &mut off, (len.max(0) as usize) * 8)?;
        entries.push((type_id, longs.to_vec()));
    }
    entries.sort_by_key(|(t, _)| *t);

    let buffer_len = frame::read_varint(body, &mut off)?;
    let buffer = frame::read_bytes(body, &mut off, buffer_len.max(0) as usize)?;

    // Normalize each section: [blockCount short][fluidCount short][states][biomes].
    let mut sections = Vec::with_capacity(buffer.len());
    let mut sp = 0usize;
    while sp < buffer.len() {
        let header = frame::read_bytes(buffer, &mut sp, 4)?;
        let (states, sp_after) = canon_paletted(buffer, sp, BLOCK_ENTRY_COUNT)?;
        let (biomes, sp_after2) = canon_paletted(buffer, sp_after, BIOME_ENTRY_COUNT)?;
        sp = sp_after2;
        sections.extend_from_slice(header);
        sections.extend_from_slice(&states);
        sections.extend_from_slice(&biomes);
    }

    let mut out = Vec::with_capacity(body.len());
    out.extend_from_slice(&body[0..8]); // chunkX, chunkZ
    frame::write_varint(&mut out, entries.len() as i32);
    for (type_id, longs) in &entries {
        frame::write_varint(&mut out, *type_id);
        frame::write_varint(&mut out, (longs.len() / 8) as i32);
        out.extend_from_slice(longs);
    }
    frame::write_varint(&mut out, sections.len() as i32);
    out.extend_from_slice(&sections);
    out.extend_from_slice(&body[off..]); // block-entity list
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip_nbt(v: &Nbt) -> Nbt {
        let mut out = Vec::new();
        write_nbt(&mut out, v);
        let mut off = 0;
        read_nbt(&out, &mut off).expect("parse")
    }

    #[test]
    fn nbt_round_trip_primitive_and_compound() {
        // Fields are stored sorted on parse, so round-trip of an already-sorted
        // compound is identity.
        let v = Nbt::Compound(vec![
            ("a".to_owned(), Nbt::Int(-7)),
            ("b".to_owned(), Nbt::String("x".to_owned())),
            (
                "c".to_owned(),
                Nbt::List {
                    elem: 3,
                    items: vec![Nbt::Int(1), Nbt::Int(2)],
                },
            ),
        ]);
        assert_eq!(round_trip_nbt(&v), v);
    }

    #[test]
    fn nbt_compound_fields_sort_canonically() {
        // Same fields inserted in different order parse to the same canonical
        // value and serialize to identical bytes.
        let a = Nbt::Compound(vec![
            ("z".to_owned(), Nbt::Int(1)),
            ("a".to_owned(), Nbt::Int(2)),
        ]);
        let b = Nbt::Compound(vec![
            ("a".to_owned(), Nbt::Int(2)),
            ("z".to_owned(), Nbt::Int(1)),
        ]);
        let mut oa = Vec::new();
        let mut ob = Vec::new();
        write_nbt(&mut oa, &a);
        write_nbt(&mut ob, &b);
        assert_eq!(oa, ob);
        // Serialized form is sorted ('a' before 'z').
        assert_eq!(
            oa,
            vec![
                0x0A, 0x03, 0x00, 0x01, b'a', 0x00, 0x00, 0x00, 0x02, 0x03, 0x00, 0x01, b'z', 0x00,
                0x00, 0x00, 0x01, 0x00
            ]
        );
    }

    #[test]
    fn nbt_preserves_float_bits() {
        let v = Nbt::Compound(vec![("v".to_owned(), Nbt::Float(-0.0f32))]);
        let mut out = Vec::new();
        write_nbt(&mut out, &v);
        // -0.0f32 is 0x8000_0000 on the wire.
        assert_eq!(
            out,
            vec![0x0A, 0x05, 0x00, 0x01, b'v', 0x80, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn canon_registry_data_sorts_entries() {
        // registry "minecraft:test", two entries reversed.
        let mut body = Vec::new();
        write_string(&mut body, "minecraft:test");
        frame::write_varint(&mut body, 2);
        write_string(&mut body, "z");
        body.push(1);
        write_nbt(
            &mut body,
            &Nbt::Compound(vec![("f".to_owned(), Nbt::Int(1))]),
        );
        write_string(&mut body, "a");
        body.push(0);

        let out = canon_registry_data(&body).expect("canon");
        let mut off = 0;
        assert_eq!(read_string(&out, &mut off).unwrap(), "minecraft:test");
        assert_eq!(frame::read_varint(&out, &mut off), Some(2));
        assert_eq!(read_string(&out, &mut off).unwrap(), "a");
        assert_eq!(out[off], 0);
        off += 1;
        assert_eq!(read_string(&out, &mut off).unwrap(), "z");
    }

    #[test]
    fn canon_update_tags_sorts_all_levels() {
        let mut body = Vec::new();
        frame::write_varint(&mut body, 2);
        // registry "b" with tag "t" ids [3,1,2]
        write_string(&mut body, "b");
        frame::write_varint(&mut body, 1);
        write_string(&mut body, "t");
        frame::write_varint(&mut body, 3);
        for id in [3, 1, 2] {
            frame::write_varint(&mut body, id);
        }
        // registry "a" with tag "u" ids [7]
        write_string(&mut body, "a");
        frame::write_varint(&mut body, 1);
        write_string(&mut body, "u");
        frame::write_varint(&mut body, 1);
        frame::write_varint(&mut body, 7);

        let out = canon_update_tags(&body).expect("canon");
        let mut off = 0;
        assert_eq!(frame::read_varint(&out, &mut off), Some(2));
        // registry a first
        assert_eq!(read_string(&out, &mut off).unwrap(), "a");
        assert_eq!(frame::read_varint(&out, &mut off), Some(1));
        assert_eq!(read_string(&out, &mut off).unwrap(), "u");
        assert_eq!(frame::read_varint(&out, &mut off), Some(1));
        assert_eq!(frame::read_varint(&out, &mut off), Some(7));
        // registry b, ids sorted
        assert_eq!(read_string(&out, &mut off).unwrap(), "b");
        assert_eq!(frame::read_varint(&out, &mut off), Some(1));
        assert_eq!(read_string(&out, &mut off).unwrap(), "t");
        assert_eq!(frame::read_varint(&out, &mut off), Some(3));
        assert_eq!(frame::read_varint(&out, &mut off), Some(1));
        assert_eq!(frame::read_varint(&out, &mut off), Some(2));
        assert_eq!(frame::read_varint(&out, &mut off), Some(3));
        assert_eq!(off, out.len());
    }

    /// Build a full `level_chunk_with_light` body with a heightmap map and one
    /// section whose block palette is `palette` (in wire order) and whose 256
    /// non-air blocks sit at palette index `block_index` (the other 3840
    /// entries are the other palette value) — the real flat-world histogram.
    fn build_chunk(palette: &[i32], block_index: u8, biome_state: i32) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&3i32.to_be_bytes()); // chunkX
        body.extend_from_slice(&(-2i32).to_be_bytes()); // chunkZ
        // heightmap map: 3 entries, deliberately unsorted.
        frame::write_varint(&mut body, 3);
        for (t, len) in [(5u8, 1u8), (1, 2), (4, 1)] {
            frame::write_varint(&mut body, t as i32);
            frame::write_varint(&mut body, len as i32);
            body.extend(std::iter::repeat_n(0xAB, len as usize * 8));
        }
        // buffer: one section.
        let mut buf = Vec::new();
        buf.extend_from_slice(&256i16.to_be_bytes()); // blockCount
        buf.extend_from_slice(&0i16.to_be_bytes()); // fluidCount
        // block states: 4 bits, palette, packed data.
        buf.push(4);
        frame::write_varint(&mut buf, palette.len() as i32);
        for s in palette {
            frame::write_varint(&mut buf, *s);
        }
        let long_count = (BLOCK_ENTRY_COUNT * 4).div_ceil(64);
        let mut words = vec![0u64; long_count];
        for i in 0..BLOCK_ENTRY_COUNT {
            let idx = if i < 256 {
                block_index
            } else {
                block_index ^ 1
            };
            words[i / 16] |= u64::from(idx) << ((i % 16) * 4);
        }
        for w in &words {
            buf.extend_from_slice(&w.to_be_bytes());
        }
        // biomes: single-value palette (bits 0 = ONE id VarInt, no count).
        buf.push(0);
        frame::write_varint(&mut buf, biome_state);
        frame::write_varint(&mut body, buf.len() as i32);
        body.extend_from_slice(&buf);
        body.push(0x42); // block-entity list: empty (varint 0)
        body
    }

    #[test]
    fn canon_chunk_sorts_heightmaps_and_palettes() {
        // Same world encoded with the palette order swapped: boot A has stone(1)
        // first with 256 blocks at index 0, boot B has air(0) first with the
        // blocks at index 1. Both must canonicalize to the same bytes.
        let a = build_chunk(&[1, 0], 0, 40);
        let b = build_chunk(&[0, 1], 1, 40);
        let ca = canon_chunk(&a).expect("canon a");
        let cb = canon_chunk(&b).expect("canon b");
        assert_eq!(ca, cb);

        // The canonical section has the sorted palette [0, 1] and all blocks at
        // index 1 (the stone).
        let mut off = 8;
        assert_eq!(frame::read_varint(&ca, &mut off), Some(3));
        for t in [1, 4, 5] {
            assert_eq!(frame::read_varint(&ca, &mut off), Some(t));
            let len = frame::read_varint(&ca, &mut off).unwrap();
            assert_eq!(len, if t == 1 { 2 } else { 1 });
            off += len as usize * 8;
        }
        let buflen = frame::read_varint(&ca, &mut off).unwrap();
        // header(4) + bits(1) + palette count(1) + 2 states(2) + data(2048)
        // + biome bits(1) + biome state(1) = 2058.
        assert_eq!(buflen as usize, 2058);
        let buf = &ca[off..off + buflen as usize];
        assert_eq!(&buf[0..2], &256i16.to_be_bytes());
        assert_eq!(buf[4], 4); // bits
        assert_eq!(buf[5], 2); // palette count 2
        assert_eq!(buf[6], 0); // state 0 (air) first
        assert_eq!(buf[7], 1); // state 1 (stone)
        // data: 256 entries at index 1 (stone), 3840 at index 0 (air).
        // 256 entries × 4 bits = 128 bytes of 0x11, then zeros.
        // 256 entries × 4 bits = 128 bytes of 0x11, then zeros up to the end
        // of the 2048-byte data array (the following biomes bytes are not data).
        let data = &buf[8..8 + (BLOCK_ENTRY_COUNT * 4).div_ceil(64) * 8];
        assert!(
            data[..128].iter().all(|&b| b == 0x11),
            "first 256 blocks at index 1"
        );
        assert!(
            data[128..].iter().all(|&b| b == 0x00),
            "remaining blocks at index 0"
        );
    }

    /// Build a `level_chunk_with_light` body whose only section carries a
    /// single-value block palette and a single-value biome palette (the wire
    /// format Paper actually writes for bits==0: one id VarInt, no count).
    fn build_single_value_chunk(block_state: i32, biome_state: i32) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&0i32.to_be_bytes()); // chunkX
        body.extend_from_slice(&0i32.to_be_bytes()); // chunkZ
        // heightmap map: empty.
        frame::write_varint(&mut body, 0);
        // buffer: one section.
        let mut buf = Vec::new();
        buf.extend_from_slice(&0i16.to_be_bytes()); // blockCount
        buf.extend_from_slice(&0i16.to_be_bytes()); // fluidCount
        // block states: bits 0, SingleValuePalette -> ONE id VarInt, no count.
        buf.push(0);
        frame::write_varint(&mut buf, block_state);
        // biomes: bits 0, SingleValuePalette -> ONE id VarInt, no count.
        buf.push(0);
        frame::write_varint(&mut buf, biome_state);
        frame::write_varint(&mut body, buf.len() as i32);
        body.extend_from_slice(&buf);
        body.push(0); // block-entity list: empty (varint 0)
        body
    }

    #[test]
    fn canon_chunk_parses_single_value_palettes_without_count() {
        // The real Paper wire format for bits==0 is a single id VarInt with no
        // count prefix (SingleValuePalette.write). A section made only of
        // single-value containers must round-trip unchanged: parse + re-emit
        // must be byte-identical, and the section buffer must land exactly on
        // its end (no over-advance).
        let body = build_single_value_chunk(1, 40);
        let out = canon_chunk(&body).expect("canon");
        assert_eq!(out, body, "single-value-only chunk must be byte-identical");

        // Walk the canonical body: [x][z][heightmap 0][buffer][block-entities 0].
        let mut off = 8;
        assert_eq!(frame::read_varint(&out, &mut off), Some(0)); // heightmaps
        let buflen = frame::read_varint(&out, &mut off).unwrap();
        assert_eq!(buflen as usize, 8); // 2 headers + bits + id + bits + id
        let buf = &out[off..off + buflen as usize];
        assert_eq!(buf[4], 0); // block bits
        assert_eq!(buf[5], 1); // block state id (single VarInt, not a count)
        assert_eq!(buf[6], 0); // biome bits
        assert_eq!(buf[7], 40); // biome state id
        assert_eq!(off + buflen as usize + 1, out.len());
    }

    #[test]
    fn canon_update_attributes_sorts_snapshots() {
        // [entity id 1][count 2][snapshots reversed across boots].
        fn build(order: &[i32]) -> Vec<u8> {
            let mut body = Vec::new();
            frame::write_varint(&mut body, 1);
            frame::write_varint(&mut body, 2);
            for attr in order {
                frame::write_varint(&mut body, *attr);
                body.extend_from_slice(&3.5f64.to_be_bytes()); // base
                frame::write_varint(&mut body, 0); // no modifiers
            }
            body
        }
        let a = build(&[9, 3]);
        let b = build(&[3, 9]);
        assert_ne!(a, b);
        let ca = canon_update_attributes(&a).expect("canon a");
        let cb = canon_update_attributes(&b).expect("canon b");
        assert_eq!(ca, cb);
        let mut off = 0;
        assert_eq!(frame::read_varint(&ca, &mut off), Some(1)); // entity id
        assert_eq!(frame::read_varint(&ca, &mut off), Some(2)); // count
        // Snapshot 1: attr id 3, then 8-byte base + mod count 0.
        assert_eq!(frame::read_varint(&ca, &mut off), Some(3));
        off += 8;
        assert_eq!(frame::read_varint(&ca, &mut off), Some(0));
        // Snapshot 2: attr id 9.
        assert_eq!(frame::read_varint(&ca, &mut off), Some(9));
        off += 8;
        assert_eq!(frame::read_varint(&ca, &mut off), Some(0));
        assert_eq!(off, ca.len());
    }
}
