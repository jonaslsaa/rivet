//! `net.minecraft.world.phys` — the physics value types (`Vec3`, `Vec2`,
//! `AABB`, `HitResult` and `BlockHitResult`). The shape classes of
//! `net.minecraft.world.phys.shapes` live in `rivet-world`; `EntityHitResult`
//! holds an entity, so it lives in `rivet-server`.

pub mod aabb;
pub mod block_hit_result;
pub mod hit_result;
pub mod vec2;
pub mod vec3;

pub use aabb::AABB;
pub use block_hit_result::BlockHitResult;
pub use hit_result::{HitResult, HitResultBehavior};
pub use vec2::Vec2;
pub use vec3::Vec3;
