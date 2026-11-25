//! `carver.carve`: carve a chunk grid with one configured carver.
//!
//! The contract (identical to `RivetReferenceOracle.carverCarve`): the target
//! chunks are the 3 x 3 grid around the origin chunk, each a copy of the
//! `stone` profile's fill (stone for y <= 63, air above). Each is carved the
//! way `NoiseBasedChunkGenerator.applyCarvers` loops: for every source chunk
//! within 8, `WorldgenRandom(LegacyRandomSource(0)).setLargeFeatureSeed(seed +
//! index, x, z)`, then `isStartChunk` and `carve`. The aquifer is
//! `Aquifer.createDisabled` over the global fluid picker (lava below y -54,
//! water below the overworld sea level 63) and there is no surface rule, so
//! carved blocks below the sea level fill with water and none is replaced by a
//! top material. `index` is the carver's position in its dimension's carver
//! list; it salts the seed.
//!
//! Per target chunk the answer is `{chunk, starts, carved, mask, changed}`:
//! the number of source chunks that were start chunks, the number whose
//! `carve` returned true, the set bits of the `CarvingMask` (index `x | z << 4
//! | (y + 64) << 8`) as `[start, length]` runs, and the blocks that differ from
//! the base fill as `[start, length, state id]` runs over the same index.

use rivet_registry::ResourceKey;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, ChunkPos};
use rivet_util::random::LegacyRandomSource;
use rivet_util::worldgen_random::WorldgenRandom;
use rivet_world::block::blocks::Blocks;
use rivet_world::chunk::carving_mask::CarvingMask;
use rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries;
use rivet_world::levelgen::carver::carving_context::CarvingContext;
use rivet_world::levelgen::carver::configured_world_carver::ConfiguredWorldCarverErased;
use rivet_world::levelgen::carver::overworld_carvers::{
    canyon_carver, cave_carver, cave_extra_underground_carver,
};
use rivet_world::levelgen::noisegen::aquifer::{self, FluidPicker, FluidStatus};
use rivet_world::levelgen::noisegen::noise_generator_settings::OVERWORLD;
use rivet_world::levelgen::noisegen::random_state::RandomState;
use serde_json::{Map, Value, json};

use super::feature::{HEIGHT, MIN_Y, RegionGenerator, copy_chunk, profile, template};
use super::{is_unported_panic, panic_message, request_i64, request_str};
use crate::ops::compare::Outcome;
use crate::ops::packet;

/// Half-width of the target grid (`RivetReferenceOracle.CARVER_GRID`).
const GRID: i32 = 1;
const SEA_LEVEL: i32 = 63;

/// The carver's index in its dimension's carver list (the seed salt): the
/// overworld biomes list cave, cave_extra_underground, canyon; the nether
/// biomes list nether_cave alone.
pub fn index_of(key: &str) -> i64 {
    match key {
        "minecraft:cave_extra_underground" => 1,
        "minecraft:canyon" => 2,
        _ => 0,
    }
}

/// The global fluid picker: lava below y -54, the default fluid (water) below the sea level.
struct GlobalFluid {
    lava: FluidStatus,
    sea: FluidStatus,
}

impl FluidPicker for GlobalFluid {
    fn compute_fluid(&self, _x: i32, y: i32, _z: i32) -> FluidStatus {
        if y < (-54).min(SEA_LEVEL) {
            self.lava
        } else {
            self.sea
        }
    }
}

fn configured(key: &str) -> Result<ConfiguredWorldCarverErased, Outcome> {
    match key {
        "minecraft:cave" => Ok(cave_carver()),
        "minecraft:cave_extra_underground" => Ok(cave_extra_underground_carver()),
        "minecraft:canyon" => Ok(canyon_carver()),
        other => Err(Outcome::Unported(format!(
            "carver {other} is not ported (Rivet ports the overworld cave, cave_extra_underground and canyon carvers)"
        ))),
    }
}

thread_local! {
    static RANDOM_STATE: &'static RandomState<'static> = {
        let access: &'static _ = Box::leak(Box::new(build_worldgen_registries()));
        let overworld: &'static ResourceKey<_> = &OVERWORLD;
        Box::leak(Box::new(RandomState::create_from_provider(access, overworld, 0)))
    };
}

fn runs(bits: impl Iterator<Item = usize>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut run: Option<(usize, usize)> = None;
    for bit in bits {
        run = match run {
            Some((start, len)) if start + len == bit => Some((start, len + 1)),
            Some((start, len)) => {
                out.push(json!([start, len]));
                Some((bit, 1))
            }
            None => Some((bit, 1)),
        };
    }
    if let Some((start, len)) = run {
        out.push(json!([start, len]));
    }
    out
}

