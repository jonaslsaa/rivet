//! Port of `net.minecraft.world.level.chunk.storage.RegionFile` (MC 26.2).
//!
//! The file-backed chunk container: sector/header management, the
//! read/write/clear path, `ChunkBuffer` semantics, `padToFullSector`, external
//! `.mcc` payloads, exact compression dispatch, corruption/failure handling,
//! and Paper's header-recalculation behavior. It sits on the `RegionBitmap`
//! (§3 of `docs/region-file-format-spec.md`) and `RegionFileVersion` (§5)
//! primitives ported in the storage foundation wave.
//!
//! Faithfulness notes (all grounded in `RegionFile.java`, the source of truth):
//!
//! - **Wrapping/unsigned behavior**: the 4-byte length field counts the
//!   compression byte (`streamLength + 1`); `ChunkBuffer.close` patches it as
//!   `count - 5 + 1`. Header ints are big-endian. `getSectorNumber` masks to
//!   24 bits (`offset >> 8 & 0xFFFFFF`); sector count is the low byte.
//! - **Spigot 255 sentinel**: a header count of `255` is "maxed out" — the
//!   reader recomputes `sectors = (length + 4)/4096 + 1` from the chunk's own
//!   first sector, reading that length *without* checking the read count
//!   (short reads leave the zero-filled tail), exactly like Paper.
//! - **Codec dispatch**: gzip/deflate/none reads ride on `RegionFileVersion`
//!   (`flate2`); lz4 read is the D13 deferral and errors
//!   ([`io::ErrorKind::Unsupported`]); id 127 reads a modified-UTF-8 id, logs
//!   "Unrecognized custom compression {}" / "Invalid custom compression id {}",
//!   and returns null — malformed/truncated ids propagate the UTF/EOF error
//!   out of `get_chunk_data_input_stream`, exactly like Paper.
//! - **External `.mcc`**: `sectors_needed >= 256` redirects to
//!   `c.<x>.<z>.mcc` via a `tmp...` temp file moved over the target
//!   (REPLACE_EXISTING) as the commit op, with a 5-byte stub
//!   `[00 00 00 01][id | 0x80]` in the region. Write order is allocate → stub
//!   /external → header → commit → free old sectors last.
//! - **Recalc never writes the header to disk** (§8 step 7): `recalculate_header`
//!   repairs the in-memory header buffer and ends with `flush` + `force` only.
//! - **`RegionFileSizeException`**: the `MAX_CHUNK_SIZE` guard in `ChunkBuffer`
//!   (Paper's "don't write garbage data to disk"); the caller converts it to a
//!   `clear`.
//!
//! Deferred with `RivetTodo(#231)` markers: the legacy Aikar oversized
//! subsystem (`*.oversized_<x>_<z>.nbt` per-chunk files, `.oversized.nbt` meta,
//! `isOversized`/`setOversized`, and the recalc branches that detect Aikar
//! files) — legacy-only, nothing creates the files anymore, and its consumer
//! (`RegionFileStorage.readOversizedChunk`) is itself deferred. The recalc's
//! modern `.mcc` re-linking is fully ported. lz4 read stays deferred per D13.
//! `RegionFileStorage` (the LRU/negative-cache wrapper) is deferred with
//! evidence in `mod.rs`.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_io;
use rivet_nbt::nbt_utils::get_data_version;
use rivet_registry::Identifier;
use rivet_registry::core::ChunkPos;
use rivet_util::data_io::{DataInput, DataInputStream};
use rivet_util::random::random_support::generate_unique_seed;
use rivet_util::random::{LegacyRandomSource, RandomSource};
use tempfile::TempPath;

use super::region_bitmap::RegionBitmap;
use super::region_file_version::{RegionFileReader, RegionFileVersion, RegionFileWriter};
use super::region_storage_info::RegionStorageInfo;

/// `MAX_CHUNK_SIZE` — Paper's in-memory chunk-buffer cap; exceeding it throws
/// `RegionFileSizeException` (the chunk is dropped, never partially written).
pub const MAX_CHUNK_SIZE: usize = 500 * 1024 * 1024;
/// `CHUNK_HEADER_SIZE` — the 5-byte per-chunk prefix
/// `[4-byte length][compression byte]`.
const CHUNK_HEADER_SIZE: usize = 5;
/// `EXTERNAL_FILE_EXTENSION` — `.mcc` oversized files.
const EXTERNAL_FILE_EXTENSION: &str = ".mcc";
/// `EXTERNAL_STREAM_FLAG` — the high bit of the compression byte marking an
/// external (.mcc) stream.
const EXTERNAL_STREAM_FLAG: u8 = 128;
/// `EXTERNAL_CHUNK_THRESHOLD` — `sectors_needed >= 256` triggers the external
/// redirect (the sector count would not fit the header low byte).
const EXTERNAL_CHUNK_THRESHOLD: i32 = 256;
/// The five registered codecs in registration order (Java `VERSIONS.values()`),
/// for the recalc's "try every codec" `.mcc` read.
const ALL_REGISTERED_VERSIONS: [RegionFileVersion; 5] = [
    RegionFileVersion::VERSION_GZIP,
    RegionFileVersion::VERSION_DEFLATE,
    RegionFileVersion::VERSION_NONE,
    RegionFileVersion::VERSION_LZ4,
    RegionFileVersion::VERSION_CUSTOM,
];

/// `RegionFileStorage.RegionFileSizeException` — Paper's "don't write garbage
/// data to disk" guard. Java nests this under `RegionFileStorage`; the
/// consumer (`RegionFileStorage.write`) is deferred, so the type lives here
/// with the `ChunkBuffer` that throws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionFileSizeException {
    /// The buffer size that exceeded `MAX_CHUNK_SIZE`.
    pub count: usize,
}

impl std::fmt::Display for RegionFileSizeException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Region file too large: {}", self.count)
    }
}

impl std::error::Error for RegionFileSizeException {}

impl From<RegionFileSizeException> for io::Error {
    fn from(e: RegionFileSizeException) -> io::Error {
        io::Error::other(e)
    }
}

/// `RegionFile.ChunkBuffer` — the in-memory accumulator for a chunk stream.
///
/// Starts with the 5-byte prefix `[0, 0, 0, 0, versionId]`; `close()` patches
/// the 4-byte length field (`streamLength = count - 5 + 1`) and, unless the
/// moonrise write-on-close seam disabled it, hands the record to
/// `RegionFile.write`. The `MAX_CHUNK_SIZE` guard throws
/// `RegionFileSizeException` so a serialization blow-up never reaches disk.
#[derive(Debug)]
pub struct ChunkBuffer {
    pos: ChunkPos,
    buf: Vec<u8>,
    write_on_close: bool,
}

impl ChunkBuffer {
    /// `new ChunkBuffer(pos)` — a fresh buffer with the 5-byte prefix written.
    /// Java's initial capacity is 8096 (`super(8096)`).
    pub fn new(pos: ChunkPos, version_id: u8) -> Self {
        let mut buf = Vec::with_capacity(8096);
        buf.extend_from_slice(&[0, 0, 0, 0, version_id]);
        Self {
            pos,
            buf,
            write_on_close: true,
        }
    }

    /// `moonrise$getWriteOnClose` — the moonrise seam that splits
    /// serialization from the write.
    pub fn get_write_on_close(&self) -> bool {
        self.write_on_close
    }

    /// `moonrise$setWriteOnClose` — `false` when the write is deferred to a
    /// separate call.
    pub fn set_write_on_close(&mut self, value: bool) {
        self.write_on_close = value;
    }

    /// `close()` — patch the length field, then write the record unless the
    /// write-on-close seam disabled it. `result.putInt(0, streamLength)` with
    /// `streamLength = count - 5 + 1`; the `JvmProfiler` call is deferred.
    pub fn close(&mut self, region: &mut RegionFile) -> io::Result<()> {
        let stream_length = (self.buf.len() - CHUNK_HEADER_SIZE + 1) as i32;
        self.buf[0..4].copy_from_slice(&stream_length.to_be_bytes());
        if self.write_on_close {
            region.write(&self.pos, &self.buf)?;
        }
        Ok(())
    }
}

