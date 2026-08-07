//! Integration tests for the `rivet-decode` harness (issue #97).
//!
//! These exercise the library surface (`protocol`, `corpus`, `frame`, `mutate`,
//! `frag`) end-to-end: canonical decode of the nine ported packets, the
//! vanilla-id dispatch, the mutation matrix, fragmentation/coalescing, and the
//! corpus provenance manifest round-trip.

use rivet_decode::corpus;
use rivet_decode::frag;
use rivet_decode::frame;
use rivet_decode::mutate;
use rivet_decode::protocol::{
    PlayPacket, decode_frame, encode_packet, packet_name, transcript_line,
};
use rivet_registry::core::{BlockPos, Direction};

/// A reference corpus: the nine ported packets with byte-exact wire payloads
/// (`[id varint][body]`), matching the `rivet-protocol` unit tests.
fn reference_payloads() -> Vec<(i32, &'static str, Vec<u8>)> {
    vec![
        (0, "accept_teleportation", vec![0x00, 0x00]),
        (
            11,
            "chunk_batch_received",
            vec![0x0b, 0x40, 0x60, 0x00, 0x00],
        ),
        (12, "client_command", vec![0x0c, 0x00]),
        (13, "client_tick_end", vec![0x0d]),
        (
            30,
            "move_player_pos",
            vec![
                0x1e, 0x3f, 0xf8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 1.5
                0xc0, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // -2.25
                0x40, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 3.5
                0x01, // on_ground, no h-collision
            ],
        ),
        (
            31,
            "move_player_pos_rot",
            vec![
                0x1f, 0x40, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 10.0
                0x40, 0x50, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 64.0
                0xc0, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // -8.0
                0x42, 0xb4, 0x00, 0x00, // 90.0 y_rot
                0x42, 0x33, 0x00, 0x00, // -45.0 x_rot
                0x01, // on_ground, no h-collision
            ],
        ),
        (
            32,
            "move_player_rot",
            vec![
                0x20, 0x43, 0x34, 0x00, 0x00, // 180.0 y_rot
                0x3f, 0x00, 0x00, 0x00, // 0.5 x_rot
                0x00, // no on_ground, no h-collision
            ],
        ),
        (33, "move_player_status_only", vec![0x21, 0x01]),
        (41, "player_action", {
            // action StartDestroyBlock (0), pos (0,0,0) packed, Down (0), seq 0.
            let pos = BlockPos::new(0, 0, 0);
            let mut v = vec![0x29, 0x00];
            v.extend_from_slice(&pos.as_long().to_be_bytes());
            v.push(0x00); // Down
            v.push(0x00); // seq
            v
        }),
    ]
}

fn reference_full_frames() -> Vec<(i32, String, Vec<u8>)> {
    reference_payloads()
        .into_iter()
        .map(|(id, name, payload)| {
            let full = frame::frame_full(&payload).unwrap();
            (id, name.to_string(), full)
        })
        .collect()
}

#[test]
fn nine_ported_packets_decode_with_real_codecs() {
    for (id, name, payload) in reference_payloads() {
        let decoded = decode_frame(&payload).unwrap();
        assert_eq!(decoded.id, id, "id for {name}");
        assert_eq!(decoded.name, format!("minecraft:{name}"));
        let expected_name: &'static str = match id {
            0 => "minecraft:accept_teleportation",
            11 => "minecraft:chunk_batch_received",
            12 => "minecraft:client_command",
            13 => "minecraft:client_tick_end",
            30 => "minecraft:move_player_pos",
            31 => "minecraft:move_player_pos_rot",
            32 => "minecraft:move_player_rot",
            33 => "minecraft:move_player_status_only",
            41 => "minecraft:player_action",
            _ => unreachable!("reference id {id}"),
        };
        assert_eq!(packet_name(id), Some(expected_name));
        // Every ported packet must decode to a real variant, not Raw.
        let is_raw = matches!(decoded.packet, PlayPacket::Raw { .. });
        assert!(!is_raw, "{name} decoded to Raw");
    }
}

#[test]
fn transcript_is_deterministic_and_normalized() {
    let payloads = reference_payloads();
    let mut first = Vec::new();
    for (seq, (id, name, payload)) in payloads.iter().enumerate() {
        let decoded = decode_frame(payload).unwrap();
        assert_eq!(decoded.id, *id);
        let line = transcript_line(seq, &decoded);
        // The frame hex must be the raw body bytes (id + payload), lowercase.
        let frame_hex = rivet_decode::protocol::hex(payload);
        assert!(line.contains(&format!("\"frame_hex\":\"{frame_hex}\"")));
        let _ = name;
        first.push(line);
    }
    // Re-decode produces the identical transcript (determinism).
    let mut second = Vec::new();
    for (seq, (_, _, payload)) in payloads.iter().enumerate() {
        let decoded = decode_frame(payload).unwrap();
        second.push(transcript_line(seq, &decoded));
    }
    assert_eq!(first, second);
}

