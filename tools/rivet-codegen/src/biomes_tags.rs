//! `rivet-codegen generate` biomes+tags half — consume the deterministic
//! `data/biomes_tags.json` fixture (produced by `extract-biomes-tags`, see
//! [`crate::extract_biomes_tags`]) and emit:
//!
//! - `generated/biomes.rs` — the `minecraft:worldgen/biome` id/name table
//!   (`BIOME_BY_NAME`/`BIOME_BY_ID`, dense `0..n`), the element table the
//!   `PalettedContainer<Holder<Biome>>` global palette indexes into (issue #49,
//!   gates chunk.access #183-b).
//! - `generated/tags.rs` — the tag network-serialization content: every
//!   registry on the `ClientboundUpdateTagsPacket` wire
//!   (`TagNetworkSerialization.serializeTagsToNetwork` → `networkSafeRegistries`
//!   = WORLDGEN networkable + STATIC), each mapped to tag-location -> element
//!   names in the tag file's value order (the wire order). Element-id
//!   resolution uses per-registry name tables; for the 7 datapack registries the
//!   report-driven surfaces cannot cover (`enchantment`, `dialog`,
//!   `painting_variant`, `timeline`, `instrument`, `banner_pattern`,
//!   `damage_type`) `tags.rs` also emits the element table; for the 8
//!   already-covered registries (`block`, `item`, `entity_type`, `fluid`,
//!   `game_event`, `potion`, `point_of_interest_type`, `worldgen/biome`) it
//!   emits only the tag table and the element ids resolve via the existing
//!   generated tables (`blocks.rs`, `registries.rs`, `biomes.rs`).
//!
//! The fixture records every tag-carrying registry's *runtime* element table,
//! which the validator cross-checks against the pinned report
//! (`data/reports/registries.json` for the report-backed surfaces, the
//! fixture's own `biomes` section for the biome registry) — so the live load
//! order cannot silently drift from the report/extract-driven ids.
//!
//! Determinism: element tables are re-ordered by id (the fixture is read
//! order-insensitively, never trusted by key order), tag locations are sorted,
//! and tag element lists preserve the wire order. Regeneration is
//! byte-idempotent.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::reports::SourceProvenance;

/// The tag-carrying registries whose element table is already emitted by
/// another generated file, so `tags.rs` emits only their tag table.
/// `minecraft:block` -> `blocks.rs`; `minecraft:worldgen/biome` -> `biomes.rs`;
/// the other six -> `registries.rs`.
const SHARED_ELEMENT_SURFACES: &[&str] = &[
    "minecraft:block",
    "minecraft:entity_type",
    "minecraft:fluid",
    "minecraft:game_event",
    "minecraft:item",
    "minecraft:point_of_interest_type",
    "minecraft:potion",
    "minecraft:worldgen/biome",
];

/// The report-backed shared surfaces (a subset of `SHARED_ELEMENT_SURFACES`)
/// cross-checked against `data/reports/registries.json`; the biome registry is
/// cross-checked against the fixture's own `biomes` section instead.
const REPORT_CROSSCHECKED: &[&str] = &[
    "minecraft:block",
    "minecraft:entity_type",
    "minecraft:fluid",
    "minecraft:game_event",
    "minecraft:item",
    "minecraft:point_of_interest_type",
    "minecraft:potion",
];

/// Ground-truth anchors the fixture must reproduce (from a live Paper 26.2
/// load). The Java probe asserts these against the running JVM; the codegen
/// asserts them against the fixture, so a fixture from a different jar or a
/// hand-edited one fails generation.
const ANCHORS: &[(&str, usize)] = &[
    ("minecraft:worldgen/biome", 66), // biome registry size
    ("minecraft:block", 1196),
    ("minecraft:item", 1537),
    ("minecraft:entity_type", 158),
    ("minecraft:fluid", 5),
    ("minecraft:game_event", 61),
    ("minecraft:potion", 46),
    ("minecraft:point_of_interest_type", 21),
    ("minecraft:enchantment", 43),
    ("minecraft:dialog", 3),
    ("minecraft:painting_variant", 51),
    ("minecraft:timeline", 4),
    ("minecraft:instrument", 8),
    ("minecraft:banner_pattern", 43),
    ("minecraft:damage_type", 51),
];

/// Total tag count across the 15 tag-carrying registries (the UpdateTags packet
/// total). Any drift in which registries carry tags, or how many, fails here.
const TAG_COUNT: usize = 697;

pub fn default_input(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/biomes_tags.json")
}

pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("crates/rivet-registry/src/generated")
}

