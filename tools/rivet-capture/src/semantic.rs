//! Read-only wire parsers and semantic invariants for the join capture (#195).
//!
//! Everything here is a *read-only* decoder built on `frame.rs` leaf primitives
//! plus the generated registry tables. It never re-emits a canonical body and
//! never calls `normalize.rs`/`structured.rs`. This independence is what catches
//! a self-consistent normalizer: the preservation checks compare content
//! derived from the raw capture to content derived from the canonical capture,
//! so a normalizer that drops/duplicates/alters content — while still producing
//! a stable fixture — fails here.
//!
//! Checks:
//!
//! - chunk world-shape + state-id validity (the superflat seed-42 world);
//! - chunk-grid shape (contiguous square around the cache center);
//! - registry/tag id-range + coverage;
//! - `set_time` structural validity;
//! - raw↔canonical content preservation (chunk histogram, tag-id multiset,
//!   registry-entry set).

use std::collections::{BTreeMap, BTreeSet};

use crate::frame;
use crate::invariants::Failure;
use crate::normalize::NormalizedPacket;
use crate::packet::{CapturedPacket, Direction, State};

// ---- shared body helpers ----------------------------------------------------

/// Read a VarInt-length-prefixed string.
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

// ---- level_chunk_with_light -----------------------------------------------

const BLOCK_ENTRY_COUNT: usize = 4096;
const BIOME_ENTRY_COUNT: usize = 64;
/// Per-container wire thresholds matching Paper's `Strategy` table.  Block
/// containers switch to the global palette at nine bits; biome containers do
/// so at four bits.  Registry size is not the network threshold.
const BLOCK_GLOBAL_BITS: usize = 9;
const BIOME_GLOBAL_BITS: usize = 4;

/// The decoded shape of one chunk section.
pub struct SectionShape {
    pub block_count: i32,
    pub block_states: BTreeMap<i32, u64>,
    pub biome_states: BTreeMap<i32, u64>,
}

/// The decoded shape of one `level_chunk_with_light` body: per-section block
/// state-id histogram and biome state-id, plus the coordinate grid.
pub struct ChunkShape {
    pub x: i32,
    pub z: i32,
    /// Keeping the complete biome histogram prevents a canonicalizer from dropping
    /// a non-first biome entry while still appearing structurally valid.
    pub sections: Vec<SectionShape>,
}

/// Decode a chunk body into its logical shape (read-only). `None` on a parse
/// error — the detector reports a malformed chunk.
pub fn decode_chunk(body: &[u8]) -> Option<ChunkShape> {
    if body.len() < 8 {
        return None;
    }
    let x = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    let z = i32::from_be_bytes([body[4], body[5], body[6], body[7]]);
    let mut off = 8;
    let heightmap_count = frame::read_varint(body, &mut off)?;
    for _ in 0..heightmap_count.max(0) {
        frame::read_varint(body, &mut off)?; // type id
        let len = frame::read_varint(body, &mut off)?;
        frame::read_bytes(body, &mut off, (len.max(0) as usize) * 8)?;
    }
    let buffer_len = frame::read_varint(body, &mut off)?;
    let buffer = frame::read_bytes(body, &mut off, buffer_len.max(0) as usize)?;

    let mut sections = Vec::new();
    let mut sp = 0usize;
    while sp < buffer.len() {
        let header = frame::read_bytes(buffer, &mut sp, 4)?;
        let block_count = i16::from_be_bytes([header[0], header[1]]) as i32;
        let (states, sp_after) = decode_paletted(buffer, sp, BLOCK_ENTRY_COUNT, BLOCK_GLOBAL_BITS)?;
        let (biomes, sp_after2) =
            decode_paletted(buffer, sp_after, BIOME_ENTRY_COUNT, BIOME_GLOBAL_BITS)?;
        sp = sp_after2;
        sections.push(SectionShape {
            block_count,
            block_states: states,
            biome_states: biomes,
        });
    }
    // This semantic detector is for the captured full-height join fixture, not
    // an arbitrary partial chunk. A missing section must remain visible rather
    // than being silently accepted as a valid all-air world.
    if sp != buffer.len() || sections.len() != 24 {
        return None;
    }
    Some(ChunkShape { x, z, sections })
}

