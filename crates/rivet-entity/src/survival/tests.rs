use super::{
    Difficulty, ExhaustionConfig, ExhaustionReason, ExperienceSaveData, ExperienceState,
    FoodConstants, FoodData, FoodProperties, FoodSaveData, Foods, MovementKind,
    PlayerSurvivalState, RegainReason, SurvivalEvent, SurvivalGameRules, SurvivalPlayerHost,
    SurvivalSaveData, xp_needed_for_next_level,
};

struct MockPlayerHost {
    health: f32,
    max_health: f32,
    invulnerable: bool,
    may_fly: bool,
    spectator: bool,
    tick_count: i32,
    score: i32,
    heals: Vec<(f32, RegainReason, bool)>,
    starve_hits: Vec<f32>,
    food_level_override: Option<Option<i32>>,
    exhaustion_override: Option<Option<f32>>,
}

impl MockPlayerHost {
    fn new(health: f32, max_health: f32) -> Self {
        Self {
            health,
            max_health,
            invulnerable: false,
            may_fly: false,
            spectator: false,
            tick_count: 1,
            score: 0,
            heals: Vec::new(),
            starve_hits: Vec::new(),
            food_level_override: None,
            exhaustion_override: None,
        }
    }
}

impl SurvivalPlayerHost for MockPlayerHost {
    fn health(&self) -> f32 {
        self.health
    }

    fn max_health(&self) -> f32 {
        self.max_health
    }

    fn is_invulnerable(&self) -> bool {
        self.invulnerable
    }

    fn may_fly(&self) -> bool {
        self.may_fly
    }

    fn is_spectator(&self) -> bool {
        self.spectator
    }

    fn tick_count(&self) -> i32 {
        self.tick_count
    }

    fn heal(&mut self, amount: f32, reason: RegainReason, is_fast_regen: bool) {
        if self.health > 0.0 {
            self.health = (self.health + amount).min(self.max_health);
        }
        self.heals.push((amount, reason, is_fast_regen));
    }

    fn hurt_starve(&mut self, amount: f32) -> bool {
        if self.invulnerable {
            return false;
        }
        self.health = (self.health - amount).max(0.0);
        self.starve_hits.push(amount);
        true
    }

    fn increase_score(&mut self, delta: i32) {
        self.score = self.score.wrapping_add(delta);
    }

    fn on_food_level_change(&mut self, _old_food_level: i32, new_food_level: i32) -> Option<i32> {
        self.food_level_override.unwrap_or(Some(new_food_level))
    }

    fn on_exhaustion(&mut self, _reason: ExhaustionReason, exhaustion: f32) -> Option<f32> {
        self.exhaustion_override.unwrap_or(Some(exhaustion))
    }
}

#[test]
fn food_defaults_and_vanilla_foods_match_paper() {
    let food = FoodData::new();
    assert_eq!(food.food_level(), 20);
    assert_eq!(food.saturation_level(), 5.0);
    assert_eq!(food.exhaustion_level, 0.0);
    assert_eq!(food.tick_timer(), 0);
    assert!(food.has_enough_food());
    assert!(!food.needs_food());

    assert_eq!(
        Foods::APPLE,
        FoodProperties::new(4, 4.0_f32 * 0.3_f32 * 2.0_f32, false)
    );
    assert_eq!(
        Foods::COOKED_BEEF,
        FoodProperties::new(8, 8.0_f32 * 0.8_f32 * 2.0_f32, false)
    );
    assert_eq!(
        Foods::GOLDEN_CARROT,
        FoodProperties::new(6, 6.0_f32 * 1.2_f32 * 2.0_f32, false)
    );
    assert_eq!(
        Foods::GOLDEN_APPLE,
        FoodProperties::new(4, 4.0_f32 * 1.2_f32 * 2.0_f32, true)
    );
    assert_eq!(
        Foods::RABBIT_STEW,
        FoodProperties::new(10, 10.0_f32 * 0.6_f32 * 2.0_f32, false)
    );
    assert_eq!(
        Foods::SUSPICIOUS_STEW,
        FoodProperties::new(6, 6.0_f32 * 0.6_f32 * 2.0_f32, true)
    );
}

