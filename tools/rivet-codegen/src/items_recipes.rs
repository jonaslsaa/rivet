//! `rivet-codegen extract-items-recipes` — dump the deterministic MC 26.2 item
//! metadata + the canonical recipe table from a real Paper registry load (issue
//! #186), mirroring `extract-biomes-tags`' Java-helper-against-the-server-
//! classpath pattern.
//!
//! The item id table itself is already covered by the report-driven `registries`
//! generator (`data/reports/registries.json`, the `RegistryDumpReport`). What
//! that report cannot capture is the per-item *behavioral* metadata this helper
//! adds: the default max stack size (which requires binding each item's holder
//! components via `DATA_COMPONENT_INITIALIZERS` — `createLookup()` alone never
//! binds them), the required feature-flag set, and the crafting-remaining item.
//! [`validate_items_recipes`] cross-checks the fixture's item id table against
//! the same report surface the biomes/tags generator uses (`read_report_surfaces`
//! in `crate::biomes_tags`, the `minecraft:item` entry), so the live-load
//! capture cannot drift from the datagen capture.
//!
//! Recipes are loaded through the exact path `RecipeManager.prepare` uses
//! (`FileToIdConverter.registry(Registries.RECIPE)` + `scanDirectory` +
//! `Recipe.CODEC` into a `TreeMap<Identifier, Recipe<?>>`) and re-encoded in
//! `DataProvider.saveStable` canonical form, so the fixture is the canonical
//! parsed form — not a raw copy of the embedded datapack — and a recipe that
//! failed to parse aborts extraction (the helper compares parsed vs file count).
//!
//! The helper (`src/java/ItemRecipeExtractor.java`) runs `Bootstrap.bootStrap()`,
//! `VanillaRegistries.createLookup()`, the `DATA_COMPONENT_INITIALIZERS` build+
//! apply, then dumps `BuiltInRegistries.ITEM` in registration order and the
//! parsed recipes in `Identifier` order. The dump is deterministic (verified
//! byte-identical across independent runs), so the committed fixture is the
//! no-drift baseline. The `generator` label the helper writes is pinned to the
//! fixture + manifest (the manifest reads it from the fixture, and a test
//! asserts the committed fixture matches the Java helper's literal).
//!
//! Output: `data/items_recipes.json` + `data/items_recipes.manifest.json`.
//!
//! The fixture is *fixture-only* for the behavioral halves: the per-item
//! `max_stack_size`/`feature_flags`/`crafting_remaining_item` metadata and the
//! recipe table are captured and pinned here, but not yet emitted into the
//! `rivet-registry` generated tables. (The item id table itself is already
//! emitted — the report-driven `registries` generator covers it as
//! `ITEM_BY_NAME`/`ITEM_BY_ID`, per the first paragraph.) Emission of the
//! behavioral + recipe halves must reuse the same report-driven
//! registry-emission seam the biome/tag/block tables use, and the downstream
//! collision domain (item ids vs the `registries.rs` `ITEM_BY_*` surface, plus
//! the M3 item/loot consumers of #22) is owned by #186 — whose "Done means"
//! requires that emission. The single `RivetTodo(#186)` marker lives at the one
//! intentional seam — `cross_check_item_report`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use crate::extract;

/// Canonical output path for the extracted item + recipe fixture.
pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/items_recipes.json")
}

/// Compile + run `ItemRecipeExtractor` against the bundler classpath, writing
/// the fixture JSON to `output`. Shared by `extract-items-recipes` and the live
/// probe (`probe-items-recipes`, which asserts byte-identity with the committed
/// fixture).
pub(crate) fn run_extractor(repo_root: &Path, bundler: &Path, output: &Path) -> Result<()> {
    let (classpath, java, javac) = extract::prepare_runtime(repo_root, bundler)?;

    let cache = repo_root.join("tools/rivet-codegen/.cache");
    let classpath_dir = cache.join("classpath");
    let (version, _) = extract::read_versions_list(bundler, &classpath_dir)?;

    let helper_dir = cache.join("itemrecipeextractor");
    fs::create_dir_all(&helper_dir).context("create item+recipe helper dir")?;
    let helper_src = include_str!("java/ItemRecipeExtractor.java");
    let helper_file = helper_dir.join("ItemRecipeExtractor.java");
    fs::write(&helper_file, helper_src).context("write ItemRecipeExtractor.java")?;
    extract::run_cmd(
        &javac,
        &[
            "-cp",
            &classpath,
            "-d",
            helper_dir.to_str().unwrap(),
            helper_file.to_str().unwrap(),
        ],
        "compile ItemRecipeExtractor.java",
    )?;

    // Quiet log4j down so the helper's stdout (any stray logging) stays empty;
    // the JSON fixture is written to `output` directly.
    let log4j_off = cache.join("log4j2-off.xml");
    if !log4j_off.is_file() {
        fs::write(
            &log4j_off,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Configuration status="off"><Loggers><Root level="off"/></Loggers></Configuration>
"#,
        )
        .context("write log4j2-off.xml")?;
    }

    let classpath_arg = format!("{classpath}:{}", helper_dir.display());
    let log4j_arg = format!("-Dlog4j.configurationFile={}", log4j_off.display());
    extract::run_cmd(
        &java,
        &[
            "-cp",
            &classpath_arg,
            "--enable-native-access=ALL-UNNAMED",
            &log4j_arg,
            "ItemRecipeExtractor",
            "--output",
            output.to_str().unwrap(),
            "--version",
            &version,
        ],
        "run ItemRecipeExtractor",
    )?;

    anyhow::ensure!(
        output.is_file(),
        "extract-items-recipes finished but {} was not produced",
        output.display()
    );
    Ok(())
}

