//! `rivet-codegen generate` registry-data half — consume the deterministic
//! `data/registry_data.json` fixture and emit `generated/registry_data.rs`: the
//! per-element NBT payload bytes the configuration registry sync serves to a
//! client that accepted no known packs (issue #109 pre-baked full content).
//!
//! `RegistrySynchronization.packRegistries` (with `Set.of()` — the client did
//! not accept the advertised `minecraft:core:26.2` pack) encodes every element
//! via its `RegistryData.elementCodec().encodeStart(NbtOps, value)`, producing
//! the `PackedRegistryEntry.data` `Optional<Tag>`. The canonical join capture
//! (`tools/rivet-capture/fixtures/join/capture.jsonl`) recorded those exact
//! bodies for all 29 `RegistryDataLoader.SYNCHRONIZED_REGISTRIES`: its
//! `registry_data` packets carry every element with content. `decode_capture_nbt.py`
//! walks each body byte-faithfully and writes each element's NBT payload (the
//! unnamed `Tag` bytes `writeAnyTag` produced — type byte + payload) to
//! `data/registry_data.json`, base64-encoded.
//!
//! The runtime server decodes those payloads back into `Tag`s and lets the
//! existing `PackedRegistryEntry` stream codec re-encode them; because
//! `NbtIo` read/write round-trips byte-for-byte (compound key order preserved
//! via `IndexMap`, DECISIONS.md D12), the re-emitted wire bytes are identical
//! to the capture's — the "pre-baked full content" path.
//!
//! Determinism: the fixture is read order-insensitively and re-emitted in the
//! fixed `SYNCHRONIZED_REGISTRIES` order (cross-checked against the names
//! fixture `data/synchronized_registries.json`); regeneration is byte-idempotent.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::reports::{SourceProvenance, sha256_hex};

pub fn default_input(repo_root: &Path) -> PathBuf {
    repo_root.join("tools/rivet-codegen/data/registry_data.json")
}

pub fn default_output(repo_root: &Path) -> PathBuf {
    repo_root.join("crates/rivet-registry/src/generated")
}

/// One synchronized registry surface: the registry key + each element's name
/// and its captured NBT payload bytes (wire order).
struct Registry {
    key: String,
    elements: Vec<Element>,
}

struct Element {
    name: String,
    data: Vec<u8>,
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
    let registries = validate(&root, &repo_root)?;
    let provenance = load_provenance(&input)?;

    fs::create_dir_all(&output).with_context(|| format!("create {}", output.display()))?;
    fs::write(
        output.join("registry_data.rs"),
        render(&registries, &provenance),
    )
    .context("write generated/registry_data.rs")?;

    println!(
        "Wrote {} registries / {} elements -> {}",
        registries.len(),
        registries.iter().map(|r| r.elements.len()).sum::<usize>(),
        output.display()
    );
    Ok(())
}

