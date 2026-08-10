//! Port of Paper 26.2's `RegionFileStorage` direct-read slice plus the
//! storage-level write lifecycle (issue #395, an M2G prerequisite under #231).
//!
//! Reads open only an already-existing `r.<regionX>.<regionZ>.mca`, retain
//! Paper's LRU and negative-cache ordering, delegate stream corruption handling
//! to `RegionFile`, parse NBT, apply the chunk-coordinate guard, and merge
//! legacy Aikar oversized supplements. The write lifecycle composes the
//! existing `RegionFile` output stream: `write(pos, value)` streams a
//! `CompoundTag` through `getChunkDataOutputStream` (or `clear`s on `None`),
//! creates regions/directories on first write, and converts a
//! `RegionFileSizeException` into a delete + log, exactly like Paper's
//! `RegionFileStorage.write`. `flush`/`close` iterate every cached region and
//! attempt all closes/flushes, returning the first error like Paper's
//! `ExceptionCollector`.
//!
//! The strict M2L read-only boundary is preserved: `new_read_only` rejects
//! every mutation entry — `write` (create/update), delete (`None`), and
//! `flush` — with `PermissionDenied` *before* any side effect, so no directory
//! is created, no region is opened for writing, no bytes change, and no backup
//! or repair runs. Deliberately not ported here: `SerializableChunkData.write`,
//! `IOWorker`, the moonrise `RegionDataController` interfaces
//! (`moonrise$startWrite`/`moonrise$finishWrite`), `scanChunk`, chunk
//! generation, upgrades, repair, and migration.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_io;
use rivet_nbt::tag::Tag;
use rivet_registry::core::ChunkPos;
use rivet_util::data_io::{DataInputStream, DataOutputStream};

use super::region_file::{
    MAX_CHUNK_SIZE, RegionFile, RegionFileSizeException, get_chunk_coordinate,
};
use super::region_file_version::RegionFileVersion;
use super::region_storage_info::RegionStorageInfo;

pub const ANVIL_EXTENSION: &str = ".mca";
const MAX_CACHE_SIZE: usize = 256;
const REGION_SHIFT: i32 = 5;
const MAX_NON_EXISTING_CACHE: usize = 1024 * 4;

/// The synchronous region-file cache used by Paper's direct storage IO.
///
/// Entries are stored most-recent-first, matching
/// `Long2ObjectLinkedOpenHashMap.getAndMoveToFirst`; the last entry is closed
/// on eviction. Writes create regions on demand (`getRegionFile(pos, false)`);
/// reads and deletes never create (`moonrise$getRegionFileIfExists`).
pub struct RegionFileStorage {
    info: RegionStorageInfo,
    folder: PathBuf,
    sync: bool,
    read_only: bool,
    region_cache: Vec<(i64, RegionFile)>,
    non_existing_region_files: Vec<i64>,
    #[cfg(test)]
    closed_region_files: Vec<i64>,
}

impl RegionFileStorage {
    pub fn new(info: RegionStorageInfo, folder: PathBuf, sync: bool) -> Self {
        Self {
            info,
            folder,
            sync,
            read_only: false,
            region_cache: Vec::new(),
            non_existing_region_files: Vec::new(),
            #[cfg(test)]
            closed_region_files: Vec::new(),
        }
    }

    /// Existing-only storage for world boot: no create/write descriptor,
    /// repair, backup, padding, or sync-on-close behavior.
    pub fn new_read_only(info: RegionStorageInfo, folder: PathBuf) -> Self {
        Self {
            info,
            folder,
            sync: false,
            read_only: true,
            region_cache: Vec::new(),
            non_existing_region_files: Vec::new(),
            #[cfg(test)]
            closed_region_files: Vec::new(),
        }
    }

    /// `getRegionFileName(chunkX, chunkZ)` — exact signed arithmetic-shift
    /// mapping, including negative chunk coordinates.
    fn get_region_file_name(chunk_x: i32, chunk_z: i32) -> String {
        format!(
            "r.{}.{}{}",
            chunk_x >> REGION_SHIFT,
            chunk_z >> REGION_SHIFT,
            ANVIL_EXTENSION
        )
    }

