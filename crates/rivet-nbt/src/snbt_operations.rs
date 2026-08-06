//! Port of `net.minecraft.nbt.SnbtOperations` — the built-in `name(args)`
//! operations the SNBT grammar can invoke from an unquoted token
//! (`bool(...)`, `uuid(...)`).
//!
//! Java holds them as a `Map<BuiltinKey, BuiltinOperation>` of anonymous
//! classes over `DynamicOps<T>` + `ParseState`. The packrat/Grammar layer is
//! STUB(mc.nbt.snbt) (owned by the `net.minecraft.util.parsing.packrat`
//! package, not yet ported), so the operations are re-encoded here as a small
//! `BuiltinOp` enum over `NbtOps` (`Tag`). The accepted syntax and the stored
//! error messages are identical to Java; only the dispatch mechanism differs
//! (enum match instead of a closure map).

use crate::nbt_ops::NbtOps;
use crate::tag::Tag;
use rivet_serialization::dynamic_ops::DynamicOps;

/// `SnbtOperations.BUILTIN_TRUE` — `"true"`.
pub const BUILTIN_TRUE: &str = "true";
/// `SnbtOperations.BUILTIN_FALSE` — `"false"`.
pub const BUILTIN_FALSE: &str = "false";

/// `SnbtOperations.BuiltinKey(String id, int argCount)` — the map key
/// `name(argCount)` that selects an operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BuiltinKey {
    pub id: String,
    pub arg_count: usize,
}

impl BuiltinKey {
    pub fn new(id: String, arg_count: usize) -> Self {
        BuiltinKey { id, arg_count }
    }
}

impl std::fmt::Display for BuiltinKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Java `BuiltinKey.toString()` — `this.id + "/" + this.argCount`.
        write!(f, "{}/{}", self.id, self.arg_count)
    }
}

/// The error an operation stores when its argument is not convertible.
///
/// Java stores a `DelayedException` of the matching
/// `SimpleCommandExceptionType` on the `ParseState`; the parser stores the
/// corresponding message text at the current cursor instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinError {
    /// `snbt.parser.expected_number_or_boolean` — "Expected a number or a boolean".
    ExpectedNumberOrBoolean,
    /// `snbt.parser.expected_string_uuid` — "Expected a string representing a valid UUID".
    ExpectedStringUuid,
}

impl BuiltinError {
    pub fn message(&self) -> &'static str {
        match self {
            BuiltinError::ExpectedNumberOrBoolean => "Expected a number or a boolean",
            BuiltinError::ExpectedStringUuid => "Expected a string representing a valid UUID",
        }
    }
}

/// The two builtin operations (`SnbtOperations.BUILTIN_OPERATIONS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinOp {
    /// `new BuiltinKey("bool", 1)` — number/boolean → boolean tag.
    Bool,
    /// `new BuiltinKey("uuid", 1)` — string → 4-int list tag.
    Uuid,
}

/// Look up a builtin by `BuiltinKey`. Java: `BUILTIN_OPERATIONS.get(key)`.
pub fn find_builtin(key: &BuiltinKey) -> Option<BuiltinOp> {
    match (key.id.as_str(), key.arg_count) {
        ("bool", 1) => Some(BuiltinOp::Bool),
        ("uuid", 1) => Some(BuiltinOp::Uuid),
        _ => None,
    }
}

/// `SnbtOperations.BUILTIN_IDS` — the `false`/`true` literals plus the builtin
/// ids, used for suggestions. Kept as a plain function (the Java side wraps it
/// in a `SuggestionSupplier` for the suggestions path).
pub fn builtin_ids() -> Vec<&'static str> {
    vec![BUILTIN_FALSE, BUILTIN_TRUE, "bool", "uuid"]
}

/// Runs a builtin operation over `NbtOps` (`Tag`).
///
/// Java signature: `<T> @Nullable T run(DynamicOps<T> ops, List<T> arguments,
/// ParseState<StringReader> state)`. `ops` here is fixed to `NbtOps` because
/// the only SNBT parser in the crate is `NBT_OPS_PARSER`; the caller stores
/// the returned `BuiltinError` as a parse error at the current cursor.
pub fn run_builtin(op: BuiltinOp, ops: &NbtOps, arguments: &[Tag]) -> Result<Tag, BuiltinError> {
    match op {
        BuiltinOp::Bool => run_bool(ops, arguments),
        BuiltinOp::Uuid => run_uuid(ops, arguments),
    }
}