/// Resolve the source jar for provenance: `--bundler` points at the bundler,
/// but the pinned source identity is the materialized server jar (the same one
/// the report provenance records). Falls back to the bundler's extracted server
/// jar when no materialized run exists.
fn source_jar(repo_root: &Path, bundler: &Path) -> PathBuf {
    let materialized = crate::reports::default_jar(repo_root);
    if materialized.is_file() {
        return materialized;
    }
    // Fall back to the server jar extracted from the bundler (same bytes as the
    // materialized run when built from the same Paper tree).
    let cache = repo_root.join("tools/rivet-codegen/.cache/classpath");
    if let Ok((_, rel)) = extract::read_versions_list(bundler, &cache) {
        let candidate = cache.join("META-INF/versions").join(&rel);
        if candidate.is_file() {
            return candidate;
        }
    }
    materialized
}

/// The canonical `generator` label written by `ItemRecipeExtractor.java`. This
/// is the single source of truth for the label; the fixture and the manifest
/// must both carry exactly this string (a test asserts the committed fixture
/// matches it), so the manifest label can never diverge from the fixture's.
pub(crate) const GENERATOR_LABEL: &str = "ItemRecipeExtractor (Bootstrap + static tags + VanillaRegistries.createLookup + DATA_COMPONENT_INITIALIZERS + Recipe.CODEC)";

