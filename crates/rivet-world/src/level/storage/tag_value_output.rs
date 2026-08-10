//! Port of `net.minecraft.world.level.storage.TagValueOutput` — the NBT-backed
//! `ValueOutput` (issue #382).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/TagValueOutput.java`.
//!
//! ## Shared mutation
//!
//! Java shares the `CompoundTag`/`ListTag` *object* between a parent output and
//! the children `child()`/`childrenList()`/`list()` return, so a child write is
//! immediately visible in the parent's tag (e.g. `Entity.save` writes
//! `Passengers` children into the same output it later `buildResult()`s).
//! Rust's `CompoundTag` is a value, so the port models the write-back with a
//! per-node `Rc<RefCell<…>>` plus a `sync` closure that copies the node's
//! current content into its parent slot and propagates the parent's own sync.
//! Every mutation ends in `sync()`, so the root (and `build_result()`) always
//! reflects children. There is no reference cycle: a node holds its parent's
//! `sync` closure (an `Rc<dyn Fn>`), while a parent only ever holds *snapshot
//! clones* of its children's content — never the children's `Rc`.
//!
//! ## Ops/context
//!
//! Every Paper 26.2 consumer of `TagValueOutput` uses the registry-context
//! factory (`createWithContext`); `createWithoutContext`/`createWrappingGlobal`
//! have no in-tree callers. This port therefore uses a single ops type
//! (`TagContextOps` = `RegistryOps<Tag, NbtOps>`), with the context-less
//! factories building over an empty `RegistryAccess`. A registry codec decoded
//! through the context-less ops reports the missing registry (Java's plain
//! `NbtOps` reports "not a registry ops") — a documented divergence only on
//! the error *message* of an unsupported path.

use crate::level::storage::value_input_context_helper::TagContextOps;
use crate::level::storage::value_output::{TypedOutputList, ValueOutput, ValueOutputList};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_registry::access::RegistryAccess;
use rivet_registry::registry_ops::RegistryOps;
use rivet_serialization::Codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::map_encoder;
use rivet_util::problem_reporter::{
    FieldPathElement, IndexedFieldPathElement, Problem, ProblemReporter,
};
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

/// `net.minecraft.world.level.storage.TagValueOutput`.
pub struct TagValueOutput {
    problem_reporter: Rc<dyn ProblemReporter>,
    ops: Arc<TagContextOps>,
    output: Rc<RefCell<CompoundTag>>,
    /// Propagates this node's current content into its parent slot and up —
    /// `None` for the root.
    sync: Option<Rc<dyn Fn()>>,
}

impl fmt::Debug for TagValueOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TagValueOutput")
    }
}

/// The single-tick `ProblemReporter` handle (OWNERSHIP's single sync tick).
type Reporter = Rc<dyn ProblemReporter>;

impl TagValueOutput {
    fn new(
        problem_reporter: Reporter,
        ops: Arc<TagContextOps>,
        output: Rc<RefCell<CompoundTag>>,
        sync: Option<Rc<dyn Fn()>>,
    ) -> Self {
        TagValueOutput {
            problem_reporter,
            ops,
            output,
            sync,
        }
    }

    /// `TagValueOutput.createWithContext(ProblemReporter,
    /// HolderLookup.Provider)`.
    pub fn create_with_context(
        problem_reporter: Reporter,
        provider: RegistryAccess,
    ) -> ValueOutput {
        ValueOutput::Tag(TagValueOutput::new(
            problem_reporter,
            context_ops(provider),
            Rc::new(RefCell::new(CompoundTag::new())),
            None,
        ))
    }

    /// `TagValueOutput.createWithoutContext(ProblemReporter)`.
    ///
    /// Built over an empty registry access (see the module doc).
    pub fn create_without_context(problem_reporter: Reporter) -> ValueOutput {
        TagValueOutput::create_with_context(problem_reporter, RegistryAccess::empty())
    }

    /// `TagValueOutput.createWrappingWithContext(ProblemReporter,
    /// HolderLookup.Provider, CompoundTag)` — writes into an existing tag.
    pub fn create_wrapping_with_context(
        problem_reporter: Reporter,
        provider: RegistryAccess,
        output: CompoundTag,
    ) -> ValueOutput {
        ValueOutput::Tag(TagValueOutput::new(
            problem_reporter,
            context_ops(provider),
            Rc::new(RefCell::new(output)),
            None,
        ))
    }

