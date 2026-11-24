//! The `brigadier.dispatch` corpus, committed as `corpus/brigadier.dispatch.jsonl`.
//!
//! Each input is one dispatcher spec (JSON, built identically by Paper and by
//! Rivet), one command string, a suggestion cursor and the facets wanted. The
//! committed sources under `tools/rivet-parity/brigadier-specs/`:
//!
//! - `<spec>.json`, six dispatcher specs: `basic` (every built-in argument
//!   type, bounds, string modes, optional trailing arguments, failing commands),
//!   `ambiguous` (overlapping sibling arguments), `forks` (redirects, forks,
//!   forwards, modifiers that throw), `usage` (optional branches, restricted
//!   nodes, redirects in usage strings) and `vanilla` (the structure of real
//!   vanilla commands: `execute`'s redirect/fork graph, `tell` redirecting to
//!   `msg`, `time`/`weather`/`xp`/`effect`/`tp`/... with vanilla bounds, op-only
//!   commands behind a fixed `requires`) and `suggest` (fixed suggestion providers
//!   with text, integer and tooltip entries);
//! - `<spec>.cases.jsonl`, the hand-written command strings of that spec: a JSON
//!   string, or `[string, cursor...]` for extra suggestion cursors, where `"*"`
//!   asks for every cursor from 0 to the end of the string.
//!
//! On top of the hand cases, per-spec seeded generation walks the spec to build
//! valid commands out of boundary-biased argument values and then damages them
//! (truncation, inserted quotes and escapes, doubled and trailing whitespace,
//! swapped characters). Per spec, one `usage` input per node (the vanilla spec:
//! every root command plus a few deeper nodes) and one `ambiguities` input.
//!
//! Each spec's tree-level inputs (`usage`, `ambiguities`) come first, then its
//! hand cases, then its seeded ones.
//!
//! Paper answers every input; nothing here hand-types an expected value.

use serde_json::{Map, Value, json};

use super::packet_corpus::Rng;

/// `(name, spec text, hand cases, command strings per suggestion input,
/// seeded commands)`.
struct SpecSource {
    name: &'static str,
    spec: &'static str,
    cases: &'static str,
    /// Every n-th hand case also gets a suggestion input at the end of the
    /// command (the vanilla spec is large, so it is sampled).
    suggest_every: usize,
    seeded: usize,
}

const SPECS: &[SpecSource] = &[
    SpecSource {
        name: "basic",
        spec: include_str!("../../brigadier-specs/basic.json"),
        cases: include_str!("../../brigadier-specs/basic.cases.jsonl"),
        suggest_every: 2,
        seeded: 60,
    },
    SpecSource {
        name: "ambiguous",
        spec: include_str!("../../brigadier-specs/ambiguous.json"),
        cases: include_str!("../../brigadier-specs/ambiguous.cases.jsonl"),
        suggest_every: 1,
        seeded: 20,
    },
    SpecSource {
        name: "forks",
        spec: include_str!("../../brigadier-specs/forks.json"),
        cases: include_str!("../../brigadier-specs/forks.cases.jsonl"),
        suggest_every: 2,
        seeded: 30,
    },
    SpecSource {
        name: "usage",
        spec: include_str!("../../brigadier-specs/usage.json"),
        cases: include_str!("../../brigadier-specs/usage.cases.jsonl"),
        suggest_every: 2,
        seeded: 20,
    },
    SpecSource {
        name: "suggest",
        spec: include_str!("../../brigadier-specs/suggest.json"),
        cases: include_str!("../../brigadier-specs/suggest.cases.jsonl"),
        suggest_every: 1,
        seeded: 40,
    },
    SpecSource {
        name: "vanilla",
        spec: include_str!("../../brigadier-specs/vanilla.json"),
        cases: include_str!("../../brigadier-specs/vanilla.cases.jsonl"),
        suggest_every: 4,
        seeded: 30,
    },
];

/// Nodes of the vanilla spec whose usage is asked in addition to every root command.
const VANILLA_USAGE_EXTRA: &[&[&str]] = &[
    &["execute", "if"],
    &["execute", "store", "result"],
    &["xp", "add"],
    &["effect", "give"],
    &["time", "set"],
    &["scoreboard", "players"],
    &["tp", "targets"],
];

