//! Generated static-builtin-table integration tests for the report-driven
//! surfaces (issue #124 phase F). Gated behind the `blocks` feature + `cfg(test)`.
//!
//! These test that each report-driven `*_BY_ID`/`*_BY_NAME` pair is a dense
//! `0..n` bijection (element id == holder id == network id == insertion index,
//! OWNERSHIP.md §Registries) and that the `DefaultedRegistry` folds line up
//! with the tables. The tables are codegen-owned (source:
//! `data/reports/registries.json`, the vanilla `RegistryDumpReport`); this
//! module reads them only and lives OUTSIDE `src/generated/` (the codegen
//! golden drift test asserts that dir contains exactly the generated files).

use crate::generated::registries::{
    BLOCK_ENTITY_TYPE_BY_ID, BLOCK_ENTITY_TYPE_BY_NAME, DATA_COMPONENT_TYPE_BY_ID,
    DATA_COMPONENT_TYPE_BY_NAME, ENTITY_TYPE_BY_ID, ENTITY_TYPE_BY_NAME, FLUID_BY_ID,
    FLUID_BY_NAME, GAME_EVENT_BY_ID, GAME_EVENT_BY_NAME, ITEM_BY_ID, ITEM_BY_NAME,
    POINT_OF_INTEREST_TYPE_BY_ID, POINT_OF_INTEREST_TYPE_BY_NAME, POTION_BY_ID, POTION_BY_NAME,
};

/// Every report-driven surface is a dense bijection: `*_BY_NAME` and `*_BY_ID`
/// agree on size, ids are contiguous `0..len`, and every name maps to its
/// index. This mirrors the invariant the #124 SCC asserts for the block tables
/// (`generated_block_tables_agree_with_the_id_space`).
#[test]
fn report_driven_tables_are_dense_bijections() {
    let pairs: [(&[&str], &phf::Map<&'static str, u16>); 8] = [
        (ITEM_BY_ID, &ITEM_BY_NAME),
        (ENTITY_TYPE_BY_ID, &ENTITY_TYPE_BY_NAME),
        (BLOCK_ENTITY_TYPE_BY_ID, &BLOCK_ENTITY_TYPE_BY_NAME),
        (DATA_COMPONENT_TYPE_BY_ID, &DATA_COMPONENT_TYPE_BY_NAME),
        (FLUID_BY_ID, &FLUID_BY_NAME),
        (GAME_EVENT_BY_ID, &GAME_EVENT_BY_NAME),
        (POTION_BY_ID, &POTION_BY_NAME),
        (
            POINT_OF_INTEREST_TYPE_BY_ID,
            &POINT_OF_INTEREST_TYPE_BY_NAME,
        ),
    ];
    for (by_id, by_name) in pairs {
        assert_eq!(by_id.len(), by_name.len(), "by_id/by_name size mismatch");
        for (id, name) in by_id.iter().enumerate() {
            assert_eq!(
                by_name.get(name).copied(),
                Some(id as u16),
                "{name} (id {id}) missing/aliased in BY_NAME"
            );
        }
    }
}

/// A `DefaultedRegistry` surface (item, entity_type, fluid, game_event) must
/// have its default element present in the tables and round-trip — the id
/// `getValue(name)` / `byId` fall back to when a lookup misses. The default id
/// need not be 0 (e.g. game_event's `minecraft:step` is id 42 in 26.2); the
/// fold is simply that element's index.
#[test]
fn defaulted_surfaces_agree_with_their_default_markers() {
    // `minecraft:item` default `minecraft:air`.
    let air = ITEM_BY_NAME["minecraft:air"];
    assert_eq!(ITEM_BY_ID[air as usize], "minecraft:air");
    // `minecraft:entity_type` default `minecraft:pig` (id 100 in 26.2).
    let pig_id = ENTITY_TYPE_BY_NAME["minecraft:pig"];
    assert_eq!(ENTITY_TYPE_BY_ID[pig_id as usize], "minecraft:pig");
    // `minecraft:fluid` default `minecraft:empty`.
    let empty = FLUID_BY_NAME["minecraft:empty"];
    assert_eq!(FLUID_BY_ID[empty as usize], "minecraft:empty");
    // `minecraft:game_event` default `minecraft:step` (id 42 in 26.2).
    let step = GAME_EVENT_BY_NAME["minecraft:step"];
    assert_eq!(GAME_EVENT_BY_ID[step as usize], "minecraft:step");
}

/// Non-defaulted surfaces must have no defaulted element: every vanilla id
/// resolves to a distinct name (the bijection check above), and the surfaces
/// that Java models as plain `Registry`s carry no `*_DEFAULT` marker.
#[test]
fn non_defaulted_surfaces_have_no_default_marker() {
    let potion_default = crate::generated::registries::POTION_DEFAULT;
    let poi_default = crate::generated::registries::POINT_OF_INTEREST_TYPE_DEFAULT;
    assert!(potion_default.is_none(), "potion must not be defaulted");
    assert!(
        poi_default.is_none(),
        "point_of_interest_type must not be defaulted"
    );
}