/// One validated tag-carrying registry surface.
#[derive(Debug)]
struct TagRegistry {
    /// `minecraft:worldgen/biome` etc.
    key: String,
    /// Element entries in id order (id == index, dense 0..n).
    elements: Vec<Entry>,
    /// Tag location -> element names in tag-file value order.
    tags: Vec<(String, Vec<String>)>,
}

/// An element table entry: namespaced name + dense id.
#[derive(Debug)]
struct Entry {
    name: String,
    id: u16,
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
    let root = crate::registries::parse_strict(&json)
        .with_context(|| format!("parse {}", input.display()))?;
    let (biomes, registries) = validate(&root)?;
    let provenance = load_provenance(&input)?;

    fs::create_dir_all(&output).with_context(|| format!("create {}", output.display()))?;
    fs::write(
        output.join("biomes.rs"),
        render_biomes(&biomes, &provenance),
    )
    .context("write generated/biomes.rs")?;
    fs::write(
        output.join("tags.rs"),
        render_tags(&registries, &provenance),
    )
    .context("write generated/tags.rs")?;

    let tag_total: usize = registries.iter().map(|r| r.tags.len()).sum();
    println!(
        "Wrote {} biomes + {} registries / {} tags -> {}",
        biomes.len(),
        registries.len(),
        tag_total,
        output.display()
    );
    Ok(())
}

/// Full validation for the committed fixture: structural validation + the
/// report cross-check + the live-Paper anchor counts.
fn validate(root: &Value) -> Result<(Vec<Entry>, Vec<TagRegistry>)> {
    let (biomes, registries, probe) = validate_structural(root)?;

    // Cross-check the runtime element tables against the pinned report (a live
    // registry-load order change that the report would catch fails here).
    let report = read_report_surfaces()?;
    for key in REPORT_CROSSCHECKED {
        let reg = registries
            .iter()
            .find(|r| r.key.as_str() == *key)
            .context("report-cross-checked registry present")?;
        let report_entries = report.get(*key).context("report surface present")?;
        cross_check_shared(key, &reg.elements, report_entries)?;
    }

    // Anchor check: element table sizes and total tag count must match the
    // live-Paper ground truth (a fixture from a different jar fails here). The
    // probe counts recorded in the fixture must agree with these anchors too.
    for (key, expected_count) in ANCHORS {
        let reg = registries
            .iter()
            .find(|r| r.key == *key)
            .context("anchor registry present")?;
        if reg.elements.len() != *expected_count {
            bail!(
                "anchor drift: `{key}` has {} elements but a live Paper 26.2 load has {expected_count}",
                reg.elements.len()
            );
        }
    }
    let tag_total: usize = registries.iter().map(|r| r.tags.len()).sum();
    if tag_total != TAG_COUNT {
        bail!(
            "anchor drift: fixture has {tag_total} tags but a live Paper 26.2 load has {TAG_COUNT}"
        );
    }
    if probe.biome_count != ANCHORS[0].1
        || probe.tag_registry_count != ANCHORS.len()
        || probe.tag_count != TAG_COUNT
    {
        bail!(
            "probe drift: recorded counts {} biomes / {} registries / {} tags do not match the \
             live-Paper anchors {} / {} / {}",
            probe.biome_count,
            probe.tag_registry_count,
            probe.tag_count,
            ANCHORS[0].1,
            ANCHORS.len(),
            TAG_COUNT
        );
    }

    Ok((biomes, registries))
}

/// The counts the extractor recorded for its live Paper load (see the `probe`
/// object in the fixture). `validate_structural` checks the recorded values
/// against the parsed structures; `validate` checks them against the anchors.
#[derive(Clone, Copy, Debug)]
struct ProbeCounts {
    biome_count: usize,
    tag_registry_count: usize,
    tag_count: usize,
}