fn input_line(
    id: String,
    spec: &Value,
    input: &str,
    cursor: Option<i32>,
    want: &[&str],
    usage_path: Option<&[String]>,
) -> String {
    let mut object = Map::new();
    object.insert("id".into(), json!(id));
    object.insert("spec".into(), spec.clone());
    object.insert("input".into(), json!(input));
    if let Some(cursor) = cursor {
        object.insert("cursor".into(), json!(cursor));
    }
    object.insert("want".into(), json!(want));
    if let Some(path) = usage_path {
        object.insert("usage_path".into(), json!(path));
    }
    serde_json::to_string(&Value::Object(object)).expect("JSON values always serialize") + "\n"
}

fn utf16_len(text: &str) -> i32 {
    text.encode_utf16().count() as i32
}

/// A hand case: the command and its extra suggestion cursors.
fn parse_cases(name: &str, text: &str) -> Result<Vec<(String, Vec<i32>)>, String> {
    let bad = |index: usize, what: &str| format!("{name}.cases.jsonl:{}: {what}", index + 1);
    let mut cases = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let value: Value = serde_json::from_str(line)
            .map_err(|e| format!("{name}.cases.jsonl:{}: {e}", index + 1))?;
        match value {
            Value::String(command) => cases.push((command, Vec::new())),
            Value::Array(items) => {
                let mut items = items.into_iter();
                let Some(Value::String(command)) = items.next() else {
                    return Err(format!("{name}.cases.jsonl:{}: bad case", index + 1));
                };
                let mut cursors = Vec::new();
                for item in items {
                    match item {
                        Value::String(star) if star == "*" => {
                            cursors.extend(0..=utf16_len(&command));
                        }
                        other => cursors.push(
                            other
                                .as_i64()
                                .and_then(|c| i32::try_from(c).ok())
                                .ok_or_else(|| bad(index, "bad cursor"))?,
                        ),
                    }
                }
                cases.push((command, cursors));
            }
            _ => return Err(format!("{name}.cases.jsonl:{}: bad case", index + 1)),
        }
    }
    Ok(cases)
}

// ---- seeded generation ---------------------------------------------------------

const NOISE: &[&str] = &[
    " ", "  ", "\"", "'", "\\", "\\\"", "\t", "-", ".", "1", "a", "@", "\u{e9}", "\u{65e5}", "/",
    "~", "\\n", "9", "e",
];

fn pick<'a>(rng: &mut Rng, items: &[&'a str]) -> &'a str {
    items[rng.below(items.len() as u64) as usize]
}

