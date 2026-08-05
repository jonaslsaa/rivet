//! Ported DFU tests: `DataResult`, `Either`, `Pair`, `Lifecycle` — the pure
//! value layer (no `DynamicOps` implementation needed).
//!
//! These mirror the DFU test suite's coverage of the error-accumulation and
//! applicative semantics (`DataResult.apply2`/`apply2stable`/`ap`, partial
//! propagation, lifecycle joining).

use rivet_serialization::DataResult;
use rivet_serialization::data_result::{ap2, ap3};
use rivet_serialization::either::Either;
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::pair::Pair;
use std::sync::Arc;

#[test]
fn success_holds_value_with_experimental_lifecycle() {
    let r = DataResult::success(5_i32);
    assert_eq!(r.result(), Some(&5));
    assert_eq!(r.lifecycle(), Lifecycle::experimental());
    assert!(!r.is_error());
}

#[test]
fn success_with_lifecycle_stable() {
    let r = DataResult::success_with_lifecycle(5_i32, Lifecycle::stable());
    assert_eq!(r.result(), Some(&5));
    assert_eq!(r.lifecycle(), Lifecycle::stable());
}

#[test]
fn error_has_no_result_and_carries_message() {
    let r: DataResult<i32> = DataResult::error("boom");
    assert_eq!(r.result(), None);
    assert!(r.is_error());
    let e = r.error_ref().expect("error");
    assert_eq!(e.message(), "boom");
    assert_eq!(e.partial(), &None);
}

#[test]
fn error_with_partial_keeps_partial_but_not_result() {
    let r: DataResult<i32> = DataResult::error_with_partial("boom", 42);
    assert_eq!(r.result(), None);
    assert!(r.has_result_or_partial());
    assert_eq!(r.clone().result_or_partial_silent(), Some(42));
    let e = r.error_ref().unwrap();
    assert_eq!(e.partial(), &Some(42));
}

#[test]
fn map_transforms_success() {
    let r = DataResult::success(5_i32).map(|v| *v + 1);
    assert_eq!(r.result(), Some(&6));
}

#[test]
fn map_on_error_keeps_error() {
    let r: DataResult<i32> = DataResult::error("nope").map(|v: &i32| *v + 1);
    assert_eq!(r.result(), None);
    assert!(r.is_error());
}

#[test]
fn map_error_transforms_message() {
    let r: DataResult<i32> = DataResult::error("nope").map_error(|e| format!("prefixed: {}", e));
    assert_eq!(r.error_ref().unwrap().message(), "prefixed: nope");
}

#[test]
fn flat_map_success_chains() {
    let r = DataResult::success(5_i32).flat_map(|v| DataResult::success(v + 2));
    assert_eq!(r.result(), Some(&7));
}

#[test]
fn flat_map_error_preserves_error_without_calling_function() {
    let r: DataResult<i32> =
        DataResult::error("bad").flat_map(|_: i32| DataResult::error("unused"));
    assert_eq!(r.result(), None);
    assert_eq!(r.error_ref().unwrap().message(), "bad");
}

#[test]
fn flat_map_error_without_partial_is_returned_unchanged() {
    // Java `Error.flatMap`: `if (partialValue.isEmpty()) return this;` — the
    // function is never called, and the message is not accumulated.
    let r: DataResult<i32> =
        DataResult::error("first").flat_map(|_: i32| DataResult::error("second"));
    assert!(r.is_error());
    assert_eq!(r.error_ref().unwrap().message(), "first");
}

#[test]
fn flat_map_accumulates_messages_when_both_have_partials() {
    let r: DataResult<i32> = DataResult::error_with_partial("first", 1)
        .flat_map(|_: i32| DataResult::error_with_partial("second", 2));
    assert!(r.is_error());
    assert_eq!(r.error_ref().unwrap().message(), "first; second");
}

#[test]
fn apply2_combines_successes() {
    let a = DataResult::success(1_i32);
    let b = DataResult::success(2_i32);
    let combined = a.apply2(|x, y| *x + *y, b);
    assert_eq!(combined.result(), Some(&3));
}

#[test]
fn apply2_propagates_error_and_accumulates_messages() {
    // Java `ap2(f, a, b)` = `b.ap(a.ap(f))`; `Error.ap` appends
    // `message + "; " + functionError.message`, so the *second* result's
    // message comes first.
    let a: DataResult<i32> = DataResult::error("left error");
    let b: DataResult<i32> = DataResult::error("right error");
    let combined = a.apply2(|x, y| *x + *y, b);
    assert!(combined.is_error());
    assert_eq!(
        combined.error_ref().unwrap().message(),
        "right error; left error"
    );
}

#[test]
fn apply2_stable_joins_lifecycles() {
    let a = DataResult::success_with_lifecycle(1_i32, Lifecycle::stable());
    let b = DataResult::success_with_lifecycle(2_i32, Lifecycle::stable());
    let combined = a.apply2_stable(|x, y| *x + *y, b);
    assert_eq!(combined.result(), Some(&3));
    // stable() combined with the stable-flagged function stays stable.
    assert_eq!(combined.lifecycle(), Lifecycle::stable());
}

