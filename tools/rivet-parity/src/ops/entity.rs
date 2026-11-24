//! The pure entity ops (VERIFICATION §5 item 9): `damage.calc`,
//! `damage_source.scaling`, `food.exhaustion`, `xp.curves`.
//!
//! The Rust twins of `tools/rivet-reference-oracle/src/EntityOps.java`. Each
//! handler decodes the request, calls the ported code in
//! `rivet-server/src/world/{damagesource,food,entity/player/survival}` and
//! renders the result in the op's declared key order. They never re-derive an
//! answer: the arithmetic is the port's.
//!
//! Floats travel as `0x` + 8 hex digits of `Float.floatToIntBits`
//! ([`rivet_harness_common::canon`]) in requests and results.
//!
//! # Live-entity harness contract (the Rust half of `EntityOps.java`'s)
//!
//! Where Paper needs a live entity, Java runs the real method on an entity
//! allocated without its constructor and answering only what the method reads;
//! [`HarnessHost`] reproduces the same answers: health is a plain field,
//! `heal` clamps `health + amount` into `[0, max]`, the starve damage
//! subtracts and floors at 0, every event is delivered unmodified, the level
//! answers the request's difficulty and `natural_health_regeneration`. The
//! full contract is the class comment of `EntityOps.java`.
//!
//! # Surfaces that are not ops (unavailable, with the reason)
//!
//! - the Breach path of `CombatRules.getDamageAfterAbsorb` (a weapon item):
//!   Paper runs `EnchantmentHelper.modifyArmorEffectiveness` through a loot
//!   context built from the live victim and `ServerLevel`;
//! - `LivingEntity.getDamageAfterArmorAbsorb` and the damage-type-tag branches
//!   around it (`BYPASSES_ARMOR`, `BYPASSES_ENCHANTMENTS`): `LivingEntity`
//!   members whose inputs are the victim's armor attributes and inventory;
//! - the resistance and absorption stages of `LivingEntity.handleEntityDamage`
//!   (`apply_resistance`): they read the victim's active effects;
//! - negative or non-finite damage through `Player.hurtServer`: `LivingEntity.hurtServer`
//!   clamps it after the scaling, while Rivet's composed `DamagePipelineState::hurt_server`
//!   clamps before scaling and rejects zero damage (#824). The op calls the leaf
//!   `scale_damage_for_difficulty`; the composed path belongs to the arena half.
//!
//! The damage-type-tag membership that is reachable without an entity is
//! `damage.calc` `fn:"type"`.

use rivet_harness_common::canon::f32_bits;
use rivet_server::world::damagesource::{
    DamageEntityRef, DamageSource, DamageTypeKey, DamageTypeTag, Difficulty as CombatDifficulty,
    get_damage_after_absorb, get_damage_after_magic_absorb, mth_clamp_f32,
    scale_damage_for_difficulty,
};
use rivet_server::world::entity::player::survival::{
    Difficulty, ExhaustionConfig, ExperienceState, RegainReason, SurvivalEvent, SurvivalGameRules,
    SurvivalPlayerHost,
};
use rivet_server::world::food::food_data::{FoodData, FoodSaveData};
use rivet_server::world::food::food_properties::FoodProperties;
use serde_json::{Map, Value, json};

use super::compare::Outcome;

// ---- request decoding -------------------------------------------------------

fn unavailable(message: impl Into<String>) -> Outcome {
    Outcome::Unavailable(message.into())
}

fn string<'a>(request: &'a Map<String, Value>, key: &str) -> Result<&'a str, Outcome> {
    request
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| unavailable(format!("request property '{key}' must be a string")))
}

fn integer(request: &Map<String, Value>, key: &str) -> Result<i32, Outcome> {
    value_i32(request.get(key), key)
}

fn value_i32(value: Option<&Value>, what: &str) -> Result<i32, Outcome> {
    value
        .and_then(Value::as_i64)
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| unavailable(format!("'{what}' must be a 32-bit integer")))
}

fn boolean(request: &Map<String, Value>, key: &str) -> Result<bool, Outcome> {
    request
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| unavailable(format!("request property '{key}' must be a boolean")))
}

