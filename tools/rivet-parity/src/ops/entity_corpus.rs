//! The corpora of the pure entity ops (`damage.calc`, `damage_source.scaling`,
//! `food.exhaustion`, `xp.curves`), committed as `corpus/<op>.jsonl`.
//!
//! Nothing here hand-types an expected value: every input is a request, Paper
//! answers it. The generators are pure functions of [`super::packet_corpus`]'s
//! committed seed (one named stream per op), so a test pins the committed
//! files to them.

use rivet_harness_common::canon::f32_bits;
use rivet_server::world::damagesource::DamageTypeKey;
use serde_json::{Value, json};

use super::packet_corpus::Rng;

fn line(id: &str, mut request: Value) -> String {
    let object = request.as_object_mut().expect("requests are objects");
    let mut with_id = serde_json::Map::new();
    with_id.insert("id".into(), json!(id));
    with_id.append(object);
    serde_json::to_string(&Value::Object(with_id)).expect("JSON values always serialize") + "\n"
}

fn fb(value: f32) -> String {
    f32_bits(value)
}

/// A float in `[0, 1)` with 24 random bits.
fn unit(rng: &mut Rng) -> f32 {
    (rng.next_u64() >> 40) as f32 / (1u32 << 24) as f32
}

/// A float in `[low, high)`.
fn between(rng: &mut Rng, low: f32, high: f32) -> f32 {
    low + unit(rng) * (high - low)
}

fn pick<T: Copy>(rng: &mut Rng, items: &[T]) -> T {
    items[rng.below(items.len() as u64) as usize]
}

// ---- damage.calc ------------------------------------------------------------

/// Damage values: the common range, tiny, huge, signed zero, negative, infinite, NaN.
const DAMAGES: [f32; 22] = [
    0.0,
    -0.0,
    0.5,
    1.0,
    2.0,
    3.0,
    4.0,
    5.0,
    7.0,
    10.0,
    15.0,
    20.0,
    35.0,
    100.0,
    4096.0,
    1.0e9,
    f32::MAX,
    f32::INFINITY,
    -1.0,
    -10.0,
    f32::NAN,
    1.0e-45,
];

/// Armor points: every integer an armor set reaches (0..=20 and a little over),
/// then out-of-range, negative and non-finite.
const ARMORS: [f32; 25] = [
    0.0,
    1.0,
    2.0,
    3.0,
    4.0,
    5.0,
    6.0,
    7.0,
    8.0,
    9.0,
    10.0,
    12.0,
    14.0,
    16.0,
    18.0,
    19.0,
    20.0,
    21.0,
    24.0,
    30.0,
    100.0,
    -3.0,
    f32::NAN,
    f32::INFINITY,
    -0.0,
];

/// Armor toughness: vanilla values (0..=12 in steps of the diamond/netherite
/// materials), then the values that make the divisor `2 + t/4` zero or negative.
const TOUGHNESS: [f32; 14] = [
    0.0,
    1.0,
    2.0,
    3.0,
    4.0,
    6.0,
    8.0,
    12.0,
    16.0,
    20.0,
    -8.0,
    -100.0,
    f32::NAN,
    f32::INFINITY,
];

/// Protection (the summed `DAMAGE_PROTECTION` enchantment value): vanilla 0..=20+.
const PROTECTIONS: [f32; 31] = [
    0.0,
    1.0,
    2.0,
    3.0,
    4.0,
    5.0,
    6.0,
    7.0,
    8.0,
    9.0,
    10.0,
    11.0,
    12.0,
    13.0,
    14.0,
    15.0,
    16.0,
    17.0,
    18.0,
    19.0,
    20.0,
    21.0,
    25.0,
    40.0,
    1.0e9,
    -1.0,
    -20.0,
    f32::NAN,
    f32::INFINITY,
    f32::NEG_INFINITY,
    -0.0,
];

