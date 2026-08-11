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
//! - `update` end-to-end with a real non-nop rewrite: a `View.create` + compose
//!   rule whose function transforms the value, exercising the full
//!   `read -> rewrite -> eval_cached -> cap_write -> write` path.
//! - `SumType.read` / `EmptyPartPassthrough.write`: the `EitherCodec.decode`
//!   error-with-partial fallback and the wrong-typed-value error.
//! - `add_schema_obj`: a repeated version key replaces the prior schema (Java's
//!   sorted-map `put`), keeping the `Vec` sorted and duplicate-free.

use rivet_serialization::data_result::DataResult;
use rivet_serialization::datafixers::data_fix::DataFix;
use rivet_serialization::datafixers::data_fix_utils::{
    get_sub_version, get_version, make_key, make_key_sub,
};
use rivet_serialization::datafixers::data_fixer_builder::DataFixerBuilder;
use rivet_serialization::datafixers::data_fixer_upper::{
    DataFixerUpper, get_lowest_schema_same_version,
};
use rivet_serialization::datafixers::rewrite_result::RewriteResult;
use rivet_serialization::datafixers::schemas::Schema;
use rivet_serialization::datafixers::type_rewrite_rule;
use rivet_serialization::datafixers::types::templates::{
    Const, EmptyPart, EmptyPartPassthrough, PrimitiveType, SumType,
};
use rivet_serialization::datafixers::types::{AnyValue, Type, TypeTemplate, any};
use rivet_serialization::datafixers::view::View;
use rivet_serialization::dynamic::Dynamic;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::json_ops::JsonOps;
use std::any::Any;
use std::sync::Arc;

type Json = JsonOps;

/// The `JsonOps` value type, fully qualified (a bare `JsonOutput` through the
/// alias is ambiguous to the compiler).
type JsonOutput = <Json as DynamicOps>::Output;

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

/// Constructs a `PrimitiveType<String, Json>` from `codec::string_codec`.
fn string_primitive() -> Arc<dyn Type<Json>> {
    Arc::new(PrimitiveType {
        codec: rivet_serialization::codec::string_codec::<Json>(),
    })
}

/// Constructs a `PrimitiveType<i32, Json>` from `codec::int_codec`.
fn int_primitive() -> Arc<dyn Type<Json>> {
    Arc::new(PrimitiveType {
        codec: rivet_serialization::codec::int_codec::<Json>(),
    })
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

// ---------------------------------------------------------------------------
// DataFixerUpper.update — end-to-end value-transforming rewrite
// ---------------------------------------------------------------------------

/// Builds the two schemas and a fix whose `makeRule` rewrites the `"name"`
/// wrapper into an uppercasing function (a genuine non-nop value transform).
fn fixer_with_uppercase_rule() -> DataFixerUpper<Json> {
    // v100 schema has a `PrimitiveType<String>` named `"name"`; v200 same but
    // parented, and the fix targets v200.
    let s0 = {
        let mut s = Schema::new(make_key(100), None);
        let template: Arc<dyn TypeTemplate<Json>> = Arc::new(Const::new(string_primitive()));
        s.type_templates.insert("name".to_string(), template);
        s.types = s.build_types();
        Arc::new(s)
    };
    let s1 = {
        let mut s = Schema::new(make_key(200), Some(s0.clone()));
        let template: Arc<dyn TypeTemplate<Json>> = Arc::new(Const::new(string_primitive()));
        s.type_templates.insert("name".to_string(), template);
        s.types = s.build_types();
        Arc::new(s)
    };
    // The view's input/output must be the actual schema type instances:
    // `PrimitiveType::equals_` is reference identity, and `cap_write` compares
    // the view's output against the (same-instance) expected type.
    let input_ty = s0.get_type_raw("name");
    let output_ty = s1.get_type_raw("name");
    let fix = DataFix::with_rule_factory(
        s1.clone(),
        false,
        Arc::new(move || {
            // makeRule: a `TypeRewriteRule` whose rewrite replaces the view's
            // function with an uppercasing transform.
            struct UpperRule {
                input: Arc<dyn Type<Json>>,
                output: Arc<dyn Type<Json>>,
            }
            impl std::fmt::Debug for UpperRule {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "UpperRule")
                }
            }
            impl type_rewrite_rule::TypeRewriteRule<Json> for UpperRule {
                fn rewrite(&self, _ty: &dyn Type<Json>) -> Option<RewriteResult<Json>> {
                    let view = View::create(
                        "name".to_string(),
                        self.input.clone(),
                        self.output.clone(),
                        Arc::new(|_ops: &Json, value: &AnyValue| {
                            // The decoded element of a `PrimitiveType<String>` is a
                            // `String` (Java's typed `A`), so the transform operates
                            // on it directly.
                            let s = value.downcast_ref::<String>().expect("string value");
                            any(s.to_uppercase())
                        }),
                    );
                    Some(RewriteResult::create(view, Vec::new()))
                }
                fn clone_rule(&self) -> Arc<dyn type_rewrite_rule::TypeRewriteRule<Json>> {
                    Arc::new(UpperRule {
                        input: self.input.clone(),
                        output: self.output.clone(),
                    })
                }
            }
            Arc::new(UpperRule {
                input: input_ty.clone(),
                output: output_ty.clone(),
            }) as Arc<dyn type_rewrite_rule::TypeRewriteRule<Json>>
        }),
    );
    let mut builder = DataFixerBuilder::new(999);
    builder.add_schema_obj(s0);
    builder.add_schema_obj(s1);
    builder.add_fixer(Arc::new(fix));
    builder.build()
}