#[test]
fn food_eat_clamps_food_and_saturation_to_food_level() {
    let mut food = FoodData::new();
    food.set_food_level(10);
    food.set_saturation(0.0);

    // Golden carrot gives +6 nutrition (food=16) and +14.4 saturation (<= 16.0).
    food.eat_properties(Foods::GOLDEN_CARROT);
    assert_eq!(food.food_level(), 16);
    assert!((food.saturation_level() - 14.4).abs() < 1e-5);

    // Eating again clamps food to 20 and saturation to 20.0.
    food.eat_properties(Foods::GOLDEN_CARROT);
    assert_eq!(food.food_level(), 20);
    assert_eq!(food.saturation_level(), 20.0);

    // When food is low, saturation is clamped to the resulting food_level, not 20.0.
    let mut low = FoodData::new();
    low.set_food_level(2);
    low.set_saturation(0.0);
    low.eat_properties(Foods::GOLDEN_APPLE); // +4 food -> 6, +9.6 sat -> clamped to 6.0
    assert_eq!(low.food_level(), 6);
    assert_eq!(low.saturation_level(), 6.0);
    assert!(!low.has_enough_food()); // 6 is NOT > 6
}

#[test]
fn exhaustion_strict_threshold_and_cap() {
    let mut food = FoodData::new();
    let mut host = MockPlayerHost::new(20.0, 20.0);
    let rules = SurvivalGameRules::default();
    let cfg = ExhaustionConfig::default();
    let mut events = Vec::new();

    // Exact 4.0 does NOT trigger drop (Paper: `exhaustionLevel > 4.0F`).
    food.add_exhaustion(4.0);
    assert_eq!(food.exhaustion_level, 4.0);
    food.tick(&mut host, Difficulty::Normal, rules, &cfg, &mut events);
    assert_eq!(food.exhaustion_level, 4.0);
    assert_eq!(food.saturation_level(), 5.0);

    // Crossing 4.0 drops 4.0 exhaustion and 1.0 saturation.
    food.add_exhaustion(0.5);
    food.tick(&mut host, Difficulty::Normal, rules, &cfg, &mut events);
    assert!((food.exhaustion_level - 0.5).abs() < 1e-6);
    assert_eq!(food.saturation_level(), 4.0);
    assert_eq!(food.food_level(), 20);

    // With saturation == 0.0, drops 1 food on Normal and emits FoodLevelChange + SendHealthPacket.
    food.set_saturation(0.0);
    food.exhaustion_level = 4.25;
    events.clear();
    food.tick(&mut host, Difficulty::Normal, rules, &cfg, &mut events);
    assert!((food.exhaustion_level - 0.25).abs() < 1e-6);
    assert_eq!(food.food_level(), 19);
    assert_eq!(
        events,
        vec![
            SurvivalEvent::FoodLevelChange {
                old_food_level: 20,
                new_food_level: 19,
            },
            SurvivalEvent::SendHealthPacket {
                health: 20.0,
                food_level: 19,
                saturation: 0.0,
            },
        ]
    );

    // Peaceful does NOT drain food when saturation is 0.0, even though exhaustion still drops.
    food.exhaustion_level = 5.0;
    events.clear();
    food.tick(&mut host, Difficulty::Peaceful, rules, &cfg, &mut events);
    assert_eq!(food.exhaustion_level, 1.0);
    assert_eq!(food.food_level(), 19);
    assert!(events.is_empty());

    // Cap at 40.0.
    food.add_exhaustion(100.0);
    assert_eq!(food.exhaustion_level, FoodConstants::MAX_EXHAUSTION);
}

