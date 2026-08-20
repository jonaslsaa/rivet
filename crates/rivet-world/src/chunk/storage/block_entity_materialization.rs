//! Pure materialization of reconstructed serialized block entities into wire
//! `BlockEntityInfo` values (issue #520).
//!
//! `SerializableChunkData.read`'s `postLoadChunk` turns unpacked serialized
//! block entities into live `BlockEntity`s; the send path later reads those
//! live entities back through `BlockEntityInfo.create`, which calls
//! `getUpdateTag` + Paper's `sanitizeSentNbt`. This layer models that
//! load → update-tag transform as a pure data transform over the already
//! computed [`SerializedBlockEntityOutcome`]s, with no live entity, no world
//! mutation, and no save writes. It is the #341 materialization boundary; the
//! #516 server send path consumes it — the server `LevelChunk` derives the
//! outcomes from the chunk's pending-map authority and feeds them here for
//! packet materialization (#537).
//!
//! Authority (#537): `ChunkAccess.pending_block_entities` — an
//! insertion-ordered, position-keyed `IndexMap<BlockPos, CompoundTag>` — is the
//! single runtime source of truth for loaded block entities. Reconstruction
//! installs the serialized tags straight into that map (duplicate corrected
//! positions collapse last-wins in place, first-insertion order for the
//! survivors) and retains no `block_entities`/`block_entity_outcomes` snapshot
//! Vecs; runtime mutators update the map, and the derived outcome/
//! materialization path reads it per call. This materializer deliberately takes
//! an immutable outcome slice and produces owned wire values, so it neither
//! creates nor cements any duplicate mutable ownership — the caller owns the
//! authority, and the outcomes it feeds here are derived from that authority,
//! not from any second snapshot.
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
//!   `saveAdditional`, which calls base `BlockEntity.saveAdditional` (writes
//!   `PublicBukkitValues` when the persistent-data container is non-empty) and
//!   then `BaseSpawner.save`; `sanitizeSentNbt` strips `PublicBukkitValues`
//!   before the wire (BlockEntityInfo.create), so the net tag is exactly
//!   `BaseSpawner.save` minus `SpawnPotentials`. The materialization models the
//!   `BaseSpawner.load` → `BaseSpawner.save` round trip from the serialized
//!   tag, including Paper's int variants and `Short.MAX_VALUE` clamps, then
//!   drops `SpawnPotentials` exactly like `getUpdateTag`.
//!
//! Every type that DOES override `getUpdateTag` with a non-empty tag (piston,
//! sign, hanging_sign, banner, beacon, structure block, end gateway, jigsaw,
//! campfire, shelf, trial spawner, vault, test block, test instance block) is
//! refused loudly as [`BlockEntityMaterializeError::UnsupportedUpdateTag`] —
//! the port never fabricates a client tag from a serialized payload whose live
//! subclass is not ported. Five overriders whose tag can be EMPTY depending on
//! the loaded state (skull, conduit, decorated pot, brushable block, creaking
//! heart) materialize a null-tag entry when empty — exactly Paper's
//! `tag.isEmpty() ? null : tag` — and are refused when non-empty. The refusal
//! sets are the exact pinned-Paper override set (minus mob_spawner, which is
//! ported), so every other resolved type materializes the null tag Paper sends.
//!
//! ## Refusals (typed, never silent)
//!
//! - [`BlockEntityMaterializeError::Pending`] — a `keepPacked`/proto entry has
//!   no unpacked data to materialize.
//! - [`BlockEntityMaterializeError::InvalidType`] — an absent/malformed/unknown
//!   `id` surfaced by reconstruction.
//! - [`BlockEntityMaterializeError::UnsupportedUpdateTag`] — a resolved type
//!   whose `getUpdateTag` override is not ported (the unconditional override
//!   set, or a conditional overrider whose tag is non-empty).
//!
//! A malformed spawner `SpawnData` is NOT an entry refusal: Java's
//! `BaseSpawner.load` reads it through `TagValueInput.read`, which reports the
//! `SpawnData.CODEC` decode problem and continues. What the `save` path then
//! writes depends on whether the codec error carried a partial value: an absent
//! or non-compound `entity` produces an error with no partial, so `nextSpawnData`
//! stays null and `save` omits only `SpawnData` (the seven numeric fields still
//! materialize — Paper sends that partial update tag to the client); a present
//! `custom_spawn_rules`/`equipment` that is not a compound also fails the decode,
//! but the error retains an entity-only partial `SpawnData`, so `save` writes it
//! with only the malformed optional key removed. The port mirrors both
//! observable outputs and surfaces every outer-field drop as a distinct
//! [`BlockEntityMaterializeDiagnostic`]. The outer shape of each `SpawnData`
//! codec field is validated like Paper's non-lenient fields — `entity` is a
//! required compound; a present `custom_spawn_rules`/`equipment` must be a
//! compound or it is dropped from the carried `SpawnData`. Unknown top-level
//! fields are ignored by the record codec and therefore disappear on re-encode.
//! The retained compounds are re-encoded at the codec boundary: malformed
//! light-limit shapes use omitted defaults, out-of-range limits drop the
//! optional field, malformed equipment map entries are omitted while valid
//! partial entries survive, and all-slot equal chances collapse to the scalar
//! form.
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
use rivet_nbt::float_tag::FloatTag;
use rivet_nbt::int_tag::IntTag;
use rivet_nbt::tag::Tag;
use rivet_protocol::protocol::game::level_chunk_packet_data::BlockEntityInfo;
use rivet_registry::Identifier;
use rivet_registry::core::{BlockPos, SectionPos};
use rivet_serialization::float_format::java_float_equals;

