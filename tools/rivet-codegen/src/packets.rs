//! `rivet-codegen generate` packets half — consume the pinned vanilla
//! `data/reports/packets.json` (the `PacketReport` output, see [`crate::reports`])
//! and emit the `rivet-protocol` packet-ID tables into
//! `crates/rivet-protocol/src/generated/`.
//!
//! The fixture maps, per protocol state and packet flow, every packet name to a
//! `protocol_id`. That id is the `addPacket` registration index in the vanilla
//! `*Protocols.TEMPLATE` definitions (`ProtocolInfoBuilder` / `IdDispatchCodec`
//! assign the next integer in call order), so sorting each flow's entries by
//! `protocol_id` recovers the exact `addPacket` order. The committed JSON keys
//! are alphabetically sorted by GsonHelper's stable writer, so the codegen never
//! trusts key order — it re-orders by `protocol_id`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde::de;
use serde_json::Value;

/// `ConnectionProtocol` enum declaration order (vanilla sources). Generated
/// state modules follow this rather than the JSON's alphabetical key order.
const STATE_ORDER: &[&str] = &["handshake", "play", "status", "login", "configuration"];

/// `PacketFlow` enum declaration order.
const FLOW_ORDER: &[&str] = &["serverbound", "clientbound"];

pub fn default_input(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/reports/packets.json")
}

pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("crates/rivet-protocol/src/generated")
}

/// Validated packet-ID tables plus the provenance linked from `manifest.json`.
#[derive(Debug)]
struct Report {
    provenance: crate::reports::SourceProvenance,
    states: Vec<StateTable>,
}

#[derive(Debug)]
struct StateTable {
    id: &'static str,
    flows: Vec<FlowTable>,
}

#[derive(Debug)]
struct FlowTable {
    id: &'static str,
    /// Entries in `addPacket` order (sorted by `protocol_id`).
    packets: Vec<PacketEntry>,
}

#[derive(Debug)]
struct PacketEntry {
    /// Canonical packet name, e.g. `"minecraft:add_entity"`.
    name: String,
    protocol_id: u32,
}

pub fn run(packets_flag: Option<&Path>, output_flag: Option<&Path>) -> Result<()> {
    let repo_root = crate::extract::find_repo_root()?;
    let input = match packets_flag {
        Some(p) => p.to_path_buf(),
        None => default_input(&repo_root),
    };
    let output = match output_flag {
        Some(p) => p.to_path_buf(),
        None => default_output(&repo_root),
    };

    let json = fs::read_to_string(&input).with_context(|| format!("read {}", input.display()))?;
    let value = parse_strict(&json).with_context(|| format!("parse {}", input.display()))?;
    let states = validate(value)?;
    let provenance = load_provenance(&input)?;
    let report = Report { provenance, states };

    fs::create_dir_all(&output).with_context(|| format!("create {}", output.display()))?;
    let (mod_rs, protocol, packets) = render(&report);
    fs::write(output.join("mod.rs"), mod_rs).context("write generated/mod.rs")?;
    fs::write(output.join("protocol.rs"), protocol).context("write generated/protocol.rs")?;
    fs::write(output.join("packets.rs"), packets).context("write generated/packets.rs")?;

    let total: usize = report
        .states
        .iter()
        .flat_map(|s| &s.flows)
        .map(|f| f.packets.len())
        .sum();
    println!(
        "Wrote {} packet types across {} protocol states -> {}",
        total,
        report.states.len(),
        output.display()
    );
    Ok(())
}

/// Parse JSON, rejecting duplicate object keys at any depth. serde_json silently
/// last-wins on duplicate keys by default; a hand-inserted duplicate packet name
/// must fail instead of silently reshaping the table.
fn parse_strict(json: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str::<StrictValue>(json).map(|strict| strict.0)
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = StrictValue;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON value without duplicate object keys")
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(v)))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(v.into())))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(v.into())))
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                let number = serde_json::Number::from_f64(v)
                    .ok_or_else(|| E::custom("non-finite JSON number"))?;
                Ok(StrictValue(Value::Number(number)))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v.to_string())))
            }

            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }

            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }

            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: de::Deserializer<'de>,
            {
                deserializer.deserialize_any(Visitor)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<StrictValue>()? {
                    values.push(value.0);
                }
                Ok(StrictValue(Value::Array(values)))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                let mut object = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    let value = map.next_value::<StrictValue>()?;
                    if object.insert(key.clone(), value.0).is_some() {
                        return Err(de::Error::custom(format!("duplicate object key `{key}`")));
                    }
                }
                Ok(StrictValue(Value::Object(object)))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// Structural validation of the packet report. Returns per-state tables with