#[test]
fn saturated_regeneration_timing_partial_saturation_and_exhaustion() {
    let mut food = FoodData::new();
    food.set_food_level(20);
    food.set_saturation(3.0);
    let mut host = MockPlayerHost::new(15.0, 20.0);
    let rules = SurvivalGameRules::default();
    let cfg = ExhaustionConfig::default();
    let mut events = Vec::new();

    for tick in 1..10 {
        food.tick(&mut host, Difficulty::Normal, rules, &cfg, &mut events);
        assert_eq!(food.tick_timer(), tick);
        assert_eq!(host.health, 15.0);
    }

    // Tick 10 triggers saturated regen: min(3.0, 6.0) / 6.0 = 0.5 HP and 3.0 exhaustion.
    food.tick(&mut host, Difficulty::Normal, rules, &cfg, &mut events);
    assert_eq!(food.tick_timer(), 0);
    assert!((host.health - 15.5).abs() < 1e-6);
    assert!((food.exhaustion_level - 3.0).abs() < 1e-6);
    assert_eq!(
        events,
        vec![
            SurvivalEvent::Heal {
                amount: 0.5,
                reason: RegainReason::Satiated,
                is_fast_regen: true,
            },
            SurvivalEvent::Exhaustion {
                amount: 3.0,
                reason: ExhaustionReason::Regen,
            },
        ]
    );
}

#[test]
fn unsaturated_regeneration_timing_and_gamerule_disable() {
    let mut food = FoodData::new();
    food.set_food_level(18);
    food.set_saturation(0.0);
    let mut host = MockPlayerHost::new(10.0, 20.0);
    let cfg = ExhaustionConfig::default();
    let mut events = Vec::new();

    // Gamerule disabled prevents regen and keeps tick_timer at 0.
    let no_regen = SurvivalGameRules {
        natural_health_regeneration: false,
        keep_inventory: false,
    };
    for _ in 0..85 {
        food.tick(&mut host, Difficulty::Normal, no_regen, &cfg, &mut events);
    }
    assert_eq!(host.health, 10.0);
    assert_eq!(food.tick_timer(), 0);
    assert!(events.is_empty());

    // Gamerule enabled heals 1.0 HP on tick 80 and adds 6.0 exhaustion.
    let regen = SurvivalGameRules::default();
    for _ in 0..79 {
        food.tick(&mut host, Difficulty::Normal, regen, &cfg, &mut events);
        assert_eq!(host.health, 10.0);
    }
    food.tick(&mut host, Difficulty::Normal, regen, &cfg, &mut events);
    assert_eq!(host.health, 11.0);
    assert_eq!(food.exhaustion_level, 6.0);
    assert_eq!(food.tick_timer(), 0);
}

#[test]
fn starvation_difficulty_thresholds_and_timer_reset() {
    let cfg = ExhaustionConfig::default();
    let rules = SurvivalGameRules::default();

    // Easy stops damaging at 10.0 HP.
    let mut easy_food = FoodData::new();
    easy_food.set_food_level(0);
    easy_food.set_saturation(0.0);
    let mut easy_host = MockPlayerHost::new(11.0, 20.0);
    let mut events = Vec::new();
    for _ in 0..80 {
        easy_food.tick(&mut easy_host, Difficulty::Easy, rules, &cfg, &mut events);
    }
    assert_eq!(easy_host.health, 10.0);
    assert_eq!(easy_food.tick_timer(), 0);

    // Another 80 ticks on Easy at 10.0 HP deals no damage, still resets timer.
    events.clear();
    for _ in 0..80 {
        easy_food.tick(&mut easy_host, Difficulty::Easy, rules, &cfg, &mut events);
    }
    assert_eq!(easy_host.health, 10.0);
    assert_eq!(easy_food.tick_timer(), 0);
    assert!(events.is_empty());

    // Normal damages down to 1.0 HP, then stops.
    let mut normal_food = FoodData::new();
    normal_food.set_food_level(0);
    normal_food.set_saturation(0.0);
    let mut normal_host = MockPlayerHost::new(2.0, 20.0);
    for _ in 0..160 {
        normal_food.tick(
            &mut normal_host,
            Difficulty::Normal,
            rules,
            &cfg,
            &mut events,
        );
    }
    assert_eq!(normal_host.health, 1.0);

    // Hard kills the player (damages from 1.0 -> 0.0).
    let mut hard_food = FoodData::new();
    hard_food.set_food_level(0);
    hard_food.set_saturation(0.0);
    let mut hard_host = MockPlayerHost::new(1.0, 20.0);
    for _ in 0..80 {
        hard_food.tick(&mut hard_host, Difficulty::Hard, rules, &cfg, &mut events);
    }
    assert_eq!(hard_host.health, 0.0);
}

