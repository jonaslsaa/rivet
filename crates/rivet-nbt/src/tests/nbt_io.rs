//! Binary NBT IO tests for `NbtIo` (`net.minecraft.nbt.NbtIo`).
//!
//! Covers the unit notes: in-memory write/read round-trips, gzip
//! compression, modified-UTF-8 (emoji surrogate pairs), non-compound root
//! error, and the streaming `parse` visitor sequence.

use crate::byte_array_tag::ByteArrayTag;
use crate::byte_tag::ByteTag;
use crate::compound_tag::CompoundTag;
use crate::double_tag::DoubleTag;
use crate::float_tag::FloatTag;
use crate::int_array_tag::IntArrayTag;
use crate::int_tag::IntTag;
use crate::list_tag::ListTag;
use crate::long_array_tag::LongArrayTag;
use crate::long_tag::LongTag;
use crate::nbt_accounter::NbtAccounter;
use crate::nbt_io;
use crate::short_tag::ShortTag;
use crate::string_tag::StringTag;
use crate::tag::Tag;
use crate::visitors::collect_to_tag::CollectToTag;
use crate::visitors::skip_all::SkipAll;

use rivet_util::data_io::{DataInput, DataOutput};
use rivet_util::{DataInputStream, DataOutputStream};

/// Write a `CompoundTag` root with `NbtIo.write` into an in-memory buffer.
fn write_to_bytes(tag: &CompoundTag) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::new();
    let mut out = DataOutputStream::new(&mut buf);
    nbt_io::write(tag, &mut out).expect("write failed");
    buf
}

/// Read a `CompoundTag` root back with `NbtIo.read`.
fn read_from_bytes(bytes: &[u8]) -> CompoundTag {
    let mut input = DataInputStream::new(std::io::Cursor::new(bytes));
    nbt_io::read(&mut input, &mut NbtAccounter::unlimited_heap()).expect("read failed")
}

/// A rich compound exercising every tag kind the binary format supports.
fn rich_compound() -> CompoundTag {
    let mut c = CompoundTag::new();
    c.put("byte".to_string(), Tag::Byte(ByteTag::new(-5)));
    c.put("short".to_string(), Tag::Short(ShortTag::new(3200)));
    c.put("int".to_string(), Tag::Int(IntTag::new(123_456)));
    c.put("long".to_string(), Tag::Long(LongTag::new(9_000_000_000)));
    c.put("float".to_string(), Tag::Float(FloatTag::new(1.5)));
    c.put("double".to_string(), Tag::Double(DoubleTag::new(-0.25)));
    c.put_string("str", "héllo wörld");

    c.put_byte_array("bytearr", vec![1, -2, 3, -4]);
    c.put_int_array("intarr", vec![1, 2, 3]);
    c.put_long_array("longarr", vec![4_000_000_000, -4_000_000_000]);

    let mut nested = CompoundTag::new();
    nested.put_boolean("flag", true);
    nested.put_string("name", "nested");
    c.put("nested".to_string(), Tag::Compound(nested));

    let mut int_list = ListTag::new();
    int_list.add(Tag::Int(IntTag::new(10)));
    int_list.add(Tag::Int(IntTag::new(20)));
    c.put("ints".to_string(), Tag::List(int_list));

    let mut comp_list = ListTag::new();
    let mut e1 = CompoundTag::new();
    e1.put_int("x", 1);
    let mut e2 = CompoundTag::new();
    e2.put_int("x", 2);
    comp_list.add(Tag::Compound(e1));
    comp_list.add(Tag::Compound(e2));
    c.put("comps".to_string(), Tag::List(comp_list));

    c
}

#[test]
fn simple_compound_binary_layout_matches_java() {
    // `{"foo": 5}` — Java `DataOutputStream` layout:
    //   writeByte(10)          compound tag id
    //   writeUTF("")           root name, length 0 -> 0x00 0x00
    //   writeByte(3)           int tag id
    //   writeUTF("foo")        -> 0x00 0x03 'f' 'o' 'o'
    //   writeInt(5)            -> 0x00 0x00 0x00 0x05
    //   writeByte(0)           compound end
    let mut c = CompoundTag::new();
    c.put_int("foo", 5);

    assert_eq!(
        write_to_bytes(&c),
        vec![
            0x0A, 0x00, 0x00, // compound + empty root name
            0x03, 0x00, 0x03, 0x66, 0x6F, 0x6F, // int + "foo"
            0x00, 0x00, 0x00, 0x05, // 5
            0x00, // end
        ]
    );
}