impl Write for ChunkBuffer {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        // Paper: `write(byte[], off, len)` checks `count + len > MAX_CHUNK_SIZE`.
        if self.buf.len() + b.len() > MAX_CHUNK_SIZE {
            return Err(RegionFileSizeException {
                count: self.buf.len() + b.len(),
            }
            .into());
        }
        self.buf.extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The write-commit op (Java's private `CommitOp` interface): run *after* the
/// header is written, before the old sectors are freed. `MoveExternal` is the
/// temp→target move (Java `Files.move(tmp, path, REPLACE_EXISTING)`);
/// `DeleteExternal` removes a stale `.mcc` on the internal-write path.
enum CommitOp {
    MoveExternal { tmp: TempPath, target: PathBuf },
    DeleteExternal(PathBuf),
}

impl CommitOp {
    fn run(self) -> io::Result<()> {
        match self {
            CommitOp::MoveExternal { tmp, target } => {
                // Java uses REPLACE_EXISTING; on Unix `rename` is atomic and
                // replaces. The TempPath's drop-side delete is a no-op after
                // the move (the old path no longer exists).
                fs::rename(&tmp, &target)
            }
            CommitOp::DeleteExternal(p) => match fs::remove_file(&p) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            },
        }
    }
}

/// The result of `attemptRead` in the header-recalc scan. Java models the
/// `OVERSIZED_COMPOUND` marker as a *shared singleton* compared by identity; a
/// real empty compound is never equal to it, so the port uses a distinct
/// variant rather than an empty-`CompoundTag` equality test.
enum ScanRead {
    /// `OVERSIZED_COMPOUND` — the compression byte's external bit is set.
    Oversized,
    /// A successfully parsed chunk compound.
    Compound(CompoundTag),
    /// Any failure / no data.
    None,
}

/// `net.minecraft.world.level.chunk.storage.RegionFile` — the file-backed
/// chunk container.
///
/// Owned by the region's single IO worker behind a `Mutex<RegionFile>`
/// (OWNERSHIP.md storage-worker amendment); never shares game state. `info`
/// itself is not stored: Java only reads it for `JvmProfiler` (deferred) and
/// `canRecalcHeader` (extracted here from `info.is_chunk_data`).
pub struct RegionFile {
    path: PathBuf,
    external_file_dir: PathBuf,
    /// The live file handle — `None` after `close()`. Accessing it after close
    /// panics, mirroring Java's `ClosedChannelException` on a closed
    /// `FileChannel`.
    file: Option<fs::File>,
    version: RegionFileVersion,
    /// The 8192-byte header buffer — the source of truth for offsets and
    /// timestamps, byte-for-byte the Java `ByteBuffer.allocateDirect(8192)`.
    header: [u8; 8192],
    /// Per-file sector allocation (`usedSectors`), loaded from the header at
    /// open time and repaired by recalc.
    used_sectors: RegionBitmap,
    /// `canRecalcHeader` — `info.dfuType()[0] == CHUNK` (flattened
    /// `is_chunk_data`).
    can_recalc_header: bool,
    /// `recalculateCount` — Paper's AtomicInteger; a plain counter is fine
    /// because the file is behind the region's IO mutex.
    recalculate_count: u64,
    /// `sync` — Java opens the FileChannel with `DSYNC` when set. Rivet
    /// emulates it with a `sync_data` after each `write_header`.
    sync: bool,
}

impl RegionFile {
    /// The live file handle, or a panic after `close()` — the Rust analogue of
    /// Java's `ClosedChannelException` on a closed `FileChannel`.
    fn file_mut(&mut self) -> &mut fs::File {
        self.file.as_mut().expect("RegionFile accessed after close")
    }

