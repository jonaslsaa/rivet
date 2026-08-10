//! Read-only port of Paper 26.2's `RegionFileStorage` direct-read slice.
//!
//! This intentionally stops before `SerializableChunkData`, `IOWorker`, world
//! boot, generation, and every write/delete/flush API. Reads open only an
//! already-existing `r.<regionX>.<regionZ>.mca`, retain Paper's LRU and
//! negative-cache ordering, delegate stream corruption handling to
//! `RegionFile`, parse NBT, apply the chunk-coordinate guard, and merge legacy
//! Aikar oversized supplements.

use std::io;
use std::path::{Path, PathBuf};

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_io;
use rivet_nbt::tag::Tag;
use rivet_registry::core::ChunkPos;
use rivet_util::data_io::DataInputStream;

use super::region_file::{RegionFile, get_chunk_coordinate};
use super::region_file_version::RegionFileVersion;
use super::region_storage_info::RegionStorageInfo;

pub const ANVIL_EXTENSION: &str = ".mca";
const MAX_CACHE_SIZE: usize = 256;
const REGION_SHIFT: i32 = 5;
const MAX_NON_EXISTING_CACHE: usize = 1024 * 4;

/// The synchronous region-file cache used by Paper's direct storage reads.
///
/// Entries are stored most-recent-first, matching
/// `Long2ObjectLinkedOpenHashMap.getAndMoveToFirst`; the last entry is closed
/// on eviction. The type exposes no write, delete, create, or flush operation.
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

    /// Existing-only storage for the loaded-world extractor and world boot: no
    /// create/write descriptor, repair, backup, padding, or sync-on-close
    /// behavior. A corrupt allocated chunk is a hard `InvalidData` error, never
    /// an absent chunk, so a disposable copy can never be mistaken for a
    /// different world.
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

    /// Close every cached region, attempting all closes and returning the first
    /// error like Paper's `ExceptionCollector` boundary.
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
    use std::fs;
    use std::io::Write as _;

    use flate2::Compression;
    use flate2::write::{GzEncoder, ZlibEncoder};
    use rivet_nbt::list_tag::ListTag;
    use rivet_nbt::tag::Tag;
    use rivet_util::data_io::DataOutputStream;

    use super::*;

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

    fn gzip(payload: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload).unwrap();
        encoder.finish().unwrap()
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
}
