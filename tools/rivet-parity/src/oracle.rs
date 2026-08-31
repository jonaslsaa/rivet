//! Persistent client for the Paper Java reference oracle.
//!
//! Spawns `tools/rivet-reference-oracle/run.sh` (a JSON-Lines process) and
//! performs synchronous request/response calls: one stdin line per request,
//! one stdout line per response. The oracle requires the M0-materialized Paper
//! runtime libraries; point `RIVET_PAPER_JAR` / `RIVET_PAPER_LIBRARIES` /
//! `RIVET_PAPER_RUNTIME_JAR` at them (see the tool README).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

/// Why a reference-oracle operation did not produce a result.
///
/// A response with `ok: false` is a real Paper verdict for operations such as
/// malformed SNBT. Transport, protocol, and process failures are not verdicts
/// and must never be accepted as a declared rejection.
#[derive(Debug, Clone)]
pub enum OracleCallError {
    Rejected(String),
    Unavailable(String),
}

impl std::fmt::Display for OracleCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(message) => write!(f, "{message}"),
            Self::Unavailable(message) => write!(f, "{message}"),
        }
    }
}

/// A running reference-oracle process.
pub struct Oracle {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Oracle {
    /// Spawn `run.sh` and confirm the protocol with a `ping`.
    pub fn spawn() -> Result<Oracle, String> {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let run_sh = manifest_dir
            .join("../rivet-reference-oracle/run.sh")
            .canonicalize()
            .map_err(|e| format!("cannot locate run.sh: {e}"))?;

        let mut child = Command::new("bash")
            .arg(&run_sh)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("failed to spawn oracle run.sh: {e}"))?;

        let stdin = child.stdin.take().ok_or("oracle stdin not captured")?;
        let stdout = child.stdout.take().ok_or("oracle stdout not captured")?;
        let mut oracle = Oracle {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
        };

        let ping = oracle.request("ping", &[]).map_err(|e| {
            format!("oracle failed to boot (is the M0 Paper runtime materialized?): {e}")
        })?;
        if !ping.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            return Err(format!("oracle ping failed: {ping}"));
        }
        Ok(oracle)
    }

    /// Send one operation and return the full JSON response object.
    pub fn request(&mut self, op: &str, fields: &[(&str, &str)]) -> Result<Value, String> {
        self.next_id += 1;
        let mut obj = serde_json::Map::new();
        obj.insert("id".into(), json!(self.next_id));
        obj.insert("op".into(), json!(op));
        for (key, value) in fields {
            obj.insert((*key).to_string(), json!(value));
        }
        let line = serde_json::to_string(&Value::Object(obj))
            .map_err(|e| format!("serialize request: {e}"))?;
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("oracle write: {e}"))?;

        let mut response = String::new();
        let n = self
            .stdout
            .read_line(&mut response)
            .map_err(|e| format!("oracle read: {e}"))?;
        if n == 0 {
            return Err("oracle closed stdout (did it crash?)".to_string());
        }
        serde_json::from_str(&response)
            .map_err(|e| format!("oracle returned non-JSON line: {e}: {response:?}"))
    }

    /// Send an operation and return the `result` object of a successful response.
    pub fn call(&mut self, op: &str, fields: &[(&str, &str)]) -> Result<Value, OracleCallError> {
        let response = self
            .request(op, fields)
            .map_err(OracleCallError::Unavailable)?;
        match response.get("ok").and_then(Value::as_bool) {
            Some(true) => response.get("result").cloned().ok_or_else(|| {
                OracleCallError::Unavailable("oracle success without result".to_string())
            }),
            Some(false) => {
                let Some(error) = response.get("error").filter(Value::is_object) else {
                    return Err(OracleCallError::Unavailable(
                        "oracle rejection without structured error".to_string(),
                    ));
                };
                Err(OracleCallError::Rejected(format!(
                    "oracle {op} failed: {error}"
                )))
            }
            None => Err(OracleCallError::Unavailable(
                "oracle response missing boolean `ok`".to_string(),
            )),
        }
    }

    /// The `ping` provenance block (Paper spec/impl/commit/sha/java).
    pub fn provenance(&mut self) -> Result<Value, OracleCallError> {
        self.call("ping", &[])
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        // Closing stdin makes the oracle's readLine return null, but the JVM
        // (Paper class-init) has started non-daemon threads, so it does NOT exit
        // on EOF — `wait()` would block forever. Kill the child instead.
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The single oracle operation the differential checks drive. Split out (rather
/// than taking `Oracle` directly) so the accept/canonical decision logic in
/// `check_component_json` can be unit-tested against a stub oracle without
/// spawning the Paper JVM (issue #98).
pub trait OracleCall {
    fn call(&mut self, op: &str, fields: &[(&str, &str)]) -> Result<Value, OracleCallError>;
}

impl OracleCall for Oracle {
    fn call(&mut self, op: &str, fields: &[(&str, &str)]) -> Result<Value, OracleCallError> {
        Oracle::call(self, op, fields)
    }
}
