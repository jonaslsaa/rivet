//! Canonical re-serialization of the `update_advancements` (130) packet body.
//!
//! Paper serializes the advancement added/removed/progress collections from
//! per-boot `HashMap`/`HashSet` order (`ClientboundUpdateAdvancementsPacket`),
//! so a capture can vary across boots. On top of the existing list/criteria
//! sorting this module structurally canonicalizes the advancement **display**
//! payloads (issue #221), the part the join fixture's canonicalizer once
//! preserved verbatim:
//!
//! - the two NBT components (`title`, `description`) re-emit compound fields
//!   sorted by name, because a `Component` serializes to an NBT compound whose
//!   field order depends on the DFU record map iteration;
//! - the `DataComponentPatch` (item icon components) re-emits the positive
//!   entries sorted by component type id and the negative entries sorted the
//!   same way, because the patch is a fastutil `Reference2ObjectMap` whose
//!   order is not stable across JVM processes (`DataComponentPatch.encode`).
//!
//! No value is fabricated: an unknown component type id, a non-empty patch the
//! canonicalizer cannot bound, or a display payload with unexpected bytes
//! makes the whole canonicalization fail (`None`), and the caller keeps the raw
//! body. For a capture that carries no advancement display data (the pinned
//! join fixture) that means the display is never rewritten or invented; the
//! body can still differ from a raw capture in the pre-existing non-display
//! ways — added/removed/progress sorting, criteria-name sorting, and
//! obtained-instant zeroing (the fixture's `unlock_right_away` criterion is
//! obtained=true with a wall-clock instant that canonicalizes to 0).
//!
//! The value dispatch is pinned to protocol 776 and the 26.2 component set:
//! every component value whose network form is NBT (`Component` title/
//! description and `custom_name`/`item_name` via `tagCodec`, `CustomData`
//! `custom_data`/`bucket_entity_data` via `COMPOUND_TAG`, the `ItemLore` list,
//! and the `TypedEntityData` `entity_data`/`block_entity_data`) is canonicalized
//! structurally. The other component values — id-mapper VarInts
//! (`Rarity`/`DyeColor`/`MapPostProcessing`), `PotionContents`/`ItemEnchantments`
//! composites, ... — are not a bare NBT payload; a patch carrying one cannot be
//! bounded honestly by this harness and makes the whole patch (and the
//! advancement) fail canonicalization.
//!
//! RivetTodo(#221): this display canonicalizer is wired into the real
//! `rivet-capture` normalize path (`normalize::normalize_packet` for packet id
//! 130), but the pinned join fixture carries no advancement display payload, so
//! the real-boot `rivet-capture verify` does not exercise it; the display path
//! is proven only by synthetic display-bearing bodies in
//! `tools/rivet-capture/src/normalize.rs`. Replace this marker when a
//! display-bearing capture is pinned or the boot path grows one.

use crate::frame;

/// A parsed network NBT value (`writeAnyTag` format: `[byte type][payload]`,
/// root has no name; compound fields are named `[byte type][u16 len][name]`).
/// Compound fields are kept in parse order; re-serialization sorts them by name
/// so the wire form is canonical.
#[derive(Debug, Clone, PartialEq)]
enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List { elem: u8, items: Vec<Nbt> },
    Compound(Vec<(String, Nbt)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
    End,
}

/// Read a bare NBT payload of `type_byte` (no name prefix).
fn read_payload(body: &[u8], off: &mut usize, type_byte: u8) -> Option<Nbt> {
    match type_byte {
        0 => Some(Nbt::End),
        1 => {
            let v = *body.get(*off)? as i8;
            *off += 1;
            Some(Nbt::Byte(v))
        }
        2 => {
            let b = frame::read_bytes(body, off, 2)?;
            Some(Nbt::Short(i16::from_be_bytes([b[0], b[1]])))
        }
        3 => {
            let b = frame::read_bytes(body, off, 4)?;
            Some(Nbt::Int(i32::from_be_bytes([b[0], b[1], b[2], b[3]])))
        }
        4 => {
            let b = frame::read_bytes(body, off, 8)?;
            Some(Nbt::Long(i64::from_be_bytes([
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
            ])))
        }
        5 => {
            let b = frame::read_bytes(body, off, 4)?;
            Some(Nbt::Float(f32::from_be_bytes([b[0], b[1], b[2], b[3]])))
        }
        6 => {
            let b = frame::read_bytes(body, off, 8)?;
            Some(Nbt::Double(f64::from_be_bytes([
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
            ])))
        }
        7 => {
            let n = frame::read_i32(body, off)?;
            if n < 0 {
                return None; // Java: DecoderException on a negative array size
            }
            let bytes = frame::read_bytes(body, off, n as usize)?.to_vec();
            Some(Nbt::ByteArray(bytes))
        }
        8 => {
            let n = frame::read_u16(body, off)? as usize;
            let s = std::str::from_utf8(body.get(*off..*off + n)?)
                .ok()?
                .to_owned();
            *off += n;
            Some(Nbt::String(s))
        }
        9 => {
            let elem = *body.get(*off)?;
            *off += 1;
            let n = frame::read_i32(body, off)?;
            if n < 0 {
                return None; // Java: DecoderException on a negative list size
            }
            let mut items = Vec::with_capacity(n as usize);
            for _ in 0..n {
                items.push(read_payload(body, off, elem)?);
            }
            Some(Nbt::List { elem, items })
        }
        10 => {
            // Compound: `[field]*[type 0]`. A field whose payload fails to
            // parse (e.g. a negative ByteArray/List/IntArray/LongArray length,
            // which `read_payload` rejects) must fail the WHOLE compound, not
            // be conflated with the type-0 terminator. Java's `NbtIo` throws a
            // `DecoderException` for a negative array size; silently treating
            // such a compound as terminated would accept wire bytes Paper
            // rejects.
            let mut fields = Vec::new();
            loop {
                let type_byte = *body.get(*off)?;
                *off += 1;
                if type_byte == 0 {
                    break; // end tag
                }
                let name_len = frame::read_u16(body, off)? as usize;
                let name = std::str::from_utf8(body.get(*off..*off + name_len)?)
                    .ok()?
                    .to_owned();
                *off += name_len;
                let value = read_payload(body, off, type_byte)?;
                fields.push((name, value));
            }
            Some(Nbt::Compound(fields))
        }
        11 => {
            let n = frame::read_i32(body, off)?;
            if n < 0 {
                return None; // Java: DecoderException on a negative array size
            }
            let mut items = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let b = frame::read_bytes(body, off, 4)?;
                items.push(i32::from_be_bytes([b[0], b[1], b[2], b[3]]));
            }
            Some(Nbt::IntArray(items))
        }
        12 => {
            let n = frame::read_i32(body, off)?;
            if n < 0 {
                return None; // Java: DecoderException on a negative array size
            }
            let mut items = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let b = frame::read_bytes(body, off, 8)?;
                items.push(i64::from_be_bytes([
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                ]));
            }
            Some(Nbt::LongArray(items))
        }
        _ => None,
    }
}