/// Decode a PalettedContainer into a state-id histogram. Returns the histogram
/// and the offset just past the container.
fn decode_paletted(
    body: &[u8],
    off: usize,
    entry_count: usize,
    global_bits: usize,
) -> Option<(BTreeMap<i32, u64>, usize)> {
    let bits = *body.get(off)? as usize;
    if bits > 64 {
        return None;
    }
    let mut o = off + 1;
    let mut palette = Vec::new();
    if bits == 0 {
        // SingleValuePalette: exactly one id VarInt, no count prefix.
        palette.push(frame::read_varint(body, &mut o)?);
    } else if bits < global_bits {
        let count = usize::try_from(frame::read_varint(body, &mut o)?).ok()?;
        if count == 0 || count > entry_count {
            return None;
        }
        for _ in 0..count {
            palette.push(frame::read_varint(body, &mut o)?);
        }
    }
    let values_per_long = if bits == 0 {
        1
    } else {
        64usize.checked_div(bits)?
    };
    let long_count = if bits == 0 {
        0
    } else {
        entry_count.div_ceil(values_per_long)
    };
    let data = frame::read_bytes(body, &mut o, long_count * 8)?;

    let mut histogram: BTreeMap<i32, u64> = BTreeMap::new();
    if bits == 0 {
        *histogram.entry(palette[0]).or_default() += entry_count as u64;
        return Some((histogram, o));
    }
    let words: Vec<u64> = data
        .chunks_exact(8)
        .map(|c| u64::from_be_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
        .collect();
    let mask = if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let values_per_long = 64 / bits;
    for i in 0..entry_count {
        let word = i / values_per_long;
        let shift = (i % values_per_long) * bits;
        let idx = ((words[word] >> shift) & mask) as usize;
        let state = if bits >= global_bits {
            idx as i32
        } else {
            *palette.get(idx)? // an out-of-range local index is malformed
        };
        *histogram.entry(state).or_default() += 1;
    }
    Some((histogram, o))
}

/// The decoded registry/tag structure of the configuration stream.
struct ConfigStream {
    /// registry name -> entry count.
    registry_counts: BTreeMap<String, i32>,
    /// tag registry name -> list of (tag name, [entry ids]).
    tags: BTreeMap<String, Vec<(String, Vec<i32>)>>,
}

fn read_config_stream(packets: &[CapturedPacket]) -> Option<ConfigStream> {
    let mut registry_counts = BTreeMap::new();
    let mut tags: BTreeMap<String, Vec<(String, Vec<i32>)>> = BTreeMap::new();
    for p in packets {
        if p.state != State::Configuration {
            continue;
        }
        match (p.direction, p.id) {
            (Direction::Clientbound, 7) => {
                let mut off = 0;
                let registry = read_string(&p.body, &mut off)?;
                let entry_count = frame::read_varint(&p.body, &mut off)?;
                registry_counts.insert(registry.clone(), entry_count);
                let mut names = BTreeSet::new();
                for _ in 0..entry_count.max(0) {
                    let id = read_string(&p.body, &mut off)?;
                    let present = *p.body.get(off)?;
                    off += 1;
                    if present != 0 {
                        // value NBT: [byte type][payload...]
                        let tag = *p.body.get(off)?;
                        if tag == 0 {
                            return None;
                        }
                        let mut noff = off + 1;
                        skip_nbt(&p.body, &mut noff, tag)?;
                        off = noff;
                    }
                    names.insert(id);
                }
                if off != p.body.len() {
                    return None;
                }
                let _ = names;
            }
            (Direction::Clientbound, 13) => {
                let mut off = 0;
                let registry_count = frame::read_varint(&p.body, &mut off)?;
                let mut entries = Vec::new();
                for _ in 0..registry_count.max(0) {
                    let name = read_string(&p.body, &mut off)?;
                    let tag_count = frame::read_varint(&p.body, &mut off)?;
                    let mut tag_list = Vec::new();
                    for _ in 0..tag_count.max(0) {
                        let tag_name = read_string(&p.body, &mut off)?;
                        let id_count = frame::read_varint(&p.body, &mut off)?;
                        let mut ids = Vec::with_capacity(id_count.max(0) as usize);
                        for _ in 0..id_count.max(0) {
                            ids.push(frame::read_varint(&p.body, &mut off)?);
                        }
                        tag_list.push((tag_name, ids));
                    }
                    entries.push((name, tag_list));
                }
                if off != p.body.len() {
                    return None;
                }
                for (name, tag_list) in entries {
                    tags.entry(name).or_default().extend(tag_list);
                }
            }
            _ => {}
        }
    }
    Some(ConfigStream {
        registry_counts,
        tags,
    })
}

/// Skip one NBT payload of `type_byte` (no name prefix).
fn skip_nbt(body: &[u8], off: &mut usize, type_byte: u8) -> Option<()> {
    match type_byte {
        0 => Some(()),
        1 => {
            *off += 1;
            Some(())
        }
        2 => {
            *off += 2;
            Some(())
        }
        3 | 5 => {
            *off += 4;
            Some(())
        }
        4 | 6 => {
            *off += 8;
            Some(())
        }
        7 => {
            let n = frame::read_i32(body, off)?;
            *off += n.max(0) as usize;
            Some(())
        }
        8 => {
            // Network NBT strings use a u16 length prefix (`writeUTF`), not a
            // VarInt.
            let len = u16::from_be_bytes([*body.get(*off)?, *body.get(*off + 1)?]) as usize;
            *off += 2 + len;
            Some(())
        }
        9 => {
            let elem = *body.get(*off)?;
            *off += 1;
            let n = frame::read_i32(body, off)?;
            for _ in 0..n.max(0) {
                skip_nbt(body, off, elem)?;
            }
            Some(())
        }
        10 => loop {
            let t = *body.get(*off)?;
            *off += 1;
            if t == 0 {
                return Some(());
            }
            let name_len = u16::from_be_bytes([*body.get(*off)?, *body.get(*off + 1)?]) as usize;
            *off += 2 + name_len;
            skip_nbt(body, off, t)?;
        },
        11 => {
            let n = frame::read_i32(body, off)?;
            *off += n.max(0) as usize * 4;
            Some(())
        }
        12 => {
            let n = frame::read_i32(body, off)?;
            *off += n.max(0) as usize * 8;
            Some(())
        }
        _ => None,
    }
}

/// Parse a `set_time` body: `[i64 gameTime][VarInt count][count ×
/// (VarInt holder][VarLong totalTicks][f32 partialTick][f32 rate])]`. Returns
/// `(gameTime, [(holder, totalTicks, partialTick, rate)])` or None on a parse
/// error / trailing bytes.
pub type SetTimeClock = (i32, i64, f32, f32);

pub fn parse_set_time(body: &[u8]) -> Option<(i64, Vec<SetTimeClock>)> {
    let mut off = 0;
    let game_time = frame::read_i64(body, &mut off)?;
    let count = frame::read_varint(body, &mut off)?;
    let mut clocks = Vec::with_capacity(count.max(0) as usize);
    for _ in 0..count.max(0) {
        let holder = frame::read_varint(body, &mut off)?;
        let total_ticks = frame::read_varlong(body, &mut off)?;
        let partial = frame::read_f32(body, &mut off)?;
        let rate = frame::read_f32(body, &mut off)?;
        clocks.push((holder, total_ticks, partial, rate));
    }
    if off != body.len() {
        return None;
    }
    Some((game_time, clocks))
}

// ---- public checks ---------------------------------------------------------

/// World-shape + state-id validity over the raw chunk packets.
pub fn check_chunk_semantics(packets: &[CapturedPacket]) -> Vec<Failure> {
    let mut f = Vec::new();
    let mut coords: Vec<(i32, i32)> = Vec::new();

    for p in packets {
        if p.state != State::Play || p.direction != Direction::Clientbound || p.id != 45 {
            continue;
        }
        let shape = match decode_chunk(&p.body) {
            Some(s) => s,
            None => {
                f.push(Failure::new(
                    "chunk-parse",
                    crate::ordering::identity(State::Play, Direction::Clientbound, 45),
                    "level_chunk_with_light body did not parse",
                ));
                continue;
            }
        };
        coords.push((shape.x, shape.z));
        // Section 0 is the y=0 ground layer in a 24-section buffer.
        if let Some(section) = shape.sections.first() {
            let bc = section.block_count;
            let hist = &section.block_states;
            let biomes = &section.biome_states;
            if bc != 256 {
                f.push(Failure::new(
                    "chunk",
                    format!(
                        "play/clientbound level_chunk_with_light [{},{}]",
                        shape.x, shape.z
                    ),
                    format!("ground section blockCount is {bc}, expected 256"),
                ));
            }
            let expected_blocks = BTreeMap::from([(0, 3840), (1, 256)]);
            if *hist != expected_blocks {
                f.push(Failure::new(
                    "chunk",
                    format!(
                        "play/clientbound level_chunk_with_light [{},{}]",
                        shape.x, shape.z
                    ),
                    format!("ground section is not 256 stone blocks (got {hist:?})"),
                ));
            }
            let expected_biomes = BTreeMap::from([(40, BIOME_ENTRY_COUNT as u64)]);
            if *biomes != expected_biomes {
                f.push(Failure::new(
                    "chunk",
                    format!(
                        "play/clientbound level_chunk_with_light [{},{}]",
                        shape.x, shape.z
                    ),
                    format!("ground section biome histogram is {biomes:?}, expected plains"),
                ));
            }
        }
        // Every other section must be all-air with plains biomes.
        for (si, section) in shape.sections.iter().enumerate().skip(1) {
            let bc = section.block_count;
            let hist = &section.block_states;
            let biomes = &section.biome_states;
            if bc != 0 {
                f.push(Failure::new(
                    "chunk",
                    format!(
                        "play/clientbound level_chunk_with_light [{},{}]",
                        shape.x, shape.z
                    ),
                    format!("section {si} has blockCount {bc}, expected all-air"),
                ));
            }
            if *hist != BTreeMap::from([(0, BLOCK_ENTRY_COUNT as u64)]) {
                f.push(Failure::new(
                    "chunk",
                    format!(
                        "play/clientbound level_chunk_with_light [{},{}]",
                        shape.x, shape.z
                    ),
                    format!("section {si} is not all-air: {hist:?}"),
                ));
            }
            if *biomes != BTreeMap::from([(40, BIOME_ENTRY_COUNT as u64)]) {
                f.push(Failure::new(
                    "chunk",
                    format!(
                        "play/clientbound level_chunk_with_light [{},{}]",
                        shape.x, shape.z
                    ),
                    format!("section {si} does not have all plains biomes: {biomes:?}"),
                ));
            }
        }
        // State-id validity: every block and biome state id is in range.
        for (si, section) in shape.sections.iter().enumerate() {
            let hist = &section.block_states;
            let biomes = &section.biome_states;
            for &state in hist.keys() {
                if !rivet_registry::generated::block_states::is_valid(
                    rivet_registry::generated::block_states::StateId(state as u16),
                ) {
                    f.push(Failure::new(
                        "chunk-state",
                        format!(
                            "play/clientbound level_chunk_with_light [{},{}]",
                            shape.x, shape.z
                        ),
                        format!("section {si} has invalid block state id {state}"),
                    ));
                }
            }
            for &biome in biomes.keys() {
                if !(0..66).contains(&biome) {
                    f.push(Failure::new(
                        "chunk-state",
                        format!(
                            "play/clientbound level_chunk_with_light [{},{}]",
                            shape.x, shape.z
                        ),
                        format!("section {si} has out-of-range biome state id {biome}"),
                    ));
                }
            }
        }
    }

    // Chunk-grid shape: a contiguous square centered on the cache center.
    let center = packets
        .iter()
        .find(|p| p.state == State::Play && p.direction == Direction::Clientbound && p.id == 94)
        .and_then(|p| {
            let mut off = 0;
            let cx = frame::read_varint(&p.body, &mut off)?;
            let cz = frame::read_varint(&p.body, &mut off)?;
            Some((cx, cz))
        });
    if let Some((cx, cz)) = center {
        let set: BTreeSet<(i32, i32)> = coords.iter().copied().collect();
        if set.len() != coords.len() {
            f.push(Failure::new(
                "chunk",
                "play/clientbound level_chunk_with_light",
                "duplicate chunk coordinates in the capture",
            ));
        }
        // The 4x4 view square minus 4 corner columns (11x11 = 121 minus 4 = 117).
        let radius = 5i32;
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let corner = dx.abs() == radius && dz.abs() == radius;
                let expected = (cx + dx, cz + dz);
                if corner {
                    if set.contains(&expected) {
                        f.push(Failure::new(
                            "chunk",
                            format!("play/clientbound level_chunk_with_light {expected:?}"),
                            "corner column present, but the view square has its 4 corners omitted",
                        ));
                    }
                } else if !set.contains(&expected) {
                    f.push(Failure::new(
                        "chunk",
                        format!("play/clientbound level_chunk_with_light {expected:?}"),
                        format!("column missing from the view square centered on ({cx}, {cz})"),
                    ));
                }
            }
        }
    }
    f
}

