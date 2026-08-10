//! Port of `net.minecraft.world.level.storage.ValueOutput` — the storage value
//! write abstraction (issue #382).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/ValueOutput.java`.
//!
//! Like `ValueInput` (see `value_input.rs`), the generic methods make the Java
//! interface non-dyn-compatible in Rust, so `ValueOutput` is a closed enum over
//! the layer's concrete variants (Java's erasure makes the element type
//! concrete at each call site anyway).

use crate::level::storage::tag_value_output::{ListWrapper, TagValueOutput, TypedListWrapper};
use crate::level::storage::value_input_context_helper::TagContextOps;
use rivet_serialization::Codec;
use rivet_serialization::map_codec::MapCodec;
use std::fmt::Debug;
use std::sync::Arc;

/// `net.minecraft.world.level.storage.ValueOutput`.
pub enum ValueOutput {
    /// The NBT-backed output (`TagValueOutput`).
    Tag(TagValueOutput),
}

impl ValueOutput {
    /// `ValueOutput.store(String, Codec<T>, T)`.
    pub fn store<A>(&self, name: &str, codec: &Arc<dyn Codec<A, TagContextOps>>, value: &A)
    where
        A: Debug + 'static,
    {
        match self {
            ValueOutput::Tag(output) => output.store(name, codec, value),
        }
    }

    /// `ValueOutput.storeNullable(String, Codec<T>, @Nullable T)`.
    pub fn store_nullable<A>(
        &self,
        name: &str,
        codec: &Arc<dyn Codec<A, TagContextOps>>,
        value: Option<&A>,
    ) where
        A: Debug + 'static,
    {
        match self {
            ValueOutput::Tag(output) => output.store_nullable(name, codec, value),
        }
    }

    /// `ValueOutput.store(MapCodec<T>, T)` — `@Deprecated`; merges the encoded
    /// map into the output.
    pub fn store_map<A>(&self, codec: &Arc<dyn MapCodec<A, TagContextOps>>, value: &A)
    where
        A: Debug + 'static,
    {
        match self {
            ValueOutput::Tag(output) => output.store_map(codec, value),
        }
    }

    /// `ValueOutput.putBoolean(String, boolean)`.
    pub fn put_boolean(&self, name: &str, value: bool) {
        match self {
            ValueOutput::Tag(output) => output.put_boolean(name, value),
        }
    }

    /// `ValueOutput.putByte(String, byte)`.
    pub fn put_byte(&self, name: &str, value: i8) {
        match self {
            ValueOutput::Tag(output) => output.put_byte(name, value),
        }
    }

    /// `ValueOutput.putShort(String, short)`.
    pub fn put_short(&self, name: &str, value: i16) {
        match self {
            ValueOutput::Tag(output) => output.put_short(name, value),
        }
    }

    /// `ValueOutput.putInt(String, int)`.
    pub fn put_int(&self, name: &str, value: i32) {
        match self {
            ValueOutput::Tag(output) => output.put_int(name, value),
        }
    }

    /// `ValueOutput.putLong(String, long)`.
    pub fn put_long(&self, name: &str, value: i64) {
        match self {
            ValueOutput::Tag(output) => output.put_long(name, value),
        }
    }

    /// `ValueOutput.putFloat(String, float)`.
    pub fn put_float(&self, name: &str, value: f32) {
        match self {
            ValueOutput::Tag(output) => output.put_float(name, value),
        }
    }

    /// `ValueOutput.putDouble(String, double)`.
    pub fn put_double(&self, name: &str, value: f64) {
        match self {
            ValueOutput::Tag(output) => output.put_double(name, value),
        }
    }

    /// `ValueOutput.putString(String, String)`.
    pub fn put_string(&self, name: &str, value: &str) {
        match self {
            ValueOutput::Tag(output) => output.put_string(name, value),
        }
    }

    /// `ValueOutput.putIntArray(String, int[])`.
    pub fn put_int_array(&self, name: &str, value: &[i32]) {
        match self {
            ValueOutput::Tag(output) => output.put_int_array(name, value),
        }
    }

    /// `ValueOutput.child(String)` — a child output that writes into a fresh
    /// sub-compound stored under `name` (eagerly created, like Java).
    pub fn child(&self, name: &str) -> ValueOutput {
        match self {
            ValueOutput::Tag(output) => output.child(name),
        }
    }

    /// `ValueOutput.childrenList(String)` — a list of child outputs.
    pub fn children_list(&self, name: &str) -> ValueOutputList {
        match self {
            ValueOutput::Tag(output) => output.children_list(name),
        }
    }

    /// `ValueOutput.list(String, Codec<T>)` — a typed list output.
    pub fn list<A>(&self, name: &str, codec: Arc<dyn Codec<A, TagContextOps>>) -> TypedOutputList<A>
    where
        A: 'static,
    {
        match self {
            ValueOutput::Tag(output) => output.list(name, codec),
        }
    }

    /// `ValueOutput.discard(String)`.
    pub fn discard(&self, name: &str) {
        match self {
            ValueOutput::Tag(output) => output.discard(name),
        }
    }

    /// `ValueOutput.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        match self {
            ValueOutput::Tag(output) => output.is_empty(),
        }
    }
}

/// `ValueOutput.TypedOutputList<T>`.
pub enum TypedOutputList<A> {
    /// A `ListTag`-backed typed list (`TagValueOutput.TypedListWrapper`).
    Tag(TypedListWrapper<A>),
}

impl<A: 'static> TypedOutputList<A> {
    /// `TypedOutputList.add(T)`.
    pub fn add(&self, value: &A)
    where
        A: Debug + 'static,
    {
        match self {
            TypedOutputList::Tag(wrapper) => wrapper.add(value),
        }
    }

    /// `TypedOutputList.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        match self {
            TypedOutputList::Tag(wrapper) => wrapper.is_empty(),
        }
    }
}

/// `ValueOutput.ValueOutputList`.
pub enum ValueOutputList {
    /// A `ListTag`-backed list of child outputs (`TagValueOutput.ListWrapper`).
    Tag(ListWrapper),
}

impl ValueOutputList {
    /// `ValueOutputList.addChild()`.
    pub fn add_child(&self) -> ValueOutput {
        match self {
            ValueOutputList::Tag(wrapper) => wrapper.add_child(),
        }
    }

    /// `ValueOutputList.discardLast()`.
    pub fn discard_last(&self) {
        match self {
            ValueOutputList::Tag(wrapper) => wrapper.discard_last(),
        }
    }

    /// `ValueOutputList.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        match self {
            ValueOutputList::Tag(wrapper) => wrapper.is_empty(),
        }
    }
}
