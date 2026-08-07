//! Port of `net.minecraft.network.syncher.EntityDataSerializers` (MC 26.2) —
//! the **identity** of the 43 entity-data serializers.
//!
//! Java registers the serializers into a `CrudeIncrementalIntIdentityHashBiMap`
//! in the `EntityDataSerializers` static block; the wire id is the *registration
//! order*, not the field-declaration order (`BLOCK_STATE` is declared before
//! `BOOLEAN` yet registers 15th). Per OWNERSHIP.md the runtime bi-map collapses
//! to a compile-time enum: the 43 ids are stable forever, so [`SerializerId`]
//! pins them as explicit discriminants.
//!
//! `EntityDataSerializers.getSerializer(int)` returns `null` for any id outside
//! `0..=42` (`CrudeIncrementalIntIdentityHashBiMap.byId` on a miss); the
//! `DecoderException("Unknown serializer type {n}")` that follows is reproduced
//! by [`SerializerId::try_from`] returning `None`. `registerSerializer` (Paper
//! plugins via `rivet-ffi`) would need a runtime table — the one place a
//! closed enum cannot express Java; deferred per "no speculative abstractions"
//! (the value-codec `match` dispatch is structured so an `id >= 43` fallback is
//! cheap when the JVM adapter lands).

/// The 43 registered entity-data serializer ids, in `EntityDataSerializers`
/// static-block registration order.
///
/// Discriminants are the wire ids (`DataValue.write` writes this as a VarInt;
/// `DataValue.read` maps the VarInt back through `try_from`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum SerializerId {
    /// `EntityDataSerializers.BYTE` — `ByteBufCodecs.BYTE` (`i8`).
    Byte = 0,
    /// `INT` — `ByteBufCodecs.VAR_INT` (`i32`).
    Int = 1,
    /// `LONG` — `ByteBufCodecs.VAR_LONG` (`i64`).
    Long = 2,
    /// `FLOAT` — `ByteBufCodecs.FLOAT` (`f32`).
    Float = 3,
    /// `STRING` — `ByteBufCodecs.STRING_UTF8` (`String`).
    String = 4,
    /// `COMPONENT` — `ComponentSerialization.TRUSTED_STREAM_CODEC`.
    Component = 5,
    /// `OPTIONAL_COMPONENT` — `ComponentSerialization.TRUSTED_OPTIONAL_STREAM_CODEC`.
    OptionalComponent = 6,
    /// `ITEM_STACK` — Paper's `OVERSIZED_ITEM_CODEC` (the only deep-copying
    /// serializer; `ItemStack.copy`).
    ItemStack = 7,
    /// `BOOLEAN` — `ByteBufCodecs.BOOL` (`bool`).
    Boolean = 8,
    /// `ROTATIONS` — `net.minecraft.core.Rotations` (3-float value).
    Rotations = 9,
    /// `BLOCK_POS` — `BlockPos.STREAM_CODEC` (packed long).
    BlockPos = 10,
    /// `OPTIONAL_BLOCK_POS` — bool-prefixed `BlockPos`.
    OptionalBlockPos = 11,
    /// `DIRECTION` — `Direction.STREAM_CODEC` (`idMapper` over 3D data value).
    Direction = 12,
    /// `OPTIONAL_LIVING_ENTITY_REFERENCE` — bool-prefixed
    /// `EntityReference<LivingEntity>`.
    OptionalLivingEntityReference = 13,
    /// `BLOCK_STATE` — `ByteBufCodecs.idMapper(Block.BLOCK_STATE_REGISTRY)`.
    BlockState = 14,
    /// `OPTIONAL_BLOCK_STATE` — 0-sentinel VarInt (id `0` = empty).
    OptionalBlockState = 15,
    /// `PARTICLE` — `ParticleTypes.STREAM_CODEC`.
    Particle = 16,
    /// `PARTICLES` — varint-counted `ParticleTypes.STREAM_CODEC` list.
    Particles = 17,
    /// `VILLAGER_DATA` — `VillagerData.STREAM_CODEC`.
    VillagerData = 18,
    /// `OPTIONAL_UNSIGNED_INT` — `OptionalInt` `±1` VarInt.
    OptionalUnsignedInt = 19,
    /// `POSE` — `Pose.STREAM_CODEC`.
    Pose = 20,
    /// `CAT_VARIANT` — `Holder<CatVariant>`.
    CatVariant = 21,
    /// `CAT_SOUND_VARIANT` — `Holder<CatSoundVariant>`.
    CatSoundVariant = 22,
    /// `COW_VARIANT` — `Holder<CowVariant>`.
    CowVariant = 23,
    /// `COW_SOUND_VARIANT` — `Holder<CowSoundVariant>`.
    CowSoundVariant = 24,
    /// `WOLF_VARIANT` — `Holder<WolfVariant>`.
    WolfVariant = 25,
    /// `WOLF_SOUND_VARIANT` — `Holder<WolfSoundVariant>`.
    WolfSoundVariant = 26,
    /// `FROG_VARIANT` — `Holder<FrogVariant>`.
    FrogVariant = 27,
    /// `PIG_VARIANT` — `Holder<PigVariant>`.
    PigVariant = 28,
    /// `PIG_SOUND_VARIANT` — `Holder<PigSoundVariant>`.
    PigSoundVariant = 29,
    /// `CHICKEN_VARIANT` — `Holder<ChickenVariant>`.
    ChickenVariant = 30,
    /// `CHICKEN_SOUND_VARIANT` — `Holder<ChickenSoundVariant>`.
    ChickenSoundVariant = 31,
    /// `ZOMBIE_NAUTILUS_VARIANT` — `Holder<ZombieNautilusVariant>`.
    ZombieNautilusVariant = 32,
    /// `OPTIONAL_GLOBAL_POS` — bool-prefixed `GlobalPos`.
    OptionalGlobalPos = 33,
    /// `PAINTING_VARIANT` — `Holder<PaintingVariant>`.
    PaintingVariant = 34,
    /// `SNIFFER_STATE` — `Sniffer.State.STREAM_CODEC`.
    SnifferState = 35,
    /// `ARMADILLO_STATE` — `Armadillo.ArmadilloState.STREAM_CODEC`.
    ArmadilloState = 36,
    /// `COPPER_GOLEM_STATE` — `CopperGolemState.STREAM_CODEC`.
    CopperGolemState = 37,
    /// `WEATHERING_COPPER_STATE` — `WeatheringCopper.WeatherState.STREAM_CODEC`.
    WeatheringCopperState = 38,
    /// `VECTOR3` — `ByteBufCodecs.VECTOR3F` (JOML `Vector3fc`).
    Vector3 = 39,
    /// `QUATERNION` — `ByteBufCodecs.QUATERNIONF` (JOML `Quaternionfc`).
    Quaternion = 40,
    /// `RESOLVABLE_PROFILE` — `ResolvableProfile.STREAM_CODEC`.
    ResolvableProfile = 41,
    /// `HUMANOID_ARM` — `HumanoidArm.STREAM_CODEC`.
    HumanoidArm = 42,
}