fn parse_f32(text: &str) -> Result<f32, Outcome> {
    let digits = text
        .strip_prefix("0x")
        .filter(|d| d.len() == 8)
        .ok_or_else(|| unavailable(format!("float must be 0x + 8 hex digits, got {text}")))?;
    u32::from_str_radix(digits, 16)
        .map(f32::from_bits)
        .map_err(|e| unavailable(format!("float {text}: {e}")))
}

fn float(request: &Map<String, Value>, key: &str) -> Result<f32, Outcome> {
    parse_f32(string(request, key)?)
}

fn value_f32(value: Option<&Value>, what: &str) -> Result<f32, Outcome> {
    parse_f32(
        value
            .and_then(Value::as_str)
            .ok_or_else(|| unavailable(format!("'{what}' must be a float string")))?,
    )
}

fn steps(request: &Map<String, Value>) -> Result<&Vec<Value>, Outcome> {
    request
        .get("steps")
        .and_then(Value::as_array)
        .ok_or_else(|| unavailable("request property 'steps' must be an array"))
}

fn difficulty(name: &str) -> Result<(Difficulty, CombatDifficulty), Outcome> {
    Ok(match name {
        "peaceful" => (Difficulty::Peaceful, CombatDifficulty::Peaceful),
        "easy" => (Difficulty::Easy, CombatDifficulty::Easy),
        "normal" => (Difficulty::Normal, CombatDifficulty::Normal),
        "hard" => (Difficulty::Hard, CombatDifficulty::Hard),
        other => return Err(unavailable(format!("unknown difficulty {other}"))),
    })
}

fn damage_type_key(id: &str) -> Result<DamageTypeKey, Outcome> {
    DamageTypeKey::ALL
        .into_iter()
        .find(|key| key.id() == id)
        .ok_or_else(|| unavailable(format!("damage type {id} is not ported")))
}

/// The `Result`-to-`Outcome` seam every handler uses.
fn outcome(result: Result<Map<String, Value>, Outcome>) -> Outcome {
    result.map_or_else(|o| o, Outcome::Accepted)
}

fn bits(map: &mut Map<String, Value>, key: &str, value: f32) {
    map.insert(key.into(), json!(f32_bits(value)));
}

// ---- damage.calc ------------------------------------------------------------

/// Every `DamageTypeTags` constant of Paper 26.2 (the Rust enum has no `ALL`).
const ALL_TAGS: [DamageTypeTag; 35] = [
    DamageTypeTag::DamagesHelmet,
    DamageTypeTag::BypassesArmor,
    DamageTypeTag::BypassesShield,
    DamageTypeTag::BypassesInvulnerability,
    DamageTypeTag::BypassesCooldown,
    DamageTypeTag::BypassesEffects,
    DamageTypeTag::BypassesResistance,
    DamageTypeTag::BypassesEnchantments,
    DamageTypeTag::IsFire,
    DamageTypeTag::IsProjectile,
    DamageTypeTag::WitchResistantTo,
    DamageTypeTag::IsExplosion,
    DamageTypeTag::IsFall,
    DamageTypeTag::IsDrowning,
    DamageTypeTag::IsFreezing,
    DamageTypeTag::IsLightning,
    DamageTypeTag::NoAnger,
    DamageTypeTag::NoImpact,
    DamageTypeTag::AlwaysMostSignificantFall,
    DamageTypeTag::WitherImmuneTo,
    DamageTypeTag::SulfurCubeWithBlockImmuneTo,
    DamageTypeTag::IgnitesArmorStands,
    DamageTypeTag::BurnsArmorStands,
    DamageTypeTag::AvoidsGuardianThorns,
    DamageTypeTag::AlwaysTriggersSilverfish,
    DamageTypeTag::AlwaysHurtsEnderDragons,
    DamageTypeTag::NoKnockback,
    DamageTypeTag::AlwaysKillsArmorStands,
    DamageTypeTag::CanBreakArmorStand,
    DamageTypeTag::BypassesWolfArmor,
    DamageTypeTag::IsPlayerAttack,
    DamageTypeTag::BurnFromStepping,
    DamageTypeTag::PanicCauses,
    DamageTypeTag::PanicEnvironmentalCauses,
    DamageTypeTag::IsMaceSmash,
];

