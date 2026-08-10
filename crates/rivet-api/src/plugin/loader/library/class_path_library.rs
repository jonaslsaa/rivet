//! Port of `io.papermc.paper.plugin.loader.library.ClassPathLibrary`
//! (Paper 26.2).

use crate::plugin::loader::library::{LibraryLoadingException, LibraryStore};

/// Java `ClassPathLibrary` interface.
///
/// "The classpath library interface represents libraries that are capable of
/// registering themselves via `register(LibraryStore)` on any given
/// `LibraryStore`."
///
/// The Java signature is `void register(LibraryStore store) throws
/// LibraryLoadingException`. The throws clause is carried as a
/// `Result<(), LibraryLoadingException>`: a registration failure is a handled
/// error (it fails the plugin load), not a crash, and the exact message and
/// cause stay inspectable. The store is `&mut` — `register` mutates it
/// (Java mutates the `ArrayList` through a shared reference; the owned
/// `&mut` is the idiomatic Rust translation of that call pattern, and the
/// caller owns the store it registers libraries into). The library itself is
/// `&mut self`, matching Java's instance method whose implementations are
/// free to mutate their own state ("complex logic") as well as the store.
pub trait ClassPathLibrary {
    /// Java `register(LibraryStore store)` — registers this library into the
    /// passed store.
    ///
    /// "This method may either be implemented by the plugins themselves if
    /// they need complex logic, or existing API exposed implementations of
    /// this interface may be used."
    fn register(&mut self, store: &mut dyn LibraryStore) -> Result<(), LibraryLoadingException>;
}
