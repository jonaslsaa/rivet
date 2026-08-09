//! Deterministic seed regressions.
//!
//! cargo-fuzz (0.13.x) never reads `fuzz/seeds/` automatically — a plain
//! `cargo fuzz run <target>` only uses `fuzz/corpus/<target>/` (see
//! `fuzz/README.md`). These tests are the deterministic complement: every
//! committed seed in `fuzz/seeds/<target>/` is fed through the same target
//! body the fuzzer invokes (`rivet_fuzz::targets`), so a seed that stops
//! parsing, changes behavior, or trips a non-faithful panic fails
//! `cargo test -p rivet-fuzz`. Faithful panics (negative list length, missing
//! list type, oversized array, accounter quota/depth, compressed-map
//! out-of-bounds) are classified and tolerated — they are the intended outcome
//! for those seeds.
//!
//! A target body that silently `return`s when a seed stops parsing would hide a
//! seed regressing to a rejected form, so `intended_reachable_seeds_reach_their_core_work`
//! pins the seeds each target is documented to run its core assertion on.

use std::fs;
use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_io::{read_unnamed_tag, write_unnamed_tag, write_unnamed_tag_with_fallback};
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag_parser::TagParser;
use rivet_util::data_io::{DataInputStream, DataOutputStream, decode_modified_utf8};

use crate::{common, seeds, targets};

/// The committed seeds for `target` as `(file name, bytes)` pairs.
fn seed_files(target: &str) -> Vec<(String, Vec<u8>)> {
    seeds::seed_paths(target)
        .into_iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let data = fs::read(&path).unwrap_or_else(|e| panic!("read seed {name}: {e}"));
            (name, data)
        })
        .collect()
}

fn seed_bytes(target: &str, name: &str) -> Vec<u8> {
    let path: PathBuf = seeds::seed_dir(target).join(name);
    fs::read(&path).unwrap_or_else(|e| panic!("read seed {name} for {target}: {e}"))
}

/// Run `f`; a non-faithful panic fails the test (naming `what` so a failing
/// seed is identifiable), a faithful one is tolerated (it is the intended
/// outcome for a hostile seed).
fn run_classified(what: &str, f: impl FnOnce()) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(f)) {
        let message = common::message_of(payload.as_ref());
        assert!(
            common::is_faithful_message(&message),
            "{what} non-faithful panic: {message}"
        );
    }
}

/// Assert that running `f` panics with a faithful message containing `fragment`
/// — proves a seed actually reaches the guarded path it pins.
fn expect_faithful(fragment: &str, f: impl FnOnce()) {
    let err = catch_unwind(AssertUnwindSafe(f))
        .expect_err("expected a faithful panic but the call returned");
    let message = common::message_of(err.as_ref());
    assert!(
        common::is_faithful_message(&message),
        "unexpected panic: {message}"
    );
    assert!(
        message.contains(fragment),
        "panic {message:?} does not mention {fragment:?}"
    );
}

/// Every committed seed for every seeded target runs through the shared target
/// logic, one independently-guarded step at a time — exactly the code libFuzzer
/// drives.
#[test]
fn all_seeded_targets_run_every_committed_seed() {
    for &target in targets::SEEDED_TARGETS {
        let files = seed_files(target);
        assert!(!files.is_empty(), "target {target} has no committed seeds");
        let steps = targets::step_count(target);
        for (name, data) in &files {
            for step in 0..steps {
                run_classified(&format!("{target} seed {name} (step {step})"), || {
                    targets::run_step(target, data, step)
                });
            }
        }
    }
}