#[test]
fn in_memory_round_trip_preserves_all_tag_kinds() {
    let original = rich_compound();
    let bytes = write_to_bytes(&original);
    let decoded = read_from_bytes(&bytes);
    assert_eq!(decoded, original);
}

#[test]
fn gzip_compressed_round_trip() {
    let original = rich_compound();

    let mut compressed: Vec<u8> = Vec::new();
    nbt_io::write_compressed(&original, &mut compressed).expect("write_compressed failed");

    // gzip magic header (0x1f 0x8b) proves the stream was actually deflated.
    assert_eq!(&compressed[0..2], &[0x1f, 0x8b]);

    let decoded = nbt_io::read_compressed(&compressed[..], &mut NbtAccounter::unlimited_heap())
        .expect("read_compressed failed");
    assert_eq!(decoded, original);
}

#[test]
fn modified_utf8_emoji_surrogate_round_trip() {
    // U+1F600 "grinning face" is a surrogate pair in UTF-16; Java writeUTF
    // encodes it as two CESU-8 3-byte sequences (6 bytes). The 2-byte length
    // prefix must therefore read 6.
    let emoji = "\u{1F600}";

    let mut c = CompoundTag::new();
    c.put_string("emoji", emoji);
    let mut long_str = String::new();
    long_str.push_str("mixed ");
    long_str.push_str(emoji);
    long_str.push_str(" text \u{1F680}");
    c.put_string("mixed", &long_str);

    // Byte-level check that Java modified UTF-8 encodes the pair as 6 bytes.
    // writeUnnamedTag emits: id byte (8 = string) + writeUTF("") name prefix
    // (0x00 0x00) + writeUTF(value): 0x00 0x06 length + 6 CESU-8 bytes
    // (ed a0 bd ed b8 80).
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut out = DataOutputStream::new(&mut buf);
        nbt_io::write_unnamed_tag(
            &Tag::String(StringTag::value_of(emoji.to_string())),
            &mut out,
        )
        .expect("write_unnamed_tag");
    }
    assert_eq!(&buf[0..3], &[0x08, 0x00, 0x00]);
    assert_eq!(&buf[3..5], &[0x00, 0x06]);
    assert_eq!(&buf[5..], &[0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80]);

    let bytes = write_to_bytes(&c);
    let decoded = read_from_bytes(&bytes);
    assert_eq!(decoded.get_string("emoji").map(String::as_str), Some(emoji));
    assert_eq!(
        decoded.get_string("mixed").map(String::as_str),
        Some(long_str.as_str())
    );
}

#[test]
fn non_compound_root_is_an_error() {
    // A bare int as an unnamed root tag is not a compound: read must fail with
    // "Root tag must be a named compound tag".
    let mut buf: Vec<u8> = Vec::new();
    let mut out = DataOutputStream::new(&mut buf);
    nbt_io::write_unnamed_tag(&Tag::Int(IntTag::new(7)), &mut out).expect("write_unnamed_tag");

    let mut input = DataInputStream::new(std::io::Cursor::new(buf));
    let err = nbt_io::read(&mut input, &mut NbtAccounter::unlimited_heap())
        .expect_err("read of a non-compound root must fail");
    assert!(
        err.to_string()
            .contains("Root tag must be a named compound tag")
    );
}

#[test]
fn any_tag_and_unnamed_tag_round_trip() {
    let tags = vec![
        Tag::End(crate::end_tag::EndTag),
        Tag::Byte(ByteTag::new(1)),
        Tag::Short(ShortTag::new(2)),
        Tag::Int(IntTag::new(3)),
        Tag::Long(LongTag::new(4)),
        Tag::Float(FloatTag::new(5.0)),
        Tag::Double(DoubleTag::new(6.0)),
        Tag::String(StringTag::value_of("s".to_string())),
        Tag::ByteArray(ByteArrayTag::new(vec![1, 2])),
        Tag::IntArray(IntArrayTag::new(vec![1, 2])),
        Tag::LongArray(LongArrayTag::new(vec![1, 2])),
    ];

    // writeAnyTag/readAnyTag.
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut out = DataOutputStream::new(&mut buf);
        for t in &tags {
            nbt_io::write_any_tag(t, &mut out).expect("write_any_tag");
        }
    }
    {
        let mut input = DataInputStream::new(std::io::Cursor::new(&buf));
        let mut acc = NbtAccounter::unlimited_heap();
        for t in &tags {
            assert_eq!(
                &nbt_io::read_any_tag(&mut input, &mut acc).expect("read_any_tag"),
                t
            );
        }
    }

    // writeUnnamedTag/readUnnamedTag.
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut out = DataOutputStream::new(&mut buf);
        for t in &tags {
            nbt_io::write_unnamed_tag(t, &mut out).expect("write_unnamed_tag");
        }
    }
    {
        let mut input = DataInputStream::new(std::io::Cursor::new(&buf));
        let mut acc = NbtAccounter::unlimited_heap();
        for t in &tags {
            assert_eq!(
                &nbt_io::read_unnamed_tag(&mut input, &mut acc).expect("read_unnamed_tag"),
                t
            );
        }
    }
}

