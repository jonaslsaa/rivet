//! Java's `String.hashCode`, `Objects.hash`, and `Long.hashCode` algorithms.
//!
//! Brigadier uses `String.hashCode`, `Objects.hash`, `31 * ... + ...` chains, and
//! `Long.hashCode` in `equals`/`hashCode` implementations and in `CommandNode`'s
//! `LinkedHashMap` keys (via `HashMap.hash`). These are only observable when hashes
//! are compared across Rust/Java (equality tests); PORTING.md keeps Java hash
//! algorithms in `rivet-util::java_hash`, but brigadier is a leaf crate with no
//! workspace deps, so the few needed ones live here.

/// Java `String.hashCode()`: `s[0]*31^(n-1) + s[1]*31^(n-2) + ... + s[n-1]`,
/// wrapping `i32` arithmetic.
pub fn string_hash(s: &str) -> i32 {
    let mut hash: i32 = 0;
    for &byte in s.as_bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as i32);
    }
    hash
}

/// Java `Objects.hash(Object...)`: `Arrays.hashCode(values)` = seed 1, then
/// `31 * hash + element_hash` for each value.
pub fn objects_hash(items: &[i32]) -> i32 {
    let mut hash: i32 = 1;
    for &item in items {
        hash = hash.wrapping_mul(31).wrapping_add(item);
    }
    hash
}

/// Java `Long.hashCode(value)`: `(int) (value ^ (value >>> 32))`.
pub fn long_hash(value: i64) -> i32 {
    ((value as u64) ^ ((value as u64) >> 32)) as i32
}

/// Java `Boolean.hashCode(value)`: `value ? 1231 : 1237`.
pub fn boolean_hash(value: bool) -> i32 {
    if value { 1231 } else { 1237 }
}

/// Java `Double.hashCode(value)`: bits of the `double` then `Long.hashCode`.
pub fn double_hash(value: f64) -> i32 {
    long_hash(value.to_bits() as i64)
}

/// Java `Float.hashCode(value)`: bits of the `float`.
pub fn float_hash(value: f32) -> i32 {
    value.to_bits() as i32
}

/// Java `Objects.hash(Object...)` for a single `String` (used by `ParsedArgument`,
/// `StringRange`, `Suggestion`).
pub fn objects_hash_single_string(s: &str) -> i32 {
    objects_hash(&[string_hash(s)])
}

/// Java `Objects.hash(a, b)` where both are `String`s (used by `ParsedArgument`).
pub fn objects_hash_two_strings(a: &str, b: &str) -> i32 {
    objects_hash(&[string_hash(a), string_hash(b)])
}