    /// `TagValueOutput.createWrappingGlobal(ProblemReporter, CompoundTag)`.
    pub fn create_wrapping_global(problem_reporter: Reporter, output: CompoundTag) -> ValueOutput {
        TagValueOutput::create_wrapping_with_context(
            problem_reporter,
            RegistryAccess::empty(),
            output,
        )
    }

    /// `TagValueOutput.buildResult()` — the accumulated `CompoundTag`.
    pub fn build_result(&self) -> CompoundTag {
        self.output.borrow().clone()
    }

    /// `reporterForChild(String)`.
    fn reporter_for_child(&self, name: &str) -> Reporter {
        self.problem_reporter
            .for_child(Rc::new(FieldPathElement(name.to_string())))
    }

    fn sync(&self) {
        if let Some(sync) = &self.sync {
            sync();
        }
    }
}

/// Build the serialization-context ops over a registry access (Java
/// `lookup.createSerializationContext(NbtOps.INSTANCE)`).
fn context_ops(access: RegistryAccess) -> Arc<TagContextOps> {
    Arc::new(RegistryOps::create_from_access(&NbtOps::instance(), access))
}

impl TagValueOutput {
    /// `ValueOutput.store(String, Codec<T>, T)`.
    pub fn store<A>(&self, name: &str, codec: &Arc<dyn Codec<A, TagContextOps>>, value: &A)
    where
        A: fmt::Debug + 'static,
    {
        let result = codec.encode_start(self.ops.as_ref(), value);
        match result.result() {
            Some(encoded) => {
                self.output
                    .borrow_mut()
                    .put(name.to_string(), encoded.clone());
            }
            None => {
                let error = result.error_ref().unwrap();
                self.problem_reporter
                    .report(Rc::new(EncodeToFieldFailedProblem::new(
                        name.to_string(),
                        format!("{value:?}"),
                        error.message().to_string(),
                    )));
                if let Some(partial) = error.partial().clone() {
                    self.output.borrow_mut().put(name.to_string(), partial);
                }
            }
        }
        self.sync();
    }

    /// `ValueOutput.storeNullable(String, Codec<T>, @Nullable T)`.
    pub fn store_nullable<A>(
        &self,
        name: &str,
        codec: &Arc<dyn Codec<A, TagContextOps>>,
        value: Option<&A>,
    ) where
        A: fmt::Debug + 'static,
    {
        if let Some(value) = value {
            self.store(name, codec, value);
        }
    }

    /// `ValueOutput.store(MapCodec<T>, T)`.
    pub fn store_map<A>(&self, codec: &Arc<dyn MapCodec<A, TagContextOps>>, value: &A)
    where
        A: fmt::Debug + 'static,
    {
        let encoder = map_encoder::encoder(Arc::new(
            rivet_serialization::map_codec::MapCodecEncoderHalf(codec.clone()),
        ));
        let result = encoder.encode_start(self.ops.as_ref(), value);
        match result.result() {
            Some(Tag::Compound(compound)) => {
                self.output.borrow_mut().merge(compound);
            }
            Some(other) => {
                // Java's unchecked `(CompoundTag)` cast on a non-compound
                // success — the encode of a `MapCodec` is always a map.
                panic!("TagValueOutput.store(MapCodec): expected compound, got {other}");
            }
            None => {
                let error = result.error_ref().unwrap();
                self.problem_reporter
                    .report(Rc::new(EncodeToMapFailedProblem::new(
                        format!("{value:?}"),
                        error.message().to_string(),
                    )));
                if let Some(partial) = error.partial().clone() {
                    match partial {
                        Tag::Compound(compound) => {
                            self.output.borrow_mut().merge(&compound);
                        }
                        other => panic!(
                            "TagValueOutput.store(MapCodec): expected compound partial, got {other}"
                        ),
                    }
                }
            }
        }
        self.sync();
    }

