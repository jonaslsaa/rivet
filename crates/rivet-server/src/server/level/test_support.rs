//! Test-only world-synthesis helpers shared by the region-backed test modules
//! (`server/level/region_backed.rs` and the session's region-backed recenter
//! tests in `server/player/session.rs`). Every helper synthesizes a disposable
//! overworld into a fresh `tempfile::TempDir` — the launcher save and the
//! `working/` tree are never touched.
//!
//! The world shape mirrors the pinned real New World the #371 corpus was
//! captured from: `level.dat` with `Data.DataVersion` 4903 and spawn
//! (-16,68,-48) overworld, `world_gen_settings.dat` seed, and the exact
//! 117-chunk view-distance-4 square centered on the spawn chunk, every position
//! installed with the committed clean spawn chunk (`xPos`/`zPos` rewritten per
//! slot). `write_entered_cells` augments the region with the beyond-view cells a
//! movement recenter enters on demand (issue #185).

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::float_tag::FloatTag;
use rivet_nbt::int_array_tag::IntArrayTag;
use rivet_nbt::nbt_io;
use rivet_nbt::string_tag::StringTag;
use rivet_nbt::tag::Tag;
use rivet_registry::core::ChunkPos;
use rivet_util::DataInputStream;
use rivet_util::data_io::DataOutputStream;

use crate::server::level::chunk_tracking_view::ChunkTrackingView;
use crate::server::level::region_backed::EXPECTED_DATA_VERSION;

/// The pinned real world values the #371 loaded-world corpus was captured
/// from: the launcher New World's `level.dat` `Data` compound (DataVersion
/// 4903, spawn (-16,68,-48) overworld) and its `world_gen_settings.dat`
/// seed. These mirror the disposable copy read by the boot; the committed
/// `fixtures/level.dat` is a different, older capture (spawn (0,-60,0)).
pub const REAL_SPAWN: [i32; 3] = [-16, 68, -48];
pub const REAL_SEED: i64 = 9_110_734_097_863_663_269;

/// The committed loaded-world spawn-chunk fixture (-1.-3.nbt).
pub fn loaded_world_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/rivet-oracle/fixtures/loaded-world/chunk/-1.-3.nbt")
}

/// Load a committed loaded-world chunk NBT (raw uncompressed).
pub fn load_fixture(fixture: &Path) -> CompoundTag {
    let bytes = fs::read(fixture).expect("loaded-world fixture readable");
    let mut input = DataInputStream::new(Cursor::new(bytes));
    nbt_io::read_unlimited(&mut input).expect("loaded-world fixture parses")
}

/// Build a temp disposable world rooted at `temp` that the boot can fully
/// compose: a real `level.dat` (pinned spawn), a real seed, and the exact
/// 117-chunk view-distance-4 square centered on the spawn chunk, every
/// position installed with the committed clean loaded-world spawn chunk
/// (xPos/zPos rewritten per slot). All files are synthesized into the fresh
/// temp copy — the launcher save and the `working/` tree are never touched.
pub fn loaded_world_root(temp: &tempfile::TempDir) {
    write_level_dat(temp.path(), REAL_SPAWN);
    write_world_gen_settings(temp.path(), REAL_SEED);
    let region_dir = temp.path().join("dimensions/minecraft/overworld/region");
    fs::create_dir_all(&region_dir).unwrap();
    write_view_chunks(
        &region_dir,
        &[(ChunkPos::new(-1, -3), loaded_world_fixture())],
    );
}

/// A chunk payload for [`write_region_chunks`]: a valid serialized chunk, or
/// raw bytes written verbatim (a corrupt chunk whose NBT decode fails at
/// the storage boundary).
#[derive(Clone)]
pub enum ChunkPayload {
    Valid(CompoundTag),
    Raw(Vec<u8>),
}