/// The seed names whose target body is *documented* to run its core assertion
/// on them. The bodies silently `return` when a seed stops parsing
/// (`let Ok(tag) = ... else { return; }`), so a seed that regresses to a
/// rejected form would no-op without failing `all_seeded_targets_run_every_committed_seed`
/// (which only classifies panics, not silent skips). This test pins the
/// intended-reach seeds: it is exactly the class of regression that produced
/// the truncated roundtrip seeds repaired in the seed-repair commit, where the
/// roundtrip write path went uncovered.
#[test]
fn intended_reachable_seeds_reach_their_core_work() {
    // nbt_binary_roundtrip: the write-path canonicalization assertion only runs
    // when `read_unnamed_tag` succeeds — these are the roundtrip-writeable set.
    for name in [
        "nbt_bad_utf8",
        "nbt_empty_root",
        "nbt_nan_double",
        "nbt_nan_float",
        "nbt_overlong_utf8",
        "nbt_raw_nul_utf8",
        "nbt_rich",
        "too_long_write",
    ] {
        let data = seed_bytes("nbt_binary_roundtrip", name);
        let mut dis = DataInputStream::new(Cursor::new(&data));
        let mut acc = NbtAccounter::default_quota();
        read_unnamed_tag(&mut dis, &mut acc).unwrap_or_else(|e| {
            panic!("roundtrip seed {name} must parse so the write path runs, got: {e}")
        });
    }

    // snbt_roundtrip: `parse(print(tag)) == tag` only runs on inputs the parser
    // accepts — these are the parseable set.
    for name in [
        "array_typed",
        "empty",
        "lists",
        "nested",
        "numbers",
        "strings_quoted",
        "unicode_escape",
    ] {
        let data = seed_bytes("snbt_roundtrip", name);
        let input = String::from_utf8_lossy(&data);
        let parser = TagParser::create(NbtOps::instance());
        parser.parse_fully(&input).unwrap_or_else(|e| {
            panic!("snbt_roundtrip seed {name} must parse so the round-trip runs, got: {e}")
        });
    }

    // data_io_modified_utf8: the body returns early unless the decoder accepts
    // the input — these are the decodable set. `too_long_write` decodes fine
    // but its canonical re-encode exceeds 65535 bytes, so the body returns on
    // the faithful write-overflow error before the idempotence assertion; it is
    // still pinned because the seed must decode to reach that overflow path.
    for name in [
        "ascii",
        "c080_nul",
        "empty",
        "nul",
        "overlong_c180",
        "surrogate_pair",
        "three_byte",
        "too_long_write",
        "two_byte",
    ] {
        let data = seed_bytes("data_io_modified_utf8", name);
        decode_modified_utf8(&data).unwrap_or_else(|e| {
            panic!("data_io seed {name} must decode so canonicalization runs, got: {e}")
        });
    }

    // codec_decode: on a parse failure the battery falls back to the empty tag,
    // so a seed that stops parsing would still "pass" while no longer reaching
    // its real content — pin the parseable set so the battery sees it.
    for name in [
        "array_typed",
        "empty",
        "lists",
        "nested",
        "numbers",
        "strings_quoted",
        "unicode_escape",
    ] {
        let data = seed_bytes("codec_decode", name);
        let input = String::from_utf8_lossy(&data);
        let parser = TagParser::create(NbtOps::instance());
        parser.parse_fully(&input).unwrap_or_else(|e| {
            panic!("codec_decode seed {name} must parse so the battery sees its content, got: {e}")
        });
    }

    // codec_compressed_decode: the codec battery only runs when the input
    // yields a first JSON value — every committed seed must reach it.
    for (name, data) in seed_files("codec_compressed_decode") {
        assert!(
            targets::compressed_decode_input(&data).is_some(),
            "codec_compressed_decode seed {name} must yield a JSON value"
        );
    }
}

/// The oversized byte-array seed declares a length of exactly `1 << 24`
/// (`0x01000000`) at the byte-array offset, so `check_array_length` fires
/// before the accounter or `read_fully` can allocate the declared 16 MiB
/// payload — the guarded panic is reached without an uncontrolled allocation.
#[test]
fn oversized_byte_array_seed_reaches_array_length_guard() {
    // The three committed copies (one per binary target) are byte-identical.
    let first = seed_bytes("nbt_binary", "nbt_oversized_byte_array");
    for target in ["nbt_binary_visitor", "nbt_binary_roundtrip"] {
        assert_eq!(
            first,
            seed_bytes(target, "nbt_oversized_byte_array"),
            "{target} oversized seed must be byte-identical to nbt_binary's"
        );
    }
    // Bytes 6..10 are the byte-array length: exactly 0x01000000 (1 << 24).
    assert_eq!(
        &first[6..10],
        &[0x01, 0x00, 0x00, 0x00],
        "oversized seed must declare length 0x01000000 at the byte-array offset"
    );
    // The seed carries no payload — reaching the panic proves the length guard
    // fired before any 16 MiB read/allocation. The seed is unnamed format
    // (root compound + name), so it must be read via `read_unnamed_tag` (which
    // skips the name) — `read_any_tag` would read it as an empty compound.
    expect_faithful("Array tag length must be < 1 << 24", || {
        let mut dis = DataInputStream::new(Cursor::new(&first));
        let mut acc = NbtAccounter::default_quota();
        let _ = read_unnamed_tag(&mut dis, &mut acc);
    });
}

