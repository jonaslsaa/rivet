//! Port of `ca.spottedleaf.moonrise.patches.starlight.util.SaveUtil` (MC 26.2)
//! — the Starlight save-format *value layer* (issue #229 spike / #184 Phase B).
//!
//! Java: `SaveUtil.java` in `working/Paper`. `SerializableChunkData.write()`
//! stores, per light section, the `SWMRNibbleArray.SaveState` returned by
//! `getSaveState()`: a `starlight.blocklight_state`/`starlight.skylight_state`
//! int plus, when the save state carries bytes, the `BlockLight`/`SkyLight`
//! 2048-byte nibble array under the vanilla keys. The chunk root carries
//! `starlight.light_version = 10` (`STARLIGHT_LIGHT_VERSION`) when the chunk is
//! lit (`isLightOn` + `status >= LIGHT`).
//!
//! This module ports the *readable/writable surface* only — the exact mapping
//! between in-memory `SwmrNibbleArray`s and the on-disk per-section
//! `(state, bytes)` pairs. The `loadLightHookReal` direction (sections ->
//! `SWMRNibbleArray`) already lives in `chunk::storage::serializable_chunk_data`
//! (`decode_section_light` + `reconstruct_lights`); the `saveLightHookReal`
//! direction (arrays -> sections) is the seam this spike must prove byte-for-byte
//! before the compute phases of #184 land. The full `SerializableChunkData`
//! section rewrite (stripping vanilla light, section-list rebuild) is #231's
//! write-path concern and is not duplicated here.
//!
//! The spike verdict (issue #229): given the height accessor and the block/sky
//! nibble arrays, `light_save_surface` must reproduce, byte-for-byte, the light
//! surface Paper wrote into the committed M0 FULL fixtures — and the
//! `superflat` filler must be compared against the same fixture to falsify
//! anything that does not match.

use crate::level::height_accessor::LevelHeightAccessor;
use crate::lighting::swmr_nibble_array::SwmrNibbleArray;

/// The light fields of one section as Paper writes them, mirroring the
/// `SectionLightData` shape `decode_section_light` reads back (issue #229).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedLightSection {
    /// The section's Y coordinate.
    pub y: i32,
    /// `starlight.blocklight_state` (absent state is `-1`, matching
    /// `state_or_absent`; Paper omits the key when `state == -1` on read and
    /// only writes it when `state > 0` in `write()`).
    pub block_state: i32,
    /// `BlockLight` bytes, when the save state carries data.
    pub block_light: Option<Vec<u8>>,
    /// `starlight.skylight_state`.
    pub sky_state: i32,
    /// `SkyLight` bytes, when the save state carries data.
    pub sky_light: Option<Vec<u8>>,
}

