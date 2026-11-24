//! Line-level client for a JSON-Lines oracle process (the Paper reference
//! oracle, `tools/rivet-reference-oracle/run.sh`).
//!
//! One process is booted once and then serves any number of requests. The
//! client owns the transport contract and nothing else:
//!
//! - every request is one compact JSON object on one stdin line; the client
//!   injects a sequential numeric `id` as the object's first key;
//! - every response is one stdout line that must start with that same
//!   `{"id":<n>,` prefix (the oracle echoes the id first), so responses are
//!   correlated without parsing JSON — the crate stays std-only;
//! - [`OracleClient::pipeline`] writes a whole batch from a writer thread while
//!   the caller's thread reads, so large batches never deadlock on full pipes;
//! - an optional [`Tamper`] rewrites exactly one response line before the
//!   caller sees it — the hook every comparator's negative control uses to
//!   prove it detects and names a wrong answer.
//!
//! Parsing and comparing response bodies is the caller's job.

use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// Why a request produced no response line.
#[derive(Debug)]
pub enum ClientError {
    /// Writing to or reading from the process failed.
    Io(io::Error),
    /// The process closed stdout before answering request `id`.
    Closed { id: u64 },
    /// A response did not carry the expected id prefix.
    Protocol { id: u64, line: String },
    /// The request body was not a JSON object text.
    BadRequest(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Io(e) => write!(f, "oracle I/O failed: {e}"),
            ClientError::Closed { id } => {
                write!(
                    f,
                    "oracle closed stdout before answering request {id} (did it crash?)"
                )
            }
            ClientError::Protocol { id, line } => {
                let shown: String = line.chars().take(200).collect();
                write!(
                    f,
                    "oracle response is not correlated to request {id}: {shown:?}"
                )
            }
            ClientError::BadRequest(body) => {
                write!(f, "request body is not a JSON object text: {body:?}")
            }
        }
    }
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(e: io::Error) -> Self {
        ClientError::Io(e)
    }
}

/// A negative-control hook: rewrite the response at `index` (0-based count of
/// responses handed to the caller since the tamper was armed).
pub struct Tamper {
    pub index: usize,
    pub mutate: Box<dyn Fn(&str) -> String + Send>,
}

/// A running JSON-Lines oracle process.
pub struct OracleClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    sent: usize,
    received: usize,
    tamper: Option<(Tamper, usize)>,
}

impl OracleClient {
    /// Spawn `command` with piped stdin/stdout (stderr inherited, so boot
    /// diagnostics stay visible).
    pub fn spawn(mut command: Command) -> io::Result<OracleClient> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("stdout"))?;
        Ok(OracleClient {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
            sent: 0,
            received: 0,
            tamper: None,
        })
    }

    /// Requests written so far.
    pub fn sent(&self) -> usize {
        self.sent
    }

    /// Correlated responses read so far.
    pub fn received(&self) -> usize {
        self.received
    }

    /// Arm a tamper: the `tamper.index`-th response returned after this call
    /// is rewritten by `tamper.mutate`. Replaces any armed tamper.
    pub fn arm_tamper(&mut self, tamper: Tamper) {
        self.tamper = Some((tamper, 0));
    }

    /// Send one request body (a compact JSON object text without `id`) and
    /// return its correlated response line.
    pub fn call(&mut self, body: &str) -> Result<String, ClientError> {
        let (id, line) = self.frame(body)?;
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.flush()?;
        self.sent += 1;
        let response = read_correlated(&mut self.stdout, id)?;
        self.received += 1;
        Ok(self.apply_tamper(response))
    }

    /// Send a batch of request bodies and return their responses in order.
    /// Requests are written from a scoped writer thread while this thread
    /// reads, so neither pipe can fill up and deadlock the exchange.
    pub fn pipeline(&mut self, bodies: &[String]) -> Result<Vec<String>, ClientError> {
        let mut framed = Vec::with_capacity(bodies.len());
        for body in bodies {
            framed.push(self.frame(body)?);
        }
        let stdin = &mut self.stdin;
        let stdout = &mut self.stdout;
        let child = &mut self.child;
        let (written, read) = std::thread::scope(|scope| {
            let writer = scope.spawn(|| -> io::Result<usize> {
                let mut count = 0;
                for (_, line) in &framed {
                    stdin.write_all(line.as_bytes())?;
                    count += 1;
                }
                stdin.flush()?;
                Ok(count)
            });
            let mut responses = Vec::with_capacity(framed.len());
            let mut failure = None;
            for (id, _) in &framed {
                match read_correlated(stdout, *id) {
                    Ok(line) => responses.push(line),
                    Err(e) => {
                        // Unblock the writer (a live process that stopped
                        // being read would otherwise fill its pipes forever);
                        // the client is unusable after a transport failure.
                        let _ = child.kill();
                        failure = Some(e);
                        break;
                    }
                }
            }
            let written = writer
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("writer panicked")));
            (written, failure.map_or(Ok(responses), Err))
        });
        // A read failure (crash, protocol) is the more specific diagnosis.
        let responses = read?;
        self.sent += written?;
        self.received += responses.len();
        Ok(responses
            .into_iter()
            .map(|line| self.apply_tamper(line))
            .collect())
    }

    fn frame(&mut self, body: &str) -> Result<(u64, String), ClientError> {
        let trimmed = body.trim();
        let Some(rest) = trimmed.strip_prefix('{') else {
            return Err(ClientError::BadRequest(body.to_owned()));
        };
        if !trimmed.ends_with('}') || trimmed.contains('\n') {
            return Err(ClientError::BadRequest(body.to_owned()));
        }
        self.next_id += 1;
        let id = self.next_id;
        let rest = rest.trim_start();
        let line = if rest == "}" {
            format!("{{\"id\":{id}}}\n")
        } else {
            format!("{{\"id\":{id},{rest}\n")
        };
        Ok((id, line))
    }

    fn apply_tamper(&mut self, line: String) -> String {
        let Some((tamper, seen)) = self.tamper.as_mut() else {
            return line;
        };
        let index = *seen;
        *seen += 1;
        if index == tamper.index {
            let mutated = (tamper.mutate)(&line);
            self.tamper = None;
            mutated
        } else {
            line
        }
    }
}

