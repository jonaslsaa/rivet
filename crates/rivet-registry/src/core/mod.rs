//! Registry-independent position/value primitives of `net.minecraft.core`
//! (issue #125, sub-issue of epic #10).
//!
//! These are **pure value types** per OWNERSHIP.md §Chunks & blocks: resolved by
//! ID, no references into registries. Java source of truth:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/core/*`.
//!
//! Module placement mirrors the Java package (`net.minecraft.core` →
//! `rivet_registry::core`). One deliberate move: Java puts `ChunkPos` in
//! `world.level`; it lives here as a pure value type, and
//! `ChunkPyramid.MAX_CHUNK_COORDINATE_VALUE` moves to a `const` here so
//! `ChunkPos::is_valid` stays value-only (OWNERSHIP.md — the module mirror is a
//! convenience and cycle-breaking justifies the one-line move).
//!
//! `game_profile` (issue #198) ports the authlib profile value types —
//! `GameProfile`/`Property`/`PropertyMap` (guava-style ordered multimap;
//! `values()` is key-grouped, and `PropertyMap::new` re-groups like
//! `ImmutableMultimap.copyOf`). They are not `net.minecraft.*` classes (they
//! come from the authlib jar), but they are pure value types of the same class
//! as the others here, and `rivet-protocol` needs them below itself; their
//! `StreamCodec` impls live in `rivet-protocol`.
//! `uuid_util` (issue #198) ports `UUIDUtil.createOfflinePlayerUUID`
//! (`UUID.nameUUIDFromBytes("OfflinePlayer:" + name)`, v3/MD5); the
//! `STREAM_CODEC` half lives in `rivet-protocol` (`FriendlyByteBuf::read_uuid`/
//! `write_uuid`). The codec slice (#373) adds `uuid_codec` (`Codec.INT_STREAM`
//! `comapFlatMap` `Util.fixedSize(…, 4)`) and the exact
//! `uuidFromIntArray`/`uuidToIntArray` conversions. `game_type` (#373) adds
//! `game_type_codec` (`StringRepresentable.fromEnum`), `game_type_legacy_id_codec`
//! (`Codec.INT.xmap`), and the `byName` resolvers.
//!
//! RivetTodo(#212): `BlockBox`, `Direction8`, `Rotations` are ported as pure
//! value types (with `Rotations::CODEC` here). The remaining #212 gaps are the
//! `Position` trait's only Java implementors — JOML `Vec3`/`Vector3d`,
//! deferred with #206 — so no in-crate type implements it and the
//! `Position`-taking overloads are omitted, and the
//! `Entity`/`ChunkAccess`/`LevelHeightAccessor`-parametered overloads plus the
//! JOML returns (`BlockBox::aabb` -> `AABB` is #206; `Vec3`, `Vector3f`,
//! `Quaternionf`, `Matrix4fc`, `OctahedralGroup` with their owning units)
//! stay deferred. RivetTodo(#126): the `StreamCodec` impls
//! (`BlockBox.STREAM_CODEC`, `Rotations.STREAM_CODEC`,
//! `FriendlyByteBuf::read/writeBlockPos`/`read/writeChunkPos`) live in
//! `rivet-protocol`.

mod axis_cycle;
mod block_box;
mod block_pos;
mod chunk_pos;
mod cursor3d;
mod difficulty;
mod direction;
mod direction8;
mod game_profile;
mod game_type;
mod global_pos;
mod position;
mod relative;
mod rotation;
mod rotations;
mod section_pos;
mod uuid_util;
mod vec3;
mod vec3i;

pub use axis_cycle::AxisCycle;
pub use block_box::BlockBox;
pub use block_pos::{BlockPos, MutableBlockPos, TraversalNodeStatus};
pub use chunk_pos::ChunkPos;
pub use cursor3d::Cursor3D;
pub use difficulty::Difficulty;
pub use direction::{Axis, AxisDirection, Direction, Plane};
pub use direction8::Direction8;
pub use game_profile::{GameProfile, Property, PropertyMap};
pub use game_type::{GameType, game_type_codec, game_type_legacy_id_codec};
pub use global_pos::GlobalPos;
pub use position::Position;
pub use relative::Relative;
pub use rotation::Rotation;
pub use rotations::{Rotations, rotations_codec};
pub use section_pos::SectionPos;
pub use uuid_util::{
    create_offline_player_uuid, uuid_codec, uuid_from_int_array, uuid_to_int_array,
};
pub use vec3::Vec3;
pub use vec3i::{Vec3i, Vec3iLike};

/// `ChunkPyramid.MAX_CHUNK_COORDINATE_VALUE` moved to a `const` here
/// (OWNERSHIP.md; issue #125). In Java this is computed as
/// `SectionPos.blockToSectionCoord(BlockPos.MAX_HORIZONTAL_COORDINATE) -
/// SAFETY_MARGIN_CHUNKS` where
/// `SAFETY_MARGIN_CHUNKS = (32 + GENERATION_PYRAMID.getStepTo(FULL).accumulatedDependencies().size() + 1) * 2`.
/// The full-size generation pyramid (`GENERATION_PYRAMID`) accumulates 12
/// dependencies at FULL. The value `12` is taken from a manual replay of
/// `ChunkStep.Builder.buildAccumulatedDependencies()` over the pinned
/// `working/Paper` `GENERATION_PYRAMID` (see the replay notes in
/// `tests/position.rs::chunk_pos_valid_bound`); it is not independently
/// re-derived by the test — if the true dependency count ever differed, the
/// bounds asserted there would silently change. So
/// `2097151 - (32 + 12 + 1) * 2 = 2097061`.
pub const MAX_CHUNK_COORDINATE_VALUE: i32 = 2097061;
