//! Pinned-source-grounded tests for the `DataFixerBuilder` foundation
//! (issue #534).
//!
//! Every assertion mirrors a behavior in the pinned DFU 10.0.21 sources
//! (`com.mojang.datafixers.DataFixerBuilder`, `DataFixerUpper`, `Schema`,
//! `DataFixUtils`), extracted to `/tmp/dfu-src`.
//!
//! - `addSchema`: version-key arithmetic, parent = lowest schema with the same
//!   version as `key - 1`, schemas kept sorted ascending by key.
//! - `addFixer`: fixes whose version exceeds the game `dataVersion` are ignored
//!   (Java logs a warning and returns); accepted fixes keep `fixerVersions`
//!   sorted.
//! - `getLowestSchemaSameVersion` / `getLowestFixSameVersion`: the `subMap`/
//!   `subSet` clamps (pre-first schema => first key; pre-first fixer => first
//!   fixer key - 1).
//! - `getRule`: the `expandedFixVersion > expandedVersion && fixVersion <=
//!   newVersion` filter, the nop-rule skip, and the single-rule collapse of
//!   `TypeRewriteRule.seq`.
//! - `update`: no-op when `version >= newVersion`; the value is preserved when
//!   the rewrite chain is empty (Java `resultOrPartial(...).orElse(input)`).
//! - `DataFix.getInputSchema`: `changesType` picks the output schema's parent.

use rivet_serialization::datafixers::data_fix::DataFix;
use rivet_serialization::datafixers::data_fix_utils::{
    get_sub_version, get_version, make_key, make_key_sub,
};
use rivet_serialization::datafixers::data_fixer_builder::DataFixerBuilder;
use rivet_serialization::datafixers::data_fixer_upper::get_lowest_schema_same_version;
use rivet_serialization::datafixers::schemas::Schema;
use rivet_serialization::datafixers::type_rewrite_rule;
use rivet_serialization::datafixers::types::TypeTemplate;
use rivet_serialization::datafixers::types::templates::{Const, EmptyPartPassthrough};
use rivet_serialization::dynamic::Dynamic;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::json_ops::JsonOps;
use std::sync::Arc;

type Json = JsonOps;

/// A schema at raw `version_key` with one registered remainder type `"x"`.
fn schema_with_key_and_type(
    version_key: i32,
    parent: Option<Arc<Schema<Json>>>,
) -> Arc<Schema<Json>> {
    let mut schema = Schema::new(version_key, parent);
    // `DSL.remainder` — a `Const(EmptyPartPassthrough)` template (the
    // register_simple body), inserted directly to avoid a `&self`+`&mut` borrow
    // conflict on the same field.
    let template: Arc<dyn TypeTemplate<Json>> =
        Arc::new(Const::new(Arc::new(EmptyPartPassthrough::new())));
    schema.type_templates.insert("x".to_string(), template);
    schema.types = schema.build_types();
    Arc::new(schema)
}

/// A non-nop rewrite rule (a `Seq` of two nops). Used so `getRule` cannot
/// skip the fix via its `fixRule == TypeRewriteRule.nop()` check.
fn non_nop_rule() -> Arc<dyn type_rewrite_rule::TypeRewriteRule<Json>> {
    type_rewrite_rule::seq(vec![type_rewrite_rule::nop(), type_rewrite_rule::nop()])
}

// ---------------------------------------------------------------------------
// Version-key arithmetic (DataFixUtils)
// ---------------------------------------------------------------------------

#[test]
fn make_key_and_version_math() {
    // makeKey(99) == 990; makeKey(99, 4) == 994; getVersion(994) == 99;
    // getSubVersion(994) == 4.
    assert_eq!(make_key(99), 990);
    assert_eq!(get_version(994), 99);
    assert_eq!(get_sub_version(994), 4);
    assert_eq!(make_key(99), 990);
    // makeKey(version, 0) == makeKey(version).
    assert_eq!(make_key(99), make_key_sub(99, 0));
}

#[test]
fn make_key_wraps_like_java_int() {
    // Java `int` wrapping: 300_000_000 * 10 = 3_000_000_000 overflows i32
    // (max 2_147_483_647) and wraps negative.
    let wrapped = make_key(300_000_000);
    assert_eq!(wrapped, 3_000_000_000i64 as i32);
    assert_eq!(get_version(wrapped), wrapped / 10);
    // Negative sub-version: makeKey(99, -1) == 989.
    assert_eq!(make_key_sub(99, -1), 989);
    assert_eq!(get_sub_version(989), 9);
}

// ---------------------------------------------------------------------------
// DataFixerBuilder.addSchema — parent computation + ordering
// ---------------------------------------------------------------------------