fn read_correlated(stdout: &mut impl BufRead, id: u64) -> Result<String, ClientError> {
    let mut line = String::new();
    if stdout.read_line(&mut line)? == 0 {
        return Err(ClientError::Closed { id });
    }
    let line = line.trim_end_matches(['\n', '\r']).to_owned();
    let prefix = format!("{{\"id\":{id},");
    if line.starts_with(&prefix) || line == format!("{{\"id\":{id}}}") {
        Ok(line)
    } else {
        Err(ClientError::Protocol { id, line })
    }
}

impl Drop for OracleClient {
    fn drop(&mut self) {
        // A Paper JVM keeps non-daemon threads after class init, so it does not
        // exit on stdin EOF; kill it instead of waiting forever.
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An echo "oracle": answers every request line with the line itself plus
    /// a marker, so correlation and ordering are observable.
    fn echo() -> OracleClient {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "while IFS= read -r line; do printf '%s\\n' \"$line\"; done",
        ]);
        OracleClient::spawn(command).expect("sh is available")
    }

    #[test]
    fn call_injects_and_correlates_ids() {
        let mut client = echo();
        let first = client.call(r#"{"op":"ping"}"#).unwrap();
        assert_eq!(first, r#"{"id":1,"op":"ping"}"#);
        let second = client.call("{}").unwrap();
        assert_eq!(second, r#"{"id":2}"#);
        assert_eq!((client.sent(), client.received()), (2, 2));
    }

    #[test]
    fn pipeline_returns_every_response_in_order_for_large_batches() {
        let mut client = echo();
        // ~4 MB each way: far beyond any pipe buffer, so a single-threaded
        // write-then-read would deadlock.
        let payload = "x".repeat(4096);
        let bodies: Vec<String> = (0..1000)
            .map(|i| format!(r#"{{"n":{i},"p":"{payload}"}}"#))
            .collect();
        let responses = client.pipeline(&bodies).unwrap();
        assert_eq!(responses.len(), 1000);
        for (i, response) in responses.iter().enumerate() {
            assert!(
                response.starts_with(&format!(r#"{{"id":{},"n":{i},"#, i + 1)),
                "{i}"
            );
        }
        assert_eq!((client.sent(), client.received()), (1000, 1000));
    }

    #[test]
    fn uncorrelated_response_is_a_protocol_error() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "read -r line; printf '%s\\n' '{\"id\":99,\"ok\":true}'",
        ]);
        let mut client = OracleClient::spawn(command).unwrap();
        match client.call(r#"{"op":"ping"}"#) {
            Err(ClientError::Protocol { id: 1, .. }) => {}
            other => panic!("expected protocol error, got {other:?}"),
        }
    }

    #[test]
    fn closed_stdout_is_reported_with_the_pending_id() {
        let mut client = OracleClient::spawn(Command::new("true")).unwrap();
        match client.call(r#"{"op":"ping"}"#) {
            Err(ClientError::Closed { id: 1 }) | Err(ClientError::Io(_)) => {}
            other => panic!("expected closed stdout, got {other:?}"),
        }
    }

    #[test]
    fn non_object_bodies_are_rejected_before_sending() {
        let mut client = echo();
        for body in ["[]", "\"x\"", "{\"a\":1}\n{\"b\":2}", "{"] {
            assert!(matches!(client.call(body), Err(ClientError::BadRequest(_))));
        }
        assert_eq!(client.sent(), 0);
    }

    #[test]
    fn tamper_rewrites_exactly_the_armed_response() {
        let mut client = echo();
        client.call(r#"{"warm":1}"#).unwrap();
        client.arm_tamper(Tamper {
            index: 1,
            mutate: Box::new(|line| line.replace("\"v\":2", "\"v\":20")),
        });
        let bodies: Vec<String> = (1..=3).map(|v| format!(r#"{{"v":{v}}}"#)).collect();
        let responses = client.pipeline(&bodies).unwrap();
        assert_eq!(
            responses,
            vec![
                r#"{"id":2,"v":1}"#.to_owned(),
                r#"{"id":3,"v":20}"#.to_owned(),
                r#"{"id":4,"v":3}"#.to_owned(),
            ]
        );
        // One-shot: later responses are untouched.
        assert_eq!(client.call(r#"{"v":2}"#).unwrap(), r#"{"id":5,"v":2}"#);
    }
}
