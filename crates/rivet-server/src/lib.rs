//! `rivet-server` — the server binary/library. Mirrors `net.minecraft.server`
//! plus Paper's server layer.
//!
//! This is the M1 skeleton slice (issue #145): a tokio TCP listener + accept
//! loop + per-connection tasks that decode VarInt21 frames via `rivet-protocol`
//! and drive the handshake → status/login state-transition boundary. All
//! handshake/status/login handling runs on the tokio side per OWNERSHIP §Network;
//! this crate owns no game state (the sync tick loop + play-state packet handoff
//! are sub-issue #93). The connection registry is the only shared mutable
//! structure — the OWNERSHIP "connection registry" exception.

pub mod server;
