//! Port of `net.minecraft.world.level.storage.ValueInput` — the storage value
//! read abstraction (issue #382).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/ValueInput.java`.
//!
//! ## Shape
//!
//! Java's `ValueInput` is an *interface* whose methods are generic over the
//! element type (`<T> Optional<T> read(String, Codec<T>)`), and every caller
//! uses it as the interface type (e.g. `Entity.load(ValueInput)`). Generic
//! methods are not dyn-compatible in Rust, so the port follows the crate's
//! closed-hierarchy idiom (the `Tag`/`AnyEntity` enums): `ValueInput` is an
//! enum over the layer's concrete variants (`Tag`-backed, and the empty
//! singleton), with inherent generic methods. This mirrors Java's erasure —
//! the element type is concrete at each call site, exactly as the JVM erases
//! `<T>` at the call site.
//!
//! The NBT-backed instantiation (`TagValueInput`) pins the ops type to
//! `TagContextOps` (`RegistryOps<Tag, NbtOps>`, the `createSerializationContext`
//! result), so the enum is not parameterized by `Ops`.

use crate::level::storage::tag_value_input::{
    CompoundListWrapper, ListWrapper, TagValueInput, TypedListWrapper,
};
use crate::level::storage::value_input_context_helper::TagContextOps;
use rivet_registry::access::RegistryAccess;
use rivet_serialization::Codec;
use rivet_serialization::map_codec::MapCodec;
use std::sync::Arc;

/// `net.minecraft.world.level.storage.ValueInput`.
pub enum ValueInput {
    /// The NBT-backed input (`TagValueInput`).
    Tag(TagValueInput),
    /// The shared empty input (Java's anonymous `empty` in
    /// `ValueInputContextHelper`).
    Empty(EmptyValueInput),
}

impl ValueInput {
    /// `ValueInput.read(String, Codec<T>)` — `Optional.empty()` when the field
    /// is absent; a codec error reports a problem and yields the partial value.
    pub fn read<A>(&self, name: &str, codec: &Arc<dyn Codec<A, TagContextOps>>) -> Option<A>
    where
        A: 'static,
    {
        match self {
            ValueInput::Tag(input) => input.read(name, codec),
            ValueInput::Empty(_) => None,
        }
    }

    /// `ValueInput.read(MapCodec<T>)` — `@Deprecated`; decodes the whole
    /// input as a map.
    pub fn read_map<A>(&self, codec: &Arc<dyn MapCodec<A, TagContextOps>>) -> Option<A>
    where
        A: 'static,
    {
        match self {
            ValueInput::Tag(input) => input.read_map(codec),
            ValueInput::Empty(_) => None,
        }
    }

    /// `ValueInput.child(String)` — `Optional.empty()` when the field is absent
    /// or not a compound.
    pub fn child(&self, name: &str) -> Option<ValueInput> {
        match self {
            ValueInput::Tag(input) => input.child(name),
            ValueInput::Empty(_) => None,
        }
    }

    /// `ValueInput.childOrEmpty(String)`.
    pub fn child_or_empty(&self, name: &str) -> ValueInput {
        match self {
            ValueInput::Tag(input) => input.child_or_empty(name),
            ValueInput::Empty(_) => ValueInput::Empty(EmptyValueInput::new(self.lookup().clone())),
        }
    }

    /// `ValueInput.childrenList(String)` — `Optional.empty()` when the field is
    /// absent or not a list.
    pub fn children_list(&self, name: &str) -> Option<ValueInputList> {
        match self {
            ValueInput::Tag(input) => input.children_list(name),
            ValueInput::Empty(_) => None,
        }
    }

    /// `ValueInput.childrenListOrEmpty(String)`.
    pub fn children_list_or_empty(&self, name: &str) -> ValueInputList {
        match self {
            ValueInput::Tag(input) => input.children_list_or_empty(name),
            ValueInput::Empty(_) => ValueInputList::Empty,
        }
    }

    /// `ValueInput.list(String, Codec<T>)`.
    pub fn list<A>(
        &self,
        name: &str,
        codec: Arc<dyn Codec<A, TagContextOps>>,
    ) -> Option<TypedInputList<A>>
    where
        A: 'static,
    {
        match self {
            ValueInput::Tag(input) => input.list(name, codec),
            ValueInput::Empty(_) => None,
        }
    }

    /// `ValueInput.listOrEmpty(String, Codec<T>)`.
    pub fn list_or_empty<A>(
        &self,
        name: &str,
        codec: Arc<dyn Codec<A, TagContextOps>>,
    ) -> TypedInputList<A>
    where
        A: 'static,
    {
        match self {
            ValueInput::Tag(input) => input.list_or_empty(name, codec),
            ValueInput::Empty(_) => TypedInputList::Empty,
        }
    }

    /// `ValueInput.getBooleanOr(String, boolean)`.
    pub fn get_boolean_or(&self, name: &str, default_value: bool) -> bool {
        match self {
            ValueInput::Tag(input) => input.get_boolean_or(name, default_value),
            ValueInput::Empty(_) => default_value,
        }
    }

    /// `ValueInput.getByteOr(String, byte)`.
    pub fn get_byte_or(&self, name: &str, default_value: i8) -> i8 {
        match self {
            ValueInput::Tag(input) => input.get_byte_or(name, default_value),
            ValueInput::Empty(_) => default_value,
        }
    }

    /// `ValueInput.getShortOr(String, short)`.
    pub fn get_short_or(&self, name: &str, default_value: i16) -> i16 {
        match self {
            ValueInput::Tag(input) => input.get_short_or(name, default_value),
            ValueInput::Empty(_) => default_value,
        }
    }

