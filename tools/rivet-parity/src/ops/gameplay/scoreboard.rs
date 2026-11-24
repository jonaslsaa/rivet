//! `scoreboard.saved_data`: the `ScoreboardSaveData` codec round trip.
//!
//! Request `{input}`: the SNBT of a `data/scoreboard.dat` `data` compound. Paper
//! decodes it with `ScoreboardSaveData.TYPE.codec()` under `NbtOps` and answers
//!
//! - `codec`: the codec's own re-encoding of the decoded value;
//! - `loaded`: the encoding after the decoded data was loaded into a scoreboard
//!   (objectives, scores, display slots, teams, in `ServerScoreboard.load` order)
//!   and stored back (`pack*`, which drops nothing but Paper's empty-team filter); one
//!   holder's `PlayerScores` entries are sorted by objective, because Paper iterates them by
//!   `Objective` identity hash (a different order on every JVM start, D12).
//!
//! or `{accept:false, phase}` with the phase that declined (`decode`, `encode`,
//! or `load` when the scoreboard rejects the data, e.g. a duplicate objective).
//! Both strings are *canonical SNBT*: compound keys sorted (the compounds
//! themselves are insertion-ordered in Rivet, D12, and in fastutil hash order in
//! Java, so raw order is not a property of the codec), every other tag printed as
//! `StringTagVisitor` prints it. The sorted form is the op's canonical form on
//! both sides; no comparator is involved in it.

use rivet_nbt::byte_tag::ByteTag;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::string_tag_visitor::StringTagVisitor;
use rivet_nbt::tag::Tag;
use rivet_world::scores::{PackedScoreboardData, ServerScoreboard};
use serde_json::{Map, Value, json};

use super::super::compare::Outcome;
use super::{line, string};

/// `key:0b` printed by the visitor, minus braces and value: the key as SNBT spells it.
fn key_text(key: &str) -> String {
    let mut probe = CompoundTag::new();
    probe
        .tags
        .insert(key.to_owned(), Tag::Byte(ByteTag::new(0)));
    let printed = StringTagVisitor::to_string(&Tag::Compound(probe));
    printed[1..printed.len() - ":0b}".len()].to_owned()
}

