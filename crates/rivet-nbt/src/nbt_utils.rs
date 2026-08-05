//! Port of `net.minecraft.nbt.NbtUtils` — `public final class` of static NBT
//! helpers. Owned by manifest unit mc.nbt.utils.
//!
//! Ported fully: `compareNbt` (deep equality with partial-list matching),
//! `prettyPrint` (with binary blobs), `structureToSnbt` / `snbtToStructure` /
//! `packStructureTemplate` / `unpackStructureTemplate` / `packBlockState` /
//! `unpackBlockState`, `getDataVersion` / `addDataVersion` /
//! `addCurrentDataVersion` (CompoundTag + Dynamic variants), `toPrettyComponent`,
//! `SNBT_DATA_TAG`.
//!
//! Error-type divergence: Java `snbtToStructure` throws brigadier
//! `CommandSyntaxException` (from `TagParser.parseCompoundFully`); here it
//! returns `Result<CompoundTag, NbtFormatException>`. rivet-nbt has no
//! brigadier dependency and approximates the exception with
//! `NbtFormatException` carrying the full message (see `tag_parser`). Valid
//! input behaves identically.
//!
//! Stubbed (world/registry parts, see markers): `readBlockState`,
//! `writeBlockState`, `writeFluidState`, and the `ValueOutput` overloads of
//! `addDataVersion` / `addCurrentDataVersion`.

use crate::compound_tag::CompoundTag;
use crate::list_tag::ListTag;
use crate::nbt_ops::NbtOps;
use crate::string_tag::StringTag;
use crate::tag::Tag;
use crate::text_component_tag_visitor::TextComponentTagVisitor;
use rivet_serialization::Dynamic;
use std::cmp::Ordering;
use std::collections::HashMap;

/// `NbtUtils.SNBT_DATA_TAG`.
pub const SNBT_DATA_TAG: &str = "data";
/// `NbtUtils.PROPERTIES_START` — `'{'`.
const PROPERTIES_START: char = '{';
/// `NbtUtils.PROPERTIES_END` — `'}'`.
const PROPERTIES_END: char = '}';
/// `NbtUtils.ELEMENT_SEPARATOR` — `","`.
const ELEMENT_SEPARATOR: &str = ",";
/// `NbtUtils.KEY_VALUE_SEPARATOR` — `':'`.
const KEY_VALUE_SEPARATOR: char = ':';
/// `NbtUtils.INDENT` — spaces per indent level in `prettyPrint`.
const INDENT: i32 = 2;
/// `NbtUtils.NOT_FOUND` — not referenced by the current Java source (kept for
/// greppability against `NbtUtils.java`).
#[allow(dead_code)]
const NOT_FOUND: i32 = -1;

/// STUB(mc.nbt.utils) — `SharedConstants.getCurrentVersion().dataVersion()
/// .version()` — 4903 in the pinned MC 26.2 build (`DetectedVersion`:
/// `new DataVersion(4903, "main")`, `SharedConstants.WORLD_VERSION = 4903`).
/// Stopgap local constant duplicating `SharedConstants.WORLD_VERSION` (rivet-core);
/// rewired when that lands. Tracked here because it silently drifts if the
/// pinned MC version changes: both this constant and the
/// `add_current_data_version_uses_world_version` test hardcode 4903, and the
/// oracle harness is not wired to this unit yet.
pub const CURRENT_DATA_VERSION: i32 = 4903;

/// `YXZ_LISTTAG_INT_COMPARATOR` — compare by y(1), then x(0), then z(2), each
/// via `getIntOr(index, 0)`.
fn yxz_listtag_int_cmp(a: &ListTag, b: &ListTag) -> Ordering {
    a.get_int_or(1, 0)
        .cmp(&b.get_int_or(1, 0))
        .then(a.get_int_or(0, 0).cmp(&b.get_int_or(0, 0)))
        .then(a.get_int_or(2, 0).cmp(&b.get_int_or(2, 0)))
}

/// `YXZ_LISTTAG_DOUBLE_COMPARATOR` — compare by y(1), then x(0), then z(2),
/// each via `getDoubleOr(index, 0.0)` using `Double.compare` semantics.
fn yxz_listtag_double_cmp(a: &ListTag, b: &ListTag) -> Ordering {
    java_double_compare(a.get_double_or(1, 0.0), b.get_double_or(1, 0.0))
        .then_with(|| java_double_compare(a.get_double_or(0, 0.0), b.get_double_or(0, 0.0)))
        .then_with(|| java_double_compare(a.get_double_or(2, 0.0), b.get_double_or(2, 0.0)))
}

/// `Double.compare(a, b)` — NaN sorts greater than everything, `-0.0 < 0.0`.
fn java_double_compare(a: f64, b: f64) -> Ordering {
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else if a == b {
        if a == 0.0 {
            match (a.is_sign_negative(), b.is_sign_negative()) {
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => Ordering::Equal,
            }
        } else {
            Ordering::Equal
        }
    } else if a.is_nan() {
        if b.is_nan() {
            Ordering::Equal
        } else {
            Ordering::Greater
        }
    } else {
        Ordering::Less
    }
}

