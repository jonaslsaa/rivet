//! Pure materialization of reconstructed serialized block entities into wire
//! `BlockEntityInfo` values (issue #520).
//!
//! `SerializableChunkData.read`'s `postLoadChunk` turns unpacked serialized
//! block entities into live `BlockEntity`s; the send path later reads those
//! live entities back through `BlockEntityInfo.create`, which calls
//! `getUpdateTag` + Paper's `sanitizeSentNbt`. This layer models that
//! load → update-tag transform as a pure data transform over the already
//! computed [`SerializedBlockEntityOutcome`]s, with no live entity, no world
//! mutation, and no save writes. It is the #341 materialization boundary the
//! reconstruction carries the outcomes for; the active #516 server send path
//! consumes it but is deliberately not wired here.
//!
//! ## Paper-faithful mapping
//!
//! Java's `BlockEntityInfo.create` (Paper 26.2, `ClientboundLevelChunkPacketData`):
//!
//! ```java
//! CompoundTag tag = blockEntity.getUpdateTag(blockEntity.getLevel().registryAccess());
//! int xz = SectionPos.sectionRelative(pos.getX()) << 4 | SectionPos.sectionRelative(pos.getZ());
//! blockEntity.sanitizeSentNbt(tag); // removes "PublicBukkitValues"
//! return new BlockEntityInfo(xz, pos.getY(), blockEntity.getType(), tag.isEmpty() ? null : tag);
//! ```
//!
//! - `packed_xz = ((x & 15) << 4) | (z & 15)`, truncated to a signed byte by
//!   the wire `writeByte` (a local X/Z of 15,15 packs to the `-1` byte).
//! - `y` is the absolute block Y truncated to a `short`.
//! - the `type` is the registry-resolved `Arc<BlockEntityType>` (the
//!   [`ResolvedSerializedBlockEntity`] carries the canonical registry `Arc`, so
//!   the materialized info shares allocation identity with the codec registry).
//! - the `tag` is `None` exactly when Java's `tag.isEmpty() ? null : tag`
//!   yields null.
//!
//! The update tag's shape is decided by whether the live subclass overrides
//! `BlockEntity.getUpdateTag`:
//!
//! - **Base behavior (null tag)** — every subclass that does NOT override
//!   `getUpdateTag` inherits the base `new CompoundTag()`; after
//!   `sanitizeSentNbt` (removes `PublicBukkitValues`, a no-op on the empty tag)
//!   it is still empty, so `tag.isEmpty() ? null : tag` yields null. That covers
//!   the vast majority of types: all containers (chest, furnace, hopper,
//!   dispenser, dropper, barrel, shulker box, brewing stand, smoker, blast
//!   furnace, lectern, crafter, …) and the plain non-tag types (jukebox,
//!   enchanting table, end portal, daylight detector, comparator, command block,
//!   bell, beehive, sculk sensors/catalyst/shrieker, chiseled bookshelf,
//!   copper golem statue, potent sulfur). The serialized contents (items, loot
//!   table, components) are irrelevant to the client update tag.
//! - **mob_spawner** — `SpawnerBlockEntity.getUpdateTag` is
//!   `saveCustomOnly` minus `SpawnPotentials`. `saveCustomOnly` is
//!   `saveAdditional`, which for the spawner is just `BaseSpawner.save` (the
//!   base `BlockEntity.saveAdditional` is a no-op). The materialization models
//!   the `BaseSpawner.load` → `BaseSpawner.save` round trip from the serialized
//!   tag, including Paper's int variants and `Short.MAX_VALUE` clamps, then
//!   drops `SpawnPotentials` exactly like `getUpdateTag`.
//!
//! Every type that DOES override `getUpdateTag` with a non-empty tag (piston,
//! sign, hanging_sign, banner, skull, beacon, conduit, structure block, end
//! gateway, jigsaw, campfire, decorated pot, brushable block, creaking heart,
//! shelf, trial spawner, vault, test block, test instance block) is refused
//! loudly as [`BlockEntityMaterializeError::UnsupportedUpdateTag`] — the port
//! never fabricates a client tag from a serialized payload whose live subclass
//! is not ported. The refusal set is the exact pinned-Paper override set
//! (minus mob_spawner, which is ported), so every other resolved type
//! materializes the null tag Paper sends.
//!
//! ## Refusals (typed, never silent)
//!
//! - [`BlockEntityMaterializeError::Pending`] — a `keepPacked`/proto entry has
//!   no unpacked data to materialize.
//! - [`BlockEntityMaterializeError::InvalidType`] — an absent/malformed/unknown
//!   `id` surfaced by reconstruction.
//! - [`BlockEntityMaterializeError::UnsupportedUpdateTag`] — a resolved type
//!   whose `getUpdateTag` override is not ported (the override set above).
//!
//! A malformed spawner `SpawnData` is NOT an entry refusal: Java's
//! `BaseSpawner.load` reads it through `TagValueInput.read`, which reports the
//! `SpawnData.CODEC` decode problem and continues, leaving `nextSpawnData`
//! null. `save` then still writes the seven numeric fields and omits only
//! `SpawnData` — Paper sends that partial update tag to the client. The port
//! mirrors that observable output (the spawner materializes partial) and
//! surfaces the field drop as a [`BlockEntityMaterializeDiagnostic`] so it is
//! never silent.
//!
//! ## Ordering
//!
//! Java builds the chunk packet by iterating `LevelChunk.blockEntities` — a
//! fastutil `Object2ObjectOpenHashMap` — whose iteration order is
//! nondeterministic. Rivet preserves the serialized source order end to end as
//! a stable carry (the same divergence the `structures.References` note
//! records): the result vector is in the exact order of the input outcomes, so
//! an entry is never lost or reordered on this path. The order is a stable
//! carry, never a byte-order oracle.

