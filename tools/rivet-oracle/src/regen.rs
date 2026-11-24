//! `rivet-oracle regen --check`: reproduce every committed probe golden from
//! Paper, byte for byte, twice (VERIFICATION §2.1, §5 item 2).
//!
//! `tools/rivet-oracle/probes.tsv` maps every Java probe script (and the
//! reference oracle's op record mode) to the committed file(s) it produces.
//! For each selected row the check runs the producer twice into scratch
//! directories under the checkout's `work/regen/` (never the system temp dir:
//! `TMPDIR` and `java.io.tmpdir` point there too), byte-diffs run 1 against the
//! committed golden and run 1 against run 2, and reports one status per row:
//!
//! - `PASS` — both runs reproduce the committed bytes exactly;
//! - `FAIL` — the producer failed or timed out, a golden diverged (the first
//!   differing line is named), or the two runs differ (nondeterminism);
//! - `UNVERIFIED` — the row has no committed output (`none`), or a committed
//!   fixture has no producing row (orphan), or the Paper runtime / Java 25 is
//!   missing (then every row is UNVERIFIED; nothing is silently skipped).
//!
//! The Paper classpath (materialized runtime jar + library dir) is resolved
//! once and handed to every producer; producers that share a compile cache or a
//! working directory run in the same lane, lanes run `--jobs` at a time (each
//! probe is its own JVM; the op recorder streams every op through one JVM).
//! Exit: 0 all PASS, 1 any FAIL, 3 otherwise UNVERIFIED.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rivet_harness_common::closure::Status;

const HEADER: &str = "script\tmode\targs\tenv\toutputs\tnote";

/// How a row's producer delivers its output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// stdout == the outputs concatenated in order.
    Stdout,
    /// `{outN}` arguments are scratch files named like output N.
    Files,
    /// `{dir}` is a scratch dir; outputs are read from it by basename.
    Dir,
    /// No committed output exists.
    None,
}

/// One `probes.tsv` row.
#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub script: String,
    pub mode: Mode,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub outputs: Vec<String>,
    pub note: String,
}

fn dash_split(field: &str, separator: char) -> Vec<String> {
    if field == "-" {
        Vec::new()
    } else {
        field.split(separator).map(str::to_owned).collect()
    }
}