/// Guava `Comparators.emptiesLast(valueComparator)` over the `Optional<ListTag>`
/// that `tag.getList("pos")` yields: `Optional.empty()` (a missing `pos` key,
/// mapped here to `None`) sorts last; a present list — even an empty one —
/// compares its contents with the base comparator. (Verified against
/// guava-33.6.0: `emptiesLast` returns `Comparator<Optional<T>>` built on
/// `nullsLast(valueComparator)`.)
fn pos_list_cmp(
    a: &Option<&ListTag>,
    b: &Option<&ListTag>,
    base: fn(&ListTag, &ListTag) -> Ordering,
) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => base(x, y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// `String.compareTo` order — UTF-16 code units (code units are unsigned; a
/// prefix sorts before a longer string).
fn utf16_cmp(a: &str, b: &str) -> Ordering {
    let mut au = a.encode_utf16();
    let mut bu = b.encode_utf16();
    loop {
        match (au.next(), bu.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => match x.cmp(&y) {
                Ordering::Equal => continue,
                o => return o,
            },
        }
    }
}

/// `String.length()` — UTF-16 code units.
fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Java `builder.length() - builder.lastIndexOf("\n")` — the UTF-16 column of
/// the last newline measured from the start of the builder (the newline itself
/// counts; a builder with no newline yields `length() + 1`).
fn column_since_last_newline_java(builder: &str) -> i32 {
    match builder.rfind('\n') {
        Some(i) => builder[i..].encode_utf16().count() as i32,
        None => builder.encode_utf16().count() as i32 + 1,
    }
}

/// `NbtUtils.compareNbt(Tag, Tag, boolean)` — deep equality where `expected`
/// may be a subset of `actual`. `None` (Java `null`) is treated as
/// "no expectation".
pub fn compare_nbt(
    expected: Option<&Tag>,
    actual: Option<&Tag>,
    partial_list_matches: bool,
) -> bool {
    // Java `expected == actual` is reference identity; value equality is a
    // conservative superset (equal values also deep-compare equal).
    if expected == actual {
        return true;
    }
    let expected = match expected {
        Some(t) => t,
        None => return true,
    };
    let actual = match actual {
        Some(t) => t,
        None => return false,
    };
    if std::mem::discriminant(expected) != std::mem::discriminant(actual) {
        return false;
    }
    match expected {
        Tag::Compound(expected_compound) => {
            let actual_compound = match actual {
                Tag::Compound(c) => c,
                _ => unreachable!("discriminants already checked equal"),
            };
            if actual_compound.size() < expected_compound.size() {
                return false;
            }
            for (key, tag) in expected_compound.entry_set() {
                if !compare_nbt(Some(tag), actual_compound.get(key), partial_list_matches) {
                    return false;
                }
            }
            true
        }
        Tag::List(expected_list) if partial_list_matches => {
            let actual_list = match actual {
                Tag::List(l) => l,
                _ => unreachable!("discriminants already checked equal"),
            };
            if expected_list.is_empty() {
                return actual_list.is_empty();
            }
            if actual_list.size() < expected_list.size() {
                return false;
            }
            for tag in expected_list.iter() {
                let mut found = false;
                for value in actual_list.iter() {
                    if compare_nbt(Some(tag), Some(value), partial_list_matches) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return false;
                }
            }
            true
        }
        _ => expected == actual,
    }
}

// STUB(mc.nbt.utils) — `NbtUtils.readBlockState(HolderGetter<Block>,
// CompoundTag) -> BlockState` and its private `setValueHelper` depend on
// `net.minecraft.world.level.block.state.BlockState`/`StateDefinition`/
// `Property` and `net.minecraft.core.HolderGetter` (rivet-world /
// rivet-registry), not yet ported.

// STUB(mc.nbt.utils) — `NbtUtils.writeBlockState(BlockState) -> CompoundTag`
// and `writeFluidState(FluidState) -> CompoundTag` (with the private
// `writeStateProperties`) depend on `BlockState`/`FluidState` and
// `BuiltInRegistries.BLOCK`/`FLUID` (rivet-world / rivet-registry), not yet
// ported.

/// `NbtUtils.prettyPrint(Tag, boolean)` — pretty-printed NBT, 2-space indent.
pub fn pretty_print(tag: &Tag, with_binary_blobs: bool) -> String {
    let mut builder = String::new();
    pretty_print_into(&mut builder, tag, 0, with_binary_blobs);
    builder
}

/// `NbtUtils.prettyPrint(StringBuilder, Tag, int, boolean)`.
pub fn pretty_print_into(builder: &mut String, input: &Tag, indent: i32, with_binary_blobs: bool) {
    match input {
        // `case PrimitiveTag primitive -> builder.append(primitive)` — Java
        // `StringBuilder.append(Object)` calls `toString`, i.e. StringTagVisitor.
        Tag::Byte(_)
        | Tag::Short(_)
        | Tag::Int(_)
        | Tag::Long(_)
        | Tag::Float(_)
        | Tag::Double(_)
        | Tag::String(_) => {
            builder.push_str(&crate::string_tag_visitor::StringTagVisitor::to_string(
                input,
            ));
        }
        Tag::End(_) => {}
        Tag::ByteArray(tag) => {
            let array = tag.get_as_byte_array();
            let length = array.len();
            indent_pad(indent, builder);
            builder.push_str(&format!("byte[{length}] {{\n"));
            if with_binary_blobs {
                indent_pad(indent + 1, builder);
                for i in 0..array.len() {
                    if i != 0 {
                        builder.push(',');
                    }
                    if i % 16 == 0 && i / 16 > 0 {
                        builder.push('\n');
                        if i < array.len() {
                            indent_pad(indent + 1, builder);
                        }
                    } else if i != 0 {
                        builder.push(' ');
                    }
                    // `String.format("0x%02X", array[i] & 255)`.
                    builder.push_str(&format!("0x{:02X}", array[i] as u8));
                }
            } else {
                indent_pad(indent + 1, builder);
                builder.push_str(" // Skipped, supply withBinaryBlobs true");
            }
            builder.push('\n');
            indent_pad(indent, builder);
            builder.push('}');
        }
        Tag::IntArray(tag) => {
            let array = tag.get_as_int_array();
            // `size = max(String.format("%X", i).length())` — uppercase hex of
            // the unsigned 32-bit pattern.
            let mut size: usize = 0;
            for i in array.iter() {
                size = size.max(format!("{:X}", *i as u32).len());
            }
            let length = array.len();
            indent_pad(indent, builder);
            builder.push_str(&format!("int[{length}] {{\n"));
            if with_binary_blobs {
                indent_pad(indent + 1, builder);
                for i in 0..array.len() {
                    if i != 0 {
                        builder.push(',');
                    }
                    if i % 16 == 0 && i / 16 > 0 {
                        builder.push('\n');
                        if i < array.len() {
                            indent_pad(indent + 1, builder);
                        }
                    } else if i != 0 {
                        builder.push(' ');
                    }
                    // `String.format("0x%0" + size + "X", array[i])`.
                    builder.push_str(&format!("0x{:0width$X}", array[i] as u32, width = size));
                }
            } else {
                indent_pad(indent + 1, builder);
                builder.push_str(" // Skipped, supply withBinaryBlobs true");
            }
            builder.push('\n');
            indent_pad(indent, builder);
            builder.push('}');
        }
        Tag::LongArray(tag) => {
            let array = tag.get_as_long_array();
            // Java declares `long size` and `long length` here.
            let mut size: i64 = 0;
            for i in array.iter() {
                size = size.max(format!("{:X}", *i as u64).len() as i64);
            }
            let length = array.len();
            indent_pad(indent, builder);
            builder.push_str(&format!("long[{length}] {{\n"));
            if with_binary_blobs {
                indent_pad(indent + 1, builder);
                for i in 0..array.len() {
                    if i != 0 {
                        builder.push(',');
                    }
                    if i % 16 == 0 && i / 16 > 0 {
                        builder.push('\n');
                        if i < array.len() {
                            indent_pad(indent + 1, builder);
                        }
                    } else if i != 0 {
                        builder.push(' ');
                    }
                    // `String.format("0x%0" + size + "X", array[i])` (long).
                    builder.push_str(&format!(
                        "0x{:0width$X}",
                        array[i] as u64,
                        width = size as usize
                    ));
                }
            } else {
                indent_pad(indent + 1, builder);
                builder.push_str(" // Skipped, supply withBinaryBlobs true");
            }
            builder.push('\n');
            indent_pad(indent, builder);
            builder.push('}');
        }
        Tag::List(tag) => {
            let size = tag.size();
            indent_pad(indent, builder);
            builder.push_str(&format!("list[{size}] ["));
            if size != 0 {
                builder.push('\n');
            }
            for i in 0..size {
                if i != 0 {
                    builder.push_str(",\n");
                }
                indent_pad(indent + 1, builder);
                pretty_print_into(builder, tag.get(i), indent + 1, with_binary_blobs);
            }
            if size != 0 {
                builder.push('\n');
            }
            indent_pad(indent, builder);
            builder.push(']');
        }
        Tag::Compound(tag) => {
            // Java copies the keys and `Collections.sort`s them (String order).
            let mut keys: Vec<String> = tag.key_set().cloned().collect();
            keys.sort_by(|a, b| utf16_cmp(a, b));
            indent_pad(indent, builder);
            builder.push('{');
            if column_since_last_newline_java(builder) > 2 * (indent + 1) {
                builder.push('\n');
                indent_pad(indent + 1, builder);
            }
            // `max(String::length)` over the keys, in UTF-16 code units.
            let padding_length = keys.iter().map(|k| utf16_len(k)).max().unwrap_or(0);
            for (i, key) in keys.iter().enumerate() {
                if i != 0 {
                    builder.push_str(",\n");
                }
                indent_pad(indent + 1, builder);
                builder.push('"');
                builder.push_str(key);
                builder.push('"');
                let pad = padding_length.saturating_sub(utf16_len(key));
                builder.push_str(&" ".repeat(pad));
                builder.push_str(": ");
                pretty_print_into(
                    builder,
                    tag.get(key).expect("key comes from key_set"),
                    indent + 1,
                    with_binary_blobs,
                );
            }
            if !keys.is_empty() {
                builder.push('\n');
            }
            indent_pad(indent, builder);
            builder.push('}');
        }
    }
}

/// `NbtUtils.indent(int, StringBuilder)` — pad with spaces so the current line
/// reaches column `2 * indent` (measured in UTF-16 units since the last
/// newline).
fn indent_pad(indent: i32, builder: &mut String) {
    let index = match builder.rfind('\n') {
        Some(i) => i + 1,
        None => 0,
    };
    let len = builder[index..].encode_utf16().count() as i32;
    for _ in 0..(INDENT * indent).saturating_sub(len) {
        builder.push(' ');
    }
}

/// `NbtUtils.toPrettyComponent(Tag)` — via `TextComponentTagVisitor("")`.
pub fn to_pretty_component(tag: &Tag) -> rivet_text::Component {
    let mut visitor = TextComponentTagVisitor::new("");
    visitor.visit(tag)
}

/// `NbtUtils.structureToSnbt(CompoundTag)` — pack then pretty-print.
pub fn structure_to_snbt(structure: &mut CompoundTag) -> String {
    let packed = pack_structure_template(structure);
    let tag = Tag::Compound(packed.clone());
    crate::snbt_printer_tag_visitor::visit(&tag)
}

/// `NbtUtils.snbtToStructure(String)` — parse then unpack.
///
/// Error-type divergence: Java throws brigadier `CommandSyntaxException`
/// (`TagParser.parseCompoundFully`); here the parse error surfaces as
/// `NbtFormatException` (rivet-nbt has no brigadier dependency).
pub fn snbt_to_structure(
    snbt: &str,
) -> Result<CompoundTag, crate::nbt_format_exception::NbtFormatException> {
    let mut parsed = crate::tag_parser::parse_compound_fully(snbt)?;
    Ok(unpack_structure_template(&mut parsed).clone())
}

/// `NbtUtils.packStructureTemplate(CompoundTag)` — deflate the palette and
/// rewrite `blocks`/`entities` into the packed form. Mutates and returns the
/// input.
fn pack_structure_template(snbt: &mut CompoundTag) -> &mut CompoundTag {
    let palettes = snbt.get_list("palettes").cloned();
    let palette = match &palettes {
        Some(palettes) => palettes.get_list_or_empty(0),
        None => snbt.get_list_or_empty("palette"),
    };

    let mut deflated_palette = ListTag::new();
    for compound in palette.compound_stream() {
        deflated_palette.add(Tag::String(StringTag::value_of(pack_block_state(compound))));
    }
    snbt.put("palette".to_string(), Tag::List(deflated_palette.clone()));

    if let Some(palettes) = &palettes {
        let mut new_palettes = ListTag::new();
        for tag in palettes.iter() {
            if let Some(old_palette) = tag.as_list() {
                let mut new_palette = CompoundTag::new();
                for i in 0..old_palette.size() {
                    let name = deflated_palette
                        .get_string(i)
                        .expect("deflated palette string");
                    let packed = pack_block_state(
                        old_palette
                            .get_compound(i)
                            .expect("old palette block state compound"),
                    );
                    new_palette.put_string(name, &packed);
                }
                new_palettes.add(Tag::Compound(new_palette));
            }
        }
        snbt.put("palettes".to_string(), Tag::List(new_palettes));
    }

    if let Some(old_entities) = snbt.get_list("entities").cloned() {
        let mut entries: Vec<CompoundTag> = old_entities.compound_stream().cloned().collect();
        entries.sort_by(|a, b| {
            pos_list_cmp(
                &a.get_list("pos"),
                &b.get_list("pos"),
                yxz_listtag_double_cmp,
            )
        });
        let mut new_entities = ListTag::new();
        for e in entries {
            new_entities.add(Tag::Compound(e));
        }
        snbt.put("entities".to_string(), Tag::List(new_entities));
    }

    // Java runs this unconditionally (NbtUtils.java:411-418): `snbt.getList(
    // "blocks")` is an `Optional` whose `.stream()` yields an empty stream when
    // the key is absent, so `blockData` is an empty `ListTag` and
    // `put("data", ...)` / `remove("blocks")` always execute.
    let mut block_data = ListTag::new();
    if let Some(blocks) = snbt.get_list("blocks").cloned() {
        let mut compounds: Vec<CompoundTag> = blocks.compound_stream().cloned().collect();
        compounds.sort_by(|a, b| {
            pos_list_cmp(&a.get_list("pos"), &b.get_list("pos"), yxz_listtag_int_cmp)
        });
        for block in &mut compounds {
            let state_index = block.get_int_or("state", 0) as usize;
            let state_name = deflated_palette
                .get_string(state_index)
                .expect("deflated state name for block");
            block.put_string("state", state_name);
            block_data.add(Tag::Compound(block.clone()));
        }
    }
    snbt.put("data".to_string(), Tag::List(block_data));
    snbt.remove("blocks");

    snbt
}

/// `NbtUtils.unpackStructureTemplate(CompoundTag)` — reverse of
/// [`pack_structure_template`]. Mutates and returns the input.
fn unpack_structure_template(template: &mut CompoundTag) -> &mut CompoundTag {
    let packed_palette = template.get_list_or_empty("palette");
    // Java `packedPalette.stream().flatMap(tag -> tag.asString().stream())
    // .collect(ImmutableMap.toImmutableMap(Function.identity(), ...))`:
    // `flatMap` skips non-string entries, `toImmutableMap` preserves stream
    // (insertion) order, and a duplicate packed name throws
    // `IllegalArgumentException`. The Vec of pairs mirrors the map's
    // insertion order; a duplicate panics like Java.
    let mut palette: Vec<(String, CompoundTag)> = Vec::new();
    let mut seen_names: HashMap<String, ()> = HashMap::new();
    for tag in packed_palette.iter() {
        if let Some(s) = tag.as_string() {
            let value = unpack_block_state(s);
            if seen_names.insert(s.clone(), ()).is_some() {
                panic!("Multiple entries with same key: {s}");
            }
            palette.push((s.clone(), value));
        }
    }

    let old_palettes = template.get_list("palettes").cloned();
    if let Some(old_palettes) = old_palettes {
        let mut new_palettes = ListTag::new();
        for old_palette in old_palettes.compound_stream() {
            let mut new_palette = ListTag::new();
            for (key, _) in palette.iter() {
                let packed = old_palette
                    .get_string(key)
                    .expect("old palette entry for packed name");
                new_palette.add(Tag::Compound(unpack_block_state(packed)));
            }
            new_palettes.add(Tag::List(new_palette));
        }
        template.put("palettes".to_string(), Tag::List(new_palettes));
        template.remove("palette");
    } else {
        let mut list = ListTag::new();
        for (_, value) in palette.iter() {
            list.add(Tag::Compound(value.clone()));
        }
        template.put("palette".to_string(), Tag::List(list));
    }

    if template.get_list("data").is_some() {
        let mut blocks = template.get_list("data").cloned().expect("checked is_some");
        let mut palette_to_id: HashMap<String, i32> = HashMap::new();
        for (i, tag) in packed_palette.iter().enumerate() {
            // Java `packedPalette.getString(i).orElseThrow()` — a non-string
            // palette entry throws `NoSuchElementException`.
            let name = tag.as_string().expect("packed palette entry is a string");
            palette_to_id.insert(name.clone(), i as i32);
        }
        for i in 0..blocks.size() {
            let block = match &mut blocks.list[i] {
                Tag::Compound(c) => c,
                _ => panic!("Expected a compound block entry in the data list"),
            };
            let state_name = block.get_string("state").expect("block state name");
            let state_id = palette_to_id.get(state_name).copied().unwrap_or(-1);
            if state_id == -1 {
                panic!("Entry {state_name} missing from palette");
            }
            block.put_int("state", state_id);
        }
        template.put("blocks".to_string(), Tag::List(blocks));
        template.remove("data");
    }

    template
}

/// `NbtUtils.packBlockState(CompoundTag)` — pack a block state compound into a
/// `Name{prop:value,...}` string (properties sorted by key).
fn pack_block_state(compound: &CompoundTag) -> String {
    let name = compound.get_string("Name").expect("block state Name");
    let mut builder = String::new();
    builder.push_str(name);
    if let Some(properties) = compound.get_compound("Properties") {
        let mut entries: Vec<(&String, &Tag)> = properties.entry_set().collect();
        entries.sort_by(|a, b| utf16_cmp(a.0, b.0));
        let key_values: Vec<String> = entries
            .iter()
            .map(|(k, v)| {
                format!(
                    "{k}{KEY_VALUE_SEPARATOR}{}",
                    v.as_string().expect("property value string")
                )
            })
            .collect();
        builder.push(PROPERTIES_START);
        builder.push_str(&key_values.join(ELEMENT_SEPARATOR));
        builder.push(PROPERTIES_END);
    }
    builder
}

/// `NbtUtils.unpackBlockState(String)` — reverse of [`pack_block_state`].
fn unpack_block_state(compound: &str) -> CompoundTag {
    let mut tag = CompoundTag::new();
    if let Some(open_index) = compound.find(PROPERTIES_START) {
        let name = &compound[..open_index];
        // Java `if (openIndex + 2 <= compound.length())` — at least one char
        // after the `{` (measured in bytes here; `{` is ASCII so the byte and
        // UTF-16 indices coincide at this boundary).
        if open_index + 2 <= compound.len() {
            let close_index = compound[open_index + 1..]
                .find(PROPERTIES_END)
                .map(|i| i + open_index + 1)
                // Java: `substring(openIndex + 1, indexOf('}', openIndex))` with
                // `indexOf == -1` throws StringIndexOutOfBoundsException.
                .expect("packed block state missing closing '}'");
            let values = &compound[open_index + 1..close_index];
            let mut properties = CompoundTag::new();
            for key_value in values.split(ELEMENT_SEPARATOR) {
                let mut parts = key_value.splitn(2, KEY_VALUE_SEPARATOR);
                match (parts.next(), parts.next()) {
                    (Some(k), Some(v)) => properties.put_string(k, v),
                    _ => {
                        // STUB(mc.nbt.utils) — Java:
                        // `LOGGER.error("Something went wrong parsing:
                        // '{}' -- incorrect gamedata!", compound)` — the logger
                        // is not ported yet (see text_component_tag_visitor.rs).
                    }
                }
            }
            tag.put("Properties".to_string(), Tag::Compound(properties));
        }
        tag.put_string("Name", name);
    } else {
        tag.put_string("Name", compound);
    }
    tag
}

/// `NbtUtils.addCurrentDataVersion(CompoundTag)`.
pub fn add_current_data_version(tag: &mut CompoundTag) -> &mut CompoundTag {
    add_data_version(tag, CURRENT_DATA_VERSION)
}

/// `NbtUtils.addDataVersion(CompoundTag, int)`.
pub fn add_data_version(tag: &mut CompoundTag, version: i32) -> &mut CompoundTag {
    tag.put_int("DataVersion", version);
    tag
}

// STUB(mc.nbt.utils) — `NbtUtils.addCurrentDataVersion(ValueOutput)` /
// `addDataVersion(ValueOutput, int)` (`output.putInt("DataVersion", version)`)
// depend on `net.minecraft.world.level.storage.ValueOutput` (rivet-world),
// not yet ported.

/// `NbtUtils.addDataVersion(Dynamic<T>, int)` — `tag.set("DataVersion",
/// tag.createInt(version))`. This implementation bypasses `Dynamic::set` and
/// inlines the `NbtOps.mergeToMap` semantics directly: a compound map is
/// `shallowCopy()`d and `put`; an `EndTag` (Java empty) becomes a fresh
/// compound; anything else is returned unchanged.
pub fn add_data_version_dynamic(tag: Dynamic<Tag>, version: i32) -> Dynamic<Tag> {
    let value = match &tag.value {
        Tag::Compound(c) => {
            let mut out = c.shallow_copy();
            out.put_int("DataVersion", version);
            Tag::Compound(out)
        }
        Tag::End(_) => {
            let mut out = CompoundTag::new();
            out.put_int("DataVersion", version);
            Tag::Compound(out)
        }
        other => other.clone(),
    };
    Dynamic::new(&NbtOps::instance(), value)
}

/// `NbtUtils.getDataVersion(CompoundTag)`.
pub fn get_data_version(tag: &CompoundTag) -> i32 {
    get_data_version_with_default(tag, -1)
}

/// `NbtUtils.getDataVersion(CompoundTag, int)`.
pub fn get_data_version_with_default(tag: &CompoundTag, default_value: i32) -> i32 {
    tag.get_int_or("DataVersion", default_value)
}

/// `NbtUtils.getDataVersion(Dynamic<?>)`.
pub fn get_data_version_dynamic(dynamic: &Dynamic<Tag>) -> i32 {
    get_data_version_dynamic_with_default(dynamic, -1)
}

/// `NbtUtils.getDataVersion(Dynamic<?>, int)` — `dynamic.get("DataVersion")
/// .asInt(default)`. For `Dynamic<Tag>` over `NbtOps`, `get` on a missing key
/// yields `empty()` (`EndTag`), whose `asInt` falls back to the default.
pub fn get_data_version_dynamic_with_default(dynamic: &Dynamic<Tag>, default_value: i32) -> i32 {
    match &dynamic.value {
        Tag::Compound(tag) => match tag.get("DataVersion") {
            Some(tag) => java_number_int_value(tag).unwrap_or(default_value),
            None => default_value,
        },
        _ => default_value,
    }
}

/// `Number.intValue()` on the boxed numeric value — `(int)` cast truncating
/// toward zero for Float/Double (matching `Dynamic.asInt` →
/// `asNumber().intValue()`), wrapping for Long. Non-numeric tags have no boxed
/// `Number` (Java `getNumberValue` errors) and yield `None`.
fn java_number_int_value(tag: &Tag) -> Option<i32> {
    match tag {
        Tag::Byte(t) => Some(t.value as i32),
        Tag::Short(t) => Some(t.value as i32),
        Tag::Int(t) => Some(t.value),
        Tag::Long(t) => Some(t.value as i32),
        Tag::Float(t) => Some(t.value as i32),
        Tag::Double(t) => Some(t.value as i32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::byte_array_tag::ByteArrayTag;
    use crate::double_tag::DoubleTag;
    use crate::end_tag::EndTag;
    use crate::float_tag::FloatTag;
    use crate::int_array_tag::IntArrayTag;
    use crate::int_tag::IntTag;
    use crate::long_array_tag::LongArrayTag;
    use crate::long_tag::LongTag;

    fn int_tag(v: i32) -> Tag {
        Tag::Int(IntTag::value_of(v))
    }

    // ---- compare_nbt ----

    #[test]
    fn compare_nbt_null_handling() {
        assert!(compare_nbt(None, None, false));
        assert!(compare_nbt(None, Some(&int_tag(1)), false));
        assert!(!compare_nbt(Some(&int_tag(1)), None, false));
    }

    #[test]
    fn compare_nbt_type_mismatch() {
        let expected: Tag = int_tag(1);
        let actual: Tag = Tag::Long(LongTag::value_of(1));
        assert!(!compare_nbt(Some(&expected), Some(&actual), false));
    }

    #[test]
    fn compare_nbt_equal_and_unequal_primitives() {
        let a: Tag = int_tag(5);
        let b: Tag = int_tag(5);
        let c: Tag = int_tag(6);
        assert!(compare_nbt(Some(&a), Some(&b), false));
        assert!(!compare_nbt(Some(&a), Some(&c), false));
        // Java record `FloatTag` equality: NaN == NaN.
        let nan_a: Tag = Tag::Float(FloatTag::value_of(f32::NAN));
        let nan_b: Tag = Tag::Float(FloatTag::value_of(f32::NAN));
        assert!(compare_nbt(Some(&nan_a), Some(&nan_b), false));
    }

    #[test]
    fn compare_nbt_compound_subset() {
        let mut expected = CompoundTag::new();
        expected.put_int("x", 1);
        let mut actual = CompoundTag::new();
        actual.put_int("x", 1);
        actual.put_int("y", 2);
        assert!(compare_nbt(
            Some(&Tag::Compound(expected)),
            Some(&Tag::Compound(actual)),
            false
        ));
    }

    #[test]
    fn compare_nbt_compound_missing_or_mismatched() {
        let mut expected = CompoundTag::new();
        expected.put_int("x", 1);
        expected.put_int("y", 2);
        let mut actual = CompoundTag::new();
        actual.put_int("x", 1);
        assert!(!compare_nbt(
            Some(&Tag::Compound(expected.clone())),
            Some(&Tag::Compound(actual)),
            false
        ));
        let mut wrong = CompoundTag::new();
        wrong.put_int("x", 2);
        wrong.put_int("y", 2);
        assert!(!compare_nbt(
            Some(&Tag::Compound(expected)),
            Some(&Tag::Compound(wrong)),
            false
        ));
    }

    #[test]
    fn compare_nbt_partial_list() {
        let mut expected = ListTag::new();
        expected.add(int_tag(1));
        expected.add(int_tag(2));
        let mut actual = ListTag::new();
        actual.add(int_tag(2));
        actual.add(int_tag(1));
        // Partial list match: order-insensitive, superset allowed.
        assert!(compare_nbt(
            Some(&Tag::List(expected.clone())),
            Some(&Tag::List(actual.clone())),
            true
        ));
        // Without partial matching, falls to ListTag.equals (ordered).
        assert!(!compare_nbt(
            Some(&Tag::List(expected)),
            Some(&Tag::List(actual)),
            false
        ));
    }

    #[test]
    fn compare_nbt_partial_list_empty_and_size() {
        let empty_expected = Tag::List(ListTag::new());
        let mut empty_actual = ListTag::new();
        empty_actual.add(int_tag(1));
        // Java: `expectedList.isEmpty()` -> `actualList.isEmpty()`.
        assert!(!compare_nbt(
            Some(&empty_expected),
            Some(&Tag::List(empty_actual)),
            true
        ));

        let mut expected = ListTag::new();
        expected.add(int_tag(1));
        expected.add(int_tag(2));
        expected.add(int_tag(3));
        let mut actual = ListTag::new();
        actual.add(int_tag(1));
        actual.add(int_tag(2));
        // actual smaller than expected.
        assert!(!compare_nbt(
            Some(&Tag::List(expected)),
            Some(&Tag::List(actual)),
            true
        ));
    }

    #[test]
    fn compare_nbt_nested_compound_in_list() {
        let mut inner = CompoundTag::new();
        inner.put_string("id", "stone");
        let mut expected = ListTag::new();
        expected.add(Tag::Compound(inner.clone()));
        let mut actual_inner = CompoundTag::new();
        actual_inner.put_string("id", "stone");
        actual_inner.put_int("aux", 0);
        let mut actual = ListTag::new();
        actual.add(Tag::Compound(actual_inner));
        assert!(compare_nbt(
            Some(&Tag::List(expected)),
            Some(&Tag::List(actual)),
            true
        ));
    }

    // ---- pretty_print ----

    #[test]
    fn pretty_print_primitives() {
        assert_eq!(pretty_print(&int_tag(5), true), "5");
        assert_eq!(
            pretty_print(&Tag::String(StringTag::value_of("hi".to_owned())), true),
            "\"hi\""
        );
        // Java `builder.append(primitive)` = `toString()` = StringTagVisitor:
        // floats/doubles keep their suffix.
        assert_eq!(
            pretty_print(&Tag::Float(FloatTag::value_of(1.0)), true),
            "1.0f"
        );
        assert_eq!(
            pretty_print(&Tag::Double(DoubleTag::value_of(1.5)), true),
            "1.5d"
        );
        // EndTag is a no-op.
        assert_eq!(pretty_print(&Tag::End(EndTag), true), "");
    }

    #[test]
    fn pretty_print_byte_array_without_blobs() {
        let tag = Tag::ByteArray(ByteArrayTag::new(vec![1, 2]));
        // Java: `indent(indent + 1)` at indent 0 emits 2 spaces, then appends
        // " // Skipped, supply withBinaryBlobs true" (leading space).
        assert_eq!(
            pretty_print(&tag, false),
            "byte[2] {\n   // Skipped, supply withBinaryBlobs true\n}"
        );
    }

    #[test]
    fn pretty_print_byte_array_with_blobs() {
        let tag = Tag::ByteArray(ByteArrayTag::new(vec![0x01, 0x02, 0xffu8 as i8]));
        assert_eq!(pretty_print(&tag, true), "byte[3] {\n  0x01, 0x02, 0xFF\n}");
    }

    #[test]
    fn pretty_print_byte_array_wraps_every_16() {
        let data: Vec<i8> = (0..17).map(|v| v as i8).collect();
        let tag = Tag::ByteArray(ByteArrayTag::new(data));
        let text = pretty_print(&tag, true);
        // 17th element (i=16, value 0x10) starts on a new line after 0x0F.
        assert!(text.contains("0x0F,\n  0x10"));
    }

    #[test]
    fn pretty_print_int_array_hex_padding() {
        let tag = Tag::IntArray(IntArrayTag::new(vec![255, 256]));
        // %X of 255 = "FF" (2), of 256 = "100" (3) -> width 3.
        assert_eq!(pretty_print(&tag, true), "int[2] {\n  0x0FF, 0x100\n}");
    }

    #[test]
    fn pretty_print_long_array_hex() {
        let tag = Tag::LongArray(LongArrayTag::new(vec![-1, 16]));
        // %X of -1 (long) = "FFFFFFFFFFFFFFFF" (16), of 16 = "10" (2) -> width 16.
        assert_eq!(
            pretty_print(&tag, true),
            "long[2] {\n  0xFFFFFFFFFFFFFFFF, 0x0000000000000010\n}"
        );
    }

    #[test]
    fn pretty_print_list() {
        let mut list = ListTag::new();
        list.add(int_tag(1));
        list.add(int_tag(2));
        assert_eq!(
            pretty_print(&Tag::List(list), true),
            "list[2] [\n  1,\n  2\n]"
        );
    }

    #[test]
    fn pretty_print_compound_padding_and_indent() {
        let mut compound = CompoundTag::new();
        compound.put_int("aa", 1);
        compound.put_int("b", 2);
        // Keys sorted ("aa","b"); padding pads "b" to the longest key ("aa",
        // 2) -> `"b" :`; keys render on a new line when the current line
        // exceeds 2*(indent+1) chars (here it does not for the first entry).
        assert_eq!(
            pretty_print(&Tag::Compound(compound), true),
            "{ \"aa\": 1,\n  \"b\" : 2\n}"
        );
    }

    #[test]
    fn pretty_print_compound_nested() {
        let mut compound = CompoundTag::new();
        compound.put_int("x", 1);
        let mut nested = CompoundTag::new();
        nested.put_string("name", "value");
        compound.put("nested".to_string(), Tag::Compound(nested));
        let text = pretty_print(&Tag::Compound(compound), true);
        // Nested compound's line "{ \"nested\": {" exceeds 2*(1+1)=4 chars, so
        // its first key goes on a new line; "x" is padded to the 6-char width
        // of "nested" -> `"x"` + 5 spaces.
        assert!(text.starts_with("{ \"nested\": {\n"));
        assert!(text.contains("\n    \"name\": \"value\"\n"));
        assert!(text.contains("\"x\"     : 1"));
        assert!(text.ends_with("\n}"));
    }

    // ---- data version ----

    #[test]
    fn get_data_version_defaults() {
        assert_eq!(get_data_version(&CompoundTag::new()), -1);
        let mut tag = CompoundTag::new();
        tag.put_int("DataVersion", 4903);
        assert_eq!(get_data_version(&tag), 4903);
        assert_eq!(get_data_version_with_default(&tag, -1), 4903);
        // Non-numeric DataVersion falls back to the default.
        let mut tag2 = CompoundTag::new();
        tag2.put_string("DataVersion", "nope");
        assert_eq!(get_data_version(&tag2), -1);
    }

    #[test]
    fn add_data_version_sets_and_returns() {
        let mut tag = CompoundTag::new();
        add_data_version(&mut tag, 4903);
        assert_eq!(tag.get_int_or("DataVersion", -1), 4903);
        // `addDataVersion` mutates and returns the same tag (`return tag`).
        let ret = add_data_version(&mut tag, 4904);
        assert_eq!(ret.get_int_or("DataVersion", -1), 4904);
        assert_eq!(tag.get_int_or("DataVersion", -1), 4904);
    }

    #[test]
    fn add_current_data_version_uses_world_version() {
        let mut tag = CompoundTag::new();
        add_current_data_version(&mut tag);
        assert_eq!(tag.get_int_or("DataVersion", -1), CURRENT_DATA_VERSION);
        assert_eq!(CURRENT_DATA_VERSION, 4903);
    }

    #[test]
    fn dynamic_data_version_roundtrip() {
        let mut compound = CompoundTag::new();
        compound.put_int("a", 1);
        let dynamic = Dynamic::new(&NbtOps::instance(), Tag::Compound(compound));
        assert_eq!(get_data_version_dynamic(&dynamic), -1);
        let set = add_data_version_dynamic(dynamic, 4903);
        assert_eq!(get_data_version_dynamic(&set), 4903);
    }

    #[test]
    fn dynamic_data_version_truncates_negative_fractional() {
        // Java `dynamic.get("DataVersion").asInt(default)` goes through the
        // boxed `Number.intValue()` (truncation toward zero), NOT the
        // NumericTag.intValue() floor used by the CompoundTag overload. So a
        // DataVersion of -1.7 reads as -1 (truncated), not -2 (floored).
        let mut compound = CompoundTag::new();
        compound.put(
            "DataVersion".to_string(),
            Tag::Double(DoubleTag::value_of(-1.7)),
        );
        let dynamic = Dynamic::new(&NbtOps::instance(), Tag::Compound(compound.clone()));
        assert_eq!(get_data_version_dynamic(&dynamic), -1);

        // The CompoundTag overload floors (NumericTag.intValue == getIntOr).
        assert_eq!(get_data_version(&compound), -2);
    }

    // ---- block state pack/unpack ----

    #[test]
    fn unpack_block_state_plain_name() {
        let tag = unpack_block_state("minecraft:stone");
        assert_eq!(
            tag.get_string("Name").map(String::as_str),
            Some("minecraft:stone")
        );
        assert_eq!(tag.get_compound("Properties"), None);
    }

    #[test]
    fn unpack_block_state_with_properties() {
        let tag = unpack_block_state("minecraft:stone{facing:up,lit:true}");
        assert_eq!(
            tag.get_string("Name").map(String::as_str),
            Some("minecraft:stone")
        );
        let props = tag.get_compound("Properties").expect("Properties");
        assert_eq!(props.get_string("facing").map(String::as_str), Some("up"));
        assert_eq!(props.get_string("lit").map(String::as_str), Some("true"));
    }

    #[test]
    fn unpack_block_state_colon_limited_to_two_parts() {
        let tag = unpack_block_state("minecraft:foo{a:1:2}");
        let props = tag.get_compound("Properties").expect("Properties");
        assert_eq!(props.get_string("a").map(String::as_str), Some("1:2"));
    }

    #[test]
    fn pack_block_state_sorts_properties() {
        let mut compound = CompoundTag::new();
        compound.put_string("Name", "minecraft:stone");
        let mut props = CompoundTag::new();
        props.put_string("lit", "true");
        props.put_string("facing", "up");
        compound.put("Properties".to_string(), Tag::Compound(props));
        assert_eq!(
            pack_block_state(&compound),
            "minecraft:stone{facing:up,lit:true}"
        );
    }

    #[test]
    fn block_state_round_trip() {
        let packed = "minecraft:stone{facing:up,lit:true}";
        let unpacked = unpack_block_state(packed);
        assert_eq!(pack_block_state(&unpacked), packed);
    }

    // ---- structure pack/unpack ----

    fn simple_structure() -> CompoundTag {
        let mut structure = CompoundTag::new();

        let mut stone = CompoundTag::new();
        stone.put_string("Name", "minecraft:stone");
        let mut air = CompoundTag::new();
        air.put_string("Name", "minecraft:air");
        let mut palette = ListTag::new();
        palette.add(Tag::Compound(stone));
        palette.add(Tag::Compound(air));
        structure.put("palette".to_string(), Tag::List(palette));

        let mut b0 = CompoundTag::new();
        let mut pos0 = ListTag::new();
        pos0.add(int_tag(0));
        pos0.add(int_tag(0));
        pos0.add(int_tag(0));
        b0.put("pos".to_string(), Tag::List(pos0));
        b0.put_int("state", 0);
        let mut b1 = CompoundTag::new();
        let mut pos1 = ListTag::new();
        pos1.add(int_tag(1));
        pos1.add(int_tag(0));
        pos1.add(int_tag(0));
        b1.put("pos".to_string(), Tag::List(pos1));
        b1.put_int("state", 1);
        let mut blocks = ListTag::new();
        blocks.add(Tag::Compound(b0));
        blocks.add(Tag::Compound(b1));
        structure.put("blocks".to_string(), Tag::List(blocks));

        let mut e0 = CompoundTag::new();
        let mut epos0 = ListTag::new();
        epos0.add(Tag::Double(DoubleTag::value_of(1.0)));
        epos0.add(Tag::Double(DoubleTag::value_of(2.0)));
        epos0.add(Tag::Double(DoubleTag::value_of(3.0)));
        e0.put("pos".to_string(), Tag::List(epos0));
        let mut e1 = CompoundTag::new();
        let mut epos1 = ListTag::new();
        epos1.add(Tag::Double(DoubleTag::value_of(0.0)));
        epos1.add(Tag::Double(DoubleTag::value_of(0.0)));
        epos1.add(Tag::Double(DoubleTag::value_of(0.0)));
        e1.put("pos".to_string(), Tag::List(epos1));
        let mut entities = ListTag::new();
        entities.add(Tag::Compound(e0));
        entities.add(Tag::Compound(e1));
        structure.put("entities".to_string(), Tag::List(entities));

        structure
    }

    #[test]
    fn structure_round_trip() {
        let mut structure = simple_structure();

        let packed = pack_structure_template(&mut structure);
        // Palette is deflated to name strings.
        let palette = packed.get_list("palette").expect("palette");
        assert_eq!(
            palette.get_string(0).map(String::as_str),
            Some("minecraft:stone")
        );
        assert_eq!(
            palette.get_string(1).map(String::as_str),
            Some("minecraft:air")
        );
        // Blocks moved to "data" with the state rewritten to a name string.
        let data = packed.get_list("data").expect("data");
        assert_eq!(
            data.get_compound(0)
                .unwrap()
                .get_string("state")
                .map(String::as_str),
            Some("minecraft:stone")
        );
        // Entities sorted by pos (y then x then z): (0,0,0) before (1,2,3).
        let entities = packed.get_list("entities").expect("entities");
        let epos0 = entities.get_compound(0).unwrap().get_list("pos").unwrap();
        assert_eq!(epos0.get_double(0), Some(0.0));

        unpack_structure_template(&mut structure);
        let palette = structure.get_list("palette").expect("palette");
        assert_eq!(
            palette
                .get_compound(0)
                .unwrap()
                .get_string("Name")
                .map(String::as_str),
            Some("minecraft:stone")
        );
        let blocks = structure.get_list("blocks").expect("blocks");
        assert_eq!(blocks.get_compound(0).unwrap().get_int("state"), Some(0));
        assert_eq!(blocks.get_compound(1).unwrap().get_int("state"), Some(1));
        assert!(structure.get_list("data").is_none());
    }

    #[test]
    fn structure_round_trip_with_palettes() {
        let mut structure = simple_structure();
        // Add a "palettes" list mirroring the palette.
        let mut stone = CompoundTag::new();
        stone.put_string("Name", "minecraft:stone");
        let mut air = CompoundTag::new();
        air.put_string("Name", "minecraft:air");
        let mut palette_list = ListTag::new();
        palette_list.add(Tag::Compound(stone));
        palette_list.add(Tag::Compound(air));
        let mut palettes = ListTag::new();
        palettes.add(Tag::List(palette_list));
        structure.put("palettes".to_string(), Tag::List(palettes));

        pack_structure_template(&mut structure);
        let palettes = structure.get_list("palettes").expect("palettes");
        // After pack, each palette entry is a CompoundTag mapping the deflated
        // name to its packed block-state string (Java `newPalette`).
        let palette = palettes.get_compound(0).expect("palette 0");
        assert_eq!(
            palette.get_string("minecraft:stone").map(String::as_str),
            Some("minecraft:stone")
        );

        unpack_structure_template(&mut structure);
        // "palettes" unpacked back to compound lists, "palette" removed.
        let palettes = structure.get_list("palettes").expect("palettes");
        let palette = palettes.get_list(0).expect("palette 0");
        assert_eq!(
            palette
                .get_compound(0)
                .unwrap()
                .get_string("Name")
                .map(String::as_str),
            Some("minecraft:stone")
        );
        assert!(structure.get_list("palette").is_none());
    }

    #[test]
    fn pack_structure_template_puts_empty_data_without_blocks() {
        // Java `packStructureTemplate` runs `put("data", blockData)`;
        // `remove("blocks")` unconditionally (NbtUtils.java:417-418), so a
        // structure with no "blocks" key gains an empty "data" list.
        let mut structure = CompoundTag::new();
        let mut stone = CompoundTag::new();
        stone.put_string("Name", "minecraft:stone");
        let mut palette = ListTag::new();
        palette.add(Tag::Compound(stone));
        structure.put("palette".to_string(), Tag::List(palette));

        pack_structure_template(&mut structure);
        let data = structure.get_list("data").expect("data present");
        assert_eq!(data.size(), 0);
        assert!(structure.get_list("blocks").is_none());
    }

    #[test]
    fn unpack_structure_template_round_trips_absent_blocks() {
        // Pack (adds empty "data"), then unpack: Java `unpackStructureTemplate`
        // turns that empty "data" list back into an empty "blocks" key.
        let mut structure = CompoundTag::new();
        let mut stone = CompoundTag::new();
        stone.put_string("Name", "minecraft:stone");
        let mut palette = ListTag::new();
        palette.add(Tag::Compound(stone));
        structure.put("palette".to_string(), Tag::List(palette));

        pack_structure_template(&mut structure);
        unpack_structure_template(&mut structure);
        assert!(structure.get_list("data").is_none());
        assert_eq!(
            structure.get_list("blocks").expect("blocks present").size(),
            0
        );
    }

    #[test]
    #[should_panic(expected = "Multiple entries with same key")]
    fn unpack_structure_template_panics_on_duplicate_palette_names() {
        // Java `ImmutableMap.toImmutableMap` throws IllegalArgumentException on
        // duplicate packed names.
        let mut structure = CompoundTag::new();
        let mut palette = ListTag::new();
        palette.add(Tag::String(StringTag::value_of(
            "minecraft:stone".to_string(),
        )));
        palette.add(Tag::String(StringTag::value_of(
            "minecraft:stone".to_string(),
        )));
        structure.put("palette".to_string(), Tag::List(palette));

        unpack_structure_template(&mut structure);
    }

    #[test]
    #[should_panic(expected = "packed palette entry is a string")]
    fn unpack_structure_template_panics_on_non_string_palette_entry_in_data() {
        // Java `packedPalette.getString(i).orElseThrow()` in the paletteToId
        // loop throws NoSuchElementException when a "data" list is present.
        let mut structure = CompoundTag::new();
        let mut palette = ListTag::new();
        palette.add(Tag::Int(IntTag::value_of(3)));
        structure.put("palette".to_string(), Tag::List(palette));
        let mut blocks = ListTag::new();
        let mut block = CompoundTag::new();
        let mut pos = ListTag::new();
        pos.add(int_tag(0));
        pos.add(int_tag(0));
        pos.add(int_tag(0));
        block.put("pos".to_string(), Tag::List(pos));
        block.put_string("state", "minecraft:stone");
        blocks.add(Tag::Compound(block));
        structure.put("data".to_string(), Tag::List(blocks));

        unpack_structure_template(&mut structure);
    }
}