    /// The full constructor — `RegionFile(info, path, externalFileDir, version, sync)`.
    ///
    /// Validates the external-file directory, opens the region file
    /// (CREATE+READ+WRITE), reserves the header sectors, replays the header's
    /// locations into the bitmap, and triggers `recalculate_header` when the
    /// replay finds an invalid/overlapping entry.
    pub fn open(
        info: RegionStorageInfo,
        path: PathBuf,
        external_file_dir: PathBuf,
        version: RegionFileVersion,
        sync: bool,
    ) -> io::Result<Self> {
        if !external_file_dir.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Expected directory, got {}", external_file_dir.display()),
            ));
        }
        // `FileChannel.open(path, CREATE, READ, WRITE)` — deliberately no
        // TRUNCATE_EXISTING: a re-open must read the existing header (Paper
        // `RegionFile` constructor), and the header is only written by
        // `write_header`/`recalculate_header`.
        #[allow(clippy::suspicious_open_options)]
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)?;
        let can_recalc_header = info.is_chunk_data;
        let mut region = Self {
            path,
            external_file_dir,
            file: Some(file),
            version,
            header: [0u8; 8192],
            used_sectors: RegionBitmap::new(),
            can_recalc_header,
            recalculate_count: 0,
            sync,
        };
        region.used_sectors.force(0, 2);

        // Read the 8192-byte header at position 0. Rust `read` returns 0 at
        // EOF where Java's FileChannel returns -1 — both mean "no header".
        let mut buf = [0u8; 8192];
        region.file_mut().seek(SeekFrom::Start(0))?;
        let read_header_bytes = region.file_mut().read(&mut buf)?;
        if read_header_bytes != 0 {
            if read_header_bytes != 8192 {
                eprintln!(
                    "Region file {} has truncated header: {}",
                    region.path.display(),
                    read_header_bytes
                );
            }
            region.header[..read_header_bytes].copy_from_slice(&buf[..read_header_bytes]);

            let size = region.file_mut().metadata()?.len();
            let mut needs_header_recalc = false;
            let mut has_backed_up = false;
            for i in 0..1024 {
                let offset = region.read_offset(i);
                if offset != 0 {
                    let sector_number = get_sector_number(offset);
                    let mut num_sectors = get_num_sectors(offset);
                    if num_sectors == 255 {
                        // Spigot sentinel: read the real length from the chunk's
                        // own first sector (unchecked read, zero-filled tail).
                        // Java `(realLen.getInt(0) + 4) / 4096 + 1` wraps on int
                        // overflow; a corrupt length must not panic.
                        num_sectors = (region
                            .read_length_unchecked(sector_number as u64)?
                            .wrapping_add(4))
                            / 4096
                            + 1;
                    }
                    if sector_number < 2 || num_sectors <= 0 || (sector_number as u64) * 4096 > size
                    {
                        if region.can_recalc_header {
                            eprintln!(
                                "Detected invalid header for regionfile {}! Recalculating header...",
                                region.path.display()
                            );
                            needs_header_recalc = true;
                            break;
                        } else {
                            eprintln!(
                                "Detected invalid header for regionfile {}! Cannot recalculate, removing local chunk ({},{}) from header",
                                region.path.display(),
                                i & 31,
                                i >> 5
                            );
                            if !has_backed_up {
                                has_backed_up = true;
                                region.backup_region_file();
                            }
                            region.write_timestamp(i, 0);
                            region.write_offset(i, 0);
                            continue;
                        }
                    }
                    let failed_to_allocate =
                        !region.used_sectors.try_allocate(sector_number, num_sectors);
                    if failed_to_allocate {
                        eprintln!(
                            "Overlapping allocation by local chunk ({},{}) in regionfile {}",
                            i & 31,
                            i >> 5,
                            region.path.display()
                        );
                    }
                    if failed_to_allocate && !region.can_recalc_header {
                        eprintln!(
                            "Detected invalid header for regionfile {}! Cannot recalculate, removing local chunk ({},{}) from header",
                            region.path.display(),
                            i & 31,
                            i >> 5
                        );
                        if !has_backed_up {
                            has_backed_up = true;
                            region.backup_region_file();
                        }
                        region.write_timestamp(i, 0);
                        region.write_offset(i, 0);
                        continue;
                    }
                    needs_header_recalc |= failed_to_allocate;
                }
            }
            if needs_header_recalc {
                eprintln!(
                    "Recalculating regionfile {}, header gave erroneous offsets & locations",
                    region.path.display()
                );
                region.recalculate_header()?;
            }
        }
        Ok(region)
    }

    /// `getPath()`.
    pub fn get_path(&self) -> &Path {
        &self.path
    }

    /// `getRecalculateCount()` — incremented on every `recalculate_header`;
    /// the moonrise read path compares it to detect a recalc during a read.
    pub fn get_recalculate_count(&self) -> u64 {
        self.recalculate_count
    }

    /// `hasChunk(pos)` — location nonzero.
    pub fn has_chunk(&self, pos: &ChunkPos) -> bool {
        self.get_offset(pos) != 0
    }

    /// `doesChunkExist(pos)` — the location is nonzero *and* the stream header
    /// validates as a real stream of a registered codec (external streams need
    /// the `.mcc` file to exist). IO failures report + return false like Paper.
    pub fn does_chunk_exist(&mut self, pos: &ChunkPos) -> bool {
        let offset = self.get_offset(pos);
        if offset == 0 {
            return false;
        }
        let sector_number = get_sector_number(offset);
        let num_sectors = get_num_sectors(offset);
        let mut stream_header = [0u8; 5];
        if self
            .seek_read_exact(sector_number as u64 * 4096, &mut stream_header)
            .is_err()
        {
            return false;
        }
        let length = i32::from_be_bytes(stream_header[0..4].try_into().unwrap());
        let version_id = stream_header[4];
        if is_external_stream_chunk(version_id) {
            if !RegionFileVersion::is_valid_version(get_external_chunk_version(version_id)) {
                return false;
            }
            self.get_external_chunk_path(pos).is_file()
        } else {
            if !RegionFileVersion::is_valid_version(version_id as i32) {
                return false;
            }
            if length == 0 {
                return false;
            }
            // Java `int streamLength = length - 1` wraps; a corrupt length must
            // follow the negative/oversize corruption path, not panic.
            let stream_length = length.wrapping_sub(1);
            stream_length >= 0 && (stream_length as u64) <= 4096 * num_sectors as u64
        }
    }

    /// `getChunkDataInputStream(pos)` — the §7 read path with all corruption
    /// checks and the Paper recalc-and-retry recursion (unbounded by
    /// construction, exactly like Paper: recalc returns true for any CHUNK
    /// file whose name parses).
    ///
    /// Returns `None` when the chunk is absent or the stream is corrupt; a
    /// successful read hands back the codec-unwrapped payload, which the caller
    /// wraps in a `DataInputStream` and parses as NBT.
    pub fn get_chunk_data_input_stream(
        &mut self,
        pos: &ChunkPos,
    ) -> io::Result<Option<RegionFileReader<std::io::Cursor<Vec<u8>>>>> {
        loop {
            let offset = self.get_offset(pos);
            if offset == 0 {
                return Ok(None);
            }
            let sector_number = get_sector_number(offset);
            let mut num_sectors = get_num_sectors(offset);
            if num_sectors == 255 {
                num_sectors = (self
                    .read_length_unchecked(sector_number as u64)?
                    .wrapping_add(4))
                    / 4096
                    + 1;
            }
            if num_sectors < 0 {
                // A corrupt Spigot-sentinel length wraps to a negative sector
                // count. Java's `ByteBuffer.allocate(numSectors * 4096)` throws
                // an `IllegalArgumentException` (unchecked, a server crash); per
                // this port's corruption convention the chunk is treated as
                // absent rather than panicking on the allocation size.
                eprintln!("Chunk {} has a negative sector count {}", pos, num_sectors);
                if self.can_recalc_header && self.recalculate_header()? {
                    continue;
                }
                return Ok(None);
            }
            let sectors_length = (num_sectors as u64) * 4096;
            let mut buffer = vec![0u8; sectors_length as usize];
            self.file_mut()
                .seek(SeekFrom::Start(sector_number as u64 * 4096))?;
            // Java reads into the ByteBuffer and relies on `remaining()` after
            // flip — i.e. the actual bytes read — for the truncation checks.
            let remaining = self.file_mut().read(&mut buffer)?;

            if remaining < 5 {
                eprintln!(
                    "Chunk {} header is truncated: expected {} but read {}",
                    pos, sectors_length, remaining
                );
                if self.can_recalc_header && self.recalculate_header()? {
                    continue;
                }
                return Ok(None);
            }
            let length = i32::from_be_bytes(buffer[0..4].try_into().unwrap());
            let version_id = buffer[4];
            if length == 0 {
                eprintln!("Chunk {} is allocated, but stream is missing", pos);
                if self.can_recalc_header && self.recalculate_header()? {
                    continue;
                }
                return Ok(None);
            }
            // Java `int streamLength = length - 1` wraps; a corrupt length must
            // follow the negative/truncated corruption path, not panic.
            let stream_length = length.wrapping_sub(1);
            if is_external_stream_chunk(version_id) {
                if stream_length != 0 {
                    // "has both internal and external streams" — a warning that
                    // still falls through to read the external file.
                    eprintln!("Chunk has both internal and external streams");
                    if self.can_recalc_header && self.recalculate_header()? {
                        continue;
                    }
                }
                let ret = self.create_external_chunk_input_stream(
                    pos,
                    get_external_chunk_version(version_id),
                )?;
                if ret.is_none() && self.can_recalc_header && self.recalculate_header()? {
                    continue;
                }
                return Ok(ret);
            } else if stream_length > (remaining - 5) as i32 {
                eprintln!(
                    "Chunk {} stream is truncated: expected {} but read {}",
                    pos,
                    stream_length,
                    remaining - 5
                );
                if self.can_recalc_header && self.recalculate_header()? {
                    continue;
                }
                return Ok(None);
            } else if stream_length < 0 {
                eprintln!("Declared size {} of chunk {} is negative", length, pos);
                if self.can_recalc_header && self.recalculate_header()? {
                    continue;
                }
                return Ok(None);
            } else {
                let payload = buffer[5..5 + stream_length as usize].to_vec();
                let ret = self.create_chunk_input_stream(pos, version_id as i32, payload)?;
                if ret.is_none() && self.can_recalc_header && self.recalculate_header()? {
                    continue;
                }
                return Ok(ret);
            }
        }
    }

    /// `createChunkInputStream(pos, versionId, chunkStream)` — dispatch on the
    /// compression id. id 127 reads a modified-UTF-8 id, logs, and returns
    /// null (malformed/truncated ids propagate the UTF/EOF error); an
    /// unregistered id logs and returns null; 1-4 unwrap the codec.
    fn create_chunk_input_stream(
        &self,
        pos: &ChunkPos,
        version_id: i32,
        chunk_stream: Vec<u8>,
    ) -> io::Result<Option<RegionFileReader<std::io::Cursor<Vec<u8>>>>> {
        if version_id == 127 {
            let mut din = DataInputStream::new(std::io::Cursor::new(chunk_stream));
            let ty = din.read_utf()?;
            if let Some(id) = Identifier::try_parse(&ty) {
                eprintln!("Unrecognized custom compression {}", id);
            } else {
                eprintln!("Invalid custom compression id {}", ty);
            }
            Ok(None)
        } else if let Some(version) = RegionFileVersion::from_id(version_id) {
            // lz4 (id 4) returns io::ErrorKind::Unsupported here — the D13
            // read deferral; Java reads lz4 fine.
            let reader = version.wrap_input(std::io::Cursor::new(chunk_stream))?;
            Ok(Some(reader))
        } else {
            eprintln!(
                "Chunk {} has invalid chunk stream version {}",
                pos, version_id
            );
            Ok(None)
        }
    }

    /// `createExternalChunkInputStream` — the `.mcc` payload (codec-wrapped
    /// NBT, no length/compression byte) unwrapped with the stub's codec id.
    fn create_external_chunk_input_stream(
        &self,
        pos: &ChunkPos,
        version_id: i32,
    ) -> io::Result<Option<RegionFileReader<std::io::Cursor<Vec<u8>>>>> {
        let external_file = self.get_external_chunk_path(pos);
        if !external_file.is_file() {
            eprintln!(
                "External chunk path {} is not file",
                external_file.display()
            );
            Ok(None)
        } else {
            let bytes = fs::read(&external_file)?;
            self.create_chunk_input_stream(pos, version_id, bytes)
        }
    }

    /// `getChunkDataOutputStream(pos)` — `new DataOutputStream(version.wrap(new
    /// ChunkBuffer(pos)))`. The caller writes NBT through the returned writer,
    /// finalizes it with `RegionFileWriter::finish` (writing the codec trailer),
    /// and calls `ChunkBuffer::close` to patch the length and hand the record
    /// to `RegionFile.write`.
    pub fn get_chunk_data_output_stream(
        &self,
        pos: &ChunkPos,
    ) -> io::Result<RegionFileWriter<ChunkBuffer>> {
        let buffer = ChunkBuffer::new(*pos, self.version.id() as u8);
        self.version.wrap_output(buffer)
    }

    /// `flush()` — `file.force(true)` (fsync including metadata).
    pub fn flush(&mut self) -> io::Result<()> {
        self.file_mut().sync_all()
    }

    /// `clear(pos)` — zero the location, write a fresh timestamp, rewrite the
    /// header, delete the `.mcc` file if present, then free the old sectors.
    /// Freed sectors are not zeroed and the file is not truncated.
    pub fn clear(&mut self, pos: &ChunkPos) -> io::Result<()> {
        let offset_index = Self::get_offset_index(pos);
        let offset = self.read_offset(offset_index);
        if offset != 0 {
            self.write_offset(offset_index, 0);
            self.write_timestamp(offset_index, get_timestamp());
            self.write_header()?;
            match fs::remove_file(self.get_external_chunk_path(pos)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            self.used_sectors
                .free(get_sector_number(offset), get_num_sectors(offset));
        }
        Ok(())
    }

    /// `close()` — Paper's nested `finally` order: `padToFullSector()` in the
    /// try, then `force(true)` in the finally, then the `FileChannel.close()`.
    /// The innermost close always runs, so the descriptor is released even if
    /// padding or forcing failed. Per JLS 14.20.2 a `try` that completes
    /// abruptly and whose `finally` also completes abruptly propagates the
    /// `finally`'s reason, so when both pad and force fail the **force** error
    /// is reported and the pad error discarded — the `force_result.and(...)`
    /// below mirrors that ordering; the close itself is a `drop` and cannot
    /// fail.
    pub fn close(&mut self) -> io::Result<()> {
        let pad_result = self.pad_to_full_sector();
        let force_result = self.file_mut().sync_all();
        self.file.take(); // FileChannel.close() — releases the descriptor
        force_result.and(pad_result)
    }

    /// `write(pos, data)` — the §10 write path. `data` is a full ChunkBuffer
    /// record (5-byte prefix + codec-wrapped payload). Sectors are allocated
    /// (oversized → 1 + `.mcc` + stub, else `ceil/4096`), data written, the
    /// location+timestamp patched, header written, the commit op run, and the
    /// old sectors freed *last*.
    pub(crate) fn write(&mut self, pos: &ChunkPos, data: &[u8]) -> io::Result<()> {
        let offset_index = Self::get_offset_index(pos);
        let offset = self.read_offset(offset_index);
        let sector_number = get_sector_number(offset);
        let current_sector_count = get_num_sectors(offset);
        let data_size = data.len();
        let mut sectors_needed = size_to_sectors(data_size as i32);
        let new_sector_number;
        let commit_op;
        if sectors_needed >= EXTERNAL_CHUNK_THRESHOLD {
            let external_chunk_path = self.get_external_chunk_path(pos);
            eprintln!(
                "Saving oversized chunk {} ({} bytes) to external file {}",
                pos,
                data_size,
                external_chunk_path.display()
            );
            sectors_needed = 1;
            new_sector_number = self.used_sectors.allocate(sectors_needed);
            // Java `writeToExternalFile` sets data.position(5) — the external
            // file holds exactly the payload after the 5-byte prefix.
            let tmp = self.write_to_external_file(&data[CHUNK_HEADER_SIZE..])?;
            let stub = self.create_external_stub(self.version);
            self.write_at(new_sector_number as u64 * 4096, &stub)?;
            commit_op = CommitOp::MoveExternal {
                tmp,
                target: external_chunk_path,
            };
        } else {
            new_sector_number = self.used_sectors.allocate(sectors_needed);
            commit_op = CommitOp::DeleteExternal(self.get_external_chunk_path(pos));
            self.write_at(new_sector_number as u64 * 4096, data)?;
        }
        self.write_offset(
            offset_index,
            Self::pack_sector_offset(new_sector_number, sectors_needed),
        );
        self.write_timestamp(offset_index, get_timestamp());
        self.write_header()?;
        commit_op.run()?;
        if sector_number != 0 {
            self.used_sectors.free(sector_number, current_sector_count);
        }
        Ok(())
    }

    /// `recalculateHeader()` — §8 tier 2: the full header repair. Backs up the
    /// file, scans every sector for valid chunk streams, re-derives the slots
    /// (newest `LastUpdate` wins), re-links `.mcc` externals by trying every
    /// registered codec, computes fresh offsets on a fresh bitmap, rewrites
    /// timestamps — and **never writes the header back to disk** (it ends with
    /// `flush` + `force` only).
    ///
    /// Returns `Ok(false)` for non-CHUNK files or unparseable filenames (like
    /// Paper); file IO failures propagate as `io::Error` (Java `throws
    /// IOException`). The Aikar oversized branches are `RivetTodo(#231)`.
    pub fn recalculate_header(&mut self) -> io::Result<bool> {
        if !self.can_recalc_header {
            return Ok(false);
        }
        let Some(our_lower_left_position) = get_region_file_coordinates(&self.path) else {
            eprintln!(
                "Unable to get chunk location of regionfile {}, cannot recover header",
                self.path.display()
            );
            return Ok(false);
        };

        self.recalculate_count += 1;

        eprintln!(
            "Corrupt regionfile header detected! Attempting to re-calculate header offsets for regionfile {}",
            self.path.display()
        );
        self.backup_region_file();

        let mut compounds: [Option<CompoundTag>; 1024] = std::array::from_fn(|_| None);
        let mut raw_lengths = [0i32; 1024];
        let mut sector_offsets = [0i32; 1024];
        // `hasAikarOversized` (the legacy Aikar per-chunk detection) is
        // RivetTodo(#231): every slot is treated as not-Aikar-oversized, which
        // is exactly the modern-world outcome.

        let file_length = self.file_mut().metadata().map(|m| m.len()).unwrap_or(0);
        let total_sectors = round_to_sectors(file_length as i64);

        // Scan sectors 2..maxSector looking for valid chunk streams. The bound
        // is Integer.MAX_VALUE >>> 8 (0x7FFFFF), NOT the 24-bit sector mask.
        let max_sector = ((i32::MAX >> 8) as i64).min(total_sectors);
        let mut i = 2i64;
        while i < max_sector {
            let chunk_data_length = self.get_length(i);
            match self.attempt_read(i, chunk_data_length, file_length) {
                ScanRead::None | ScanRead::Oversized => {
                    i += 1;
                    continue;
                }
                ScanRead::Compound(compound) => {
                    let chunk_pos = get_chunk_coordinate(&compound);
                    if !in_same_regionfile(&our_lower_left_position, &chunk_pos) {
                        eprintln!(
                            "Ignoring absolute chunk {:?} in regionfile as it is not contained in the bounds of the regionfile '{}'. It should be in regionfile ({},{})",
                            chunk_pos,
                            self.path.display(),
                            chunk_pos.get_region_x(),
                            chunk_pos.get_region_z()
                        );
                        i += 1;
                        continue;
                    }
                    let location = ((chunk_pos.x() & 31) | ((chunk_pos.z() & 31) << 5)) as usize;
                    if let Some(other) = &compounds[location]
                        && get_last_world_save_time(other) > get_last_world_save_time(&compound)
                    {
                        i += 1;
                        continue; // don't overwrite newer data
                    }
                    compounds[location] = Some(compound);
                    // Java `rawLengths[location] = chunkDataLength + 4` wraps on
                    // int overflow; the sector math must not panic on corrupt data.
                    raw_lengths[location] = chunk_data_length.wrapping_add(4);
                    sector_offsets[location] = i as i32;
                    // Java: `i += chunkSectorLength; --i;` then the for-loop's
                    // `++i` — net advance is exactly chunkSectorLength.
                    let chunk_sector_length = round_to_sectors(raw_lengths[location] as i64);
                    i += chunk_sector_length;
                }
            }
        }

        // External (.mcc) chunks: read each in-bounds file trying every
        // registered codec; the slot is marked oversized when the external
        // compound's LastUpdate is newer than the local one (ties prefer the
        // local record). Sorted for determinism — Java iterates directory
        // order, but each location is decided independently so the outcome is
        // order-insensitive.
        let mut oversized = [false; 1024];
        let mut oversized_compression_types = [None; 1024];

        let mut region_files: Vec<PathBuf> = fs::read_dir(&self.external_file_dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        region_files.sort();

        let lower_x_bound = our_lower_left_position.x();
        let lower_z_bound = our_lower_left_position.z();
        let upper_x_bound = lower_x_bound + 31;
        let upper_z_bound = lower_z_bound + 31;

        for region_file in &region_files {
            let Some(oversized_coords) = get_oversized_chunk_pair(region_file) else {
                continue;
            };
            if oversized_coords.x() < lower_x_bound
                || oversized_coords.x() > upper_x_bound
                || oversized_coords.z() < lower_z_bound
                || oversized_coords.z() > upper_z_bound
            {
                continue;
            }
            let location =
                ((oversized_coords.x() & 31) | ((oversized_coords.z() & 31) << 5)) as usize;
            let chunk_data = match fs::read(region_file) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!(
                        "Failed to read oversized chunk data in file {}, data will be lost: {}",
                        region_file.display(),
                        e
                    );
                    continue;
                }
            };
            // We do not know the compression type, as it's stored in the
            // regionfile. Try all of them; the first that parses wins.
            let mut compound: Option<CompoundTag> = None;
            let mut compression: Option<RegionFileVersion> = None;
            for version in ALL_REGISTERED_VERSIONS {
                match version.wrap_input(chunk_data.as_slice()) {
                    Ok(reader) => {
                        let mut din = DataInputStream::new(reader);
                        match nbt_io::read_unlimited(&mut din) {
                            Ok(t) => {
                                compound = Some(t);
                                compression = Some(version);
                                break;
                            }
                            Err(_) => continue,
                        }
                    }
                    Err(_) => continue, // id 127/lz4 wrappers throw → try next
                }
            }
            let Some(compound) = compound else {
                eprintln!(
                    "Failed to read oversized chunk data in file {}, it's corrupt. Its data will be lost",
                    region_file.display()
                );
                continue;
            };
            if get_chunk_coordinate(&compound) != oversized_coords {
                eprintln!(
                    "Can't use oversized chunk stored in {}, got absolute chunkpos: {:?}, expected {:?}",
                    region_file.display(),
                    get_chunk_coordinate(&compound),
                    oversized_coords
                );
                continue;
            }
            let local = &compounds[location];
            if local.is_none()
                || get_last_world_save_time(&compound)
                    > get_last_world_save_time(local.as_ref().unwrap())
            {
                oversized[location] = true;
                oversized_compression_types[location] = compression;
            }
        }

        // New locations on a fresh bitmap, so overlapping/duplicate data gets
        // only one owner; oversized stubs are re-emitted into single sectors.
        let mut calculated_offsets = [0i32; 1024];
        let mut new_sector_allocations = RegionBitmap::new();
        new_sector_allocations.force(0, 2); // make space for header

        // Paper iterates `chunkX` outer, `chunkZ` inner, so a sector span is
        // claimed in Z-major order (`location = chunkX | chunkZ << 5`); the
        // first claimant wins, which decides overlapping corrupt data.
        for chunk_x in 0..32 {
            for chunk_z in 0..32 {
                let location = (chunk_x | (chunk_z << 5)) as usize;
                if oversized[location] {
                    continue;
                }
                let raw_length = raw_lengths[location];
                let sector_offset = sector_offsets[location];
                let sector_length = round_to_sectors(raw_length as i64) as i32;
                if new_sector_allocations.try_allocate(sector_offset, sector_length) {
                    calculated_offsets[location] = sector_offset << 8
                        | (if sector_length > 255 {
                            255
                        } else {
                            sector_length
                        });
                } else {
                    eprintln!(
                        "Failed to allocate space for local chunk (overlapping data??) at ({},{}) in regionfile {}, chunk will be regenerated",
                        chunk_x,
                        chunk_z,
                        self.path.display()
                    );
                }
            }
        }

        // Same Z-major order: oversized stubs claim single sectors in the same
        // sequence Paper does, so the assigned sector numbers line up.
        for chunk_x in 0..32 {
            for chunk_z in 0..32 {
                let location = (chunk_x | (chunk_z << 5)) as usize;
                if !oversized[location] {
                    continue;
                }
                let sector_offset = new_sector_allocations.allocate(1);
                let sector_length = 1;
                let stub = self.create_external_stub(
                    oversized_compression_types[location].unwrap_or(self.version),
                );
                match self.write_at(sector_offset as u64 * 4096, &stub) {
                    Ok(()) => {
                        calculated_offsets[location] = sector_offset << 8
                            | (if sector_length > 255 {
                                255
                            } else {
                                sector_length
                            });
                    }
                    Err(e) => {
                        new_sector_allocations.free(sector_offset, sector_length);
                        eprintln!(
                            "Failed to write new oversized chunk data holder, local chunk at ({},{}) in regionfile {} will be regenerated: {}",
                            chunk_x,
                            chunk_z,
                            self.path.display(),
                            e
                        );
                    }
                }
            }
        }

        // RivetTodo(#231): the Aikar-style oversized meta rewrite (oversized[]
        // flags → .oversized.nbt) is deferred with the rest of the Aikar
        // subsystem — recalc leaves any pre-existing meta file untouched.

        self.used_sectors.copy_from(&new_sector_allocations);

        eprintln!(
            "Starting summary of changes for regionfile {}",
            self.path.display()
        );
        for (location, &new_offset) in calculated_offsets.iter().enumerate() {
            let old_offset = self.read_offset(location);
            if old_offset == new_offset {
                continue;
            }
            self.write_offset(location, new_offset);
            if old_offset == 0 {
                eprintln!(
                    "Found missing data for local chunk ({},{}) in regionfile {}",
                    location & 31,
                    location >> 5,
                    self.path.display()
                );
            } else if new_offset == 0 {
                eprintln!(
                    "Data for local chunk ({},{}) could not be recovered in regionfile {}, it will be regenerated",
                    location & 31,
                    location >> 5,
                    self.path.display()
                );
            } else {
                eprintln!(
                    "Local chunk ({},{}) changed to point to newer data or correct chunk in regionfile {}",
                    location & 31,
                    location >> 5,
                    self.path.display()
                );
            }
        }
        eprintln!(
            "End of change summary for regionfile {}",
            self.path.display()
        );

        // "simply destroy the timestamp header, it's not used"
        for (i, &new_offset) in calculated_offsets.iter().enumerate() {
            self.write_timestamp(i, if new_offset != 0 { get_timestamp() } else { 0 });
        }

        // Repaired header stays memory-only: recalc never calls write_header.
        // The "Successfully wrote new header to disk" log is misleading in
        // Paper too — this is just flush + an extra force.
        match self.flush().and_then(|_| self.file_mut().sync_all()) {
            Ok(()) => eprintln!(
                "Successfully wrote new header to disk for regionfile {}",
                self.path.display()
            ),
            Err(e) => eprintln!(
                "Failed to write new header to disk for regionfile {}: {}",
                self.path.display(),
                e
            ),
        }

        Ok(true)
    }

    /// `attemptRead(sector, chunkDataLength, fileLength)` — the recalc scan's
    /// per-sector probe: bounds-check, read exactly the declared bytes, unwrap
    /// the codec, parse NBT. Any failure → `None`; an external-bit compression
    /// byte → `Oversized` (skipped for local data).
    fn attempt_read(&mut self, sector: i64, chunk_data_length: i32, file_length: u64) -> ScanRead {
        if chunk_data_length < 0 {
            return ScanRead::None;
        }
        let offset = sector * 4096 + 4;
        if (offset as u64) + chunk_data_length as u64 > file_length {
            return ScanRead::None;
        }
        let mut chunk_data = vec![0u8; chunk_data_length as usize];
        if self
            .seek_read_exact(offset as u64, &mut chunk_data)
            .is_err()
        {
            return ScanRead::None;
        }
        if chunk_data.is_empty() {
            // A zero-length sector (freed/never-written) — Java's
            // `chunkData.get()` on the empty flipped buffer throws
            // BufferUnderflowException, caught → null.
            return ScanRead::None;
        }
        let compression_type = chunk_data[0];
        if compression_type & EXTERNAL_STREAM_FLAG != 0 {
            return ScanRead::Oversized;
        }
        let Some(compression) = RegionFileVersion::from_id(compression_type as i32) else {
            return ScanRead::None;
        };
        let Ok(reader) = compression.wrap_input(&chunk_data[1..]) else {
            // id 127 (UnsupportedOperationException) and lz4 (Rivet's D13
            // deferral) land here; Java's `catch (Exception) → null` covers both.
            return ScanRead::None;
        };
        let mut din = DataInputStream::new(reader);
        match nbt_io::read_unlimited(&mut din) {
            Ok(compound) => ScanRead::Compound(compound),
            Err(_) => ScanRead::None,
        }
    }

    /// `getLength(sector)` — the 4-byte length field at `sector*4096`, or -1 on
    /// a short read (Java `if (4 != read) return -1`).
    fn get_length(&mut self, sector: i64) -> i32 {
        let mut length = [0u8; 4];
        if self
            .seek_read_exact((sector * 4096) as u64, &mut length)
            .is_err()
        {
            return -1;
        }
        i32::from_be_bytes(length)
    }

    /// The Spigot-255 sentinel length read: Java reads into a zero-initialized
    /// 4-byte buffer *without* checking the count, so a short read leaves the
    /// tail at 0 and the value is still interpreted. Used by the constructor
    /// replay and `get_chunk_data_input_stream`.
    fn read_length_unchecked(&mut self, sector: u64) -> io::Result<i32> {
        let mut b = [0u8; 4];
        self.file_mut().seek(SeekFrom::Start(sector * 4096))?;
        let _n = self.file_mut().read(&mut b)?;
        Ok(i32::from_be_bytes(b))
    }

    /// `backupRegionFile()` — `file.force(true)` then copy to
    /// `<parent>/<filename>.<random>.backup`, logging each step.
    fn backup_region_file(&mut self) {
        let Some(parent) = self.path.parent() else {
            return;
        };
        let file_name = self
            .path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let backup = parent.join(format!("{}.{}.backup", file_name, next_random_long()));
        if let Err(e) = self.file_mut().sync_all() {
            eprintln!("Failed to backup to {}: {}", backup.display(), e);
            return;
        }
        eprintln!(
            "Backing up regionfile \"{}\" to {}",
            self.path.display(),
            backup.display()
        );
        match fs::copy(&self.path, &backup) {
            Ok(_) => eprintln!("Backed up the regionfile to {}", backup.display()),
            Err(e) => eprintln!("Failed to backup to {}: {}", backup.display(), e),
        }
    }

    /// `writeHeader()` — rewrite all 8192 header bytes at position 0.
    fn write_header(&mut self) -> io::Result<()> {
        self.file_mut().seek(SeekFrom::Start(0))?;
        // Copy the 8KiB header first: `file_mut` borrows all of `self`, so the
        // header can't be re-borrowed inside the same call.
        let header = self.header;
        self.file_mut().write_all(&header)?;
        if self.sync {
            // Java opens the channel with DSYNC when `sync` is set; Rivet
            // emulates it with a sync after each header write.
            self.file_mut().sync_data()?;
        }
        Ok(())
    }

    /// `padToFullSector()` — extend a non-sector-multiple file to a full final
    /// sector by writing one zero byte at `paddedSize - 1`.
    fn pad_to_full_sector(&mut self) -> io::Result<()> {
        let file_size = self.file_mut().metadata()?.len() as i32;
        let padded_size = size_to_sectors(file_size) * 4096;
        if file_size != padded_size {
            self.file_mut()
                .seek(SeekFrom::Start((padded_size - 1) as u64))?;
            self.file_mut().write_all(&[0u8])?;
        }
        Ok(())
    }

    /// `writeToExternalFile(data)` — write the payload to a `tmp...` file in the
    /// external dir, returning the kept-alive temp path for the commit op to
    /// move over the target.
    fn write_to_external_file(&mut self, data: &[u8]) -> io::Result<TempPath> {
        let mut tmp = tempfile::Builder::new()
            .prefix("tmp")
            .tempfile_in(&self.external_file_dir)?;
        tmp.write_all(data)?;
        let tmp_path = tmp.into_temp_path();
        Ok(tmp_path)
    }

    /// `createExternalStub(version)` — the 5-byte `.mcc` stub
    /// `[00 00 00 01][id | 0x80]`.
    fn create_external_stub(&self, version: RegionFileVersion) -> Vec<u8> {
        let mut stub = vec![0u8; 5];
        stub[0..4].copy_from_slice(&1i32.to_be_bytes());
        stub[4] = (version.id() as u8) | EXTERNAL_STREAM_FLAG;
        stub
    }

    /// `getExternalChunkPath(pos)` — `c.<x>.<z>.mcc` in the external dir.
    fn get_external_chunk_path(&self, pos: &ChunkPos) -> PathBuf {
        self.external_file_dir
            .join(format!("c.{}.{}.mcc", pos.x(), pos.z()))
    }

    /// `getOffset(pos)`.
    fn get_offset(&self, pos: &ChunkPos) -> i32 {
        self.read_offset(Self::get_offset_index(pos))
    }

    /// Big-endian read of location `index` from the header buffer
    /// (`offsets.get(index)` on Java's IntBuffer view).
    fn read_offset(&self, index: usize) -> i32 {
        i32::from_be_bytes(self.header[index * 4..index * 4 + 4].try_into().unwrap())
    }

    /// Big-endian write of location `index` (`offsets.put(index, value)`).
    fn write_offset(&mut self, index: usize, value: i32) {
        self.header[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }

    /// Big-endian write of timestamp `index`. The timestamp header is
    /// write-only — Paper never reads it back (`recalculateHeader` rewrites it
    /// from scratch: "simply destroy the timestamp header, it's not used").
    fn write_timestamp(&mut self, index: usize, value: i32) {
        self.header[4096 + index * 4..4096 + index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }

    /// `getOffsetIndex(pos)` — `getRegionLocalX() + getRegionLocalZ() * 32`.
    fn get_offset_index(pos: &ChunkPos) -> usize {
        (pos.get_region_local_x() + pos.get_region_local_z() * 32) as usize
    }

    /// `packSectorOffset(index, size)` — `index << 8 | size`.
    fn pack_sector_offset(index: i32, size: i32) -> i32 {
        index << 8 | size
    }

    /// Positioned `read_exact`.
    fn seek_read_exact(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.file_mut().seek(SeekFrom::Start(offset))?;
        self.file_mut().read_exact(buf)
    }

    /// Positioned `write_all`.
    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> io::Result<()> {
        self.file_mut().seek(SeekFrom::Start(offset))?;
        self.file_mut().write_all(bytes)
    }
}

/// `getSectorNumber(offset)` — `offset >> 8 & 0xFFFFFF`.
const fn get_sector_number(offset: i32) -> i32 {
    offset >> 8 & 0xFFFFFF
}

/// `getNumSectors(offset)` — `offset & 0xFF`.
const fn get_num_sectors(offset: i32) -> i32 {
    offset & 0xFF
}

/// `sizeToSectors(size)` — `(size + 4096 - 1) / 4096`, wrapping int arithmetic.
const fn size_to_sectors(size: i32) -> i32 {
    size.wrapping_add(4095) / 4096
}

/// `roundToSectors(bytes)` — Paper's bit-twiddling
/// `sectors = bytes >>> 12; sectors + (rem != 0 ? 1 : 0)`, branchless.
fn round_to_sectors(bytes: i64) -> i64 {
    let sectors = (bytes as u64) >> 12;
    let remaining = bytes & 4095;
    let sign = -remaining; // sign bit set iff remaining != 0
    (sectors as i64) + (((sign as u64) >> 63) as i64)
}

/// `isExternalStreamChunk(version)` — `(version & 0x80) != 0`.
fn is_external_stream_chunk(version: u8) -> bool {
    version & EXTERNAL_STREAM_FLAG != 0
}

/// `getExternalChunkVersion(version)` — `version & ~0x80` (the real codec id).
fn get_external_chunk_version(version: u8) -> i32 {
    (version as i32) & !(EXTERNAL_STREAM_FLAG as i32)
}

/// `getTimestamp()` — `(int)(Util.getEpochMillis() / 1000L)`, truncated.
fn get_timestamp() -> i32 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    (millis / 1000) as i32
}

