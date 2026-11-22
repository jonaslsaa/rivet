//! Host-owned generational handles for the native plugin boundary (#506).
//!
//! No arena pointer, retained caller buffer, or Rust reference crosses the
//! plugin boundary. Plugins hold only opaque copyable [`Handle<T>`] values
//! (`index` + `generation`, packable into a `u64`). The host validates index,
//! generation, and owning [`PluginId`] on every call and revokes all handles
//! owned by a plugin on disable, failure, or unload.

use std::fmt;
use std::marker::PhantomData;

/// Opaque, monotonic identifier assigned to a registered native plugin.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct PluginId(u32);

impl PluginId {
    pub(crate) const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Construct a `PluginId` from a host-assigned raw identifier.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Raw numeric identifier suitable for logging or FFI-free diagnostics.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "plugin#{}", self.0)
    }
}

/// Generational handle to a host-owned value of marker type `T`.
///
/// Generation `0` is reserved as a sentinel invalid generation so
/// `Handle::from_raw_u64(0)` is always rejected by [`HandleTable`].
pub struct Handle<T> {
    index: u32,
    generation: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Copy for Handle<T> {}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Handle<T> {}

impl<T> std::hash::Hash for Handle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

impl<T> PartialOrd for Handle<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Handle<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.index, self.generation).cmp(&(other.index, other.generation))
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle({}:g{})", self.index, self.generation)
    }
}

impl<T> Handle<T> {
    pub(crate) const fn new(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            _marker: PhantomData,
        }
    }

    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }

    /// Packs `(generation << 32) | index` into an opaque `u64`.
    #[must_use]
    pub const fn to_raw_u64(self) -> u64 {
        ((self.generation as u64) << 32) | (self.index as u64)
    }

    /// Unpacks a raw `u64` previously produced by [`Self::to_raw_u64`].
    #[must_use]
    pub const fn from_raw_u64(raw: u64) -> Self {
        Self {
            index: raw as u32,
            generation: (raw >> 32) as u32,
            _marker: PhantomData,
        }
    }
}

/// Errors produced when resolving or mutating a generational handle.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum HandleError {
    /// Handle index is out of bounds or uses the reserved generation `0`.
    Invalid { index: u32, generation: u32 },
    /// Handle refers to a slot that was freed, revoked, or reused at a newer generation.
    Stale {
        index: u32,
        provided_generation: u32,
        current_generation: u32,
    },
    /// Handle is live, but belongs to a different plugin than the caller.
    UnauthorizedOwner {
        index: u32,
        owner: PluginId,
        caller: PluginId,
    },
    /// Table reached the maximum number of addressable slots (`u32::MAX`).
    TableExhausted,
}

impl fmt::Display for HandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { index, generation } => {
                write!(f, "invalid handle ({index}:g{generation})")
            }
            Self::Stale {
                index,
                provided_generation,
                current_generation,
            } => write!(
                f,
                "stale handle at slot {index}: provided generation {provided_generation}, current generation {current_generation}"
            ),
            Self::UnauthorizedOwner {
                index,
                owner,
                caller,
            } => write!(
                f,
                "handle at slot {index} is owned by {owner}, not {caller}"
            ),
            Self::TableExhausted => write!(f, "handle table exhausted"),
        }
    }
}

impl std::error::Error for HandleError {}

struct Slot<V> {
    generation: u32,
    owner: Option<PluginId>,
    value: Option<V>,
}

/// Host-owned generational table mapping [`Handle<M>`] to `V` with per-plugin ownership.
pub struct HandleTable<M, V> {
    slots: Vec<Slot<V>>,
    free_list: Vec<u32>,
    live: usize,
    _marker: PhantomData<fn() -> M>,
}

