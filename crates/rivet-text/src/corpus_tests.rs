//! Offline (oracle-free) tests consuming the committed issue-#98 text corpus.
//!
//! The corpus + golden under `tools/rivet-oracle/fixtures/text/` are the
//! Paper-grounded fixture: `input` is the exact component JSON a packet
//! carries, `accept` records Paper's verdict in the Bootstrap-only oracle
//! context, and `canonical` is Paper's `ComponentSerialization.CODEC`
//! decode->re-encode under non-compressed `JsonOps`. These tests run with no
//! JVM and fail loudly if the corpus is absent or vacuous, so the fixture can
//! never silently stop being exercised.
//!
//! The one honest divergence is the `ClickEvent`/`HoverEvent` STUB (RivetTodo
//! #89, epic #12): `click-copy-to-clipboard`, `click-open-url`,
//! `click-run-command`, and `hover-show-text` are all Paper-accepted (their
//! codec field names match Paper 26.2: ShowText `value`, OpenUrl `url`,
//! RunCommand `command`, CopyToClipboard `value`) but rejected by the Rust STUB
//! codec. Those entries are tracked as a documented divergence, and
//! `accepted-but-Rust-rejects` must equal exactly the documented set. The
//! `malformed-*-wrong-key` negatives are the same content with a wrong field
//! name (show_text `contents`, open_url `href`, run_command `value`,
//! copy_to_clipboard `text`): Paper rejects them, pinning the field names as
//! load-bearing so the corrected fixtures fail in Rust only at the real
//! codec/STUB boundary — never for registry/Holder context or malformed fields.

use crate::component::Component;
use rivet_serialization::codec::Codec;
use rivet_serialization::json_ops::JsonOps;

/// Accepted corpus entries the Rust STUB codec cannot decode yet (RivetTodo
/// #89, epic #12). If this set grows, the divergence list must be updated with
/// the codecs that landed — the test below enforces the match.
const DOCUMENTED_STUB_DIVERGENCES: &[&str] = &[
    "click-copy-to-clipboard",
    "click-open-url",
    "click-run-command",
    "hover-show-text",
];

/// Locate the committed `fixtures/text/` corpus + golden relative to the
/// workspace root (this crate is two levels under it, like `tools/rivet-parity`).
fn text_fixtures_dir() -> Option<std::path::PathBuf> {
    let ws = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?;
    let dir = ws.join("tools/rivet-oracle/fixtures/text");
    dir.is_dir().then_some(dir)
}

struct CorpusEntry {
    id: String,
    input: String,
    accept: bool,
    canonical: Option<String>,
}

/// Load corpus.json + golden.json, merged by id in corpus order. A missing
/// file or malformed pair is `None` so the non-vacuous test can fail loudly.
fn load_text_corpus() -> Option<Vec<CorpusEntry>> {
    let dir = text_fixtures_dir()?;
    let corpus: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("corpus.json")).ok()?).ok()?;
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("golden.json")).ok()?).ok()?;
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
            Some(g.get("canonical")?.as_str()?.to_string())
        } else {
            None
        };
        out.push(CorpusEntry {
            id,
            input,
            accept,
            canonical,
        });
    }
    Some(out)
}

/// The Rust mirror of the Paper oracle op. Shared with `rivet-parity` via
/// `component_serialization::json_canonical`, so the offline and live compare
/// can never drift apart.
fn rust_component_json(input: &str) -> Result<String, String> {
    crate::component_serialization::json_canonical(&crate::component_serialization::codec(), input)
}

/// `ComponentSerialization.CODEC` as used in these tests.
fn component_codec() -> std::sync::Arc<dyn Codec<Component, JsonOps>> {
    crate::component_serialization::codec()
}

#[test]
fn text_corpus_present_and_non_vacuous() {
    let corpus = load_text_corpus().expect(
        "issue-#98 text corpus + golden must be present at \
         tools/rivet-oracle/fixtures/text/ — the fixture is the test",
    );
    assert!(
        corpus.len() >= 50,
        "corpus must be substantial, got {}",
        corpus.len()
    );
    let accepts = corpus.iter().filter(|e| e.accept).count();
    let rejects = corpus.len() - accepts;
    assert!(
        accepts >= 30,
        "corpus must exercise accepted components, got {accepts}"
    );
    assert!(
        rejects >= 5,
        "corpus must include strict malformed fixtures, got {rejects}"
    );
    for e in &corpus {
        assert!(!e.input.is_empty(), "entry {} has an empty input", e.id);
        if e.accept {
            assert!(
                e.canonical.is_some(),
                "accepted entry {} must carry Paper's canonical",
                e.id
            );
        }
    }
}