    /// `read(pos)` — existing-only direct NBT read.
    pub fn read(&mut self, pos: &ChunkPos) -> io::Result<Option<CompoundTag>> {
        loop {
            let Some(index) = self.get_region_file_if_exists(pos.x(), pos.z())? else {
                return Ok(None);
            };

            let serialised_chunk_data = if self.region_cache[index].1.is_oversized(pos.x(), pos.z())
            {
                self.read_oversized_chunk(index, pos)?
            } else {
                let region = &mut self.region_cache[index].1;
                let Some(reader) = region.get_chunk_data_input_stream(pos)? else {
                    return Ok(None);
                };
                let mut input = DataInputStream::new(reader);
                nbt_io::read_unlimited(&mut input)?
            };

            if self.info.is_chunk_data && get_chunk_coordinate(&serialised_chunk_data) != *pos {
                let region = &mut self.region_cache[index].1;
                eprintln!(
                    "Attempting to read chunk data at {} but got chunk data for {} instead! Attempting regionfile recalculation for regionfile {}",
                    pos,
                    get_chunk_coordinate(&serialised_chunk_data),
                    region.get_path().display()
                );
                if region.is_read_only() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "chunk coordinate mismatch in read-only region {}",
                            region.get_path().display()
                        ),
                    ));
                }
                if region.recalculate_header()? {
                    continue;
                }
                eprintln!(
                    "Can't recalculate regionfile header, regenerating chunk {} for {}",
                    pos,
                    region.get_path().display()
                );
                return Ok(None);
            }

            return Ok(Some(serialised_chunk_data));
        }
    }

    /// `write(pos, value)` — the storage-level write lifecycle. `None` is the
    /// delete path: `clear` the chunk (no-op when the region does not exist, so
    /// no file is created for a delete). `Some` streams the NBT through the
    /// region's output writer (`getChunkDataOutputStream` → `ChunkBuffer`),
    /// mirroring `NbtIo.write` then `output.close()`. A `RegionFileSizeException`
    /// from the chunk buffer deletes the chunk and logs, exactly like Paper's
    /// "don't write garbage data to disk" handling.
    ///
    /// The `SharedConstants.DEBUG_DONT_SAVE_WORLD` dev flag that Paper gates
    /// the whole method on is not ported (it is false in production).
    pub fn write(&mut self, pos: &ChunkPos, value: Option<CompoundTag>) -> io::Result<()> {
        self.ensure_writable()?;
        let region_index = match self.get_region_file(pos, value.is_none())? {
            Some(index) => index,
            None => return Ok(()),
        };
        let Some(value) = value else {
            return self.region_cache[region_index].1.clear(pos);
        };

        let result = (|| -> io::Result<()> {
            let region = &mut self.region_cache[region_index].1;
            let mut writer = region.get_chunk_data_output_stream(pos)?;
            {
                let mut out = DataOutputStream::new(&mut writer);
                nbt_io::write(&value, &mut out)?;
            }
            // Paper clears the legacy Aikar flag here
            // (`region.setOversized(x, z, false)`); that meta-mutation surface
            // stays deferred with the rest of the Aikar subsystem (RivetTodo(#231)).
            let mut buffer = writer.finish()?;
            buffer.close(region)
        })();

        match result {
            Ok(()) => Ok(()),
            Err(error) if is_region_file_size(&error) => {
                self.region_cache[region_index].1.clear(pos)?;
                eprintln!(
                    "Chunk at ({}) in regionfile '{}' exceeds max size of {}MiB, it has been deleted from disk.",
                    pos,
                    self.region_cache[region_index].1.get_path().display(),
                    MAX_CHUNK_SIZE / (1024 * 1024)
                );
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// `flush()` — `file.force(true)` on every cached region, attempting all
    /// flushes and returning the first error like Paper's `ExceptionCollector`
    /// boundary. In read-only mode this rejects before any region is touched.
    pub fn flush(&mut self) -> io::Result<()> {
        self.ensure_writable()?;
        let mut first_error = None;
        for (_, region) in &mut self.region_cache {
            if let Err(error) = region.flush()
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Close every cached region, attempting all closes and returning the first
    /// error like Paper's `ExceptionCollector` boundary. Read-only regions close
    /// descriptor-only (no pad/force), so a read-only storage close is a no-op
    /// on disk.
    pub fn close(&mut self) -> io::Result<()> {
        let mut first_error = None;
        for (_, region) in &mut self.region_cache {
            if let Err(error) = region.close()
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        self.region_cache.clear();
        first_error.map_or(Ok(()), Err)
    }

    /// `getRegionFile(pos, existingOnly)` — Paper's region selection. The
    /// existing-only path is `moonrise$getRegionFileIfExists` (never creates;
    /// the read and delete paths). The create path is reached only by writes:
    /// it evicts the LRU, drops the negative-cache entry (`createRegionFile`),
    /// creates the storage folder (`FileUtil.createDirectoriesSafe`), opens the
    /// region with CREATE+READ+WRITE, and caches it. Rejects with
    /// `PermissionDenied` in read-only mode before any filesystem side effect.
    fn get_region_file(
        &mut self,
        pos: &ChunkPos,
        existing_only: bool,
    ) -> io::Result<Option<usize>> {
        if existing_only {
            return self.get_region_file_if_exists(pos.x(), pos.z());
        }
        self.ensure_writable()?;
        let key = ChunkPos::pack_coords(pos.x() >> REGION_SHIFT, pos.z() >> REGION_SHIFT);
        if let Some(index) = self.region_cache.iter().position(|(k, _)| *k == key) {
            let entry = self.region_cache.remove(index);
            self.region_cache.insert(0, entry);
            return Ok(Some(0));
        }

        self.evict_lru_if_full()?;

        let region_path = self
            .folder
            .join(Self::get_region_file_name(pos.x(), pos.z()));
        // `createRegionFile(key)` — a prior read may have negative-cached this
        // region as missing; the create path always clears that so the fresh
        // region is served.
        self.non_existing_region_files
            .retain(|cached| *cached != key);
        // `FileUtil.createDirectoriesSafe(folder)` — `Files.createDirectories`
        // on the (real) path; `create_dir_all` matches it: no-op for an
        // existing directory, creates parents, and errors when a file occupies
        // the path.
        fs::create_dir_all(&self.folder)?;

        let region = RegionFile::open(
            self.info.clone(),
            region_path,
            self.folder.clone(),
            RegionFileVersion::get_selected(),
            self.sync,
        )?;
        self.region_cache.insert(0, (key, region));
        Ok(Some(0))
    }

    /// The strict M2L read-only boundary. Paper has no read-only storage; Rivet
    /// adds this so boot never writes. Every storage-level mutation entry
    /// checks it *before* touching the filesystem: no directory is created, no
    /// region is opened for writing, no bytes change, no backup/repair runs.
    fn ensure_writable(&self) -> io::Result<()> {
        if self.read_only {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("region file storage {} is read-only", self.folder.display()),
            ))
        } else {
            Ok(())
        }
    }

    pub fn info(&self) -> &RegionStorageInfo {
        &self.info
    }

    fn get_region_file_if_exists(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
    ) -> io::Result<Option<usize>> {
        let key = ChunkPos::pack_coords(chunk_x >> REGION_SHIFT, chunk_z >> REGION_SHIFT);
        if let Some(index) = self.region_cache.iter().position(|(k, _)| *k == key) {
            let entry = self.region_cache.remove(index);
            self.region_cache.insert(0, entry);
            return Ok(Some(0));
        }

        if let Some(index) = self
            .non_existing_region_files
            .iter()
            .position(|cached| *cached == key)
        {
            let key = self.non_existing_region_files.remove(index);
            self.non_existing_region_files.insert(0, key);
            return Ok(None);
        }

        self.evict_lru_if_full()?;

        let region_path = self
            .folder
            .join(Self::get_region_file_name(chunk_x, chunk_z));
        if !region_path.exists() {
            self.mark_non_existing(key);
            return Ok(None);
        }
        self.non_existing_region_files
            .retain(|cached| *cached != key);

        let region = if self.read_only {
            RegionFile::open_read_only(
                self.info.clone(),
                region_path,
                self.folder.clone(),
                RegionFileVersion::get_selected(),
            )?
        } else {
            RegionFile::open(
                self.info.clone(),
                region_path,
                self.folder.clone(),
                RegionFileVersion::get_selected(),
                self.sync,
            )?
        };
        self.region_cache.insert(0, (key, region));
        Ok(Some(0))
    }

    fn evict_lru_if_full(&mut self) -> io::Result<()> {
        if self.region_cache.len() < MAX_CACHE_SIZE {
            return Ok(());
        }
        let Some((key, mut evicted)) = self.region_cache.pop() else {
            return Ok(());
        };
        evicted.close()?;
        #[cfg(test)]
        self.closed_region_files.push(key);
        #[cfg(not(test))]
        let _ = key;
        Ok(())
    }

    fn mark_non_existing(&mut self, key: i64) {
        if let Some(index) = self
            .non_existing_region_files
            .iter()
            .position(|cached| *cached == key)
        {
            self.non_existing_region_files.remove(index);
        }
        self.non_existing_region_files.insert(0, key);
        // Paper uses `while (size >= MAX_NON_EXISTING_CACHE)`, so the stable
        // maximum is 4095 entries, not 4096.
        while self.non_existing_region_files.len() >= MAX_NON_EXISTING_CACHE {
            self.non_existing_region_files.pop();
        }
    }

    fn read_oversized_chunk(
        &mut self,
        region_index: usize,
        pos: &ChunkPos,
    ) -> io::Result<CompoundTag> {
        let region = &mut self.region_cache[region_index].1;
        let reader = region.get_chunk_data_input_stream(pos)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("oversized chunk {pos} has no base region stream"),
            )
        })?;
        // Paper opens the base stream first, then reads the supplement, then
        // parses the base NBT. Preserve that observable error order.
        let oversized_data = region.get_oversized_data(pos.x(), pos.z())?;
        let mut input = DataInputStream::new(reader);
        let mut chunk = nbt_io::read_unlimited(&mut input)?;

        let oversized_level = oversized_data.get_compound_or_empty("Level");
        if let Some(Tag::Compound(level)) = chunk.tags.get_mut("Level") {
            merge_chunk_list(level, &oversized_level, "Entities", "Entities");
            merge_chunk_list(level, &oversized_level, "TileEntities", "TileEntities");
        }
        Ok(chunk)
    }
}

