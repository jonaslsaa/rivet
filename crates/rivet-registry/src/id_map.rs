//! Port of `net.minecraft.core.IdMap<T>` (MC 26.2).
//!
//! PROVENANCE: leaf of the `mc.core` manifest unit. Java source:
//! `net/minecraft/core/IdMap.java` (31 lines, 26.2).
//!
//! Ownership B — registry lifecycle (`net.minecraft.core`).
//!
//! Java maps `@Nullable` returns to `Option<&T>`: `IdMap.byId(int)` returns a
//! Java object reference (`@Nullable T`), and the id <-> value implementors
//! (`Registry<T>`, `HolderIdMap`) own their elements, so the borrowed form is
//! the faithful one.
//!
//! The default combinators panic with Java's exact messages where the operand
//! is available:
//!
//! - `by_id_or_throw` — `IllegalArgumentException("No value with id " + id)`:
//!   exact (the id is a primitive).
//! - `get_id_or_throw` — `IllegalArgumentException("Can't find id for '" +
//!   value + "' in map " + this)`: the `value.toString()` part is
//!   unreproducible (`T` is unbounded — no `Debug`/`Display` bound, see
//!   `Registry<T>`'s hand-written `Debug`), so the default drops the value part.
//!   `Registry<T>` overrides it to reproduce the `"in map " + this` part via its
//!   `Display` impl.

/// `net.minecraft.core.IdMap<T>`.
pub trait IdMap<T> {
    /// `IdMap.getId(T)` — `-1` when absent.
    fn get_id(&self, thing: &T) -> i32;

    /// `IdMap.byId(int)` — `@Nullable`, a reference to the stored element.
    fn by_id(&self, id: i32) -> Option<&T>;

    /// `IdMap.byIdOrThrow(int)` — panics `"No value with id {id}"` (Java's
    /// `IllegalArgumentException`).
    fn by_id_or_throw(&self, id: i32) -> &T {
        match self.by_id(id) {
            Some(t) => t,
            None => panic!("No value with id {}", id),
        }
    }

    /// `IdMap.getIdOrThrow(T)`.
    ///
    /// Panics when the value is absent. The default cannot reproduce Java's
    /// `"Can't find id for '" + value + "' in map " + this` (the value has no
    /// `Display`), so it drops both variable parts; `Registry<T>` overrides the
    /// message to include the registry.
    fn get_id_or_throw(&self, value: &T) -> i32 {
        let id = self.get_id(value);
        if id == DEFAULT_ID {
            panic!("Can't find id for value");
        }
        id
    }

    /// `IdMap.size()`.
    fn size(&self) -> i32;
}

/// `IdMap.DEFAULT` = `-1`.
pub const DEFAULT_ID: i32 = -1;