/// Structural validation (independent of the report/provenance): malformed
/// names, sparse/non-contiguous/duplicate ids, tag elements outside the element
/// table, and the authoritative registry set.
fn validate_structural(root: &Value) -> Result<(Vec<Entry>, Vec<TagRegistry>, ProbeCounts)> {
    let object = root
        .as_object()
        .context("biomes_tags.json root must be a JSON object")?;
    for field in object.keys() {
        if !matches!(
            field.as_str(),
            "generator"
                | "minecraft_version"
                | "protocol_version"
                | "world_version"
                | "biomes"
                | "registries"
                | "probe"
        ) {
            bail!("biomes_tags.json has unexpected top-level field `{field}`");
        }
    }
    // The `probe` object records the extractor's live-load counts (written by
    // the Java helper into the fixture because Bootstrap.wrapStreams() hijacks
    // stdout). Only the key allowlist and presence are checked here; the values
    // must match the parsed structures below (internal consistency), and the
    // live `probe-biomes-tags` re-derives them from a fresh load.
    let probe = object
        .get("probe")
        .and_then(Value::as_object)
        .context("biomes_tags.json is missing the `probe` object")?;
    for field in probe.keys() {
        if !matches!(
            field.as_str(),
            "biome_count" | "tag_registry_count" | "tag_count"
        ) {
            bail!("biomes_tags.json `probe` has unexpected field `{field}`");
        }
    }
    let probe_counts: HashMap<&str, usize> = probe
        .iter()
        .map(|(k, v)| {
            v.as_u64()
                .map(|n| (k.as_str(), n as usize))
                .with_context(|| format!("biomes_tags.json `probe.{k}` is not a count"))
        })
        .collect::<Result<_>>()?;
    let _mc = object
        .get("minecraft_version")
        .and_then(Value::as_str)
        .context("biomes_tags.json is missing `minecraft_version`")?;
    for (field, min) in [("protocol_version", 0u64), ("world_version", 0u64)] {
        match object.get(field).and_then(Value::as_u64) {
            Some(v) if v >= min => {}
            Some(_) => bail!("biomes_tags.json `{field}` is out of range"),
            None => bail!("biomes_tags.json is missing `{field}`"),
        }
    }

    let biomes = validate_elements("minecraft:worldgen/biome", &object["biomes"], true)?;

    let registries_obj = object
        .get("registries")
        .and_then(Value::as_object)
        .context("biomes_tags.json is missing `registries`")?;

    // The full authoritative tag-carrying set: every surface must be present.
    let expected = ANCHORS.iter().map(|(k, _)| *k).collect::<Vec<_>>();
    for key in &expected {
        if !registries_obj.contains_key(*key) {
            bail!("biomes_tags.json is missing tag-carrying registry `{key}`");
        }
    }
    for key in registries_obj.keys() {
        if !expected.contains(&key.as_str()) {
            bail!("biomes_tags.json has unknown tag-carrying registry `{key}`");
        }
    }

    let mut registries = Vec::with_capacity(expected.len());
    for key in &expected {
        let reg = validate_tag_registry(key, &registries_obj[*key])?;
        registries.push(reg);
    }

    // The probe counts recorded by the extractor must match the parsed
    // structures (internal consistency — a hand-edited fixture that bumps one
    // count without the other, or drifts from the tables, fails here).
    let probe_actual = ProbeCounts {
        biome_count: biomes.len(),
        tag_registry_count: registries.len(),
        tag_count: registries.iter().map(|r| r.tags.len()).sum(),
    };
    let expected_probe = [
        ("biome_count", probe_actual.biome_count),
        ("tag_registry_count", probe_actual.tag_registry_count),
        ("tag_count", probe_actual.tag_count),
    ];
    for (key, actual) in expected_probe {
        match probe_counts.get(key) {
            Some(&v) if v == actual => {}
            Some(&v) => bail!(
                "biomes_tags.json `probe.{key}` is {v} but the fixture has {actual} (hand-edited \
                 fixture?)"
            ),
            None => bail!("biomes_tags.json `probe` is missing `{key}`"),
        }
    }

    Ok((biomes, registries, probe_actual))
}

fn validate_elements(registry: &str, value: &Value, allow_empty: bool) -> Result<Vec<Entry>> {
    let obj = value
        .as_object()
        .with_context(|| format!("`{registry}` element table must be a JSON object"))?;
    let mut entries = Vec::with_capacity(obj.len());
    for (name, id) in obj {
        crate::registries::validate_name(registry, name)?;
        let id = parse_id(registry, name, id)?;
        entries.push(Entry {
            name: name.clone(),
            id,
        });
    }
    entries.sort_unstable_by_key(|e| e.id);
    check_dense_bijection(registry, &entries, allow_empty)?;
    Ok(entries)
}

/// Dense `0..n` id space with a unique name per id and a unique id per name.
fn check_dense_bijection(registry: &str, entries: &[Entry], allow_empty: bool) -> Result<()> {
    if entries.is_empty() && allow_empty {
        return Ok(());
    }
    for (i, e) in entries.iter().enumerate() {
        if e.id as usize != i {
            bail!(
                "`{registry}` element ids are not contiguous 0..{}: expected {} at index {i}, got {}",
                entries.len(),
                i,
                e.id
            );
        }
    }
    // Unique names (a duplicate id already fails the contiguity check, and two
    // names cannot share an id in a dense id==index space).
    let mut names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    for pair in names.windows(2) {
        if pair[0] == pair[1] {
            bail!("duplicate element name `{}` in `{registry}`", pair[0]);
        }
    }
    Ok(())
}