fn damage_calc() -> String {
    let mut out = String::new();
    let mut n = 0usize;
    for damage in DAMAGES {
        for armor in ARMORS {
            for toughness in TOUGHNESS {
                out += &line(
                    &format!("absorb.{n}"),
                    json!({"fn": "after_absorb", "damage": fb(damage), "armor": fb(armor), "toughness": fb(toughness)}),
                );
                n += 1;
            }
        }
    }
    // Fractional values between the grid points, where the clamp edges bend.
    let mut rng = Rng::for_stream("damage.calc.absorb");
    for _ in 0..1500 {
        let damage = between(&mut rng, 0.0, 60.0);
        let armor = between(&mut rng, 0.0, 24.0);
        let toughness = between(&mut rng, 0.0, 24.0);
        out += &line(
            &format!("absorb.{n}"),
            json!({"fn": "after_absorb", "damage": fb(damage), "armor": fb(armor), "toughness": fb(toughness)}),
        );
        n += 1;
    }
    let mut n = 0usize;
    for damage in DAMAGES {
        for protection in PROTECTIONS {
            out += &line(
                &format!("magic.{n}"),
                json!({"fn": "after_magic_absorb", "damage": fb(damage), "protection": fb(protection)}),
            );
            n += 1;
        }
    }
    for _ in 0..300 {
        let damage = between(&mut rng, 0.0, 60.0);
        let protection = between(&mut rng, 0.0, 24.0);
        out += &line(
            &format!("magic.{n}"),
            json!({"fn": "after_magic_absorb", "damage": fb(damage), "protection": fb(protection)}),
        );
        n += 1;
    }
    for key in DamageTypeKey::ALL {
        out += &line(
            &format!("type.{}", key.id()),
            json!({"fn": "type", "damage_type": key.id()}),
        );
    }
    out
}

// ---- damage_source.scaling --------------------------------------------------

const DIFFICULTIES: [&str; 4] = ["peaceful", "easy", "normal", "hard"];

/// (direct, causing) entity kinds: no entity, a mob hitting directly, a player
/// hitting directly, and projectiles shot by nobody, a non-living entity, a mob or a player.
const ATTACKERS: [(&str, &str); 7] = [
    ("none", "none"),
    ("living", "living"),
    ("player", "player"),
    ("non_living", "none"),
    ("non_living", "non_living"),
    ("non_living", "living"),
    ("non_living", "player"),
];

/// Damage values for the scaling arithmetic (negatives are clamped to zero by
/// `LivingEntity.hurtServer` before the scaled value is observable, so they are
/// not inputs; see the module comment of `entity.rs` and #824).
const SCALED_DAMAGES: [f32; 28] = [
    0.0,
    -0.0,
    0.5,
    1.0,
    1.5,
    2.0,
    2.5,
    3.0,
    4.0,
    5.0,
    6.0,
    7.0,
    8.0,
    9.0,
    10.0,
    13.0,
    20.0,
    33.0,
    100.0,
    1.0e9,
    16_777_216.0,
    f32::MAX,
    f32::MAX / 2.0,
    f32::INFINITY,
    f32::NAN,
    1.0e-45,
    f32::MIN_POSITIVE,
    0.1,
];

fn damage_source_scaling() -> String {
    let mut out = String::new();
    for key in DamageTypeKey::ALL {
        for (direct, causing) in ATTACKERS {
            for difficulty in DIFFICULTIES {
                out += &line(
                    &format!("{}.{direct}.{causing}.{difficulty}", key.id()),
                    json!({
                        "damage_type": key.id(), "direct": direct, "causing": causing,
                        "difficulty": difficulty, "damage": fb(7.5),
                    }),
                );
            }
        }
    }
    // The arithmetic over a damage sweep, on one type per scaling kind.
    for (type_id, direct, causing) in [
        ("minecraft:bad_respawn_point", "none", "none"),
        ("minecraft:mob_attack", "living", "living"),
        ("minecraft:generic", "none", "none"),
    ] {
        for difficulty in DIFFICULTIES {
            for damage in SCALED_DAMAGES {
                out += &line(
                    &format!("sweep.{type_id}.{difficulty}.{}", fb(damage)),
                    json!({
                        "damage_type": type_id, "direct": direct, "causing": causing,
                        "difficulty": difficulty, "damage": fb(damage),
                    }),
                );
            }
        }
    }
    out
}

// ---- food.exhaustion --------------------------------------------------------

/// One scripted start state: (food, saturation, exhaustion, tick_timer, health, max_health).
type FoodProfile = (i32, f32, f32, i32, f32, f32);