#[test]
fn experience_thresholds_progression_negative_and_levelup_sound() {
    assert_eq!(xp_needed_for_next_level(0), 7);
    assert_eq!(xp_needed_for_next_level(14), 35);
    assert_eq!(xp_needed_for_next_level(15), 37);
    assert_eq!(xp_needed_for_next_level(29), 107);
    assert_eq!(xp_needed_for_next_level(30), 112);
    assert_eq!(xp_needed_for_next_level(31), 121);

    let mut xp = ExperienceState::new();
    let mut events = Vec::new();

    // 7 + 9 + 11 + 13 + 15 = 55 points reaches level 5 exactly.
    // Tick 200 satisfies last_level_up_time (0) < 200 - 100.
    let score_delta = xp.give_experience_points(55, 200, &mut events);
    assert_eq!(score_delta, 55);
    assert_eq!(xp.experience_level, 5);
    assert_eq!(xp.total_experience, 55);
    assert!(xp.experience_progress.abs() < 1e-6);
    assert_eq!(xp.last_sent_exp, -1);
    assert_eq!(
        events,
        vec![SurvivalEvent::LevelUpSound {
            volume: (5.0_f32 / 30.0) * 0.75,
            pitch: 1.0,
        }]
    );

    // Level-up sound cooldown: hitting level 10 within 100 ticks does NOT emit sound.
    events.clear();
    xp.give_experience_levels(5, 250, &mut events);
    assert_eq!(xp.experience_level, 10);
    assert!(events.is_empty());

    // Negative points carry across level boundary.
    xp.give_experience_points(-10, 400, &mut events);
    assert_eq!(xp.experience_level, 9);
    assert_eq!(xp.total_experience, 45);
    assert!((xp.experience_progress - (1.0 - 10.0 / 25.0)).abs() < 1e-6);

    // Saturating add at i32::MAX does not wrap negative.
    xp.set_experience_levels(i32::MAX - 2);
    xp.give_experience_levels(10, 500, &mut events);
    assert_eq!(xp.experience_level, i32::MAX);

    // Dropping below 0 resets level, progress, and total_experience.
    xp.set_experience_levels(2);
    xp.set_experience_points(4);
    xp.on_enchantment_performed(3);
    assert_eq!(xp.experience_level, 0);
    assert_eq!(xp.experience_progress, 0.0);
    assert_eq!(xp.total_experience, 0);

    // Base death XP reward formula: min(level * 7, 100), 0 on keepInventory or spectator.
    xp.set_experience_levels(10);
    assert_eq!(xp.base_experience_reward(false, false), 70);
    xp.set_experience_levels(20);
    assert_eq!(xp.base_experience_reward(false, false), 100);
    assert_eq!(xp.base_experience_reward(true, false), 0);
    assert_eq!(xp.base_experience_reward(false, true), 0);
}

