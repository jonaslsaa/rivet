//! `rivet-codegen generate` block-behaviors half — consume the pinned live-Paper
//! behavior dump `data/block_behaviors.json` (see [`crate::extract_block_behaviors`])
//! and emit the compact run-length-encoded per-`StateId` behavior table into
//! `crates/rivet-registry/src/generated/block_behaviors.rs` (issue #228).
//!
//! # Ground truth
//!
//! The fixture is the real `Block.BLOCK_STATE_REGISTRY` dump evaluated by
//! `BlockBehaviourProbe` against the pinned Paper 26.2 jar: for every state id
//! in 0..32366 it packs the worldgen/heightmap/lighting behaviors into a 32-bit
//! word (bit layout documented in the probe and re-emitted here), then RLEs
//! consecutive equal words. It also records the exact per-direction
//! `SupportType.FULL.isSupporting` result, which delegates to
//! `getBlockSupportShape`, as a six-bit mask and RLEs those masks independently.
//! The `BlockState` newtype in `rivet-registry` decodes both tables; no behavior
//! is hand-typed.
//!
//! Validation: both run tables must partition `0..state_count` densely (first starts
//! at 0, starts strictly increasing, lengths positive, no overlap, sum ==
//! count), every run's start/length must fit the emitted `(u16, u16, u32)`
//! tuple and lie within `state_count`, the accumulation is overflow-checked,
//! the word's reserved bits (27..32) must be zero, and the fixture must match
//! its provenance manifest sha256. `state_count` is pinned to 32366 (the
//! emitted `BLOCK_STATE_COUNT`) by the live probe.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::model::BlockRegistry;
use crate::reports::SourceProvenance;

/// The canonical pinned behavior-table fixture.
pub fn default_input(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/block_behaviors.json")
}

/// Tables are written into the same committed `generated/` dir as the block
/// tables (the golden drift test in [`crate::generate`] asserts that dir
/// contains exactly the generated files).
pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("crates/rivet-registry/src/generated")
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

    let (runs, face_sturdy_runs) = validate(root)?;
    // The behavior table must span exactly the same state space as the
    // block-state registry it indexes: a behavior dump regenerated from a
    // differently-sized registry would leave real states silently decoding as
    // air (`behavior_of` falls back to `BLOCK_BEHAVIOR_RUNS[0]`). The registry
    // total is re-derived from the extract artifact so this cannot pass a
    // self-consistent-but-stale pair of fixtures.
    let block_state_count = registry_state_count(&repo_root)?;
    check_state_count_matches(&runs, block_state_count)?;
    let source = load_provenance(&input)?;

    fs::create_dir_all(&output).with_context(|| format!("create {}", output.display()))?;
    fs::write(
        output.join("block_behaviors.rs"),
        render(&runs, &face_sturdy_runs, &source),
    )
    .context("write generated/block_behaviors.rs")?;

    println!(
        "Wrote {} behavior runs across {} states -> {}",
        runs.len(),
        runs.iter().map(|r| r.length).sum::<u32>(),
        output.display()
    );
    Ok(())
}

/// One RLE run: states `[start, start + length)` share `word`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Run {
    start: u32,
    length: u32,
    word: u32,
}

/// One RLE run of Paper's per-direction `SupportType.FULL` face mask.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FaceSturdyRun {
    start: u32,
    length: u32,
    mask: u8,
}

/// The total number of block states in the registry (`BLOCK_STATE_COUNT`),
/// re-derived from the extract artifact `block_states.json`: each block
/// contributes the product of its property value counts (a single-state block
/// has no properties, so a product of zero factors is `1`). `block_states`
/// pins this product to the report's contiguous per-block state ranges, so it
/// equals the emitted `BLOCK_STATE_COUNT`.
fn registry_state_count(repo_root: &Path) -> Result<u32> {
    let input = crate::generate::default_input(repo_root);
    let json = fs::read_to_string(&input).with_context(|| format!("read {}", input.display()))?;
    let registry: BlockRegistry =
        serde_json::from_str(&json).with_context(|| format!("parse {}", input.display()))?;
    Ok(registry
        .blocks
        .iter()
        .map(|b| {
            b.properties
                .iter()
                .map(|p| p.values.len() as u32)
                .product::<u32>()
                .max(1) // a block with no properties is a single state
        })
        .sum())
}