/// The states the `FoodData` branches turn on: saturated regen (food 20,
/// saturation > 0), unsaturated regen (food 18..=19), the heal threshold,
/// starvation (food 0) at each health band, and exhaustion at the drop threshold.
const FOOD_PROFILES: [FoodProfile; 18] = [
    (20, 5.0, 0.0, 0, 10.0, 20.0),
    (20, 5.0, 3.5, 8, 19.5, 20.0),
    (20, 0.5, 4.5, 9, 10.0, 20.0),
    (20, 0.0, 4.5, 9, 10.0, 20.0),
    (20, 0.0, 0.0, 79, 10.0, 20.0),
    (19, 0.0, 4.0, 78, 10.0, 20.0),
    (18, 0.0, 4.5, 79, 10.0, 20.0),
    (17, 0.0, 4.5, 79, 10.0, 20.0),
    (17, 2.0, 8.5, 0, 1.0, 20.0),
    (1, 0.0, 4.5, 0, 10.0, 20.0),
    (0, 0.0, 0.0, 78, 20.0, 20.0),
    (0, 0.0, 0.0, 79, 10.0, 20.0),
    (0, 0.0, 0.0, 79, 10.5, 20.0),
    (0, 0.0, 0.0, 79, 1.0, 20.0),
    (0, 0.0, 0.0, 79, 1.5, 20.0),
    (20, 20.0, 40.0, 5, 3.0, 20.0),
    (6, 0.0, 12.0, 0, 20.0, 20.0),
    (20, 3.0, 0.0, 9, 0.0, 20.0),
];

fn food_state_json(
    difficulty: &str,
    natural_regen: bool,
    invulnerable: bool,
    profile: FoodProfile,
    regen_exhaustion: f32,
    steps: Vec<Value>,
) -> Value {
    let (food, saturation, exhaustion, tick_timer, health, max_health) = profile;
    json!({
        "difficulty": difficulty, "natural_regen": natural_regen, "invulnerable": invulnerable,
        "food": food, "saturation": fb(saturation), "exhaustion": fb(exhaustion),
        "tick_timer": tick_timer, "health": fb(health), "max_health": fb(max_health),
        "regen_exhaustion": fb(regen_exhaustion), "steps": steps,
    })
}

fn random_food_step(rng: &mut Rng) -> Value {
    match rng.below(10) {
        0..=4 => json!(["tick"]),
        5 => json!(["exhaust", fb(between(rng, 0.0, 9.0))]),
        6 => json!(["eat", rng.below(21) as i32 - 2, fb(between(rng, 0.0, 1.3))]),
        7 => json!([
            "eat_props",
            rng.below(21) as i32 - 2,
            fb(between(rng, 0.0, 12.0))
        ]),
        8 => json!([
            "eat_event",
            rng.below(21) as i32,
            fb(between(rng, 0.0, 12.0))
        ]),
        _ => match rng.below(3) {
            0 => json!(["set_food", rng.below(24) as i32 - 2]),
            1 => json!(["set_saturation", fb(between(rng, -1.0, 24.0))]),
            _ => json!([
                "exhaust",
                fb(pick(rng, &[0.0, 4.0, 4.5, 39.9, 40.0, 100.0, -1.0]))
            ]),
        },
    }
}