    /// `ValueInput.getInt(String)`.
    pub fn get_int(&self, name: &str) -> Option<i32> {
        match self {
            ValueInput::Tag(input) => input.get_int(name),
            ValueInput::Empty(_) => None,
        }
    }

    /// `ValueInput.getIntOr(String, int)`.
    pub fn get_int_or(&self, name: &str, default_value: i32) -> i32 {
        match self {
            ValueInput::Tag(input) => input.get_int_or(name, default_value),
            ValueInput::Empty(_) => default_value,
        }
    }

    /// `ValueInput.getLongOr(String, long)`.
    pub fn get_long_or(&self, name: &str, default_value: i64) -> i64 {
        match self {
            ValueInput::Tag(input) => input.get_long_or(name, default_value),
            ValueInput::Empty(_) => default_value,
        }
    }

    /// `ValueInput.getLong(String)`.
    pub fn get_long(&self, name: &str) -> Option<i64> {
        match self {
            ValueInput::Tag(input) => input.get_long(name),
            ValueInput::Empty(_) => None,
        }
    }

    /// `ValueInput.getFloatOr(String, float)`.
    pub fn get_float_or(&self, name: &str, default_value: f32) -> f32 {
        match self {
            ValueInput::Tag(input) => input.get_float_or(name, default_value),
            ValueInput::Empty(_) => default_value,
        }
    }

    /// `ValueInput.getDoubleOr(String, double)`.
    pub fn get_double_or(&self, name: &str, default_value: f64) -> f64 {
        match self {
            ValueInput::Tag(input) => input.get_double_or(name, default_value),
            ValueInput::Empty(_) => default_value,
        }
    }

    /// `ValueInput.getString(String)`.
    pub fn get_string(&self, name: &str) -> Option<String> {
        match self {
            ValueInput::Tag(input) => input.get_string(name),
            ValueInput::Empty(_) => None,
        }
    }

    /// `ValueInput.getStringOr(String, String)`.
    pub fn get_string_or(&self, name: &str, default_value: &str) -> String {
        match self {
            ValueInput::Tag(input) => input.get_string_or(name, default_value),
            ValueInput::Empty(_) => default_value.to_string(),
        }
    }

    /// `ValueInput.getIntArray(String)`.
    pub fn get_int_array(&self, name: &str) -> Option<Vec<i32>> {
        match self {
            ValueInput::Tag(input) => input.get_int_array(name),
            ValueInput::Empty(_) => None,
        }
    }

    /// `ValueInput.lookup()` — `@Deprecated`; the `HolderLookup.Provider`.
    pub fn lookup(&self) -> &RegistryAccess {
        match self {
            ValueInput::Tag(input) => input.lookup(),
            ValueInput::Empty(empty) => &empty.lookup,
        }
    }
}

/// The anonymous `empty` `ValueInput` (Java `ValueInputContextHelper.empty`).
///
/// The Java singleton's `childOrEmpty` returns `this`; value semantics make
/// that a fresh `Empty` — observationally identical (it is stateless beyond the
/// provider).
pub struct EmptyValueInput {
    lookup: RegistryAccess,
}

impl EmptyValueInput {
    pub fn new(lookup: RegistryAccess) -> Self {
        EmptyValueInput { lookup }
    }
}

/// `ValueInput.TypedInputList<T>` — `Iterable<T>` with `isEmpty()`/`stream()`.
pub enum TypedInputList<A> {
    /// A non-empty `ListTag`-backed list (`TagValueInput.TypedListWrapper`).
    Tag(TypedListWrapper<A>),
    /// The shared empty typed list.
    Empty,
}

impl<A: 'static> TypedInputList<A> {
    /// `TypedInputList.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        match self {
            TypedInputList::Tag(wrapper) => wrapper.is_empty(),
            TypedInputList::Empty => true,
        }
    }

    /// `TypedInputList.stream()`.
    ///
    /// Java's `Streams.mapWithIndex(...).filter(Objects::nonNull)` is lazy; the
    /// port materializes eagerly, which is observationally identical for any
    /// consumer that iterates (the per-element decode, reporting, and
    /// partial-drop ordering is unchanged).
    pub fn stream(&self) -> Box<dyn Iterator<Item = A> + '_> {
        match self {
            TypedInputList::Tag(wrapper) => wrapper.stream(),
            TypedInputList::Empty => Box::new(std::iter::empty()),
        }
    }
}

/// `ValueInput.ValueInputList` — `Iterable<ValueInput>` with
/// `isEmpty()`/`stream()`.
pub enum ValueInputList {
    /// A `ListTag`-backed children list (`TagValueInput.ListWrapper`).
    Tag(ListWrapper),
    /// A `List<CompoundTag>`-backed children list (the list-form factory).
    CompoundList(CompoundListWrapper),
    /// The shared empty children list.
    Empty,
}

impl ValueInputList {
    /// `ValueInputList.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        match self {
            ValueInputList::Tag(wrapper) => wrapper.is_empty(),
            ValueInputList::CompoundList(wrapper) => wrapper.is_empty(),
            ValueInputList::Empty => true,
        }
    }

    /// `ValueInputList.stream()`.
    pub fn stream(&self) -> Box<dyn Iterator<Item = ValueInput> + '_> {
        match self {
            ValueInputList::Tag(wrapper) => wrapper.stream(),
            ValueInputList::CompoundList(wrapper) => wrapper.stream(),
            ValueInputList::Empty => Box::new(std::iter::empty()),
        }
    }
}