/// Whether `error` is Paper's `RegionFileSizeException` — the chunk-buffer cap
/// thrown by `ChunkBuffer.write`. The write lifecycle converts it into a
/// delete + log ("don't write garbage data to disk").
fn is_region_file_size(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|e| e.is::<RegionFileSizeException>())
}

fn merge_chunk_list(
    level: &mut CompoundTag,
    oversized_level: &CompoundTag,
    key: &str,
    oversized_key: &str,
) {
    let oversized_list = oversized_level.get_list_or_empty(oversized_key);
    if oversized_list.is_empty() {
        return;
    }
    let mut level_list = level.get_list_or_empty(key);
    level_list.list.extend(oversized_list.list);
    level.put(key.to_string(), Tag::List(level_list));
}

/// The inverse filename parser lives on `RegionFile` because header recalc
/// also consumes it; re-exporting this helper keeps both layers on one parser.
pub fn get_region_file_coordinates(file: &Path) -> Option<ChunkPos> {
    super::region_file::get_region_file_coordinates(file)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::{Seek, SeekFrom, Write as _};

    use flate2::Compression;
    use flate2::write::{GzEncoder, ZlibEncoder};
    use rivet_nbt::list_tag::ListTag;
    use rivet_nbt::tag::Tag;
    use rivet_util::data_io::DataOutputStream;

    use super::*;
    use crate::chunk::storage::region_file_version::SELECTION_LOCK;

    const PAPER_CHUNK_0_0: &[u8] = include_bytes!(
        "../../../../../tools/rivet-oracle/fixtures/regions/superflat-full/chunk/overworld/0.0/0.0.nbt"
    );

    fn info(is_chunk_data: bool) -> RegionStorageInfo {
        RegionStorageInfo::new(
            "test".to_string(),
            crate::level::overworld(),
            "region".to_string(),
            is_chunk_data,
        )
    }

    fn write_region(folder: &Path, pos: ChunkPos, version: u8, payload: &[u8]) {
        let path = folder.join(RegionFileStorage::get_region_file_name(pos.x(), pos.z()));
        let record_len = 5 + payload.len();
        let sectors = record_len.div_ceil(4096);
        let mut bytes = vec![0u8; 8192 + sectors * 4096];
        let slot = (pos.get_region_local_x() + pos.get_region_local_z() * 32) as usize;
        bytes[slot * 4..slot * 4 + 4]
            .copy_from_slice(&((2i32 << 8) | sectors as i32).to_be_bytes());
        bytes[8192..8196].copy_from_slice(&((payload.len() as i32) + 1).to_be_bytes());
        bytes[8196] = version;
        bytes[8197..8197 + payload.len()].copy_from_slice(payload);
        fs::write(path, bytes).unwrap();
    }

    fn write_empty_regions(folder: &Path, region_xs: impl IntoIterator<Item = i32>) {
        for region_x in region_xs {
            fs::write(folder.join(format!("r.{region_x}.0.mca")), []).unwrap();
        }
    }

    fn region_key(region_x: i32, region_z: i32) -> i64 {
        ChunkPos::pack_coords(region_x, region_z)
    }

    #[test]
    fn read_only_storage_does_not_pad_or_modify_existing_region() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.0.0.mca");
        let bytes = vec![0u8; 8193];
        fs::write(&path, &bytes).unwrap();

        let mut storage = RegionFileStorage::new_read_only(info(true), dir.path().into());
        assert!(storage.read(&ChunkPos::ZERO).unwrap().is_none());
        storage.close().unwrap();

        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn read_only_storage_rejects_corrupt_header_without_backup_or_repair() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.0.0.mca");
        let mut bytes = vec![0u8; 8192];
        bytes[..4].copy_from_slice(&((1i32 << 8) | 1).to_be_bytes());
        fs::write(&path, &bytes).unwrap();

        let mut storage = RegionFileStorage::new_read_only(info(true), dir.path().into());
        let error = storage.read(&ChunkPos::ZERO).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        storage.close().unwrap();

        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn read_only_storage_rejects_truncated_header_without_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.0.0.mca");
        let bytes = vec![7u8; 128];
        fs::write(&path, &bytes).unwrap();

        let mut storage = RegionFileStorage::new_read_only(info(true), dir.path().into());
        let error = storage.read(&ChunkPos::ZERO).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        storage.close().unwrap();

        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn read_only_region_rejects_every_mutation_entry_before_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        write_region(dir.path(), ChunkPos::ZERO, 3, PAPER_CHUNK_0_0);
        let path = dir.path().join("r.0.0.mca");
        let before = fs::read(&path).unwrap();
        let mut region = RegionFile::open_read_only(
            info(true),
            path.clone(),
            dir.path().into(),
            RegionFileVersion::get_selected(),
        )
        .unwrap();

        assert_eq!(
            region
                .get_chunk_data_output_stream(&ChunkPos::ZERO)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            region.flush().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            region.clear(&ChunkPos::ZERO).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            region.write(&ChunkPos::ZERO, &[0; 16]).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            region.recalculate_header().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(
            region
                .get_chunk_data_input_stream(&ChunkPos::ZERO)
                .unwrap()
                .is_some(),
            "failed clear must not change the in-memory header"
        );
        region.close().unwrap();

        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    fn gzip(payload: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload).unwrap();
        encoder.finish().unwrap()
    }

    fn deflate(payload: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload).unwrap();
        encoder.finish().unwrap()
    }

    fn lz4_block(payload: &[u8], compress: bool) -> Vec<u8> {
        let compressed = if compress {
            lz4_flex::block::compress(payload)
        } else {
            payload.to_vec()
        };
        let method = if compress { 0x20 } else { 0x10 };
        let mut framed = Vec::new();
        framed.extend_from_slice(b"LZ4Block");
        framed.push(method | 4); // 1 << (10 + 4) = 16 KiB maximum block.
        framed.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        framed.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        framed.extend_from_slice(&xxhash_rust::xxh32::xxh32(payload, 0x9747_b28c).to_le_bytes());
        framed.extend_from_slice(&compressed);
        framed.extend_from_slice(b"LZ4Block");
        framed.push(0x10);
        framed.extend_from_slice(&[0; 12]);
        framed
    }

    fn encode(tag: &CompoundTag) -> Vec<u8> {
        let mut bytes = Vec::new();
        nbt_io::write(tag, &mut DataOutputStream::new(&mut bytes)).unwrap();
        bytes
    }

    fn chunk_tag(x: i32, z: i32) -> CompoundTag {
        let mut tag = CompoundTag::new();
        tag.put_int("DataVersion", 5000);
        tag.put_int("xPos", x);
        tag.put_int("zPos", z);
        tag
    }

    #[test]
    fn exact_region_filename_mapping_includes_negative_boundaries() {
        for (x, z, expected) in [
            (0, 0, "r.0.0.mca"),
            (31, 31, "r.0.0.mca"),
            (32, 32, "r.1.1.mca"),
            (-1, -1, "r.-1.-1.mca"),
            (-32, -32, "r.-1.-1.mca"),
            (-33, 63, "r.-2.1.mca"),
        ] {
            assert_eq!(RegionFileStorage::get_region_file_name(x, z), expected);
        }
    }

    #[test]
    fn reads_captured_paper_chunk_without_mutating_source_files() {
        let dir = tempfile::tempdir().unwrap();
        write_region(dir.path(), ChunkPos::ZERO, 3, PAPER_CHUNK_0_0);
        let path = dir.path().join("r.0.0.mca");
        let before = fs::read(&path).unwrap();

        let mut storage = RegionFileStorage::new(info(true), dir.path().to_path_buf(), false);
        let tag = storage
            .read(&ChunkPos::ZERO)
            .unwrap()
            .expect("captured chunk");
        assert_eq!(get_chunk_coordinate(&tag), ChunkPos::ZERO);
        storage.close().unwrap();

        assert_eq!(fs::read(&path).unwrap(), before);
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("r.0.0.mca")]);
    }

    #[test]
    fn reads_all_four_registered_payload_codecs() {
        let payloads = [
            (1, gzip(PAPER_CHUNK_0_0)),
            (2, deflate(PAPER_CHUNK_0_0)),
            (3, PAPER_CHUNK_0_0.to_vec()),
            (4, lz4_block(PAPER_CHUNK_0_0, true)),
        ];
        for (version, payload) in payloads {
            let dir = tempfile::tempdir().unwrap();
            write_region(dir.path(), ChunkPos::ZERO, version, &payload);
            let mut storage = RegionFileStorage::new(info(true), dir.path().to_path_buf(), false);
            let tag = storage.read(&ChunkPos::ZERO).unwrap().expect("codec chunk");
            assert_eq!(
                get_chunk_coordinate(&tag),
                ChunkPos::ZERO,
                "codec {version}"
            );
        }
    }

    #[test]
    fn reads_external_lz4_payload_from_mcc_stub() {
        let dir = tempfile::tempdir().unwrap();
        write_region(dir.path(), ChunkPos::ZERO, 0x80 | 4, &[]);
        fs::write(
            dir.path().join("c.0.0.mcc"),
            lz4_block(PAPER_CHUNK_0_0, false),
        )
        .unwrap();
        let mut storage = RegionFileStorage::new(info(true), dir.path().to_path_buf(), false);
        let tag = storage
            .read(&ChunkPos::ZERO)
            .unwrap()
            .expect("external chunk");
        assert_eq!(get_chunk_coordinate(&tag), ChunkPos::ZERO);
    }

    #[test]
    fn absent_read_does_not_create_folder_or_region_and_is_negative_cached() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("missing");
        let mut storage = RegionFileStorage::new(info(true), folder.clone(), false);
        assert!(storage.read(&ChunkPos::ZERO).unwrap().is_none());
        assert!(!folder.exists(), "read must not create the storage folder");

        fs::create_dir(&folder).unwrap();
        write_region(&folder, ChunkPos::ZERO, 3, PAPER_CHUNK_0_0);
        assert!(
            storage.read(&ChunkPos::ZERO).unwrap().is_none(),
            "Paper's negative cache suppresses later filesystem discovery"
        );
    }

    #[test]
    fn uncached_missing_read_at_capacity_closes_and_evicts_lru_before_existence_check() {
        let dir = tempfile::tempdir().unwrap();
        write_empty_regions(dir.path(), 0..MAX_CACHE_SIZE as i32);
        let mut storage = RegionFileStorage::new(info(true), dir.path().to_path_buf(), false);

        for region_x in 0..MAX_CACHE_SIZE as i32 {
            assert!(
                storage
                    .read(&ChunkPos::new(region_x << REGION_SHIFT, 0))
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(storage.region_cache.len(), MAX_CACHE_SIZE);

        // Promote region 0, making region 1 the LRU before the uncached miss.
        assert!(storage.read(&ChunkPos::ZERO).unwrap().is_none());
        let missing_region_x = MAX_CACHE_SIZE as i32;
        assert!(
            storage
                .read(&ChunkPos::new(missing_region_x << REGION_SHIFT, 0))
                .unwrap()
                .is_none()
        );

        assert_eq!(storage.region_cache.len(), MAX_CACHE_SIZE - 1);
        assert_eq!(storage.closed_region_files, [region_key(1, 0)]);
        assert!(
            storage
                .region_cache
                .iter()
                .all(|(key, _)| *key != region_key(1, 0)),
            "the promoted entry must survive and the true LRU must be removed"
        );
        assert_eq!(
            storage.non_existing_region_files[0],
            region_key(missing_region_x, 0)
        );
    }

    #[test]
    fn negative_cache_hit_at_capacity_does_not_evict_or_close_lru() {
        let dir = tempfile::tempdir().unwrap();
        let missing_region_x = MAX_CACHE_SIZE as i32;
        let missing_pos = ChunkPos::new(missing_region_x << REGION_SHIFT, 0);
        let mut storage = RegionFileStorage::new(info(true), dir.path().to_path_buf(), false);

        assert!(storage.read(&missing_pos).unwrap().is_none());
        write_empty_regions(dir.path(), 0..MAX_CACHE_SIZE as i32);
        for region_x in 0..MAX_CACHE_SIZE as i32 {
            assert!(
                storage
                    .read(&ChunkPos::new(region_x << REGION_SHIFT, 0))
                    .unwrap()
                    .is_none()
            );
        }
        let cache_keys_before: Vec<_> = storage.region_cache.iter().map(|(key, _)| *key).collect();

        assert!(storage.read(&missing_pos).unwrap().is_none());

        assert_eq!(storage.region_cache.len(), MAX_CACHE_SIZE);
        assert_eq!(
            storage
                .region_cache
                .iter()
                .map(|(key, _)| *key)
                .collect::<Vec<_>>(),
            cache_keys_before,
            "a negative-cache hit must return before touching the positive LRU"
        );
        assert!(storage.closed_region_files.is_empty());
    }

    #[test]
    fn corrupt_stream_checks_return_absent_in_paper_order() {
        let cases: &[(&str, &[u8])] = &[
            ("missing stream", &[0, 0, 0, 0, 3]),
            ("invalid codec", &[0, 0, 0, 1, 5]),
            ("negative length", &[0xff, 0xff, 0xff, 0xff, 3]),
        ];
        for (name, record) in cases {
            let dir = tempfile::tempdir().unwrap();
            let mut bytes = vec![0u8; 3 * 4096];
            bytes[..4].copy_from_slice(&((2i32 << 8) | 1).to_be_bytes());
            bytes[8192..8192 + record.len()].copy_from_slice(record);
            fs::write(dir.path().join("r.0.0.mca"), bytes).unwrap();
            let mut storage = RegionFileStorage::new(info(false), dir.path().to_path_buf(), false);
            assert!(storage.read(&ChunkPos::ZERO).unwrap().is_none(), "{name}");
        }

        // The stream truncation check uses the actual FileChannel read count,
        // so this controlled negative ends the file seven bytes into sector 2.
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = vec![0u8; 8199];
        bytes[..4].copy_from_slice(&((2i32 << 8) | 1).to_be_bytes());
        bytes[8192..].copy_from_slice(&[0, 0, 0, 9, 3, 1, 2]);
        fs::write(dir.path().join("r.0.0.mca"), bytes).unwrap();
        let mut storage = RegionFileStorage::new(info(false), dir.path().to_path_buf(), false);
        assert!(storage.read(&ChunkPos::ZERO).unwrap().is_none());
    }

    #[test]
    fn custom_codec_read_utf_and_lz4_failures_propagate_as_io() {
        let dir = tempfile::tempdir().unwrap();
        // A readable custom id is a soft null, even when it is a syntactically
        // valid resource identifier.
        write_region(dir.path(), ChunkPos::ZERO, 127, &[0, 1, b'a']);
        let mut storage = RegionFileStorage::new(info(false), dir.path().to_path_buf(), false);
        assert!(storage.read(&ChunkPos::ZERO).unwrap().is_none());

        let dir = tempfile::tempdir().unwrap();
        // `readUTF` declares two bytes but receives one: Paper propagates the
        // EOF IOException instead of converting custom compression to null.
        write_region(dir.path(), ChunkPos::ZERO, 127, &[0, 2, b'a']);
        let mut storage = RegionFileStorage::new(info(false), dir.path().to_path_buf(), false);
        assert_eq!(
            storage.read(&ChunkPos::ZERO).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );

        let dir = tempfile::tempdir().unwrap();
        let mut corrupt_lz4 = lz4_block(PAPER_CHUNK_0_0, true);
        corrupt_lz4[17] ^= 1; // first block's little-endian xxHash32
        write_region(dir.path(), ChunkPos::ZERO, 4, &corrupt_lz4);
        let mut storage = RegionFileStorage::new(info(false), dir.path().to_path_buf(), false);
        assert_eq!(
            storage.read(&ChunkPos::ZERO).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn chunk_coordinate_mismatch_recalculates_then_returns_absent() {
        let dir = tempfile::tempdir().unwrap();
        write_region(dir.path(), ChunkPos::ZERO, 3, &encode(&chunk_tag(1, 0)));
        let mut storage = RegionFileStorage::new(info(true), dir.path().to_path_buf(), false);
        assert!(storage.read(&ChunkPos::ZERO).unwrap().is_none());
    }

    #[test]
    fn legacy_aikar_oversized_lists_merge_after_base_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut base = chunk_tag(0, 0);
        base.put_int("DataVersion", 2500);
        let mut base_level = CompoundTag::new();
        base_level.put_int("xPos", 0);
        base_level.put_int("zPos", 0);
        base_level.put("Entities".to_string(), Tag::List(ListTag::new()));
        base.put("Level".to_string(), Tag::Compound(base_level));
        write_region(dir.path(), ChunkPos::ZERO, 3, &encode(&base));

        let mut extra = CompoundTag::new();
        let mut extra_level = CompoundTag::new();
        let mut entities = ListTag::new();
        entities.add(Tag::Compound(chunk_tag(9, 9)));
        extra_level.put("Entities".to_string(), Tag::List(entities));
        extra.put("Level".to_string(), Tag::Compound(extra_level));
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&encode(&extra)).unwrap();
        fs::write(
            dir.path().join("r.0.0_oversized_0_0.nbt"),
            encoder.finish().unwrap(),
        )
        .unwrap();
        let mut meta = [0u8; 1024];
        meta[0] = 1;
        fs::write(dir.path().join("r.0.0.oversized.nbt"), meta).unwrap();

        let mut storage = RegionFileStorage::new(info(true), dir.path().to_path_buf(), false);
        let merged = storage
            .read(&ChunkPos::ZERO)
            .unwrap()
            .expect("merged chunk");
        assert_eq!(
            merged
                .get_compound("Level")
                .unwrap()
                .get_list("Entities")
                .unwrap()
                .size(),
            1
        );
    }

    #[test]
    fn legacy_aikar_oversized_coordinate_mismatch_recalculates_without_mutating_sources() {
        let dir = tempfile::tempdir().unwrap();
        let region_path = dir.path().join("r.0.0.mca");

        let mut misplaced = chunk_tag(1, 0);
        misplaced.put_int("DataVersion", 2500);
        let mut misplaced_level = CompoundTag::new();
        misplaced_level.put_int("xPos", 1);
        misplaced_level.put_int("zPos", 0);
        misplaced.put("Level".to_string(), Tag::Compound(misplaced_level));

        let mut expected = chunk_tag(0, 0);
        expected.put_int("DataVersion", 2500);
        let mut expected_level = CompoundTag::new();
        expected_level.put_int("xPos", 0);
        expected_level.put_int("zPos", 0);
        expected_level.put("Entities".to_string(), Tag::List(ListTag::new()));
        expected.put("Level".to_string(), Tag::Compound(expected_level));

        let misplaced = encode(&misplaced);
        let expected = encode(&expected);
        let mut region = vec![0u8; 4 * 4096];
        // The stale slot for (0,0) points at sector 2, whose payload belongs to
        // (1,0). Sector 3 contains the correct, currently unlinked payload.
        region[..4].copy_from_slice(&((2i32 << 8) | 1).to_be_bytes());
        for (sector, payload) in [(2usize, misplaced), (3usize, expected)] {
            let start = sector * 4096;
            region[start..start + 4].copy_from_slice(&((payload.len() as i32) + 1).to_be_bytes());
            region[start + 4] = 3;
            region[start + 5..start + 5 + payload.len()].copy_from_slice(&payload);
        }
        fs::write(&region_path, &region).unwrap();

        let mut extra = CompoundTag::new();
        let mut extra_level = CompoundTag::new();
        let mut entities = ListTag::new();
        entities.add(Tag::Compound(chunk_tag(9, 9)));
        extra_level.put("Entities".to_string(), Tag::List(entities));
        extra.put("Level".to_string(), Tag::Compound(extra_level));
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&encode(&extra)).unwrap();
        let oversized_path = dir.path().join("r.0.0_oversized_0_0.nbt");
        fs::write(&oversized_path, encoder.finish().unwrap()).unwrap();

        let mut meta = [0u8; 1024];
        meta[0] = 1;
        let meta_path = dir.path().join("r.0.0.oversized.nbt");
        fs::write(&meta_path, meta).unwrap();

        let before_region = fs::read(&region_path).unwrap();
        let before_oversized = fs::read(&oversized_path).unwrap();
        let before_meta = fs::read(&meta_path).unwrap();

        let mut storage = RegionFileStorage::new(info(true), dir.path().to_path_buf(), false);
        let merged = storage
            .read(&ChunkPos::ZERO)
            .unwrap()
            .expect("recalculated oversized chunk");
        assert_eq!(
            storage.region_cache[0].1.get_recalculate_count(),
            1,
            "the oversized coordinate mismatch must enter the shared retry path"
        );
        assert_eq!(get_chunk_coordinate(&merged), ChunkPos::ZERO);
        assert_eq!(
            merged
                .get_compound("Level")
                .unwrap()
                .get_list("Entities")
                .unwrap()
                .size(),
            1,
            "the oversized supplement is merged into the corrected base payload"
        );
        storage.close().unwrap();

        assert_eq!(fs::read(&region_path).unwrap(), before_region);
        assert_eq!(fs::read(&oversized_path).unwrap(), before_oversized);
        assert_eq!(fs::read(&meta_path).unwrap(), before_meta);
    }

    /// Pin the process-global region-file compression selection to `none` (the
    /// D13 byte-identity gate codec) for the write tests, and serialize against
    /// `region_file_version`'s own `configure`-mutating tests.
    fn with_selection_none<T>(f: impl FnOnce() -> T) -> T {
        let _guard = SELECTION_LOCK.lock().unwrap();
        RegionFileVersion::configure("none");
        f()
    }

    fn writable_storage(dir: &Path) -> RegionFileStorage {
        RegionFileStorage::new(info(true), dir.to_path_buf(), false)
    }

    #[test]
    fn write_creates_region_then_reads_back_and_reopens() {
        with_selection_none(|| {
            let dir = tempfile::tempdir().unwrap();
            let pos = ChunkPos::new(3, 5);
            let tag = chunk_tag(3, 5);
            {
                let mut storage = writable_storage(dir.path());
                storage.write(&pos, Some(tag.clone())).unwrap();
                assert_eq!(
                    storage.read(&pos).unwrap().expect("written chunk"),
                    tag,
                    "write then read in the same storage"
                );
                assert!(
                    dir.path().join("r.0.0.mca").is_file(),
                    "write creates the region file"
                );
                storage.close().unwrap();
            }
            // Reopen: the chunk is on disk and readable through a fresh storage.
            let mut reopened = writable_storage(dir.path());
            assert_eq!(reopened.read(&pos).unwrap().expect("reopened chunk"), tag);
            reopened.close().unwrap();
        });
    }

    #[test]
    fn write_creates_missing_folder_chain() {
        with_selection_none(|| {
            let root = tempfile::tempdir().unwrap();
            let folder = root.path().join("deep/nested/region");
            let pos = ChunkPos::new(0, 0);
            let mut storage = RegionFileStorage::new(info(true), folder.clone(), false);
            storage.write(&pos, Some(chunk_tag(0, 0))).unwrap();
            assert!(
                folder.join("r.0.0.mca").is_file(),
                "write creates the folder chain like FileUtil.createDirectoriesSafe"
            );
            storage.close().unwrap();
        });
    }

    #[test]
    fn write_rewrites_grown_chunk_and_reads_back() {
        with_selection_none(|| {
            let dir = tempfile::tempdir().unwrap();
            let pos = ChunkPos::new(0, 0);
            let mut storage = writable_storage(dir.path());
            storage.write(&pos, Some(chunk_tag(0, 0))).unwrap();

            let mut big = chunk_tag(0, 0);
            big.put_byte_array("filler", vec![0i8; 20_000]);
            storage.write(&pos, Some(big.clone())).unwrap();
            assert_eq!(storage.read(&pos).unwrap().expect("rewritten chunk"), big);
            storage.close().unwrap();

            let mut reopened = writable_storage(dir.path());
            assert_eq!(reopened.read(&pos).unwrap().expect("rewritten chunk"), big);
            reopened.close().unwrap();
        });
    }

    #[test]
    fn delete_clears_chunk_and_is_a_noop_when_region_absent() {
        with_selection_none(|| {
            let dir = tempfile::tempdir().unwrap();
            let pos = ChunkPos::new(1, 1);
            {
                let mut storage = writable_storage(dir.path());
                storage.write(&pos, Some(chunk_tag(1, 1))).unwrap();
                assert!(storage.read(&pos).unwrap().is_some());
                storage.write(&pos, None).unwrap();
                assert!(
                    storage.read(&pos).unwrap().is_none(),
                    "delete clears the chunk"
                );
                storage.close().unwrap();
            }
            assert!(
                dir.path().join("r.0.0.mca").is_file(),
                "delete keeps the region file"
            );
            let mut reopened = writable_storage(dir.path());
            assert!(reopened.read(&pos).unwrap().is_none());
            reopened.close().unwrap();

            // Deleting a chunk in a region that does not exist creates nothing:
            // Paper's `getRegionFile(pos, true)` returns null for a missing
            // region, and the delete returns early.
            let root = tempfile::tempdir().unwrap();
            let missing_folder = root.path().join("missing");
            let mut storage = RegionFileStorage::new(info(true), missing_folder.clone(), false);
            storage.write(&ChunkPos::new(0, 0), None).unwrap();
            assert!(
                !missing_folder.exists(),
                "delete must not create the folder"
            );
            storage.close().unwrap();
        });
    }

    #[test]
    fn write_clears_negative_cache_entry() {
        with_selection_none(|| {
            let dir = tempfile::tempdir().unwrap();
            let pos = ChunkPos::new(0, 0);
            let mut storage = writable_storage(dir.path());
            // Seed the negative cache: reading a missing region marks it missing.
            assert!(storage.read(&pos).unwrap().is_none());
            assert_eq!(storage.non_existing_region_files[0], region_key(0, 0));

            // Paper's create path (`getRegionFile(pos, false)`) calls
            // `createRegionFile`, clearing the negative-cache entry, then opens
            // the fresh file — so a write is served even after a miss.
            storage.write(&pos, Some(chunk_tag(0, 0))).unwrap();
            assert!(
                !storage
                    .non_existing_region_files
                    .contains(&region_key(0, 0)),
                "write clears the negative-cache entry"
            );
            assert_eq!(
                storage.read(&pos).unwrap().expect("written chunk"),
                chunk_tag(0, 0)
            );
            storage.close().unwrap();
        });
    }

    #[test]
    fn cross_region_cache_serves_two_regions() {
        with_selection_none(|| {
            let dir = tempfile::tempdir().unwrap();
            let a = ChunkPos::new(0, 0); // r.0.0.mca
            let b = ChunkPos::new(32, 32); // r.1.1.mca
            let mut storage = writable_storage(dir.path());
            storage.write(&a, Some(chunk_tag(0, 0))).unwrap();
            storage.write(&b, Some(chunk_tag(32, 32))).unwrap();
            assert_eq!(storage.region_cache.len(), 2);
            assert_eq!(
                storage.read(&a).unwrap().expect("region 0 chunk"),
                chunk_tag(0, 0)
            );
            assert_eq!(
                storage.read(&b).unwrap().expect("region 1 chunk"),
                chunk_tag(32, 32)
            );
            storage.close().unwrap();
        });
    }

    #[test]
    fn oversized_chunk_write_uses_external_mcc_and_reads_back() {
        with_selection_none(|| {
            let dir = tempfile::tempdir().unwrap();
            let pos = ChunkPos::new(2, 2);
            let mut storage = writable_storage(dir.path());
            let mut tag = chunk_tag(2, 2);
            // A > 256-sector record (1.2 MiB at `none`) redirects to `.mcc`.
            tag.put_byte_array("big", vec![0i8; 1_200_000]);
            storage.write(&pos, Some(tag.clone())).unwrap();
            assert!(
                dir.path().join("c.2.2.mcc").is_file(),
                "oversized chunk is written to the external .mcc file"
            );
            assert_eq!(storage.read(&pos).unwrap().expect("oversized chunk"), tag);
            storage.close().unwrap();
        });
    }

    #[test]
    fn flush_persists_then_reopen_reads() {
        with_selection_none(|| {
            let dir = tempfile::tempdir().unwrap();
            let pos = ChunkPos::new(0, 0);
            let mut storage = writable_storage(dir.path());
            storage.write(&pos, Some(chunk_tag(0, 0))).unwrap();
            storage.flush().unwrap();
            storage.close().unwrap();

            let mut reopened = writable_storage(dir.path());
            assert!(reopened.read(&pos).unwrap().is_some());
            reopened.close().unwrap();
        });
    }

    #[test]
    fn region_file_size_error_is_recognized() {
        let size_error: io::Error = RegionFileSizeException { count: 42 }.into();
        assert!(is_region_file_size(&size_error));
        assert!(!is_region_file_size(&io::Error::other("not a size error")));
    }

    #[test]
    fn corrupt_stream_write_is_treated_absent_like_paper() {
        with_selection_none(|| {
            let dir = tempfile::tempdir().unwrap();
            let pos = ChunkPos::new(0, 0);
            {
                let mut storage = writable_storage(dir.path());
                storage.write(&pos, Some(chunk_tag(0, 0))).unwrap();
                storage.close().unwrap();
            }
            // Corrupt the written chunk's stream length to a huge value; the
            // readable region degrades it to absent (recalc relinks to 0).
            let path = dir.path().join("r.0.0.mca");
            {
                let mut f = OpenOptions::new().write(true).open(&path).unwrap();
                f.seek(SeekFrom::Start(2 * 4096)).unwrap();
                f.write_all(&i32::MAX.to_be_bytes()).unwrap();
            }
            let mut storage = writable_storage(dir.path());
            assert!(
                storage.read(&pos).unwrap().is_none(),
                "corrupt stream is treated as absent"
            );
            storage.close().unwrap();
        });
    }

    #[test]
    fn read_only_storage_rejects_every_write_mutation_before_side_effects() {
        // The strict M2L read-only boundary: no create, no delete, no flush may
        // touch the filesystem. Every attempt rejects with PermissionDenied
        // before the folder is created or a region is loaded, and no bytes or
        // files change.
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("world/region");
        let mut storage = RegionFileStorage::new_read_only(info(true), folder.clone());

        // Create: rejects before the folder is created.
        assert_eq!(
            storage
                .write(&ChunkPos::new(0, 0), Some(chunk_tag(0, 0)))
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(
            !folder.exists(),
            "a read-only write must not create the storage folder"
        );
        assert!(storage.region_cache.is_empty());

        // Delete: rejects before the region is loaded or touched.
        fs::create_dir_all(&folder).unwrap();
        write_region(&folder, ChunkPos::ZERO, 3, PAPER_CHUNK_0_0);
        let region_path = folder.join("r.0.0.mca");
        let before = fs::read(&region_path).unwrap();
        assert_eq!(
            storage.write(&ChunkPos::ZERO, None).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            fs::read(&region_path).unwrap(),
            before,
            "a read-only delete must not mutate region bytes"
        );
        assert_eq!(
            fs::read_dir(&folder).unwrap().count(),
            1,
            "a read-only delete must not create or remove files"
        );
        assert!(
            storage.region_cache.is_empty(),
            "a read-only delete must not load the region"
        );

        // Flush: rejects in read-only mode.
        assert_eq!(
            storage.flush().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );

        // Reads still work, and close is descriptor-only.
        assert!(storage.read(&ChunkPos::ZERO).unwrap().is_some());
        storage.close().unwrap();
        assert_eq!(
            fs::read(&region_path).unwrap(),
            before,
            "close must not mutate bytes"
        );
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
    }
}
