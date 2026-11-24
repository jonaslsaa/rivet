//! The arena-trace comparator (docs/VERIFICATION.md section 2.4): canonical
//! JSON lines compared byte for byte, first divergence reported as tick and
//! field path. Field exclusions are applied by both emitters from
//! `arenas/exclusions.tsv`, so nothing is dropped here.

use serde_json::Value;

/// Re-render one trace line in the canonical form: compact, keys in the
/// emitter's fixed order, numbers as written. Both Gson and serde_json lines
/// map onto the same bytes when their values agree.
pub fn canonical(line: &str) -> Result<String, String> {
    serde_json::from_str::<Value>(line)
        .map(|v| v.to_string())
        .map_err(|e| format!("not a JSON line ({e}): {}", truncate(line, 120)))
}

pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        format!("{}...", text.chars().take(max).collect::<String>())
    }
}

/// The first point where `actual` departs from `expected`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    /// The tick record (`t`) of the expected line, or None for header/end.
    pub tick: Option<i64>,
    /// Field path inside the record, e.g. `.fluid_ticks[0][4]`.
    pub path: String,
    pub expected: String,
    pub actual: String,
}

impl std::fmt::Display for Divergence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.tick {
            Some(t) => write!(f, "t={t} ")?,
            None => write!(f, "record ")?,
        }
        write!(
            f,
            "{} paper={} rivet={}",
            if self.path.is_empty() {
                "."
            } else {
                &self.path
            },
            truncate(&self.expected, 160),
            truncate(&self.actual, 160)
        )
    }
}

/// Result of comparing two traces of the same arena.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    /// Tick records identical before the first divergence.
    pub matched: usize,
    /// Tick records declared by the expected trace.
    pub declared: usize,
    pub divergence: Option<Divergence>,
}

impl Comparison {
    pub fn passed(&self) -> bool {
        self.divergence.is_none() && self.matched == self.declared
    }
    pub fn mismatched(&self) -> usize {
        self.declared - self.matched
    }
}

fn path_diff(expected: &Value, actual: &Value, path: &mut String) -> Option<(String, String)> {
    match (expected, actual) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                let len = path.len();
                path.push('.');
                path.push_str(key);
                match b.get(key) {
                    Some(other) => {
                        if let Some(found) = path_diff(value, other, path) {
                            return Some(found);
                        }
                    }
                    None => return Some((value.to_string(), "<absent>".into())),
                }
                path.truncate(len);
            }
            if let Some(key) = b.keys().find(|k| !a.contains_key(*k)) {
                path.push('.');
                path.push_str(key);
                return Some(("<absent>".into(), b[key].to_string()));
            }
            // Same members, different order.
            (a.keys().ne(b.keys())).then(|| ("<key order>".into(), "<key order>".into()))
        }
        (Value::Array(a), Value::Array(b)) => {
            for (i, value) in a.iter().enumerate() {
                let len = path.len();
                path.push_str(&format!("[{i}]"));
                match b.get(i) {
                    Some(other) => {
                        if let Some(found) = path_diff(value, other, path) {
                            return Some(found);
                        }
                    }
                    None => return Some((value.to_string(), "<absent>".into())),
                }
                path.truncate(len);
            }
            if b.len() > a.len() {
                path.push_str(&format!("[{}]", a.len()));
                return Some(("<absent>".into(), b[a.len()].to_string()));
            }
            None
        }
        _ => (expected != actual).then(|| (expected.to_string(), actual.to_string())),
    }
}

/// Compare an expected (Paper) trace with an actual one (Rivet, or a Paper
/// twin). Lines are compared in order; the first canonical mismatch stops.
pub fn compare(expected: &str, actual: &str) -> Result<Comparison, String> {
    let expected: Vec<&str> = expected.lines().collect();
    let actual: Vec<&str> = actual.lines().collect();
    let declared = expected
        .iter()
        .filter(|l| l.starts_with("{\"kind\":\"tick\""))
        .count();
    let mut matched = 0;
    for (index, line) in expected.iter().enumerate() {
        let want = canonical(line)?;
        let want_value: Value = serde_json::from_str(&want).expect("canonical");
        let tick = (want_value["kind"] == "tick")
            .then(|| want_value["t"].as_i64())
            .flatten();
        let Some(other) = actual.get(index) else {
            return Ok(Comparison {
                matched,
                declared,
                divergence: Some(Divergence {
                    tick,
                    path: String::new(),
                    expected: truncate(&want, 160),
                    actual: "<trace ended>".into(),
                }),
            });
        };
        let got = canonical(other)?;
        if got == want {
            if tick.is_some() {
                matched += 1;
            }
            continue;
        }
        let got_value: Value = serde_json::from_str(&got).expect("canonical");
        let divergence = if got_value["kind"] == "error" {
            Divergence {
                tick,
                path: format!(
                    "<rivet stopped in {}>",
                    got_value["phase"].as_str().unwrap_or("?")
                ),
                expected: "<record>".into(),
                actual: got_value["message"].as_str().unwrap_or_default().to_owned(),
            }
        } else {
            let mut path = String::new();
            let (expected, actual) = path_diff(&want_value, &got_value, &mut path)
                .unwrap_or_else(|| (want.clone(), got.clone()));
            Divergence {
                tick,
                path,
                expected,
                actual,
            }
        };
        return Ok(Comparison {
            matched,
            declared,
            divergence: Some(divergence),
        });
    }
    if actual.len() > expected.len() {
        return Ok(Comparison {
            matched,
            declared,
            divergence: Some(Divergence {
                tick: None,
                path: String::new(),
                expected: "<trace ended>".into(),
                actual: truncate(actual[expected.len()], 160),
            }),
        });
    }
    Ok(Comparison {
        matched,
        declared,
        divergence: None,
    })
}

