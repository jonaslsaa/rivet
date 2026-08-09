//! Held localhost port reservations.
//!
//! Every harness boots servers that bind a port read from config
//! (`server.properties` for Paper, `--port` for rivet-server). The naive
//! pattern — bind an ephemeral port, read it, drop the listener, later spawn
//! the server — leaves a window in which the OS can hand that port to another
//! process, so two servers in one scenario silently collide.
//!
//! [`PortReservation`] instead *holds* the bound listener for the whole boot
//! prep, so the port cannot be stolen between reservation and the moment the
//! child process is spawned. [`PortReservation::release`] drops the listener
//! immediately before the spawn, narrowing the unavoidable bind-drop-boot race
//! to the spawn->child-bind gap (the child must bind the port itself — the
//! harness cannot pass an fd).

use std::io;
use std::net::TcpListener;

/// A bound loopback listener whose port is reserved until
/// [`PortReservation::release`] or drop.
pub struct PortReservation {
    listener: TcpListener,
}

impl PortReservation {
    /// Bind a fresh ephemeral loopback port and hold it.
    pub fn bind() -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        Ok(Self { listener })
    }

    /// The reserved port number.
    pub fn port(&self) -> u16 {
        self.listener
            .local_addr()
            .expect("bound listener has an address")
            .port()
    }

    /// Drop the held listener so the child process can bind the same port.
    /// Call this as late as possible — immediately before spawning the server.
    pub fn release(self) {
        drop(self.listener);
    }
}

/// Reserve `n` distinct ephemeral loopback ports, held simultaneously so the
/// OS cannot hand out the same port twice.
pub fn reserve(n: usize) -> io::Result<Vec<PortReservation>> {
    let mut reservations = Vec::with_capacity(n);
    for _ in 0..n {
        reservations.push(PortReservation::bind()?);
    }
    Ok(reservations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn reserve_gives_distinct_nonzero_ports() {
        let held = reserve(3).unwrap();
        let ports: Vec<u16> = held.iter().map(PortReservation::port).collect();
        let unique: std::collections::BTreeSet<u16> = ports.iter().copied().collect();
        assert_eq!(unique.len(), 3, "ports must be distinct: {ports:?}");
        assert!(
            ports.iter().all(|p| *p > 0),
            "ephemeral ports must be nonzero: {ports:?}"
        );
    }

    #[test]
    fn held_reservation_blocks_rebind_of_the_same_port() {
        let held = PortReservation::bind().unwrap();
        let port = held.port();
        assert!(
            TcpListener::bind(("127.0.0.1", port)).is_err(),
            "a held reservation must keep the port bound (no SO_REUSEPORT here)"
        );
    }

    #[test]
    fn release_frees_the_port_for_rebind() {
        let held = PortReservation::bind().unwrap();
        let port = held.port();
        held.release();
        // Parallel tests binding ephemeral ports can transiently grab `port`
        // between the release and this rebind (the OS advances its ephemeral
        // cursor, so the collision clears quickly). Retry briefly before
        // concluding the release did not free it.
        let mut rebound = TcpListener::bind(("127.0.0.1", port));
        for _ in 0..50 {
            if rebound.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
            rebound = TcpListener::bind(("127.0.0.1", port));
        }
        assert!(
            rebound.is_ok(),
            "after release the port must be bindable again (the child server's bind target)"
        );
    }

    #[test]
    fn dropping_a_reservation_releases_the_port() {
        let port = PortReservation::bind().unwrap().port();
        // Same parallel-test ephemeral collision window as
        // `release_frees_the_port_for_rebind`; retry briefly before concluding
        // the drop did not release the listener.
        let mut rebound = TcpListener::bind(("127.0.0.1", port));
        for _ in 0..50 {
            if rebound.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
            rebound = TcpListener::bind(("127.0.0.1", port));
        }
        assert!(
            rebound.is_ok(),
            "drop releases the held listener like release()"
        );
    }
}