/// Registry/tag id-range + coverage over the raw configuration stream.
pub fn check_registry_tags(packets: &[CapturedPacket]) -> Vec<Failure> {
    let stream = match read_config_stream(packets) {
        Some(s) => s,
        None => {
            return vec![Failure::new(
                "config-parse",
                "configuration/clientbound registry_data/update_tags",
                "the configuration registry/tag stream did not parse",
            )];
        }
    };
    let mut f = Vec::new();

    for (name, tag_list) in &stream.tags {
        let entry_count = stream.registry_counts.get(name).copied();
        for (tag_name, ids) in tag_list {
            for &id in ids {
                let out_of_range = match entry_count {
                    Some(count) => id < 0 || id >= count,
                    None => false,
                };
                if out_of_range {
                    f.push(Failure::new(
                        "tag-range",
                        format!("configuration/clientbound update_tags {name}/#{tag_name}"),
                        format!(
                            "tag entry id {id} is outside the registry's {entry_count:?} entries"
                        ),
                    ));
                }
            }
        }
    }

    // Every tag registry must be a synced registry (present in registry_data) or
    // a statically-known registry whose contents the client holds natively. In
    // MC 26.2 those static registries are exactly the ones Paper tags without
    // sending a registry_data stream for: block/item plus the five codec-native
    // registries (entity_type, fluid, game_event, point_of_interest_type, potion).
    const STATIC: &[&str] = &[
        "minecraft:block",
        "minecraft:entity_type",
        "minecraft:fluid",
        "minecraft:game_event",
        "minecraft:item",
        "minecraft:point_of_interest_type",
        "minecraft:potion",
    ];
    for name in stream.tags.keys() {
        if !stream.registry_counts.contains_key(name) && !STATIC.contains(&name.as_str()) {
            f.push(Failure::new(
                "tag-coverage",
                format!("configuration/clientbound update_tags {name}"),
                "tag registry has no registry_data stream and is not a statically-known registry",
            ));
        }
    }
    f
}