/// `damage.calc`: `CombatRules` and the vanilla damage-type definitions.
pub fn damage_calc(request: &Map<String, Value>) -> Outcome {
    outcome(damage_calc_inner(request))
}

fn damage_calc_inner(request: &Map<String, Value>) -> Result<Map<String, Value>, Outcome> {
    let mut result = Map::new();
    match string(request, "fn")? {
        "after_absorb" => bits(
            &mut result,
            "damage",
            get_damage_after_absorb(
                float(request, "damage")?,
                float(request, "armor")?,
                float(request, "toughness")?,
            ),
        ),
        "after_magic_absorb" => bits(
            &mut result,
            "damage",
            get_damage_after_magic_absorb(float(request, "damage")?, float(request, "protection")?),
        ),
        "type" => {
            let key = damage_type_key(string(request, "damage_type")?)?;
            let definition = key.vanilla_type();
            result.insert("msg_id".into(), json!(definition.msg_id));
            result.insert(
                "scaling".into(),
                json!(definition.scaling.serialized_name()),
            );
            bits(&mut result, "exhaustion", definition.exhaustion);
            result.insert(
                "effects".into(),
                json!(definition.effects.serialized_name()),
            );
            result.insert(
                "death_message".into(),
                json!(definition.death_message_type.serialized_name()),
            );
            let mut tags: Vec<&str> = ALL_TAGS
                .into_iter()
                .filter(|tag| key.is_in_tag(*tag))
                .map(DamageTypeTag::id)
                .collect();
            tags.sort_unstable();
            result.insert("tags".into(), json!(tags));
        }
        other => return Err(unavailable(format!("unknown damage.calc fn {other}"))),
    }
    Ok(result)
}

// ---- damage_source.scaling --------------------------------------------------

fn entity_ref(kind: &str, id: i32) -> Result<Option<DamageEntityRef>, Outcome> {
    Ok(match kind {
        "none" => None,
        "living" => Some(DamageEntityRef::living(id, "living")),
        "player" => Some(DamageEntityRef::player(id, "player", false)),
        "non_living" => Some(DamageEntityRef::non_living(id, "non_living")),
        other => return Err(unavailable(format!("unknown entity kind {other}"))),
    })
}

/// `damage_source.scaling`: `DamageSource.scalesWithDifficulty` and the
/// difficulty scaling of `Player.hurtServer`.
pub fn damage_source_scaling(request: &Map<String, Value>) -> Outcome {
    outcome(damage_source_scaling_inner(request))
}

fn damage_source_scaling_inner(
    request: &Map<String, Value>,
) -> Result<Map<String, Value>, Outcome> {
    let key = damage_type_key(string(request, "damage_type")?)?;
    let (_, combat) = difficulty(string(request, "difficulty")?)?;
    let damage = float(request, "damage")?;
    let source = DamageSource::with_direct_and_causing(
        key,
        entity_ref(string(request, "direct")?, 1)?,
        entity_ref(string(request, "causing")?, 2)?,
    );
    let mut result = Map::new();
    result.insert("scales".into(), json!(source.scales_with_difficulty()));
    match scale_damage_for_difficulty(damage, combat, &source) {
        Some(scaled) => {
            result.insert("hurt".into(), json!(true));
            bits(&mut result, "damage", scaled);
        }
        None => {
            result.insert("hurt".into(), json!(false));
        }
    }
    Ok(result)
}

// ---- food.exhaustion --------------------------------------------------------

/// The harness player of the module comment: health only, no inventory.
struct HarnessHost {
    health: f32,
    max_health: f32,
    invulnerable: bool,
}