use crate::chunk::storage::serializable_chunk_data::{
    BlockEntityTypeError, PendingBlockEntityReason, ResolvedSerializedBlockEntity,
    SerializedBlockEntityOutcome,
};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::tag::Tag;
use rivet_protocol::protocol::game::level_chunk_packet_data::BlockEntityInfo;
use rivet_registry::Identifier;
use rivet_registry::core::{BlockPos, SectionPos};

const SPAWNER_TYPE: &str = "minecraft:mob_spawner";

/// The pinned-Paper set of `BlockEntityType` values whose live subclass
/// overrides `getUpdateTag` with a non-empty tag and which the port does not
/// reproduce (`mob_spawner` is ported and excluded). Every other resolved type
/// inherits the base empty `getUpdateTag` and materializes a null tag, exactly
/// like Paper's `tag.isEmpty() ? null : tag`.
///
/// Derived from the pinned Paper 26.2 sources: `SignBlockEntity` (also
/// inherited by `HangingSignBlockEntity`), `BannerBlockEntity`,
/// `SkullBlockEntity`, `BeaconBlockEntity`, `ConduitBlockEntity`,
/// `StructureBlockEntity`, `TheEndGatewayBlockEntity`, `JigsawBlockEntity`,
/// `CampfireBlockEntity`, `DecoratedPotBlockEntity`, `BrushableBlockEntity`,
/// `CreakingHeartBlockEntity`, `ShelfBlockEntity`,
/// `TrialSpawnerBlockEntity`, `vault.VaultBlockEntity`, `TestBlockEntity`,
/// `TestInstanceBlockEntity`, and `PistonMovingBlockEntity`
/// (`world/level/block/piston`, registered as `minecraft:piston`).
///
/// RivetTodo(#520): re-audit this set when the generated registry is
/// regenerated — a newly added type whose subclass overrides `getUpdateTag`
/// must join this set to stay loud instead of silently sending a null tag.
const UNSUPPORTED_UPDATE_TAG_TYPES: &[&str] = &[
    "minecraft:piston",
    "minecraft:sign",
    "minecraft:hanging_sign",
    "minecraft:banner",
    "minecraft:skull",
    "minecraft:beacon",
    "minecraft:conduit",
    "minecraft:structure_block",
    "minecraft:end_gateway",
    "minecraft:jigsaw",
    "minecraft:campfire",
    "minecraft:decorated_pot",
    "minecraft:brushable_block",
    "minecraft:creaking_heart",
    "minecraft:shelf",
    "minecraft:trial_spawner",
    "minecraft:vault",
    "minecraft:test_block",
    "minecraft:test_instance_block",
];

/// Why a serialized block entity cannot be turned into a wire
/// [`BlockEntityInfo`]. Each variant keeps the corrected absolute position and
/// the specific reason so the caller can log and continue (the #338 value
/// stream pattern).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BlockEntityMaterializeError {
    /// The entry is opaque pending data (`keepPacked` or a proto chunk); there
    /// is no unpacked payload to materialize.
    #[error("block entity at {position} is pending ({reason:?}) and cannot be materialized")]
    Pending {
        position: BlockPos,
        reason: PendingBlockEntityReason,
    },
    /// The entry's `id` could not be resolved by reconstruction (absent,
    /// malformed, or unknown).
    #[error("block entity at {position} has an invalid type: {error:?}")]
    InvalidType {
        position: BlockPos,
        error: BlockEntityTypeError,
    },
    /// The resolved type's update tag is not ported; only chest and
    /// mob_spawner have a materializable `getUpdateTag` today.
    #[error("block entity type {entity_type} at {position} has no materialized update tag")]
    UnsupportedUpdateTag {
        position: BlockPos,
        entity_type: String,
    },
}

/// A recoverable, field-level drop during materialization. The enclosing entry
/// still materializes — Paper logs the field decode problem and continues — so
/// the diagnostic exists to keep the drop from being silent.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BlockEntityMaterializeDiagnostic {
    /// A spawner's `SpawnData` could not be decoded (absent `entity`,
    /// wrong-typed `entity`, or a non-compound `SpawnData`). Paper's
    /// `TagValueInput.read` reports the `SpawnData.CODEC` problem and leaves
    /// `nextSpawnData` null; `BaseSpawner.save` then omits only the `SpawnData`
    /// key. The spawner's numeric fields still materialize.
    #[error("spawner block entity at {position} dropped malformed {field}")]
    SpawnDataDropped {
        position: BlockPos,
        field: &'static str,
    },
}

/// The result of materializing one outcome list: per-entry wire infos in exact
/// source order, plus the recoverable field-level drops surfaced so they are
/// never silent.
#[derive(Clone, Debug)]
pub struct BlockEntityMaterialization {
    /// Per-outcome results in exact source order. `Err` entries are the
    /// entry-level refusals (pending, invalid type, unsupported type); `Ok`
    /// entries are the wire `BlockEntityInfo` values.
    pub infos: Vec<Result<BlockEntityInfo, BlockEntityMaterializeError>>,
    /// Recoverable field-level drops (Paper logs and continues) surfaced so
    /// the drop is never silent.
    pub diagnostics: Vec<BlockEntityMaterializeDiagnostic>,
}