#[test]
fn do_tick_ordering_sync_sentinels_and_peaceful_before_food_tick() {
    let mut state = PlayerSurvivalState::new();
    let mut host = MockPlayerHost::new(19.0, 20.0);
    host.tick_count = 20;
    state.food.set_food_level(19);
    state.food.set_saturation(0.0);
    let rules = SurvivalGameRules::default();
    let cfg = ExhaustionConfig::default();

    // On Peaceful tick 20:
    // 1. tick_regeneration heals 19 -> 20, saturation 0 -> 1, food 19 -> 20 (tick % 10 == 0)
    // 2. food.tick runs AFTER tick_regeneration (player is now full HP, so no saturated regen)
    // 3. SendHealthPacket + SendExperiencePacket emitted in exact ServerPlayer.doTick order.
    let events = state.do_tick(&mut host, Difficulty::Peaceful, rules, &cfg, true, false);
    assert_eq!(host.health, 20.0);
    assert_eq!(state.food.food_level(), 20);
    assert_eq!(state.food.saturation_level(), 1.0);
    assert_eq!(
        events,
        vec![
            SurvivalEvent::Heal {
                amount: 1.0,
                reason: RegainReason::Regen,
                is_fast_regen: false,
            },
            SurvivalEvent::SendHealthPacket {
                health: 20.0,
                food_level: 20,
                saturation: 1.0,
            },
            SurvivalEvent::SendExperiencePacket {
                progress: 0.0,
                total_experience: 0,
                level: 0,
            },
        ]
    );

    // Steady-state next tick emits NO packets.
    host.tick_count = 21;
    let steady = state.do_tick(&mut host, Difficulty::Normal, rules, &cfg, true, false);
    assert!(steady.is_empty());

    // Saturation changing 5.0 -> 4.0 (still > 0) does NOT trigger SendHealthPacket,
    // matching ServerPlayer.java:834 (`saturation == 0.0F != lastFoodSaturationZero`).
    state.food.set_saturation(4.0);
    let non_zero_sat = state.do_tick(&mut host, Difficulty::Normal, rules, &cfg, true, false);
    assert!(non_zero_sat.is_empty());

    // Transitioning saturation 4.0 -> 0.0 DOES trigger SendHealthPacket.
    state.food.set_saturation(0.0);
    let zero_sat = state.do_tick(&mut host, Difficulty::Normal, rules, &cfg, true, false);
    assert_eq!(
        zero_sat,
        vec![SurvivalEvent::SendHealthPacket {
            health: 20.0,
            food_level: 20,
            saturation: 0.0,
        }]
    );

    // reset_sent_info forces both health and experience resync on next do_tick.
    state.reset_sent_info();
    let resync = state.do_tick(&mut host, Difficulty::Normal, rules, &cfg, true, false);
    assert_eq!(resync.len(), 2);
    assert!(matches!(resync[0], SurvivalEvent::SendHealthPacket { .. }));
    assert!(matches!(
        resync[1],
        SurvivalEvent::SendExperiencePacket { .. }
    ));
}

#[test]
fn restore_from_respawn_and_end_portal_and_save_round_trip() {
    let mut old = PlayerSurvivalState::new();
    let mut host = MockPlayerHost::new(14.0, 20.0);
    host.tick_count = 200;
    let mut events = Vec::new();
    old.food.set_food_level(12);
    old.food.set_saturation(2.0);
    old.give_experience_points(&mut host, 30, &mut events);

    // Death respawn without keepInventory: fresh food, XP/score reset, sentinels invalidated.
    let mut respawned = PlayerSurvivalState::new();
    let death_outcome = respawned.restore_from(
        &old,
        0.0,
        false,
        false,
        SurvivalGameRules {
            natural_health_regeneration: true,
            keep_inventory: false,
        },
    );
    assert_eq!(death_outcome.restored_health, None);
    assert!(!death_outcome.kept_inventory_and_xp);
    assert_eq!(respawned.food.food_level(), 20);
    assert_eq!(respawned.food.saturation_level(), 5.0);
    assert_eq!(respawned.experience.experience_level, 0);
    assert_eq!(respawned.experience.total_experience, 0);
    assert_eq!(respawned.score, 0);
    assert_eq!(respawned.experience.last_sent_exp, -1);
    assert_eq!(respawned.last_sent_health, -1.0);
    assert_eq!(respawned.last_sent_food, -1);

    // Death respawn WITH keepInventory: fresh food, XP & score preserved.
    let mut kept = PlayerSurvivalState::new();
    let keep_outcome = kept.restore_from(
        &old,
        0.0,
        false,
        false,
        SurvivalGameRules {
            natural_health_regeneration: true,
            keep_inventory: true,
        },
    );
    assert!(keep_outcome.kept_inventory_and_xp);
    assert_eq!(kept.food.food_level(), 20);
    assert_eq!(
        kept.experience.total_experience,
        old.experience.total_experience
    );
    assert_eq!(kept.score, old.score);

    // restore_all (End portal): keeps health, food, XP, and score.
    let mut portal = PlayerSurvivalState::new();
    let portal_outcome = portal.restore_from(&old, 14.0, false, true, SurvivalGameRules::default());
    assert_eq!(portal_outcome.restored_health, Some(14.0));
    assert_eq!(portal.food.food_level(), 12);
    assert_eq!(portal.food.saturation_level(), 2.0);
    assert_eq!(
        portal.experience.total_experience,
        old.experience.total_experience
    );

    // Save/load round-trip.
    let saved = old.write_save_data();
    let mut loaded = PlayerSurvivalState::new();
    loaded.read_save_data(saved);
    assert_eq!(loaded.food.food_level(), 12);
    assert_eq!(loaded.food.saturation_level(), 2.0);
    assert_eq!(
        loaded.experience.total_experience,
        old.experience.total_experience
    );
    assert_eq!(
        loaded.experience.experience_level,
        old.experience.experience_level
    );

    // Empty save data restores Paper defaults.
    loaded.read_save_data(SurvivalSaveData {
        food: FoodSaveData {
            food_level: None,
            food_tick_timer: None,
            food_saturation_level: None,
            food_exhaustion_level: None,
        },
        experience: ExperienceSaveData {
            xp_progress: None,
            xp_level: None,
            xp_total: None,
        },
    });
    assert_eq!(loaded.food.food_level(), 20);
    assert_eq!(loaded.food.saturation_level(), 5.0);
    assert_eq!(loaded.experience.experience_level, 0);
}