#[test]
fn parse_collects_stream_back_into_tree() {
    let original = rich_compound();
    let bytes = write_to_bytes(&original);

    let mut collector = CollectToTag::new();
    let mut input = DataInputStream::new(std::io::Cursor::new(&bytes));
    nbt_io::parse(
        &mut input,
        &mut collector,
        &mut NbtAccounter::unlimited_heap(),
    )
    .expect("parse failed");

    let result = collector.get_result().expect("parse produced no root");
    assert_eq!(result, Tag::Compound(original));
}

#[test]
fn parse_with_skip_all_consumes_without_error() {
    let original = rich_compound();
    let bytes = write_to_bytes(&original);

    let mut visitor = SkipAll;
    let mut input = DataInputStream::new(std::io::Cursor::new(&bytes));
    nbt_io::parse(
        &mut input,
        &mut visitor,
        &mut NbtAccounter::unlimited_heap(),
    )
    .expect("parse with SkipAll failed");
}

#[test]
fn heterogeneous_list_writes_wrapped_element_ids() {
    // A mixed-type list [Int(5), String("x")] — `identifyRawElementType()`
    // returns TAG_COMPOUND, so ListTag.write wraps every non-compound element.
    // Java's wrapElement produces `new CompoundTag(Map.of("", tag))` whose
    // write() emits `writeByte(tag.getId())` — the ELEMENT's id (3 for Int, 8
    // for String), not 10. Emitting 10 here would desynchronize the read side.
    let mut list = ListTag::new();
    list.add(Tag::Int(IntTag::new(5)));
    list.add(Tag::String(StringTag::value_of("x".to_string())));

    let mut c = CompoundTag::new();
    c.put("mixed".to_string(), Tag::List(list));

    let bytes = write_to_bytes(&c);
    // Root compound(10) + "" + "mixed" + list(9) + element type COMPOUND(10) + count 2,
    // then for each element: element's own id + "" name + payload + 0 terminator.
    assert_eq!(
        &bytes,
        &[
            0x0A, 0x00, 0x00, // compound + empty root name
            0x09, 0x00, 0x05, b'm', b'i', b'x', b'e', b'd', // list + "mixed"
            0x0A, 0x00, 0x00, 0x00, 0x02, // element type compound, count 2
            // wrapped Int(5): id 3, "", 5
            0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00,
            // wrapped String("x"): id 8, "", len 1, 'x'
            0x08, 0x00, 0x00, 0x00, 0x01, b'x', 0x00, 0x00, // compound end
        ]
    );

    // And it must round-trip losslessly.
    let decoded = read_from_bytes(&bytes);
    assert_eq!(decoded, c);
}

#[test]
fn negative_list_length_panics_like_java() {
    // Java `readListCount` throws the unchecked `NbtFormatException` for a
    // negative count; nothing catches it, so the parse crashes (panic).
    // Stream: compound(10) "" list(9) ""(empty key) elementType(0) count(-1).
    let input: Vec<u8> = vec![
        0x0A, 0x00, 0x00, // compound + empty root name
        0x09, 0x00, 0x00, // list + empty key
        0x00, // element type 0
        0xFF, 0xFF, 0xFF, 0xFF, // count -1
    ];
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut din = DataInputStream::new(std::io::Cursor::new(&input[..]));
        let _ = nbt_io::read(&mut din, &mut NbtAccounter::unlimited_heap());
    }));
    assert!(result.is_err());
}

#[test]
fn missing_list_element_type_panics_like_java() {
    // Java `loadList` throws `NbtFormatException("Missing type on ListTag")`
    // when typeId == 0 and count > 0 — unchecked, crashes the parse.
    // Stream: compound(10) "" list(9) ""(empty key) elementType(0) count(1).
    let input: Vec<u8> = vec![
        0x0A, 0x00, 0x00, // compound + empty root name
        0x09, 0x00, 0x00, // list + empty key
        0x00, // element type 0
        0x00, 0x00, 0x00, 0x01, // count 1
    ];
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut din = DataInputStream::new(std::io::Cursor::new(&input[..]));
        let _ = nbt_io::read(&mut din, &mut NbtAccounter::unlimited_heap());
    }));
    assert!(result.is_err());
}

