use std::net::TcpListener;
use std::process::Command;

#[test]
fn refused_connection_is_structured_and_fails_immediately() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve a local port");
    let address = listener.local_addr().expect("read reserved address");
    drop(listener);

    let output = Command::new(env!("CARGO_BIN_EXE_rivet-client"))
        .args(["--address", &address.to_string(), "--timeout-seconds", "5"])
        .output()
        .expect("run rivet-client");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("failed to create connection"));
    assert!(!stderr.contains("panicked"));

    let records: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .expect("stdout is UTF-8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("stdout line is JSON"))
        .collect();
    assert!((2..=3).contains(&records.len()));
    assert!(records.iter().all(|record| record["protocol"] == 1));
    assert_eq!(records[0]["event"], "starting");
    if records.len() == 3 {
        assert_eq!(records[1]["event"], "init");
    }
    assert_eq!(
        records.last().expect("at least two records")["event"],
        "connection_failed"
    );
}

#[test]
fn invalid_address_has_a_structured_terminal_record() {
    let output = Command::new(env!("CARGO_BIN_EXE_rivet-client"))
        .args(["--address", "[not-an-address", "--timeout-seconds", "2"])
        .output()
        .expect("run rivet-client");

    assert_eq!(output.status.code(), Some(1));
    let records: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .expect("stdout is UTF-8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("stdout line is JSON"))
        .collect();
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| record["protocol"] == 1));
    assert_eq!(records[0]["event"], "starting");
    assert_eq!(records[1]["event"], "connection_failed");
}
