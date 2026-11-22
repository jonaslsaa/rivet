//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager`
//! (MC 26.2) loader path validation, source abstraction, compressed `.nbt` and
//! `.snbt` loading, and deterministic in-memory template cache.

use super::structure_template::StructureTemplate;
use indexmap::{IndexMap, IndexSet};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::{nbt_io, nbt_utils};
use rivet_registry::identifier::Identifier;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Component, Path, PathBuf};

/// Errors produced when validating template identifiers or loading/saving templates.
#[derive(Debug)]
pub enum StructureTemplateError {
    InvalidPath(String),
    Io(io::Error),
    Nbt(String),
}

impl std::fmt::Display for StructureTemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPath(msg) => write!(f, "Invalid structure template path: {msg}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Nbt(msg) => write!(f, "NBT parse error: {msg}"),
        }
    }
}

impl std::error::Error for StructureTemplateError {}

impl From<io::Error> for StructureTemplateError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Validates that a structure template identifier path cannot escape the
/// template root directory (matching Paper's `FileUtil.validatePath` /
/// `StructureTemplateManager.createAndValidatePathToGeneratedStructure`).
pub fn validate_template_identifier(id: &Identifier) -> Result<(), StructureTemplateError> {
    let path = id.path();
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path.contains('\0')
    {
        return Err(StructureTemplateError::InvalidPath(id.to_string()));
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(StructureTemplateError::InvalidPath(id.to_string()));
        }
    }
    let rel = Path::new(path);
    for comp in rel.components() {
        match comp {
            Component::Normal(_) => {}
            _ => return Err(StructureTemplateError::InvalidPath(id.to_string())),
        }
    }
    Ok(())
}

/// Resolves `<root>/<namespace>/<folder>/<path><suffix>` after validating `id`.
pub fn resolve_template_path(
    root: &Path,
    id: &Identifier,
    folder: &str,
    suffix: &str,
) -> Result<PathBuf, StructureTemplateError> {
    validate_template_identifier(id)?;
    let mut out = root.join(id.namespace()).join(folder);
    let rel = format!("{}{}", id.path(), suffix);
    out.push(rel);
    Ok(out)
}

/// Abstract template source (mirrors Paper's `StructureTemplateManager.Source(loader, lister)`).
pub trait StructureTemplateSource: Send + Sync {
    fn load(&self, id: &Identifier) -> Result<Option<StructureTemplate>, StructureTemplateError>;
    fn list(&self) -> Vec<Identifier>;
}

/// In-memory source preserving insertion order (`IndexMap`).
#[derive(Default)]
pub struct MemoryTemplateSource {
    templates: IndexMap<Identifier, StructureTemplate>,
}

impl MemoryTemplateSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: Identifier, template: StructureTemplate) {
        self.templates.insert(id, template);
    }
}

impl StructureTemplateSource for MemoryTemplateSource {
    fn load(&self, id: &Identifier) -> Result<Option<StructureTemplate>, StructureTemplateError> {
        validate_template_identifier(id)?;
        Ok(self.templates.get(id).cloned())
    }

    fn list(&self) -> Vec<Identifier> {
        self.templates.keys().cloned().collect()
    }
}

/// `StructureTemplateManager.STRUCTURE_DIRECTORY_NAME`.
pub const STRUCTURE_DIRECTORY_NAME: &str = "structure";

/// Filesystem source reading gzip-compressed `.nbt` or text `.snbt` templates
/// under `<root>/<namespace>/structure/<path>.<ext>`.
pub struct FilesystemTemplateSource {
    root: PathBuf,
    folder: String,
    snbt: bool,
}

impl FilesystemTemplateSource {
    pub fn nbt(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            folder: STRUCTURE_DIRECTORY_NAME.to_string(),
            snbt: false,
        }
    }

    pub fn snbt(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            folder: STRUCTURE_DIRECTORY_NAME.to_string(),
            snbt: true,
        }
    }

    fn suffix(&self) -> &'static str {
        if self.snbt { ".snbt" } else { ".nbt" }
    }
}

