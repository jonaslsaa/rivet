//! Paper 26.2 GameTest resource census and fail-closed resource decoding.
//!
//! Records the exact pinned Paper 26.2 commit and the complete census of
//! GameTest resources bundled under `paper-server/src/minecraft/resources/`:
//! - `data/minecraft/test_environment/default.json`
//! - `data/minecraft/test_instance/always_pass.json`
//! - `data/minecraft/structure/empty.nbt` (referenced by `always_pass.json`)
//!
//! Missing, unreadable, extra, tampered, or cross-reference-broken resources
//! fail closed with [`GameTestResourceError`].

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_io;
use rivet_nbt::tag::Tag;
use rivet_registry::generated::registry_data::SYNCHRONIZED_NBT;
use rivet_registry::{Identifier, ResourceKey};
use rivet_util::DataInputStream;

use crate::server::gametest::game_test_instance::{GameTestInstance, InstanceDecodeError};
use crate::server::gametest::registries::{
    GameTestRegistries, GameTestRegistryError, TEST_ENVIRONMENT, TEST_INSTANCE,
};
use crate::server::gametest::test_environment_definition::{
    EnvironmentDecodeError, TestEnvironmentDefinition,
};
use crate::server::gametest::test_function_loader::TestFunctionLoaderRegistry;

/// Pinned Paper 26.2 commit serving as the sole behavioral and resource oracle.
pub const PAPER_COMMIT: &str = "0a993450f129c4942c2a9ed45ba047412b4667cf";

/// Kind of GameTest resource in the Paper 26.2 census.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameTestResourceKind {
    TestEnvironment,
    TestInstance,
    ReferencedStructure,
}

/// One expected resource entry in the pinned Paper 26.2 census.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedResource {
    pub relative_path: &'static str,
    pub kind: GameTestResourceKind,
    pub expected_bytes: usize,
    pub expected_sha256: &'static str,
}

/// Complete, ordered census of GameTest resources in Paper 26.2
/// (`0a993450f129c4942c2a9ed45ba047412b4667cf`).
pub const EXPECTED_RESOURCES: [ExpectedResource; 3] = [
    ExpectedResource {
        relative_path: "data/minecraft/test_environment/default.json",
        kind: GameTestResourceKind::TestEnvironment,
        expected_bytes: 54,
        expected_sha256: "90dc223eb105765dc4f04a479086b9b8277d38d7708d7fb33b58983e85678acc",
    },
    ExpectedResource {
        relative_path: "data/minecraft/test_instance/always_pass.json",
        kind: GameTestResourceKind::TestInstance,
        expected_bytes: 205,
        expected_sha256: "d7473c316c2c7e54624470689f02a0387e16113006f4ec42e76582f33d03819e",
    },
    ExpectedResource {
        relative_path: "data/minecraft/structure/empty.nbt",
        kind: GameTestResourceKind::ReferencedStructure,
        expected_bytes: 125,
        expected_sha256: "8d686f586f31fd8ddcd9c77bba3dab3e5bb996522c6fc77ec3a14ba1718d8bf9",
    },
];

/// Fail-closed error returned when verifying or loading GameTest resources.
#[derive(Debug, thiserror::Error)]
pub enum GameTestResourceError {
    #[error("missing expected GameTest resource: `{path}`")]
    MissingResource { path: String },
    #[error("unexpected GameTest resource in `{directory}`: `{path}`")]
    UnexpectedResource { directory: String, path: String },
    #[error("failed to read GameTest resource `{path}`: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "GameTest resource `{path}` size mismatch: expected {expected} bytes, got {actual} bytes"
    )]
    SizeMismatch {
        path: String,
        expected: usize,
        actual: usize,
    },
    #[error("GameTest resource `{path}` SHA-256 mismatch: expected {expected}, got {actual}")]
    HashMismatch {
        path: String,
        expected: &'static str,
        actual: String,
    },
    #[error("invalid JSON in `{path}`: {source}")]
    InvalidJson {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid NBT in `{registry}` / `{element}`: {message}")]
    InvalidNbt {
        registry: String,
        element: String,
        message: String,
    },
    #[error("missing pre-baked registry `{0}` in SYNCHRONIZED_NBT")]
    MissingPrebakedRegistry(&'static str),
    #[error("invalid resource key identifier `{value}` derived from `{path}`")]
    InvalidResourceId { path: String, value: String },
    #[error("failed to decode test_environment `{path}`: {source}")]
    EnvironmentDecode {
        path: String,
        #[source]
        source: EnvironmentDecodeError,
    },
    #[error("failed to decode test_instance `{path}`: {source}")]
    InstanceDecode {
        path: String,
        #[source]
        source: InstanceDecodeError,
    },
    #[error(
        "test_instance `{instance}` references missing structure `{structure}` (expected `{expected_path}`)"
    )]
    MissingReferencedStructure {
        instance: String,
        structure: String,
        expected_path: String,
    },
    #[error(transparent)]
    Registry(#[from] GameTestRegistryError),
}