impl<M, V> Default for HandleTable<M, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M, V> HandleTable<M, V> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_list: Vec::new(),
            live: 0,
            _marker: PhantomData,
        }
    }

    #[must_use]
    pub const fn live_count(&self) -> usize {
        self.live
    }

    #[must_use]
    pub fn live_count_for(&self, owner: PluginId) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.value.is_some() && slot.owner == Some(owner))
            .count()
    }

    pub fn insert(&mut self, owner: PluginId, value: V) -> Result<Handle<M>, HandleError> {
        if let Some(index) = self.free_list.pop() {
            let slot = &mut self.slots[index as usize];
            debug_assert!(slot.value.is_none());
            slot.owner = Some(owner);
            slot.value = Some(value);
            self.live += 1;
            return Ok(Handle::new(index, slot.generation));
        }

        let index = u32::try_from(self.slots.len()).map_err(|_| HandleError::TableExhausted)?;
        let generation = 1;
        self.slots.push(Slot {
            generation,
            owner: Some(owner),
            value: Some(value),
        });
        self.live += 1;
        Ok(Handle::new(index, generation))
    }

    fn resolve_slot_index(&self, handle: Handle<M>) -> Result<usize, HandleError> {
        if handle.generation == 0 {
            return Err(HandleError::Invalid {
                index: handle.index,
                generation: handle.generation,
            });
        }
        let idx = handle.index as usize;
        let slot = self.slots.get(idx).ok_or(HandleError::Invalid {
            index: handle.index,
            generation: handle.generation,
        })?;
        if slot.generation != handle.generation || slot.value.is_none() {
            return Err(HandleError::Stale {
                index: handle.index,
                provided_generation: handle.generation,
                current_generation: slot.generation,
            });
        }
        Ok(idx)
    }

    pub fn owner_of(&self, handle: Handle<M>) -> Result<PluginId, HandleError> {
        let idx = self.resolve_slot_index(handle)?;
        Ok(self.slots[idx].owner.expect("live slot always has owner"))
    }

    pub fn get(&self, handle: Handle<M>, caller: Option<PluginId>) -> Result<&V, HandleError> {
        let idx = self.resolve_slot_index(handle)?;
        let slot = &self.slots[idx];
        if let Some(caller_id) = caller {
            let owner = slot.owner.expect("live slot always has owner");
            if owner != caller_id {
                return Err(HandleError::UnauthorizedOwner {
                    index: handle.index,
                    owner,
                    caller: caller_id,
                });
            }
        }
        Ok(slot.value.as_ref().expect("live slot has value"))
    }

    pub fn get_mut(
        &mut self,
        handle: Handle<M>,
        caller: Option<PluginId>,
    ) -> Result<&mut V, HandleError> {
        let idx = self.resolve_slot_index(handle)?;
        let slot = &mut self.slots[idx];
        if let Some(caller_id) = caller {
            let owner = slot.owner.expect("live slot always has owner");
            if owner != caller_id {
                return Err(HandleError::UnauthorizedOwner {
                    index: handle.index,
                    owner,
                    caller: caller_id,
                });
            }
        }
        Ok(slot.value.as_mut().expect("live slot has value"))
    }

    pub fn remove(
        &mut self,
        handle: Handle<M>,
        caller: Option<PluginId>,
    ) -> Result<V, HandleError> {
        let idx = self.resolve_slot_index(handle)?;
        let slot = &mut self.slots[idx];
        if let Some(caller_id) = caller {
            let owner = slot.owner.expect("live slot always has owner");
            if owner != caller_id {
                return Err(HandleError::UnauthorizedOwner {
                    index: handle.index,
                    owner,
                    caller: caller_id,
                });
            }
        }
        let value = slot.value.take().expect("live slot has value");
        slot.owner = None;
        slot.generation = next_generation(slot.generation);
        self.free_list.push(handle.index);
        self.live -= 1;
        Ok(value)
    }

    /// Revokes every live slot owned by `owner`, bumping each slot's generation
    /// so all handles previously handed to that plugin immediately become stale.
    pub fn revoke_owned_by(&mut self, owner: PluginId) -> Vec<V> {
        let mut revoked = Vec::new();
        for (idx, slot) in self.slots.iter_mut().enumerate() {
            if slot.owner == Some(owner)
                && let Some(val) = slot.value.take()
            {
                slot.owner = None;
                slot.generation = next_generation(slot.generation);
                self.free_list.push(idx as u32);
                self.live -= 1;
                revoked.push(val);
            }
        }
        revoked
    }
}

const fn next_generation(current: u32) -> u32 {
    let next = current.wrapping_add(1);
    if next == 0 { 1 } else { next }
}