/// A boundary-biased value for an argument type, valid or not.
fn argument_text(rng: &mut Rng, kind: &Map<String, Value>) -> String {
    let bound = |key: &str| kind.get(key).map(|v| v.to_string());
    let mut choices: Vec<String> = Vec::new();
    match kind.get("kind").and_then(Value::as_str).unwrap_or("") {
        "bool" => choices.extend(
            [
                "true", "false", "true", "false", "TRUE", "yes", "1", "tru", "falsex",
            ]
            .map(str::to_owned),
        ),
        "integer" | "long" => {
            choices.extend(
                [
                    "0",
                    "1",
                    "-1",
                    "7",
                    "42",
                    "100",
                    "255",
                    "1000",
                    "2147483647",
                    "-2147483648",
                    "2147483648",
                    "x",
                    "1.5",
                    "-",
                    "1x",
                    "9223372036854775807",
                    "9223372036854775808",
                ]
                .map(str::to_owned),
            );
            choices.extend(bound("min"));
            choices.extend(bound("max"));
            for key in ["min", "max"] {
                if let Some(value) = kind.get(key).and_then(Value::as_i64) {
                    choices.push((value - 1).to_string());
                    choices.push((value.saturating_add(1)).to_string());
                }
            }
        }
        "float" | "double" => {
            choices.extend(
                [
                    "0",
                    "1",
                    "-1",
                    "1.5",
                    "0.5",
                    ".5",
                    "5.",
                    "1.2.3",
                    "-",
                    "abc",
                    "0.1",
                    "1e5",
                    "3.4028235",
                    "1000000000000000000000000000000000000000",
                    "123456789.123456789",
                    "0.30000000000000004",
                ]
                .map(str::to_owned),
            );
            choices.extend(bound("min"));
            choices.extend(bound("max"));
            for key in ["min", "max"] {
                if let Some(value) = kind.get(key).and_then(Value::as_f64) {
                    choices.push(format!("{}", value - 0.001));
                    choices.push(format!("{}", value + 0.001));
                }
            }
        }
        "string" => match kind.get("mode").and_then(Value::as_str).unwrap_or("") {
            "word" => choices.extend(
                [
                    "a",
                    "foo",
                    "a_b",
                    "a-b",
                    "a.b",
                    "@e",
                    "Steve",
                    "x:y",
                    "\u{65e5}\u{672c}",
                    "\u{e9}",
                    "1",
                    "true",
                    "a+b",
                    "\"q\"",
                    "a\"b",
                ]
                .map(str::to_owned),
            ),
            "quotable" => choices.extend(
                [
                    "foo",
                    "\"a b\"",
                    "'a b'",
                    "\"a\\\"b\"",
                    "\"unclosed",
                    "''",
                    "\"\"",
                    "'a\\'b'",
                    "\"a\\\\b\"",
                    "\"a\\nb\"",
                    "a\\",
                    "\"a\"b",
                    "\"a b\" c",
                ]
                .map(str::to_owned),
            ),
            _ => choices.extend(
                [
                    "hello world",
                    "  spaced  ",
                    "\"quoted\" text",
                    "a\\b",
                    "x",
                    "\"unclosed",
                    "\u{e9} \u{65e5}",
                ]
                .map(str::to_owned),
            ),
        },
        _ => choices.push("?".into()),
    }
    let index = rng.below(choices.len() as u64) as usize;
    choices.swap_remove(index)
}

/// Walks the spec from a random root command down to a random depth, producing
/// the command words (following redirects into the target's own children).
fn generate(rng: &mut Rng, commands: &[Value]) -> String {
    let start = &commands[rng.below(commands.len() as u64) as usize];
    let mut words = vec![start["literal"].as_str().unwrap_or("").to_owned()];
    let mut node = start;
    for _ in 0..14 {
        let mut here = node;
        if let Some(redirect) = node.get("redirect") {
            let target = redirect["to"].as_str().unwrap_or("root");
            if target == "root" {
                let next = &commands[rng.below(commands.len() as u64) as usize];
                words.push(next["literal"].as_str().unwrap_or("").to_owned());
                node = next;
                continue;
            }
            match commands.iter().find(|c| c["literal"] == target) {
                Some(found) => here = found,
                None => break,
            }
        }
        let Some(children) = here.get("children").and_then(Value::as_array) else {
            break;
        };
        if children.is_empty() || rng.below(6) == 0 {
            break;
        }
        let child = &children[rng.below(children.len() as u64) as usize];
        match child.get("literal").and_then(Value::as_str) {
            Some(name) => words.push(name.to_owned()),
            None => {
                let kind = child["type"]
                    .as_object()
                    .expect("argument types are objects");
                words.push(argument_text(rng, kind));
            }
        }
        node = child;
    }
    words.join(" ")
}

/// Damages a command string the way a typo, a paste or a hostile client would.
fn damage(rng: &mut Rng, command: &str) -> String {
    let mut chars: Vec<char> = command.chars().collect();
    match rng.below(9) {
        0 if !chars.is_empty() => {
            let at = rng.below(chars.len() as u64) as usize;
            chars.truncate(at);
        }
        1 if !chars.is_empty() => {
            let at = rng.below(chars.len() as u64) as usize;
            chars.remove(at);
        }
        2 => {
            let at = rng.below(chars.len() as u64 + 1) as usize;
            for c in pick(rng, NOISE).chars().rev() {
                chars.insert(at, c);
            }
        }
        3 if !chars.is_empty() => {
            let at = rng.below(chars.len() as u64) as usize;
            let noise: Vec<char> = pick(rng, NOISE).chars().collect();
            chars.splice(at..=at, noise);
        }
        4 => chars.push(' '),
        5 => chars.extend("  ".chars()),
        6 => chars.extend(" x".chars()),
        7 => {
            if let Some(at) = chars.iter().position(|&c| c == ' ') {
                chars.insert(at, ' ');
            }
        }
        _ => {
            if chars.len() > 1 {
                let at = rng.below(chars.len() as u64 - 1) as usize;
                chars.swap(at, at + 1);
            }
        }
    }
    chars.into_iter().collect()
}

