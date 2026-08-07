//! Port of `net.minecraft.nbt.NbtIo` — the binary NBT read/write path.
//!
//! Java: `public class NbtIo`. Faithful to `NbtIo.java` in `working/Paper`
//! (vanilla 26.2; the Spigot `ByteBufInputStream`/`LimitStream` wrapping is a
//! server-side network concern, not part of the tag codec, and is not ported).
//!
//! The per-tag `load`/`parse`/`skip` dispatch lives here as free functions on
//! `TagType` (Java keeps them as methods on the `TagType.TYPE` singletons in
//! `StringTag`, `CompoundTag`, ...; `tag_type.rs` intentionally does not carry
//! the data-IO members, see its module doc).
//!
//! `NbtAccounter` is threaded as `&mut` — Java passes one shared mutable
//! `NbtAccounter` reference through the whole read/parse.
//!
//! Error mapping follows PORTING.md line 33: Java's checked `IOException`
//! (EOF, UTF errors, root-not-compound) maps to `Result<T, io::Error>`, while
//! the unchecked `RuntimeException`s the read path throws — `NbtFormatException`
//! (negative list length, missing list element type) and Spigot's
//! `IllegalArgumentException` (oversized array, `check_array_length`) — are not
//! caught by Java's `readTagSafe`/`readNamedTagData` `catch (IOException)`, so
//! they map to `panic!` (crashing the parse) exactly like `check_array_length`.
//!
//! One deliberate deviation from that split: `readTagSafe` and
//! `readNamedTagData` themselves catch a tag-load `IOException` and rethrow it
//! as the *unchecked* `ReportedNbtException` (a `RuntimeException` that escapes
//! the read path and crashes Java's parse). Here that path maps to
//! `Err(io::Error)` carrying a `ReportedException` (see `tag_load_error`) rather
//! than `panic!`: the underlying cause is an I/O condition (EOF / bad UTF-8)
//! and the checked-vs-unchecked distinction is not observable at the byte level.
//! The differential oracle must therefore treat Java's `ReportedNbtException`
//! (crash) and Rust's `Err` (recoverable) as equivalent outcomes.
//!
//! The Java `Path`-based entry points (`readCompressed(Path)`, `write(Path)`,
//! ...) take the `Files`/`OpenOption` machinery; the faithful stream-based
//! surface (`read`, `write`, `read_compressed`, `write_compressed`, `parse`)
//! is provided here. The `Path` variants resolve to the same streams and can be
//! added with the real file-IO glue once a `Path` port exists.

use std::io::{self, Read, Write};

use crate::byte_array_tag::ByteArrayTag;
use crate::byte_tag::ByteTag;
use crate::compound_tag::CompoundTag;
use crate::double_tag::DoubleTag;
use crate::end_tag::EndTag;
use crate::float_tag::FloatTag;
use crate::int_array_tag::IntArrayTag;
use crate::int_tag::IntTag;
use crate::list_tag::ListTag;
use crate::long_array_tag::LongArrayTag;
use crate::long_tag::LongTag;
use crate::nbt_accounter::NbtAccounter;
use crate::reported_nbt_exception::ReportedNbtException;
use crate::short_tag::ShortTag;
use crate::stream_tag_visitor::{EntryResult, StreamTagVisitor, ValueResult};
use crate::string_tag::StringTag;
use crate::tag::{TAG_COMPOUND, Tag};
use crate::tag_type::TagType;
use crate::tag_types;
use rivet_core::{CrashReport, ReportedException};

use rivet_util::data_io::{DataInput, DataOutput};
use rivet_util::delegate_data_output::DelegateDataOutput;
use rivet_util::fast_buffered_input_stream::FastBufferedInputStream;
use rivet_util::{DataInputStream, DataOutputStream, log_and_pause_if_in_ide};

// RivetTodo(#231): the region-file write path (RegionFileVersion) uses
// `VERSION_DEFLATE`/`VERSION_LZ4` compression in general. The M2 byte-identity
// gate pins `region-file-compression=none` (DECISIONS.md D13); only gzip (this
// module) and `none` are written today. Deflate write parity is not `flate2`-
// reproducible against Java `Deflater` in general (deferred), and LZ4 write
// support is not yet ported (deferred) — both land with the chunk.storage wave.

/// `NbtIo.createDecompressorStream(InputStream)` — `DataInputStream(
/// FastBufferedInputStream(GZIPInputStream(in)))`.
///
/// `MultiGzDecoder` mirrors `GZIPInputStream`, which continues reading
/// concatenated gzip members within one stream (a single-member file is read
/// identically to `GzDecoder`).
fn create_decompressor_stream<R: Read>(
    input: R,
) -> DataInputStream<FastBufferedInputStream<flate2::read::MultiGzDecoder<R>>> {
    DataInputStream::new(FastBufferedInputStream::new(
        flate2::read::MultiGzDecoder::new(input),
    ))
}