#[test]
fn update_runs_a_real_value_transforming_rewrite() {
    // A genuine non-nop fix: `makeRule` replaces the "name" view function with
    // an uppercasing transform. If `cap_write`/`read_and_write` short-circuits
    // or drops the transform, the output would keep the lowercased value.
    let fixer = fixer_with_uppercase_rule();
    let ops = JsonOps::INSTANCE;
    let input = Dynamic::new(&ops, ops.create_string("hello".to_string()));
    let out = fixer.update(&ops, "name", &input, 100, 200);
    assert_eq!(out.value, ops.create_string("HELLO".to_string()));
}

// ---------------------------------------------------------------------------
// SumType.read — EitherCodec.decode parity
// ---------------------------------------------------------------------------

#[test]
fn sum_type_read_returns_second_success() {
    let ops = JsonOps::INSTANCE;
    let input = ops.create_string("42".to_string());
    let int_ty: Arc<dyn Type<Json>> = int_primitive();
    let str_ty: Arc<dyn Type<Json>> = string_primitive();
    let sum = SumType::new(int_ty, str_ty);
    // First branch (int) fails on a string; second (string) succeeds.
    let result = sum.read(&ops, &input);
    assert!(result.is_success());
    let (value, _rest) = result.get_or_throw_unchecked();
    let either = value
        .downcast_ref::<rivet_serialization::either::Either<AnyValue, AnyValue>>()
        .expect("either");
    assert!(matches!(
        either,
        rivet_serialization::either::Either::Right(_)
    ));
}

