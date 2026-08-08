//! Semantic (order-insensitive) canonical hash — **triage only, never the gate**.
//!
//! A serialized Level-compound payload is order-sensitive (the NBT writer
//! emits keys in insertion order). Two chunks that differ only in compound key
//! order hash differently at the byte level but describe the same chunk; a
//! differential that fails only on such a chunk is best triaged as
//! "semantically identical, serialization-order difference" before anyone
//! chases a worldgen bug. This module canonicalizes by recursively sorting
//! compound keys, then re-encoding with the proven byte-identical codec — so
//! `canonical_xxh3_64` is order-insensitive by construction.
//!
//! It is explicitly NOT the gate digest: the gate hashes the raw serialized
//! payload (order-sensitive), because Rivet must reproduce Paper's exact byte
//! order. The `mutate_nbt_order` negative proves this split: reordering a
//! compound changes the serialized digest but leaves the canonical digest
//! unchanged. See README "Semantic triage".

use std::io::Cursor;

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_io;
use rivet_nbt::tag::Tag;
use rivet_util::DataOutputStream;

use crate::hash::xxh3_64_hex;

/// Recursively sort every compound's keys lexicographically in place. Lists of
/// compounds are sorted element-by-element (list order itself is preserved —
/// it is semantically meaningful); compound *keys* are the only order that is
/// arbitrary on the wire.
pub fn canonicalize(compound: &mut CompoundTag) {
    let mut pairs: Vec<(String, Tag)> = compound.tags.drain(..).collect();
    pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (_, tag) in &mut pairs {
        canonicalize_tag(tag);
    }
    compound.tags = pairs.into_iter().collect();
}

fn canonicalize_tag(tag: &mut Tag) {
    match tag {
        Tag::Compound(c) => canonicalize(c),
        Tag::List(l) => {
            for elem in &mut l.list {
                if let Tag::Compound(c) = elem {
                    canonicalize(c);
                }
            }
        }
        _ => {}
    }
}

/// Re-encode the given compound into the canonical serialized form
/// (`NbtIo.write`'s unnamed-root framing, keys sorted recursively) and return
/// the bytes.
pub fn canonical_bytes(compound: &CompoundTag) -> Result<Vec<u8>, String> {
    let mut c = compound.clone();
    canonicalize(&mut c);
    let mut out = Vec::new();
    {
        let mut dos = DataOutputStream::new(Cursor::new(&mut out));
        nbt_io::write(&c, &mut dos).map_err(|e| format!("canonical NBT write failed: {e}"))?;
    }
    Ok(out)
}

/// Canonical xxh3_64 of the chunk's Level compound (order-insensitive).
pub fn canonical_xxh3_64(compound: &CompoundTag) -> Result<String, String> {
    Ok(xxh3_64_hex(&canonical_bytes(compound)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::xxh3_64_hex;

    fn sample_compound() -> CompoundTag {
        let mut c = CompoundTag::new();
        c.put_string("Status", "minecraft:full");
        let mut inner = CompoundTag::new();
        inner.put_int("b", 1);
        inner.put_int("a", 2);
        c.put("inner".to_string(), Tag::Compound(inner));
        c
    }

    #[test]
    fn canonical_serializes_order_independently() {
        let a = sample_compound();
        let mut b = sample_compound();
        // Swap the insertion order of `inner`'s keys.
        let inner = b.get_compound("inner").unwrap().clone();
        let mut swapped = CompoundTag::new();
        swapped.put_int("a", 2);
        swapped.put_int("b", 1);
        b.put("inner".to_string(), Tag::Compound(swapped));
        drop(inner);

        assert_ne!(
            xxh3_64_hex(&serialized(&a)),
            xxh3_64_hex(&serialized(&b)),
            "serialized form is order-sensitive"
        );
        assert_eq!(
            canonical_xxh3_64(&a).unwrap(),
            canonical_xxh3_64(&b).unwrap(),
            "canonical form is order-insensitive"
        );
    }

    #[test]
    fn canonical_differs_for_real_changes() {
        let a = sample_compound();
        let mut b = sample_compound();
        b.put_string("Status", "minecraft:structure_starts");
        assert_ne!(
            canonical_xxh3_64(&a).unwrap(),
            canonical_xxh3_64(&b).unwrap()
        );
    }

    fn serialized(c: &CompoundTag) -> Vec<u8> {
        let mut out = Vec::new();
        nbt_io::write(c, &mut DataOutputStream::new(Cursor::new(&mut out)))
            .expect("write succeeds");
        out
    }
}
