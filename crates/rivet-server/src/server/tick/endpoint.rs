//! The network side's handle to the tick boundary: owns the lifecycle sender
//! (network→tick registration) and the shared shutdown signal. Shared as an
//! `Arc` between the accept loop and per-connection tasks.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

use super::channels::{InboundDrained, LifecycleEvent, OutboundEvent, ServerboundFrame};
use super::shutdown::Shutdown;
use crate::server::network::connection_id::ConnectionId;
use crate::server::network::packet_listener::DisconnectReason;

#[derive(Clone, Debug)]
pub struct NetworkEndpoint {
    lifecycle_tx: mpsc::Sender<LifecycleEvent>,
    shutdown: Arc<Shutdown>,
}

/// Outcome of handing a new connection to the tick thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterResult {
    /// The tick thread accepted the connection and now owns its channel ends.
    Registered,
    /// The tick thread is gone (lifecycle receiver dropped — shutting down);
    /// the socket must be dropped.
    ServerShuttingDown,
}

impl NetworkEndpoint {
    pub fn new(lifecycle_tx: mpsc::Sender<LifecycleEvent>, shutdown: Arc<Shutdown>) -> Self {
        NetworkEndpoint {
            lifecycle_tx,
            shutdown,
        }
    }

    /// Register a new connection with the tick thread (called by the per-
    /// connection task, which owns the channel pair ends handed over). A full
    /// lifecycle channel is a registration *overload*, not a shutdown: await
    /// capacity (the tick thread drains lifecycle events each tick) rather than
    /// dropping the connection. Only a closed channel — the tick thread exiting
    /// while we wait — means the server is stopping and the socket must be
    /// dropped.
    pub async fn register_connection(
        &self,
        id: ConnectionId,
        remote: SocketAddr,
        in_rx: mpsc::Receiver<ServerboundFrame>,
        out_tx: mpsc::Sender<OutboundEvent>,
        drained: InboundDrained,
    ) -> RegisterResult {
        let event = LifecycleEvent::Connect {
            id,
            remote,
            in_rx,
            out_tx,
            drained,
        };
        match self.lifecycle_tx.try_send(event) {
            Ok(()) => RegisterResult::Registered,
            Err(TrySendError::Closed(_)) => RegisterResult::ServerShuttingDown,
            Err(TrySendError::Full(event)) => match self.lifecycle_tx.send(event).await {
                Ok(()) => RegisterResult::Registered,
                Err(_) => RegisterResult::ServerShuttingDown,
            },
        }
    }

    /// Report a closed connection (per-connection task exit). Best-effort: the
    /// tick registry is already self-healing — dropping the task's inbound
    /// sender closes the channel, and the tick thread prunes the entry on its
    /// next drain. The lifecycle event is a courtesy so the reason (for logging)
    /// reaches the tick side; a full or closed lifecycle channel is never a
    /// problem here.
    pub fn connection_closed(&self, id: ConnectionId, reason: DisconnectReason) {
        let _ = self
            .lifecycle_tx
            .try_send(LifecycleEvent::Disconnect { id, reason });
    }

    pub fn shutdown(&self) {
        self.shutdown.request();
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown.is_requested()
    }
}
