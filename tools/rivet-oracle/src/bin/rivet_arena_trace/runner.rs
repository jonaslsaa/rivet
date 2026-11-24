//! L4 arena tick-trace runner: replays a Paper-resolved arena on Rivet's live
//! level runtime and emits the `rivet.arena-trace/1` schema
//! (`tools/rivet-oracle/arenas/README.md`, docs/VERIFICATION.md section 2.2).
//!
//! The runner lives in the tool, over rivet-server's public level API, so no
//! test support is compiled into the server library. It owns no game logic.
//! It builds a [`ServerLevel`] with the canonical owners the live paths
//! require, applies the arena through `set_block_live`, and drives the
//! `ServerLevel.tick` phases that have a Rust entry point, in Paper's order:
//!
//! | Paper `ServerLevel.tick` phase | Here |
//! |---|---|
//! | clock managers (`MinecraftServer.tickChildren`), world border | `LevelClockRuntime::tick`, `tick_world_border` |
//! | `tickTime` | `tick_world_time` |
//! | `blockTicks.tick(.., this::tickBlock)` | `run_block_ticks_live` (`ServerLevel::tick_block_live`) |
//! | `fluidTicks.tick(.., this::tickFluid)` | `run_fluid_ticks_live` |
//! | raids, `chunkSource.tick` (random ticks at speed 0, no spawning) | nothing runs in Paper either |
//! | `runBlockEvents` | `ServerLevel` has no block-event queue: no Rust path can queue one |
//! | entity ticking | no entity owner is bound: entity admission refuses (`MissingOwner`) |
//! | `tickBlockEntities` | `tick_block_entities` |
//!
//! A Rust authority that is missing or refuses (an `Unavailable` error from a
//! live path, a block tick without a Rust body, an entity the runner cannot
//! render) ends the trace with an `error` line at that tick. The comparator
//! reports it as the first divergence; nothing is substituted for the missing
//! surface.
//!
//! Inputs come from the Paper header line, which carries the arena resolved to
//! registry ids (block-state parsing lives in Paper's `BlockStateParser`).
use rivet_nbt::{
    compound_tag::CompoundTag, string_tag::StringTag, tag::Tag, tag_parser::parse_compound_fully,
};
use rivet_registry::{
    block_state::BlockState,
    core::{BlockPos, ChunkPos},
    fluid_id::FluidId,
    generated::block_states::StateId,
    identifier::Identifier,
};
use rivet_server::server::level::{
    ServerLevel,
    level_block_entity_runtime::{
        BlockEntityTickerError, LevelBlockEntityRuntime, NativeTickerKind,
    },
    level_chunk::LevelChunk,
    level_clock_runtime::LevelClockRuntime,
    native_world_services::{NativeWorldEffects, NativeWorldServices},
    server_level::{
        BlockEntityWorldServices, LevelWorldTime, LiveBlockEffect, LiveBlockMutationError,
        MissingChunkPolicy, ServerLevelConfig, UnavailableScheduledFunctions,
    },
    world_events::NativeWorldEventHost,
    world_runtime_settings::WorldRuntimeSettings,
};
use rivet_server::server::{level::ChunkMap, lighting::LiveLightRuntime};
use rivet_util::{
    RandomSource,
    mth::Uuid,
    random::{ThreadUnsafeRandom, random_support::mix_stafford13},
};
use rivet_world::{
    block::Block,
    fluid::{FluidError, FluidResult, FluidState},
    level::{block_shape_context::ShapeGeometry, height_accessor::create},
    ticks::{ScheduledTick, TickPriority},
};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

pub const SCHEMA: &str = "rivet.arena-trace/1";
/// UPDATE_KNOWN_SHAPE | UPDATE_SUPPRESS_DROPS | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS
/// | UPDATE_SKIP_ON_PLACE: the probe's quiet arena placement.
const QUIET: u32 = 16 | 32 | 256 | 512;
/// `Level.setBlock(pos, state, flags)` delegates with `UPDATE_LIMIT`.
const UPDATE_LIMIT: i32 = 512;
/// Seed-injection site 1 (`tools/rivet-oracle/arenas/seed-sites.tsv`).
const SITE_LEVEL_RANDOM: i64 = 1;
/// The arena boot's `max-chained-neighbor-updates`.
const MAX_CHAINED_NEIGHBOR_UPDATES: i32 = 1_000_000;
/// Fixed world identity: native event dispatch is keyed by world UUID. It is
/// never observed by the trace (entity NBT world keys are excluded).
const WORLD_UUID: Uuid = Uuid {
    most: 0x5249_5645_5441_5245,
    least: 0x4e41_5452_4143_4531,
};

