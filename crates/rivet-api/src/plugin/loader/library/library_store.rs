//! Port of `io.papermc.paper.plugin.loader.library.LibraryStore` (Paper 26.2).

use std::path::Path;

/// Java `LibraryStore` interface.
///
/// "Represents a storage that stores library jars."
///
/// The library store API allows plugins to register specific dependencies
/// into their runtime classloader when their `PluginLoader` is processed. The
/// Java interface is annotated `@ApiStatus.Internal` (internal API surface)
/// and `@NullMarked` (the library path is non-null).
///
/// Java's `void addLibrary(Path library)` cannot fail and mutates the store's
/// list; the port takes `&mut self` and a `&Path`.
pub trait LibraryStore {
    /// Java `addLibrary(Path library)` — adds the provided library path to
    /// this library store.
    ///
    /// The path is the library's jar file on disk.
    fn add_library(&mut self, library: &Path);
}