#[test]
fn sum_type_read_prefers_first_error_with_partial_over_second_plain_error() {
    // Java `EitherCodec.decode`: when first is an error-with-partial and second
    // is a plain error, the FIRST partial is returned (not the second's error).
    let ops = JsonOps::INSTANCE;
    let input = ops.create_string("x".to_string());
    // The first branch must error WITH a partial for the order to matter;
    // `int_primitive` errors without one, so use a shim type as the first.
    #[derive(Debug)]
    struct PartialErrType;
    impl Type<Json> for PartialErrType {
        fn read(&self, ops: &Json, _input: &JsonOutput) -> DataResult<(AnyValue, JsonOutput)> {
            DataResult::error_with_partial(
                "first branch failed".to_string(),
                (
                    any(Dynamic::new(ops, ops.create_string("partial".to_string()))),
                    ops.empty(),
                ),
            )
        }
        fn write(
            &self,
            _ops: &Json,
            _value: &AnyValue,
            prefix: &JsonOutput,
        ) -> DataResult<JsonOutput> {
            DataResult::success(prefix.clone())
        }
        fn equals_(&self, other: &dyn Type<Json>, _i: bool, _c: bool) -> bool {
            other
                .as_any_type()
                .downcast_ref::<PartialErrType>()
                .is_some()
        }
        fn template(&self) -> Arc<dyn TypeTemplate<Json>> {
            Arc::new(Const::new(Arc::new(EmptyPart::new())))
        }
        fn type_to_string(&self) -> String {
            "PartialErrType".to_string()
        }
        fn clone_ty(&self) -> Arc<dyn Type<Json>> {
            Arc::new(PartialErrType)
        }
        fn as_any_type(&self) -> &dyn Any {
            self
        }
    }
    // Second: a plain error (no partial).
    #[derive(Debug)]
    struct PlainErrType;
    impl Type<Json> for PlainErrType {
        fn read(&self, _ops: &Json, _input: &JsonOutput) -> DataResult<(AnyValue, JsonOutput)> {
            DataResult::error("second branch failed")
        }
        fn write(
            &self,
            _ops: &Json,
            _value: &AnyValue,
            prefix: &JsonOutput,
        ) -> DataResult<JsonOutput> {
            DataResult::success(prefix.clone())
        }
        fn equals_(&self, other: &dyn Type<Json>, _i: bool, _c: bool) -> bool {
            other.as_any_type().downcast_ref::<PlainErrType>().is_some()
        }
        fn template(&self) -> Arc<dyn TypeTemplate<Json>> {
            Arc::new(Const::new(Arc::new(EmptyPart::new())))
        }
        fn type_to_string(&self) -> String {
            "PlainErrType".to_string()
        }
        fn clone_ty(&self) -> Arc<dyn Type<Json>> {
            Arc::new(PlainErrType)
        }
        fn as_any_type(&self) -> &dyn Any {
            self
        }
    }
    let sum = SumType::new(
        Arc::new(PartialErrType) as Arc<dyn Type<Json>>,
        Arc::new(PlainErrType) as Arc<dyn Type<Json>>,
    );
    let result = sum.read(&ops, &input);
    assert!(result.has_result_or_partial());
    // The first (partial) value survives: the result is the LEFT branch partial.
    let partial = result.result_or_partial_silent().expect("partial");
    let (value, _rest) = partial;
    let either = value
        .downcast_ref::<rivet_serialization::either::Either<AnyValue, AnyValue>>()
        .expect("either");
    assert!(matches!(
        either,
        rivet_serialization::either::Either::Left(_)
    ));
}

#[test]
fn sum_type_read_combines_error_messages_when_both_plain_errors() {
    let ops = JsonOps::INSTANCE;
    let input = ops.create_string("x".to_string());
    let int_ty: Arc<dyn Type<Json>> = int_primitive();
    let also_int: Arc<dyn Type<Json>> = int_primitive();
    let sum = SumType::new(int_ty, also_int);
    let result = sum.read(&ops, &input);
    assert!(result.is_error());
    let msg = result.error_ref().expect("error").message().to_string();
    assert!(
        msg.contains("Failed to parse either. First:") && msg.contains("Second:"),
        "unexpected combined message: {msg}"
    );
}

// ---------------------------------------------------------------------------
// EmptyPartPassthrough.write — wrong-typed value errors loudly
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "EmptyPartPassthrough value is not a Dynamic")]
fn empty_part_passthrough_write_panics_on_wrong_value_type() {
    let ops = JsonOps::INSTANCE;
    let ty = EmptyPartPassthrough::new();
    let wrong = any(42i32);
    let _ = ty.write(&ops, &wrong, &ops.empty());
}

// ---------------------------------------------------------------------------
// DataFixerBuilder.addSchema — repeated key replaces (Java put)
// ---------------------------------------------------------------------------

#[test]
fn add_schema_obj_repeated_key_replaces_existing() {
    let mut builder = DataFixerBuilder::new(999);
    let s0 = schema_with_key_and_type(make_key(100), None);
    let s0b = schema_with_key_and_type(make_key(100), None);
    builder.add_schema_obj(s0.clone());
    builder.add_schema_obj(s0b.clone());
    let keys: Vec<i32> = builder
        .build()
        .schemas
        .iter()
        .map(|s| s.get_version_key())
        .collect();
    // No duplicate: the later schema replaces the earlier one at the same key.
    assert_eq!(keys, vec![make_key(100)]);
    assert!(Arc::ptr_eq(&builder.build().schemas[0], &s0b));
}
