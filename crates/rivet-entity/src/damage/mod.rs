//! Damage sources, combat rules, combat tracking, health, death, and drops (Paper 26.2).

pub mod combat_rules;
pub mod combat_tracker;
pub mod damage_source;
pub mod damage_type;
pub mod pipeline;

pub use combat_rules::{
    ARMOR_PROTECTION_DIVIDER, BASE_ARMOR_TOUGHNESS, Difficulty, MAX_ARMOR, MIN_ARMOR_RATIO,
    apply_resistance, get_damage_after_absorb, get_damage_after_absorb_with_modifier,
    get_damage_after_magic_absorb, java_max_f32, java_max_f64, java_min_f32, java_min_f64,
    mth_clamp_f32, scale_damage_for_difficulty,
};
pub use combat_tracker::{CombatEntry, CombatTracker, FallLocation};
pub use damage_source::{DamageEntityRef, DamageSource, DamageSources, DeathMessage};
pub use damage_type::{
    DamageEffects, DamageScaling, DamageTagSet, DamageType, DamageTypeKey, DamageTypeTag,
    DeathMessageType,
};
pub use pipeline::{
    DEATH_REMOVAL_TICKS, DamageModifiers, DamagePipelineState, DeathOutcome, DeathTickOutcome,
    HURT_DURATION, HurtContext, HurtOutcome, HurtRejection, INVULNERABLE_DURATION,
    PLAYER_HURT_EXPERIENCE_TIME,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn damage_types_bootstrap_matches_paper_26_2() {
        assert_eq!(DamageTypeKey::ALL.len(), 51);

        let fall = DamageTypeKey::Fall.vanilla_type();
        assert_eq!(fall.msg_id, "fall");
        assert_eq!(fall.scaling, DamageScaling::WhenCausedByLivingNonPlayer);
        assert_eq!(fall.exhaustion, 0.0);
        assert_eq!(fall.effects, DamageEffects::Hurt);
        assert_eq!(fall.death_message_type, DeathMessageType::FallVariants);

        let bad_respawn = DamageTypeKey::BadRespawnPoint.vanilla_type();
        assert_eq!(bad_respawn.msg_id, "badRespawnPoint");
        assert_eq!(bad_respawn.scaling, DamageScaling::Always);
        assert_eq!(bad_respawn.exhaustion, 0.1);
        assert_eq!(
            bad_respawn.death_message_type,
            DeathMessageType::IntentionalGameDesign
        );

        let campfire = DamageTypeKey::Campfire.vanilla_type();
        assert_eq!(campfire.msg_id, "inFire");
        assert_eq!(campfire.effects, DamageEffects::Burning);

        let sonic = DamageTypeKey::SonicBoom.vanilla_type();
        assert_eq!(sonic.scaling, DamageScaling::Always);
        assert_eq!(sonic.exhaustion, 0.0);
    }

    #[test]
    fn damage_type_tags_resolve_nested_memberships() {
        // #minecraft:bypasses_armor is nested inside bypasses_shield.
        assert!(DamageTypeKey::Magic.is_in_tag(DamageTypeTag::BypassesArmor));
        assert!(DamageTypeKey::Magic.is_in_tag(DamageTypeTag::BypassesShield));
        assert!(DamageTypeKey::Cactus.is_in_tag(DamageTypeTag::BypassesShield));
        assert!(!DamageTypeKey::Cactus.is_in_tag(DamageTypeTag::BypassesArmor));

        // #minecraft:is_explosion is nested inside always_hurts_ender_dragons and avoids_guardian_thorns.
        assert!(DamageTypeKey::BadRespawnPoint.is_in_tag(DamageTypeTag::IsExplosion));
        assert!(DamageTypeKey::BadRespawnPoint.is_in_tag(DamageTypeTag::AlwaysHurtsEnderDragons));
        assert!(DamageTypeKey::BadRespawnPoint.is_in_tag(DamageTypeTag::AvoidsGuardianThorns));

        // #minecraft:is_player_attack is nested inside can_break_armor_stand and panic_causes.
        assert!(DamageTypeKey::MaceSmash.is_in_tag(DamageTypeTag::IsPlayerAttack));
        assert!(DamageTypeKey::MaceSmash.is_in_tag(DamageTypeTag::CanBreakArmorStand));
        assert!(DamageTypeKey::MaceSmash.is_in_tag(DamageTypeTag::PanicCauses));
        assert!(DamageTypeKey::MaceSmash.is_in_tag(DamageTypeTag::IsMaceSmash));

        // #minecraft:bypasses_invulnerability is nested inside bypasses_wolf_armor.
        assert!(DamageTypeKey::FellOutOfWorld.is_in_tag(DamageTypeTag::BypassesInvulnerability));
        assert!(DamageTypeKey::FellOutOfWorld.is_in_tag(DamageTypeTag::BypassesWolfArmor));
        assert!(!DamageTypeKey::MobAttack.is_in_tag(DamageTypeTag::BypassesCooldown));
    }

    #[test]
    fn combat_rules_armor_and_magic_exact_paper_formulas() {
        // Diamond armor (20 armor, 8 toughness), 10 damage:
        // toughness = 2 + 8/4 = 4; realArmor = clamp(20 - 10/4, 4, 20) = 17.5
        // multiplier = 1 - 17.5/25 = 0.3; result = 3.0
        assert_close(get_damage_after_absorb(10.0, 20.0, 8.0), 3.0);

        // Full armor floor clamp: 20 armor, 0 toughness, 100 damage:
        // toughness = 2; 20 - 50 = -30 -> clamped to 20 * 0.2 = 4.0
        // multiplier = 1 - 4/25 = 0.84; result = 84.0
        assert_close(get_damage_after_absorb(100.0, 20.0, 0.0), 84.0);

        // Upper armor clamp at 20: 30 armor, 0 toughness, 2 damage -> realArmor = 20 -> 2 * 0.2 = 0.4
        assert_close(get_damage_after_absorb(2.0, 30.0, 0.0), 0.4);

        // Magic absorption: Protection IV x4 = 16 -> 10 * (1 - 16/25) = 3.6
        assert_close(get_damage_after_magic_absorb(10.0, 16.0), 3.6);
        // Magic absorption cap at 20 -> 10 * (1 - 20/25) = 2.0
        assert_close(get_damage_after_magic_absorb(10.0, 30.0), 2.0);
        // Negative magic armor clamps to 0.0 -> full damage
        assert_close(get_damage_after_magic_absorb(10.0, -5.0), 10.0);

        // Resistance I (amp=0) -> 20% reduction; Resistance V (amp=4) -> 100% reduction
        assert_close(apply_resistance(10.0, 0), 8.0);
        assert_close(apply_resistance(10.0, 4), 0.0);
        assert_close(apply_resistance(10.0, 10), 0.0);
    }

    #[test]
    fn difficulty_scaling_matrix_matches_player_hurt_server() {
        let sources = DamageSources::new();
        let zombie = DamageEntityRef::living(10, "Zombie");
        let player = DamageEntityRef::player(20, "Alex", false);

        let mob_attack = sources.mob_attack(zombie);
        let player_attack = sources.player_attack(player);
        let explosion = sources.explosion(None, None);

        assert!(mob_attack.scales_with_difficulty());
        assert!(!player_attack.scales_with_difficulty());
        assert!(explosion.scales_with_difficulty());

        // Peaceful cancels scaling damage sources completely.
        assert_eq!(
            scale_damage_for_difficulty(6.0, Difficulty::Peaceful, &mob_attack),
            None
        );
        // Non-scaling player attack is untouched even on Peaceful.
        assert_eq!(
            scale_damage_for_difficulty(6.0, Difficulty::Peaceful, &player_attack),
            Some(6.0)
        );
        // Easy: min(damage / 2 + 1, damage)
        assert_eq!(
            scale_damage_for_difficulty(6.0, Difficulty::Easy, &mob_attack),
            Some(4.0)
        );
        assert_eq!(
            scale_damage_for_difficulty(1.0, Difficulty::Easy, &mob_attack),
            Some(1.0)
        );
        // Normal: unchanged
        assert_eq!(
            scale_damage_for_difficulty(6.0, Difficulty::Normal, &mob_attack),
            Some(6.0)
        );
        // Hard: damage * 3 / 2
        assert_eq!(
            scale_damage_for_difficulty(6.0, Difficulty::Hard, &mob_attack),
            Some(9.0)
        );
    }

    #[test]
    fn damage_source_factories_and_attribution() {
        let sources = DamageSources::new();
        let fireball = DamageEntityRef::non_living(1, "Fireball").with_position([1.0, 2.0, 3.0]);
        let ghast = DamageEntityRef::living(2, "Ghast");

        let unattributed = sources.fireball(fireball.clone(), None);
        assert_eq!(unattributed.type_key(), DamageTypeKey::UnattributedFireball);
        assert!(unattributed.is_direct());
        assert_eq!(unattributed.source_position(), Some([1.0, 2.0, 3.0]));
        assert_eq!(unattributed.source_position_raw(), None);

        let attributed = sources.fireball(fireball, Some(ghast));
        assert_eq!(attributed.type_key(), DamageTypeKey::Fireball);
        assert!(!attributed.is_direct());

        let tnt = DamageEntityRef::non_living(3, "TNT");
        let steve = DamageEntityRef::player(4, "Steve", true);
        let player_boom = sources.explosion(Some(tnt), Some(steve));
        assert_eq!(player_boom.type_key(), DamageTypeKey::PlayerExplosion);
        assert!(player_boom.is_creative_player());

        let env_boom = sources.explosion(None, None);
        assert_eq!(env_boom.type_key(), DamageTypeKey::Explosion);
    }

    #[test]
    fn invulnerability_window_strict_greater_bypass_and_negative_rejection() {
        let sources = DamageSources::new();
        let zombie = DamageEntityRef::living(10, "Zombie");
        let mut state = DamagePipelineState::new(20.0);
        let ctx = HurtContext {
            victim_name: "Steve".into(),
            tick_count: 100,
            ..Default::default()
        };

        // First hit deals 4.0 and sets invulnerable_time = 20, last_hurt = 4.0.
        let first = state.hurt_server(sources.mob_attack(zombie.clone()), 4.0, &ctx);
        match first {
            HurtOutcome::Damaged {
                modifiers,
                took_full_damage,
                entered_combat,
                ..
            } => {
                assert_close(modifiers.final_damage, 4.0);
                assert!(took_full_damage);
                assert!(entered_combat);
            }
            other => panic!("expected Damaged, got {other:?}"),
        }
        assert_close(state.health, 16.0);
        assert_eq!(state.invulnerable_time, 20);

        // Controlled negative case: equal or lower damage during invulnerability window is rejected.
        let blocked = state.hurt_server(sources.mob_attack(zombie.clone()), 4.0, &ctx);
        assert_eq!(
            blocked,
            HurtOutcome::Rejected(HurtRejection::InvulnerabilityCooldown)
        );
        assert_close(state.health, 16.0);

        // Strictly greater damage during invulnerability window deals only the delta (7.0 - 4.0 = 3.0).
        let partial = state.hurt_server(sources.mob_attack(zombie.clone()), 7.0, &ctx);
        match partial {
            HurtOutcome::Damaged {
                modifiers,
                took_full_damage,
                ..
            } => {
                assert_close(modifiers.invulnerability_reduction, -4.0);
                assert_close(modifiers.final_damage, 3.0);
                assert!(!took_full_damage);
            }
            other => panic!("expected Damaged, got {other:?}"),
        }
        assert_close(state.health, 13.0);
        assert_close(state.last_hurt, 7.0);

        // BypassesCooldown deals full damage even inside invulnerability window.
        let bypass_src = sources
            .mob_attack(zombie)
            .with_extra_tag(DamageTypeTag::BypassesCooldown);
        let bypass = state.hurt_server(bypass_src, 2.0, &ctx);
        match bypass {
            HurtOutcome::Damaged {
                modifiers,
                took_full_damage,
                ..
            } => {
                assert_close(modifiers.invulnerability_reduction, 0.0);
                assert_close(modifiers.final_damage, 2.0);
                assert!(took_full_damage);
            }
            other => panic!("expected Damaged, got {other:?}"),
        }
        assert_close(state.health, 11.0);
    }

    #[test]
    fn modifier_pipeline_ordering_absorption_death_and_drops() {
        let sources = DamageSources::new();
        let skeleton =
            DamageEntityRef::living(30, "Skeleton").with_main_hand_custom_name("Doom Bow");
        let player = DamageEntityRef::player(40, "Alex", false);

        let mut state = DamagePipelineState::new(10.0);
        state.set_absorption_amount(2.0);

        // Player hits mob first so player kill credit is recorded.
        let hit1 = state.hurt_server(
            sources.player_attack(player.clone()),
            4.0,
            &HurtContext {
                victim_name: "Zombie".into(),
                tick_count: 10,
                armor: 10.0,
                resistance_amplifier: Some(0), // -20%
                magic_armor: 5.0,              // -20%
                ..Default::default()
            },
        );
        match hit1 {
            HurtOutcome::Damaged {
                modifiers, death, ..
            } => {
                // armor(4, 10, 0): realArmor = clamp(10 - 4/2, 2, 20) = 8 -> 4 * (1 - 8/25) = 2.72
                // resistance I: 2.72 * 0.8 = 2.176
                // magic(5): 2.176 * 0.8 = 1.7408
                // absorption(2.0): absorbs all 1.7408 -> final_damage = 0.0
                assert_close(modifiers.final_damage, 0.0);
                assert_close(modifiers.absorption, -1.7408);
                assert!(death.is_none());
            }
            other => panic!("expected Damaged, got {other:?}"),
        }
        assert_close(state.health, 10.0);
        assert_close(state.absorption_amount, 2.0 - 1.7408);

        // Expire invulnerability cooldown (10 ticks -> invulnerable_time == 10, not > 10.0).
        for t in 11..=20 {
            state.tick_timers(t);
        }
        assert_eq!(state.invulnerable_time, 10);

        // Fatal blow from skeleton; kill credit still prefers player within 100-tick memory.
        let fatal = state.hurt_server(
            sources.mob_attack(skeleton),
            20.0,
            &HurtContext {
                victim_name: "Zombie".into(),
                tick_count: 20,
                is_baby: false,
                mob_drops_gamerule: true,
                ..Default::default()
            },
        );
        match fatal {
            HurtOutcome::Damaged { death: Some(d), .. } => {
                assert_eq!(d.kill_credit, Some(player));
                assert!(d.player_killed);
                assert!(d.should_drop_loot);
                assert!(d.should_drop_experience);
                assert_eq!(d.death_message.translation_key, "death.attack.mob.item");
                assert_eq!(d.death_message.args, vec!["Zombie", "Skeleton", "Doom Bow"]);
            }
            other => panic!("expected fatal Damaged, got {other:?}"),
        }

        // Subsequent damage after death is rejected.
        let after_death = state.hurt_server(sources.generic(), 5.0, &HurtContext::default());
        assert_eq!(
            after_death,
            HurtOutcome::Rejected(HurtRejection::DeadOrRemoved)
        );

        // tick_death removes on the 20th tick.
        for expected in 1..20 {
            assert_eq!(
                state.tick_death(),
                DeathTickOutcome::Dying {
                    death_time: expected
                }
            );
            assert!(!state.removed);
        }
        assert_eq!(state.tick_death(), DeathTickOutcome::RemoveKilled);
        assert!(state.removed);
    }

    #[test]
    fn combat_tracker_fall_variants_and_timeout_edges() {
        let sources = DamageSources::new();
        let mut tracker = CombatTracker::new();
        let zombie = DamageEntityRef::living(5, "Zombie");

        // Mob knocks victim off a ladder, then victim dies to a 10-block fall.
        assert!(tracker.record_damage(
            sources.mob_attack(zombie),
            2.0,
            Some(FallLocation::Ladder),
            0.0,
            100,
            true
        ));
        assert!(tracker.is_in_combat());

        assert!(!tracker.record_damage(sources.fall(), 12.0, None, 10.0, 120, true));
        let msg = tracker.death_message("Steve", None);
        assert_eq!(msg.translation_key, "death.fell.assist");
        assert_eq!(msg.args, vec!["Steve", "Zombie"]);

        // Still in combat at exact 300-tick boundary (condition is `> 300`, not `>= 300`).
        assert!(!tracker.recheck_status(420, true));
        assert!(tracker.is_in_combat());

        // Leaves combat at 301 ticks after last damage.
        assert!(tracker.recheck_status(421, true));
        assert!(!tracker.is_in_combat());
        assert!(tracker.entries().is_empty());
    }

    #[test]
    fn controlled_negative_cases_fire_resistance_peaceful_and_baby_drops() {
        let sources = DamageSources::new();
        let mut state = DamagePipelineState::new(20.0);

        // Fire resistance rejects fire damage.
        let fire = state.hurt_server(
            sources.lava(),
            4.0,
            &HurtContext {
                has_fire_resistance: true,
                ..Default::default()
            },
        );
        assert_eq!(fire, HurtOutcome::Rejected(HurtRejection::FireResistance));

        // Peaceful rejects mob attack on player.
        let zombie = DamageEntityRef::living(1, "Zombie");
        let peaceful = state.hurt_server(
            sources.mob_attack(zombie),
            4.0,
            &HurtContext {
                is_player_victim: true,
                difficulty: Difficulty::Peaceful,
                ..Default::default()
            },
        );
        assert_eq!(
            peaceful,
            HurtOutcome::Rejected(HurtRejection::PeacefulDifficulty)
        );

        // Baby mob drops neither loot nor experience even on player kill.
        assert!(!DamagePipelineState::should_drop_loot(true, true));
        assert!(!state.should_drop_experience(true, true, true));

        // Intentional game design death message produces `.message` + `.link`.
        let mut igd_tracker = CombatTracker::new();
        igd_tracker.record_damage(
            sources.bad_respawn_point_explosion([0.0, 64.0, 0.0]),
            25.0,
            None,
            0.0,
            10,
            true,
        );
        let igd_msg = igd_tracker.death_message("Steve", None);
        assert_eq!(
            igd_msg.translation_key,
            "death.attack.badRespawnPoint.message"
        );
        assert_eq!(
            igd_msg.intentional_game_design_link_key.as_deref(),
            Some("death.attack.badRespawnPoint.link")
        );
    }

    #[test]
    fn java_min_max_nan_and_signed_zero_semantics() {
        let sources = DamageSources::new();
        let zombie = DamageEntityRef::living(1, "Zombie");
        let scaling = sources.mob_attack(zombie);

        assert!(
            scale_damage_for_difficulty(f32::NAN, Difficulty::Easy, &scaling)
                .unwrap()
                .is_nan()
        );
        assert!(apply_resistance(f32::NAN, 0).is_nan());
        assert!(java_min_f32(f32::NAN, 1.0).is_nan());
        assert!(java_max_f32(f32::NAN, 0.0).is_nan());

        assert!(java_min_f32(0.0, -0.0).is_sign_negative());
        assert!(java_min_f32(-0.0, 0.0).is_sign_negative());
        assert!(!java_max_f32(0.0, -0.0).is_sign_negative());
        assert!(!java_max_f32(-0.0, 0.0).is_sign_negative());
        assert!(java_max_f32(-0.0, -0.0).is_sign_negative());
    }
}
