//! Strict network-NBT parser (`writeAnyTag`/`readNbt` format) shared by the
//! join-path canonicalizers.
//!
//! The `registry_data` (7) canonicalizer in `rivet-capture::structured` and the
//! `update_advancements` (130) display canonicalizer in [`crate::advancement`]
//! both parse network NBT, and both must reject exactly the wire bytes Java
//! rejects (a negative array/list size, a `ListTag` with elem type `End` and a
//! positive count, a truncated payload). One copy of the parser lives here so
//! the two canonicalizers cannot drift; `rivet-capture` depends on `rivet-decode`.
//!
//! Wire format (`NbtIo.writeAnyTag`): `[byte type][payload]` with the root
//! un-named and compound fields named `[byte type][u16 len][name][payload]`.
//! Strings are modified UTF-8 (`DataInput.readUTF`/`writeUTF`), decoded by the
//! OpenJDK-faithful [`decode_modified_utf8`].
//!
//! [`crate::advancement`]: crate::advancement

use crate::frame;

/// A parsed network NBT value. Compound fields are kept in parse order;
/// re-serialization sorts them by name so the wire form is canonical.
#[derive(Debug, Clone, PartialEq)]
pub enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List { elem: u8, items: Vec<Nbt> },
    Compound(Vec<(String, Nbt)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
    End,
}

/// `NbtIo` reads String payloads and compound field names with
/// `DataInput.readUTF` (modified UTF-8); `std::str::from_utf8` would reject the
/// astral (6-byte surrogate-pair) and NUL (`C0 80`) forms Java accepts. This is
/// the OpenJDK-25-faithful decoder ported in `rivet-util::data_io`.
pub fn decode_modified_utf8(bytes: &[u8]) -> Option<String> {
    rivet_util::data_io::decode_modified_utf8(bytes).ok()
}

/// `DataOutput.writeUTF` over `&str` — the 2-byte length counts the *encoded*
/// (modified-UTF-8) bytes, not the UTF-8 bytes, so an astral or NUL string is
/// re-encoded byte-for-byte the way Java wrote it. Shares the OpenJDK-faithful
/// encoder in `rivet-util::data_io`, so write/read round-trip through the same
/// codec. `write_utf_body` only fails on a body over 65535 encoded bytes; the
/// canonicalizer only re-encodes strings it decoded from u16-prefixed wire
/// fields, which are at most 65535 bytes and re-encode to at most that.
pub fn encode_modified_utf8(s: &str) -> Vec<u8> {
    rivet_util::data_io::write_utf_body(s)
        .expect("decoded u16-prefixed string re-encodes within the u16 limit")
}

