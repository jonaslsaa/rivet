//! Port of `net.minecraft.world.level.levelgen.Heightmap.Types` (MC 26.2) — the
//! wire-visible slice of the world-level `Heightmap` enum.
//!
//! Java: `Heightmap.java` in `working/Paper`. `Heightmap.Types` is a world-level
//! enum (not a registry): its `STREAM_CODEC` is `ByteBufCodecs.idMapper(BY_ID,
//! t -> t.id)` — a plain varint of the enum's `id` field. The chunk-send packet
//! bodies (`ClientboundLevelChunkPacketData.HEIGHTMAPS_STREAM_CODEC`) key their
//! heightmap map on `Heightmap.Types`, so the wire needs the enum's id values and
//! names.
//!
//! OWNERSHIP.md: `Heightmap` itself belongs to `rivet-world` (it computes the
//! `long[]` from a chunk); `rivet-protocol` cannot depend on `rivet-world` (the
//! world crate already depends on protocol). The minimal `Types` value lives here
//! and `rivet-world` consumes it (world → protocol exists). The send path takes
//! plain values — this enum plus the raw `Vec<i64>` — so no
//! `&LevelChunk`/`&Heightmap` back-reference crosses the crate boundary.
//!
//! The three `Usage.CLIENT` types are what `sendToClient()` filters to on the
//! server (`LevelChunkPacketData` construction); the other three
//! (`WORLD_SURFACE_WG`, `OCEAN_FLOOR_WG`, `OCEAN_FLOOR`) are never sent but
//! keep their real ids here so a hostile wire id round-trips through the full
//! enum like Java's `ByIdMap.continuous(..., OutOfBoundsStrategy.ZERO)`
//! (an out-of-range id falls back to id 0, `WORLD_SURFACE_WG`).

use crate::codec::StreamCodec;
use crate::codec::byte_buf_codecs;
use crate::friendly_byte_buf::FriendlyByteBuf;

/// `Heightmap.Types` — the six world heightmap types, in Java enum order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeightmapType {
    /// `WORLD_SURFACE_WG` (id 0, `Usage.WORLDGEN`) — worldgen-only.
    WorldSurfaceWg,
    /// `WORLD_SURFACE` (id 1, `Usage.CLIENT`) — the highest non-air block.
    WorldSurface,
    /// `OCEAN_FLOOR_WG` (id 2, `Usage.WORLDGEN`).
    OceanFloorWg,
    /// `OCEAN_FLOOR` (id 3, `Usage.LIVE_WORLD`).
    OceanFloor,
    /// `MOTION_BLOCKING` (id 4, `Usage.CLIENT`) — blocks motion or a fluid.
    MotionBlocking,
    /// `MOTION_BLOCKING_NO_LEAVES` (id 5, `Usage.CLIENT`) — `MOTION_BLOCKING`
    /// minus leaves.
    MotionBlockingNoLeaves,
}

impl HeightmapType {
    /// The enum `id` (the wire form), `Heightmap.Types` `id` field.
    pub const fn id(self) -> i32 {
        match self {
            HeightmapType::WorldSurfaceWg => 0,
            HeightmapType::WorldSurface => 1,
            HeightmapType::OceanFloorWg => 2,
            HeightmapType::OceanFloor => 3,
            HeightmapType::MotionBlocking => 4,
            HeightmapType::MotionBlockingNoLeaves => 5,
        }
    }