    /// `ValueOutput.putBoolean(String, boolean)`.
    pub fn put_boolean(&self, name: &str, value: bool) {
        self.output.borrow_mut().put_boolean(name, value);
        self.sync();
    }

    /// `ValueOutput.putByte(String, byte)`.
    pub fn put_byte(&self, name: &str, value: i8) {
        self.output.borrow_mut().put_byte(name, value);
        self.sync();
    }

    /// `ValueOutput.putShort(String, short)`.
    pub fn put_short(&self, name: &str, value: i16) {
        self.output.borrow_mut().put_short(name, value);
        self.sync();
    }

    /// `ValueOutput.putInt(String, int)`.
    pub fn put_int(&self, name: &str, value: i32) {
        self.output.borrow_mut().put_int(name, value);
        self.sync();
    }

    /// `ValueOutput.putLong(String, long)`.
    pub fn put_long(&self, name: &str, value: i64) {
        self.output.borrow_mut().put_long(name, value);
        self.sync();
    }

    /// `ValueOutput.putFloat(String, float)`.
    pub fn put_float(&self, name: &str, value: f32) {
        self.output.borrow_mut().put_float(name, value);
        self.sync();
    }

    /// `ValueOutput.putDouble(String, double)`.
    pub fn put_double(&self, name: &str, value: f64) {
        self.output.borrow_mut().put_double(name, value);
        self.sync();
    }

    /// `ValueOutput.putString(String, String)`.
    pub fn put_string(&self, name: &str, value: &str) {
        self.output.borrow_mut().put_string(name, value);
        self.sync();
    }

    /// `ValueOutput.putIntArray(String, int[])`.
    pub fn put_int_array(&self, name: &str, value: &[i32]) {
        self.output.borrow_mut().put_int_array(name, value.to_vec());
        self.sync();
    }

    /// `ValueOutput.child(String)`.
    pub fn child(&self, name: &str) -> ValueOutput {
        let name = name.to_string();
        let child_cell = Rc::new(RefCell::new(CompoundTag::new()));
        {
            let mut output = self.output.borrow_mut();
            output.put(name.to_string(), Tag::Compound(child_cell.borrow().clone()));
        }
        let parent_cell = Rc::clone(&self.output);
        let parent_sync = self.sync.clone();
        let sync_child_cell = Rc::clone(&child_cell);
        let sync_name = name.clone();
        let sync: Rc<dyn Fn()> = Rc::new(move || {
            parent_cell.borrow_mut().put(
                sync_name.clone(),
                Tag::Compound(sync_child_cell.borrow().clone()),
            );
            if let Some(parent_sync) = &parent_sync {
                parent_sync();
            }
        });
        ValueOutput::Tag(TagValueOutput::new(
            self.reporter_for_child(&name),
            Arc::clone(&self.ops),
            child_cell,
            Some(sync),
        ))
    }

    /// `ValueOutput.childrenList(String)`.
    pub fn children_list(&self, name: &str) -> ValueOutputList {
        let name = name.to_string();
        let list_cell = Rc::new(RefCell::new(ListTag::new()));
        {
            let mut output = self.output.borrow_mut();
            output.put(name.to_string(), Tag::List(list_cell.borrow().clone()));
        }
        let parent_cell = Rc::clone(&self.output);
        let parent_sync = self.sync.clone();
        let sync_list_cell = Rc::clone(&list_cell);
        let sync_name = name.clone();
        let sync: Rc<dyn Fn()> = Rc::new(move || {
            parent_cell.borrow_mut().put(
                sync_name.clone(),
                Tag::List(sync_list_cell.borrow().clone()),
            );
            if let Some(parent_sync) = &parent_sync {
                parent_sync();
            }
        });
        ValueOutputList::Tag(ListWrapper {
            field_name: name.clone(),
            problem_reporter: self.reporter_for_child(&name),
            ops: Arc::clone(&self.ops),
            output: list_cell,
            sync: Some(sync),
        })
    }

