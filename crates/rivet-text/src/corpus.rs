//! Loader for the committed issue-#98 component-JSON text corpus
//! (`tools/rivet-oracle/fixtures/text/`).
//!
//! The corpus + golden are the Paper-grounded fixture: `input` is the exact
//! component JSON a packet carries, `accept` records Paper's verdict in the
//! Bootstrap-only oracle context, and `canonical` is Paper's
//! `ComponentSerialization.CODEC` decode->re-encode under non-compressed
//! `JsonOps`. One loader is shared by the offline corpus tests
//! (`corpus_tests.rs`) and the live `rivet-parity` differential so the schema
//! parsing can never drift between the two (issue #98). It lives here — the
//! crate whose codec the corpus exercises — rather than in either consumer;
//! `rivet-parity` already depends on `rivet-text`, so sharing adds no crate
//! dependency.
//!
//! Numeric canonicalization is Paper-grounded through the byte-identity test:
//! every accepted entry's decode->re-encode is compared against Paper's golden
//! canonical byte-for-byte. The committed corpus currently exercises only
//! integral and small-magnitude float literals; the single serialization point
//! (`serde_json::to_string` on the re-encoded value) is where a Paper-vs-ryu
//! divergence in any future exponent-form / large-magnitude literal would
//! surface, and the byte-identity test would catch it — no such entry can be
//! added without Paper's actual canonical.

use std::fmt;
use std::path::PathBuf;

/// One entry of the committed component-JSON text corpus (issue #98): the
/// exact wire JSON (`input`), Paper's verdict at capture (`accept`, from
/// `golden.json`), and — for accepted entries — Paper's canonical
/// decode->re-encode JSON under non-compressed `JsonOps` (`canonical`, copied
/// verbatim so the byte identity is preserved).
pub struct TextFixtureEntry {
    pub id: String,
    pub input: String,
    pub accept: bool,
    pub canonical: Option<String>,
}

/// Why `text_corpus()` could not produce the fixture list.
#[derive(Debug)]
pub enum CorpusError {
    /// The committed fixtures tree (or a required file) is not present — e.g.
    /// fixtures were pruned or not checked out. Callers may skip cleanly.
    Absent,
    /// The fixtures are present but malformed or internally inconsistent. The
    /// message names the first problem found. This is a broken committed
    /// fixture and must hard-fail, never silently skip.
    Malformed(String),
}

impl fmt::Display for CorpusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CorpusError::Absent => write!(f, "text corpus fixtures are absent"),
            CorpusError::Malformed(m) => write!(f, "text corpus is malformed: {m}"),
        }
    }
}

/// Locate the committed `fixtures/text/` component-JSON corpus + golden
/// (issue #98), relative to the workspace root.
///
/// This crate lives at `<ws>/crates/rivet-text`, two levels under the
/// workspace root — the same depth as `tools/rivet-parity`, which reuses this
/// loader.
pub fn text_fixtures_dir() -> Option<PathBuf> {
    let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = ws
        .parent()?
        .parent()?
        .join("tools/rivet-oracle/fixtures/text");
    dir.is_dir().then_some(dir)
}

/// Load the committed text corpus + golden, merged by id in corpus order.
///
/// Every corpus entry must have a matching golden entry (the golden covers
/// every input), the two must AGREE on the accept verdict (`accept` is
/// duplicated in both files as provenance; a divergence is a corrupted
/// fixture), and accepted entries must carry Paper's canonical JSON. Absence
/// yields `Err(CorpusError::Absent)` so callers can skip; a present-but-broken
/// pair yields `Err(CorpusError::Malformed(..))` and must hard-fail — a
/// malformed committed fixture must never silently stop being exercised.
pub fn text_corpus() -> Result<Vec<TextFixtureEntry>, CorpusError> {
    let dir = text_fixtures_dir().ok_or(CorpusError::Absent)?;
    parse_text_corpus(&dir.join("corpus.json"), &dir.join("golden.json"))
}