fn parse_id(registry: &str, name: &str, id: &Value) -> Result<u16> {
    let id = match id {
        Value::Number(n) => n.as_u64().with_context(|| {
            format!("element `{name}` in `{registry}` has a non-integer id ({n})")
        })?,
        _ => bail!("element `{name}` in `{registry}` has a non-numeric id"),
    };
    u16::try_from(id).with_context(|| {
        format!("element `{name}` in `{registry}` has an id outside the u16 range")
    })
}

fn validate_tag_registry(key: &str, value: &Value) -> Result<TagRegistry> {
    let obj = value
        .as_object()
        .with_context(|| format!("tag-carrying registry `{key}` must be a JSON object"))?;
    for field in obj.keys() {
        if !matches!(field.as_str(), "elements" | "tags") {
            bail!("registry `{key}` has unexpected field `{field}`");
        }
    }
    let elements = validate_elements(key, &obj["elements"], true)?;

    let tags_obj = obj
        .get("tags")
        .and_then(Value::as_object)
        .with_context(|| format!("registry `{key}` is missing `tags`"))?;
    let mut tags = Vec::with_capacity(tags_obj.len());
    for (loc, names_value) in tags_obj {
        crate::registries::validate_name(key, loc)?;
        let names = names_value
            .as_array()
            .with_context(|| format!("tag `{loc}` in `{key}` must be an array"))?;
        let mut resolved = Vec::with_capacity(names.len());
        for name in names {
            let name = name
                .as_str()
                .with_context(|| format!("tag `{loc}` in `{key}` has a non-string element"))?;
            if !elements.iter().any(|e| e.name == name) {
                bail!(
                    "tag `{loc}` in `{key}` references element `{name}` not in the registry's element table"
                );
            }
            resolved.push(name.to_string());
        }
        tags.push((loc.clone(), resolved));
    }
    tags.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(TagRegistry {
        key: key.to_string(),
        elements,
        tags,
    })
}

/// Cross-check a runtime element table (from the live registry load) against the
/// report-driven surface: identical name set and identical ids.
fn cross_check_shared(key: &str, runtime: &[Entry], report: &HashMap<String, u32>) -> Result<()> {
    if runtime.len() != report.len() {
        bail!(
            "runtime element table for `{key}` has {} entries but the report has {}",
            runtime.len(),
            report.len()
        );
    }
    for e in runtime {
        match report.get(&e.name) {
            Some(&id) if id as u16 == e.id => {}
            Some(&id) => bail!(
                "runtime/report id mismatch for `{}`: runtime {} vs report {id}",
                e.name,
                e.id
            ),
            None => bail!(
                "runtime element `{}` is absent from the report surface `{key}`",
                e.name
            ),
        }
    }
    Ok(())
}

/// Read the pinned `data/reports/registries.json` report surfaces for the
/// report-backed tag-carrying registries. Returns name -> protocol_id per key.
fn read_report_surfaces() -> Result<HashMap<String, HashMap<String, u32>>> {
    let repo_root = crate::extract::find_repo_root()?;
    let path = repo_root.join("tools/rivet-codegen/data/reports/registries.json");
    let json = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let root = crate::registries::parse_strict(&json)?;
    let object = root
        .as_object()
        .context("registries.json root must be a JSON object")?;

    let mut out = HashMap::new();
    for key in REPORT_CROSSCHECKED {
        let reg = object
            .get(*key)
            .with_context(|| format!("registries.json is missing report surface `{key}`"))?;
        let entries = reg
            .get("entries")
            .and_then(Value::as_object)
            .with_context(|| format!("report surface `{key}` is missing `entries`"))?;
        let mut map = HashMap::with_capacity(entries.len());
        for (name, entry) in entries {
            let id = entry
                .get("protocol_id")
                .and_then(Value::as_u64)
                .with_context(|| {
                    format!("report entry `{name}` in `{key}` is missing `protocol_id`")
                })?;
            map.insert(name.clone(), id as u32);
        }
        out.insert(key.to_string(), map);
    }
    Ok(out)
}