/// A random `long` for the backup filename's suffix — Java `new
/// java.util.Random().nextLong()`; the value is only used for uniqueness.
fn next_random_long() -> i64 {
    let mut random = LegacyRandomSource::new(generate_unique_seed());
    random.next_long()
}

/// `inSameRegionfile(first, second)` — `(x & ~31) == (x' & ~31)` on both axes,
/// equivalent to comparing region coordinates.
fn in_same_regionfile(first: &ChunkPos, second: &ChunkPos) -> bool {
    first.get_region_x() == second.get_region_x() && first.get_region_z() == second.get_region_z()
}

/// `RegionFileStorage.getRegionFileCoordinates(path)` — parse `r.<x>.<z>.mca`
/// into the region-origin `ChunkPos` (`x << 5, z << 5`), or `None`. Lives here
/// because `RegionFileStorage` is deferred; recalc needs it.
pub fn get_region_file_coordinates(path: &Path) -> Option<ChunkPos> {
    let file_name = path.file_name()?.to_str()?;
    if !file_name.starts_with("r.") || !file_name.ends_with(".mca") {
        return None;
    }
    let split: Vec<&str> = file_name.split('.').collect();
    if split.len() != 4 {
        return None;
    }
    let x = split[1].parse::<i32>().ok()?;
    let z = split[2].parse::<i32>().ok()?;
    Some(ChunkPos::new(x << 5, z << 5))
}