/// `NbtIo.createCompressorStream(OutputStream)` — `DataOutputStream(
/// BufferedOutputStream(GZIPOutputStream(out)))`. `flate2::Compression::default()`
/// matches `java.util.zip.GZIPOutputStream`'s default (level 6).
fn create_compressor_stream<W: Write>(
    output: W,
) -> DataOutputStream<io::BufWriter<flate2::write::GzEncoder<W>>> {
    DataOutputStream::new(io::BufWriter::new(flate2::write::GzEncoder::new(
        output,
        flate2::Compression::default(),
    )))
}

/// `NbtIo.readCompressed(InputStream, NbtAccounter)`.
pub fn read_compressed<R: Read>(
    input: R,
    accounter: &mut NbtAccounter,
) -> Result<CompoundTag, io::Error> {
    let mut dis = create_decompressor_stream(input);
    read(&mut dis, accounter)
}

/// `NbtIo.parseCompressed(InputStream, StreamTagVisitor, NbtAccounter)`.
pub fn parse_compressed<R: Read>(
    input: R,
    output: &mut dyn StreamTagVisitor,
    accounter: &mut NbtAccounter,
) -> Result<(), io::Error> {
    let mut dis = create_decompressor_stream(input);
    parse(&mut dis, output, accounter)
}

/// `NbtIo.writeCompressed(CompoundTag, OutputStream)`.
///
/// Java's try-with-resources closes the stream, flushing the `BufferedOutputStream`
/// and finishing the `GZIPOutputStream`; we mirror that by flushing the buffer,
/// finishing the gzip encoder, and flushing the caller-supplied writer (Java
/// closes the whole chain down to and including the caller's `OutputStream`).
pub fn write_compressed<W: Write>(tag: &CompoundTag, output: W) -> Result<(), io::Error> {
    let mut dos = create_compressor_stream(output);
    write(tag, &mut dos)?;
    let buffer = dos.into_inner();
    let encoder = buffer.into_inner().map_err(|e| e.into_error())?;
    let mut output = encoder.finish()?;
    output.flush()?;
    Ok(())
}

/// `NbtIo.write(CompoundTag, DataOutput)` — `writeUnnamedTagWithFallback(tag,
/// output)`.
pub fn write(tag: &CompoundTag, output: &mut dyn DataOutput) -> Result<(), io::Error> {
    let mut fallback = StringFallbackDataOutput::new(output);
    // Write the unnamed root compound without deep-cloning it into a `Tag`
    // (Java passes the live reference; the clone is forced by the value-owned
    // `Tag` enum, so the payload is written from `&CompoundTag` directly).
    write_unnamed_tag_parts(TAG_COMPOUND, |out| write_compound(tag, out), &mut fallback)
}

/// `NbtIo.read(DataInput, NbtAccounter)`.
pub fn read(
    input: &mut dyn DataInput,
    accounter: &mut NbtAccounter,
) -> Result<CompoundTag, io::Error> {
    match read_unnamed_tag(input, accounter)? {
        Tag::Compound(compound_tag) => Ok(compound_tag),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Root tag must be a named compound tag",
        )),
    }
}

/// `NbtIo.read(DataInput)` — `read(input, NbtAccounter.unlimitedHeap())`.
pub fn read_unlimited(input: &mut dyn DataInput) -> Result<CompoundTag, io::Error> {
    read(input, &mut NbtAccounter::unlimited_heap())
}

/// `NbtIo.parse(DataInput, StreamTagVisitor, NbtAccounter)`.
pub fn parse(
    input: &mut dyn DataInput,
    output: &mut dyn StreamTagVisitor,
    accounter: &mut NbtAccounter,
) -> Result<(), io::Error> {
    let type_id = input.read_unsigned_byte()? as i8;
    let ty = tag_types::get_type(type_id);
    if ty == TagType::End {
        if output.visit_root_entry(TagType::End) == ValueResult::Continue {
            output.visit_end();
        }
    } else {
        match output.visit_root_entry(ty) {
            ValueResult::Halt => {}
            ValueResult::Break => {
                skip_string(input)?;
                skip(input, ty, accounter)?;
            }
            ValueResult::Continue => {
                skip_string(input)?;
                parse_tag(input, output, ty, accounter)?;
            }
        }
    }
    Ok(())
}

/// `NbtIo.readAnyTag(DataInput, NbtAccounter)`.
pub fn read_any_tag(
    input: &mut dyn DataInput,
    accounter: &mut NbtAccounter,
) -> Result<Tag, io::Error> {
    let type_id = input.read_unsigned_byte()? as i8;
    if type_id == 0 {
        Ok(Tag::End(EndTag))
    } else {
        read_tag_safe(input, accounter, type_id)
    }
}

/// `NbtIo.writeAnyTag(Tag, DataOutput)`.
pub fn write_any_tag(tag: &Tag, output: &mut dyn DataOutput) -> Result<(), io::Error> {
    output.write_byte(tag.id() as i32)?;
    if tag.id() != 0 {
        write_tag(tag, output)?;
    }
    Ok(())
}