/// packets in `addPacket` order (sorted by `protocol_id`).
fn validate(value: Value) -> Result<Vec<StateTable>> {
    let root = value
        .as_object()
        .context("packets.json root must be a JSON object")?;

    for state in root.keys() {
        if !STATE_ORDER.contains(&state.as_str()) {
            bail!(
                "unknown protocol state `{state}` in packets.json (expected one of: {})",
                STATE_ORDER.join(", ")
            );
        }
    }
    for state in STATE_ORDER {
        if !root.contains_key(*state) {
            bail!("missing protocol state `{state}` in packets.json");
        }
    }

    let mut states = Vec::with_capacity(STATE_ORDER.len());
    for state in STATE_ORDER {
        let Some(state_obj) = root[*state].as_object() else {
            bail!("protocol state `{state}` in packets.json must be a JSON object");
        };
        for flow in state_obj.keys() {
            if !FLOW_ORDER.contains(&flow.as_str()) {
                bail!(
                    "unknown packet flow `{flow}` in state `{state}` (expected one of: {})",
                    FLOW_ORDER.join(", ")
                );
            }
        }
        let mut flows = Vec::new();
        for flow in FLOW_ORDER {
            let Some(flow_value) = state_obj.get(*flow) else {
                continue;
            };
            let Some(flow_obj) = flow_value.as_object() else {
                bail!(
                    "packet flow `{flow}` in state `{state}` must be a JSON object of packet entries"
                );
            };
            let mut packets = Vec::with_capacity(flow_obj.len());
            for (name, entry) in flow_obj {
                validate_packet_name(state, flow, name)?;
                let protocol_id = parse_protocol_id(state, flow, name, entry)?;
                packets.push(PacketEntry {
                    name: name.clone(),
                    protocol_id,
                });
            }
            // addPacket order is the protocol_id order.
            packets.sort_unstable_by_key(|p| p.protocol_id);
            // Duplicate ids: two packets cannot claim the same addPacket index.
            for pair in packets.windows(2) {
                if pair[0].protocol_id == pair[1].protocol_id {
                    bail!(
                        "duplicate protocol_id {} in {state}/{flow}: `{}` and `{}`",
                        pair[0].protocol_id,
                        pair[0].name,
                        pair[1].name
                    );
                }
            }
            // Contiguity: the generated id-indexed table assumes id == index.
            for (i, p) in packets.iter().enumerate() {
                if p.protocol_id as usize != i {
                    bail!(
                        "protocol ids in {state}/{flow} are not contiguous 0..{}: expected {} at index {i}, got {}",
                        packets.len(),
                        i,
                        p.protocol_id
                    );
                }
            }
            // Variant collisions: two distinct names (e.g. `minecraft:a/b` and
            // `minecraft:a_b`) can camel-case to the same Rust variant, which
            // would emit code that does not compile.
            {
                let mut variants = packets
                    .iter()
                    .map(|p| packet_variant(&p.name))
                    .collect::<Vec<_>>();
                variants.sort_unstable();
                for pair in variants.windows(2) {
                    if pair[0] == pair[1] {
                        bail!(
                            "packet names in {state}/{flow} collide on Rust variant `{}` — \
                             the generated enum would not compile; rename or separate the colliding packets",
                            pair[0]
                        );
                    }
                }
            }
            flows.push(FlowTable { id: flow, packets });
        }
        states.push(StateTable { id: state, flows });
    }

    Ok(states)
}

fn validate_packet_name(state: &str, flow: &str, name: &str) -> Result<()> {
    let Some((namespace, path)) = name.split_once(':') else {
        bail!(
            "packet `{name}` in {state}/{flow} is not a namespaced ResourceLocation (`namespace:path`)"
        );
    };
    if namespace.is_empty() {
        bail!("packet `{name}` in {state}/{flow} has an empty namespace");
    }
    if path.is_empty() {
        bail!("packet `{name}` in {state}/{flow} has an empty path");
    }
    // The name becomes a Rust enum variant; a name that maps to something that
    // is not a valid identifier would emit code that does not compile.
    let variant = packet_variant(name);
    if !is_valid_ident(&variant) {
        bail!(
            "packet `{name}` in {state}/{flow} does not map to a valid Rust identifier (`{variant}`)"
        );
    }
    Ok(())
}

