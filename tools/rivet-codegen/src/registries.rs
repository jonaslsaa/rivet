//! `rivet-codegen generate` registries half — consume the pinned vanilla
//! `data/reports/registries.json` (the `RegistryDumpReport` output, see
//! [`crate::reports`]) and emit the static-builtin id tables for the ordered
//! registry surfaces M1 (config sync + superflat join) actually touches.
//!
//! The fixture is a full dump of `BuiltInRegistries.REGISTRY` (every static
//! registry, in ROOT-registry registration order). Each registry maps every
//! element name to its `protocol_id` — the element id in that registry's own
//! id space. `RegistryDumpReport` calls `registry.getId(value)`, which is the
//! `MappedRegistry.byId` insertion index, so `protocol_id` is exactly the
//! element id (element id == holder id == network id == insertion index,
//! OWNERSHIP.md §Registries). The committed JSON keys are alphabetically sorted
//! by GsonHelper's stable writer, so the codegen never trusts key order — it
//! re-orders by `protocol_id`, recovering the Java registration order.
//!
//! # The authoritative surface set (M1)
//!
//! Issue #124 phase F emits tables for the *minimal* set of report-backed
//! registries whose element ids appear on the M1 wire — not all 95 dumped
//! registries, and not the datapack registries (dimension_type, biome,
//! worldgen/*, ...) which the report cannot cover because they are loaded from
//! datapacks and assigned ids at load time, not builtin.
//!
//! The surfaces below are required by:
//!
//! - `minecraft:item` — `ItemStack`/held-item/inventory via
//!   `ByteBufCodecs.holderRegistry(Registries.ITEM)`.
//! - `minecraft:entity_type` — `ClientboundAddEntity` (any entity near spawn)
//!   via `EntityType` holder codec.
//! - `minecraft:data_component_type` — the `ItemStack` component-patch path
//!   (`DataComponentPatch`), which composes the item codec.
//! - `minecraft:fluid` / `minecraft:game_event` / `minecraft:potion` /
//!   `minecraft:point_of_interest_type` — static builtin registries that carry
//!   vanilla datapack tags (`tags/fluid/*`, `tags/game_event/*`,
//!   `tags/potion/*`, `tags/point_of_interest_type/*`).
//!
//! Why the tagged static registries are on the M1 wire: the config-sync
//! `ClientboundUpdateTagsPacket` payload is
//! `TagNetworkSerialization.serializeTagsToNetwork` (SynchronizeRegistriesTask),
//! which walks `RegistrySynchronization.networkSafeRegistries`:
//! `Stream.concat(networkedRegistries, registries.getLayer(STATIC).registries())`.
//! Only the WORLDGEN stream is filtered by `isNetworkable`
//! (`SYNCHRONIZED_REGISTRIES` — datapack-loaded registries); the STATIC layer
//! passes through unfiltered. Vanilla binds each static registry's tags from
//! datapacks at server start (`TagLoader.loadTagsForExistingRegistries` on the
//! STATIC layer, committed via `MappedRegistry.prepareTagReload`/`PendingTags`
//! `apply`), so every tagged element's `registry.getId(value)` — its element id
//! — is serialized in the `UpdateTags` payload. These four tables are not part
//! of `SYNCHRONIZED_REGISTRIES`; that set is unpinnable by the builtin report.
//!
//! `minecraft:block` is *also* on the M1 wire (chunk/block-state palettes +
//! `UpdateTags`), but its tables already live in the extract-driven
//! `generated/blocks.rs` (source `data/block_states.json`); the two sources
//! must agree and a test asserts they do (see `drift_tests` in
//! [`crate::generate`]). Deferred surfaces (block_entity_type, mob_effect,
//! particle_type, sound_event, ...) are not on the M1 config-sync/join byte
//! path, so emitting them now would be speculative.
//!
//! Determinism: entries are emitted in `protocol_id` (registration) order, so
//! regeneration is byte-idempotent; the phf map text order follows the same
//! order as the id-indexed slice.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::reports::SourceProvenance;

