//! Persistent client for the Paper Java reference oracle.
//!
//! Spawns `tools/rivet-reference-oracle/run.sh` (a JSON-Lines process) once and
//! drives it through `rivet_harness_common::oracle_client`, the shared
//! boot-once client (id correlation, pipelining, tamper hook). This module adds
//! only the response envelope: `ok` + `result`, or `ok:false` + a structured
//! error classified as a rejection verdict or unavailable evidence. The oracle
//! requires the M0-materialized Paper runtime libraries; point
//! `RIVET_PAPER_JAR` / `RIVET_PAPER_LIBRARIES` / `RIVET_PAPER_RUNTIME_JAR` at
//! them (see the tool README).

use std::path::{Path, PathBuf};
use std::process::Command;

use rivet_harness_common::oracle_client::OracleClient;
use serde_json::{Value, json};

/// Why a reference-oracle operation did not produce a result.
///
/// Only an explicit operation-specific input-rejection discriminator is a
/// Paper rejection. Unexpected Java exceptions and malformed/legacy error
/// envelopes are unavailable evidence, never passing rejection verdicts.
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

/// `tools/rivet-reference-oracle/run.sh`, resolved from this crate.
pub fn run_sh() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rivet-reference-oracle/run.sh")
        .canonicalize()
        .map_err(|e| format!("cannot locate run.sh: {e}"))
}

/// A running reference-oracle process.
pub struct Oracle {
    client: OracleClient,
}

impl Oracle {
    /// Spawn `run.sh` and confirm the protocol with a `ping`.
    pub fn spawn() -> Result<Oracle, String> {
        let mut command = Command::new("bash");
        command.arg(run_sh()?);
        let client = OracleClient::spawn(command)
            .map_err(|e| format!("failed to spawn oracle run.sh: {e}"))?;
        let mut oracle = Oracle { client };

        let ping = oracle.request("ping", &[]).map_err(|e| {
            format!("oracle failed to boot (is the M0 Paper runtime materialized?): {e}")
        })?;
        if !ping.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            return Err(format!("oracle ping failed: {ping}"));
        }
        Ok(oracle)
    }

    /// The shared line-level client (pipelining, tamper hook, counts).
    pub fn client(&mut self) -> &mut OracleClient {
        &mut self.client
    }

    /// Send one operation and return the full JSON response object.
    pub fn request(&mut self, op: &str, fields: &[(&str, &str)]) -> Result<Value, String> {
        let mut obj = serde_json::Map::new();
        obj.insert("op".into(), json!(op));
        for (key, value) in fields {
            obj.insert((*key).to_string(), json!(value));
        }
        let body = serde_json::to_string(&Value::Object(obj))
            .map_err(|e| format!("serialize request: {e}"))?;
        let response = self.client.call(&body).map_err(|e| e.to_string())?;
        serde_json::from_str(&response)
            .map_err(|e| format!("oracle returned non-JSON line: {e}: {response:?}"))
    }

    /// Send an operation and return the `result` object of a successful response.
    pub fn call(&mut self, op: &str, fields: &[(&str, &str)]) -> Result<Value, OracleCallError> {
        let response = self
            .request(op, fields)
            .map_err(OracleCallError::Unavailable)?;
        classify_response(op, response)
    }

    /// The `ping` provenance block (Paper spec/impl/commit/sha/java).
    pub fn provenance(&mut self) -> Result<Value, OracleCallError> {
        self.call("ping", &[])
    }
}

fn classify_response(op: &str, response: Value) -> Result<Value, OracleCallError> {
    match response.get("ok").and_then(Value::as_bool) {
        Some(true) => response.get("result").cloned().ok_or_else(|| {
            OracleCallError::Unavailable("oracle success without result".to_string())
        }),
        Some(false) => {
            let Some(error) = response.get("error").filter(|value| value.is_object()) else {
                return Err(OracleCallError::Unavailable(
                    "oracle rejection without structured error".to_string(),
                ));
            };
            let message = format!("oracle {op} failed: {error}");
            if error.get("kind").and_then(Value::as_str) == Some("input_rejected")
                && error.get("operation").and_then(Value::as_str) == Some(op)
                && error
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|name| !name.is_empty())
                && error.get("message").and_then(Value::as_str).is_some()
            {
                Err(OracleCallError::Rejected(message))
            } else {
                Err(OracleCallError::Unavailable(message))
            }
        }
        None => Err(OracleCallError::Unavailable(
            "oracle response missing boolean `ok`".to_string(),
        )),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_requires_an_explicit_well_formed_rejection_for_the_requested_operation() {
        for error in [
            json!({}),
            json!({"type":"java.lang.NullPointerException", "message":"internal failure"}),
            json!({"kind":"unavailable", "operation":"snbt.parse", "type":"java.lang.NullPointerException", "message":"internal failure"}),
            json!({"kind":"input_rejected", "operation":"nbt.decode", "type":"java.io.EOFException", "message":"truncated"}),
            json!({"kind":"input_rejected", "operation":"snbt.parse"}),
        ] {
            assert!(matches!(
                classify_response("snbt.parse", json!({"ok":false,"error":error})),
                Err(OracleCallError::Unavailable(_))
            ));
        }
        let rejection = json!({"ok":false,"error":{"kind":"input_rejected","operation":"snbt.parse","type":"com.mojang.brigadier.exceptions.CommandSyntaxException","message":"Expected value"}});
        assert!(matches!(
            classify_response("snbt.parse", rejection),
            Err(OracleCallError::Rejected(_))
        ));
        let result = json!({"accept":false});
        assert_eq!(
            classify_response("component.json", json!({"ok":true,"result":result})).unwrap(),
            result
        );
    }
}
