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
/// every input), and accepted entries must carry Paper's canonical JSON. A
/// missing or malformed pair yields `None` (fixtures absent) so each caller
/// decides how to surface it: the offline tests fail loudly on `None`, the
/// parity tool skips the `component.json` section. Entries are never silently
/// fabricated.
pub fn text_corpus() -> Option<Vec<TextFixtureEntry>> {
    let dir = text_fixtures_dir()?;
    let corpus_path = dir.join("corpus.json");
    let golden_path = dir.join("golden.json");
    if !corpus_path.is_file() || !golden_path.is_file() {
        return None;
    }
    let corpus: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(corpus_path).ok()?).ok()?;
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(golden_path).ok()?).ok()?;

    let golden_by_id: std::collections::HashMap<&str, &serde_json::Value> = golden["entries"]
        .as_array()?
        .iter()
        .map(|e| (e["id"].as_str().unwrap_or_default(), e))
        .collect();

    let mut out = Vec::new();
    for entry in corpus["entries"].as_array()? {
        let id = entry["id"].as_str().map(str::to_string)?;
        let input = entry["input"].as_str().map(str::to_string)?;
        let accept = entry["accept"].as_bool()?;
        let g = golden_by_id.get(id.as_str()).copied()?;
        let canonical = if accept {
            Some(
                g.get("canonical")
                    .and_then(serde_json::Value::as_str)?
                    .to_string(),
            )
        } else {
            None
        };
        out.push(TextFixtureEntry {
            id,
            input,
            accept,
            canonical,
        });
    }
    Some(out)
}