#[test]
fn nan_and_inf_bits_round_trip_through_decode() {
    // chunk_batch_received with a quiet NaN float: raw bits must survive.
    let nan = f32::NAN;
    let mut payload = vec![0x0b];
    payload.extend_from_slice(&nan.to_bits().to_be_bytes());
    let decoded = decode_frame(&payload).unwrap();
    match decoded.packet {
        PlayPacket::ChunkBatchReceived(p) => {
            assert_eq!(p.desired_chunks_per_tick().to_bits(), nan.to_bits());
        }
        other => panic!("expected ChunkBatchReceived, got {other:?}"),
    }

    // move_player_pos with +Inf x: raw bits must survive.
    let inf = f64::INFINITY;
    let mut payload = vec![0x1e];
    payload.extend_from_slice(&inf.to_bits().to_be_bytes());
    payload.extend_from_slice(&0.0f64.to_bits().to_be_bytes());
    payload.extend_from_slice(&0.0f64.to_bits().to_be_bytes());
    payload.push(0x00);
    let decoded = decode_frame(&payload).unwrap();
    match decoded.packet {
        PlayPacket::MovePlayerPos(p) => {
            assert_eq!(p.get_x(0.0).to_bits(), inf.to_bits());
        }
        other => panic!("expected MovePlayerPos, got {other:?}"),
    }
}

#[test]
fn unknown_id_is_rejected_with_java_message() {
    // id 100 is outside the 0..69 vanilla table.
    let err = decode_frame(&[0x64]).unwrap_err();
    assert_eq!(err, "Received unknown packet id 100");
    // Negative id.
    let err = decode_frame(&[0xff, 0xff, 0xff, 0xff, 0x0f]).unwrap_err();
    assert_eq!(err, "Received unknown packet id -1");
}

#[test]
fn out_of_range_enum_panics_with_java_aioobe_message() {
    // client_command action 3 (length 3).
    let err = decode_frame(&[0x0c, 0x03]).unwrap_err();
    assert_eq!(err, "Index 3 out of bounds for length 3");
    // player_action action 8 (length 8).
    let err = decode_frame(&[0x29, 0x08]).unwrap_err();
    assert_eq!(err, "Index 8 out of bounds for length 8");
}

#[test]
fn trailing_body_bytes_are_rejected_with_java_message() {
    // client_tick_end has a unit body; one trailing byte is an error. The text
    // mirrors Java's `PacketDecoder.decode` (the `({class})` slot reuses the
    // packet name, since the harness does not track Java class names).
    let err = decode_frame(&[0x0d, 0x00]).unwrap_err();
    assert_eq!(
        err,
        "Packet play/minecraft:client_tick_end (minecraft:client_tick_end) was larger \
         than I expected, found 1 bytes extra whilst reading packet minecraft:client_tick_end"
    );
}