/// Whether `s` is a valid Rust identifier (`[a-zA-Z_][a-zA-Z0-9_]*`).
fn is_valid_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn parse_protocol_id(state: &str, flow: &str, name: &str, entry: &Value) -> Result<u32> {
    let obj = entry
        .as_object()
        .with_context(|| format!("packet `{name}` in {state}/{flow} must be a JSON object"))?;
    let extra: Vec<&String> = obj.keys().filter(|k| k.as_str() != "protocol_id").collect();
    if !extra.is_empty() {
        bail!(
            "packet `{name}` in {state}/{flow} has unexpected fields: {}",
            extra
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let id = obj
        .get("protocol_id")
        .with_context(|| format!("packet `{name}` in {state}/{flow} is missing `protocol_id`"))?;
    let id = match id {
        Value::Number(n) => {
            if let Some(id) = n.as_u64() {
                id
            } else if n.as_i64().is_some_and(|i| i < 0) {
                bail!("packet `{name}` in {state}/{flow} has a negative `protocol_id` ({n})");
            } else {
                // Not representable as an integer at all (e.g. a float like
                // `1.5`): must fail loudly, not silently coerce.
                bail!("packet `{name}` in {state}/{flow} has a non-integer `protocol_id`");
            }
        }
        _ => bail!("packet `{name}` in {state}/{flow} has a non-integer `protocol_id`"),
    };
    let id = u32::try_from(id).with_context(|| {
        format!("packet `{name}` in {state}/{flow} has a `protocol_id` outside the u32 range")
    })?;
    Ok(id)
}

/// Link the consumed fixture to its pinned provenance (`manifest.json`): the
/// packet report's recorded sha256 must match the file actually being read, and
/// the header carries the MC/protocol/world versions + source jar identity.
fn load_provenance(input: &Path) -> Result<crate::reports::SourceProvenance> {
    let manifest_path = input
        .parent()
        .map(|p| p.join("manifest.json"))
        .with_context(|| format!("packets.json has no parent dir: {}", input.display()))?;
    let manifest_json = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "read {} (expected next to the pinned packets.json fixture; a --packets input must \
             be the committed fixture, whose provenance lives in the sibling manifest.json)",
            manifest_path.display()
        )
    })?;
    let manifest: crate::reports::ProvenanceManifest = serde_json::from_str(&manifest_json)
        .with_context(|| format!("parse {}", manifest_path.display()))?;
    let entry = manifest
        .reports
        .iter()
        .find(|e| e.path == "packets.json")
        .with_context(|| {
            format!(
                "manifest {} has no packets.json entry",
                manifest_path.display()
            )
        })?;
    let bytes = fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let actual = crate::reports::sha256_hex(&bytes);
    if actual != entry.sha256 {
        bail!(
            "packets.json does not match the provenance manifest (expected sha256 {}, got {}) — \
             run `rivet-codegen reports` to refresh the pinned fixture",
            entry.sha256,
            actual
        );
    }
    crate::reports::verify_pinned_source(&manifest.source)?;
    Ok(manifest.source)
}

/// Render `(mod.rs, protocol.rs, packets.rs)` for `crates/rivet-protocol`.
fn render(report: &Report) -> (String, String, String) {
    let source = &report.provenance;
    let header = format!(
        "// Generated by `tools/rivet-codegen generate` from data/reports/packets.json\n\
         // (vanilla PacketReport; MC {}, protocol {}, world {}).\n\
         // Source jar sha256 {}; provenance linked to data/reports/manifest.json.\n\
         // Do not edit by hand — PORTING.md: packet IDs are generated, not hand-ported.\n\n",
        source.minecraft_version,
        source.protocol_version,
        source.world_version,
        source.jar_sha256.get(..16).unwrap_or(&source.jar_sha256)
    );
    let mod_rs = format!("{header}pub mod packets;\npub mod protocol;\n");
    let protocol = format!("{header}{}", render_protocol());
    let packets = format!("{header}{}", render_packets(report));
    (mod_rs, protocol, packets)
}

