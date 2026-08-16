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
//! - `NbtKey`: inserts a root NBT key Paper's writer never emits (a content
//!   change — unlike `NbtOrder`, the canonical digest changes too; proves a
//!   key-level tamper that adds data is detected, #175 7(d)).
//!
//! Every mutation is parse → locate → mutate → re-encode, so the changed bytes
//! are exactly the named field's; the tests assert both that the serialized
//! digest changed *and* that the mutation landed in the field the kind names
//! (never just "something changed"). The bogus-seed negative (#175 7(e) — a
//! capture generated under a different seed hashes differently) is a
//! whole-tree concern, so it lives in the `hash-diff` tests, which regenerate
//! the deterministic fixture payloads under a different seed via
//! `fixture_full_payload_with_seed`.

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
    NbtKey,
}

impl TamperKind {
    pub const ALL: [TamperKind; 5] = [
        TamperKind::Block,
        TamperKind::Light,
        TamperKind::Heightmap,
        TamperKind::NbtOrder,
        TamperKind::NbtKey,
    ];

    /// Parse the CLI name (`block`, `light`, `heightmap`, `nbt-order`,
    /// `nbt-key`).
    pub fn from_cli(name: &str) -> Option<TamperKind> {
        match name {
            "block" => Some(TamperKind::Block),
            "light" => Some(TamperKind::Light),
            "heightmap" => Some(TamperKind::Heightmap),
            "nbt-order" => Some(TamperKind::NbtOrder),
            "nbt-key" => Some(TamperKind::NbtKey),
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
            TamperKind::NbtKey => "nbt-key",
        }
    }
}