#[test]
fn movement_and_jump_exhaustion_and_dead_player_guard() {
    let mut state = PlayerSurvivalState::new();
    let mut host = MockPlayerHost::new(20.0, 20.0);
    let cfg = ExhaustionConfig::default();
    let mut events = Vec::new();

    // Sprint jump adds 0.2, normal jump adds 0.05.
    state.on_jump(&mut host, true, &cfg, &mut events);
    state.on_jump(&mut host, false, &cfg, &mut events);
    assert!((state.food.exhaustion_level - 0.25).abs() < 1e-6);

    // Sprinting 10 blocks horizontally -> 1000 cm * 0.1 * 0.01 = 1.0 exhaustion.
    state.check_movement_exhaustion(
        &mut host,
        MovementKind::OnGroundSprinting,
        [10.0, 0.0, 0.0],
        &cfg,
        &mut events,
    );
    assert!((state.food.exhaustion_level - 1.25).abs() < 1e-6);

    // Invulnerable player accumulates zero exhaustion.
    host.invulnerable = true;
    state.on_jump(&mut host, true, &cfg, &mut events);
    assert!((state.food.exhaustion_level - 1.25).abs() < 1e-6);

    // Invalid/pending-respawn player skips food tick (Paper doTick guard).
    host.invulnerable = false;
    state.food.exhaustion_level = 5.0;
    let _ = state.do_tick(
        &mut host,
        Difficulty::Normal,
        SurvivalGameRules::default(),
        &cfg,
        false,
        false,
    );
    assert_eq!(state.food.exhaustion_level, 5.0);
}

#[test]
fn controlled_negative_detects_non_paper_exhaustion_and_easy_starvation() {
    // Controlled negative #1: a buggy implementation using `>= 4.0` instead of Paper's `> 4.0`
    // would drain saturation at exact 4.0 exhaustion. Verify our implementation rejects that bug.
    let mut food = FoodData::new();
    let mut host = MockPlayerHost::new(20.0, 20.0);
    let mut events = Vec::new();
    food.add_exhaustion(4.0);
    food.tick(
        &mut host,
        Difficulty::Normal,
        SurvivalGameRules::default(),
        &ExhaustionConfig::default(),
        &mut events,
    );
    let buggy_drained_at_exact_4 = food.saturation_level() < 5.0;
    assert!(
        !buggy_drained_at_exact_4,
        "controlled negative: exhaustion == 4.0 must not drain saturation"
    );

    // Controlled negative #2: a buggy Easy-mode starvation check that ignores the `health > 10.0`
    // floor would damage a 10.0 HP player. Verify Paper's floor holds.
    let mut starving = FoodData::new();
    starving.set_food_level(0);
    starving.set_saturation(0.0);
    let mut easy_player = MockPlayerHost::new(10.0, 20.0);
    for _ in 0..80 {
        starving.tick(
            &mut easy_player,
            Difficulty::Easy,
            SurvivalGameRules::default(),
            &ExhaustionConfig::default(),
            &mut events,
        );
    }
    let buggy_easy_damaged_at_10hp = easy_player.health < 10.0;
    assert!(
        !buggy_easy_damaged_at_10hp,
        "controlled negative: Easy starvation must stop at 10.0 HP"
    );
}