/// `set_time` structural validity over the canonical capture. The normalizer
/// re-encodes the body, so the canonical form must still be valid wire format
/// and its holder set must match the world-clock registry.
pub fn check_set_time(canon: &[NormalizedPacket]) -> Vec<Failure> {
    let mut f = Vec::new();
    let body = match canon
        .iter()
        .find(|p| p.state == State::Play && p.direction == Direction::Clientbound && p.id == 113)
    {
        Some(p) => &p.body,
        // The world clock is synced immediately on join; a canonical capture
        // without `set_time` means the normalizer dropped it, so absence is a
        // violation — the byte-diff would also catch it, but the detector names
        // the defect instead of leaving it to the diff layer alone.
        None => {
            f.push(Failure::new(
                "set_time",
                "play/clientbound set_time",
                "missing from the canonical capture — the world clock sync was dropped",
            ));
            return f;
        }
    };
    match parse_set_time(body) {
        Some((game_time, clocks)) => {
            // Holder ids must be a permutation of the two world clocks (0, 1).
            let mut holders: Vec<i32> = clocks.iter().map(|(h, _, _, _)| *h).collect();
            holders.sort_unstable();
            if holders != vec![0, 1] {
                f.push(Failure::new(
                    "set_time",
                    "play/clientbound set_time",
                    format!("clock holder ids {holders:?} are not the overworld + the_end pair"),
                ));
            }
            let _ = game_time;
        }
        None => {
            f.push(Failure::new(
                "set_time",
                "play/clientbound set_time",
                "body is not a valid wire set_time (parse failed or trailing bytes)",
            ));
        }
    }
    f
}