/// Field-exclusion table (`tools/rivet-oracle/arenas/exclusions.tsv`): top-level
/// NBT keys both emitters drop. A key enters only when Paper twin runs differ.
#[derive(Debug, Default, Clone)]
pub struct Exclusions {
    pub block_entity_nbt: Vec<String>,
    pub entity_nbt: Vec<String>,
}

impl Exclusions {
    pub fn parse(tsv: &str) -> Result<Self, String> {
        let mut out = Self::default();
        for line in tsv.lines() {
            if line.trim().is_empty() || line.starts_with('#') || line.starts_with("scope\t") {
                continue;
            }
            let cells: Vec<_> = line.split('\t').collect();
            if cells.len() != 4 || cells[2].trim().is_empty() {
                return Err(format!(
                    "exclusion row needs scope, key, reason, issue: {line:?}"
                ));
            }
            match cells[0] {
                "block_entity_nbt" => out.block_entity_nbt.push(cells[1].into()),
                "entity_nbt" => out.entity_nbt.push(cells[1].into()),
                other => return Err(format!("unknown exclusion scope {other:?}")),
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone)]
enum ScriptOp {
    Set {
        pos: [i32; 3],
        state: BlockState,
        flags: u32,
    },
    ScheduleBlock {
        pos: [i32; 3],
        block: Block,
        delay: i32,
        priority: TickPriority,
    },
    ScheduleFluid {
        pos: [i32; 3],
        fluid: FluidId,
        delay: i32,
        priority: TickPriority,
    },
}

/// The Paper header line, parsed.
#[derive(Debug, Clone)]
pub struct ArenaHeader {
    raw: Value,
    pub name: String,
    origin: BlockPos,
    size: [i32; 3],
    pub ticks: i32,
    seed: i64,
    opt_in: Vec<String>,
    placements: Vec<([i32; 3], BlockState)>,
    block_entities: Vec<([i32; 3], CompoundTag)>,
    script: Vec<(i32, ScriptOp)>,
}

fn int(value: &Value, what: &str) -> Result<i64, String> {
    value
        .as_i64()
        .ok_or_else(|| format!("header {what} is not an integer: {value}"))
}
fn triple(value: &Value, what: &str) -> Result<[i32; 3], String> {
    let items = value
        .as_array()
        .filter(|a| a.len() >= 3)
        .ok_or_else(|| format!("header {what} is not a position: {value}"))?;
    Ok([
        int(&items[0], what)? as i32,
        int(&items[1], what)? as i32,
        int(&items[2], what)? as i32,
    ])
}
fn state(id: i64) -> Result<BlockState, String> {
    let state = u16::try_from(id)
        .map(|id| BlockState::new(StateId(id)))
        .map_err(|_| format!("block state id {id} out of range"))?;
    if state.is_valid() {
        Ok(state)
    } else {
        Err(format!("block state id {id} is not in the generated table"))
    }
}

impl ArenaHeader {
    pub fn parse(raw: &Value) -> Result<Self, String> {
        if raw["kind"] != "header" || raw["schema"] != SCHEMA {
            return Err(format!("not a {SCHEMA} header line"));
        }
        let name = raw["arena"].as_str().ok_or("header arena name")?.to_owned();
        let origin = triple(&raw["origin"], "origin")?;
        let mut placements = Vec::new();
        for row in raw["placements"].as_array().ok_or("header placements")? {
            placements.push((
                triple(row, "placement")?,
                state(int(&row[3], "placement")?)?,
            ));
        }
        let mut block_entities = Vec::new();
        for row in raw["block_entities"]
            .as_array()
            .ok_or("header block_entities")?
        {
            let snbt = row[3].as_str().ok_or("block entity snbt")?;
            let tag =
                parse_compound_fully(snbt).map_err(|e| format!("block entity snbt: {e:?}"))?;
            block_entities.push((triple(row, "block entity")?, tag));
        }
        let mut script = Vec::new();
        for op in raw["script"].as_array().ok_or("header script")? {
            let pos = triple(&op["pos"], "script pos")?;
            let priority =
                || int(&op["priority"], "priority").map(|p| TickPriority::by_value(p as i32));
            let parsed = match op["op"].as_str() {
                Some("set") => ScriptOp::Set {
                    pos,
                    state: state(int(&op["state"], "state")?)?,
                    flags: int(&op["flags"], "flags")? as u32,
                },
                Some("schedule_block") => ScriptOp::ScheduleBlock {
                    pos,
                    block: Block::from_name(op["block"].as_str().unwrap_or_default())
                        .ok_or_else(|| format!("unknown block in {op}"))?,
                    delay: int(&op["delay"], "delay")? as i32,
                    priority: priority()?,
                },
                Some("schedule_fluid") => ScriptOp::ScheduleFluid {
                    pos,
                    fluid: FluidId::from_name(op["fluid"].as_str().unwrap_or_default())
                        .ok_or_else(|| format!("unknown fluid in {op}"))?,
                    delay: int(&op["delay"], "delay")? as i32,
                    priority: priority()?,
                },
                _ => return Err(format!("unknown script op {op}")),
            };
            script.push((int(&op["tick"], "tick")? as i32, parsed));
        }
        Ok(Self {
            raw: raw.clone(),
            name,
            origin: BlockPos::new(origin[0], origin[1], origin[2]),
            size: triple(&raw["size"], "size")?,
            ticks: int(&raw["ticks"], "ticks")? as i32,
            seed: raw["seed"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .ok_or("header seed")?,
            opt_in: raw["opt_in"]
                .as_array()
                .ok_or("header opt_in")?
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_owned())
                .collect(),
            placements,
            block_entities,
            script,
        })
    }

    fn abs(&self, rel: [i32; 3]) -> BlockPos {
        BlockPos::new(
            self.origin.get_x() + rel[0],
            self.origin.get_y() + rel[1],
            self.origin.get_z() + rel[2],
        )
    }
    fn rel(&self, pos: BlockPos) -> [i32; 3] {
        [
            pos.get_x() - self.origin.get_x(),
            pos.get_y() - self.origin.get_y(),
            pos.get_z() - self.origin.get_z(),
        ]
    }
    fn contains(&self, pos: BlockPos) -> bool {
        let [x, y, z] = self.rel(pos);
        (0..self.size[0]).contains(&x)
            && (0..self.size[1]).contains(&y)
            && (0..self.size[2]).contains(&z)
    }
    /// The probe's force-loaded chunks: the box grown by one block in x/z.
    fn chunks(&self) -> Vec<ChunkPos> {
        let mut out = Vec::new();
        let (x0, z0) = (self.origin.get_x(), self.origin.get_z());
        for cx in (x0 - 1) >> 4..=(x0 + self.size[0]) >> 4 {
            for cz in (z0 - 1) >> 4..=(z0 + self.size[2]) >> 4 {
                out.push(ChunkPos::new(cx, cz));
            }
        }
        out
    }
}

/// NBT floats and doubles as IEEE-754 bits, the probe's `normalizeFloats`
/// (VERIFICATION 2.4): a float becomes the string `#f:0x<floatToIntBits>`, a
/// double `#d:0x<doubleToLongBits>` (NaN canonicalized, D14), and a real string
/// starting with `#` gains one more `#` so the mapping stays injective.
pub fn normalize_floats(tag: &mut Tag) {
    let replacement = match tag {
        Tag::Float(f) => {
            let bits = if f.value.is_nan() {
                0x7fc0_0000
            } else {
                f.value.to_bits()
            };
            format!("#f:0x{bits:08x}")
        }
        Tag::Double(d) => {
            let bits = if d.value.is_nan() {
                0x7ff8_0000_0000_0000
            } else {
                d.value.to_bits()
            };
            format!("#d:0x{bits:016x}")
        }
        Tag::String(s) if s.value.starts_with('#') => format!("#{}", s.value),
        Tag::Compound(c) => {
            c.for_each(|_, value| normalize_floats(value));
            return;
        }
        Tag::List(l) => {
            for i in 0..l.size() {
                let mut item = l.get(i).clone();
                normalize_floats(&mut item);
                l.set(i, item);
            }
            return;
        }
        _ => return,
    };
    *tag = Tag::String(StringTag::value_of(replacement));
}

/// `mix(seed, site, ordinal)` of the seed-injection contract.
pub fn mix(seed: i64, site: i64, ordinal: i64) -> i64 {
    mix_stafford13(
        seed.wrapping_add(site.wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64))
            .wrapping_add(ordinal),
    )
}

/// World effects with no Rust owner yet are refusals. Client transport and
/// level events have no recipients: the arena has no players and no debug
/// subscribers, which is the state the Paper boot runs in.
struct ArenaEffects {
    water_over_lava_flow_speed: i32,
}

fn refuse<T>(what: impl std::fmt::Display) -> Result<T, LiveBlockMutationError> {
    Err(LiveBlockMutationError::Unavailable(format!(
        "arena runner: {what}"
    )))
}
fn fluid_refuse<T>(what: impl std::fmt::Display) -> FluidResult<T> {
    Err(FluidError::Unavailable(format!("arena runner: {what}")))
}
fn be_refuse<T>(what: impl std::fmt::Display) -> Result<T, BlockEntityTickerError> {
    Err(BlockEntityTickerError::Unavailable(format!(
        "arena runner: {what}"
    )))
}

impl BlockEntityWorldServices for ArenaEffects {
    fn on_block_entity_added(&mut self, _: BlockPos) -> Result<(), BlockEntityTickerError> {
        // Optional observer after the canonical debug hook; no observer here.
        Ok(())
    }
    fn require_ticker_body(&self, kind: NativeTickerKind) -> Result<(), BlockEntityTickerError> {
        // The trait forbids accepting a ticker whose body cannot execute: the
        // runner wires no tick-body world (entity construction, collision,
        // events) for any native family yet.
        be_refuse(format_args!(
            "{kind:?} tick-body world (entity construction, collision, events) is not wired"
        ))
    }
    fn tick_body(
        &mut self,
        _: &mut LevelBlockEntityRuntime,
        _: &mut ChunkMap,
        _: &mut ThreadUnsafeRandom,
        _: BlockPos,
        kind: NativeTickerKind,
    ) -> Result<(), BlockEntityTickerError> {
        be_refuse(format_args!(
            "{kind:?} tick body world (entities, collision, events)"
        ))
    }
    fn report_tick_exception(
        &mut self,
        pos: BlockPos,
        message: &str,
    ) -> Result<(), BlockEntityTickerError> {
        be_refuse(format_args!("tick exception at {pos:?}: {message}"))
    }
    fn mid_tick_tasks(&mut self) -> Result<(), BlockEntityTickerError> {
        // MinecraftServer mid-tick chunk tasks: none are queued in an arena.
        Ok(())
    }
}

impl NativeWorldEffects for ArenaEffects {
    fn external_block_effect(
        &mut self,
        _: &mut ServerLevel,
        effect: LiveBlockEffect,
    ) -> Result<(), LiveBlockMutationError> {
        match effect {
            // ChunkHolder.blockChanged / AntiXray (disabled): no player tracks
            // an arena chunk, so no packet has a recipient.
            LiveBlockEffect::PacketBlockChange { .. }
            | LiveBlockEffect::SendBlockUpdated { .. } => Ok(()),
            LiveBlockEffect::OutputSignal { pos, state } => refuse(format_args!(
                "updateNeighbourForOutputSignal of {} at {pos:?}",
                state.block().name()
            )),
            LiveBlockEffect::PreRemoveBlockEntity { pos, old } => refuse(format_args!(
                "preRemoveSideEffects of {} at {pos:?}",
                old.block().name()
            )),
            LiveBlockEffect::DropPoi { pos } => refuse(format_args!("POI debug drop at {pos:?}")),
            LiveBlockEffect::RegisterPoi { .. } => refuse("POI debug registration"),
            _ => refuse("external block effect"),
        }
    }
    fn neighbor_debug_update(
        &mut self,
        _: &mut ServerLevel,
        _: &[BlockPos],
    ) -> Result<(), LiveBlockMutationError> {
        refuse("neighbor-update debug listener")
    }
    fn neighbor_destroy_block(
        &mut self,
        _: &mut ServerLevel,
        pos: BlockPos,
        _: bool,
        _: i32,
    ) -> Result<bool, LiveBlockMutationError> {
        refuse(format_args!("Level.destroyBlock at {pos:?}"))
    }
    fn redstone_experiments_enabled(&self) -> bool {
        false
    }
    fn fluid_drop_resources(
        &mut self,
        _: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        _: BlockPos,
    ) -> FluidResult<()> {
        fluid_refuse(format_args!(
            "fluid drops of {} at {pos:?}",
            state.block().name()
        ))
    }
    fn lava_fizz(&mut self, _: &mut ServerLevel, _: BlockPos) -> FluidResult<()> {
        // levelEvent(1501): a client broadcast with no recipient in the arena.
        Ok(())
    }
    fn contextual_collision(
        &mut self,
        _: &ServerLevel,
        pos: BlockPos,
        state: BlockState,
    ) -> FluidResult<ShapeGeometry> {
        fluid_refuse(format_args!(
            "contextual collision of {} at {pos:?}",
            state.block().name()
        ))
    }
    fn water_over_lava_flow_speed(&self) -> i32 {
        self.water_over_lava_flow_speed
    }
    fn place_effectful_container(
        &mut self,
        _: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        _: FluidState,
    ) -> FluidResult<bool> {
        fluid_refuse(format_args!(
            "effectful liquid placement into {} at {pos:?}",
            state.block().name()
        ))
    }
}

type Services = NativeWorldServices<ArenaEffects>;

fn build_world(
    header: &ArenaHeader,
    world_defaults: &str,
) -> Result<(ServerLevel, Services), String> {
    let mut level = ServerLevel::new_region_backed(ServerLevelConfig {
        missing_chunk_policy: MissingChunkPolicy::RequireLoaded,
        ..ServerLevelConfig::default()
    });
    level.install_world_uuid(WORLD_UUID);
    level.install_world_time(LevelWorldTime::new(true));
    let overworld = Identifier::with_default_namespace("overworld");
    level.install_clock_runtime(
        LevelClockRuntime::new(&overworld, false).map_err(|e| format!("clock runtime: {e:?}"))?,
    );
    let mut environment =
        rivet_server::server::level::generation_environment::GenerationEnvironment::load(
            std::path::Path::new("/nonexistent-arena-world"),
            &overworld,
            &rivet_world::flag::feature_flags::vanilla_set(),
            0,
        )
        .map_err(|e| format!("environment: {e:?}"))?;
    environment.set_spawn_mobs(false);
    level
        .install_generation_environment(environment)
        .map_err(|e| format!("environment install: {:?}", e.error))?;
    let settings = WorldRuntimeSettings::from_yaml(world_defaults, "", "", "minecraft:overworld")
        .map_err(|e| format!("runtime settings: {e:?}"))?;
    let water_over_lava_flow_speed = settings.water_over_lava_flow_speed;
    level
        .install_runtime_settings(
            rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries(),
            settings,
        )
        .map_err(|e| format!("runtime settings install: {e:?}"))?;
    level.install_live_light(LiveLightRuntime::new(create(-64, 384), true, true));
    for pos in header.chunks() {
        let mut chunk = LevelChunk::new(pos);
        chunk.set_loaded(true);
        level
            .install_loaded_chunk(chunk)
            .map_err(|_| format!("chunk {pos} installed twice"))?;
        level
            .register_chunk_ticks(pos)
            .and_then(|()| level.activate_chunk_ticks(pos))
            .map_err(|e| format!("chunk ticks {pos}: {e}"))?;
    }
    // The superflat chunk carries terrain at the world bottom only; the arena
    // box must start empty, as the probe asserts on the Paper side.
    for y in 0..header.size[1] {
        for z in 0..header.size[2] {
            for x in 0..header.size[0] {
                let pos = header.abs([x, y, z]);
                if !read(&level, pos).is_air() {
                    return Err(format!("arena box not empty at {pos:?}"));
                }
            }
        }
    }
    let host = NativeWorldEventHost::start_on_current_thread(
        Arc::new(
            rivet_server::server::level::structure_spawn_events::TracingStructureEventDiagnostics,
        ),
        Arc::new(rivet_server::server::tick::shutdown::Shutdown::new()),
    );
    host.finish_loading();
    let services = NativeWorldServices::new(
        Arc::new(host),
        ArenaEffects {
            water_over_lava_flow_speed,
        },
        MAX_CHAINED_NEIGHBOR_UPDATES,
    );
    Ok((level, services))
}

fn read(level: &ServerLevel, pos: BlockPos) -> BlockState {
    level
        .chunk_map()
        .get_chunk(ChunkPos::containing(&pos))
        .map(|c| BlockState::new(c.get_block_state(pos.get_x(), pos.get_y(), pos.get_z())))
        .expect("arena chunk installed")
}

struct Runner<'a> {
    header: &'a ArenaHeader,
    exclusions: &'a Exclusions,
    level: ServerLevel,
    services: Services,
    chunks: HashSet<ChunkPos>,
    blocks: std::collections::HashMap<BlockPos, u16>,
    block_entities: std::collections::BTreeMap<(i32, i32, i32), String>,
}

