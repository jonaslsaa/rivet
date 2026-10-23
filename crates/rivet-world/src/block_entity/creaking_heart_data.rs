//! CreakingHeartBlockEntity persistent data and actual server-tick state.
//! An actor reference is a reference to the owning world's real protector,
//! never a recreated UUID-only stand-in or a second entity payload.
use super::NativeBlockEntityError;
use rivet_nbt::{compound_tag::CompoundTag, nbt_ops::NbtOps, tag::Tag};
use rivet_registry::{
    block_state::BlockState,
    block_state_properties::{BlockStateProperties, CreakingHeartState},
    core::{BlockPos, Direction, Vec3, uuid_codec},
};
use rivet_util::{RandomSource, mth::Uuid};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HeartTickError {
    #[error("creaking-heart authority unavailable: {0}")]
    Unavailable(String),
    #[error("creaking-heart callback failed: {0}")]
    Callback(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProtectorBounds {
    pub min: Vec3,
    pub max: Vec3,
}

/// Real cached actor operations. Removed actors must remain queryable through
/// their existing reference, exactly as Java Either.left(Entity) does. There
/// is deliberately no implementation for an unported Creaking entity body.
pub trait CreakingHeartProtector: std::fmt::Debug + Send + Sync {
    fn uuid(&self) -> Uuid;
    fn is_removed(&self) -> bool;
    fn position(&self) -> Vec3;
    fn bounding_box(&self) -> ProtectorBounds;
    fn is_persistence_required(&self) -> bool;
    fn player_is_stuck_in_you(&self) -> Result<bool, HeartTickError>;
    fn tear_down(&self) -> Result<(), HeartTickError>;
    fn set_transient(&self, heart_pos: BlockPos) -> Result<(), HeartTickError>;
    fn make_spawn_sound(&self) -> Result<(), HeartTickError>;
}

#[derive(Debug, Clone)]
pub struct ProtectorRef(pub Arc<dyn CreakingHeartProtector>);
impl PartialEq for ProtectorRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ProtectorInfo {
    Resolved(ProtectorRef),
    Unresolved(Uuid),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeartTrailParticle {
    pub source: Vec3,
    pub destination: Vec3,
    pub color: i32,
    pub duration: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeartSound {
    Spawn,
    Hurt { volume: f32 },
}

pub trait HeartTickWorld {
    type Random: RandomSource;
    fn random(&mut self) -> &mut Self::Random;
    fn has_level(&self) -> bool;
    fn get_block_state(&self, pos: BlockPos) -> Result<BlockState, HeartTickError>;
    fn is_pale_oak_log(&self, state: BlockState) -> bool;
    fn creaking_active(&self, pos: BlockPos) -> Result<bool, HeartTickError>;
    /// Actual ServerLevel.getEntity(UUID), accepting only a real Creaking.
    fn lookup_creaking(&mut self, uuid: Uuid) -> Result<Option<ProtectorRef>, HeartTickError>;
    /// BlockEntity.setChanged: actual dirty/comparator effects, not a raw tag
    /// write. Honors the owning level's ignoreBlockEntityUpdates flag.
    fn mark_changed(&mut self, pos: BlockPos) -> Result<(), HeartTickError>;
    fn update_output_neighbors(&mut self, pos: BlockPos) -> Result<(), HeartTickError>;
    fn set_block_update_all(
        &mut self,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<(), HeartTickError>;
    fn spawning_monsters(&self) -> bool;
    fn peaceful(&self) -> bool;
    /// getNearestPlayerThatAffectsSpawning(integer XYZ,32,false).
    fn has_nearest_affecting_player(
        &mut self,
        pos: BlockPos,
        range: f64,
    ) -> Result<bool, HeartTickError>;
    /// Actual SpawnUtil.trySpawnMob(CREAKING,SPAWNER,5,16,8,
    /// ON_TOP_OF_COLLIDER_NO_LEAVES,true). No null fallback for missing body.
    fn try_spawn_creaking(&mut self, pos: BlockPos)
    -> Result<Option<ProtectorRef>, HeartTickError>;
    fn entity_place_event(&mut self, protector: &ProtectorRef) -> Result<(), HeartTickError>;
    fn broadcast_poof(&mut self, protector: &ProtectorRef) -> Result<(), HeartTickError>;
    /// sendParticles(TrailParticleOption,true,true,...,1,0,0,0,0).
    fn send_trail_particle(&mut self, particle: HeartTrailParticle) -> Result<(), HeartTickError>;
    /// BLOCKS category, pitch1; Spawn volume1, Hurt's source-computed volume.
    fn play_heart_sound(&mut self, pos: BlockPos, sound: HeartSound) -> Result<(), HeartTickError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeartData {
    creaking: Option<ProtectorInfo>,
    ticks_existed: i64,
    ticker: i32,
    emitter: i32,
    emitter_target: Option<Vec3>,
    output_signal: i32,
    public_bukkit_values: CompoundTag,
}

impl HeartData {
    pub(super) fn load(input: &CompoundTag) -> Result<Self, NativeBlockEntityError> {
        let mut common = CompoundTag::new();
        super::common_data(&mut common, input)?;
        let creaking = input
            .get("creaking")
            .and_then(|tag| {
                uuid_codec::<NbtOps>()
                    .parse(&NbtOps, tag)
                    .result_or_partial_silent()
            })
            .map(ProtectorInfo::Unresolved);
        Ok(Self {
            creaking,
            ticks_existed: 0,
            ticker: 0,
            emitter: 0,
            emitter_target: None,
            output_signal: 0,
            public_bukkit_values: common
                .get_compound("PublicBukkitValues")
                .cloned()
                .unwrap_or_default(),
        })
    }
    pub(super) fn save(&self, pos: BlockPos) -> CompoundTag {
        let mut tag = super::base_tag(pos, "minecraft:creaking_heart");
        if !self.public_bukkit_values.tags.is_empty() {
            tag.put(
                "PublicBukkitValues".into(),
                Tag::Compound(self.public_bukkit_values.clone()),
            );
        }
        if let Some(info) = &self.creaking {
            let uuid = match info {
                ProtectorInfo::Resolved(actor) => actor.0.uuid(),
                ProtectorInfo::Unresolved(uuid) => *uuid,
            };
            tag.put(
                "creaking".into(),
                uuid_codec::<NbtOps>()
                    .encode_start(&NbtOps, &uuid)
                    .result()
                    .expect("valid uuid")
                    .clone(),
            );
        }
        tag
    }
    pub fn output_signal(&self) -> i32 {
        self.output_signal
    }

    fn clear_info<W: HeartTickWorld>(
        &mut self,
        world: &mut W,
        pos: BlockPos,
    ) -> Result<(), HeartTickError> {
        self.creaking = None;
        world.mark_changed(pos)
    }
    fn set_resolved<W: HeartTickWorld>(
        &mut self,
        world: &mut W,
        pos: BlockPos,
        actor: ProtectorRef,
    ) -> Result<(), HeartTickError> {
        self.creaking = Some(ProtectorInfo::Resolved(actor));
        world.mark_changed(pos)
    }
    fn protector<W: HeartTickWorld>(
        &mut self,
        world: &mut W,
        pos: BlockPos,
    ) -> Result<Option<ProtectorRef>, HeartTickError> {
        if let Some(ProtectorInfo::Resolved(actor)) = &self.creaking {
            if !actor.0.is_removed() {
                return Ok(Some(actor.clone()));
            }
            self.creaking = Some(ProtectorInfo::Unresolved(actor.0.uuid()));
            self.ticks_existed = 0;
            world.mark_changed(pos)?;
        }
        if world.has_level()
            && let Some(ProtectorInfo::Unresolved(uuid)) = &self.creaking
        {
            if let Some(actor) = world.lookup_creaking(*uuid)? {
                self.set_resolved(world, pos, actor.clone())?;
                return Ok(Some(actor));
            }
            if self.ticks_existed >= 30 {
                self.clear_info(world, pos)?;
            }
        }
        Ok(None)
    }
    fn distance<W: HeartTickWorld>(
        &mut self,
        world: &mut W,
        pos: BlockPos,
    ) -> Result<f64, HeartTickError> {
        Ok(self.protector(world, pos)?.map_or(0.0, |actor| {
            let p = actor.0.position();
            ((p.x - (f64::from(pos.get_x()) + 0.5)).powi(2)
                + (p.y - f64::from(pos.get_y())).powi(2)
                + (p.z - (f64::from(pos.get_z()) + 0.5)).powi(2))
            .sqrt()
        }))
    }
    pub fn compute_analog_output<W: HeartTickWorld>(
        &mut self,
        world: &mut W,
        pos: BlockPos,
    ) -> Result<i32, HeartTickError> {
        if self.creaking.is_some() && self.protector(world, pos)?.is_some() {
            let scaled = self.distance(world, pos)?.clamp(0.0, 32.0) / 32.0;
            Ok(15 - (scaled * 15.0).floor() as i32)
        } else {
            Ok(0)
        }
    }
    fn emit_particles<W: HeartTickWorld>(
        &mut self,
        world: &mut W,
        pos: BlockPos,
        count: i32,
        toward_creaking: bool,
    ) -> Result<(), HeartTickError> {
        if let Some(actor) = self.protector(world, pos)? {
            for _ in 0..count {
                let bounds = actor.0.bounding_box();
                let random = world.random();
                let mut source = Vec3::new(
                    bounds.min.x + random.next_double() * (bounds.max.x - bounds.min.x),
                    bounds.min.y + random.next_double() * (bounds.max.y - bounds.min.y),
                    bounds.min.z + random.next_double() * (bounds.max.z - bounds.min.z),
                );
                let mut destination = Vec3::new(
                    f64::from(pos.get_x()) + random.next_double(),
                    f64::from(pos.get_y()) + random.next_double(),
                    f64::from(pos.get_z()) + random.next_double(),
                );
                if toward_creaking {
                    std::mem::swap(&mut source, &mut destination);
                }
                let duration = random.next_int_bound(40) + 10;
                world.send_trail_particle(HeartTrailParticle {
                    source,
                    destination,
                    color: if toward_creaking { 16545810 } else { 6250335 },
                    duration,
                })?;
            }
        }
        Ok(())
    }
    pub fn remove_protector<W: HeartTickWorld>(
        &mut self,
        world: &mut W,
        pos: BlockPos,
    ) -> Result<(), HeartTickError> {
        if let Some(actor) = self.protector(world, pos)? {
            actor.0.tear_down()?;
            self.clear_info(world, pos)?;
        }
        Ok(())
    }
    pub fn server_tick<W: HeartTickWorld>(
        &mut self,
        world: &mut W,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<(), HeartTickError> {
        self.ticks_existed = self.ticks_existed.wrapping_add(1);
        let output = self.compute_analog_output(world, pos)?;
        if self.output_signal != output {
            self.output_signal = output;
            world.update_output_neighbors(pos)?;
        }
        if self.emitter > 0 {
            if self.emitter > 50 {
                self.emit_particles(world, pos, 1, true)?;
                self.emit_particles(world, pos, 1, false)?;
            }
            if self.emitter % 10 == 0
                && let Some(mut target) = self.emitter_target
            {
                if let Some(actor) = self.protector(world, pos)? {
                    let bounds = actor.0.bounding_box();
                    target = Vec3::new(
                        bounds.min.x + 0.5 * (bounds.max.x - bounds.min.x),
                        bounds.min.y + 0.5 * (bounds.max.y - bounds.min.y),
                        bounds.min.z + 0.5 * (bounds.max.z - bounds.min.z),
                    );
                    self.emitter_target = Some(target);
                }
                let progress = 0.2f32
                    + 0.8f32 * self.emitter.wrapping_neg().wrapping_add(100) as f32 / 100.0f32;
                let p = f64::from(progress);
                let sound = Vec3::new(
                    ((f64::from(pos.get_x()) + 0.5) - target.x) * p + target.x,
                    ((f64::from(pos.get_y()) + 0.5) - target.y) * p + target.y,
                    ((f64::from(pos.get_z()) + 0.5) - target.z) * p + target.z,
                );
                world.play_heart_sound(
                    BlockPos::containing(sound.x, sound.y, sound.z),
                    HeartSound::Hurt {
                        volume: self.emitter as f32 / 2.0f32 / 100.0f32 + 0.5f32,
                    },
                )?;
            }
            self.emitter = self.emitter.wrapping_sub(1);
        }
        let previous = self.ticker;
        self.ticker = self.ticker.wrapping_sub(1);
        if previous < 0 {
            self.ticker = if world.has_level() {
                world.random().next_int_bound(5) + 20
            } else {
                20
            };
            let has_logs = has_required_logs(world, pos, state)?;
            let next = if !has_logs && self.creaking.is_none() {
                CreakingHeartState::Uprooted
            } else if world.creaking_active(pos)? {
                CreakingHeartState::Awake
            } else {
                CreakingHeartState::Dormant
            };
            let updated = state
                .set_value(BlockStateProperties::CREAKING_HEART_STATE, next)
                .map_err(|e| HeartTickError::Callback(e.to_string()))?;
            if updated != state {
                world.set_block_update_all(pos, updated)?;
                if next == CreakingHeartState::Uprooted {
                    return Ok(());
                }
            }
            if self.creaking.is_none() {
                if next == CreakingHeartState::Awake
                    && world.spawning_monsters()
                    && !world.peaceful()
                    && world.has_nearest_affecting_player(pos, 32.0)?
                    && let Some(actor) = world.try_spawn_creaking(pos)?
                {
                    world.entity_place_event(&actor)?;
                    world.broadcast_poof(&actor)?;
                    actor.0.set_transient(pos)?;
                    self.set_resolved(world, pos, actor.clone())?;
                    actor.0.make_spawn_sound()?;
                    world.play_heart_sound(pos, HeartSound::Spawn)?;
                }
            } else if let Some(actor) = self.protector(world, pos)?
                && ((!world.creaking_active(pos)? && !actor.0.is_persistence_required())
                    || self.distance(world, pos)? > 34.0
                    || actor.0.player_is_stuck_in_you()?)
            {
                self.remove_protector(world, pos)?;
            }
        }
        Ok(())
    }
}

fn has_required_logs<W: HeartTickWorld>(
    world: &W,
    pos: BlockPos,
    state: BlockState,
) -> Result<bool, HeartTickError> {
    use rivet_registry::block_state_property::PropertyValue;
    let axis = state
        .get_value(BlockStateProperties::AXIS)
        .ok_or_else(|| HeartTickError::Callback("heart state has no axis".into()))?;
    let directions = match axis {
        PropertyValue::Enum("x") => [Direction::East, Direction::West],
        PropertyValue::Enum("y") => [Direction::Up, Direction::Down],
        PropertyValue::Enum("z") => [Direction::South, Direction::North],
        _ => return Err(HeartTickError::Callback("invalid heart axis".into())),
    };
    for direction in directions {
        let neighbor = world.get_block_state(pos.relative(&direction))?;
        if !world.is_pale_oak_log(neighbor) {
            return Ok(false);
        }
        let neighbor_axis = neighbor
            .get_value(BlockStateProperties::AXIS)
            .ok_or_else(|| HeartTickError::Callback("pale_oak_logs member has no axis".into()))?;
        if neighbor_axis != axis {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::tag_parser::parse_compound_fully;
    use rivet_registry::{block_state_property::PropertyValue, generated::blocks::BlockId};
    use rivet_util::random_source::LegacyRandomSource;
    use std::cell::RefCell;

    fn block(name: &str) -> BlockState {
        BlockState::of(BlockId::from_name(name).unwrap())
    }
    struct World {
        random: LegacyRandomSource,
        active: bool,
        logs: bool,
        state: BlockState,
        trace: Vec<String>,
        reads: RefCell<Vec<String>>,
    }
    impl HeartTickWorld for World {
        type Random = LegacyRandomSource;
        fn random(&mut self) -> &mut Self::Random {
            &mut self.random
        }
        fn has_level(&self) -> bool {
            true
        }
        fn get_block_state(&self, pos: BlockPos) -> Result<BlockState, HeartTickError> {
            self.reads.borrow_mut().push(format!(
                "{},{},{}",
                pos.get_x(),
                pos.get_y(),
                pos.get_z()
            ));
            Ok(block(if self.logs {
                "minecraft:pale_oak_log"
            } else {
                "minecraft:air"
            }))
        }
        fn is_pale_oak_log(&self, state: BlockState) -> bool {
            state.block().name() == "minecraft:pale_oak_log"
        }
        fn creaking_active(&self, _: BlockPos) -> Result<bool, HeartTickError> {
            Ok(self.active)
        }
        fn lookup_creaking(&mut self, _: Uuid) -> Result<Option<ProtectorRef>, HeartTickError> {
            self.trace.push("lookup".into());
            Ok(None)
        }
        fn mark_changed(&mut self, _: BlockPos) -> Result<(), HeartTickError> {
            self.trace.extend(["changed".into(), "neighbors".into()]);
            Ok(())
        }
        fn update_output_neighbors(&mut self, _: BlockPos) -> Result<(), HeartTickError> {
            self.trace.push("neighbors".into());
            Ok(())
        }
        fn set_block_update_all(
            &mut self,
            _: BlockPos,
            state: BlockState,
        ) -> Result<(), HeartTickError> {
            self.state = state;
            let Some(PropertyValue::Enum(name)) =
                state.get_value(BlockStateProperties::CREAKING_HEART_STATE)
            else {
                panic!("heart property")
            };
            self.trace.push(format!("state:{name}"));
            Ok(())
        }
        fn spawning_monsters(&self) -> bool {
            true
        }
        fn peaceful(&self) -> bool {
            false
        }
        fn has_nearest_affecting_player(
            &mut self,
            _: BlockPos,
            range: f64,
        ) -> Result<bool, HeartTickError> {
            assert_eq!(range, 32.0);
            self.trace.push("nearest".into());
            Ok(false)
        }
        fn try_spawn_creaking(
            &mut self,
            _: BlockPos,
        ) -> Result<Option<ProtectorRef>, HeartTickError> {
            panic!("source world has no nearest player")
        }
        fn entity_place_event(&mut self, _: &ProtectorRef) -> Result<(), HeartTickError> {
            unreachable!()
        }
        fn broadcast_poof(&mut self, _: &ProtectorRef) -> Result<(), HeartTickError> {
            unreachable!()
        }
        fn send_trail_particle(&mut self, _: HeartTrailParticle) -> Result<(), HeartTickError> {
            panic!("no resolved protector")
        }
        fn play_heart_sound(
            &mut self,
            pos: BlockPos,
            sound: HeartSound,
        ) -> Result<(), HeartTickError> {
            let HeartSound::Hurt { volume } = sound else {
                panic!("no spawn")
            };
            self.trace.push(format!(
                "sound:{},{},{}:{}",
                pos.get_x(),
                pos.get_y(),
                pos.get_z(),
                volume.to_bits() as i32
            ));
            Ok(())
        }
    }

    #[test]
    fn actual_paper_heart_tick_grace_state_schedule_orphan_emitter_and_rng() {
        let pos = BlockPos::new(3, 64, -8);
        let mut cases = 0;
        for line in include_str!("../../../../tools/rivet-oracle/fixtures/heart-tick.jsonl").lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let input = parse_compound_fully(row["input_snbt"].as_str().unwrap()).unwrap();
            let mut data = HeartData::load(&input).unwrap();
            data.emitter = row["initial_emitter"].as_i64().unwrap() as i32;
            if data.emitter != 0 {
                data.emitter_target = Some(Vec3::new(10.0, 65.0, -4.0));
            }
            let mut world = World {
                random: LegacyRandomSource::new(99),
                active: row["active"].as_bool().unwrap(),
                logs: row["logs"].as_bool().unwrap(),
                state: block("minecraft:creaking_heart")
                    .set_value(
                        BlockStateProperties::CREAKING_HEART_STATE,
                        CreakingHeartState::Dormant,
                    )
                    .unwrap(),
                trace: Vec::new(),
                reads: RefCell::new(Vec::new()),
            };
            for _ in 0..row["ticks"].as_u64().unwrap() {
                let state = world.state;
                data.server_tick(&mut world, pos, state).unwrap();
            }
            assert_eq!(
                data.save(pos),
                parse_compound_fully(row["output_snbt"].as_str().unwrap()).unwrap(),
                "{row}"
            );
            assert_eq!(
                data.ticks_existed,
                row["ticks_existed"].as_i64().unwrap(),
                "{row}"
            );
            assert_eq!(
                i64::from(data.ticker),
                row["ticker"].as_i64().unwrap(),
                "{row}"
            );
            assert_eq!(
                i64::from(data.emitter),
                row["emitter"].as_i64().unwrap(),
                "{row}"
            );
            assert_eq!(
                i64::from(data.output_signal),
                row["output_signal"].as_i64().unwrap(),
                "{row}"
            );
            assert_eq!(
                serde_json::to_value(world.trace).unwrap(),
                row["trace"],
                "{row}"
            );
            assert_eq!(
                serde_json::to_value(world.reads.into_inner()).unwrap(),
                row["reads"],
                "{row}"
            );
            assert_eq!(
                world.random.next_long(),
                row["rng_next"].as_i64().unwrap(),
                "{row}"
            );
            cases += 1;
        }
        assert_eq!(cases, 64);
    }
}