// ---- raw↔canonical content preservation -------------------------------------

/// Content-preservation checks: the canonicalizer is *claimed* to be order-only,
/// so content derived from the raw capture must equal content derived from the
/// canonical capture. Any drop/duplicate/alteration diverges.
pub fn check_preservation(raw: &[CapturedPacket], canon: &[NormalizedPacket]) -> Vec<Failure> {
    let mut f = Vec::new();
    check_chunk_histograms(raw, canon, &mut f);
    check_tag_id_multiset(raw, canon, &mut f);
    check_registry_entry_set(raw, canon, &mut f);
    f
}

fn check_chunk_histograms(
    raw: &[CapturedPacket],
    canon: &[NormalizedPacket],
    f: &mut Vec<Failure>,
) {
    let raw_chunks: Vec<(i32, i32, ChunkShape)> = raw
        .iter()
        .filter(|p| p.state == State::Play && p.direction == Direction::Clientbound && p.id == 45)
        .filter_map(|p| decode_chunk(&p.body).map(|s| (s.x, s.z, s)))
        .collect();
    let canon_chunks: Vec<(i32, i32, ChunkShape)> = canon
        .iter()
        .filter(|p| p.state == State::Play && p.direction == Direction::Clientbound && p.id == 45)
        .filter_map(|p| decode_chunk(&p.body).map(|s| (s.x, s.z, s)))
        .collect();

    // Normalize coordinates: canonical coords are spawn-relative (0,0 center);
    // raw coords are absolute. Compare by shape-key regardless of the offset.
    let raw_keys: BTreeMap<(i32, i32), ChunkShape> = raw_chunks
        .into_iter()
        .map(|(_, _, s)| (s.x, s.z, s))
        .map(|(x, z, s)| ((x, z), s))
        .collect();
    let canon_keys: BTreeMap<(i32, i32), ChunkShape> = canon_chunks
        .into_iter()
        .map(|(_, _, s)| (s.x, s.z, s))
        .map(|(x, z, s)| ((x, z), s))
        .collect();

    // Align by sorted-coordinate rank: both capture the same 117-column square,
    // so the i-th raw column matches the i-th canonical column.
    let mut raw_sorted: Vec<&ChunkShape> = raw_keys.values().collect();
    raw_sorted.sort_by_key(|s| (s.x, s.z));
    let mut canon_sorted: Vec<&ChunkShape> = canon_keys.values().collect();
    canon_sorted.sort_by_key(|s| (s.x, s.z));

    if raw_sorted.len() != canon_sorted.len() {
        f.push(Failure::new(
            "preserve",
            "play/clientbound level_chunk_with_light",
            format!(
                "chunk count changed from {} (raw) to {} (canonical) — the normalizer dropped or duplicated a chunk",
                raw_sorted.len(),
                canon_sorted.len()
            ),
        ));
        return;
    }
    for (r, c) in raw_sorted.iter().zip(canon_sorted.iter()) {
        if r.sections.len() != c.sections.len() {
            f.push(Failure::new(
                "preserve",
                format!("play/clientbound level_chunk_with_light [{},{}]", r.x, r.z),
                format!(
                    "section count changed from {} (raw) to {} (canonical)",
                    r.sections.len(),
                    c.sections.len()
                ),
            ));
            continue;
        }
        for (i, (rb, cb)) in r.sections.iter().zip(c.sections.iter()).enumerate() {
            // blockCount and biome must be identical; the block-state histogram
            // must be identical (content), independent of palette order.
            if rb.block_count != cb.block_count {
                f.push(Failure::new(
                    "preserve",
                    format!("play/clientbound level_chunk_with_light [{},{}]", r.x, r.z),
                    format!(
                        "section {i} blockCount changed from {} (raw) to {} (canonical)",
                        rb.block_count, cb.block_count
                    ),
                ));
            }
            if rb.block_states != cb.block_states {
                f.push(Failure::new(
                    "preserve",
                    format!("play/clientbound level_chunk_with_light [{},{}]", r.x, r.z),
                    format!(
                        "section {i} block-state histogram changed from {:?} (raw) to {:?} (canonical) — the palette sort corrupted content",
                        rb.block_states, cb.block_states
                    ),
                ));
            }
            if rb.biome_states != cb.biome_states {
                f.push(Failure::new(
                    "preserve",
                    format!("play/clientbound level_chunk_with_light [{},{}]", r.x, r.z),
                    format!(
                        "section {i} biome histogram changed from {:?} (raw) to {:?} (canonical)",
                        rb.biome_states, cb.biome_states
                    ),
                ));
            }
        }
    }
}