    /// `ValueOutput.list(String, Codec<T>)`.
    pub fn list<A>(&self, name: &str, codec: Arc<dyn Codec<A, TagContextOps>>) -> TypedOutputList<A>
    where
        A: 'static,
    {
        let name = name.to_string();
        let list_cell = Rc::new(RefCell::new(ListTag::new()));
        {
            let mut output = self.output.borrow_mut();
            output.put(name.to_string(), Tag::List(list_cell.borrow().clone()));
        }
        let parent_cell = Rc::clone(&self.output);
        let parent_sync = self.sync.clone();
        let sync_list_cell = Rc::clone(&list_cell);
        let sync_name = name.clone();
        let sync: Rc<dyn Fn()> = Rc::new(move || {
            parent_cell.borrow_mut().put(
                sync_name.clone(),
                Tag::List(sync_list_cell.borrow().clone()),
            );
            if let Some(parent_sync) = &parent_sync {
                parent_sync();
            }
        });
        TypedOutputList::Tag(TypedListWrapper {
            problem_reporter: Rc::clone(&self.problem_reporter),
            name: name.clone(),
            ops: Arc::clone(&self.ops),
            codec,
            output: list_cell,
            sync: Some(sync),
        })
    }

    /// `ValueOutput.discard(String)`.
    pub fn discard(&self, name: &str) {
        self.output.borrow_mut().remove(name);
        self.sync();
    }

    /// `ValueOutput.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.output.borrow().is_empty()
    }
}

// ---------------------------------------------------------------------------
// Problem records
// ---------------------------------------------------------------------------

/// `TagValueOutput.EncodeToFieldFailedProblem`.
#[derive(Debug)]
pub struct EncodeToFieldFailedProblem {
    name: String,
    value: String,
    message: String,
}

impl EncodeToFieldFailedProblem {
    fn new(name: String, value: String, message: String) -> Self {
        EncodeToFieldFailedProblem {
            name,
            value,
            message,
        }
    }
}

impl Problem for EncodeToFieldFailedProblem {
    fn description(&self) -> String {
        format!(
            "Failed to encode value '{}' to field '{}': {}",
            self.value, self.name, self.message
        )
    }
}

/// `TagValueOutput.EncodeToListFailedProblem`.
#[derive(Debug)]
pub struct EncodeToListFailedProblem {
    name: String,
    value: String,
    message: String,
}

impl EncodeToListFailedProblem {
    fn new(name: String, value: String, message: String) -> Self {
        EncodeToListFailedProblem {
            name,
            value,
            message,
        }
    }
}

impl Problem for EncodeToListFailedProblem {
    fn description(&self) -> String {
        format!(
            "Failed to append value '{}' to list '{}': {}",
            self.value, self.name, self.message
        )
    }
}

/// `TagValueOutput.EncodeToMapFailedProblem`.
#[derive(Debug)]
pub struct EncodeToMapFailedProblem {
    value: String,
    message: String,
}

impl EncodeToMapFailedProblem {
    fn new(value: String, message: String) -> Self {
        EncodeToMapFailedProblem { value, message }
    }
}

impl Problem for EncodeToMapFailedProblem {
    fn description(&self) -> String {
        format!(
            "Failed to merge value '{}' to an object: {}",
            self.value, self.message
        )
    }
}

// ---------------------------------------------------------------------------
// List wrappers
// ---------------------------------------------------------------------------

/// `TagValueOutput.ListWrapper` — the `childrenList` list of child outputs.
pub struct ListWrapper {
    field_name: String,
    problem_reporter: Reporter,
    ops: Arc<TagContextOps>,
    output: Rc<RefCell<ListTag>>,
    sync: Option<Rc<dyn Fn()>>,
}

impl ListWrapper {
    fn sync(&self) {
        if let Some(sync) = &self.sync {
            sync();
        }
    }

    /// `ValueOutputList.addChild()`.
    pub fn add_child(&self) -> ValueOutput {
        let child_cell = Rc::new(RefCell::new(CompoundTag::new()));
        let index = {
            let mut list = self.output.borrow_mut();
            list.add(Tag::Compound(child_cell.borrow().clone()));
            list.size() - 1
        };
        let list_cell = Rc::clone(&self.output);
        let list_sync = self.sync.clone();
        let sync_child_cell = Rc::clone(&child_cell);
        let sync: Rc<dyn Fn()> = Rc::new(move || {
            list_cell
                .borrow_mut()
                .set(index, Tag::Compound(sync_child_cell.borrow().clone()));
            if let Some(list_sync) = &list_sync {
                list_sync();
            }
        });
        let reporter = self
            .problem_reporter
            .for_child(Rc::new(IndexedFieldPathElement(
                self.field_name.clone(),
                index as i32,
            )));
        ValueOutput::Tag(TagValueOutput::new(
            reporter,
            Arc::clone(&self.ops),
            child_cell,
            Some(sync),
        ))
    }