/// The authoritative surface set (see the module docs). Order is fixed so the
/// emitted file layout is deterministic.
pub const TARGETS: &[&str] = &[
    "minecraft:item",
    "minecraft:entity_type",
    "minecraft:data_component_type",
    "minecraft:fluid",
    "minecraft:game_event",
    "minecraft:potion",
    "minecraft:point_of_interest_type",
];

pub fn default_input(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/reports/registries.json")
}

/// Tables are written into the same committed `generated/` dir as the block
/// tables (the golden drift test in [`crate::generate`] asserts that dir
/// contains exactly the generated files).
pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("crates/rivet-registry/src/generated")
}

/// One validated ordered registry surface.
#[derive(Debug)]
pub struct Surface {
    /// `minecraft:item` etc.
    key: &'static str,
    /// Registry-level `default` marker (`DefaultedRegistry` fold), if any.
    default: Option<String>,
    /// Entries in registration (`protocol_id`) order.
    entries: Vec<Entry>,
}

#[derive(Debug)]
struct Entry {
    /// `namespace:path`, e.g. `"minecraft:air"`.
    name: String,
    /// The element id in the registry's own id space; also the emitted-table
    /// index, so it must fit `u16`.
    protocol_id: u16,
}

pub fn run(input_flag: Option<&Path>, output_flag: Option<&Path>) -> Result<()> {
    let repo_root = crate::extract::find_repo_root()?;
    let input = match input_flag {
        Some(p) => p.to_path_buf(),
        None => default_input(&repo_root),
    };
    let output = match output_flag {
        Some(p) => p.to_path_buf(),
        None => default_output(&repo_root),
    };

    let json = fs::read_to_string(&input).with_context(|| format!("read {}", input.display()))?;
    let root = parse_strict(&json).with_context(|| format!("parse {}", input.display()))?;
    // Validate structurally first: a malformed/sparse fixture fails fast on
    // its own, before the provenance link pulls in the sibling manifest.
    let surfaces = validate(root)?;
    let provenance = load_provenance(&input)?;

    fs::create_dir_all(&output).with_context(|| format!("create {}", output.display()))?;
    fs::write(output.join("registries.rs"), render(&surfaces, &provenance))
        .context("write generated/registries.rs")?;

    let total: usize = surfaces.iter().map(|s| s.entries.len()).sum();
    println!(
        "Wrote {} entries across {} static-builtin registry surfaces -> {}",
        total,
        surfaces.len(),
        output.display()
    );
    Ok(())
}

/// Parse JSON, rejecting duplicate object keys at any depth (a hand-inserted
/// duplicate element name must fail, not silently reshape the table). Mirrors
/// the strict parser in [`crate::packets`].
fn parse_strict(json: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str::<StrictValue>(json).map(|strict| strict.0)
}

struct StrictValue(Value);

impl<'de> serde::Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StrictValue;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON value without duplicate object keys")
            }

            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(v.into())))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(v.into())))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                let number = serde_json::Number::from_f64(v)
                    .ok_or_else(|| E::custom("non-finite JSON number"))?;
                Ok(StrictValue(Value::Number(number)))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v.to_string())))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: serde::de::Deserializer<'de>,
            {
                deserializer.deserialize_any(Visitor)
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<StrictValue>()? {
                    values.push(value.0);
                }
                Ok(StrictValue(Value::Array(values)))
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut object = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    let value = map.next_value::<StrictValue>()?;
                    if object.insert(key.clone(), value.0).is_some() {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate object key `{key}`"
                        )));
                    }
                }
                Ok(StrictValue(Value::Object(object)))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// Structural validation of the registry report against the authoritative