/// `carver.carve`: `{carver, seed, index}` -> `{chunks}`.
pub fn carver_carve(request: &Map<String, Value>) -> Outcome {
    let key = match request_str(request, "carver") {
        Ok(key) => key,
        Err(outcome) => return outcome,
    };
    let seed = match request_i64(request, "seed") {
        Ok(seed) => seed,
        Err(outcome) => return outcome,
    };
    let index = match request_i64(request, "index") {
        Ok(index) => index,
        Err(outcome) => return outcome,
    };
    let carver = match configured(key) {
        Ok(carver) => carver,
        Err(outcome) => return outcome,
    };
    match packet::guarded(|| carve_grid(&carver, seed, index)) {
        Ok(result) => Outcome::Accepted(result),
        Err(payload) => {
            let message = panic_message(payload.as_ref());
            if is_unported_panic(&message) {
                Outcome::Unported(message.lines().next().unwrap_or("").to_owned())
            } else {
                Outcome::Unavailable(format!("Rust panic: {message}"))
            }
        }
    }
}

fn carve_grid(carver: &ConfiguredWorldCarverErased, seed: i64, index: i64) -> Map<String, Value> {
    let stone = profile("stone").expect("stone profile exists");
    let generator = RegionGenerator {
        sea_level: SEA_LEVEL,
    };
    let aquifer = aquifer::create_disabled(Box::new(GlobalFluid {
        lava: FluidStatus {
            fluid_level: -54,
            fluid_type: Blocks::LAVA.default_block_state(),
        },
        sea: FluidStatus {
            fluid_level: SEA_LEVEL,
            fluid_type: Blocks::WATER.default_block_state(),
        },
    }));
    let air = Blocks::AIR.default_block_state();
    let stone_state = Blocks::STONE.default_block_state();
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    let mut chunks = Vec::new();
    RANDOM_STATE.with(|random_state| {
        for cx in -GRID..=GRID {
            for cz in -GRID..=GRID {
                let mut chunk = copy_chunk(&template(stone, cx, cz));
                let context =
                    CarvingContext::new(&generator, &chunk.height_accessor(), random_state);
                let mut mask = CarvingMask::new(HEIGHT, MIN_Y);
                let (mut starts, mut carved) = (0, 0);
                for dx in -8..=8 {
                    for dz in -8..=8 {
                        let source = ChunkPos::new(cx + dx, cz + dz);
                        random.set_large_feature_seed(
                            seed.wrapping_add(index),
                            source.x(),
                            source.z(),
                        );
                        if carver.is_start_chunk(&mut random) {
                            starts += 1;
                            if carver.carve(
                                &context,
                                &mut chunk,
                                &mut random,
                                aquifer.as_ref(),
                                &source,
                                &mut mask,
                            ) {
                                carved += 1;
                            }
                        }
                    }
                }
                let words = mask.to_array();
                let set = words.iter().enumerate().flat_map(|(word, bits)| {
                    (0..64).filter_map(move |bit| {
                        ((*bits as u64) >> bit & 1 == 1).then_some(word * 64 + bit)
                    })
                });
                let mask_runs = runs(set);
                let origin = ChunkPos::new(cx, cz);
                let mut changed = Vec::new();
                let mut run: Option<(usize, usize, u16)> = None;
                let column = 256 * HEIGHT as usize;
                for at in 0..=column {
                    let id = (at < column).then(|| {
                        let y = MIN_Y + (at >> 8) as i32;
                        let pos = BlockPos::new(
                            origin.get_min_block_x() + (at & 15) as i32,
                            y,
                            origin.get_min_block_z() + (at >> 4 & 15) as i32,
                        );
                        let state: BlockState = chunk.get_block_state(pos.get_x(), y, pos.get_z());
                        let base = if y <= 63 { stone_state } else { air };
                        (state != base).then(|| state.id().0)
                    });
                    let id = id.flatten();
                    if let Some((start, len, state)) = run
                        && id != Some(state)
                    {
                        changed.push(json!([start, len, state]));
                        run = None;
                    }
                    if let Some(id) = id {
                        run = match run {
                            Some((start, len, state)) => Some((start, len + 1, state)),
                            None => Some((at, 1, id)),
                        };
                    }
                }
                let mut entry = Map::new();
                entry.insert("chunk".into(), json!([cx, cz]));
                entry.insert("starts".into(), json!(starts));
                entry.insert("carved".into(), json!(carved));
                entry.insert("mask".into(), Value::Array(mask_runs));
                entry.insert("changed".into(), Value::Array(changed));
                chunks.push(Value::Object(entry));
            }
        }
    });
    let mut result = Map::new();
    result.insert("chunks".into(), Value::Array(chunks));
    result
}
