//! Port of `net.minecraft.world.level.chunk.storage.RegionFileVersion` (MC 26.2).
//!
//! The region-file codec registry (§5 of `docs/region-file-format-spec.md`),
//! keyed by the `compression_type` byte of every chunk stream:
//!
//! | id | name | Paper read | Paper write |
//! |----|------|------------|-------------|
//! | 1 | gzip | `FastBufferedInputStream(GZIPInputStream)` | `BufferedOutputStream(GZIPOutputStream)` |
//! | 2 | deflate | `FastBufferedInputStream(InflaterInputStream)` | `BufferedOutputStream(DeflaterOutputStream)` |
//! | 3 | none | byte-transparent | byte-transparent |
//! | 4 | lz4 | `FastBufferedInputStream(LZ4BlockInputStream)` | `BufferedOutputStream(LZ4BlockOutputStream)` |
//! | 127 | — | reads a modified-UTF-8 id, logs, returns null | `UnsupportedOperationException` |
//!
//! `DEFAULT = VERSION_DEFLATE`; `configure` switches the selected version from
//! `server.properties` `region-file-compression`. Under DECISIONS.md D13 the
//! byte-identity gate pins `region-file-compression=none`, so Rivet's writer is
//! only required to emit id 3; the other ids are read-side-only requirements.
//!
//! `wrap_input`/`wrap_output` (Java's `StreamWrapper` functional interface)
//! unwrap/rewrap a stream with the registered codec. gzip read and gzip/none
//! write are wired on `flate2`; deflate **read** is zlib-wrapped — Java's
//! `InflaterInputStream` defaults to `nowrap=false`, the RFC 1950 zlib framing
//! that `flate2`'s `ZlibDecoder` reads — and everything else errors.
//!
//! The id `127` custom codec has no `option_name` and so can never be selected
//! (it is absent from `VERSIONS_BY_NAME`), exactly like Paper.

// RivetTodo(#231): the id-127 read path (read a modified-UTF-8 id, log
// "Unrecognized custom compression" / "Invalid custom compression id", return
// null), lz4 read (the lz4-java "LZ4 Block" format: `lz4_flex` + `xxhash-rust`'s
// xxh32 with seed `0x9747b28c`, see CRATES.md), and deflate/lz4 **write** land
// with the file-backed `RegionFile` read/write wave. deflate write stays
// deferred (Java `Deflater` is not `flate2`-reproducible in general — D13);
// lz4 write is not ported. gzip read/write and deflate read (zlib-wrapped,
// via `flate2`'s `ZlibDecoder`) are wired on `flate2` here.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicI32, Ordering};

/// A registered region-file codec (Java `RegionFileVersion`). A value type: the
/// five registered codecs are `Copy` and shared freely like `GameData`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionFileVersion {
    id: i32,
    option_name: Option<&'static str>,
}

/// `RegionFileVersion.selected` — the codec the writer uses for a whole region
/// file, switched by `configure`. `AtomicI32` mirrors Java's `volatile` field
/// without a lock; it only ever holds a registered write-side id (never 127).
static SELECTED_ID: AtomicI32 = AtomicI32::new(RegionFileVersion::DEFAULT.id());

impl RegionFileVersion {
    /// `VERSION_GZIP` — id 1.
    pub const VERSION_GZIP: Self = Self {
        id: 1,
        option_name: Some("gzip"),
    };
    /// `VERSION_DEFLATE` — id 2; `DEFAULT`.
    pub const VERSION_DEFLATE: Self = Self {
        id: 2,
        option_name: Some("deflate"),
    };
    /// `VERSION_NONE` — id 3; the D13 byte-identity gate codec.
    pub const VERSION_NONE: Self = Self {
        id: 3,
        option_name: Some("none"),
    };
    /// `VERSION_LZ4` — id 4.
    pub const VERSION_LZ4: Self = Self {
        id: 4,
        option_name: Some("lz4"),
    };
    /// `VERSION_CUSTOM` — id 127; no `option_name`, so never selectable.
    pub const VERSION_CUSTOM: Self = Self {
        id: 127,
        option_name: None,
    };
    /// `DEFAULT` — `VERSION_DEFLATE`.
    pub const DEFAULT: Self = Self::VERSION_DEFLATE;