/// `getOversizedChunkPair(path)` — parse `c.<x>.<z>.mcc` into its `ChunkPos`,
/// or `None`.
fn get_oversized_chunk_pair(path: &Path) -> Option<ChunkPos> {
    let file_name = path.file_name()?.to_str()?;
    if !file_name.starts_with("c.") || !file_name.ends_with(EXTERNAL_FILE_EXTENSION) {
        return None;
    }
    let split: Vec<&str> = file_name.split('.').collect();
    if split.len() != 4 {
        return None;
    }
    let x = split[1].parse::<i32>().ok()?;
    let z = split[2].parse::<i32>().ok()?;
    Some(ChunkPos::new(x, z))
}

/// `SerializableChunkData.getChunkCoordinate(chunkData)` — the chunk's own
/// `xPos`/`zPos`, read from the root compound (DataVersion >= 2842) or the
/// legacy `Level` sub-compound. Ported here because the header-recalc slot
/// matching needs it and `SerializableChunkData` is a later wave.
pub fn get_chunk_coordinate(chunk_data: &CompoundTag) -> ChunkPos {
    if get_data_version(chunk_data) < 2842 {
        let level_data = chunk_data.get_compound_or_empty("Level");
        ChunkPos::new(
            level_data.get_int_or("xPos", 0),
            level_data.get_int_or("zPos", 0),
        )
    } else {
        ChunkPos::new(
            chunk_data.get_int_or("xPos", 0),
            chunk_data.get_int_or("zPos", 0),
        )
    }
}

