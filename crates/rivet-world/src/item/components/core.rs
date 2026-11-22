//! Core leaf component helpers:
//! - `CustomData` (`net.minecraft.world.item.component.CustomData`)
//! - `TypedEntityData<Id>` (`net.minecraft.world.item.component.TypedEntityData`)
//! - `Filterable<T>` (`net.minecraft.server.network.Filterable`)
//! - `HolderSetRef` (homogeneous list seam for `RegistryCodecs.homogeneousList`)

use crate::item::codec_from_fns;
use crate::item::components::{
    decode_compound_tag, decode_identifier, encode_compound_tag, encode_identifier, get_map_field,
};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_utils::compare_nbt;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;
use rivet_serialization::{Codec, DataResult, DynamicOps};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// CustomData
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.CustomData`.
///
/// Wraps an immutable `CompoundTag` snapshot; all mutating operations copy
/// before modification so shared component values remain side-effect free.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomData {
    tag: CompoundTag,
}

impl CustomData {
    /// `CustomData.EMPTY` — an empty compound tag.
    pub fn empty() -> Self {
        Self {
            tag: CompoundTag::new(),
        }
    }

    /// `CustomData.of(CompoundTag)` — stores a defensive copy of `tag`.
    pub fn of(tag: &CompoundTag) -> Self {
        Self { tag: tag.clone() }
    }

    /// `CustomData.copyTag()` — returns a defensive copy of the underlying tag.
    pub fn copy_tag(&self) -> CompoundTag {
        self.tag.clone()
    }

    /// Read-only view of the underlying `CompoundTag` (equivalent to `getUnsafe()`).
    pub fn tag(&self) -> &CompoundTag {
        &self.tag
    }

    /// `CustomData.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.tag.is_empty()
    }

    /// `CustomData.matchedBy(CompoundTag)` — returns `true` iff `expected` is
    /// a recursive subset of this tag (`NbtUtils.compareNbt(expected, tag, true)`).
    pub fn matched_by(&self, expected: &CompoundTag) -> bool {
        let exp = Tag::Compound(expected.clone());
        let act = Tag::Compound(self.tag.clone());
        compare_nbt(Some(&exp), Some(&act), true)
    }

    /// `CustomData.update(Consumer<CompoundTag>)` — clones the underlying tag,
    /// applies `f`, and returns the updated `CustomData`.
    pub fn update<F: FnOnce(&mut CompoundTag)>(&self, f: F) -> Self {
        let mut copy = self.tag.clone();
        f(&mut copy);
        Self { tag: copy }
    }
}

impl Default for CustomData {
    fn default() -> Self {
        Self::empty()
    }
}

/// `CustomData.CODEC` — encodes/decodes directly as an NBT compound (or map in JSON).
pub fn custom_data_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<CustomData, Ops>> {
    codec_from_fns(
        "custom_data",
        |v: &CustomData, ops: &Ops| DataResult::success(encode_compound_tag(&v.tag, ops)),
        |ops: &Ops, input: &Ops::Output| {
            decode_compound_tag(ops, input).map_owned(|tag| CustomData { tag })
        },
    )
}