/// surface set. Fails on: an unknown/missing target surface, a malformed
/// element name, a non-integer/negative/overflowing `protocol_id`, a sparse or
/// non-contiguous id space, duplicate ids, a `default` naming no element, an
/// inconsistent reverse table, and Rust-identifier collisions among element
/// names.
fn validate(root: Value) -> Result<Vec<Surface>> {
    let object = root
        .as_object()
        .context("registries.json root must be a JSON object")?;

    // Every target must be present in the report — a missing target is a
    // stale-fixture bug, not something to silently skip.
    for target in TARGETS {
        if !object.contains_key(*target) {
            bail!("registry report is missing target surface `{target}`");
        }
    }
    let mut surfaces = Vec::with_capacity(TARGETS.len());
    for target in TARGETS {
        let surface = validate_surface(target, &object[*target])?;
        surfaces.push(surface);
    }
    Ok(surfaces)
}

fn validate_surface(key: &'static str, value: &Value) -> Result<Surface> {
    let obj = value
        .as_object()
        .with_context(|| format!("registry `{key}` in registries.json must be a JSON object"))?;

    // Reject unknown registry-level fields so a fixture change (e.g. Gson
    // adding a new property) fails loudly instead of being ignored.
    for field in obj.keys() {
        if !matches!(field.as_str(), "default" | "protocol_id" | "entries") {
            bail!("registry `{key}` has unexpected field `{field}`");
        }
    }

    let default = match obj.get("default") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => bail!("registry `{key}` has a non-string `default`"),
    };

    let entries_value = obj
        .get("entries")
        .with_context(|| format!("registry `{key}` is missing `entries`"))?;
    let entries_obj = entries_value
        .as_object()
        .with_context(|| format!("registry `{key}` `entries` must be a JSON object"))?;

    let mut entries = Vec::with_capacity(entries_obj.len());
    for (name, entry) in entries_obj {
        validate_name(key, name)?;
        let protocol_id = parse_protocol_id(key, name, entry)?;
        entries.push(Entry {
            name: name.clone(),
            protocol_id,
        });
    }

    // Registration order is the `protocol_id` order.
    entries.sort_unstable_by_key(|e| e.protocol_id);

    // Duplicate ids: two elements cannot claim the same id.
    for pair in entries.windows(2) {
        if pair[0].protocol_id == pair[1].protocol_id {
            bail!(
                "duplicate protocol_id {} in `{key}`: `{}` and `{}`",
                pair[0].protocol_id,
                pair[0].name,
                pair[1].name
            );
        }
    }
    // Contiguity: the id-indexed table assumes id == index.
    for (i, e) in entries.iter().enumerate() {
        if e.protocol_id as usize != i {
            bail!(
                "protocol ids in `{key}` are not contiguous 0..{}: expected {} at index {i}, got {}",
                entries.len(),
                i,
                e.protocol_id
            );
        }
    }
    // Reverse-table consistency: id -> name must be a bijection (each name a
    // unique id), and every id must appear exactly once. Dense + unique ids
    // already imply this, but check explicitly so a hand-inserted duplicate
    // id or a mis-keyed entry cannot silently slip through a future reorder.
    {
        let mut seen: Vec<&str> = Vec::with_capacity(entries.len());
        for e in &entries {
            if seen.contains(&e.name.as_str()) {
                bail!("duplicate element name `{}` in `{key}`", e.name);
            }
            seen.push(&e.name);
        }
    }
    // The `default` fold must name a real element: a `DefaultedRegistry`
    // registers its default like any other element (the report's `default`
    // string is that element's name), so a default outside the entries is a
    // hand-edited/inconsistent report, not a legitimately-empty lookup.
    if let Some(d) = &default
        && !entries.iter().any(|e| e.name == *d)
    {
        bail!("registry `{key}` default `{d}` is not an element of its `entries`");
    }
    // Rust-identifier collisions: two distinct names (e.g. `minecraft:a/b`
    // and `minecraft:a_b`) can camel-case to the same Rust identifier. The
    // emitted tables key by raw string so this cannot break the output today,
    // but the generated surface is the canonical id enumeration that future
    // units turn into consts/enums — fail now, cheaply.
    {
        let mut idents = entries
            .iter()
            .map(|e| entry_ident(&e.name))
            .collect::<Vec<_>>();
        idents.sort_unstable();
        for pair in idents.windows(2) {
            if pair[0] == pair[1] {
                bail!(
                    "element names in `{key}` collide on Rust identifier `{}`",
                    pair[0]
                );
            }
        }
    }

    Ok(Surface {
        key,
        default,
        entries,
    })
}

