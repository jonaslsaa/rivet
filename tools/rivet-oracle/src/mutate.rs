//! Controlled NBT mutations for the #54 tamper negatives.
//!
//! The comparator must be *proven* to detect real divergence, not just assumed
//! to (false-green threat 4). Each `TamperKind` mutates a specific named field
//! of a serialized Level-compound payload through the rivet-nbt codec:
//!
//! - `Block`: flips a block-state palette `Name` in a section's `block_states`.
//! - `Light`: flips a byte of a section's `SkyLight`/`BlockLight` nibble array.
//! - `Heightmap`: flips a long of a `Heightmaps` array.
//! - `NbtOrder`: swaps two root compound keys (order-only change — must NOT be
//!   a canonical difference; proves the semantic triage split).
//!
//! Every mutation is parse → locate → mutate → re-encode, so the changed bytes
//! are exactly the named field's; the tests assert both that the serialized
//! digest changed *and* that the mutation landed in the field the kind names
//! (never just "something changed"). The `WrongSeed` negative is a manifest-
//! level concern (a different seed produces different bytes), so it lives in
//! the manifest tests, not here.

use std::io::Cursor;

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::nbt_io;
use rivet_nbt::tag::Tag;
use rivet_util::{DataInputStream, DataOutputStream};

/// Which field class a mutation targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TamperKind {
    Block,
    Light,
    Heightmap,
    NbtOrder,
}

impl TamperKind {
    pub const ALL: [TamperKind; 4] = [
        TamperKind::Block,
        TamperKind::Light,
        TamperKind::Heightmap,
        TamperKind::NbtOrder,
    ];

    /// Parse the CLI name (`block`, `light`, `heightmap`, `nbt-order`).
    pub fn from_cli(name: &str) -> Option<TamperKind> {
        match name {
            "block" => Some(TamperKind::Block),
            "light" => Some(TamperKind::Light),
            "heightmap" => Some(TamperKind::Heightmap),
            "nbt-order" => Some(TamperKind::NbtOrder),
            _ => None,
        }
    }

    /// The CLI name, mirroring `from_cli`.
    pub fn cli_name(self) -> &'static str {
        match self {
            TamperKind::Block => "block",
            TamperKind::Light => "light",
            TamperKind::Heightmap => "heightmap",
            TamperKind::NbtOrder => "nbt-order",
        }
    }
}

/// Parse a serialized Level-compound payload into a `CompoundTag`.
pub fn parse_payload(bytes: &[u8]) -> Result<CompoundTag, String> {
    let mut input = DataInputStream::new(Cursor::new(bytes));
    nbt_io::read_unlimited(&mut input).map_err(|e| format!("NBT read failed: {e}"))
}

/// Re-encode a `CompoundTag` to the unnamed-root serialized form.
pub fn encode_payload(compound: &CompoundTag) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    nbt_io::write(compound, &mut DataOutputStream::new(Cursor::new(&mut out)))
        .map_err(|e| format!("NBT write failed: {e}"))?;
    Ok(out)
}

/// A minimal but structurally faithful FULL Level payload carrying every field
/// the mutations target (block palette, SkyLight/BlockLight, Heightmaps) and
/// the chunk coordinate (so distinct chunks hash distinctly). Deterministic by
/// construction — the diff tests and the tamper negatives build whole fixture
/// trees from it instead of committing thousands of payload blobs.
#[cfg(test)]
pub fn fixture_full_payload(cx: i32, cz: i32) -> Vec<u8> {
    let mut root = CompoundTag::new();
    root.put_string("Status", "minecraft:full");
    root.put_int("xPos", cx);
    root.put_int("zPos", cz);
    let mut section = CompoundTag::new();
    section.put_byte("Y", 0);
    let mut bs = CompoundTag::new();
    let mut palette = CompoundTag::new();
    palette.put_string(
        "Name",
        if (cx + cz) % 2 == 0 {
            "minecraft:air"
        } else {
            "minecraft:stone"
        },
    );
    bs.put(
        "palette".to_string(),
        Tag::List(ListTag::with_list(vec![Tag::Compound(palette)])),
    );
    bs.put_long_array("data", vec![0; 256]);
    section.put("block_states".to_string(), Tag::Compound(bs));
    section.put_byte_array("SkyLight", vec![0i8; 2048]);
    section.put_byte_array("BlockLight", vec![0i8; 2048]);
    root.put(
        "sections".to_string(),
        Tag::List(ListTag::with_list(vec![Tag::Compound(section)])),
    );
    let mut heightmaps = CompoundTag::new();
    heightmaps.put_long_array("WORLD_SURFACE", vec![1; 37]);
    root.put("Heightmaps".to_string(), Tag::Compound(heightmaps));
    encode_payload(&root).expect("fixture payload encodes")
}