#[test]
fn synchronous_event_cancellation_and_override_affect_progression() {
    let mut state = PlayerSurvivalState::new();
    let mut host = MockPlayerHost::new(15.0, 20.0);
    let rules = SurvivalGameRules::default();
    let cfg = ExhaustionConfig::default();
    let mut events = Vec::new();

    // 1. Cancelling FoodLevelChangeEvent on eat prevents food/saturation mutation,
    //    but still sends the health packet (matching Paper FoodData.eat lines 40-44).
    state.food.set_food_level(10);
    state.food.set_saturation(2.0);
    host.food_level_override = Some(None);
    state.eat_food(&mut host, Foods::APPLE, &mut events);
    assert_eq!(state.food.food_level(), 10);
    assert_eq!(state.food.saturation_level(), 2.0);
    assert_eq!(
        events,
        vec![SurvivalEvent::SendHealthPacket {
            health: 15.0,
            food_level: 10,
            saturation: 2.0,
        }]
    );

    // 2. Modifying new_food_level on eat applies the handler-modified level.
    events.clear();
    host.food_level_override = Some(Some(17));
    state.eat_food(&mut host, Foods::APPLE, &mut events);
    assert_eq!(state.food.food_level(), 17);
    assert!((state.food.saturation_level() - (2.0 + Foods::APPLE.saturation)).abs() < 1e-6);
    assert_eq!(
        events[0],
        SurvivalEvent::FoodLevelChange {
            old_food_level: 10,
            new_food_level: 17,
        }
    );

    // 3. Cancelling FoodLevelChangeEvent on exhaustion drain keeps food_level unchanged.
    events.clear();
    state.food.set_food_level(10);
    state.food.set_saturation(0.0);
    state.food.exhaustion_level = 4.5;
    host.food_level_override = Some(None);
    state
        .food
        .tick(&mut host, Difficulty::Normal, rules, &cfg, &mut events);
    assert_eq!(state.food.food_level(), 10);
    assert_eq!(
        events,
        vec![SurvivalEvent::SendHealthPacket {
            health: 15.0,
            food_level: 10,
            saturation: 0.0,
        }]
    );

    // 4. Cancelling EntityExhaustionEvent skips exhaustion in both cause_food_exhaustion and regen.
    events.clear();
    state.food.exhaustion_level = 0.0;
    host.exhaustion_override = Some(None);
    state.cause_food_exhaustion(&mut host, 2.0, ExhaustionReason::Jump, &mut events);
    assert_eq!(state.food.exhaustion_level, 0.0);
    assert!(events.is_empty());

    // 5. Modifying exhaustion in EntityExhaustionEvent applies the modified amount.
    host.exhaustion_override = Some(Some(0.75));
    state.cause_food_exhaustion(&mut host, 2.0, ExhaustionReason::Jump, &mut events);
    assert!((state.food.exhaustion_level - 0.75).abs() < 1e-6);
    assert_eq!(
        events,
        vec![SurvivalEvent::Exhaustion {
            amount: 0.75,
            reason: ExhaustionReason::Jump,
        }]
    );
}