/// Parse and validate `probes.tsv`.
pub fn parse_table(text: &str) -> Result<Vec<Row>, String> {
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.starts_with('#') && !line.trim().is_empty());
    match lines.next() {
        Some((_, header)) if header == HEADER => {}
        _ => return Err(format!("probes.tsv must start with the header `{HEADER}`")),
    }
    let mut rows = Vec::new();
    for (index, line) in lines {
        let at = format!("probes.tsv:{}", index + 1);
        let columns: Vec<&str> = line.split('\t').collect();
        let [script, mode, args, env, outputs, note] = columns.as_slice() else {
            return Err(format!("{at}: expected 6 tab-separated columns"));
        };
        let mode = match *mode {
            "stdout" => Mode::Stdout,
            "files" => Mode::Files,
            "dir" => Mode::Dir,
            "none" => Mode::None,
            other => return Err(format!("{at}: unknown mode {other:?}")),
        };
        let args = dash_split(args, ' ');
        let outputs = dash_split(outputs, ',');
        let mut env_pairs = Vec::new();
        for pair in dash_split(env, ' ') {
            let Some((key, value)) = pair.split_once('=') else {
                return Err(format!("{at}: env entry {pair:?} is not KEY=VALUE"));
            };
            env_pairs.push((key.to_owned(), value.to_owned()));
        }
        let placeholders: Vec<&String> = args.iter().filter(|a| a.contains('{')).collect();
        match mode {
            Mode::None => {
                if !outputs.is_empty() || note.trim().is_empty() || *note == "-" {
                    return Err(format!(
                        "{at}: a `none` row has no outputs and must say why in its note"
                    ));
                }
            }
            _ if outputs.is_empty() => return Err(format!("{at}: no committed outputs")),
            Mode::Stdout if !placeholders.is_empty() => {
                return Err(format!("{at}: stdout rows take no placeholders"));
            }
            Mode::Files => {
                let expected: Vec<String> =
                    (0..outputs.len()).map(|i| format!("{{out{i}}}")).collect();
                let mut found: Vec<String> = placeholders.iter().map(|a| (*a).clone()).collect();
                found.sort();
                let mut sorted = expected.clone();
                sorted.sort();
                if found != sorted {
                    return Err(format!(
                        "{at}: files rows need exactly the placeholders {expected:?}"
                    ));
                }
            }
            Mode::Dir => {
                if placeholders.len() != 1 || placeholders[0] != "{dir}" {
                    return Err(format!(
                        "{at}: dir rows need exactly one {{dir}} placeholder"
                    ));
                }
                let names: BTreeSet<&str> = outputs.iter().map(|o| basename(o)).collect();
                if names.len() != outputs.len() {
                    return Err(format!("{at}: dir outputs need distinct basenames"));
                }
            }
            _ => {}
        }
        rows.push(Row {
            id: String::new(),
            script: (*script).to_owned(),
            mode,
            args,
            env: env_pairs,
            outputs,
            note: (*note).to_owned(),
        });
    }
    // Ids: the script name, qualified by args/env when a script has several rows.
    let mut per_script: BTreeMap<String, usize> = BTreeMap::new();
    for row in &rows {
        *per_script.entry(row.script.clone()).or_default() += 1;
    }
    let mut seen = BTreeSet::new();
    for row in &mut rows {
        // A generic `run.sh` is named after the tool directory holding it.
        let name = match basename(&row.script) {
            "run.sh" => basename(row.script.trim_end_matches("/run.sh")).to_owned(),
            script => script.trim_end_matches(".sh").to_owned(),
        };
        row.id = if per_script[&row.script] > 1 {
            let mut qualifier: Vec<String> = row
                .args
                .iter()
                .filter(|a| !a.contains('{'))
                .cloned()
                .collect();
            qualifier.extend(row.env.iter().map(|(k, v)| format!("{k}={v}")));
            if qualifier.is_empty() {
                name
            } else {
                format!("{name}[{}]", qualifier.join(" "))
            }
        } else {
            name
        };
        if !seen.insert(row.id.clone()) {
            return Err(format!("duplicate probes.tsv row id {}", row.id));
        }
    }
    Ok(rows)
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `*` / `?` glob match (the `--only` filter).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match (p.first(), t.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], t) || (!t.is_empty() && go(p, &t[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &t[1..]),
            (Some(a), Some(b)) if a == b => go(&p[1..], &t[1..]),
            _ => false,
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    go(&p, &t)
}

/// Keys whose holders must not run concurrently: the shared javac output dir
/// (`.cache/<name>`) and any working directory the script `cd`s into.
pub fn lane_keys(script_text: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for (offset, _) in script_text.match_indices(".cache/") {
        let rest = &script_text[offset + ".cache/".len()..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !name.is_empty() {
            keys.insert(format!("cache:{name}"));
        }
    }
    for line in script_text.lines() {
        let line = line.trim();
        if let Some(target) = line.strip_prefix("cd ") {
            keys.insert(format!("cwd:{}", target.to_ascii_lowercase()));
        }
    }
    keys
}

/// Group rows into lanes: rows sharing any lane key run sequentially.
pub fn lanes(keys: &[BTreeSet<String>]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..keys.len()).collect();
    fn find(parent: &mut [usize], i: usize) -> usize {
        let mut root = i;
        while parent[root] != root {
            root = parent[root];
        }
        parent[i] = root;
        root
    }
    let mut owner: BTreeMap<&String, usize> = BTreeMap::new();
    for (index, row_keys) in keys.iter().enumerate() {
        for key in row_keys {
            match owner.get(key) {
                Some(&other) => {
                    let (a, b) = (find(&mut parent, index), find(&mut parent, other));
                    parent[a] = b;
                }
                None => {
                    owner.insert(key, index);
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for index in 0..keys.len() {
        let root = find(&mut parent, index);
        groups.entry(root).or_default().push(index);
    }
    let mut lanes: Vec<Vec<usize>> = groups.into_values().collect();
    lanes.sort_by_key(|lane| std::cmp::Reverse(lane.len()));
    lanes
}

/// The first difference between two byte strings, by line.
pub fn describe_divergence(expected: &[u8], actual: &[u8]) -> String {
    fn clip(text: &str) -> String {
        const MAX: usize = 120;
        if text.chars().count() <= MAX {
            text.to_owned()
        } else {
            format!("{}…", text.chars().take(MAX).collect::<String>())
        }
    }
    let expected_text = String::from_utf8_lossy(expected);
    let actual_text = String::from_utf8_lossy(actual);
    let e: Vec<&str> = expected_text.split('\n').collect();
    let a: Vec<&str> = actual_text.split('\n').collect();
    let line = (0..e.len().max(a.len()))
        .find(|&i| e.get(i) != a.get(i))
        .unwrap_or(0);
    format!(
        "line {}: expected {:?} got {:?} (expected {} bytes, got {} bytes)",
        line + 1,
        clip(e.get(line).copied().unwrap_or("<end of file>")),
        clip(a.get(line).copied().unwrap_or("<end of file>")),
        expected.len(),
        actual.len()
    )
}

/// What the producer must be given to run.
#[derive(Debug, Clone)]
pub struct Context {
    /// Checkout root (scripts and outputs are relative to it).
    pub repo: PathBuf,
    /// Scratch root (`<repo>/work/regen`).
    pub work: PathBuf,
    pub runtime_jar: PathBuf,
    pub libraries: PathBuf,
    pub timeout: Duration,
}

/// One row's verdict.
#[derive(Debug, Clone)]
pub struct Report {
    pub id: String,
    pub status: Status,
    pub seconds: f64,
    pub detail: String,
}

fn slug(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn run_once(row: &Row, ctx: &Context, dir: &Path) -> Result<Vec<Vec<u8>>, String> {
    let io_err = |what: &str, e: io::Error| format!("{what}: {e}");
    let (cwd, out, tmp) = (dir.join("cwd"), dir.join("out"), dir.join("tmp"));
    for d in [&cwd, &out, &tmp] {
        fs::create_dir_all(d).map_err(|e| io_err("create scratch dir", e))?;
    }
    let args: Vec<String> = row
        .args
        .iter()
        .map(|arg| {
            if arg == "{dir}" {
                return out.display().to_string();
            }
            if let Some(index) = arg
                .strip_prefix("{out")
                .and_then(|r| r.strip_suffix('}'))
                .and_then(|n| n.parse::<usize>().ok())
            {
                return out
                    .join(basename(&row.outputs[index]))
                    .display()
                    .to_string();
            }
            // Repo-relative path arguments resolve against the checkout.
            if arg.contains('/') && ctx.repo.join(arg).exists() {
                return ctx.repo.join(arg).display().to_string();
            }
            arg.clone()
        })
        .collect();
    let stdout_path = dir.join("stdout");
    let stderr_path = dir.join("stderr.log");
    let stdout = fs::File::create(&stdout_path).map_err(|e| io_err("stdout file", e))?;
    let stderr = fs::File::create(&stderr_path).map_err(|e| io_err("stderr file", e))?;
    let java_tool_options = match std::env::var("JAVA_TOOL_OPTIONS") {
        Ok(existing) if !existing.is_empty() => {
            format!("{existing} -Djava.io.tmpdir={}", tmp.display())
        }
        _ => format!("-Djava.io.tmpdir={}", tmp.display()),
    };
    let mut command = Command::new("bash");
    command
        .arg(ctx.repo.join(&row.script))
        .args(&args)
        .current_dir(&cwd)
        .env("TMPDIR", &tmp)
        .env("JAVA_TOOL_OPTIONS", java_tool_options)
        .env("RIVET_PAPER_RUNTIME_JAR", &ctx.runtime_jar)
        .env("RIVET_PAPER_LIBRARIES", &ctx.libraries)
        .envs(row.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .process_group(0);
    let mut child = command.spawn().map_err(|e| io_err("spawn bash", e))?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| io_err("wait", e))? {
            break status;
        }
        if started.elapsed() > ctx.timeout {
            // Kill the whole process group: bash and the JVM it started.
            let _ = Command::new("kill")
                .args(["-KILL", &format!("-{}", child.id())])
                .status();
            let _ = child.wait();
            return Err(format!(
                "timed out after {}s (log {})",
                ctx.timeout.as_secs(),
                stderr_path.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    if !status.success() {
        let tail = fs::read_to_string(&stderr_path)
            .map(|log| {
                log.lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("")
                    .to_owned()
            })
            .unwrap_or_default();
        return Err(format!(
            "producer exited with {status} (log {}; last line: {tail:?})",
            stderr_path.display()
        ));
    }
    match row.mode {
        Mode::Stdout => Ok(vec![
            fs::read(&stdout_path).map_err(|e| io_err("read stdout", e))?,
        ]),
        Mode::Files | Mode::Dir => row
            .outputs
            .iter()
            .map(|output| {
                let path = out.join(basename(output));
                fs::read(&path)
                    .map_err(|e| format!("producer did not write {}: {e}", basename(output)))
            })
            .collect(),
        Mode::None => Ok(Vec::new()),
    }
}

/// Run one row twice and judge it.
pub fn check_row(row: &Row, ctx: &Context) -> Report {
    let started = Instant::now();
    let report = |status: Status, detail: String| Report {
        id: row.id.clone(),
        status,
        seconds: started.elapsed().as_secs_f64(),
        detail,
    };
    if row.mode == Mode::None {
        return report(
            Status::Unverified,
            format!("no committed output: {}", row.note),
        );
    }
    let mut committed = Vec::new();
    for output in &row.outputs {
        match fs::read(ctx.repo.join(output)) {
            Ok(bytes) => committed.push(bytes),
            Err(e) => {
                return report(
                    Status::Fail,
                    format!("committed output {output} unreadable: {e}"),
                );
            }
        }
    }
    let (expected, labels): (Vec<Vec<u8>>, Vec<String>) = if row.mode == Mode::Stdout {
        (
            vec![committed.concat()],
            vec![format!("stdout vs {}", row.outputs.join(" + "))],
        )
    } else {
        (committed, row.outputs.clone())
    };
    let base = ctx.work.join(slug(&row.id));
    let _ = fs::remove_dir_all(&base);
    let first = match run_once(row, ctx, &base.join("run1")) {
        Ok(produced) => produced,
        Err(e) => return report(Status::Fail, format!("run 1: {e}")),
    };
    let second = match run_once(row, ctx, &base.join("run2")) {
        Ok(produced) => produced,
        Err(e) => return report(Status::Fail, format!("run 2: {e}")),
    };
    let mut problems = Vec::new();
    for (index, label) in labels.iter().enumerate() {
        if first[index] != second[index] {
            problems.push(format!(
                "NONDETERMINISTIC {label}: run 1 vs run 2 {}",
                describe_divergence(&first[index], &second[index])
            ));
        }
        if first[index] != expected[index] {
            problems.push(format!(
                "DRIFT {label}: {}",
                describe_divergence(&expected[index], &first[index])
            ));
        }
    }
    if problems.is_empty() {
        report(
            Status::Pass,
            format!(
                "{} output(s) reproduced byte-identically twice",
                labels.len()
            ),
        )
    } else {
        report(Status::Fail, problems.join("; "))
    }
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("git {args:?}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Resolve the materialized Paper runtime once for every producer: explicit
/// env overrides, else this checkout's M0 run dir, else the main checkout's
/// (linked worktrees share it, like `scripts/gate.sh`).
fn resolve_runtime(repo: &Path) -> Result<(PathBuf, PathBuf), String> {
    if let (Ok(jar), Ok(libs)) = (
        std::env::var("RIVET_PAPER_RUNTIME_JAR"),
        std::env::var("RIVET_PAPER_LIBRARIES"),
    ) {
        let (jar, libs) = (PathBuf::from(jar), PathBuf::from(libs));
        if jar.is_file() && libs.is_dir() {
            return Ok((jar, libs));
        }
        return Err(format!(
            "RIVET_PAPER_RUNTIME_JAR={} / RIVET_PAPER_LIBRARIES={} do not exist",
            jar.display(),
            libs.display()
        ));
    }
    let mut roots = vec![repo.to_path_buf()];
    if let Ok(common) = git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) && let Some(main) = Path::new(common.trim()).parent()
    {
        roots.push(main.to_path_buf());
    }
    for root in roots {
        let run = root.join("tools/rivet-oracle/work/run");
        let jar = run.join("versions/26.2/paper-26.2.jar");
        let libs = run.join("libraries");
        if jar.is_file() && libs.is_dir() {
            return Ok((jar, libs));
        }
    }
    Err("no materialized Paper runtime (tools/rivet-oracle/work/run/{versions,libraries}); boot the M0 fixture server once or set RIVET_PAPER_RUNTIME_JAR and RIVET_PAPER_LIBRARIES".into())
}

/// Several probe scripts hard-code `tools/rivet-oracle/work/run/...`. Give a
/// checkout that has no runtime of its own the resolved one: `versions` as a
/// symlink (only the jar path is used), `libraries` as a real directory tree of
/// hard links (the scripts `find` it without following symlinks). A probe that
/// `cd`s into `work/run` then writes into this checkout only.
fn prepare_run_dir(repo: &Path, jar: &Path, libs: &Path) -> Result<(), String> {
    let run = repo.join("tools/rivet-oracle/work/run");
    fs::create_dir_all(&run).map_err(|e| format!("create {}: {e}", run.display()))?;
    let versions = jar
        .parent()
        .and_then(Path::parent)
        .ok_or("runtime jar is not under versions/26.2/")?;
    let link = run.join("versions");
    if fs::symlink_metadata(&link).is_err() {
        std::os::unix::fs::symlink(versions, &link)
            .map_err(|e| format!("link {} -> {}: {e}", link.display(), versions.display()))?;
    }
    let libraries = run.join("libraries");
    if fs::symlink_metadata(&libraries).is_err() {
        mirror_tree(libs, &libraries)
            .map_err(|e| format!("mirror {} -> {}: {e}", libs.display(), libraries.display()))?;
    }
    if !run.join("versions/26.2/paper-26.2.jar").is_file() || !libraries.is_dir() {
        return Err(format!(
            "{} does not expose the Paper runtime",
            run.display()
        ));
    }
    Ok(())
}

/// Recreate `src` under `dst` with hard links (copies across filesystems).
fn mirror_tree(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        if from.is_dir() {
            mirror_tree(&from, &to)?;
        } else if fs::hard_link(&from, &to).is_err() {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn java25() -> Result<(), String> {
    for (tool, marker) in [("java", "version \"25"), ("javac", "javac 25")] {
        let output = Command::new(tool)
            .arg("-version")
            .output()
            .map_err(|e| format!("{tool} not on PATH: {e}"))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !text.contains(marker) {
            return Err(format!("{tool} on PATH is not Java 25: {}", text.trim()));
        }
    }
    Ok(())
}

/// Rows whose producer, Java sources, arguments or outputs a diff touched
/// (the merge-tier path trigger).
pub fn triggered(rows: &[Row], repo: &Path, changed: &[String]) -> Vec<usize> {
    if changed
        .iter()
        .any(|path| path == "tools/rivet-oracle/probes.tsv")
    {
        return (0..rows.len()).collect();
    }
    let changed_java: BTreeSet<&str> = changed
        .iter()
        .filter(|p| p.ends_with(".java"))
        .map(|p| basename(p))
        .collect();
    (0..rows.len())
        .filter(|&index| {
            let row = &rows[index];
            let text = fs::read_to_string(repo.join(&row.script)).unwrap_or_default();
            changed.iter().any(|path| {
                *path == row.script
                    || row.outputs.contains(path)
                    || row.args.iter().any(|arg| {
                        arg.contains('/')
                            && path.starts_with(&format!("{}/", arg.trim_end_matches('/')))
                    })
            }) || changed_java.iter().any(|java| text.contains(java))
        })
        .collect()
}

/// Closure of the table against the tree: every probe script has a row, every
/// row's script and outputs exist, every top-level golden has a producer.
pub fn coverage(rows: &[Row], repo: &Path) -> Vec<Report> {
    let mut reports = Vec::new();
    let failing = |id: String, status: Status, detail: String| Report {
        id,
        status,
        seconds: 0.0,
        detail,
    };
    let scripts: BTreeSet<&str> = rows.iter().map(|r| r.script.as_str()).collect();
    if let Ok(listing) = fs::read_dir(repo.join("tools/rivet-oracle/scripts")) {
        let mut names: Vec<String> = listing
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("run_") && n.ends_with(".sh"))
            .collect();
        names.sort();
        for name in names {
            let path = format!("tools/rivet-oracle/scripts/{name}");
            if !scripts.contains(path.as_str()) {
                reports.push(failing(
                    name.trim_end_matches(".sh").to_owned(),
                    Status::Fail,
                    "probe script has no probes.tsv row".into(),
                ));
            }
        }
    }
    for row in rows {
        if !repo.join(&row.script).is_file() {
            reports.push(failing(
                row.id.clone(),
                Status::Fail,
                format!("script {} does not exist", row.script),
            ));
        }
        for output in &row.outputs {
            if !repo.join(output).is_file() {
                reports.push(failing(
                    row.id.clone(),
                    Status::Fail,
                    format!("committed output {output} does not exist"),
                ));
            }
        }
    }
    let claimed: BTreeSet<&str> = rows
        .iter()
        .flat_map(|r| r.outputs.iter().map(String::as_str))
        .collect();
    if let Ok(listing) = fs::read_dir(repo.join("tools/rivet-oracle/fixtures")) {
        let mut orphans: Vec<String> = listing
            .flatten()
            .filter(|e| e.path().is_file())
            .map(|e| {
                format!(
                    "tools/rivet-oracle/fixtures/{}",
                    e.file_name().to_string_lossy()
                )
            })
            .filter(|p| {
                [".jsonl", ".tsv", ".txt"]
                    .iter()
                    .any(|ext| p.ends_with(ext))
            })
            .filter(|p| !claimed.contains(p.as_str()))
            .collect();
        orphans.sort();
        for orphan in orphans {
            reports.push(failing(
                format!("orphan:{}", basename(&orphan)),
                Status::Unverified,
                format!("{orphan} has no producing probes.tsv row (not reproducible by regen)"),
            ));
        }
    }
    reports
}

/// Run `rows` with `jobs` lanes in parallel, printing each verdict as it lands.
pub fn run_rows(rows: &[Row], ctx: &Context, jobs: usize) -> Vec<Report> {
    let keys: Vec<BTreeSet<String>> = rows
        .iter()
        .map(|row| lane_keys(&fs::read_to_string(ctx.repo.join(&row.script)).unwrap_or_default()))
        .collect();
    let queue: Arc<Mutex<VecDeque<Vec<usize>>>> =
        Arc::new(Mutex::new(lanes(&keys).into_iter().collect()));
    let results: Arc<Mutex<Vec<Report>>> = Arc::new(Mutex::new(Vec::new()));
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1) {
            let queue = Arc::clone(&queue);
            let results = Arc::clone(&results);
            scope.spawn(move || {
                loop {
                    let Some(lane) = queue.lock().expect("queue").pop_front() else {
                        break;
                    };
                    for index in lane {
                        let report = check_row(&rows[index], ctx);
                        eprintln!(
                            "  {:<10} {:<60} {:>7.1}s  {}",
                            report.status.to_string(),
                            report.id,
                            report.seconds,
                            report.detail
                        );
                        results.lock().expect("results").push(report);
                    }
                }
            });
        }
    });
    let mut reports = Arc::try_unwrap(results)
        .expect("workers joined")
        .into_inner()
        .expect("results");
    let order: BTreeMap<&str, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| (r.id.as_str(), i))
        .collect();
    reports.sort_by_key(|r| order.get(r.id.as_str()).copied().unwrap_or(usize::MAX));
    reports
}

fn usage() -> i32 {
    eprintln!(
        "usage: rivet-oracle regen --check [--only <glob>] [--changed <git-range>] [--jobs N] [--timeout SECS]\n\
         Runs every probes.tsv producer twice into work/regen/ and byte-diffs against the\n\
         committed goldens. --only filters row ids (glob); --changed selects the rows a git\n\
         diff touches (the merge-tier path trigger). Exit 0 PASS, 1 FAIL, 3 UNVERIFIED."
    );
    2
}

/// `rivet-oracle regen ...`; returns the process exit code.
pub fn run_cli(args: &[String]) -> i32 {
    let mut check = false;
    let mut only: Option<String> = None;
    let mut changed: Option<String> = None;
    let mut jobs = 2usize;
    let mut timeout = 900u64;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--check" => check = true,
            "--only" => only = iter.next().cloned(),
            "--changed" => changed = iter.next().cloned(),
            "--jobs" => match iter.next().and_then(|n| n.parse().ok()) {
                Some(n) if n > 0 => jobs = n,
                _ => return usage(),
            },
            "--timeout" => match iter.next().and_then(|n| n.parse().ok()) {
                Some(n) if n > 0 => timeout = n,
                _ => return usage(),
            },
            _ => return usage(),
        }
    }
    if !check {
        return usage();
    }
    let repo = match PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
    {
        Ok(repo) => repo,
        Err(e) => {
            eprintln!("regen: cannot resolve the checkout root: {e}");
            return 1;
        }
    };
    let table = repo.join("tools/rivet-oracle/probes.tsv");
    let rows = match fs::read_to_string(&table)
        .map_err(|e| e.to_string())
        .and_then(|t| parse_table(&t))
    {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("regen: {}: {e}", table.display());
            return 1;
        }
    };
    let filtered = only.is_some() || changed.is_some();
    let mut selected: Vec<usize> = (0..rows.len()).collect();
    if let Some(range) = &changed {
        let diff = match git(&repo, &["diff", "--name-only", range]) {
            Ok(diff) => diff,
            Err(e) => {
                eprintln!("regen: {e}");
                return 1;
            }
        };
        let paths: Vec<String> = diff.lines().map(str::to_owned).collect();
        let hit: BTreeSet<usize> = triggered(&rows, &repo, &paths).into_iter().collect();
        selected.retain(|i| hit.contains(i));
        if selected.is_empty() {
            eprintln!("regen: no probe row is triggered by {range} (nothing to regenerate)");
            return 0;
        }
    }
    if let Some(pattern) = &only {
        selected.retain(|&i| glob_match(pattern, &rows[i].id));
        if selected.is_empty() {
            eprintln!("regen: --only {pattern:?} matches no probes.tsv row id");
            return 2;
        }
    }
    let chosen: Vec<Row> = selected.iter().map(|&i| rows[i].clone()).collect();

    let mut reports = if filtered {
        Vec::new()
    } else {
        coverage(&rows, &repo)
    };
    let prerequisites = resolve_runtime(&repo).and_then(|(jar, libs)| {
        java25()?;
        prepare_run_dir(&repo, &jar, &libs)?;
        Ok((jar, libs))
    });
    let work = repo.join("work/regen");
    eprintln!(
        "regen --check: {} row(s), {jobs} lane(s) in parallel, scratch {}",
        chosen.len(),
        work.display()
    );
    let started = Instant::now();
    match prerequisites {
        Ok((runtime_jar, libraries)) => {
            let _ = fs::remove_dir_all(&work);
            if let Err(e) = fs::create_dir_all(&work) {
                eprintln!("regen: cannot create {}: {e}", work.display());
                return 1;
            }
            let ctx = Context {
                repo: repo.clone(),
                work: work.clone(),
                runtime_jar,
                libraries,
                timeout: Duration::from_secs(timeout),
            };
            reports.extend(run_rows(&chosen, &ctx, jobs));
        }
        Err(e) => {
            eprintln!("regen: prerequisites missing: {e}");
            reports.extend(chosen.iter().map(|row| Report {
                id: row.id.clone(),
                status: Status::Unverified,
                seconds: 0.0,
                detail: format!("not run: {e}"),
            }));
        }
    }

    let mut tsv = String::from("id\tstatus\tseconds\tdetail\n");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for report in &reports {
        let _ = writeln!(
            tsv,
            "{}\t{}\t{:.1}\t{}",
            report.id,
            report.status,
            report.seconds,
            report.detail.replace(['\t', '\n'], " ")
        );
        println!(
            "{}\t{}\t{:.1}\t{}",
            report.status, report.id, report.seconds, report.detail
        );
        *counts.entry(report.status.to_string()).or_default() += 1;
    }
    if fs::create_dir_all(&work).is_ok() {
        let _ = fs::write(work.join("report.tsv"), tsv);
    }
    let status = Status::combine(reports.iter().map(|r| r.status));
    eprintln!(
        "regen --check: {status} — {} PASS, {} FAIL, {} UNVERIFIED in {:.0}s (report {})",
        counts.get("PASS").copied().unwrap_or(0),
        counts.get("FAIL").copied().unwrap_or(0),
        counts.get("UNVERIFIED").copied().unwrap_or(0),
        started.elapsed().as_secs_f64(),
        work.join("report.tsv").display()
    );
    status.exit_code().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &str) -> String {
        format!("# comment\n{HEADER}\n{rows}")
    }

    #[test]
    fn committed_table_parses_and_covers_every_probe_script() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let rows =
            parse_table(&fs::read_to_string(repo.join("tools/rivet-oracle/probes.tsv")).unwrap())
                .unwrap();
        let failures: Vec<String> = coverage(&rows, &repo)
            .into_iter()
            .filter(|r| r.status == Status::Fail)
            .map(|r| format!("{}: {}", r.id, r.detail))
            .collect();
        assert!(failures.is_empty(), "{failures:#?}");
        assert!(rows.len() >= 92, "{}", rows.len());
    }

    #[test]
    fn table_validation_rejects_malformed_rows() {
        for bad in [
            "s.sh\tstdout\t{out0}\t-\to\t-\n",
            "s.sh\tfiles\t{out0}\t-\ta,b\t-\n",
            "s.sh\tdir\t-\t-\to\t-\n",
            "s.sh\tnone\t-\t-\t-\t-\n",
            "s.sh\tstdout\t-\t-\t-\t-\n",
            "s.sh\tweird\t-\t-\to\t-\n",
            "s.sh\tstdout\t-\tNOEQUALS\to\t-\n",
            "s.sh\tdir\t{dir}\t-\ta/x,b/x\t-\n",
        ] {
            assert!(parse_table(&table(bad)).is_err(), "{bad:?}");
        }
        assert!(parse_table("wrong\n").is_err());
    }

    #[test]
    fn duplicate_scripts_get_qualified_ids() {
        let rows = parse_table(&table(
            "a/run_x.sh\tstdout\t-\t-\to1\t-\na/run_x.sh\tstdout\treload\t-\to2\t-\na/run_x.sh\tstdout\t-\tK=v\to3\t-\n",
        ))
        .unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["run_x", "run_x[reload]", "run_x[K=v]"]);
        let tool = parse_table(&table("tools/some-tool/run.sh\tstdout\t-\t-\to\t-\n")).unwrap();
        assert_eq!(tool[0].id, "some-tool");
    }

    #[test]
    fn glob_matches_star_and_question_mark() {
        assert!(glob_match("run_poi_*", "run_poi_chunk_probe"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("run_?eq_probe", "run_seq_probe"));
        assert!(!glob_match("run_poi_*", "run_seq_probe"));
        assert!(glob_match("*[reload]", "run_spawner_tick_probe[reload]"));
    }

    #[test]
    fn lanes_join_rows_sharing_a_cache_or_working_dir() {
        let keys = vec![
            lane_keys("classes=\"$root/.cache/a-java\"\n"),
            lane_keys("classes=\"$root/.cache/b-java\"\ncd \"$root/work/run\"\n"),
            lane_keys("classes=\"$root/.cache/c-java\"\n  cd \"$ROOT/work/run\"\n"),
            lane_keys("classes=\"$root/.cache/a-java\"\n"),
            lane_keys("classes=\"$root/.cache/d-java\"\n"),
        ];
        let mut lanes = lanes(&keys);
        for lane in &mut lanes {
            lane.sort();
        }
        lanes.sort();
        assert_eq!(lanes, vec![vec![0, 3], vec![1, 2], vec![4]]);
    }

    #[test]
    fn divergence_names_the_first_differing_line() {
        let d = describe_divergence(b"a\nb\nc\n", b"a\nB\nc\n");
        assert!(d.starts_with("line 2: expected \"b\" got \"B\""), "{d}");
        let d = describe_divergence(b"a\n", b"a\nextra\n");
        assert!(d.starts_with("line 2:"), "{d}");
    }

    fn fake_repo(scripts: &[(&str, &str)], files: &[(&str, &str)]) -> (tempfile::TempDir, Context) {
        let dir = tempfile::tempdir().unwrap();
        for (path, body) in scripts.iter().chain(files) {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, body).unwrap();
        }
        let ctx = Context {
            repo: dir.path().to_path_buf(),
            work: dir.path().join("work/regen"),
            runtime_jar: dir.path().join("jar"),
            libraries: dir.path().join("libs"),
            timeout: Duration::from_secs(20),
        };
        (dir, ctx)
    }

    fn one_row(line: &str) -> Row {
        parse_table(&table(line)).unwrap().remove(0)
    }

    #[test]
    fn reproducing_producers_pass_in_every_mode() {
        let (_dir, ctx) = fake_repo(
            &[
                ("s/out.sh", "printf 'a\\nb\\n'\n"),
                (
                    "s/files.sh",
                    "printf 'x\\n' > \"$1\"; printf 'y\\n' > \"$2\"\n",
                ),
                ("s/dir.sh", "printf 'm\\n' > \"$1/m.json\"\n"),
            ],
            &[
                ("f/a.txt", "a\n"),
                ("f/b.txt", "b\n"),
                ("f/x", "x\n"),
                ("f/y", "y\n"),
                ("f/d/m.json", "m\n"),
            ],
        );
        for line in [
            "s/out.sh\tstdout\t-\t-\tf/a.txt,f/b.txt\t-\n",
            "s/files.sh\tfiles\t{out0} {out1}\t-\tf/x,f/y\t-\n",
            "s/dir.sh\tdir\t{dir}\t-\tf/d/m.json\t-\n",
        ] {
            let report = check_row(&one_row(line), &ctx);
            assert_eq!(report.status, Status::Pass, "{line}: {}", report.detail);
        }
    }

    /// The regen comparator's negatives: a drifted golden, a nondeterministic
    /// producer, a failing producer and a hung producer are each FAIL and named.
    #[test]
    fn drift_nondeterminism_failure_and_timeout_are_detected() {
        let (_dir, mut ctx) = fake_repo(
            &[
                ("s/drift.sh", "printf 'a\\nB\\n'\n"),
                ("s/nondet.sh", "date +%N\n"),
                ("s/crash.sh", "echo boom >&2; exit 3\n"),
                ("s/hang.sh", "sleep 30\n"),
            ],
            &[("f/golden", "a\nb\n")],
        );
        let drift = check_row(&one_row("s/drift.sh\tstdout\t-\t-\tf/golden\t-\n"), &ctx);
        assert_eq!(drift.status, Status::Fail);
        assert!(
            drift.detail.contains("DRIFT") && drift.detail.contains("line 2"),
            "{}",
            drift.detail
        );
        let nondet = check_row(&one_row("s/nondet.sh\tstdout\t-\t-\tf/golden\t-\n"), &ctx);
        assert!(
            nondet.detail.contains("NONDETERMINISTIC"),
            "{}",
            nondet.detail
        );
        let crash = check_row(&one_row("s/crash.sh\tstdout\t-\t-\tf/golden\t-\n"), &ctx);
        assert!(crash.detail.contains("boom"), "{}", crash.detail);
        ctx.timeout = Duration::from_secs(1);
        let hang = check_row(&one_row("s/hang.sh\tstdout\t-\t-\tf/golden\t-\n"), &ctx);
        assert!(hang.detail.contains("timed out"), "{}", hang.detail);
    }

    #[test]
    fn producers_see_scratch_tmpdir_and_the_resolved_runtime() {
        let (_dir, ctx) = fake_repo(
            &[(
                "s/env.sh",
                "case \"$TMPDIR\" in */work/regen/*) ;; *) exit 9;; esac; printf '%s\\n' \"$(basename \"$RIVET_PAPER_RUNTIME_JAR\")\" \"$K\"\n",
            )],
            &[("f/env", "jar\nv\n")],
        );
        let report = check_row(&one_row("s/env.sh\tstdout\t-\tK=v\tf/env\t-\n"), &ctx);
        assert_eq!(report.status, Status::Pass, "{}", report.detail);
    }

    #[test]
    fn none_rows_are_unverified_never_skipped() {
        let (_dir, ctx) = fake_repo(&[], &[]);
        let report = check_row(
            &one_row("s/x.sh\tnone\t-\t-\t-\tno fixture consumes it\n"),
            &ctx,
        );
        assert_eq!(report.status, Status::Unverified);
        assert!(report.detail.contains("no fixture consumes it"));
    }

    #[test]
    fn path_trigger_selects_rows_by_script_output_java_source_and_arg_dir() {
        let (_dir, ctx) = fake_repo(
            &[
                ("s/a.sh", "javac src/java/AProbe.java\n"),
                ("s/b.sh", "true\n"),
                ("s/c.sh", "true\n"),
            ],
            &[("f/a", ""), ("f/b", ""), ("c/corpus/x.jsonl", "")],
        );
        let rows = parse_table(&table(
            "s/a.sh\tstdout\t-\t-\tf/a\t-\ns/b.sh\tstdout\t-\t-\tf/b\t-\ns/c.sh\tdir\tc/corpus {dir}\t-\tf/c\t-\n",
        ))
        .unwrap();
        let hit = |paths: &[&str]| {
            triggered(
                &rows,
                &ctx.repo,
                &paths.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>(),
            )
        };
        assert_eq!(hit(&["tools/rivet-oracle/src/java/AProbe.java"]), vec![0]);
        assert_eq!(hit(&["f/b"]), vec![1]);
        assert_eq!(hit(&["s/b.sh"]), vec![1]);
        assert_eq!(hit(&["c/corpus/x.jsonl"]), vec![2]);
        assert!(hit(&["crates/unrelated.rs"]).is_empty());
        assert_eq!(hit(&["tools/rivet-oracle/probes.tsv"]), vec![0, 1, 2]);
    }
}
