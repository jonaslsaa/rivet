//! Port of `net.minecraft.network.protocol.game.CommonPlayerSpawnInfo` (#108).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! network/protocol/game/CommonPlayerSpawnInfo.java`. A record with a
//! `(RegistryFriendlyByteBuf)` decode constructor and a `write(RegistryFriendlyByteBuf)`
//! — **no `STREAM_CODEC` constant** (unlike `GameType`/`DimensionType`). It is
//! embedded by `ClientboundLoginPacket` (field 4) and `ClientboundRespawnPacket`
//! (`new CommonPlayerSpawnInfo(input)`), both owned by the #87 join wave; this
//! slice ports the value type + its wire codec as the shared foundation.
//!
//! The Rust `stream_codec()` mirrors the Java decode-ctor + `write()` exactly
//! (`of` over [`RegistryFriendlyByteBuf`]); the wire layout is:
//!
//! | field | wire form |
//! |---|---|
//! | `dimension_type` | varint holder id (`holderRegistry(DIMENSION_TYPE)`) |
//! | `dimension` | identifier **string** (`readResourceKey(DIMENSION)`) |
//! | `seed` | `long` big-endian (the *raw* seed — `BiomeManager.obfuscateSeed` runs server-side at construction, not in the codec) |
//! | `game_type` | **signed byte**, `byId` (ZERO fallback: any byte outside 0..3 incl. negative → SURVIVAL) |
//! | `previous_game_type` | **signed byte**, `byNullableId` (`-1` → null; encode `getNullableId`: null → `-1`) |
//! | `is_debug` | boolean byte |
//! | `is_flat` | boolean byte |
//! | `last_death_location` | optional: boolean presence prefix, then identifier string + packed BlockPos long |
//! | `portal_cooldown` | varint |
//! | `sea_level` | varint |
//!
//! All arithmetic is Java-faithful: no wrapping, no Mth tables, no
//! HashMap-iteration order in this slice.

use crate::codec::registry_byte_buf_codecs::dimension_type_stream_codec;
use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, of};
use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_registry::ResourceKey;
use rivet_registry::core::{GameType, GlobalPos};
use rivet_registry::holder::Holder;
use rivet_registry::registries;
use rivet_registry::registries::{DimensionType, Level};

/// `net.minecraft.network.protocol.game.CommonPlayerSpawnInfo` — the record
/// `(dimensionType, dimension, seed, gameType, previousGameType, isDebug,
/// isFlat, lastDeathLocation, portalCooldown, seaLevel)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonPlayerSpawnInfo {
    /// `dimensionType` — a `Holder<DimensionType>` reference, resolved through
    /// the `DIMENSION_TYPE` registry.
    dimension_type: Holder<DimensionType>,
    /// `dimension` — the level's `ResourceKey<Level>`.
    dimension: ResourceKey<Level>,
    /// `seed` — the raw world seed (obfuscation is server-side, see the doc).
    seed: i64,
    /// `gameType`.
    game_type: GameType,
    /// `previousGameType` — Java `@Nullable`; `None` is wire `-1`.
    previous_game_type: Option<GameType>,
    /// `isDebug`.
    is_debug: bool,
    /// `isFlat`.
    is_flat: bool,
    /// `lastDeathLocation` — Java `Optional<GlobalPos>`.
    last_death_location: Option<GlobalPos>,
    /// `portalCooldown`.
    portal_cooldown: i32,
    /// `seaLevel`.
    sea_level: i32,
}