    /// `ValueOutputList.discardLast()`.
    pub fn discard_last(&self) {
        let size = self.output.borrow().size();
        self.output.borrow_mut().remove(size - 1);
        self.sync();
    }

    /// `ValueOutputList.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.output.borrow().is_empty()
    }
}

/// `TagValueOutput.TypedListWrapper<T>` — the `list` typed list.
pub struct TypedListWrapper<A> {
    problem_reporter: Reporter,
    name: String,
    ops: Arc<TagContextOps>,
    codec: Arc<dyn Codec<A, TagContextOps>>,
    output: Rc<RefCell<ListTag>>,
    sync: Option<Rc<dyn Fn()>>,
}

impl<A> TypedListWrapper<A> {
    fn sync(&self) {
        if let Some(sync) = &self.sync {
            sync();
        }
    }

    /// `TypedOutputList.add(T)`.
    pub fn add(&self, value: &A)
    where
        A: fmt::Debug + 'static,
    {
        let result = self.codec.encode_start(self.ops.as_ref(), value);
        match result.result() {
            Some(encoded) => {
                self.output.borrow_mut().add(encoded.clone());
            }
            None => {
                let error = result.error_ref().unwrap();
                self.problem_reporter
                    .report(Rc::new(EncodeToListFailedProblem::new(
                        self.name.clone(),
                        format!("{value:?}"),
                        error.message().to_string(),
                    )));
                if let Some(partial) = error.partial().clone() {
                    self.output.borrow_mut().add(partial);
                }
            }
        }
        self.sync();
    }

    /// `TypedOutputList.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.output.borrow().is_empty()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::storage::value_output::ValueOutput;
    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_nbt::int_tag::IntTag;
    use rivet_nbt::tag::Tag;
    use rivet_registry::access::RegistryAccess;
    use rivet_serialization::codec::{self, Codec};
    use rivet_util::problem_reporter::Collector;
    use std::rc::Rc;
    use std::sync::Arc;

    fn reporter() -> Rc<Collector> {
        Rc::new(Collector::new())
    }

    fn output(reporter: Rc<Collector>) -> ValueOutput {
        TagValueOutput::create_with_context(reporter, RegistryAccess::empty())
    }

    /// `buildResult` reflects the primitives put through the interface.
    #[test]
    fn build_result_reflects_puts() {
        let out = output(reporter());
        out.put_boolean("flag", true);
        out.put_int("count", 7);
        out.put_string("name", "x");
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert!(result.get_boolean_or("flag", false));
        assert_eq!(result.get_int_or("count", 0), 7);
        assert_eq!(result.get_string_or("name", ""), "x");
    }

    /// `discard` removes a previously stored field.
    #[test]
    fn discard_removes_field() {
        let out = output(reporter());
        out.put_int("a", 1);
        out.put_int("b", 2);
        out.discard("a");
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert!(!result.contains("a"));
        assert!(result.contains("b"));
    }

    /// `store` encodes through the codec (Java `codec.encodeStart`).
    #[test]
    fn store_encodes_through_codec() {
        let out = output(reporter());
        out.store("n", &codec::int_codec::<TagContextOps>(), &42);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert_eq!(result.get_int("n"), Some(42));
    }

    /// `storeNullable(None)` is a no-op.
    #[test]
    fn store_nullable_none_is_noop() {
        let out = output(reporter());
        out.store_nullable("n", &codec::int_codec::<TagContextOps>(), None);
        assert!(out.is_empty());
        out.store_nullable("n", &codec::int_codec::<TagContextOps>(), Some(&3));
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert_eq!(result.get_int("n"), Some(3));
    }