    /// `fromId(int)` — the codec registered for `id`, or `None` for any
    /// unregistered id (the reader treats those streams as corrupt).
    pub fn from_id(id: i32) -> Option<Self> {
        match id {
            1 => Some(Self::VERSION_GZIP),
            2 => Some(Self::VERSION_DEFLATE),
            3 => Some(Self::VERSION_NONE),
            4 => Some(Self::VERSION_LZ4),
            127 => Some(Self::VERSION_CUSTOM),
            _ => None,
        }
    }

    /// `VERSIONS_BY_NAME.get(optionName)` — the name lookup behind `configure`.
    /// `VERSION_CUSTOM` is deliberately absent, so it can never be selected.
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "gzip" => Some(Self::VERSION_GZIP),
            "deflate" => Some(Self::VERSION_DEFLATE),
            "none" => Some(Self::VERSION_NONE),
            "lz4" => Some(Self::VERSION_LZ4),
            _ => None,
        }
    }

    /// `isValidVersion(int)` — whether `id` names a registered codec.
    pub fn is_valid_version(id: i32) -> bool {
        Self::from_id(id).is_some()
    }

    /// `configure(optionName)` — switch the selected write codec from
    /// `server.properties` `region-file-compression`. An unregistered name
    /// keeps the current selection; Paper's error log routes through the
    /// codebase's `log_and_pause_if_in_ide` error-and-continue seam.
    ///
    /// The "please use one of" list is derived from [`Self::option_names`] in
    /// Rivet's registration order. It is NOT Java's ordering: Paper joins
    /// `VERSIONS_BY_NAME.keySet()`, a fastutil `Object2ObjectOpenHashMap`
    /// whose iteration order is hash-slot order — an implementation artifact
    /// (it spells "lz4, none, deflate, gzip" for the pinned fastutil 8.5.18
    /// default table), not a spec'd format. Rivet keeps its own stable order
    /// and claims no parity on the exact string.
    pub fn configure(option_name: &str) {
        match Self::from_name(option_name) {
            Some(version) => SELECTED_ID.store(version.id, Ordering::Relaxed),
            None => rivet_util::log_and_pause_if_in_ide(&format!(
                "Invalid `region-file-compression` value `{option_name}` in server.properties. Please use one of: {}",
                Self::option_names().join(", ")
            )),
        }
    }

    /// The `option_name`s of every selectable codec, in registration order —
    /// the `server.properties` `region-file-compression` choices.
    fn option_names() -> [&'static str; 4] {
        ["gzip", "deflate", "none", "lz4"]
    }

    /// `getSelected()` — the codec `RegionFile.write` uses for new streams.
    pub fn get_selected() -> Self {
        Self::from_id(SELECTED_ID.load(Ordering::Relaxed)).unwrap_or(Self::DEFAULT)
    }

    /// `getId()` — the `compression_type` byte value.
    pub const fn id(self) -> i32 {
        self.id
    }

    /// The `server.properties` `region-file-compression` value, if this codec
    /// is selectable (`None` for id 127).
    pub const fn option_name(self) -> Option<&'static str> {
        self.option_name
    }

    /// `wrap(InputStream)` — unwrap a chunk stream with this codec (Java's
    /// `StreamWrapper<InputStream>`). gzip/deflate/none are supported; the
    /// unported codecs error (Java's `StreamWrapper.wrap` throws `IOException`).
    pub fn wrap_input<R: Read>(self, input: R) -> io::Result<RegionFileReader<R>> {
        match self.id {
            1 => Ok(RegionFileReader::Gzip(flate2::read::MultiGzDecoder::new(
                input,
            ))),
            2 => Ok(RegionFileReader::Deflate(flate2::read::ZlibDecoder::new(
                input,
            ))),
            3 => Ok(RegionFileReader::Identity(input)),
            4 => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "lz4 read is not ported yet (lz4-java \"LZ4 Block\" format, issue #231)",
            )),
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "custom region-file compression has no stream wrapper",
            )),
        }
    }

    /// `wrap(OutputStream)` — rewrap a chunk stream with this codec (Java's
    /// `StreamWrapper<OutputStream>`). Only `none` and `gzip` are writable:
    /// D13 pins the gate to `none`, and gzip write is proven in `rivet-nbt`;
    /// deflate/lz4 writes and custom error. The returned writer must be
    /// finalized with [`RegionFileWriter::finish`] to emit a complete stream.
    pub fn wrap_output<W: Write>(self, output: W) -> io::Result<RegionFileWriter<W>> {
        match self.id {
            1 => Ok(RegionFileWriter::Gzip(flate2::write::GzEncoder::new(
                output,
                flate2::Compression::default(),
            ))),
            3 => Ok(RegionFileWriter::Identity(output)),
            2 => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "deflate write is deferred: Java `Deflater` output is not `flate2`-reproducible (DECISIONS.md D13)",
            )),
            4 => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "lz4 write is deferred: the lz4-java block compressor is not ported (issue #231)",
            )),
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "custom region-file compression has no stream wrapper",
            )),
        }
    }
}