    /// `Heightmap.Types.getSerializationKey()` — the canonical name.
    pub const fn serialization_key(self) -> &'static str {
        match self {
            HeightmapType::WorldSurfaceWg => "WORLD_SURFACE_WG",
            HeightmapType::WorldSurface => "WORLD_SURFACE",
            HeightmapType::OceanFloorWg => "OCEAN_FLOOR_WG",
            HeightmapType::OceanFloor => "OCEAN_FLOOR",
            HeightmapType::MotionBlocking => "MOTION_BLOCKING",
            HeightmapType::MotionBlockingNoLeaves => "MOTION_BLOCKING_NO_LEAVES",
        }
    }

    /// `sendToClient()` — `usage == CLIENT`. The three types the server
    /// actually transmits in a `level_chunk_with_light` body.
    pub const fn send_to_client(self) -> bool {
        matches!(
            self,
            HeightmapType::WorldSurface
                | HeightmapType::MotionBlocking
                | HeightmapType::MotionBlockingNoLeaves
        )
    }

    /// `Heightmap.Types.BY_ID` (built `ByIdMap.continuous(t -> t.id, values(),
    /// OutOfBoundsStrategy.ZERO)`): a varint id -> type, out-of-range -> id 0.
    pub fn by_id(id: i32) -> HeightmapType {
        match id {
            1 => HeightmapType::WorldSurface,
            2 => HeightmapType::OceanFloorWg,
            3 => HeightmapType::OceanFloor,
            4 => HeightmapType::MotionBlocking,
            5 => HeightmapType::MotionBlockingNoLeaves,
            // `OutOfBoundsStrategy.ZERO` — 0 and any other id fall back to id 0.
            _ => HeightmapType::WorldSurfaceWg,
        }
    }

    /// `Heightmap.Types.STREAM_CODEC` — `ByteBufCodecs.idMapper(BY_ID, t -> t.id)`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, HeightmapType> {
        byte_buf_codecs::id_mapper(Self::by_id, |t| t.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    fn buf() -> FriendlyByteBuf {
        FriendlyByteBuf::new(BytesMut::new())
    }

    #[test]
    fn ids_and_names_match_java() {
        let cases = [
            (HeightmapType::WorldSurfaceWg, 0, "WORLD_SURFACE_WG", false),
            (HeightmapType::WorldSurface, 1, "WORLD_SURFACE", true),
            (HeightmapType::OceanFloorWg, 2, "OCEAN_FLOOR_WG", false),
            (HeightmapType::OceanFloor, 3, "OCEAN_FLOOR", false),
            (HeightmapType::MotionBlocking, 4, "MOTION_BLOCKING", true),
            (
                HeightmapType::MotionBlockingNoLeaves,
                5,
                "MOTION_BLOCKING_NO_LEAVES",
                true,
            ),
        ];
        for (ty, id, name, client) in cases {
            assert_eq!(ty.id(), id, "{name}");
            assert_eq!(ty.serialization_key(), name);
            assert_eq!(ty.send_to_client(), client, "{name}");
            assert_eq!(HeightmapType::by_id(id), ty, "{name}");
        }
    }

    #[test]
    fn stream_codec_wire_is_varint_id() {
        let mut out = buf();
        HeightmapType::stream_codec()
            .encode(&mut out, &HeightmapType::MotionBlockingNoLeaves)
            .unwrap();
        assert_eq!(out.into_inner().to_vec(), vec![5]);
    }

    #[test]
    fn by_id_out_of_range_falls_back_to_world_surface_wg() {
        // `ByIdMap.continuous(..., OutOfBoundsStrategy.ZERO)`.
        assert_eq!(HeightmapType::by_id(-1), HeightmapType::WorldSurfaceWg);
        assert_eq!(HeightmapType::by_id(99), HeightmapType::WorldSurfaceWg);
    }

    #[test]
    fn round_trips() {
        for ty in [
            HeightmapType::WorldSurfaceWg,
            HeightmapType::WorldSurface,
            HeightmapType::OceanFloorWg,
            HeightmapType::OceanFloor,
            HeightmapType::MotionBlocking,
            HeightmapType::MotionBlockingNoLeaves,
        ] {
            let mut out = buf();
            HeightmapType::stream_codec().encode(&mut out, &ty).unwrap();
            let mut input = FriendlyByteBuf::new(out.into_inner());
            assert_eq!(
                HeightmapType::stream_codec().decode(&mut input).unwrap(),
                ty
            );
        }
    }
}