/// Write many chunk payloads into their Anvil region files (grouped by
/// region coordinate, so multiple chunks share one file). `Valid` entries
/// are serialized with `nbt_io::write` (matching the single-chunk
/// `write_region_nbt`); `Raw` entries are placed byte-for-byte. The region
/// header layout mirrors `write_region_nbt`.
///
/// A call MERGES into an existing region file instead of rebuilding it: the
/// header of a file already on disk is carried over and new entries append
/// to the body, so a later write (e.g. the beyond-view enter cells the
/// movement tests place after `loaded_world_root`) augments chunks the
/// earlier boot-view write already placed rather than clobbering them.
pub fn write_region_chunks(region_dir: &Path, chunks: &[(ChunkPos, ChunkPayload)]) {
    use std::collections::BTreeMap;
    let mut regions: BTreeMap<(i32, i32), Vec<(ChunkPos, ChunkPayload)>> = BTreeMap::new();
    for (pos, payload) in chunks {
        regions
            .entry((pos.x() >> 5, pos.z() >> 5))
            .or_default()
            .push((*pos, payload.clone()));
    }
    for ((rx, rz), entries) in regions {
        let path = region_dir.join(format!("r.{rx}.{rz}.mca"));
        let (mut header, mut body) = if path.exists() {
            let existing = fs::read(&path).unwrap();
            (existing[..8192].to_vec(), existing[8192..].to_vec())
        } else {
            (vec![0u8; 8192], Vec::new())
        };
        // The next free sector follows whatever body the file already
        // carries (the header occupies sectors 0..2).
        let mut sector = (body.len() / 4096) + 2;
        for (pos, payload) in entries {
            let nbt = match payload {
                ChunkPayload::Valid(tag) => {
                    let mut nbt = Vec::new();
                    nbt_io::write(&tag, &mut DataOutputStream::new(&mut nbt)).unwrap();
                    nbt
                }
                ChunkPayload::Raw(bytes) => bytes,
            };
            let length = nbt.len() + 1; // the +1 is the compression-type byte.
            let sectors = length.div_ceil(4096);
            let slot = ((pos.x() & 31) + (pos.z() & 31) * 32) as usize;
            header[slot * 4..slot * 4 + 4]
                .copy_from_slice(&(((sector as i32) << 8) | sectors as i32).to_be_bytes());
            let mut data = Vec::with_capacity(4 + length);
            data.extend_from_slice(&(length as i32).to_be_bytes());
            data.push(3); // compression type (uncompressed, like `write_region_nbt`).
            data.extend_from_slice(&nbt);
            data.resize(sectors * 4096, 0);
            body.extend_from_slice(&data);
            sector += sectors;
        }
        let mut region = Vec::with_capacity(header.len() + body.len());
        region.extend_from_slice(&header);
        region.extend_from_slice(&body);
        fs::write(path, region).unwrap();
    }
}

/// Write the exact 117-chunk view square, every position installed with the
/// committed clean spawn chunk (or the caller's override for that position).
/// `overrides` maps a view position to a fixture path written there instead.
pub fn write_view_chunks(region_dir: &Path, overrides: &[(ChunkPos, PathBuf)]) -> Vec<ChunkPos> {
    let view = ChunkTrackingView::of(ChunkPos::new(-1, -3), 4);
    let mut positions = Vec::with_capacity(view.chunk_count());
    let mut chunks = Vec::with_capacity(view.chunk_count());
    let override_for = |pos: ChunkPos| -> Option<&PathBuf> {
        overrides.iter().find(|(p, _)| *p == pos).map(|(_, f)| f)
    };
    view.for_each(|pos| {
        positions.push(pos);
        let fixture = override_for(pos)
            .cloned()
            .unwrap_or_else(loaded_world_fixture);
        let mut chunk = load_fixture(&fixture);
        chunk.put_int("xPos", pos.x());
        chunk.put_int("zPos", pos.z());
        chunks.push((pos, ChunkPayload::Valid(chunk)));
    });
    write_region_chunks(region_dir, &chunks);
    positions
}

/// Write a gzip `level.dat` with `Data.DataVersion` 4903 and the given
/// spawn (the `RespawnData.CODEC` NBT shape: `dimension` string, `pos` int
/// array, `yaw`/`pitch` floats).
pub fn write_level_dat(root: &Path, spawn_pos: [i32; 3]) {
    let mut spawn = CompoundTag::new();
    spawn.put(
        "pos".to_string(),
        Tag::IntArray(IntArrayTag::new(spawn_pos.to_vec())),
    );
    spawn.put(
        "dimension".to_string(),
        Tag::String(StringTag::value_of("minecraft:overworld".to_string())),
    );
    spawn.put("yaw".to_string(), Tag::Float(FloatTag::new(0.0)));
    spawn.put("pitch".to_string(), Tag::Float(FloatTag::new(0.0)));
    let mut data = CompoundTag::new();
    data.put_int("DataVersion", EXPECTED_DATA_VERSION);
    data.put("spawn".to_string(), Tag::Compound(spawn));
    let mut level = CompoundTag::new();
    level.put("Data".to_string(), Tag::Compound(data));
    let mut bytes = Vec::new();
    nbt_io::write_compressed(&level, &mut bytes).unwrap();
    fs::write(root.join("level.dat"), bytes).unwrap();
}

