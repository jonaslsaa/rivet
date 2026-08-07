//! Port of `net.minecraft.network.protocol.game.ServerboundMovePlayerPacket`
//! (MC 26.2).
//!
//! Java: `working/Paper/paper-server/src/minecraft/java/net/minecraft/network/
//! protocol/game/ServerboundMovePlayerPacket.java`. The abstract base plus its
//! four concrete subclasses (`Pos`, `PosRot`, `Rot`, `StatusOnly`) map onto a
//! single Rust enum — one variant per subclass, one network id per variant. The
//! four `Packet.codec(writer, reader)` `STREAM_CODEC`s become the four codec
//! constructors below.
//!
//! Shared flag byte: bit 0 = `onGround`, bit 1 = `horizontalCollision`
//! (`FLAG_ON_GROUND`/`FLAG_HORIZONTAL_COLLISION`). Wire layouts (protocol 776):
//!   Pos        3 doubles + flags byte
//!   PosRot     3 doubles + 2 floats + flags byte
//!   Rot        2 floats + flags byte
//!   StatusOnly 1 flags byte
//!
//! `handle` is a documented STUB (listener dispatch lands with the
//! `ServerGamePacketListener` hierarchy); the fallback getters (`get_x(double
//! fallback)` etc.) are ported for the handler layer (#158), which consumes
//! them with its own fallbacks.

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;

/// `FLAG_ON_GROUND` (bit 0).
const FLAG_ON_GROUND: i32 = 1;
/// `FLAG_HORIZONTAL_COLLISION` (bit 1).
const FLAG_HORIZONTAL_COLLISION: i32 = 2;

/// `packFlags(boolean onGround, boolean horizontalCollision)`.
fn pack_flags(on_ground: bool, horizontal_collision: bool) -> i32 {
    let mut flags = 0;
    if on_ground {
        flags |= FLAG_ON_GROUND;
    }
    if horizontal_collision {
        flags |= FLAG_HORIZONTAL_COLLISION;
    }
    flags
}

/// `unpackOnGround(int flags)`.
fn unpack_on_ground(flags: i32) -> bool {
    flags & FLAG_ON_GROUND != 0
}

/// `unpackHorizontalCollision(int flags)`.
fn unpack_horizontal_collision(flags: i32) -> bool {
    flags & FLAG_HORIZONTAL_COLLISION != 0
}

/// `ServerboundMovePlayerPacket` — the abstract base erased to one enum.
///
/// Each variant corresponds to one Java subclass; the stored fields are that
/// subclass's constructor arguments. `hasPos`/`hasRot` are implicit in the
/// variant (see [`Self::has_position`]/[`Self::has_rotation`]); the Java fields
/// that a subclass zeroes (`Pos`'s rotations, `Rot`/`StatusOnly`'s coords) are
/// simply absent from the variant, and the fallback getters return the caller's
/// fallback for them, exactly as Java does.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerboundMovePlayerPacket {
    /// `ServerboundMovePlayerPacket.Pos` — `hasPos=true, hasRot=false`.
    Pos {
        x: f64,
        y: f64,
        z: f64,
        on_ground: bool,
        horizontal_collision: bool,
    },
    /// `ServerboundMovePlayerPacket.PosRot` — `hasPos=true, hasRot=true`.
    PosRot {
        x: f64,
        y: f64,
        z: f64,
        y_rot: f32,
        x_rot: f32,
        on_ground: bool,
        horizontal_collision: bool,
    },
    /// `ServerboundMovePlayerPacket.Rot` — `hasPos=false, hasRot=true`.
    Rot {
        y_rot: f32,
        x_rot: f32,
        on_ground: bool,
        horizontal_collision: bool,
    },
    /// `ServerboundMovePlayerPacket.StatusOnly` — `hasPos=false, hasRot=false`.
    StatusOnly {
        on_ground: bool,
        horizontal_collision: bool,
    },
}

impl ServerboundMovePlayerPacket {
    /// `hasPosition()`.
    pub fn has_position(&self) -> bool {
        matches!(
            self,
            ServerboundMovePlayerPacket::Pos { .. } | ServerboundMovePlayerPacket::PosRot { .. }
        )
    }