// ---- usage paths -------------------------------------------------------------------

fn node_paths(node: &Value, path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    let name = node
        .get("literal")
        .or_else(|| node.get("argument"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    path.push(name);
    out.push(path.clone());
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            node_paths(child, path, out);
        }
    }
    path.pop();
}

fn usage_paths(source: &SpecSource, commands: &[Value]) -> Vec<Vec<String>> {
    let mut all = Vec::new();
    for command in commands {
        node_paths(command, &mut Vec::new(), &mut all);
    }
    let mut paths = vec![Vec::new()];
    if source.name == "vanilla" {
        paths.extend(all.iter().filter(|p| p.len() == 1).cloned());
        // Only the nodes deeper than the root commands: the root ones are above.
        paths.extend(
            VANILLA_USAGE_EXTRA
                .iter()
                .filter(|p| p.len() > 1)
                .map(|p| p.iter().map(|s| (*s).to_owned()).collect()),
        );
    } else {
        paths.extend(all);
    }
    paths
}

/// The whole corpus, as committed.
pub fn emit() -> Result<String, String> {
    let mut out = String::new();
    for source in SPECS {
        let spec: Value =
            serde_json::from_str(source.spec).map_err(|e| format!("{}.json: {e}", source.name))?;
        let commands = spec["commands"]
            .as_array()
            .ok_or_else(|| format!("{}.json has no commands", source.name))?
            .clone();
        let name = source.name;
        let mut seen = std::collections::BTreeSet::new();

        for path in usage_paths(source, &commands) {
            let label = if path.is_empty() {
                "<root>".to_owned()
            } else {
                path.join("/")
            };
            out += &input_line(
                format!("{name}.usage.{label}"),
                &spec,
                "",
                None,
                &["usage"],
                Some(&path),
            );
        }
        out += &input_line(
            format!("{name}.ambiguities"),
            &spec,
            "",
            None,
            &["ambiguities"],
            None,
        );

        for (index, (command, cursors)) in parse_cases(name, source.cases)?.iter().enumerate() {
            seen.insert(command.clone());
            out += &input_line(
                format!("{name}.run.h{index:03}"),
                &spec,
                command,
                None,
                &["run"],
                None,
            );
            if index % source.suggest_every == 0 {
                out += &input_line(
                    format!("{name}.sug.h{index:03}"),
                    &spec,
                    command,
                    Some(utf16_len(command)),
                    &["suggestions"],
                    None,
                );
            }
            for cursor in cursors {
                out += &input_line(
                    format!("{name}.sug.h{index:03}@{cursor}"),
                    &spec,
                    command,
                    Some(*cursor),
                    &["suggestions"],
                    None,
                );
            }
        }

        let mut rng = Rng::for_stream(&format!("brigadier.dispatch.{name}"));
        let mut made = 0;
        let mut attempts = 0;
        while made < source.seeded && attempts < source.seeded * 20 {
            attempts += 1;
            let mut command = generate(&mut rng, &commands);
            if rng.below(10) < 6 {
                command = damage(&mut rng, &command);
                if rng.below(4) == 0 {
                    command = damage(&mut rng, &command);
                }
            }
            if !seen.insert(command.clone()) {
                continue;
            }
            out += &input_line(
                format!("{name}.run.s{made:03}"),
                &spec,
                &command,
                None,
                &["run"],
                None,
            );
            let length = utf16_len(&command);
            if made % 2 == 0 {
                out += &input_line(
                    format!("{name}.sug.s{made:03}"),
                    &spec,
                    &command,
                    Some(length),
                    &["suggestions"],
                    None,
                );
            } else {
                let cursor = rng.below(length as u64 + 1) as i32;
                out += &input_line(
                    format!("{name}.sug.s{made:03}@{cursor}"),
                    &spec,
                    &command,
                    Some(cursor),
                    &["suggestions"],
                    None,
                );
            }
            made += 1;
        }
        if made < source.seeded {
            return Err(format!("{name}: only {made} distinct seeded commands"));
        }
    }
    Ok(out)
}