fn render_protocol() -> String {
    let mut out = String::new();

    out.push_str("/// A connection protocol state. Mirrors\n");
    out.push_str("/// `net.minecraft.network.ConnectionProtocol`; variant order matches the\n");
    out.push_str(
        "/// vanilla enum declaration order (handshake, play, status, login, configuration).\n",
    );
    out.push_str("#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]\n");
    out.push_str("pub enum ConnectionProtocol {\n");
    for state in STATE_ORDER {
        out.push_str(&format!("    /// `{state}`\n    {},\n", camel(state)));
    }
    out.push_str("}\n\n");

    out.push_str("impl ConnectionProtocol {\n");
    out.push_str("    /// The vanilla string id (e.g. `\"play\"`).\n");
    out.push_str("    pub const fn id(self) -> &'static str {\n        match self {\n");
    for state in STATE_ORDER {
        out.push_str(&format!(
            "            Self::{} => {state:?},\n",
            camel(state)
        ));
    }
    out.push_str("        }\n    }\n\n");
    out.push_str("    /// Look up a state by its vanilla string id.\n");
    out.push_str("    pub fn from_id(id: &str) -> Option<Self> {\n        match id {\n");
    for state in STATE_ORDER {
        out.push_str(&format!(
            "            {state:?} => Some(Self::{}),\n",
            camel(state)
        ));
    }
    out.push_str("            _ => None,\n        }\n    }\n\n");
    out.push_str(&format!(
        "    /// All states in `ConnectionProtocol` declaration order.\n    pub const ALL: [Self; {}] = [",
        STATE_ORDER.len()
    ));
    for (i, state) in STATE_ORDER.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("Self::{}", camel(state)));
    }
    out.push_str("];\n}\n\n");

    out.push_str("/// A packet direction. Mirrors `net.minecraft.network.protocol.PacketFlow`.\n");
    out.push_str("#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]\n");
    out.push_str("pub enum PacketFlow {\n");
    for flow in FLOW_ORDER {
        out.push_str(&format!("    /// `{flow}`\n    {},\n", camel(flow)));
    }
    out.push_str("}\n\n");

    out.push_str("impl PacketFlow {\n");
    out.push_str("    /// The vanilla string id (`\"serverbound\"` / `\"clientbound\"`).\n");
    out.push_str("    pub const fn id(self) -> &'static str {\n        match self {\n");
    for flow in FLOW_ORDER {
        out.push_str(&format!("            Self::{} => {flow:?},\n", camel(flow)));
    }
    out.push_str("        }\n    }\n\n");
    out.push_str("    /// Look up a flow by its vanilla string id.\n");
    out.push_str("    pub fn from_id(id: &str) -> Option<Self> {\n        match id {\n");
    for flow in FLOW_ORDER {
        out.push_str(&format!(
            "            {flow:?} => Some(Self::{}),\n",
            camel(flow)
        ));
    }
    out.push_str("            _ => None,\n        }\n    }\n\n");
    out.push_str("    /// The opposite direction.\n");
    out.push_str("    pub const fn opposite(self) -> Self {\n        match self {\n");
    out.push_str("            Self::Serverbound => Self::Clientbound,\n");
    out.push_str("            Self::Clientbound => Self::Serverbound,\n");
    out.push_str("        }\n    }\n\n");
    out.push_str(&format!(
        "    /// Both flows in `PacketFlow` declaration order.\n    pub const ALL: [Self; {}] = [Self::Serverbound, Self::Clientbound];\n",
        FLOW_ORDER.len()
    ));
    out.push_str("}\n");

    out
}

fn render_packets(report: &Report) -> String {
    let mut out = String::new();
    for state in &report.states {
        out.push_str(&format!(
            "/// Packets for the `{}` connection state.\n",
            state.id
        ));
        out.push_str(&format!("pub mod {} {{\n", state.id));
        let mut state_body = String::new();
        for flow in &state.flows {
            let mut flow_body = String::new();
            render_flow(&mut flow_body, state, flow);
            state_body.push_str(&format!(
                "/// `{}`-direction packet types (PacketFlow).\n",
                flow.id
            ));
            state_body.push_str(&format!("pub mod {} {{\n", flow.id));
            state_body.push_str(&indent(&flow_body, 4));
            state_body.push_str("\n}\n");
        }
        out.push_str(&indent(&state_body, 4));
        // `indent` strips the trailing newline; re-add it before the state close.
        out.push_str("\n}\n");
    }
    out
}

