//! `effect`: `MobEffect.createModifiers(amplifier)` for every mob effect.
//!
//! Request `{effect, amplifier}`; answer `{category, color, modifiers:[{attribute,
//! id, amount (f64 bits), operation}]}`. The Rust side is the table of
//! `rivet_world::effect::MobEffects` and `AttributeTemplate::create`.

use rivet_protocol::game::clientbound_update_attributes_packet::Operation;
use rivet_world::effect::{MobEffectCategory, MobEffectId, MobEffects};
use serde_json::{Map, Value, json};

use super::super::compare::Outcome;
use super::{f64_bits, integer, line, string};

/// The amplifiers every effect is asked for: both sides of zero, the vanilla
/// range's ends (0 and 255), powers of two and the `int` extremes where
/// `amplifier + 1` wraps.
pub const AMPLIFIERS: [i32; 13] = [-1, 0, 1, 2, 3, 4, 7, 15, 63, 127, 254, 255, i32::MAX];

pub fn emit() -> String {
    let mut out = String::new();
    for effect in MobEffectId::ALL {
        let name = format!("minecraft:{}", effect.path());
        for amplifier in AMPLIFIERS {
            out += &line(
                &format!("{name}#{amplifier}"),
                vec![("effect", json!(name)), ("amplifier", json!(amplifier))],
            );
        }
    }
    out
}

fn operation_name(operation: Operation) -> &'static str {
    match operation {
        Operation::AddValue => "add_value",
        Operation::AddMultipliedBase => "add_multiplied_base",
        Operation::AddMultipliedTotal => "add_multiplied_total",
    }
}

pub fn effect(request: &Map<String, Value>) -> Outcome {
    let (name, amplifier) = match (string(request, "effect"), integer(request, "amplifier")) {
        (Ok(name), Ok(amplifier)) => (name, amplifier),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let Ok(amplifier) = i32::try_from(amplifier) else {
        return Outcome::Unavailable("amplifier is not an int".into());
    };
    let Some(id) = MobEffectId::by_name(name) else {
        return Outcome::Unavailable(format!("effect {name} is not in the Rust registry"));
    };
    let def = &MobEffects::DEFS[id as usize];
    let modifiers: Vec<Value> = def
        .attribute_modifiers
        .iter()
        .map(|template| {
            let modifier = template.create(amplifier);
            json!({
                "attribute": template.attribute,
                "id": modifier.id.to_string(),
                "amount": f64_bits(modifier.amount),
                "operation": operation_name(modifier.operation),
            })
        })
        .collect();
    let mut result = Map::new();
    result.insert(
        "category".into(),
        json!(match def.category {
            MobEffectCategory::Beneficial => "beneficial",
            MobEffectCategory::Harmful => "harmful",
            MobEffectCategory::Neutral => "neutral",
        }),
    );
    result.insert("color".into(), json!(def.color()));
    result.insert("modifiers".into(), Value::Array(modifiers));
    Outcome::Accepted(result)
}