/// `NbtIo.writeUnnamedTag(Tag, DataOutput)`.
pub fn write_unnamed_tag(tag: &Tag, output: &mut dyn DataOutput) -> Result<(), io::Error> {
    write_unnamed_tag_parts(tag.id(), |out| write_tag(tag, out), output)
}

/// `NbtIo.writeUnnamedTagWithFallback(Tag, DataOutput)`.
pub fn write_unnamed_tag_with_fallback(
    tag: &Tag,
    output: &mut dyn DataOutput,
) -> Result<(), io::Error> {
    let mut fallback = StringFallbackDataOutput::new(output);
    write_unnamed_tag(tag, &mut fallback)
}

/// `NbtIo.writeUnnamedTag(Tag, DataOutput)` split into `id` + payload writer so
/// callers that hold a concrete tag (e.g. `&CompoundTag` in `write`) can emit an
/// unnamed tag without materializing a `Tag` enum value.
fn write_unnamed_tag_parts(
    id: i8,
    write_payload: impl Fn(&mut dyn DataOutput) -> Result<(), io::Error>,
    output: &mut dyn DataOutput,
) -> Result<(), io::Error> {
    output.write_byte(id as i32)?;
    if id != 0 {
        output.write_utf("")?;
        write_payload(output)?;
    }
    Ok(())
}

/// `NbtIo.readUnnamedTag(DataInput, NbtAccounter)`.
pub fn read_unnamed_tag(
    input: &mut dyn DataInput,
    accounter: &mut NbtAccounter,
) -> Result<Tag, io::Error> {
    let type_id = input.read_unsigned_byte()? as i8;
    if type_id == 0 {
        Ok(Tag::End(EndTag))
    } else {
        skip_string(input)?;
        read_tag_safe(input, accounter, type_id)
    }
}

/// `NbtIo.readTagSafe(DataInput, NbtAccounter, byte)` — load, wrapping any
/// failure in the `ReportedNbtException`-style error (Java catches `IOException`
/// and rethrows as `ReportedNbtException`).
fn read_tag_safe(
    input: &mut dyn DataInput,
    accounter: &mut NbtAccounter,
    type_id: i8,
) -> Result<Tag, io::Error> {
    let ty = tag_types::get_type(type_id);
    load(input, accounter, ty).map_err(|e| tag_load_error(ty, Some(type_id), None, &e))
}

/// `CompoundTag.readNamedTagData` / `NbtIo.readTagSafe` failure path — builds
/// the `ReportedNbtException` `io::Error` mirroring
/// `CrashReport.forThrowable(e, "Loading NBT data")` + `addCategory("NBT Tag")`
/// with `setDetail("Tag name", name)` (when named) and `setDetail("Tag type",
/// type)`.
///
/// The two Java call sites emit different "Tag type" values: `readNamedTagData`
/// uses `type.getName()` (e.g. "INT"), while `readTagSafe` sets the raw byte id
/// it was handed (e.g. 3). `raw_id = Some(id)` selects the readTagSafe form;
/// `None` the named form.
fn tag_load_error(
    tag_type: TagType,
    raw_id: Option<i8>,
    name: Option<&str>,
    error: &dyn std::fmt::Display,
) -> io::Error {
    let report = CrashReport::for_throwable(error, "Loading NBT data");
    let category = report.add_category("NBT Tag");
    if let Some(name) = name {
        category.set_detail("Tag name", name);
    }
    match raw_id {
        Some(id) => category.set_detail("Tag type", id),
        None => category.set_detail("Tag type", tag_type.name()),
    }
    io::Error::new(
        io::ErrorKind::InvalidData,
        ReportedException::from(ReportedNbtException::new(report)),
    )
}

