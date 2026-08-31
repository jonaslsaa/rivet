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
//! The click/hover entries (`click-open-url`, `click-run-command`,
//! `click-copy-to-clipboard`, `hover-show-text`) exercise the ported
//! `ClickEvent`/`HoverEvent` codecs; the deferred actions (`show_dialog`,
//! `custom`, `show_item`, `show_entity`) are STUB (RivetTodo #85, epic #12) and
//! no corpus entry exercises them. `accepted-but-Rust-rejects` must be empty —
//! every Paper-accepted entry re-encodes byte-identical to the golden — and the
//! `malformed-*-wrong-key` negatives pin the codec field names as load-bearing
//! so a schema drift fails in Rust at the real codec boundary.

use crate::component::Component;
use crate::corpus::{CorpusError, TextFixtureEntry, text_corpus};
use rivet_serialization::codec::Codec;
use rivet_serialization::json_ops::JsonOps;

/// Accepted corpus entries the Rust codec cannot decode yet. Empty after the
/// `ClickEvent`/`HoverEvent` codecs landed (#85): every Paper-accepted entry
/// must re-encode byte-identical, enforced below.
const DOCUMENTED_STUB_DIVERGENCES: &[&str] = &[];

/// The Rust mirror of the Paper oracle op, run through ONE codec graph.
///
/// Building a fresh `component_serialization::codec()` per call constructs a
/// permanent strong `Arc` cycle per entry (issue #207); the corpus tests run
/// per-entry across all 62 fixtures, so they must reuse a single graph. Each
/// test constructs one codec and threads it through every call.
fn rust_component_json(
    input: &str,
    codec: &std::sync::Arc<dyn Codec<Component, JsonOps>>,
) -> Result<String, String> {
    crate::component_serialization::json_canonical(codec, input)
}

/// `ComponentSerialization.CODEC` as used in these tests.
fn component_codec() -> std::sync::Arc<dyn Codec<Component, JsonOps>> {
    crate::component_serialization::codec()
}