/// Verify the exact Paper 26.2 resource census against `resource_root`
/// (e.g. `working/Paper/paper-server/src/minecraft/resources`) and decode all
/// `test_environment` and `test_instance` JSON files into a frozen
/// [`GameTestRegistries`] bundle.
///
/// Fails closed if any expected resource is missing, unreadable, wrong size,
/// wrong SHA-256, if unexpected files appear in `test_environment` or
/// `test_instance`, or if any cross-reference (environment, test function, or
/// structure NBT file) is missing.
pub fn load_strict_paper_resources(
    resource_root: &Path,
    loaders: &TestFunctionLoaderRegistry,
) -> Result<GameTestRegistries, GameTestResourceError> {
    // 1. Verify every expected census entry exists with exact byte length & SHA-256.
    for entry in EXPECTED_RESOURCES {
        let full_path = resource_root.join(entry.relative_path);
        if !full_path.is_file() {
            return Err(GameTestResourceError::MissingResource {
                path: entry.relative_path.to_string(),
            });
        }
        let bytes = fs::read(&full_path).map_err(|source| GameTestResourceError::Io {
            path: entry.relative_path.to_string(),
            source,
        })?;
        if bytes.len() != entry.expected_bytes {
            return Err(GameTestResourceError::SizeMismatch {
                path: entry.relative_path.to_string(),
                expected: entry.expected_bytes,
                actual: bytes.len(),
            });
        }
        let digest = sha256_hex(&bytes);
        if digest != entry.expected_sha256 {
            return Err(GameTestResourceError::HashMismatch {
                path: entry.relative_path.to_string(),
                expected: entry.expected_sha256,
                actual: digest,
            });
        }
    }

    // 2. Verify no unexpected files exist in data/minecraft/test_environment or test_instance.
    verify_exact_directory_entries(
        resource_root,
        "data/minecraft/test_environment",
        &["default.json"],
    )?;
    verify_exact_directory_entries(
        resource_root,
        "data/minecraft/test_instance",
        &["always_pass.json"],
    )?;

    // 3. Load and decode all files deterministically via the general directory loader.
    load_from_resource_root(resource_root, loaders)
}