#[test]
fn accepted_components_reencode_byte_identical_to_golden() {
    let corpus = load_text_corpus().expect("corpus present (see non-vacuous test)");
    let mut accepted_but_rejected: Vec<&str> = Vec::new();

    for e in corpus.iter().filter(|e| e.accept) {
        match rust_component_json(&e.input) {
            Ok(rust_canonical) => {
                let golden = e
                    .canonical
                    .as_deref()
                    .expect("accepted entry has canonical");
                assert_eq!(
                    rust_canonical, golden,
                    "decode->re-encode of {} diverges from Paper's canonical",
                    e.id
                );
            }
            Err(_) => accepted_but_rejected.push(e.id.as_str()),
        }
    }

    // Paper-accepted entries the Rust STUB codec rejects must be exactly the
    // documented divergences — never a silent surprise.
    let mut expected: Vec<&str> = DOCUMENTED_STUB_DIVERGENCES.to_vec();
    accepted_but_rejected.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        accepted_but_rejected, expected,
        "accepted-but-rejected set must equal the documented ClickEvent/\
         HoverEvent STUB divergences (RivetTodo #89, epic #12)"
    );
}

#[test]
fn rejected_entries_are_rejected_by_rust() {
    let corpus = load_text_corpus().expect("corpus present (see non-vacuous test)");
    let rejects: Vec<&CorpusEntry> = corpus.iter().filter(|e| !e.accept).collect();
    assert!(!rejects.is_empty(), "corpus must include rejected fixtures");
    for e in rejects {
        assert!(
            rust_component_json(&e.input).is_err(),
            "Rust must reject {} — Paper's verdict is reject",
            e.id
        );
    }
}

/// The corrected Paper-accepted click/hover fixtures (issue #98) use exactly
/// Paper 26.2's codec field names — ShowText `value`, OpenUrl `url`,
/// RunCommand `command`, CopyToClipboard `value` — and Paper's canonical
/// re-encode is captured in golden.json. These assertions pin both: the input
/// schema is the Paper one, and the expected canonical is the exact JSON Paper
/// emits (a chat/title/player-info/scoreboard wire form under non-compressed
/// `JsonOps`). They are load-bearing against schema drift: if a future editor
/// "corrects" a field name away from Paper's schema, or the golden capture
/// stops matching, this test fails even though the entries are still
/// Paper-accepted (the parity layer would otherwise only see a STUB divergence
/// and could mask a mislabelled fixture).
#[test]
fn corrected_click_hover_fixtures_use_paper_schemas_and_canonicals() {
    let corpus = load_text_corpus().expect("corpus present (see non-vacuous test)");
    let by_id: std::collections::HashMap<&str, &CorpusEntry> =
        corpus.iter().map(|e| (e.id.as_str(), e)).collect();

    // (id, input, Paper canonical under non-compressed JsonOps).
    let expected: &[(&str, &str, &str)] = &[
        (
            "hover-show-text",
            "{\"text\":\"h\",\"hover_event\":{\"action\":\"show_text\",\"value\":\"hover!\"}}",
            "{\"text\":\"h\",\"hover_event\":{\"value\":\"hover!\",\"action\":\"show_text\"}}",
        ),
        (
            "click-open-url",
            "{\"text\":\"c\",\"click_event\":{\"action\":\"open_url\",\"url\":\"https://example.com/path?q=1&r=2\"}}",
            "{\"text\":\"c\",\"click_event\":{\"url\":\"https://example.com/path?q=1&r=2\",\"action\":\"open_url\"}}",
        ),
        (
            "click-run-command",
            "{\"text\":\"r\",\"click_event\":{\"action\":\"run_command\",\"command\":\"/say hi\"}}",
            "{\"text\":\"r\",\"click_event\":{\"command\":\"/say hi\",\"action\":\"run_command\"}}",
        ),
        (
            "click-copy-to-clipboard",
            "{\"text\":\"cp\",\"click_event\":{\"action\":\"copy_to_clipboard\",\"value\":\"copied\"}}",
            "{\"text\":\"cp\",\"click_event\":{\"value\":\"copied\",\"action\":\"copy_to_clipboard\"}}",
        ),
    ];

    for (id, input, canonical) in expected {
        let entry = by_id
            .get(id)
            .unwrap_or_else(|| panic!("missing corpus entry {id}"));
        assert!(
            entry.accept,
            "{id} must be Paper-accepted (issue #98 corrected fixture)"
        );
        assert_eq!(
            &entry.input, input,
            "{id} must use Paper 26.2's field names: ShowText `value`, OpenUrl `url`, \
             RunCommand `command`, CopyToClipboard `value`"
        );
        assert_eq!(
            entry.canonical.as_deref(),
            Some(*canonical),
            "{id} must capture Paper's canonical decode->re-encode byte-for-byte"
        );
    }
}