/// Link the fixture to its pinned provenance: the fixture must match the sha256
/// recorded next to it in `data/biomes_tags.manifest.json`, and the emitted
/// header carries that provenance (jar identity + MC/proto/world versions).
fn load_provenance(input: &Path) -> Result<SourceProvenance> {
    let manifest_path = input
        .parent()
        .map(|p| p.join("biomes_tags.manifest.json"))
        .with_context(|| format!("{} has no parent dir", input.display()))?;
    let manifest_json = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "read {} (expected next to the pinned fixture)",
            manifest_path.display()
        )
    })?;
    let manifest: FixtureManifest = serde_json::from_str(&manifest_json)
        .with_context(|| format!("parse {}", manifest_path.display()))?;
    let bytes = fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let actual = crate::reports::sha256_hex(&bytes);
    if actual != manifest.file.sha256 {
        bail!(
            "biomes_tags.json does not match its provenance manifest (expected sha256 {}, got {}) — \
             run `rivet-codegen extract-biomes-tags` to refresh the pinned fixture",
            manifest.file.sha256,
            actual
        );
    }
    crate::reports::verify_pinned_source(&manifest.source)?;
    Ok(manifest.source)
}

#[derive(serde::Deserialize)]
struct FixtureManifest {
    source: SourceProvenance,
    file: FixtureFile,
}

#[derive(serde::Deserialize)]
struct FixtureFile {
    sha256: String,
}

/// Render `generated/biomes.rs`.
fn render_biomes(biomes: &[Entry], source: &SourceProvenance) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by `tools/rivet-codegen generate` from data/biomes_tags.json\n\
         // (live Paper registry load via BiomeTagExtractor; MC {}, protocol {}, world {}).\n\
         // Source jar sha256 {}; provenance linked to data/biomes_tags.manifest.json.\n\
         // Do not edit by hand — PORTING.md: registries/data are generated, not hand-ported.\n\n",
        source.minecraft_version,
        source.protocol_version,
        source.world_version,
        source.jar_sha256.get(..16).unwrap_or(&source.jar_sha256)
    ));
    out.push_str(
        "// `minecraft:worldgen/biome` id/name table, dense 0..n (element id == holder id\n\
         // == network id == insertion index; OWNERSHIP.md §Registries). Biome ids are\n\
         // assigned at runtime from a `TreeMap<Identifier, Resource>` sorted by\n\
         // `Identifier` compareTo (path first, then namespace) — id 0 is\n\
         // `minecraft:badlands`, alphabetical. The global palette of\n\
         // `PalettedContainer<Holder<Biome>>` (chunk.access) indexes this table.\n\n",
    );
    out.push_str(&format!(
        "/// `minecraft:worldgen/biome` — biome name -> registry id (dense `0..{}`).\n",
        biomes.len()
    ));
    out.push_str("pub static BIOME_BY_NAME: phf::Map<&'static str, u16> = phf::phf_map! {\n");
    for e in biomes {
        out.push_str(&format!("    {:?} => {}u16,\n", e.name, e.id));
    }
    out.push_str("};\n\n");

    out.push_str(
        "/// `minecraft:worldgen/biome` — biome names indexed by registry id (id == index).\n",
    );
    out.push_str("pub static BIOME_BY_ID: &[&str] = &[\n");
    for e in biomes {
        out.push_str(&format!("    {:?}, // {}\n", e.name, e.id));
    }
    out.push_str("];\n\n");

    out.push_str("/// Number of vanilla biomes (the biome registry size).\n");
    out.push_str(&format!(
        "pub const BIOME_COUNT: usize = {};\n",
        biomes.len()
    ));
    out
}

/// Render `generated/tags.rs`.
fn render_tags(registries: &[TagRegistry], source: &SourceProvenance) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by `tools/rivet-codegen generate` from data/biomes_tags.json\n\
         // (live Paper registry load via BiomeTagExtractor; MC {}, protocol {}, world {}).\n\
         // Source jar sha256 {}; provenance linked to data/biomes_tags.manifest.json.\n\
         // Do not edit by hand — PORTING.md: registries/data are generated, not hand-ported.\n\n",
        source.minecraft_version,
        source.protocol_version,
        source.world_version,
        source.jar_sha256.get(..16).unwrap_or(&source.jar_sha256)
    ));
    out.push_str(
        "// Tag network-serialization content: the `ClientboundUpdateTagsPacket` payload\n\
         // (`TagNetworkSerialization.serializeTagsToNetwork` over `networkSafeRegistries`\n\
         // = WORLDGEN networkable + STATIC). Every tag's element list preserves the tag\n\
         // JSON file's value order — the wire order (never sorted). Element ids resolve\n\
         // via the per-registry `*_BY_NAME` table: the ones emitted here for the 7\n\
         // datapack registries the report cannot cover, and the existing tables\n\
         // (`blocks.rs`/`registries.rs`/`biomes.rs`) for the 8 shared surfaces.\n\n",
    );

    out.push_str(
        "/// The tag-carrying registries on the UpdateTags wire, in a deterministic order.\n",
    );
    out.push_str("pub static TAG_REGISTRIES: &[&str] = &[\n");
    for r in registries {
        out.push_str(&format!("    {:?},\n", r.key));
    }
    out.push_str("];\n\n");

    for r in registries {
        out.push_str(&render_registry(r));
    }
    out
}

