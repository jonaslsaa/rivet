//! Paper `net.minecraft.world.inventory.RemoteSlot`.

use super::item_stack::{HashedStack, InventoryItemStack};

/// Paper `net.minecraft.world.inventory.RemoteSlot`.
#[derive(Debug, Clone)]
pub enum RemoteSlot {
    /// `RemoteSlot.PLACEHOLDER` — always reports matching.
    Placeholder,
    /// `RemoteSlot.Synchronized` — compares local stacks against either the
    /// last forced full stack or a client-reported [`HashedStack`].
    Synchronized {
        remote_stack: Option<InventoryItemStack>,
        remote_hash: Option<HashedStack>,
    },
}

impl RemoteSlot {
    /// Create a synchronized remote slot with no known remote state.
    pub fn synchronized() -> Self {
        Self::Synchronized {
            remote_stack: None,
            remote_hash: None,
        }
    }

    /// `RemoteSlot.force(ItemStack)`.
    pub fn force(&mut self, outgoing: &InventoryItemStack) {
        if let Self::Synchronized {
            remote_stack,
            remote_hash,
        } = self
        {
            *remote_stack = Some(outgoing.clone());
            *remote_hash = None;
        }
    }

    /// `RemoteSlot.receive(HashedStack)`.
    pub fn receive(&mut self, incoming: HashedStack) {
        if let Self::Synchronized {
            remote_stack,
            remote_hash,
        } = self
        {
            *remote_stack = None;
            *remote_hash = Some(incoming);
        }
    }

    /// `RemoteSlot.matches(ItemStack)`.
    pub fn matches(&mut self, local: &InventoryItemStack) -> bool {
        match self {
            Self::Placeholder => true,
            Self::Synchronized {
                remote_stack,
                remote_hash,
            } => {
                if let Some(remote) = remote_stack {
                    InventoryItemStack::matches(remote, local)
                } else if let Some(hash) = remote_hash {
                    if hash.matches(local) {
                        *remote_stack = Some(local.clone());
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
        }
    }

    /// `RemoteSlot.Synchronized.copyFrom(RemoteSlot.Synchronized)`.
    pub fn copy_from(&mut self, other: &Self) {
        if let (
            Self::Synchronized {
                remote_stack: dst_stack,
                remote_hash: dst_hash,
            },
            Self::Synchronized {
                remote_stack: src_stack,
                remote_hash: src_hash,
            },
        ) = (self, other)
        {
            *dst_stack = src_stack.clone();
            *dst_hash = src_hash.clone();
        }
    }
}
