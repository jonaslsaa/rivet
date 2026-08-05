//! `net.minecraft.util.Util` port surface — the helpers `net.minecraft.nbt`
//! needs.
//!
//! STUB(mc.nbt.io) — minimal faithful surface for the NBT write path.

/// `Util.logAndPauseIfInIde(message, throwable)`.
///
/// Java logs at ERROR (via LOGGER) and, when running in an IDE, pauses. In the
/// port this is a no-op that swallows the message — there is no logging
/// framework and no IDE pause yet. The important behavior for NbtIo is that the
/// method RETURNS (it does not throw) after a failed string write, which the
/// `StringFallbackDataOutput` relies on.
pub fn log_and_pause_if_in_ide(_message: &str) {}