impl SurvivalPlayerHost for HarnessHost {
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
        false
    }

    fn is_spectator(&self) -> bool {
        false
    }

    fn tick_count(&self) -> i32 {
        0
    }

    fn heal(&mut self, amount: f32, _reason: RegainReason, _is_fast_regen: bool) {
        self.health = mth_clamp_f32(self.health + amount, 0.0, self.max_health);
    }

    fn hurt_starve(&mut self, amount: f32) -> bool {
        self.health = rivet_server::world::damagesource::java_max_f32(self.health - amount, 0.0);
        true
    }

    fn increase_score(&mut self, _delta: i32) {}
}

fn food_state(food: &FoodData, host: &HarnessHost) -> Value {
    let mut state = Map::new();
    state.insert("food".into(), json!(food.food_level()));
    bits(&mut state, "saturation", food.saturation_level());
    bits(&mut state, "exhaustion", food.exhaustion_level);
    state.insert("tick_timer".into(), json!(food.tick_timer()));
    bits(&mut state, "health", host.health);
    state.insert("needs_food".into(), json!(food.needs_food()));
    state.insert("has_enough_food".into(), json!(food.has_enough_food()));
    Value::Object(state)
}

/// `food.exhaustion`: scripted `FoodData` sequences.
pub fn food_exhaustion(request: &Map<String, Value>) -> Outcome {
    outcome(food_exhaustion_inner(request))
}

fn food_exhaustion_inner(request: &Map<String, Value>) -> Result<Map<String, Value>, Outcome> {
    let (difficulty, _) = difficulty(string(request, "difficulty")?)?;
    let rules = SurvivalGameRules {
        natural_health_regeneration: boolean(request, "natural_regen")?,
        ..SurvivalGameRules::default()
    };
    let config = ExhaustionConfig {
        regen_exhaustion: float(request, "regen_exhaustion")?,
        ..ExhaustionConfig::default()
    };
    let mut host = HarnessHost {
        health: float(request, "health")?,
        max_health: float(request, "max_health")?,
        invulnerable: boolean(request, "invulnerable")?,
    };
    let mut food = FoodData::new();
    food.read_save_data(FoodSaveData {
        food_level: Some(integer(request, "food")?),
        food_tick_timer: Some(integer(request, "tick_timer")?),
        food_saturation_level: Some(float(request, "saturation")?),
        food_exhaustion_level: Some(float(request, "exhaustion")?),
    });

    let mut events: Vec<SurvivalEvent> = Vec::new();
    let mut states = Vec::new();
    for step in steps(request)? {
        let step = step
            .as_array()
            .ok_or_else(|| unavailable("a food step must be an array"))?;
        let name = step
            .first()
            .and_then(Value::as_str)
            .ok_or_else(|| unavailable("a food step must start with its name"))?;
        match name {
            "tick" => food.tick(&mut host, difficulty, rules, &config, &mut events),
            "eat" => food.eat(
                value_i32(step.get(1), "nutrition")?,
                value_f32(step.get(2), "saturation modifier")?,
            ),
            "eat_props" => food.eat_properties(FoodProperties::new(
                value_i32(step.get(1), "nutrition")?,
                value_f32(step.get(2), "saturation")?,
                false,
            )),
            "eat_event" => food.eat_with_events(
                &mut host,
                FoodProperties::new(
                    value_i32(step.get(1), "nutrition")?,
                    value_f32(step.get(2), "saturation")?,
                    false,
                ),
                &mut events,
            ),
            "exhaust" => food.add_exhaustion(value_f32(step.get(1), "exhaustion")?),
            "set_food" => food.set_food_level(value_i32(step.get(1), "food")?),
            "set_saturation" => food.set_saturation(value_f32(step.get(1), "saturation")?),
            other => return Err(unavailable(format!("unknown food step {other}"))),
        }
        events.clear();
        states.push(food_state(&food, &host));
    }
    let mut result = Map::new();
    result.insert("states".into(), Value::Array(states));
    Ok(result)
}

// ---- xp.curves --------------------------------------------------------------

/// `xp.curves`: `Player.getXpNeededForNextLevel` and the level/progress
/// arithmetic of the experience methods.
pub fn xp_curves(request: &Map<String, Value>) -> Outcome {
    outcome(xp_curves_inner(request))
}