// ---------------------------------------------------------------------------
// TypedEntityData<Id>
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.TypedEntityData<IdType>`.
///
/// Stores a typed entity or block-entity ID alongside an NBT `CompoundTag`
/// whose `"id"` entry has been stripped on construction and re-inserted on
/// serialization.
#[derive(Clone, Debug, PartialEq)]
pub struct TypedEntityData<Id> {
    id: Id,
    tag: CompoundTag,
}

impl<Id> TypedEntityData<Id> {
    /// `TypedEntityData.of(IdType, CompoundTag)` — strips any `"id"` key from a
    /// defensive copy of `tag`.
    pub fn of(id: Id, tag: &CompoundTag) -> Self {
        let mut stripped = tag.clone();
        stripped.remove("id");
        Self { id, tag: stripped }
    }

    /// `TypedEntityData.type()`.
    pub fn id(&self) -> &Id {
        &self.id
    }

    /// `TypedEntityData.copyTagWithoutId()`.
    pub fn copy_tag_without_id(&self) -> CompoundTag {
        self.tag.clone()
    }

    /// `TypedEntityData.getUnsafe()`.
    pub fn tag_without_id(&self) -> &CompoundTag {
        &self.tag
    }

    /// `TypedEntityData.contains(String)`.
    pub fn contains(&self, key: &str) -> bool {
        self.tag.contains(key)
    }

    /// `TypedEntityData.copyTagWithEntityId()` when `Id` can be converted to an
    /// `Identifier` string.
    pub fn copy_tag_with_id_string(&self, id_str: &str) -> CompoundTag {
        let mut out = self.tag.clone();
        out.put_string("id", id_str);
        out
    }
}

/// `TypedEntityData.codec(Codec<IdType>)`.
///
/// Decodes a compound, removes `"id"`, decodes the ID with `id_codec` (failing
/// if `"id"` is absent), and stores the remaining fields. Encoding copies the
/// tag and inserts the encoded `"id"`.
pub fn typed_entity_data_codec<Id, Ops>(
    id_codec: Arc<dyn Codec<Id, Ops>>,
) -> Arc<dyn Codec<TypedEntityData<Id>, Ops>>
where
    Id: Clone + 'static,
    Ops: DynamicOps + 'static,
{
    let enc_id = Arc::clone(&id_codec);
    let dec_id = id_codec;
    codec_from_fns(
        "typed_entity_data",
        move |v: &TypedEntityData<Id>, ops: &Ops| {
            let encoded_id = enc_id.encode_start(ops, &v.id);
            let id_out = match encoded_id.result() {
                Some(id_val) => id_val.clone(),
                None => {
                    return DataResult::error(
                        encoded_id
                            .error_ref()
                            .map(|e| e.message().to_string())
                            .unwrap_or_else(|| "Failed to encode TypedEntityData id".to_string()),
                    );
                }
            };
            let id_tag = ops.convert_to(&rivet_nbt::nbt_ops::NbtOps::instance(), &id_out);
            let mut copy = v.tag.clone();
            copy.put("id".to_string(), id_tag);
            DataResult::success(encode_compound_tag(&copy, ops))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let mut compound = match decode_compound_tag(ops, input).result() {
                Some(c) => c.clone(),
                None => return DataResult::error("TypedEntityData must be a compound tag"),
            };
            let Some(id_tag) = compound.remove("id") else {
                return DataResult::error("TypedEntityData is missing required 'id' field");
            };
            let id_in = rivet_nbt::nbt_ops::NbtOps::instance().convert_to(ops, &id_tag);
            let decoded_id = dec_id.parse(ops, &id_in);
            match decoded_id.result() {
                Some(id) => DataResult::success(TypedEntityData {
                    id: id.clone(),
                    tag: compound,
                }),
                None => DataResult::error(
                    decoded_id
                        .error_ref()
                        .map(|e| e.message().to_string())
                        .unwrap_or_else(|| "Invalid TypedEntityData 'id' field".to_string()),
                ),
            }
        },
    )
}

// ---------------------------------------------------------------------------
// Filterable<T>
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.server.network.Filterable<T>`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Filterable<T> {
    pub raw: T,
    pub filtered: Option<T>,
}

impl<T> Filterable<T> {
    /// `new Filterable<>(raw, filtered)`.
    pub const fn new(raw: T, filtered: Option<T>) -> Self {
        Self { raw, filtered }
    }

    /// `Filterable.passThrough(T)`.
    pub const fn pass_through(value: T) -> Self {
        Self {
            raw: value,
            filtered: None,
        }
    }

    /// `Filterable.get(boolean filterEnabled)` — returns `filtered` when
    /// `filter_enabled` is true and a filtered value exists, otherwise `raw`.
    pub fn get(&self, filter_enabled: bool) -> &T {
        if filter_enabled {
            self.filtered.as_ref().unwrap_or(&self.raw)
        } else {
            &self.raw
        }
    }

    /// `Filterable.map(Function<T, U>)`.
    pub fn map<U, F: FnMut(&T) -> U>(&self, mut f: F) -> Filterable<U> {
        Filterable {
            raw: f(&self.raw),
            filtered: self.filtered.as_ref().map(f),
        }
    }

    /// `Filterable.resolve(Function<T, Optional<U>>)`.
    pub fn resolve<U, F: FnMut(&T) -> Option<U>>(&self, mut f: F) -> Option<Filterable<U>> {
        let raw = f(&self.raw)?;
        let filtered = match &self.filtered {
            Some(v) => Some(f(v)?),
            None => None,
        };
        Some(Filterable { raw, filtered })
    }
}