/// Apply one mutation of `kind` to a serialized payload, returning the
/// re-encoded bytes. Errors if the named field is absent (the caller is
/// expected to use a fixture that carries it).
pub fn tamper(bytes: &[u8], kind: TamperKind) -> Result<Vec<u8>, String> {
    let mut compound = parse_payload(bytes)?;
    match kind {
        TamperKind::Block => tamper_block(&mut compound)?,
        TamperKind::Light => tamper_light(&mut compound)?,
        TamperKind::Heightmap => tamper_heightmap(&mut compound)?,
        TamperKind::NbtOrder => tamper_nbt_order(&mut compound)?,
    }
    encode_payload(&compound)
}

/// Flip a block-state palette `Name` in the first section that has one. The
/// palette name is the load-bearing block identity: changing `minecraft:air`
/// to `minecraft:stone` is a real worldgen-visible difference.
fn tamper_block(compound: &mut CompoundTag) -> Result<(), String> {
    let sections = sections_mut(compound)?;
    for i in 0..sections.list.len() {
        let Tag::Compound(section) = &mut sections.list[i] else {
            continue;
        };
        let bs = section.get_compound_or_empty_mut("block_states");
        let palette = bs.get_list_or_empty_mut("palette");
        for j in 0..palette.list.len() {
            let Tag::Compound(entry) = &mut palette.list[j] else {
                continue;
            };
            let Some(Tag::String(name)) = entry.tags.get("Name") else {
                continue;
            };
            let new = if name.value == "minecraft:air" {
                "minecraft:stone".to_string()
            } else {
                "minecraft:air".to_string()
            };
            entry.tags.insert(
                "Name".to_string(),
                Tag::String(rivet_nbt::string_tag::StringTag::value_of(new)),
            );
            return Ok(());
        }
    }
    Err("chunk has no block palette to mutate".into())
}

/// Flip the first byte of a section's `SkyLight` (falling back to
/// `BlockLight`) nibble array.
fn tamper_light(compound: &mut CompoundTag) -> Result<(), String> {
    let sections = sections_mut(compound)?;
    for i in 0..sections.list.len() {
        let Tag::Compound(section) = &mut sections.list[i] else {
            continue;
        };
        let light = if let Some(Tag::ByteArray(arr)) = section.tags.get("SkyLight") {
            Some(("SkyLight", arr.data.clone()))
        } else if let Some(Tag::ByteArray(arr)) = section.tags.get("BlockLight") {
            Some(("BlockLight", arr.data.clone()))
        } else {
            None
        };
        if let Some((key, mut data)) = light {
            if data.is_empty() {
                continue;
            }
            data[0] ^= 0x40;
            section.tags.insert(
                key.to_string(),
                Tag::ByteArray(rivet_nbt::byte_array_tag::ByteArrayTag::new(data)),
            );
            return Ok(());
        }
    }
    Err("no section has SkyLight/BlockLight to mutate".into())
}

