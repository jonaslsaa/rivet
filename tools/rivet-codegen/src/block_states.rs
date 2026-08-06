//! `rivet-codegen generate` block-state half — consume the canonical pinned
//! vanilla `data/reports/blocks.json` (`BlockListReport`, see [`crate::reports`])
//! and emit the dense global block-state id table into
//! `crates/rivet-registry/src/generated/block_states.rs` (issue #154).
//!
//! # Ground truth
//!
//! `blocks.json` is the report-driven capture of Paper's
//! `Block.BLOCK_STATE_REGISTRY`: every state carries its real registry id
//! (0..32365, dense and contiguous), each block names its `default` state, and
//! the `properties` field holds each property's `getPossibleValues()` order.
//! `GlobalPalette`'s `idFor`/`valueFor` delegate straight to that registry, so
//! the report's per-state `id` **is** the wire global palette index — there is
//! no separate `GlobalPalette` numbering. The generator therefore pins the
//! emitted ids to the report rather than inferring them from lexical/registry
//! order.
//!
//! The block *registry* ids (0..1195, the index into `BLOCK_BY_ID`) come from
//! the extract artifact `data/block_states.json` (a cross-test pins them to
//! `registries.json`'s `minecraft:block` surface). `block_states.json` is also
//! the shape/value-order source for the emitted file's mixed-radix arithmetic:
//! the forward/reverse functions reuse the already-generated
//! `BLOCK_STATE_SHAPES` + `BLOCK_PROPERTY_VALUES` tables, so no per-block
//! property table is duplicated here.
//!
//! # Id algorithm (verified against the report)
//!
//! `Blocks.java` iterates `BuiltInRegistries.BLOCK` in registry-id order and
//! adds every state of each block to `BLOCK_STATE_REGISTRY`, so ids are
//! assigned block by block and each block occupies the contiguous range
//! `[base, base + count)`. Within a block, `StateDefinition` orders states by
//! the mixed-radix Cartesian product over properties **sorted by name**
//! (`ImmutableSortedMap`), the last property varying fastest; a property's
//! digit is its position in `getPossibleValues()`. The generator re-derives
//! every local index from the report's value orders and requires it to equal
//! `id - base` for all 32366 states — the oracle-conformance check that makes
//! a silent reordering (or a wrong property/value source) fail generation.
//!
//! Determinism: entries are emitted in block registry-id order; the committed
//! file is the golden artifact asserted by `generate::drift_tests`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::model::{BlockDef, BlockRegistry};
use crate::registries::{load_provenance, parse_strict, validate_name};
use crate::reports::SourceProvenance;

/// The canonical pinned block-state report (`BlockListReport`).
pub fn default_input(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/reports/blocks.json")
}

/// Tables are written into the same committed `generated/` dir as the block
/// tables (the golden drift test in [`crate::generate`] asserts that dir
/// contains exactly the generated files).
pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("crates/rivet-registry/src/generated")
}

/// One validated block's state-range anchor (all fields fit `u16`: block ids
/// are 0..1195, state ids 0..32365, and every per-block range is < 65535).
#[derive(Debug)]
pub struct BlockEntry {
    /// `namespace:path`, e.g. `"minecraft:acacia_button"`.
    name: String,
    /// Block registry id (index into `BLOCK_BY_ID`).
    block_id: u16,
    /// Global id of the block's first (lowest) state.
    base: u16,
    /// Number of states the block occupies (`[base, base + count)`).
    count: u16,
    /// Global id of the block's default state (the report's `default` marker).
    default: u16,
}

pub fn run(
    blocks_input_flag: Option<&Path>,
    states_input_flag: Option<&Path>,
    output_flag: Option<&Path>,
) -> Result<()> {
    let repo_root = crate::extract::find_repo_root()?;
    let blocks_input = match blocks_input_flag {
        Some(p) => p.to_path_buf(),
        None => default_input(&repo_root),
    };
    let states_input = match states_input_flag {
        Some(p) => p.to_path_buf(),
        None => crate::generate::default_input(&repo_root),
    };
    let output = match output_flag {
        Some(p) => p.to_path_buf(),
        None => default_output(&repo_root),
    };

    let json = fs::read_to_string(&blocks_input)
        .with_context(|| format!("read {}", blocks_input.display()))?;
    let root = parse_strict(&json).with_context(|| format!("parse {}", blocks_input.display()))?;
    let states_json = fs::read_to_string(&states_input)
        .with_context(|| format!("read {}", states_input.display()))?;
    let registry: BlockRegistry = serde_json::from_str(&states_json)
        .with_context(|| format!("parse {}", states_input.display()))?;

    // Validate structurally first: a malformed/sparse fixture fails fast on its
    // own, before the provenance link pulls in the sibling manifest.
    let entries = validate(root, &registry)?;
    let source = load_provenance(&blocks_input, "blocks.json")?;

    fs::create_dir_all(&output).with_context(|| format!("create {}", output.display()))?;
    fs::write(output.join("block_states.rs"), render(&entries, &source))
        .context("write generated/block_states.rs")?;

    let total: u32 = entries.iter().map(|b| b.count as u32).sum();
    println!(
        "Wrote {} global block-state ids across {} blocks -> {}",
        total,
        entries.len(),
        output.display()
    );
    Ok(())
}