/// A phase failure: the tick, the phase and the Rust refusal.
struct Stop {
    phase: &'static str,
    message: String,
}
fn stop(phase: &'static str) -> impl Fn(String) -> Stop {
    move |message| Stop { phase, message }
}

impl Runner<'_> {
    fn place(&mut self) -> Result<(), Stop> {
        // Seed-injection site 1, before anything in the arena draws from it.
        self.level
            .random_mut()
            .set_seed(mix(self.header.seed, SITE_LEVEL_RANDOM, 0));
        // Paper loads the block-entity NBT into the fresh entity after the
        // quiet placement; here the same resolved NBT is pending before it, so
        // the live entity is created from it by the same placement.
        for (rel, tag) in &self.header.block_entities {
            let pos = self.header.abs(*rel);
            self.level
                .chunk_map_mut()
                .get_chunk_mut(ChunkPos::containing(&pos))
                .expect("arena chunk installed")
                .set_block_entity_nbt(tag.clone());
        }
        for (rel, state) in &self.header.placements {
            let pos = self.header.abs(*rel);
            self.level
                .set_block_live(pos, *state, QUIET, UPDATE_LIMIT, &mut self.services)
                .map_err(|e| e.to_string())
                .map_err(stop("placement"))?;
            if read(&self.level, pos) != *state {
                return Err(stop("placement")(format!(
                    "quiet placement rejected at {pos:?}"
                )));
            }
        }
        Ok(())
    }

    fn run_script(&mut self, tick: i32) -> Result<(), Stop> {
        let ops: Vec<_> = self
            .header
            .script
            .iter()
            .filter(|(t, _)| *t == tick)
            .map(|(_, op)| op.clone())
            .collect();
        for op in ops {
            match op {
                ScriptOp::Set { pos, state, flags } => {
                    let pos = self.header.abs(pos);
                    self.level
                        .set_block_live(pos, state, flags, UPDATE_LIMIT, &mut self.services)
                        .map_err(|e| e.to_string())
                        .map_err(stop("script"))?;
                }
                ScriptOp::ScheduleBlock {
                    pos,
                    block,
                    delay,
                    priority,
                } => {
                    let tick = ScheduledTick::new(
                        block,
                        self.header.abs(pos),
                        self.level.game_time().wrapping_add(i64::from(delay)),
                        priority,
                        self.level.next_sub_tick_count(),
                    );
                    self.level.schedule_block_tick(tick);
                }
                ScriptOp::ScheduleFluid {
                    pos,
                    fluid,
                    delay,
                    priority,
                } => {
                    let tick = ScheduledTick::new(
                        fluid,
                        self.header.abs(pos),
                        self.level.game_time().wrapping_add(i64::from(delay)),
                        priority,
                        self.level.next_sub_tick_count(),
                    );
                    self.level.schedule_fluid_tick(tick);
                }
            }
        }
        Ok(())
    }

    /// One `ServerLevel.tick` over the phases with a Rust body (module docs).
    fn level_tick(&mut self) -> Result<(), Stop> {
        let settings = self.level.runtime_settings().expect("installed").clone();
        if let Some(clocks) = self.level.clock_runtime_mut() {
            clocks.tick();
        }
        self.level.tick_world_border();
        self.level
            .tick_world_time(&mut UnavailableScheduledFunctions)
            .map_err(|e| e.to_string())
            .map_err(stop("tickTime"))?;
        let chunks = self.chunks.clone();
        self.level
            .run_block_ticks_live(
                settings.max_block_ticks,
                |pos| chunks.contains(&pos),
                &mut self.services,
            )
            .map_err(|e| e.to_string())
            .map_err(stop("blockTicks"))?;
        let chunks = self.chunks.clone();
        self.level
            .run_fluid_ticks_live(
                settings.max_fluid_ticks,
                |pos| chunks.contains(&pos),
                &mut self.services,
            )
            .map_err(|e| e.to_string())
            .map_err(stop("fluidTicks"))?;
        if settings.block_entity_ticking {
            self.level
                .tick_block_entities(true, &mut self.services)
                .map_err(|e| e.to_string())
                .map_err(stop("blockEntities"))?;
        }
        Ok(())
    }

    /// `entities`: read from the level's entity owner. The runner binds none,
    /// so every entity admission refuses (`MissingOwner`) and the level holds
    /// no entity. An owner that holds one is a refusal: no Rust emitter for
    /// the `{n,type,pos,delta,health,nbt}` row exists yet.
    fn entities(&self) -> Result<Vec<Value>, Stop> {
        let Some(lookup) = self.level.entity_lookup() else {
            return Ok(Vec::new());
        };
        match lookup.all().next() {
            None => Ok(Vec::new()),
            Some(entity) => Err(stop("entities")(format!(
                "{} at {:?} is in the arena level: the runner has no entity row emitter",
                entity.encode_id(),
                entity.position()
            ))),
        }
    }

    fn snapshot(&mut self, t: i32) -> Result<Value, Stop> {
        let h = self.header;
        let mut blocks = Vec::new();
        for y in 0..h.size[1] {
            for z in 0..h.size[2] {
                for x in 0..h.size[0] {
                    let pos = h.abs([x, y, z]);
                    let id = read(&self.level, pos).id().0;
                    if self.blocks.get(&pos).copied().unwrap_or(0) != id {
                        self.blocks.insert(pos, id);
                        blocks.push(json!([x, y, z, id]));
                    }
                }
            }
        }
        let now = self.level.game_time();
        let mut block_ticks = Vec::new();
        let mut fluid_ticks = Vec::new();
        for pos in h.chunks() {
            let chunk = self
                .level
                .chunk_map()
                .get_chunk(pos)
                .expect("arena chunk installed");
            block_ticks.extend(
                chunk
                    .block_ticks()
                    .all()
                    .filter(|t| h.contains(t.pos))
                    .cloned(),
            );
            fluid_ticks.extend(
                chunk
                    .fluid_ticks()
                    .all()
                    .filter(|t| h.contains(t.pos))
                    .cloned(),
            );
        }
        block_ticks.sort_by(ScheduledTick::drain_cmp);
        fluid_ticks.sort_by(ScheduledTick::drain_cmp);
        let tick_row = |pos: BlockPos, name: &str, trigger: i64, priority: TickPriority| {
            let [x, y, z] = h.rel(pos);
            json!([x, y, z, name, trigger.wrapping_sub(now), priority.value()])
        };
        let block_ticks: Vec<_> = block_ticks
            .iter()
            .map(|t| tick_row(t.pos, t.r#type.name(), t.trigger_tick, t.priority))
            .collect();
        let fluid_ticks: Vec<_> = fluid_ticks
            .iter()
            .map(|t| tick_row(t.pos, t.r#type.name(), t.trigger_tick, t.priority))
            .collect();

        let mut now_entities = std::collections::BTreeMap::new();
        for pos in h.chunks() {
            let chunk = self
                .level
                .chunk_map()
                .get_chunk(pos)
                .expect("arena chunk installed");
            // Keyed by (y, z, x) below, so the map's order is not observed.
            for (&be_pos, entity) in chunk.block_entities() {
                if !h.contains(be_pos) {
                    continue;
                }
                let mut tag = entity.save_with_full_metadata();
                for key in &self.exclusions.block_entity_nbt {
                    tag.remove(key);
                }
                let mut tag = Tag::Compound(tag);
                normalize_floats(&mut tag);
                let [x, y, z] = h.rel(be_pos);
                now_entities.insert((y, z, x), tag.to_string());
            }
        }
        let mut changed: Vec<(i32, i32, i32)> = now_entities
            .iter()
            .filter(|(k, v)| self.block_entities.get(k) != Some(v))
            .map(|(k, _)| *k)
            .chain(
                self.block_entities
                    .keys()
                    .filter(|k| !now_entities.contains_key(k))
                    .copied(),
            )
            .collect();
        changed.sort();
        let mut block_entities = Vec::new();
        for key @ (y, z, x) in changed {
            match now_entities.get(&key) {
                Some(snbt) => {
                    block_entities.push(json!([x, y, z, snbt]));
                    self.block_entities.insert(key, snbt.clone());
                }
                None => {
                    block_entities.push(json!([x, y, z, null]));
                    self.block_entities.remove(&key);
                }
            }
        }
        let entities = self.entities()?;
        // `ServerLevel` has no `blockEvents` queue (`runBlockEvents`): no Rust
        // path can queue a block event, so a Paper event is a divergence here.
        // The port that adds the queue makes this read it.
        let block_events: [Value; 0] = [];
        Ok(json!({
            "kind": "tick",
            "t": t,
            "blocks": blocks,
            "block_ticks": block_ticks,
            "fluid_ticks": fluid_ticks,
            "block_events": block_events,
            "block_entities": block_entities,
            "entities": entities,
        }))
    }
}

/// Replay one arena. `world_defaults` is the `paper-world-defaults.yml` the
/// Paper arena boot ran under. `Err` is a harness failure (the arena could not
/// be set up at all); a Rust refusal during the run is an `error` line instead.
pub fn run(
    header: &ArenaHeader,
    exclusions: &Exclusions,
    world_defaults: &str,
) -> Result<Vec<String>, String> {
    if !header.opt_in.is_empty() {
        return Err(format!(
            "opt-in {:?} has no Rust runner support",
            header.opt_in
        ));
    }
    let (level, services) = build_world(header, world_defaults)?;
    let mut runner = Runner {
        header,
        exclusions,
        level,
        services,
        chunks: header.chunks().into_iter().collect(),
        blocks: Default::default(),
        block_entities: Default::default(),
    };
    let mut lines = vec![serde_json::to_string(&header.raw).expect("json")];
    for t in 0..=header.ticks {
        // A Rust body that panics on an unported branch is a refusal too: the
        // run stops at that tick with the panic message as the divergence.
        let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if t == 0 {
                runner.place().and_then(|()| runner.run_script(0))
            } else {
                runner.level_tick().and_then(|()| runner.run_script(t))
            }
        }))
        .unwrap_or_else(|payload| {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "non-string panic payload".into());
            Err(Stop {
                phase: if t == 0 {
                    "placement/script"
                } else {
                    "tick/script"
                },
                message: format!("panic: {message}"),
            })
        });
        match step.and_then(|()| runner.snapshot(t)) {
            Ok(record) => lines.push(record.to_string()),
            Err(Stop { phase, message }) => {
                lines.push(
                    json!({"kind": "error", "t": t, "phase": phase, "message": message})
                        .to_string(),
                );
                return Ok(lines);
            }
        }
    }
    lines.push(json!({"kind": "end", "ticks": header.ticks}).to_string());
    Ok(lines)
}