fn render_flow(out: &mut String, state: &StateTable, flow: &FlowTable) {
    let template = format!(
        "{}.{}_TEMPLATE",
        protocols_class(state.id),
        flow.id.to_uppercase()
    );
    let n = flow.packets.len();

    out.push_str("/// One packet type in this state/flow. Discriminant == the vanilla\n");
    out.push_str(&format!(
        "/// `protocol_id` (the `addPacket` index in `{template}`).\n"
    ));
    out.push_str("/// Variants are in `addPacket` registration order.\n");
    out.push_str("#[repr(u32)]\n");
    out.push_str("#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]\n");
    out.push_str("pub enum PacketType {\n");
    for p in &flow.packets {
        out.push_str(&format!("    /// `{}`\n", p.name));
        out.push_str(&format!(
            "    {} = {},\n",
            packet_variant(&p.name),
            p.protocol_id
        ));
    }
    out.push_str("}\n\n");

    out.push_str(&format!(
        "/// Packet names indexed by `protocol_id` (id == index; ids are contiguous `0..{n}`).\n"
    ));
    out.push_str("pub static PACKET_BY_ID: &[&str] = &[\n");
    for p in &flow.packets {
        out.push_str(&format!("    {}, // {}\n", quote(&p.name), p.protocol_id));
    }
    out.push_str("];\n\n");

    out.push_str("/// Canonical packet name (`minecraft:...`) -> `protocol_id`.\n");
    out.push_str("pub static PACKET_BY_NAME: phf::Map<&'static str, u32> = phf::phf_map! {\n");
    for p in &flow.packets {
        out.push_str(&format!("    {} => {},\n", quote(&p.name), p.protocol_id));
    }
    out.push_str("};\n\n");

    out.push_str("impl PacketType {\n");
    out.push_str("    /// The vanilla `protocol_id` (the `addPacket` index).\n");
    out.push_str(
        "    #[inline]\n    pub const fn id(self) -> u32 {\n        self as u32\n    }\n\n",
    );
    out.push_str("    /// The canonical packet name, e.g. `\"minecraft:add_entity\"`.\n");
    out.push_str(
        "    pub fn name(self) -> &'static str {\n        PACKET_BY_ID[self as usize]\n    }\n\n",
    );
    out.push_str("    /// Look up a packet type by `protocol_id`.\n");
    out.push_str("    pub fn from_id(id: u32) -> Option<Self> {\n        match id {\n");
    for p in &flow.packets {
        out.push_str(&format!(
            "            {} => Some(Self::{}),\n",
            p.protocol_id,
            packet_variant(&p.name)
        ));
    }
    out.push_str("            _ => None,\n        }\n    }\n\n");
    out.push_str("    /// Look up a packet type by its canonical packet name.\n");
    out.push_str("    pub fn from_name(name: &str) -> Option<Self> {\n        PACKET_BY_NAME.get(name).copied().and_then(Self::from_id)\n    }\n");
    out.push_str("}\n\n");

    out.push_str(&format!(
        "/// Every packet type in this state/flow, in `addPacket` order.\n\
         pub const ALL: [PacketType; {n}] = [\n"
    ));
    for p in &flow.packets {
        out.push_str(&format!("    PacketType::{},\n", packet_variant(&p.name)));
    }
    out.push_str("];\n");
}

/// `"play"` -> `Play`; `"clientbound"` -> `Clientbound`.
fn camel(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `minecraft:add_entity` -> `AddEntity`; `minecraft:debug/block_value` ->
/// `DebugBlockValue` (the `/` separator in e.g. the debug packets is treated
/// like `_`). A leading digit in a path part is prefixed with `_`.
fn packet_variant(name: &str) -> String {
    let path = name.rsplit_once(':').map(|(_, p)| p).unwrap_or(name);
    let mut out = String::new();
    for part in path.split(['_', '/']).filter(|p| !p.is_empty()) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            if first.is_ascii_digit() {
                out.push('_');
            }
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        "Packet".to_string()
    } else {
        out
    }
}

/// Which vanilla `*Protocols` class holds the templates for a state.
fn protocols_class(state: &str) -> &'static str {
    match state {
        "handshake" => "HandshakeProtocols",
        "play" => "GameProtocols",
        "status" => "StatusProtocols",
        "login" => "LoginProtocols",
        "configuration" => "ConfigurationProtocols",
        _ => unreachable!("validated state"),
    }
}

