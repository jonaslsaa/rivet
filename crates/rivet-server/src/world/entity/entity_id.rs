//! No Java counterpart of its own: the entity handle (`OWNERSHIP.md`'s
//! `EntityKey`, which does not exist yet) that `EntityHitResult`,
//! `EntityCollisionContext`, the projectile and the AI-goal code hold in
//! place of a Java `Entity` reference.

/// Unique runtime entity identifier (single-owner arena key per `OWNERSHIP.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(pub u64);
