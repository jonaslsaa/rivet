//! The parity corpus: hand-built SNBT inputs exercising every tag type, plus
//! discovery of the 432 committed M0 chunk-NBT fixtures.

/// Hand-built SNBT inputs for `snbt.parse` (each parses to some tag). These
/// cover every tag type, numeric suffix, array form, string quoting/escaping,
/// Unicode, and the pretty printer's `KEY_ORDER` / `no_indentation` paths.
pub fn parse_corpus() -> Vec<(&'static str, &'static str)> {
    vec![
        // ---- numeric primitives and suffix variants ----
        ("byte-min", "-128b"),
        ("byte-max", "127b"),
        ("byte-upper-B", "5B"),
        ("short-min", "-32768s"),
        ("short-max", "32767s"),
        ("short-upper-S", "5S"),
        ("int-min", "-2147483648"),
        ("int-max", "2147483647"),
        ("int-upper-I", "5I"),
        ("long-min", "-9223372036854775808L"),
        ("long-max", "9223372036854775807L"),
        ("long-upper-L", "5L"),
        ("unsigned-byte", "1ub"),
        ("unsigned-short", "1us"),
        ("unsigned-int", "1ui"),
        ("unsigned-long", "1ul"),
        ("signed-byte", "1sb"),
        ("signed-short", "1ss"),
        ("signed-int", "1si"),
        ("signed-long", "1sl"),
        ("unsigned-byte-upper", "1UB"),
        // ---- integer bases and underscores ----
        ("hex", "0x10"),
        ("hex-upper-X", "0X1F"),
        ("hex-negative", "-0x10"),
        ("hex-unsigned-int-max", "0xFFFFFFFF"),
        ("hex-unsigned-long-max", "0xFFFFFFFFFFFFFFFFL"),
        ("hex-b-is-digit", "0xFFb"),
        ("binary", "0b101"),
        ("binary-upper-B", "0B101"),
        ("binary-long", "0b11111111111111111111111111111111L"),
        ("underscore-thousands", "1_000"),
        ("underscore-hex", "0x1_0"),
        ("underscore-binary", "0b1_0_1"),
        ("float-underscores", "1_0.5"),
        ("zero", "0"),
        ("zero-long", "0L"),
        // ---- floats / doubles across ranges ----
        ("float-one", "1.0f"),
        ("float-zero", "0.0f"),
        ("float-neg-zero", "-0.0f"),
        ("float-half", "0.5f"),
        ("float-neg", "-1.5f"),
        ("float-max", "3.4028235E38f"),
        ("float-min-normal", "1.17549435E-38f"),
        ("float-min-subnormal", "1.4E-45f"),
        ("float-sci-upper", "1.0E7f"),
        ("float-sci-lower", "1.0E-4f"),
        ("float-pi-ish", "3.1415927f"),
        ("double-one", "1.0d"),
        ("double-neg-zero", "-0.0d"),
        ("double-max", "1.7976931348623157E308d"),
        ("double-min-normal", "2.2250738585072014E-308d"),
        ("double-min-subnormal", "4.9E-324d"),
        ("double-sci", "1.0E7d"),
        ("double-pi-ish", "3.141592653589793d"),
        ("plain-double", "2.25"),
        ("dot-five", ".5"),
        ("trailing-dot", "1."),
        ("exp", "1e3"),
        ("exp-neg", "1e-3"),
        ("exp-plus-f", "1e+3f"),
        ("exp-upper-E", "1E3"),
        // ---- strings ----
        ("string-double", "\"hi\""),
        ("string-single", "'hi'"),
        ("string-esc-nl", "\"a\\nb\""),
        ("string-esc-tab", "\"a\\tb\""),
        ("string-esc-backslash", "\"a\\\\b\""),
        ("string-esc-quote", "\"a\\\"b\""),
        ("string-esc-apos", "\"a\\'b\""),
        ("string-literal-single-in-double", "\"a'b\""),
        ("string-literal-double-in-single", "'a\"b'"),
        ("string-hex-x", "\"\\x41\""),
        ("string-hex-u", "\"\\u0041\""),
        ("string-hex-upper-U", "\"\\U00000041\""),
        ("string-esc-backspace", "\"\\b\""),
        ("string-esc-ff", "\"\\f\""),
        ("string-esc-return", "\"\\r\""),
        ("string-esc-space", "\"\\s\""),
        ("string-emoji", "\"\u{1F600}\""),
        ("string-emoji-rocket", "\"\u{1F680}\""),
        ("string-utf8", "\"h\u{e9}llo w\u{f6}rld\""),
        ("string-cjk", "\"\u{65E5}\u{672C}\u{8A9E}\""),
        ("string-empty", "\"\""),
        ("string-single-empty", "''"),
        ("string-unquoted", "hello"),
        ("string-unquoted-underscore", "hello_world"),
        ("string-unquoted-dot", "a.b"),
        ("string-unquoted-dash", "abc-def"),
        ("string-unquoted-plus", "abc+def"),
        ("string-unquoted-underscore-start", "_1"),
        ("string-unquoted-lone", "x"),
        ("string-unquoted-nan", "NaN"),
        // ---- true / false ----
        ("true", "true"),
        ("false", "false"),
        ("true-upper", "TRUE"),
        ("false-mixed", "False"),
        ("true-word", "truex"),
        // ---- builtins ----
        ("builtin-bool-1", "bool(1)"),
        ("builtin-bool-0", "bool(0)"),
        ("builtin-bool-true", "bool(true)"),
        ("builtin-bool-false", "bool(false)"),
        (
            "builtin-uuid",
            "uuid(\"01020304-0506-0708-090a-0b0c0d0e0f10\")",
        ),
        // ---- typed arrays ----
        ("byte-array-empty", "[B;]"),
        ("byte-array", "[B;1B,-1B,2B]"),
        ("byte-array-unsigned", "[B;0ub,255ub]"),
        ("byte-array-mixed-suffix", "[B;1B,2,3b]"),
        ("int-array-empty", "[I;]"),
        ("int-array", "[I;1,2]"),
        ("int-array-widened", "[I;1b,2s]"),
        ("int-array-hex", "[I;0xFF,0b10]"),
        ("int-array-negative", "[I;-1,-2]"),
        ("long-array-empty", "[L;]"),
        ("long-array", "[L;1L,2L]"),
        ("long-array-widened", "[L;1,2]"),
        ("long-array-long-max", "[L;9223372036854775807L]"),
        // ---- lists ----
        ("list-empty", "[]"),
        ("list-ints", "[1,2]"),
        ("list-mixed", "[1,\"a\"]"),
        ("list-strings", "[\"a\",\"b\"]"),
        ("list-compounds", "[{a:1},{a:2}]"),
        ("list-doubles", "[1.5d,2.5d]"),
        ("list-longs", "[1L,2L]"),
        ("list-nested", "[[1],[2]]"),
        ("list-nested-arrays", "[[B;1B],[B;2B]]"),
        ("list-trailing-comma", "[1,]"),
        ("list-single", "[42]"),
        // ---- compounds ----
        ("compound-empty", "{}"),
        ("compound-simple", "{a:1}"),
        ("compound-multi", "{a:1,b:two}"),
        ("compound-nested", "{outer:{inner:1}}"),
        ("compound-bool-flag", "{flag:true}"),
        (
            "compound-all-types",
            "{byte:1b,short:2s,int:3,long:4L,float:5.5f,double:6.5d,str:\"x\"}",
        ),
        ("compound-arrays", "{a:[I;1,2],b:[B;1B]}"),
        ("compound-key-quoted-digit", "{123:1}"),
        ("compound-key-quoted-true", "{true:1}"),
        ("compound-key-dot", "{a.b:1}"),
        ("compound-key-hyphen", "{a-b:1}"),
        ("compound-key-plus", "{a+b:1}"),
        ("compound-key-underscore", "{a_b:1}"),
        ("compound-key-space", "{\"a b\":1}"),
        ("compound-key-colon", "{'a:b':1}"),
        ("compound-trailing-comma", "{a:1,}"),
        ("compound-nested-list-compounds", "{a:[{x:1,y:2}]}"),
        // ---- whitespace ----
        ("whitespace-compound", "  { a : 1 }  "),
        ("whitespace-list", "[ 1 , 2 ]"),
        // ---- pretty printer KEY_ORDER / no_indentation paths ----
        (
            "pretty-root-keyorder",
            "{zzz:9,DataVersion:1,author:\"a\",size:2}",
        ),
        (
            "pretty-data-blockorder",
            "{data:[{nbt:\"n\",state:\"s\",pos:1}]}",
        ),
        (
            "pretty-entities-blockpos",
            "{entities:[{pos:\"p\",blockPos:\"bp\"}]}",
        ),
        ("pretty-size-noindent", "{size:[1,2,3]}"),
        ("pretty-palette-noindent", "{palette:[{Name:\"x\"}]}"),
        ("pretty-root-keyorder-missing", "{author:\"a\",zzz:9}"),
    ]
}