/// `SerializableChunkData.getLastWorldSaveTime(chunkData)` — the `LastUpdate`
/// long, from the root (DataVersion >= 2842) or legacy `Level` sub-compound;
/// used by the recalc's newest-wins tie-break.
pub fn get_last_world_save_time(chunk_data: &CompoundTag) -> i64 {
    if get_data_version(chunk_data) < 2842 {
        let level_data = chunk_data.get_compound_or_empty("Level");
        level_data.get_long_or("LastUpdate", 0)
    } else {
        chunk_data.get_long_or("LastUpdate", 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::nbt_utils::add_current_data_version;
    use rivet_nbt::tag::Tag;
    use rivet_util::data_io::DataOutputStream;

    /// A chunk compound the container treats as chunk data: DataVersion >= 2842
    /// (so `get_chunk_coordinate` reads the root), plus `xPos`/`zPos`/`Status`.
    fn chunk_tag(x: i32, z: i32, last_update: i64) -> CompoundTag {
        let mut tag = CompoundTag::new();
        add_current_data_version(&mut tag);
        tag.put_int("xPos", x);
        tag.put_int("zPos", z);
        tag.put_long("LastUpdate", last_update);
        tag.put_string("Status", "full");
        tag
    }

    fn info() -> RegionStorageInfo {
        RegionStorageInfo::new(
            "test".to_string(),
            crate::level::overworld(),
            "region".to_string(),
            true,
        )
    }

    /// Open a region file at `<dir>/r.0.0.mca` (the name must parse for recalc).
    fn open_region(dir: &Path, version: RegionFileVersion) -> RegionFile {
        RegionFile::open(
            info(),
            dir.join("r.0.0.mca"),
            dir.to_path_buf(),
            version,
            false,
        )
        .unwrap()
    }

    fn write_chunk(region: &mut RegionFile, pos: ChunkPos, tag: &CompoundTag) {
        let mut writer = region.get_chunk_data_output_stream(&pos).unwrap();
        {
            let mut out = DataOutputStream::new(&mut writer);
            nbt_io::write(tag, &mut out).unwrap();
        }
        let mut buffer = writer.finish().unwrap();
        buffer.close(region).unwrap();
    }

    fn read_chunk(region: &mut RegionFile, pos: ChunkPos) -> Option<CompoundTag> {
        let reader = region.get_chunk_data_input_stream(&pos).unwrap()?;
        let mut din = DataInputStream::new(reader);
        nbt_io::read_unlimited(&mut din).ok()
    }

    #[test]
    fn round_trip_none_codec() {
        let dir = tempfile::tempdir().unwrap();
        let mut region = open_region(dir.path(), RegionFileVersion::VERSION_NONE);
        let pos = ChunkPos::new(0, 0);
        let tag = chunk_tag(0, 0, 7);
        write_chunk(&mut region, pos, &tag);
        let got = read_chunk(&mut region, pos).expect("chunk must read back");
        assert_eq!(got, tag);
        assert!(region.has_chunk(&pos));
        // The record is byte-transparent at `none`: the header location must
        // reference sector 2 with a 1-sector count.
        let offset = region.get_offset(&pos);
        assert_eq!(get_sector_number(offset), 2);
        assert_eq!(get_num_sectors(offset), 1);
    }

    #[test]
    fn round_trip_gzip_codec() {
        let dir = tempfile::tempdir().unwrap();
        let mut region = open_region(dir.path(), RegionFileVersion::VERSION_GZIP);
        let pos = ChunkPos::new(3, 5);
        let tag = chunk_tag(3, 5, 99);
        write_chunk(&mut region, pos, &tag);
        assert_eq!(read_chunk(&mut region, pos).expect("gzip chunk"), tag);
    }

    #[test]
    fn deflate_read_through_container() {
        // deflate write is deferred (D13), so exercise the read path by
        // hand-crafting a zlib-wrapped chunk in the region file.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.0.0.mca");
        let tag = chunk_tag(1, 1, 5);
        let mut payload = Vec::new();
        nbt_io::write(&tag, &mut DataOutputStream::new(&mut payload)).unwrap();
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&payload).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut stream = Vec::new();
        stream.extend_from_slice(&((compressed.len() as i32) + 1).to_be_bytes());
        stream.push(2); // deflate
        stream.extend_from_slice(&compressed);
        // Header location for chunk (1,1): local x=1 | local z=1<<5.
        write_raw_stream_region(&path, 33, &stream, 2, 1);

        let mut region = open_region(dir.path(), RegionFileVersion::VERSION_NONE);
        assert_eq!(
            read_chunk(&mut region, ChunkPos::new(1, 1)).expect("deflate chunk"),
            tag
        );
    }

    #[test]
    fn external_oversized_chunk_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut region = open_region(dir.path(), RegionFileVersion::VERSION_NONE);
        let pos = ChunkPos::new(2, 2);
        let mut tag = chunk_tag(2, 2, 11);
        // A > 256-sector record (1.2 MiB of byte-array payload at `none`).
        tag.put_byte_array("big", vec![0i8; 1_200_000]);

        write_chunk(&mut region, pos, &tag);

        let got = read_chunk(&mut region, pos).expect("oversized chunk");
        assert_eq!(got, tag);
        // The stub: length 1, compression = none | 0x80.
        let offset = region.get_offset(&pos);
        assert_eq!(get_num_sectors(offset), 1, "external stub uses 1 sector");
        let stub_sector = get_sector_number(offset);
        let stub = read_sector(&mut region, stub_sector, 5);
        assert_eq!(
            i32::from_be_bytes(stub[0..4].try_into().unwrap()),
            1,
            "stub length field is 1"
        );
        assert_eq!(stub[4], 3 | EXTERNAL_STREAM_FLAG, "stub codec id | 0x80");
        // The external file exists and holds exactly the raw NBT payload (no
        // length field, no compression byte) — read it back directly.
        let external = dir.path().join("c.2.2.mcc");
        assert!(external.is_file());
        let mut f = fs::File::open(&external).unwrap();
        let ext_bytes = {
            let mut v = Vec::new();
            f.read_to_end(&mut v).unwrap();
            v
        };
        let mut din = DataInputStream::new(std::io::Cursor::new(ext_bytes));
        let reparsed = nbt_io::read_unlimited(&mut din).unwrap();
        assert_eq!(reparsed, tag, "external file holds the raw NBT payload");
    }

    #[test]
    fn clear_frees_sector_and_deletes_external() {
        let dir = tempfile::tempdir().unwrap();
        let mut region = open_region(dir.path(), RegionFileVersion::VERSION_NONE);
        let pos = ChunkPos::new(1, 1);

        // Small internal chunk first.
        write_chunk(&mut region, pos, &chunk_tag(1, 1, 1));
        assert!(region.has_chunk(&pos));
        region.clear(&pos).unwrap();
        assert!(!region.has_chunk(&pos));
        assert!(region.get_chunk_data_input_stream(&pos).unwrap().is_none());

        // The freed sector is reused: first-fit allocate returns sector 2 again.
        write_chunk(&mut region, pos, &chunk_tag(1, 1, 2));
        assert_eq!(get_sector_number(region.get_offset(&pos)), 2);

        // External chunk: clear must delete the .mcc file too.
        let mut big = chunk_tag(1, 1, 3);
        big.put_byte_array("big", vec![0i8; 1_200_000]);
        write_chunk(&mut region, pos, &big);
        let external = dir.path().join("c.1.1.mcc");
        assert!(external.is_file());
        region.clear(&pos).unwrap();
        assert!(!external.exists(), "clear deletes the .mcc file");
    }

    #[test]
    fn sector_relocation_on_rewrite() {
        // Writing a bigger chunk relocates it past the old span and frees the
        // old sectors (allocate is first-fit from sector 0, so the used old
        // span is skipped).
        let dir = tempfile::tempdir().unwrap();
        let mut region = open_region(dir.path(), RegionFileVersion::VERSION_NONE);
        let pos = ChunkPos::new(0, 0);

        let small = chunk_tag(0, 0, 1);
        write_chunk(&mut region, pos, &small);
        let first = get_sector_number(region.get_offset(&pos));
        assert_eq!(first, 2);

        let mut big = chunk_tag(0, 0, 2);
        big.put_byte_array("filler", vec![0i8; 20_000]); // 5 sectors
        write_chunk(&mut region, pos, &big);
        let second = get_sector_number(region.get_offset(&pos));
        assert!(
            second > first,
            "grown chunk must move forward: {second} > {first}"
        );
        assert_eq!(read_chunk(&mut region, pos).expect("rewritten chunk"), big);
        // Old sector 2 is freed: a fresh chunk now lands back at sector 2.
        let other = ChunkPos::new(0, 1);
        write_chunk(&mut region, other, &chunk_tag(0, 1, 3));
        assert_eq!(get_sector_number(region.get_offset(&other)), 2);
    }

    #[test]
    fn corrupt_stream_length_returns_none_without_recalc() {
        // A non-CHUNK file (canRecalcHeader == false) must return null on a
        // truncated stream and leave the header slot untouched (Tier-1 path).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.0.0.mca");
        let mut stream = Vec::new();
        stream.extend_from_slice(&0x7FFF_FFFFi32.to_be_bytes()); // huge declared length
        stream.push(3);
        stream.extend_from_slice(&[0u8; 8]);
        write_raw_stream_region(&path, 0, &stream, 2, 1);

        let mut region = RegionFile::open(
            RegionStorageInfo::new(
                "test".to_string(),
                crate::level::overworld(),
                "region".to_string(),
                false, // not chunk data → no recalc
            ),
            path.clone(),
            dir.path().to_path_buf(),
            RegionFileVersion::VERSION_NONE,
            false,
        )
        .unwrap();
        assert!(
            region
                .get_chunk_data_input_stream(&ChunkPos::new(0, 0))
                .unwrap()
                .is_none()
        );
        // Header untouched: location still points at sector 2 count 1.
        assert_eq!(region.get_offset(&ChunkPos::new(0, 0)), (2 << 8) | 1);
    }

    #[test]
    fn custom_127_returns_null() {
        // A stream with the custom id (127) and a readable modified-UTF-8 id
        // logs and returns null (the chunk is treated as absent).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.0.0.mca");
        let mut stream = Vec::new();
        stream.extend_from_slice(&4i32.to_be_bytes()); // stream_length + 1 = 4
        stream.push(127);
        stream.extend_from_slice(&[0u8, 1, b'a']); // modified-UTF-8 "a"
        write_raw_stream_region(&path, 0, &stream, 2, 1);

        let mut region = RegionFile::open(
            RegionStorageInfo::new(
                "test".to_string(),
                crate::level::overworld(),
                "region".to_string(),
                false,
            ),
            path,
            dir.path().to_path_buf(),
            RegionFileVersion::VERSION_NONE,
            false,
        )
        .unwrap();
        assert!(
            region
                .get_chunk_data_input_stream(&ChunkPos::new(0, 0))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn recalc_relinks_corrupt_header_from_scan() {
        // A header whose location points at sector 1 (the header itself) is
        // invalid; opening a CHUNK file triggers recalc, which scans sector 2,
        // finds the valid chunk, and relinks location[0] to it. The repaired
        // header stays memory-only — the on-disk header is untouched.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.0.0.mca");
        let tag = chunk_tag(0, 0, 42);

        let mut stream = Vec::new();
        let mut payload = Vec::new();
        nbt_io::write(&tag, &mut DataOutputStream::new(&mut payload)).unwrap();
        stream.extend_from_slice(&((payload.len() as i32) + 1).to_be_bytes());
        stream.push(3);
        stream.extend_from_slice(&payload);
        // The header points at sector 1 (overlaps the header — the corruption),
        // but the real data sits at sector 2 where the recalc scan finds it.
        write_raw_stream_region(&path, 0, &stream, 2, 1);
        // Corrupt the header location to point at sector 1 after the file is
        // written.
        {
            let mut f = OpenOptions::new().write(true).open(&path).unwrap();
            f.write_all(&((1i32 << 8) | 1).to_be_bytes()).unwrap();
        }

        let mut region = open_region(dir.path(), RegionFileVersion::VERSION_NONE);
        assert!(region.get_recalculate_count() >= 1, "recalc ran at open");
        assert_eq!(
            read_chunk(&mut region, ChunkPos::new(0, 0)).expect("recovered chunk"),
            tag
        );
        let offset = region.get_offset(&ChunkPos::new(0, 0));
        assert_eq!(
            get_sector_number(offset),
            2,
            "relinked to the scanned sector"
        );

        // Recalc never wrote the header to disk: the first location byte is
        // still the bogus sector-1 value.
        let mut disk_header = [0u8; 4];
        let mut f = fs::OpenOptions::new().read(true).open(&path).unwrap();
        f.read_exact(&mut disk_header).unwrap();
        assert_eq!(
            i32::from_be_bytes(disk_header),
            (1 << 8) | 1,
            "recalc leaves the on-disk header corrupt"
        );
    }

    #[test]
    fn recalc_skips_zeroed_sectors_between_chunks() {
        // A freed/never-written sector (length 0) inside the scan range must be
        // skipped, not panic — Java's `chunkData.get()` on the empty buffer
        // throws BufferUnderflowException, caught → null.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.0.0.mca");
        let tag = chunk_tag(0, 0, 7);

        // Chunk at sector 2 (a 1-sector stream).
        let mut stream = Vec::new();
        let mut payload = Vec::new();
        nbt_io::write(&tag, &mut DataOutputStream::new(&mut payload)).unwrap();
        stream.extend_from_slice(&((payload.len() as i32) + 1).to_be_bytes());
        stream.push(3);
        stream.extend_from_slice(&payload);
        write_raw_stream_region(&path, 0, &stream, 2, 1);

        // Leave sector 3 zeroed (length 0) and pad the file past it so the
        // scan reaches it, then corrupt the header to force recalc.
        {
            let mut f = OpenOptions::new().write(true).open(&path).unwrap();
            f.seek(SeekFrom::Start(3 * 4096 + 4096 - 1)).unwrap();
            f.write_all(&[0u8]).unwrap();
        }
        {
            let mut f = OpenOptions::new().write(true).open(&path).unwrap();
            f.write_all(&((1i32 << 8) | 1).to_be_bytes()).unwrap();
        }

        let mut region = open_region(dir.path(), RegionFileVersion::VERSION_NONE);
        assert!(region.get_recalculate_count() >= 1, "recalc ran at open");
        assert_eq!(
            read_chunk(&mut region, ChunkPos::new(0, 0)).expect("recovered chunk"),
            tag
        );
    }

    #[test]
    fn recalc_assigns_oversized_stub_sectors_in_z_major_order() {
        // Paper's recalc allocates oversized stubs in Z-major order (chunkX
        // outer, chunkZ inner). Two oversized chunks whose X-major and Z-major
        // orderings disagree (1,2) vs (2,1) must get sectors in Paper's order:
        // (1,2) is visited at x=1 before (2,1) at x=2, so (1,2) → sector 2.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.0.0.mca");

        // Header-only file with location[0] pointing at sector 1 (invalid),
        // forcing recalc; no local chunk data.
        let mut header = [0u8; 8192];
        header[0..4].copy_from_slice(&((1i32 << 8) | 1).to_be_bytes());
        fs::write(&path, header).unwrap();

        // Two oversized (.mcc) payloads in the external dir, raw NBT (none).
        for (x, z) in [(1, 2), (2, 1)] {
            let tag = chunk_tag(x, z, 100);
            let mut payload = Vec::new();
            nbt_io::write(&tag, &mut DataOutputStream::new(&mut payload)).unwrap();
            fs::write(dir.path().join(format!("c.{}.{}.mcc", x, z)), &payload).unwrap();
        }

        let region = open_region(dir.path(), RegionFileVersion::VERSION_NONE);
        assert!(region.get_recalculate_count() >= 1, "recalc ran at open");
        let a = region.get_offset(&ChunkPos::new(1, 2));
        let b = region.get_offset(&ChunkPos::new(2, 1));
        assert_eq!(get_num_sectors(a), 1, "oversized stub uses 1 sector");
        assert_eq!(get_num_sectors(b), 1, "oversized stub uses 1 sector");
        assert_eq!(
            get_sector_number(a),
            2,
            "Z-major: chunk (1,2) is visited before (2,1) and claims sector 2"
        );
        assert_eq!(get_sector_number(b), 3);
    }

    #[test]
    fn pad_to_full_sector_on_close() {
        // After a normal write the file is a multiple of 4096; append a torn
        // tail and close must extend the file to a full sector boundary.
        let dir = tempfile::tempdir().unwrap();
        let mut region = open_region(dir.path(), RegionFileVersion::VERSION_NONE);
        write_chunk(&mut region, ChunkPos::new(0, 0), &chunk_tag(0, 0, 1));
        let path = region.get_path().to_path_buf();
        drop(region); // force the file to flush

        // Append 100 bytes of torn tail.
        {
            let mut f = OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(&[0u8; 100]).unwrap();
        }
        let mut region = open_region(dir.path(), RegionFileVersion::VERSION_NONE);
        region.close().unwrap();
        let len = fs::metadata(&path).unwrap().len();
        assert_eq!(len % 4096, 0, "close pads to a full sector");
    }

    #[test]
    fn close_releases_descriptor_and_panics_on_access() {
        // `close()` takes the live handle (Java's `FileChannel.close()`), so the
        // descriptor is released even though the file stays on disk; any later
        // access panics, mirroring Java's `ClosedChannelException`.
        let dir = tempfile::tempdir().unwrap();
        let mut region = open_region(dir.path(), RegionFileVersion::VERSION_NONE);
        let path = region.get_path().to_path_buf();
        region.close().unwrap();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            region.flush().unwrap();
        }));
        assert!(result.is_err(), "accessing a closed RegionFile panics");

        // The file itself is untouched on disk (and re-openable).
        let mut reopened = open_region(dir.path(), RegionFileVersion::VERSION_NONE);
        assert_eq!(fs::metadata(&path).unwrap().len() % 4096, 0);
        assert!(
            reopened
                .get_chunk_data_input_stream(&ChunkPos::new(0, 0))
                .unwrap()
                .is_none(),
            "empty region has no chunk"
        );
    }

    #[test]
    fn spigot_sentinel_negative_length_returns_none_without_panic() {
        // A Spigot-255 location whose chunk's first sector declares a negative
        // length wraps to a negative sector count on read. Java would crash on
        // `ByteBuffer.allocate` (unchecked `IllegalArgumentException`); the port
        // treats the chunk as absent. The header is corrupted *after* open
        // because `open()` itself repairs a corrupt sentinel at startup — this
        // simulates the region file being modified beneath the open handle.
        let dir = tempfile::tempdir().unwrap();
        let mut region = open_region(dir.path(), RegionFileVersion::VERSION_NONE);
        // Spigot sentinel on location[0], pointing at sector 2.
        region.write_offset(0, (2 << 8) | 255);
        // Negative declared length (`0x80000000`) at sector 2.
        {
            let mut f = OpenOptions::new()
                .write(true)
                .open(region.get_path())
                .unwrap();
            f.seek(SeekFrom::Start(2 * 4096)).unwrap();
            f.write_all(&i32::MIN.to_be_bytes()).unwrap();
        }
        assert!(
            region
                .get_chunk_data_input_stream(&ChunkPos::new(0, 0))
                .unwrap()
                .is_none(),
            "negative sector count follows the corruption path"
        );
    }

    #[test]
    fn get_region_file_coordinates_parses() {
        assert_eq!(
            get_region_file_coordinates(Path::new("r.0.0.mca")),
            Some(ChunkPos::new(0, 0))
        );
        assert_eq!(
            get_region_file_coordinates(Path::new("/tmp/x/r.-2.3.mca")),
            Some(ChunkPos::new(-64, 96))
        );
        assert_eq!(get_region_file_coordinates(Path::new("r.0.mca")), None);
        assert_eq!(get_region_file_coordinates(Path::new("r.a.0.mca")), None);
        assert_eq!(get_region_file_coordinates(Path::new("c.0.0.mcc")), None);
        assert_eq!(get_region_file_coordinates(Path::new("other.txt")), None);
    }

    #[test]
    fn get_oversized_chunk_pair_parses() {
        assert_eq!(
            get_oversized_chunk_pair(Path::new("c.-1.3.mcc")),
            Some(ChunkPos::new(-1, 3))
        );
        assert_eq!(get_oversized_chunk_pair(Path::new("c.0.mcc")), None);
        assert_eq!(get_oversized_chunk_pair(Path::new("r.0.0.mca")), None);
        assert_eq!(get_oversized_chunk_pair(Path::new("c.a.b.mcc")), None);
    }

    #[test]
    fn coordinate_and_save_time_helpers() {
        // Modern layout (DataVersion >= 2842): xPos/zPos/LastUpdate on the root.
        let modern = chunk_tag(4, -6, 1234);
        assert_eq!(get_chunk_coordinate(&modern), ChunkPos::new(4, -6));
        assert_eq!(get_last_world_save_time(&modern), 1234);

        // Legacy layout: the Level sub-compound.
        let mut legacy = CompoundTag::new();
        legacy.put_int("DataVersion", 2500);
        let mut level = CompoundTag::new();
        level.put_int("xPos", 7);
        level.put_int("zPos", -9);
        level.put_long("LastUpdate", 99);
        legacy.put("Level".to_string(), Tag::Compound(level));
        assert_eq!(get_chunk_coordinate(&legacy), ChunkPos::new(7, -9));
        assert_eq!(get_last_world_save_time(&legacy), 99);
    }

    /// Write a raw region file: an 8192-byte header with `location` pointing at
    /// `(sector, count)`, `stream` written at `sector * 4096`, and the file
    /// padded to a full sector (the read path reads `numSectors * 4096` bytes).
    fn write_raw_stream_region(
        path: &Path,
        location: usize,
        stream: &[u8],
        sector: i32,
        count: i32,
    ) {
        let mut header = [0u8; 8192];
        header[location * 4..location * 4 + 4]
            .copy_from_slice(&((sector << 8) | count).to_be_bytes());
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .unwrap();
        f.write_all(&header).unwrap();
        let sector_offset = sector as u64 * 4096;
        f.seek(SeekFrom::Start(sector_offset)).unwrap();
        f.write_all(stream).unwrap();
        // Pad to the end of the claimed sector span so positioned reads return
        // the full sector count.
        let end = (sector as u64 + count as u64) * 4096;
        let len = f.metadata().unwrap().len();
        if len < end {
            f.seek(SeekFrom::Start(end - 1)).unwrap();
            f.write_all(&[0u8]).unwrap();
        }
    }

    /// Read `n` bytes at `sector * 4096`.
    fn read_sector(region: &mut RegionFile, sector: i32, n: usize) -> Vec<u8> {
        let mut buf = vec![0u8; n];
        region
            .seek_read_exact(sector as u64 * 4096, &mut buf)
            .unwrap();
        buf
    }
}