    /// `store` on a failing codec reports and stores the partial (Java's
    /// `Error.partialValue()`).
    #[test]
    fn store_error_reports_and_stores_partial() {
        let reporter = reporter();
        let out = output(reporter.clone());
        let failing: Arc<dyn Codec<i32, TagContextOps>> = codec::validate(
            codec::int_codec::<TagContextOps>(),
            Arc::new(|_: &i32| rivet_serialization::DataResult::error_with_partial("bad", 99)),
        );
        out.store("n", &failing, &5);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert_eq!(result.get_int("n"), Some(99), "partial is stored");
        assert!(
            reporter
                .get_report()
                .contains("Failed to encode value '5' to field 'n': bad"),
            "report was: {}",
            reporter.get_report()
        );
    }

    /// A child's writes land in the parent's slot immediately (Java shares the
    /// `CompoundTag` object).
    #[test]
    fn child_writes_visible_in_parent() {
        let out = output(reporter());
        let child = out.child("sub");
        child.put_int("x", 9);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let sub = result.get_compound("sub").expect("sub compound present");
        assert_eq!(sub.get_int("x"), Some(9), "child write visible in parent");
    }

    /// `childrenList` + `addChild` — a grandchild's writes propagate all the
    /// way to the root's `buildResult`.
    #[test]
    fn children_list_add_child_propagates_to_root() {
        let out = output(reporter());
        let list = out.children_list("items");
        let child0 = list.add_child();
        child0.put_int("v", 1);
        let child1 = list.add_child();
        child1.put_int("v", 2);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let list_tag = result.get_list("items").expect("list present");
        assert_eq!(list_tag.size(), 2);
        assert_eq!(
            list_tag.get_compound(0).and_then(|c| c.get_int("v")),
            Some(1)
        );
        assert_eq!(
            list_tag.get_compound(1).and_then(|c| c.get_int("v")),
            Some(2)
        );
    }

    /// `discardLast` removes the last child and propagates.
    #[test]
    fn children_list_discard_last_propagates() {
        let out = output(reporter());
        let list = out.children_list("items");
        list.add_child().put_int("v", 1);
        list.add_child().put_int("v", 2);
        list.discard_last();
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let list_tag = result.get_list("items").expect("list present");
        assert_eq!(list_tag.size(), 1);
        assert_eq!(
            list_tag.get_compound(0).and_then(|c| c.get_int("v")),
            Some(1)
        );
    }

    /// `list` appends encoded values (Java `codec.encodeStart` per `add`).
    #[test]
    fn typed_list_appends_encoded_values() {
        let out = output(reporter());
        let list = out.list("nums", codec::int_codec::<TagContextOps>());
        list.add(&10);
        list.add(&20);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let list_tag = result.get_list("nums").expect("list present");
        assert_eq!(list_tag.get_int(0), Some(10));
        assert_eq!(list_tag.get_int(1), Some(20));
    }

