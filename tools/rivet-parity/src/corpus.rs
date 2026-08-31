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
        ("hex-negative", "-0x10"),
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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

const MANIFEST_BYTES: &[u8] = include_bytes!("../fixtures/corpus-manifest.json");

#[derive(Debug, Clone)]
pub struct FixtureFile {
    pub label: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct CorpusClosure {
    pub chunk_root: PathBuf,
    pub files: Vec<FixtureFile>,
    pub manifest_sha256: String,
    pub declared: usize,
    pub discovered: usize,
    pub text_entries: Vec<rivet_text::corpus::TextFixtureEntry>,
}

struct ValidatedTextCorpus {
    entries: Vec<rivet_text::corpus::TextFixtureEntry>,
    files: BTreeMap<PathBuf, Vec<u8>>,
}

#[derive(Debug)]
pub enum ClosureError {
    Absent(String),
    Invalid(String),
}

impl fmt::Display for ClosureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent(message) => write!(f, "parity corpus absent: {message}"),
            Self::Invalid(message) => write!(f, "parity corpus invalid: {message}"),
        }
    }
}

fn workspace_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()
        .map(Path::to_path_buf)
}

fn safe_relative_path(raw: &str) -> Result<PathBuf, ClosureError> {
    let path = Path::new(raw);
    if raw.is_empty() || raw.contains('\\') || path.is_absolute() {
        return Err(ClosureError::Invalid(format!(
            "manifest path is not a safe normalized relative path: {raw:?}"
        )));
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(ClosureError::Invalid(format!(
                "manifest path is not a safe normalized relative path: {raw:?}"
            )));
        }
        normalized.push(component.as_os_str());
    }
    if normalized != path {
        return Err(ClosureError::Invalid(format!(
            "manifest path is not a safe normalized relative path: {raw:?}"
        )));
    }
    Ok(path.to_path_buf())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn trusted_regular_file_bytes(path: &Path, expected_size: usize) -> Result<Vec<u8>, ClosureError> {
    let meta = std::fs::symlink_metadata(path).map_err(|error| {
        ClosureError::Invalid(format!(
            "{} is missing or unreadable: {error}",
            path.display()
        ))
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(ClosureError::Invalid(format!(
            "{} is not a regular non-symlink file",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() != 1 {
            return Err(ClosureError::Invalid(format!(
                "{} has {} hard links",
                path.display(),
                meta.nlink()
            )));
        }
    }
    if meta.len() != expected_size as u64 {
        return Err(ClosureError::Invalid(format!(
            "{} has {} bytes, expected {expected_size}",
            path.display(),
            meta.len()
        )));
    }

    let file = std::fs::File::open(path).map_err(|error| {
        ClosureError::Invalid(format!("{} unreadable: {error}", path.display()))
    })?;
    let limit = u64::try_from(expected_size)
        .ok()
        .and_then(|size| size.checked_add(1))
        .ok_or_else(|| ClosureError::Invalid(format!("{} size overflows", path.display())))?;
    let mut bytes = Vec::with_capacity(expected_size);
    file.take(limit).read_to_end(&mut bytes).map_err(|error| {
        ClosureError::Invalid(format!("{} unreadable: {error}", path.display()))
    })?;
    if bytes.len() != expected_size {
        return Err(ClosureError::Invalid(format!(
            "{} changed size while being read",
            path.display()
        )));
    }
    Ok(bytes)
}

/// Load the committed chunk corpus only after proving exact manifest closure.
/// Missing roots are distinct from present-but-invalid trees so strict callers
/// can report UNVERIFIED vs FAILED without silently skipping either case.
pub fn load_fixture_closure() -> Result<CorpusClosure, ClosureError> {
    let workspace = workspace_root()
        .ok_or_else(|| ClosureError::Absent("workspace root cannot be resolved".to_string()))?;
    let chunk_root = workspace.join("tools/rivet-oracle/fixtures/chunk");
    let mut closure = load_fixture_closure_from(chunk_root, MANIFEST_BYTES)?;
    let text = load_text_fixture_closure_from(
        workspace.join("tools/rivet-oracle/fixtures/text"),
        MANIFEST_BYTES,
    )?;

    let mut contract = Sha256::new();
    contract.update(b"rivet-parity-corpus-closure-v2\0");
    hash_contract_field(&mut contract, MANIFEST_BYTES);
    for (path, bytes) in &text.files {
        hash_contract_field(&mut contract, path.to_string_lossy().as_bytes());
        hash_contract_field(&mut contract, bytes);
    }
    closure.manifest_sha256 = format!("{:x}", contract.finalize());
    closure.text_entries = text.entries;
    Ok(closure)
}

fn hash_contract_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn handbuilt_contract_sha256_from(
    valid: &[(String, String)],
    invalid: &[(String, String)],
    encode: &[(String, String)],
) -> String {
    let mut contract = Sha256::new();
    contract.update(b"rivet-parity-handbuilt-v1\0");
    for (kind, entries) in [
        ("snbt-valid", valid),
        ("snbt-invalid", invalid),
        ("nbt-encode", encode),
    ] {
        for (label, input) in entries {
            hash_contract_field(&mut contract, kind.as_bytes());
            hash_contract_field(&mut contract, label.as_bytes());
            hash_contract_field(&mut contract, input.as_bytes());
        }
    }
    format!("{:x}", contract.finalize())
}

fn validate_handbuilt_contract(
    expected: &serde_json::Map<String, serde_json::Value>,
    valid: &[(String, String)],
    invalid: &[(String, String)],
    encode: &[(String, String)],
) -> Result<(), ClosureError> {
    let actual_hash = handbuilt_contract_sha256_from(valid, invalid, encode);
    if expected["snbt_valid"].as_u64() != Some(valid.len() as u64)
        || expected["snbt_invalid"].as_u64() != Some(invalid.len() as u64)
        || expected["nbt_encode"].as_u64() != Some(encode.len() as u64)
        || expected["handbuilt_sha256"].as_str() != Some(actual_hash.as_str())
    {
        return Err(ClosureError::Invalid(format!(
            "manifest hand-built corpus contract does not match executable declarations: {actual_hash}"
        )));
    }
    Ok(())
}

fn load_fixture_closure_from(
    chunk_root: PathBuf,
    manifest_bytes: &[u8],
) -> Result<CorpusClosure, ClosureError> {
    let root_meta = match std::fs::symlink_metadata(&chunk_root) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ClosureError::Absent(format!(
                "{} does not exist",
                chunk_root.display()
            )));
        }
        Err(error) => {
            return Err(ClosureError::Invalid(format!(
                "{} cannot be inspected: {error}",
                chunk_root.display()
            )));
        }
    };
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return Err(ClosureError::Invalid(format!(
            "{} is not a regular directory",
            chunk_root.display()
        )));
    }

    let manifest: serde_json::Value = serde_json::from_slice(manifest_bytes).map_err(|error| {
        ClosureError::Invalid(format!("embedded manifest is malformed: {error}"))
    })?;
    if manifest["format"].as_u64() != Some(1)
        || manifest["kind"].as_str() != Some("rivet-parity-corpus")
        || manifest["paper"].as_str() != Some("26.2-DEV-main@0a99345")
    {
        return Err(ClosureError::Invalid(
            "embedded manifest has the wrong format/kind".to_string(),
        ));
    }
    let expected = manifest["expected"]
        .as_object()
        .ok_or_else(|| ClosureError::Invalid("manifest lacks expected counts".to_string()))?;
    let expected_chunks = expected["chunk_nbt"]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| ClosureError::Invalid("manifest chunk_nbt count is invalid".to_string()))?;
    let valid: Vec<_> = parse_corpus()
        .into_iter()
        .map(|(label, input)| (label.to_string(), input.to_string()))
        .collect();
    let invalid: Vec<_> = invalid_corpus()
        .into_iter()
        .map(|(label, input)| (label.to_string(), input.to_string()))
        .collect();
    let encode = encode_corpus();
    let mut handbuilt_ids = BTreeSet::new();
    for (kind, entries) in [
        ("snbt-valid", &valid),
        ("snbt-invalid", &invalid),
        ("nbt-encode", &encode),
    ] {
        for (label, _) in entries {
            if !handbuilt_ids.insert((kind, label.clone())) {
                return Err(ClosureError::Invalid(format!(
                    "duplicate hand-built fixture id {kind}:{label}"
                )));
            }
        }
    }
    validate_handbuilt_contract(expected, &valid, &invalid, &encode)?;

    let entries = manifest["chunk_files"]
        .as_array()
        .ok_or_else(|| ClosureError::Invalid("manifest lacks chunk_files".to_string()))?;
    if entries.len() != expected_chunks {
        return Err(ClosureError::Invalid(format!(
            "manifest declares {expected_chunks} chunks but lists {}",
            entries.len()
        )));
    }
    let mut declared = BTreeMap::new();
    for entry in entries {
        let raw = entry["path"]
            .as_str()
            .ok_or_else(|| ClosureError::Invalid("chunk entry lacks path".to_string()))?;
        let relative = safe_relative_path(raw)?;
        if relative.extension().and_then(|value| value.to_str()) != Some("nbt") {
            return Err(ClosureError::Invalid(format!(
                "manifest chunk path is not .nbt: {raw}"
            )));
        }
        let digest = entry["sha256"]
            .as_str()
            .filter(|value| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| ClosureError::Invalid(format!("{raw}: invalid sha256")))?;
        let size = entry["bytes"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| ClosureError::Invalid(format!("{raw}: invalid byte count")))?;
        if declared
            .insert(relative, (digest.to_ascii_lowercase(), size))
            .is_some()
        {
            return Err(ClosureError::Invalid(format!(
                "duplicate/aliased manifest path: {raw}"
            )));
        }
    }

    let mut discovered = BTreeSet::new();
    fn walk(root: &Path, dir: &Path, out: &mut BTreeSet<PathBuf>) -> Result<(), ClosureError> {
        let entries = std::fs::read_dir(dir).map_err(|error| {
            ClosureError::Invalid(format!("{} unreadable: {error}", dir.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                ClosureError::Invalid(format!("{} entry unreadable: {error}", dir.display()))
            })?;
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path).map_err(|error| {
                ClosureError::Invalid(format!("{} cannot be inspected: {error}", path.display()))
            })?;
            if meta.file_type().is_symlink() {
                return Err(ClosureError::Invalid(format!(
                    "fixture tree contains symlink {}",
                    path.display()
                )));
            }
            if meta.is_dir() {
                walk(root, &path, out)?;
            } else if meta.is_file() {
                let relative = path.strip_prefix(root).map_err(|_| {
                    ClosureError::Invalid(format!("{} escaped fixture root", path.display()))
                })?;
                if relative.extension().and_then(|value| value.to_str()) != Some("nbt") {
                    return Err(ClosureError::Invalid(format!(
                        "unlisted non-NBT fixture file {}",
                        relative.display()
                    )));
                }
                out.insert(relative.to_path_buf());
            } else {
                return Err(ClosureError::Invalid(format!(
                    "fixture tree contains non-regular entry {}",
                    path.display()
                )));
            }
        }
        Ok(())
    }
    walk(&chunk_root, &chunk_root, &mut discovered)?;
    let declared_set: BTreeSet<_> = declared.keys().cloned().collect();
    if discovered != declared_set {
        let missing: Vec<_> = declared_set.difference(&discovered).collect();
        let unlisted: Vec<_> = discovered.difference(&declared_set).collect();
        return Err(ClosureError::Invalid(format!(
            "chunk fixture set differs from manifest: missing={missing:?} unlisted={unlisted:?}"
        )));
    }

    let mut files = Vec::with_capacity(declared.len());
    for (relative, (expected_digest, expected_size)) in declared {
        let path = chunk_root.join(&relative);
        let bytes = trusted_regular_file_bytes(&path, expected_size)?;
        if sha256(&bytes) != expected_digest {
            return Err(ClosureError::Invalid(format!(
                "{} content differs from manifest",
                relative.display()
            )));
        }
        files.push(FixtureFile {
            label: relative.to_string_lossy().into_owned(),
            bytes,
        });
    }

    Ok(CorpusClosure {
        chunk_root,
        declared: files.len(),
        discovered: discovered.len(),
        files,
        manifest_sha256: sha256(manifest_bytes),
        text_entries: Vec::new(),
    })
}