/// A negative list count crashes Java's parse (`NbtFormatException`), which the
/// read path maps to a faithful panic.
#[test]
fn negative_list_length_seed_reaches_guard() {
    let data = seed_bytes("nbt_binary", "nbt_neg_list_len");
    expect_faithful("ListTag length cannot be negative", || {
        let mut dis = DataInputStream::new(Cursor::new(&data));
        let mut acc = NbtAccounter::default_quota();
        let _ = read_unnamed_tag(&mut dis, &mut acc);
    });
}

/// A list with element type 0 and a non-zero count crashes Java's parse
/// (`NbtFormatException`), mapped to a faithful panic.
#[test]
fn missing_list_type_seed_reaches_guard() {
    let data = seed_bytes("nbt_binary", "nbt_missing_list_type");
    expect_faithful("Missing type on ListTag", || {
        let mut dis = DataInputStream::new(Cursor::new(&data));
        let mut acc = NbtAccounter::default_quota();
        let _ = read_unnamed_tag(&mut dis, &mut acc);
    });
}

/// The committed deep-nesting seed nests > 512 empty compounds, so the
/// accounter's `MAX_STACK_DEPTH` guard fires before recursion can grow without
/// bound.
#[test]
fn deep_nesting_seed_reaches_depth_guard() {
    let data = seed_bytes("nbt_binary", "nbt_deep_nesting");
    expect_faithful("too high complexity", || {
        let mut dis = DataInputStream::new(Cursor::new(&data));
        let mut acc = NbtAccounter::default_quota();
        let _ = read_unnamed_tag(&mut dis, &mut acc);
    });
}

/// A packed list shorter than the record's key table reads a slot past the end:
/// Java's `IndexOutOfBoundsException` from `CompressedMapLike.get`, mapped to a
/// faithful panic on the compressed decode path.
#[test]
fn compressed_map_out_of_bounds_seed_reaches_guard() {
    let data = seed_bytes("codec_compressed_decode", "array_short");
    expect_faithful("out of bounds for compressed-map list", || {
        targets::codec_compressed_decode_step(&data, 0);
    });
}

/// The committed `too_long_write` seed holds a compound whose string's canonical
/// modified-UTF-8 re-encoding exceeds 65535 bytes. A plain write rejects it
/// (Java `UTFDataFormatException`), while NbtIo's `StringFallbackDataOutput` —
/// the roundtrip target's write path and the one `NbtIo.write` uses on disk —
/// writes the empty string and succeeds.
#[test]
fn too_long_write_routes_through_string_fallback() {
    let data = seed_bytes("nbt_binary_roundtrip", "too_long_write");
    let mut dis = DataInputStream::new(Cursor::new(&data));
    let mut acc = NbtAccounter::default_quota();
    let tag = read_unnamed_tag(&mut dis, &mut acc).expect("seed must parse");

    let mut plain = Vec::new();
    let err = write_unnamed_tag(&tag, &mut DataOutputStream::new(&mut plain))
        .expect_err("overlong string must fail a plain write");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);

    let mut fallback = Vec::new();
    write_unnamed_tag_with_fallback(&tag, &mut DataOutputStream::new(&mut fallback))
        .expect("fallback must absorb the overlong string");
    assert!(
        !fallback.is_empty(),
        "fallback write must still emit the tag"
    );
}