fn food_exhaustion() -> String {
    let mut out = String::new();
    for difficulty in DIFFICULTIES {
        for natural_regen in [true, false] {
            for (index, profile) in FOOD_PROFILES.into_iter().enumerate() {
                let invulnerable = index % 6 == 5;
                let steps = vec![json!(["tick"]); 24];
                out += &line(
                    &format!(
                        "ticks.{difficulty}.{}.{index}",
                        if natural_regen { "regen" } else { "noregen" }
                    ),
                    food_state_json(difficulty, natural_regen, invulnerable, profile, 6.0, steps),
                );
            }
        }
    }
    // Eating, exhausting and setting at the clamp edges (the 32-bit overflow of
    // `food + nutrition` included), one step each, from the default start state.
    let edge_steps = [
        json!(["eat", 3, fb(0.6)]),
        json!(["eat", i32::MAX, fb(1.0)]),
        json!(["eat", i32::MIN, fb(1.0)]),
        json!(["eat", -5, fb(-2.0)]),
        json!(["eat", 4, fb(f32::NAN)]),
        json!(["eat_props", 6, fb(7.2)]),
        json!(["eat_props", i32::MAX, fb(f32::INFINITY)]),
        json!(["eat_event", i32::MAX, fb(1.0)]),
        json!(["eat_event", 5, fb(f32::NAN)]),
        json!(["exhaust", fb(f32::INFINITY)]),
        json!(["exhaust", fb(f32::NEG_INFINITY)]),
        json!(["exhaust", fb(f32::NAN)]),
        json!(["exhaust", fb(40.0)]),
        json!(["exhaust", fb(-0.0)]),
        json!(["set_food", i32::MIN]),
        json!(["set_food", i32::MAX]),
        json!(["set_saturation", fb(f32::NAN)]),
        json!(["set_saturation", fb(-0.0)]),
    ];
    for start in [
        (20, 5.0, 0.0, 0, 10.0, 20.0),
        (0, 0.0, 0.0, 0, 20.0, 20.0),
        (7, 3.0, 3.9, 0, 10.0, 20.0),
    ] {
        for (index, step) in edge_steps.iter().enumerate() {
            out += &line(
                &format!("edge.{}.{index}", start.0),
                food_state_json(
                    "normal",
                    true,
                    false,
                    start,
                    6.0,
                    vec![step.clone(), json!(["tick"])],
                ),
            );
        }
    }
    let mut rng = Rng::for_stream("food.exhaustion");
    for index in 0..320 {
        let difficulty = pick(&mut rng, &DIFFICULTIES);
        let profile = FOOD_PROFILES[rng.below(FOOD_PROFILES.len() as u64) as usize];
        let regen_exhaustion = pick(&mut rng, &[6.0, 0.0, 1.5, 100.0]);
        let steps: Vec<Value> = (0..32).map(|_| random_food_step(&mut rng)).collect();
        out += &line(
            &format!("random.{index}"),
            food_state_json(
                difficulty,
                rng.below(5) != 0,
                rng.below(8) == 0,
                profile,
                regen_exhaustion,
                steps,
            ),
        );
    }
    out
}

// ---- xp.curves --------------------------------------------------------------

fn xp_request(
    level: i32,
    progress: f32,
    total: i32,
    tick_count: i32,
    last_level_up_time: i32,
    steps: Vec<Value>,
) -> Value {
    json!({
        "level": level, "progress": fb(progress), "total": total,
        "tick_count": tick_count, "last_level_up_time": last_level_up_time, "steps": steps,
    })
}

/// Levels around every branch and overflow edge of `getXpNeededForNextLevel`:
/// the 15/30 breakpoints, negatives, the `(level - 30) * 9` wrap at 238_609_325,
/// the `level * 2` wrap below -2^30, and the `int` ends.
const NEEDED_LEVELS: [i32; 35] = [
    i32::MIN,
    i32::MIN + 1,
    -1_073_741_825,
    -1_073_741_824,
    -1_073_741_823,
    -1000,
    -100,
    -2,
    -1,
    0,
    1,
    2,
    13,
    14,
    15,
    16,
    28,
    29,
    30,
    31,
    32,
    100,
    1000,
    65_536,
    1_000_000,
    238_609_293,
    238_609_294,
    238_609_295,
    238_609_324,
    238_609_325,
    238_609_326,
    1_073_741_823,
    1_073_741_824,
    i32::MAX - 1,
    i32::MAX,
];

fn random_xp_step(rng: &mut Rng, max_level: i32) -> Value {
    match rng.below(10) {
        0..=3 => json!(["points", rng.below(3000) as i32 - 600]),
        4..=5 => json!(["levels", rng.below(41) as i32 - 12]),
        6 => json!(["set_points", rng.below(400) as i32 - 20]),
        7 => json!(["set_levels", rng.below(max_level as u64 + 1) as i32]),
        8 => json!(["enchant", rng.below(31) as i32]),
        _ => json!([
            "points",
            pick(rng, &[i32::MAX, i32::MIN, 0, 1, -1, 2_000_000, -2_000_000])
        ]),
    }
}