#[test]
fn corpus_manifest_round_trips_and_hashes() {
    let dir = std::env::temp_dir().join(format!("rivet-decode-corpus-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let frames = reference_full_frames();
    let stored = corpus::write_corpus(&dir, &frames).unwrap();
    assert_eq!(stored.len(), 9);
    // Verify every stored sha256 matches the recomputed value.
    for entry in &stored {
        let recomputed = corpus::sha256_hex(&entry.full_frame);
        assert_eq!(entry.sha256, recomputed);
    }
    // Re-read validates manifest + files.
    let re = corpus::read_corpus(&dir).unwrap();
    assert_eq!(re.len(), stored.len());
    assert_eq!(re[0].name, "accept_teleportation");
    assert_eq!(re[0].id, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mutation_matrix_matches_expected_outcomes() {
    let dir = std::env::temp_dir().join(format!("rivet-decode-mutate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let frames = reference_full_frames();
    let stored = corpus::write_corpus(&dir, &frames).unwrap();
    let payloads: Vec<(i32, Vec<u8>)> = stored
        .iter()
        .map(|e| (e.id, corpus::payload_of(e).unwrap()))
        .collect();
    let reports = mutate::run(&payloads);
    assert_eq!(reports.len(), 6, "six matrix rows");
    for report in &reports {
        assert!(report.applied, "{} not applied", report.kind);
        match report.kind {
            "nan_inf" => assert_eq!(report.outcome, "accepted"),
            _ => assert_eq!(
                report.outcome, "rejected",
                "{} was not rejected",
                report.kind
            ),
        }
    }
    assert!(mutate::all_ok(&reports));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mutation_matrix_reports_skipped_when_no_target_frame() {
    // A corpus without client_command (id 12) leaves the enum_out_of_range rows
    // unapplied and reports them as skipped, never as a silent pass.
    let payloads: Vec<(i32, Vec<u8>)> = reference_payloads()
        .into_iter()
        .filter(|(id, _, _)| *id != 12)
        .map(|(id, _, payload)| (id, payload))
        .collect();
    let reports = mutate::run(&payloads);
    let client_cmd = reports
        .iter()
        .find(|r| r.kind == "enum_out_of_range" && r.target_id == 12)
        .expect("client_command enum row present");
    assert!(!client_cmd.applied, "client_command row must be skipped");
    assert_eq!(client_cmd.outcome, "skipped");
    assert!(mutate::all_ok(&reports), "skipped rows never fail the run");
}

#[test]
fn encode_packet_surfaces_panics_as_err() {
    // Passing a PlayPacket variant mismatched to the entry id hits the table
    // map's `unreachable!()`; encode_packet must surface that panic as an Err
    // (matching decode_frame), never let it escape the crate surface.
    let err = encode_packet(
        13, // client_tick_end
        &PlayPacket::ClientCommand(rivet_protocol::game::ServerboundClientCommandPacket::new(
            rivet_protocol::game::serversbound_client_command_packet::Action::PerformRespawn,
        )),
    )
    .unwrap_err();
    assert!(err.contains("unreachable"), "got {err}");
}

#[test]
fn truncated_raw_slot_is_rejected_by_framing() {
    // A truncated unported packet (keep_alive body missing bytes) still passes
    // through the raw slot as a Raw capture — the raw passthrough is honest
    // about not interpreting unported bodies. Its frame-level truncation is
    // instead caught by the varint21 frame decoder (mid-frame leftover).
    let full = frame::frame_full(&[0x1c, 0x2a]).unwrap(); // id 28, 1 body byte
    let mut stream = full.clone();
    stream.truncate(full.len() - 1); // cut the last body byte
    let split = frame::split_stream(&stream).unwrap();
    assert!(split.frames.is_empty(), "no complete frame");
    assert!(
        !split.leftover.is_empty(),
        "the truncated frame is held back"
    );
}

#[test]
fn committed_corpus_manifest_packet_count_is_validated() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/corpus");
    // The committed fixture must report 9 files.
    let entries = corpus::read_corpus(&dir).unwrap();
    assert_eq!(entries.len(), 9);
    // A manifest whose packet_count disagrees with its files must be rejected.
    let manifest_path = dir.join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path).unwrap();
    let bad = text.replace("\"packet_count\": 9", "\"packet_count\": 8");
    let tmp = std::env::temp_dir().join(format!("rivet-decode-count-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("manifest.json"), bad).unwrap();
    for name in [
        "0000_accept_teleportation.frame",
        "0001_chunk_batch_received.frame",
    ] {
        std::fs::copy(dir.join(name), tmp.join(name)).unwrap();
    }
    let err = corpus::read_corpus(&tmp).unwrap_err();
    assert!(
        err.contains("packet_count"),
        "packet_count mismatch must be reported, got: {err}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn fragmentation_and_coalescing_are_equivalent() {
    // Build a capture stream: two frames concatenated.
    let mut stream = Vec::new();
    for (_, _, payload) in reference_payloads() {
        stream.extend_from_slice(&frame::frame_full(&payload).unwrap());
    }
    let report = frag::run(&stream).unwrap();
    // 9 reference frames.
    assert_eq!(report.reference.len(), 9);
    assert!(frag::all_ok(&report), "all splits must match reference");
    // The coalesced split frame count equals the reference.
    let coalesced = report
        .splits
        .iter()
        .find(|(name, _, _)| *name == "coalesced")
        .unwrap();
    assert_eq!(coalesced.1.len(), 9);
}

#[test]
fn encode_reproduces_byte_exact_frames() {
    // Re-encode a decoded packet and confirm byte identity with the original
    // payload (the `verify` path's re-encode guarantee).
    for (id, _, payload) in reference_payloads() {
        let decoded = decode_frame(&payload).unwrap();
        let reencoded = encode_packet(id, &decoded.packet).unwrap();
        assert_eq!(
            reencoded, payload,
            "id {id} did not round-trip byte-exactly"
        );
    }
}

#[test]
fn block_pos_and_direction_normalize() {
    // player_action with a known position + direction.
    let pos = BlockPos::new(1, 2, 3);
    let mut payload = vec![0x29, 0x00];
    payload.extend_from_slice(&pos.as_long().to_be_bytes());
    payload.push(Direction::East.get_3d_data_value() as u8); // 5
    payload.push(42); // seq
    let decoded = decode_frame(&payload).unwrap();
    match decoded.packet {
        PlayPacket::PlayerAction(p) => {
            assert_eq!(p.get_pos(), BlockPos::new(1, 2, 3));
            assert_eq!(p.get_direction(), Direction::East);
            assert_eq!(p.get_sequence(), 42);
        }
        other => panic!("expected PlayerAction, got {other:?}"),
    }
    let line = transcript_line(0, &decoded);
    assert!(line.contains("\"position\":[1,2,3]"));
    assert!(line.contains("\"direction\":\"East\""));
}

#[test]
fn raw_slots_capture_unported_packet_bodies() {
    // A real join includes many serverbound packets we have not ported. Their
    // bodies must be captured, not interpreted. id 28 = keep_alive (raw).
    let mut keep_alive = vec![0x1c]; // 28
    keep_alive.extend_from_slice(&42i64.to_be_bytes());
    let decoded = decode_frame(&keep_alive).unwrap();
    assert_eq!(decoded.id, 28);
    assert_eq!(decoded.name, "minecraft:keep_alive");
    match decoded.packet {
        PlayPacket::Raw { id, body } => {
            assert_eq!(id, 28);
            // The body is the uninterpreted 8-byte keep-alive long.
            assert_eq!(body, 42i64.to_be_bytes().to_vec());
        }
        other => panic!("expected Raw, got {other:?}"),
    }
}

#[test]
fn frame_payload_round_trip_through_varint21() {
    // The frame codec must round-trip `[id varint][body]` through the varint21
    // length header (the `decode` subcommand path).
    let payload = vec![0x0b, 0x40, 0x60, 0x00, 0x00];
    let full = frame::frame_full(&payload).unwrap();
    let payload_back = frame::frame_payload(&full).unwrap();
    assert_eq!(payload_back, payload);
    let decoded = decode_frame(&payload_back).unwrap();
    assert_eq!(decoded.id, 11);
}

/// The committed canonical capture corpus fixture is byte-stable: every frame
/// decodes with a real codec and its sha256 matches the manifest. This pins the
/// golden corpus against drift (a codec change would fail the decode or the
/// manifest hash).
#[test]
fn committed_corpus_fixture_is_byte_stable() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/corpus");
    let entries = corpus::read_corpus(&dir)
        .unwrap_or_else(|e| panic!("fixtures/corpus failed verification: {e}"));
    assert_eq!(entries.len(), 9, "canonical corpus has 9 packets");
    for entry in &entries {
        let payload = corpus::payload_of(entry).unwrap();
        // Every canonical frame must decode with a real codec (no Raw).
        let decoded = decode_frame(&payload)
            .unwrap_or_else(|e| panic!("id {} failed to decode: {e}", entry.id));
        assert!(
            !matches!(decoded.packet, PlayPacket::Raw { .. }),
            "id {} decoded to Raw",
            entry.id
        );
        // Re-encode reproduces the canonical bytes (byte-exact round-trip).
        let reencoded = encode_packet(entry.id, &decoded.packet).unwrap();
        assert_eq!(
            reencoded, payload,
            "id {} re-encode diverged from canonical corpus",
            entry.id
        );
    }
}

#[test]
fn frag_reports_mid_frame_stream_as_err() {
    // A stream that ends mid-frame (a frame length header with no payload)
    // must be surfaced as an error, not silently truncated.
    let mut stream = frame::frame_full(&[0x0b, 0x40, 0x60, 0x00, 0x00]).unwrap();
    // Append a length header claiming 5 more bytes that never arrive.
    stream.push(5);
    let err = frag::run(&stream).unwrap_err();
    assert!(err.contains("mid-frame"), "got {err}");
}

#[test]
fn binary_exit_codes_match_the_gate_contract() {
    // The `rivet-oracle` gate contract: 0 = VERIFIED, 1 = FAILED, 3 =
    // UNVERIFIED (a required input is missing or unreadable).
    let bin = env!("CARGO_BIN_EXE_rivet-decode");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/corpus");
    let fixture = fixture.to_str().unwrap();

    // verify/mutate against the committed fixture succeed (exit 0).
    for sub in ["verify", "mutate"] {
        let out = std::process::Command::new(bin)
            .arg(sub)
            .arg(fixture)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{sub} on the fixture must exit 0:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // A missing corpus is UNVERIFIED (exit 3), not a silent pass.
    let missing = std::process::Command::new(bin)
        .args(["verify", "/nonexistent/rivet-decode-corpus"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(3));

    // A capture that ends mid-frame is FAILED (exit 1).
    let tmp = std::env::temp_dir().join(format!("rivet-decode-bin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let mid = tmp.join("mid.bin");
    std::fs::write(&mid, [0x05]).unwrap(); // length header claims 5 bytes; none follow
    let mid = mid.to_str().unwrap();
    let out = std::process::Command::new(bin)
        .args(["decode", mid])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&tmp);
}