/// Parse a corpus + golden pair from explicit paths. Split out of
/// [`text_corpus`] so the malformed/accept-divergence paths are testable on
/// temp trees without touching the committed fixtures.
pub(crate) fn parse_text_corpus(
    corpus_path: &std::path::Path,
    golden_path: &std::path::Path,
) -> Result<Vec<TextFixtureEntry>, CorpusError> {
    // Both files absent -> the fixtures tree was pruned / not checked out
    // (Absent, callers may skip). Exactly one present is a broken tree, not an
    // absence, and must hard-error.
    let corpus_meta = std::fs::metadata(corpus_path);
    let golden_meta = std::fs::metadata(golden_path);
    match (corpus_meta.is_ok(), golden_meta.is_ok()) {
        (false, false) => return Err(CorpusError::Absent),
        (false, true) => {
            return Err(CorpusError::Malformed(format!(
                "{} is missing (golden present)",
                corpus_path.display()
            )));
        }
        (true, false) => {
            return Err(CorpusError::Malformed(format!(
                "{} is missing (corpus present)",
                golden_path.display()
            )));
        }
        _ => {}
    }
    let corpus: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(corpus_path).map_err(|e| {
            CorpusError::Malformed(format!("{} unreadable: {e}", corpus_path.display()))
        })?)
        .map_err(|e| {
            CorpusError::Malformed(format!("{} unparsable: {e}", corpus_path.display()))
        })?;
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(golden_path).map_err(|e| {
            CorpusError::Malformed(format!("{} unreadable: {e}", golden_path.display()))
        })?)
        .map_err(|e| {
            CorpusError::Malformed(format!("{} unparsable: {e}", golden_path.display()))
        })?;

    let corpus_entries = corpus["entries"]
        .as_array()
        .ok_or_else(|| CorpusError::Malformed("corpus.json has no `entries` array".to_string()))?;
    let golden_entries = golden["entries"]
        .as_array()
        .ok_or_else(|| CorpusError::Malformed("golden.json has no `entries` array".to_string()))?;
    let mut golden_by_id: std::collections::HashMap<&str, &serde_json::Value> =
        std::collections::HashMap::new();
    for g in golden_entries {
        let gid = g["id"].as_str().ok_or_else(|| {
            CorpusError::Malformed("golden entry missing a string `id`".to_string())
        })?;
        golden_by_id.insert(gid, g);
    }

    let mut out = Vec::new();
    for entry in corpus_entries {
        let id = entry["id"]
            .as_str()
            .ok_or_else(|| CorpusError::Malformed("corpus entry missing `id`".to_string()))?;
        let input = entry["input"]
            .as_str()
            .ok_or_else(|| CorpusError::Malformed(format!("{id}: missing `input`")))?;
        let accept = entry["accept"]
            .as_bool()
            .ok_or_else(|| CorpusError::Malformed(format!("{id}: `accept` is not a boolean")))?;
        let g = golden_by_id
            .get(id)
            .copied()
            .ok_or_else(|| CorpusError::Malformed(format!("{id}: no matching golden entry")))?;
        // The accept verdict is duplicated in corpus.json and golden.json as
        // cross-file provenance; a divergence means one of them was edited
        // without the other and must be caught, not silently preferred.
        let golden_accept = g["accept"].as_bool().ok_or_else(|| {
            CorpusError::Malformed(format!("{id}: golden `accept` is not a boolean"))
        })?;
        if golden_accept != accept {
            return Err(CorpusError::Malformed(format!(
                "{id}: corpus accept {accept} contradicts golden accept {golden_accept}"
            )));
        }
        let canonical = if accept {
            Some(
                g["canonical"]
                    .as_str()
                    .ok_or_else(|| {
                        CorpusError::Malformed(format!(
                            "{id}: accepted entry lacks golden `canonical`"
                        ))
                    })?
                    .to_string(),
            )
        } else {
            None
        };
        out.push(TextFixtureEntry {
            id: id.to_string(),
            input: input.to_string(),
            accept,
            canonical,
        });
    }
    Ok(out)
}