/// Write a gzip `world_gen_settings.dat` with the real overworld generator
/// shape (`minecraft:noise` — not flat) and `data.seed` — the modern (26.2)
/// home of the world seed and of the generator type `ServerLevel.isFlat()`
/// reads.
pub fn write_world_gen_settings(root: &Path, seed: i64) {
    write_world_gen_settings_type(root, seed, "minecraft:noise");
}

/// As [`write_world_gen_settings`], with an explicit overworld generator
/// `type` (the flat-login test uses `minecraft:flat` → the booted world is
/// flat, like a `FlatLevelSource`).
pub fn write_world_gen_settings_type(root: &Path, seed: i64, generator_type: &str) {
    let mut generator = CompoundTag::new();
    generator.put_string("type", generator_type);
    let mut overworld = CompoundTag::new();
    overworld.put("generator".to_string(), Tag::Compound(generator));
    let mut dimensions = CompoundTag::new();
    dimensions.put("minecraft:overworld".to_string(), Tag::Compound(overworld));
    let mut data = CompoundTag::new();
    data.put_long("seed", seed);
    data.put("dimensions".to_string(), Tag::Compound(dimensions));
    let mut settings = CompoundTag::new();
    settings.put("data".to_string(), Tag::Compound(data));
    let mut bytes = Vec::new();
    nbt_io::write_compressed(&settings, &mut bytes).unwrap();
    fs::create_dir_all(root.join("data/minecraft")).unwrap();
    fs::write(root.join("data/minecraft/world_gen_settings.dat"), bytes).unwrap();
}

/// Rewrite a top-level tick list's `x`/`z` block coordinates into the given
/// chunk's bounds. Stored ticks are decoded and filtered to the chunk at
/// parse time (`filter_tick_list_for_chunk`), so an aux fixture carried at
/// the spawn chunk position must also carry its tick entries inside
/// (-1,-3)'s 16-block bounds or they are dropped before the boundary
/// check.
pub fn relocate_ticks(chunk: &mut CompoundTag, field: &str, pos: ChunkPos) {
    let ticks = chunk.get_list_or_empty_mut(field);
    for index in 0..ticks.size() {
        let tick = ticks.get_compound_or_empty_mut(index);
        tick.put_int("x", pos.x() * 16);
        tick.put_int("z", pos.z() * 16);
    }
}

/// The cells the spawn view ((−1,−3), send 4) enters on a one-chunk-east
/// recenter to (0,−3) — the deterministic `ChunkTrackingView::difference`
/// enter set (11 cells): the new view's x=5 column (z=−7..1) plus the two
/// corner-shift cells (4,−8)/(4,2). Every entered cell lies OUTSIDE the
/// boot-time 117-chunk square (the square's max x is 4, and both corner
/// cells are boot-time corners), so the movement-driven recenter (issue
/// #185) must load each on demand from the region source instead of
/// disconnecting.
pub fn spawn_east_move_enter() -> Vec<ChunkPos> {
    let boot = ChunkTrackingView::of(ChunkPos::new(-1, -3), 4);
    let next = ChunkTrackingView::of(ChunkPos::new(0, -3), 4);
    let mut enter = Vec::new();
    ChunkTrackingView::difference(&boot, &next, |pos| enter.push(pos), |_| {});
    assert_eq!(
        enter.len(),
        11,
        "one-chunk east move enters exactly 11 cells"
    );
    for pos in &enter {
        assert!(
            !boot.contains_pos(pos),
            "every entered cell is outside the boot view: {pos}"
        );
    }
    enter
}

/// Write the entered beyond-view cells into the region (in addition to the
/// 117-chunk boot view `loaded_world_root` already wrote), each carrying
/// the committed clean spawn fixture with its coordinates rewritten.
pub fn write_entered_cells(region_dir: &Path, enter: &[ChunkPos]) {
    let mut extras = Vec::with_capacity(enter.len());
    for pos in enter {
        let mut chunk = load_fixture(&loaded_world_fixture());
        chunk.put_int("xPos", pos.x());
        chunk.put_int("zPos", pos.z());
        extras.push((*pos, ChunkPayload::Valid(chunk)));
    }
    write_region_chunks(region_dir, &extras);
}