/// The behavior runs must cover exactly `block_state_count` states (the
/// registry's `BLOCK_STATE_COUNT`). A mismatch means the two tables were
/// regenerated from different-sized registries, so generation must fail rather
/// than emit a table whose uncovered tail silently decodes as air.
fn check_state_count_matches(runs: &[Run], block_state_count: u32) -> Result<()> {
    let behavior_count: u64 = runs.iter().map(|r| r.length as u64).sum();
    if behavior_count != block_state_count as u64 {
        bail!(
            "block_behaviors.json covers {behavior_count} states but block_states.json defines \
             {block_state_count} (BLOCK_STATE_COUNT) — regenerate both from the same jar"
        );
    }
    Ok(())
}

/// Structural + oracle-conformance validation of `block_behaviors.json`. Fails
/// on: a missing/malformed/non-integer field, a `state_count` that does not fit
/// u16 (the emitted run tuple width), a non-positive run length, a run
/// start/length that does not fit u32 (rejected before the lossy cast), a run
/// that extends past `state_count`, a first run not starting at 0,
/// non-strictly-increasing run starts, runs that overlap or leave a gap, a
/// total that does not equal `state_count`, `word` with reserved bits set, or a
/// word field outside its documented bit width.
fn validate(root: Value) -> Result<(Vec<Run>, Vec<FaceSturdyRun>)> {
    let obj = root
        .as_object()
        .context("block_behaviors.json root must be a JSON object")?;
    let state_count = obj
        .get("state_count")
        .and_then(Value::as_u64)
        .with_context(|| "`state_count` must be a non-negative integer")?;
    // The emitted runs are `(u16, u16, u32)` tuples; `state_count` is the upper
    // bound on every run's start and length, so it must fit u16. Checked up
    // front so a hostile total is rejected before any per-run work.
    u16::try_from(state_count).context("state_count does not fit u16 (emitted runs are u16)")?;

    let runs_value = obj
        .get("runs")
        .context("`runs` missing from block_behaviors.json")?;
    let runs_value = runs_value.as_array().context("`runs` must be an array")?;
    if runs_value.is_empty() {
        bail!("`runs` is empty");
    }

    let mut runs = Vec::with_capacity(runs_value.len());
    let mut expected_start = 0u64;
    for (i, run) in runs_value.iter().enumerate() {
        let run_obj = run
            .as_object()
            .with_context(|| format!("run {i} must be a JSON object"))?;
        for field in run_obj.keys() {
            if !matches!(field.as_str(), "start" | "length" | "word") {
                bail!("run {i} has unexpected field `{field}`");
            }
        }
        let start = run_obj
            .get("start")
            .and_then(Value::as_u64)
            .with_context(|| format!("run {i} `start` must be a non-negative integer"))?;
        let length = run_obj
            .get("length")
            .and_then(Value::as_u64)
            .with_context(|| format!("run {i} `length` must be a non-negative integer"))?;
        if length == 0 {
            bail!("run {i} has zero length");
        }
        // Reject values that cannot be represented in the emitted `(u16, u16,
        // u32)` tuples before the lossy casts below — a hostile fixture must
        // fail cleanly, never truncate.
        if start > u32::MAX as u64 {
            bail!("run {i} start {start} exceeds u32");
        }
        if length > u32::MAX as u64 {
            bail!("run {i} length {length} exceeds u32");
        }
        let word = run_obj
            .get("word")
            .and_then(Value::as_u64)
            .with_context(|| format!("run {i} `word` must be a non-negative integer"))?;
        // Reserved bits 27..32 must be zero — a probe emitting a field outside
        // its documented width would otherwise be silently dropped.
        if word >> 27 != 0 {
            bail!("run {i} word {word} has reserved bits set");
        }
        // The 3-bit `fluid_id` field admits 0..=7, but the registry only
        // defines the five built-in fluids (0..=4) — assert the narrower
        // contract here (the probe pins it too) so a fixture with `fluid_id`
        // 5..=7 is rejected rather than silently accepted.
        if (word >> 24) & 0x7 > 4 {
            bail!("run {i} word {word} has fluid_id out of 0..4");
        }
        // The remaining fields (light_dampening, light_emission <= 15,
        // map_color <= 63, is_solid, can_be_replaced) are implied by their
        // widths, so no further field-level bound is worth asserting here.

        if start != expected_start {
            bail!(
                "runs do not densely partition 0..{state_count}: run {i} starts at {start} but \
                 the previous run ends at {expected_start}"
            );
        }
        // A run may not extend past the registry total, and neither the run's
        // end nor the accumulated start may overflow — use checked arithmetic
        // so a hostile fixture is rejected rather than panicking or wrapping.
        let end = start
            .checked_add(length)
            .with_context(|| format!("run {i} start+length overflows u64"))?;
        if end > state_count {
            bail!(
                "run {i} extends past state_count: [start, start+length) = [{start}, {end}) \
                 overruns {state_count}"
            );
        }
        expected_start = expected_start
            .checked_add(length)
            .with_context(|| format!("run {i} length overflows the accumulated start"))?;
        runs.push(Run {
            start: start as u32,
            length: length as u32,
            word: word as u32,
        });
    }
    if expected_start != state_count {
        bail!("runs cover [0, {expected_start}) but state_count is {state_count}");
    }

    let face_sturdy_runs = validate_face_sturdy_runs(
        obj.get("face_sturdy_runs")
            .context("`face_sturdy_runs` missing from block_behaviors.json")?,
        state_count,
    )?;

    Ok((runs, face_sturdy_runs))
}