#[test]
fn ap_free_function_fast_path() {
    let f: DataResult<Arc<dyn Fn(&i32, &i32) -> i32>> =
        DataResult::success(Arc::new(|a: &i32, b: &i32| a + b));
    let a = DataResult::success(4_i32);
    let b = DataResult::success(5_i32);
    let r = ap2(f, a, b);
    assert_eq!(r.result(), Some(&9));
}

#[test]
fn ap3_free_function_fast_path() {
    let f: DataResult<Arc<dyn Fn(&i32, &i32, &i32) -> i32>> =
        DataResult::success(Arc::new(|a: &i32, b: &i32, c: &i32| a + b + c));
    let a = DataResult::success(1_i32);
    let b = DataResult::success(2_i32);
    let c = DataResult::success(3_i32);
    let r = ap3(f, a, b, c);
    assert_eq!(r.result(), Some(&6));
}

#[test]
fn set_partial_on_error_replaces_partial() {
    let r: DataResult<i32> = DataResult::error("boom").set_partial(7);
    assert!(r.is_error());
    assert_eq!(r.result_or_partial_silent(), Some(7));
}

#[test]
fn set_partial_on_success_is_noop() {
    let r = DataResult::success(5_i32).set_partial(9);
    assert_eq!(r.result(), Some(&5));
}

#[test]
fn promote_partial_turns_error_with_partial_into_success() {
    let mut seen: Option<String> = None;
    let r: DataResult<i32> = DataResult::error_with_partial("warn", 42).promote_partial(|e| {
        seen = Some(e.to_string());
    });
    assert_eq!(r.result(), Some(&42));
    assert_eq!(seen.as_deref(), Some("warn"));
}

#[test]
fn set_lifecycle_overrides() {
    let r = DataResult::success(1_i32)
        .set_lifecycle(Lifecycle::stable())
        .set_lifecycle(Lifecycle::experimental());
    assert_eq!(r.lifecycle(), Lifecycle::experimental());
}

#[test]
fn add_lifecycle_joins() {
    // `success` defaults to experimental; adding stable keeps experimental.
    let r = DataResult::success_with_lifecycle(1_i32, Lifecycle::stable())
        .add_lifecycle(Lifecycle::stable());
    assert_eq!(r.lifecycle(), Lifecycle::stable());
}

#[test]
fn get_or_throw_returns_success_value() {
    let r = DataResult::success(5_i32);
    assert_eq!(*r.get_or_throw("missing"), 5);
}

#[test]
fn get_partial_or_throw_returns_partial() {
    let r: DataResult<i32> = DataResult::error_with_partial("boom", 42);
    assert_eq!(*r.get_partial_or_throw("missing"), 42);
}

// ---------------------------------------------------------------------------
// Either / Pair
// ---------------------------------------------------------------------------

#[test]
fn either_left_right_accessors() {
    let l: Either<i32, &str> = Either::left(1);
    let r: Either<i32, &str> = Either::right("x");
    assert_eq!(l.left_opt(), Some(&1));
    assert_eq!(l.right_opt(), None);
    assert_eq!(r.right_opt(), Some(&"x"));
}

#[test]
fn either_map_folds() {
    let l: Either<i32, &str> = Either::left(1);
    let folded = l.map(|v| v + 1, |_s| 0);
    assert_eq!(folded, 2);
}

#[test]
fn either_swap() {
    let l: Either<i32, &str> = Either::left(1);
    let swapped = l.swap();
    assert_eq!(swapped.right_opt(), Some(&1));
}

#[test]
fn either_or_throw_returns_left() {
    let l: Either<i32, &str> = Either::left(42);
    assert_eq!(l.or_throw(), 42);
}

#[test]
fn pair_fields_and_swap() {
    let p = Pair::of(1_i32, "x");
    assert_eq!(p.get_first(), &1);
    assert_eq!(p.get_second(), &"x");
    let swapped = p.swap();
    assert_eq!(swapped.get_first(), &"x");
    assert_eq!(swapped.get_second(), &1);
}

#[test]
fn pair_map_fields() {
    let p = Pair::of(1_i32, "x");
    let mapped = p.map_first(|f| f + 1).map_second(|s| s.len());
    assert_eq!(mapped, Pair::of(2, 1));
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_experimental_wins_over_stable() {
    // Java `Lifecycle.add`: experimental wins (it is the most informative).
    assert_eq!(
        Lifecycle::experimental().add(Lifecycle::stable()),
        Lifecycle::experimental()
    );
    assert_eq!(
        Lifecycle::stable().add(Lifecycle::experimental()),
        Lifecycle::experimental()
    );
}

#[test]
fn lifecycle_stable_add_stable_is_stable() {
    assert_eq!(
        Lifecycle::stable().add(Lifecycle::stable()),
        Lifecycle::stable()
    );
}

#[test]
fn lifecycle_deprecated_keeps_oldest() {
    assert_eq!(
        Lifecycle::deprecated(3).add(Lifecycle::deprecated(1)),
        Lifecycle::deprecated(1)
    );
}
