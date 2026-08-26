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
//! `getBlockSupportShape`, as a six-bit zero-context mask. It records the
//! per-direction full `SupportType.CENTER` and `SupportType.RIGID` masks, the
//! full collision-face mask used by `MultifaceBlock.canAttachTo`, and the full
//! occlusion-face mask at the same probe origin. For `hasDynamicShape` states,
//! these masks are static samples rather than authoritative production
//! semantics; issue #646 owns the live context-aware contract. The `BlockState`
//! newtype in `rivet-registry` decodes all tables; no behavior is hand-typed.
//!
//! Validation: every run table must partition `0..state_count` densely (first starts at
//! 0, starts strictly increasing, lengths positive, no overlap, sum == count), every run's
//! start/length must fit the emitted `(u16, u16, u32)` tuple and lie within `state_count`,
//! the accumulation is overflow-checked, the word's reserved bits (27..32) must be zero,
//! and the fixture must match its provenance manifest sha256. `state_count` is pinned to
//! 32366 (the emitted `BLOCK_STATE_COUNT`) by the live probe.

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

    let (
        runs,
        face_sturdy_runs,
        center_support_runs,
        rigid_support_runs,
        collision_face_runs,
        occlusion_face_runs,
        dynamic_shape_runs,
        collision_boxes,
        collision_shapes,
        collision_shape_ids,
        dynamic_fixtures,
    ) = validate(root)?;
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
        render(RenderInput {
            runs: &runs,
            face_sturdy_runs: &face_sturdy_runs,
            center_support_runs: &center_support_runs,
            rigid_support_runs: &rigid_support_runs,
            collision_face_runs: &collision_face_runs,
            occlusion_face_runs: &occlusion_face_runs,
            dynamic_shape_runs: &dynamic_shape_runs,
            collision_boxes: &collision_boxes,
            collision_shapes: &collision_shapes,
            collision_shape_ids: &collision_shape_ids,
            dynamic_fixtures: &dynamic_fixtures,
            source: &source,
        }),
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