/// Flip one long of the `Heightmaps` compound's first array.
fn tamper_heightmap(compound: &mut CompoundTag) -> Result<(), String> {
    let hm = compound
        .get_compound("Heightmaps")
        .ok_or_else(|| "chunk has no Heightmaps compound".to_string())?;
    let (key, mut data) = hm
        .tags
        .iter()
        .find_map(|(k, v)| match v {
            Tag::LongArray(l) => Some((k.clone(), l.data.clone())),
            _ => None,
        })
        .ok_or_else(|| "Heightmaps has no long array".to_string())?;
    if data.is_empty() {
        return Err("Heightmaps long array is empty".into());
    }
    data[0] ^= 1 << 20;
    compound
        .get_compound_or_empty_mut("Heightmaps")
        .tags
        .insert(
            key,
            Tag::LongArray(rivet_nbt::long_array_tag::LongArrayTag::new(data)),
        );
    Ok(())
}

/// Swap two root compound keys' tags. Serialized digest must change; canonical
/// digest must not (order-only). `swap_indices` swaps in place so both keys keep
/// their values and no key disappears.
fn tamper_nbt_order(compound: &mut CompoundTag) -> Result<(), String> {
    if compound.tags.len() < 2 {
        return Err("chunk root has fewer than 2 keys to swap".into());
    }
    compound.tags.swap_indices(0, 1);
    Ok(())
}

fn sections_mut(compound: &mut CompoundTag) -> Result<&mut ListTag, String> {
    if !matches!(compound.tags.get("sections"), Some(Tag::List(_))) {
        return Err("sections is not a list".into());
    }
    Ok(compound.get_list_or_empty_mut("sections"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::xxh3_64_hex;

    /// A minimal but structurally faithful Level payload with all four
    /// mutation targets present (the shared `fixture_full_payload` builder).
    fn fixture_payload() -> Vec<u8> {
        fixture_full_payload(0, 0)
    }

    #[test]
    fn mutation_lands_in_named_field() {
        for kind in TamperKind::ALL {
            let original = fixture_payload();
            let mutated = tamper(&original, kind).expect("mutation succeeds");
            let orig = parse_payload(&original).unwrap();
            let m = parse_payload(&mutated).unwrap();
            match kind {
                TamperKind::Block => {
                    let orig_name = palette_name(&orig);
                    let mut_name = palette_name(&m);
                    assert_eq!(orig_name, "minecraft:air");
                    assert_ne!(mut_name, "minecraft:air");
                }
                TamperKind::Light => {
                    let orig_light = light(&orig);
                    let mut_light = light(&m);
                    assert_eq!(orig_light.len(), 2048);
                    assert_ne!(orig_light[0], mut_light[0]);
                }
                TamperKind::Heightmap => {
                    let orig_hm = heightmap(&orig);
                    let mut_hm = heightmap(&m);
                    assert_ne!(orig_hm[0], mut_hm[0]);
                }
                TamperKind::NbtOrder => {
                    let orig_canon = crate::semantic_hash::canonical_xxh3_64(&orig).unwrap();
                    let mut_canon = crate::semantic_hash::canonical_xxh3_64(&m).unwrap();
                    assert_eq!(orig_canon, mut_canon, "order swap is canonical-identical");
                }
            }
            assert_ne!(
                xxh3_64_hex(&original),
                xxh3_64_hex(&mutated),
                "serialized digest must change for {kind:?}"
            );
        }
    }

    fn palette_name(c: &CompoundTag) -> String {
        c.get_list("sections")
            .unwrap()
            .get_compound(0)
            .unwrap()
            .get_compound("block_states")
            .unwrap()
            .get_list("palette")
            .unwrap()
            .get_compound(0)
            .unwrap()
            .get_string("Name")
            .unwrap()
            .clone()
    }

    fn light(c: &CompoundTag) -> Vec<i8> {
        c.get_list("sections")
            .unwrap()
            .get_compound(0)
            .unwrap()
            .get_byte_array("SkyLight")
            .unwrap()
            .clone()
    }

    fn heightmap(c: &CompoundTag) -> Vec<i64> {
        c.get_compound("Heightmaps")
            .unwrap()
            .get_long_array("WORLD_SURFACE")
            .unwrap()
            .clone()
    }
}