fn check_tag_id_multiset(raw: &[CapturedPacket], canon: &[NormalizedPacket], f: &mut Vec<Failure>) {
    let mut raw_ids: Vec<i32> = Vec::new();
    for p in raw {
        if p.state != State::Configuration || p.direction != Direction::Clientbound || p.id != 13 {
            continue;
        }
        if let Some(ids) = collect_tag_ids(&p.body) {
            raw_ids.extend(ids);
        }
    }
    let mut canon_ids: Vec<i32> = Vec::new();
    for p in canon {
        if p.state != State::Configuration || p.direction != Direction::Clientbound || p.id != 13 {
            continue;
        }
        if let Some(ids) = collect_tag_ids(&p.body) {
            canon_ids.extend(ids);
        }
    }
    raw_ids.sort_unstable();
    canon_ids.sort_unstable();
    if raw_ids != canon_ids {
        f.push(Failure::new(
            "preserve",
            "configuration/clientbound update_tags",
            format!(
                "tag entry-id multiset changed: {} raw vs {} canonical",
                raw_ids.len(),
                canon_ids.len()
            ),
        ));
    }
}

fn collect_tag_ids(body: &[u8]) -> Option<Vec<i32>> {
    let mut off = 0;
    let registry_count = frame::read_varint(body, &mut off)?;
    let mut ids = Vec::new();
    for _ in 0..registry_count.max(0) {
        let name_len = frame::read_varint(body, &mut off)?;
        off += name_len.max(0) as usize;
        let tag_count = frame::read_varint(body, &mut off)?;
        for _ in 0..tag_count.max(0) {
            let tag_len = frame::read_varint(body, &mut off)?;
            off += tag_len.max(0) as usize;
            let id_count = frame::read_varint(body, &mut off)?;
            for _ in 0..id_count.max(0) {
                ids.push(frame::read_varint(body, &mut off)?);
            }
        }
    }
    Some(ids)
}

fn check_registry_entry_set(
    raw: &[CapturedPacket],
    canon: &[NormalizedPacket],
    f: &mut Vec<Failure>,
) {
    if let (Some(r), Some(c)) = (registry_entry_names(raw), registry_entry_names_canon(canon))
        && r != c
    {
        f.push(Failure::new(
            "preserve",
            "configuration/clientbound registry_data",
            "registry entry-name set changed between raw and canonical — the normalizer dropped or duplicated an entry",
        ));
    }
}

fn registry_entry_names(packets: &[CapturedPacket]) -> Option<BTreeMap<String, BTreeSet<String>>> {
    let mut out = BTreeMap::new();
    for p in packets {
        if p.state != State::Configuration || p.direction != Direction::Clientbound || p.id != 7 {
            continue;
        }
        let mut off = 0;
        let registry = read_string(&p.body, &mut off)?;
        let entry_count = frame::read_varint(&p.body, &mut off)?;
        let mut names = BTreeSet::new();
        for _ in 0..entry_count.max(0) {
            let id = read_string(&p.body, &mut off)?;
            let present = *p.body.get(off)?;
            off += 1;
            if present != 0 {
                let tag = *p.body.get(off)?;
                let mut noff = off + 1;
                skip_nbt(&p.body, &mut noff, tag)?;
                off = noff;
            }
            names.insert(id);
        }
        out.insert(registry, names);
    }
    Some(out)
}