const SPAWNER_TYPE: &str = "minecraft:mob_spawner";

/// The pinned-Paper set of `BlockEntityType` values whose live subclass
/// overrides `getUpdateTag` with a tag the port does not reproduce
/// (`mob_spawner` is ported and excluded). Every other resolved type inherits
/// the base empty `getUpdateTag` and materializes a null tag, exactly like
/// Paper's `tag.isEmpty() ? null : tag`.
///
/// Derived from the pinned Paper 26.2 sources: `SignBlockEntity` (also
/// inherited by `HangingSignBlockEntity`), `BannerBlockEntity`,
/// `BeaconBlockEntity`, `StructureBlockEntity`, `TheEndGatewayBlockEntity`,
/// `JigsawBlockEntity`, `CampfireBlockEntity`, `ShelfBlockEntity`,
/// `TrialSpawnerBlockEntity`, `vault.VaultBlockEntity`, `TestBlockEntity`,
/// `TestInstanceBlockEntity`, and `PistonMovingBlockEntity`
/// (`world/level/block/piston`, registered as `minecraft:piston`).
/// [`CONDITIONAL_UPDATE_TAG_TYPES`] holds the overriders whose tag can be empty
/// (materialized null) or non-empty (refused) depending on the loaded state.
///
/// RivetTodo(#520): re-audit these sets when the generated registry is
/// regenerated — a newly added type whose subclass overrides `getUpdateTag`
/// must join one of the sets to stay loud instead of silently sending a null
/// tag.
const UNSUPPORTED_UPDATE_TAG_TYPES: &[&str] = &[
    "minecraft:piston",
    "minecraft:sign",
    "minecraft:hanging_sign",
    "minecraft:banner",
    "minecraft:beacon",
    "minecraft:structure_block",
    "minecraft:end_gateway",
    "minecraft:jigsaw",
    "minecraft:campfire",
    "minecraft:shelf",
    "minecraft:trial_spawner",
    "minecraft:vault",
    "minecraft:test_block",
    "minecraft:test_instance_block",
];

/// The `getUpdateTag` overriders whose tag can be EMPTY depending on their
/// loaded state. Paper's `tag.isEmpty() ? null : tag` then sends a null-tag
/// entry (present) — the port materializes that. When the override tag would
/// be non-empty (a state-carrying field is present in the serialized tag), the
/// port refuses loudly because it cannot reproduce the tag.
///
/// The emptiness is computable from the serialized raw tag because on the
/// chunk-load path each type's `loadAdditional` rebuilds the live state
/// exclusively from the raw top-level fields — `applyImplicitComponents` is
/// not invoked there, so a `components` compound only ever populates the
/// entity's component map, never its live fields — and the override's
/// conditional writes mirror exactly the raw fields' presence:
///
/// - `minecraft:skull` (`SkullBlockEntity`) — `saveCustomOnly` writes only the
///   nullable `profile`, `note_block_sound`, `custom_name`; empty when none
///   are present.
/// - `minecraft:conduit` (`ConduitBlockEntity`) — `saveCustomOnly` writes
///   `Target` only when a destroy target is present; empty when absent.
/// - `minecraft:decorated_pot` (`DecoratedPotBlockEntity`) — `getUpdateTag`
///   writes `sherds` only when not `PotDecorations.EMPTY` (Paper hides the
///   item); empty when `sherds` is absent.
/// - `minecraft:brushable_block` (`BrushableBlockEntity`) — `getUpdateTag`
///   writes the nullable `hit_direction` and `item` only when non-empty; empty
///   when both are absent.
/// - `minecraft:creaking_heart` (`CreakingHeartBlockEntity`) — `saveCustomOnly`
///   writes `creaking` only when a creaking is active; empty when absent.
///
/// `minecraft:trial_spawner` is NOT here: its override can also be empty (a
/// non-ACTIVE state with no `SpawnData`), but that emptiness depends on the
/// block-state `TrialSpawnerBlock.STATE`, which the serialized tag does not
/// carry — so it cannot be computed here and stays in
/// [`UNSUPPORTED_UPDATE_TAG_TYPES`] (refused loudly rather than risking a
/// silent wrong tag).
const CONDITIONAL_UPDATE_TAG_TYPES: &[&str] = &[
    "minecraft:skull",
    "minecraft:conduit",
    "minecraft:decorated_pot",
    "minecraft:brushable_block",
    "minecraft:creaking_heart",
];