/// `SaveUtil.saveLightHookReal`'s per-section save-state mapping — the exact
/// surface Paper writes for every light section of a lit chunk.
///
/// This is the *value mapping only*: it emits sections from whatever nibble
/// arrays it is given. Paper's caller (`saveLightHook`) only invokes the seam
/// for a chunk that is light-correct (`isLightOn` + `status >= LIGHT`); the
/// #184 integration must preserve that gate rather than calling this
/// unconditionally.
///
/// Java walks light sections `minLightSection..=maxLightSection` (the range
/// `minSectionY - 1 ..= maxSectionY + 1`), reads
/// `blockNibbles[i - minSection].getSaveState()` and
/// `skyNibbles[i - minSection].getSaveState()`, and emits a section whenever
/// either is non-null. Per save state:
///
/// - with bytes: writes the vanilla `BlockLight`/`SkyLight` array plus the
///   `starlight.*_state` int;
/// - without bytes: writes only the state int (`Uninitialised` compresses to
///   state 1 with no array; an all-zero `Initialised` section also
///   zero-compresses to state 1 via `get_save_state`);
/// - `Null`/`Hidden`-all-zero: absent from the save.
///
/// `block_nibbles`/`sky_nibbles` are indexed `0..lightSectionCount` (the
/// caller supplies the array of the right length, matching Java's
/// `StarLightEngine.getFilledEmptyLight(world)` sizing). A shorter array
/// panics exactly where Java's `blockNibbles[i - minSection].getSaveState()`
/// throws `ArrayIndexOutOfBoundsException` — Paper catches that in
/// `saveLightHook` and leaves the chunk unlit so it is relit on load, and the
/// caller of this seam must do the same rather than silently truncate the save.
pub fn light_save_surface<H: LevelHeightAccessor>(
    height: &H,
    block_nibbles: &[SwmrNibbleArray],
    sky_nibbles: &[SwmrNibbleArray],
) -> Vec<SavedLightSection> {
    let min_section = height.get_min_section_y() - 1;
    let max_section = height.get_max_section_y() + 1;

    let mut out = Vec::new();
    for section_y in min_section..=max_section {
        let index = (section_y - min_section) as usize;
        let block_save = block_nibbles[index].get_save_state();
        let sky_save = sky_nibbles[index].get_save_state();
        let (block_state, block_light) = match &block_save {
            Some(save) => (save.state.to_i32(), save.data.clone()),
            None => (-1, None),
        };
        let (sky_state, sky_light) = match &sky_save {
            Some(save) => (save.state.to_i32(), save.data.clone()),
            None => (-1, None),
        };
        if block_state == -1 && sky_state == -1 {
            // Neither layer exists here — Java emits no section.
            continue;
        }
        out.push(SavedLightSection {
            y: section_y,
            block_state,
            block_light,
            sky_state,
            sky_light,
        });
    }
    out
}

/// Compare two light surfaces byte-for-byte (the `Vec` is already the
/// normalized `(y, state, bytes)` shape, so equality on the `PartialEq` impl is
/// the exact byte identity). Returns the list of divergent sections with a
/// human-readable reason, empty when the surfaces are byte-identical.
///
/// Sections are paired by their Y coordinate, so a missing or shifted section
/// reports its own absence/extra rather than shifting every subsequent pair
/// onto the wrong section. The comparison is independent of NBT key order (a
/// #231 serialization concern): it compares the *decoded light surface* — the
/// very pairs `decode_section_light` reads and `light_save_surface` writes.
pub fn surface_divergences(
    expected: &[SavedLightSection],
    actual: &[SavedLightSection],
) -> Vec<String> {
    let mut out = Vec::new();
    // Surfaces are per-section, so a duplicate Y is malformed input: fail loudly
    // rather than silently dropping a section's divergence (a false PASS).
    let mut expected_by_y = collect_by_y(expected, "expected");
    let mut actual_by_y = collect_by_y(actual, "actual");

    while let Some((y, exp)) = expected_by_y.pop_first() {
        match actual_by_y.remove(&y) {
            Some(act) if exp != act => out.push(section_divergence(y, exp, act)),
            Some(_) => {}
            None => out.push(format!(
                "section y={y}: expected present, got absent ({})",
                concise(exp)
            )),
        }
    }
    for (y, act) in actual_by_y {
        out.push(format!(
            "section y={y}: expected absent, got present ({})",
            concise(act)
        ));
    }
    out
}

/// A one-line, byte-exact description of one section's divergence, pointing at
/// the first differing light byte rather than dumping two full 2048-byte arrays
/// (which is what `Debug` on the `Vec<u8>` produces and is unreadable/unsearchable
/// in a failing test). State differences and array-length differences are named
/// directly; a same-length byte difference names the first byte where the
/// nibble-packed layers diverge and both values at that byte.
fn section_divergence(y: i32, exp: &SavedLightSection, act: &SavedLightSection) -> String {
    let mut parts = Vec::new();
    if exp.block_state != act.block_state {
        parts.push(format!(
            "block_state {} != {}",
            exp.block_state, act.block_state
        ));
    }
    if exp.sky_state != act.sky_state {
        parts.push(format!("sky_state {} != {}", exp.sky_state, act.sky_state));
    }
    if exp.block_light != act.block_light {
        parts.push(byte_divergence(
            "block_light",
            exp.block_light.as_deref(),
            act.block_light.as_deref(),
        ));
    }
    if exp.sky_light != act.sky_light {
        parts.push(byte_divergence(
            "sky_light",
            exp.sky_light.as_deref(),
            act.sky_light.as_deref(),
        ));
    }
    format!("section y={y}: {}", parts.join("; "))
}