/// Mirror `Identifier` validation (MC 26.2 `ResourceLocation`): namespace chars
/// `[a-z0-9_.-]`, path chars `[a-z0-9/._-]` — `:` is never valid inside either,
/// and the first `:` is the only separator. A name that Java could not parse
/// must fail generation, not silently emit a lookup key the runtime can never
/// see.
fn validate_name(registry: &str, name: &str) -> Result<()> {
    let Some((namespace, path)) = name.split_once(':') else {
        bail!("element `{name}` in `{registry}` is not a namespaced identifier (`namespace:path`)");
    };
    if namespace.is_empty() {
        bail!("element `{name}` in `{registry}` has an empty namespace");
    }
    if path.is_empty() {
        bail!("element `{name}` in `{registry}` has an empty path");
    }
    if path.contains(':') {
        bail!(
            "element `{name}` in `{registry}` has a `:` inside its path (only the first `:` is the namespace separator)"
        );
    }
    if !namespace
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'))
    {
        bail!("element `{name}` in `{registry}` has invalid namespace characters (`{namespace}`)");
    }
    if !path
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-' | '/'))
    {
        bail!("element `{name}` in `{registry}` has invalid path characters (`{path}`)");
    }
    Ok(())
}

/// Parse and range-check an element's `protocol_id`. The emitted tables are
/// `u16`-indexed, so an id beyond `u16::MAX` fails here rather than silently
/// truncating in `render_surface`'s `{}u16` cast.
fn parse_protocol_id(registry: &str, name: &str, entry: &Value) -> Result<u16> {
    let obj = entry
        .as_object()
        .with_context(|| format!("element `{name}` in `{registry}` must be a JSON object"))?;
    let extra: Vec<&String> = obj.keys().filter(|k| k.as_str() != "protocol_id").collect();
    if !extra.is_empty() {
        bail!(
            "element `{name}` in `{registry}` has unexpected fields: {}",
            extra
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let id = obj
        .get("protocol_id")
        .with_context(|| format!("element `{name}` in `{registry}` is missing `protocol_id`"))?;
    let id = match id {
        Value::Number(n) => {
            if let Some(id) = n.as_u64() {
                id
            } else if n.as_i64().is_some_and(|i| i < 0) {
                bail!("element `{name}` in `{registry}` has a negative `protocol_id` ({n})");
            } else {
                bail!("element `{name}` in `{registry}` has a non-integer `protocol_id`");
            }
        }
        _ => bail!("element `{name}` in `{registry}` has a non-numeric `protocol_id`"),
    };
    u16::try_from(id).with_context(|| {
        format!("element `{name}` in `{registry}` has a `protocol_id` outside the u16 range (tables are u16)")
    })
}

/// `minecraft:axolotl/variant` -> `AxolotlVariant`; the `/` separator in a path
/// is treated like `_`. A leading digit is prefixed with `_`. This is the
/// identifier future consts/enums would be derived from; the collision check
/// runs on it.
fn entry_ident(name: &str) -> String {
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
        "RegistryEntry".to_string()
    } else {
        out
    }
}

/// Link the consumed fixture to its pinned provenance (`manifest.json`): the
/// registry report's recorded sha256 must match the file actually being read.
fn load_provenance(input: &Path) -> Result<SourceProvenance> {
    let manifest_path = input
        .parent()
        .map(|p| p.join("manifest.json"))
        .with_context(|| format!("registries.json has no parent dir: {}", input.display()))?;
    let manifest_json = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "read {} (expected next to the pinned registries.json fixture)",
            manifest_path.display()
        )
    })?;
    let manifest: crate::reports::ProvenanceManifest = serde_json::from_str(&manifest_json)
        .with_context(|| format!("parse {}", manifest_path.display()))?;
    let entry = manifest
        .reports
        .iter()
        .find(|e| e.path == "registries.json")
        .with_context(|| {
            format!(
                "manifest {} has no registries.json entry",
                manifest_path.display()
            )
        })?;
    let bytes = fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let actual = crate::reports::sha256_hex(&bytes);
    if actual != entry.sha256 {
        bail!(
            "registries.json does not match the provenance manifest (expected sha256 {}, got {}) — \
             run `rivet-codegen reports` to refresh the pinned fixture",
            entry.sha256,
            actual
        );
    }
    Ok(manifest.source)
}

