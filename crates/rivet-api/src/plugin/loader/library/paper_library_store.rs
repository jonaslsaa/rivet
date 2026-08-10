//! Port of `io.papermc.paper.plugin.loader.library.PaperLibraryStore`
//! (paper-server, Paper 26.2).

use std::path::{Path, PathBuf};

use crate::plugin::loader::library::LibraryStore;

/// Java `PaperLibraryStore implements LibraryStore` — the server's library
/// store.
///
/// Holds the registered library paths in an `ArrayList<Path>` (insertion
/// order, duplicates retained) and exposes them by reference via
/// `getPaths()`. The port keeps the same ordering and duplicate semantics and
/// returns the live accumulated slice, which grows with later `add_library`
/// calls — Java's `getPaths()` returns the live backing list.
#[derive(Debug, Default)]
pub struct PaperLibraryStore {
    paths: Vec<PathBuf>,
}

impl PaperLibraryStore {
    /// Java `new PaperLibraryStore()` — an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Java `getPaths()` — the backing `List<Path>`, by reference. The caller
    /// observes the store's own list, so it grows with subsequent
    /// `addLibrary` calls.
    pub fn get_paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

impl LibraryStore for PaperLibraryStore {
    /// Java `addLibrary(Path library)` — `this.paths.add(library)`.
    fn add_library(&mut self, library: &Path) {
        self.paths.push(library.to_path_buf());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `addLibrary` appends in insertion order and `getPaths` returns the
    /// store's own accumulated list — Paper's `ArrayList` ordering, with
    /// later additions visible to subsequent `get_paths` calls. (Java's
    /// "same live reference" can't be held across a mutation in safe Rust;
    /// the borrow-checker enforces at compile time that a held slice cannot
    /// overlap a push, where a `RefCell` would instead panic at runtime.)
    #[test]
    fn add_library_appends_in_order_and_get_paths_accumulates() {
        let mut store = PaperLibraryStore::new();
        assert!(store.get_paths().is_empty());

        let first = Path::new("libs/first.jar");
        let second = Path::new("libs/second.jar");
        store.add_library(first);
        store.add_library(second);

        assert_eq!(
            store.get_paths(),
            &[first.to_path_buf(), second.to_path_buf()]
        );

        store.add_library(Path::new("libs/third.jar"));
        assert_eq!(store.get_paths().len(), 3);
        assert_eq!(store.get_paths()[2], PathBuf::from("libs/third.jar"));
    }

    /// The store is a plain ordered list: duplicates are retained, matching
    /// `ArrayList.add`.
    #[test]
    fn duplicates_are_retained() {
        let mut store = PaperLibraryStore::new();
        let jar = Path::new("libs/dupe.jar");
        store.add_library(jar);
        store.add_library(jar);
        store.add_library(jar);
        assert_eq!(store.get_paths().len(), 3);
        assert!(
            store
                .get_paths()
                .iter()
                .all(|path| path == &jar.to_path_buf())
        );
    }

    /// The `LibraryStore` trait method dispatches to the same backing list.
    #[test]
    fn trait_method_adds_to_same_store() {
        let mut store = PaperLibraryStore::new();
        let library_store: &mut dyn LibraryStore = &mut store;
        library_store.add_library(Path::new("libs/trait.jar"));
        assert_eq!(store.get_paths(), &[PathBuf::from("libs/trait.jar")]);
    }
}