/// Structural + oracle-conformance validation of `blocks.json` against the
/// extract's block registry. Fails on: a malformed block/state object or
/// property name, a non-integer/negative/overflowing state id, a sparse or
/// non-contiguous id space (per block and globally), duplicate state ids,
/// overlapping block ranges, a global id space that does not anchor at 0 or
/// densely partition `0..total-1`, zero or multiple `default` markers, a
/// default outside its block's range, a block missing from or extra in
/// `block_states.json`, a non-alphabetical declaration-order property list, a
/// value-order mismatch between the report and the extract, a state property
/// set that is not exactly the block's, and any local index that does not
/// re-derive the report's own id.
fn validate(root: Value, registry: &BlockRegistry) -> Result<Vec<BlockEntry>> {
    let object = root
        .as_object()
        .context("blocks.json root must be a JSON object")?;

    // The block registry (extract artifact) must enumerate exactly the same
    // blocks — a stale/mismatched pair of fixtures is a bug, not a drift to
    // paper over.
    let id_by_name: HashMap<&str, u16> = registry
        .blocks
        .iter()
        .map(|b| (b.name.as_str(), b.id))
        .collect();
    let mut entries = Vec::with_capacity(object.len());
    for (name, block_value) in object {
        validate_name("blocks.json", name)?;
        let block_id = id_by_name
            .get(name.as_str())
            .with_context(|| format!("block `{name}` is missing from block_states.json"))?;
        let def = registry
            .blocks
            .iter()
            .find(|b| b.name == *name)
            .expect("name present in id_by_name");
        let entry = parse_block(name, block_value, *block_id, def)?;
        // The oracle-conformance check lives in parse_block: it re-derives
        // every state's local index and requires it to equal `id - base`.
        entries.push(entry);
    }
    // The reverse direction: every extract block must also appear in the
    // report (a stale/partial fixture pair is a bug, not a drift to ignore).
    for block in &registry.blocks {
        if !object.contains_key(block.name.as_str()) {
            bail!(
                "block `{}` in block_states.json is missing from blocks.json",
                block.name
            );
        }
    }

    // Emit in block registry-id order so `BLOCK_STATE_BASES`'s id == index
    // invariant holds by construction (mirrors render_blocks).
    entries.sort_unstable_by_key(|e| e.block_id);

    // Block ids must be dense 0..n for the id-indexed array.
    for (i, e) in entries.iter().enumerate() {
        if e.block_id as usize != i {
            bail!(
                "block registry ids are not contiguous (expected {} at index {i})",
                e.block_id
            );
        }
    }

    // The global id space must be a dense partition of 0..total-1: blocks are
    // assigned states block after block in registry order, so each block's
    // range starts exactly where the previous one ended. Two invariants cover
    // this: bases strictly increasing, AND adjacent ranges disjoint (no
    // overlap). Bases increasing alone is not enough — a pair of blocks can
    // both claim the same ids (e.g. `[100, 110)` and `[105, 115)` share
    // 105..110) while their bases still ascend. Together with the last range
    // ending at the count-total, disjointness forces a full partition of
    // `[0, total)`: the ranges are disjoint, have `sum(counts) = total`
    // elements, and all lie inside `[first_base, total)`, so `first_base` must
    // be 0 and no gap can remain.
    for pair in entries.windows(2) {
        if pair[0].base >= pair[1].base {
            bail!(
                "block-state bases are not strictly increasing: `{}` starts at {} and `{}` at {}",
                pair[0].name,
                pair[0].base,
                pair[1].name,
                pair[1].base
            );
        }
        if pair[0].base as u32 + pair[0].count as u32 > pair[1].base as u32 {
            bail!(
                "block-state ranges overlap: `{}` spans [{}, {}) and `{}` spans [{}, {})",
                pair[0].name,
                pair[0].base,
                pair[0].base as u32 + pair[0].count as u32,
                pair[1].name,
                pair[1].base,
                pair[1].base as u32 + pair[1].count as u32
            );
        }
    }
    let total: u32 = entries.iter().map(|b| b.count as u32).sum();
    let last = entries.last().expect("blocks.json is non-empty");
    if last.base as u32 + last.count as u32 != total {
        bail!(
            "global block-state ids are not dense 0..{total}: last block `{}` ends at {}",
            last.name,
            last.base as u32 + last.count as u32
        );
    }
    // State ids must fit the emitted `u16` `StateId`.
    u16::try_from(total).with_context(|| {
        format!("global state count {total} does not fit the emitted u16 StateId")
    })?;

    Ok(entries)
}