    /// `hasRotation()`.
    pub fn has_rotation(&self) -> bool {
        matches!(
            self,
            ServerboundMovePlayerPacket::PosRot { .. } | ServerboundMovePlayerPacket::Rot { .. }
        )
    }

    /// `isOnGround()`.
    pub fn is_on_ground(&self) -> bool {
        match self {
            ServerboundMovePlayerPacket::Pos { on_ground, .. }
            | ServerboundMovePlayerPacket::PosRot { on_ground, .. }
            | ServerboundMovePlayerPacket::Rot { on_ground, .. }
            | ServerboundMovePlayerPacket::StatusOnly { on_ground, .. } => *on_ground,
        }
    }

    /// `horizontalCollision()`.
    pub fn horizontal_collision(&self) -> bool {
        match self {
            ServerboundMovePlayerPacket::Pos {
                horizontal_collision,
                ..
            }
            | ServerboundMovePlayerPacket::PosRot {
                horizontal_collision,
                ..
            }
            | ServerboundMovePlayerPacket::Rot {
                horizontal_collision,
                ..
            }
            | ServerboundMovePlayerPacket::StatusOnly {
                horizontal_collision,
                ..
            } => *horizontal_collision,
        }
    }

    /// `getX(double fallback)` — the stored x when `hasPos`, else `fallback`.
    pub fn get_x(&self, fallback: f64) -> f64 {
        match self {
            ServerboundMovePlayerPacket::Pos { x, .. }
            | ServerboundMovePlayerPacket::PosRot { x, .. } => *x,
            _ => fallback,
        }
    }

    /// `getY(double fallback)`.
    pub fn get_y(&self, fallback: f64) -> f64 {
        match self {
            ServerboundMovePlayerPacket::Pos { y, .. }
            | ServerboundMovePlayerPacket::PosRot { y, .. } => *y,
            _ => fallback,
        }
    }

    /// `getZ(double fallback)`.
    pub fn get_z(&self, fallback: f64) -> f64 {
        match self {
            ServerboundMovePlayerPacket::Pos { z, .. }
            | ServerboundMovePlayerPacket::PosRot { z, .. } => *z,
            _ => fallback,
        }
    }

    /// `getYRot(float fallback)` — the stored yaw when `hasRot`, else `fallback`.
    pub fn get_y_rot(&self, fallback: f32) -> f32 {
        match self {
            ServerboundMovePlayerPacket::PosRot { y_rot, .. }
            | ServerboundMovePlayerPacket::Rot { y_rot, .. } => *y_rot,
            _ => fallback,
        }
    }

    /// `getXRot(float fallback)` — the stored pitch when `hasRot`, else `fallback`.
    pub fn get_x_rot(&self, fallback: f32) -> f32 {
        match self {
            ServerboundMovePlayerPacket::PosRot { x_rot, .. }
            | ServerboundMovePlayerPacket::Rot { x_rot, .. } => *x_rot,
            _ => fallback,
        }
    }
}

impl Packet for ServerboundMovePlayerPacket {
    fn packet_type(&self) -> PacketType {
        match self {
            ServerboundMovePlayerPacket::Pos { .. } => PacketType::serverbound("move_player_pos"),
            ServerboundMovePlayerPacket::PosRot { .. } => {
                PacketType::serverbound("move_player_pos_rot")
            }
            ServerboundMovePlayerPacket::Rot { .. } => PacketType::serverbound("move_player_rot"),
            ServerboundMovePlayerPacket::StatusOnly { .. } => {
                PacketType::serverbound("move_player_status_only")
            }
        }
    }
}

