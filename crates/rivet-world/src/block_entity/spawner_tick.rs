//! BaseSpawner.serverTick orchestration. All world/event/entity operations
//! below are required authorities, not permissive defaults. In particular a
//! constructor, event cancellation, unavailable body and failed admission are
//! distinct results with different source RNG/delay behavior.
use super::SpawnerData;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::{
    Identifier,
    core::{BlockPos, Vec3},
    generated::registries::ENTITY_TYPE_BY_NAME,
};
use rivet_util::RandomSource;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpawnerTickError {
    #[error("spawner authority unavailable: {0}")]
    Unavailable(String),
    #[error("spawner callback failed: {0}")]
    Callback(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreSpawnerDecision {
    Allow,
    Cancel { abort: bool },
}

pub trait SpawnerTickWorld {
    type Random: RandomSource;
    type Entity;
    fn random(&mut self) -> &mut Self::Random;
    fn mob_spawner_tick_rate(&self) -> i32;
    fn near_affecting_player(
        &mut self,
        pos: BlockPos,
        range: i32,
    ) -> Result<bool, SpawnerTickError>;
    fn spawner_blocks_enabled(&self) -> bool;
    /// TagValueInput.read("Pos", Vec3.CODEC), including its partial result.
    fn read_spawn_position(
        &self,
        entity_data: &CompoundTag,
    ) -> Result<Option<Vec3>, SpawnerTickError>;
    fn no_collision(
        &mut self,
        entity_type: &Identifier,
        position: Vec3,
    ) -> Result<bool, SpawnerTickError>;
    /// Includes the source friendly-category/peaceful short circuit followed
    /// by block light then effective sky light inclusive-range tests.
    fn custom_rules_allow(
        &mut self,
        entity_type: &Identifier,
        pos: BlockPos,
        rules: &CompoundTag,
    ) -> Result<bool, SpawnerTickError>;
    fn check_spawn_placement(
        &mut self,
        entity_type: &Identifier,
        pos: BlockPos,
    ) -> Result<bool, SpawnerTickError>;
    fn pre_spawner_event(
        &mut self,
        entity_type: &Identifier,
        position: Vec3,
        spawner: BlockPos,
    ) -> Result<PreSpawnerDecision, SpawnerTickError>;
    /// Real recursive SPAWNER construction/load, each node snapped to its
    /// supplied position retaining its loaded/current yaw/pitch, before nearby
    /// count (the lambda runs after Entity.load).
    fn load_and_snap(
        &mut self,
        entity_data: &CompoundTag,
        position: Vec3,
    ) -> Result<Option<Self::Entity>, SpawnerTickError>;
    fn exact_class_non_spectator_count(
        &mut self,
        entity: &Self::Entity,
        spawner: BlockPos,
        range: i32,
    ) -> Result<i32, SpawnerTickError>;
    /// Root snapTo(current x/y/z, yaw, 0.0F): pitch MUST be reset, not retained.
    fn snap_yaw_zero_pitch(&mut self, entity: &mut Self::Entity, yaw: f32);
    fn is_mob(&self, entity: &Self::Entity) -> bool;
    fn check_mob_spawn_rules(
        &mut self,
        entity: &mut Self::Entity,
    ) -> Result<bool, SpawnerTickError>;
    fn check_mob_obstruction(
        &mut self,
        entity: &mut Self::Entity,
    ) -> Result<bool, SpawnerTickError>;
    fn finalize_spawn(&mut self, entity: &mut Self::Entity) -> Result<(), SpawnerTickError>;
    fn equip(
        &mut self,
        entity: &mut Self::Entity,
        equipment: &CompoundTag,
    ) -> Result<(), SpawnerTickError>;
    fn nerf_spawner_mobs(&self) -> bool;
    fn set_unaware(&mut self, entity: &mut Self::Entity);
    /// Sets spawnedViaMobSpawner and Bukkit SPAWNER reason BEFORE event.
    fn mark_spawner_origin(&mut self, entity: &mut Self::Entity);
    /// Return true only when actual SpawnerSpawnEvent is cancelled.
    fn spawner_spawn_event_cancelled(
        &mut self,
        entity: &mut Self::Entity,
        spawner: BlockPos,
    ) -> Result<bool, SpawnerTickError>;
    /// Consume into canonical world ownership. Only on successful recursive
    /// admission emit level event2004, ENTITY_PLACE at spawn pos and Mob.spawnAnim
    /// in that order. The combined callback keeps the entity single-owned.
    fn admit_and_emit(
        &mut self,
        entity: Self::Entity,
        spawner: BlockPos,
        spawn_pos: BlockPos,
    ) -> Result<bool, SpawnerTickError>;
    /// Concrete SpawnerBlockEntity.setNextSpawnData sends its block update.
    fn next_spawn_data_changed(&mut self, pos: BlockPos) -> Result<(), SpawnerTickError>;
    fn broadcast_spawn_event(&mut self, pos: BlockPos) -> Result<(), SpawnerTickError>;
}

impl SpawnerData {
    fn select_potential<R: RandomSource>(&self, random: &mut R) -> Option<CompoundTag> {
        let total: i32 = self.potentials.iter().map(|(weight, _)| *weight).sum();
        if total == 0 {
            return None;
        }
        // WeightedList draws even for exactly one entry/one total weight.
        let mut roll = random.next_int_bound(total);
        self.potentials.iter().find_map(|(weight, data)| {
            if roll < *weight {
                Some(data.clone())
            } else {
                roll -= *weight;
                None
            }
        })
    }

    fn choose_next<W: SpawnerTickWorld>(
        &mut self,
        world: &mut W,
        pos: BlockPos,
    ) -> Result<(), SpawnerTickError> {
        if self.next_spawn.is_none() {
            self.next_spawn = Some(self.select_potential(world.random()).unwrap_or_else(|| {
                let mut tag = CompoundTag::new();
                tag.put(
                    "entity".into(),
                    rivet_nbt::tag::Tag::Compound(CompoundTag::new()),
                );
                tag
            }));
            world.next_spawn_data_changed(pos)?;
        }
        Ok(())
    }

    fn reset_delay<W: SpawnerTickWorld>(
        &mut self,
        world: &mut W,
        pos: BlockPos,
    ) -> Result<(), SpawnerTickError> {
        self.delay = if self.max_delay <= self.min_delay {
            self.min_delay
        } else {
            self.min_delay.wrapping_add(
                world
                    .random()
                    .next_int_bound(self.max_delay.wrapping_sub(self.min_delay)),
            )
        };
        if let Some(next) = self.select_potential(world.random()) {
            self.next_spawn = Some(next);
            world.next_spawn_data_changed(pos)?;
        }
        world.broadcast_spawn_event(pos)
    }

    pub fn server_tick<W: SpawnerTickWorld>(
        &mut self,
        world: &mut W,
        pos: BlockPos,
    ) -> Result<(), SpawnerTickError> {
        if self.spawn_count <= 0 || self.max_nearby <= 0 {
            return Ok(());
        }
        if self.delay > 0 {
            self.tick_delay = self.tick_delay.wrapping_sub(1);
            if self.tick_delay > 0 {
                return Ok(());
            }
        }
        self.tick_delay = world.mob_spawner_tick_rate();
        if self.tick_delay == -1 {
            return Ok(());
        }
        if !world.near_affecting_player(pos, self.required_range)?
            || !world.spawner_blocks_enabled()
        {
            return Ok(());
        }
        if self.delay < self.tick_delay.wrapping_neg() {
            self.reset_delay(world, pos)?;
        }
        if self.delay > 0 {
            self.delay = self.delay.wrapping_sub(self.tick_delay);
            return Ok(());
        }
        self.choose_next(world, pos)?;
        let next = self.next_spawn.as_ref().expect("chosen SpawnData").clone();
        let entity_data = next
            .get_compound("entity")
            .expect("normalized SpawnData entity");
        let custom = next.get_compound("custom_spawn_rules");
        let mut delay = false;
        for _ in 0..self.spawn_count {
            let entity_type = entity_data
                .get_string("id")
                .and_then(|id| Identifier::try_parse_result(id).ok().flatten())
                .filter(|id| ENTITY_TYPE_BY_NAME.contains_key(id.to_string().as_str()));
            let Some(entity_type) = entity_type else {
                self.reset_delay(world, pos)?;
                return Ok(());
            };
            let position = if let Some(position) = world.read_spawn_position(entity_data)? {
                position
            } else {
                let random = world.random();
                Vec3::new(
                    f64::from(pos.get_x())
                        + (random.next_double() - random.next_double())
                            * f64::from(self.spawn_range)
                        + 0.5,
                    f64::from(
                        pos.get_y()
                            .wrapping_add(random.next_int_bound(3))
                            .wrapping_sub(1),
                    ),
                    f64::from(pos.get_z())
                        + (random.next_double() - random.next_double())
                            * f64::from(self.spawn_range)
                        + 0.5,
                )
            };
            if !world.no_collision(&entity_type, position)? {
                continue;
            }
            let spawn_pos = BlockPos::containing(position.x, position.y, position.z);
            if let Some(custom) = custom {
                if !world.custom_rules_allow(&entity_type, spawn_pos, custom)? {
                    continue;
                }
            } else if !world.check_spawn_placement(&entity_type, spawn_pos)? {
                continue;
            }
            match world.pre_spawner_event(&entity_type, position, pos)? {
                PreSpawnerDecision::Allow => {}
                PreSpawnerDecision::Cancel { abort } => {
                    delay = true;
                    if abort { break } else { continue }
                }
            }
            let Some(mut entity) = world.load_and_snap(entity_data, position)? else {
                self.reset_delay(world, pos)?;
                return Ok(());
            };
            if world.exact_class_non_spectator_count(&entity, pos, self.spawn_range)?
                >= self.max_nearby
            {
                self.reset_delay(world, pos)?;
                return Ok(());
            }
            let yaw = world.random().next_float() * 360.0;
            world.snap_yaw_zero_pitch(&mut entity, yaw);
            if world.is_mob(&entity) {
                if (custom.is_none() && !world.check_mob_spawn_rules(&mut entity)?)
                    || !world.check_mob_obstruction(&mut entity)?
                {
                    continue;
                }
                if entity_data.tags.len() == 1 && entity_data.get_string("id").is_some() {
                    world.finalize_spawn(&mut entity)?;
                }
                if let Some(equipment) = next.get_compound("equipment") {
                    world.equip(&mut entity, equipment)?;
                }
                if world.nerf_spawner_mobs() {
                    world.set_unaware(&mut entity);
                }
            }
            world.mark_spawner_origin(&mut entity);
            delay = true;
            if world.spawner_spawn_event_cancelled(&mut entity, pos)? {
                continue;
            }
            if !world.admit_and_emit(entity, pos, spawn_pos)? {
                self.reset_delay(world, pos)?;
                return Ok(());
            }
        }
        if delay {
            self.reset_delay(world, pos)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::tag_parser::parse_compound_fully;
    use rivet_util::random_source::LegacyRandomSource;

    struct BlockedWorld {
        random: LegacyRandomSource,
        rate: i32,
        near: bool,
        enabled: bool,
        near_calls: u64,
        collisions: u64,
        trace: Vec<String>,
    }
    impl SpawnerTickWorld for BlockedWorld {
        type Random = LegacyRandomSource;
        type Entity = ();
        fn random(&mut self) -> &mut Self::Random {
            &mut self.random
        }
        fn mob_spawner_tick_rate(&self) -> i32 {
            self.rate
        }
        fn near_affecting_player(
            &mut self,
            _: BlockPos,
            range: i32,
        ) -> Result<bool, SpawnerTickError> {
            assert_eq!(range, 16);
            self.near_calls += 1;
            Ok(self.near)
        }
        fn spawner_blocks_enabled(&self) -> bool {
            self.enabled
        }
        fn read_spawn_position(
            &self,
            data: &CompoundTag,
        ) -> Result<Option<Vec3>, SpawnerTickError> {
            let position = data
                .get_list("Pos")
                .filter(|list| list.list.len() >= 3)
                .and_then(|list| {
                    Some(Vec3::new(
                        list.list[0].as_number_f64()?,
                        list.list[1].as_number_f64()?,
                        list.list[2].as_number_f64()?,
                    ))
                });
            Ok(position)
        }
        fn no_collision(&mut self, _: &Identifier, _: Vec3) -> Result<bool, SpawnerTickError> {
            self.collisions += 1;
            Ok(false)
        }
        fn custom_rules_allow(
            &mut self,
            _: &Identifier,
            _: BlockPos,
            _: &CompoundTag,
        ) -> Result<bool, SpawnerTickError> {
            panic!("actual oracle collision rejected")
        }
        fn check_spawn_placement(
            &mut self,
            _: &Identifier,
            _: BlockPos,
        ) -> Result<bool, SpawnerTickError> {
            panic!("actual oracle collision rejected")
        }
        fn pre_spawner_event(
            &mut self,
            _: &Identifier,
            _: Vec3,
            _: BlockPos,
        ) -> Result<PreSpawnerDecision, SpawnerTickError> {
            panic!("actual oracle collision rejected")
        }
        fn load_and_snap(
            &mut self,
            _: &CompoundTag,
            _: Vec3,
        ) -> Result<Option<Self::Entity>, SpawnerTickError> {
            panic!("actual oracle collision rejected")
        }
        fn exact_class_non_spectator_count(
            &mut self,
            _: &Self::Entity,
            _: BlockPos,
            _: i32,
        ) -> Result<i32, SpawnerTickError> {
            unreachable!()
        }
        fn snap_yaw_zero_pitch(&mut self, _: &mut Self::Entity, _: f32) {
            unreachable!()
        }
        fn is_mob(&self, _: &Self::Entity) -> bool {
            unreachable!()
        }
        fn check_mob_spawn_rules(
            &mut self,
            _: &mut Self::Entity,
        ) -> Result<bool, SpawnerTickError> {
            unreachable!()
        }
        fn check_mob_obstruction(
            &mut self,
            _: &mut Self::Entity,
        ) -> Result<bool, SpawnerTickError> {
            unreachable!()
        }
        fn finalize_spawn(&mut self, _: &mut Self::Entity) -> Result<(), SpawnerTickError> {
            unreachable!()
        }
        fn equip(&mut self, _: &mut Self::Entity, _: &CompoundTag) -> Result<(), SpawnerTickError> {
            unreachable!()
        }
        fn nerf_spawner_mobs(&self) -> bool {
            unreachable!()
        }
        fn set_unaware(&mut self, _: &mut Self::Entity) {
            unreachable!()
        }
        fn mark_spawner_origin(&mut self, _: &mut Self::Entity) {
            unreachable!()
        }
        fn spawner_spawn_event_cancelled(
            &mut self,
            _: &mut Self::Entity,
            _: BlockPos,
        ) -> Result<bool, SpawnerTickError> {
            unreachable!()
        }
        fn admit_and_emit(
            &mut self,
            _: Self::Entity,
            _: BlockPos,
            _: BlockPos,
        ) -> Result<bool, SpawnerTickError> {
            unreachable!()
        }
        fn next_spawn_data_changed(&mut self, _: BlockPos) -> Result<(), SpawnerTickError> {
            self.trace.push("next".into());
            Ok(())
        }
        fn broadcast_spawn_event(&mut self, _: BlockPos) -> Result<(), SpawnerTickError> {
            self.trace.push("broadcast:1".into());
            Ok(())
        }
    }

    #[test]
    fn actual_paper_spawner_rate_gates_delay_weighted_selection_and_position_rng() {
        let pos = BlockPos::new(3, 64, -8);
        let mut cases = 0;
        for line in
            include_str!("../../../../tools/rivet-oracle/fixtures/spawner-tick.jsonl").lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let mut data = SpawnerData::load(
                pos,
                &parse_compound_fully(row["input_snbt"].as_str().unwrap()).unwrap(),
            )
            .unwrap();
            let mut world = BlockedWorld {
                random: LegacyRandomSource::new(99),
                rate: row["rate"].as_i64().unwrap() as i32,
                near: row["near"].as_bool().unwrap(),
                enabled: row["enabled"].as_bool().unwrap(),
                near_calls: 0,
                collisions: 0,
                trace: Vec::new(),
            };
            for _ in 0..row["ticks"].as_u64().unwrap() {
                data.server_tick(&mut world, pos).unwrap();
            }
            let mut saved = data.save(pos);
            for key in ["id", "x", "y", "z", "components"] {
                saved.remove(key);
            }
            assert_eq!(
                saved,
                parse_compound_fully(row["output_snbt"].as_str().unwrap()).unwrap(),
                "{row}"
            );
            assert_eq!(
                data.tick_delay,
                row["tick_delay"].as_i64().unwrap() as i32,
                "{row}"
            );
            assert_eq!(
                world.near_calls,
                row["near_calls"].as_u64().unwrap(),
                "{row}"
            );
            assert_eq!(
                world.collisions,
                row["collisions"].as_u64().unwrap(),
                "{row}"
            );
            assert_eq!(
                serde_json::to_value(world.trace).unwrap(),
                row["trace"],
                "{row}"
            );
            assert_eq!(
                world.random.next_long(),
                row["rng_next"].as_i64().unwrap(),
                "{row}"
            );
            cases += 1;
        }
        assert_eq!(cases, 53);
    }

    #[test]
    fn actual_paper_spawner_reload_preserves_throttle_phase_and_prior_spawn_choice() {
        let pos = BlockPos::new(3, 64, -8);
        let mut cases = 0;
        for line in
            include_str!("../../../../tools/rivet-oracle/fixtures/spawner-reload.jsonl").lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let mut data = SpawnerData::load(
                pos,
                &parse_compound_fully(row["initial_snbt"].as_str().unwrap()).unwrap(),
            )
            .unwrap();
            let mut world = BlockedWorld {
                random: LegacyRandomSource::new(99),
                rate: 4,
                near: false,
                enabled: true,
                near_calls: 0,
                collisions: 0,
                trace: Vec::new(),
            };
            data.server_tick(&mut world, pos).unwrap();
            assert_eq!(
                i64::from(data.tick_delay),
                row["before_counter"].as_i64().unwrap()
            );
            data.reload(
                pos,
                &parse_compound_fully(row["reload_snbt"].as_str().unwrap()).unwrap(),
            )
            .unwrap();
            data.server_tick(&mut world, pos).unwrap();
            // This is behavioral, not only a private counter comparison: the
            // post-load tick must stay throttled and never query players.
            assert_eq!(
                world.near_calls,
                row["near_calls"].as_u64().unwrap(),
                "{row}"
            );
            assert_eq!(
                i64::from(data.tick_delay),
                row["after_counter"].as_i64().unwrap(),
                "{row}"
            );
            let mut saved = data.save(pos);
            for key in ["id", "x", "y", "z", "components"] {
                saved.remove(key);
            }
            assert_eq!(
                saved,
                parse_compound_fully(row["output_snbt"].as_str().unwrap()).unwrap(),
                "{row}"
            );
            assert_eq!(
                world.random.next_long(),
                row["rng_next"].as_i64().unwrap(),
                "{row}"
            );
            cases += 1;
        }
        assert_eq!(cases, 3);
    }
}