/// The unwrapped read stream — Java's `StreamWrapper<InputStream>` result.
/// `none` is byte-transparent; gzip/deflate delegate to `flate2` (mirroring
/// `GZIPInputStream`/`InflaterInputStream`).
#[derive(Debug)]
pub enum RegionFileReader<R: Read> {
    /// `FastBufferedInputStream(GZIPInputStream(in))` — reads concatenated
    /// gzip members like Java (the `rivet-nbt` precedent).
    Gzip(flate2::read::MultiGzDecoder<R>),
    /// `FastBufferedInputStream(InflaterInputStream(in))` — zlib-wrapped
    /// deflate (Java's `InflaterInputStream` defaults to `nowrap=false`, the
    /// RFC 1950 zlib framing; NOT raw RFC 1951 deflate).
    Deflate(flate2::read::ZlibDecoder<R>),
    /// `FastBufferedInputStream(in)` — byte-transparent.
    Identity(R),
}

impl<R: Read> Read for RegionFileReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            RegionFileReader::Gzip(r) => r.read(buf),
            RegionFileReader::Deflate(r) => r.read(buf),
            RegionFileReader::Identity(r) => r.read(buf),
        }
    }
}

/// The wrapped write stream — Java's `StreamWrapper<OutputStream>` result.
///
/// The variants are private: the only way to recover the underlying writer is
/// [`RegionFileWriter::finish`], which also finalizes the codec stream. A gzip
/// stream that is `Write::flush`ed but never `finish`ed is not a valid gzip
/// member — the 8-byte trailer is missing; `finish` writes it and surfaces any
/// trailer-write error (dropping the encoder would swallow it silently).
#[derive(Debug)]
pub enum RegionFileWriter<W: Write> {
    /// `BufferedOutputStream(GZIPOutputStream(out))` — `Compression::default()`
    /// matches `GZIPOutputStream`'s default level 6 (the `rivet-nbt` precedent).
    Gzip(flate2::write::GzEncoder<W>),
    /// `BufferedOutputStream(out)` — byte-transparent.
    Identity(W),
}

impl<W: Write> RegionFileWriter<W> {
    /// Finalize the stream and recover the underlying writer.
    ///
    /// gzip: writes the 8-byte trailer, completing the gzip member — the
    /// counterpart of Java closing the `GZIPOutputStream` (which writes the
    /// trailer). `Write::flush` deliberately does *not* write the trailer.
    /// none: returns the identity writer unchanged.
    ///
    /// This is the only way to unwrap the writer. `RegionFile` calls it after
    /// writing a chunk's payload, mirroring `out.close()` on the wrapped
    /// `OutputStream` in `RegionFile.getChunkDataOutputStream`.
    pub fn finish(self) -> io::Result<W> {
        match self {
            RegionFileWriter::Gzip(w) => w.finish(),
            RegionFileWriter::Identity(w) => Ok(w),
        }
    }
}