/// Full validation for the committed fixture: structural validation + the
/// cross-check against the names fixture `data/synchronized_registries.json`
/// (registry set, order, and per-element names) + the live capture sha256 +
/// every element carrying a non-empty compound payload.
fn validate(root: &Value, repo_root: &Path) -> Result<Vec<Registry>> {
    let registries = validate_structural(root)?;

    // The payloads must come from the exact committed capture: the fixture's
    // `capture_sha256` must equal the live capture file's sha256. A capture
    // edit (or a fixture regenerated against a different capture) fails here.
    let capture_path = repo_root.join("tools/rivet-capture/fixtures/join/capture.jsonl");
    let capture_bytes =
        fs::read(&capture_path).with_context(|| format!("read {}", capture_path.display()))?;
    let actual_capture = sha256_hex(&capture_bytes);
    let fixture_capture = root
        .get("capture_sha256")
        .and_then(Value::as_str)
        .context("registry_data.json is missing `capture_sha256`")?;
    if fixture_capture != actual_capture {
        bail!(
            "registry_data.json `capture_sha256` is {fixture_capture} but the committed capture has {actual_capture} — \
             regenerate with decode_capture_nbt.py"
        );
    }

    // Cross-check against the names fixture: the registry set (in order) and
    // each registry's element names (in order) must match. The names fixture
    // is itself validated against `SYNCHRONIZED_KEYS`/the id tables by
    // `synchronized::validate`, so matching it ties the NBT data to the
    // authoritative element tables.
    let sync_path = crate::synchronized::default_input(repo_root);
    let sync_json =
        fs::read_to_string(&sync_path).with_context(|| format!("read {}", sync_path.display()))?;
    let sync_root: Value = serde_json::from_str(&sync_json)
        .with_context(|| format!("parse {}", sync_path.display()))?;
    let sync_registries = sync_root
        .get("registries")
        .and_then(Value::as_object)
        .context("synchronized_registries.json is missing `registries`")?;

    // Iterate the canonical `SYNCHRONIZED_KEYS` order (matching how
    // `synchronized::validate` builds its tables) so the names fixture's map
    // iteration order cannot cause a spurious drift.
    let names_by_key: Vec<(String, Vec<String>)> = crate::synchronized::SYNCHRONIZED_KEYS
        .iter()
        .map(|key| {
            let names = sync_registries
                .get(*key)
                .with_context(|| {
                    format!("`{key}` names are missing from synchronized_registries.json")
                })?
                .as_array()
                .with_context(|| format!("`{key}` names are not a list"))?
                .iter()
                .map(|v| v.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
                .with_context(|| format!("`{key}` names are not strings"))?;
            Ok((key.to_string(), names))
        })
        .collect::<Result<Vec<_>>>()?;

    if names_by_key.len() != registries.len() {
        bail!(
            "registry_data.json has {} registries but synchronized_registries.json has {}",
            registries.len(),
            names_by_key.len()
        );
    }
    for (reg, (sync_key, sync_names)) in registries.iter().zip(&names_by_key) {
        if &reg.key != sync_key {
            bail!(
                "registry order drift: registry_data.json has `{}` where synchronized_registries.json has `{sync_key}`",
                reg.key
            );
        }
        if reg.elements.len() != sync_names.len() {
            bail!(
                "`{}` has {} elements but synchronized_registries.json has {} — drift",
                reg.key,
                reg.elements.len(),
                sync_names.len()
            );
        }
        for (i, (element, sync_name)) in reg.elements.iter().zip(sync_names).enumerate() {
            if &element.name != sync_name {
                bail!(
                    "`{}` element {i} is `{}` but synchronized_registries.json has `{sync_name}` — drift",
                    reg.key,
                    element.name
                );
            }
        }
    }

    Ok(registries)
}

/// Structural validation: top-level fields, the registry set, and per-element
/// payload presence/type.
fn validate_structural(root: &Value) -> Result<Vec<Registry>> {
    let object = root
        .as_object()
        .context("registry_data.json root must be a JSON object")?;
    for field in object.keys() {
        if !matches!(
            field.as_str(),
            "generator"
                | "minecraft_version"
                | "protocol_version"
                | "world_version"
                | "capture_sha256"
                | "registries"
        ) {
            bail!("registry_data.json has unexpected top-level field `{field}`");
        }
    }
    let _mc = object
        .get("minecraft_version")
        .and_then(Value::as_str)
        .context("registry_data.json is missing `minecraft_version`")?;
    for (field, min) in [("protocol_version", 0u64), ("world_version", 0u64)] {
        match object.get(field).and_then(Value::as_u64) {
            Some(v) if v >= min => {}
            Some(_) => bail!("registry_data.json `{field}` is out of range"),
            None => bail!("registry_data.json is missing `{field}`"),
        }
    }

    let registries_obj = object
        .get("registries")
        .and_then(Value::as_object)
        .context("registry_data.json is missing `registries`")?;

    // Iterate the canonical `SYNCHRONIZED_KEYS` order (the packet wire order)
    // and require exactly that set: the emitted table serves packets in this
    // order, so a fixture with a missing, extra, or reordered registry is drift.
    let mut registries = Vec::with_capacity(crate::synchronized::SYNCHRONIZED_KEYS.len());
    for key in crate::synchronized::SYNCHRONIZED_KEYS {
        let elements = registries_obj
            .get(*key)
            .and_then(Value::as_array)
            .with_context(|| format!("`{key}` is missing or not a list in registry_data.json"))?;
        if elements.is_empty() {
            bail!("`{key}` has an empty element list");
        }
        let mut seen = std::collections::HashSet::new();
        let mut parsed = Vec::with_capacity(elements.len());
        for (j, element) in elements.iter().enumerate() {
            let element_obj = element
                .as_object()
                .with_context(|| format!("`{key}` element {j} is not an object"))?;
            for field in element_obj.keys() {
                if !matches!(field.as_str(), "id" | "data_b64") {
                    bail!("`{key}` element {j} has unexpected field `{field}`");
                }
            }
            let name = element_obj
                .get("id")
                .and_then(Value::as_str)
                .with_context(|| format!("`{key}` element {j} is missing `id`"))?
                .to_string();
            if !name.contains(':') {
                bail!("`{key}` element `{name}` is not namespaced");
            }
            if !seen.insert(name.clone()) {
                bail!("`{key}` has duplicate element `{name}`");
            }
            let data_b64 = element_obj
                .get("data_b64")
                .and_then(Value::as_str)
                .with_context(|| format!("`{key}` element `{name}` is missing `data_b64`"))?;
            use base64::Engine as _;
            let data = base64::engine::general_purpose::STANDARD
                .decode(data_b64)
                .with_context(|| format!("`{key}` element `{name}` has invalid base64"))?;
            if data.is_empty() {
                bail!("`{key}` element `{name}` has an empty payload");
            }
            // The full-content path always encodes a compound
            // (`elementCodec().encodeStart`), so a non-compound payload is
            // drift (a capture where the client held the pack, or a walk bug).
            if data[0] != 0x0a {
                bail!(
                    "`{key}` element `{name}` payload is not a compound tag (type 0x{:02x})",
                    data[0]
                );
            }
            parsed.push(Element { name, data });
        }
        registries.push(Registry {
            key: key.to_string(),
            elements: parsed,
        });
    }
    // Unknown registries in the fixture are drift (mirrors
    // `synchronized::validate`).
    for key in registries_obj.keys() {
        if !crate::synchronized::SYNCHRONIZED_KEYS.contains(&key.as_str()) {
            bail!("registry_data.json has unknown registry `{key}`");
        }
    }

    Ok(registries)
}

/// Link the fixture to its pinned provenance: the fixture must match the sha256
/// recorded next to it in `data/registry_data.manifest.json`, and the emitted
/// header carries that provenance (capture identity + MC/proto/world versions).
fn load_provenance(input: &Path) -> Result<SourceProvenance> {
    let manifest_path = input
        .parent()
        .map(|p| p.join("registry_data.manifest.json"))
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
    let actual = sha256_hex(&bytes);
    if actual != manifest.file.sha256 {
        bail!(
            "registry_data.json does not match its provenance manifest (expected sha256 {}, got {})",
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

/// Render `generated/registry_data.rs`.
fn render(registries: &[Registry], source: &SourceProvenance) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by `tools/rivet-codegen generate` from data/registry_data.json\n\
         // (the canonical join capture's registry_data NBT element payloads, decoded via\n\
         //  decode_capture_nbt.py; MC {}, protocol {}, world {}).\n\
         // Source capture sha256 {}; provenance linked to data/registry_data.manifest.json.\n\
         // Do not edit by hand — PORTING.md: registries/data are generated, not hand-ported.\n\n",
        source.minecraft_version,
        source.protocol_version,
        source.world_version,
        source.jar_sha256.get(..16).unwrap_or(&source.jar_sha256)
    ));
    out.push_str(
        "// The per-element `data` NBT payloads `RegistrySynchronization.packRegistry` serves\n\
         // when the client accepted no known packs (every element encoded via its\n\
         // `RegistryData.elementCodec`). Each entry is the element id name + its unnamed\n\
         // NBT payload (`writeAnyTag` form: type byte + payload, a compound) exactly as\n\
         // Paper wrote it in the canonical join capture. `PackedRegistryEntry.STREAM_CODEC`\n\
         // re-encodes the decoded `Tag` byte-for-byte (issue #109 pre-baked full content).\n\n",
    );

    out.push_str(
        "/// One synchronized registry's pre-baked elements: each `(element id, NBT payload\n\
         /// bytes)` in ascending registry id order. Named to keep the `SYNCHRONIZED_NBT`\n\
         /// static's type readable (clippy `type_complexity`).\n",
    );
    out.push_str(
        "pub type SynchronizedRegistryElements = &'static [(&'static str, &'static [u8])];\n",
    );
    out.push_str(
        "/// The 29 `RegistryDataLoader.SYNCHRONIZED_REGISTRIES` full-content payloads: each\n\
         /// registry key paired with `(element id, NBT payload bytes)` in ascending registry\n\
         /// id order (aligned with `SYNCHRONIZED_REGISTRIES`).\n",
    );
    out.push_str("pub static SYNCHRONIZED_NBT: &[(&str, SynchronizedRegistryElements)] = &[\n");
    for r in registries {
        out.push_str(&format!("    ({:?}, &[\n", r.key));
        for e in &r.elements {
            out.push_str(&format!(
                "        ({:?}, {}),\n",
                e.name,
                render_bytes(&e.data)
            ));
        }
        out.push_str("    ]),\n");
    }
    out.push_str("];\n");
    out
}

/// A Rust byte-string literal for a payload. Printable ASCII (except `"` and
/// `\`) is emitted literally so the generated table stays readable; everything
/// else is a `\xNN` escape. Deterministic and byte-exact (see `tests::byte_literal_round_trip`).
fn render_bytes(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 4 + 4);
    s.push_str("b\"");
    for &b in data {
        match b {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            0x20..=0x7e => s.push(b as char),
            _ => write!(s, "\\x{b:02x}").unwrap(),
        }
    }
    s.push('"');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a Rust byte-string literal (the subset `render_bytes` emits) back
    /// into its bytes.
    fn parse_bytes(literal: &str) -> Vec<u8> {
        assert!(literal.starts_with("b\"") && literal.ends_with('"'));
        let inner = &literal[2..literal.len() - 1];
        let bytes = inner.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => match bytes[i + 1] {
                    b'"' => {
                        out.push(b'"');
                        i += 2;
                    }
                    b'\\' => {
                        out.push(b'\\');
                        i += 2;
                    }
                    b'x' => {
                        let hex = std::str::from_utf8(&bytes[i + 2..i + 4]).unwrap();
                        out.push(u8::from_str_radix(hex, 16).unwrap());
                        i += 4;
                    }
                    other => panic!("unexpected escape \\{}", other as char),
                },
                c => {
                    out.push(c);
                    i += 1;
                }
            }
        }
        out
    }

    #[test]
    fn byte_literal_round_trips_all_values() {
        // Every byte value, including the ones needing escapes (`"`, `\`) and
        // every non-printable.
        for b in 0u16..=255 {
            let data = [b as u8];
            let literal = render_bytes(&data);
            assert_eq!(parse_bytes(&literal), data, "byte {b}");
        }
    }

    #[test]
    fn byte_literal_is_deterministic() {
        let data = b"\x0a\x00\x04chat\t\x00\"quoted\"\\path\\\x01";
        assert_eq!(render_bytes(data), render_bytes(data));
        // The printable quote/backslash are escaped, binary bytes are `\xNN`.
        let literal = render_bytes(data);
        assert!(literal.contains("\\\""));
        assert!(literal.contains("\\\\"));
        assert!(literal.contains("\\x0a"));
    }

    #[test]
    fn render_is_deterministic() {
        let registries = vec![Registry {
            key: "minecraft:worldgen/biome".into(),
            elements: vec![
                Element {
                    name: "minecraft:badlands".into(),
                    data: vec![0x0a, 0x0a, 0x00],
                },
                Element {
                    name: "minecraft:bamboo_jungle".into(),
                    data: vec![0x0a, 0x00],
                },
            ],
        }];
        let source: SourceProvenance = serde_json::from_str(
            r#"{"jar":"x","jar_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","minecraft_version":"26.2","protocol_version":776,"world_version":4903}"#,
        )
        .unwrap();
        let a = render(&registries, &source);
        let b = render(&registries, &source);
        assert_eq!(a, b);
        assert!(a.contains("(\"minecraft:worldgen/biome\", &["));
        assert!(a.contains("(\"minecraft:badlands\", b\"\\x0a\\x0a\\x00\")"));
    }

    #[test]
    fn unknown_registry_key_is_rejected() {
        // Every `SYNCHRONIZED_KEYS` registry present with a valid element list,
        // plus an unknown extra key — the fixture must be rejected as drift
        // (mirrors `synchronized::validate`).
        let mut registries = serde_json::Map::new();
        for key in crate::synchronized::SYNCHRONIZED_KEYS {
            registries.insert(
                (*key).to_string(),
                serde_json::json!([{"id": "minecraft:element", "data_b64": "CgA="}]),
            );
        }
        registries.insert(
            "minecraft:unknown_registry".to_string(),
            serde_json::json!([{"id": "minecraft:element", "data_b64": "CgA="}]),
        );
        let root = serde_json::json!({
            "generator": "test",
            "minecraft_version": "26.2",
            "protocol_version": 776,
            "world_version": 4903,
            "capture_sha256": "test",
            "registries": registries,
        });
        match validate_structural(&root) {
            Err(err) => assert!(
                err.to_string().contains("unknown registry"),
                "unexpected error: {err}"
            ),
            Ok(_) => panic!("unknown registry was not rejected"),
        }
    }

    #[test]
    fn base64_decode_round_trips() {
        // The decoder is the `base64` crate's STANDARD engine; the fixture's
        // payloads must decode (they were encoded with the same alphabet).
        use base64::Engine as _;
        let data = b"hello \x00\xff world";
        let encoded = base64::engine::general_purpose::STANDARD.encode(data);
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(&encoded)
                .unwrap(),
            data
        );
    }

    #[test]
    fn provenance_rejects_unpinned_source() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("registry_data.json");
        let bytes = b"fixture\n";
        fs::write(&input, bytes).unwrap();
        let manifest_path = root.path().join("registry_data.manifest.json");
        for (jar_sha256, paper_git, expected) in [
            (
                "deadbeef",
                crate::reports::PINNED_PAPER_COMMIT,
                "source SHA",
            ),
            (
                crate::reports::PINNED_JOIN_CAPTURE_SHA256,
                "deadbeef",
                "Paper commit",
            ),
        ] {
            fs::write(
                &manifest_path,
                serde_json::to_vec(&serde_json::json!({
                    "source": {
                        "jar": "capture.jsonl",
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
