//! Port of `net.minecraft.world.level.levelgen.placement.CaveSurface`
//! (enum, 26.2).
//!
//! Java: a `StringRepresentable` enum with the two cave surfaces — `CEILING`
//! (direction UP, y `+1`) and `FLOOR` (direction DOWN, y `-1`) — whose `CODEC`
//! is `StringRepresentable.fromEnum(CaveSurface::values)`. It is the enum the
//! `.blender` unit's `CaveSurfacePlacement` (the namesake of this worktree)
//! consumes for its `surface` codec field.
//!
//! The `CODEC` is the ops-generic `cave_surface_codec::<Ops>()` factory, reusing
//! the heightmap `types_codec` shape (`string_representable::from_enum`).

use rivet_registry::core::Direction;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::string_representable::{self, EnumCodec, EnumOrdinal, StringRepresentable};

/// `CaveSurface` — the two cave surfaces, in Java's declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaveSurface {
    /// `CEILING(Direction.UP, 1, "ceiling")`.
    Ceiling,
    /// `FLOOR(Direction.DOWN, -1, "floor")`.
    Floor,
}

impl CaveSurface {
    /// `getDirection()` — the surface's scan direction.
    pub fn get_direction(&self) -> Direction {
        match self {
            CaveSurface::Ceiling => Direction::Up,
            CaveSurface::Floor => Direction::Down,
        }
    }

    /// `getY()` — the surface's `y` step (`+1` ceiling, `-1` floor).
    pub fn get_y(&self) -> i32 {
        match self {
            CaveSurface::Ceiling => 1,
            CaveSurface::Floor => -1,
        }
    }
}

impl StringRepresentable for CaveSurface {
    /// `getSerializedName()` — the `id` field ("ceiling"/"floor").
    fn get_serialized_name(&self) -> &str {
        match self {
            CaveSurface::Ceiling => "ceiling",
            CaveSurface::Floor => "floor",
        }
    }
}

impl EnumOrdinal for CaveSurface {
    /// `Enum.ordinal()` — the declaration position.
    fn ordinal(&self) -> usize {
        match self {
            CaveSurface::Ceiling => 0,
            CaveSurface::Floor => 1,
        }
    }
}

impl std::fmt::Display for CaveSurface {
    /// `Enum.toString()` — the constant name (`CEILING`/`FLOOR`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CaveSurface::Ceiling => "CEILING",
            CaveSurface::Floor => "FLOOR",
        })
    }
}

/// `CaveSurface.CODEC` — `StringRepresentable.fromEnum(CaveSurface::values)`,
/// as the ops-generic `cave_surface_codec::<Ops>()` factory. Callers erase it
/// at the field-builder site (`Arc::new(cave_surface_codec::<Ops>())`), exactly
/// like the heightmap `types_codec` reuse. The consuming `.blender` unit's
/// `CaveSurfacePlacement` lands next wave, so the factory is forward-declared
/// (exercised here by the codec tests).
#[allow(dead_code)]
pub fn cave_surface_codec<Ops: DynamicOps + 'static>() -> EnumCodec<CaveSurface, Ops> {
    const VALUES: [CaveSurface; 2] = [CaveSurface::Ceiling, CaveSurface::Floor];
    string_representable::from_enum::<CaveSurface, Ops>(&VALUES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::codec::Codec;
    use rivet_serialization::json_ops::JsonOps;
    use std::sync::Arc;

    #[test]
    fn direction_and_y_follow_the_enum_constructor_args() {
        assert_eq!(CaveSurface::Ceiling.get_direction(), Direction::Up);
        assert_eq!(CaveSurface::Ceiling.get_y(), 1);
        assert_eq!(CaveSurface::Floor.get_direction(), Direction::Down);
        assert_eq!(CaveSurface::Floor.get_y(), -1);
    }

    #[test]
    fn codec_round_trips_all_values() {
        let ops = JsonOps::INSTANCE;
        let codec: Arc<dyn Codec<CaveSurface, JsonOps>> = Arc::new(cave_surface_codec::<JsonOps>());
        for value in [CaveSurface::Ceiling, CaveSurface::Floor] {
            let encoded = codec
                .encode_start(&ops, &value)
                .get_or_throw("encode")
                .clone();
            assert_eq!(
                encoded,
                ops.create_string(value.get_serialized_name().to_string())
            );
            let decoded = codec.decode(&ops, &encoded).get_or_throw("decode").clone();
            assert_eq!(decoded.0, value);
        }
    }

    #[test]
    fn codec_rejects_an_unknown_name() {
        let ops = JsonOps::INSTANCE;
        let codec: Arc<dyn Codec<CaveSurface, JsonOps>> = Arc::new(cave_surface_codec::<JsonOps>());
        let unknown = ops.create_string("side".to_string());
        let decoded = codec.decode(&ops, &unknown);
        assert!(decoded.result().is_none());
    }
}