impl StructureTemplateSource for FilesystemTemplateSource {
    fn load(&self, id: &Identifier) -> Result<Option<StructureTemplate>, StructureTemplateError> {
        let path = resolve_template_path(&self.root, id, &self.folder, self.suffix())?;
        if !path.is_file() {
            return Ok(None);
        }
        let tag = if self.snbt {
            let text = fs::read_to_string(&path)?;
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                nbt_utils::snbt_to_structure(&text)
            })) {
                Ok(res) => res.map_err(|e| StructureTemplateError::Nbt(format!("{e:?}")))?,
                Err(payload) => {
                    let msg = payload
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| payload.downcast_ref::<&str>().copied())
                        .unwrap_or("malformed packed SNBT structure");
                    return Err(StructureTemplateError::Nbt(msg.to_string()));
                }
            }
        } else {
            let mut reader = BufReader::new(File::open(&path)?);
            nbt_io::read_compressed(&mut reader, &mut NbtAccounter::unlimited_heap())
                .map_err(|e| StructureTemplateError::Nbt(format!("{e:?}")))?
        };
        let mut template = StructureTemplate::new();
        template.load(&tag);
        Ok(Some(template))
    }

    fn list(&self) -> Vec<Identifier> {
        let mut ids = Vec::new();
        let Ok(ns_entries) = fs::read_dir(&self.root) else {
            return ids;
        };
        let mut namespaces: Vec<String> = ns_entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        namespaces.sort();

        for ns in namespaces {
            let base = self.root.join(&ns).join(&self.folder);
            if !base.is_dir() {
                continue;
            }
            let mut files = Vec::new();
            collect_files_sorted(&base, &base, self.suffix(), &mut files);
            for rel_no_ext in files {
                if let Some(id) = Identifier::try_build(&ns, &rel_no_ext) {
                    ids.push(id);
                }
            }
        }
        ids
    }
}

fn collect_files_sorted(base: &Path, dir: &Path, suffix: &str, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_files_sorted(base, &path, suffix, out);
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name.ends_with(suffix)
            && let Ok(rel) = path.strip_prefix(base)
            && let Some(rel_str) = rel.to_str()
        {
            let stripped = rel_str
                .strip_suffix(suffix)
                .unwrap_or(rel_str)
                .replace('\\', "/");
            out.push(stripped);
        }
    }
}

/// `net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager`.
///
/// Queries configured `sources` in declaration order and caches loaded/missing
/// results (`Option<StructureTemplate>`) per `Identifier`.
pub struct StructureTemplateManager {
    sources: Vec<Box<dyn StructureTemplateSource>>,
    generated_dir: Option<PathBuf>,
    structure_repository: IndexMap<Identifier, Option<StructureTemplate>>,
}

impl Default for StructureTemplateManager {
    fn default() -> Self {
        Self::new(Vec::new(), None)
    }
}