/// The `bool` operation — `SnbtOperations` anonymous class #1.
///
/// `convert(ops, arg)`: `ops.getBooleanValue(arg).result()` else
/// `ops.getNumberValue(arg).result().map(n -> n.doubleValue() != 0.0)`, else
/// null (→ error).
fn run_bool(ops: &NbtOps, arguments: &[Tag]) -> Result<Tag, BuiltinError> {
    let arg = &arguments[0];
    // Java `SnbtOperations` bool: `getNumberValue(arg).result().map(n ->
    // n.doubleValue() != 0.0)`.
    match arg.as_number_f64() {
        Some(n) => Ok(ops.create_boolean(n != 0.0)),
        None => Err(BuiltinError::ExpectedNumberOrBoolean),
    }
}

/// The `uuid` operation — `SnbtOperations` anonymous class #2.
///
/// Requires the argument to be a string; `UUID.fromString` (see
/// `uuid_from_string`) then `UUIDUtil.uuidToIntArray` → 4-int list tag. The
/// conversion is faithful: Java's `uuidToIntArray` splits the most/least
/// significant bits into 32-bit `>>>` (logical) shifts, identical to the
/// `u64` → `u32` truncation here.
fn run_uuid(ops: &NbtOps, arguments: &[Tag]) -> Result<Tag, BuiltinError> {
    let arg = &arguments[0];
    let s = match arg {
        Tag::String(s) => s.value.as_str(),
        _ => return Err(BuiltinError::ExpectedStringUuid),
    };
    match uuid_from_string(s) {
        Some(uuid) => {
            let msb = (uuid >> 64) as u64;
            let lsb = uuid as u64;
            let ints = vec![
                (msb >> 32) as u32 as i32,
                msb as u32 as i32,
                (lsb >> 32) as u32 as i32,
                lsb as u32 as i32,
            ];
            Ok(ops.create_int_list(ints))
        }
        None => Err(BuiltinError::ExpectedStringUuid),
    }
}

