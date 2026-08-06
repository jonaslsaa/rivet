use std::fmt;

/// `ConnectionId` — the per-connection registration key. OWNERSHIP §Network:
/// decoded play-state packets cross to the tick thread over bounded channels
/// keyed by `ConnectionId` (sub-issue #93). This slice only assigns and tracks
/// ids through the connection registry; the channel handoff is not built yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(pub u64);

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "conn#{}", self.0)
    }
}
