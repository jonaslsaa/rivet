//! Normalized-transcript comparator.
//!
//! Diffs two normalized transcripts (serde_json objects) into a structured
//! list of field-level differences. The diff is deterministic and exhaustive:
//! every differing field is reported with its path, expected value (run A) and
//! actual value (run B). Arrays are compared element-wise by index; a length
//! difference is reported at `path.length`.
//!
//! A transcript may carry an `excluded` object: an explicit, justified
//! declaration of nondeterministic fields (WORKFLOWS.md §headless-client-driver).
//! The comparator never silently ignores a difference — any field listed in
//! `excluded` is skipped and reported in `excluded`, and any field that is
//! *present* on both sides but differs while NOT listed is reported as a
//! normal diff. If the two transcripts disagree about the `excluded` set
//! itself, that is reported as a diff (the exclusion policy must be stable
//! across the runs being compared).

use std::collections::BTreeSet;

use serde_json::{Map, Value};

/// A single differing field between two transcripts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDiff {
    /// Dot/bracket path into the transcript, e.g. `position.y` or `chunks[3][1]`.
    pub path: String,
    /// Value in the first (expected/reference) transcript.
    pub expected: Value,
    /// Value in the second (actual/comparison) transcript.
    pub actual: Value,
}

impl std::fmt::Display for FieldDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: expected {}, got {}",
            self.path, self.expected, self.actual
        )
    }
}

/// Result of comparing two normalized transcripts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscriptDiff {
    /// Non-excluded fields that differ between the two transcripts.
    pub diffs: Vec<FieldDiff>,
    /// Fields that differ but were explicitly excluded from parity by the
    /// transcripts' `excluded` declarations (with justification). Reporting
    /// these proves the exclusion was applied, never silent.
    pub excluded: Vec<FieldDiff>,
    /// Fields listed in `excluded` by one side but not the other. The
    /// exclusion policy must be identical for both runs being compared.
    pub excluded_policy_diffs: Vec<FieldDiff>,
}

impl TranscriptDiff {
    pub fn is_identical(&self) -> bool {
        self.diffs.is_empty()
    }
}

/// Read the `excluded` map from a transcript (an object of path -> reason).
fn excluded_map(transcript: &Value) -> Map<String, Value> {
    match transcript.get("excluded") {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    }
}

/// Compare `expected` (the reference transcript) against `actual`.
pub fn diff(expected: &Value, actual: &Value) -> TranscriptDiff {
    let excluded_expected = excluded_map(expected);
    let excluded_actual = excluded_map(actual);
    let mut excluded = BTreeSet::new();
    excluded.extend(excluded_expected.keys().cloned());
    excluded.extend(excluded_actual.keys().cloned());

    let mut result = TranscriptDiff::default();

    // Compare the exclusion policy itself.
    for path in &excluded {
        let (e, a) = (excluded_expected.get(path), excluded_actual.get(path));
        if e != a {
            result.excluded_policy_diffs.push(FieldDiff {
                path: format!("excluded.{path}"),
                expected: e.cloned().unwrap_or(Value::Null),
                actual: a.cloned().unwrap_or(Value::Null),
            });
        }
    }

    // Compare the transcripts, skipping excluded paths.
    let mut diffs = Vec::new();
    diff_at("", expected, actual, &excluded, &mut diffs);
    for field in diffs {
        if excluded.contains(field.path.as_str()) {
            result.excluded.push(field);
        } else {
            result.diffs.push(field);
        }
    }
    result
}

