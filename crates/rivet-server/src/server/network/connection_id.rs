use std::fmt;

/// `ConnectionId` — the per-connection registration key. OWNERSHIP §Network:
/// decoded play-state packets cross to the tick thread over bounded channels
/// keyed by `ConnectionId`. The accept loop assigns ids; the tick thread's
/// [`ConnectionRegistry`](crate::server::tick::registry::ConnectionRegistry)
/// keys its per-connection channel ends by them (sub-issue #93).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(pub u64);

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "conn#{}", self.0)
    }
}