fn indent(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{pad}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn quote(s: &str) -> String {
    format!("{s:?}")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn minimal_report() -> Value {
        serde_json::json!({
            "handshake": {},
            "play": {},
            "status": {},
            "login": {},
            "configuration": {}
        })
    }

    #[test]
    fn minimal_report_is_valid() {
        assert!(validate(minimal_report()).is_ok());
    }

    #[test]
    fn non_object_root_is_rejected() {
        let err = validate(serde_json::json!([1, 2, 3])).unwrap_err();
        assert!(
            err.to_string()
                .contains("packets.json root must be a JSON object"),
            "got: {err}"
        );
    }

    #[test]
    fn non_object_entry_is_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({
            "minecraft:attack": 42
        });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("must be a JSON object"),
            "got: {err}"
        );
    }

    #[test]
    fn non_integer_protocol_id_is_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({
            "minecraft:attack": { "protocol_id": 1.5 }
        });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("non-integer `protocol_id`"),
            "got: {err}"
        );
    }

    #[test]
    fn non_numeric_protocol_id_is_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({
            "minecraft:attack": { "protocol_id": "one" }
        });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("non-integer `protocol_id`"),
            "got: {err}"
        );
    }

    #[test]
    fn negative_protocol_id_is_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({
            "minecraft:attack": { "protocol_id": -1 }
        });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("negative `protocol_id`"),
            "got: {err}"
        );
    }

    #[test]
    fn overflow_protocol_id_is_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({
            "minecraft:attack": { "protocol_id": 4294967296u64 }
        });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("outside the u32 range"),
            "got: {err}"
        );
    }

    #[test]
    fn duplicate_protocol_id_is_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({
            "minecraft:accept_teleportation": { "protocol_id": 0 },
            "minecraft:attack": { "protocol_id": 0 }
        });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string()
                .contains("duplicate protocol_id 0 in play/serverbound"),
            "got: {err}"
        );
    }

    #[test]
    fn non_contiguous_ids_are_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({
            "minecraft:accept_teleportation": { "protocol_id": 0 },
            "minecraft:attack": { "protocol_id": 2 }
        });
        let err = validate(value).unwrap_err();
        assert!(err.to_string().contains("not contiguous"), "got: {err}");
    }

    #[test]
    fn ids_not_starting_at_zero_are_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({
            "minecraft:accept_teleportation": { "protocol_id": 1 },
            "minecraft:attack": { "protocol_id": 2 }
        });
        let err = validate(value).unwrap_err();
        assert!(err.to_string().contains("not contiguous"), "got: {err}");
    }

    #[test]
    fn input_key_order_is_not_trusted() {
        // The committed JSON keys are alphabetically sorted by GsonHelper, so
        // the generator must re-order by protocol_id rather than trust key
        // order. This mirrors a hand-edited fixture that put ids out of order.
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({
            "minecraft:attack": { "protocol_id": 1 },
            "minecraft:accept_teleportation": { "protocol_id": 0 }
        });
        let states = validate(value).unwrap();
        let play = states.iter().find(|s| s.id == "play").unwrap();
        let sb = play.flows.iter().find(|f| f.id == "serverbound").unwrap();
        assert_eq!(sb.packets[0].name, "minecraft:accept_teleportation");
        assert_eq!(sb.packets[0].protocol_id, 0);
        assert_eq!(sb.packets[1].name, "minecraft:attack");
        assert_eq!(sb.packets[1].protocol_id, 1);
    }

    #[test]
    fn unknown_state_is_rejected() {
        let mut value = minimal_report();
        value["sillywalk"] = serde_json::json!({});
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string()
                .contains("unknown protocol state `sillywalk`"),
            "got: {err}"
        );
    }

    #[test]
    fn missing_state_is_rejected() {
        let mut value = minimal_report();
        value.as_object_mut().unwrap().remove("configuration");
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string()
                .contains("missing protocol state `configuration`"),
            "got: {err}"
        );
    }

    #[test]
    fn unknown_flow_is_rejected() {
        let mut value = minimal_report();
        value["play"]["sideways"] = serde_json::json!({});
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string()
                .contains("unknown packet flow `sideways` in state `play`"),
            "got: {err}"
        );
    }

    #[test]
    fn non_object_flow_is_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!("not-an-object");
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string()
                .contains("packet flow `serverbound` in state `play` must be a JSON object"),
            "got: {err}"
        );
    }

    #[test]
    fn non_object_state_is_rejected() {
        let mut value = minimal_report();
        value["play"] = serde_json::json!(42);
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string()
                .contains("protocol state `play` in packets.json must be a JSON object"),
            "got: {err}"
        );
    }

    #[test]
    fn non_namespaced_packet_is_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] =
            serde_json::json!({ "not_namespaced": { "protocol_id": 0 } });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string()
                .contains("not a namespaced ResourceLocation"),
            "got: {err}"
        );
    }

    #[test]
    fn empty_path_packet_is_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({ "minecraft:": { "protocol_id": 0 } });
        let err = validate(value).unwrap_err();
        assert!(err.to_string().contains("has an empty path"), "got: {err}");
    }

    #[test]
    fn empty_namespace_packet_is_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({ ":foo": { "protocol_id": 0 } });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("has an empty namespace"),
            "got: {err}"
        );
    }

    #[test]
    fn invalid_ident_packet_is_rejected() {
        // A path whose name cannot become a Rust identifier (here the hyphen is
        // not an accepted separator) must fail validation, not emit uncompilable
        // code.
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({ "minecraft:a-b": { "protocol_id": 0 } });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string()
                .contains("does not map to a valid Rust identifier"),
            "got: {err}"
        );
    }

    #[test]
    fn colliding_variants_are_rejected() {
        // `minecraft:a/b` and `minecraft:a_b` both camel-case to `AB`; the
        // generated enum would not compile, so validation must fail.
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({
            "minecraft:a/b": { "protocol_id": 0 },
            "minecraft:a_b": { "protocol_id": 1 }
        });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("collide on Rust variant"),
            "got: {err}"
        );
    }

    #[test]
    fn missing_protocol_id_is_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] = serde_json::json!({ "minecraft:a": {} });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("missing `protocol_id`"),
            "got: {err}"
        );
    }

    #[test]
    fn unexpected_entry_fields_are_rejected() {
        let mut value = minimal_report();
        value["play"]["serverbound"] =
            serde_json::json!({ "minecraft:a": { "protocol_id": 0, "extra": 1 } });
        let err = validate(value).unwrap_err();
        assert!(err.to_string().contains("unexpected fields"), "got: {err}");
    }

    #[test]
    fn duplicate_packet_name_is_rejected() {
        let json = r#"{
            "handshake": {},
            "play": {
                "serverbound": {
                    "minecraft:accept_teleportation": { "protocol_id": 0 },
                    "minecraft:accept_teleportation": { "protocol_id": 1 }
                }
            },
            "status": {},
            "login": {},
            "configuration": {}
        }"#;
        let err = parse_strict(json).unwrap_err();
        assert!(
            err.to_string().contains("duplicate object key"),
            "got: {err}"
        );
    }

    #[test]
    fn packet_variants_are_upper_camel() {
        assert_eq!(packet_variant("minecraft:add_entity"), "AddEntity");
        assert_eq!(
            packet_variant("minecraft:debug/block_value"),
            "DebugBlockValue"
        );
        assert_eq!(
            packet_variant("minecraft:custom_query_answer"),
            "CustomQueryAnswer"
        );
        assert_eq!(camel("clientbound"), "Clientbound");
        assert_eq!(camel("handshake"), "Handshake");
    }

    #[test]
    fn provenance_mismatch_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let packets = tmp.path().join("packets.json");
        fs::write(&packets, b"{}").unwrap();
        fs::write(
            tmp.path().join("manifest.json"),
            r#"{
                "format": 1,
                "generator": "net.minecraft.data.Main --reports",
                "source": {
                    "jar": "x",
                    "jar_sha256": "ab",
                    "minecraft_version": "26.2",
                    "protocol_version": 776,
                    "world_version": 4903
                },
                "reports": [{ "path": "packets.json", "bytes": 2, "sha256": "deadbeef" }]
            }"#,
        )
        .unwrap();
        let err = load_provenance(&packets).unwrap_err();
        assert!(
            err.to_string()
                .contains("does not match the provenance manifest"),
            "got: {err}"
        );
    }

    #[test]
    fn provenance_matches_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = b"{\"a\":1}";
        let sha = crate::reports::sha256_hex(bytes);
        let packets = tmp.path().join("packets.json");
        fs::write(&packets, bytes).unwrap();
        fs::write(
            tmp.path().join("manifest.json"),
            format!(
                r#"{{
                    "format": 1,
                    "generator": "net.minecraft.data.Main --reports",
                    "source": {{
                        "jar": "x",
                        "jar_sha256": "e1a027e9481a16ec1da0f0e139d370280050d123a14c022a476c2dc8a697ebda",
                        "paper_git": "0a993450f129c4942c2a9ed45ba047412b4667cf",
                        "minecraft_version": "26.2",
                        "protocol_version": 776,
                        "world_version": 4903
                    }},
                    "reports": [{{ "path": "packets.json", "bytes": 7, "sha256": "{sha}" }}]
                }}"#
            ),
        )
        .unwrap();
        let provenance = load_provenance(&packets).unwrap();
        assert_eq!(provenance.minecraft_version, "26.2");
        assert_eq!(provenance.protocol_version, 776);
    }

    #[test]
    fn manifest_source_pin_mismatch_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = b"{\"a\":1}";
        let packets = tmp.path().join("packets.json");
        fs::write(&packets, bytes).unwrap();
        let sha = crate::reports::sha256_hex(bytes);
        fs::write(
            tmp.path().join("manifest.json"),
            format!(
                r#"{{
                    "format": 1,
                    "generator": "net.minecraft.data.Main --reports",
                    "source": {{
                        "jar": "x",
                        "jar_sha256": "deadbeef",
                        "paper_git": "0a993450f129c4942c2a9ed45ba047412b4667cf",
                        "minecraft_version": "26.2",
                        "protocol_version": 776,
                        "world_version": 4903
                    }},
                    "reports": [{{ "path": "packets.json", "bytes": 7, "sha256": "{sha}" }}]
                }}"#
            ),
        )
        .unwrap();
        let error = load_provenance(&packets).unwrap_err();
        assert!(error.to_string().contains("source SHA"), "got: {error}");
    }

    #[test]
    fn committed_fixture_links_to_provenance() {
        // The pinned fixture's provenance must resolve (sibling manifest + sha256
        // match), proving the committed packets.json is exactly the artifact the
        // manifest records.
        let repo_root = crate::extract::find_repo_root().unwrap();
        let provenance = load_provenance(&default_input(&repo_root)).unwrap();
        assert_eq!(provenance.minecraft_version, "26.2");
        assert_eq!(provenance.protocol_version, 776);
        assert_eq!(provenance.world_version, 4903);
        assert_eq!(provenance.jar_sha256.len(), 64);
    }

    #[test]
    fn committed_fixture_parses_and_matches_totals() {
        // The real pinned fixture must always parse + validate (a drift in the
        // datagen shape would surface here as a hard failure).
        let repo_root = crate::extract::find_repo_root().unwrap();
        let json = fs::read_to_string(default_input(&repo_root)).unwrap();
        let value = parse_strict(&json).unwrap();
        let states = validate(value).unwrap();

        let play = states.iter().find(|s| s.id == "play").unwrap();
        let clientbound = play.flows.iter().find(|f| f.id == "clientbound").unwrap();
        assert_eq!(clientbound.packets.len(), 141);
        // addPacket order (sorted by protocol_id): bundle_delimiter is the id-0
        // boundary packet, add_entity the first body packet.
        assert_eq!(clientbound.packets[0].name, "minecraft:bundle_delimiter");
        assert_eq!(clientbound.packets[0].protocol_id, 0);
        assert_eq!(clientbound.packets[1].name, "minecraft:add_entity");
        assert_eq!(clientbound.packets[1].protocol_id, 1);

        let total: usize = states
            .iter()
            .flat_map(|s| &s.flows)
            .map(|f| f.packets.len())
            .sum();
        assert_eq!(total, 256);
    }
}