/// Validate the text fixture directory against the independently embedded
/// contract, then parse the exact validated bytes without reopening paths.
fn load_text_fixture_closure_from(
    dir: PathBuf,
    trusted_manifest_bytes: &[u8],
) -> Result<ValidatedTextCorpus, ClosureError> {
    let root_meta = match std::fs::symlink_metadata(&dir) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ClosureError::Absent(
                "tools/rivet-oracle/fixtures/text is absent".to_string(),
            ));
        }
        Err(error) => {
            return Err(ClosureError::Invalid(format!(
                "{} cannot be inspected: {error}",
                dir.display()
            )));
        }
    };
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return Err(ClosureError::Invalid(format!(
            "{} is not a regular directory",
            dir.display()
        )));
    }
    let trusted: serde_json::Value = serde_json::from_slice(trusted_manifest_bytes)
        .map_err(|error| ClosureError::Invalid(format!("trusted manifest malformed: {error}")))?;
    let trusted_entries = trusted["text_files"]
        .as_array()
        .ok_or_else(|| ClosureError::Invalid("trusted manifest lacks text_files".to_string()))?;
    if trusted_entries.len() != 3 {
        return Err(ClosureError::Invalid(
            "trusted manifest must pin exactly three text files".to_string(),
        ));
    }
    let mut trusted_files = BTreeMap::new();
    for entry in trusted_entries {
        let raw = entry["path"]
            .as_str()
            .ok_or_else(|| ClosureError::Invalid("trusted text file lacks path".to_string()))?;
        let relative = safe_relative_path(raw)?;
        let digest = entry["sha256"]
            .as_str()
            .filter(|value| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| ClosureError::Invalid(format!("{raw}: invalid trusted sha256")))?;
        let size = entry["bytes"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| ClosureError::Invalid(format!("{raw}: invalid trusted byte count")))?;
        if trusted_files
            .insert(relative, (digest.to_ascii_lowercase(), size))
            .is_some()
        {
            return Err(ClosureError::Invalid(format!(
                "duplicate trusted text path {raw}"
            )));
        }
    }
    let expected: BTreeSet<_> = ["corpus.json", "golden.json", "manifest.json"]
        .into_iter()
        .map(PathBuf::from)
        .collect();
    if trusted_files.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(ClosureError::Invalid(
            "trusted manifest pins the wrong text files".to_string(),
        ));
    }

    let mut discovered = BTreeSet::new();
    for entry in std::fs::read_dir(&dir)
        .map_err(|error| ClosureError::Invalid(format!("{} unreadable: {error}", dir.display())))?
    {
        let entry = entry.map_err(|error| ClosureError::Invalid(error.to_string()))?;
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|error| ClosureError::Invalid(format!("{}: {error}", path.display())))?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(ClosureError::Invalid(format!(
                "text fixture entry is not a regular file: {}",
                path.display()
            )));
        }
        discovered.insert(PathBuf::from(entry.file_name()));
    }
    if discovered != expected {
        return Err(ClosureError::Invalid(format!(
            "text fixture file set differs: expected={expected:?} discovered={discovered:?}"
        )));
    }

    let mut files = BTreeMap::new();
    for (relative, (expected_digest, expected_size)) in trusted_files {
        let bytes = trusted_regular_file_bytes(&dir.join(&relative), expected_size)?;
        if sha256(&bytes) != expected_digest {
            return Err(ClosureError::Invalid(format!(
                "text fixture {} differs from trusted manifest",
                relative.display()
            )));
        }
        files.insert(relative, bytes);
    }

    let manifest_bytes = &files[Path::new("manifest.json")];
    let manifest: serde_json::Value = serde_json::from_slice(manifest_bytes)
        .map_err(|error| ClosureError::Invalid(format!("text manifest malformed: {error}")))?;
    validate_text_header(&manifest, "manifest.json")?;
    let captured = manifest["captured"]
        .as_array()
        .ok_or_else(|| ClosureError::Invalid("text manifest lacks captured files".to_string()))?;
    if captured.len() != 2 {
        return Err(ClosureError::Invalid(
            "text manifest must capture exactly corpus.json and golden.json".to_string(),
        ));
    }
    let mut captured_names = BTreeSet::new();
    for entry in captured {
        let raw = entry["path"]
            .as_str()
            .ok_or_else(|| ClosureError::Invalid("text capture lacks path".to_string()))?;
        let relative = safe_relative_path(raw)?;
        if !captured_names.insert(relative.clone()) {
            return Err(ClosureError::Invalid(format!(
                "duplicate text capture {raw}"
            )));
        }
        let bytes = files.get(&relative).ok_or_else(|| {
            ClosureError::Invalid(format!("text manifest captures untrusted file {raw}"))
        })?;
        let expected_size = entry["bytes"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| ClosureError::Invalid(format!("{raw}: invalid byte count")))?;
        let expected_digest = entry["sha256"]
            .as_str()
            .ok_or_else(|| ClosureError::Invalid(format!("{raw}: missing sha256")))?;
        if bytes.len() != expected_size || sha256(bytes) != expected_digest {
            return Err(ClosureError::Invalid(format!(
                "text fixture {raw} differs from capture manifest"
            )));
        }
    }
    let expected_captures: BTreeSet<_> =
        [PathBuf::from("corpus.json"), PathBuf::from("golden.json")]
            .into_iter()
            .collect();
    if captured_names != expected_captures {
        return Err(ClosureError::Invalid(format!(
            "text manifest captures wrong files: {captured_names:?}"
        )));
    }

    let corpus_bytes = &files[Path::new("corpus.json")];
    let golden_bytes = &files[Path::new("golden.json")];
    let corpus_json: serde_json::Value = serde_json::from_slice(corpus_bytes)
        .map_err(|error| ClosureError::Invalid(format!("corpus.json malformed: {error}")))?;
    let golden_json: serde_json::Value = serde_json::from_slice(golden_bytes)
        .map_err(|error| ClosureError::Invalid(format!("golden.json malformed: {error}")))?;
    validate_text_header(&corpus_json, "corpus.json")?;
    validate_text_header(&golden_json, "golden.json")?;
    validate_text_entry_sets(&corpus_json, &golden_json)?;

    let entries = rivet_text::corpus::parse_text_corpus_bytes(corpus_bytes, golden_bytes)
        .map_err(|error| ClosureError::Invalid(error.to_string()))?;
    validate_text_counts(&trusted, &entries)?;
    Ok(ValidatedTextCorpus { entries, files })
}