/// `UUID.fromString(String)` for the JDK the workspace builds with (Temurin
/// 25), verified against `java.base/java/util/UUID.java` and empirically.
/// Returns the 128-bit value on success, `None` where Java throws
/// `IllegalArgumentException` (the `uuid(...)` op catches it and stores
/// `ERROR_EXPECTED_STRING_UUID`).
///
/// Java semantics (all verified on JDK 25):
/// - The strict 36-char `8-4-4-4-12` form with hyphens. All 32 non-hyphen
///   chars must be hex: `parse4Nibbles` reads the NIBBLES table which is
///   filled with -1, so a non-hex char yields -1 and the `>= 0` gate fails,
///   falling through to `fromString1`, where `Long.parseLong` throws on the
///   offending group. A non-hex char therefore rejects the whole string.
/// - The 32-char no-hyphen form and the `{...}`/`urn:uuid:` forms are NOT
///   accepted on this JDK (`len > 36` → "UUID string too large"; `len != 36`
///   or wrong dashes → the fallback needs exactly 4 dashes).
/// - Fallback `fromString1`: at most 36 chars, exactly 4 dashes, each group
///   parsed by `Long.parseLong(s, begin, end, 16)` and OR'd in. Groups may be
///   *shorter* than the canonical widths and are left-padded; a `+`/`-` first
///   char is a sign. Out-of-range or non-hex groups throw (→ `None`). E.g.
///   `"1-2-3-4-5"` → `00000001-0002-0003-0004-000000000005`.
fn uuid_from_string(s: &str) -> Option<u128> {
    fn hex_digit(c: u16) -> Option<u32> {
        match c {
            0x30..=0x39 => Some((c - 0x30) as u32),      // 0-9
            0x61..=0x66 => Some((c - 0x61 + 10) as u32), // a-f
            0x41..=0x46 => Some((c - 0x41 + 10) as u32), // A-F
            _ => None,
        }
    }

    let units: Vec<u16> = s.encode_utf16().collect();

    // Fast path (JDK `UUID.fromString`): the 36-char canonical form with all
    // content hex. The JDK's `parse4Nibbles` at the fixed offsets {0,4,9,14,
    // 19,24,28,32}; non-hex content falls through to `fromString1` (rejected
    // by `Long.parseLong` on the group), so here it is rejected outright.
    if units.len() == 36
        && units[8] == b'-' as u16
        && units[13] == b'-' as u16
        && units[18] == b'-' as u16
        && units[23] == b'-' as u16
    {
        let content_is_hex =
            (0..36).all(|i| matches!(i, 8 | 13 | 18 | 23) || hex_digit(units[i]).is_some());
        if content_is_hex {
            let nib = |pos: usize| -> u128 {
                (hex_digit(units[pos]).unwrap() << 12
                    | hex_digit(units[pos + 1]).unwrap() << 8
                    | hex_digit(units[pos + 2]).unwrap() << 4
                    | hex_digit(units[pos + 3]).unwrap()) as u128
            };
            let msb = nib(0) << 48 | nib(4) << 32 | nib(9) << 16 | nib(14);
            let lsb = nib(19) << 48 | nib(24) << 32 | nib(28) << 16 | nib(32);
            return Some(msb << 64 | lsb);
        }
        // Non-hex content: fall through to `fromString1`, which rejects it.
    }

    // Fallback `fromString1`: at most 36 chars, exactly 4 dashes.
    if units.len() > 36 {
        return None;
    }
    let dash: Vec<usize> = units
        .iter()
        .enumerate()
        .filter(|&(_, &c)| c == b'-' as u16)
        .map(|(i, _)| i)
        .collect();

    if dash.len() != 4 {
        return None;
    }
    let (d1, d2, d3, d4) = (dash[0], dash[1], dash[2], dash[3]);
    let parse = |begin: usize, end: usize| -> Option<i64> {
        if begin >= end {
            return None; // empty group: `Long.parseLong` throws
        }
        let (neg, first) = match units[begin] {
            c if c == b'-' as u16 => (true, begin + 1),
            c if c == b'+' as u16 => (false, begin + 1),
            _ => (false, begin),
        };
        if first >= end {
            return None;
        }
        let mut acc: i64 = 0;
        for &c in &units[first..end] {
            let digit = hex_digit(c)? as i64;
            acc = acc.checked_mul(16)?.checked_add(digit)?;
        }
        Some(if neg { 0i64.wrapping_sub(acc) } else { acc })
    };
    let most_sig_bits = (parse(0, d1)? as u64 & 0xffff_ffff) << 32
        | (parse(d1 + 1, d2)? as u64 & 0xffff) << 16
        | (parse(d2 + 1, d3)? as u64 & 0xffff);
    let least_sig_bits = (parse(d3 + 1, d4)? as u64 & 0xffff) << 48
        | (parse(d4 + 1, units.len())? as u64 & 0xffff_ffff_ffff);
    let most = most_sig_bits as u128;
    let least = least_sig_bits as u128;
    Some(most << 64 | least)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_key_display_matches_java() {
        // Java `BuiltinKey.toString()` = `id + "/" + argCount`.
        assert_eq!(BuiltinKey::new("bool".to_string(), 1).to_string(), "bool/1");
        assert_eq!(BuiltinKey::new("uuid".to_string(), 2).to_string(), "uuid/2");
    }

    #[test]
    fn find_builtin_matches_java_map() {
        let ops = &[
            BuiltinKey::new("bool".to_string(), 1),
            BuiltinKey::new("uuid".to_string(), 1),
            BuiltinKey::new("bool".to_string(), 2),
            BuiltinKey::new("nope".to_string(), 1),
        ];
        assert_eq!(find_builtin(&ops[0]), Some(BuiltinOp::Bool));
        assert_eq!(find_builtin(&ops[1]), Some(BuiltinOp::Uuid));
        assert_eq!(find_builtin(&ops[2]), None);
        assert_eq!(find_builtin(&ops[3]), None);
    }

    #[test]
    fn bool_operation_converts_numbers() {
        let ops = NbtOps::instance();
        // bool(1) → true → ByteTag(1); bool(0) → false → ByteTag(0).
        use crate::byte_tag::ByteTag;
        let one = Tag::Byte(ByteTag::value_of(1));
        let zero = Tag::Byte(ByteTag::value_of(0));
        assert_eq!(
            run_builtin(BuiltinOp::Bool, &ops, std::slice::from_ref(&one)),
            Ok(Tag::Byte(ByteTag::value_of_bool(true)))
        );
        assert_eq!(
            run_builtin(BuiltinOp::Bool, &ops, std::slice::from_ref(&zero)),
            Ok(Tag::Byte(ByteTag::value_of_bool(false)))
        );
        // bool(true) → true.
        assert_eq!(
            run_builtin(BuiltinOp::Bool, &ops, std::slice::from_ref(&one)),
            Ok(Tag::Byte(ByteTag::value_of_bool(true)))
        );
        // Non-numeric → error.
        use crate::string_tag::StringTag;
        let s = Tag::String(StringTag::value_of("x".to_string()));
        assert_eq!(
            run_builtin(BuiltinOp::Bool, &ops, &[s]),
            Err(BuiltinError::ExpectedNumberOrBoolean)
        );
    }

    #[test]
    fn uuid_operation_matches_uuid_to_int_array() {
        let ops = NbtOps::instance();
        // Zero UUID → [0, 0, 0, 0].
        let uuid = "00000000-0000-0000-0000-000000000000";
        use crate::int_array_tag::IntArrayTag;
        assert_eq!(
            run_builtin(
                BuiltinOp::Uuid,
                &ops,
                &[Tag::String(crate::string_tag::StringTag::value_of(
                    uuid.to_string()
                ))]
            ),
            Ok(Tag::IntArray(IntArrayTag::new(vec![0, 0, 0, 0])))
        );
        // A known UUID: msb = 0x0102030405060708, lsb = 0x090a0b0c0d0e0f10.
        let uuid = "01020304-0506-0708-090a-0b0c0d0e0f10";
        assert_eq!(
            run_builtin(
                BuiltinOp::Uuid,
                &ops,
                &[Tag::String(crate::string_tag::StringTag::value_of(
                    uuid.to_string()
                ))]
            ),
            Ok(Tag::IntArray(IntArrayTag::new(vec![
                0x01020304, 0x05060708, 0x090a0b0c, 0x0d0e0f10
            ])))
        );
        // Non-string → error.
        use crate::int_tag::IntTag;
        assert_eq!(
            run_builtin(BuiltinOp::Uuid, &ops, &[Tag::Int(IntTag::new(5))]),
            Err(BuiltinError::ExpectedStringUuid)
        );
        // Malformed UUID string → error.
        assert_eq!(
            run_builtin(
                BuiltinOp::Uuid,
                &ops,
                &[Tag::String(crate::string_tag::StringTag::value_of(
                    "not-a-uuid".to_string()
                ))]
            ),
            Err(BuiltinError::ExpectedStringUuid)
        );
    }

    #[test]
    fn uuid_from_string_matches_jdk_25() {
        use crate::int_array_tag::IntArrayTag;

        // A "uuid(...)" test helper that maps the raw value to the int array.
        let arr = |s: &str| -> Tag {
            run_builtin(
                BuiltinOp::Uuid,
                &NbtOps::instance(),
                &[Tag::String(crate::string_tag::StringTag::value_of(
                    s.to_string(),
                ))],
            )
            .unwrap()
        };

        // Canonical form → the 4 ints.
        assert_eq!(
            arr("01020304-0506-0708-090a-0b0c0d0e0f10"),
            Tag::IntArray(IntArrayTag::new(vec![
                0x01020304, 0x05060708, 0x090a0b0c, 0x0d0e0f10
            ]))
        );
        // Non-hex content in the fast path is rejected (parse4Nibbles maps a
        // non-hex char to -1, and fromString1 then throws on the group).
        assert_eq!(
            run_builtin(
                BuiltinOp::Uuid,
                &NbtOps::instance(),
                &[Tag::String(crate::string_tag::StringTag::value_of(
                    "gggggggg-gggg-gggg-gggg-gggggggggggg".to_string()
                ))],
            ),
            Err(BuiltinError::ExpectedStringUuid)
        );
        // Fallback: short groups are left-padded; "1-2-3-4-5" → the padded uuid.
        assert_eq!(
            arr("1-2-3-4-5"),
            Tag::IntArray(IntArrayTag::new(vec![1, 131075, 262144, 5]))
        );
        // Fallback: a leading '-' in group 1 makes the first group negative
        // (5 dashes → rejected).
        assert_eq!(
            run_builtin(
                BuiltinOp::Uuid,
                &NbtOps::instance(),
                &[Tag::String(crate::string_tag::StringTag::value_of(
                    "-1-2-3-4-5".to_string()
                ))],
            ),
            Err(BuiltinError::ExpectedStringUuid)
        );
        // 32-char no-hyphen form: NOT accepted on this JDK (len 32 != 36, and
        // the fallback requires exactly 4 dashes).
        assert_eq!(
            run_builtin(
                BuiltinOp::Uuid,
                &NbtOps::instance(),
                &[Tag::String(crate::string_tag::StringTag::value_of(
                    "0102030405060708090a0b0c0d0e0f10".to_string()
                ))],
            ),
            Err(BuiltinError::ExpectedStringUuid)
        );
        // Brace-wrapped / urn:uuid: forms: NOT accepted (len > 36).
        assert_eq!(
            run_builtin(
                BuiltinOp::Uuid,
                &NbtOps::instance(),
                &[Tag::String(crate::string_tag::StringTag::value_of(
                    "{01020304-0506-0708-090a-0b0c0d0e0f10}".to_string()
                ))],
            ),
            Err(BuiltinError::ExpectedStringUuid)
        );
        // Too-large groups (2^64-1) → None.
        assert_eq!(
            run_builtin(
                BuiltinOp::Uuid,
                &NbtOps::instance(),
                &[Tag::String(crate::string_tag::StringTag::value_of(
                    "10000000000000000-0-0-0-0".to_string()
                ))],
            ),
            Err(BuiltinError::ExpectedStringUuid)
        );
        // Left-padded last group (35-char form) — JDK-verified:
        // lsb = 0x090a << 48 | 0x00b0c0d0e0f1, so int3 = 0x090a00b0 and
        // int4 = 0xc0d0e0f1 (= -1060052751 as signed i32).
        assert_eq!(
            arr("01020304-0506-0708-090a-00b0c0d0e0f1"),
            Tag::IntArray(IntArrayTag::new(vec![
                0x01020304,
                0x05060708,
                0x090a00b0,
                -1060052751
            ]))
        );
    }
}