/// SNBT with every compound's keys sorted (byte order of the key strings, as Java's `String.compareTo` on ASCII).
pub fn canonical_snbt(tag: &Tag) -> String {
    match tag {
        Tag::Compound(compound) => {
            let mut keys: Vec<&String> = compound.tags.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            let entries: Vec<String> = keys
                .into_iter()
                .map(|key| format!("{}:{}", key_text(key), canonical_snbt(&compound.tags[key])))
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        Tag::List(list) => {
            let items: Vec<String> = list.list.iter().map(canonical_snbt).collect();
            format!("[{}]", items.join(","))
        }
        other => StringTagVisitor::to_string(other),
    }
}

/// The stored `PlayerScores` list with each holder's run of consecutive entries sorted by
/// `Objective` (Paper keys one holder's scores by `Objective` identity hash, so their order is
/// not stable from one JVM start to the next; the order of the holders is kept).
fn sort_holder_scores(stored: &Tag) -> Tag {
    let Tag::Compound(compound) = stored else {
        return stored.clone();
    };
    let Some(Tag::List(scores)) = compound.tags.get("PlayerScores") else {
        return stored.clone();
    };
    let text = |tag: &Tag, key: &str| -> String {
        match tag {
            Tag::Compound(entry) => entry.get_string_or(key, "").to_owned(),
            _ => String::new(),
        }
    };
    let entries = &scores.list;
    let mut sorted: Vec<Tag> = Vec::with_capacity(entries.len());
    let mut from = 0;
    while from < entries.len() {
        let holder = text(&entries[from], "Name");
        let mut to = from;
        while to < entries.len() && text(&entries[to], "Name") == holder {
            to += 1;
        }
        let mut run: Vec<Tag> = entries[from..to].to_vec();
        run.sort_by_key(|entry| text(entry, "Objective").encode_utf16().collect::<Vec<u16>>());
        sorted.extend(run);
        from = to;
    }
    let mut copy = compound.clone();
    let mut list = scores.clone();
    list.list = sorted;
    copy.tags.insert("PlayerScores".to_owned(), Tag::List(list));
    Tag::Compound(copy)
}

pub fn scoreboard_saved_data(request: &Map<String, Value>) -> Outcome {
    let input = match string(request, "input") {
        Ok(input) => input,
        Err(e) => return e,
    };
    let Ok(tag) = crate::rust_parse_snbt(input) else {
        // `TagParser.parseFully` failing is the op's rejection verdict.
        return Outcome::Rejected;
    };
    let decline = |phase: &str| {
        let mut result = Map::new();
        result.insert("accept".into(), json!(false));
        result.insert("phase".into(), json!(phase));
        Outcome::Accepted(result)
    };
    // A panic inside the ported codec or scoreboard is the Rust spelling of a Java exception;
    // it declines in the phase it happened in, as the oracle's own catch blocks do (a Java
    // exception while loading or re-storing is `load`).
    let phase = std::cell::Cell::new("decode");
    let answered = crate::ops::packet::guarded(|| {
        let Ok(decoded) = PackedScoreboardData::decode_nbt(&tag) else {
            return Err("decode");
        };
        phase.set("encode");
        let Ok(direct) = decoded.encode_nbt() else {
            return Err("encode");
        };
        phase.set("load");
        let mut scoreboard = ServerScoreboard::new();
        let loaded = scoreboard.load_packed(
            &decoded.objectives,
            &decoded.scores,
            &decoded.display_slots,
            &decoded.teams,
        );
        if loaded.is_err() {
            return Err("load");
        }
        let stored = PackedScoreboardData {
            objectives: scoreboard.pack_objectives(),
            scores: scoreboard.pack_player_scores(),
            display_slots: scoreboard.pack_display_slots(),
            teams: scoreboard.pack_player_teams(),
        };
        let Ok(stored) = stored.encode_nbt() else {
            return Err("encode");
        };
        Ok((canonical_snbt(&direct), canonical_snbt(&sort_holder_scores(&stored))))
    });
    match answered {
        Ok(Ok((codec, loaded))) => {
            let mut result = Map::new();
            result.insert("accept".into(), json!(true));
            result.insert("codec".into(), json!(codec));
            result.insert("loaded".into(), json!(loaded));
            Outcome::Accepted(result)
        }
        Ok(Err(phase)) => decline(phase),
        Err(_) => decline(phase.get()),
    }
}

// ---- corpus ------------------------------------------------------------------------------

/// SNBT string literal for `text`.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

const SLOTS: [&str; 19] = [
    "list",
    "sidebar",
    "below_name",
    "sidebar.team.black",
    "sidebar.team.dark_blue",
    "sidebar.team.dark_green",
    "sidebar.team.dark_aqua",
    "sidebar.team.dark_red",
    "sidebar.team.dark_purple",
    "sidebar.team.gold",
    "sidebar.team.gray",
    "sidebar.team.dark_gray",
    "sidebar.team.blue",
    "sidebar.team.green",
    "sidebar.team.aqua",
    "sidebar.team.red",
    "sidebar.team.light_purple",
    "sidebar.team.yellow",
    "sidebar.team.white",
];

const COLORS: [&str; 16] = [
    "black",
    "dark_blue",
    "dark_green",
    "dark_aqua",
    "dark_red",
    "dark_purple",
    "gold",
    "gray",
    "dark_gray",
    "blue",
    "green",
    "aqua",
    "red",
    "light_purple",
    "yellow",
    "white",
];

/// `(label, SNBT)` of every input: valid documents covering every field and enum
/// value, then documents each broken in one way.
fn documents() -> Vec<(String, String)> {
    let mut docs: Vec<(String, String)> = Vec::new();
    let mut add = |label: &str, snbt: String| docs.push((label.to_owned(), snbt));
    add("empty", "{}".into());
    add(
        "objective_minimal",
        r#"{Objectives:[{Name:"a",DisplayName:"A"}]}"#.into(),
    );
    add(
        "objective_full",
        r#"{Objectives:[{Name:"kills",CriteriaName:"playerKillCount",DisplayName:{text:"Kills",color:"red",bold:1b},RenderType:"integer",display_auto_update:1b,format:{type:"minecraft:fixed",value:"x"}}]}"#.into(),
    );
    for criteria in [
        "dummy",
        "trigger",
        "health",
        "xp",
        "food",
        "air",
        "armor",
        "level",
        "deathCount",
        "playerKillCount",
        "totalKillCount",
        "teamkill.red",
        "killedByTeam.blue",
        "minecraft.mined:minecraft.stone",
        "minecraft.custom:minecraft.jump",
        "no_such_criteria",
    ] {
        add(
            &format!("criteria_{criteria}"),
            format!(
                r#"{{Objectives:[{{Name:"o",CriteriaName:{},DisplayName:"O"}}]}}"#,
                quoted(criteria)
            ),
        );
    }
    for render in ["integer", "hearts", "sideways"] {
        add(
            &format!("render_{render}"),
            format!(
                r#"{{Objectives:[{{Name:"o",DisplayName:"O",RenderType:{}}}]}}"#,
                quoted(render)
            ),
        );
    }
    for (label, format) in [
        ("blank", r#"{type:"minecraft:blank"}"#),
        (
            "fixed",
            r#"{type:"minecraft:fixed",value:{text:"n",italic:1b}}"#,
        ),
        ("styled", r#"{type:"minecraft:styled",color:"red",bold:1b}"#),
        ("unknown_type", r#"{type:"minecraft:nope"}"#),
        ("not_a_compound", "7"),
    ] {
        add(
            &format!("objective_format_{label}"),
            format!(r#"{{Objectives:[{{Name:"o",DisplayName:"O",format:{format}}}]}}"#),
        );
    }
    for (label, name) in [
        ("string", r#""plain""#),
        ("compound", r##"{text:"c",color:"#ff00aa"}"##),
        ("list", r#"[{text:"a"},{text:"b",bold:1b}]"#),
        (
            "translate",
            r#"{translate:"chat.type.text",with:["a","b"]}"#,
        ),
        ("number", "5"),
    ] {
        add(
            &format!("display_name_{label}"),
            format!(r#"{{Objectives:[{{Name:"o",DisplayName:{name}}}]}}"#),
        );
    }
    add(
        "scores",
        r#"{Objectives:[{Name:"a",DisplayName:"A"},{Name:"b",DisplayName:"B"}],PlayerScores:[{Name:"x",Objective:"a",Score:5},{Name:"x",Objective:"b",Score:-7,Locked:1b},{Name:"y",Objective:"a",Score:2147483647,display:{text:"top"},format:{type:"minecraft:blank"}},{Name:"z",Objective:"b"}]}"#.into(),
    );
    add(
        "score_unknown_objective",
        r#"{Objectives:[{Name:"a",DisplayName:"A"}],PlayerScores:[{Name:"x",Objective:"ghost",Score:1},{Name:"y",Objective:"a",Score:2}]}"#.into(),
    );
    add(
        "score_wrong_type",
        r#"{Objectives:[{Name:"a",DisplayName:"A"}],PlayerScores:[{Name:"x",Objective:"a",Score:"many"}]}"#.into(),
    );
    add(
        "score_missing_name",
        r#"{Objectives:[{Name:"a",DisplayName:"A"}],PlayerScores:[{Objective:"a",Score:1}]}"#
            .into(),
    );
    let all_slots: Vec<String> = SLOTS
        .iter()
        .map(|s| format!("{}:\"a\"", quoted(s)))
        .collect();
    add(
        "display_slots_all",
        format!(
            r#"{{Objectives:[{{Name:"a",DisplayName:"A"}}],DisplaySlots:{{{}}}}}"#,
            all_slots.join(",")
        ),
    );
    add(
        "display_slot_unknown_objective",
        r#"{Objectives:[{Name:"a",DisplayName:"A"}],DisplaySlots:{sidebar:"ghost",list:"a"}}"#
            .into(),
    );
    add(
        "display_slot_unknown_slot",
        r#"{Objectives:[{Name:"a",DisplayName:"A"}],DisplaySlots:{nowhere:"a"}}"#.into(),
    );
    add("team_minimal", r#"{Teams:[{Name:"t"}]}"#.into());
    add(
        "team_full",
        r#"{Teams:[{Name:"red",DisplayName:{text:"Red Team"},TeamColor:"red",AllowFriendlyFire:0b,SeeFriendlyInvisibles:0b,MemberNamePrefix:{text:"[R] "},MemberNameSuffix:"!",NameTagVisibility:"hideForOtherTeams",DeathMessageVisibility:"never",CollisionRule:"pushOwnTeam",Players:["a","b","c"]}]}"#.into(),
    );
    for color in COLORS {
        add(
            &format!("team_color_{color}"),
            format!(
                r#"{{Teams:[{{Name:"t",TeamColor:{},Players:["p"]}}]}}"#,
                quoted(color)
            ),
        );
    }
    add(
        "team_color_unknown",
        r#"{Teams:[{Name:"t",TeamColor:"mauve",Players:["p"]}]}"#.into(),
    );
    for visibility in [
        "always",
        "never",
        "hideForOtherTeams",
        "hideForOwnTeam",
        "sometimes",
    ] {
        add(
            &format!("team_visibility_{visibility}"),
            format!(
                r#"{{Teams:[{{Name:"t",NameTagVisibility:{v},DeathMessageVisibility:{v},Players:["p"]}}]}}"#,
                v = quoted(visibility)
            ),
        );
    }
    for rule in [
        "always",
        "never",
        "pushOtherTeams",
        "pushOwnTeam",
        "pushAll",
    ] {
        add(
            &format!("team_collision_{rule}"),
            format!(
                r#"{{Teams:[{{Name:"t",CollisionRule:{},Players:["p"]}}]}}"#,
                quoted(rule)
            ),
        );
    }
    add(
        "teams_member_in_two",
        r#"{Teams:[{Name:"a",Players:["p","q"]},{Name:"b",Players:["p"]}]}"#.into(),
    );
    add(
        "teams_duplicate_name",
        r#"{Teams:[{Name:"a",Players:["p"]},{Name:"a",Players:["q"]}]}"#.into(),
    );
    add(
        "team_empty_and_filled",
        r#"{Teams:[{Name:"empty"},{Name:"filled",Players:["p"]}]}"#.into(),
    );
    add(
        "team_duplicate_players",
        r#"{Teams:[{Name:"a",Players:["p","p"]}]}"#.into(),
    );
    add(
        "objective_duplicate_name",
        r#"{Objectives:[{Name:"a",DisplayName:"A"},{Name:"a",DisplayName:"B"}]}"#.into(),
    );
    add(
        "objective_missing_name",
        r#"{Objectives:[{DisplayName:"A"}]}"#.into(),
    );
    add(
        "objective_missing_display_name",
        r#"{Objectives:[{Name:"a"}]}"#.into(),
    );
    add(
        "objective_name_wrong_type",
        r#"{Objectives:[{Name:5,DisplayName:"A"}]}"#.into(),
    );
    add("objectives_wrong_type", "{Objectives:7}".into());
    add("teams_wrong_type", r#"{Teams:"none"}"#.into());
    add("scores_wrong_type", "{PlayerScores:{}}".into());
    add("display_slots_wrong_type", "{DisplaySlots:[1b]}".into());
    add(
        "unknown_top_level_key",
        r#"{Objectives:[{Name:"a",DisplayName:"A",Extra:1b}],Mystery:5}"#.into(),
    );
    add(
        "everything",
        r#"{Objectives:[{Name:"a",DisplayName:"A"},{Name:"b",CriteriaName:"health",DisplayName:{text:"B"},RenderType:"hearts"}],PlayerScores:[{Name:"p",Objective:"a",Score:3},{Name:"q",Objective:"b",Score:20}],DisplaySlots:{list:"b",sidebar:"a","sidebar.team.red":"a"},Teams:[{Name:"red",TeamColor:"red",Players:["p"]},{Name:"blue",TeamColor:"blue",Players:["q"]}]}"#.into(),
    );
    docs
}

pub fn emit() -> String {
    documents()
        .into_iter()
        .map(|(label, snbt)| line(&label, vec![("input", json!(snbt))]))
        .collect()
}