/// `ServerboundMovePlayerPacket.Pos.STREAM_CODEC` — the `Packet.codec` writer/
/// reader pair for the `Pos` subclass.
pub fn pos_codec() -> StreamCodec<FriendlyByteBuf, ServerboundMovePlayerPacket> {
    codec(
        |value: &ServerboundMovePlayerPacket, output: &mut FriendlyByteBuf| match value {
            ServerboundMovePlayerPacket::Pos {
                x,
                y,
                z,
                on_ground,
                horizontal_collision,
            } => {
                output.write_double(*x);
                output.write_double(*y);
                output.write_double(*z);
                output.write_byte(pack_flags(*on_ground, *horizontal_collision) as i8);
                Ok(())
            }
            _ => unreachable!("pos_codec encodes only ServerboundMovePlayerPacket::Pos"),
        },
        |input: &mut FriendlyByteBuf| {
            let x = input.read_double();
            let y = input.read_double();
            let z = input.read_double();
            let flags = input.read_unsigned_byte() as i32;
            Ok(ServerboundMovePlayerPacket::Pos {
                x,
                y,
                z,
                on_ground: unpack_on_ground(flags),
                horizontal_collision: unpack_horizontal_collision(flags),
            })
        },
    )
}

/// `ServerboundMovePlayerPacket.PosRot.STREAM_CODEC`.
pub fn pos_rot_codec() -> StreamCodec<FriendlyByteBuf, ServerboundMovePlayerPacket> {
    codec(
        |value: &ServerboundMovePlayerPacket, output: &mut FriendlyByteBuf| match value {
            ServerboundMovePlayerPacket::PosRot {
                x,
                y,
                z,
                y_rot,
                x_rot,
                on_ground,
                horizontal_collision,
            } => {
                output.write_double(*x);
                output.write_double(*y);
                output.write_double(*z);
                output.write_float(*y_rot);
                output.write_float(*x_rot);
                output.write_byte(pack_flags(*on_ground, *horizontal_collision) as i8);
                Ok(())
            }
            _ => unreachable!("pos_rot_codec encodes only ServerboundMovePlayerPacket::PosRot"),
        },
        |input: &mut FriendlyByteBuf| {
            let x = input.read_double();
            let y = input.read_double();
            let z = input.read_double();
            let y_rot = input.read_float();
            let x_rot = input.read_float();
            let flags = input.read_unsigned_byte() as i32;
            Ok(ServerboundMovePlayerPacket::PosRot {
                x,
                y,
                z,
                y_rot,
                x_rot,
                on_ground: unpack_on_ground(flags),
                horizontal_collision: unpack_horizontal_collision(flags),
            })
        },
    )
}

/// `ServerboundMovePlayerPacket.Rot.STREAM_CODEC`.
pub fn rot_codec() -> StreamCodec<FriendlyByteBuf, ServerboundMovePlayerPacket> {
    codec(
        |value: &ServerboundMovePlayerPacket, output: &mut FriendlyByteBuf| match value {
            ServerboundMovePlayerPacket::Rot {
                y_rot,
                x_rot,
                on_ground,
                horizontal_collision,
            } => {
                output.write_float(*y_rot);
                output.write_float(*x_rot);
                output.write_byte(pack_flags(*on_ground, *horizontal_collision) as i8);
                Ok(())
            }
            _ => unreachable!("rot_codec encodes only ServerboundMovePlayerPacket::Rot"),
        },
        |input: &mut FriendlyByteBuf| {
            let y_rot = input.read_float();
            let x_rot = input.read_float();
            let flags = input.read_unsigned_byte() as i32;
            Ok(ServerboundMovePlayerPacket::Rot {
                y_rot,
                x_rot,
                on_ground: unpack_on_ground(flags),
                horizontal_collision: unpack_horizontal_collision(flags),
            })
        },
    )
}