/// First differing byte (index + values) of two same-length light arrays, or a
/// length mismatch when they differ in size.
fn byte_divergence(name: &str, exp: Option<&[u8]>, act: Option<&[u8]>) -> String {
    match (exp, act) {
        (Some(exp), Some(act)) if exp.len() != act.len() => format!(
            "{name}: length differs ({} != {} bytes)",
            exp.len(),
            act.len()
        ),
        (Some(exp), Some(act)) => {
            let i = exp
                .iter()
                .zip(act.iter())
                .position(|(e, a)| e != a)
                .expect("same bytes but Option<Vec> differed");
            format!(
                "{name}: first difference at byte {i} (nibble {i}, 0x{:02x} != 0x{:02x})",
                exp[i], act[i]
            )
        }
        (Some(_), None) => format!("{name}: expected some, got absent"),
        (None, Some(_)) => format!("{name}: expected absent, got some"),
        (None, None) => unreachable!("length-differing check ran on equal-length Some"),
    }
}

/// Compact one-line section render for absence/extra messages — states and byte
/// lengths only, never the full arrays.
fn concise(s: &SavedLightSection) -> String {
    let bl = s
        .block_light
        .as_deref()
        .map(|d| format!("{} bytes", d.len()))
        .unwrap_or_else(|| "no bytes".into());
    let sl = s
        .sky_light
        .as_deref()
        .map(|d| format!("{} bytes", d.len()))
        .unwrap_or_else(|| "no bytes".into());
    format!(
        "block_state {}, block_light {bl}, sky_state {}, sky_light {sl}",
        s.block_state, s.sky_state
    )
}

/// Index a surface by section Y, panicking on a duplicate Y instead of silently
/// dropping a section's divergence (which would turn a real mismatch into a
/// false PASS).
fn collect_by_y<'a>(
    sections: &'a [SavedLightSection],
    which: &str,
) -> std::collections::BTreeMap<i32, &'a SavedLightSection> {
    let mut by_y: std::collections::BTreeMap<i32, &'a SavedLightSection> =
        std::collections::BTreeMap::new();
    for s in sections {
        if by_y.insert(s.y, s).is_some() {
            panic!(
                "surface_divergences: duplicate section y={} in the {which} surface \
                 (each section Y must appear at most once; a duplicate would silently \
                 drop a divergence)",
                s.y
            );
        }
    }
    by_y
}

