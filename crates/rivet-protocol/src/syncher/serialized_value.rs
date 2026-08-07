//! The erased entity-data value union (`SynchedEntityData.DataValue`'s value).
//!
//! Java's `DataValue<T>` stores the concrete `T`; the 43 serializers are 43
//! distinct value types. Rust models that erased store as [`SerializedValue`],
//! one variant per serializer with the same id-pinning discriminants as
//! [`SerializerId`], so future ports cannot drift ids.
//!
//! Wire codecs live here, with the value types, rather than in the
//! [`crate::codec`] module — the port's usual home for `ByteBufCodecs` codecs.
//! The deviation is structural: several value types are entity/world-layer
//! (`ItemStack`, `ParticleOptions`, the variant holders) that `rivet-protocol`'s
//! codec module can never reference without a cycle, so the codec dispatch rides
//! on the value union. The join-critical subset proven by the #153 fixture is
//! `BYTE` (id 0) and `FLOAT` (id 3) — the only two value codecs implemented.
//! Every other variant is **blocked** for the M3 entity wave (see the `RivetTodo`
//! marker below): its payload is a unit placeholder and `read`/`write` panic
//! loudly with a blocked note rather than inventing a value. This matches #90's
//! acceptance (non-join serializers carry a blocked note); decoding a
//! `set_entity_data` item whose serializer is blocked therefore fails loudly,
//! exactly as the honest port must.
//!
//! RivetTodo(#222): the 41 non-BYTE/FLOAT serializer value codecs are blocked
//! for the M3 entity wave — their payloads are unit placeholders and read/write
//! panic with a blocked note until the owning entity unit lands.
//!
//! Copy semantics: `DataValue::create` calls `serializer.copy(value)`. All
//! `ForValueType` serializers copy by identity, which `SerializedValue: Clone`
//! expresses; the one deep-copying serializer is `ITEM_STACK` (`ItemStack.copy`),
//! so when that variant gains a payload its `Clone` must be the deep copy.

use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;

use super::serializer_id::SerializerId;

/// The erased synced-data value. Discriminants match [`SerializerId`] exactly.
///
/// Only [`SerializedValue::Byte`] and [`SerializedValue::Float`] carry payloads
/// today; the 41 other variants are unit placeholders pinned to their serializer
/// id (see the module doc for the blocking policy).
#[derive(Debug, Clone)]
pub enum SerializedValue {
    /// `BYTE` — `i8`.
    Byte(i8),
    /// `INT` — BLOCKED (`ByteBufCodecs.VAR_INT` exists; M3).
    Int,
    /// `LONG` — BLOCKED (`ByteBufCodecs.VAR_LONG` exists; M3).
    Long,
    /// `FLOAT` — `f32` (raw bits).
    Float(f32),
    /// `STRING` — BLOCKED (`ByteBufCodecs.STRING_UTF8` exists; M3).
    String,
    /// `COMPONENT` — BLOCKED (component wire stream codec not ported).
    Component,
    /// `OPTIONAL_COMPONENT` — BLOCKED.
    OptionalComponent,
    /// `ITEM_STACK` — BLOCKED (ItemStack not ported; the only deep-copy
    /// serializer — its `Clone` must be `ItemStack.copy`).
    ItemStack,
    /// `BOOLEAN` — BLOCKED (`ByteBufCodecs.BOOL` exists; M3).
    Boolean,
    /// `ROTATIONS` — BLOCKED (`net.minecraft.core.Rotations`, 3 floats; M3).
    Rotations,
    /// `BLOCK_POS` — BLOCKED (`BlockPos.STREAM_CODEC` exists; M3).
    BlockPos,
    /// `OPTIONAL_BLOCK_POS` — BLOCKED (bool-prefixed `BlockPos`; M3).
    OptionalBlockPos,
    /// `DIRECTION` — BLOCKED (`Direction.STREAM_CODEC` not ported).
    Direction,
    /// `OPTIONAL_LIVING_ENTITY_REFERENCE` — BLOCKED (depends on the entity
    /// model).
    OptionalLivingEntityReference,
    /// `BLOCK_STATE` — BLOCKED (block-state id codec not ported).
    BlockState,
    /// `OPTIONAL_BLOCK_STATE` — BLOCKED.
    OptionalBlockState,
    /// `PARTICLE` — BLOCKED (`ParticleOptions` not ported).
    Particle,
    /// `PARTICLES` — BLOCKED.
    Particles,
    /// `VILLAGER_DATA` — BLOCKED.
    VillagerData,
    /// `OPTIONAL_UNSIGNED_INT` — BLOCKED (`OptionalInt` `±1` varint exists; M3).
    OptionalUnsignedInt,
    /// `POSE` — BLOCKED (entity-layer enum).
    Pose,
    /// `CAT_VARIANT` — BLOCKED (variant holder element not ported).
    CatVariant,
    /// `CAT_SOUND_VARIANT` — BLOCKED.
    CatSoundVariant,
    /// `COW_VARIANT` — BLOCKED.
    CowVariant,
    /// `COW_SOUND_VARIANT` — BLOCKED.
    CowSoundVariant,
    /// `WOLF_VARIANT` — BLOCKED.
    WolfVariant,
    /// `WOLF_SOUND_VARIANT` — BLOCKED.
    WolfSoundVariant,
    /// `FROG_VARIANT` — BLOCKED.
    FrogVariant,
    /// `PIG_VARIANT` — BLOCKED.
    PigVariant,
    /// `PIG_SOUND_VARIANT` — BLOCKED.
    PigSoundVariant,
    /// `CHICKEN_VARIANT` — BLOCKED.
    ChickenVariant,
    /// `CHICKEN_SOUND_VARIANT` — BLOCKED.
    ChickenSoundVariant,
    /// `ZOMBIE_NAUTILUS_VARIANT` — BLOCKED.
    ZombieNautilusVariant,
    /// `OPTIONAL_GLOBAL_POS` — BLOCKED (bool-prefixed `GlobalPos`; M3).
    OptionalGlobalPos,
    /// `PAINTING_VARIANT` — BLOCKED.
    PaintingVariant,
    /// `SNIFFER_STATE` — BLOCKED.
    SnifferState,
    /// `ARMADILLO_STATE` — BLOCKED.
    ArmadilloState,
    /// `COPPER_GOLEM_STATE` — BLOCKED.
    CopperGolemState,
    /// `WEATHERING_COPPER_STATE` — BLOCKED.
    WeatheringCopperState,
    /// `VECTOR3` — BLOCKED (JOML `Vector3fc`; no JOML port).
    Vector3,
    /// `QUATERNION` — BLOCKED (JOML `Quaternionfc`).
    Quaternion,
    /// `RESOLVABLE_PROFILE` — BLOCKED (GameProfile not ported).
    ResolvableProfile,
    /// `HUMANOID_ARM` — BLOCKED (entity-layer enum).
    HumanoidArm,
}