#[cfg(test)]
mod drift_tests {
    use std::fs;
    use std::process::Command;

    use super::run;

    const GENERATED_FILES: [&str; 3] = ["mod.rs", "packets.rs", "protocol.rs"];

    /// The committed `crates/rivet-protocol/src/generated/` is the golden
    /// artifact. Regenerate to a temp dir, rustfmt the temp copy (`phf` map
    /// output is not format-clean as emitted), and assert byte-equality with
    /// what is committed — without mutating repository source.
    #[test]
    fn generated_packets_match_committed() {
        let repo_root = crate::extract::find_repo_root().unwrap();
        let committed = repo_root.join("crates/rivet-protocol/src/generated");
        let tmp = tempfile::tempdir().unwrap();

        run(None, Some(tmp.path())).unwrap();

        let files: Vec<_> = GENERATED_FILES.iter().map(|f| tmp.path().join(f)).collect();
        let status = Command::new("rustfmt")
            .args(["--edition", "2024"])
            .args(&files)
            .status()
            .expect("failed to run rustfmt");
        assert!(
            status.success(),
            "rustfmt failed on freshly generated output"
        );

        let mut committed_files: Vec<String> = fs::read_dir(&committed)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        committed_files.sort();
        assert_eq!(
            committed_files, GENERATED_FILES,
            "committed protocol src/generated/ has unexpected files"
        );

        for name in GENERATED_FILES {
            let generated = fs::read(tmp.path().join(name)).unwrap();
            let wanted = fs::read(committed.join(name)).unwrap();
            assert_eq!(
                generated, wanted,
                "generated output for {name} drifted from the committed golden copy; \
                 run `tools/rivet-codegen generate` and commit the result"
            );
        }
    }
}