/// Whether a [`CONDITIONAL_UPDATE_TAG_TYPES`] type's `getUpdateTag` override is
/// empty given the serialized tag (see the set's doc for the per-type
/// derivation). A present-but-malformed field loads to null in Paper (the
/// codec reports and drops it), which would make the override tag empty and
/// Paper send null; the port treats a present field as non-empty and refuses —
/// the conservative, never-fabricate direction, matching the module's other
/// malformed-field boundaries.
fn conditional_override_tag_is_empty(name: &str, raw: &CompoundTag) -> bool {
    match name {
        "minecraft:skull" => {
            raw.get("profile").is_none()
                && raw.get("note_block_sound").is_none()
                && raw.get("custom_name").is_none()
        }
        "minecraft:conduit" => raw.get("Target").is_none(),
        "minecraft:decorated_pot" => raw.get("sherds").is_none(),
        "minecraft:brushable_block" => {
            raw.get("hit_direction").is_none() && raw.get("item").is_none()
        }
        "minecraft:creaking_heart" => raw.get("creaking").is_none(),
        _ => false,
    }
}

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
    /// The resolved type's update tag is not ported; only mob_spawner has a
    /// ported non-empty `getUpdateTag` today — every other materializable type
    /// sends Paper's null tag.
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
    /// A spawner's whole `SpawnData` was dropped: `entity` is absent or not a
    /// compound, so the `SpawnData.CODEC` decode produces an error with no
    /// partial value. Paper's `TagValueInput.read` reports it and leaves
    /// `nextSpawnData` null; `BaseSpawner.save` then omits only the `SpawnData`
    /// key. The spawner's numeric fields still materialize.
    #[error("spawner block entity at {position} dropped malformed {field}")]
    SpawnDataDropped {
        position: BlockPos,
        field: &'static str,
    },
    /// A present-but-wrong-typed optional `SpawnData` field was dropped while
    /// the rest of the `SpawnData` was retained. Paper's `SpawnData.CODEC`
    /// decode reports the wrong-typed `custom_spawn_rules`/`equipment` but the
    /// `DataResult` keeps an entity-only partial value, so `TagValueInput.read`
    /// still yields a `SpawnData` and `BaseSpawner.save` writes it with only the
    /// malformed key removed. The port mirrors that: the dropped field is
    /// surfaced, never silent.
    #[error("spawner block entity at {position} dropped malformed {field} from carried SpawnData")]
    SpawnDataFieldDropped {
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
/// `BaseSpawner.save`-minus-`SpawnPotentials` tag; a [`CONDITIONAL_UPDATE_TAG_TYPES`]
/// override materializes null when its tag is empty (Paper sends the entry with
/// a null tag) and is refused when non-empty; the remaining
/// [`UNSUPPORTED_UPDATE_TAG_TYPES`] override `getUpdateTag` with a tag the port
/// cannot reproduce and are refused loudly.
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
    if CONDITIONAL_UPDATE_TAG_TYPES.contains(&name) {
        if conditional_override_tag_is_empty(name, &entry.raw_tag) {
            // The override tag is empty, so Paper's `tag.isEmpty() ? null : tag`
            // sends a null-tag entry — the entry is present with no tag.
            return Ok(BlockEntityInfo::new(
                packed_xz,
                y,
                entry.entity_type.clone(),
                None,
            ));
        }
        return Err(BlockEntityMaterializeError::UnsupportedUpdateTag {
            position: entry.position,
            entity_type: name.to_string(),
        });
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
/// `SpawnData` constructor's `entity.id` normalization applied. A `SpawnData`
/// whose `entity` is absent or not a compound is dropped exactly like Paper
/// (`TagValueInput.read` reports the `SpawnData.CODEC` problem, the error has no
/// partial value, `nextSpawnData` stays null and `BaseSpawner.save` omits only
/// the key) — surfaced as [`BlockEntityMaterializeDiagnostic::SpawnDataDropped`].
/// A present `custom_spawn_rules`/`equipment` that is not a compound also fails
/// the decode, but the retained partial value keeps the entity-only `SpawnData`
/// — Paper writes it with the malformed key removed — so the port drops only
/// that field from the carried compound and surfaces it as
/// [`BlockEntityMaterializeDiagnostic::SpawnDataFieldDropped`]. In both cases
/// the spawner's numeric fields still materialize.
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
            Ok((spawn_data, dropped)) => {
                for field in dropped {
                    diagnostics.push(BlockEntityMaterializeDiagnostic::SpawnDataFieldDropped {
                        position: entry.position,
                        field,
                    });
                }
                Some(spawn_data)
            }
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
    validate_spawn_potentials(raw);

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
/// `SpawnData.CODEC` requires the `entity` compound field and, when present,
/// compound `custom_spawn_rules`/`equipment` fields (all non-lenient at the
/// outer shape). A missing or wrong-typed `entity` fails the decode with no
/// partial value — Paper's `TagValueInput.read` reports it, `nextSpawnData`
/// stays null and `save` omits `SpawnData`; the port returns the field name for
/// the [`BlockEntityMaterializeDiagnostic::SpawnDataDropped`] surface.
/// A present `custom_spawn_rules`/`equipment` that is not a compound also fails
/// the decode, but the retained partial keeps an entity-only `SpawnData` that
/// Paper writes with the malformed key removed — the port drops only that key
/// from the carried compound and returns it for the
/// [`BlockEntityMaterializeDiagnostic::SpawnDataFieldDropped`] surface.
/// Otherwise the compound is carried through the codec's canonical re-encode:
/// unknown top-level fields are dropped, light-limit intervals use their
/// canonical form, malformed equipment map entries are omitted while valid
/// partial entries survive, and the `SpawnData` record constructor normalizes
/// `entity.id` (valid id rewritten canonically, invalid/absent/non-string id
/// removed).
fn load_spawn_data(
    spawn_data: &CompoundTag,
) -> Result<(CompoundTag, Vec<&'static str>), &'static str> {
    let Some(entity) = spawn_data.get_compound("entity").cloned() else {
        return Err("SpawnData.entity");
    };
    let mut out = CompoundTag::new();
    out.put("entity".to_string(), Tag::Compound(entity));
    let mut dropped = Vec::new();
    if let Some(value) = spawn_data.get("custom_spawn_rules") {
        if let Tag::Compound(rules) = value {
            match normalize_custom_spawn_rules(rules) {
                Ok(normalized) => {
                    out.put("custom_spawn_rules".to_string(), Tag::Compound(normalized));
                }
                Err(()) => dropped.push("SpawnData.custom_spawn_rules"),
            }
        } else {
            dropped.push("SpawnData.custom_spawn_rules");
        }
    }
    if let Some(value) = spawn_data.get("equipment") {
        if let Tag::Compound(equipment) = value {
            if let Some(normalized) = normalize_equipment(equipment) {
                out.put("equipment".to_string(), Tag::Compound(normalized));
            } else {
                dropped.push("SpawnData.equipment");
            }
        } else {
            dropped.push("SpawnData.equipment");
        }
    }
    normalize_spawn_data_entity_id(out.tags.get_mut("entity"));
    Ok((out, dropped))
}

fn validate_spawn_potentials(raw: &CompoundTag) {
    let Some(Tag::List(list)) = raw.get("SpawnPotentials") else {
        return;
    };
    let mut total = 0_i64;
    for entry in &list.list {
        let Tag::Compound(entry) = entry else {
            continue;
        };
        let Some(weight) = entry.get_int("weight").filter(|weight| *weight >= 0) else {
            continue;
        };
        let Some(data) = entry.get_compound("data") else {
            continue;
        };
        if load_spawn_data(data).is_err() {
            continue;
        }
        total += i64::from(weight);
        if total > i64::from(i32::MAX) {
            panic!("Sum of weights must be <= 2147483647");
        }
    }
}

fn normalize_custom_spawn_rules(rules: &CompoundTag) -> Result<CompoundTag, ()> {
    let mut normalized = CompoundTag::new();
    for field in ["block_light_limit", "sky_light_limit"] {
        if let Some(value) = rules.get(field)
            && let Some(value) = normalize_light_limit(value)?
        {
            normalized.put(field.to_string(), value);
        }
    }
    Ok(normalized)
}

fn normalize_light_limit(value: &Tag) -> Result<Option<Tag>, ()> {
    let bounds = if let Some(value) = value.as_int() {
        Some((value, value))
    } else if let Tag::List(list) = value {
        if list.list.len() == 2 {
            list.list[0].as_int().zip(list.list[1].as_int())
        } else {
            None
        }
    } else if let Tag::Compound(compound) = value {
        compound
            .get("min_inclusive")
            .and_then(Tag::as_int)
            .zip(compound.get("max_inclusive").and_then(Tag::as_int))
    } else {
        None
    };
    let Some((min, max)) = bounds else {
        return Ok(None);
    };
    if !(0..=15).contains(&min) || !(0..=15).contains(&max) || min > max {
        return Err(());
    }
    if min == 0 && max == 15 {
        return Ok(None);
    }
    if min == max {
        Ok(Some(Tag::Int(IntTag::value_of(min))))
    } else {
        let mut range = CompoundTag::new();
        range.put_int("min_inclusive", min);
        range.put_int("max_inclusive", max);
        Ok(Some(Tag::Compound(range)))
    }
}

fn normalize_equipment(equipment: &CompoundTag) -> Option<CompoundTag> {
    let loot_table = equipment.get_string("loot_table")?;
    let identifier = Identifier::try_parse_result(loot_table).ok()??;
    let slot_drop_chances = match equipment.get("slot_drop_chances") {
        Some(value) => normalize_slot_drop_chances(value).ok()?,
        None => None,
    };
    let mut normalized = CompoundTag::new();
    normalized.put_string("loot_table", &identifier.to_string());
    if let Some(slot_drop_chances) = slot_drop_chances {
        normalized.put("slot_drop_chances".to_string(), slot_drop_chances);
    }
    Some(normalized)
}

fn normalize_slot_drop_chances(value: &Tag) -> Result<Option<Tag>, ()> {
    if let Some(chance) = value.as_float() {
        return Ok(Some(Tag::Float(FloatTag::value_of(chance))));
    }
    let Tag::Compound(chances) = value else {
        return Err(());
    };
    const SLOTS: [&str; 8] = [
        "mainhand", "offhand", "feet", "legs", "chest", "head", "body", "saddle",
    ];
    let mut normalized = CompoundTag::new();
    let mut values = Vec::with_capacity(chances.size());
    for (slot, chance) in chances.entry_set() {
        if !SLOTS.contains(&slot.as_str()) {
            continue;
        }
        let Some(chance) = chance.as_float() else {
            continue;
        };
        values.push(chance);
        normalized.put(slot.clone(), Tag::Float(FloatTag::value_of(chance)));
    }
    let Some(first) = values.first().copied() else {
        return Ok(None);
    };
    if values.len() == SLOTS.len() && values.iter().all(|value| java_float_equals(*value, first)) {
        Ok(Some(Tag::Float(FloatTag::value_of(first))))
    } else {
        Ok(Some(Tag::Compound(normalized)))
    }
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
    fn spawner_non_compound_custom_spawn_rules_keeps_entity_only_spawn_data() {
        let mut tag = spawner_tag();
        let mut spawn_data = tag
            .get_compound("SpawnData")
            .expect("spawn data present")
            .clone();
        spawn_data.put_int("custom_spawn_rules", 1);
        tag.tags
            .insert("SpawnData".to_string(), Tag::Compound(spawn_data));
        let (info, diagnostics) = materialize_entry(&resolved_outcome(tag));
        // Paper's SpawnData.CODEC optionalFieldOf("custom_spawn_rules") is
        // non-lenient: a present-but-non-compound value fails the decode, but
        // the retained partial keeps an entity-only SpawnData that BaseSpawner
        // writes with the malformed key removed.
        let info = info.expect("spawner still materializes partial");
        let carried = info
            .tag()
            .unwrap()
            .get_compound("SpawnData")
            .expect("SpawnData carried");
        assert!(carried.get("custom_spawn_rules").is_none());
        assert_eq!(
            carried
                .get_compound("entity")
                .and_then(|e| e.get_string("id"))
                .map(String::as_str),
            Some("minecraft:pig")
        );
        assert_eq!(
            diagnostics,
            vec![BlockEntityMaterializeDiagnostic::SpawnDataFieldDropped {
                position: BlockPos::new(3, 64, 11),
                field: "SpawnData.custom_spawn_rules",
            }]
        );
    }

    #[test]
    fn spawner_non_compound_equipment_keeps_entity_only_spawn_data() {
        let mut tag = spawner_tag();
        let mut spawn_data = tag
            .get_compound("SpawnData")
            .expect("spawn data present")
            .clone();
        spawn_data.put_int("equipment", 1);
        tag.tags
            .insert("SpawnData".to_string(), Tag::Compound(spawn_data));
        let (info, diagnostics) = materialize_entry(&resolved_outcome(tag));
        // Same non-lenient optionalFieldOf("equipment") semantics: the entity-only
        // partial SpawnData is retained and written with the malformed key removed.
        let info = info.expect("spawner still materializes partial");
        let carried = info
            .tag()
            .unwrap()
            .get_compound("SpawnData")
            .expect("SpawnData carried");
        assert!(carried.get("equipment").is_none());
        assert_eq!(
            carried
                .get_compound("entity")
                .and_then(|e| e.get_string("id"))
                .map(String::as_str),
            Some("minecraft:pig")
        );
        assert_eq!(
            diagnostics,
            vec![BlockEntityMaterializeDiagnostic::SpawnDataFieldDropped {
                position: BlockPos::new(3, 64, 11),
                field: "SpawnData.equipment",
            }]
        );
    }

    #[test]
    fn spawner_both_optional_wrong_typed_keep_entity_only_and_report_in_codec_order() {
        let mut tag = spawner_tag();
        let mut spawn_data = tag
            .get_compound("SpawnData")
            .expect("spawn data present")
            .clone();
        spawn_data.put_int("custom_spawn_rules", 1);
        spawn_data.put_string("equipment", "nope");
        tag.tags
            .insert("SpawnData".to_string(), Tag::Compound(spawn_data));
        let (info, diagnostics) = materialize_entry(&resolved_outcome(tag));
        // Both optional fields fail the decode, but the retained partial keeps
        // the entity-only SpawnData with both malformed keys removed. The
        // diagnostics surface in the codec field order.
        let info = info.expect("spawner still materializes partial");
        let carried = info
            .tag()
            .unwrap()
            .get_compound("SpawnData")
            .expect("SpawnData carried");
        assert!(carried.get("custom_spawn_rules").is_none());
        assert!(carried.get("equipment").is_none());
        assert_eq!(
            carried
                .get_compound("entity")
                .and_then(|e| e.get_string("id"))
                .map(String::as_str),
            Some("minecraft:pig")
        );
        assert_eq!(
            diagnostics,
            vec![
                BlockEntityMaterializeDiagnostic::SpawnDataFieldDropped {
                    position: BlockPos::new(3, 64, 11),
                    field: "SpawnData.custom_spawn_rules",
                },
                BlockEntityMaterializeDiagnostic::SpawnDataFieldDropped {
                    position: BlockPos::new(3, 64, 11),
                    field: "SpawnData.equipment",
                },
            ]
        );
    }

    #[test]
    fn spawner_nested_optional_values_are_codec_normalized() {
        let mut tag = spawner_tag();
        let mut spawn_data = tag
            .get_compound("SpawnData")
            .expect("spawn data present")
            .clone();
        let mut rules = CompoundTag::new();
        rules.put_string("block_light_limit", "malformed");
        spawn_data.put("custom_spawn_rules".to_string(), Tag::Compound(rules));
        spawn_data.put("equipment".to_string(), Tag::Compound(CompoundTag::new()));
        tag.tags
            .insert("SpawnData".to_string(), Tag::Compound(spawn_data));

        let (info, diagnostics) = materialize_entry(&resolved_outcome(tag));
        let info = info.expect("spawner still materializes partial");
        let carried = info
            .tag()
            .unwrap()
            .get_compound("SpawnData")
            .expect("SpawnData carried");
        assert_eq!(
            carried.get_compound("custom_spawn_rules"),
            Some(&CompoundTag::new())
        );
        assert!(carried.get("equipment").is_none());
        assert_eq!(
            diagnostics,
            vec![BlockEntityMaterializeDiagnostic::SpawnDataFieldDropped {
                position: BlockPos::new(3, 64, 11),
                field: "SpawnData.equipment",
            }]
        );
    }

    #[test]
    fn spawner_out_of_range_custom_rules_are_dropped_as_a_partial_field() {
        let mut tag = spawner_tag();
        let mut spawn_data = tag
            .get_compound("SpawnData")
            .expect("spawn data present")
            .clone();
        let mut rules = CompoundTag::new();
        rules.put_int("block_light_limit", 3);
        rules.put_int("sky_light_limit", 20);
        spawn_data.put("custom_spawn_rules".to_string(), Tag::Compound(rules));
        spawn_data.put("equipment".to_string(), Tag::Compound(CompoundTag::new()));
        tag.tags
            .insert("SpawnData".to_string(), Tag::Compound(spawn_data));

        let (info, diagnostics) = materialize_entry(&resolved_outcome(tag));
        let carried = info
            .expect("spawner still materializes partial")
            .tag()
            .unwrap()
            .get_compound("SpawnData")
            .cloned()
            .expect("SpawnData carried");
        assert!(carried.get("custom_spawn_rules").is_none());
        assert!(carried.get("equipment").is_none());
        assert_eq!(
            diagnostics,
            vec![
                BlockEntityMaterializeDiagnostic::SpawnDataFieldDropped {
                    position: BlockPos::new(3, 64, 11),
                    field: "SpawnData.custom_spawn_rules",
                },
                BlockEntityMaterializeDiagnostic::SpawnDataFieldDropped {
                    position: BlockPos::new(3, 64, 11),
                    field: "SpawnData.equipment",
                },
            ]
        );
    }

    #[test]
    fn spawner_equal_slot_drop_chances_use_the_scalar_codec_form() {
        let mut tag = spawner_tag();
        let mut spawn_data = tag
            .get_compound("SpawnData")
            .expect("spawn data present")
            .clone();
        let mut equipment = CompoundTag::new();
        equipment.put_string("loot_table", "minecraft:chest");
        let mut chances = CompoundTag::new();
        for slot in [
            "mainhand", "offhand", "feet", "legs", "chest", "head", "body", "saddle",
        ] {
            chances.put_float(slot, f32::from_bits(0x7fc0_1234));
        }
        equipment.put("slot_drop_chances".to_string(), Tag::Compound(chances));
        spawn_data.put("equipment".to_string(), Tag::Compound(equipment));
        tag.tags
            .insert("SpawnData".to_string(), Tag::Compound(spawn_data));

        let (info, diagnostics) = materialize_entry(&resolved_outcome(tag));
        assert!(diagnostics.is_empty());
        let info = info.expect("spawner materializes");
        let equipment = info
            .tag()
            .unwrap()
            .get_compound("SpawnData")
            .unwrap()
            .get_compound("equipment")
            .expect("equipment retained");
        let Some(Tag::Float(chance)) = equipment.get("slot_drop_chances") else {
            panic!("equal slot chances should use the scalar form")
        };
        assert_eq!(chance.value.to_bits(), 0x7fc0_1234);
    }

    #[test]
    fn spawner_equipment_keeps_valid_partial_slot_drop_chances() {
        let mut tag = spawner_tag();
        let mut spawn_data = tag
            .get_compound("SpawnData")
            .expect("spawn data present")
            .clone();
        let mut equipment = CompoundTag::new();
        equipment.put_string("loot_table", "minecraft:chest");
        let mut chances = CompoundTag::new();
        chances.put_float("mainhand", 0.5);
        chances.put_string("offhand", "malformed");
        chances.put_float("bogus", 0.75);
        equipment.put("slot_drop_chances".to_string(), Tag::Compound(chances));
        spawn_data.put("equipment".to_string(), Tag::Compound(equipment));
        tag.tags
            .insert("SpawnData".to_string(), Tag::Compound(spawn_data));

        let (info, diagnostics) = materialize_entry(&resolved_outcome(tag));
        assert!(diagnostics.is_empty());
        let equipment = info
            .expect("spawner materializes")
            .tag()
            .unwrap()
            .get_compound("SpawnData")
            .unwrap()
            .get_compound("equipment")
            .cloned()
            .expect("equipment retained");
        let chances = equipment
            .get_compound("slot_drop_chances")
            .expect("valid partial chance map retained");
        assert_eq!(chances.get_float("mainhand"), Some(0.5));
        assert!(chances.get("offhand").is_none());
        assert!(chances.get("bogus").is_none());
    }

    #[test]
    fn spawner_unknown_spawn_data_fields_are_dropped_on_reencode() {
        let mut tag = spawner_tag();
        let mut spawn_data = tag
            .get_compound("SpawnData")
            .expect("spawn data present")
            .clone();
        spawn_data.put_int("unknown_spawn_data_field", 9);
        tag.tags
            .insert("SpawnData".to_string(), Tag::Compound(spawn_data));

        let (info, diagnostics) = materialize_entry(&resolved_outcome(tag));
        assert!(diagnostics.is_empty());
        let spawn_data = info
            .expect("spawner materializes")
            .tag()
            .unwrap()
            .get_compound("SpawnData")
            .cloned()
            .expect("SpawnData retained");
        assert!(spawn_data.get("unknown_spawn_data_field").is_none());
        assert!(spawn_data.get_compound("entity").is_some());
    }

    #[test]
    #[should_panic(expected = "Sum of weights must be <= 2147483647")]
    fn spawn_potential_overflow_is_rejected_during_block_entity_materialization() {
        let mut tag = spawner_tag();
        let mut potentials = ListTag::new();
        for weight in [i32::MAX, 1] {
            let mut potential = CompoundTag::new();
            potential.put_int("weight", weight);
            let mut data = CompoundTag::new();
            data.put("entity".to_string(), Tag::Compound(CompoundTag::new()));
            potential.put("data".to_string(), Tag::Compound(data));
            potentials.list.push(Tag::Compound(potential));
        }
        tag.put("SpawnPotentials".to_string(), Tag::List(potentials));
        let _ = materialize_entry(&resolved_outcome(tag));
    }

    #[test]
    fn spawner_valid_compound_optional_kept_while_wrong_typed_other_is_dropped() {
        let mut tag = spawner_tag();
        let mut spawn_data = tag
            .get_compound("SpawnData")
            .expect("spawn data present")
            .clone();
        let mut rules = CompoundTag::new();
        rules.put_int("block_light_limit", 0);
        rules.put_int("sky_light_limit", 15);
        spawn_data.put("custom_spawn_rules".to_string(), Tag::Compound(rules));
        spawn_data.put_string("equipment", "nope");
        tag.tags
            .insert("SpawnData".to_string(), Tag::Compound(spawn_data));
        let (info, diagnostics) = materialize_entry(&resolved_outcome(tag));
        // A well-formed compound optional is carried verbatim; only the
        // wrong-typed optional is dropped from the retained SpawnData.
        let info = info.expect("spawner still materializes partial");
        let carried = info
            .tag()
            .unwrap()
            .get_compound("SpawnData")
            .expect("SpawnData carried");
        assert!(carried.get_compound("custom_spawn_rules").is_some());
        assert!(carried.get("equipment").is_none());
        assert_eq!(
            diagnostics,
            vec![BlockEntityMaterializeDiagnostic::SpawnDataFieldDropped {
                position: BlockPos::new(3, 64, 11),
                field: "SpawnData.equipment",
            }]
        );
    }

    #[test]
    fn spawner_compound_custom_spawn_rules_and_equipment_are_carried_verbatim() {
        // Well-formed compound values for the optional codec fields are the
        // codec's stored form and are carried through. The inner-field
        // re-encode normalization (light-range defaults, slot_drop_chances
        // collapsing) defers with the SpawnData/EquipmentTable codec port.
        let mut tag = spawner_tag();
        let mut spawn_data = tag
            .get_compound("SpawnData")
            .expect("spawn data present")
            .clone();
        let mut rules = CompoundTag::new();
        rules.put_int("block_light_limit", 0);
        rules.put_int("sky_light_limit", 15);
        spawn_data.put("custom_spawn_rules".to_string(), Tag::Compound(rules));
        let mut equipment = CompoundTag::new();
        equipment.put_string("loot_table", "minecraft:chest");
        spawn_data.put("equipment".to_string(), Tag::Compound(equipment));
        tag.tags
            .insert("SpawnData".to_string(), Tag::Compound(spawn_data));
        let (info, diagnostics) = materialize_entry(&resolved_outcome(tag));
        assert!(diagnostics.is_empty());
        let info = info.expect("spawner materializes");
        let out = info.tag().unwrap().get_compound("SpawnData").unwrap();
        assert!(out.get_compound("custom_spawn_rules").is_some());
        assert!(out.get_compound("equipment").is_some());
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
    /// or inherited) minus `mob_spawner`, split into the unconditional
    /// non-empty-override types and the conditional empty-capable overriders.
    /// Written out from the Java source audit, NOT derived from the production
    /// constants, so a misclassification in `UNSUPPORTED_UPDATE_TAG_TYPES` or
    /// `CONDITIONAL_UPDATE_TAG_TYPES` fails the tests instead of mirroring the
    /// bug.
    const EXPECTED_UNSUPPORTED: &[&str] = &[
        "minecraft:piston",
        "minecraft:sign",
        "minecraft:hanging_sign",
        "minecraft:banner",
        "minecraft:beacon",
        "minecraft:structure_block",
        "minecraft:end_gateway",
        "minecraft:jigsaw",
        "minecraft:campfire",
        "minecraft:shelf",
        "minecraft:trial_spawner",
        "minecraft:vault",
        "minecraft:test_block",
        "minecraft:test_instance_block",
    ];
    const EXPECTED_CONDITIONAL: &[&str] = &[
        "minecraft:skull",
        "minecraft:conduit",
        "minecraft:decorated_pot",
        "minecraft:brushable_block",
        "minecraft:creaking_heart",
    ];

    #[test]
    fn update_tag_sets_match_the_pinned_paper_override_audit() {
        // The production constants must exactly match the independently-pinned
        // Java audit sets.
        let mut constant = UNSUPPORTED_UPDATE_TAG_TYPES.to_vec();
        constant.sort_unstable();
        let mut expected = EXPECTED_UNSUPPORTED.to_vec();
        expected.sort_unstable();
        assert_eq!(constant, expected);

        let mut conditional = CONDITIONAL_UPDATE_TAG_TYPES.to_vec();
        conditional.sort_unstable();
        let mut expected_conditional = EXPECTED_CONDITIONAL.to_vec();
        expected_conditional.sort_unstable();
        assert_eq!(conditional, expected_conditional);
    }

    #[test]
    fn conditional_overriders_materialize_null_when_empty_and_refuse_when_not() {
        // A conditional overrider with no state-carrying field loads to an
        // empty getUpdateTag, so Paper sends a null-tag entry (present).
        for name in EXPECTED_CONDITIONAL {
            let entry = resolved_outcome(block_entity(name, 1, 64, 1));
            let (result, diagnostics) = materialize_entry(&entry);
            assert!(diagnostics.is_empty(), "{name}");
            let info = result.unwrap_or_else(|e| panic!("{name} empty override -> null: {e}"));
            assert!(info.tag().is_none(), "{name} empty override sends null");
        }

        // Each conditional overrider's state-carrying field makes the tag
        // non-empty, so the entry is refused loudly (the tag is not ported).
        let non_empty = [
            ("minecraft:skull", "profile"),
            ("minecraft:conduit", "Target"),
            ("minecraft:decorated_pot", "sherds"),
            ("minecraft:brushable_block", "item"),
            ("minecraft:creaking_heart", "creaking"),
        ];
        for (name, field) in non_empty {
            let mut tag = block_entity(name, 1, 64, 1);
            tag.put_string(field, "state");
            let entry = resolved_outcome(tag);
            let (result, diagnostics) = materialize_entry(&entry);
            assert!(diagnostics.is_empty(), "{name}");
            assert_eq!(
                result.unwrap_err(),
                BlockEntityMaterializeError::UnsupportedUpdateTag {
                    position: BlockPos::new(1, 64, 1),
                    entity_type: name.to_string(),
                },
                "{name} with {field} must be refused"
            );
        }
    }

    #[test]
    fn every_generated_type_is_classified_faithfully() {
        // Pin the Paper-faithful classification across the whole generated
        // registry: mob_spawner is ported, the unconditional overriders are
        // refused loudly, the conditional overriders materialize null (their
        // loop tag carries no state field), and every other type materializes
        // the base null tag. The refusal sets are checked against the
        // independently-pinned audit, not the production constants.
        let access = BlockEntityType::built_in_registry_access();
        let registry = access.lookup(&BLOCK_ENTITY_TYPE).unwrap();
        let mut unsupported_seen = Vec::new();
        let mut conditional_seen = Vec::new();
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
            } else if EXPECTED_CONDITIONAL.contains(name) {
                // The loop's bare tag carries no state field, so the override
                // tag is empty and Paper sends a null-tag entry (present).
                let info = result.unwrap_or_else(|e| panic!("{name} should materialize null: {e}"));
                assert!(info.tag().is_none(), "{name} empty override sends null");
                conditional_seen.push(*name);
            } else {
                let info = result.unwrap_or_else(|e| panic!("{name} should materialize null: {e}"));
                assert!(info.tag().is_none(), "{name} must send the base null tag");
                null_seen.push(*name);
            }
        }
        assert_eq!(unsupported_seen.len(), EXPECTED_UNSUPPORTED.len());
        assert_eq!(conditional_seen.len(), EXPECTED_CONDITIONAL.len());
        assert!(!null_seen.is_empty());
        assert_eq!(
            null_seen.len() + unsupported_seen.len() + conditional_seen.len() + 1,
            BLOCK_ENTITY_TYPE_BY_ID.len()
        );
    }

    #[test]
    fn materialized_infos_round_trip_through_the_wire_codec() {
        // The #520 boundary produces the exact `BlockEntityInfo` values the
        // active #516 send path encodes. Prove the materialized values survive
        // the real `BlockEntityInfo.STREAM_CODEC` encode -> decode round trip:
        // a tagged spawner and a null-tag chest both decode back with the same
        // packed position, absolute Y, canonical registry Arc, and tag.
        use bytes::BytesMut;
        use rivet_protocol::codec::{StreamDecoder, StreamEncoder};
        use rivet_protocol::protocol::game::level_chunk_packet_data::BlockEntityInfo;
        use rivet_protocol::registry_friendly_byte_buf::RegistryFriendlyByteBuf;

        let access = BlockEntityType::built_in_registry_access();

        let spawner = resolved_outcome(spawner_tag());
        let (spawner_info, spawner_diags) = materialize_entry(&spawner);
        assert!(spawner_diags.is_empty());
        let spawner_info = spawner_info.expect("spawner materializes");
        let spawner_tag_value = spawner_info.tag().expect("spawner tag is non-null").clone();

        let chest = resolved_outcome(block_entity("minecraft:chest", 1, 65, 1));
        let (chest_info, chest_diags) = materialize_entry(&chest);
        assert!(chest_diags.is_empty());
        let chest_info = chest_info.expect("chest materializes");

        // Encode both infos back-to-back in a single buffer (the packet writes
        // the whole block-entity list), then decode them in order.
        let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone());
        BlockEntityInfo::stream_codec()
            .encode(&mut out, &spawner_info)
            .unwrap();
        BlockEntityInfo::stream_codec()
            .encode(&mut out, &chest_info)
            .unwrap();
        let bytes = out.into_inner().to_vec();

        let mut input =
            RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), access.clone());
        let decoded_spawner = BlockEntityInfo::stream_codec().decode(&mut input).unwrap();
        let decoded_chest = BlockEntityInfo::stream_codec().decode(&mut input).unwrap();
        assert_eq!(input.readable_bytes(), 0);

        // The spawner decodes with identical packed position, absolute Y,
        // registry Arc identity (the canonical allocation), and tag.
        assert_eq!(decoded_spawner.packed_xz(), spawner_info.packed_xz());
        assert_eq!(decoded_spawner.y(), spawner_info.y());
        assert!(Arc::ptr_eq(
            decoded_spawner.entity_type(),
            spawner_info.entity_type()
        ));
        assert_eq!(decoded_spawner.tag(), Some(&spawner_tag_value));

        // The chest's null tag stays null through the wire.
        assert_eq!(decoded_chest.packed_xz(), 0x11);
        assert_eq!(decoded_chest.y(), 65);
        assert!(Arc::ptr_eq(
            decoded_chest.entity_type(),
            chest_info.entity_type()
        ));
        assert!(decoded_chest.tag().is_none());
    }
}