#[test]
fn add_schema_parent_is_lowest_schema_same_version_below_key() {
    let mut builder = DataFixerBuilder::new(999);
    // First schema: no parent.
    let s0 = builder.add_schema(100, |key, parent| {
        assert_eq!(key, 1000);
        assert!(parent.is_none());
        schema_with_key_and_type(key, None)
    });
    // Second schema version 99 sub 1: key 991. Parent = lowest schema whose
    // key <= 990; the only schema so far (1000) is above 990, so the clamp
    // returns it.
    let s1 = builder.add_schema_sub(99, 1, |key, parent| {
        assert_eq!(key, 991);
        assert_eq!(parent.unwrap().get_version_key(), 1000);
        schema_with_key_and_type(key, None)
    });
    // Schemas kept sorted ascending by key: [991, 1000].
    let keys: Vec<i32> = builder
        .build()
        .schemas
        .iter()
        .map(|s| s.get_version_key())
        .collect();
    assert_eq!(keys, vec![991, 1000]);
    assert_eq!(s0.get_version_key(), 1000);
    assert_eq!(s1.get_version_key(), 991);
}

#[test]
fn add_schema_parent_when_inserted_out_of_order() {
    let mut builder = DataFixerBuilder::new(999);
    builder.add_schema(100, |key, _parent| schema_with_key_and_type(key, None));
    // addSchema(99): key 990, parent = lowest schema with key <= 989. The only
    // schema (key 1000) is above 989, so the clamp returns it.
    let s = builder.add_schema(99, |key, parent| {
        assert_eq!(key, 990);
        assert_eq!(parent.unwrap().get_version_key(), 1000);
        schema_with_key_and_type(key, None)
    });
    assert_eq!(s.get_version_key(), 990);
}

// ---------------------------------------------------------------------------
// getLowestSchemaSameVersion (the subMap clamp)
// ---------------------------------------------------------------------------

#[test]
fn lowest_schema_same_version_clamps_below_first() {
    let schemas = vec![
        schema_with_key_and_type(1000, None),
        schema_with_key_and_type(2000, None),
    ];
    // versionKey below the first schema key => Java returns the FIRST key.
    assert_eq!(get_lowest_schema_same_version(&schemas, 500), 0);
    assert_eq!(get_lowest_schema_same_version(&schemas, 1000), 0);
    assert_eq!(get_lowest_schema_same_version(&schemas, 1999), 0);
    assert_eq!(get_lowest_schema_same_version(&schemas, 2000), 1);
    assert_eq!(get_lowest_schema_same_version(&schemas, 3000), 1);
}

// ---------------------------------------------------------------------------
// DataFixerBuilder.addFixer — version gating + sorted fixer versions
// ---------------------------------------------------------------------------

#[test]
fn add_fixer_ignores_fixes_above_data_version() {
    let mut builder = DataFixerBuilder::new(99);
    let s_high = schema_with_key_and_type(make_key(100), None);
    let s_ok = schema_with_key_and_type(make_key(10), None);
    builder.add_schema_obj(s_high.clone());
    builder.add_schema_obj(s_ok.clone());

    // fix at version 100 > dataVersion 99 => ignored.
    builder.add_fixer(Arc::new(DataFix::new(s_high, false)));
    // fix at version 10 <= 99 => accepted.
    builder.add_fixer(Arc::new(DataFix::new(s_ok, false)));

    let fixer = builder.build();
    assert_eq!(fixer.global_list.len(), 1);
    assert_eq!(fixer.fixer_versions, vec![make_key(10)]);
}

#[test]
fn add_fixer_keeps_fixer_versions_sorted() {
    let mut builder = DataFixerBuilder::new(999);
    for key in [3000, 1000, 2000] {
        let s = schema_with_key_and_type(key, None);
        builder.add_schema_obj(s.clone());
        builder.add_fixer(Arc::new(DataFix::new(s, false)));
    }
    let fixer = builder.build();
    assert_eq!(fixer.fixer_versions, vec![1000, 2000, 3000]);
}

// ---------------------------------------------------------------------------
// DataFixerUpper.getRule — the expandedVersion filter and ordering
// ---------------------------------------------------------------------------

#[test]
fn get_rule_includes_in_range_fix_and_excludes_out_of_range() {
    let mut builder = DataFixerBuilder::new(999);
    let s0 = schema_with_key_and_type(make_key(100), None);
    let s1 = schema_with_key_and_type(make_key(200), None);
    builder.add_schema_obj(s0);
    builder.add_schema_obj(s1.clone());

    let fix = DataFix::with_rule_factory(s1, false, Arc::new(non_nop_rule));
    let fix_rule = fix.get_rule();
    builder.add_fixer(Arc::new(fix));
    let fixer = builder.build();

    // version 150 (key 1500): fixer_versions = [2000]; 1500 < 2000, so
    // expandedVersion = 2000 - 1 = 1999. Fix 2000 > 1999 and fixVersion 200
    // <= 299, so it is included. seq of one rule collapses to that rule.
    let rule = fixer.get_rule(150, 299);
    assert!(Arc::ptr_eq(&rule, &fix_rule));

    // version 250 (key 2500): expandedVersion = last fixer <= 2500 = 2000.
    // expandedFixVersion 2000 > 2000 is false => excluded (even though the
    // fixVersion 200 <= 299). Result is an empty seq, not the fix's rule.
    let rule = fixer.get_rule(250, 299);
    assert!(!rule.is_nop());
    assert!(!Arc::ptr_eq(&rule, &fix_rule));

    // newVersion 99: fixVersion 200 <= 99 is false => excluded.
    let rule = fixer.get_rule(10, 99);
    assert!(!rule.is_nop());
    assert!(!Arc::ptr_eq(&rule, &fix_rule));
}