/// Render `generated/registries.rs`.
fn render(surfaces: &[Surface], source: &SourceProvenance) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by `tools/rivet-codegen generate` from data/reports/registries.json\n\
         // (vanilla RegistryDumpReport; MC {}, protocol {}, world {}).\n\
         // Source jar sha256 {}; provenance linked to data/reports/manifest.json.\n\
         // Do not edit by hand — PORTING.md: registries/data are generated, not hand-ported.\n\n",
        source.minecraft_version,
        source.protocol_version,
        source.world_version,
        source.jar_sha256.get(..16).unwrap_or(&source.jar_sha256)
    ));
    out.push_str(
        "// id-indexed static builtin entries for the ordered registry surfaces M1\n\
         // (config sync + superflat join) actually touches. Each surface is a dense\n\
         // 0..n bijection: element id == holder id == network id == insertion index\n\
         // (OWNERSHIP.md §Registries). The `*_BY_NAME`/`*_BY_ID` split mirrors the\n\
         // block tables in blocks.rs.\n\n",
    );
    for surface in surfaces {
        out.push_str(&render_surface(surface));
    }
    out
}

fn render_surface(surface: &Surface) -> String {
    let prefix = prefix_for(surface.key);
    let mut out = String::new();

    out.push_str(&format!(
        "/// `{}` — element name -> registry id (dense `0..{}`).\n",
        surface.key,
        surface.entries.len()
    ));
    out.push_str(&format!(
        "pub static {prefix}_BY_NAME: phf::Map<&'static str, u16> = phf::phf_map! {{\n"
    ));
    for e in &surface.entries {
        out.push_str(&format!("    {:?} => {}u16,\n", e.name, e.protocol_id));
    }
    out.push_str("};\n\n");

    out.push_str(&format!(
        "/// `{}` — element names indexed by registry id (id == index).\n",
        surface.key
    ));
    out.push_str(&format!("pub static {prefix}_BY_ID: &[&str] = &[\n"));
    for e in &surface.entries {
        out.push_str(&format!("    {:?}, // {}\n", e.name, e.protocol_id));
    }
    out.push_str("];\n\n");

    // `static` values elide the `'static` lifetime (clippy
    // redundant_static_lifetimes), matching the `BLOCK_BY_ID: &[&str]` style of
    // the block tables.
    match &surface.default {
        Some(d) => out.push_str(&format!(
            "/// `{}` — the `DefaultedRegistry` fold: element `{d}` has the fallback\n\
             /// id that `byId`/`getValue(name)` return for missing lookups.\n\
             pub static {prefix}_DEFAULT: &str = {d:?};\n\n",
            surface.key
        )),
        None => out.push_str(&format!(
            "/// `{}` has no defaulted element (a plain `Registry`, not a\n\
             /// `DefaultedRegistry`); lookups of missing names return `None`.\n\
             pub static {prefix}_DEFAULT: Option<&str> = None;\n\n",
            surface.key
        )),
    }

    out
}