/// Materialize every outcome in source order into wire [`BlockEntityInfo`]
/// values, preserving the input order exactly (see the module-level ordering
/// note). `Err` entries are the entry-level refusals; the enclosing
/// [`BlockEntityMaterialization::diagnostics`] carry the recoverable field-level
/// drops — the caller logs and continues, never inventing a fallback tag.
pub fn materialize_block_entities(
    outcomes: &[SerializedBlockEntityOutcome],
) -> BlockEntityMaterialization {
    let mut diagnostics = Vec::new();
    let infos = outcomes
        .iter()
        .map(|outcome| materialize_block_entity(outcome, &mut diagnostics))
        .collect();
    BlockEntityMaterialization { infos, diagnostics }
}

fn materialize_block_entity(
    outcome: &SerializedBlockEntityOutcome,
    diagnostics: &mut Vec<BlockEntityMaterializeDiagnostic>,
) -> Result<BlockEntityInfo, BlockEntityMaterializeError> {
    match outcome {
        SerializedBlockEntityOutcome::Pending(entry) => Err(BlockEntityMaterializeError::Pending {
            position: entry.position,
            reason: entry.reason,
        }),
        SerializedBlockEntityOutcome::InvalidUnpacked(entry) => {
            Err(BlockEntityMaterializeError::InvalidType {
                position: entry.position,
                error: entry.error.clone(),
            })
        }
        SerializedBlockEntityOutcome::ResolvedUnpacked(entry) => {
            materialize_resolved(entry, diagnostics)
        }
    }
}

/// `BlockEntityInfo.create` for a resolved unpacked entry.
///
/// `packed_xz` truncates Java's `sectionRelative(x) << 4 | sectionRelative(z)`
/// to the wire byte; `y` truncates the absolute Y to the wire short. The type
/// is the entry's canonical registry `Arc`, so encode resolves the same
/// registry id.
///
/// The tag follows `BlockEntity.getUpdateTag` faithfully: a subclass that does
/// not override it (the vast majority — all containers, chest included) yields
/// the empty base tag, so the wire tag is `None`; `mob_spawner` has the ported
/// `BaseSpawner.save`-minus-`SpawnPotentials` tag; the remaining
/// [`UNSUPPORTED_UPDATE_TAG_TYPES`] override `getUpdateTag` with a non-empty
/// tag the port cannot reproduce and are refused loudly.
fn materialize_resolved(
    entry: &ResolvedSerializedBlockEntity,
    diagnostics: &mut Vec<BlockEntityMaterializeDiagnostic>,
) -> Result<BlockEntityInfo, BlockEntityMaterializeError> {
    let packed_xz = ((SectionPos::section_relative(entry.position.get_x()) << 4)
        | SectionPos::section_relative(entry.position.get_z())) as i8;
    let y = entry.position.get_y() as i16;
    let name = entry.entity_type.name();
    if name == SPAWNER_TYPE {
        let tag = materialize_spawner_update_tag(entry, diagnostics);
        return Ok(BlockEntityInfo::new(
            packed_xz,
            y,
            entry.entity_type.clone(),
            Some(tag),
        ));
    }
    if UNSUPPORTED_UPDATE_TAG_TYPES.contains(&name) {
        return Err(BlockEntityMaterializeError::UnsupportedUpdateTag {
            position: entry.position,
            entity_type: name.to_string(),
        });
    }
    // Base `getUpdateTag` = new CompoundTag() -> empty -> null tag, for every
    // type whose subclass does not override it.
    Ok(BlockEntityInfo::new(
        packed_xz,
        y,
        entry.entity_type.clone(),
        None,
    ))
}

/// `SpawnerBlockEntity.getUpdateTag` — `saveCustomOnly` minus `SpawnPotentials`
/// — modeled from the serialized tag.
///
/// The serialized spawner's `BaseSpawner` fields are loaded with Paper's exact
/// `load` coercions (`getIntOr("Paper.Delay", getShortOr("Delay", 20))`, the
/// `Paper.*` int variants, the defaulted counts), then re-saved with Paper's
/// `save` clamps: the `Paper.*` int variants appear only when a delay exceeds
/// `Short.MAX_VALUE`, the legacy `Delay`/`MinSpawnDelay`/`MaxSpawnDelay` shorts
/// clamp to `Short.MAX_VALUE`, and the count/range shorts wrap like Java's
/// `(short)` cast. `SpawnPotentials` is deliberately dropped.
///
/// `SpawnData` is carried through its codec's stored form (a compound) with the
/// `SpawnData` constructor's `entity.id` normalization applied. A wrong-typed or
/// entity-less `SpawnData` is dropped exactly like Paper (`TagValueInput.read`
/// reports the `SpawnData.CODEC` problem and leaves `nextSpawnData` null, so
/// `BaseSpawner.save` omits only the key) — the drop is surfaced as a
/// [`BlockEntityMaterializeDiagnostic::SpawnDataDropped`] and the spawner's
/// numeric fields still materialize.
fn materialize_spawner_update_tag(
    entry: &ResolvedSerializedBlockEntity,
    diagnostics: &mut Vec<BlockEntityMaterializeDiagnostic>,
) -> CompoundTag {
    let raw = &entry.raw_tag;
    // BaseSpawner.load (Paper's int-first variants).
    let spawn_delay = raw.get_int_or("Paper.Delay", raw.get_short_or("Delay", 20) as i32);
    let min_spawn_delay =
        raw.get_int_or("Paper.MinSpawnDelay", raw.get_int_or("MinSpawnDelay", 200));
    let max_spawn_delay =
        raw.get_int_or("Paper.MaxSpawnDelay", raw.get_int_or("MaxSpawnDelay", 800));
    let spawn_count = raw.get_int_or("SpawnCount", 4);
    let max_nearby_entities = raw.get_int_or("MaxNearbyEntities", 6);
    let required_player_range = raw.get_int_or("RequiredPlayerRange", 16);
    let spawn_range = raw.get_int_or("SpawnRange", 4);

    let spawn_data = match raw.get("SpawnData") {
        None => None,
        Some(Tag::Compound(compound)) => match load_spawn_data(compound) {
            Ok(spawn_data) => Some(spawn_data),
            Err(field) => {
                diagnostics.push(BlockEntityMaterializeDiagnostic::SpawnDataDropped {
                    position: entry.position,
                    field,
                });
                None
            }
        },
        Some(_) => {
            diagnostics.push(BlockEntityMaterializeDiagnostic::SpawnDataDropped {
                position: entry.position,
                field: "SpawnData",
            });
            None
        }
    };

    // BaseSpawner.save (Paper's clamps), minus SpawnPotentials.
    let mut tag = CompoundTag::new();
    if spawn_delay > i16::MAX as i32 {
        tag.put_int("Paper.Delay", spawn_delay);
    }
    tag.put_short("Delay", spawn_delay.min(i16::MAX as i32) as i16);
    if min_spawn_delay > i16::MAX as i32 || max_spawn_delay > i16::MAX as i32 {
        tag.put_int("Paper.MinSpawnDelay", min_spawn_delay);
        tag.put_int("Paper.MaxSpawnDelay", max_spawn_delay);
    }
    tag.put_short("MinSpawnDelay", min_spawn_delay.min(i16::MAX as i32) as i16);
    tag.put_short("MaxSpawnDelay", max_spawn_delay.min(i16::MAX as i32) as i16);
    tag.put_short("SpawnCount", spawn_count as i16);
    tag.put_short("MaxNearbyEntities", max_nearby_entities as i16);
    tag.put_short("RequiredPlayerRange", required_player_range as i16);
    tag.put_short("SpawnRange", spawn_range as i16);
    if let Some(spawn_data) = spawn_data {
        tag.put("SpawnData".to_string(), Tag::Compound(spawn_data));
    }
    // SpawnPotentials is not written: SpawnerBlockEntity.getUpdateTag removes it.
    tag
}