fn nbt_type_id(v: &Nbt) -> u8 {
    match v {
        Nbt::End => 0,
        Nbt::Byte(_) => 1,
        Nbt::Short(_) => 2,
        Nbt::Int(_) => 3,
        Nbt::Long(_) => 4,
        Nbt::Float(_) => 5,
        Nbt::Double(_) => 6,
        Nbt::ByteArray(_) => 7,
        Nbt::String(_) => 8,
        Nbt::List { .. } => 9,
        Nbt::Compound(_) => 10,
        Nbt::IntArray(_) => 11,
        Nbt::LongArray(_) => 12,
    }
}

/// Write a named compound field (`[byte type][u16 name len][name][payload]`).
fn write_named_field(out: &mut Vec<u8>, name: &str, value: &Nbt) {
    out.push(nbt_type_id(value));
    out.extend_from_slice(&(name.len() as u16).to_be_bytes());
    out.extend_from_slice(name.as_bytes());
    write_payload(out, value);
}

fn write_payload(out: &mut Vec<u8>, value: &Nbt) {
    match value {
        Nbt::End => {}
        Nbt::Byte(v) => out.push(*v as u8),
        Nbt::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::Float(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::Double(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::ByteArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            out.extend_from_slice(v);
        }
        Nbt::String(v) => {
            out.extend_from_slice(&(v.len() as u16).to_be_bytes());
            out.extend_from_slice(v.as_bytes());
        }
        Nbt::List { elem, items } => {
            out.push(*elem);
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for item in items {
                write_payload(out, item);
            }
        }
        Nbt::Compound(fields) => {
            // Always emit fields in sorted order so the serialized form is
            // canonical no matter how the compound was constructed.
            let mut sorted = fields.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, value) in &sorted {
                write_named_field(out, name, value);
            }
            out.push(0);
        }
        Nbt::IntArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        Nbt::LongArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
    }
}

/// Read a root NBT value (`[byte type][payload]`, root un-named).
fn read_nbt(body: &[u8], off: &mut usize) -> Option<Nbt> {
    let type_byte = *body.get(*off)?;
    *off += 1;
    read_payload(body, off, type_byte)
}

/// Write a root NBT value (root un-named).
fn write_nbt(out: &mut Vec<u8>, value: &Nbt) {
    out.push(nbt_type_id(value));
    write_payload(out, value);
}

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

/// The pinned 26.2 component type name for a network registry id, restricted to
/// exactly the NBT-shaped components `read_component_value` can bound (a bare
/// NBT tag, an `ItemLore` NBT list, or a `TypedEntityData` type-id + NBT tag).
/// Every other component's network value — a scalar VarInt/Int, an id-mapper
/// enum, an `ItemEnchantments`/`PotionContents`/`ItemAttributeModifiers`
/// composite — is not NBT-shaped, so the harness cannot bound it honestly and
/// `None` refuses the whole patch rather than misparse.
fn component_type_name(id: u32) -> Option<&'static str> {
    match id {
        0 => Some("minecraft:custom_data"),
        6 => Some("minecraft:custom_name"),
        9 => Some("minecraft:item_name"),
        11 => Some("minecraft:lore"),
        58 => Some("minecraft:entity_data"),
        59 => Some("minecraft:bucket_entity_data"),
        60 => Some("minecraft:block_entity_data"),
        _ => None,
    }
}