/// Parse a serialized Level-compound payload into a `CompoundTag`.
pub fn parse_payload(bytes: &[u8]) -> Result<CompoundTag, String> {
    let mut input = DataInputStream::new(Cursor::new(bytes));
    let compound =
        nbt_io::read_unlimited(&mut input).map_err(|e| format!("NBT read failed: {e}"))?;
    let cursor = input.into_inner();
    if cursor.position() != bytes.len() as u64 {
        return Err(format!(
            "NBT payload has {} trailing bytes",
            bytes.len() as u64 - cursor.position()
        ));
    }
    Ok(compound)
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
///
/// It also carries the FULL-time structure fields `hash_manifest` requires of a
/// genuinely finished chunk (`structures` + all four FINAL heightmaps as 37-long
/// arrays), so `build_from_payloads`' `validate_full_payload` accepts it and the
/// synthetic trees hash the same way the live superflat FULL capture does.
///
/// Delegates to `fixture_full_payload_with_seed` with the working seed 42
/// (mirroring `CAPTURE_SEED`) so a tree built by the plain builder is a
/// different-seed tree from one built with a bogus seed — the seed flows into
/// the block content (`block_states.data`), which is the bogus-seed negative's
/// mechanism.
#[cfg(test)]
pub fn fixture_full_payload(cx: i32, cz: i32) -> Vec<u8> {
    fixture_full_payload_with_seed(cx, cz, 42)
}

/// Like `fixture_full_payload`, but carries the given world seed into the
/// chunk's worldgen *content* (the `block_states.data` per-block array), so
/// two payloads built for the same coordinate under different seeds hash
/// differently — the deterministic analogue of the #175 7(e) bogus-seed
/// negative without booting Paper.
///
/// The seed is deliberately **not** injected into the root `LastUpdate` /
/// `InhabitedTime` tick counters: those are game/inhabited time, which in a
/// fresh world are 0 for every seed and never a function of the world seed.
/// The mechanism Paper actually has is that a different seed generates
/// different worldgen content, so this builder folds the seed into the
/// per-block placement array — an honest, multi-bit stand-in for that content
/// difference (a single palette-name parity bit would model only two worlds
/// and collide for same-parity seeds).
#[cfg(test)]
pub fn fixture_full_payload_with_seed(cx: i32, cz: i32, seed: i64) -> Vec<u8> {
    let mut root = CompoundTag::new();
    root.put_string("Status", "minecraft:full");
    root.put_int("xPos", cx);
    root.put_int("zPos", cz);
    // Root tick counters `SerializableChunkData.write()` emits from Level state
    // (`LastUpdate` = game time, `InhabitedTime` = per-chunk inhabited time).
    // These are time-based, not seed-based — a fresh world is 0 for every seed,
    // so they are fixed constants here, never derived from `seed`.
    root.put_long("LastUpdate", 0);
    root.put_long("InhabitedTime", 0);
    let mut section = CompoundTag::new();
    section.put_byte("Y", 0);
    let mut bs = CompoundTag::new();
    let mut palette = CompoundTag::new();
    // The palette name alternates by coordinate only (air/stone) — the
    // seed-dependent part of this chunk's content lives in the `data` array
    // below, not here.
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
    // The seed folds into the `block_states.data` long array — the per-block
    // packed placement real worldgen derives from the seed. Different seeds at
    // the same coordinate produce different arrays, so two trees compared
    // coordinate-for-coordinate (as the diff does) differ at every chunk for
    // any seed pair, never just an opposite-parity pair.
    bs.put_long_array("data", seed_block_data(seed, cx, cz));
    section.put("block_states".to_string(), Tag::Compound(bs));
    section.put_byte_array("SkyLight", vec![0i8; 2048]);
    section.put_byte_array("BlockLight", vec![0i8; 2048]);
    root.put(
        "sections".to_string(),
        Tag::List(ListTag::with_list(vec![Tag::Compound(section)])),
    );
    let mut heightmaps = CompoundTag::new();
    for key in [
        "OCEAN_FLOOR",
        "WORLD_SURFACE",
        "MOTION_BLOCKING",
        "MOTION_BLOCKING_NO_LEAVES",
    ] {
        heightmaps.put_long_array(key, vec![1; 37]);
    }
    root.put("Heightmaps".to_string(), Tag::Compound(heightmaps));
    let mut structures = CompoundTag::new();
    structures.put("starts".to_string(), Tag::Compound(CompoundTag::new()));
    structures.put("References".to_string(), Tag::Compound(CompoundTag::new()));
    root.put("structures".to_string(), Tag::Compound(structures));
    // lightCorrect (SerializableChunkData §6): a genuine FULL chunk always
    // carries isLightOn (true at write, then clobbered to false) and
    // starlight.light_version == 10. The FULL validator requires both.
    root.put_byte("isLightOn", 0);
    root.put_int("starlight.light_version", 10);
    encode_payload(&root).expect("fixture payload encodes")
}

/// Deterministic per-chunk block data derived from the world seed: a fixed
/// 256-long array mixed with `(seed, cx, cz)` via a small xorshift-style
/// mixer. For a **fixed coordinate** the seed mix is a bijection mod 2^64 (the
/// multiplier is odd and the xorshift steps are invertible), so different seeds
/// at the same coordinate *always* produce different arrays — the property the
/// bogus-seed negative relies on, since it compares each coordinate across two
/// trees. For a fixed seed, distinct coordinates diverge too (the xorshift
/// steps are invertible and the coordinate xors differ). The joint
/// (seed, coordinate) space is *not* injective: a seed `s` at `(cx, cz)` and a
/// different seed `s'` at a different coordinate can coincide, so the claim is
/// per-coordinate only, and the tests vary one axis at a time. Either way a
/// two-seed comparison never reduces to a single parity bit (which would
/// collide for same-parity seeds).
#[cfg(test)]
fn seed_block_data(seed: i64, cx: i32, cz: i32) -> Vec<i64> {
    let mut state = (seed as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(0xBF58_476D_1CE4_E5B9);
    state ^= (cx as u32 as u64) << 32;
    state ^= cz as u32 as u64;
    (0..256)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as i64
        })
        .collect()
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
        TamperKind::NbtKey => tamper_nbt_key(&mut compound)?,
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

/// Insert a root NBT key that `SerializableChunkData.write()` never emits.
/// Unlike `tamper_nbt_order` this is a real content change — the canonical
/// digest changes too — so it proves a key-level tamper that *adds* data is
/// detected (#175 7(d)), not just key reordering. Every existing key is kept;
/// a root already carrying the marker is refused so the tamper is exactly one
/// inserted key, never a rewrite.
fn tamper_nbt_key(compound: &mut CompoundTag) -> Result<(), String> {
    if compound.tags.contains_key("TamperKey") {
        return Err("chunk root already carries the TamperKey marker".into());
    }
    compound.put_int("TamperKey", 1);
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
    fn payload_parser_rejects_trailing_bytes() {
        let mut payload = fixture_payload();
        payload.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        let error = parse_payload(&payload).expect_err("trailing bytes must be rejected");
        assert!(
            error.contains("trailing bytes"),
            "unexpected error: {error}"
        );
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
                TamperKind::NbtKey => {
                    assert!(
                        !orig.tags.contains_key("TamperKey"),
                        "original has no marker key"
                    );
                    assert_eq!(m.get_int("TamperKey"), Some(1), "marker key inserted");
                    assert_ne!(
                        crate::semantic_hash::canonical_xxh3_64(&orig).unwrap(),
                        crate::semantic_hash::canonical_xxh3_64(&m).unwrap(),
                        "inserting a key is a content change, not order-only"
                    );
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

    /// The first `LongArray` in the `Heightmaps` compound — the exact array
    /// `tamper_heightmap` flips (its `find_map` picks the first long array).
    fn heightmap(c: &CompoundTag) -> Vec<i64> {
        c.get_compound("Heightmaps")
            .unwrap()
            .tags
            .iter()
            .find_map(|(_, v)| match v {
                Tag::LongArray(l) => Some(l.data.clone()),
                _ => None,
            })
            .expect("Heightmaps has a long array")
    }

    /// A different seed must produce different bytes for the same coordinate —
    /// the deterministic mechanism behind the #175 7(e) bogus-seed negative.
    #[test]
    fn different_seed_different_payload() {
        let a = fixture_full_payload_with_seed(0, 0, 42);
        let b = fixture_full_payload_with_seed(0, 0, 999);
        assert_ne!(a, b, "bogus seed must change the serialized payload");
        assert_ne!(xxh3_64_hex(&a), xxh3_64_hex(&b));
        // Same parity, different seed (42 and 1000 are both even): the content
        // difference must still be real — a single parity bit would make these
        // byte-identical, so this pins the multi-bit seed mechanism.
        let same_parity = fixture_full_payload_with_seed(0, 0, 1000);
        assert_ne!(
            a, same_parity,
            "same-parity seeds must still produce different content"
        );
        assert_ne!(xxh3_64_hex(&a), xxh3_64_hex(&same_parity));
        // Same seed, same coordinate — deterministic by construction.
        assert_eq!(
            fixture_full_payload_with_seed(0, 0, 42),
            fixture_full_payload_with_seed(0, 0, 42)
        );
        // Same seed, different coordinate — distinct chunks hash distinctly.
        // Pin the mixer's coordinate axis directly: the payload-level assert
        // below is confounded by xPos/zPos and palette parity, so it alone
        // could not catch a mixer that ignored the coordinate.
        assert_ne!(
            seed_block_data(42, 0, 0),
            seed_block_data(42, 1, 0),
            "the seed mixer must fold the coordinate into the block data"
        );
        assert_ne!(a, fixture_full_payload_with_seed(1, 0, 42));
    }
}
