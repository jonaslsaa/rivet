//! Generated-block-table integration tests for the #124 registry SCC
//! (ownership C — access/provider). Gated behind the `blocks` feature.
//!
//! These test that the SCC's provider/access layer carries the block registry
//! across the erased `RegistryAccess` boundary, and that the generated
//! `BLOCK_BY_NAME`/`BLOCK_BY_ID` tables (codegen-owned) line up with the
//! registry id space (element id == holder id == network id == insertion index,
//! OWNERSHIP.md §Registries). The tables are codegen-owned; this module is
//! SCC-owned and reads them only.
//!
//! This module lives OUTSIDE the codegen-owned `generated/` directory: the
//! golden drift test (`rivet-codegen`'s
//! `generated_output_matches_committed`) asserts `src/generated/` contains
//! exactly the generated files, so no hand-written file may sit in it. It is
//! wired from `lib.rs` via `#[path]` (the `generated/mod.rs` wiring is
//! codegen-owned and untouched) and only exists under the `blocks` feature +
//! `cfg(test)`.
//!
//! The `blocks` feature is off by default, so these tests only build/run when a
//! consumer enables it (matching the `required-features` test pattern in
//! `rivet-protocol`).

use crate::Identifier;
use crate::ResourceKey;
use crate::access::RegistryAccess;
use crate::builder::RegistryBuilder;
use crate::generated::blocks::{BLOCK_BY_ID, BLOCK_BY_NAME, BlockId};
use crate::registry::{Registry, RegistryKey};
use crate::root::AnyBox;

use std::sync::Arc;

/// `BLOCK_BY_NAME` and `BLOCK_BY_ID` must be consistent: a dense bijection
/// between the vanilla block names and the numeric ids (`0..len`). The
/// generated tables already encode this; this test guards the wiring
/// (ownership C) rather than the tables themselves.
#[test]
fn block_name_id_roundtrip_is_consistent() {
    assert_eq!(
        BlockId::from_name("minecraft:air").map(BlockId::id),
        Some(0)
    );
    assert_eq!(BlockId::from_id(0).name(), "minecraft:air");
    assert_eq!(BlockId::from_id(1).name(), "minecraft:stone");
    // The two tables must agree in size (id space == number of block names),
    // and the id space is dense (more than a hundred vanilla blocks).
    assert_eq!(BLOCK_BY_ID.len(), BLOCK_BY_NAME.len());
    assert!(BLOCK_BY_ID.len() > 100);
}

/// The SCC provider must carry the block registry across the erased
/// `RegistryAccess` boundary and resolve it by its registry key, with the
/// codegen-owned `BlockId` as the element type. The full 1196-block id-space
/// contract is asserted in `registry::generated_block_tests` (ownership B);
/// this ownership-C test wires the *seam* through which any `RegistryBuilder`-
/// built registry — here a `Registry<BlockId>` carrying the real codegen-owned
/// id type — flows across the erased boundary and back out by downcast.
#[test]
fn generated_block_ids_map_to_registry_insertion_order() {
    let block_key: RegistryKey<BlockId> =
        ResourceKey::create_registry_key(Identifier::with_default_namespace("block"));

    // A small representative slice of the codegen-owned table, keyed by the
    // real `BlockId` type: element id == holder id == insertion index.
    let names = ["minecraft:air", "minecraft:stone", "minecraft:dirt"];
    let mut builder = RegistryBuilder::new(&block_key);
    let keys: Vec<ResourceKey<BlockId>> = names
        .iter()
        .map(|name| ResourceKey::create(&block_key, Identifier::parse(name)))
        .collect();
    for (id, key) in keys.iter().enumerate() {
        builder.register(
            key,
            Arc::new(BlockId::from_id(id as u16)),
            crate::registration_info::RegistrationInfo::BUILT_IN,
        );
    }
    let block_registry: Registry<BlockId> = builder.freeze();

    let erased_key: RegistryKey<()> =
        ResourceKey::create_registry_key(Identifier::with_default_namespace("block"));
    let access = RegistryAccess::from_pairs(vec![(
        erased_key.clone(),
        Box::new(block_registry) as AnyBox,
    )]);

    // The erased boundary (RegistryAccess) resolves the block registry...
    let erased = access
        .lookup_erased(&erased_key)
        .expect("block registry present");
    // ...and downcasts to the concrete `Registry<BlockId>` value table.
    let resolved: &Registry<BlockId> = erased
        .as_any()
        .downcast_ref()
        .expect("typed downcast at the erased boundary");

    // The id-space contract through the seam, against the codegen-owned tables
    // for the representative entries.
    assert_eq!(resolved.size(), 3);
    for (id, name) in names.iter().enumerate() {
        let key = &keys[id];
        let value = resolved.get_value(key).expect("registered");
        assert_eq!(resolved.get_id(value), id as i32);
        assert_eq!(value.id(), id as u16);
        assert_eq!(resolved.get_key(value), Some(Identifier::parse(name)));
        assert!(resolved.contains_key(&Identifier::parse(name)));
        assert_eq!(resolved.by_id(id as i32), Some(value));
    }
    // The representative ids agree with the codegen-owned tables.
    assert_eq!(BlockId::from_name(names[0]).map(BlockId::id), Some(0));
    assert_eq!(BlockId::from_name(names[1]).map(BlockId::id), Some(1));
}