/// Read one component value and return its canonical bytes, for the pinned
/// 26.2 components whose network value the harness can bound honestly.
/// Supported shapes (canonicalized, never fabricated):
///   - a single NBT tag: `custom_data`, `custom_name`, `item_name`,
///     `bucket_entity_data` — a `CustomData`/`Component` value
///     (`tagCodec`/`COMPOUND_TAG`), re-emitted as NBT;
///   - a list of NBT tags: `lore` — an `ItemLore` (`list(256)` of `tagCodec`);
///   - a typed NBT tag: `entity_data`, `block_entity_data` — a
///     `TypedEntityData` (`[type registry id VarInt][compound tag]`), with the
///     type id preserved verbatim.
///
/// Every other component value — a scalar VarInt/Int, an id-mapper enum, an
/// `ItemEnchantments`/`PotionContents`/`ItemAttributeModifiers` composite — is
/// NOT a bare NBT payload, and the harness cannot bound its length honestly, so
/// the whole patch is refused rather than misparsed or guessed at.
fn read_component_value(body: &[u8], off: &mut usize, name: &str) -> Option<Vec<u8>> {
    match name {
        "minecraft:custom_data"          // CustomData.STREAM_CODEC = COMPOUND_TAG
        | "minecraft:custom_name"        // ComponentSerialization.STREAM_CODEC = tagCodec
        | "minecraft:item_name"          // ComponentSerialization.STREAM_CODEC = tagCodec
        | "minecraft:bucket_entity_data" // CustomData.STREAM_CODEC = COMPOUND_TAG
        => {
            let value = read_nbt(body, off)?;
            let mut out = Vec::with_capacity(body.len() / 8);
            write_nbt(&mut out, &value);
            Some(out)
        }
        "minecraft:lore" => {
            // ItemLore.STREAM_CODEC = list(256) of tagCodec.
            let count = frame::read_varint(body, off)?;
            if count < 0 {
                return None;
            }
            let mut out = Vec::with_capacity(body.len() / 4);
            frame::write_varint(&mut out, count);
            for _ in 0..count {
                let line = read_nbt(body, off)?;
                write_nbt(&mut out, &line);
            }
            Some(out)
        }
        "minecraft:entity_data" | "minecraft:block_entity_data" => {
            // TypedEntityData.streamCodec = [type registry id VarInt][tag].
            let type_id = frame::read_varint(body, off)?;
            if type_id < 0 {
                return None;
            }
            let value = read_nbt(body, off)?;
            let mut out = Vec::with_capacity(body.len() / 4);
            frame::write_varint(&mut out, type_id);
            write_nbt(&mut out, &value);
            Some(out)
        }
        _ => None,
    }
}

/// One positive patch entry: the component type id plus the canonical value.
struct PatchEntry {
    type_id: u32,
    value: Vec<u8>,
}

/// Canonicalize a `DataComponentPatch` at `*off`. The caller passes the offset
/// at the very start of the patch value (before the counts); this function owns
/// and consumes the leading `positive` and `negative` count VarInts, then the
/// `positive` entries (`[type id VarInt][value]`) and `negative` entries
/// (`[type id VarInt]`), advancing `*off` past all of them. It returns the
/// canonical re-serialization with the positive entries sorted by type id and
/// the negatives sorted the same way.
fn canon_data_component_patch_body(body: &[u8], off: &mut usize) -> Option<Vec<u8>> {
    let positive = frame::read_varint(body, off)?;
    let negative = frame::read_varint(body, off)?;
    if positive < 0 || negative < 0 {
        return None;
    }
    let mut entries = Vec::with_capacity(positive as usize);
    for _ in 0..positive {
        let type_id = frame::read_varint(body, off)?;
        if type_id < 0 {
            return None;
        }
        let name = component_type_name(type_id as u32)?;
        let value = read_component_value(body, off, name)?;
        entries.push(PatchEntry {
            type_id: type_id as u32,
            value,
        });
    }
    let mut negatives = Vec::with_capacity(negative as usize);
    for _ in 0..negative {
        let type_id = frame::read_varint(body, off)?;
        if type_id < 0 {
            return None;
        }
        negatives.push(type_id as u32);
    }
    entries.sort_by_key(|e| e.type_id);
    negatives.sort_unstable();

    let mut out = Vec::with_capacity(body.len() / 2);
    frame::write_varint(&mut out, entries.len() as i32);
    frame::write_varint(&mut out, negatives.len() as i32);
    for e in &entries {
        frame::write_varint(&mut out, e.type_id as i32);
        out.extend_from_slice(&e.value);
    }
    for t in &negatives {
        frame::write_varint(&mut out, *t as i32);
    }
    Some(out)
}

/// Canonicalize a `DisplayInfo` value and return its canonical bytes:
/// `[title NBT][description NBT][icon][frame VarInt][flags int][bg?][x float][y float]`.
/// `icon` is an `ItemStackTemplate` = `[item VarInt][count VarInt][patch]`.
fn canon_display_info(body: &[u8], off: &mut usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(body.len() / 2);

    let title = read_nbt(body, off)?;
    write_nbt(&mut out, &title);
    let description = read_nbt(body, off)?;
    write_nbt(&mut out, &description);

    // icon: ItemStackTemplate = [item VarInt][count VarInt][DataComponentPatch].
    let item = frame::read_varint(body, off)?;
    let count = frame::read_varint(body, off)?;
    if item < 0 || count < 0 {
        return None;
    }
    frame::write_varint(&mut out, item);
    frame::write_varint(&mut out, count);
    let patch = canon_data_component_patch_body(body, off)?;
    out.extend_from_slice(&patch);

    let frame_v = frame::read_varint(body, off)?;
    if frame_v < 0 {
        return None;
    }
    frame::write_varint(&mut out, frame_v);
    let flags = frame::read_i32(body, off)?;
    out.extend_from_slice(&flags.to_be_bytes());
    if flags & 1 != 0 {
        let bg = read_string(body, off)?;
        write_string(&mut out, &bg);
    }
    let xy = frame::read_bytes(body, off, 8)?;
    out.extend_from_slice(xy);
    Some(out)
}