fn diff_at(
    path: &str,
    expected: &Value,
    actual: &Value,
    excluded: &BTreeSet<String>,
    out: &mut Vec<FieldDiff>,
) {
    // Never recurse into an excluded subtree: record a single diff at the
    // excluded path (if the values differ at all) and stop.
    if !path.is_empty() && excluded.contains(path) {
        if expected != actual {
            out.push(FieldDiff {
                path: path.to_string(),
                expected: expected.clone(),
                actual: actual.clone(),
            });
        }
        return;
    }
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            let mut keys: BTreeSet<&String> = BTreeSet::new();
            keys.extend(e.keys());
            keys.extend(a.keys());
            for key in keys {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match (e.get(key), a.get(key)) {
                    (Some(x), Some(y)) => diff_at(&child_path, x, y, excluded, out),
                    (Some(x), None) => out.push(FieldDiff {
                        path: child_path,
                        expected: x.clone(),
                        actual: Value::Null,
                    }),
                    (None, Some(y)) => out.push(FieldDiff {
                        path: child_path,
                        expected: Value::Null,
                        actual: y.clone(),
                    }),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(e), Value::Array(a)) => {
            if e.len() != a.len() {
                out.push(FieldDiff {
                    path: format!("{path}.length"),
                    expected: Value::from(e.len()),
                    actual: Value::from(a.len()),
                });
            }
            for (i, (x, y)) in e.iter().zip(a.iter()).enumerate() {
                diff_at(&format!("{path}[{i}]"), x, y, excluded, out);
            }
        }
        _ => {
            if expected != actual {
                out.push(FieldDiff {
                    path: path.to_string(),
                    expected: expected.clone(),
                    actual: actual.clone(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "protocol": 1,
            "scenario": "join",
            "outcome": "spawned",
            "lifecycle": ["init", "login", "spawn"],
            "position": { "x": 9.5, "y": -60.0, "z": -3.5 },
            "world": "minecraft:overworld",
            "gamemode": "survival",
            "health": { "health": 20.0, "food": 20, "saturation": 5.0 },
            "chunk_count": 117,
            "chunks": [[-4, -4], [-4, -3], [0, 0], [4, 4]],
            "excluded": {
                "position.x": "randomized per boot",
                "position.z": "randomized per boot",
            },
        })
    }

    #[test]
    fn identical_transcripts_diff_clean() {
        let a = sample();
        let b = sample();
        let d = diff(&a, &b);
        assert!(
            d.is_identical(),
            "identical transcripts must diff clean: {d:?}"
        );
        assert!(d.excluded.is_empty());
        assert!(d.excluded_policy_diffs.is_empty());
    }

    #[test]
    fn tampered_compared_field_is_detected_as_a_single_diff() {
        let a = sample();
        let mut b = sample();
        // position.y is NOT excluded — tampering it must be detected.
        b["position"]["y"] = json!(-59.0);
        let d = diff(&a, &b);
        assert!(!d.is_identical());
        assert_eq!(d.diffs.len(), 1, "exactly one field should differ: {d:?}");
        assert_eq!(d.diffs[0].path, "position.y");
        assert_eq!(d.diffs[0].expected, json!(-60.0));
        assert_eq!(d.diffs[0].actual, json!(-59.0));
        assert!(d.excluded.is_empty());
    }

    #[test]
    fn tampered_compared_field_detected_when_expected_and_actual_both_nonzero() {
        // Tamper a value that differs on both sides but is a compared field.
        let a = sample();
        let mut b = sample();
        b["health"]["health"] = json!(19.5);
        let d = diff(&a, &b);
        assert!(!d.is_identical());
        assert_eq!(d.diffs[0].path, "health.health");
        assert_eq!(d.diffs[0].actual, json!(19.5));
    }

    #[test]
    fn excluded_nondeterministic_field_is_reported_not_compared() {
        let a = sample();
        let mut b = sample();
        // position.x/z differ but are excluded — must be reported in `excluded`,
        // not in `diffs`, and the transcript must still be "identical".
        b["position"]["x"] = json!(-4.5);
        b["position"]["z"] = json!(-5.5);
        let d = diff(&a, &b);
        assert!(
            d.is_identical(),
            "excluded diffs must not fail parity: {d:?}"
        );
        assert!(d.diffs.is_empty());
        assert_eq!(d.excluded.len(), 2);
        let paths: Vec<&str> = d.excluded.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"position.x"));
        assert!(paths.contains(&"position.z"));
        assert!(d.excluded_policy_diffs.is_empty());
    }

    #[test]
    fn differing_exclusion_policy_is_detected() {
        let a = sample();
        let mut b = sample();
        b.as_object_mut().unwrap()["excluded"]
            .as_object_mut()
            .unwrap()
            .remove("position.z");
        let d = diff(&a, &b);
        assert!(!d.is_identical());
        assert_eq!(d.excluded_policy_diffs.len(), 1);
        assert_eq!(d.excluded_policy_diffs[0].path, "excluded.position.z");
    }

    #[test]
    fn changed_chunk_count_is_detected() {
        let a = sample();
        let mut b = sample();
        b["chunk_count"] = json!(80);
        b["chunks"] = json!([[-4, -4], [-4, -3], [0, 0]]);
        let d = diff(&a, &b);
        assert!(!d.is_identical());
        let paths: Vec<&str> = d.diffs.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"chunk_count"));
        assert!(paths.contains(&"chunks.length"));
    }

    #[test]
    fn missing_top_level_field_is_detected() {
        let a = sample();
        let mut b = sample();
        b.as_object_mut().unwrap().remove("gamemode");
        let d = diff(&a, &b);
        assert!(!d.is_identical());
        assert_eq!(d.diffs[0].path, "gamemode");
        assert_eq!(d.diffs[0].actual, Value::Null);
    }
}