/// Port of `Filterable.codec(Codec<T>)`.
///
/// Decodes either a full `{ "raw": T, "filtered"?: T }` map or a bare `T`
/// value (via `Codec.withAlternative`), matching Paper's `Filterable.codec`.
/// Encodes as `{ "raw": T, "filtered"?: T }` when `filtered` is `Some`, or as
/// the full map (`raw` only) matching `FULL_CODEC`.
pub fn filterable_codec<T, Ops>(inner: Arc<dyn Codec<T, Ops>>) -> Arc<dyn Codec<Filterable<T>, Ops>>
where
    T: Clone + 'static,
    Ops: DynamicOps + 'static,
{
    let enc_inner = Arc::clone(&inner);
    let dec_inner = inner;
    codec_from_fns(
        "filterable",
        move |v: &Filterable<T>, ops: &Ops| {
            let raw_res = enc_inner.encode_start(ops, &v.raw);
            let Some(raw_val) = raw_res.result().cloned() else {
                return DataResult::error(
                    raw_res
                        .error_ref()
                        .map(|e| e.message().to_string())
                        .unwrap_or_else(|| "Failed to encode Filterable.raw".to_string()),
                );
            };
            let mut entries = vec![rivet_serialization::Pair::of(
                ops.create_string("raw".to_string()),
                raw_val,
            )];
            if let Some(filtered) = &v.filtered {
                let filt_res = enc_inner.encode_start(ops, filtered);
                let Some(filt_val) = filt_res.result().cloned() else {
                    return DataResult::error(
                        filt_res
                            .error_ref()
                            .map(|e| e.message().to_string())
                            .unwrap_or_else(|| "Failed to encode Filterable.filtered".to_string()),
                    );
                };
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("filtered".to_string()),
                    filt_val,
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            // Try full map form `{ "raw": ..., "filtered"?: ... }` first.
            if let Some(map) = ops.get_map(input).result()
                && let Some(raw_in) = get_map_field(ops, map.as_ref(), "raw")
            {
                let raw_res = dec_inner.parse(ops, &raw_in);
                let Some(raw) = raw_res.result().cloned() else {
                    return DataResult::error(
                        raw_res
                            .error_ref()
                            .map(|e| e.message().to_string())
                            .unwrap_or_else(|| "Invalid Filterable.raw".to_string()),
                    );
                };
                let filtered = if let Some(filt_in) = get_map_field(ops, map.as_ref(), "filtered") {
                    let filt_res = dec_inner.parse(ops, &filt_in);
                    match filt_res.result().cloned() {
                        Some(f) => Some(f),
                        None => {
                            return DataResult::error(
                                filt_res
                                    .error_ref()
                                    .map(|e| e.message().to_string())
                                    .unwrap_or_else(|| "Invalid Filterable.filtered".to_string()),
                            );
                        }
                    }
                } else {
                    None
                };
                return DataResult::success(Filterable { raw, filtered });
            }
            // Fallback: bare value `T` -> `Filterable.passThrough(v)`.
            dec_inner
                .parse(ops, input)
                .map_owned(Filterable::pass_through)
        },
    )
}

// ---------------------------------------------------------------------------
// HolderSetRef (homogeneous list seam for RegistryCodecs.homogeneousList)
// ---------------------------------------------------------------------------

/// Serializable representation of a registry `HolderSet<T>` matching Paper's
/// `RegistryCodecs.homogeneousList`: either a `#tag` reference or an ordered
/// direct list of element `Identifier`s (which also decodes a single
/// `Identifier` string as a 1-element list).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HolderSetRef {
    Tag(Identifier),
    Direct(Vec<Identifier>),
}

impl HolderSetRef {
    pub fn tag(id: Identifier) -> Self {
        Self::Tag(id)
    }

    pub fn direct(ids: Vec<Identifier>) -> Self {
        Self::Direct(ids)
    }

    pub fn single(id: Identifier) -> Self {
        Self::Direct(vec![id])
    }

    /// Returns `true` if `id` is explicitly present in a `Direct` holder set.
    /// (Tag membership requires a registry lookup context.)
    pub fn contains_direct(&self, id: &Identifier) -> bool {
        match self {
            Self::Tag(_) => false,
            Self::Direct(ids) => ids.iter().any(|x| x == id),
        }
    }
}

/// Codec matching `RegistryCodecs.homogeneousList(...)`:
/// - `"#namespace:path"` -> `HolderSetRef::Tag`
/// - `"namespace:path"` -> `HolderSetRef::Direct(vec![id])`
/// - `["namespace:a", "namespace:b"]` -> `HolderSetRef::Direct(ids)`
pub fn holder_set_ref_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<HolderSetRef, Ops>> {
    codec_from_fns(
        "holder_set_ref",
        |v: &HolderSetRef, ops: &Ops| match v {
            HolderSetRef::Tag(id) => DataResult::success(ops.create_string(format!("#{id}"))),
            HolderSetRef::Direct(ids) => {
                let items: Vec<Ops::Output> =
                    ids.iter().map(|id| encode_identifier(id, ops)).collect();
                DataResult::success(ops.create_list(items))
            }
        },
        |ops: &Ops, input: &Ops::Output| {
            if let Some(s) = ops.get_string_value(input).result() {
                if let Some(rest) = s.strip_prefix('#') {
                    return Identifier::try_parse(rest)
                        .map(|id| DataResult::success(HolderSetRef::Tag(id)))
                        .unwrap_or_else(|| {
                            DataResult::error(format!("Invalid tag identifier: {s}"))
                        });
                }
                return Identifier::try_parse(s)
                    .map(|id| DataResult::success(HolderSetRef::Direct(vec![id])))
                    .unwrap_or_else(|| DataResult::error(format!("Invalid identifier: {s}")));
            }
            let list_res = ops.get_stream(input);
            let Some(items) = list_res.result() else {
                return DataResult::error(
                    "Expected #tag string, identifier string, or list of identifiers",
                );
            };
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                let id_res = decode_identifier(ops, item);
                match id_res.result().cloned() {
                    Some(id) => ids.push(id),
                    None => {
                        return DataResult::error(
                            id_res
                                .error_ref()
                                .map(|e| e.message().to_string())
                                .unwrap_or_else(|| "Invalid identifier in holder set".to_string()),
                        );
                    }
                }
            }
            DataResult::success(HolderSetRef::Direct(ids))
        },
    )
}