/// Canonicalize one advancement value at `*off` and return `(id, raw_bytes)`,
/// where `raw_bytes` is the canonical `[id][parent?][display?][requirements]`
/// `[telemetry]`. A display payload is structurally canonicalized; an absent
/// display (the pinned join fixture) is passed through with no display payload
/// fabricated or rewritten.
fn canon_advancement_value(body: &[u8], off: &mut usize) -> Option<(String, Vec<u8>)> {
    let id = read_string(body, off)?;
    let mut out = Vec::with_capacity(body.len() / 4);
    write_string(&mut out, &id);

    if *body.get(*off)? != 0 {
        *off += 1;
        let parent = read_string(body, off)?;
        out.push(1);
        write_string(&mut out, &parent);
    } else {
        *off += 1;
        out.push(0);
    }

    if *body.get(*off)? != 0 {
        *off += 1;
        let display = canon_display_info(body, off)?;
        out.push(1);
        out.extend_from_slice(&display);
    } else {
        *off += 1;
        out.push(0);
    }

    // requirements: [VarInt groups][group × ([VarInt names][names])] — inner
    // sets are order-insensitive, so re-emit them sorted (as before #221).
    let group_count = frame::read_varint(body, off)?;
    if group_count < 0 {
        return None;
    }
    let mut groups: Vec<Vec<String>> = Vec::with_capacity(group_count as usize);
    for _ in 0..group_count {
        let name_count = frame::read_varint(body, off)?;
        if name_count < 0 {
            return None;
        }
        let mut names = Vec::with_capacity(name_count as usize);
        for _ in 0..name_count {
            names.push(read_string(body, off)?);
        }
        names.sort();
        groups.push(names);
    }
    let telemetry = *body.get(*off)?;
    *off += 1;

    frame::write_varint(&mut out, groups.len() as i32);
    for names in &groups {
        frame::write_varint(&mut out, names.len() as i32);
        for name in names {
            write_string(&mut out, name);
        }
    }
    out.push(telemetry);
    Some((id, out))
}