fn parse_block(name: &str, value: &Value, block_id: u16, def: &BlockDef) -> Result<BlockEntry> {
    let obj = value
        .as_object()
        .with_context(|| format!("block `{name}` in blocks.json must be a JSON object"))?;

    // Reject unknown per-block fields so a fixture change (e.g. Gson adding a
    // new property) fails loudly instead of being ignored.
    for field in obj.keys() {
        if !matches!(field.as_str(), "definition" | "properties" | "states") {
            bail!("block `{name}` has unexpected field `{field}`");
        }
    }
    let definition = obj
        .get("definition")
        .with_context(|| format!("block `{name}` is missing `definition`"))?;
    if !definition.is_object() {
        bail!("block `{name}` `definition` must be a JSON object");
    }

    // Property digit spaces from the report. `properties` is absent for the
    // 399 single-state blocks. The JSON key order is the Gson writer order (not
    // the state-computation order — e.g. chest writes `type,facing,waterlogged`),
    // so names are re-sorted alphabetically to recover Java's
    // `ImmutableSortedMap` order. Values keep the report's order verbatim.
    let mut properties: Vec<(String, Vec<String>)> = match obj.get("properties") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Object(props)) => {
            let mut out = Vec::with_capacity(props.len());
            for (prop_name, values) in props {
                // Vanilla property names are `[a-z0-9_]+` (e.g. `waterlogged`,
                // `powered`); a name Java could never have is a fixture bug.
                if prop_name.is_empty()
                    || !prop_name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                {
                    bail!("block `{name}` has an invalid property name `{prop_name}`");
                }
                let values = values.as_array().with_context(|| {
                    format!("block `{name}` property `{prop_name}` values must be an array")
                })?;
                if values.is_empty() {
                    bail!("block `{name}` property `{prop_name}` has an empty value list");
                }
                let mut list = Vec::with_capacity(values.len());
                for (i, v) in values.iter().enumerate() {
                    let v = v.as_str().with_context(|| {
                        format!("block `{name}` property `{prop_name}` value {i} is not a string")
                    })?;
                    if v.is_empty() {
                        bail!("block `{name}` property `{prop_name}` value {i} is empty");
                    }
                    list.push(v.to_string());
                }
                out.push((prop_name.clone(), list));
            }
            out
        }
        Some(_) => bail!("block `{name}` `properties` must be a JSON object"),
    };
    properties.sort_by(|a, b| a.0.cmp(&b.0));

    // The extract's declaration-order property list must equal the sorted
    // report names, and its value orders must equal the report's — the emitted
    // file's mixed-radix reuses BLOCK_STATE_SHAPES (generated from the extract),
    // so any divergence would silently compute ids the report never assigned.
    let def_names: Vec<&str> = def.properties.iter().map(|p| p.name.as_str()).collect();
    let report_names: Vec<&str> = properties.iter().map(|(n, _)| n.as_str()).collect();
    if def_names != report_names {
        bail!(
            "block `{name}` property order/names differ between block_states.json ({}) and the sorted report ({})",
            def_names.join(","),
            report_names.join(",")
        );
    }
    for (prop, values) in &properties {
        let extract_values = def
            .properties
            .iter()
            .find(|p| p.name == *prop)
            .map(|p| p.values.iter().map(String::as_str).collect::<Vec<_>>())
            .expect("name matched above");
        if extract_values != values.iter().map(String::as_str).collect::<Vec<_>>() {
            bail!(
                "block `{name}` property `{prop}` value order differs between block_states.json and blocks.json"
            );
        }
    }

    let states = obj
        .get("states")
        .with_context(|| format!("block `{name}` is missing `states`"))?
        .as_array()
        .with_context(|| format!("block `{name}` `states` must be an array"))?;
    if states.is_empty() {
        bail!("block `{name}` has no states");
    }

    // Each state: an integer `id`, an optional `default` flag, and a
    // `properties` object that must name exactly the block's properties.
    let mut ids: Vec<u16> = Vec::with_capacity(states.len());
    let mut default: Option<u16> = None;
    for (i, state) in states.iter().enumerate() {
        let state_obj = state
            .as_object()
            .with_context(|| format!("block `{name}` state {i} must be a JSON object"))?;
        for field in state_obj.keys() {
            if !matches!(field.as_str(), "id" | "default" | "properties") {
                bail!("block `{name}` state {i} has unexpected field `{field}`");
            }
        }
        let id = parse_state_id(name, i, state_obj)?;
        ids.push(id);
        match state_obj.get("default") {
            None | Some(Value::Null) => {}
            Some(Value::Bool(true)) => {
                if default.is_some() {
                    bail!("block `{name}` has more than one default state");
                }
                default = Some(id);
            }
            Some(Value::Bool(false)) => {}
            Some(_) => bail!("block `{name}` state {i} has a non-boolean `default`"),
        }
        // `properties` is present on every state of a multi-state block and
        // omitted on single-state blocks (which have no properties at all).
        let mut state_names: Vec<&str> = Vec::new();
        match state_obj.get("properties") {
            Some(Value::Object(props)) => {
                state_names = props.keys().map(String::as_str).collect();
            }
            Some(_) => bail!("block `{name}` state {i} `properties` must be a JSON object"),
            None => {
                if !report_names.is_empty() {
                    bail!("block `{name}` state {i} is missing `properties`");
                }
            }
        }
        state_names.sort_unstable();
        if state_names != report_names {
            bail!(
                "block `{name}` state {i} property names ({}) do not match the block's ({})",
                state_names.join(","),
                report_names.join(",")
            );
        }
    }

    // Per-block range: ascending + contiguous (`base..base + count`).
    for pair in ids.windows(2) {
        if pair[0] >= pair[1] {
            bail!(
                "block `{name}` state ids are not strictly ascending ({} then {})",
                pair[0],
                pair[1]
            );
        }
    }
    let base = ids[0];
    let count = u16::try_from(ids.len())
        .with_context(|| format!("block `{name}` has too many states for u16"))?;
    for (i, id) in ids.iter().enumerate() {
        if *id != base + i as u16 {
            bail!(
                "block `{name}` state ids are not contiguous (expected {} at offset {i}, got {id})",
                base + i as u16
            );
        }
    }
    let default = default.with_context(|| format!("block `{name}` has no default state"))?;
    if !(base..base + count).contains(&default) {
        bail!(
            "block `{name}` default state {default} is outside its id range [{base}, {})",
            base + count
        );
    }

    // Oracle-conformance: re-derive each state's local index from the report's
    // (sorted, reversed) mixed-radix and require it to equal `id - base`. This
    // is what pins the emitted ids to Paper's actual numbering — a reordered
    // value list or a wrong property source makes this fail.
    for state in states {
        let id = state["id"].as_u64().expect("validated above") as u16;
        let local = local_index(&properties, state)
            .with_context(|| format!("block `{name}` state {id}"))?;
        if local != id - base {
            bail!(
                "block `{name}` state {id}: mixed-radix re-derives local index {local}, expected {} — \
                 the property order/value order does not reproduce the report's id",
                id - base
            );
        }
    }

    Ok(BlockEntry {
        name: name.to_string(),
        block_id,
        base,
        count,
        default,
    })
}