/// One RLE run of Paper's per-direction full collision-face mask.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CollisionFaceRun {
    start: u32,
    length: u32,
    mask: u8,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DynamicShapeRun {
    start: u32,
    length: u32,
    dynamic: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CollisionBox {
    min_x: i8,
    min_y: i8,
    min_z: i8,
    max_x: i8,
    max_y: i8,
    max_z: i8,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CollisionShape {
    start: u16,
    length: u8,
}

#[derive(Debug, Clone)]
pub(crate) struct DynamicFixture {
    name: String,
    block: String,
    state_id: u32,
    dynamic: bool,
    support_full: u8,
    support_center: u8,
    support_rigid: u8,
    collision_full: u8,
    occlusion_full: u8,
}

type ValidatedTables = (
    Vec<Run>,
    Vec<FaceSturdyRun>,
    Vec<FaceSturdyRun>,
    Vec<FaceSturdyRun>,
    Vec<CollisionFaceRun>,
    Vec<FaceSturdyRun>,
    Vec<DynamicShapeRun>,
    Vec<CollisionBox>,
    Vec<CollisionShape>,
    Vec<u16>,
    Vec<DynamicFixture>,
);

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
/// word field outside its documented bit width. Both mask tables are subject
/// to the same dense-partition checks.
fn validate(root: Value) -> Result<ValidatedTables> {
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
    let center_support_runs = validate_face_sturdy_runs(
        obj.get("center_support_runs")
            .context("`center_support_runs` missing from block_behaviors.json")?,
        state_count,
    )?;
    let rigid_support_runs = validate_face_sturdy_runs(
        obj.get("rigid_support_runs")
            .context("`rigid_support_runs` missing from block_behaviors.json")?,
        state_count,
    )?;
    let collision_face_runs = validate_collision_face_runs(
        obj.get("collision_face_runs")
            .context("`collision_face_runs` missing from block_behaviors.json")?,
        state_count,
    )?;
    let occlusion_face_runs = validate_face_sturdy_runs(
        obj.get("occlusion_face_runs")
            .context("`occlusion_face_runs` missing from block_behaviors.json")?,
        state_count,
    )?;
    let dynamic_shape_runs = validate_dynamic_shape_runs(
        obj.get("dynamic_shape_runs")
            .context("`dynamic_shape_runs` missing from block_behaviors.json")?,
        state_count,
    )?;
    let (collision_boxes, collision_shapes, collision_shape_ids) = validate_collision_shapes(
        obj.get("collision_shapes")
            .context("`collision_shapes` missing from block_behaviors.json")?,
        obj.get("collision_shape_ids")
            .context("`collision_shape_ids` missing from block_behaviors.json")?,
        state_count,
        &dynamic_shape_runs,
    )?;
    let dynamic_fixtures = validate_dynamic_fixtures(
        obj.get("dynamic_fixtures")
            .context("`dynamic_fixtures` missing from block_behaviors.json")?,
        state_count,
    )?;

    Ok((
        runs,
        face_sturdy_runs,
        center_support_runs,
        rigid_support_runs,
        collision_face_runs,
        occlusion_face_runs,
        dynamic_shape_runs,
        collision_boxes,
        collision_shapes,
        collision_shape_ids,
        dynamic_fixtures,
    ))
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

/// Validate Paper's per-direction full collision-face mask runs.
fn validate_collision_face_runs(value: &Value, state_count: u64) -> Result<Vec<CollisionFaceRun>> {
    let runs_value = value
        .as_array()
        .context("`collision_face_runs` must be an array")?;
    if runs_value.is_empty() {
        bail!("`collision_face_runs` is empty");
    }

    let mut runs = Vec::with_capacity(runs_value.len());
    let mut expected_start = 0u64;
    for (i, run) in runs_value.iter().enumerate() {
        let run_obj = run
            .as_object()
            .with_context(|| format!("collision_face_runs run {i} must be a JSON object"))?;
        for field in run_obj.keys() {
            if !matches!(field.as_str(), "start" | "length" | "mask") {
                bail!("collision_face_runs run {i} has unexpected field `{field}`");
            }
        }
        let start = run_obj
            .get("start")
            .and_then(Value::as_u64)
            .with_context(|| {
                format!("collision_face_runs run {i} `start` must be a non-negative integer")
            })?;
        let length = run_obj
            .get("length")
            .and_then(Value::as_u64)
            .with_context(|| {
                format!("collision_face_runs run {i} `length` must be a non-negative integer")
            })?;
        if length == 0 {
            bail!("collision_face_runs run {i} has zero length");
        }
        let mask = run_obj
            .get("mask")
            .and_then(Value::as_u64)
            .with_context(|| {
                format!("collision_face_runs run {i} `mask` must be a non-negative integer")
            })?;
        if mask > 0x3F {
            bail!("collision_face_runs run {i} mask {mask} has bits outside six directions");
        }
        if start > u32::MAX as u64 || length > u32::MAX as u64 {
            bail!("collision_face_runs run {i} start/length exceeds u32");
        }
        if start != expected_start {
            bail!(
                "collision_face_runs do not densely partition 0..{state_count}: run {i} starts at {start} but the previous run ends at {expected_start}"
            );
        }
        let end = start
            .checked_add(length)
            .with_context(|| format!("collision_face_runs run {i} start+length overflows u64"))?;
        if end > state_count {
            bail!(
                "collision_face_runs run {i} extends past state_count: [{start}, {end}) overruns {state_count}"
            );
        }
        expected_start = expected_start.checked_add(length).with_context(|| {
            format!("collision_face_runs run {i} length overflows accumulated start")
        })?;
        runs.push(CollisionFaceRun {
            start: start as u32,
            length: length as u32,
            mask: mask as u8,
        });
    }
    if expected_start != state_count {
        bail!("collision_face_runs cover [0, {expected_start}) but state_count is {state_count}");
    }

    Ok(runs)
}

fn validate_collision_shapes(
    shapes_value: &Value,
    ids_value: &Value,
    state_count: u64,
    dynamic_shape_runs: &[DynamicShapeRun],
) -> Result<(Vec<CollisionBox>, Vec<CollisionShape>, Vec<u16>)> {
    let shapes = shapes_value
        .as_array()
        .context("`collision_shapes` must be an array")?;
    if shapes.is_empty() {
        bail!("`collision_shapes` is empty; shape 0 must be the empty shape");
    }
    if shapes.len() > u16::MAX as usize {
        bail!("`collision_shapes` has too many geometries for u16 shape ids");
    }

    let mut boxes = Vec::new();
    let mut shape_records = Vec::with_capacity(shapes.len());
    for (shape_id, value) in shapes.iter().enumerate() {
        let object = value
            .as_object()
            .with_context(|| format!("collision_shapes shape {shape_id} must be an object"))?;
        for field in object.keys() {
            if field != "boxes" {
                bail!("collision_shapes shape {shape_id} has unexpected field `{field}`");
            }
        }
        let shape_boxes = object
            .get("boxes")
            .and_then(Value::as_array)
            .with_context(|| {
                format!("collision_shapes shape {shape_id} `boxes` must be an array")
            })?;
        if shape_boxes.len() > u8::MAX as usize {
            bail!("collision_shapes shape {shape_id} has too many boxes");
        }
        if shape_id == 0 && !shape_boxes.is_empty() {
            bail!("collision_shapes shape 0 must be the empty shape");
        }
        let start = u16::try_from(boxes.len()).with_context(|| {
            format!("collision_shapes shape {shape_id} starts past the u16 box table")
        })?;
        for (box_id, value) in shape_boxes.iter().enumerate() {
            let object = value.as_object().with_context(|| {
                format!("collision_shapes shape {shape_id} box {box_id} must be an object")
            })?;
            for field in object.keys() {
                if !matches!(
                    field.as_str(),
                    "min_x" | "min_y" | "min_z" | "max_x" | "max_y" | "max_z"
                ) {
                    bail!(
                        "collision_shapes shape {shape_id} box {box_id} has unexpected field `{field}`"
                    );
                }
            }
            let coordinate = |field: &str| -> Result<i8> {
                let value = object.get(field).and_then(Value::as_i64).with_context(|| {
                    format!("collision_shapes shape {shape_id} box {box_id} `{field}` must be an integer")
                })?;
                if !(-8..=48).contains(&value) {
                    bail!(
                        "collision_shapes shape {shape_id} box {box_id} `{field}` {value} is outside [-8, 48]"
                    );
                }
                Ok(value as i8)
            };
            let min_x = coordinate("min_x")?;
            let min_y = coordinate("min_y")?;
            let min_z = coordinate("min_z")?;
            let max_x = coordinate("max_x")?;
            let max_y = coordinate("max_y")?;
            let max_z = coordinate("max_z")?;
            if min_x >= max_x || min_y >= max_y || min_z >= max_z {
                bail!("collision_shapes shape {shape_id} box {box_id} is empty or inverted");
            }
            boxes.push(CollisionBox {
                min_x,
                min_y,
                min_z,
                max_x,
                max_y,
                max_z,
            });
        }
        shape_records.push(CollisionShape {
            start,
            length: shape_boxes.len() as u8,
        });
    }

    let ids = ids_value
        .as_array()
        .context("`collision_shape_ids` must be an array")?;
    if ids.len() != state_count as usize {
        bail!(
            "collision_shape_ids has {} entries but state_count is {state_count}",
            ids.len()
        );
    }
    let mut shape_ids = Vec::with_capacity(ids.len());
    for (state_id, value) in ids.iter().enumerate() {
        let id = value
            .as_i64()
            .with_context(|| format!("collision_shape_ids state {state_id} must be an integer"))?;
        let dynamic = dynamic_shape_at(dynamic_shape_runs, state_id as u32);
        if dynamic {
            if id != -1 {
                bail!(
                    "collision_shape_ids state {state_id} is dynamic but has shape id {id}; dynamic states must use -1"
                );
            }
            shape_ids.push(u16::MAX);
        } else {
            let id = usize::try_from(id).with_context(|| {
                format!("collision_shape_ids state {state_id} has negative static shape id {id}")
            })?;
            if id >= shape_records.len() {
                bail!(
                    "collision_shape_ids state {state_id} shape id {id} is outside {} geometries",
                    shape_records.len()
                );
            }
            shape_ids.push(id as u16);
        }
    }
    Ok((boxes, shape_records, shape_ids))
}

fn dynamic_shape_at(runs: &[DynamicShapeRun], state_id: u32) -> bool {
    runs.iter().find_map(|run| {
        (state_id >= run.start && state_id < run.start + run.length).then_some(run.dynamic)
    }) == Some(true)
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

fn validate_dynamic_shape_runs(value: &Value, state_count: u64) -> Result<Vec<DynamicShapeRun>> {
    let runs_value = value
        .as_array()
        .context("`dynamic_shape_runs` must be an array")?;
    if runs_value.is_empty() {
        bail!("`dynamic_shape_runs` is empty");
    }
    let mut out = Vec::with_capacity(runs_value.len());
    let mut expected_start = 0u64;
    for (i, run) in runs_value.iter().enumerate() {
        let obj = run
            .as_object()
            .with_context(|| format!("dynamic_shape_runs run {i} must be a JSON object"))?;
        for field in obj.keys() {
            if !matches!(field.as_str(), "start" | "length" | "dynamic") {
                bail!("dynamic_shape_runs run {i} has unexpected field `{field}`");
            }
        }
        let start = obj
            .get("start")
            .and_then(Value::as_u64)
            .with_context(|| format!("dynamic_shape_runs run {i} `start` must be an integer"))?;
        let length = obj
            .get("length")
            .and_then(Value::as_u64)
            .with_context(|| format!("dynamic_shape_runs run {i} `length` must be an integer"))?;
        let dynamic = obj
            .get("dynamic")
            .and_then(Value::as_bool)
            .with_context(|| format!("dynamic_shape_runs run {i} `dynamic` must be boolean"))?;
        if length == 0 {
            bail!("dynamic_shape_runs run {i} has zero length");
        }
        if start != expected_start {
            bail!(
                "dynamic_shape_runs do not densely partition 0..{state_count}: run {i} starts at {start} but previous ends at {expected_start}"
            );
        }
        let end = start
            .checked_add(length)
            .with_context(|| format!("dynamic_shape_runs run {i} start+length overflows"))?;
        if end > state_count {
            bail!("dynamic_shape_runs run {i} extends past state_count");
        }
        expected_start = end;
        out.push(DynamicShapeRun {
            start: u32::try_from(start).context("dynamic-shape start exceeds u32")?,
            length: u32::try_from(length).context("dynamic-shape length exceeds u32")?,
            dynamic,
        });
    }
    if expected_start != state_count {
        bail!("dynamic_shape_runs cover [0, {expected_start}) but state_count is {state_count}");
    }
    Ok(out)
}

fn validate_dynamic_fixtures(value: &Value, state_count: u64) -> Result<Vec<DynamicFixture>> {
    let values = value
        .as_array()
        .context("`dynamic_fixtures` must be an array")?;
    if values.is_empty() {
        bail!("`dynamic_fixtures` is empty");
    }
    let mut fixtures = Vec::with_capacity(values.len());
    let mut names = std::collections::HashSet::new();
    for (index, value) in values.iter().enumerate() {
        let object = value
            .as_object()
            .with_context(|| format!("dynamic_fixtures fixture {index} must be an object"))?;
        for field in object.keys() {
            if !matches!(
                field.as_str(),
                "name"
                    | "block"
                    | "state_id"
                    | "dynamic"
                    | "support_full"
                    | "support_center"
                    | "support_rigid"
                    | "collision_full"
                    | "occlusion_full"
            ) {
                bail!("dynamic_fixtures fixture {index} has unexpected field `{field}`");
            }
        }
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .with_context(|| format!("dynamic_fixtures fixture {index} `name` must be a string"))?;
        if !names.insert(name) {
            bail!("dynamic_fixtures fixture {index} duplicates name `{name}`");
        }
        let block = object
            .get("block")
            .and_then(Value::as_str)
            .with_context(|| {
                format!("dynamic_fixtures fixture {index} `block` must be a string")
            })?;
        let state_id = object
            .get("state_id")
            .and_then(Value::as_u64)
            .with_context(|| {
                format!("dynamic_fixtures fixture {index} `state_id` must be an integer")
            })?;
        if state_id >= state_count {
            bail!(
                "dynamic_fixtures fixture {index} state_id {state_id} is outside state_count {state_count}"
            );
        }
        let dynamic = object
            .get("dynamic")
            .and_then(Value::as_bool)
            .with_context(|| {
                format!("dynamic_fixtures fixture {index} `dynamic` must be boolean")
            })?;
        if !dynamic {
            bail!("dynamic_fixtures fixture {index} is not marked dynamic");
        }
        let mask = |field: &str| -> Result<u8> {
            let value = object.get(field).and_then(Value::as_u64).with_context(|| {
                format!("dynamic_fixtures fixture {index} `{field}` must be an integer")
            })?;
            if value > 0x3F {
                bail!(
                    "dynamic_fixtures fixture {index} `{field}` mask {value} has bits outside six directions"
                );
            }
            Ok(value as u8)
        };
        fixtures.push(DynamicFixture {
            name: name.to_string(),
            block: block.to_string(),
            state_id: state_id as u32,
            dynamic,
            support_full: mask("support_full")?,
            support_center: mask("support_center")?,
            support_rigid: mask("support_rigid")?,
            collision_full: mask("collision_full")?,
            occlusion_full: mask("occlusion_full")?,
        });
    }
    Ok(fixtures)
}

struct RenderInput<'a> {
    runs: &'a [Run],
    face_sturdy_runs: &'a [FaceSturdyRun],
    center_support_runs: &'a [FaceSturdyRun],
    rigid_support_runs: &'a [FaceSturdyRun],
    collision_face_runs: &'a [CollisionFaceRun],
    occlusion_face_runs: &'a [FaceSturdyRun],
    dynamic_shape_runs: &'a [DynamicShapeRun],
    collision_boxes: &'a [CollisionBox],
    collision_shapes: &'a [CollisionShape],
    collision_shape_ids: &'a [u16],
    dynamic_fixtures: &'a [DynamicFixture],
    source: &'a SourceProvenance,
}

/// Render `generated/block_behaviors.rs`.
fn render(input: RenderInput<'_>) -> String {
    let RenderInput {
        runs,
        face_sturdy_runs,
        center_support_runs,
        rigid_support_runs,
        collision_face_runs,
        occlusion_face_runs,
        dynamic_shape_runs,
        collision_boxes,
        collision_shapes,
        collision_shape_ids,
        dynamic_fixtures,
        source,
    } = input;
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
         // against the real Block.BLOCK_STATE_REGISTRY — never hand-typed. The\n\
         // support/collision masks below are zero-context samples at the probe\n\
         // origin; they are not authoritative for hasDynamicShape states, whose\n\
         // production answers require the live context contract owned by #646.\n\
         // Bit layout:\n\
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
         }\n\n",
    );

    out.push_str(
        "/// Run-length-encoded Paper `SupportType.FULL` face masks sampled at\n\
         /// `BlockPos.ZERO` with `EmptyBlockGetter`. Bit order is\n\
         /// `Direction.values()` (`DOWN`, `UP`, `NORTH`, `SOUTH`, `WEST`, `EAST`).\n\
         /// These samples are not authoritative for `hasDynamicShape` states;\n\
         /// live context belongs to issue #646.\n\
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
        "/// The Paper FULL-face support sample for a state id at the probe origin.\n\
         /// Ids outside `0..BLOCK_STATE_COUNT` fall back to state 0, mirroring\n\
         /// `Block.stateById`; `hasDynamicShape` states require issue #646 live\n\
         /// context instead of this static sample.\n\
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
         }\n\n",
    );

    for (name, runs, description) in [
        ("CENTER_SUPPORT", center_support_runs, "SupportType.CENTER"),
        ("RIGID_SUPPORT", rigid_support_runs, "SupportType.RIGID"),
    ] {
        out.push_str(&format!(
            "/// Run-length-encoded Paper `{description}` face masks at the probe origin.\n/// Dynamic-shape states require live context instead of these samples.\npub static BLOCK_{name}_RUNS: &[(u16, u16, u8)] = &[\n"
        ));
        for run in runs {
            out.push_str(&format!(
                "    ({}, {}, 0x{:02X}),\n",
                run.start, run.length, run.mask
            ));
        }
        out.push_str("];\n\n");
        let lower = name.to_ascii_lowercase();
        out.push_str(&format!(
            "/// The Paper `{description}` face sample for a state id.\npub fn {lower}_mask_of(id: StateId) -> u8 {{\n    let id = id.0 as u32;\n    let idx = BLOCK_{name}_RUNS.partition_point(|(start, _, _)| *start as u32 <= id);\n    if idx == 0 {{ return BLOCK_{name}_RUNS[0].2; }}\n    let (start, len, mask) = BLOCK_{name}_RUNS[idx - 1];\n    if id < start as u32 + len as u32 {{ mask }} else {{ BLOCK_{name}_RUNS[0].2 }}\n}}\n\n"
        ));
    }

    out.push_str(
        "/// Run-length-encoded full collision-face masks sampled at\n/// `BlockPos.ZERO` with `EmptyBlockGetter`, as used by\n/// `MultifaceBlock.canAttachTo`. Bit order is `Direction.values()`\n/// (`DOWN`, `UP`, `NORTH`, `SOUTH`, `WEST`, `EAST`). These samples are\n/// not authoritative for `hasDynamicShape` states; live context belongs\n/// to issue #646.\npub static BLOCK_COLLISION_FACE_RUNS: &[(u16, u16, u8)] = &[\n",
    );
    for run in collision_face_runs {
        out.push_str(&format!(
            "    ({}, {}, 0x{:02X}),\n",
            run.start, run.length, run.mask
        ));
    }
    out.push_str("];\n\n");
    out.push_str(
        "/// The Paper full collision-face sample for a state id at the probe origin.\n\
         /// Ids outside `0..BLOCK_STATE_COUNT` fall back to state 0, mirroring\n\
         /// `Block.stateById`; `hasDynamicShape` states require issue #646 live\n\
         /// context instead of this static sample.\n\
         pub fn collision_face_mask_of(id: StateId) -> u8 {\n\
             let id = id.0 as u32;\n\
             let idx = BLOCK_COLLISION_FACE_RUNS.partition_point(|(start, _, _)| *start as u32 <= id);\n\
             if idx == 0 {\n\
                 return BLOCK_COLLISION_FACE_RUNS[0].2;\n\
             }\n\
             let (start, len, mask) = BLOCK_COLLISION_FACE_RUNS[idx - 1];\n\
             if id < start as u32 + len as u32 {\n\
                 mask\n\
             } else {\n\
                 BLOCK_COLLISION_FACE_RUNS[0].2\n\
             }\n\
         }\n\n",
    );

    out.push_str("/// Run-length-encoded full occlusion-face masks at the probe origin.\npub static BLOCK_OCCLUSION_FACE_RUNS: &[(u16, u16, u8)] = &[\n");
    for run in occlusion_face_runs {
        out.push_str(&format!(
            "    ({}, {}, 0x{:02X}),\n",
            run.start, run.length, run.mask
        ));
    }
    out.push_str("];\n\n");
    out.push_str(
        "/// The Paper full occlusion-face sample for a state id.\n\
         pub fn occlusion_face_mask_of(id: StateId) -> u8 {\n\
             let id = id.0 as u32;\n\
             let idx = BLOCK_OCCLUSION_FACE_RUNS.partition_point(|(start, _, _)| *start as u32 <= id);\n\
             if idx == 0 { return BLOCK_OCCLUSION_FACE_RUNS[0].2; }\n\
             let (start, len, mask) = BLOCK_OCCLUSION_FACE_RUNS[idx - 1];\n\
             if id < start as u32 + len as u32 { mask } else { BLOCK_OCCLUSION_FACE_RUNS[0].2 }\n\
         }\n\n",
    );

    out.push_str(
        "/// Primitive coordinates for exact static `getCollisionShape` boxes.\n\
         /// Values are signed 1/32 block units; this table has no world-geometry dependency.\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct StaticCollisionBox {\n\
             pub min_x: i8,\n\
             pub min_y: i8,\n\
             pub min_z: i8,\n\
             pub max_x: i8,\n\
             pub max_y: i8,\n\
             pub max_z: i8,\n\
         }\n\
\
         #[rustfmt::skip]\n\
         pub static STATIC_COLLISION_BOXES: &[StaticCollisionBox] = &[\n",
    );
    for shape in collision_boxes {
        out.push_str(&format!(
            "    StaticCollisionBox {{ min_x: {}, min_y: {}, min_z: {}, max_x: {}, max_y: {}, max_z: {} }},\n",
            shape.min_x, shape.min_y, shape.min_z, shape.max_x, shape.max_y, shape.max_z
        ));
    }
    out.push_str("];\n\n");
    out.push_str(
        "/// A geometry is a contiguous range in `STATIC_COLLISION_BOXES`.\n\
         #[rustfmt::skip]\n\
         pub static STATIC_COLLISION_SHAPES: &[(u16, u8)] = &[\n",
    );
    for shape in collision_shapes {
        out.push_str(&format!("    ({}, {}),\n", shape.start, shape.length));
    }
    out.push_str("];\n\n");
    out.push_str(
        "/// Per-StateId static collision geometry. `u16::MAX` marks a dynamic\n\
         /// state; callers must reject `has_dynamic_shape` before using this table.\n\
         #[rustfmt::skip]\n\
         pub static STATIC_COLLISION_SHAPE_IDS: &[u16] = &[\n",
    );
    for ids in collision_shape_ids.chunks(16) {
        out.push_str("    ");
        for (index, id) in ids.iter().enumerate() {
            if index != 0 {
                out.push_str(", ");
            }
            if *id == u16::MAX {
                out.push_str("u16::MAX");
            } else {
                out.push_str(&id.to_string());
            }
        }
        out.push_str(",\n");
    }
    out.push_str("];\n\n");
    out.push_str(
        "/// Resolve exact static collision boxes for a non-dynamic state.\n\
         pub fn static_collision_shape_of(id: StateId) -> Option<&'static [StaticCollisionBox]> {\n\
             if has_dynamic_shape(id) {\n\
                 return None;\n\
             }\n\
             let shape_id = *STATIC_COLLISION_SHAPE_IDS.get(id.0 as usize)? as usize;\n\
             let (start, length) = *STATIC_COLLISION_SHAPES.get(shape_id)?;\n\
             Some(&STATIC_COLLISION_BOXES[start as usize..start as usize + length as usize])\n\
         }\n\
\
",
    );

    out.push_str("/// Pinned dynamic-shape fixtures emitted by the Paper probe.\n#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub struct DynamicShapeFixture {\n    pub name: &'static str,\n    pub block: &'static str,\n    pub state_id: StateId,\n    pub dynamic: bool,\n    pub support_full: u8,\n    pub support_center: u8,\n    pub support_rigid: u8,\n    pub collision_full: u8,\n    pub occlusion_full: u8,\n}\n\npub static DYNAMIC_SHAPE_FIXTURES: &[DynamicShapeFixture] = &[\n");
    for fixture in dynamic_fixtures {
        out.push_str(&format!(
            "    DynamicShapeFixture {{ name: {:?}, block: {:?}, state_id: StateId({}), dynamic: {}, support_full: 0x{:02X}, support_center: 0x{:02X}, support_rigid: 0x{:02X}, collision_full: 0x{:02X}, occlusion_full: 0x{:02X} }},\n",
            fixture.name,
            fixture.block,
            fixture.state_id,
            fixture.dynamic,
            fixture.support_full,
            fixture.support_center,
            fixture.support_rigid,
            fixture.collision_full,
            fixture.occlusion_full,
        ));
    }
    out.push_str("];\n\n");
    out.push_str(
        "/// Resolve a named pinned dynamic-shape fixture.\n\
         pub fn dynamic_shape_fixture(name: &str) -> Option<&'static DynamicShapeFixture> {\n\
             DYNAMIC_SHAPE_FIXTURES.iter().find(|fixture| fixture.name == name)\n\
         }\n\n\
         /// Run-length-encoded `Block.hasDynamicShape()` metadata.\n\
         /// Dynamic states must not answer context-sensitive shape predicates\n\
         /// from the zero-context support/collision samples.\n\
         pub static DYNAMIC_SHAPE_RUNS: &[(u16, u16, bool)] = &[\n",
    );
    for run in dynamic_shape_runs {
        out.push_str(&format!(
            "    ({}, {}, {}),\n",
            run.start, run.length, run.dynamic
        ));
    }
    out.push_str("];\n\n");
    out.push_str(
        "/// Whether a state requires live world context for shape queries.\n\
         pub fn has_dynamic_shape(state: StateId) -> bool {\n\
             let id = state.0 as u32;\n\
             let idx = DYNAMIC_SHAPE_RUNS.partition_point(|(start, _, _)| *start as u32 <= id);\n\
             if idx == 0 {\n\
                 return DYNAMIC_SHAPE_RUNS[0].2;\n\
             }\n\
             let (start, len, dynamic) = DYNAMIC_SHAPE_RUNS[idx - 1];\n\
             if id < start as u32 + len as u32 { dynamic } else { DYNAMIC_SHAPE_RUNS[0].2 }\n\
         }\n",
    );

    out.trim_end_matches('\n').to_owned() + "\n"
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
            "center_support_runs": [
                {"start": 0, "length": 2, "mask": 0},
                {"start": 2, "length": 3, "mask": 63},
            ],
            "rigid_support_runs": [
                {"start": 0, "length": 2, "mask": 0},
                {"start": 2, "length": 3, "mask": 63},
            ],
            "collision_face_runs": [
                {"start": 0, "length": 2, "mask": 0},
                {"start": 2, "length": 3, "mask": 63},
            ],
            "occlusion_face_runs": [
                {"start": 0, "length": 2, "mask": 0},
                {"start": 2, "length": 3, "mask": 63},
            ],
            "dynamic_shape_runs": [
                {"start": 0, "length": 2, "dynamic": false},
                {"start": 2, "length": 3, "dynamic": true},
            ],
            "collision_shapes": [
                {"boxes": []},
                {"boxes": [{"min_x": 0, "min_y": 0, "min_z": 0,
                            "max_x": 32, "max_y": 32, "max_z": 32}]}
            ],
            "collision_shape_ids": [0, 0, -1, -1, -1],
            "dynamic_fixtures": [
                {"name": "test_dynamic", "block": "minecraft:test", "state_id": 2, "dynamic": true,
                 "support_full": 63, "support_center": 63, "support_rigid": 63,
                 "collision_full": 63, "occlusion_full": 63}
            ],
        })
    }

    #[test]
    fn dense_partition_passes() {
        let (
            runs,
            face_sturdy_runs,
            center_support_runs,
            rigid_support_runs,
            collision_face_runs,
            occlusion_face_runs,
            dynamic_shape_runs,
            _collision_boxes,
            _collision_shapes,
            _collision_shape_ids,
            dynamic_fixtures,
        ) = validate(valid_root()).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[1].word, 0x20015);
        assert_eq!(face_sturdy_runs[1].mask, 63);
        assert_eq!(center_support_runs[1].mask, 63);
        assert_eq!(rigid_support_runs[1].mask, 63);
        assert_eq!(collision_face_runs[1].mask, 63);
        assert_eq!(occlusion_face_runs[1].mask, 63);
        assert!(dynamic_shape_runs[1].dynamic);
        assert_eq!(dynamic_fixtures[0].support_full, 63);
    }

    #[test]
    fn dynamic_state_must_not_have_static_geometry() {
        let mut v = valid_root();
        v["collision_shape_ids"][2] = serde_json::json!(0);
        let err = validate(v).unwrap_err();
        assert!(
            err.to_string().contains("dynamic but has shape id"),
            "got: {err}"
        );
    }

    #[test]
    fn collision_shape_zero_must_be_empty() {
        let mut v = valid_root();
        v["collision_shapes"][0]["boxes"] = serde_json::json!([{
            "min_x": 0,
            "min_y": 0,
            "min_z": 0,
            "max_x": 1,
            "max_y": 1,
            "max_z": 1
        }]);
        let err = validate(v).unwrap_err();
        assert!(
            err.to_string().contains("shape 0 must be the empty shape"),
            "got: {err}"
        );
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
    fn collision_face_mask_out_of_range_fails() {
        let mut v = valid_root();
        v["collision_face_runs"][0]["mask"] = serde_json::json!(64);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("six directions"), "got: {err}");
    }

    #[test]
    fn collision_face_runs_must_cover_all_states() {
        let mut v = valid_root();
        v["collision_face_runs"][1]["length"] = serde_json::json!(2);
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
    fn duplicate_dynamic_fixture_name_fails() {
        let mut v = valid_root();
        let fixture = v["dynamic_fixtures"][0].clone();
        v["dynamic_fixtures"] = serde_json::json!([fixture.clone(), fixture]);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("duplicates name"), "got: {err}");
    }

    #[test]
    fn non_dynamic_fixture_fails() {
        let mut v = valid_root();
        v["dynamic_fixtures"][0]["dynamic"] = serde_json::json!(false);
        let err = validate(v).unwrap_err();
        assert!(err.to_string().contains("not marked dynamic"), "got: {err}");
    }

    #[test]
    fn dynamic_fixture_state_id_out_of_range_fails() {
        let mut v = valid_root();
        v["dynamic_fixtures"][0]["state_id"] = serde_json::json!(5);
        let err = validate(v).unwrap_err();
        assert!(
            err.to_string().contains("outside state_count"),
            "got: {err}"
        );
    }

    #[test]
    fn dynamic_fixture_mask_out_of_range_fails() {
        let mut v = valid_root();
        v["dynamic_fixtures"][0]["support_full"] = serde_json::json!(64);
        let err = validate(v).unwrap_err();
        assert!(
            err.to_string().contains("outside six directions"),
            "got: {err}"
        );
    }

    #[test]
    fn dynamic_fixture_unknown_field_fails() {
        let mut v = valid_root();
        v["dynamic_fixtures"][0]["extra"] = serde_json::json!(1);
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
            "center_support_runs": [
                {"start": 0, "length": 5, "mask": 0},
            ],
            "rigid_support_runs": [
                {"start": 0, "length": 5, "mask": 0},
            ],
            "collision_face_runs": [
                {"start": 0, "length": 5, "mask": 0},
            ],
            "occlusion_face_runs": [
                {"start": 0, "length": 5, "mask": 0},
            ],
            "dynamic_shape_runs": [
                {"start": 0, "length": 5, "dynamic": false},
            ],
            "collision_shapes": [
                {"boxes": []}
            ],
            "collision_shape_ids": [0, 0, 0, 0, 0],
            "dynamic_fixtures": [
                {"name": "test_dynamic", "block": "minecraft:test", "state_id": 0, "dynamic": true,
                 "support_full": 0, "support_center": 0, "support_rigid": 0,
                 "collision_full": 0, "occlusion_full": 0}
            ],
        });
        let (runs, _, _, _, _, _, _, _, _, _, _) = validate(v).unwrap();
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
    fn mutated_fixture_bytes_fail_provenance() {
        let repo_root = crate::extract::find_repo_root().unwrap();
        let source_fixture = default_input(&repo_root);
        let source_manifest = source_fixture.with_file_name("block_behaviors.manifest.json");
        let temp = tempfile::tempdir().unwrap();
        let fixture = temp.path().join("block_behaviors.json");
        let manifest = temp.path().join("block_behaviors.manifest.json");
        fs::copy(source_fixture, &fixture).unwrap();
        fs::copy(source_manifest, &manifest).unwrap();
        let mut bytes = fs::read(&fixture).unwrap();
        bytes[0] ^= 1;
        fs::write(&fixture, bytes).unwrap();
        let err = load_provenance(&fixture).unwrap_err();
        assert!(
            err.to_string().contains("does not match its provenance"),
            "got: {err}"
        );
    }

    #[test]
    fn mutated_fixture_manifest_digest_fails_provenance() {
        let repo_root = crate::extract::find_repo_root().unwrap();
        let source_fixture = default_input(&repo_root);
        let source_manifest = source_fixture.with_file_name("block_behaviors.manifest.json");
        let temp = tempfile::tempdir().unwrap();
        let fixture = temp.path().join("block_behaviors.json");
        let manifest = temp.path().join("block_behaviors.manifest.json");
        fs::copy(source_fixture, &fixture).unwrap();
        fs::copy(source_manifest, &manifest).unwrap();
        let mut value: Value =
            serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
        value["file"]["sha256"] = serde_json::json!("00");
        fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        let err = load_provenance(&fixture).unwrap_err();
        assert!(
            err.to_string().contains("does not match its provenance"),
            "got: {err}"
        );
    }

    #[test]
    fn mutated_fixture_source_provenance_fails_pinned_source() {
        let repo_root = crate::extract::find_repo_root().unwrap();
        let source_fixture = default_input(&repo_root);
        let source_manifest = source_fixture.with_file_name("block_behaviors.manifest.json");
        let temp = tempfile::tempdir().unwrap();
        let fixture = temp.path().join("block_behaviors.json");
        let manifest = temp.path().join("block_behaviors.manifest.json");
        fs::copy(source_fixture, &fixture).unwrap();
        fs::copy(source_manifest, &manifest).unwrap();
        let mut value: Value =
            serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
        value["source"]["paper_git"] = serde_json::json!("deadbeef");
        fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        let err = load_provenance(&fixture).unwrap_err();
        assert!(err.to_string().contains("Paper commit"), "got: {err}");
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
        let center_support_runs = face_sturdy_runs.clone();
        let rigid_support_runs = face_sturdy_runs.clone();
        let collision_face_runs = vec![CollisionFaceRun {
            start: 0,
            length: 3,
            mask: 0x3F,
        }];
        let occlusion_face_runs = face_sturdy_runs.clone();
        let dynamic_shape_runs = vec![DynamicShapeRun {
            start: 0,
            length: 3,
            dynamic: false,
        }];
        let collision_boxes = vec![CollisionBox {
            min_x: 0,
            min_y: 0,
            min_z: 0,
            max_x: 32,
            max_y: 32,
            max_z: 32,
        }];
        let collision_shapes = vec![CollisionShape {
            start: 0,
            length: 1,
        }];
        let collision_shape_ids = vec![0, 0, 0];
        let dynamic_fixtures = vec![DynamicFixture {
            name: "test".to_string(),
            block: "minecraft:test".to_string(),
            state_id: 0,
            dynamic: true,
            support_full: 0x3F,
            support_center: 0x3F,
            support_rigid: 0x3F,
            collision_full: 0x3F,
            occlusion_full: 0x3F,
        }];
        let input = || RenderInput {
            runs: &runs,
            face_sturdy_runs: &face_sturdy_runs,
            center_support_runs: &center_support_runs,
            rigid_support_runs: &rigid_support_runs,
            collision_face_runs: &collision_face_runs,
            occlusion_face_runs: &occlusion_face_runs,
            dynamic_shape_runs: &dynamic_shape_runs,
            collision_boxes: &collision_boxes,
            collision_shapes: &collision_shapes,
            collision_shape_ids: &collision_shape_ids,
            dynamic_fixtures: &dynamic_fixtures,
            source: &source,
        };
        let a = render(input());
        let b = render(input());
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
        let (runs, _, _, _, _, _, _, _, _, _, _) = validate(root).unwrap();
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

    /// The emitted `face_sturdy_mask_of` binary search must reproduce Paper's
    /// mask for every state in the real table, including both boundaries of
    /// every run and the out-of-range fallback.
    #[test]
    fn face_sturdy_mask_of_matches_fixture_masks() {
        let repo_root = crate::extract::find_repo_root().unwrap();
        let json = fs::read_to_string(default_input(&repo_root)).unwrap();
        let root = crate::registries::parse_strict(&json).unwrap();
        let (_, runs, _, _, _, _, _, _, _, _, _) = validate(root).unwrap();
        let state_count: u64 = runs.iter().map(|r| r.length as u64).sum();
        assert_eq!(state_count, 32366, "registry state count drifted");

        let mut masks = vec![0u8; state_count as usize];
        for r in &runs {
            for mask in masks[r.start as usize..(r.start + r.length) as usize].iter_mut() {
                *mask = r.mask;
            }
        }
        let decode = |id: u32| -> u8 {
            let idx = runs.partition_point(|r| r.start <= id);
            if idx == 0 {
                return runs[0].mask;
            }
            let (start, len, mask) = (
                runs[idx - 1].start,
                runs[idx - 1].length,
                runs[idx - 1].mask,
            );
            if id < start + len { mask } else { runs[0].mask }
        };

        for id in 0..state_count {
            assert_eq!(decode(id as u32), masks[id as usize], "state {id}");
        }
        assert_eq!(decode(state_count as u32), masks[0], "first out-of-range");
        assert_eq!(decode(u16::MAX as u32), masks[0], "far out-of-range");
    }

    /// The collision-face decoder uses the same RLE/binary-search contract as
    /// the support-mask decoder and must reproduce every fixture mask.
    #[test]
    fn collision_face_mask_of_matches_fixture_masks() {
        let repo_root = crate::extract::find_repo_root().unwrap();
        let json = fs::read_to_string(default_input(&repo_root)).unwrap();
        let root = crate::registries::parse_strict(&json).unwrap();
        let (_, _, runs, _, _, _, _, _, _, _, _) = validate(root).unwrap();
        let state_count: u64 = runs.iter().map(|r| r.length as u64).sum();
        assert_eq!(state_count, 32366, "registry state count drifted");

        let mut masks = vec![0u8; state_count as usize];
        for r in &runs {
            for mask in masks[r.start as usize..(r.start + r.length) as usize].iter_mut() {
                *mask = r.mask;
            }
        }
        let decode = |id: u32| -> u8 {
            let idx = runs.partition_point(|r| r.start <= id);
            if idx == 0 {
                return runs[0].mask;
            }
            let (start, len, mask) = (
                runs[idx - 1].start,
                runs[idx - 1].length,
                runs[idx - 1].mask,
            );
            if id < start + len { mask } else { runs[0].mask }
        };

        for id in 0..state_count {
            assert_eq!(decode(id as u32), masks[id as usize], "state {id}");
        }
        assert_eq!(decode(state_count as u32), masks[0], "first out-of-range");
        assert_eq!(decode(u16::MAX as u32), masks[0], "far out-of-range");
    }

    /// The real fixture must agree with `block_states.json` on the total state
    /// count (`BLOCK_STATE_COUNT`), so a behavior dump from a differently-sized
    /// registry fails `generate` even when it is internally self-consistent.
    #[test]
    fn static_collision_fixture_has_pinned_non_vacuous_coverage() {
        let repo_root = crate::extract::find_repo_root().unwrap();
        let json = fs::read_to_string(default_input(&repo_root)).unwrap();
        let root = crate::registries::parse_strict(&json).unwrap();
        let (_, _, _, _, _, _, dynamic_runs, boxes, shapes, shape_ids, _) = validate(root).unwrap();
        assert_eq!(shapes.len(), 318, "Paper static geometry dedup drifted");
        assert!(
            shapes[0].length == 0,
            "geometry zero must be the empty shape"
        );
        assert_eq!(
            shapes.iter().filter(|shape| shape.length != 0).count(),
            317,
            "static geometry table must retain all non-empty signatures"
        );
        assert_eq!(shape_ids.len(), 32366);
        assert_eq!(
            shape_ids.iter().filter(|id| **id == u16::MAX).count(),
            199,
            "all dynamic states must be excluded from static lookup"
        );
        assert_eq!(
            shape_ids.iter().filter(|id| **id != u16::MAX).count(),
            32167
        );
        assert_eq!(
            dynamic_runs
                .iter()
                .filter(|run| run.dynamic)
                .map(|run| run.length)
                .sum::<u32>(),
            199
        );
        assert!(
            boxes.len() > 317,
            "non-empty geometries must carry their boxes"
        );
        assert!(shapes.iter().all(|shape| shape.length <= 15));
    }

    #[test]
    fn real_fixture_state_count_matches_registry() {
        let repo_root = crate::extract::find_repo_root().unwrap();
        assert_eq!(registry_state_count(&repo_root).unwrap(), 32366);
        let json = fs::read_to_string(default_input(&repo_root)).unwrap();
        let root = crate::registries::parse_strict(&json).unwrap();
        let (runs, _, _, _, _, _, _, _, _, _, _) = validate(root).unwrap();
        check_state_count_matches(&runs, 32366).unwrap();
    }
}