    /// `store` with a MapCodec merges the encoded map into the output.
    #[test]
    fn store_map_merges_encoded_map() {
        use rivet_serialization::record_builder::{self, RecordCodecBuilder};
        // A trivial map codec: decode/encode a two-int pair as fields.
        let map_codec: Arc<
            dyn rivet_serialization::map_codec::MapCodec<(i32, i32), TagContextOps>,
        > = record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of_named(
                    Arc::new(|t: &(i32, i32)| t.0),
                    "a".to_string(),
                    codec::int_codec::<TagContextOps>(),
                ))
                .and(RecordCodecBuilder::of_named(
                    Arc::new(|t: &(i32, i32)| t.1),
                    "b".to_string(),
                    codec::int_codec::<TagContextOps>(),
                ))
                .apply(instance, Arc::new(|a: i32, b: i32| (a, b)))
        });
        let out = output(reporter());
        out.store_map(&map_codec, &(3, 4));
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert_eq!(result.get_int("a"), Some(3));
        assert_eq!(result.get_int("b"), Some(4));
    }

    /// `isEmpty` reflects the parent's content including children.
    #[test]
    fn empty_reflects_children() {
        let out = output(reporter());
        assert!(out.is_empty());
        out.child("sub").put_int("x", 1);
        assert!(!out.is_empty());
    }

    /// Wrapping an existing tag (Paper's `createWrappingGlobal`) writes into it.
    #[test]
    fn wrapping_global_writes_into_existing_tag() {
        let existing = CompoundTag::with_map(
            [("keep".to_string(), Tag::Int(IntTag::new(5)))]
                .into_iter()
                .collect(),
        );
        let out = TagValueOutput::create_wrapping_global(reporter(), existing);
        out.put_int("new", 6);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert_eq!(result.get_int("keep"), Some(5), "existing key preserved");
        assert_eq!(result.get_int("new"), Some(6));
    }

    // -----------------------------------------------------------------------
    // Registry context
    // -----------------------------------------------------------------------

    #[derive(Debug, Clone, PartialEq)]
    struct TestElement(u8);

    fn registry_key() -> rivet_registry::registry::RegistryKey<TestElement> {
        rivet_registry::ResourceKey::create_registry_key(
            rivet_registry::Identifier::with_default_namespace("test"),
        )
    }

    fn element_key(id: &str) -> rivet_registry::ResourceKey<TestElement> {
        rivet_registry::ResourceKey::create(
            &registry_key(),
            rivet_registry::Identifier::with_default_namespace(id),
        )
    }

    fn access_with_registry() -> RegistryAccess {
        use rivet_registry::builder::RegistryBuilder;
        use rivet_registry::registration_info::RegistrationInfo;
        use rivet_registry::registry::Registry;
        use rivet_registry::root::AnyBox;

        let mut builder = RegistryBuilder::new(&registry_key());
        builder.register(
            &element_key("alpha"),
            Arc::new(TestElement(1)),
            RegistrationInfo::BUILT_IN,
        );
        let registry: Registry<TestElement> = builder.freeze();
        RegistryAccess::from_pairs(vec![(
            rivet_registry::ResourceKey::create_registry_key(
                rivet_registry::Identifier::with_default_namespace("test"),
            ),
            Box::new(registry) as AnyBox,
        )])
    }

    /// A `RegistryFileCodec<TestElement>` — encodes a `Holder::Reference` to
    /// its identifier string through the ops' registry context.
    fn element_codec() -> Arc<dyn Codec<rivet_registry::Holder<TestElement>, TagContextOps>> {
        use rivet_registry::registry_file_codec::RegistryFileCodec;
        use rivet_serialization::codec as serialization_codec;
        let element = serialization_codec::xmap(
            rivet_registry::identifier::identifier_codec::<TagContextOps>(),
            Arc::new(|_id: &rivet_registry::Identifier| TestElement(0)),
            Arc::new(|_e: &TestElement| rivet_registry::Identifier::with_default_namespace("e")),
        );
        Arc::new(RegistryFileCodec::create(&registry_key(), element))
    }

    /// A registry-grounded `store` resolves a `Holder::Reference` to its
    /// identifier through the output's serialization-context ops.
    #[test]
    fn registry_context_encodes_holder_reference() {
        let access = access_with_registry();
        let registry_id = access
            .lookup::<TestElement>(&registry_key())
            .expect("frozen registry")
            .registry_id();
        let out = TagValueOutput::create_with_context(reporter(), access);
        out.store(
            "e",
            &element_codec(),
            &rivet_registry::Holder::<TestElement>::reference(registry_id, 0),
        );
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let encoded = result.get_string("e").expect("encoded identifier");
        assert_eq!(
            encoded, "minecraft:alpha",
            "a registry reference encodes to its identifier"
        );
    }

    /// Storing a holder from a *different* registry owner reports (Java's
    /// `"Element ... is not valid in current registry set"`).
    #[test]
    fn registry_context_wrong_owner_reports() {
        let access = access_with_registry();
        let reporter = reporter();
        let out = TagValueOutput::create_with_context(reporter.clone(), access);
        let foreign = rivet_registry::Holder::<TestElement>::reference(
            rivet_registry::holder::RegistryId(u32::MAX),
            0,
        );
        out.store("e", &element_codec(), &foreign);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert!(!result.contains("e"), "invalid holder stores nothing");
        assert!(
            reporter
                .get_report()
                .contains("is not valid in current registry set"),
            "report was: {}",
            reporter.get_report()
        );
    }
}