/// `ServerboundMovePlayerPacket.StatusOnly.STREAM_CODEC`.
pub fn status_only_codec() -> StreamCodec<FriendlyByteBuf, ServerboundMovePlayerPacket> {
    codec(
        |value: &ServerboundMovePlayerPacket, output: &mut FriendlyByteBuf| match value {
            ServerboundMovePlayerPacket::StatusOnly {
                on_ground,
                horizontal_collision,
            } => {
                output.write_byte(pack_flags(*on_ground, *horizontal_collision) as i8);
                Ok(())
            }
            _ => unreachable!(
                "status_only_codec encodes only ServerboundMovePlayerPacket::StatusOnly"
            ),
        },
        |input: &mut FriendlyByteBuf| {
            let flags = input.read_unsigned_byte() as i32;
            Ok(ServerboundMovePlayerPacket::StatusOnly {
                on_ground: unpack_on_ground(flags),
                horizontal_collision: unpack_horizontal_collision(flags),
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    fn buf() -> FriendlyByteBuf {
        FriendlyByteBuf::new(BytesMut::new())
    }

    fn written(b: FriendlyByteBuf) -> Vec<u8> {
        b.into_inner().to_vec()
    }

    fn encode(
        codec: &StreamCodec<FriendlyByteBuf, ServerboundMovePlayerPacket>,
        v: &ServerboundMovePlayerPacket,
    ) -> Vec<u8> {
        let mut out = buf();
        codec.encode(&mut out, v).unwrap();
        written(out)
    }

    fn decode(
        codec: &StreamCodec<FriendlyByteBuf, ServerboundMovePlayerPacket>,
        bytes: &[u8],
    ) -> ServerboundMovePlayerPacket {
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes));
        let v = codec.decode(&mut input).unwrap();
        assert_eq!(input.readable_bytes(), 0, "trailing bytes");
        v
    }

    /// Appends the Java `packFlags` byte to a payload prefix.
    fn flags_byte(on_ground: bool, horizontal_collision: bool) -> u8 {
        pack_flags(on_ground, horizontal_collision) as u8
    }

    #[test]
    fn pos_round_trips_exact_bytes_and_length() {
        let packet = ServerboundMovePlayerPacket::Pos {
            x: 1.5,
            y: -2.25,
            z: 3.5,
            on_ground: true,
            horizontal_collision: false,
        };
        let codec = pos_codec();
        let bytes = encode(&codec, &packet);
        assert_eq!(bytes.len(), 25, "3 doubles + 1 flags byte");
        let mut expected = Vec::new();
        expected.extend_from_slice(&1.5f64.to_be_bytes());
        expected.extend_from_slice(&(-2.25f64).to_be_bytes());
        expected.extend_from_slice(&3.5f64.to_be_bytes());
        expected.push(flags_byte(true, false));
        assert_eq!(bytes, expected);
        assert_eq!(decode(&codec, &bytes), packet);
    }

    #[test]
    fn pos_rot_round_trips_exact_bytes_and_length() {
        let packet = ServerboundMovePlayerPacket::PosRot {
            x: 10.0,
            y: 64.0,
            z: -8.0,
            y_rot: 90.0,
            x_rot: -45.0,
            on_ground: true,
            horizontal_collision: true,
        };
        let codec = pos_rot_codec();
        let bytes = encode(&codec, &packet);
        assert_eq!(bytes.len(), 33, "3 doubles + 2 floats + 1 flags byte");
        let mut expected = Vec::new();
        expected.extend_from_slice(&10.0f64.to_be_bytes());
        expected.extend_from_slice(&64.0f64.to_be_bytes());
        expected.extend_from_slice(&(-8.0f64).to_be_bytes());
        expected.extend_from_slice(&90.0f32.to_be_bytes());
        expected.extend_from_slice(&(-45.0f32).to_be_bytes());
        expected.push(flags_byte(true, true));
        assert_eq!(bytes, expected);
        assert_eq!(decode(&codec, &bytes), packet);
    }

    #[test]
    fn rot_round_trips_exact_bytes_and_length() {
        let packet = ServerboundMovePlayerPacket::Rot {
            y_rot: 180.0,
            x_rot: 0.5,
            on_ground: false,
            horizontal_collision: true,
        };
        let codec = rot_codec();
        let bytes = encode(&codec, &packet);
        assert_eq!(bytes.len(), 9, "2 floats + 1 flags byte");
        let mut expected = Vec::new();
        expected.extend_from_slice(&180.0f32.to_be_bytes());
        expected.extend_from_slice(&0.5f32.to_be_bytes());
        expected.push(flags_byte(false, true));
        assert_eq!(bytes, expected);
        assert_eq!(decode(&codec, &bytes), packet);
    }

    #[test]
    fn status_only_round_trips_exact_bytes_and_length() {
        let packet = ServerboundMovePlayerPacket::StatusOnly {
            on_ground: true,
            horizontal_collision: false,
        };
        let codec = status_only_codec();
        let bytes = encode(&codec, &packet);
        assert_eq!(bytes.len(), 1);
        assert_eq!(bytes, vec![flags_byte(true, false)]);
        assert_eq!(decode(&codec, &bytes), packet);
    }

    #[test]
    fn flags_matrix_packs_each_combination() {
        // All four variants x the four (onGround, horizontalCollision) combos.
        for on_ground in [false, true] {
            for horizontal_collision in [false, true] {
                let flags = flags_byte(on_ground, horizontal_collision);
                assert_eq!(unpack_on_ground(flags as i32), on_ground);
                assert_eq!(
                    unpack_horizontal_collision(flags as i32),
                    horizontal_collision
                );

                let pos = ServerboundMovePlayerPacket::Pos {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    on_ground,
                    horizontal_collision,
                };
                let bytes = encode(&pos_codec(), &pos);
                assert_eq!(*bytes.last().unwrap(), flags);
                assert_eq!(decode(&pos_codec(), &bytes), pos);

                let status = ServerboundMovePlayerPacket::StatusOnly {
                    on_ground,
                    horizontal_collision,
                };
                let bytes = encode(&status_only_codec(), &status);
                assert_eq!(bytes, vec![flags]);
                assert_eq!(decode(&status_only_codec(), &bytes), status);
            }
        }
    }

    #[test]
    fn packet_type_matches_vanilla_ids() {
        use crate::generated::packets::play::serverbound::PacketType as Play;
        assert_eq!(
            ServerboundMovePlayerPacket::Pos {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                on_ground: false,
                horizontal_collision: false,
            }
            .packet_type(),
            PacketType::serverbound("move_player_pos")
        );
        assert_eq!(Play::MovePlayerPos.id(), 30);
        assert_eq!(Play::MovePlayerPosRot.id(), 31);
        assert_eq!(Play::MovePlayerRot.id(), 32);
        assert_eq!(Play::MovePlayerStatusOnly.id(), 33);
    }

    #[test]
    fn fallback_getters_follow_has_pos_has_rot() {
        let pos = ServerboundMovePlayerPacket::Pos {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            on_ground: false,
            horizontal_collision: false,
        };
        assert!(pos.has_position());
        assert!(!pos.has_rotation());
        assert_eq!(pos.get_x(99.0), 1.0);
        assert_eq!(pos.get_y(99.0), 2.0);
        assert_eq!(pos.get_z(99.0), 3.0);
        assert_eq!(pos.get_y_rot(7.0), 7.0, "no rotation stored -> fallback");
        assert_eq!(pos.get_x_rot(7.0), 7.0);

        let rot = ServerboundMovePlayerPacket::Rot {
            y_rot: 90.0,
            x_rot: 30.0,
            on_ground: false,
            horizontal_collision: false,
        };
        assert!(!rot.has_position());
        assert!(rot.has_rotation());
        assert_eq!(rot.get_x(-1.0), -1.0, "no position stored -> fallback");
        assert_eq!(rot.get_y(-1.0), -1.0);
        assert_eq!(rot.get_z(-1.0), -1.0);
        assert_eq!(rot.get_y_rot(0.0), 90.0);
        assert_eq!(rot.get_x_rot(0.0), 30.0);

        let status = ServerboundMovePlayerPacket::StatusOnly {
            on_ground: true,
            horizontal_collision: true,
        };
        assert!(!status.has_position());
        assert!(!status.has_rotation());
        assert_eq!(status.get_x(5.0), 5.0);
        assert_eq!(status.get_y_rot(5.0), 5.0);
        assert!(status.is_on_ground());
        assert!(status.horizontal_collision());
        assert!(!pos.is_on_ground());
    }
}