fn render_registry(r: &TagRegistry) -> String {
    let prefix = crate::registries::prefix_for(&r.key);
    let mut out = String::new();

    if !SHARED_ELEMENT_SURFACES.contains(&r.key.as_str()) {
        out.push_str(&format!(
            "/// `{}` — element name -> registry id (dense `0..{}`).\n",
            r.key,
            r.elements.len()
        ));
        out.push_str(&format!(
            "pub static {prefix}_BY_NAME: phf::Map<&'static str, u16> = phf::phf_map! {{\n"
        ));
        for e in &r.elements {
            out.push_str(&format!("    {:?} => {}u16,\n", e.name, e.id));
        }
        out.push_str("};\n\n");

        out.push_str(&format!(
            "/// `{}` — element names indexed by registry id (id == index).\n",
            r.key
        ));
        out.push_str(&format!("pub static {prefix}_BY_ID: &[&str] = &[\n"));
        for e in &r.elements {
            out.push_str(&format!("    {:?}, // {}\n", e.name, e.id));
        }
        out.push_str("];\n\n");
    }

    let resolve = shared_resolution_hint(&r.key);
    out.push_str(&format!(
        "/// `{}` — tag location -> element names in tag-file value order (the wire order).\n",
        r.key
    ));
    if let Some(hint) = resolve {
        out.push_str(&format!("/// Element ids resolve via `{hint}`.\n"));
    }
    out.push_str(&format!(
        "pub static {prefix}_TAG_BY_NAME: phf::Map<&'static str, &'static [&'static str]> = phf::phf_map! {{\n"
    ));
    for (loc, names) in &r.tags {
        out.push_str(&format!("    {loc:?} => &["));
        for (i, name) in names.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("{name:?}"));
        }
        out.push_str("],\n");
    }
    out.push_str("};\n\n");

    out
}

