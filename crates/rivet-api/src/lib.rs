//! `rivet-api` — the native Rust Paper API surface (M4, epic #28).
//!
//! Mirrors `io.papermc.paper` (and, in time, `org.bukkit`): the paper-api
//! shape as an idiomatic Rust surface over the same internals the JVM adapter
//! uses. Source of truth is the Java under
//! `working/Paper/paper-api/.../io/papermc/paper/` (plus `paper-server` for
//! Paper's server-side API implementations). Module paths mirror Java
//! packages with the `io.papermc.paper` prefix folded into the crate root:
//! `io.papermc.paper.plugin.loader.library` → `plugin::loader::library`.

pub mod plugin;