/// `TagType.parse(DataInput, StreamTagVisitor, NbtAccounter)` dispatch for a
/// single tag (after the visitor accepted the entry). Returns the visitor's
/// `ValueResult` so `parseList`/`parseCompound` can propagate HALT/BREAK.
fn parse_tag(
    input: &mut dyn DataInput,
    output: &mut dyn StreamTagVisitor,
    ty: TagType,
    accounter: &mut NbtAccounter,
) -> Result<ValueResult, io::Error> {
    match ty {
        TagType::End => {
            accounter.account_bytes(8);
            Ok(output.visit_end())
        }
        TagType::Byte => {
            accounter.account_bytes(9);
            Ok(output.visit_byte(input.read_unsigned_byte()? as i8))
        }
        TagType::Short => {
            accounter.account_bytes(10);
            Ok(output.visit_short(input.read_unsigned_short()? as i16))
        }
        TagType::Int => {
            accounter.account_bytes(12);
            Ok(output.visit_int(input.read_int()?))
        }
        TagType::Long => {
            accounter.account_bytes(16);
            Ok(output.visit_long(input.read_long()?))
        }
        TagType::Float => {
            accounter.account_bytes(12);
            Ok(output.visit_float(input.read_float()?))
        }
        TagType::Double => {
            accounter.account_bytes(16);
            Ok(output.visit_double(input.read_double()?))
        }
        TagType::ByteArray => {
            accounter.account_bytes(24);
            let length = input.read_int()?;
            check_array_length(length);
            accounter.account_bytes_per_entry(1, length as i64);
            let data: Vec<u8> = input.read_fully(length as usize)?;
            let bytes: Vec<i8> = data.into_iter().map(|b| b as i8).collect();
            Ok(output.visit_byte_array(&bytes))
        }
        TagType::IntArray => {
            accounter.account_bytes(24);
            let length = input.read_int()?;
            check_array_length(length);
            accounter.account_bytes_per_entry(4, length as i64);
            let mut data = Vec::with_capacity(length as usize);
            for _ in 0..length {
                data.push(input.read_int()?);
            }
            Ok(output.visit_int_array(&data))
        }
        TagType::LongArray => {
            accounter.account_bytes(24);
            let length = input.read_int()?;
            accounter.account_bytes_per_entry(8, length as i64);
            let mut data = Vec::with_capacity(length as usize);
            for _ in 0..length {
                data.push(input.read_long()?);
            }
            Ok(output.visit_long_array(&data))
        }
        TagType::String => {
            accounter.account_bytes(36);
            let s = input.read_utf()?;
            accounter.account_bytes_per_entry(2, s.encode_utf16().count() as i64);
            Ok(output.visit_string(&s))
        }
        TagType::List => parse_list(input, output, accounter),
        TagType::Compound => parse_compound(input, output, accounter),
        TagType::Invalid(id) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid tag id: {id}"),
        )),
    }
}

/// Spigot guard `Preconditions.checkArgument(length < 1 << 24)` from the array
/// tags' `readAccounted`. Java throws an unchecked `IllegalArgumentException`
/// (crashing the parse) which nothing catches; per PORTING.md an unchecked
/// crash maps to `panic!`.
fn check_array_length(length: i32) {
    if length >= (1 << 24) {
        panic!("Array tag length must be < 1 << 24, got {length}");
    }
}

/// `TagType.load(DataInput, NbtAccounter)` dispatch.
pub fn load(
    input: &mut dyn DataInput,
    accounter: &mut NbtAccounter,
    ty: TagType,
) -> Result<Tag, io::Error> {
    match ty {
        TagType::End => {
            accounter.account_bytes(8);
            Ok(Tag::End(EndTag))
        }
        TagType::Byte => {
            accounter.account_bytes(9);
            Ok(Tag::Byte(ByteTag::value_of(
                input.read_unsigned_byte()? as i8
            )))
        }
        TagType::Short => {
            accounter.account_bytes(10);
            Ok(Tag::Short(ShortTag::value_of(
                input.read_unsigned_short()? as i16
            )))
        }
        TagType::Int => {
            accounter.account_bytes(12);
            Ok(Tag::Int(IntTag::value_of(input.read_int()?)))
        }
        TagType::Long => {
            accounter.account_bytes(16);
            Ok(Tag::Long(LongTag::value_of(input.read_long()?)))
        }
        TagType::Float => {
            accounter.account_bytes(12);
            Ok(Tag::Float(FloatTag::value_of(input.read_float()?)))
        }
        TagType::Double => {
            accounter.account_bytes(16);
            Ok(Tag::Double(DoubleTag::value_of(input.read_double()?)))
        }
        TagType::ByteArray => {
            accounter.account_bytes(24);
            let length = input.read_int()?;
            check_array_length(length);
            accounter.account_bytes_per_entry(1, length as i64);
            let data = input
                .read_fully(length as usize)?
                .into_iter()
                .map(|b| b as i8)
                .collect();
            Ok(Tag::ByteArray(ByteArrayTag::new(data)))
        }
        TagType::IntArray => {
            accounter.account_bytes(24);
            let length = input.read_int()?;
            check_array_length(length);
            accounter.account_bytes_per_entry(4, length as i64);
            let mut data = Vec::with_capacity(length as usize);
            for _ in 0..length {
                data.push(input.read_int()?);
            }
            Ok(Tag::IntArray(IntArrayTag::new(data)))
        }
        TagType::LongArray => {
            accounter.account_bytes(24);
            let length = input.read_int()?;
            accounter.account_bytes_per_entry(8, length as i64);
            let mut data = Vec::with_capacity(length as usize);
            for _ in 0..length {
                data.push(input.read_long()?);
            }
            Ok(Tag::LongArray(LongArrayTag::new(data)))
        }
        TagType::String => {
            accounter.account_bytes(36);
            let s = input.read_utf()?;
            accounter.account_bytes_per_entry(2, s.encode_utf16().count() as i64);
            Ok(Tag::String(StringTag::value_of(s)))
        }
        TagType::List => load_list(input, accounter),
        TagType::Compound => load_compound(input, accounter),
        TagType::Invalid(id) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid tag id: {id}"),
        )),
    }
}