/// Deliberately-invalid SNBT inputs: the oracle and Rust must agree on
/// accept/reject. Exact error text is informational only.
pub fn invalid_corpus() -> Vec<(&'static str, &'static str)> {
    vec![
        ("trailing-data", "1 2"),
        ("leading-zero", "0123"),
        ("trailing-underscore", "1_"),
        ("overflow-byte", "128b"),
        ("underflow-byte", "-129b"),
        ("overflow-short", "32768s"),
        ("overflow-int", "2147483648"),
        ("underflow-int", "-2147483649"),
        ("overflow-long", "9223372036854775808L"),
        ("inf-float", "1e400f"),
        ("inf-double", "1e400d"),
        ("unclosed-string", "\"abc"),
        ("unclosed-compound", "{a:1"),
        ("unclosed-list", "[1,2"),
        ("bad-array-element", "[B;1L]"),
        ("empty-key", "{:1}"),
        ("double-comma", "{a:1,,}"),
        ("number-start-unquoted", "+abc"),
        ("uuid-bad", "uuid(\"nope\")"),
        ("unknown-builtin", "foo(1)"),
    ]
}

/// Hand-built compound SNBT inputs for `nbt.encode`. Labels ending in
/// `-multi` have a compound with two or more keys somewhere (their binary
/// field order is a known HashMap-drift divergence); the rest are
/// single-key-deep and must be byte-for-byte identical.
pub fn encode_corpus() -> Vec<(String, String)> {
    let cases: Vec<(&'static str, &'static str)> = vec![
        ("empty", "{}"),
        ("byte", "{a:1b}"),
        ("short", "{a:1s}"),
        ("int", "{a:1}"),
        ("long", "{a:1L}"),
        ("float", "{a:1.5f}"),
        ("double", "{a:1.5d}"),
        ("string", "{a:\"hi\"}"),
        ("string-emoji", "{a:\"\u{1F600}\"}"),
        ("string-utf8", "{a:\"h\u{e9}llo w\u{f6}rld\"}"),
        ("byte-array", "{a:[B;1B,-1B,2B]}"),
        ("byte-array-empty", "{a:[B;]}"),
        ("int-array", "{a:[I;1,2]}"),
        ("int-array-empty", "{a:[I;]}"),
        ("long-array", "{a:[L;1L,2L]}"),
        ("long-array-empty", "{a:[L;]}"),
        ("list-ints", "{a:[1,2,3]}"),
        ("list-strings", "{a:[\"x\",\"y\"]}"),
        ("list-empty", "{a:[]}"),
        ("list-mixed", "{a:[1,\"x\"]}"),
        ("nested-single", "{a:{b:{c:1}}}"),
        ("long-max", "{a:9223372036854775807L}"),
        ("long-min", "{a:-9223372036854775808L}"),
        ("byte-min", "{a:-128b}"),
        ("int-min", "{a:-2147483648}"),
        ("int-array-big", "{a:[I;1,2,3,4,5,6,7,8,9,10]}"),
        ("neg-zero-float", "{a:-0.0f}"),
        ("neg-zero-double", "{a:-0.0d}"),
        ("max-double", "{a:1.7976931348623157E308d}"),
        // multi-key (semantic compare only)
        ("multi-2", "{a:1,b:2}"),
        ("multi-mixed", "{a:1,b:\"two\",c:[I;1,2]}"),
        ("multi-nested", "{outer:{a:1,b:2},z:true}"),
        ("multi-reversed", "{b:2,a:1}"),
        ("multi-root-order", "{DataVersion:1,author:\"x\",zzz:9}"),
    ];

    let mut out: Vec<(String, String)> = cases
        .into_iter()
        .map(|(label, snbt)| (label.to_string(), snbt.to_string()))
        .collect();
    // A 40k-char string exercises long modified-UTF-8 values.
    out.push((
        "big-string".to_string(),
        format!("{{a:\"{}\"}}", "a".repeat(40_000)),
    ));
    out
}