/// Doc hint for where the shared element table lives (a cross-file pointer, not
/// a code dependency — each table is independently readable). No backticks: the
/// caller wraps the hint in `` ` `` already.
fn shared_resolution_hint(key: &str) -> Option<&'static str> {
    match key {
        "minecraft:block" => Some("BLOCK_BY_NAME in generated/blocks.rs"),
        "minecraft:worldgen/biome" => Some("BIOME_BY_NAME in generated/biomes.rs"),
        _ if SHARED_ELEMENT_SURFACES.contains(&key) => {
            Some("the *_BY_NAME surface in generated/registries.rs")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, id: u16) -> Entry {
        Entry {
            name: name.to_string(),
            id,
        }
    }

    fn fixture() -> Value {
        serde_json::json!({
            "generator": "BiomeTagExtractor",
            "minecraft_version": "26.2",
            "protocol_version": 776,
            "world_version": 4903,
            "biomes": {
                "minecraft:badlands": 0,
                "minecraft:bamboo_jungle": 1
            },
            "registries": {
                "minecraft:banner_pattern": {
                    "elements": {
                        "minecraft:base": 0,
                        "minecraft:square_bottom_left": 1
                    },
                    "tags": {
                        "minecraft:no_item_required": ["minecraft:base", "minecraft:square_bottom_left"]
                    }
                },
                "minecraft:block": {
                    "elements": {
                        "minecraft:air": 0,
                        "minecraft:stone": 1
                    },
                    "tags": {
                        "minecraft:mineable/pickaxe": ["minecraft:stone"]
                    }
                },
                "minecraft:damage_type": {
                    "elements": { "minecraft:in_fire": 0 },
                    "tags": { "minecraft:bypasses_armor": [] }
                },
                "minecraft:dialog": {
                    "elements": { "minecraft:actor": 0 },
                    "tags": { "minecraft:foo": [] }
                },
                "minecraft:enchantment": {
                    "elements": { "minecraft:protection": 0 },
                    "tags": { "minecraft:foo": [] }
                },
                "minecraft:entity_type": {
                    "elements": { "minecraft:allay": 0, "minecraft:pig": 1 },
                    "tags": { "minecraft:skeletons": ["minecraft:allay"] }
                },
                "minecraft:fluid": {
                    "elements": { "minecraft:empty": 0, "minecraft:water": 1 },
                    "tags": { "minecraft:water": ["minecraft:water"] }
                },
                "minecraft:game_event": {
                    "elements": { "minecraft:step": 0 },
                    "tags": { "minecraft:foo": [] }
                },
                "minecraft:instrument": {
                    "elements": { "minecraft:ponder_goat_horn": 0 },
                    "tags": { "minecraft:foo": [] }
                },
                "minecraft:item": {
                    "elements": { "minecraft:air": 0, "minecraft:stone": 1 },
                    "tags": { "minecraft:stone_tool_materials": ["minecraft:stone"] }
                },
                "minecraft:painting_variant": {
                    "elements": { "minecraft:alban": 0 },
                    "tags": { "minecraft:foo": [] }
                },
                "minecraft:point_of_interest_type": {
                    "elements": { "minecraft:armorer": 0 },
                    "tags": { "minecraft:foo": [] }
                },
                "minecraft:potion": {
                    "elements": { "minecraft:empty": 0, "minecraft:swiftness": 1 },
                    "tags": { "minecraft:foo": [] }
                },
                "minecraft:timeline": {
                    "elements": { "minecraft:foo": 0 },
                    "tags": { "minecraft:foo": [] }
                },
                "minecraft:worldgen/biome": {
                    "elements": {
                        "minecraft:badlands": 0,
                        "minecraft:bamboo_jungle": 1
                    },
                    "tags": {
                        "minecraft:is_overworld": ["minecraft:badlands"]
                    }
                }
            },
            "probe": {
                "biome_count": 2,
                "tag_registry_count": 15,
                "tag_count": 15
            }
        })
    }

    /// The minimal test fixture satisfies the structural checks (dense ids, tag
    /// membership, the 15-registry set) but not the report/anchor cross-checks
    /// (which only the real fixture passes). So the mutation tests drive
    /// `validate_structural`, and the report/anchor paths get their own tests.
    fn structural(v: &Value) -> (Vec<Entry>, Vec<TagRegistry>) {
        let (b, r, _) = validate_structural(v).unwrap();
        (b, r)
    }

    #[test]
    fn minimal_fixture_is_structurally_valid() {
        let (biomes, registries) = structural(&fixture());
        assert_eq!(biomes.len(), 2);
        assert_eq!(registries.len(), 15);
        // The biome registry's element table round-trips in id order.
        assert_eq!(biomes[0].name, "minecraft:badlands");
        assert_eq!(biomes[0].id, 0);
        assert_eq!(biomes[1].name, "minecraft:bamboo_jungle");
        assert_eq!(biomes[1].id, 1);
    }

    #[test]
    fn test_fixture_has_all_expected_anchors() {
        // ANCHORS must enumerate every tag-carrying registry with its size.
        let keys: Vec<&str> = ANCHORS.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"minecraft:worldgen/biome"));
        assert!(keys.contains(&"minecraft:block"));
        assert!(keys.contains(&"minecraft:damage_type"));
        assert_eq!(keys.len(), 15);
        assert_eq!(TAG_COUNT, 697);
    }

    #[test]
    fn validation_rejects_unknown_tag_registry() {
        let mut v = fixture();
        v["registries"].as_object_mut().unwrap().insert(
            "minecraft:made_up".into(),
            serde_json::json!({ "elements": {}, "tags": {} }),
        );
        let err = validate_structural(&v).unwrap_err();
        assert!(
            err.to_string().contains("unknown tag-carrying registry"),
            "got: {err}"
        );
    }

    #[test]
    fn validation_rejects_missing_tag_registry() {
        let mut v = fixture();
        v["registries"]
            .as_object_mut()
            .unwrap()
            .remove("minecraft:painting_variant");
        let err = validate_structural(&v).unwrap_err();
        assert!(
            err.to_string().contains("missing tag-carrying registry"),
            "got: {err}"
        );
    }

    #[test]
    fn sparse_element_ids_are_rejected() {
        let mut v = fixture();
        // biomes is {badlands:0, bamboo_jungle:1}; adding id 3 leaves a hole at 2.
        v["biomes"]["minecraft:sparse"] = serde_json::json!(3);
        let err = validate_structural(&v).unwrap_err();
        assert!(err.to_string().contains("not contiguous"), "got: {err}");
    }

    #[test]
    fn tag_referencing_unknown_element_is_rejected() {
        let mut v = fixture();
        v["registries"]["minecraft:block"]["tags"]["minecraft:mineable/pickaxe"] =
            serde_json::json!(["minecraft:not_a_block"]);
        let err = validate_structural(&v).unwrap_err();
        assert!(
            err.to_string()
                .contains("not in the registry's element table"),
            "got: {err}"
        );
    }

    #[test]
    fn duplicate_json_keys_are_rejected() {
        let json = r#"{
            "generator": "BiomeTagExtractor",
            "minecraft_version": "26.2",
            "protocol_version": 776,
            "world_version": 4903,
            "biomes": {
                "minecraft:badlands": 0,
                "minecraft:badlands": 1
            }
        }"#;
        let err = crate::registries::parse_strict(json).unwrap_err();
        assert!(
            err.to_string()
                .contains("duplicate object key `minecraft:badlands`"),
            "got: {err}"
        );
    }

    #[test]
    fn cross_check_detects_runtime_report_mismatch() {
        let runtime = vec![entry("minecraft:air", 0), entry("minecraft:stone", 1)];
        let mut report = HashMap::new();
        report.insert("minecraft:air".to_string(), 0u32);
        report.insert("minecraft:stone".to_string(), 2u32); // drift
        let err = cross_check_shared("minecraft:block", &runtime, &report).unwrap_err();
        assert!(err.to_string().contains("id mismatch"), "got: {err}");

        // Same lengths, one name differing -> "absent from the report".
        let mut renamed = HashMap::new();
        renamed.insert("minecraft:air".to_string(), 0u32);
        renamed.insert("minecraft:dirt".to_string(), 1u32);
        let err = cross_check_shared("minecraft:block", &runtime, &renamed).unwrap_err();
        assert!(
            err.to_string().contains("absent from the report"),
            "got: {err}"
        );
    }

    #[test]
    fn rendering_is_deterministic() {
        let (biomes, registries) = structural(&fixture());
        let source: SourceProvenance = serde_json::from_str(
            r#"{"jar":"paper-26.2.jar","jar_sha256":"e1a027e9481a16ec1da0f0e139d370280050d123a14c022a476c2dc8a697ebda","minecraft_version":"26.2","protocol_version":776,"world_version":4903}"#,
        )
        .unwrap();
        let first = render_biomes(&biomes, &source);
        let second = render_biomes(&biomes, &source);
        assert_eq!(first, second);
        assert!(first.contains("BIOME_BY_NAME"));
        assert!(first.contains("\"minecraft:badlands\" => 0u16"));
        assert!(first.contains("BIOME_COUNT: usize = 2"));

        let tags_first = render_tags(&registries, &source);
        let tags_second = render_tags(&registries, &source);
        assert_eq!(tags_first, tags_second);
        // The tag table emits a slice literal preserving wire order.
        assert!(tags_first.contains("minecraft:mineable/pickaxe\" => &[\"minecraft:stone\""));
        // Shared surfaces emit no element table (resolved elsewhere): the tag
        // table static exists, but no `pub static BLOCK_BY_NAME` is emitted.
        assert!(tags_first.contains("pub static BLOCK_TAG_BY_NAME"));
        assert!(!tags_first.contains("pub static BLOCK_BY_NAME"));
        // New datapack surfaces emit their own element table.
        assert!(tags_first.contains("BANNER_PATTERN_BY_NAME"));
        assert!(tags_first.contains("DAMAGE_TYPE_BY_ID"));
    }

    #[test]
    fn prefix_and_shared_sets() {
        assert_eq!(
            crate::registries::prefix_for("minecraft:worldgen/biome"),
            "WORLDGEN_BIOME"
        );
        assert_eq!(
            crate::registries::prefix_for("minecraft:point_of_interest_type"),
            "POINT_OF_INTEREST_TYPE"
        );
        assert!(SHARED_ELEMENT_SURFACES.contains(&"minecraft:block"));
        assert!(!SHARED_ELEMENT_SURFACES.contains(&"minecraft:enchantment"));
    }

    #[test]
    fn provenance_rejects_unpinned_source() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("biomes_tags.json");
        let bytes = b"fixture\n";
        fs::write(&input, bytes).unwrap();
        let manifest_path = root.path().join("biomes_tags.manifest.json");
        for (jar_sha256, paper_git, expected) in [
            (
                "deadbeef",
                crate::reports::PINNED_PAPER_COMMIT,
                "source SHA",
            ),
            (
                crate::reports::PINNED_SERVER_JAR_SHA256,
                "deadbeef",
                "Paper commit",
            ),
        ] {
            fs::write(
                &manifest_path,
                serde_json::to_vec(&serde_json::json!({
                    "source": {
                        "jar": "paper-26.2.jar",
                        "jar_sha256": jar_sha256,
                        "paper_git": paper_git,
                        "minecraft_version": "26.2",
                        "protocol_version": 776,
                        "world_version": 4903
                    },
                    "file": { "sha256": crate::reports::sha256_hex(bytes) }
                }))
                .unwrap(),
            )
            .unwrap();
            let error = load_provenance(&input).unwrap_err();
            assert!(error.to_string().contains(expected), "got: {error}");
        }
    }
}