/// `ListTag.TYPE.load` — `loadList` with the surrounding depth push/pop.
fn load_list(input: &mut dyn DataInput, accounter: &mut NbtAccounter) -> Result<Tag, io::Error> {
    accounter.push_depth();
    let result = load_list_inner(input, accounter);
    accounter.pop_depth();
    result
}

fn load_list_inner(
    input: &mut dyn DataInput,
    accounter: &mut NbtAccounter,
) -> Result<Tag, io::Error> {
    accounter.account_bytes(36);
    let type_id = input.read_unsigned_byte()? as i8;
    let count = read_list_count(input)?;
    if type_id == 0 && count > 0 {
        // Java `new NbtFormatException("Missing type on ListTag")` — an
        // unchecked RuntimeException that readTagSafe/readNamedTagData's
        // `catch (IOException)` does not catch, so it crashes the parse (see
        // the module doc + `check_array_length` precedent).
        panic!("Missing type on ListTag");
    }

    accounter.account_bytes_per_entry(4, count as i64);
    let element_type = tag_types::get_type(type_id);
    let mut list = ListTag::with_list(Vec::with_capacity(count as usize));

    for _ in 0..count {
        list.add_and_unwrap(load(input, accounter, element_type)?);
    }

    Ok(Tag::List(list))
}

/// `ListTag.TYPE.parse` — `parseList` with the surrounding depth push/pop.
fn parse_list(
    input: &mut dyn DataInput,
    output: &mut dyn StreamTagVisitor,
    accounter: &mut NbtAccounter,
) -> Result<ValueResult, io::Error> {
    accounter.push_depth();
    let result = parse_list_inner(input, output, accounter);
    accounter.pop_depth();
    result
}

fn parse_list_inner(
    input: &mut dyn DataInput,
    output: &mut dyn StreamTagVisitor,
    accounter: &mut NbtAccounter,
) -> Result<ValueResult, io::Error> {
    accounter.account_bytes(36);
    let element_type = tag_types::get_type(input.read_unsigned_byte()? as i8);
    let count = read_list_count(input)?;
    match output.visit_list(element_type, count as usize) {
        ValueResult::Halt => return Ok(ValueResult::Halt),
        ValueResult::Break => {
            skip_count(input, element_type, count, accounter)?;
            return Ok(output.visit_container_end());
        }
        ValueResult::Continue => {}
    }

    accounter.account_bytes_per_entry(4, count as i64);
    let mut i: i32 = 0;

    loop {
        if i < count {
            match output.visit_element(element_type, i as usize) {
                EntryResult::Halt => return Ok(ValueResult::Halt),
                EntryResult::Break => {
                    skip(input, element_type, accounter)?;
                    // fall through to the amountToSkip tail
                }
                EntryResult::Skip => {
                    skip(input, element_type, accounter)?;
                    i += 1;
                    continue;
                }
                EntryResult::Enter => match parse_tag(input, output, element_type, accounter)? {
                    ValueResult::Halt => return Ok(ValueResult::Halt),
                    ValueResult::Break => {
                        // fall through to the amountToSkip tail
                    }
                    ValueResult::Continue => {
                        i += 1;
                        continue;
                    }
                },
            }
        }

        let amount_to_skip = count - 1 - i;
        if amount_to_skip > 0 {
            skip_count(input, element_type, amount_to_skip, accounter)?;
        }
        return Ok(output.visit_container_end());
    }
}

/// `CompoundTag.TYPE.load` — `loadCompound` with depth push/pop.
fn load_compound(
    input: &mut dyn DataInput,
    accounter: &mut NbtAccounter,
) -> Result<Tag, io::Error> {
    accounter.push_depth();
    let result = load_compound_inner(input, accounter);
    accounter.pop_depth();
    result
}

fn load_compound_inner(
    input: &mut dyn DataInput,
    accounter: &mut NbtAccounter,
) -> Result<Tag, io::Error> {
    accounter.account_bytes(48);
    // IndexMap preserves on-disk field order so the binary round-trips
    // byte-for-byte (DECISIONS.md D12).
    let mut values = indexmap::IndexMap::new();

    loop {
        let tag_type_id = input.read_unsigned_byte()? as i8;
        if tag_type_id == 0 {
            break;
        }
        let key = read_compound_string(input, accounter)?;
        let tag = load_named_tag_data(tag_types::get_type(tag_type_id), &key, input, accounter)?;
        if values.insert(key, tag).is_none() {
            accounter.account_bytes(36);
        }
    }

    Ok(Tag::Compound(CompoundTag::with_map(values)))
}