fn xp_curves() -> String {
    let mut out = String::new();
    // The curve itself: `needed` after a `set_levels` to each level.
    let sweep: Vec<Value> = (-5..=120)
        .map(|level| json!(["set_levels", level]))
        .collect();
    out += &line("curve.dense", xp_request(0, 0.0, 0, 0, 0, sweep));
    let sweep: Vec<Value> = (121..=600)
        .step_by(7)
        .map(|level| json!(["set_levels", level]))
        .collect();
    out += &line("curve.sparse", xp_request(0, 0.0, 0, 0, 0, sweep));
    let sweep: Vec<Value> = NEEDED_LEVELS
        .iter()
        .map(|level| json!(["set_levels", level]))
        .collect();
    out += &line("curve.edges", xp_request(0, 0.0, 0, 0, 0, sweep));
    for level in NEEDED_LEVELS {
        out += &line(
            &format!("needed.{level}"),
            xp_request(
                level,
                0.25,
                1234,
                0,
                0,
                vec![json!(["levels", 1]), json!(["levels", -1])],
            ),
        );
    }

    // Level arithmetic: `giveExperienceLevels` saturates at both int ends and
    // zeroes everything below level 0.
    for level in [
        0,
        1,
        4,
        5,
        29,
        30,
        31,
        100,
        1000,
        i32::MAX - 3,
        i32::MAX,
        i32::MIN + 1,
    ] {
        let mut amounts = vec![
            0,
            1,
            4,
            5,
            -1,
            -5,
            30,
            1000,
            i32::MAX,
            i32::MIN,
            -(level.max(0)),
        ];
        amounts.sort_unstable();
        amounts.dedup();
        for amount in amounts {
            out += &line(
                &format!("levels.{level}.{amount}"),
                xp_request(level, 0.75, 4321, 1000, 0, vec![json!(["levels", amount])]),
            );
        }
    }

    // Points: level-ups through the 15/30 breakpoints, level-downs to zero, the
    // clamp of the running total.
    for level in [0, 1, 14, 15, 16, 29, 30, 31, 100, 1000] {
        for points in [
            1,
            7,
            8,
            9,
            17,
            100,
            5000,
            100_000,
            20_000_000,
            i32::MAX,
            -1,
            -7,
            -100,
            -5000,
            -20_000_000,
            i32::MIN,
        ] {
            for progress in [0.0f32, 0.5, 0.999_999_9] {
                out += &line(
                    &format!("points.{level}.{points}.{}", fb(progress)),
                    xp_request(
                        level,
                        progress,
                        50,
                        1000,
                        0,
                        vec![json!(["points", points])],
                    ),
                );
            }
        }
    }
    out += &line(
        "points.total-clamp",
        xp_request(
            50,
            0.5,
            i32::MAX - 10,
            1000,
            0,
            vec![
                json!(["points", 100]),
                json!(["points", -5]),
                json!(["points", i32::MAX]),
                json!(["points", i32::MIN]),
            ],
        ),
    );

    // The level-up sound: every multiple of five, the 100-tick cool-down boundary.
    for (tick_count, last) in [
        (0, 0),
        (100, 0),
        (101, 0),
        (1000, 0),
        (1000, 900),
        (1000, 899),
        (1000, 901),
        (-5, -200),
    ] {
        out += &line(
            &format!("sound.{tick_count}.{last}"),
            xp_request(
                3,
                0.5,
                100,
                tick_count,
                last,
                vec![
                    json!(["levels", 2]),
                    json!(["levels", 5]),
                    json!(["levels", 25]),
                    json!(["levels", 1]),
                    json!(["points", 4000]),
                    json!(["levels", 5]),
                ],
            ),
        );
    }

    // Setters and enchanting.
    for level in [0, 5, 30, 100] {
        for amount in [0, 1, 6, 7, 8, 100, 1000, -1, i32::MAX, i32::MIN] {
            out += &line(
                &format!("set_points.{level}.{amount}"),
                xp_request(level, 0.3, 77, 0, 0, vec![json!(["set_points", amount])]),
            );
            out += &line(
                &format!("enchant.{level}.{amount}"),
                xp_request(level, 0.3, 77, 0, 0, vec![json!(["enchant", amount])]),
            );
        }
    }

    // Mixed scripts from the committed seed.
    let mut rng = Rng::for_stream("xp.curves");
    for index in 0..220 {
        let level = rng.below(70) as i32;
        let progress = unit(&mut rng);
        let total = rng.below(100_000) as i32;
        let tick_count = pick(&mut rng, &[0, 50, 150, 1000, 100_000]);
        let steps: Vec<Value> = (0..12).map(|_| random_xp_step(&mut rng, 70)).collect();
        out += &line(
            &format!("random.{index}"),
            xp_request(level, progress, total, tick_count, 0, steps),
        );
    }
    out
}

/// The four corpora, by op name.
pub fn emit() -> Vec<(&'static str, String)> {
    vec![
        ("damage.calc", damage_calc()),
        ("damage_source.scaling", damage_source_scaling()),
        ("food.exhaustion", food_exhaustion()),
        ("xp.curves", xp_curves()),
    ]
}