/// Decode the stored `SpawnData` compound into the codec's re-encodable form.
///
/// `SpawnData.CODEC` requires the `entity` compound field; a missing or
/// wrong-typed `entity` fails the decode, and Paper's `TagValueInput.read`
/// reports it and leaves the field dropped (the port returns the field name for
/// the [`BlockEntityMaterializeDiagnostic::SpawnDataDropped`] surface).
/// Otherwise the compound is the codec's stored form and is carried through
/// with the `SpawnData` record constructor's `entity.id` normalization applied
/// (valid id rewritten canonically, invalid/absent/non-string id removed).
///
/// RivetTodo(#520): `SpawnData.CODEC`'s `optionalFieldOf` fields are
/// non-lenient — a present-but-wrong-typed `custom_spawn_rules` or a
/// malformed `equipment` also fails the whole `SpawnData` decode (dropping it
/// from the update tag), and a well-formed `custom_spawn_rules` re-encodes
/// with its two light ranges always stored (defaults filled for a missing
/// inner field). The port validates only the `entity` hard field and carries
/// the stored compound verbatim (with the id normalization), so those deeper
/// re-encode divergences defer with the `SpawnData`/`EquipmentTable` codec
/// port.
fn load_spawn_data(spawn_data: &CompoundTag) -> Result<CompoundTag, &'static str> {
    let Some(entity) = spawn_data.tags.get("entity") else {
        return Err("SpawnData.entity");
    };
    if !matches!(entity, Tag::Compound(_)) {
        return Err("SpawnData.entity");
    }
    let mut out = spawn_data.clone();
    normalize_spawn_data_entity_id(out.tags.get_mut("entity"));
    Ok(out)
}