impl CommonPlayerSpawnInfo {
    /// The record's canonical constructor — `CommonPlayerSpawnInfo.of(...)` in
    /// Java (a record's positional all-args constructor).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dimension_type: Holder<DimensionType>,
        dimension: ResourceKey<Level>,
        seed: i64,
        game_type: GameType,
        previous_game_type: Option<GameType>,
        is_debug: bool,
        is_flat: bool,
        last_death_location: Option<GlobalPos>,
        portal_cooldown: i32,
        sea_level: i32,
    ) -> Self {
        CommonPlayerSpawnInfo {
            dimension_type,
            dimension,
            seed,
            game_type,
            previous_game_type,
            is_debug,
            is_flat,
            last_death_location,
            portal_cooldown,
            sea_level,
        }
    }

    /// `CommonPlayerSpawnInfo.dimensionType()`.
    pub fn dimension_type(&self) -> &Holder<DimensionType> {
        &self.dimension_type
    }

    /// `CommonPlayerSpawnInfo.dimension()`.
    pub fn dimension(&self) -> &ResourceKey<Level> {
        &self.dimension
    }

    /// `CommonPlayerSpawnInfo.seed()`.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// `CommonPlayerSpawnInfo.gameType()`.
    pub fn game_type(&self) -> GameType {
        self.game_type
    }

    /// `CommonPlayerSpawnInfo.previousGameType()` — the `@Nullable` game type
    /// (Java null ⇄ Rust `None`).
    pub fn previous_game_type(&self) -> Option<GameType> {
        self.previous_game_type
    }

    /// `CommonPlayerSpawnInfo.isDebug()`.
    pub fn is_debug(&self) -> bool {
        self.is_debug
    }

    /// `CommonPlayerSpawnInfo.isFlat()`.
    pub fn is_flat(&self) -> bool {
        self.is_flat
    }

    /// `CommonPlayerSpawnInfo.lastDeathLocation()`.
    pub fn last_death_location(&self) -> Option<&GlobalPos> {
        self.last_death_location.as_ref()
    }

    /// `CommonPlayerSpawnInfo.portalCooldown()`.
    pub fn portal_cooldown(&self) -> i32 {
        self.portal_cooldown
    }

    /// `CommonPlayerSpawnInfo.seaLevel()`.
    pub fn sea_level(&self) -> i32 {
        self.sea_level
    }

    /// The wire codec — `CommonPlayerSpawnInfo(RegistryFriendlyByteBuf)` +
    /// `write(RegistryFriendlyByteBuf)` via `of`, in the Java field order.
    ///
    /// Not a `Packet`, so there is no `PacketType` registration; the codec is
    /// `StreamCodec<RegistryFriendlyByteBuf, _>` because `dimensionType`
    /// resolves through the `RegistryAccess` the buffer carries.
    pub fn stream_codec() -> StreamCodec<RegistryFriendlyByteBuf, CommonPlayerSpawnInfo> {
        of(
            |output: &mut RegistryFriendlyByteBuf, value: &CommonPlayerSpawnInfo| {
                dimension_type_stream_codec().encode(output, &value.dimension_type)?;
                output.write_resource_key(&value.dimension);
                output.write_long(value.seed);
                // `writeByte(getId())` — the low 8 bits of the int id.
                output.write_byte(value.game_type.get_id() as i8);
                // `writeByte(getNullableId(previousGameType))` — null -> -1.
                output.write_byte(GameType::get_nullable_id(value.previous_game_type) as i8);
                output.write_boolean(value.is_debug);
                output.write_boolean(value.is_flat);
                output.write_optional(
                    value.last_death_location.as_ref(),
                    RegistryFriendlyByteBuf::write_global_pos,
                );
                output.write_var_int(value.portal_cooldown);
                output.write_var_int(value.sea_level);
                Ok(())
            },
            |input: &mut RegistryFriendlyByteBuf| {
                let dimension_type = dimension_type_stream_codec().decode(input)?;
                let dimension = input.read_resource_key(&*registries::DIMENSION);
                let seed = input.read_long();
                let game_type = GameType::by_id(input.read_byte() as i32);
                let previous_game_type = GameType::by_nullable_id(input.read_byte() as i32);
                let is_debug = input.read_boolean();
                let is_flat = input.read_boolean();
                let last_death_location =
                    input.read_optional(RegistryFriendlyByteBuf::read_global_pos);
                let portal_cooldown = input.read_var_int();
                let sea_level = input.read_var_int();
                Ok(CommonPlayerSpawnInfo {
                    dimension_type,
                    dimension,
                    seed,
                    game_type,
                    previous_game_type,
                    is_debug,
                    is_flat,
                    last_death_location,
                    portal_cooldown,
                    sea_level,
                })
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use rivet_registry::core::BlockPos;
    use rivet_registry::{
        HolderGetter, Identifier, RegistrationInfo, RegistryAccess, RegistryBuilder,
    };
    use std::panic::catch_unwind;

    /// `{overworld: 0, the_nether: 1}` DIMENSION_TYPE registry + access.
    fn dimension_type_access() -> RegistryAccess {
        let mut builder = RegistryBuilder::new(&*registries::DIMENSION_TYPE);
        builder.register(
            &ResourceKey::create(
                &*registries::DIMENSION_TYPE,
                Identifier::with_default_namespace("overworld"),
            ),
            std::sync::Arc::new(DimensionType),
            RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &ResourceKey::create(
                &*registries::DIMENSION_TYPE,
                Identifier::with_default_namespace("the_nether"),
            ),
            std::sync::Arc::new(DimensionType),
            RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        RegistryAccess::from_single_registry((*registries::DIMENSION_TYPE).clone(), registry)
    }

    fn overworld_key() -> ResourceKey<Level> {
        ResourceKey::create(
            &*registries::DIMENSION,
            Identifier::with_default_namespace("overworld"),
        )
    }

    /// A full record: every field present, exercising all wire forms.
    fn full_record(access: &RegistryAccess) -> CommonPlayerSpawnInfo {
        let holder = access
            .lookup(&*registries::DIMENSION_TYPE)
            .unwrap()
            .get(&ResourceKey::create(
                &*registries::DIMENSION_TYPE,
                Identifier::with_default_namespace("the_nether"),
            ))
            .unwrap();
        CommonPlayerSpawnInfo::new(
            holder,
            overworld_key(),
            -6320987654321,
            GameType::Adventure,
            Some(GameType::Spectator),
            true,
            false,
            Some(GlobalPos::of(overworld_key(), BlockPos::new(10, 64, -20))),
            7,
            63,
        )
    }

    /// The exact wire bytes for `full_record` (Java-grounded field order).
    fn full_record_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        // dimensionType: varint holder id 1 (the_nether).
        bytes.push(1);
        // dimension: identifier string "minecraft:overworld" (19 chars).
        bytes.push(19);
        bytes.extend_from_slice(b"minecraft:overworld");
        // seed: big-endian i64.
        bytes.extend_from_slice(&(-6320987654321i64).to_be_bytes());
        // gameType: signed byte id 2 (adventure).
        bytes.push(2);
        // previousGameType: signed byte id 3 (spectator).
        bytes.push(3);
        // isDebug: true, isFlat: false.
        bytes.push(1);
        bytes.push(0);
        // lastDeathLocation present: boolean 1, identifier string, packed long.
        bytes.push(1);
        bytes.push(19);
        bytes.extend_from_slice(b"minecraft:overworld");
        bytes.extend_from_slice(&BlockPos::new(10, 64, -20).as_long().to_be_bytes());
        // portalCooldown: varint 7; seaLevel: varint 63.
        bytes.push(7);
        bytes.push(63);
        bytes
    }

    fn buffer(access: &RegistryAccess) -> RegistryFriendlyByteBuf {
        RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone())
    }

    fn decode(
        access: &RegistryAccess,
        bytes: &[u8],
    ) -> Result<CommonPlayerSpawnInfo, crate::codec::CodecError> {
        let mut input =
            RegistryFriendlyByteBuf::new(BytesMut::from(bytes.to_vec().as_slice()), access.clone());
        CommonPlayerSpawnInfo::stream_codec().decode(&mut input)
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
    fn golden_wire_bytes_and_round_trip() {
        let access = dimension_type_access();
        let record = full_record(&access);
        let mut out = buffer(&access);
        CommonPlayerSpawnInfo::stream_codec()
            .encode(&mut out, &record)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes, full_record_bytes());
        assert_eq!(decode(&access, &bytes).unwrap(), record);
    }

    #[test]
    fn previous_game_type_none_and_no_last_death_location() {
        let access = dimension_type_access();
        let registry = access.lookup(&*registries::DIMENSION_TYPE).unwrap();
        let record = CommonPlayerSpawnInfo::new(
            Holder::Reference {
                registry: registry.registry_id(),
                id: 0,
            },
            overworld_key(),
            0,
            GameType::Survival,
            None,
            false,
            false,
            None,
            0,
            0,
        );
        let mut out = buffer(&access);
        CommonPlayerSpawnInfo::stream_codec()
            .encode(&mut out, &record)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        // Exact wire: holder id 0, "minecraft:overworld" identifier, seed 0,
        // gameType 0, previous null -> byte -1 (0xFF), isDebug/isFlat false,
        // lastDeathLocation absent, portal/sea 0.
        let mut expected = vec![0, 19];
        expected.extend_from_slice(b"minecraft:overworld");
        expected.extend_from_slice(&0i64.to_be_bytes());
        expected.extend_from_slice(&[0, 0xFF, 0, 0, 0, 0, 0]);
        assert_eq!(bytes, expected);
        assert_eq!(decode(&access, &bytes).unwrap(), record);
    }

    #[test]
    fn out_of_range_game_type_byte_falls_back_to_survival() {
        // `GameType.byId(input.readByte())` — a signed byte outside 0..3
        // (including negative) maps to the ZERO-fallback SURVIVAL.
        let access = dimension_type_access();
        for bad_byte in [0xFFu8, 0x04, 0x80] {
            // A minimal valid prefix with the two game-type bytes set.
            let mut bytes = vec![0u8, 19];
            bytes.extend_from_slice(b"minecraft:overworld");
            bytes.extend_from_slice(&0i64.to_be_bytes());
            bytes.push(bad_byte); // gameType (signed, out of range)
            bytes.push(0xFF); // previousGameType -> null (-1)
            // isDebug, isFlat, lastDeathLocation absent, portalCooldown, seaLevel.
            bytes.extend_from_slice(&[0, 0, 0, 0, 0]);
            let mut input =
                RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), access.clone());
            let record = CommonPlayerSpawnInfo::stream_codec()
                .decode(&mut input)
                .unwrap();
            assert_eq!(record.game_type(), GameType::Survival);
            assert_eq!(record.previous_game_type(), None);
        }
    }

    #[test]
    fn out_of_range_game_type_bytes_collapse_on_reencode() {
        // Java `GameType.byId(input.readByte())`/`byNullableId` map any byte
        // outside 0..3 to the ZERO-fallback SURVIVAL, and the re-encode writes
        // `getId(SURVIVAL) == 0`. So a decode -> encode round-trip is a
        // deliberate non-identity for out-of-range ids — exactly Java's
        // `CommonPlayerSpawnInfo` decode-ctor + `write` behavior, not a
        // byte-preservation contract. A byte `4` in both slots decodes to
        // `Some(Survival)` (never `None`: only `-1` is null) and re-encodes as
        // `0`.
        let access = dimension_type_access();
        let mut bytes = vec![0u8, 19];
        bytes.extend_from_slice(b"minecraft:overworld");
        bytes.extend_from_slice(&0i64.to_be_bytes());
        bytes.push(4); // gameType: signed byte 4 (out of range)
        bytes.push(4); // previousGameType: signed byte 4 (out of range, not -1)
        // isDebug, isFlat, lastDeathLocation absent, portalCooldown, seaLevel.
        bytes.extend_from_slice(&[0, 0, 0, 0, 0]);
        let record = decode(&access, &bytes).unwrap();
        assert_eq!(record.game_type(), GameType::Survival);
        assert_eq!(record.previous_game_type(), Some(GameType::Survival));

        // Re-encode: both slots collapse to byte 0 (`getId(SURVIVAL)`).
        let mut out = buffer(&access);
        CommonPlayerSpawnInfo::stream_codec()
            .encode(&mut out, &record)
            .unwrap();
        let reencoded = out.into_inner().to_vec();
        let mut expected = vec![0u8, 19];
        expected.extend_from_slice(b"minecraft:overworld");
        expected.extend_from_slice(&0i64.to_be_bytes());
        expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(reencoded, expected);
    }

    #[test]
    fn dimension_type_unknown_holder_id_panics() {
        // `holderRegistry(DIMENSION_TYPE)` is strict-bounds: an out-of-range
        // varint id panics "No value with id {id}".
        let access = dimension_type_access();
        let mut input = buffer(&access);
        input.write_var_int(99);
        let msg = panic_message(|| {
            let _ = CommonPlayerSpawnInfo::stream_codec().decode(&mut input);
        });
        assert_eq!(msg, "No value with id 99");
    }

    #[test]
    fn dimension_type_holder_id_resolves_through_the_full_codec() {
        // Flip the wire holder id to another in-range value and verify the
        // composed `stream_codec()` resolves the registry entry the id names
        // (the encoder neither hard-codes the holder id nor re-resolves the
        // wrong entry). `overworld` is id 0, `the_nether` id 1.
        let access = dimension_type_access();
        let registry = access.lookup(&*registries::DIMENSION_TYPE).unwrap();
        let registry_id = registry.registry_id();
        let overworld = Holder::Reference {
            registry: registry_id,
            id: 0,
        };
        let the_nether = Holder::Reference {
            registry: registry_id,
            id: 1,
        };

        let record = full_record(&access); // dimensionType = the_nether (id 1).
        assert_eq!(record.dimension_type(), &the_nether);
        let mut out = buffer(&access);
        CommonPlayerSpawnInfo::stream_codec()
            .encode(&mut out, &record)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes[0], 1);
        assert_eq!(
            decode(&access, &bytes).unwrap().dimension_type(),
            &the_nether
        );

        // Same record with the wire holder id flipped to 0 decodes to overworld.
        let mut mutated = bytes;
        mutated[0] = 0;
        assert_eq!(
            decode(&access, &mutated).unwrap().dimension_type(),
            &overworld
        );
    }

    #[test]
    fn every_truncated_prefix_panics() {
        // The `read_*` primitives panic on insufficient bytes (netty's EOF
        // contract, established in `friendly_byte_buf`); every field consumes
        // at least one byte, so no proper prefix of a valid record may decode
        // into a partial record. This is the negative half of the codec's
        // shared-foundation contract for the #87 join wave.
        let access = dimension_type_access();
        let full = full_record_bytes();
        for len in 0..full.len() {
            let mut input =
                RegistryFriendlyByteBuf::new(BytesMut::from(&full[..len]), access.clone());
            let msg = panic_message(|| {
                let _ = CommonPlayerSpawnInfo::stream_codec().decode(&mut input);
            });
            assert!(!msg.is_empty(), "prefix of len {len} did not panic");
        }
        // The complete record still decodes to the source record.
        assert_eq!(decode(&access, &full).unwrap(), full_record(&access));
    }
}
