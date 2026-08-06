//! `rivet-server` — the server binary/library. Mirrors `net.minecraft.server`
//! plus Paper's server layer.
//!
//! Two halves meet here (OWNERSHIP):
//!   - the tokio side: a TCP listener + accept loop + per-connection tasks that
//!     decode VarInt21 frames via `rivet-protocol` and drive the handshake →
//!     status/login state-transition boundary (issues #145/#96);
//!   - the sync tick thread: a deterministic 20 TPS loop with Paper's monotonic
//!     schedule + catch-up cap, owning the tick-side connection registry (issue
//!     #93).
//!
//! Handshake/status/login run on the tokio side per OWNERSHIP §Network;
//! play-state packets cross to the tick thread over bounded channels keyed by
//! `ConnectionId`. This crate owns no game state beyond the tick-side connection
//! registry — the OWNERSHIP "connection registry" exception. Play-state packet
//! bodies are epic #10; login completion that routes frames to the tick thread
//! is #96.

pub mod server;