impl<W: Write> Write for RegionFileWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            RegionFileWriter::Gzip(w) => w.write(buf),
            RegionFileWriter::Identity(w) => w.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        // Flushes compressed data out of the encoder but does NOT write the
        // gzip trailer — only `RegionFileWriter::finish` does.
        match self {
            RegionFileWriter::Gzip(w) => w.flush(),
            RegionFileWriter::Identity(w) => w.flush(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::{Read as _, Write as _};
    use std::rc::Rc;
    use std::sync::Mutex;

    use super::*;

    /// Serializes the `configure`-mutating tests: `SELECTED_ID` is process
    /// global, and the test harness runs tests in parallel.
    static SELECTION_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_is_deflate() {
        let _guard = SELECTION_LOCK.lock().unwrap();
        RegionFileVersion::configure("deflate");
        assert_eq!(
            RegionFileVersion::DEFAULT,
            RegionFileVersion::VERSION_DEFLATE
        );
        assert_eq!(RegionFileVersion::get_selected().id(), 2);
    }

    #[test]
    fn configure_switches_selection() {
        let _guard = SELECTION_LOCK.lock().unwrap();
        RegionFileVersion::configure("gzip");
        assert_eq!(RegionFileVersion::get_selected().id(), 1);
        RegionFileVersion::configure("none");
        assert_eq!(RegionFileVersion::get_selected().id(), 3);
        RegionFileVersion::configure("lz4");
        assert_eq!(RegionFileVersion::get_selected().id(), 4);
        RegionFileVersion::configure("deflate");
        assert_eq!(RegionFileVersion::get_selected().id(), 2);
    }

    #[test]
    fn configure_unknown_name_keeps_selection() {
        let _guard = SELECTION_LOCK.lock().unwrap();
        RegionFileVersion::configure("none");
        let before = RegionFileVersion::get_selected();
        for bad in ["bogus", "", "GZIP", "deflate ", "custom"] {
            RegionFileVersion::configure(bad);
            assert_eq!(RegionFileVersion::get_selected(), before);
        }
    }

    #[test]
    fn custom_is_registered_but_not_selectable() {
        let _guard = SELECTION_LOCK.lock().unwrap();
        assert_eq!(RegionFileVersion::VERSION_CUSTOM.id(), 127);
        assert_eq!(RegionFileVersion::VERSION_CUSTOM.option_name(), None);
        assert_eq!(
            RegionFileVersion::from_id(127),
            Some(RegionFileVersion::VERSION_CUSTOM)
        );
        assert!(RegionFileVersion::is_valid_version(127));
    }

    #[test]
    fn from_id_registry() {
        assert_eq!(
            RegionFileVersion::from_id(1),
            Some(RegionFileVersion::VERSION_GZIP)
        );
        assert_eq!(
            RegionFileVersion::from_id(2),
            Some(RegionFileVersion::VERSION_DEFLATE)
        );
        assert_eq!(
            RegionFileVersion::from_id(3),
            Some(RegionFileVersion::VERSION_NONE)
        );
        assert_eq!(
            RegionFileVersion::from_id(4),
            Some(RegionFileVersion::VERSION_LZ4)
        );
        assert_eq!(
            RegionFileVersion::from_id(127),
            Some(RegionFileVersion::VERSION_CUSTOM)
        );
    }

    #[test]
    fn from_id_unregistered_returns_none() {
        for id in [0, 5, 126, 128, 255, -1, i32::MAX, i32::MIN] {
            assert_eq!(RegionFileVersion::from_id(id), None, "id {id}");
        }
    }

    #[test]
    fn is_valid_version_boundaries() {
        for id in [1, 2, 3, 4, 127] {
            assert!(RegionFileVersion::is_valid_version(id), "id {id}");
        }
        for id in [0, 5, 126, 128, 255, -1] {
            assert!(!RegionFileVersion::is_valid_version(id), "id {id}");
        }
    }

    #[test]
    fn option_names() {
        assert_eq!(RegionFileVersion::VERSION_GZIP.option_name(), Some("gzip"));
        assert_eq!(
            RegionFileVersion::VERSION_DEFLATE.option_name(),
            Some("deflate")
        );
        assert_eq!(RegionFileVersion::VERSION_NONE.option_name(), Some("none"));
        assert_eq!(RegionFileVersion::VERSION_LZ4.option_name(), Some("lz4"));
        assert_eq!(RegionFileVersion::VERSION_CUSTOM.option_name(), None);
    }

    #[test]
    fn ids() {
        assert_eq!(RegionFileVersion::VERSION_GZIP.id(), 1);
        assert_eq!(RegionFileVersion::VERSION_DEFLATE.id(), 2);
        assert_eq!(RegionFileVersion::VERSION_NONE.id(), 3);
        assert_eq!(RegionFileVersion::VERSION_LZ4.id(), 4);
        assert_eq!(RegionFileVersion::VERSION_CUSTOM.id(), 127);
    }

    #[test]
    fn none_wrap_round_trips_bytes_identically() {
        let mut writer = RegionFileVersion::VERSION_NONE
            .wrap_output(Vec::new())
            .unwrap();
        writer.write_all(b"chunk payload").unwrap();
        let bytes = writer.finish().unwrap();
        assert_eq!(bytes, b"chunk payload");
        let mut reader = RegionFileVersion::VERSION_NONE
            .wrap_input(bytes.as_slice())
            .unwrap();
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn gzip_wrap_finish_round_trips() {
        let mut writer = RegionFileVersion::VERSION_GZIP
            .wrap_output(Vec::new())
            .unwrap();
        writer.write_all(b"hello hello hello").unwrap();
        // `finish` is the only way to unwrap the writer, and it is what writes
        // the gzip trailer — the round-trip below only works on a *finished*
        // member.
        let compressed = writer.finish().unwrap();
        // A finished gzip member: the RFC 1952 magic header (10 bytes) at the
        // front and the 8-byte trailer (CRC32 + ISIZE) at the back. gzip is
        // lossless — the round-trip below restores the payload exactly; the
        // length bound only confirms the member is complete (header + trailer),
        // since tiny payloads can still *expand* under gzip's fixed overhead.
        assert_eq!(&compressed[..2], &[0x1f, 0x8b]);
        assert!(
            compressed.len() >= 18,
            "must fit header + deflate + trailer"
        );
        assert_ne!(compressed, b"hello hello hello");
        let mut reader = RegionFileVersion::VERSION_GZIP
            .wrap_input(compressed.as_slice())
            .unwrap();
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"hello hello hello");
    }

    /// A `Write` sink that keeps the produced bytes reachable after the writer
    /// is dropped — the only way to observe a `RegionFileWriter` that was
    /// abandoned without `finish`.
    #[derive(Debug, Clone)]
    struct SharedSink(Rc<RefCell<Vec<u8>>>);

    impl Write for SharedSink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn gzip_drop_without_finish_still_finalizes() {
        // The not-finalized path must not silently lose the trailer: dropping
        // the writer without `finish` still writes the gzip trailer (flate2's
        // encoder finalizes on drop, best-effort). `finish` is what surfaces
        // trailer errors; drop swallows them but never omits the trailer.
        let sink = SharedSink(Rc::new(RefCell::new(Vec::new())));
        let mut writer = RegionFileVersion::VERSION_GZIP
            .wrap_output(SharedSink(Rc::clone(&sink.0)))
            .unwrap();
        writer.write_all(b"hello hello hello").unwrap();
        writer.flush().unwrap();
        drop(writer);
        let bytes = sink.0.borrow();
        assert_eq!(&bytes[..2], &[0x1f, 0x8b], "gzip magic");
        assert!(
            bytes.len() >= 18,
            "drop must still finalize header + deflate + trailer"
        );
        // Trailer tail: ISIZE, the uncompressed length mod 2^32, little-endian.
        assert_eq!(
            &bytes[bytes.len() - 4..],
            &(b"hello hello hello".len() as u32).to_le_bytes(),
            "ISIZE must be written even on the drop path"
        );
    }

    #[test]
    fn gzip_flush_without_finish_is_not_a_valid_member() {
        // The not-finalized failure mode: a stream that is written and flushed
        // but never `finish`ed lacks the 8-byte trailer, so the reader rejects
        // it. The unfinished stream is built with flate2's `get_ref()` snapshot
        // (header + deflate, no trailer) because the unfinished state is not
        // observable through `RegionFileWriter`'s public API — the encoder's
        // `Drop` would finalize it, silently swallowing the trailer write.
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"hello hello hello").unwrap();
        encoder.flush().unwrap();
        let unfinished = encoder.get_ref().clone();
        let mut reader = RegionFileVersion::VERSION_GZIP
            .wrap_input(unfinished.as_slice())
            .unwrap();
        assert!(
            reader.read_to_end(&mut Vec::new()).is_err(),
            "a trailer-less gzip member must be rejected"
        );
    }

    #[test]
    fn deflate_wrap_reads_java_zlib_stream() {
        // The write side is deferred (D13), so the reader is exercised against
        // Java/Paper-compatible bytes: Java's `DeflaterOutputStream` (default
        // `nowrap=false`) writes RFC 1950 zlib-wrapped deflate, which flate2's
        // `ZlibEncoder` produces byte-for-byte compatibly (a `0x78` CMF header
        // byte — deflate with a 32K window — and an adler32 trailer).
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"deflate payload").unwrap();
        let compressed = encoder.finish().unwrap();
        assert_eq!(
            compressed[0], 0x78,
            "must be a zlib header, not raw deflate"
        );
        let mut reader = RegionFileVersion::VERSION_DEFLATE
            .wrap_input(compressed.as_slice())
            .unwrap();
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"deflate payload");
    }

    #[test]
    fn deflate_wrap_rejects_raw_deflate_stream() {
        // Counterfactual proving the raw-vs-zlib distinction: the id-2 reader
        // must NOT accept raw (RFC 1951) deflate — Java's `InflaterInputStream`
        // expects zlib framing and fails on raw streams. This guards against
        // regressing the reader back to flate2's raw `DeflateDecoder`.
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"deflate payload").unwrap();
        let raw = encoder.finish().unwrap();
        assert_ne!(raw[0], 0x78, "raw deflate has no zlib CMF header");
        let mut reader = RegionFileVersion::VERSION_DEFLATE
            .wrap_input(raw.as_slice())
            .unwrap();
        assert!(
            reader.read_to_end(&mut Vec::new()).is_err(),
            "raw deflate must not decode as zlib-wrapped deflate"
        );
    }

    #[test]
    fn wrap_input_rejects_unported_codecs() {
        for version in [
            RegionFileVersion::VERSION_LZ4,
            RegionFileVersion::VERSION_CUSTOM,
        ] {
            let err = version.wrap_input(std::io::empty()).unwrap_err();
            assert_eq!(
                err.kind(),
                std::io::ErrorKind::Unsupported,
                "codec {version:?}"
            );
        }
        for version in [
            RegionFileVersion::VERSION_GZIP,
            RegionFileVersion::VERSION_DEFLATE,
            RegionFileVersion::VERSION_NONE,
        ] {
            assert!(
                version.wrap_input(std::io::empty()).is_ok(),
                "codec {version:?}"
            );
        }
    }

    #[test]
    fn wrap_output_rejects_deferred_and_custom_codecs() {
        for version in [
            RegionFileVersion::VERSION_DEFLATE,
            RegionFileVersion::VERSION_LZ4,
            RegionFileVersion::VERSION_CUSTOM,
        ] {
            let err = version.wrap_output(std::io::sink()).unwrap_err();
            assert_eq!(
                err.kind(),
                std::io::ErrorKind::Unsupported,
                "codec {version:?}"
            );
        }
        for version in [
            RegionFileVersion::VERSION_GZIP,
            RegionFileVersion::VERSION_NONE,
        ] {
            assert!(
                version.wrap_output(std::io::sink()).is_ok(),
                "codec {version:?}"
            );
        }
    }
}