/// `minecraft:item` -> `ITEM`; `minecraft:point_of_interest_type` ->
/// `POINT_OF_INTEREST_TYPE`. Mirrors the `UPPER_SNAKE` const naming of the
/// existing `BLOCK_BY_NAME`/`BLOCK_BY_ID` tables.
fn prefix_for(key: &str) -> String {
    let path = key.rsplit_once(':').map(|(_, p)| p).unwrap_or(key);
    path.replace(['/', '-'], "_").to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal report with two dense surfaces.
    fn minimal_report() -> Value {
        serde_json::json!({
            "minecraft:item": {
                "entries": {
                    "minecraft:air": { "protocol_id": 0 },
                    "minecraft:stone": { "protocol_id": 1 }
                },
                "protocol_id": 7
            },
            "minecraft:entity_type": {
                "entries": {
                    "minecraft:allay": { "protocol_id": 0 },
                    "minecraft:pig": { "protocol_id": 1 }
                },
                "protocol_id": 6,
                "default": "minecraft:pig"
            },
            "minecraft:data_component_type": {
                "entries": {
                    "minecraft:custom_name": { "protocol_id": 0 }
                },
                "protocol_id": 64
            },
            "minecraft:fluid": {
                "entries": {
                    "minecraft:empty": { "protocol_id": 0 },
                    "minecraft:flowing_water": { "protocol_id": 1 },
                    "minecraft:water": { "protocol_id": 2 },
                    "minecraft:flowing_lava": { "protocol_id": 3 },
                    "minecraft:lava": { "protocol_id": 4 }
                },
                "protocol_id": 2,
                "default": "minecraft:empty"
            },
            "minecraft:game_event": {
                "entries": {
                    "minecraft:step": { "protocol_id": 0 },
                    "minecraft:flap": { "protocol_id": 1 }
                },
                "protocol_id": 0,
                "default": "minecraft:step"
            },
            "minecraft:potion": {
                "entries": {
                    "minecraft:empty": { "protocol_id": 0 },
                    "minecraft:swiftness": { "protocol_id": 1 }
                },
                "protocol_id": 8
            },
            "minecraft:point_of_interest_type": {
                "entries": {
                    "minecraft:armorer": { "protocol_id": 0 },
                    "minecraft:beehive": { "protocol_id": 1 }
                },
                "protocol_id": 25
            }
        })
    }

    fn surfaces_of(value: &Value) -> Vec<Surface> {
        validate(value.clone()).unwrap()
    }

    fn surface<'a>(surfaces: &'a [Surface], key: &str) -> &'a Surface {
        surfaces.iter().find(|s| s.key == key).unwrap()
    }

    #[test]
    fn minimal_report_is_valid_and_ordered() {
        let surfaces = surfaces_of(&minimal_report());
        assert_eq!(surfaces.len(), TARGETS.len());
        // Entries are re-ordered by protocol_id, not by the (JSON) key order.
        let item = surface(&surfaces, "minecraft:item");
        assert_eq!(item.entries[0].name, "minecraft:air");
        assert_eq!(item.entries[0].protocol_id, 0);
        assert_eq!(item.entries[1].name, "minecraft:stone");
        assert_eq!(item.entries[1].protocol_id, 1);
        // The defaulted marker round-trips for DefaultedRegistries.
        let entity = surface(&surfaces, "minecraft:entity_type");
        assert_eq!(entity.default.as_deref(), Some("minecraft:pig"));
        // Non-defaulted surfaces carry no default marker.
        let potion = surface(&surfaces, "minecraft:potion");
        assert_eq!(potion.default, None);
    }

    #[test]
    fn missing_target_is_rejected() {
        let mut value = minimal_report();
        value.as_object_mut().unwrap().remove("minecraft:fluid");
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string()
                .contains("missing target surface `minecraft:fluid`"),
            "got: {err}"
        );
    }

    #[test]
    fn non_object_root_is_rejected() {
        let err = validate(serde_json::json!([1, 2, 3])).unwrap_err();
        assert!(
            err.to_string().contains("root must be a JSON object"),
            "got: {err}"
        );
    }

    #[test]
    fn sparse_ids_are_rejected() {
        let mut value = minimal_report();
        value["minecraft:item"]["entries"]["minecraft:dirt"] =
            serde_json::json!({ "protocol_id": 3 });
        let err = validate(value).unwrap_err();
        assert!(err.to_string().contains("not contiguous"), "got: {err}");
    }

    #[test]
    fn non_zero_start_ids_are_rejected() {
        let mut value = minimal_report();
        value["minecraft:item"]["entries"]["minecraft:stone"] =
            serde_json::json!({ "protocol_id": 1 });
        value["minecraft:item"]["entries"]["minecraft:dirt"] =
            serde_json::json!({ "protocol_id": 2 });
        // `minecraft:air` now has id 0 still; make the set start at 1 by
        // removing air's entry and giving stone id 1, dirt id 2 -> missing 0.
        value["minecraft:item"]["entries"]
            .as_object_mut()
            .unwrap()
            .remove("minecraft:air");
        let err = validate(value).unwrap_err();
        assert!(err.to_string().contains("not contiguous"), "got: {err}");
    }

    #[test]
    fn duplicate_protocol_id_is_rejected() {
        let mut value = minimal_report();
        value["minecraft:item"]["entries"]["minecraft:stone"] =
            serde_json::json!({ "protocol_id": 0 });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("duplicate protocol_id 0"),
            "got: {err}"
        );
    }

    #[test]
    fn non_integer_protocol_id_is_rejected() {
        let mut value = minimal_report();
        value["minecraft:item"]["entries"]["minecraft:stone"] =
            serde_json::json!({ "protocol_id": 1.5 });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("non-integer `protocol_id`"),
            "got: {err}"
        );
    }

    #[test]
    fn negative_protocol_id_is_rejected() {
        let mut value = minimal_report();
        value["minecraft:item"]["entries"]["minecraft:stone"] =
            serde_json::json!({ "protocol_id": -1 });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("negative `protocol_id`"),
            "got: {err}"
        );
    }

    #[test]
    fn missing_protocol_id_is_rejected() {
        let mut value = minimal_report();
        value["minecraft:item"]["entries"]["minecraft:stone"] = serde_json::json!({});
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("missing `protocol_id`"),
            "got: {err}"
        );
    }

    #[test]
    fn id_beyond_u16_is_rejected() {
        // The emitted tables are u16; an id above u16::MAX must fail rather
        // than silently truncate in the render `{}u16` cast.
        let mut value = minimal_report();
        value["minecraft:item"]["entries"]["minecraft:stone"] =
            serde_json::json!({ "protocol_id": 65536 });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("outside the u16 range"),
            "got: {err}"
        );
    }

    #[test]
    fn unexpected_entry_fields_are_rejected() {
        let mut value = minimal_report();
        value["minecraft:item"]["entries"]["minecraft:stone"] =
            serde_json::json!({ "protocol_id": 1, "extra": true });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string().contains("unexpected fields: extra"),
            "got: {err}"
        );
    }

    #[test]
    fn default_outside_entries_is_rejected() {
        let mut value = minimal_report();
        // entity_type's entries hold `allay` + `pig`; a default that names
        // neither is an inconsistent reverse-table, not a valid fold.
        value["minecraft:entity_type"]["default"] = serde_json::json!("minecraft:not_registered");
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string()
                .contains("default `minecraft:not_registered` is not an element"),
            "got: {err}"
        );
    }

    #[test]
    fn malformed_names_are_rejected() {
        for bad in ["no_namespace", ":empty_ns", "minecraft:", "minecraft:a:b"] {
            let mut value = minimal_report();
            value["minecraft:item"]["entries"][bad] = serde_json::json!({ "protocol_id": 0 });
            let err = validate(value).unwrap_err();
            assert!(
                err.to_string().contains("not a namespaced identifier")
                    || err.to_string().contains("has an empty namespace")
                    || err.to_string().contains("has an empty path")
                    || err.to_string().contains("`:` inside its path"),
                "name `{bad}`: got: {err}"
            );
        }
    }

    #[test]
    fn identifier_collisions_are_rejected() {
        // `minecraft:foo/bar` and `minecraft:foo_bar` both map to `FooBar`.
        let mut value = minimal_report();
        value["minecraft:item"]["entries"]
            .as_object_mut()
            .unwrap()
            .clear();
        value["minecraft:item"]["entries"]["minecraft:foo/bar"] =
            serde_json::json!({ "protocol_id": 0 });
        value["minecraft:item"]["entries"]["minecraft:foo_bar"] =
            serde_json::json!({ "protocol_id": 1 });
        let err = validate(value).unwrap_err();
        assert!(
            err.to_string()
                .contains("collide on Rust identifier `FooBar`"),
            "got: {err}"
        );
    }

    #[test]
    fn duplicate_names_cannot_appear_in_json() {
        // Duplicate JSON keys are rejected by the strict parser (last-wins is
        // not allowed), so a hand-inserted duplicate element name fails early.
        let json = r#"{
            "minecraft:item": {
                "entries": {
                    "minecraft:air": { "protocol_id": 0 },
                    "minecraft:air": { "protocol_id": 1 }
                }
            },
            "minecraft:entity_type": {"entries": {}},
            "minecraft:data_component_type": {"entries": {}},
            "minecraft:fluid": {"entries": {}},
            "minecraft:game_event": {"entries": {}},
            "minecraft:potion": {"entries": {}},
            "minecraft:point_of_interest_type": {"entries": {}}
        }"#;
        let err = parse_strict(json).unwrap_err();
        assert!(
            err.to_string()
                .contains("duplicate object key `minecraft:air`"),
            "got: {err}"
        );
    }

    #[test]
    fn rendering_is_deterministic() {
        let surfaces = surfaces_of(&minimal_report());
        // Build the provenance via serde so the render test stays decoupled
        // from field visibility (jar/paper_git are private to reports.rs).
        let source: SourceProvenance = serde_json::from_str(
            r#"{"jar":"paper-26.2.jar","jar_sha256":"e1a027e9481a16ec1da0f0e139d370280050d123a14c022a476c2dc8a697ebda","minecraft_version":"26.2","protocol_version":776,"world_version":4903}"#,
        )
        .unwrap();
        let first = render(&surfaces, &source);
        let second = render(&surfaces, &source);
        assert_eq!(first, second);
        // The emitted file references the provenance in its header.
        assert!(first.contains("RegistryDumpReport; MC 26.2, protocol 776, world 4903"));
        assert!(first.contains("e1a027e9481a16ec"));
        // Prefix naming matches the block-table convention.
        assert!(first.contains("ITEM_BY_NAME"));
        assert!(first.contains("ITEM_BY_ID"));
        assert!(first.contains("POINT_OF_INTEREST_TYPE_BY_ID"));
        // The defaulted fold renders for defaulted registries only.
        assert!(first.contains("ENTITY_TYPE_DEFAULT: &str = \"minecraft:pig\""));
        assert!(first.contains("POTION_DEFAULT: Option<&str> = None"));
    }

    #[test]
    fn prefix_and_ident_helpers() {
        assert_eq!(prefix_for("minecraft:item"), "ITEM");
        assert_eq!(
            prefix_for("minecraft:point_of_interest_type"),
            "POINT_OF_INTEREST_TYPE"
        );
        assert_eq!(entry_ident("minecraft:axolotl/variant"), "AxolotlVariant");
        assert_eq!(entry_ident("minecraft:air"), "Air");
        assert_eq!(entry_ident("minecraft:1x1_slot"), "_1x1Slot");
    }
}