impl PartialEq for SerializedValue {
    /// Java's `DataValue` is a record whose `equals` compares the concrete
    /// value with its own `equals`. For a FLOAT slot that is `Float.equals`,
    /// which canonicalizes through `floatToIntBits`: **any NaN equals any NaN**
    /// and **-0.0 != 0.0** — the opposite of IEEE `f32 ==`. A derived
    /// `PartialEq` would make the store's dirty-flag/`isSetToDefault` checks
    /// diverge from Java, so the FLOAT arm is implemented explicitly.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (SerializedValue::Byte(a), SerializedValue::Byte(b)) => a == b,
            (SerializedValue::Float(a), SerializedValue::Float(b)) => {
                // `floatToIntBits`: NaN canonicalizes (all NaNs equal); non-NaN
                // values compare by raw bits (so -0.0 differs from 0.0).
                (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
            }
            _ => false,
        }
    }
}

impl SerializedValue {
    /// The serializer this value belongs to — the reverse of `read`'s dispatch
    /// (`SerializerId::try_from`). Used by `DataValue`/`DataItem` to cross-check
    /// the id that selected the value.
    pub fn serializer(&self) -> SerializerId {
        // The discriminants match SerializerId one-to-one; a cast is exact.
        let id = match self {
            SerializedValue::Byte(_) => 0,
            SerializedValue::Int => 1,
            SerializedValue::Long => 2,
            SerializedValue::Float(_) => 3,
            SerializedValue::String => 4,
            SerializedValue::Component => 5,
            SerializedValue::OptionalComponent => 6,
            SerializedValue::ItemStack => 7,
            SerializedValue::Boolean => 8,
            SerializedValue::Rotations => 9,
            SerializedValue::BlockPos => 10,
            SerializedValue::OptionalBlockPos => 11,
            SerializedValue::Direction => 12,
            SerializedValue::OptionalLivingEntityReference => 13,
            SerializedValue::BlockState => 14,
            SerializedValue::OptionalBlockState => 15,
            SerializedValue::Particle => 16,
            SerializedValue::Particles => 17,
            SerializedValue::VillagerData => 18,
            SerializedValue::OptionalUnsignedInt => 19,
            SerializedValue::Pose => 20,
            SerializedValue::CatVariant => 21,
            SerializedValue::CatSoundVariant => 22,
            SerializedValue::CowVariant => 23,
            SerializedValue::CowSoundVariant => 24,
            SerializedValue::WolfVariant => 25,
            SerializedValue::WolfSoundVariant => 26,
            SerializedValue::FrogVariant => 27,
            SerializedValue::PigVariant => 28,
            SerializedValue::PigSoundVariant => 29,
            SerializedValue::ChickenVariant => 30,
            SerializedValue::ChickenSoundVariant => 31,
            SerializedValue::ZombieNautilusVariant => 32,
            SerializedValue::OptionalGlobalPos => 33,
            SerializedValue::PaintingVariant => 34,
            SerializedValue::SnifferState => 35,
            SerializedValue::ArmadilloState => 36,
            SerializedValue::CopperGolemState => 37,
            SerializedValue::WeatheringCopperState => 38,
            SerializedValue::Vector3 => 39,
            SerializedValue::Quaternion => 40,
            SerializedValue::ResolvableProfile => 41,
            SerializedValue::HumanoidArm => 42,
        };
        SerializerId::try_from(id).expect("every variant maps to a registered id")
    }