/// Write `data/items_recipes.manifest.json`: the source provenance (same shape
/// as the reports manifest) + the fixture's sha256, so the codegen can pin the
/// fixture to its source.
fn write_manifest(repo_root: &Path, output: &Path, bundler: &Path) -> Result<()> {
    let jar = source_jar(repo_root, bundler);
    if !jar.is_file() {
        // No jar to record provenance for (e.g. a test-only extraction); skip.
        return Ok(());
    }
    let mut source = crate::reports::capture_source(&jar, repo_root)?;
    // Record the canonical repo-relative source location (same as the reports
    // manifest) even when the bytes came from the bundler's extracted server jar
    // — the sha256 is the load-bearing identity; the path is context only.
    let canonical = crate::reports::default_jar(repo_root)
        .strip_prefix(repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| source.jar.clone());
    source.jar = canonical;
    let bytes = fs::read(output).with_context(|| format!("read {}", output.display()))?;
    let manifest = FixtureManifest {
        generator: GENERATOR_LABEL.to_string(),
        source,
        file: FixtureFile {
            bytes: bytes.len() as u64,
            sha256: crate::reports::sha256_hex(&bytes),
        },
    };
    let manifest_path = output.with_extension("manifest.json");
    let json = format!("{}\n", serde_json::to_string_pretty(&manifest)?);
    fs::write(&manifest_path, json).context("write items_recipes.manifest.json")?;
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FixtureManifest {
    generator: String,
    source: crate::reports::SourceProvenance,
    file: FixtureFile,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FixtureFile {
    bytes: u64,
    sha256: String,
}

/// The live-Paper 26.2 anchor counts a valid fixture must reproduce (asserted
/// against the fixture's `probe` object; the `probe-items-recipes` probe also
/// re-derives them from a fresh load).
const ANCHORS: &[(&str, usize)] = &[
    ("item_count", 1537),
    ("recipe_count", 1585),
    ("recipe_file_count", 1585),
];

/// The canonical `Recipe.CODEC` serializers present in a vanilla 26.2 load
/// (the `type` discriminator values). A recipe type outside this set means a
/// different jar or a new vanilla recipe type.
const RECIPE_TYPES: &[&str] = &[
    "minecraft:blasting",
    "minecraft:campfire_cooking",
    "minecraft:crafting_decorated_pot",
    "minecraft:crafting_dye",
    "minecraft:crafting_imbue",
    "minecraft:crafting_shaped",
    "minecraft:crafting_shapeless",
    "minecraft:crafting_special_bannerduplicate",
    "minecraft:crafting_special_bookcloning",
    "minecraft:crafting_special_firework_rocket",
    "minecraft:crafting_special_firework_star",
    "minecraft:crafting_special_firework_star_fade",
    "minecraft:crafting_special_mapextending",
    "minecraft:crafting_special_repairitem",
    "minecraft:crafting_special_shielddecoration",
    "minecraft:crafting_transmute",
    "minecraft:smelting",
    "minecraft:smithing_transform",
    "minecraft:smithing_trim",
    "minecraft:smoking",
    "minecraft:stonecutting",
];

/// The serialized `Recipe.CODEC` field names each recipe type may carry (the
/// fixture re-encodes every recipe with `Recipe.CODEC`, so this is the exact
/// `MapCodec` surface per type, excluding the shared `type` discriminator). A
/// recipe type outside this set — or a field outside it — means a different jar
/// or a new vanilla recipe type/field.
const RECIPE_FIELDS: &[(&str, &[&str])] = &[
    (
        "minecraft:blasting",
        &["category", "experience", "group", "ingredient", "result"],
    ),
    (
        "minecraft:campfire_cooking",
        &[
            "category",
            "cookingtime",
            "experience",
            "ingredient",
            "result",
        ],
    ),
    (
        "minecraft:crafting_decorated_pot",
        &["back", "front", "left", "result", "right"],
    ),
    (
        "minecraft:crafting_dye",
        &["dye", "group", "result", "target"],
    ),
    (
        "minecraft:crafting_imbue",
        &["material", "result", "source"],
    ),
    (
        "minecraft:crafting_shaped",
        &[
            "category",
            "group",
            "key",
            "pattern",
            "result",
            "show_notification",
        ],
    ),
    (
        "minecraft:crafting_shapeless",
        &["category", "group", "ingredients", "result"],
    ),
    (
        "minecraft:crafting_special_bannerduplicate",
        &["banner", "result"],
    ),
    (
        "minecraft:crafting_special_bookcloning",
        &["material", "result", "source"],
    ),
    (
        "minecraft:crafting_special_firework_rocket",
        &["fuel", "result", "shell", "star"],
    ),
    (
        "minecraft:crafting_special_firework_star",
        &["dye", "fuel", "result", "shapes", "trail", "twinkle"],
    ),
    (
        "minecraft:crafting_special_firework_star_fade",
        &["dye", "result", "target"],
    ),
    (
        "minecraft:crafting_special_mapextending",
        &["map", "material", "result"],
    ),
    ("minecraft:crafting_special_repairitem", &[]),
    (
        "minecraft:crafting_special_shielddecoration",
        &["banner", "result", "target"],
    ),
    (
        "minecraft:crafting_transmute",
        &[
            "add_material_count_to_result",
            "category",
            "group",
            "input",
            "material",
            "material_count",
            "result",
        ],
    ),
    (
        "minecraft:smelting",
        &["category", "experience", "group", "ingredient", "result"],
    ),
    (
        "minecraft:smithing_transform",
        &["addition", "base", "result", "template"],
    ),
    (
        "minecraft:smithing_trim",
        &["addition", "base", "pattern", "template"],
    ),
    (
        "minecraft:smoking",
        &["category", "experience", "ingredient", "result"],
    ),
    ("minecraft:stonecutting", &["ingredient", "result"]),
];

/// Full validation for a fixture at `path`: structural validation + the
/// item-id cross-check against the pinned report + the live-Paper anchor
/// counts, and — when the provenance manifest sits next to the fixture — a
/// manifest sha256 verification (so the pinned fixture is read-verified, not
/// just written). Used by `extract-items-recipes` (on the freshly written
/// fixture) and the fixture-pinned conformance test. The probe validates its
/// fresh scratch load through the byte-level `validate_items_recipes_str` (no
/// manifest next to the scratch) and then byte-compares it against this
/// committed path.
pub(crate) fn validate_items_recipes(path: &Path) -> Result<()> {
    let json = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    validate_items_recipes_str(&json)?;
    // Consume + verify the pinned provenance manifest rather than treating it as
    // write-only metadata: a tampered fixture that still passes the structural/
    // anchor/report checks fails the manifest sha256 here. Skipped only when no
    // manifest exists next to the input (e.g. a probe scratch load).
    let manifest_path = path.with_extension("manifest.json");
    if manifest_path.is_file() {
        load_provenance(path)?;
    }
    Ok(())
}

/// Validation entry point that works on bytes (tests, probe's fresh load).
fn validate_items_recipes_str(json: &str) -> Result<()> {
    let root = crate::registries::parse_strict(json)?;
    validate_structural(&root)?;
    check_anchors(&root)?;
    cross_check_item_report(&root)?;
    Ok(())
}

/// Structural validation (independent of the report): allow-listed top-level
/// and per-item fields, dense `0..n` item ids, item metadata shape, recipe
/// type shape, and probe-vs-parsed internal consistency.
fn validate_structural(root: &Value) -> Result<()> {
    let object = root
        .as_object()
        .context("items_recipes.json root must be a JSON object")?;
    for field in object.keys() {
        if !matches!(
            field.as_str(),
            "generator"
                | "minecraft_version"
                | "protocol_version"
                | "world_version"
                | "items"
                | "recipes"
                | "probe"
        ) {
            bail!("items_recipes.json has unexpected top-level field `{field}`");
        }
    }

    // The `probe` object records the extractor's live-load counts (written by
    // the Java helper into the fixture because Bootstrap.wrapStreams() hijacks
    // stdout). Only the key allowlist and presence are checked here; the values
    // must match the parsed structures below (internal consistency), and the
    // live `probe-items-recipes` re-derives them from a fresh load.
    let probe = object
        .get("probe")
        .and_then(Value::as_object)
        .context("items_recipes.json is missing the `probe` object")?;
    for field in probe.keys() {
        if !matches!(
            field.as_str(),
            "item_count" | "recipe_count" | "recipe_file_count"
        ) {
            bail!("items_recipes.json `probe` has unexpected field `{field}`");
        }
    }
    let probe_counts: HashMap<&str, usize> = probe
        .iter()
        .map(|(k, v)| {
            v.as_u64()
                .map(|n| (k.as_str(), n as usize))
                .with_context(|| format!("items_recipes.json `probe.{k}` is not a count"))
        })
        .collect::<Result<_>>()?;

    let _mc = object
        .get("minecraft_version")
        .and_then(Value::as_str)
        .context("items_recipes.json is missing `minecraft_version`")?;
    for (field, min) in [("protocol_version", 0u64), ("world_version", 0u64)] {
        match object.get(field).and_then(Value::as_u64) {
            Some(v) if v >= min => {}
            Some(_) => bail!("items_recipes.json `{field}` is out of range"),
            None => bail!("items_recipes.json is missing `{field}`"),
        }
    }

    let items_obj = object
        .get("items")
        .and_then(Value::as_object)
        .context("items_recipes.json is missing `items`")?;
    let items = validate_items(items_obj)?;

    let recipes_obj = object
        .get("recipes")
        .and_then(Value::as_object)
        .context("items_recipes.json is missing `recipes`")?;
    validate_recipes(recipes_obj)?;

    // The probe counts recorded by the extractor must match the parsed
    // structures (internal consistency — a hand-edited fixture that bumps one
    // count without the other, or drifts from the tables, fails here).
    let expected_probe = [
        ("item_count", items.len()),
        ("recipe_count", recipes_obj.len()),
        ("recipe_file_count", recipes_obj.len()),
    ];
    for (key, actual) in expected_probe {
        match probe_counts.get(key) {
            Some(&v) if v == actual => {}
            Some(&v) => bail!(
                "items_recipes.json `probe.{key}` is {v} but the fixture has {actual} (hand-edited \
                 fixture?)"
            ),
            None => bail!("items_recipes.json `probe` is missing `{key}`"),
        }
    }
    Ok(())
}

/// One validated item table entry.
#[derive(Debug)]
struct ItemEntry {
    name: String,
    id: u16,
}

fn validate_items(items_obj: &Map<String, Value>) -> Result<Vec<ItemEntry>> {
    let mut entries = Vec::with_capacity(items_obj.len());
    for (name, meta) in items_obj {
        crate::registries::validate_name("minecraft:item", name)?;
        let meta = meta
            .as_object()
            .with_context(|| format!("item `{name}` must be a JSON object"))?;
        for field in meta.keys() {
            if !matches!(
                field.as_str(),
                "id" | "max_stack_size" | "feature_flags" | "crafting_remaining_item"
            ) {
                bail!("item `{name}` has unexpected field `{field}`");
            }
        }
        let id = meta
            .get("id")
            .and_then(Value::as_u64)
            .with_context(|| format!("item `{name}` is missing `id`"))?;
        let id = u16::try_from(id)
            .with_context(|| format!("item `{name}` has an id outside the u16 range"))?;

        let stack = meta
            .get("max_stack_size")
            .and_then(Value::as_u64)
            .with_context(|| format!("item `{name}` is missing `max_stack_size`"))?;
        if !(1..=64).contains(&stack) {
            bail!("item `{name}` has max_stack_size {stack} outside the 1..=64 range");
        }

        let flags = meta
            .get("feature_flags")
            .and_then(Value::as_array)
            .with_context(|| format!("item `{name}` is missing `feature_flags`"))?;
        if flags.is_empty() {
            bail!("item `{name}` has an empty `feature_flags` list");
        }
        for flag in flags {
            let flag = flag
                .as_str()
                .with_context(|| format!("item `{name}` has a non-string feature flag"))?;
            crate::registries::validate_name("minecraft:feature_flag", flag)?;
        }

        if let Some(rem) = meta.get("crafting_remaining_item") {
            rem.as_str().with_context(|| {
                format!("item `{name}` has a non-string `crafting_remaining_item`")
            })?;
        }

        entries.push(ItemEntry {
            name: name.clone(),
            id,
        });
    }
    entries.sort_unstable_by_key(|e| e.id);
    for (i, e) in entries.iter().enumerate() {
        if e.id as usize != i {
            bail!(
                "item ids are not contiguous 0..{}: expected {} at index {i}, got {}",
                entries.len(),
                i,
                e.id
            );
        }
    }
    // Names are unique by construction (a JSON object's keys), and every
    // `crafting_remaining_item` must reference an existing item.
    let existing: std::collections::HashSet<&str> =
        entries.iter().map(|e| e.name.as_str()).collect();
    for (name, meta) in items_obj {
        if let Some(Value::String(rem)) = meta.get("crafting_remaining_item")
            && !existing.contains(rem.as_str())
        {
            bail!("item `{name}` has a crafting_remaining_item `{rem}` not in the item table");
        }
    }
    Ok(entries)
}

fn validate_recipes(recipes_obj: &Map<String, Value>) -> Result<()> {
    for (name, recipe) in recipes_obj {
        crate::registries::validate_name("minecraft:recipe", name)?;
        let recipe = recipe
            .as_object()
            .with_context(|| format!("recipe `{name}` must be a JSON object"))?;
        let rtype = recipe
            .get("type")
            .and_then(Value::as_str)
            .with_context(|| format!("recipe `{name}` is missing a `type`"))?;
        crate::registries::validate_name("minecraft:recipe_type", rtype)?;
        if !RECIPE_TYPES.contains(&rtype) {
            bail!(
                "recipe `{name}` has unknown type `{rtype}` (a different jar or a new recipe type)"
            );
        }
        // Allow-list the serialized fields per recipe type, so a hand-edited
        // fixture that injects a made-up field (or an extractor that silently
        // drops a field) fails here instead of passing an opaque blob through.
        let allowed = RECIPE_FIELDS
            .iter()
            .find(|(t, _)| *t == rtype)
            .map(|(_, fields)| *fields)
            .with_context(|| format!("recipe type `{rtype}` has no allow-listed fields"))?;
        for field in recipe.keys() {
            // The `type` discriminator is shared by every type and is not part
            // of the per-type field allow-list.
            if field != "type" && !allowed.contains(&field.as_str()) {
                bail!(
                    "recipe `{name}` (type `{rtype}`) has unexpected field `{field}` (a different \
                     jar or a new Recipe.CODEC field)"
                );
            }
        }
    }
    Ok(())
}

/// The `probe` counts must equal the live-Paper anchors.
fn check_anchors(root: &Value) -> Result<()> {
    let probe = root
        .get("probe")
        .and_then(Value::as_object)
        .context("missing `probe`")?;
    for (key, expected) in ANCHORS {
        let found = probe
            .get(*key)
            .and_then(Value::as_u64)
            .with_context(|| format!("probe is missing `{key}`"))?;
        if found != *expected as u64 {
            bail!("probe drift: `{key}={found}` does not match the live-Paper anchor {expected}");
        }
    }
    Ok(())
}

/// Cross-check the fixture's item id table against the pinned report's
/// `minecraft:item` surface — the live-load registration order cannot drift
/// from the datagen capture.
// RivetTodo(#186): the report cross-check below already covers the item id table
// against the surface that feeds the generated `ITEM_BY_*` tables; when #186
// emits the item behavioral metadata + recipes into
// crates/rivet-registry/src/generated/, this cross-check should extend to that
// emitted surface (the generated table is what consumers link against).
fn cross_check_item_report(root: &Value) -> Result<()> {
    let report = read_item_report_surface()?;
    let items = root
        .get("items")
        .and_then(Value::as_object)
        .context("fixture is missing `items`")?;
    cross_check_items(items, &report)
}

/// Same-length name/id cross-check (injectable for tests).
fn cross_check_items(items: &Map<String, Value>, report: &HashMap<String, u32>) -> Result<()> {
    if items.len() != report.len() {
        bail!(
            "fixture item table has {} entries but the report has {}",
            items.len(),
            report.len()
        );
    }
    for (name, meta) in items {
        let id = meta
            .get("id")
            .and_then(Value::as_u64)
            .with_context(|| format!("item `{name}` is missing an `id`"))?;
        match report.get(name) {
            Some(&rid) if rid as u64 == id => {}
            Some(&rid) => {
                bail!("fixture/report id mismatch for `{name}`: fixture {id} vs report {rid}")
            }
            None => {
                bail!("fixture item `{name}` is absent from the report surface `minecraft:item`")
            }
        }
    }
    Ok(())
}

/// The `minecraft:item` name -> protocol_id surface from the pinned
/// `data/reports/registries.json` report.
fn read_item_report_surface() -> Result<HashMap<String, u32>> {
    // Reuse the biomes/tags generator's report reader so the two cross-checks
    // cannot drift from each other: `minecraft:item` is one of its
    // `REPORT_CROSSCHECKED` surfaces, so the surface is guaranteed present.
    let surfaces = crate::biomes_tags::read_report_surfaces()?;
    surfaces
        .get("minecraft:item")
        .cloned()
        .context("biomes_tags report surfaces are missing `minecraft:item`")
}

/// Link the fixture to its pinned provenance: the fixture must match the sha256
/// recorded next to it in `data/items_recipes.manifest.json`.
fn load_provenance(input: &Path) -> Result<crate::reports::SourceProvenance> {
    let manifest_path = input.with_extension("manifest.json");
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
            "items_recipes.json does not match its provenance manifest (expected sha256 {}, got {}) — \
             run `rivet-codegen extract-items-recipes` to refresh the pinned fixture",
            manifest.file.sha256,
            actual
        );
    }
    Ok(manifest.source)
}