fn validate_text_header(value: &serde_json::Value, name: &str) -> Result<(), ClosureError> {
    if value["format"].as_u64() != Some(1)
        || value["kind"].as_str() != Some("text")
        || value["paper"].as_str() != Some("26.2-DEV-main@0a99345")
    {
        return Err(ClosureError::Invalid(format!(
            "{name} has the wrong format/kind/Paper pin"
        )));
    }
    Ok(())
}

fn text_entry_ids(value: &serde_json::Value, name: &str) -> Result<BTreeSet<String>, ClosureError> {
    let entries = value["entries"]
        .as_array()
        .ok_or_else(|| ClosureError::Invalid(format!("{name} lacks entries")))?;
    let mut ids = BTreeSet::new();
    for entry in entries {
        let id = entry["id"]
            .as_str()
            .ok_or_else(|| ClosureError::Invalid(format!("{name} entry lacks string id")))?;
        if !ids.insert(id.to_string()) {
            return Err(ClosureError::Invalid(format!(
                "{name} contains duplicate id {id}"
            )));
        }
    }
    Ok(ids)
}

fn validate_text_entry_sets(
    corpus: &serde_json::Value,
    golden: &serde_json::Value,
) -> Result<(), ClosureError> {
    let corpus_ids = text_entry_ids(corpus, "corpus.json")?;
    let golden_ids = text_entry_ids(golden, "golden.json")?;
    if corpus_ids != golden_ids {
        return Err(ClosureError::Invalid(
            "corpus.json and golden.json contain different id sets".to_string(),
        ));
    }
    Ok(())
}