/// Read a bare NBT payload of `type_byte` (no name prefix).
pub fn read_payload(body: &[u8], off: &mut usize, type_byte: u8) -> Option<Nbt> {
    match type_byte {
        0 => Some(Nbt::End),
        1 => {
            let v = *body.get(*off)? as i8;
            *off += 1;
            Some(Nbt::Byte(v))
        }
        2 => {
            let b = frame::read_bytes(body, off, 2)?;
            Some(Nbt::Short(i16::from_be_bytes([b[0], b[1]])))
        }
        3 => {
            let b = frame::read_bytes(body, off, 4)?;
            Some(Nbt::Int(i32::from_be_bytes([b[0], b[1], b[2], b[3]])))
        }
        4 => {
            let b = frame::read_bytes(body, off, 8)?;
            Some(Nbt::Long(i64::from_be_bytes([
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
            ])))
        }
        5 => {
            let b = frame::read_bytes(body, off, 4)?;
            Some(Nbt::Float(f32::from_be_bytes([b[0], b[1], b[2], b[3]])))
        }
        6 => {
            let b = frame::read_bytes(body, off, 8)?;
            Some(Nbt::Double(f64::from_be_bytes([
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
            ])))
        }
        7 => {
            let n = frame::read_i32(body, off)?;
            if n < 0 {
                return None; // Java: DecoderException on a negative array size
            }
            let bytes = frame::read_bytes(body, off, n as usize)?.to_vec();
            Some(Nbt::ByteArray(bytes))
        }
        8 => {
            let n = frame::read_u16(body, off)? as usize;
            let bytes = frame::read_bytes(body, off, n)?;
            let s = decode_modified_utf8(bytes)?;
            Some(Nbt::String(s))
        }
        9 => {
            let elem = *body.get(*off)?;
            *off += 1;
            let n = frame::read_i32(body, off)?;
            if n < 0 {
                return None; // Java: DecoderException on a negative list size
            }
            // Java's ListTag.loadList throws "Missing type on ListTag" when the
            // elem type is End (0) with a positive count.
            if elem == 0 && n > 0 {
                return None;
            }
            let mut items = Vec::with_capacity(n as usize);
            for _ in 0..n {
                items.push(read_payload(body, off, elem)?);
            }
            Some(Nbt::List { elem, items })
        }
        10 => {
            // Compound: `[field]*[type 0]`. A field whose payload fails to
            // parse (e.g. a negative ByteArray/List/IntArray/LongArray length,
            // or a ListTag with elem End and a positive count) must fail the
            // WHOLE compound, not be conflated with the type-0 terminator.
            // Java's `NbtIo` throws a `DecoderException` for a negative array
            // size; silently treating such a compound as terminated would
            // accept wire bytes Paper rejects.
            let mut fields = Vec::new();
            loop {
                let type_byte = *body.get(*off)?;
                *off += 1;
                if type_byte == 0 {
                    break; // end tag
                }
                let name_len = frame::read_u16(body, off)? as usize;
                let name_bytes = frame::read_bytes(body, off, name_len)?;
                // Field names are read with DataInput.readUTF (modified UTF-8).
                let name = decode_modified_utf8(name_bytes)?;
                let value = read_payload(body, off, type_byte)?;
                fields.push((name, value));
            }
            Some(Nbt::Compound(fields))
        }
        11 => {
            let n = frame::read_i32(body, off)?;
            if n < 0 {
                return None; // Java: DecoderException on a negative array size
            }
            let mut items = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let b = frame::read_bytes(body, off, 4)?;
                items.push(i32::from_be_bytes([b[0], b[1], b[2], b[3]]));
            }
            Some(Nbt::IntArray(items))
        }
        12 => {
            let n = frame::read_i32(body, off)?;
            if n < 0 {
                return None; // Java: DecoderException on a negative array size
            }
            let mut items = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let b = frame::read_bytes(body, off, 8)?;
                items.push(i64::from_be_bytes([
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                ]));
            }
            Some(Nbt::LongArray(items))
        }
        _ => None,
    }
}

/// Read a root NBT value (`[byte type][payload]`, root un-named).
pub fn read_nbt(body: &[u8], off: &mut usize) -> Option<Nbt> {
    let type_byte = *body.get(*off)?;
    *off += 1;
    read_payload(body, off, type_byte)
}

pub fn nbt_type_id(v: &Nbt) -> u8 {
    match v {
        Nbt::End => 0,
        Nbt::Byte(_) => 1,
        Nbt::Short(_) => 2,
        Nbt::Int(_) => 3,
        Nbt::Long(_) => 4,
        Nbt::Float(_) => 5,
        Nbt::Double(_) => 6,
        Nbt::ByteArray(_) => 7,
        Nbt::String(_) => 8,
        Nbt::List { .. } => 9,
        Nbt::Compound(_) => 10,
        Nbt::IntArray(_) => 11,
        Nbt::LongArray(_) => 12,
    }
}

/// Write a named compound field (`[byte type][u16 name len][name][payload]`).
/// The name length counts *encoded* (modified-UTF-8) bytes, matching
/// `DataOutput.writeUTF`; a plain `name.len()` would misreport astral names.
pub fn write_named_field(out: &mut Vec<u8>, name: &str, value: &Nbt) {
    let name_bytes = encode_modified_utf8(name);
    out.push(nbt_type_id(value));
    out.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(&name_bytes);
    write_payload(out, value);
}

pub fn write_payload(out: &mut Vec<u8>, value: &Nbt) {
    match value {
        Nbt::End => {}
        Nbt::Byte(v) => out.push(*v as u8),
        Nbt::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::Float(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::Double(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::ByteArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            out.extend_from_slice(v);
        }
        Nbt::String(v) => {
            // Length counts encoded (modified-UTF-8) bytes, matching
            // DataOutput.writeUTF.
            let bytes = encode_modified_utf8(v);
            out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
            out.extend_from_slice(&bytes);
        }
        Nbt::List { elem, items } => {
            out.push(*elem);
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for item in items {
                write_payload(out, item);
            }
        }
        Nbt::Compound(fields) => {
            // Always emit fields in sorted order so the serialized form is
            // canonical no matter how the compound was constructed.
            let mut sorted = fields.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, value) in &sorted {
                write_named_field(out, name, value);
            }
            out.push(0);
        }
        Nbt::IntArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        Nbt::LongArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
    }
}

/// Write a root NBT value (root un-named).
pub fn write_nbt(out: &mut Vec<u8>, value: &Nbt) {
    out.push(nbt_type_id(value));
    write_payload(out, value);
}