/// The `malformed-*-wrong-key` negatives (issue #98) are the corrected
/// click/hover fixtures with the field name swapped to a wrong one (show_text
/// `contents`, open_url `href`, run_command `value`, copy_to_clipboard `text`).
/// Paper rejects each — pinning the field names as load-bearing — and the
/// corpus records that verdict. If a wrong-key fixture ever flips to accepted,
/// the field-name pin has drifted.
#[test]
fn wrong_key_negatives_are_recorded_as_rejected() {
    let corpus = load_text_corpus().expect("corpus present (see non-vacuous test)");
    let by_id: std::collections::HashMap<&str, &CorpusEntry> =
        corpus.iter().map(|e| (e.id.as_str(), e)).collect();
    for id in [
        "malformed-hover-show-text-wrong-key",
        "malformed-click-open-url-wrong-key",
        "malformed-click-run-command-wrong-key",
        "malformed-click-copy-clipboard-wrong-key",
    ] {
        let entry = by_id
            .get(id)
            .unwrap_or_else(|| panic!("missing corpus entry {id}"));
        assert!(
            !entry.accept,
            "{id} must be Paper-rejected (wrong field name pins the correct schema)"
        );
        assert!(
            rust_component_json(&entry.input).is_err(),
            "Rust must reject {id} too (the ClickEvent/HoverEvent codec is a STUB, \
             and the field name is wrong either way)"
        );
    }
}

#[test]
fn mutation_control_canonical_is_sensitive_to_input() {
    // The byte-identical compare must follow the component content, not be a
    // hardcoded passthrough: a content change must change the re-encode.
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();

    let parse = |input: &str| -> String {
        let value: serde_json::Value = serde_json::from_str(input).unwrap();
        let decoded = codec.parse(&ops, &value).result().unwrap().clone();
        let encoded = codec.encode_start(&ops, &decoded).result().unwrap().clone();
        serde_json::to_string(&encoded).unwrap()
    };

    assert_eq!(parse("\"hello\""), "\"hello\"");
    assert_eq!(parse("\"HELLO\""), "\"HELLO\"");
    assert_ne!(parse("\"HELLO\""), parse("\"hello\""));

    // A style mutation flows through to the canonical.
    assert_eq!(
        parse("{\"text\":\"bold\",\"bold\":true}"),
        "{\"text\":\"bold\",\"bold\":true}"
    );
    assert_ne!(
        parse("{\"text\":\"bold\",\"bold\":true}"),
        parse("{\"text\":\"bold\",\"bold\":false}")
    );
}

#[test]
fn negative_control_malformed_mutations_reject() {
    // A valid input mutates to strict malformed: the mutation must reject
    // through the codec, not be swallowed by a permissive decode.
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();

    let parses = |input: &str| -> bool {
        let value: serde_json::Value = serde_json::from_str(input).unwrap();
        codec.parse(&ops, &value).result().is_some()
    };

    assert!(parses("{\"text\":\"a\",\"bold\":true}"));
    assert!(!parses("{\"text\":\"a\",\"bold\":\"yes\"}"));
    assert!(!parses("{\"text\":\"a\",\"extra\":[]}"));
    assert!(!parses("{\"text\":\"a\",\"extra\":5}"));
    assert!(!parses("{\"translate\":42}"));
    assert!(!parses("{\"score\":\"p\"}"));
}

/// Malformed *field-name* mutations of the corrected click/hover fixtures
/// (issue #98): the same content with the wrong codec key — show_text
/// `contents`, open_url `href`, run_command `value`, copy_to_clipboard `text` —
/// must reject through the Rust codec, mirroring Paper's reject verdicts.
/// These are distinct from the schema-value malformations above: they pin the
/// codec field names themselves as load-bearing.
#[test]
fn malformed_field_name_mutations_reject() {
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();

    let parses = |input: &str| -> bool {
        let value: serde_json::Value = serde_json::from_str(input).unwrap();
        codec.parse(&ops, &value).result().is_some()
    };

    // Correct (Paper-schema) inputs parse as far as the codec supports them:
    // none of these carries a click/hover field, so they decode.
    assert!(parses("{\"text\":\"a\",\"bold\":true}"));
    assert!(parses("\"plain\""));

    // A top-level unknown key is NOT a rejection case: Paper ignores it and
    // drops it from the canonical (verified against the pinned oracle:
    // `{"text":"a","txet":"a"}` -> accept, canonical `"a"`). So we do NOT
    // assert rejection here — the wrong-key fixtures that Paper actually
    // rejects are the ones inside click_event / hover_event below.
    assert!(parses("{\"text\":\"a\",\"txet\":\"a\"}"));

    // Wrong field names inside click_event / hover_event reject — the field
    // names are Paper's schemas, so any mutation away from them is malformed.
    // (Verified against the pinned oracle: each is accept:false.)
    assert!(!parses(
        "{\"text\":\"h\",\"hover_event\":{\"action\":\"show_text\",\"contents\":\"x\"}}"
    ));
    assert!(!parses(
        "{\"text\":\"c\",\"click_event\":{\"action\":\"open_url\",\"href\":\"https://e\"}}"
    ));
    assert!(!parses(
        "{\"text\":\"r\",\"click_event\":{\"action\":\"run_command\",\"value\":\"/say hi\"}}"
    ));
    assert!(!parses(
        "{\"text\":\"cp\",\"click_event\":{\"action\":\"copy_to_clipboard\",\"text\":\"x\"}}"
    ));
}