/// Parse and range-check a state's `id`. The emitted table stores ids as `u16`
/// (max 32365), so anything beyond that fails here rather than truncating.
fn parse_state_id(
    block: &str,
    index: usize,
    state: &serde_json::Map<String, Value>,
) -> Result<u16> {
    let id = state
        .get("id")
        .with_context(|| format!("block `{block}` state {index} is missing `id`"))?;
    let id = match id {
        Value::Number(n) => {
            if let Some(id) = n.as_u64() {
                id
            } else if n.as_i64().is_some_and(|i| i < 0) {
                bail!("block `{block}` state {index} has a negative `id` ({n})");
            } else {
                bail!("block `{block}` state {index} has a non-integer `id`");
            }
        }
        _ => bail!("block `{block}` state {index} has a non-numeric `id`"),
    };
    u16::try_from(id).with_context(|| {
        format!(
            "block `{block}` state {index} has an `id` outside the u16 range (state ids are u16)"
        )
    })
}

/// The local index of a state within its block, using Java's `StateDefinition`
/// ordering: properties sorted by name, the last property varying fastest
/// (mixed-radix with the first property as the most significant digit).
fn local_index(properties: &[(String, Vec<String>)], state: &Value) -> Result<u16> {
    let mut local: u32 = 0;
    let mut stride: u32 = 1;
    for (name, values) in properties.iter().rev() {
        let value = state["properties"][name]
            .as_str()
            .with_context(|| format!("state {} is missing property `{name}`", state["id"]))?;
        let digit = values.iter().position(|v| v == value).with_context(|| {
            format!("value `{value}` for property `{name}` is not in the block's value list")
        })?;
        local += digit as u32 * stride;
        stride *= values.len() as u32;
    }
    u16::try_from(local)
        .with_context(|| format!("state {} local index {local} does not fit u16", state["id"]))
}

/// Bits needed to index every global id on the wire: `ceillog2(count)`.
/// 32366 -> 15, the global palette's in-memory width (`globalMap.size()` ->
/// `GlobalPalette.globalPaletteBitsInMemory`).
fn global_palette_bits(count: u32) -> u8 {
    (count as u64).next_power_of_two().trailing_zeros() as u8
}