#[test]
fn java_round_f32_matches_jdk_6430675_boundaries() {
    use super::player_survival::java_round_f32;

    // 0.49999997_f32 (0x3effffff): naive `(v + 0.5_f32).floor()` returns 1; Java returns 0.
    assert_eq!(java_round_f32(f32::from_bits(0x3eff_ffff)), 0);
    assert_eq!(java_round_f32(0.5_f32), 1);
    assert_eq!(java_round_f32(-0.5_f32), 0);
    // -0.50000006_f32 (0xbf000001): strictly below -0.5 rounds to -1.
    assert_eq!(java_round_f32(f32::from_bits(0xbf00_0001)), -1);

    // Odd integers >= 2^23: naive `(v + 0.5_f32)` rounds to even (+1); Java preserves exact integer.
    assert_eq!(java_round_f32(8_388_608.0_f32), 8_388_608);
    assert_eq!(java_round_f32(8_388_609.0_f32), 8_388_609);
    assert_eq!(java_round_f32(16_777_215.0_f32), 16_777_215);
    assert_eq!(java_round_f32(-8_388_609.0_f32), -8_388_609);

    // NaN and infinities / i32 saturation bounds.
    assert_eq!(java_round_f32(f32::NAN), 0);
    assert_eq!(java_round_f32(f32::INFINITY), i32::MAX);
    assert_eq!(java_round_f32(f32::NEG_INFINITY), i32::MIN);
    assert_eq!(java_round_f32(2_147_483_648.0_f32), i32::MAX);
    assert_eq!(java_round_f32(-2_147_483_648.0_f32), i32::MIN);

    // Movement exhaustion at sub-0.5 cm distance must round to 0 cm and emit no exhaustion.
    let mut state = PlayerSurvivalState::new();
    let mut host = MockPlayerHost::new(20.0, 20.0);
    let cfg = ExhaustionConfig::default();
    let mut events = Vec::new();
    let dx = 0.004_999_999_4_f64;
    state.check_movement_exhaustion(
        &mut host,
        MovementKind::OnGroundSprinting,
        [dx, 0.0, 0.0],
        &cfg,
        &mut events,
    );
    assert_eq!(state.food.exhaustion_level, 0.0);
    assert!(events.is_empty());
}

#[test]
fn zero_delta_experience_calls_unconditionally_invalidate_last_sent_exp() {
    let mut state = PlayerSurvivalState::new();
    let mut host = MockPlayerHost::new(20.0, 20.0);
    let rules = SurvivalGameRules::default();
    let cfg = ExhaustionConfig::default();
    let mut events = Vec::new();

    // Prime sync state via an initial tick so last_sent_exp == total_experience (0).
    let _ = state.do_tick(&mut host, Difficulty::Normal, rules, &cfg, true, false);
    assert_eq!(state.experience.last_sent_exp, 0);

    // give_experience_points(0) leaves XP/score unchanged, sets last_sent_exp = -1,
    // and forces a SendExperiencePacket on the next tick.
    state.give_experience_points(&mut host, 0, &mut events);
    assert_eq!(state.experience.last_sent_exp, -1);
    assert_eq!(state.experience.total_experience, 0);
    assert_eq!(host.score, 0);
    assert!(events.is_empty());

    let tick_events = state.do_tick(&mut host, Difficulty::Normal, rules, &cfg, true, false);
    assert_eq!(state.experience.last_sent_exp, 0);
    assert!(tick_events.iter().any(|e| matches!(
        e,
        SurvivalEvent::SendExperiencePacket {
            progress,
            total_experience: 0,
            level: 0,
        } if *progress == 0.0
    )));

    // give_experience_levels(0) likewise sets last_sent_exp = -1 and forces a sync packet.
    state.give_experience_levels(&host, 0, &mut events);
    assert_eq!(state.experience.last_sent_exp, -1);
    assert_eq!(state.experience.experience_level, 0);
    assert!(events.is_empty());

    let tick_events = state.do_tick(&mut host, Difficulty::Normal, rules, &cfg, true, false);
    assert_eq!(state.experience.last_sent_exp, 0);
    assert!(tick_events.iter().any(|e| matches!(
        e,
        SurvivalEvent::SendExperiencePacket {
            progress,
            total_experience: 0,
            level: 0,
        } if *progress == 0.0
    )));
}
