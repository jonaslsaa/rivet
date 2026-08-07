//! Port of `net.minecraft.world.entity.PositionMoveRotation` (#87) — the
//! `(Vec3 position, Vec3 deltaMovement, float yRot, float xRot)` record the
//! player-position packet body carries.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/entity/PositionMoveRotation.java`. `STREAM_CODEC` is two `Vec3`
//! (three big-endian doubles each) then two floats. The wire-only `Vec3` value
//! slice lives in `rivet-registry::core` (see `core/vec3.rs`); the `of(Entity)`/
//! `of(TeleportTransition)`/`withRotation`/`calculateAbsolute` factories need
//! the entity/JOML math and are deferred with the entity unit (M3).

use crate::codec::{StreamCodec, composite_4, float, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use rivet_registry::core::Vec3;

/// `PositionMoveRotation` — the record `(position, deltaMovement, yRot, xRot)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionMoveRotation {
    /// `position`.
    position: Vec3,
    /// `deltaMovement`.
    delta_movement: Vec3,
    /// `yRot`.
    y_rot: f32,
    /// `xRot`.
    x_rot: f32,
}

impl PositionMoveRotation {
    /// The record's canonical constructor.
    pub fn new(position: Vec3, delta_movement: Vec3, y_rot: f32, x_rot: f32) -> Self {
        PositionMoveRotation {
            position,
            delta_movement,
            y_rot,
            x_rot,
        }
    }

    /// `PositionMoveRotation.position()`.
    pub fn position(&self) -> Vec3 {
        self.position
    }

    /// `PositionMoveRotation.deltaMovement()`.
    pub fn delta_movement(&self) -> Vec3 {
        self.delta_movement
    }

    /// `PositionMoveRotation.yRot()`.
    pub fn y_rot(&self) -> f32 {
        self.y_rot
    }

    /// `PositionMoveRotation.xRot()`.
    pub fn x_rot(&self) -> f32 {
        self.x_rot
    }

    /// `PositionMoveRotation.STREAM_CODEC` — `Vec3.STREAM_CODEC`, `Vec3.STREAM_CODEC`,
    /// `FLOAT`, `FLOAT`, in that order.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, PositionMoveRotation> {
        composite_4(
            vec3_stream_codec(),
            PositionMoveRotation::position,
            vec3_stream_codec(),
            PositionMoveRotation::delta_movement,
            float(),
            PositionMoveRotation::y_rot,
            float(),
            PositionMoveRotation::x_rot,
            PositionMoveRotation::new,
        )
    }
}

/// `Vec3.STREAM_CODEC` — three big-endian doubles.
fn vec3_stream_codec() -> StreamCodec<FriendlyByteBuf, Vec3> {
    of(
        |output: &mut FriendlyByteBuf, value: &Vec3| {
            output.write_double(value.x);
            output.write_double(value.y);
            output.write_double(value.z);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| {
            Ok(Vec3::new(
                input.read_double(),
                input.read_double(),
                input.read_double(),
            ))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn stream_codec_round_trips() {
        let value = PositionMoveRotation::new(
            Vec3::new(1.5, -63.0, 2.25),
            Vec3::new(0.0, 0.0, 0.0),
            10.0,
            -20.0,
        );
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        PositionMoveRotation::stream_codec()
            .encode(&mut out, &value)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes.len(), 3 * 8 + 3 * 8 + 4 + 4);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(
            PositionMoveRotation::stream_codec()
                .decode(&mut input)
                .unwrap(),
            value
        );
        assert_eq!(input.readable_bytes(), 0);
    }
}
