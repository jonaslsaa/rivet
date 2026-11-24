//! `trade.offers`: the offers of every vanilla trade set for a fresh merchant,
//! over the 16 seeds.
//!
//! Request `{trade_set, merchant, variant, seed}` (the merchant is the villager,
//! or the wandering trader for the `wandering_trader/*` sets; the villager
//! variant is `minecraft:plains`, the only one swept: eleven trades are
//! restricted to other variants by their merchant predicate). Paper answers the
//! offers as `MerchantOffer.CODEC` SNBT.
//!
//! The Rust side has the trade value types and the selection loop
//! (`rivet_world::trading`) but no decoder for the `villager_trade` and
//! `trade_set` data-pack documents, so every cell is unported (porting it is
//! not part of this verification item).

use serde_json::{Map, Value, json};

use super::super::compare::Outcome;
use super::{SEEDS, data, line};

pub fn emit() -> String {
    let mut out = String::new();
    for set in &data().trade_sets {
        let merchant = if set.starts_with("minecraft:wandering_trader/") {
            "minecraft:wandering_trader"
        } else {
            "minecraft:villager"
        };
        for seed in SEEDS {
            out += &line(
                &format!("{set}#{seed}"),
                vec![
                    ("trade_set", json!(set)),
                    ("merchant", json!(merchant)),
                    ("variant", json!("minecraft:plains")),
                    ("seed", json!(seed)),
                ],
            );
        }
    }
    out
}

pub fn trade_offers(_request: &Map<String, Value>) -> Outcome {
    Outcome::Unported(
        "villager_trade / trade_set data-pack documents have no Rust decoder (rivet_world::trading holds the value types only)".into(),
    )
}