#[test]
fn get_rule_skips_nop_fix_rules() {
    // A fix whose makeRule() returns nop is skipped (Java:
    // `if (fixRule == TypeRewriteRule.nop()) continue;`).
    let mut builder = DataFixerBuilder::new(999);
    let s1 = schema_with_key_and_type(make_key(200), None);
    builder.add_schema_obj(s1.clone());
    builder.add_fixer(Arc::new(DataFix::new(s1, false)));
    let fixer = builder.build();

    // Only fix has a nop rule => empty rule seq (not the nop singleton).
    let rule = fixer.get_rule(100, 299);
    assert!(!rule.is_nop());
}

// ---------------------------------------------------------------------------
// DataFixerUpper.update — no-op and passthrough
// ---------------------------------------------------------------------------

#[test]
fn update_is_noop_when_version_ge_new_version() {
    let mut builder = DataFixerBuilder::new(999);
    let s0 = schema_with_key_and_type(make_key(100), None);
    let s1 = schema_with_key_and_type(make_key(200), None);
    builder.add_schema_obj(s0);
    builder.add_schema_obj(s1.clone());
    builder.add_fixer(Arc::new(DataFix::new(s1, false)));
    let fixer = builder.build();

    let ops = JsonOps::INSTANCE;
    let input = Dynamic::new(&ops, ops.create_string("hello".to_string()));
    // version == newVersion => input unchanged.
    let out = fixer.update(&ops, "x", &input, 200, 200);
    assert_eq!(out.value, ops.create_string("hello".to_string()));
    // version > newVersion => input unchanged.
    let out = fixer.update(&ops, "x", &input, 300, 200);
    assert_eq!(out.value, ops.create_string("hello".to_string()));
}

#[test]
fn update_preserves_value_when_rewrite_chain_is_empty() {
    // A nop-rule fix yields an empty rewrite chain, so `readAndWrite` returns
    // the input unchanged (Java `resultOrPartial(...).orElse(input)`).
    let mut builder = DataFixerBuilder::new(999);
    let s0 = schema_with_key_and_type(make_key(100), None);
    let s1 = schema_with_key_and_type(make_key(200), None);
    builder.add_schema_obj(s0);
    builder.add_schema_obj(s1.clone());
    builder.add_fixer(Arc::new(DataFix::new(s1, false)));
    let fixer = builder.build();

    let ops = JsonOps::INSTANCE;
    let input = Dynamic::new(&ops, ops.create_string("value".to_string()));
    let out = fixer.update(&ops, "x", &input, 100, 200);
    assert_eq!(out.value, ops.create_string("value".to_string()));
}

// ---------------------------------------------------------------------------
// DataFixerUpper.getSchema — lowest-schema lookup
// ---------------------------------------------------------------------------

#[test]
fn get_schema_uses_lowest_schema_same_version() {
    let mut builder = DataFixerBuilder::new(999);
    let s0 = schema_with_key_and_type(make_key(100), None);
    let s1 = schema_with_key_and_type(make_key(200), None);
    builder.add_schema_obj(s0);
    builder.add_schema_obj(s1);
    let fixer = builder.build();

    // Schema keys 1000, 2000. getSchema(1500) => v100 (key 1000).
    assert_eq!(fixer.get_schema(1500).unwrap().get_version_key(), 1000);
    assert_eq!(fixer.get_schema(2500).unwrap().get_version_key(), 2000);
    // Below the first schema key, Java returns the first schema.
    assert_eq!(fixer.get_schema(999).unwrap().get_version_key(), 1000);
}

// ---------------------------------------------------------------------------
// DataFix.getInputSchema — the changesType flag
// ---------------------------------------------------------------------------

#[test]
fn data_fix_changes_type_parent_lookup() {
    let parent = schema_with_key_and_type(make_key(100), None);
    let child = schema_with_key_and_type(make_key(200), Some(parent.clone()));
    let fix = DataFix::new(child.clone(), true);
    // changesType => input schema is the output schema's parent.
    assert_eq!(fix.get_input_schema().get_version_key(), 1000);
    assert_eq!(fix.get_output_schema().get_version_key(), 2000);
    assert_eq!(fix.get_version_key(), 2000);

    // changesType = false => input schema is the output schema itself.
    let fix2 = DataFix::new(child, false);
    assert_eq!(fix2.get_input_schema().get_version_key(), 2000);
}