fn registry_entry_names_canon(
    packets: &[NormalizedPacket],
) -> Option<BTreeMap<String, BTreeSet<String>>> {
    let mut out = BTreeMap::new();
    for p in packets {
        if p.state != State::Configuration || p.direction != Direction::Clientbound || p.id != 7 {
            continue;
        }
        let mut off = 0;
        let registry = read_string(&p.body, &mut off)?;
        let entry_count = frame::read_varint(&p.body, &mut off)?;
        let mut names = BTreeSet::new();
        for _ in 0..entry_count.max(0) {
            let id = read_string(&p.body, &mut off)?;
            let present = *p.body.get(off)?;
            off += 1;
            if present != 0 {
                let tag = *p.body.get(off)?;
                let mut noff = off + 1;
                skip_nbt(&p.body, &mut noff, tag)?;
                off = noff;
            }
            names.insert(id);
        }
        out.insert(registry, names);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::test_helpers::write_varlong;
    use crate::frame::write_varint;

    fn cp(state: State, direction: Direction, id: i32, body: Vec<u8>) -> CapturedPacket {
        CapturedPacket {
            state,
            direction,
            id,
            body,
        }
    }

    /// Build a minimal 24-section superflat chunk body (section 0 = 256 stone,
    /// others all-air with single-value palettes), matching the real wire format.
    fn build_chunk(x: i32, z: i32) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&x.to_be_bytes());
        body.extend_from_slice(&z.to_be_bytes());
        write_varint(&mut body, 0); // no heightmaps
        let mut buf = Vec::new();
        for si in 0..24 {
            if si == 0 {
                buf.extend_from_slice(&256i16.to_be_bytes());
            } else {
                buf.extend_from_slice(&0i16.to_be_bytes());
            }
            buf.extend_from_slice(&0i16.to_be_bytes()); // fluidCount
            if si == 0 {
                // Ground section: 4-bit palette [1, 0], first 256 entries at
                // index 0 (stone), the rest at index 1 (air).
                buf.push(4);
                write_varint(&mut buf, 2);
                write_varint(&mut buf, 1);
                write_varint(&mut buf, 0);
                let long_count = (BLOCK_ENTRY_COUNT * 4).div_ceil(64);
                let mut words = vec![0u64; long_count];
                for i in 0..BLOCK_ENTRY_COUNT {
                    let idx = if i < 256 { 0u64 } else { 1 };
                    words[i / 16] |= idx << ((i % 16) * 4);
                }
                for w in &words {
                    buf.extend_from_slice(&w.to_be_bytes());
                }
            } else {
                // All-air sections: SingleValuePalette (bits 0, one id, no count).
                buf.push(0);
                write_varint(&mut buf, 0);
            }
            // biomes: single-value plains.
            buf.push(0);
            write_varint(&mut buf, 40);
        }
        write_varint(&mut body, buf.len() as i32);
        body.extend_from_slice(&buf);
        body.push(0); // no block entities
        body
    }

    #[test]
    fn decode_chunk_superflat_shape() {
        let shape = decode_chunk(&build_chunk(0, 0)).expect("chunk");
        assert_eq!(shape.sections.len(), 24);
        let section = &shape.sections[0];
        assert_eq!(section.block_count, 256);
        assert_eq!(section.block_states.get(&1), Some(&256));
        assert_eq!(
            section.biome_states,
            BTreeMap::from([(40, BIOME_ENTRY_COUNT as u64)])
        );
        for section in &shape.sections[1..] {
            assert_eq!(section.block_count, 0);
            assert_eq!(
                section.block_states,
                BTreeMap::from([(0, BLOCK_ENTRY_COUNT as u64)])
            );
            assert_eq!(
                section.biome_states,
                BTreeMap::from([(40, BIOME_ENTRY_COUNT as u64)])
            );
        }
    }

    #[test]
    fn chunk_semantics_pass_on_superflat() {
        let mut packets = Vec::new();
        packets.push(cp(
            State::Play,
            Direction::Clientbound,
            94,
            vec![0x00, 0x00],
        ));
        for x in -5i32..=5 {
            for z in -5i32..=5 {
                if x.abs() == 5 && z.abs() == 5 {
                    continue;
                }
                packets.push(cp(
                    State::Play,
                    Direction::Clientbound,
                    45,
                    build_chunk(x, z),
                ));
            }
        }
        let fails = check_chunk_semantics(&packets);
        assert!(fails.is_empty(), "{fails:?}");
    }

    #[test]
    fn chunk_duplicate_coord_fails() {
        let mut packets = Vec::new();
        packets.push(cp(
            State::Play,
            Direction::Clientbound,
            94,
            vec![0x00, 0x00],
        ));
        for x in -5i32..=5 {
            for z in -5i32..=5 {
                if x.abs() == 5 && z.abs() == 5 {
                    continue;
                }
                packets.push(cp(
                    State::Play,
                    Direction::Clientbound,
                    45,
                    build_chunk(x, z),
                ));
            }
        }
        packets.push(cp(
            State::Play,
            Direction::Clientbound,
            45,
            build_chunk(0, 0),
        ));
        let fails = check_chunk_semantics(&packets);
        assert!(
            fails.iter().any(|x| x.message.contains("duplicate chunk")),
            "{fails:?}"
        );
    }

    #[test]
    fn chunk_missing_column_fails() {
        let mut packets = Vec::new();
        packets.push(cp(
            State::Play,
            Direction::Clientbound,
            94,
            vec![0x00, 0x00],
        ));
        for x in -5i32..=5 {
            for z in -5i32..=5 {
                if x.abs() == 5 && z.abs() == 5 {
                    continue;
                }
                if x == 0 && z == 0 {
                    continue; // drop the center column
                }
                packets.push(cp(
                    State::Play,
                    Direction::Clientbound,
                    45,
                    build_chunk(x, z),
                ));
            }
        }
        let fails = check_chunk_semantics(&packets);
        assert!(
            fails.iter().any(|x| x.message.contains("column missing")),
            "{fails:?}"
        );
    }

    #[test]
    fn set_time_parse_full_and_empty() {
        // Full 2-clock sync: gameTime 16, holders {0,1} at ticks 16, rate 1.
        let mut full = Vec::new();
        full.extend_from_slice(&16i64.to_be_bytes());
        write_varint(&mut full, 2);
        for h in [0, 1] {
            write_varint(&mut full, h);
            write_varlong(&mut full, 16);
            full.extend_from_slice(&0.0f32.to_be_bytes());
            full.extend_from_slice(&1.0f32.to_be_bytes());
        }
        let (gt, clocks) = parse_set_time(&full).expect("full");
        assert_eq!(gt, 16);
        assert_eq!(clocks.len(), 2);
        // Empty periodic broadcast: gameTime + a zero clock count.
        let mut empty = Vec::new();
        empty.extend_from_slice(&0i64.to_be_bytes());
        write_varint(&mut empty, 0);
        let (gt, clocks) = parse_set_time(&empty).expect("empty");
        assert_eq!(gt, 0);
        assert!(clocks.is_empty());
    }

    #[test]
    fn set_time_structural_check_catches_bad_holders() {
        let mut bad = Vec::new();
        bad.extend_from_slice(&0i64.to_be_bytes());
        write_varint(&mut bad, 2);
        write_varint(&mut bad, 0);
        write_varlong(&mut bad, 0);
        bad.extend_from_slice(&0.0f32.to_be_bytes());
        bad.extend_from_slice(&1.0f32.to_be_bytes());
        write_varint(&mut bad, 5); // out-of-range holder
        write_varlong(&mut bad, 0);
        bad.extend_from_slice(&0.0f32.to_be_bytes());
        bad.extend_from_slice(&1.0f32.to_be_bytes());
        let canon = vec![NormalizedPacket {
            state: State::Play,
            direction: Direction::Clientbound,
            id: 113,
            body: bad,
            note: String::new(),
        }];
        let fails = check_set_time(&canon);
        assert!(
            fails
                .iter()
                .any(|x| x.kind == "set_time" && x.message.contains("holder ids")),
            "{fails:?}"
        );
    }

    #[test]
    fn tag_id_range_catches_out_of_range() {
        // registry_data with 2 entries; update_tags with an id of 5. The registry
        // name must be one the stream syncs so only the range check fires.
        fn registry_data(name: &str, count: i32) -> Vec<u8> {
            let mut rd = Vec::new();
            write_varint(&mut rd, name.len() as i32);
            rd.extend_from_slice(name.as_bytes());
            write_varint(&mut rd, count);
            for id in ["a", "b"] {
                write_varint(&mut rd, id.len() as i32);
                rd.extend_from_slice(id.as_bytes());
                rd.push(0);
            }
            rd
        }
        let mut ut = Vec::new();
        write_varint(&mut ut, 1); // one registry
        write_varint(&mut ut, "minecraft:banner_pattern".len() as i32);
        ut.extend_from_slice(b"minecraft:banner_pattern");
        write_varint(&mut ut, 1); // one tag
        write_varint(&mut ut, 1);
        ut.push(b't');
        write_varint(&mut ut, 1);
        write_varint(&mut ut, 5); // out of range (registry has 2 entries)
        let packets = vec![
            cp(
                State::Configuration,
                Direction::Clientbound,
                7,
                registry_data("minecraft:banner_pattern", 2),
            ),
            cp(State::Configuration, Direction::Clientbound, 13, ut),
        ];
        let fails = check_registry_tags(&packets);
        assert!(fails.iter().any(|x| x.kind == "tag-range"), "{fails:?}");
    }
}