    /// `DataValue.read`'s value half — `serializer.codec().decode(input)` for
    /// the serializer that selected this value. `FLOAT`/`BYTE` run the raw-bits
    /// `ByteBufCodecs.FLOAT`/`BYTE`; every blocked serializer panics loudly
    /// (Java would decode it; Rivet cannot until the M3 entity wave).
    pub fn read(input: &mut RegistryFriendlyByteBuf, serializer: SerializerId) -> Self {
        match serializer {
            SerializerId::Byte => SerializedValue::Byte(input.inner_mut().read_byte()),
            SerializerId::Float => SerializedValue::Float(input.inner_mut().read_float()),
            blocked => panic!(
                "blocked: serializer {blocked:?} value codec not ported (M3 entity wave, #90)"
            ),
        }
    }

    /// `DataValue.write`'s value half — `serializer.codec().encode(output, value)`.
    pub fn write(&self, output: &mut RegistryFriendlyByteBuf) {
        match self {
            SerializedValue::Byte(v) => {
                output.inner_mut().write_byte(*v);
            }
            SerializedValue::Float(v) => {
                output.inner_mut().write_float(*v);
            }
            blocked => panic!(
                "blocked: serializer {blocked:?} value codec not ported (M3 entity wave, #90)"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
    use bytes::BytesMut;
    use rivet_registry::RegistryAccess;
    use std::panic::catch_unwind;

    fn buffer() -> RegistryFriendlyByteBuf {
        RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty())
    }

    fn panic_message<F: FnOnce() -> R, R>(f: F) -> String {
        let err = match catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(_) => panic!("expected the closure to panic"),
            Err(err) => err,
        };
        err.downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "non-string panic payload".to_string())
    }

    #[test]
    fn serializer_mapping_is_exhaustive_and_id_consistent() {
        // Cross-checks the two parallel 43-arm tables: every `SerializedValue`
        // variant must map back through `serializer()` to the `SerializerId`
        // whose wire id the variant declares. This pins the whole table so a
        // future port cannot drift ids between the value union and the id enum.
        let cases: &[(SerializerId, SerializedValue)] = &[
            (SerializerId::Byte, SerializedValue::Byte(0)),
            (SerializerId::Int, SerializedValue::Int),
            (SerializerId::Long, SerializedValue::Long),
            (SerializerId::Float, SerializedValue::Float(0.0)),
            (SerializerId::String, SerializedValue::String),
            (SerializerId::Component, SerializedValue::Component),
            (
                SerializerId::OptionalComponent,
                SerializedValue::OptionalComponent,
            ),
            (SerializerId::ItemStack, SerializedValue::ItemStack),
            (SerializerId::Boolean, SerializedValue::Boolean),
            (SerializerId::Rotations, SerializedValue::Rotations),
            (SerializerId::BlockPos, SerializedValue::BlockPos),
            (
                SerializerId::OptionalBlockPos,
                SerializedValue::OptionalBlockPos,
            ),
            (SerializerId::Direction, SerializedValue::Direction),
            (
                SerializerId::OptionalLivingEntityReference,
                SerializedValue::OptionalLivingEntityReference,
            ),
            (SerializerId::BlockState, SerializedValue::BlockState),
            (
                SerializerId::OptionalBlockState,
                SerializedValue::OptionalBlockState,
            ),
            (SerializerId::Particle, SerializedValue::Particle),
            (SerializerId::Particles, SerializedValue::Particles),
            (SerializerId::VillagerData, SerializedValue::VillagerData),
            (
                SerializerId::OptionalUnsignedInt,
                SerializedValue::OptionalUnsignedInt,
            ),
            (SerializerId::Pose, SerializedValue::Pose),
            (SerializerId::CatVariant, SerializedValue::CatVariant),
            (
                SerializerId::CatSoundVariant,
                SerializedValue::CatSoundVariant,
            ),
            (SerializerId::CowVariant, SerializedValue::CowVariant),
            (
                SerializerId::CowSoundVariant,
                SerializedValue::CowSoundVariant,
            ),
            (SerializerId::WolfVariant, SerializedValue::WolfVariant),
            (
                SerializerId::WolfSoundVariant,
                SerializedValue::WolfSoundVariant,
            ),
            (SerializerId::FrogVariant, SerializedValue::FrogVariant),
            (SerializerId::PigVariant, SerializedValue::PigVariant),
            (
                SerializerId::PigSoundVariant,
                SerializedValue::PigSoundVariant,
            ),
            (
                SerializerId::ChickenVariant,
                SerializedValue::ChickenVariant,
            ),
            (
                SerializerId::ChickenSoundVariant,
                SerializedValue::ChickenSoundVariant,
            ),
            (
                SerializerId::ZombieNautilusVariant,
                SerializedValue::ZombieNautilusVariant,
            ),
            (
                SerializerId::OptionalGlobalPos,
                SerializedValue::OptionalGlobalPos,
            ),
            (
                SerializerId::PaintingVariant,
                SerializedValue::PaintingVariant,
            ),
            (SerializerId::SnifferState, SerializedValue::SnifferState),
            (
                SerializerId::ArmadilloState,
                SerializedValue::ArmadilloState,
            ),
            (
                SerializerId::CopperGolemState,
                SerializedValue::CopperGolemState,
            ),
            (
                SerializerId::WeatheringCopperState,
                SerializedValue::WeatheringCopperState,
            ),
            (SerializerId::Vector3, SerializedValue::Vector3),
            (SerializerId::Quaternion, SerializedValue::Quaternion),
            (
                SerializerId::ResolvableProfile,
                SerializedValue::ResolvableProfile,
            ),
            (SerializerId::HumanoidArm, SerializedValue::HumanoidArm),
        ];
        assert_eq!(cases.len(), 43, "one case per registered serializer");
        for (id, value) in cases {
            assert_eq!(value.serializer(), *id, "{id:?}");
            assert_eq!(
                value.serializer().serialized_id(),
                id.serialized_id(),
                "wire id for {id:?}"
            );
        }
    }