/// Validate Paper's per-direction `SupportType.FULL` face-mask runs.
fn validate_face_sturdy_runs(value: &Value, state_count: u64) -> Result<Vec<FaceSturdyRun>> {
    let runs_value = value
        .as_array()
        .context("`face_sturdy_runs` must be an array")?;
    if runs_value.is_empty() {
        bail!("`face_sturdy_runs` is empty");
    }

    let mut runs = Vec::with_capacity(runs_value.len());
    let mut expected_start = 0u64;
    for (i, run) in runs_value.iter().enumerate() {
        let run_obj = run
            .as_object()
            .with_context(|| format!("face_sturdy_runs run {i} must be a JSON object"))?;
        for field in run_obj.keys() {
            if !matches!(field.as_str(), "start" | "length" | "mask") {
                bail!("face_sturdy_runs run {i} has unexpected field `{field}`");
            }
        }
        let start = run_obj
            .get("start")
            .and_then(Value::as_u64)
            .with_context(|| {
                format!("face_sturdy_runs run {i} `start` must be a non-negative integer")
            })?;
        let length = run_obj
            .get("length")
            .and_then(Value::as_u64)
            .with_context(|| {
                format!("face_sturdy_runs run {i} `length` must be a non-negative integer")
            })?;
        if length == 0 {
            bail!("face_sturdy_runs run {i} has zero length");
        }
        let mask = run_obj
            .get("mask")
            .and_then(Value::as_u64)
            .with_context(|| {
                format!("face_sturdy_runs run {i} `mask` must be a non-negative integer")
            })?;
        if mask > 0x3F {
            bail!("face_sturdy_runs run {i} mask {mask} has bits outside six directions");
        }
        if start > u32::MAX as u64 || length > u32::MAX as u64 {
            bail!("face_sturdy_runs run {i} start/length exceeds u32");
        }
        if start != expected_start {
            bail!(
                "face_sturdy_runs do not densely partition 0..{state_count}: run {i} starts at {start} but the previous run ends at {expected_start}"
            );
        }
        let end = start
            .checked_add(length)
            .with_context(|| format!("face_sturdy_runs run {i} start+length overflows u64"))?;
        if end > state_count {
            bail!(
                "face_sturdy_runs run {i} extends past state_count: [{start}, {end}) overruns {state_count}"
            );
        }
        expected_start = expected_start.checked_add(length).with_context(|| {
            format!("face_sturdy_runs run {i} length overflows accumulated start")
        })?;
        runs.push(FaceSturdyRun {
            start: start as u32,
            length: length as u32,
            mask: mask as u8,
        });
    }
    if expected_start != state_count {
        bail!("face_sturdy_runs cover [0, {expected_start}) but state_count is {state_count}");
    }

    Ok(runs)
}