fn xp_curves_inner(request: &Map<String, Value>) -> Result<Map<String, Value>, Outcome> {
    let mut xp = ExperienceState::new();
    xp.experience_level = integer(request, "level")?;
    xp.experience_progress = float(request, "progress")?;
    xp.total_experience = integer(request, "total")?;
    xp.last_level_up_time = integer(request, "last_level_up_time")?;
    let tick_count = integer(request, "tick_count")?;

    let mut result = Map::new();
    result.insert("needed".into(), json!(xp.xp_needed_for_next_level()));
    let mut states = Vec::new();
    let mut events: Vec<SurvivalEvent> = Vec::new();
    for step in steps(request)? {
        let step = step
            .as_array()
            .ok_or_else(|| unavailable("an xp step must be an array"))?;
        let name = step
            .first()
            .and_then(Value::as_str)
            .ok_or_else(|| unavailable("an xp step must start with its name"))?;
        let amount = value_i32(step.get(1), "amount")?;
        match name {
            "points" => {
                xp.give_experience_points(amount, tick_count, &mut events);
            }
            "levels" => xp.give_experience_levels(amount, tick_count, &mut events),
            "set_points" => xp.set_experience_points(amount),
            "set_levels" => xp.set_experience_levels(amount),
            "enchant" => xp.on_enchantment_performed(amount),
            other => return Err(unavailable(format!("unknown xp step {other}"))),
        }
        let sounds: Vec<Value> = events
            .drain(..)
            .filter_map(|event| match event {
                SurvivalEvent::LevelUpSound { volume, pitch } => {
                    Some(json!([f32_bits(volume), f32_bits(pitch)]))
                }
                _ => None,
            })
            .collect();
        let mut state = Map::new();
        state.insert("level".into(), json!(xp.experience_level));
        bits(&mut state, "progress", xp.experience_progress);
        state.insert("total".into(), json!(xp.total_experience));
        state.insert("needed".into(), json!(xp.xp_needed_for_next_level()));
        state.insert("sounds".into(), Value::Array(sounds));
        states.push(Value::Object(state));
    }
    result.insert("states".into(), Value::Array(states));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use rivet_harness_common::closure::Status;

    use super::super::{OpFixture, OpRow, check, compare, load_fixtures, op_source, ops_dir};
    use super::*;

    const ENTITY_OPS: [&str; 4] = [
        "damage.calc",
        "damage_source.scaling",
        "food.exhaustion",
        "xp.curves",
    ];

    fn entity_fixtures() -> Vec<OpFixture> {
        let fixtures = load_fixtures(&ops_dir(), &op_source())
            .expect("committed recorded ops load and close")
            .into_iter()
            .filter(|f| ENTITY_OPS.contains(&f.op.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            fixtures.len(),
            ENTITY_OPS.len(),
            "every entity op is recorded"
        );
        fixtures
    }

    fn exclusions() -> Vec<compare::Exclusion> {
        compare::load_exclusions(&ops_dir().join("exclusions.tsv")).unwrap()
    }

    fn row<'a>(rows: &'a [OpRow], op: &str) -> &'a OpRow {
        rows.iter().find(|r| r.op == op).unwrap()
    }

    /// The offline tier: Paper's recorded answers vs the ported code, closure
    /// held, the tamper negative detected and named.
    #[test]
    fn recorded_entity_ops_match_the_ported_code() {
        let rows = check(&entity_fixtures(), &exclusions(), None);
        for op in ENTITY_OPS {
            let row = row(&rows, op);
            assert_eq!(
                row.status,
                Status::Pass,
                "{op}: {} first={:?} note={:?}",
                row.closure,
                row.first.as_ref().map(ToString::to_string),
                row.note
            );
            assert!(row.closure.executed > 0, "{op}");
            assert_eq!(
                row.closure.executed + row.closure.excluded,
                row.closure.declared,
                "{op}"
            );
            assert!(
                row.negative.as_ref().is_some_and(|v| v.passed()),
                "{op}: tamper negative {:?}",
                row.negative
            );
        }
    }

    /// A deliberately broken Rust handler is detected and named, per op.
    #[test]
    fn broken_entity_handlers_are_detected_and_named() {
        let fixtures = entity_fixtures();
        for op in ENTITY_OPS {
            let rows = check(&fixtures, &exclusions(), Some(op));
            let broken = row(&rows, op);
            assert_eq!(broken.status, Status::Fail, "{op}");
            let first = broken.first.as_ref().expect("a named divergence");
            assert_eq!(first.op, op);
            assert!(!first.fields.is_empty());
        }
    }

    /// The only open divergence of these ops is the negative-armor one (#816):
    /// without its exclusion rows exactly those inputs, and nothing else,
    /// mismatch; and every excluded input really has a negative armor value.
    #[test]
    fn the_exclusions_are_exactly_the_negative_armor_divergence() {
        let all = exclusions();
        let without: Vec<_> = all
            .iter()
            .filter(|e| !ENTITY_OPS.contains(&e.op.as_str()))
            .cloned()
            .collect();
        let rows = check(&entity_fixtures(), &without, None);
        for op in ENTITY_OPS.into_iter().filter(|op| *op != "damage.calc") {
            assert_eq!(row(&rows, op).status, Status::Pass, "{op}");
        }
        let damage = row(&rows, "damage.calc");
        assert_eq!(damage.status, Status::Fail);
        let mut mismatched = damage.mismatched_ids.clone();
        mismatched.sort();
        let mut excluded: Vec<String> = all
            .iter()
            .filter(|e| e.op == "damage.calc")
            .map(|e| e.field.strip_prefix("input:").unwrap().to_owned())
            .collect();
        excluded.sort();
        assert_eq!(mismatched, excluded);
        assert!(
            all.iter()
                .filter(|e| e.op == "damage.calc")
                .all(|e| e.issue == "816")
        );

        let fixtures = entity_fixtures();
        let corpus = &fixtures
            .iter()
            .find(|f| f.op == "damage.calc")
            .unwrap()
            .corpus;
        for id in &excluded {
            let entry = corpus.iter().find(|e| &e.id == id).unwrap();
            assert_eq!(entry.request["fn"], "after_absorb", "{id}");
            let armor = parse_f32(entry.request["armor"].as_str().unwrap())
                .ok()
                .unwrap();
            assert!(armor < 0.0, "{id}: armor {armor}");
        }
    }

    /// A tampered recorded answer is detected and named by the comparator.
    #[test]
    fn a_tampered_recorded_entity_answer_is_named() {
        let exclusions = exclusions();
        for fixture in entity_fixtures() {
            let side = super::super::rust_side(&fixture.op, None).unwrap();
            let actual = super::super::run_rust(&fixture.op, side, &fixture.corpus, &exclusions);
            let verdict =
                compare::tamper_negative(&fixture.op, &fixture.recorded, &actual, &exclusions)
                    .expect("an input matched");
            assert!(verdict.passed(), "{}: {verdict}", fixture.op);
        }
    }

    /// The Rust half of the harness contract in `EntityOps.java`.
    #[test]
    fn harness_host_follows_the_documented_contract() {
        let mut host = HarnessHost {
            health: 19.5,
            max_health: 20.0,
            invulnerable: false,
        };
        host.heal(1.0, RegainReason::Satiated, false);
        assert_eq!(host.health, 20.0, "heal clamps at the maximum");
        assert!(!host.is_hurt());
        assert!(host.hurt_starve(30.0));
        assert_eq!(host.health, 0.0, "starve damage floors at zero");
        assert!(!host.is_hurt(), "dead is not hurt");
    }

    #[test]
    fn malformed_requests_are_unavailable_not_verdicts() {
        for handler in [
            damage_calc,
            damage_source_scaling,
            food_exhaustion,
            xp_curves,
        ] {
            assert!(matches!(handler(&Map::new()), Outcome::Unavailable(_)));
        }
        let mut request = Map::new();
        request.insert("fn".into(), json!("nope"));
        assert!(matches!(damage_calc(&request), Outcome::Unavailable(_)));
    }
}