/// `CompoundTag.readNamedTagData(TagType, String, DataInput, NbtAccounter)`.
fn load_named_tag_data(
    ty: TagType,
    name: &str,
    input: &mut dyn DataInput,
    accounter: &mut NbtAccounter,
) -> Result<Tag, io::Error> {
    load(input, accounter, ty).map_err(|e| tag_load_error(ty, None, Some(name), &e))
}

/// `CompoundTag.readString(DataInput, NbtAccounter)`.
fn read_compound_string(
    input: &mut dyn DataInput,
    accounter: &mut NbtAccounter,
) -> Result<String, io::Error> {
    let key = input.read_utf()?;
    accounter.account_bytes(28);
    accounter.account_bytes_per_entry(2, key.encode_utf16().count() as i64);
    Ok(key)
}

/// `CompoundTag.TYPE.parse` — `parseCompound` with depth push/pop.
fn parse_compound(
    input: &mut dyn DataInput,
    output: &mut dyn StreamTagVisitor,
    accounter: &mut NbtAccounter,
) -> Result<ValueResult, io::Error> {
    accounter.push_depth();
    let result = parse_compound_inner(input, output, accounter);
    accounter.pop_depth();
    result
}

/// `CompoundTag.TYPE.parseCompound`.
///
/// `label35` = the Java break-out label; a `BREAK` entry leaves the main loop,
/// then any remaining entries are skipped before `visitContainerEnd`.
fn parse_compound_inner(
    input: &mut dyn DataInput,
    output: &mut dyn StreamTagVisitor,
    accounter: &mut NbtAccounter,
) -> Result<ValueResult, io::Error> {
    accounter.account_bytes(48);

    let mut broke_out = false;
    let mut tag_type_id: i8;

    loop {
        tag_type_id = input.read_unsigned_byte()? as i8;
        if tag_type_id == 0 {
            break;
        }
        let tag_type = tag_types::get_type(tag_type_id);
        match output.visit_entry(tag_type) {
            EntryResult::Halt => return Ok(ValueResult::Halt),
            EntryResult::Break => {
                skip_string(input)?;
                skip(input, tag_type, accounter)?;
                broke_out = true;
                break;
            }
            EntryResult::Skip => {
                skip_string(input)?;
                skip(input, tag_type, accounter)?;
                continue;
            }
            EntryResult::Enter => {}
        }
        let key = read_compound_string(input, accounter)?;
        match output.visit_entry_named(tag_type, &key) {
            EntryResult::Halt => return Ok(ValueResult::Halt),
            EntryResult::Break => {
                skip(input, tag_type, accounter)?;
                broke_out = true;
                break;
            }
            EntryResult::Skip => {
                skip(input, tag_type, accounter)?;
                continue;
            }
            EntryResult::Enter => {}
        }
        accounter.account_bytes(36);
        match parse_tag(input, output, tag_type, accounter)? {
            // Java `case BREAK:` is empty — BREAK and CONTINUE both continue the
            // loop; only HALT stops.
            ValueResult::Halt => return Ok(ValueResult::Halt),
            ValueResult::Break | ValueResult::Continue => {}
        }
    }

    if broke_out {
        loop {
            tag_type_id = input.read_unsigned_byte()? as i8;
            if tag_type_id == 0 {
                break;
            }
            skip_string(input)?;
            skip(input, tag_types::get_type(tag_type_id), accounter)?;
        }
    }

    Ok(output.visit_container_end())
}

/// `Tag.write(DataOutput)` dispatch.
pub fn write_tag(tag: &Tag, output: &mut dyn DataOutput) -> Result<(), io::Error> {
    match tag {
        Tag::End(_) => Ok(()),
        Tag::Byte(t) => output.write_byte(t.value as i32),
        Tag::Short(t) => output.write_short(t.value),
        Tag::Int(t) => output.write_int(t.value),
        Tag::Long(t) => output.write_long(t.value),
        Tag::Float(t) => output.write_float(t.value),
        Tag::Double(t) => output.write_double(t.value),
        Tag::ByteArray(t) => {
            output.write_int(t.data.len() as i32)?;
            let bytes: Vec<u8> = t.data.iter().map(|&b| b as u8).collect();
            output.write_all(&bytes)
        }
        Tag::IntArray(t) => {
            output.write_int(t.data.len() as i32)?;
            for &v in &t.data {
                output.write_int(v)?;
            }
            Ok(())
        }
        Tag::LongArray(t) => {
            output.write_int(t.data.len() as i32)?;
            for &v in &t.data {
                output.write_long(v)?;
            }
            Ok(())
        }
        Tag::String(t) => output.write_utf(&t.value),
        Tag::List(t) => write_list(t, output),
        Tag::Compound(t) => write_compound(t, output),
    }
}