/// Locate the committed M0 chunk fixtures relative to the workspace root.
///
/// This crate lives at `<ws>/tools/rivet-parity`, two levels under the
/// workspace root (unlike the `crates/*` packages, which are three deep).
pub fn fixtures_dir() -> Option<std::path::PathBuf> {
    let ws = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?;
    let dir = ws.join("tools/rivet-oracle/fixtures/chunk");
    dir.is_dir().then_some(dir)
}

/// Locate + load the committed `fixtures/text/` component-JSON corpus + golden
/// (issue #98). Shared with the offline corpus tests: the single loader lives
/// in `rivet-text` (the crate whose codec the corpus exercises), and this tool
/// reuses it so the schema parsing can never drift between the two. The entry
/// type is `rivet_text::corpus::TextFixtureEntry` (visible through the returned
/// `text_corpus()` signature; callers never name it). A malformed committed
/// corpus is a hard `CorpusError::Malformed` — only genuine absence
/// (`CorpusError::Absent`) lets the caller skip the section.
pub use rivet_text::corpus::{CorpusError, text_corpus, text_fixtures_dir};

/// Walk the fixtures tree collecting `*.nbt` files in deterministic order.
pub fn collect_fixtures(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("fixtures dir readable") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("nbt") {
                out.push(path);
            }
        }
    }
    walk(dir, &mut out);
    out.sort();
    out
}

/// Label for a fixture path relative to the fixtures `chunk` root, e.g.
/// `overworld/0.0/0.0.nbt`.
pub fn fixture_label(chunk_root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(chunk_root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}