/// Canonicalize an `update_advancements` (130) body: sort the added list, the
/// removed set, and the progress map (all HashMap/HashSet-backed per boot) by
/// identifier; sort each progress's criteria by criterion name; and
/// structurally canonicalize each advancement's display payload (NBT compound
/// field order + `DataComponentPatch` entry order). Obtained instants are
/// wall-clock per boot, so they are zeroed like the existing canonicalizer.
///
/// Returns `None` when the body does not parse or a display payload cannot be
/// bounded; the caller then keeps the raw body (honest non-canonicalization).
pub fn canon_update_advancements(body: &[u8]) -> Option<Vec<u8>> {
    let mut off = 0;
    let reset = *body.get(off)?;
    off += 1;

    let added_count = frame::read_varint(body, &mut off)?;
    if added_count < 0 {
        return None;
    }
    let mut added = Vec::with_capacity(added_count as usize);
    for _ in 0..added_count {
        let (id, raw) = canon_advancement_value(body, &mut off)?;
        added.push((id, raw));
    }
    added.sort_by(|a, b| a.0.cmp(&b.0));

    let removed_count = frame::read_varint(body, &mut off)?;
    if removed_count < 0 {
        return None;
    }
    let mut removed = Vec::with_capacity(removed_count as usize);
    for _ in 0..removed_count {
        let start = off;
        let id = read_string(body, &mut off)?;
        removed.push((id, body[start..off].to_vec()));
    }
    removed.sort_by(|a, b| a.0.cmp(&b.0));

    let progress_count = frame::read_varint(body, &mut off)?;
    if progress_count < 0 {
        return None;
    }
    let mut progress = Vec::with_capacity(progress_count as usize);
    for _ in 0..progress_count {
        let id = read_string(body, &mut off)?;
        // AdvancementProgress: [VarInt criteria][criteria × ([String][bool][?Instant])].
        let crit_count = frame::read_varint(body, &mut off)?;
        if crit_count < 0 {
            return None;
        }
        let mut criteria = Vec::with_capacity(crit_count as usize);
        for _ in 0..crit_count {
            let name = read_string(body, &mut off)?;
            let obtained = *body.get(off)?;
            off += 1;
            if obtained != 0 {
                frame::read_bytes(body, &mut off, 8)?;
            }
            let mut raw = Vec::with_capacity(name.len() + 16);
            write_string(&mut raw, &name);
            raw.push(obtained);
            if obtained != 0 {
                raw.extend_from_slice(&0i64.to_be_bytes()); // obtained instant -> 0
            }
            criteria.push((name, raw));
        }
        criteria.sort_by(|a, b| a.0.cmp(&b.0));
        let mut prog = Vec::with_capacity(crit_count as usize * 4 + id.len() + 4);
        write_string(&mut prog, &id);
        frame::write_varint(&mut prog, criteria.len() as i32);
        for (_, raw) in &criteria {
            prog.extend_from_slice(raw);
        }
        progress.push((id, prog));
    }
    progress.sort_by(|a, b| a.0.cmp(&b.0));

    let show = *body.get(off)?;
    if off + 1 != body.len() {
        return None;
    }

    let mut out = Vec::with_capacity(body.len());
    out.push(reset);
    frame::write_varint(&mut out, added.len() as i32);
    for (_, raw) in &added {
        out.extend_from_slice(raw);
    }
    frame::write_varint(&mut out, removed.len() as i32);
    for (_, raw) in &removed {
        out.extend_from_slice(raw);
    }
    frame::write_varint(&mut out, progress.len() as i32);
    for (_, raw) in &progress {
        out.extend_from_slice(raw);
    }
    out.push(show);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The id of the first added advancement whose canonical form does not match
    /// the baseline's, or `None` when every advancement canonicalizes identically.
    /// Test-only helper: names the affected advancement when a negative test
    /// asserts that a semantic mutation is detected.
    fn first_advancement_mismatch(body: &[u8], baseline: &[u8]) -> Option<String> {
        let added = canonical_added(body)?;
        let baseline_added = canonical_added(baseline)?;
        for (id, raw) in &added {
            if baseline_added.get(id) != Some(raw) {
                return Some(id.clone());
            }
        }
        None
    }

    /// The `added` advancements of a body as `id -> canonical bytes`, sorted by id.
    fn canonical_added(body: &[u8]) -> Option<std::collections::HashMap<String, Vec<u8>>> {
        let mut off = 0;
        let _reset = *body.get(off)?;
        off += 1;
        let added_count = frame::read_varint(body, &mut off)?;
        if added_count < 0 {
            return None;
        }
        let mut added = std::collections::HashMap::with_capacity(added_count as usize);
        for _ in 0..added_count {
            let (id, raw) = canon_advancement_value(body, &mut off)?;
            added.insert(id, raw);
        }
        Some(added)
    }

    // -- wire builders (Paper-faithful: [bool] optional/display, VarInt counts,
    //    VarInt-prefixed UTF-8 identifiers, big-endian primitives) -------------

    fn nbt_str(s: &str) -> Nbt {
        Nbt::String(s.to_owned())
    }

    fn nbt_compound(fields: Vec<(&str, Nbt)>) -> Nbt {
        Nbt::Compound(fields.into_iter().map(|(n, v)| (n.to_owned(), v)).collect())
    }

    fn nbt_bytes(v: &Nbt) -> Vec<u8> {
        let mut out = Vec::new();
        write_nbt(&mut out, v);
        out
    }

    /// Raw NBT compound bytes with the fields encoded in the given order —
    /// bypasses the `write_payload` on-write sort so a test can feed a
    /// genuinely unsorted compound to the canonicalizer.
    fn raw_compound(fields: &[(&str, u8, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(10); // compound tag
        for (name, type_id, payload) in fields {
            out.push(*type_id);
            out.extend_from_slice(&(name.len() as u16).to_be_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(payload);
        }
        out.push(0); // end tag
        out
    }

    /// The `[u16 len][chars]` payload of an NBT string tag (no type byte).
    fn raw_str(s: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(s.len() as u16).to_be_bytes());
        out.extend_from_slice(s.as_bytes());
        out
    }

    fn patch(pos: &[(u32, Vec<u8>)], neg: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        frame::write_varint(&mut out, pos.len() as i32);
        frame::write_varint(&mut out, neg.len() as i32);
        for (id, value) in pos {
            frame::write_varint(&mut out, *id as i32);
            out.extend_from_slice(value);
        }
        for id in neg {
            frame::write_varint(&mut out, *id as i32);
        }
        out
    }

    fn icon(item: i32, count: i32, components: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        frame::write_varint(&mut out, item);
        frame::write_varint(&mut out, count);
        out.extend_from_slice(components);
        out
    }

    fn display(
        title: &Nbt,
        desc: &Nbt,
        icon: &[u8],
        frame_v: i32,
        flags: i32,
        position: (f32, f32),
        bg: Option<&str>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        write_nbt(&mut out, title);
        write_nbt(&mut out, desc);
        out.extend_from_slice(icon);
        frame::write_varint(&mut out, frame_v);
        out.extend_from_slice(&flags.to_be_bytes());
        if let Some(bg) = bg {
            write_string(&mut out, bg);
        }
        out.extend_from_slice(&position.0.to_be_bytes());
        out.extend_from_slice(&position.1.to_be_bytes());
        out
    }

    fn advancement(
        id: &str,
        parent: Option<&str>,
        display: Option<&[u8]>,
        groups: &[Vec<String>],
        telemetry: bool,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        write_string(&mut out, id);
        match parent {
            Some(p) => {
                out.push(1);
                write_string(&mut out, p);
            }
            None => out.push(0),
        }
        match display {
            Some(d) => {
                out.push(1);
                out.extend_from_slice(d);
            }
            None => out.push(0),
        }
        frame::write_varint(&mut out, groups.len() as i32);
        for g in groups {
            frame::write_varint(&mut out, g.len() as i32);
            for name in g {
                write_string(&mut out, name);
            }
        }
        out.push(telemetry as u8);
        out
    }

    fn progress(id: &str, criteria: &[(&str, bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        write_string(&mut out, id);
        frame::write_varint(&mut out, criteria.len() as i32);
        for (name, obtained) in criteria {
            write_string(&mut out, name);
            out.push(*obtained as u8);
            if *obtained {
                out.extend_from_slice(&42i64.to_be_bytes());
            }
        }
        out
    }

    fn body(
        reset: bool,
        added: &[Vec<u8>],
        removed: &[&str],
        progress: &[Vec<u8>],
        show: bool,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(reset as u8);
        frame::write_varint(&mut out, added.len() as i32);
        for a in added {
            out.extend_from_slice(a);
        }
        frame::write_varint(&mut out, removed.len() as i32);
        for r in removed {
            write_string(&mut out, r);
        }
        frame::write_varint(&mut out, progress.len() as i32);
        for p in progress {
            out.extend_from_slice(p);
        }
        out.push(show as u8);
        out
    }

    // -- fixtures -------------------------------------------------------------

    /// A custom_name component value: a bare NBT tag (id 6).
    fn custom_name(s: &str) -> (u32, Vec<u8>) {
        (6, nbt_bytes(&nbt_compound(vec![("text", nbt_str(s))])))
    }

    /// A custom_data component value: a compound NBT tag (id 0).
    fn custom_data(s: &str) -> (u32, Vec<u8>) {
        (0, nbt_bytes(&nbt_compound(vec![("extra", nbt_str(s))])))
    }

    /// A lore component value: `[VarInt count][count × NBT]` (id 11).
    fn lore(lines: &[&str]) -> (u32, Vec<u8>) {
        let mut out = Vec::new();
        frame::write_varint(&mut out, lines.len() as i32);
        for l in lines {
            let mut tag = Vec::new();
            write_nbt(&mut tag, &nbt_compound(vec![("text", nbt_str(l))]));
            out.extend_from_slice(&tag);
        }
        (11, out)
    }

    /// An entity_data component value: `[type registry id VarInt][compound NBT]`
    /// (id 58).
    fn entity_data(type_id: i32, tag: &Nbt) -> (u32, Vec<u8>) {
        let mut out = Vec::new();
        frame::write_varint(&mut out, type_id);
        write_nbt(&mut out, tag);
        (58, out)
    }

    /// Like `display`, but takes the title/description as PRE-ENCODED raw NBT
    /// wire bytes (e.g. `raw_compound` output). This bypasses `write_nbt`'s
    /// on-write compound-field sort so a test can feed a genuinely unsorted
    /// compound directly to the canonicalizer, independent of the writer.
    fn display_raw(
        title: &[u8],
        desc: &[u8],
        icon: &[u8],
        frame_v: i32,
        flags: i32,
        position: (f32, f32),
        bg: Option<&str>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(title);
        out.extend_from_slice(desc);
        out.extend_from_slice(icon);
        frame::write_varint(&mut out, frame_v);
        out.extend_from_slice(&flags.to_be_bytes());
        if let Some(bg) = bg {
            write_string(&mut out, bg);
        }
        out.extend_from_slice(&position.0.to_be_bytes());
        out.extend_from_slice(&position.1.to_be_bytes());
        out
    }

    /// Like `display_for`, but for pre-encoded raw title/description bytes.
    fn display_for_raw(
        title: &[u8],
        desc: &[u8],
        components: &[(u32, Vec<u8>)],
        negatives: &[u32],
        order: &[usize],
    ) -> Vec<u8> {
        let mut entries = Vec::new();
        for &i in order {
            entries.push(components[i].clone());
        }
        display_raw(
            title,
            desc,
            &icon(926, 1, &patch(&entries, negatives)),
            0,
            0,
            (0.5, -1.25),
            None,
        )
    }

    fn display_for(
        title: &Nbt,
        desc: &Nbt,
        components: &[(u32, Vec<u8>)],
        negatives: &[u32],
        order: &[usize],
    ) -> Vec<u8> {
        let mut entries = Vec::new();
        for &i in order {
            entries.push(components[i].clone());
        }
        display(
            title,
            desc,
            &icon(926, 1, &patch(&entries, negatives)),
            0,
            0,
            (0.5, -1.25),
            None,
        )
    }

    // -- tests ----------------------------------------------------------------

    #[test]
    fn reordered_equivalent_displays_canonicalize_identically() {
        // The two bodies carry the SAME semantic display payloads, encoded with
        // genuinely different wire byte orders in FOUR independent dimensions:
        //   - the title/description NBT compound field order (unsorted
        //     [text,color] vs the canonical [color,text], built with
        //     `raw_compound` so the writer's on-write sort cannot mask it);
        //   - the DataComponentPatch positive-entry order;
        //   - the added-list order;
        //   - the progress-criteria order.
        // Every variation must collapse to one canonical byte string. This is
        // non-vacuous for the compound sort: without `write_payload` sorting
        // compound fields on emit, body_a's story:first would keep its
        // unsorted [text,color] wire bytes while body_b's would keep
        // [color,text], and the two canonical forms would differ.
        let title_unsorted =
            raw_compound(&[("text", 8, raw_str("A")), ("color", 8, raw_str("red"))]);
        let title_sorted = raw_compound(&[("color", 8, raw_str("red")), ("text", 8, raw_str("A"))]);
        let desc_unsorted =
            raw_compound(&[("text", 8, raw_str("D")), ("color", 8, raw_str("green"))]);
        let desc_sorted =
            raw_compound(&[("color", 8, raw_str("green")), ("text", 8, raw_str("D"))]);
        let zombie = nbt_compound(vec![("id", nbt_str("zombie"))]);

        // Shared component set: custom_name(6), custom_data(0), lore(11),
        // entity_data(58). All four are NBT-shaped and must canonicalize by
        // component type id regardless of entry order.
        let components = &[
            custom_name("x"),
            custom_data("y"),
            lore(&["L1", "L2"]),
            entity_data(7, &zombie),
        ];
        let negatives = &[3u32, 11, 58];

        // body_a: story:first carries the UNSORTED title/desc compounds,
        // story:second the sorted ones. body_b swaps which advancement gets the
        // unsorted wire bytes, and additionally scrambles the patch-entry order.
        let body_a = body(
            false,
            &[
                advancement(
                    "story:second",
                    None,
                    Some(&display_for_raw(
                        &title_sorted,
                        &desc_sorted,
                        components,
                        negatives,
                        &[0, 1, 2, 3],
                    )),
                    &[vec!["a".into()]],
                    false,
                ),
                advancement(
                    "story:first",
                    None,
                    Some(&display_for_raw(
                        &title_unsorted,
                        &desc_unsorted,
                        components,
                        negatives,
                        &[0, 1, 2, 3],
                    )),
                    &[vec!["b".into()]],
                    false,
                ),
            ],
            &["story:zz"],
            &[progress(
                "story:first",
                &[("crit_b", true), ("crit_a", false)],
            )],
            true,
        );
        let body_b = body(
            false,
            &[
                advancement(
                    "story:first",
                    None,
                    Some(&display_for_raw(
                        &title_sorted,
                        &desc_sorted,
                        components,
                        negatives,
                        &[3, 2, 1, 0],
                    )),
                    &[vec!["b".into()]],
                    false,
                ),
                advancement(
                    "story:second",
                    None,
                    Some(&display_for_raw(
                        &title_unsorted,
                        &desc_unsorted,
                        components,
                        negatives,
                        &[2, 0, 3, 1],
                    )),
                    &[vec!["a".into()]],
                    false,
                ),
            ],
            &["story:zz"],
            &[progress(
                "story:first",
                &[("crit_a", false), ("crit_b", true)],
            )],
            true,
        );

        let canon_a = canon_update_advancements(&body_a).expect("body_a canonicalizes");
        let canon_b = canon_update_advancements(&body_b).expect("body_b canonicalizes");
        assert_eq!(
            canon_a, canon_b,
            "reordered-equivalent displays must canonicalize identically"
        );
        // Idempotent: canonicalizing the canonical form is a no-op.
        assert_eq!(canon_update_advancements(&canon_a), Some(canon_a.clone()));
        assert_eq!(canon_update_advancements(&canon_b), Some(canon_b.clone()));
    }

    #[test]
    fn unsorted_compound_fields_canonicalize_to_sorted_bytes() {
        // #221 headline: a Component serializes to an NBT compound whose field
        // order depends on the DFU record map iteration, so a capture can carry
        // the fields in any order. The `write_payload` writer sorts on emit, so
        // the `nbt_compound` builders cannot produce an unsorted compound —
        // hand-craft the raw wire bytes instead and assert the canonical form
        // is the sorted order (not a pass-through of the parse order). Without
        // the compound-field sort a regression would emit the unsorted input
        // verbatim and fail this assertion.
        let title_unsorted = raw_compound(&[
            ("text", 8, raw_str("A")),
            ("italic", 1, vec![1]),
            ("color", 8, raw_str("red")),
        ]);
        let title_sorted = raw_compound(&[
            ("color", 8, raw_str("red")),
            ("italic", 1, vec![1]),
            ("text", 8, raw_str("A")),
        ]);
        let desc_unsorted =
            raw_compound(&[("text", 8, raw_str("D")), ("color", 8, raw_str("green"))]);
        let desc_sorted =
            raw_compound(&[("color", 8, raw_str("green")), ("text", 8, raw_str("D"))]);

        // DisplayInfo with the title/description compounds in unsorted field
        // order on the wire: [title][description][icon item=0 count=0 patch
        // (pos=0,neg=0)][frame 0][flags 0][x y floats].
        let mut display = Vec::new();
        display.extend_from_slice(&title_unsorted);
        display.extend_from_slice(&desc_unsorted);
        display.extend_from_slice(&[0, 0, 0, 0]); // item, count, patch(0, 0)
        display.extend_from_slice(&[0]); // frame
        display.extend_from_slice(&0i32.to_be_bytes()); // flags
        display.extend_from_slice(&0.5f32.to_be_bytes());
        display.extend_from_slice(&(-1.25f32).to_be_bytes());

        let mut off = 0;
        let canon = canon_display_info(&display, &mut off)
            .expect("display with unsorted compounds canonicalizes");
        assert_eq!(off, display.len(), "whole display consumed");

        // Title and description must be re-emitted with fields sorted by name,
        // with every non-compound field passed through verbatim.
        let mut expected = Vec::new();
        expected.extend_from_slice(&title_sorted);
        expected.extend_from_slice(&desc_sorted);
        expected.extend_from_slice(&[0, 0, 0, 0]);
        expected.extend_from_slice(&[0]);
        expected.extend_from_slice(&0i32.to_be_bytes());
        expected.extend_from_slice(&0.5f32.to_be_bytes());
        expected.extend_from_slice(&(-1.25f32).to_be_bytes());
        assert_eq!(canon, expected);
    }

    #[test]
    fn semantic_mutation_is_detected_and_names_advancement() {
        let title = nbt_compound(vec![("text", nbt_str("Original"))]);
        let adv = advancement(
            "story:root",
            None,
            Some(&display_for(&title, &title, &[custom_name("x")], &[], &[0])),
            &[vec!["c".into()]],
            false,
        );
        let base = body(false, std::slice::from_ref(&adv), &[], &[], true);

        // Changed title text (a real semantic difference).
        let mutated_title = nbt_compound(vec![("text", nbt_str("Mutated"))]);
        let mutated = advancement(
            "story:root",
            None,
            Some(&display_for(
                &mutated_title,
                &title,
                &[custom_name("x")],
                &[],
                &[0],
            )),
            &[vec!["c".into()]],
            false,
        );
        let mutated_body = body(false, &[mutated], &[], &[], true);
        assert_eq!(
            first_advancement_mismatch(&base, &mutated_body),
            Some("story:root".to_owned())
        );

        // The mutated body must NOT canonicalize to the baseline.
        assert_ne!(
            canon_update_advancements(&base),
            canon_update_advancements(&mutated_body)
        );
    }

    #[test]
    fn no_display_body_is_honest_and_idempotent() {
        // Like the pinned join fixture, this body carries no advancement display
        // data (display byte 0). The body is built already in canonical order
        // (added/progress lists and criteria sorted, no obtained instants to
        // zero), so canonicalization is a byte-for-byte identity — no fabricated
        // display payload is invented. Note this holds only because every
        // criterion is NOT obtained: a fresh join's obtained criteria carry a
        // wall-clock instant that canonicalizes to 0, so a no-display body with
        // obtained=true would NOT be byte-identical (see
        // `progress_obtained_instants_are_zeroed`). This test deliberately uses
        // obtained=false to isolate the display-honesty claim.
        let adv = advancement("story:root", None, None, &[vec!["c".into()]], false);
        let input = body(
            true,
            std::slice::from_ref(&adv),
            &[],
            &[progress("story:root", &[("c", false)])],
            true,
        );
        let canon = canon_update_advancements(&input).expect("no-display body canonicalizes");
        assert_eq!(
            canon, input,
            "no-display body (all criteria un-obtained) must pass through byte-identically"
        );
        assert_eq!(canon_update_advancements(&canon), Some(canon.clone()));
    }

    #[test]
    fn unknown_component_value_fails_canonicalization_honestly() {
        // item_model (id 10) is an Identifier, not an NBT payload; the harness
        // cannot bound its value, so the whole canonicalization is refused
        // (None) rather than misparsed or guessed at.
        let title = nbt_compound(vec![("text", nbt_str("T"))]);
        let unknown = (10u32, vec![0x0A, 0x01, 0x02]); // arbitrary bytes after the type id
        let adv = advancement(
            "story:root",
            None,
            Some(&display_for(&title, &title, &[unknown], &[], &[0])),
            &[vec!["c".into()]],
            false,
        );
        let input = body(false, &[adv], &[], &[], true);
        assert_eq!(canon_update_advancements(&input), None);
    }

    #[test]
    fn progress_obtained_instants_are_zeroed() {
        // The pre-existing canonicalizer behavior: obtained instants are
        // wall-clock per boot, so they are zeroed. Two bodies that differ only
        // in the instant value must canonicalize identically.
        let adv = advancement("story:root", None, None, &[vec!["c".into()]], false);
        let instant_a = progress("story:root", &[("c", true)]);
        let mut instant_b = progress("story:root", &[("c", true)]);
        // The entry is [id][count][name][obtained=1][8-byte long]; the long is
        // the final 8 bytes. Flip only its least-significant byte so the two
        // bodies differ solely in the obtained instant.
        let len = instant_b.len();
        instant_b[len - 1] ^= 0x01;
        let body_a = body(false, std::slice::from_ref(&adv), &[], &[instant_a], true);
        let body_b = body(false, std::slice::from_ref(&adv), &[], &[instant_b], true);
        assert_ne!(body_a, body_b);
        assert_eq!(
            canon_update_advancements(&body_a),
            canon_update_advancements(&body_b)
        );
    }

    /// A raw `[type byte][payload]` root NBT value with a NEGATIVE payload
    /// length for `type_byte`. `length` is written big-endian as the tag's
    /// count field (i32 for arrays/list, u16 for a string).
    fn raw_negative_length(type_byte: u8, length: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(type_byte);
        out.extend_from_slice(&length.to_be_bytes());
        out
    }

    /// Build a `DisplayInfo` whose title is the given raw NBT bytes, with an
    /// otherwise-valid icon/frame/flags/position, so a title that refuses to
    /// parse makes `canon_display_info` return `None`.
    fn display_with_title(title: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(title);
        // description: a valid empty compound.
        out.push(10);
        out.push(0);
        // icon: item 0, count 1, patch (pos=0, neg=0).
        frame::write_varint(&mut out, 0);
        frame::write_varint(&mut out, 1);
        frame::write_varint(&mut out, 0);
        frame::write_varint(&mut out, 0);
        frame::write_varint(&mut out, 0); // frame
        out.extend_from_slice(&0i32.to_be_bytes()); // flags
        out.extend_from_slice(&0.5f32.to_be_bytes());
        out.extend_from_slice(&(-1.25f32).to_be_bytes());
        out
    }

    #[test]
    fn negative_nbt_array_list_lengths_are_rejected() {
        // Java's `NbtIo` throws a DecoderException for a negative array/list
        // size. The canonicalizer must refuse the payload (None), not coerce it
        // to zero and emit a fabricated empty array.
        for type_byte in [
            7u8, /* ByteArray */
            11,  /* IntArray */
            12,  /* LongArray */
        ] {
            let display = display_with_title(&raw_negative_length(type_byte, -1));
            let mut off = 0;
            assert_eq!(
                canon_display_info(&display, &mut off),
                None,
                "negative {type_byte} array length must fail canonicalization"
            );
        }

        // List (type 9): [elem type byte][count i32][elems].
        let mut list = Vec::new();
        list.push(9);
        list.push(8); // elem: string
        list.extend_from_slice(&(-1i32).to_be_bytes()); // count -1
        let display = display_with_title(&list);
        let mut off = 0;
        assert_eq!(
            canon_display_info(&display, &mut off),
            None,
            "negative list length must fail canonicalization"
        );
    }

    #[test]
    fn negative_length_inside_compound_is_not_a_terminator() {
        // A negative ByteArray length nested INSIDE a compound field must fail
        // the whole compound — the pre-#221 parser conflated a failed field
        // with the type-0 end tag and would have silently terminated the
        // compound, accepting wire bytes Java rejects.
        let mut title = Vec::new();
        title.push(10); // compound
        title.push(7); // field type: ByteArray
        title.extend_from_slice(&2u16.to_be_bytes()); // name len
        title.extend_from_slice(b"ab"); // name
        title.extend_from_slice(&(-1i32).to_be_bytes()); // negative length
        title.push(0); // would-be end tag
        let display = display_with_title(&title);
        let mut off = 0;
        assert_eq!(
            canon_display_info(&display, &mut off),
            None,
            "a negative array length inside a compound must fail the whole compound"
        );
    }

    #[test]
    fn background_identifier_branch_is_covered() {
        // DisplayInfo flags bit 0 (background present): the wire carries a
        // VarInt-prefixed identifier string after the flags int, before the
        // position floats. The canonical form must preserve the identifier
        // verbatim and consume the whole display.
        let title = nbt_compound(vec![("text", nbt_str("T"))]);
        let desc = nbt_compound(vec![("text", nbt_str("D"))]);
        let mut display = Vec::new();
        write_nbt(&mut display, &title);
        write_nbt(&mut display, &desc);
        frame::write_varint(&mut display, 0); // icon item
        frame::write_varint(&mut display, 1); // icon count
        frame::write_varint(&mut display, 0); // patch positive
        frame::write_varint(&mut display, 0); // patch negative
        frame::write_varint(&mut display, 0); // frame
        display.extend_from_slice(&1i32.to_be_bytes()); // flags: background present
        write_string(
            &mut display,
            "minecraft:textures/gui/advancements/backgrounds/adventure.png",
        );
        display.extend_from_slice(&0.5f32.to_be_bytes());
        display.extend_from_slice(&(-1.25f32).to_be_bytes());

        let mut off = 0;
        let canon = canon_display_info(&display, &mut off).expect("display canonicalizes");
        assert_eq!(off, display.len(), "whole display consumed");
        // Byte-identical to the input: everything here is already canonical
        // (single-field compounds, sorted patch, identifier passed verbatim).
        assert_eq!(canon, display);
    }
}