fn validate_text_counts(
    trusted: &serde_json::Value,
    entries: &[rivet_text::corpus::TextFixtureEntry],
) -> Result<(), ClosureError> {
    let expected = trusted["expected"]
        .as_object()
        .ok_or_else(|| ClosureError::Invalid("trusted manifest lacks expected".to_string()))?;
    let accepted = entries.iter().filter(|entry| entry.accept).count();
    let invalid = entries.len() - accepted;
    if expected["text_entries"].as_u64() != Some(entries.len() as u64)
        || expected["text_accepted"].as_u64() != Some(accepted as u64)
        || expected["text_invalid"].as_u64() != Some(invalid as u64)
    {
        return Err(ClosureError::Invalid(format!(
            "text accept/invalid counts differ: total={} accepted={accepted} invalid={invalid}",
            entries.len()
        )));
    }

    let mut actual_kinds = BTreeMap::<String, u64>::new();
    for entry in entries {
        let kind = entry
            .id
            .split_once('-')
            .map_or(entry.id.as_str(), |pair| pair.0);
        *actual_kinds.entry(kind.to_string()).or_default() += 1;
    }
    let expected_kinds = expected["text_kinds"]
        .as_object()
        .ok_or_else(|| ClosureError::Invalid("trusted manifest lacks text_kinds".to_string()))?;
    let parsed_expected: BTreeMap<_, _> = expected_kinds
        .iter()
        .map(|(kind, count)| {
            count
                .as_u64()
                .map(|count| (kind.clone(), count))
                .ok_or_else(|| {
                    ClosureError::Invalid(format!("trusted text kind {kind} count is invalid"))
                })
        })
        .collect::<Result<_, _>>()?;
    if parsed_expected != actual_kinds {
        return Err(ClosureError::Invalid(format!(
            "text kind counts differ: expected={parsed_expected:?} actual={actual_kinds:?}"
        )));
    }
    Ok(())
}

