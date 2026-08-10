//! `io.papermc.paper.plugin.loader.library` — the library value layer (issue
//! #400, epic #28).
//!
//! Port of the four Paper 26.2 classes that make up the dependency-clean
//! library-loading value layer:
//!
//! - [`LibraryLoadingException`] (`LibraryLoadingException.java`) — the
//!   unchecked exception a library that fails to load throws.
//! - [`ClassPathLibrary`] (`ClassPathLibrary.java`) — the interface for
//!   libraries capable of registering themselves into a [`LibraryStore`].
//! - [`LibraryStore`] (`LibraryStore.java`) — the store a library registers
//!   paths into when a plugin's loader is processed.
//! - [`PaperLibraryStore`] (`PaperLibraryStore.java`, paper-server) — the
//!   server's `LibraryStore` implementation: an ordered, reference-returning
//!   `ArrayList<Path>`.
//!
//! Boundaries (per #400): no JVM-adapter loading, Maven/network resolution,
//! class loading, downloads, plugin lifecycle, or compatibility shims. This
//! slice only registers/accumulates library jar paths and carries the
//! registration error surface; resolution and classpath consumption are
//! future units (`paper.plugin.loader`, `paper.plugin.loader.library.impl`).

mod class_path_library;
mod library_loading_exception;
mod library_store;
mod paper_library_store;

pub use class_path_library::ClassPathLibrary;
pub use library_loading_exception::LibraryLoadingException;
pub use library_store::LibraryStore;
pub use paper_library_store::PaperLibraryStore;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// A library that registers a jar path into the store, mirroring
    /// `JarLibrary.register`'s success path.
    struct SimpleJarLibrary {
        path: String,
    }

    impl ClassPathLibrary for SimpleJarLibrary {
        fn register(&self, store: &mut dyn LibraryStore) -> Result<(), LibraryLoadingException> {
            store.add_library(Path::new(&self.path));
            Ok(())
        }
    }

    /// A library whose registration fails, mirroring `JarLibrary.register`'s
    /// missing-file path (exact message text: "Could not find library at " +
    /// path).
    struct MissingJarLibrary {
        path: String,
    }

    impl ClassPathLibrary for MissingJarLibrary {
        fn register(&self, _store: &mut dyn LibraryStore) -> Result<(), LibraryLoadingException> {
            // `Files.notExists(path)` is out of scope (no filesystem I/O in
            // this value slice), so the failure is injected directly. The
            // store is untouched, matching `JarLibrary`'s check-then-add
            // order (the exception is thrown before `addLibrary`).
            Err(LibraryLoadingException::new(format!(
                "Could not find library at {}",
                self.path
            )))
        }
    }

    /// The full registration flow: libraries register paths in order into the
    /// store, and the store carries them exactly as Paper's `PaperLibraryStore`
    /// would.
    #[test]
    fn libraries_register_paths_in_order_into_the_store() {
        let mut store = PaperLibraryStore::new();
        let first = SimpleJarLibrary {
            path: "libs/first.jar".to_string(),
        };
        let second = SimpleJarLibrary {
            path: "libs/second.jar".to_string(),
        };
        first.register(&mut store).expect("first library registers");
        second
            .register(&mut store)
            .expect("second library registers");
        assert_eq!(
            store.get_paths(),
            &[
                PathBuf::from("libs/first.jar"),
                PathBuf::from("libs/second.jar")
            ]
        );
    }

    /// A failing library surfaces the exact Paper message and leaves the store
    /// exactly as it was before the attempt (no partial registration).
    #[test]
    fn failing_library_returns_exact_message_and_mutates_nothing() {
        let mut store = PaperLibraryStore::new();
        let missing = MissingJarLibrary {
            path: "libs/never-here.jar".to_string(),
        };
        let error = missing
            .register(&mut store)
            .expect_err("missing library must fail");
        assert_eq!(
            error.get_message(),
            "Could not find library at libs/never-here.jar"
        );
        assert!(store.get_paths().is_empty());
    }

    /// Hostile duplicate check through the public API: registering the same
    /// library twice adds the path twice (ArrayList semantics, no dedup).
    #[test]
    fn registering_same_library_twice_keeps_both() {
        let mut store = PaperLibraryStore::new();
        let lib = SimpleJarLibrary {
            path: "libs/once.jar".to_string(),
        };
        lib.register(&mut store).unwrap();
        lib.register(&mut store).unwrap();
        assert_eq!(
            store.get_paths(),
            &[
                PathBuf::from("libs/once.jar"),
                PathBuf::from("libs/once.jar")
            ]
        );
    }

    /// Paper's consumption pattern (`PaperClasspathBuilder.buildLibraryPaths`)
    /// is fail-fast: the registration loop aborts at the first
    /// `LibraryLoadingException`, leaving the libraries registered before the
    /// failure in the store and the later ones never registered.
    #[test]
    fn fail_fast_loop_aborts_at_first_error_keeping_earlier_paths() {
        let mut store = PaperLibraryStore::new();
        let first = SimpleJarLibrary {
            path: "libs/before.jar".to_string(),
        };
        let broken = MissingJarLibrary {
            path: "libs/broken.jar".to_string(),
        };
        let last = SimpleJarLibrary {
            path: "libs/after.jar".to_string(),
        };

        first.register(&mut store).expect("first registers");
        let error = broken
            .register(&mut store)
            .expect_err("broken aborts the loop");
        assert_eq!(
            error.get_message(),
            "Could not find library at libs/broken.jar"
        );

        // The store holds the earlier path; the later library never ran.
        assert_eq!(store.get_paths(), &[PathBuf::from("libs/before.jar")]);
        last.register(&mut store).expect("still usable afterwards");
        assert_eq!(
            store.get_paths(),
            &[
                PathBuf::from("libs/before.jar"),
                PathBuf::from("libs/after.jar")
            ]
        );
    }
}