// The spike tests live in `spike_229` (issue #229); `light_save_surface` is the
// production seam they exercise.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::storage::serializable_chunk_data::{
        parse_light_correct, parse_section_lights, reconstruct_lights,
    };
    use crate::level::height_accessor::create;
    use crate::lighting::swmr_nibble_array::{ARRAY_SIZE, InitState};
    use base64::Engine as _;
    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_nbt::nbt_accounter::NbtAccounter;
    use rivet_nbt::nbt_io;
    use rivet_util::DataInputStream;
    use serde_json::Value;
    use std::io::Cursor;
    use std::path::PathBuf;

    /// The light surface for a superflat overworld chunk per the committed M0
    /// fixture (`fixtures/chunk/overworld/0.0/0.0.nbt`, section -4 terrain
    /// bedrock/dirt/dirt/grass with top opaque at -61).
    fn overworld_height() -> crate::level::height_accessor::SimpleLevelHeightAccessor {
        create(-64, 384)
    }

    /// `StarLightEngine.getFilledEmptyLight` — `new SWMRNibbleArray(null, true)`
    /// for every light section (state `Null`, absent from the save).
    fn empty_nibbles(count: usize) -> Vec<SwmrNibbleArray> {
        (0..count)
            .map(|_| SwmrNibbleArray::new_with_bytes_and_null(None, true))
            .collect()
    }

    #[test]
    fn light_save_surface_round_trips_save_state() {
        let height = overworld_height();
        // A full non-zero array, an empty (all-zero) array, and an uninitialised
        // section: the three save-state shapes. Empty slots are `Null` (Java's
        // `getFilledEmptyLight`), so they are absent from the save.
        let mut blocks = empty_nibbles(26);
        let mut skies = empty_nibbles(26);
        blocks[0] = SwmrNibbleArray::new(); // Uninitialised
        skies[0] = SwmrNibbleArray::new_with_bytes(vec![0; ARRAY_SIZE]); // all-zero INIT compresses
        skies[1] = SwmrNibbleArray::new_with_bytes(vec![0xAB; ARRAY_SIZE]);

        let surface = light_save_surface(&height, &blocks, &skies);
        assert_eq!(
            surface,
            vec![
                SavedLightSection {
                    y: -5,
                    block_state: InitState::Uninitialised.to_i32(),
                    block_light: None,
                    sky_state: InitState::Uninitialised.to_i32(), // all-zero INIT compresses
                    sky_light: None,
                },
                SavedLightSection {
                    y: -4,
                    block_state: -1, // Null block nibble is absent
                    block_light: None,
                    sky_state: InitState::Initialised.to_i32(),
                    sky_light: Some(vec![0xAB; ARRAY_SIZE]),
                },
            ]
        );
    }

    #[test]
    fn null_and_hidden_sections_are_absent() {
        let height = overworld_height();
        let blocks = empty_nibbles(26);
        let mut skies = empty_nibbles(26);
        let mut hidden_zero = SwmrNibbleArray::new_with_bytes(vec![0; ARRAY_SIZE]);
        hidden_zero.set_hidden();
        hidden_zero.update_visible();
        skies[0] = hidden_zero;
        let surface = light_save_surface(&height, &blocks, &skies);
        assert!(surface.is_empty());
    }

    /// Read a committed Paper 26.2 FULL-status chunk fixture for a dimension.
    /// A missing fixture fails the test loudly (this panic); the oracle verify
    /// gate independently fails with exit 1 because the chunk payload is a
    /// manifest-captured fixture file. No layer silently skips.
    fn load_fixture(dim: &str) -> CompoundTag {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/chunk")
            .join(dim)
            .join("0.0")
            .join("0.0.nbt");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "UNVERIFIED: {dim} Starlight fixture {} missing ({e}) — the #229 byte-identity \
                 spike cannot run. Regenerate the fixtures (`cargo run -p rivet-oracle -- \
                 regenerate --m0` for the M0 superflat slice) before relying on this proof. \
                 This is a test-binary failure, not an oracle exit-3 UNVERIFIED: the oracle \
                 gate fails with a hard manifest error whenever a captured fixture file is \
                 absent.",
                path.display()
            )
        });
        let mut input = DataInputStream::new(Cursor::new(bytes));
        nbt_io::read(&mut input, &mut NbtAccounter::unlimited_heap())
            .expect("Paper 26.2 chunk fixture parses")
    }

    /// The fixture's own light surface (`SectionLightData`), filtered to the
    /// sections `saveLightHookReal` actually writes (a light state present), in
    /// the normalized `SavedLightSection` shape.
    fn fixture_surface(chunk: &CompoundTag) -> Vec<SavedLightSection> {
        parse_section_lights(chunk)
            .into_iter()
            .filter(|s| s.block_state >= 0 || s.sky_state >= 0)
            .map(|s| SavedLightSection {
                y: s.y,
                block_state: s.block_state,
                block_light: s.block_light,
                sky_state: s.sky_state,
                sky_light: s.sky_light,
            })
            .collect()
    }

    /// Load the committed full-array light fixture (`light-full.json`) — the
    /// independent Paper ground truth the spike compares against. It is captured
    /// by `extract_light_full.py` from the same M0 FULL `.nbt` chunks but through
    /// a *separate* decode path (Python NBT reader + base64), so a decode bug
    /// shared by `parse_section_lights` cannot hide a divergence. When the fixture
    /// is absent or malformed the spike fails loudly — never silently skips.
    fn load_light_full_fixture() -> Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/worldgen/light-full.json");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "UNVERIFIED: full-array light fixture {} missing ({e}) — the #229 \
                 byte-identity spike cannot run its independent ground-truth check. \
                 Regenerate the fixtures (`cargo run -p rivet-oracle -- sample`), which \
                 re-extracts light-full.json from the M0 FULL chunks. The oracle verify \
                 gate also fails loudly when this captured fixture file is absent.",
                path.display()
            )
        });
        serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            panic!(
                "UNVERIFIED: full-array light fixture {} malformed ({e}) — refusing to \
                 compare against a corrupt ground truth",
                path.display()
            )
        })
    }

    /// The light surface for a dimension from the committed `light-full.json`
    /// (`extract_light_full.py` output), filtered to the sections
    /// `saveLightHookReal` writes (a light state present) and normalized to the
    /// `SavedLightSection` shape. Base64 arrays are decoded to the raw 2048 bytes;
    /// absent states map to `-1` (`state_or_absent`).
    fn json_surface(light_full: &Value, dim: &str) -> Vec<SavedLightSection> {
        let chunks = light_full
            .get("chunks")
            .and_then(|c| c.as_array())
            .unwrap_or_else(|| panic!("light-full.json: missing `chunks` array"));
        let chunk = chunks
            .iter()
            .find(|c| c.get("dim").and_then(|d| d.as_str()) == Some(dim))
            .unwrap_or_else(|| panic!("light-full.json: no chunk for dimension {dim}"));
        let sections = chunk
            .get("sections")
            .and_then(|s| s.as_array())
            .unwrap_or_else(|| panic!("light-full.json: missing `sections` for {dim}"));
        sections
            .iter()
            .map(|s| {
                let y = s
                    .get("sectionY")
                    .and_then(|v| v.as_i64())
                    .unwrap_or_else(|| {
                        panic!(
                            "light-full.json: {dim} section entry is missing a valid `sectionY` \
                             (a lost key must fail loudly, not silently collapse onto y=0)"
                        )
                    }) as i32;
                let block_state = s
                    .get("blocklight_state")
                    .and_then(|v| v.as_i64())
                    .map(|v| v as i32)
                    .unwrap_or(-1);
                let sky_state = s
                    .get("skylight_state")
                    .and_then(|v| v.as_i64())
                    .map(|v| v as i32)
                    .unwrap_or(-1);
                SavedLightSection {
                    y,
                    block_state,
                    block_light: json_array(s, "blocklight"),
                    sky_state,
                    sky_light: json_array(s, "skylight"),
                }
            })
            .filter(|s| s.block_state >= 0 || s.sky_state >= 0)
            .collect()
    }

    /// Decode a base64 light array from a `light-full.json` section entry; `None`
    /// when the key is absent.
    fn json_array(section: &Value, key: &str) -> Option<Vec<u8>> {
        let encoded = section.get(key)?.as_str()?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap_or_else(|e| panic!("light-full.json: invalid base64 for {key}: {e}"));
        assert_eq!(
            decoded.len(),
            ARRAY_SIZE,
            "light-full.json: {key} array has {} bytes, expected {ARRAY_SIZE}",
            decoded.len()
        );
        Some(decoded)
    }

    /// `#229` round-trip proof: parse the fixture's light tags -> rebuild the
    /// Starlight SWMR nibbles (`reconstruct_lights`) -> run the `saveLightHookReal`
    /// save seam (`light_save_surface`) and compare byte-for-byte with the
    /// fixture's own light surface. Empty when the surface round-trips exactly.
    fn round_trip_divergences(dim: &str, min_y: i32, height: i32, has_sky: bool) -> Vec<String> {
        let chunk = load_fixture(dim);
        let sections = parse_section_lights(&chunk);
        let light_correct = parse_light_correct(&chunk, true);
        assert!(
            light_correct,
            "{dim} FULL fixture must carry isLightOn + starlight.light_version=10"
        );
        let accessor = create(min_y, height);
        let rebuilt = reconstruct_lights(accessor, &sections, light_correct, has_sky);
        assert!(
            rebuilt.light_correct,
            "{dim} reconstructed light must stay correct"
        );
        let expected = fixture_surface(&chunk);
        let actual = light_save_surface(&accessor, &rebuilt.block_nibbles, &rebuilt.sky_nibbles);
        surface_divergences(&expected, &actual)
    }

    /// `SaveUtil.saveLightHookReal` / `loadLightHookReal` byte identity across
    /// all three dimensions (issue #229 / #184 Phase B): the light surface Paper
    /// wrote into the committed M0 FULL fixtures round-trips through the Starlight
    /// SWMR save state and comes back byte-identical.
    #[test]
    fn spike_229_fixture_light_round_trips_through_swmr_all_dimensions() {
        // (min_y, height, has_skylight): overworld -64/384, nether 0/256 (no sky,
        // per `dimensionType.hasSkylight()`), end 0/256 (sky, as the fixture's
        // SkyLight arrays attest).
        for (dim, min_y, height, has_sky) in [
            ("overworld", -64, 384, true),
            ("the_nether", 0, 256, false),
            ("the_end", 0, 256, true),
        ] {
            let divergences = round_trip_divergences(dim, min_y, height, has_sky);
            assert!(
                divergences.is_empty(),
                "{dim}: fixture light surface does NOT round-trip through SWMR save state: {}",
                divergences.join("; ")
            );
        }
    }

    /// `#229` cross-validation: the `saveLightHookReal` save seam, fed the SWMR
    /// nibbles rebuilt from the `.nbt` chunks, must reproduce the committed
    /// `light-full.json` full-array fixture byte-for-byte — the independent
    /// Paper ground truth captured through a separate decode path — for every
    /// dimension and every section. This makes `light-full.json` load-bearing:
    /// a decode bug shared by the NBT path can no longer hide a divergence.
    #[test]
    fn spike_229_save_surface_matches_committed_light_full_fixture() {
        let light_full = load_light_full_fixture();
        // (min_y, height, has_skylight): overworld -64/384, nether 0/256 (no sky,
        // per `dimensionType.hasSkylight()`), end 0/256 (sky). Explicit rather than
        // derived from the fixture so a fixture bug that dropped sky tags cannot
        // silently pass.
        for (dim, min_y, height, has_sky) in [
            ("overworld", -64, 384, true),
            ("the_nether", 0, 256, false),
            ("the_end", 0, 256, true),
        ] {
            let expected = json_surface(&light_full, dim);

            let chunk = load_fixture(dim);
            let sections = parse_section_lights(&chunk);
            let light_correct = parse_light_correct(&chunk, true);
            assert!(light_correct, "{dim} FULL fixture must be light-correct");
            let accessor = create(min_y, height);
            let rebuilt = reconstruct_lights(accessor, &sections, light_correct, has_sky);
            let actual =
                light_save_surface(&accessor, &rebuilt.block_nibbles, &rebuilt.sky_nibbles);

            let divergences = surface_divergences(&expected, &actual);
            assert!(
                divergences.is_empty(),
                "{dim}: SaveUtil save seam does NOT match the committed light-full.json \
                 fixture: {}",
                divergences.join("; ")
            );
        }
    }

    /// `#229` falsification: the existing `superflat` filler is NOT byte-identical
    /// with the overworld fixture — the exact secY -4 sky divergence (fixture:
    /// 4 zero planes + 12 FF, top opaque at -61; filler: 1 zero plane + 15 FF).
    /// Everything else matches.
    #[test]
    fn spike_229_superflat_filler_is_not_byte_identical_at_sec_y_minus_4() {
        let chunk = load_fixture("overworld");
        let sections = parse_section_lights(&chunk);
        let light_correct = parse_light_correct(&chunk, true);
        let accessor = create(-64, 384);
        let rebuilt = reconstruct_lights(accessor, &sections, light_correct, true);
        let fixture_surface =
            light_save_surface(&accessor, &rebuilt.block_nibbles, &rebuilt.sky_nibbles);

        // The superflat filler's light layers (superflat_sky_layers / _block_layers)
        // through `fromVanilla` -> the same save seam.
        let block: Vec<SwmrNibbleArray> = crate::superflat::superflat_block_layers()
            .iter()
            .map(|layer| SwmrNibbleArray::from_vanilla(layer.as_ref()))
            .collect();
        let sky: Vec<SwmrNibbleArray> = crate::superflat::superflat_sky_layers()
            .iter()
            .map(|layer| SwmrNibbleArray::from_vanilla(layer.as_ref()))
            .collect();
        let filler_surface = light_save_surface(&accessor, &block, &sky);

        let divergences = surface_divergences(&fixture_surface, &filler_surface);
        assert_eq!(
            divergences.len(),
            1,
            "expected exactly the secY -4 sky divergence, got: {divergences:?}"
        );
        assert!(
            divergences[0].contains("section y=-4"),
            "the divergence must name section y=-4: {}",
            divergences[0]
        );
    }

    /// Controlled negative: a tampered light byte is detected by the byte-identity
    /// comparison (the array differs, the surface shape does not).
    #[test]
    fn spike_229_tampered_light_byte_is_detected() {
        let expected = vec![SavedLightSection {
            y: -4,
            block_state: 1,
            block_light: None,
            sky_state: 2,
            sky_light: Some(vec![0xAB; ARRAY_SIZE]),
        }];
        let mut tampered = expected.clone();
        tampered[0].sky_light.as_mut().expect("tampered sky")[7] ^= 0x01;
        let divergences = surface_divergences(&expected, &tampered);
        assert_eq!(divergences.len(), 1);
        assert!(divergences[0].contains("section y=-4"));
    }

    /// Controlled negative: a tampered state int is detected even when the array
    /// bytes are unchanged.
    #[test]
    fn spike_229_tampered_state_is_detected() {
        let expected = vec![SavedLightSection {
            y: -4,
            block_state: 1,
            block_light: None,
            sky_state: 2,
            sky_light: Some(vec![0xAB; ARRAY_SIZE]),
        }];
        let mut tampered = expected.clone();
        tampered[0].sky_state = InitState::Uninitialised.to_i32(); // 2 -> 1
        let divergences = surface_divergences(&expected, &tampered);
        assert_eq!(divergences.len(), 1);
        assert!(divergences[0].contains("section y=-4"));
    }

    /// A missing interior section is reported as its own absence — the
    /// comparison pairs by Y, so the following section is not misattributed.
    #[test]
    fn spike_229_missing_section_is_reported_without_shift() {
        let section = |y: i32, sky_state: i32| SavedLightSection {
            y,
            block_state: 1,
            block_light: None,
            sky_state,
            sky_light: None,
        };
        let expected = vec![section(-5, 1), section(-4, 2), section(-3, 2)];
        // Actual drops the middle section: y=-4 is absent, y=-3 is untouched.
        let actual = vec![section(-5, 1), section(-3, 2)];
        let divergences = surface_divergences(&expected, &actual);
        assert_eq!(divergences.len(), 1);
        assert!(
            divergences[0].contains("section y=-4") && divergences[0].contains("expected present"),
            "y=-4 absence must be reported, not a shifted pair: {divergences:?}"
        );
    }

    /// Controlled negative: a duplicate section Y fails loudly instead of the
    /// last-wins overwrite silently dropping a divergence (a false PASS).
    #[test]
    #[should_panic(expected = "duplicate section y=-4")]
    fn spike_229_duplicate_section_y_panics() {
        let section = |y: i32| SavedLightSection {
            y,
            block_state: 1,
            block_light: None,
            sky_state: 1,
            sky_light: None,
        };
        let dup = vec![section(-4), section(-4)];
        let single = vec![section(-4)];
        let _ = surface_divergences(&dup, &single);
    }

    /// Controlled negative: a `light-full.json` section missing `sectionY` fails
    /// loudly instead of silently collapsing onto y=0.
    #[test]
    #[should_panic(expected = "missing a valid `sectionY`")]
    fn spike_229_json_surface_missing_section_y_panics() {
        let light_full = serde_json::json!({
            "chunks": [{
                "dim": "overworld",
                "sections": [{
                    "blocklight_state": 1
                }]
            }]
        });
        let _ = json_surface(&light_full, "overworld");
    }
}