/// Load and decode all `data/<namespace>/test_environment/*.json` and
/// `data/<namespace>/test_instance/*.json` under `resource_root`, validating
/// that every referenced environment, test function, and
/// `data/<ns>/structure/<path>.nbt` file exists.
///
/// Always fails closed on missing directories/files, invalid JSON/schema, or
/// broken references. Entries are registered in sorted path order for
/// deterministic registry IDs.
pub fn load_from_resource_root(
    resource_root: &Path,
    loaders: &TestFunctionLoaderRegistry,
) -> Result<GameTestRegistries, GameTestResourceError> {
    let env_dir = resource_root.join("data/minecraft/test_environment");
    let inst_dir = resource_root.join("data/minecraft/test_instance");

    let env_files = collect_sorted_json_files(&env_dir, "data/minecraft/test_environment")?;
    let inst_files = collect_sorted_json_files(&inst_dir, "data/minecraft/test_instance")?;

    let mut environments = Vec::with_capacity(env_files.len());
    for (rel_path, full_path) in env_files {
        let stem = strip_json_stem(&rel_path)?;
        let id = Identifier::try_parse_result(&format!("minecraft:{stem}"))
            .ok()
            .flatten()
            .ok_or_else(|| GameTestResourceError::InvalidResourceId {
                path: rel_path.clone(),
                value: format!("minecraft:{stem}"),
            })?;
        let text = fs::read_to_string(&full_path).map_err(|source| GameTestResourceError::Io {
            path: rel_path.clone(),
            source,
        })?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|source| GameTestResourceError::InvalidJson {
                path: rel_path.clone(),
                source,
            })?;
        let def = TestEnvironmentDefinition::decode_json(&json).map_err(|source| {
            GameTestResourceError::EnvironmentDecode {
                path: rel_path.clone(),
                source,
            }
        })?;
        environments.push((ResourceKey::create(&TEST_ENVIRONMENT, id), def));
    }

    let mut instances = Vec::with_capacity(inst_files.len());
    for (rel_path, full_path) in inst_files {
        let stem = strip_json_stem(&rel_path)?;
        let id = Identifier::try_parse_result(&format!("minecraft:{stem}"))
            .ok()
            .flatten()
            .ok_or_else(|| GameTestResourceError::InvalidResourceId {
                path: rel_path.clone(),
                value: format!("minecraft:{stem}"),
            })?;
        let text = fs::read_to_string(&full_path).map_err(|source| GameTestResourceError::Io {
            path: rel_path.clone(),
            source,
        })?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|source| GameTestResourceError::InvalidJson {
                path: rel_path.clone(),
                source,
            })?;
        let inst = GameTestInstance::decode_json(&json).map_err(|source| {
            GameTestResourceError::InstanceDecode {
                path: rel_path.clone(),
                source,
            }
        })?;

        // Fail closed if the referenced structure NBT file is missing.
        let structure_rel = format!(
            "data/{}/structure/{}.nbt",
            inst.structure().namespace(),
            inst.structure().path()
        );
        if !resource_root.join(&structure_rel).is_file() {
            return Err(GameTestResourceError::MissingReferencedStructure {
                instance: id.to_string(),
                structure: inst.structure().to_string(),
                expected_path: structure_rel,
            });
        }

        instances.push((ResourceKey::create(&TEST_INSTANCE, id), inst));
    }

    Ok(GameTestRegistries::from_entries(
        loaders,
        environments,
        instances,
    )?)
}

/// Decode `minecraft:test_environment` and `minecraft:test_instance` directly
/// from `rivet_registry::generated::registry_data::SYNCHRONIZED_NBT` (captured
/// from pinned Paper 26.2) into a validated frozen [`GameTestRegistries`].
pub fn load_from_synchronized_nbt(
    loaders: &TestFunctionLoaderRegistry,
) -> Result<GameTestRegistries, GameTestResourceError> {
    let env_entries = SYNCHRONIZED_NBT
        .iter()
        .find(|(reg, _)| *reg == "minecraft:test_environment")
        .map(|(_, entries)| *entries)
        .ok_or(GameTestResourceError::MissingPrebakedRegistry(
            "minecraft:test_environment",
        ))?;

    let inst_entries = SYNCHRONIZED_NBT
        .iter()
        .find(|(reg, _)| *reg == "minecraft:test_instance")
        .map(|(_, entries)| *entries)
        .ok_or(GameTestResourceError::MissingPrebakedRegistry(
            "minecraft:test_instance",
        ))?;

    let mut environments = Vec::with_capacity(env_entries.len());
    for (name, bytes) in env_entries {
        let id = Identifier::try_parse_result(name)
            .ok()
            .flatten()
            .ok_or_else(|| GameTestResourceError::InvalidResourceId {
                path: format!("SYNCHRONIZED_NBT/minecraft:test_environment/{name}"),
                value: (*name).to_string(),
            })?;
        let tag = decode_nbt_payload("minecraft:test_environment", name, bytes)?;
        let def = TestEnvironmentDefinition::decode_nbt(&tag).map_err(|source| {
            GameTestResourceError::EnvironmentDecode {
                path: format!("minecraft:test_environment/{name}"),
                source,
            }
        })?;
        environments.push((ResourceKey::create(&TEST_ENVIRONMENT, id), def));
    }

    let mut instances = Vec::with_capacity(inst_entries.len());
    for (name, bytes) in inst_entries {
        let id = Identifier::try_parse_result(name)
            .ok()
            .flatten()
            .ok_or_else(|| GameTestResourceError::InvalidResourceId {
                path: format!("SYNCHRONIZED_NBT/minecraft:test_instance/{name}"),
                value: (*name).to_string(),
            })?;
        let tag = decode_nbt_payload("minecraft:test_instance", name, bytes)?;
        let inst = GameTestInstance::decode_nbt(&tag).map_err(|source| {
            GameTestResourceError::InstanceDecode {
                path: format!("minecraft:test_instance/{name}"),
                source,
            }
        })?;
        instances.push((ResourceKey::create(&TEST_INSTANCE, id), inst));
    }

    Ok(GameTestRegistries::from_entries(
        loaders,
        environments,
        instances,
    )?)
}