/// The `SpawnData` compact constructor's `entity.id` normalization:
/// `entityToSpawn.read("id", Identifier.CODEC)` — a valid id is rewritten to
/// its canonical identifier text (defaulting the namespace), while an invalid,
/// absent, or non-string id is removed.
fn normalize_spawn_data_entity_id(entity: Option<&mut Tag>) {
    let Some(Tag::Compound(entity)) = entity else {
        return;
    };
    match entity.get_string("id") {
        Some(id) => match Identifier::try_parse_result(id) {
            Ok(Some(identifier)) => {
                entity.put_string("id", &identifier.to_string());
            }
            Ok(None) | Err(_) => {
                entity.remove("id");
            }
        },
        // Absent or non-string "id": Java's string-codec read fails/returns
        // empty, so the constructor removes the key (a no-op when absent).
        None => {
            entity.remove("id");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::storage::serializable_chunk_data::{
        BlockEntityChunkKind, BlockEntityTypeError, PendingBlockEntityReason,
        reconstruct_block_entities,
    };
    use rivet_nbt::int_tag::IntTag;
    use rivet_nbt::list_tag::ListTag;
    use rivet_nbt::nbt_accounter::NbtAccounter;
    use rivet_nbt::nbt_io;
    use rivet_registry::block_entity_type::BlockEntityType;
    use rivet_registry::core::ChunkPos;
    use rivet_registry::generated::block_entity_types::BLOCK_ENTITY_TYPE_BY_ID;
    use rivet_registry::registries::BLOCK_ENTITY_TYPE;
    use rivet_util::DataInputStream;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn load_fixture(path: PathBuf) -> CompoundTag {
        let bytes = std::fs::read(&path).expect("Paper 26.2 chunk fixture");
        let mut input = DataInputStream::new(Cursor::new(bytes));
        nbt_io::read(&mut input, &mut NbtAccounter::unlimited_heap()).expect("valid fixture")
    }

    /// The radius-1 loaded-world chest fixture (issue #371): one unpacked chest
    /// at (-299, -51, -321) with a loot table, components, and keepPacked byte 0.
    fn chest_fixture() -> CompoundTag {
        load_fixture(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/rivet-oracle/fixtures/loaded-world/chunk/-19.-21.nbt"),
        )
    }

    /// The committed block-entity fixture: a chest at (1,65,1) and a furnace at
    /// (2,65,1), both unpacked.
    fn block_entity_fixture() -> CompoundTag {
        load_fixture(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/rivet-oracle/fixtures/block-entities/chunk-0-0.nbt"),
        )
    }

    fn materialize_fixture(chunk: &CompoundTag, chunk_pos: ChunkPos) -> BlockEntityMaterialization {
        let raw = crate::chunk::storage::serializable_chunk_data::parse_block_entities(chunk);
        let outcomes = reconstruct_block_entities(&chunk_pos, &raw, BlockEntityChunkKind::Level);
        materialize_block_entities(&outcomes)
    }

    fn block_entity(id: &str, x: i32, y: i32, z: i32) -> CompoundTag {
        let mut tag = CompoundTag::new();
        tag.put_int("x", x);
        tag.put_int("y", y);
        tag.put_int("z", z);
        tag.put_string("id", id);
        tag
    }

    fn resolved_outcome(tag: CompoundTag) -> ResolvedSerializedBlockEntity {
        let outcomes =
            reconstruct_block_entities(&ChunkPos::ZERO, &[tag], BlockEntityChunkKind::Level);
        let SerializedBlockEntityOutcome::ResolvedUnpacked(entry) = &outcomes[0] else {
            panic!("expected a resolved outcome");
        };
        ResolvedSerializedBlockEntity {
            source_index: entry.source_index,
            position: entry.position,
            entity_type: entry.entity_type.clone(),
            raw_tag: entry.raw_tag.clone(),
        }
    }

    /// Materialize one resolved entry, returning its per-entry result and the
    /// recoverable field-level diagnostics it produced.
    fn materialize_entry(
        entry: &ResolvedSerializedBlockEntity,
    ) -> (
        Result<BlockEntityInfo, BlockEntityMaterializeError>,
        Vec<BlockEntityMaterializeDiagnostic>,
    ) {
        let mut diagnostics = Vec::new();
        let info = materialize_resolved(entry, &mut diagnostics);
        (info, diagnostics)
    }

    fn spawner_tag() -> CompoundTag {
        let mut tag = block_entity(SPAWNER_TYPE, 3, 64, -5);
        tag.put_short("Delay", 20);
        tag.put_short("MinSpawnDelay", 200);
        tag.put_short("MaxSpawnDelay", 800);
        tag.put_short("SpawnCount", 4);
        tag.put_short("MaxNearbyEntities", 6);
        tag.put_short("RequiredPlayerRange", 16);
        tag.put_short("SpawnRange", 4);
        let mut entity = CompoundTag::new();
        entity.put_string("id", "minecraft:pig");
        let mut spawn_data = CompoundTag::new();
        spawn_data.put("entity".to_string(), Tag::Compound(entity));
        let potential = spawn_data.clone();
        tag.put("SpawnData".to_string(), Tag::Compound(spawn_data));
        tag.put(
            "SpawnPotentials".to_string(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(potential)])),
        );
        tag
    }

    #[test]
    fn real_loaded_world_chest_materializes_to_null_tag() {
        let chunk = chest_fixture();
        let materialized = materialize_fixture(&chunk, ChunkPos::new(-19, -21));
        assert!(materialized.diagnostics.is_empty());
        let results = materialized.infos;
        assert_eq!(results.len(), 1);
        let info = results[0].as_ref().expect("fixture chest materializes");
        // (x & 15, z & 15) = (5, 15) => 0x5F; absolute y = -51; tag None.
        assert_eq!(info.packed_xz(), 0x5F);
        assert_eq!(info.y(), -51);
        assert_eq!(info.entity_type().name(), "minecraft:chest");
        assert!(info.tag().is_none());
        assert!(Arc::ptr_eq(
            info.entity_type(),
            &BlockEntityType::from_name("minecraft:chest").unwrap()
        ));
    }

    #[test]
    fn real_fixture_chest_and_furnace_materialize_with_null_tags() {
        // The committed fixture carries a chest and a furnace; neither subclass
        // overrides `getUpdateTag`, so Paper sends both with a null tag
        // (`new CompoundTag()` -> empty -> `tag.isEmpty() ? null : tag`).
        let chunk = block_entity_fixture();
        let materialized = materialize_fixture(&chunk, ChunkPos::ZERO);
        assert!(materialized.diagnostics.is_empty());
        let results = materialized.infos;
        assert_eq!(results.len(), 2);

        let chest = results[0].as_ref().expect("fixture chest materializes");
        assert_eq!(chest.packed_xz(), 0x11);
        assert_eq!(chest.y(), 65);
        assert_eq!(chest.entity_type().name(), "minecraft:chest");
        assert!(chest.tag().is_none());

        let furnace = results[1].as_ref().expect("fixture furnace materializes");
        assert_eq!(furnace.packed_xz(), 0x21);
        assert_eq!(furnace.y(), 65);
        assert_eq!(furnace.entity_type().name(), "minecraft:furnace");
        assert!(furnace.tag().is_none());
    }

    #[test]
    fn get_update_tag_overriding_types_are_unsupported_refusals() {
        // A resolved type that overrides `getUpdateTag` with a non-empty tag
        // (e.g. banner) cannot be reproduced and is refused loudly. A resolved
        // type that does not override it (e.g. hopper) materializes null.
        let banner = resolved_outcome(block_entity("minecraft:banner", 3, 64, 5));
        let (banner_result, banner_diags) = materialize_entry(&banner);
        assert!(banner_diags.is_empty());
        assert_eq!(
            banner_result.unwrap_err(),
            BlockEntityMaterializeError::UnsupportedUpdateTag {
                position: BlockPos::new(3, 64, 5),
                entity_type: "minecraft:banner".to_string(),
            }
        );

        let hopper = resolved_outcome(block_entity("minecraft:hopper", 4, 64, 6));
        let (hopper_result, hopper_diags) = materialize_entry(&hopper);
        assert!(hopper_diags.is_empty());
        let hopper = hopper_result.expect("hopper inherits the base null tag");
        assert_eq!(hopper.entity_type().name(), "minecraft:hopper");
        assert!(hopper.tag().is_none());
        assert_eq!(hopper.packed_xz(), 0x46);
    }

    #[test]
    fn synthetic_spawner_materializes_base_spawner_save_without_spawn_potentials() {
        let entry = resolved_outcome(spawner_tag());
        let (info, diagnostics) = materialize_entry(&entry);
        assert!(diagnostics.is_empty());
        let info = info.expect("spawner materializes");
        assert_eq!(info.packed_xz(), ((3 & 15) << 4 | (-5 & 15)) as i8);
        assert_eq!(info.y(), 64);
        assert_eq!(info.entity_type().name(), SPAWNER_TYPE);

        let tag = info.tag().expect("spawner update tag is non-empty");
        assert_eq!(tag.get_short_or("Delay", 0), 20);
        assert_eq!(tag.get_short_or("MinSpawnDelay", 0), 200);
        assert_eq!(tag.get_short_or("MaxSpawnDelay", 0), 800);
        assert_eq!(tag.get_short_or("SpawnCount", 0), 4);
        assert_eq!(tag.get_short_or("MaxNearbyEntities", 0), 6);
        assert_eq!(tag.get_short_or("RequiredPlayerRange", 0), 16);
        assert_eq!(tag.get_short_or("SpawnRange", 0), 4);
        assert!(tag.get("Paper.Delay").is_none());
        // SpawnPotentials is dropped by SpawnerBlockEntity.getUpdateTag.
        assert!(tag.get("SpawnPotentials").is_none());
        // The update tag is saveCustomOnly: no position/id metadata, no
        // PublicBukkitValues, no SpawnPotentials.
        assert!(tag.get("x").is_none());
        assert!(tag.get("id").is_none());
        assert!(tag.get("PublicBukkitValues").is_none());
        // SpawnData is carried through its codec stored form.
        let spawn_data = tag.get_compound("SpawnData").expect("SpawnData carried");
        assert_eq!(
            spawn_data
                .get_compound("entity")
                .and_then(|e| e.get_string("id"))
                .map(String::as_str),
            Some("minecraft:pig")
        );
    }

    #[test]
    fn spawner_large_delays_write_paper_int_variants_and_clamp_shorts() {
        let mut tag = block_entity(SPAWNER_TYPE, 3, 64, -5);
        tag.put_int("Paper.Delay", 40_000);
        tag.put_short("Delay", 20);
        tag.put_int("Paper.MinSpawnDelay", 40_000);
        tag.put_int("Paper.MaxSpawnDelay", 90_000);
        let entry = resolved_outcome(tag);
        let (info, diagnostics) = materialize_entry(&entry);
        assert!(diagnostics.is_empty());
        let info = info.expect("spawner materializes");
        let update = info.tag().expect("non-empty update tag");

        // BaseSpawner.load prefers the Paper int variant...
        assert_eq!(update.get_int_or("Paper.Delay", 0), 40_000);
        assert_eq!(update.get_int_or("Paper.MinSpawnDelay", 0), 40_000);
        assert_eq!(update.get_int_or("Paper.MaxSpawnDelay", 0), 90_000);
        // ...and save clamps the legacy shorts to Short.MAX_VALUE.
        assert_eq!(update.get_short_or("Delay", 0), i16::MAX);
        assert_eq!(update.get_short_or("MinSpawnDelay", 0), i16::MAX);
        assert_eq!(update.get_short_or("MaxSpawnDelay", 0), i16::MAX);
    }

    #[test]
    fn spawner_wrong_typed_spawn_data_drops_only_the_field_with_a_diagnostic() {
        let mut tag = block_entity(SPAWNER_TYPE, 3, 64, -5);
        tag.put_short("Delay", 20);
        tag.put("SpawnData".to_string(), Tag::Int(IntTag::value_of(5)));
        let entry = resolved_outcome(tag);
        let (info, diagnostics) = materialize_entry(&entry);
        // Paper sends the partial tag: numeric fields present, SpawnData omitted.
        let info = info.expect("spawner still materializes partial");
        assert_eq!(info.tag().unwrap().get_short_or("Delay", 0), 20);
        assert!(info.tag().unwrap().get("SpawnData").is_none());
        assert_eq!(
            diagnostics,
            vec![BlockEntityMaterializeDiagnostic::SpawnDataDropped {
                position: BlockPos::new(3, 64, 11),
                field: "SpawnData",
            }]
        );
    }

    #[test]
    fn spawner_entity_less_spawn_data_drops_only_the_field_with_a_diagnostic() {
        let mut tag = block_entity(SPAWNER_TYPE, 3, 64, -5);
        tag.put_short("Delay", 20);
        let mut spawn_data = CompoundTag::new();
        spawn_data.put_int("custom_spawn_rules", 1);
        tag.put("SpawnData".to_string(), Tag::Compound(spawn_data));
        let entry = resolved_outcome(tag);
        let (info, diagnostics) = materialize_entry(&entry);
        // Paper sends the partial tag; the entity-less SpawnData is omitted.
        let info = info.expect("spawner still materializes partial");
        assert_eq!(info.tag().unwrap().get_short_or("Delay", 0), 20);
        assert!(info.tag().unwrap().get("SpawnData").is_none());
        assert_eq!(
            diagnostics,
            vec![BlockEntityMaterializeDiagnostic::SpawnDataDropped {
                position: BlockPos::new(3, 64, 11),
                field: "SpawnData.entity",
            }]
        );
    }

    #[test]
    fn spawn_data_entity_id_is_normalized_canonically_or_removed() {
        // A default-namespace id is canonicalized to "minecraft:".
        let mut tag = spawner_tag();
        entity_owned_id(&mut tag, "pig");
        let (info, diagnostics) = materialize_entry(&resolved_outcome(tag));
        assert!(diagnostics.is_empty());
        let info = info.expect("spawner materializes");
        assert_eq!(
            info.tag()
                .unwrap()
                .get_compound("SpawnData")
                .and_then(|s| s.get_compound("entity"))
                .and_then(|e| e.get_string("id"))
                .map(String::as_str),
            Some("minecraft:pig")
        );

        // A malformed id is removed, matching the SpawnData constructor.
        let mut tag = spawner_tag();
        entity_owned_id(&mut tag, "not valid");
        let (info, diagnostics) = materialize_entry(&resolved_outcome(tag));
        assert!(diagnostics.is_empty());
        let info = info.expect("spawner materializes");
        assert!(
            info.tag()
                .unwrap()
                .get_compound("SpawnData")
                .and_then(|s| s.get_compound("entity"))
                .and_then(|e| e.get_string("id"))
                .is_none()
        );

        // A non-string id is removed too.
        let mut tag = spawner_tag();
        let spawn_data = tag
            .get_compound("SpawnData")
            .expect("spawn data present")
            .clone();
        let mut entity = spawn_data.get_compound("entity").unwrap().clone();
        entity.tags.shift_remove("id");
        entity
            .tags
            .insert("id".to_string(), Tag::Int(IntTag::value_of(7)));
        let mut fixed = spawn_data.clone();
        fixed
            .tags
            .insert("entity".to_string(), Tag::Compound(entity));
        tag.tags
            .insert("SpawnData".to_string(), Tag::Compound(fixed));
        let (info, diagnostics) = materialize_entry(&resolved_outcome(tag));
        assert!(diagnostics.is_empty());
        let info = info.expect("spawner materializes");
        assert!(
            info.tag()
                .unwrap()
                .get_compound("SpawnData")
                .and_then(|s| s.get_compound("entity"))
                .and_then(|e| e.get_string("id"))
                .is_none()
        );
    }

    /// Replace the `SpawnData.entity.id` string on a spawner tag.
    fn entity_owned_id(tag: &mut CompoundTag, id: &str) {
        let spawn_data = tag
            .get_compound("SpawnData")
            .expect("spawn data present")
            .clone();
        let mut entity = spawn_data
            .get_compound("entity")
            .expect("entity present")
            .clone();
        entity.put_string("id", id);
        let mut fixed = spawn_data;
        fixed
            .tags
            .insert("entity".to_string(), Tag::Compound(entity));
        tag.tags
            .insert("SpawnData".to_string(), Tag::Compound(fixed));
    }

    #[test]
    fn pending_and_invalid_and_unsupported_outcomes_are_typed_refusals() {
        let mut pending = block_entity("minecraft:chest", 1, 64, 1);
        pending.put_byte("keepPacked", 1);
        let mut invalid = block_entity("not valid", 2, 64, 2);
        invalid.put_short("Delay", 5);
        let banner = block_entity("minecraft:banner", 3, 64, 3);

        let outcomes = reconstruct_block_entities(
            &ChunkPos::new(2, -3),
            &[pending, invalid, banner],
            BlockEntityChunkKind::Level,
        );
        let materialized = materialize_block_entities(&outcomes);
        assert!(materialized.diagnostics.is_empty());
        let results = materialized.infos;
        assert_eq!(results.len(), 3);
        assert_eq!(
            results[0].as_ref().unwrap_err(),
            &BlockEntityMaterializeError::Pending {
                position: BlockPos::new(33, 64, -47),
                reason: PendingBlockEntityReason::KeepPacked,
            }
        );
        assert!(matches!(
            results[1].as_ref().unwrap_err(),
            BlockEntityMaterializeError::InvalidType {
                position,
                error: BlockEntityTypeError::MalformedId { value },
            } if *position == BlockPos::new(34, 64, -46) && value == "not valid"
        ));
        assert_eq!(
            results[2].as_ref().unwrap_err(),
            &BlockEntityMaterializeError::UnsupportedUpdateTag {
                position: BlockPos::new(35, 64, -45),
                entity_type: "minecraft:banner".to_string(),
            }
        );
    }

    #[test]
    fn proto_outcomes_are_pending_refusals() {
        let tag = block_entity("minecraft:chest", 1, 64, 1);
        let outcomes =
            reconstruct_block_entities(&ChunkPos::new(2, -3), &[tag], BlockEntityChunkKind::Proto);
        let materialized = materialize_block_entities(&outcomes);
        assert!(materialized.diagnostics.is_empty());
        let results = materialized.infos;
        assert_eq!(
            results[0].as_ref().unwrap_err(),
            &BlockEntityMaterializeError::Pending {
                position: BlockPos::new(33, 64, -47),
                reason: PendingBlockEntityReason::ProtoChunk,
            }
        );
    }

    #[test]
    fn materialize_preserves_source_order_through_failures() {
        let outcomes = reconstruct_block_entities(
            &ChunkPos::ZERO,
            &[
                block_entity("minecraft:chest", 1, 64, 1),
                block_entity("minecraft:banner", 2, 64, 2),
                {
                    let mut tag = block_entity(SPAWNER_TYPE, 3, 64, 3);
                    tag.put_short("Delay", 20);
                    tag
                },
                {
                    let mut tag = block_entity("minecraft:chest", 4, 64, 4);
                    tag.put_byte("keepPacked", 1);
                    tag
                },
            ],
            BlockEntityChunkKind::Level,
        );
        let materialized = materialize_block_entities(&outcomes);
        assert!(materialized.diagnostics.is_empty());
        let results = materialized.infos;
        assert_eq!(results.len(), 4);
        // Chest ok, banner unsupported, spawner ok, keepPacked pending.
        assert!(results[0].is_ok());
        assert!(matches!(
            results[1].as_ref().unwrap_err(),
            BlockEntityMaterializeError::UnsupportedUpdateTag { .. }
        ));
        assert!(results[2].is_ok());
        assert!(matches!(
            results[3].as_ref().unwrap_err(),
            BlockEntityMaterializeError::Pending { .. }
        ));
        // Order is preserved even though element 1 and 3 failed.
        assert_eq!(results[0].as_ref().unwrap().packed_xz(), 0x11);
        assert_eq!(results[2].as_ref().unwrap().packed_xz(), 0x33);
    }

    #[test]
    fn registry_arc_identity_is_preserved_through_materialization() {
        let entry = resolved_outcome(spawner_tag());
        let (info, diagnostics) = materialize_entry(&entry);
        assert!(diagnostics.is_empty());
        let info = info.expect("spawner materializes");
        let registered = BlockEntityType::from_name(SPAWNER_TYPE).unwrap();
        assert!(Arc::ptr_eq(info.entity_type(), &registered));
        // The codec registry resolves the same allocation identity.
        let access = BlockEntityType::built_in_registry_access();
        let registry = access.lookup(&BLOCK_ENTITY_TYPE).unwrap();
        assert_eq!(registry.get_id(info.entity_type()), 9);
    }

    /// The independently-pinned Paper 26.2 `getUpdateTag`-override set (direct
    /// or inherited), minus `mob_spawner` which the port reproduces. This is
    /// written out from the Java source audit, NOT derived from the production
    /// constant, so a misclassification in `UNSUPPORTED_UPDATE_TAG_TYPES` fails
    /// this test instead of mirroring the bug.
    const EXPECTED_UNSUPPORTED: &[&str] = &[
        "minecraft:piston",
        "minecraft:sign",
        "minecraft:hanging_sign",
        "minecraft:banner",
        "minecraft:skull",
        "minecraft:beacon",
        "minecraft:conduit",
        "minecraft:structure_block",
        "minecraft:end_gateway",
        "minecraft:jigsaw",
        "minecraft:campfire",
        "minecraft:decorated_pot",
        "minecraft:brushable_block",
        "minecraft:creaking_heart",
        "minecraft:shelf",
        "minecraft:trial_spawner",
        "minecraft:vault",
        "minecraft:test_block",
        "minecraft:test_instance_block",
    ];

    #[test]
    fn unsupported_update_tag_set_matches_the_pinned_paper_override_audit() {
        // The production constant must exactly match the independently-pinned
        // Java audit set (the constant's order is canonical by registry id).
        let mut constant = UNSUPPORTED_UPDATE_TAG_TYPES.to_vec();
        constant.sort_unstable();
        let mut expected = EXPECTED_UNSUPPORTED.to_vec();
        expected.sort_unstable();
        assert_eq!(constant, expected);
    }

    #[test]
    fn every_generated_type_is_classified_faithfully() {
        // Pin the Paper-faithful classification across the whole generated
        // registry: mob_spawner is ported, the getUpdateTag-overriding set is
        // refused loudly, and every other type materializes the base null tag.
        // The refusal set is checked against the independently-pinned
        // EXPECTED_UNSUPPORTED audit, not the production constant.
        let access = BlockEntityType::built_in_registry_access();
        let registry = access.lookup(&BLOCK_ENTITY_TYPE).unwrap();
        let mut unsupported_seen = Vec::new();
        let mut null_seen = Vec::new();
        for (id, name) in BLOCK_ENTITY_TYPE_BY_ID.iter().enumerate() {
            let entry = resolved_outcome(block_entity(name, id as i32, 64, id as i32));
            let (result, diagnostics) = materialize_entry(&entry);
            assert!(diagnostics.is_empty(), "{name}");
            if name == &SPAWNER_TYPE {
                let info = result.expect("mob_spawner materializes");
                assert_eq!(registry.get_id(info.entity_type()), id as i32);
                assert!(info.tag().is_some(), "mob_spawner sends its ported tag");
            } else if EXPECTED_UNSUPPORTED.contains(name) {
                assert!(
                    matches!(
                        result,
                        Err(BlockEntityMaterializeError::UnsupportedUpdateTag { .. })
                    ),
                    "{name} overrides getUpdateTag and must be refused"
                );
                unsupported_seen.push(*name);
            } else {
                let info = result.unwrap_or_else(|e| panic!("{name} should materialize null: {e}"));
                assert!(info.tag().is_none(), "{name} must send the base null tag");
                null_seen.push(*name);
            }
        }
        assert_eq!(unsupported_seen.len(), EXPECTED_UNSUPPORTED.len());
        assert!(!null_seen.is_empty());
        assert_eq!(
            null_seen.len() + unsupported_seen.len() + 1,
            BLOCK_ENTITY_TYPE_BY_ID.len()
        );
    }
}