impl SerializerId {
    /// `EntityDataSerializers.getSerializedId(serializer)` — the wire id. Unlike
    /// Java there is no unregistered sentinel: the enum *is* the closed set of
    /// registered ids, so every value encodes `>= 0`.
    pub fn serialized_id(self) -> i32 {
        self as i32
    }
}

impl SerializerId {
    /// `EntityDataSerializers.getSerializer(int)` — `Some` for the registered
    /// range `0..=42`, `None` for anything else (Java's `null`, which the
    /// `DataValue.read` decoder turns into `DecoderException("Unknown serializer
    /// type {n}")`).
    pub fn try_from(id: i32) -> Option<SerializerId> {
        let s = match id {
            0 => SerializerId::Byte,
            1 => SerializerId::Int,
            2 => SerializerId::Long,
            3 => SerializerId::Float,
            4 => SerializerId::String,
            5 => SerializerId::Component,
            6 => SerializerId::OptionalComponent,
            7 => SerializerId::ItemStack,
            8 => SerializerId::Boolean,
            9 => SerializerId::Rotations,
            10 => SerializerId::BlockPos,
            11 => SerializerId::OptionalBlockPos,
            12 => SerializerId::Direction,
            13 => SerializerId::OptionalLivingEntityReference,
            14 => SerializerId::BlockState,
            15 => SerializerId::OptionalBlockState,
            16 => SerializerId::Particle,
            17 => SerializerId::Particles,
            18 => SerializerId::VillagerData,
            19 => SerializerId::OptionalUnsignedInt,
            20 => SerializerId::Pose,
            21 => SerializerId::CatVariant,
            22 => SerializerId::CatSoundVariant,
            23 => SerializerId::CowVariant,
            24 => SerializerId::CowSoundVariant,
            25 => SerializerId::WolfVariant,
            26 => SerializerId::WolfSoundVariant,
            27 => SerializerId::FrogVariant,
            28 => SerializerId::PigVariant,
            29 => SerializerId::PigSoundVariant,
            30 => SerializerId::ChickenVariant,
            31 => SerializerId::ChickenSoundVariant,
            32 => SerializerId::ZombieNautilusVariant,
            33 => SerializerId::OptionalGlobalPos,
            34 => SerializerId::PaintingVariant,
            35 => SerializerId::SnifferState,
            36 => SerializerId::ArmadilloState,
            37 => SerializerId::CopperGolemState,
            38 => SerializerId::WeatheringCopperState,
            39 => SerializerId::Vector3,
            40 => SerializerId::Quaternion,
            41 => SerializerId::ResolvableProfile,
            42 => SerializerId::HumanoidArm,
            _ => return None,
        };
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_registration_order_and_contiguous() {
        let expected = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42,
        ];
        for (i, want) in expected.iter().enumerate() {
            let id = SerializerId::try_from(i as i32).expect("in range");
            assert_eq!(id as i32, *want, "id {i}");
            assert_eq!(id.serialized_id(), *want);
        }
        // `try_from` is total over the enum: serializing a variant id re-resolves.
        let mut seen = 0;
        let mut id = 0;
        while let Some(s) = SerializerId::try_from(id) {
            assert_eq!(s.serialized_id(), id);
            seen += 1;
            id += 1;
        }
        assert_eq!(seen, 43);
    }

    #[test]
    fn out_of_range_is_none_like_java_null() {
        assert_eq!(SerializerId::try_from(-1), None);
        assert_eq!(SerializerId::try_from(43), None);
        assert_eq!(SerializerId::try_from(1000), None);
    }
}