/// Link the fixture to its pinned provenance: the fixture must match the sha256
/// recorded next to it in `data/block_behaviors.manifest.json`, and the emitted
/// header carries that provenance (jar identity + MC/proto/world versions).
fn load_provenance(input: &Path) -> Result<SourceProvenance> {
    let manifest_path = input
        .parent()
        .map(|p| p.join("block_behaviors.manifest.json"))
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
            "block_behaviors.json does not match its provenance manifest (expected sha256 {}, got {}) — \
             run `rivet-codegen extract-block-behaviors` to refresh the pinned fixture",
            manifest.file.sha256,
            actual
        );
    }
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

/// Render `generated/block_behaviors.rs`.
fn render(runs: &[Run], face_sturdy_runs: &[FaceSturdyRun], source: &SourceProvenance) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by `tools/rivet-codegen generate` from data/block_behaviors.json\n\
         // (live Paper Block.BLOCK_STATE_REGISTRY dump; MC {}, protocol {}, world {}).\n\
         // Source jar sha256 {}; provenance linked to data/block_behaviors.manifest.json.\n\
         // Do not edit by hand — PORTING.md: registries/data are generated, not hand-ported.\n\n",
        source.minecraft_version,
        source.protocol_version,
        source.world_version,
        source.jar_sha256.get(..16).unwrap_or(&source.jar_sha256)
    ));
    out.push_str(
        "// Per-StateId worldgen/heightmap/lighting behavior words (issue #228). The\n\
         // values are Paper's cached state accessors evaluated by BlockBehaviourProbe\n\
         // against the real Block.BLOCK_STATE_REGISTRY — never hand-typed. Bit layout:\n\
         //   bit  0  is_air\n\
         //   bit  1  blocks_motion\n\
         //   bit  2  solid_render\n\
         //   bit  3  can_occlude\n\
         //   bit  4  use_shape_for_light_occlusion\n\
         //   bit  5  propagates_skylight_down\n\
         //   bit  6  random_ticking\n\
         //   bit  7  fluid_empty\n\
         //   bits  8-11   light_dampening (0..15)\n\
         //   bits  12-15  light_emission (0..15)\n\
         //   bits  16-21  map_color_id (0..63)\n\
         //   bit  22 is_solid (isSolid() — the cached legacySolid from calculateSolid():\n\
         //            non-empty collision-shape bounds volume >= 35/48 or ysize >= 1.0,\n\
         //            after the forceSolidOn/Off and dynamic-shape guards; NOT hasCollision)\n\
         //   bit  23 can_be_replaced (canBeReplaced() — Properties.replaceable)\n\
         //   bits  24-26  fluid_id (BuiltInRegistries.FLUID.getId(getFluidState().getType()), 0..4)\n\
         //   bits  27-31  reserved (always 0)\n\
         // Words are run-length encoded: each (start, length, word) covers states\n\
         // [start, start + length). Runs partition 0..BLOCK_STATE_COUNT and are sorted\n\
         // by start.\n\n",
    );
    // `StateId` is referenced by `behavior_of`; `BLOCK_STATE_COUNT` appears only
    // in doc comments, so importing it would trip the unused-import lint.
    out.push_str("use crate::generated::block_states::StateId;\n\n");

    for (name, expr, doc) in [
        ("BEHAVIOR_FLAG_IS_AIR", "1 << 0", "state is air"),
        (
            "BEHAVIOR_FLAG_BLOCKS_MOTION",
            "1 << 1",
            "state blocks motion (Heightmap OCEAN_FLOOR/MOTION_BLOCKING predicate)",
        ),
        (
            "BEHAVIOR_FLAG_SOLID_RENDER",
            "1 << 2",
            "state's occlusion shape is a full block",
        ),
        (
            "BEHAVIOR_FLAG_CAN_OCCLUDE",
            "1 << 3",
            "state can occlude light (canOcclude)",
        ),
        (
            "BEHAVIOR_FLAG_USE_SHAPE_FOR_LIGHT_OCCLUSION",
            "1 << 4",
            "light occlusion follows the non-full occlusion shape",
        ),
        (
            "BEHAVIOR_FLAG_PROPAGATES_SKYLIGHT_DOWN",
            "1 << 5",
            "sky light passes straight through",
        ),
        (
            "BEHAVIOR_FLAG_RANDOM_TICKING",
            "1 << 6",
            "state is random-ticked",
        ),
        (
            "BEHAVIOR_FLAG_FLUID_EMPTY",
            "1 << 7",
            "the state carries no fluid",
        ),
        (
            "BEHAVIOR_FLAG_IS_SOLID",
            "1 << 22",
            "state is solid (BlockStateBase.isSolid() — cached legacySolid: collision-shape \
             bounds volume >= 35/48 or ysize >= 1.0; SolidPredicate)",
        ),
        (
            "BEHAVIOR_FLAG_CAN_BE_REPLACED",
            "1 << 23",
            "state can be replaced (Properties.replaceable — ReplaceablePredicate)",
        ),
    ] {
        out.push_str(&format!(
            "/// {doc}.\n\
             pub const {name}: u32 = {expr};\n"
        ));
    }
    out.push('\n');
    out.push_str(
        "/// Shift/width of the `light_dampening` field (0..15).\n\
         pub const BEHAVIOR_SHIFT_LIGHT_DAMPENING: u32 = 8;\n\
         /// Shift/width of the `light_emission` field (0..15).\n\
         pub const BEHAVIOR_SHIFT_LIGHT_EMISSION: u32 = 12;\n\
         /// Shift/width of the `map_color_id` field (0..63).\n\
         pub const BEHAVIOR_SHIFT_MAP_COLOR: u32 = 16;\n\
         /// Shift/width of the `fluid_id` field (0..4, the 5 built-in fluids).\n\
         pub const BEHAVIOR_SHIFT_FLUID_ID: u32 = 24;\n\
         pub const BEHAVIOR_MASK_LIGHT_DAMPENING: u32 = 0xF;\n\
         pub const BEHAVIOR_MASK_LIGHT_EMISSION: u32 = 0xF;\n\
         pub const BEHAVIOR_MASK_MAP_COLOR: u32 = 0x3F;\n\
         pub const BEHAVIOR_MASK_FLUID_ID: u32 = 0x7;\n\n",
    );

    out.push_str(
        "/// Run-length-encoded behavior words: `(start_state_id, length, word)`.\n\
         /// Runs partition `0..BLOCK_STATE_COUNT` and are sorted by start.\n\
         pub static BLOCK_BEHAVIOR_RUNS: &[(u16, u16, u32)] = &[\n",
    );
    for run in runs {
        out.push_str(&format!(
            "    ({}, {}, {:#X}),\n",
            run.start, run.length, run.word
        ));
    }
    out.push_str("];\n\n");

    out.push_str(
        "/// The behavior word for a state id. Ids outside `0..BLOCK_STATE_COUNT` fall\n\
         /// back to state 0 (air), mirroring `Block.stateById`.\n\
         pub fn behavior_of(id: StateId) -> u32 {\n\
             let id = id.0 as u32;\n\
             let idx = BLOCK_BEHAVIOR_RUNS.partition_point(|(start, _, _)| *start as u32 <= id);\n\
             if idx == 0 {\n\
                 return BLOCK_BEHAVIOR_RUNS[0].2;\n\
             }\n\
             let (start, len, word) = BLOCK_BEHAVIOR_RUNS[idx - 1];\n\
             if id < start as u32 + len as u32 {\n\
                 word\n\
             } else {\n\
                 BLOCK_BEHAVIOR_RUNS[0].2\n\
             }\n\
         }\n",
    );

    out.push_str(
        "/// Run-length-encoded Paper `SupportType.FULL` face masks. Bit order is\n\
         /// `Direction.values()` (`DOWN`, `UP`, `NORTH`, `SOUTH`, `WEST`, `EAST`).\n\
         pub static BLOCK_FACE_STURDY_RUNS: &[(u16, u16, u8)] = &[\n",
    );
    for run in face_sturdy_runs {
        out.push_str(&format!(
            "    ({}, {}, 0x{:02X}),\n",
            run.start, run.length, run.mask
        ));
    }
    out.push_str("];\n\n");
    out.push_str(
        "/// The Paper FULL-face support mask for a state id. Ids outside\n\
         /// `0..BLOCK_STATE_COUNT` fall back to state 0, mirroring\n\
         /// `Block.stateById`.\n\
         pub fn face_sturdy_mask_of(id: StateId) -> u8 {\n\
             let id = id.0 as u32;\n\
             let idx = BLOCK_FACE_STURDY_RUNS.partition_point(|(start, _, _)| *start as u32 <= id);\n\
             if idx == 0 {\n\
                 return BLOCK_FACE_STURDY_RUNS[0].2;\n\
             }\n\
             let (start, len, mask) = BLOCK_FACE_STURDY_RUNS[idx - 1];\n\
             if id < start as u32 + len as u32 {\n\
                 mask\n\
             } else {\n\
                 BLOCK_FACE_STURDY_RUNS[0].2\n\
             }\n\
         }\n",
    );

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_root() -> Value {
        serde_json::json!({
            "generator": "test",
            "minecraft_version": "26.2",
            "state_count": 5,
            "runs": [
                {"start": 0, "length": 2, "word": 1},
                {"start": 2, "length": 3, "word": 0x20015},
            ],
            "face_sturdy_runs": [
                {"start": 0, "length": 2, "mask": 0},
                {"start": 2, "length": 3, "mask": 63},
            ],
        })
    }

    #[test]
    fn dense_partition_passes() {
        let (runs, face_sturdy_runs) = validate(valid_root()).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[1].word, 0x20015);
        assert_eq!(face_sturdy_runs[1].mask, 63);
    }

    #[test]
    fn face_sturdy_mask_out_of_range_fails() {
        let mut v = valid_root();
        v["face_sturdy_runs"][0]["mask"] = serde_json::json!(64);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("six directions"), "got: {err}");
    }

    #[test]
    fn face_sturdy_runs_must_cover_all_states() {
        let mut v = valid_root();
        v["face_sturdy_runs"][1]["length"] = serde_json::json!(2);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("cover [0, 4)"), "got: {err}");
    }

    #[test]
    fn sparse_runs_fail() {
        let mut v = valid_root();
        v["runs"][1]["start"] = serde_json::json!(3);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("densely partition"), "got: {err}");
    }

    #[test]
    fn overlapping_runs_fail() {
        // First run longer than the second's expected start is overlap.
        let mut v = valid_root();
        v["runs"][0]["length"] = serde_json::json!(4);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("densely partition"), "got: {err}");
    }

    #[test]
    fn total_mismatch_fails() {
        let mut v = valid_root();
        v["state_count"] = serde_json::json!(6);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("state_count is 6"), "got: {err}");
    }

    #[test]
    fn zero_length_run_fails() {
        let mut v = valid_root();
        v["runs"][0]["length"] = serde_json::json!(0);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("zero length"), "got: {err}");
    }

    #[test]
    fn reserved_bits_fail() {
        // Bit 22 is `is_solid` (assigned for #180); the first truly reserved
        // bit is 27 (`word >> 27 != 0` is the validator's reserved-bits check).
        let mut v = valid_root();
        v["runs"][0]["word"] = serde_json::json!(1u64 << 27);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("reserved bits"), "got: {err}");
    }

    #[test]
    fn fluid_id_out_of_range_fails() {
        // The 3-bit `fluid_id` field admits 0..=7, but the registry only
        // defines 0..=4 — a fixture with fluid_id 5 must be rejected by the
        // explicit bound (the reserved-bits check alone can no longer catch
        // it, since bits 24..26 are a legitimate field).
        let mut v = valid_root();
        v["runs"][0]["word"] = serde_json::json!(5u64 << 24);
        let err = validate(v).unwrap_err();
        assert!(
            err.to_string().contains("fluid_id out of 0..4"),
            "got: {err}"
        );
    }

    #[test]
    fn unknown_field_fails() {
        let mut v = valid_root();
        v["runs"][0]["extra"] = serde_json::json!(1);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("unexpected field"), "got: {err}");
    }

    #[test]
    fn empty_runs_fail() {
        let mut v = valid_root();
        v["runs"] = serde_json::json!([]);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("empty"), "got: {err}");
    }

    #[test]
    fn huge_length_near_u64_max_fails() {
        // A length near u64::MAX must be rejected by the u32 bound before any
        // lossy cast — previously it truncated and then failed with a
        // misleading density error.
        let mut v = valid_root();
        v["runs"][0]["length"] = serde_json::json!(u64::MAX - 1);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("exceeds u32"), "got: {err}");
    }

    #[test]
    fn huge_start_near_u64_max_fails() {
        // A start near u64::MAX must be rejected by the u32 bound, not hit the
        // density check with a truncated value.
        let mut v = valid_root();
        v["runs"][1]["start"] = serde_json::json!(u64::MAX - 1);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("exceeds u32"), "got: {err}");
    }

    #[test]
    fn run_extending_past_state_count_fails() {
        // state_count is 5; the first run alone covers 6 states — an overrun,
        // not a gap, so it must fail the state_count bound rather than the
        // density check.
        let mut v = valid_root();
        v["runs"][0]["length"] = serde_json::json!(6);
        let err = validate(v).unwrap_err();
        assert!(
            err.to_string().contains("extends past state_count"),
            "got: {err}"
        );
    }

    #[test]
    fn state_count_beyond_u16_fails() {
        // The emitted runs are (u16, u16, u32); a total that cannot be spanned
        // by u16 run fields is rejected up front.
        let mut v = valid_root();
        v["state_count"] = serde_json::json!(1u64 << 40);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("does not fit u16"), "got: {err}");
    }

    #[test]
    fn run_touching_state_count_boundary_passes() {
        // A single run spanning exactly [0, state_count) is the maximal valid
        // partition — the over-state-count bound must not reject equality.
        let v = serde_json::json!({
            "generator": "test",
            "minecraft_version": "26.2",
            "state_count": 5,
            "runs": [
                {"start": 0, "length": 5, "word": 1},
            ],
            "face_sturdy_runs": [
                {"start": 0, "length": 5, "mask": 0},
            ],
        });
        let (runs, _) = validate(v).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].length, 5);
    }

    #[test]
    fn state_count_mismatch_with_registry_fails() {
        // Dense, self-consistent runs that span 5 states — but the registry
        // defines 6. `validate` alone cannot see this; the cross-check against
        // `block_states.json` is what catches a behavior dump regenerated from
        // a differently-sized registry.
        let runs = [
            Run {
                start: 0,
                length: 2,
                word: 0x1,
            },
            Run {
                start: 2,
                length: 3,
                word: 0x20015,
            },
        ];
        let err = check_state_count_matches(&runs, 6).unwrap_err();
        assert!(err.to_string().contains("covers 5 states"), "got: {err}");
        assert!(err.to_string().contains("defines 6"), "got: {err}");
    }

    #[test]
    fn state_count_match_passes() {
        let runs = [
            Run {
                start: 0,
                length: 2,
                word: 0x1,
            },
            Run {
                start: 2,
                length: 3,
                word: 0x20015,
            },
        ];
        check_state_count_matches(&runs, 5).unwrap();
    }

    #[test]
    fn rendering_is_deterministic_and_carries_provenance() {
        let source: SourceProvenance = serde_json::from_str(
            r#"{"jar":"paper-26.2.jar","jar_sha256":"e1a027e9481a16ec1da0f0e139d370280050d123a14c022a476c2dc8a697ebda","minecraft_version":"26.2","protocol_version":776,"world_version":4903}"#,
        )
        .unwrap();
        let runs = vec![
            Run {
                start: 0,
                length: 1,
                word: 0x3,
            },
            Run {
                start: 1,
                length: 2,
                word: 0x20000,
            },
        ];
        let face_sturdy_runs = vec![FaceSturdyRun {
            start: 0,
            length: 3,
            mask: 0x3F,
        }];
        let a = render(&runs, &face_sturdy_runs, &source);
        let b = render(&runs, &face_sturdy_runs, &source);
        assert_eq!(a, b);
        assert!(a.contains("MC 26.2, protocol 776, world 4903"));
        assert!(a.contains("e1a027e9481a16ec"));
        assert!(a.contains("(0, 1, 0x3)"));
        assert!(a.contains("(1, 2, 0x20000)"));
    }

    /// The emitted `behavior_of` binary search must reproduce the fixture words
    /// for every state in the real table (the RLE decode is the load-bearing
    /// consumer path). Walking all 32366 states — including both boundaries of
    /// every one of the 16757 runs and the out-of-range fallback — proves the
    /// `partition_point` decode has no off-by-one anywhere.
    #[test]
    fn behavior_of_matches_fixture_words() {
        let repo_root = crate::extract::find_repo_root().unwrap();
        let json = fs::read_to_string(default_input(&repo_root)).unwrap();
        let root = crate::registries::parse_strict(&json).unwrap();
        let (runs, _) = validate(root).unwrap();
        let state_count: u64 = runs.iter().map(|r| r.length as u64).sum();
        assert_eq!(state_count, 32366, "registry state count drifted");

        // A dense word array is the ground truth to decode against.
        let mut words = vec![0u32; state_count as usize];
        for r in &runs {
            for w in words[r.start as usize..(r.start + r.length) as usize].iter_mut() {
                *w = r.word;
            }
        }

        // The exact decode the emitted `behavior_of` performs: the last run
        // whose start is <= id, in-range check, else the air fallback.
        let decode = |id: u32| -> u32 {
            let idx = runs.partition_point(|r| r.start <= id);
            if idx == 0 {
                return runs[0].word;
            }
            let (start, len, word) = (
                runs[idx - 1].start,
                runs[idx - 1].length,
                runs[idx - 1].word,
            );
            if id < start + len { word } else { runs[0].word }
        };

        // Every state decodes to the fixture's own word.
        for id in 0..state_count {
            assert_eq!(decode(id as u32), words[id as usize], "state {id}");
        }
        // Out-of-range falls back to air's word (state 0).
        assert_eq!(decode(state_count as u32), words[0], "first out-of-range");
        assert_eq!(decode(u16::MAX as u32), words[0], "far out-of-range");
    }

    /// The real fixture must agree with `block_states.json` on the total state
    /// count (`BLOCK_STATE_COUNT`), so a behavior dump from a differently-sized
    /// registry fails `generate` even when it is internally self-consistent.
    #[test]
    fn real_fixture_state_count_matches_registry() {
        let repo_root = crate::extract::find_repo_root().unwrap();
        assert_eq!(registry_state_count(&repo_root).unwrap(), 32366);
        let json = fs::read_to_string(default_input(&repo_root)).unwrap();
        let root = crate::registries::parse_strict(&json).unwrap();
        let (runs, _) = validate(root).unwrap();
        check_state_count_matches(&runs, 32366).unwrap();
    }
}