/// `ListTag.write(DataOutput)` — homogenous element type + count, then each
/// element written (wrapped into a `{"": tag}` compound when needed).
fn write_list(list: &ListTag, output: &mut dyn DataOutput) -> Result<(), io::Error> {
    let element_type = list.identify_raw_element_type();
    output.write_byte(element_type as i32)?;
    output.write_int(list.size() as i32)?;

    for element in list.iter() {
        if element_type != TAG_COMPOUND {
            write_tag(element, output)?;
        } else if let Tag::Compound(c) = element {
            if !is_wrapper(c) {
                write_compound(c, output)?;
            } else {
                write_wrapped_compound(element, output)?;
            }
        } else {
            write_wrapped_compound(element, output)?;
        }
    }
    Ok(())
}

/// `ListTag.wrapElement(Tag)` written inline — `new CompoundTag(Map.of("",
/// tag)).write(output)`.
///
/// Java's `CompoundTag.write` calls `writeNamedTag("", tag)`, which emits
/// `writeByte(tag.getId()); writeUTF(""); tag.write(output); writeByte(0)`.
/// The leading byte is the *wrapped element's* id, which equals `TAG_COMPOUND`
/// (10) only when the element is itself a compound; for a non-compound element
/// in a mixed-type list (`identifyRawElementType()` returns `TAG_COMPOUND`) it
/// must be the element's own id (e.g. 3 for an Int) — writing 10 there would
/// emit an invalid stream that reads back as an empty compound and
/// desynchronizes the rest of the list.
fn write_wrapped_compound(tag: &Tag, output: &mut dyn DataOutput) -> Result<(), io::Error> {
    output.write_byte(tag.id() as i32)?;
    output.write_utf("")?;
    write_tag(tag, output)?;
    output.write_byte(0)?;
    Ok(())
}

/// `ListTag.isWrapper(CompoundTag)`.
fn is_wrapper(tag: &CompoundTag) -> bool {
    tag.size() == 1 && tag.contains("")
}

/// `CompoundTag.write(DataOutput)`.
///
/// Iterates `CompoundTag.tags`, which is an insertion-ordered `IndexMap`
/// (DECISIONS.md D12): the field order emitted is the order the tags were
/// inserted, so a compound that was read from binary NBT re-emits its on-disk
/// order byte-for-byte. For hand-built compounds the order is Rust's put
/// sequence, which differs from Java's fastutil hash order — the documented
/// `compound_key_order` divergence counted in PARITY.md, never a byte-identity
/// failure on read-back fixtures.
pub fn write_compound(
    compound: &CompoundTag,
    output: &mut dyn DataOutput,
) -> Result<(), io::Error> {
    for (key, tag) in compound.tags.iter() {
        write_named_tag(key, tag, output)?;
    }
    output.write_byte(0)
}

/// `CompoundTag.writeNamedTag(String, Tag, DataOutput)`.
fn write_named_tag(name: &str, tag: &Tag, output: &mut dyn DataOutput) -> Result<(), io::Error> {
    output.write_byte(tag.id() as i32)?;
    if tag.id() != 0 {
        output.write_utf(name)?;
        write_tag(tag, output)?;
    }
    Ok(())
}

/// `TagType.skip(DataInput, NbtAccounter)` dispatch.
pub fn skip(
    input: &mut dyn DataInput,
    ty: TagType,
    accounter: &mut NbtAccounter,
) -> Result<(), io::Error> {
    match ty {
        TagType::End => Ok(()),
        // StaticSize.skip — `input.skipBytes(size())`. `skip_bytes` mirrors
        // `DataInputStream.skipBytes`, which on EOF silently returns the bytes
        // actually skipped rather than failing (the parse then continues,
        // misaligned, and fails later — matching Java).
        TagType::Byte
        | TagType::Short
        | TagType::Int
        | TagType::Long
        | TagType::Float
        | TagType::Double => {
            let size = ty.static_size().unwrap_or(0) as usize;
            input.skip_bytes(size)?;
            Ok(())
        }
        // Java array `skip`: `input.skipBytes(readInt() * unit)` — the product
        // is int arithmetic (wrapping), and `skipBytes` with a negative n skips
        // nothing, so guard on the wrapped product.
        TagType::ByteArray => {
            let length = input.read_int()?;
            let n = length.wrapping_mul(1);
            if n > 0 {
                input.skip_bytes(n as usize)?;
            }
            Ok(())
        }
        TagType::IntArray => {
            let length = input.read_int()?;
            let n = length.wrapping_mul(4);
            if n > 0 {
                input.skip_bytes(n as usize)?;
            }
            Ok(())
        }
        TagType::LongArray => {
            let length = input.read_int()?;
            let n = length.wrapping_mul(8);
            if n > 0 {
                input.skip_bytes(n as usize)?;
            }
            Ok(())
        }
        TagType::String => skip_string(input),
        TagType::List => skip_list(input, accounter),
        TagType::Compound => skip_compound(input, accounter),
        TagType::Invalid(id) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid tag id: {id}"),
        )),
    }
}