fn decode_nbt_payload(
    registry: &str,
    element: &str,
    bytes: &[u8],
) -> Result<Tag, GameTestResourceError> {
    let mut input = DataInputStream::new(Cursor::new(bytes));
    let mut accounter = NbtAccounter::unlimited_heap();
    nbt_io::read_any_tag(&mut input, &mut accounter).map_err(|err| {
        GameTestResourceError::InvalidNbt {
            registry: registry.to_string(),
            element: element.to_string(),
            message: err.to_string(),
        }
    })
}

fn verify_exact_directory_entries(
    resource_root: &Path,
    rel_dir: &str,
    expected_names: &[&str],
) -> Result<(), GameTestResourceError> {
    let dir = resource_root.join(rel_dir);
    let entries = collect_sorted_files_recursive(&dir, rel_dir)?;
    let expected: BTreeMap<&str, ()> = expected_names.iter().map(|n| (*n, ())).collect();
    for (rel_file, _) in &entries {
        if !expected.contains_key(rel_file.as_str()) {
            return Err(GameTestResourceError::UnexpectedResource {
                directory: rel_dir.to_string(),
                path: format!("{rel_dir}/{rel_file}"),
            });
        }
    }
    for name in expected_names {
        if !entries.iter().any(|(rel, _)| rel == name) {
            return Err(GameTestResourceError::MissingResource {
                path: format!("{rel_dir}/{name}"),
            });
        }
    }
    Ok(())
}

fn collect_sorted_json_files(
    dir: &Path,
    rel_dir: &str,
) -> Result<Vec<(String, PathBuf)>, GameTestResourceError> {
    if !dir.is_dir() {
        return Err(GameTestResourceError::MissingResource {
            path: rel_dir.to_string(),
        });
    }
    let all = collect_sorted_files_recursive(dir, rel_dir)?;
    let mut json_files = Vec::new();
    for (rel, full) in all {
        if rel.ends_with(".json") {
            json_files.push((rel, full));
        } else {
            return Err(GameTestResourceError::UnexpectedResource {
                directory: rel_dir.to_string(),
                path: format!("{rel_dir}/{rel}"),
            });
        }
    }
    Ok(json_files)
}

fn collect_sorted_files_recursive(
    root: &Path,
    rel_dir: &str,
) -> Result<Vec<(String, PathBuf)>, GameTestResourceError> {
    let mut found = BTreeMap::new();
    collect_files_inner(root, root, rel_dir, &mut found)?;
    Ok(found.into_iter().collect())
}

fn collect_files_inner(
    root: &Path,
    current: &Path,
    rel_dir: &str,
    out: &mut BTreeMap<String, PathBuf>,
) -> Result<(), GameTestResourceError> {
    let read_dir = fs::read_dir(current).map_err(|source| GameTestResourceError::Io {
        path: rel_dir.to_string(),
        source,
    })?;
    for entry in read_dir {
        let entry = entry.map_err(|source| GameTestResourceError::Io {
            path: rel_dir.to_string(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_inner(root, &path, rel_dir, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(rel, path);
        }
    }
    Ok(())
}

fn strip_json_stem(rel_path: &str) -> Result<&str, GameTestResourceError> {
    rel_path
        .strip_suffix(".json")
        .ok_or_else(|| GameTestResourceError::InvalidResourceId {
            path: rel_path.to_string(),
            value: rel_path.to_string(),
        })
}

/// Pure-Rust SHA-256 (FIPS 180-4) returning lowercase hex, avoiding any
/// `Cargo.toml` edits (`sha2` is dev-only in `rivet-server`).
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let offset = i * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = String::with_capacity(64);
    for word in h {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{word:08x}");
    }
    out
}