pub fn run(bundler_flag: Option<&Path>, output_flag: Option<&Path>) -> Result<()> {
    let repo_root = extract::find_repo_root()?;
    let bundler = match bundler_flag {
        Some(p) => p.to_path_buf(),
        None => extract::default_bundler(&repo_root),
    };
    anyhow::ensure!(
        bundler.is_file(),
        "bundler jar not found at {} — pass --bundler or build Paper first (working/Paper/paper-server/build/libs)",
        bundler.display()
    );
    let output = match output_flag {
        Some(p) => p.to_path_buf(),
        None => default_output(&repo_root),
    };
    if let Some(dir) = output.parent() {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }

    run_extractor(&repo_root, &bundler, &output)?;
    // Validate the freshly written fixture before pinning it: structural +
    // report + anchors. (The manifest sha256 check inside validate_items_recipes
    // is deliberately not run here — the manifest next to `output` still records
    // the *previous* fixture's hash, so a refresh would fail against it. The
    // structural/anchor/report checks are the shape gate; the manifest read-back
    // below is the pin gate.)
    validate_items_recipes_str(
        &fs::read_to_string(&output).with_context(|| format!("read {}", output.display()))?,
    )?;
    write_manifest(&repo_root, &output, &bundler)?;
    // Read the manifest back and assert the fixture matches its recorded sha256
    // — provenance pinning is read-verified, not just written. (write_manifest
    // skips writing when no source jar is resolvable, so only verify when the
    // manifest exists.)
    let manifest_path = output.with_extension("manifest.json");
    if manifest_path.is_file() {
        load_provenance(&output)?;
    }
    println!(
        "Wrote item metadata + canonical recipes ({} bytes) to {}",
        fs::metadata(&output).map(|m| m.len()).unwrap_or(0),
        output.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A minimal fixture that passes `validate_structural` (dense ids, valid
    /// metadata, known recipe type, internally consistent probe) but not the
    /// report/anchor cross-checks (which only the real fixture passes).
    fn fixture() -> Value {
        serde_json::json!({
            "generator": GENERATOR_LABEL,
            "minecraft_version": "26.2",
            "protocol_version": 776,
            "world_version": 4903,
            "items": {
                "minecraft:air": {
                    "id": 0,
                    "max_stack_size": 64,
                    "feature_flags": ["minecraft:vanilla"]
                },
                "minecraft:stone": {
                    "id": 1,
                    "max_stack_size": 64,
                    "feature_flags": ["minecraft:vanilla"]
                }
            },
            "recipes": {
                "minecraft:acacia_boat": {
                    "type": "minecraft:crafting_shaped",
                    "group": "boat",
                    "pattern": ["# #", "###"],
                    "key": {"#": "minecraft:acacia_planks"},
                    "result": {"id": "minecraft:acacia_boat"}
                }
            },
            "probe": {
                "item_count": 2,
                "recipe_count": 1,
                "recipe_file_count": 1
            }
        })
    }

    /// The minimal fixture must satisfy the structural checks (dense ids, item
    /// metadata shape, recipe type, probe internal consistency) but not the
    /// report/anchor cross-checks (which only the real fixture passes). The
    /// mutation tests drive `validate_structural`; the report path gets its own
    /// test.
    fn structural(v: &Value) {
        validate_structural(v).unwrap();
    }

    #[test]
    fn minimal_fixture_is_structurally_valid() {
        structural(&fixture());
    }

    #[test]
    fn real_fixture_validates() {
        // The committed fixture is the ground truth: it must pass structural,
        // anchor, and report cross-checks. This is the fixture-pinned
        // conformance test (the `probe-items-recipes` subcommand is the live
        // half).
        let repo_root = extract::find_repo_root().unwrap();
        let path = default_output(&repo_root);
        if !path.is_file() {
            panic!(
                "committed {} missing — run `rivet-codegen extract-items-recipes` and commit it",
                path.display()
            );
        }
        validate_items_recipes(&path).unwrap();
    }

    #[test]
    fn structural_rejects_unknown_top_level_field() {
        let mut v = fixture();
        v.as_object_mut()
            .unwrap()
            .insert("bogus".into(), serde_json::json!(1));
        let err = validate_structural(&v).unwrap_err();
        assert!(
            err.to_string().contains("unexpected top-level field"),
            "got: {err}"
        );
    }

    #[test]
    fn structural_rejects_unknown_item_field() {
        let mut v = fixture();
        v["items"]["minecraft:air"]
            .as_object_mut()
            .unwrap()
            .insert("enchantments".into(), serde_json::json!([]));
        let err = validate_structural(&v).unwrap_err();
        assert!(
            err.to_string().contains("unexpected field `enchantments`"),
            "got: {err}"
        );
    }

    #[test]
    fn structural_rejects_sparse_item_ids() {
        let mut v = fixture();
        v["items"]["minecraft:dirt"] = serde_json::json!({
            "id": 3, "max_stack_size": 64, "feature_flags": ["minecraft:vanilla"]
        });
        let err = validate_structural(&v).unwrap_err();
        assert!(err.to_string().contains("not contiguous"), "got: {err}");
    }

    #[test]
    fn structural_rejects_unknown_recipe_type() {
        let mut v = fixture();
        v["recipes"]["minecraft:acacia_boat"]["type"] = serde_json::json!("minecraft:made_up");
        let err = validate_structural(&v).unwrap_err();
        assert!(
            err.to_string().contains("unknown type `minecraft:made_up`"),
            "got: {err}"
        );
    }

    #[test]
    fn structural_rejects_probe_mismatch() {
        let mut v = fixture();
        v["probe"]["item_count"] = serde_json::json!(99);
        let err = validate_structural(&v).unwrap_err();
        assert!(err.to_string().contains("probe.item_count"), "got: {err}");
    }

    #[test]
    fn structural_rejects_empty_feature_flags() {
        let mut v = fixture();
        v["items"]["minecraft:air"]["feature_flags"] = serde_json::json!([]);
        let err = validate_structural(&v).unwrap_err();
        assert!(
            err.to_string().contains("empty `feature_flags`"),
            "got: {err}"
        );
    }

    #[test]
    fn structural_rejects_crafting_remainder_not_in_table() {
        let mut v = fixture();
        v["items"]["minecraft:stone"]["crafting_remaining_item"] =
            serde_json::json!("minecraft:not_an_item");
        let err = validate_structural(&v).unwrap_err();
        assert!(
            err.to_string().contains("not in the item table"),
            "got: {err}"
        );
    }

    #[test]
    fn cross_check_detects_report_mismatch() {
        let mut items = Map::new();
        items.insert("minecraft:air".to_string(), serde_json::json!({"id": 0}));
        items.insert("minecraft:stone".to_string(), serde_json::json!({"id": 1}));
        let mut report = HashMap::new();
        report.insert("minecraft:air".to_string(), 0u32);
        report.insert("minecraft:stone".to_string(), 2u32); // drift
        let err = cross_check_items(&items, &report).unwrap_err();
        assert!(err.to_string().contains("id mismatch"), "got: {err}");

        // Same lengths, one name differing -> "absent from the report".
        let mut renamed = HashMap::new();
        renamed.insert("minecraft:air".to_string(), 0u32);
        renamed.insert("minecraft:dirt".to_string(), 1u32);
        let err = cross_check_items(&items, &renamed).unwrap_err();
        assert!(
            err.to_string().contains("absent from the report"),
            "got: {err}"
        );
    }

    #[test]
    fn cross_check_rejects_length_mismatch() {
        let mut items = Map::new();
        items.insert("minecraft:air".to_string(), serde_json::json!({"id": 0}));
        let mut report = HashMap::new();
        report.insert("minecraft:air".to_string(), 0u32);
        report.insert("minecraft:stone".to_string(), 1u32);
        let err = cross_check_items(&items, &report).unwrap_err();
        assert!(
            err.to_string()
                .contains("has 1 entries but the report has 2"),
            "got: {err}"
        );
    }

    #[test]
    fn anchors_cover_the_live_paper_counts() {
        // The anchor set must stay in lockstep with what `probe-items-recipes`
        // and the Java helper record.
        assert_eq!(
            ANCHORS,
            &[
                ("item_count", 1537),
                ("recipe_count", 1585),
                ("recipe_file_count", 1585),
            ]
        );
    }

    #[test]
    fn recipe_types_cover_the_live_paper_serials() {
        // Every recipe in the committed fixture must resolve against
        // RECIPE_TYPES (the anchor test). Guard against an accidental empty
        // or duplicated set.
        let uniq: HashSet<&&str> = RECIPE_TYPES.iter().collect();
        assert_eq!(uniq.len(), RECIPE_TYPES.len());
        let repo_root = extract::find_repo_root().unwrap();
        let path = default_output(&repo_root);
        if !path.is_file() {
            return;
        }
        let json = fs::read_to_string(&path).unwrap();
        let root: Value = serde_json::from_str(&json).unwrap();
        validate_recipes(root["recipes"].as_object().unwrap()).unwrap();
    }

    #[test]
    fn recipe_field_allowlists_cover_every_live_recipe() {
        // The RECIPE_FIELDS allow-list must be exactly the fields the committed
        // fixture's recipes carry (a field the fixture gained that the list
        // forgot fails here — and the fixture's own validation in
        // `real_fixture_validates` catches the reverse).
        let repo_root = extract::find_repo_root().unwrap();
        let path = default_output(&repo_root);
        if !path.is_file() {
            return;
        }
        let json = fs::read_to_string(&path).unwrap();
        let root: Value = serde_json::from_str(&json).unwrap();
        let recipes = root["recipes"].as_object().unwrap();
        // Every type in RECIPE_FIELDS must appear in the fixture, and vice versa.
        let allowlisted: HashSet<&&str> = RECIPE_FIELDS.iter().map(|(t, _)| t).collect();
        let present: HashSet<&str> = recipes
            .values()
            .filter_map(|r| r["type"].as_str())
            .collect();
        assert_eq!(
            allowlisted.len(),
            present.len(),
            "RECIPE_FIELDS must cover exactly the live recipe types"
        );
        for (t, _) in RECIPE_FIELDS {
            assert!(
                present.contains(t),
                "RECIPE_FIELDS lists `{t}` but the fixture has no such recipe"
            );
        }
        // The exact field sets match (already enforced per-type by
        // `validate_recipes` over the committed fixture, but assert the allow-list
        // is exactly as large as the union the fixture uses).
        let mut fixture_union: Vec<String> = recipes
            .values()
            .flat_map(|r| {
                r.as_object()
                    .unwrap()
                    .keys()
                    .filter(|k| *k != "type")
                    .map(|k| k.to_string())
                    .collect::<Vec<_>>()
            })
            .collect();
        fixture_union.sort();
        fixture_union.dedup();
        let mut allow_union: Vec<String> = RECIPE_FIELDS
            .iter()
            .flat_map(|(_, fields)| fields.iter().map(|f| f.to_string()))
            .collect();
        allow_union.sort();
        allow_union.dedup();
        assert_eq!(allow_union, fixture_union);
    }

    #[test]
    fn structural_rejects_unknown_recipe_field() {
        let mut v = fixture();
        v["recipes"]["minecraft:acacia_boat"]["bogus"] = serde_json::json!(1);
        let err = validate_structural(&v).unwrap_err();
        assert!(
            err.to_string().contains("unexpected field `bogus`"),
            "got: {err}"
        );
    }

    #[test]
    fn committed_fixture_carries_the_canonical_generator_label() {
        // The generator label is pinned by GENERATOR_LABEL; a fixture that was
        // produced by a helper writing a different label (or hand-edited) fails
        // the structural `generator`-field check via the allow-list + the probe,
        // so this asserts the committed fixture matches the canonical label.
        let repo_root = extract::find_repo_root().unwrap();
        let path = default_output(&repo_root);
        if !path.is_file() {
            return;
        }
        let json = fs::read_to_string(&path).unwrap();
        let root: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            root["generator"].as_str(),
            Some(GENERATOR_LABEL),
            "committed fixture's generator label drifted from GENERATOR_LABEL"
        );
    }

    #[test]
    fn manifest_sha256_mismatch_is_rejected() {
        // The manifest sha256 verification is read-verified: writing a fixture
        // then a manifest recording a *different* hash must fail
        // `load_provenance` (proving the pin is consumed, not write-only).
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("items_recipes.json");
        let manifest_path = output.with_extension("manifest.json");
        fs::write(&output, serde_json::to_vec_pretty(&fixture()).unwrap()).unwrap();
        let manifest = serde_json::json!({
            "generator": GENERATOR_LABEL,
            "source": {
                "jar": "x",
                "jar_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "minecraft_version": "26.2",
                "protocol_version": 776,
                "world_version": 4903
            },
            "file": {
                "bytes": 0,
                "sha256": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
            }
        });
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let err = load_provenance(&output).unwrap_err();
        assert!(
            err.to_string()
                .contains("does not match its provenance manifest"),
            "got: {err}"
        );
    }

    /// Full determinism check against the real Paper bundler jar: run the
    /// extractor twice and require byte-identity of the fixture. Requires the
    /// bundler jar (a Paper build); opt-in via `cargo test -- --ignored` — the
    /// `probe-items-recipes` subcommand and `real_fixture_validates` are the
    /// always-on guards.
    #[test]
    #[ignore = "requires the paper-bundler jar (build working/Paper or pass --bundler)"]
    fn extract_items_recipes_are_byte_stable_across_runs() {
        let repo_root = extract::find_repo_root().unwrap();
        let bundler = extract::default_bundler(&repo_root);
        assert!(
            bundler.is_file(),
            "bundler jar not found at {} — build Paper first",
            bundler.display()
        );
        let cache = repo_root.join("tools/rivet-codegen/.cache");
        fs::create_dir_all(&cache).unwrap();
        let a = cache.join("itest-items-a.json");
        let b = cache.join("itest-items-b.json");
        run_extractor(&repo_root, &bundler, &a).unwrap();
        run_extractor(&repo_root, &bundler, &b).unwrap();
        let bytes_a = fs::read(&a).unwrap();
        let bytes_b = fs::read(&b).unwrap();
        assert_eq!(
            bytes_a, bytes_b,
            "items_recipes.json differs between two independent extractor runs"
        );
        let _ = fs::remove_file(&a);
        let _ = fs::remove_file(&b);
    }
}