impl StructureTemplateManager {
    pub fn new(
        sources: Vec<Box<dyn StructureTemplateSource>>,
        generated_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            sources,
            generated_dir,
            structure_repository: IndexMap::new(),
        }
    }

    /// `StructureTemplateManager.getOrCreate(Identifier)`.
    pub fn get_or_create(
        &mut self,
        id: &Identifier,
    ) -> Result<&StructureTemplate, StructureTemplateError> {
        validate_template_identifier(id)?;
        let loaded = self.get(id)?;
        if loaded.is_none() {
            self.structure_repository
                .insert(id.clone(), Some(StructureTemplate::new()));
        }
        Ok(self
            .structure_repository
            .get(id)
            .and_then(|opt| opt.as_ref())
            .expect("inserted above"))
    }

    /// `StructureTemplateManager.get(Identifier)` — checks cache first, then
    /// queries sources in order and caches the outcome.
    pub fn get(
        &mut self,
        id: &Identifier,
    ) -> Result<Option<&StructureTemplate>, StructureTemplateError> {
        validate_template_identifier(id)?;
        if !self.structure_repository.contains_key(id) {
            let mut found = None;
            for source in &self.sources {
                if let Some(template) = source.load(id)? {
                    found = Some(template);
                    break;
                }
            }
            self.structure_repository.insert(id.clone(), found);
        }
        Ok(self
            .structure_repository
            .get(id)
            .and_then(|opt| opt.as_ref()))
    }

    /// `StructureTemplateManager.listTemplates()` — distinct template IDs across
    /// all sources in deterministic order.
    pub fn list_templates(&self) -> Vec<Identifier> {
        let mut seen: IndexSet<Identifier> = IndexSet::new();
        for source in &self.sources {
            for id in source.list() {
                seen.insert(id);
            }
        }
        seen.into_iter().collect()
    }

    /// `StructureTemplateManager.save(Identifier)` — writes the cached template
    /// to `<generated_dir>/<namespace>/structure/<path>.nbt` as compressed NBT.
    pub fn save(&self, id: &Identifier) -> Result<bool, StructureTemplateError> {
        let Some(Some(template)) = self.structure_repository.get(id) else {
            return Ok(false);
        };
        let Some(dir) = &self.generated_dir else {
            return Ok(false);
        };
        let path = resolve_template_path(dir, id, STRUCTURE_DIRECTORY_NAME, ".nbt")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut tag = CompoundTag::new();
        template.save(&mut tag);
        let mut writer = BufWriter::new(File::create(&path)?);
        nbt_io::write_compressed(&tag, &mut writer)
            .map_err(|e| StructureTemplateError::Nbt(format!("{e:?}")))?;
        Ok(true)
    }

    /// `StructureTemplateManager.remove(Identifier)` — evicts from cache.
    pub fn remove(&mut self, id: &Identifier) {
        self.structure_repository.shift_remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::structure::templatesystem::structure_template::{
        Palette, StructureBlockInfo,
    };
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::BlockPos;
    use rivet_registry::generated::blocks::BlockId;

    #[test]
    fn validate_template_identifier_rejects_traversal() {
        let bad = Identifier::from_namespace_and_path("minecraft", "../escape");
        assert!(validate_template_identifier(&bad).is_err());

        let good = Identifier::from_namespace_and_path("minecraft", "village/plains/houses");
        assert!(validate_template_identifier(&good).is_ok());
    }

    #[test]
    fn manager_queries_sources_in_order_and_caches() {
        let id = Identifier::from_namespace_and_path("minecraft", "test/house");

        let mut first = MemoryTemplateSource::new();
        first.insert(
            id.clone(),
            StructureTemplate::from_parts(
                [1, 1, 1],
                vec![Palette::new(vec![StructureBlockInfo::new(
                    BlockPos::ZERO,
                    BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
                    None,
                )])],
                Vec::new(),
            ),
        );

        let mut second = MemoryTemplateSource::new();
        second.insert(
            id.clone(),
            StructureTemplate::from_parts(
                [2, 2, 2],
                vec![Palette::new(vec![StructureBlockInfo::new(
                    BlockPos::ZERO,
                    BlockState::of(BlockId::from_name("minecraft:dirt").unwrap()),
                    None,
                )])],
                Vec::new(),
            ),
        );

        let mut mgr = StructureTemplateManager::new(vec![Box::new(first), Box::new(second)], None);
        let tpl = mgr.get(&id).unwrap().expect("should find template");
        assert_eq!(tpl.get_size(), [1, 1, 1]);
        assert_eq!(mgr.list_templates(), vec![id]);
    }

    #[test]
    fn snbt_source_unpacks_paper_packed_snbt() {
        let dir =
            std::env::temp_dir().join(format!("rivet_snbt_packed_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let id = Identifier::from_namespace_and_path("minecraft", "packed_box");
        let tpl = StructureTemplate::from_parts(
            [1, 1, 1],
            vec![Palette::new(vec![StructureBlockInfo::new(
                BlockPos::ZERO,
                BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
                None,
            )])],
            Vec::new(),
        );
        let mut raw = CompoundTag::new();
        tpl.save(&mut raw);
        let packed_snbt = nbt_utils::structure_to_snbt(&mut raw);
        assert!(packed_snbt.contains("data:"));

        let source = FilesystemTemplateSource::snbt(&dir);
        let file_path = dir
            .join("minecraft")
            .join(&source.folder)
            .join("packed_box.snbt");
        fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        fs::write(&file_path, packed_snbt).unwrap();

        let loaded = source.load(&id).unwrap().expect("should load packed snbt");
        assert_eq!(loaded.get_size(), [1, 1, 1]);
        assert_eq!(loaded.palettes().len(), 1);
        assert_eq!(loaded.palettes()[0].blocks().len(), 1);
        assert_eq!(
            loaded.palettes()[0].blocks()[0].state,
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap())
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn filesystem_source_and_save_use_singular_structure_directory() {
        let dir =
            std::env::temp_dir().join(format!("rivet_structure_dir_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let id = Identifier::from_namespace_and_path("minecraft", "village/house");

        let mut mem = MemoryTemplateSource::new();
        mem.insert(
            id.clone(),
            StructureTemplate::from_parts(
                [1, 1, 1],
                vec![Palette::new(vec![StructureBlockInfo::new(
                    BlockPos::ZERO,
                    BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
                    None,
                )])],
                Vec::new(),
            ),
        );

        let mut mgr = StructureTemplateManager::new(vec![Box::new(mem)], Some(dir.clone()));
        assert!(mgr.get(&id).unwrap().is_some());
        assert!(mgr.save(&id).unwrap());

        let expected_path = dir
            .join("minecraft")
            .join("structure")
            .join("village/house.nbt");
        assert!(expected_path.is_file());

        let fs_source = FilesystemTemplateSource::nbt(&dir);
        assert_eq!(fs_source.list(), vec![id.clone()]);
        assert!(fs_source.load(&id).unwrap().is_some());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snbt_source_returns_error_on_missing_palette_entry() {
        let dir = std::env::temp_dir().join(format!(
            "rivet_snbt_bad_palette_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let id = Identifier::from_namespace_and_path("minecraft", "bad_palette");
        let file = dir
            .join("minecraft")
            .join("structure")
            .join("bad_palette.snbt");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(
            &file,
            r#"{size:[1,1,1],palette:["minecraft:stone"],data:[{pos:[0,0,0],state:"minecraft:dirt"}],entities:[]}"#,
        )
        .unwrap();

        let source = FilesystemTemplateSource::snbt(&dir);
        let err = source
            .load(&id)
            .expect_err("missing palette entry should return Err instead of panicking");
        assert!(matches!(err, StructureTemplateError::Nbt(_)));

        let _ = fs::remove_dir_all(&dir);
    }
}