    #[test]
    fn byte_and_float_round_trip() {
        for value in [
            SerializedValue::Byte(127),
            SerializedValue::Byte(-128),
            SerializedValue::Float(20.0),
            SerializedValue::Float(-0.0),
        ] {
            let mut out = buffer();
            value.write(&mut out);
            let mut input = RegistryFriendlyByteBuf::new(out.into_inner(), RegistryAccess::empty());
            let got = SerializedValue::read(&mut input, value.serializer());
            assert_eq!(got, value);
            assert_eq!(input.readable_bytes(), 0);
        }
    }

    #[test]
    fn float_equality_follows_java_float_equals() {
        // Java `Float.equals` — `floatToIntBits` canonicalizes NaN (all NaNs
        // equal) and preserves the sign of zero (-0.0 != 0.0). The derived IEEE
        // `f32 ==` would be the opposite on both.
        assert_ne!(
            SerializedValue::Float(0.0),
            SerializedValue::Float(-0.0),
            "-0.0 must not equal 0.0"
        );
        assert_eq!(
            SerializedValue::Float(f32::from_bits(0x7fc0_0001)),
            SerializedValue::Float(f32::from_bits(0xffc0_0002)),
            "any NaN equals any NaN"
        );
        assert_eq!(
            SerializedValue::Float(f32::NAN),
            SerializedValue::Float(f32::NAN)
        );
        assert_eq!(SerializedValue::Float(20.0), SerializedValue::Float(20.0));
        assert_ne!(SerializedValue::Float(20.0), SerializedValue::Float(20.5));
    }

    #[test]
    fn float_wire_is_raw_bits() {
        let mut out = buffer();
        SerializedValue::Float(20.0).write(&mut out);
        assert_eq!(out.as_slice(), &20.0f32.to_be_bytes());
        // NaN payload passes through raw (floatToRawIntBits, no canonicalization).
        let mut out = buffer();
        let nan = SerializedValue::Float(f32::from_bits(0x7fc0_0001));
        nan.write(&mut out);
        assert_eq!(out.as_slice(), &0x7fc0_0001u32.to_be_bytes());
    }

    #[test]
    fn byte_wire_is_single_byte() {
        let mut out = buffer();
        SerializedValue::Byte(-5).write(&mut out);
        // The byte is written raw (`writeByte`), so -5 is 0xFB on the wire.
        assert_eq!(out.as_slice(), &[0xFB]);
    }

    #[test]
    fn blocked_value_panics_loudly() {
        let mut out = buffer();
        let msg = panic_message(|| SerializedValue::Int.write(&mut out));
        assert!(msg.contains("blocked"), "got: {msg}");
        let mut input = buffer();
        let msg = panic_message(|| {
            let _ = SerializedValue::read(&mut input, SerializerId::Int);
        });
        assert!(msg.contains("blocked"), "got: {msg}");
    }
}
