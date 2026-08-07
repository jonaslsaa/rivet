//! Consumer tests for the generated biome id table + tag network content
//! (issue #49, `data/biomes_tags.json` → `generated/biomes.rs` + `generated/tags.rs`).
//!
//! Lives OUTSIDE the codegen-owned `generated/` dir (the golden drift test
//! asserts `src/generated/` contains exactly the generated files), only under
//! the `blocks` feature + `cfg(test)` — same pattern as `block_id_tests` /
//! `static_builtin_tests` / `block_state_tests`.
//!
//! These tests read the generated tables only; the tables themselves are
//! codegen-owned and guarded by the drift test + the live `probe-biomes-tags`.

use crate::generated::biomes::{BIOME_BY_ID, BIOME_BY_NAME, BIOME_COUNT};
use crate::generated::blocks::BLOCK_BY_NAME;
use crate::generated::tags::BLOCK_TAG_BY_NAME;
use crate::generated::tags::{TAG_REGISTRIES, WORLDGEN_BIOME_TAG_BY_NAME};

/// The biome table must be a dense bijection (name <-> id), id 0 is badlands
/// (the `TreeMap<Identifier>` alphabetical load order), and the global-palette
/// count is 66.
#[test]
fn biome_name_id_roundtrip_is_consistent() {
    assert_eq!(BIOME_COUNT, 66);
    assert_eq!(BIOME_BY_ID.len(), BIOME_BY_NAME.len());
    // id 0 = badlands (Identifier.compareTo path-first ordering).
    assert_eq!(BIOME_BY_ID[0], "minecraft:badlands");
    assert_eq!(BIOME_BY_NAME["minecraft:badlands"], 0);
    // Dense 0..66: every id maps to a name, every name to an id.
    for (i, name) in BIOME_BY_ID.iter().enumerate() {
        assert_eq!(BIOME_BY_NAME[*name], i as u16);
    }
}

/// The shared element surfaces the tag tables resolve against must agree with
/// the existing generated tables (block and biome): a tag element that names a
/// block must be a real block.
#[test]
fn tag_elements_resolve_against_shared_tables() {
    // The biome tag table's elements are all biomes.
    for names in WORLDGEN_BIOME_TAG_BY_NAME.values() {
        for name in *names {
            assert!(
                BIOME_BY_NAME.contains_key(name),
                "tag references non-biome `{name}`"
            );
        }
    }
    // The block tag table's elements are all blocks.
    for names in BLOCK_TAG_BY_NAME.values() {
        for name in *names {
            assert!(
                BLOCK_BY_NAME.contains_key(name),
                "tag references non-block `{name}`"
            );
        }
    }
}

/// The 15 tag-carrying registries must be present in the deterministic order,
/// the biome registry must be on the wire, and the two surfaces the superflat
/// chunk (chunk.access #183-b) consumes — biome + block tags — are non-empty.
/// The full per-registry tag counts (summing to 697) are codegen-asserted.
#[test]
fn tag_registry_surface_is_complete_and_consistent() {
    assert_eq!(TAG_REGISTRIES.len(), 15);
    assert!(TAG_REGISTRIES.contains(&"minecraft:worldgen/biome"));
    assert!(TAG_REGISTRIES.contains(&"minecraft:block"));
    assert!(!WORLDGEN_BIOME_TAG_BY_NAME.is_empty());
    assert!(!BLOCK_TAG_BY_NAME.is_empty());
}

/// The `is_overworld`/`is_nether` tags (which superflat preset biomes resolve
/// through) must be present and non-empty.
#[test]
fn overworld_and_nether_biome_tags_exist() {
    assert!(WORLDGEN_BIOME_TAG_BY_NAME.contains_key("minecraft:is_overworld"));
    assert!(WORLDGEN_BIOME_TAG_BY_NAME.contains_key("minecraft:is_nether"));
    assert!(!WORLDGEN_BIOME_TAG_BY_NAME["minecraft:is_overworld"].is_empty());
    assert!(!WORLDGEN_BIOME_TAG_BY_NAME["minecraft:is_nether"].is_empty());
}