/// The tamper negative: one deterministic edit to a trace's last tick record
/// (an extra block-state change at the box origin). A comparator that does
/// not report it at that tick and path is broken.
pub fn tamper(trace: &str) -> Result<(String, i64), String> {
    let mut lines: Vec<String> = trace.lines().map(str::to_owned).collect();
    let index = lines
        .iter()
        .rposition(|l| l.starts_with("{\"kind\":\"tick\""))
        .ok_or("trace has no tick record to tamper")?;
    let mut record: Value = serde_json::from_str(&lines[index]).map_err(|e| e.to_string())?;
    let t = record["t"].as_i64().ok_or("tick record without t")?;
    record["blocks"]
        .as_array_mut()
        .ok_or("tick record without blocks")?
        .push(serde_json::json!([0, 0, 0, 65535]));
    lines[index] = record.to_string();
    Ok((lines.join("\n") + "\n", t))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = r#"{"kind":"header","schema":"rivet.arena-trace/1","arena":"a"}"#;

    fn trace(ticks: &[&str]) -> String {
        let mut out = vec![HEADER.to_owned()];
        out.extend(ticks.iter().map(|t| (*t).to_owned()));
        out.push(r#"{"kind":"end","ticks":1}"#.into());
        out.join("\n") + "\n"
    }

    #[test]
    fn gson_and_serde_spellings_of_the_same_record_are_equal() {
        // Gson escapes `=` only with HTML escaping on; the probe turns it off,
        // and both sides escape a control character differently.
        assert_eq!(
            canonical(r#"{"a":"x=y\u000a","b":[1,-2]}"#).unwrap(),
            canonical("{\"a\":\"x=y\\n\",\"b\":[1,-2]}").unwrap()
        );
    }

    #[test]
    fn first_divergence_names_tick_and_field_path() {
        let paper = trace(&[
            r#"{"kind":"tick","t":0,"blocks":[[0,0,0,1]],"fluid_ticks":[]}"#,
            r#"{"kind":"tick","t":1,"blocks":[],"fluid_ticks":[[4,1,4,"minecraft:water",5,0]]}"#,
        ]);
        let rivet = trace(&[
            r#"{"kind":"tick","t":0,"blocks":[[0,0,0,1]],"fluid_ticks":[]}"#,
            r#"{"kind":"tick","t":1,"blocks":[],"fluid_ticks":[[4,1,4,"minecraft:water",4,0]]}"#,
        ]);
        let result = compare(&paper, &rivet).unwrap();
        assert!(!result.passed());
        assert_eq!(result.matched, 1);
        assert_eq!(result.mismatched(), 1);
        let d = result.divergence.unwrap();
        assert_eq!((d.tick, d.path.as_str()), (Some(1), ".fluid_ticks[0][4]"));
        assert_eq!((d.expected.as_str(), d.actual.as_str()), ("5", "4"));
        assert!(compare(&paper, &paper).unwrap().passed());
    }

    #[test]
    fn rivet_error_line_and_short_trace_are_divergences() {
        let paper = trace(&[r#"{"kind":"tick","t":0,"blocks":[]}"#]);
        let stopped = format!(
            "{HEADER}\n{}\n",
            r#"{"kind":"error","t":0,"phase":"placement","message":"no furnace"}"#
        );
        let d = compare(&paper, &stopped).unwrap().divergence.unwrap();
        assert_eq!(d.tick, Some(0));
        assert_eq!(d.path, "<rivet stopped in placement>");
        assert_eq!(d.actual, "no furnace");
        let short = format!("{HEADER}\n");
        let d = compare(&paper, &short).unwrap().divergence.unwrap();
        assert_eq!(d.actual, "<trace ended>");
        let long = paper.clone() + "{\"kind\":\"tick\",\"t\":9}\n";
        assert!(!compare(&paper, &long).unwrap().passed());
    }

    #[test]
    fn missing_extra_and_reordered_fields_diverge() {
        let a = trace(&[r#"{"kind":"tick","t":0,"x":1,"y":2}"#]);
        let missing = trace(&[r#"{"kind":"tick","t":0,"x":1}"#]);
        let extra = trace(&[r#"{"kind":"tick","t":0,"x":1,"y":2,"z":3}"#]);
        let reordered = trace(&[r#"{"kind":"tick","t":0,"y":2,"x":1}"#]);
        for other in [missing, extra, reordered] {
            assert!(!compare(&a, &other).unwrap().passed(), "{other}");
        }
    }

    #[test]
    fn tamper_is_detected_at_the_last_tick() {
        let paper = trace(&[
            r#"{"kind":"tick","t":0,"blocks":[]}"#,
            r#"{"kind":"tick","t":1,"blocks":[]}"#,
        ]);
        let (tampered, t) = tamper(&paper).unwrap();
        assert_eq!(t, 1);
        let d = compare(&paper, &tampered).unwrap().divergence.unwrap();
        assert_eq!((d.tick, d.path.as_str()), (Some(1), ".blocks[0]"));
    }
}