pub use rivet_text::corpus::text_fixtures_dir;

#[cfg(test)]
mod closure_tests {
    use super::*;

    #[test]
    fn committed_corpus_has_exact_manifest_closure() {
        let closure = load_fixture_closure().expect("committed chunk closure");
        assert_eq!(closure.declared, 432);
        assert_eq!(closure.discovered, 432);
        assert_eq!(closure.files.len(), 432);
        assert_eq!(closure.text_entries.len(), 62);
        assert_eq!(
            closure
                .text_entries
                .iter()
                .filter(|entry| entry.accept)
                .count(),
            46
        );
    }

    #[test]
    fn executable_corpus_counts_match_manifest() {
        let manifest: serde_json::Value = serde_json::from_slice(MANIFEST_BYTES).unwrap();
        assert_eq!(manifest["expected"]["snbt_valid"], parse_corpus().len());
        assert_eq!(manifest["expected"]["snbt_invalid"], invalid_corpus().len());
        assert_eq!(manifest["expected"]["nbt_encode"], encode_corpus().len());
        let valid: Vec<_> = parse_corpus()
            .into_iter()
            .map(|(label, input)| (label.to_string(), input.to_string()))
            .collect();
        let invalid: Vec<_> = invalid_corpus()
            .into_iter()
            .map(|(label, input)| (label.to_string(), input.to_string()))
            .collect();
        assert_eq!(
            manifest["expected"]["handbuilt_sha256"],
            handbuilt_contract_sha256_from(&valid, &invalid, &encode_corpus())
        );
    }

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "rivet-parity-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn copy_text_root(name: &str) -> PathBuf {
        let root = temp_root(name);
        let source = text_fixtures_dir().expect("committed text fixtures");
        for file in ["manifest.json", "corpus.json", "golden.json"] {
            std::fs::copy(source.join(file), root.join(file)).unwrap();
        }
        root
    }

    fn write_json(path: &Path, value: &serde_json::Value) {
        let mut bytes = serde_json::to_vec_pretty(value).unwrap();
        bytes.push(b'\n');
        std::fs::write(path, bytes).unwrap();
    }

    fn refresh_capture_manifest(root: &Path) {
        let path = root.join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for entry in manifest["captured"].as_array_mut().unwrap() {
            let name = entry["path"].as_str().unwrap();
            let bytes = std::fs::read(root.join(name)).unwrap();
            entry["bytes"] = bytes.len().into();
            entry["sha256"] = sha256(&bytes).into();
        }
        write_json(&path, &manifest);
    }

    fn trusted_manifest_for_text_root(root: &Path) -> Vec<u8> {
        let mut trusted: serde_json::Value = serde_json::from_slice(MANIFEST_BYTES).unwrap();
        for entry in trusted["text_files"].as_array_mut().unwrap() {
            let name = entry["path"].as_str().unwrap();
            let bytes = std::fs::read(root.join(name)).unwrap();
            entry["bytes"] = bytes.len().into();
            entry["sha256"] = sha256(&bytes).into();
        }
        serde_json::to_vec(&trusted).unwrap()
    }

    fn one_file_manifest(path: &str, bytes: &[u8]) -> Vec<u8> {
        let valid: Vec<_> = parse_corpus()
            .into_iter()
            .map(|(label, input)| (label.to_string(), input.to_string()))
            .collect();
        let invalid: Vec<_> = invalid_corpus()
            .into_iter()
            .map(|(label, input)| (label.to_string(), input.to_string()))
            .collect();
        let encode = encode_corpus();
        let handbuilt_sha256 = handbuilt_contract_sha256_from(&valid, &invalid, &encode);
        serde_json::to_vec(&serde_json::json!({
            "format": 1,
            "kind": "rivet-parity-corpus",
            "paper": "26.2-DEV-main@0a99345",
            "expected": {
                "chunk_nbt": 1,
                "text_entries": 0,
                "snbt_valid": parse_corpus().len(),
                "snbt_invalid": invalid_corpus().len(),
                "nbt_encode": encode_corpus().len(),
                "handbuilt_sha256": handbuilt_sha256,
            },
            "chunk_files": [{
                "path": path,
                "sha256": sha256(bytes),
                "bytes": bytes.len(),
            }]
        }))
        .unwrap()
    }

    #[test]
    fn closure_rejects_missing_extra_and_changed_files() {
        let root = temp_root("set");
        let bytes = b"fixture";
        let manifest = one_file_manifest("a.nbt", bytes);
        assert!(matches!(
            load_fixture_closure_from(root.clone(), &manifest),
            Err(ClosureError::Invalid(_))
        ));
        std::fs::write(root.join("a.nbt"), bytes).unwrap();
        load_fixture_closure_from(root.clone(), &manifest).unwrap();
        std::fs::write(root.join("extra.nbt"), bytes).unwrap();
        assert!(matches!(
            load_fixture_closure_from(root.clone(), &manifest),
            Err(ClosureError::Invalid(_))
        ));
        std::fs::remove_file(root.join("extra.nbt")).unwrap();
        std::fs::write(root.join("a.nbt"), b"changed").unwrap();
        assert!(matches!(
            load_fixture_closure_from(root.clone(), &manifest),
            Err(ClosureError::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn closure_rejects_aliases_and_unsafe_entries() {
        for bad in [
            "../a.nbt",
            "/a.nbt",
            "a\\b.nbt",
            "a/./b.nbt",
            "a//b.nbt",
            "",
        ] {
            let root = temp_root("path");
            let manifest = one_file_manifest(bad, b"fixture");
            assert!(matches!(
                load_fixture_closure_from(root.clone(), &manifest),
                Err(ClosureError::Invalid(_))
            ));
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn closure_rejects_duplicate_manifest_paths() {
        let root = temp_root("duplicates");
        let bytes = b"fixture";
        std::fs::write(root.join("a.nbt"), bytes).unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&one_file_manifest("a.nbt", bytes)).unwrap();
        manifest["expected"]["chunk_nbt"] = 2.into();
        let duplicate = manifest["chunk_files"][0].clone();
        manifest["chunk_files"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(matches!(
            load_fixture_closure_from(root.clone(), &serde_json::to_vec(&manifest).unwrap()),
            Err(ClosureError::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn text_closure_rejects_self_authored_replacement_corpus() {
        let root = copy_text_root("text-replacement");
        let mut corpus: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("corpus.json")).unwrap()).unwrap();
        corpus["entries"][0]["input"] = "\"replacement\"".into();
        write_json(&root.join("corpus.json"), &corpus);
        let mut golden: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("golden.json")).unwrap()).unwrap();
        golden["entries"][0]["canonical"] = "\"replacement\"".into();
        write_json(&root.join("golden.json"), &golden);
        refresh_capture_manifest(&root);

        assert!(matches!(
            load_text_fixture_closure_from(root.clone(), MANIFEST_BYTES),
            Err(ClosureError::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn text_closure_rejects_changed_invalid_contract_with_updated_hashes() {
        let root = copy_text_root("text-invalid-count");
        let mut corpus: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("corpus.json")).unwrap()).unwrap();
        corpus["entries"][0]["accept"] = false.into();
        write_json(&root.join("corpus.json"), &corpus);
        let mut golden: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("golden.json")).unwrap()).unwrap();
        golden["entries"][0]["accept"] = false.into();
        golden["entries"][0]
            .as_object_mut()
            .unwrap()
            .remove("canonical");
        write_json(&root.join("golden.json"), &golden);
        refresh_capture_manifest(&root);
        let trusted = trusted_manifest_for_text_root(&root);

        assert!(matches!(
            load_text_fixture_closure_from(root.clone(), &trusted),
            Err(ClosureError::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn text_closure_rejects_changed_kind_counts_with_updated_hashes() {
        let root = copy_text_root("text-kind-count");
        let mut corpus: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("corpus.json")).unwrap()).unwrap();
        corpus["entries"][0]["id"] = "styled-reclassified".into();
        write_json(&root.join("corpus.json"), &corpus);
        let mut golden: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("golden.json")).unwrap()).unwrap();
        golden["entries"][0]["id"] = "styled-reclassified".into();
        write_json(&root.join("golden.json"), &golden);
        refresh_capture_manifest(&root);
        let trusted = trusted_manifest_for_text_root(&root);

        assert!(matches!(
            load_text_fixture_closure_from(root.clone(), &trusted),
            Err(ClosureError::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn handbuilt_contract_rejects_changed_snbt_with_same_counts() {
        let trusted: serde_json::Value = serde_json::from_slice(MANIFEST_BYTES).unwrap();
        let expected = trusted["expected"].as_object().unwrap();
        let mut valid: Vec<_> = parse_corpus()
            .into_iter()
            .map(|(label, input)| (label.to_string(), input.to_string()))
            .collect();
        valid[0].1 = "0b".to_string();
        let invalid: Vec<_> = invalid_corpus()
            .into_iter()
            .map(|(label, input)| (label.to_string(), input.to_string()))
            .collect();
        assert!(matches!(
            validate_handbuilt_contract(expected, &valid, &invalid, &encode_corpus()),
            Err(ClosureError::Invalid(_))
        ));
    }

    #[test]
    fn validated_text_entries_survive_path_swap_without_reopen() {
        let root = copy_text_root("text-swap");
        let validated = load_text_fixture_closure_from(root.clone(), MANIFEST_BYTES).unwrap();
        let first = validated.entries[0].input.clone();
        for file in ["manifest.json", "corpus.json", "golden.json"] {
            std::fs::write(root.join(file), b"replaced after validation").unwrap();
        }
        assert_eq!(validated.entries.len(), 62);
        assert_eq!(validated.entries[0].input, first);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn text_closure_rejects_huge_sparse_file_before_reading() {
        let root = copy_text_root("text-sparse");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(root.join("corpus.json"))
            .unwrap();
        file.set_len(16 * 1024 * 1024 * 1024).unwrap();
        assert!(matches!(
            load_text_fixture_closure_from(root.clone(), MANIFEST_BYTES),
            Err(ClosureError::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn closure_rejects_symlinks_and_hardlinks() {
        use std::os::unix::fs::symlink;

        let root = temp_root("links");
        let outside = root.with_extension("outside");
        std::fs::write(&outside, b"fixture").unwrap();
        symlink(&outside, root.join("a.nbt")).unwrap();
        let manifest = one_file_manifest("a.nbt", b"fixture");
        assert!(matches!(
            load_fixture_closure_from(root.clone(), &manifest),
            Err(ClosureError::Invalid(_))
        ));
        std::fs::remove_file(root.join("a.nbt")).unwrap();
        std::fs::hard_link(&outside, root.join("a.nbt")).unwrap();
        assert!(matches!(
            load_fixture_closure_from(root.clone(), &manifest),
            Err(ClosureError::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
        symlink(&outside, &root).unwrap();
        assert!(matches!(
            load_fixture_closure_from(root.clone(), &manifest),
            Err(ClosureError::Invalid(_))
        ));
        let _ = std::fs::remove_file(root);
    }
}