/// Render `generated/block_states.rs`.
fn render(entries: &[BlockEntry], source: &SourceProvenance) -> String {
    let total: u32 = entries.iter().map(|b| b.count as u32).sum();
    let bits = global_palette_bits(total);
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by `tools/rivet-codegen generate` from data/reports/blocks.json\n\
         // (vanilla BlockListReport; MC {}, protocol {}, world {}).\n\
         // Source jar sha256 {}; provenance linked to data/reports/manifest.json.\n\
         // Do not edit by hand — PORTING.md: registries/data are generated, not hand-ported.\n\n",
        source.minecraft_version,
        source.protocol_version,
        source.world_version,
        source.jar_sha256.get(..16).unwrap_or(&source.jar_sha256)
    ));
    out.push_str(
        "// Dense global block-state ids — Paper's `Block.BLOCK_STATE_REGISTRY` order, which\n\
         // `GlobalPalette`/`PalettedContainer` use directly as the wire global palette index.\n\
         // Each block occupies the contiguous range `[base, base + count)`;\n\
         // ids are assigned block by block in registry order, and within a block by the\n\
         // `StateDefinition` mixed-radix product (properties sorted by name, last property\n\
         // varying fastest). The forward/reverse arithmetic reuses BLOCK_STATE_SHAPES +\n\
         // BLOCK_PROPERTY_VALUES (block_properties.rs) and is cross-checked against this\n\
         // report by the oracle-conformance test in tools/rivet-codegen.\n\n",
    );
    out.push_str(
        "use crate::generated::block_properties::{BLOCK_PROPERTY_VALUES, BLOCK_STATE_SHAPES};\n",
    );
    out.push_str("use crate::generated::blocks::BlockId;\n\n");

    out.push_str(&format!(
        "/// Total number of global block-state ids (`Block.BLOCK_STATE_REGISTRY` size — the\n\
         /// global palette's `globalMap`).\n\
         pub const BLOCK_STATE_COUNT: u16 = {total};\n\n"
    ));
    out.push_str(&format!(
        "/// Bits needed to index every global block-state id on the wire\n\
         /// (`ceillog2(BLOCK_STATE_COUNT)` = {bits}) — the global palette's in-memory width.\n\
         pub const GLOBAL_PALETTE_BITS: u8 = {bits};\n\n"
    ));

    out.push_str(
        "/// A dense global block-state id (index into the global palette, `0..BLOCK_STATE_COUNT`).\n\
         #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]\n\
         pub struct StateId(pub u16);\n\n",
    );
    out.push_str(
        "/// Per-block anchor into the global id space.\n\
         #[derive(Clone, Copy, Debug)]\n\
         pub struct BlockStateBase {\n\
         \x20   /// Global id of the block's first (lowest) state.\n\
         \x20   pub base: u16,\n\
         \x20   /// Number of states the block occupies (`[base, base + count)`).\n\
         \x20   pub count: u16,\n\
         \x20   /// Global id of the block's default state (the report's `default` marker).\n\
         \x20   pub default: u16,\n\
         }\n\n",
    );
    out.push_str(
        "/// State anchors indexed by block registry id (id == index, matches `BLOCK_BY_ID`).\n\
         /// Bases are strictly increasing, so `block_of` binary-searches.\n\
         pub static BLOCK_STATE_BASES: &[BlockStateBase] = &[\n",
    );
    for e in entries {
        out.push_str(&format!(
            "    BlockStateBase {{ base: {}, count: {}, default: {} }}, // {}\n",
            e.base, e.count, e.default, e.name
        ));
    }
    out.push_str("];\n\n");

    out.push_str(
        "/// The block's property shape (property ids into `BLOCK_PROPERTY_VALUES`, in\n\
         /// declaration order), or `&[]` for a single-state block.\n\
         pub fn shape_of(block: BlockId) -> &'static [u16] {\n\
         \x20   match BLOCK_STATE_SHAPES.binary_search_by_key(&block.0, |(b, _)| *b) {\n\
         \x20       Ok(i) => BLOCK_STATE_SHAPES[i].1,\n\
         \x20       Err(_) => &[],\n\
         \x20   }\n\
         }\n\n",
    );
    out.push_str(
        "/// Forward: block + property value indices (in the block's declaration order) ->\n\
         /// dense global id. `values.len()` must equal `shape_of(block).len()`, and each\n\
         /// value must be < its property's value count (a corrupt index would otherwise\n\
         /// silently compose an out-of-range id in release).\n\
         pub fn state_id(block: BlockId, values: &[u16]) -> StateId {\n\
         \x20   let shape = shape_of(block);\n\
         \x20   debug_assert_eq!(values.len(), shape.len());\n\
         \x20   let base = BLOCK_STATE_BASES[block.0 as usize].base;\n\
         \x20   // Mixed-radix over the shape, last property varying fastest (stride 1).\n\
         \x20   let mut local = 0u16;\n\
         \x20   let mut stride = 1u16;\n\
         \x20   for (value, prop) in values.iter().zip(shape.iter()).rev() {\n\
         \x20       local += *value * stride;\n\
         \x20       stride *= BLOCK_PROPERTY_VALUES[*prop as usize].len() as u16;\n\
         \x20   }\n\
         \x20   // Every in-range digit keeps `local < count` (mixed-radix bound); a value\n\
         \x20   // index >= its property's value count is a caller bug, caught in debug.\n\
         \x20   debug_assert!(local < BLOCK_STATE_BASES[block.0 as usize].count);\n\
         \x20   StateId(base + local)\n\
         }\n\n",
    );
    out.push_str(
        "/// Reverse: global id -> block. Ids outside `0..BLOCK_STATE_COUNT` fall back to air\n\
         /// (block 0, state 0), mirroring `Block.stateById` / the global palette's\n\
         /// missing-id behaviour.\n\
         pub fn block_of(id: StateId) -> BlockId {\n\
         \x20   let id = id.0 as u32;\n\
         \x20   // Last block whose base is <= id (bases are strictly increasing).\n\
         \x20   let idx = BLOCK_STATE_BASES.partition_point(|b| b.base as u32 <= id);\n\
         \x20   if idx == 0 {\n\
         \x20       return BlockId(0);\n\
         \x20   }\n\
         \x20   let b = &BLOCK_STATE_BASES[idx - 1];\n\
         \x20   if id < b.base as u32 + b.count as u32 {\n\
         \x20       BlockId((idx - 1) as u16)\n\
         \x20   } else {\n\
         \x20       BlockId(0)\n\
         \x20   }\n\
         }\n\n",
    );
    out.push_str(
        "/// Reverse: global id -> the owning block's property value indices, written in the\n\
         /// block's declaration order. `out.len()` must be at least `shape_of(block).len()`.\n\
         pub fn values_of(id: StateId, out: &mut [u16]) {\n\
         \x20   let block = block_of(id);\n\
         \x20   let shape = shape_of(block);\n\
         \x20   debug_assert!(out.len() >= shape.len());\n\
         \x20   let mut local = id.0 - BLOCK_STATE_BASES[block.0 as usize].base;\n\
         \x20   for (i, prop) in shape.iter().enumerate().rev() {\n\
         \x20       let size = BLOCK_PROPERTY_VALUES[*prop as usize].len() as u16;\n\
         \x20       out[i] = local % size;\n\
         \x20       local /= size;\n\
         \x20   }\n\
         }\n\n",
    );
    out.push_str(
        "/// The block's default state (the report's `default` marker).\n\
         pub fn default_state(block: BlockId) -> StateId {\n\
         \x20   StateId(BLOCK_STATE_BASES[block.0 as usize].default)\n\
         }\n\n",
    );
    out.push_str(
        "/// Whether `id` names a real block state (`id < BLOCK_STATE_COUNT`).\n\
         pub fn is_valid(id: StateId) -> bool {\n\
         \x20   id.0 < BLOCK_STATE_COUNT\n\
         }\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BlockProperty, BlockRegistry};

    /// A two-block fixture: air (single state) + stone_slab (2x2 = 4 states,
    /// base 1). The `properties` JSON key order is deliberately non-alphabetical
    /// (Gson writer order) — validation must re-sort names, not trust key order.
    fn minimal_report() -> Value {
        serde_json::json!({
            "minecraft:air": {
                "definition": { "type": "minecraft:air" },
                "states": [ { "default": true, "id": 0 } ]
            },
            "minecraft:stone_slab": {
                "definition": { "type": "minecraft:slab" },
                "properties": {
                    "waterlogged": ["false", "true"],
                    "type": ["top", "bottom"]
                },
                "states": [
                    { "id": 1, "properties": { "type": "top", "waterlogged": "false" } },
                    { "default": true, "id": 2, "properties": { "type": "top", "waterlogged": "true" } },
                    { "id": 3, "properties": { "type": "bottom", "waterlogged": "false" } },
                    { "id": 4, "properties": { "type": "bottom", "waterlogged": "true" } }
                ]
            }
        })
    }

    fn minimal_registry() -> BlockRegistry {
        BlockRegistry {
            minecraft_version: "26.2".into(),
            blocks: vec![
                BlockDef {
                    id: 0,
                    name: "minecraft:air".into(),
                    properties: vec![],
                },
                BlockDef {
                    id: 1,
                    name: "minecraft:stone_slab".into(),
                    properties: vec![
                        BlockProperty {
                            name: "type".into(),
                            values: vec!["top".into(), "bottom".into()],
                        },
                        BlockProperty {
                            name: "waterlogged".into(),
                            values: vec!["false".into(), "true".into()],
                        },
                    ],
                },
            ],
        }
    }

    fn entries_of(value: &Value) -> Vec<BlockEntry> {
        validate(value.clone(), &minimal_registry()).unwrap()
    }

    fn entry<'a>(entries: &'a [BlockEntry], name: &str) -> &'a BlockEntry {
        entries.iter().find(|e| e.name == name).unwrap()
    }

    #[test]
    fn minimal_report_validates_and_orders_by_block_id() {
        let entries = entries_of(&minimal_report());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "minecraft:air");
        assert_eq!(
            (entries[0].base, entries[0].count, entries[0].default),
            (0, 1, 0)
        );
        let slab = entry(&entries, "minecraft:stone_slab");
        assert_eq!((slab.base, slab.count, slab.default), (1, 4, 2));
        // The mixed-radix anchors re-derive the report's ids. Properties are
        // re-sorted by name regardless of the JSON key order, last property
        // varying fastest: type(top/bottom) * waterlogged(false/true), so
        // {type: bottom, waterlogged: false} is local 2 (id 3, base 1).
        let props = vec![
            (
                "type".to_string(),
                vec!["top".to_string(), "bottom".to_string()],
            ),
            (
                "waterlogged".to_string(),
                vec!["false".to_string(), "true".to_string()],
            ),
        ];
        let state = &minimal_report()["minecraft:stone_slab"]["states"][2];
        assert_eq!(local_index(&props, state).unwrap(), 2);
        let state = &minimal_report()["minecraft:stone_slab"]["states"][3];
        assert_eq!(local_index(&props, state).unwrap(), 3);
    }

    #[test]
    fn global_palette_bits_is_ceillog2() {
        assert_eq!(global_palette_bits(32366), 15);
        assert_eq!(global_palette_bits(1), 0);
        assert_eq!(global_palette_bits(8), 3);
        assert_eq!(global_palette_bits(16), 4);
        assert_eq!(global_palette_bits(16384), 14);
        assert_eq!(global_palette_bits(16385), 15);
    }

    #[test]
    fn missing_default_is_rejected() {
        let mut value = minimal_report();
        value["minecraft:air"]["states"][0]["default"] = serde_json::Value::Null;
        let err = validate(value, &minimal_registry()).unwrap_err();
        assert!(
            err.to_string().contains("has no default state"),
            "got: {err}"
        );
    }

    #[test]
    fn multiple_defaults_are_rejected() {
        let mut value = minimal_report();
        value["minecraft:stone_slab"]["states"][3]["default"] = serde_json::json!(true);
        let err = validate(value, &minimal_registry()).unwrap_err();
        assert!(
            err.to_string().contains("more than one default state"),
            "got: {err}"
        );
    }

    #[test]
    fn sparse_state_ids_are_rejected() {
        // Remove the id-2 state: stone_slab becomes 1,3,4 — a hole at 2.
        let mut value = minimal_report();
        let states = value["minecraft:stone_slab"]["states"]
            .as_array_mut()
            .unwrap();
        states.remove(1);
        let err = validate(value, &minimal_registry()).unwrap_err();
        assert!(err.to_string().contains("not contiguous"), "got: {err}");
    }

    #[test]
    fn duplicate_state_id_is_rejected() {
        let mut value = minimal_report();
        value["minecraft:stone_slab"]["states"][2]["id"] = serde_json::json!(2);
        let err = validate(value, &minimal_registry()).unwrap_err();
        assert!(
            err.to_string().contains("not strictly ascending"),
            "got: {err}"
        );
    }

    #[test]
    fn non_alphabetical_extract_property_order_is_rejected() {
        // The extract's declaration order must equal the sorted report names;
        // a non-alphabetical extract order would break the mixed-radix reuse.
        let mut registry = minimal_registry();
        registry.blocks[1].properties.reverse();
        let err = validate(minimal_report(), &registry).unwrap_err();
        assert!(
            err.to_string().contains("property order/names differ"),
            "got: {err}"
        );
    }

    #[test]
    fn extract_value_order_mismatch_is_rejected() {
        let mut registry = minimal_registry();
        registry.blocks[1].properties[1].values.reverse();
        let err = validate(minimal_report(), &registry).unwrap_err();
        assert!(
            err.to_string().contains("value order differs"),
            "got: {err}"
        );
    }

    #[test]
    fn unknown_block_field_is_rejected() {
        let mut value = minimal_report();
        value["minecraft:air"]["extra"] = serde_json::json!(1);
        let err = validate(value, &minimal_registry()).unwrap_err();
        assert!(
            err.to_string().contains("unexpected field `extra`"),
            "got: {err}"
        );
    }

    #[test]
    fn block_missing_from_extract_is_rejected() {
        let mut value = minimal_report();
        value.as_object_mut().unwrap().insert(
            "minecraft:dirt".into(),
            serde_json::json!({
                "definition": { "type": "minecraft:block" },
                "states": [ { "default": true, "id": 5 } ]
            }),
        );
        let err = validate(value, &minimal_registry()).unwrap_err();
        assert!(
            err.to_string().contains("missing from block_states.json"),
            "got: {err}"
        );
    }

    #[test]
    fn state_property_set_mismatch_is_rejected() {
        // State 1 of stone_slab drops `waterlogged`.
        let mut value = minimal_report();
        value["minecraft:stone_slab"]["states"][0]["properties"] =
            serde_json::json!({ "type": "top" });
        let err = validate(value, &minimal_registry()).unwrap_err();
        assert!(
            err.to_string().contains("do not match the block's"),
            "got: {err}"
        );
    }

    #[test]
    fn state_value_outside_block_list_is_rejected() {
        let mut value = minimal_report();
        value["minecraft:stone_slab"]["states"][0]["properties"]["type"] =
            serde_json::json!("diagonal");
        let err = validate(value, &minimal_registry()).unwrap_err();
        assert!(
            format!("{err:#}").contains("is not in the block's value list"),
            "got: {err:#}"
        );
    }

    #[test]
    fn rendering_is_deterministic_and_carries_provenance() {
        let source: SourceProvenance = serde_json::from_str(
            r#"{"jar":"paper-26.2.jar","jar_sha256":"e1a027e9481a16ec1da0f0e139d370280050d123a14c022a476c2dc8a697ebda","minecraft_version":"26.2","protocol_version":776,"world_version":4903}"#,
        )
        .unwrap();
        let entries = entries_of(&minimal_report());
        let first = render(&entries, &source);
        let second = render(&entries, &source);
        assert_eq!(first, second);
        assert!(first.contains("BlockListReport; MC 26.2, protocol 776, world 4903"));
        assert!(first.contains("e1a027e9481a16ec"));
        assert!(first.contains("BLOCK_STATE_COUNT: u16 = 5"));
        assert!(first.contains("GLOBAL_PALETTE_BITS: u8 = 3"));
        assert!(first.contains("BlockStateBase { base: 1, count: 4, default: 2 }"));
    }

    // ---- Oracle-conformance + mutation tests against the real fixtures ----

    fn real_fixtures() -> (Value, BlockRegistry) {
        let repo_root = crate::extract::find_repo_root().unwrap();
        let blocks = fs::read_to_string(default_input(&repo_root)).unwrap();
        let states = fs::read_to_string(crate::generate::default_input(&repo_root)).unwrap();
        (
            parse_strict(&blocks).unwrap(),
            serde_json::from_str(&states).unwrap(),
        )
    }

    /// The whole report must validate: all 32366 states re-derive the report's
    /// own ids (the oracle-conformance check), the global id space is dense
    /// 0..32365, and every default sits inside its block's range.
    #[test]
    fn oracle_conformance_all_32366_states_match_report() {
        let (root, registry) = real_fixtures();
        let entries = validate(root, &registry).unwrap();
        let total: u32 = entries.iter().map(|b| b.count as u32).sum();
        assert_eq!(total, 32366);
        assert_eq!(entries.len(), 1196);
        assert_eq!(global_palette_bits(total), 15);
        assert_eq!(entries[0].base, 0);
        assert_eq!(entries[0].default, 0);
        // One default per block, all inside their ranges.
        for e in &entries {
            assert!(
                (e.base..e.base + e.count).contains(&e.default),
                "default {} out of range for {}",
                e.default,
                e.name
            );
        }
        // Representative report anchors (Paper ground truth): acacia_button's
        // default (wall/north/false) is 10780, redstone_wire spans 4011..5306.
        let button = entries
            .iter()
            .find(|e| e.name == "minecraft:acacia_button")
            .unwrap();
        assert_eq!(
            (button.base, button.count, button.default),
            (10771, 24, 10780)
        );
        let wire = entries
            .iter()
            .find(|e| e.name == "minecraft:redstone_wire")
            .unwrap();
        assert_eq!((wire.base, wire.count), (4011, 1296));
    }

    /// Flipping a property's value order in the report must fail generation —
    /// the report/extract value-order cross-check catches the silent reorder
    /// rather than emitting wrong ids.
    #[test]
    fn property_value_order_flip_fails() {
        let (mut root, registry) = real_fixtures();
        let props = root["minecraft:redstone_wire"]["properties"]["power"]
            .as_array_mut()
            .unwrap();
        props.reverse();
        let err = validate(root, &registry).unwrap_err();
        assert!(
            format!("{err:#}").contains("value order differs"),
            "got: {err:#}"
        );
    }

    /// Flipping a value order in the *extract* (the shape source) must also
    /// fail — the emitted file reuses the extract's BLOCK_PROPERTY_VALUES, so a
    /// divergence from the report is a hard error.
    #[test]
    fn extract_value_order_flip_fails() {
        let (root, mut registry) = real_fixtures();
        let wire = registry
            .blocks
            .iter_mut()
            .find(|b| b.name == "minecraft:redstone_wire")
            .unwrap();
        let power = wire
            .properties
            .iter_mut()
            .find(|p| p.name == "power")
            .unwrap();
        power.values.reverse();
        let err = validate(root, &registry).unwrap_err();
        assert!(
            format!("{err:#}").contains("value order differs"),
            "got: {err:#}"
        );
    }

    /// Flip a value order in *both* sources identically (so the report/extract
    /// cross-check passes) — the emitted ids must still fail to re-derive the
    /// report's own ids. This is the oracle-conformance check in isolation.
    #[test]
    fn reordered_enumeration_breaks_oracle_conformance() {
        let (mut root, mut registry) = real_fixtures();
        // Flip the report's digit space and the extract's value list together.
        root["minecraft:redstone_wire"]["properties"]["power"]
            .as_array_mut()
            .unwrap()
            .reverse();
        let wire = registry
            .blocks
            .iter_mut()
            .find(|b| b.name == "minecraft:redstone_wire")
            .unwrap();
        let p = wire
            .properties
            .iter_mut()
            .find(|p| p.name == "power")
            .unwrap();
        p.values.reverse();
        let err = validate(root, &registry).unwrap_err();
        assert!(
            format!("{err:#}").contains("does not reproduce the report's id"),
            "got: {err:#}"
        );
    }

    /// Shifting one block's ids down so its range overlaps the previous block's
    /// must fail generation. This is the invariant the "bases strictly
    /// increasing" check alone does not cover: `jungle_button [10747,10771)` +
    /// `acacia_button [10769,10793)` both ascend and yet claim ids 10769..10770
    /// twice.
    #[test]
    fn cross_block_range_overlap_fails() {
        let (mut root, registry) = real_fixtures();
        let states = root["minecraft:acacia_button"]["states"]
            .as_array_mut()
            .unwrap();
        for s in states.iter_mut() {
            let id = s["id"].as_u64().unwrap();
            s["id"] = serde_json::json!(id - 2);
        }
        let err = validate(root, &registry).unwrap_err();
        assert!(
            format!("{err:#}").contains("ranges overlap"),
            "got: {err:#}"
        );
    }

    /// Duplicating a state id in the real report must fail generation.
    #[test]
    fn duplicate_state_id_in_real_report_fails() {
        let (mut root, registry) = real_fixtures();
        let states = root["minecraft:redstone_wire"]["states"]
            .as_array_mut()
            .unwrap();
        let dup_id = states[1]["id"].clone();
        states[0]["id"] = dup_id;
        let err = validate(root, &registry).unwrap_err();
        assert!(
            err.to_string().contains("not strictly ascending"),
            "got: {err}"
        );
    }
}