#[test]
fn text_corpus_present_and_non_vacuous() {
    let corpus = text_corpus().expect(
        "issue-#98 text corpus + golden must be present and well-formed at \
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

/// A present-but-broken corpus must hard-error, never silently skip: a
/// malformed file, a missing golden entry, and an accept verdict that
/// contradicts golden.json's provenance all yield `Malformed`, while a wholly
/// absent pair yields `Absent` (the "fixtures pruned" case callers may skip).
#[test]
fn loader_hard_errors_on_malformed_and_absent_distinguish() {
    let tmp = std::env::temp_dir().join(format!("rivet-text-corpus-loader-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let corpus = tmp.join("corpus.json");
    let golden = tmp.join("golden.json");

    // Absent pair -> Absent.
    assert!(matches!(
        crate::corpus::parse_text_corpus(&corpus, &golden),
        Err(CorpusError::Absent)
    ));

    // Exactly one present is a broken tree -> Malformed, never Absent.
    std::fs::write(&corpus, "{}").unwrap();
    assert!(matches!(
        crate::corpus::parse_text_corpus(&corpus, &golden),
        Err(CorpusError::Malformed(m)) if m.contains("missing")
    ));
    std::fs::remove_file(&corpus).unwrap();
    std::fs::write(&golden, "{}").unwrap();
    assert!(matches!(
        crate::corpus::parse_text_corpus(&corpus, &golden),
        Err(CorpusError::Malformed(m)) if m.contains("missing")
    ));
    std::fs::remove_file(&golden).unwrap();

    // Unparsable JSON -> Malformed.
    std::fs::write(&corpus, "{broken").unwrap();
    std::fs::write(&golden, "{also-broken").unwrap();
    assert!(matches!(
        crate::corpus::parse_text_corpus(&corpus, &golden),
        Err(CorpusError::Malformed(_))
    ));

    // Valid JSON but a corpus entry with no matching golden -> Malformed.
    std::fs::write(
        &corpus,
        r#"{"entries":[{"id":"a","input":"\"x\"","accept":true}]}"#,
    )
    .unwrap();
    std::fs::write(&golden, r#"{"entries":[]}"#).unwrap();
    assert!(matches!(
        crate::corpus::parse_text_corpus(&corpus, &golden),
        Err(CorpusError::Malformed(m)) if m.contains("no matching golden")
    ));

    // Accept verdict contradicts the golden's provenance -> Malformed.
    std::fs::write(&golden, r#"{"entries":[{"id":"a","accept":false}]}"#).unwrap();
    assert!(matches!(
        crate::corpus::parse_text_corpus(&corpus, &golden),
        Err(CorpusError::Malformed(m)) if m.contains("contradicts")
    ));

    // Consistent pair -> Ok, with the golden's canonical threaded through.
    std::fs::write(
        &golden,
        r#"{"entries":[{"id":"a","accept":true,"canonical":"\"x\""}]}"#,
    )
    .unwrap();
    let loaded = crate::corpus::parse_text_corpus(&corpus, &golden).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, "a");
    assert!(loaded[0].accept);
    assert_eq!(loaded[0].canonical.as_deref(), Some("\"x\""));

    // Duplicate corpus ids, duplicate golden ids, and unpaired golden ids are
    // malformed provenance, not alternate representations to silently merge.
    std::fs::write(
        &corpus,
        r#"{"entries":[{"id":"a","input":"\"x\"","accept":true},{"id":"a","input":"\"y\"","accept":true}]}"#,
    )
    .unwrap();
    assert!(matches!(
        crate::corpus::parse_text_corpus(&corpus, &golden),
        Err(CorpusError::Malformed(m)) if m.contains("duplicate id")
    ));
    std::fs::write(
        &corpus,
        r#"{"entries":[{"id":"a","input":"\"x\"","accept":true}]}"#,
    )
    .unwrap();
    std::fs::write(
        &golden,
        r#"{"entries":[{"id":"a","accept":true,"canonical":"\"x\""},{"id":"a","accept":true,"canonical":"\"y\""}]}"#,
    )
    .unwrap();
    assert!(matches!(
        crate::corpus::parse_text_corpus(&corpus, &golden),
        Err(CorpusError::Malformed(m)) if m.contains("duplicate id")
    ));
    std::fs::write(
        &golden,
        r#"{"entries":[{"id":"a","accept":true,"canonical":"\"x\""},{"id":"b","accept":true,"canonical":"\"y\""}]}"#,
    )
    .unwrap();
    assert!(matches!(
        crate::corpus::parse_text_corpus(&corpus, &golden),
        Err(CorpusError::Malformed(m)) if m.contains("not present")
    ));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn accepted_components_reencode_byte_identical_to_golden() {
    let corpus = text_corpus().expect("corpus present (see non-vacuous test)");
    let codec = component_codec();
    let mut accepted_but_rejected: Vec<&str> = Vec::new();

    for e in corpus.iter().filter(|e| e.accept) {
        match rust_component_json(&e.input, &codec) {
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

    // Paper-accepted entries the Rust codec rejects must be exactly the
    // documented divergences (empty now that ClickEvent/HoverEvent landed) —
    // never a silent surprise.
    let mut expected: Vec<&str> = DOCUMENTED_STUB_DIVERGENCES.to_vec();
    accepted_but_rejected.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        accepted_but_rejected, expected,
        "accepted-but-rejected set must equal the documented codec divergences"
    );
}

#[test]
fn rejected_entries_are_rejected_by_rust() {
    let corpus = text_corpus().expect("corpus present (see non-vacuous test)");
    let codec = component_codec();
    let rejects: Vec<&TextFixtureEntry> = corpus.iter().filter(|e| !e.accept).collect();
    assert!(!rejects.is_empty(), "corpus must include rejected fixtures");
    for e in rejects {
        assert!(
            rust_component_json(&e.input, &codec).is_err(),
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
    let corpus = text_corpus().expect("corpus present (see non-vacuous test)");
    let by_id: std::collections::HashMap<&str, &TextFixtureEntry> =
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
    let corpus = text_corpus().expect("corpus present (see non-vacuous test)");
    let codec = component_codec();
    let by_id: std::collections::HashMap<&str, &TextFixtureEntry> =
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
            rust_component_json(&entry.input, &codec).is_err(),
            "Rust must reject {id} too (the field name is wrong for the ported \
             ClickEvent/HoverEvent codec)"
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

/// Issue #207: a `hover_event.show_text` whose value is itself a styled
/// `Component` recurses into the `Component` graph through `Style.Serializer`
/// -> `HoverEvent` -> `ComponentSerialization.CODEC` (the threaded `top`).
/// Repeated encodes must reuse the single cached graph — `CODEC_BUILD_COUNT`
/// (a thread-local counter of `component_serialization::codec()` constructions)
/// must stay flat, never growing one strong `Arc` cycle per encode.
#[test]
fn hover_show_text_nested_component_reuses_component_graph() {
    use crate::component_serialization::CODEC_BUILD_COUNT;

    let graph_count = || CODEC_BUILD_COUNT.with(|c| c.get());

    // A nested show_text value that does NOT collapse to a bare string (styled
    // bold), so it decodes as a true `Component` arg and the encode re-enters
    // the Component codec through the hover path.
    let input = "{\"text\":\"root\",\"hover_event\":{\"action\":\"show_text\",\
                 \"value\":{\"text\":\"nested\",\"bold\":true}}}";

    let codec = component_codec();
    let first = rust_component_json(input, &codec)
        .unwrap_or_else(|e| panic!("hover show_text must decode+re-encode: {e}"));
    let after_first = graph_count();

    for _ in 0..50 {
        let again = rust_component_json(input, &codec).expect("repeated encode must succeed");
        assert_eq!(again, first, "hover show_text re-encode must be stable");
    }
    let after_repeat = graph_count();
    assert_eq!(
        after_repeat, after_first,
        "nested hover show_text rebuilt the recursive Component graph per use (leak)"
    );
}