#[test]
fn write_fallback_handles_overlong_string() {
    // A >65535-byte modified-UTF-8 string cannot be length-prefixed; Java's
    // StringFallbackDataOutput writes "" instead of throwing.
    let mut c = CompoundTag::new();
    // "a" * 40000 -> 40000 modified-UTF-8 bytes, under 65535 (need over).
    // Use 70000 ASCII chars (each 1 CESU-8 byte) to exceed the 2-byte limit.
    c.put_string("big", &"a".repeat(70_000));

    // NbtIo.write routes through StringFallbackDataOutput, so this must succeed
    // and round-trip as the empty string.
    let bytes = write_to_bytes(&c);
    let decoded = read_from_bytes(&bytes);
    assert_eq!(decoded.get_string("big").map(String::as_str), Some(""));
}

/// Locate the committed M0 oracle fixtures relative to the workspace root.
///
/// The crate compiles with its manifest dir at `<ws>/crates/rivet-nbt`, so the
/// fixtures live three levels up. Absent when the fixtures aren't checked out
/// (CI-less local merges, pruned trees) — the test then skips.
fn fixtures_dir() -> Option<std::path::PathBuf> {
    let ws = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .parent()?;
    let dir = ws.join("tools/rivet-oracle/fixtures/chunk");
    dir.is_dir().then_some(dir)
}

/// Walk the fixtures tree collecting `*.nbt` files (deterministic order).
fn collect_fixtures(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("fixtures dir readable") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("nbt") {
                out.push(path);
            }
        }
    }
    walk(dir, &mut out);
    out.sort();
    out
}

/// PARITY PILOT: every committed chunk-NBT fixture (a real decompressed chunk
/// the vanilla server produced) must parse through `NbtIo.read_any_tag` into a
/// `CompoundTag` with no error. This is the "rivet-nbt reads what Paper wrote"
/// proof for the 432-fixture corpus.
#[test]
fn all_committed_fixtures_parse_as_compounds() {
    let Some(dir) = fixtures_dir() else {
        eprintln!("M0 fixtures not present — skipping parity fixture test");
        return;
    };
    let fixtures = collect_fixtures(&dir);
    assert!(
        fixtures.len() >= 432,
        "expected >=432 chunk fixtures, found {}",
        fixtures.len()
    );
    let mut failures = Vec::new();
    for path in &fixtures {
        let bytes = std::fs::read(path).expect("fixture readable");
        let mut input = DataInputStream::new(std::io::Cursor::new(&bytes));
        match nbt_io::read_any_tag(&mut input, &mut NbtAccounter::unlimited_heap()) {
            Ok(Tag::Compound(_)) => {}
            Ok(other) => failures.push(format!(
                "{}: parsed as {:?} not a compound",
                path.display(),
                other.id()
            )),
            Err(e) => failures.push(format!("{}: {e}", path.display())),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} fixtures failed to parse as compounds:\n{}",
        failures.len(),
        fixtures.len(),
        failures.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn float_double_nan_canonicalized_on_write() {
    // DataOutputStream.writeFloat/writeDouble use Float.floatToIntBits /
    // Double.doubleToLongBits, which canonicalize EVERY NaN payload to
    // 0x7fc00000 / 0x7ff8000000000000. A NaN with a non-canonical payload must
    // therefore serialize to the canonical bits. Written via writeUnnamedTag to
    // avoid compound-key iteration order.
    let custom_float_nan = f32::from_bits(0x7fc0_1234);
    let custom_double_nan = f64::from_bits(0x7ff8_1234_5678_9abc);

    let mut buf: Vec<u8> = Vec::new();
    {
        let mut out = DataOutputStream::new(&mut buf);
        nbt_io::write_unnamed_tag(&Tag::Float(FloatTag::new(custom_float_nan)), &mut out)
            .expect("write_unnamed_tag float");
        nbt_io::write_unnamed_tag(&Tag::Double(DoubleTag::new(custom_double_nan)), &mut out)
            .expect("write_unnamed_tag double");
    }
    // writeUnnamedTag emits id + "" name (0x00 0x00) + payload.
    assert_eq!(
        buf,
        vec![
            0x05, 0x00, 0x00, 0x7F, 0xC0, 0x00, 0x00, // float NaN canonicalized
            0x06, 0x00, 0x00, 0x7F, 0xF8, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, // double NaN canonicalized
        ]
    );
}