/// `TagType.skip(DataInput, int count, NbtAccounter)` — `StaticSize` multiplies
/// by the element size; `VariableSize` loops.
pub fn skip_count(
    input: &mut dyn DataInput,
    ty: TagType,
    count: i32,
    accounter: &mut NbtAccounter,
) -> Result<(), io::Error> {
    if let Some(size) = ty.static_size() {
        // StaticSize.skip(input, count) — `input.skipBytes(size * count)`, int
        // arithmetic (wrapping); `skipBytes` with a negative n skips nothing.
        let total = size.wrapping_mul(count);
        if total > 0 {
            input.skip_bytes(total as usize)?;
        }
        Ok(())
    } else {
        for _ in 0..count {
            skip(input, ty, accounter)?;
        }
        Ok(())
    }
}

/// `ListTag.TYPE.skip` — depth push/pop around `type.skip(input, count)`.
fn skip_list(input: &mut dyn DataInput, accounter: &mut NbtAccounter) -> Result<(), io::Error> {
    accounter.push_depth();
    let result = (|| {
        let element_type = tag_types::get_type(input.read_unsigned_byte()? as i8);
        let count = input.read_int()?;
        skip_count(input, element_type, count, accounter)
    })();
    accounter.pop_depth();
    result
}

/// `CompoundTag.TYPE.skip` — depth push/pop around the entry loop.
fn skip_compound(input: &mut dyn DataInput, accounter: &mut NbtAccounter) -> Result<(), io::Error> {
    accounter.push_depth();
    let result = (|| loop {
        let tag_type_id = input.read_unsigned_byte()? as i8;
        if tag_type_id == 0 {
            return Ok(());
        }
        skip_string(input)?;
        skip(input, tag_types::get_type(tag_type_id), accounter)?;
    })();
    accounter.pop_depth();
    result
}

/// `StringTag.skipString(DataInput)` — `input.skipBytes(len)`.
pub fn skip_string(input: &mut dyn DataInput) -> Result<(), io::Error> {
    let len = input.read_unsigned_short()? as usize;
    input.skip_bytes(len)?;
    Ok(())
}

/// `ListTag.readListCount(DataInput)` — a negative length throws the unchecked
/// `NbtFormatException("ListTag length cannot be negative: " + count)`, which
/// Java's read path does not catch, so it crashes the parse (see the module
/// doc + `check_array_length` precedent).
fn read_list_count(input: &mut dyn DataInput) -> Result<i32, io::Error> {
    let count = input.read_int()?;
    if count < 0 {
        panic!("ListTag length cannot be negative: {count}");
    }
    Ok(count)
}

/// `NbtIo.StringFallbackDataOutput` — overrides `writeUTF`: on a
/// `UTFDataFormatException` (string too long for modified UTF-8's 2-byte
/// length prefix) it logs and writes the empty string instead.
pub struct StringFallbackDataOutput<T: DataOutput> {
    parent: DelegateDataOutput<T>,
}

impl<T: DataOutput> StringFallbackDataOutput<T> {
    /// `new StringFallbackDataOutput(DataOutput parent)`.
    pub fn new(parent: T) -> Self {
        StringFallbackDataOutput {
            parent: DelegateDataOutput::new(parent),
        }
    }
}

impl<T: DataOutput> DataOutput for StringFallbackDataOutput<T> {
    fn write(&mut self, b: i32) -> io::Result<()> {
        self.parent.write(b)
    }

    fn write_all(&mut self, b: &[u8]) -> io::Result<()> {
        self.parent.write_all(b)
    }

    fn write_utf(&mut self, s: &str) -> io::Result<()> {
        // Java's StringFallbackDataOutput catches only UTFDataFormatException —
        // the string-too-long-for-modified-UTF-8 case — and writes "" instead.
        // DataOutputStream.writeUTF throws that before writing anything, so
        // pre-checking the CESU-8 length here matches the Java condition exactly
        // without swallowing an unrelated InvalidData I/O error from the parent
        // (every other error propagates, as in Java).
        if cesu8::to_java_cesu8(s).len() > u16::MAX as usize {
            log_and_pause_if_in_ide("Failed to write NBT String");
            self.parent.write_utf("")
        } else {
            self.parent.write_utf(s)
        }
    }

    fn write_boolean(&mut self, v: bool) -> io::Result<()> {
        self.parent.write_boolean(v)
    }

    fn write_byte(&mut self, v: i32) -> io::Result<()> {
        self.parent.write_byte(v)
    }

    fn write_short(&mut self, v: i16) -> io::Result<()> {
        self.parent.write_short(v)
    }

    fn write_int(&mut self, v: i32) -> io::Result<()> {
        self.parent.write_int(v)
    }

    fn write_long(&mut self, v: i64) -> io::Result<()> {
        self.parent.write_long(v)
    }

    fn write_float(&mut self, v: f32) -> io::Result<()> {
        self.parent.write_float(v)
    }

    fn write_double(&mut self, v: f64) -> io::Result<()> {
        self.parent.write_double(v)
    }
}
