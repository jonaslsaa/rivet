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
//! Every mutation ends in `sync()`, so the root (and `build_result()`) reflects
//! children.
//!
//! Slot ownership is tracked by an **owned-slot registry** per parent: a field
//! maps to the live `ChildHandle` (output cell + nested slot registry) of the
//! child that created it, so a node's sync only writes back while its slot is
//! still owned by its own cell (identity, not value equality). The parent
//! removes the slot when it `discard()`s or replaces a field (only when a tag
//! was actually put — an encode error without a partial leaves the field and
//! any retained child untouched, matching Java), and a recreated
//! `child()`/`childrenList()`/`list()` or `addChild()` registers a fresh
//! handle — so a retained child after a discard/replace (even against an equal
//! empty recreated slot) is detached and its writes stay invisible. Because the
//! registry holds the child *cells* (not just identifiers), a parent-side
//! `merge` can recurse into a child-owned compound field, mutate the child's
//! own cell in place (Java's shared-object `CompoundTag.merge`), and refresh
//! the parent slot from it — so merged content and the child's future writes
//! share one object. There is no reference cycle: a node holds its parent's
//! `sync` closure (an `Rc<dyn Fn>`), while a parent only ever holds *snapshot
//! clones* of its children's content in the NBT and the children's `ChildHandle`
//! cells — never the children's `TagValueOutput`/`sync`.
//!
//! The wrapping factories (`createWrappingWithContext`/`createWrappingGlobal`)
//! take a `SharedCompoundTag` — the caller keeps a clone of the handle and
//! shares the underlying tag and the root slot-token registry with the output.
//! The handle exposes only token-tracking mutations (`put`/`remove`/`merge`)
//! plus read access, so a caller's field replacement detaches a retained child
//! exactly as Java's shared-object `CompoundTag.put` would (including equal
//! value replacements), and writes are visible both ways.
//!
//! ## Ops/context
//!
//! Every Paper 26.2 consumer of `TagValueOutput` uses a registry-context
//! factory (`createWithContext`/`createWrappingWithContext`);
//! `createWithoutContext`/`createWrappingGlobal` have no in-tree callers. This
//! port therefore uses a single ops type
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
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

/// A live child node owning a parent slot: its output cell and its own slot
/// registry. Keeping both lets a parent-side `merge` recurse into the child at
/// arbitrary depth (Java's shared-object `CompoundTag.merge`) and refresh the
/// parent slot from the child's cell, so the child's future writes preserve
/// merged content.
struct ChildHandle {
    output: Rc<RefCell<CompoundTag>>,
    slots: Rc<RefCell<HashMap<String, OwnedSlot>>>,
}

/// A slot owned by a live child — identity is the cell pointer, so a retained
/// child is detached the moment the parent replaces/discards its slot (even by
/// an equal-value replacement), and a parent-side merge can update the child's
/// cell in place.
enum OwnedSlot {
    Compound(Rc<ChildHandle>),
    List(Rc<RefCell<ListTag>>),
}

/// `net.minecraft.world.level.storage.TagValueOutput`.
pub struct TagValueOutput {
    problem_reporter: Rc<dyn ProblemReporter>,
    ops: Arc<TagContextOps>,
    output: Rc<RefCell<CompoundTag>>,
    /// Propagates this node's current content into its parent slot and up —
    /// `None` for the root.
    sync: Option<Rc<dyn Fn()>>,
    /// Field name → the live child owning that field slot. Cleared when this
    /// node discards or replaces a field; set when a child is created at a
    /// field.
    slot_tokens: Rc<RefCell<HashMap<String, OwnedSlot>>>,
}

impl fmt::Debug for TagValueOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TagValueOutput")
    }
}

/// The `ProblemReporter` handle — `Rc` (non-`Send`), confined to the tick
/// thread per OWNERSHIP.
type Reporter = Rc<dyn ProblemReporter>;

impl TagValueOutput {
    fn new(
        problem_reporter: Reporter,
        ops: Arc<TagContextOps>,
        output: Rc<RefCell<CompoundTag>>,
        sync: Option<Rc<dyn Fn()>>,
        slot_tokens: Rc<RefCell<HashMap<String, OwnedSlot>>>,
    ) -> Self {
        TagValueOutput {
            problem_reporter,
            ops,
            output,
            sync,
            slot_tokens,
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
            Rc::new(RefCell::new(HashMap::new())),
        ))
    }

    /// `TagValueOutput.createWithoutContext(ProblemReporter)`.
    ///
    /// RivetTodo(#382): Java builds this over plain `NbtOps`; the port pins
    /// every factory to `TagContextOps`, so an empty `RegistryAccess` reports
    /// the missing registry ("Unknown registry: …") where Java's plain `NbtOps`
    /// reports "Not a registry ops" — message-only divergence on an unused
    /// path (see the module doc).
    pub fn create_without_context(problem_reporter: Reporter) -> ValueOutput {
        TagValueOutput::create_with_context(problem_reporter, RegistryAccess::empty())
    }

    /// `TagValueOutput.createWrappingWithContext(ProblemReporter,
    /// HolderLookup.Provider, CompoundTag)` — writes into an existing tag.
    ///
    /// Java passes the `CompoundTag` *object* and shares it with the caller;
    /// the port takes a `SharedCompoundTag` so the caller keeps a handle and
    /// mutations are visible both ways (writes through the output appear in
    /// the caller's tag, and the caller's tracked mutations appear in
    /// `buildResult()`).
    pub fn create_wrapping_with_context(
        problem_reporter: Reporter,
        provider: RegistryAccess,
        output: SharedCompoundTag,
    ) -> ValueOutput {
        ValueOutput::Tag(TagValueOutput::new(
            problem_reporter,
            context_ops(provider),
            output.inner,
            None,
            output.slot_tokens,
        ))
    }

    /// `TagValueOutput.createWrappingGlobal(ProblemReporter, CompoundTag)`.
    ///
    /// Same `TagContextOps` pinning as `createWithoutContext` (see the
    /// `RivetTodo` there): empty `RegistryAccess` instead of Java's plain
    /// `NbtOps`.
    pub fn create_wrapping_global(
        problem_reporter: Reporter,
        output: SharedCompoundTag,
    ) -> ValueOutput {
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

    /// Detach any child that currently owns the `name` field slot — this node
    /// is about to overwrite or remove it (Java `CompoundTag.put`/`remove`
    /// replaces the object reference, so the retained child becomes invisible).
    fn detach_field(&self, name: &str) {
        self.slot_tokens.borrow_mut().remove(name);
    }
}

/// Build the serialization-context ops over a registry access (Java
/// `lookup.createSerializationContext(NbtOps.INSTANCE)`).
fn context_ops(access: RegistryAccess) -> Arc<TagContextOps> {
    Arc::new(RegistryOps::create_from_access(&NbtOps::instance(), access))
}

/// `CompoundTag.merge(other)` with Java's shared-object semantics.
///
/// Keys whose map value is a **live compound child** are merged *into the
/// child's own cell* (recursing through the child's slot registry), then the
/// parent slot is refreshed from the child's cell — so the merged content and
/// the child's future writes share one object, exactly like Java. All other
/// keys are merged with the ordinary `CompoundTag.merge` (compound-over-
/// compound in the map value, `put` otherwise), and any live child whose field
/// that ordinary merge replaced is detached.
fn merge_compound(
    inner: &Rc<RefCell<CompoundTag>>,
    slot_tokens: &Rc<RefCell<HashMap<String, OwnedSlot>>>,
    compound: &CompoundTag,
) {
    // Partition the merged keys: those bound to a live compound child recurse
    // into the child cell; the rest go through the ordinary merge.
    let mut child_merges: Vec<(String, Rc<ChildHandle>, CompoundTag)> = Vec::new();
    let mut rest = CompoundTag::new();
    {
        let slots = slot_tokens.borrow();
        for (key, merged) in compound.entry_set() {
            match (slots.get(key), merged) {
                (Some(OwnedSlot::Compound(handle)), Tag::Compound(other_compound)) => {
                    child_merges.push((key.clone(), Rc::clone(handle), other_compound.clone()));
                }
                _ => {
                    rest.put(key.clone(), merged.copy_tag());
                }
            }
        }
    }
    inner.borrow_mut().merge(&rest);
    // The ordinary merge replaced (put) every `rest` field that had a live
    // child, so detach those children.
    let replaced: Vec<String> = {
        let slots = slot_tokens.borrow();
        rest.key_set()
            .filter(|key| slots.contains_key(*key))
            .cloned()
            .collect()
    };
    for key in replaced {
        slot_tokens.borrow_mut().remove(&key);
    }
    // Recurse into live compound children, then refresh the parent slot from
    // the (possibly merged) child cell so the parent always reflects it.
    for (key, handle, other_compound) in child_merges {
        merge_compound(&handle.output, &handle.slots, &other_compound);
        let refreshed = Tag::Compound(handle.output.borrow().clone());
        inner.borrow_mut().put(key, refreshed);
    }
}

impl TagValueOutput {
    /// `ValueOutput.store(String, Codec<T>, T)`.
    pub fn store<A>(&self, name: &str, codec: &Arc<dyn Codec<A, TagContextOps>>, value: &A)
    where
        A: fmt::Debug + 'static,
    {
        let result = codec.encode_start(self.ops.as_ref(), value);
        // Java only touches the field when the codec produced a tag — success
        // or error-with-partial. An error without a partial leaves the field
        // (and any retained child at it) untouched.
        let mut placed = false;
        match result.result() {
            Some(encoded) => {
                self.output
                    .borrow_mut()
                    .put(name.to_string(), encoded.clone());
                placed = true;
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
                    placed = true;
                }
            }
        }
        if placed {
            // Storing replaces any child slot at `name` (Java
            // `CompoundTag.put`), detaching a retained child.
            self.detach_field(name);
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
                merge_compound(&self.output, &self.slot_tokens, compound);
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
                            merge_compound(&self.output, &self.slot_tokens, &compound);
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
        self.detach_field(name);
        self.sync();
    }

    /// `ValueOutput.putByte(String, byte)`.
    pub fn put_byte(&self, name: &str, value: i8) {
        self.output.borrow_mut().put_byte(name, value);
        self.detach_field(name);
        self.sync();
    }

    /// `ValueOutput.putShort(String, short)`.
    pub fn put_short(&self, name: &str, value: i16) {
        self.output.borrow_mut().put_short(name, value);
        self.detach_field(name);
        self.sync();
    }

    /// `ValueOutput.putInt(String, int)`.
    pub fn put_int(&self, name: &str, value: i32) {
        self.output.borrow_mut().put_int(name, value);
        self.detach_field(name);
        self.sync();
    }

    /// `ValueOutput.putLong(String, long)`.
    pub fn put_long(&self, name: &str, value: i64) {
        self.output.borrow_mut().put_long(name, value);
        self.detach_field(name);
        self.sync();
    }

    /// `ValueOutput.putFloat(String, float)`.
    pub fn put_float(&self, name: &str, value: f32) {
        self.output.borrow_mut().put_float(name, value);
        self.detach_field(name);
        self.sync();
    }

    /// `ValueOutput.putDouble(String, double)`.
    pub fn put_double(&self, name: &str, value: f64) {
        self.output.borrow_mut().put_double(name, value);
        self.detach_field(name);
        self.sync();
    }

    /// `ValueOutput.putString(String, String)`.
    pub fn put_string(&self, name: &str, value: &str) {
        self.output.borrow_mut().put_string(name, value);
        self.detach_field(name);
        self.sync();
    }

    /// `ValueOutput.putIntArray(String, int[])`.
    pub fn put_int_array(&self, name: &str, value: &[i32]) {
        self.output.borrow_mut().put_int_array(name, value.to_vec());
        self.detach_field(name);
        self.sync();
    }

    /// `ValueOutput.child(String)`.
    pub fn child(&self, name: &str) -> ValueOutput {
        let name = name.to_string();
        let child_cell = Rc::new(RefCell::new(CompoundTag::new()));
        let child_slots: Rc<RefCell<HashMap<String, OwnedSlot>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let handle = Rc::new(ChildHandle {
            output: Rc::clone(&child_cell),
            slots: Rc::clone(&child_slots),
        });
        {
            let mut output = self.output.borrow_mut();
            output.put(name.to_string(), Tag::Compound(child_cell.borrow().clone()));
        }
        self.slot_tokens
            .borrow_mut()
            .insert(name.clone(), OwnedSlot::Compound(handle));
        let parent_cell = Rc::clone(&self.output);
        let parent_slot_tokens = Rc::clone(&self.slot_tokens);
        let parent_sync = self.sync.clone();
        let sync_child_cell = Rc::clone(&child_cell);
        let sync_name = name.clone();
        let sync: Rc<dyn Fn()> = Rc::new(move || {
            let still_owned = match parent_slot_tokens.borrow().get(&sync_name) {
                Some(OwnedSlot::Compound(handle)) => Rc::ptr_eq(&handle.output, &sync_child_cell),
                _ => false,
            };
            if still_owned {
                let tag = Tag::Compound(sync_child_cell.borrow().clone());
                parent_cell.borrow_mut().put(sync_name.clone(), tag.clone());
                if let Some(parent_sync) = &parent_sync {
                    parent_sync();
                }
            }
        });
        ValueOutput::Tag(TagValueOutput::new(
            self.reporter_for_child(&name),
            Arc::clone(&self.ops),
            child_cell,
            Some(sync),
            child_slots,
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
        self.slot_tokens
            .borrow_mut()
            .insert(name.clone(), OwnedSlot::List(Rc::clone(&list_cell)));
        let parent_cell = Rc::clone(&self.output);
        let parent_slot_tokens = Rc::clone(&self.slot_tokens);
        let parent_sync = self.sync.clone();
        let sync_list_cell = Rc::clone(&list_cell);
        let sync_name = name.clone();
        let sync: Rc<dyn Fn()> = Rc::new(move || {
            let still_owned = match parent_slot_tokens.borrow().get(&sync_name) {
                Some(OwnedSlot::List(cell)) => Rc::ptr_eq(cell, &sync_list_cell),
                _ => false,
            };
            if still_owned {
                let tag = Tag::List(sync_list_cell.borrow().clone());
                parent_cell.borrow_mut().put(sync_name.clone(), tag.clone());
                if let Some(parent_sync) = &parent_sync {
                    parent_sync();
                }
            }
        });
        ValueOutputList::Tag(ListWrapper {
            field_name: name.clone(),
            problem_reporter: Rc::clone(&self.problem_reporter),
            ops: Arc::clone(&self.ops),
            output: list_cell,
            sync: Some(sync),
            slot_tokens: Rc::new(RefCell::new(Vec::new())),
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
        self.slot_tokens
            .borrow_mut()
            .insert(name.clone(), OwnedSlot::List(Rc::clone(&list_cell)));
        let parent_cell = Rc::clone(&self.output);
        let parent_slot_tokens = Rc::clone(&self.slot_tokens);
        let parent_sync = self.sync.clone();
        let sync_list_cell = Rc::clone(&list_cell);
        let sync_name = name.clone();
        let sync: Rc<dyn Fn()> = Rc::new(move || {
            let still_owned = match parent_slot_tokens.borrow().get(&sync_name) {
                Some(OwnedSlot::List(cell)) => Rc::ptr_eq(cell, &sync_list_cell),
                _ => false,
            };
            if still_owned {
                let tag = Tag::List(sync_list_cell.borrow().clone());
                parent_cell.borrow_mut().put(sync_name.clone(), tag.clone());
                if let Some(parent_sync) = &parent_sync {
                    parent_sync();
                }
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
        self.detach_field(name);
        self.sync();
    }

    /// `ValueOutput.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.output.borrow().is_empty()
    }
}

// ---------------------------------------------------------------------------
// SharedCompoundTag — the caller-held handle for the wrapping factories
// ---------------------------------------------------------------------------

/// A caller-held, shared `CompoundTag` for `createWrappingWithContext` /
/// `createWrappingGlobal`.
///
/// Shares the underlying tag *and* the root output's slot-token registry with
/// the `TagValueOutput` it wraps, so mutations through this handle detach a
/// retained child exactly as Java's shared-object `CompoundTag.put` would:
/// replacing or removing a field clears the owning child's token regardless of
/// value (even an equal-value replacement), and `buildResult()` always reflects
/// the caller's writes. The handle exposes read access (`borrow`) and
/// token-tracking mutations (`put`/`remove`/`merge`) but deliberately no raw
/// mutable borrow, so every external field replacement is observable.
#[derive(Clone)]
pub struct SharedCompoundTag {
    inner: Rc<RefCell<CompoundTag>>,
    slot_tokens: Rc<RefCell<HashMap<String, OwnedSlot>>>,
}

impl SharedCompoundTag {
    /// Wrap an existing `CompoundTag`, sharing its content with a future
    /// wrapping output. Clone the handle to keep one copy for the caller while
    /// passing another to `createWrapping*`.
    pub fn new(tag: CompoundTag) -> Self {
        SharedCompoundTag {
            inner: Rc::new(RefCell::new(tag)),
            slot_tokens: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Read access to the shared tag (no raw mutable borrow — use `put`/
    /// `remove`/`merge` to mutate so detachment is tracked).
    pub fn borrow(&self) -> std::cell::Ref<'_, CompoundTag> {
        self.inner.borrow()
    }

    /// `CompoundTag.put(String, Tag)` — replaces the field, detaching any
    /// retained child at it (Java object identity).
    pub fn put(&self, name: &str, tag: Tag) {
        self.inner.borrow_mut().put(name.to_string(), tag);
        self.slot_tokens.borrow_mut().remove(name);
    }

    /// `CompoundTag.remove(String)`.
    pub fn remove(&self, name: &str) {
        self.inner.borrow_mut().remove(name);
        self.slot_tokens.borrow_mut().remove(name);
    }

    /// `CompoundTag.merge(CompoundTag)` — with Java's shared-object semantics:
    /// merges into live child cells in place and detaches only the fields the
    /// merge actually replaced.
    pub fn merge(&self, other: &CompoundTag) {
        merge_compound(&self.inner, &self.slot_tokens, other);
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
    /// Element index → the live child owning that element.
    slot_tokens: Rc<RefCell<Vec<OwnedSlot>>>,
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
        let child_slots: Rc<RefCell<HashMap<String, OwnedSlot>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let handle = Rc::new(ChildHandle {
            output: Rc::clone(&child_cell),
            slots: Rc::clone(&child_slots),
        });
        let index = {
            let mut list = self.output.borrow_mut();
            list.add(Tag::Compound(child_cell.borrow().clone()));
            list.size() - 1
        };
        self.slot_tokens
            .borrow_mut()
            .push(OwnedSlot::Compound(handle));
        let list_cell = Rc::clone(&self.output);
        let list_slot_tokens = Rc::clone(&self.slot_tokens);
        let list_sync = self.sync.clone();
        let sync_child_cell = Rc::clone(&child_cell);
        let sync: Rc<dyn Fn()> = Rc::new(move || {
            let still_owned = match list_slot_tokens.borrow().get(index) {
                Some(OwnedSlot::Compound(handle)) => Rc::ptr_eq(&handle.output, &sync_child_cell),
                _ => false,
            };
            if still_owned {
                let tag = Tag::Compound(sync_child_cell.borrow().clone());
                list_cell.borrow_mut().set(index, tag.clone());
                if let Some(list_sync) = &list_sync {
                    list_sync();
                }
            }
        });
        let reporter = self
            .problem_reporter
            .for_child(Rc::new(IndexedFieldPathElement(
                self.field_name.clone(),
                index as i32,
            )));
        // Java's `addChild` adds the empty child to the shared list object
        // immediately, so propagate the new element to the parent right away.
        self.sync();
        ValueOutput::Tag(TagValueOutput::new(
            reporter,
            Arc::clone(&self.ops),
            child_cell,
            Some(sync),
            child_slots,
        ))
    }

    /// `ValueOutputList.discardLast()`.
    ///
    /// Java's `ListTag.removeLast()` (via `AbstractList`/`SequencedCollection`)
    /// throws `NoSuchElementException` on an empty list; the port panics with
    /// that message instead of underflowing a `usize` index.
    pub fn discard_last(&self) {
        let size = self.output.borrow().size();
        if size == 0 {
            panic!(
                "TagValueOutput.ListWrapper.discardLast() on an empty list (Java: NoSuchElementException)"
            );
        }
        self.output.borrow_mut().remove(size - 1);
        self.slot_tokens.borrow_mut().pop();
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

    /// `childrenList` child problems report against the *root* reporter with a
    /// path of just the field + index — Java passes the current reporter (not a
    /// field-scoped child) to `ListWrapper`, and `addChild` forks it with
    /// `IndexedFieldPathElement(field, index)`.
    #[test]
    fn children_list_child_reports_field_indexed_path() {
        let reporter = reporter();
        let out = output(reporter.clone());
        let list = out.children_list("items");
        let failing: Arc<dyn Codec<i32, TagContextOps>> = codec::validate(
            codec::int_codec::<TagContextOps>(),
            Arc::new(|_: &i32| rivet_serialization::DataResult::error_with_partial("bad", 99)),
        );
        list.add_child().store("v", &failing, &5);
        let report = reporter.get_report();
        assert!(
            report.contains(" at .items[0]: Failed to encode value '5' to field 'v': bad"),
            "Java path is field+index only (no extra field segment), report was: {report}"
        );
    }

    /// A child of a child propagates through two sync closures (Java shares the
    /// nested `CompoundTag` objects) — the deepest plain-child chain.
    #[test]
    fn nested_child_writes_propagate_to_root() {
        let out = output(reporter());
        out.child("a").child("b").put_int("x", 5);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let a = result.get_compound("a").expect("a compound");
        let b = a.get_compound("b").expect("b compound");
        assert_eq!(b.get_int("x"), Some(5), "grandchild write visible at root");
    }

    /// `addChild` adds the empty child to the shared list immediately — Java's
    /// `ListWrapper.addChild` does `this.output.add(child)`, so `buildResult`
    /// right after `addChild` (before any write) already shows one element.
    #[test]
    fn add_child_empty_element_is_immediately_visible() {
        let out = output(reporter());
        let list = out.children_list("items");
        list.add_child();
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let list_tag = result.get_list("items").expect("list present");
        assert_eq!(
            list_tag.size(),
            1,
            "the empty child is visible before any write, got: {list_tag:?}"
        );
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

    // -----------------------------------------------------------------------
    // Detachment after parent discard/overwrite (Java object-identity
    // semantics: `discard`/`store`/`put*` replace the parent map entry, so a
    // retained child's writes go into its detached object and are invisible to
    // the parent).
    // -----------------------------------------------------------------------

    /// A retained child must not reinsert its field after the parent discards
    /// it (Java: `discard` removes the map entry; the child's object is
    /// detached, so later writes are invisible to the parent).
    #[test]
    fn child_write_after_parent_discard_is_invisible() {
        let out = output(reporter());
        let child = out.child("x");
        child.put_int("a", 1);
        out.discard("x");
        child.put_int("b", 2);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert!(
            !result.contains("x"),
            "a retained child's write must not reinsert a discarded field, got: {result:?}"
        );
    }

    /// A `childrenList` retained after the parent discards its field must not
    /// reinsert the field on `addChild`.
    #[test]
    fn children_list_write_after_parent_discard_is_invisible() {
        let out = output(reporter());
        let list = out.children_list("items");
        out.discard("items");
        list.add_child().put_int("v", 1);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert!(
            !result.contains("items"),
            "a retained list's write must not reinsert a discarded field, got: {result:?}"
        );
    }

    /// A typed `list` retained after the parent discards its field must not
    /// reinsert the field on `add`.
    #[test]
    fn typed_list_add_after_parent_discard_is_invisible() {
        let out = output(reporter());
        let list = out.list("nums", codec::int_codec::<TagContextOps>());
        out.discard("nums");
        list.add(&5);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert!(
            !result.contains("nums"),
            "a retained typed list's add must not reinsert a discarded field, got: {result:?}"
        );
    }

    /// A list child retained after `discardLast` removes its slot must not
    /// corrupt the list on write (Java: the removed object is detached and
    /// invisible; Rust must not panic on the stale index).
    #[test]
    fn retained_list_child_write_after_discard_last_is_invisible() {
        let out = output(reporter());
        let list = out.children_list("items");
        let child = list.add_child();
        child.put_int("v", 1);
        list.discard_last();
        child.put_int("w", 2);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let list_tag = result.get_list("items").expect("list present");
        assert_eq!(
            list_tag.size(),
            0,
            "discardLast removes the child; a retained write stays invisible, got: {list_tag:?}"
        );
    }

    /// A parent `store`/`put*` that replaces a field detaches a retained child
    /// (Java: the map entry now references a different object).
    #[test]
    fn child_write_after_parent_overwrite_is_invisible() {
        let out = output(reporter());
        let child = out.child("x");
        child.put_int("a", 1);
        out.put_int("x", 5);
        child.put_int("b", 2);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert_eq!(
            result.get_int("x"),
            Some(5),
            "parent overwrite detaches the child; the child's later write stays invisible, got: {result:?}"
        );
    }

    /// A **never-written** old child must not clobber a recreated equal empty
    /// slot after `discard` + `child` at the same name. Java detaches by object
    /// identity: the recreated slot is a different (but value-identical empty)
    /// `CompoundTag`, so the old handle's write stays invisible.
    #[test]
    fn never_written_child_does_not_clobber_recreated_slot() {
        let out = output(reporter());
        let old = out.child("x");
        out.discard("x");
        out.child("x");
        old.put_int("a", 1);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let x = result.get_compound("x").expect("recreated x present");
        assert!(
            x.is_empty(),
            "old handle's write must not clobber the recreated empty slot, got: {result:?}"
        );
    }

    /// The `childrenList` analogue: a never-written old element must not
    /// clobber a recreated equal empty element after `discardLast` + `addChild`.
    #[test]
    fn never_written_list_child_does_not_clobber_recreated_slot() {
        let out = output(reporter());
        let list = out.children_list("items");
        let old = list.add_child();
        list.discard_last();
        list.add_child();
        old.put_int("v", 1);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let items = result.get_list("items").expect("list present");
        assert_eq!(items.size(), 1, "got: {result:?}");
        let slot = items.get_compound(0).expect("element compound");
        assert!(
            slot.is_empty(),
            "old handle's write must not clobber the recreated empty slot, got: {result:?}"
        );
    }

    /// A never-written `childrenList` itself must not clobber a recreated equal
    /// empty list after `discard` + `childrenList` at the same name.
    #[test]
    fn never_written_list_does_not_clobber_recreated_slot() {
        let out = output(reporter());
        let old_list = out.children_list("items");
        out.discard("items");
        out.children_list("items");
        old_list.add_child().put_int("v", 1);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let items = result.get_list("items").expect("recreated list present");
        assert!(
            items.is_empty(),
            "old list handle's add must not clobber the recreated empty list, got: {result:?}"
        );
    }

    /// Wrapping an existing tag (Paper's `createWrappingGlobal`) writes into it
    /// and shares with the caller: the output's writes are visible in the
    /// caller's tag, and the caller's tracked mutations are visible in the
    /// output's `buildResult` (Java's shared-object `CompoundTag` parameter).
    #[test]
    fn wrapping_global_writes_into_existing_tag_and_shares() {
        let shared = SharedCompoundTag::new(CompoundTag::with_map(
            [("keep".to_string(), Tag::Int(IntTag::new(5)))]
                .into_iter()
                .collect(),
        ));
        let out = TagValueOutput::create_wrapping_global(reporter(), shared.clone());
        out.put_int("new", 6);
        shared.put("caller", Tag::Int(IntTag::new(7)));
        assert_eq!(
            shared.borrow().get_int("keep"),
            Some(5),
            "existing key preserved"
        );
        assert_eq!(
            shared.borrow().get_int("new"),
            Some(6),
            "output write visible in caller tag"
        );
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert_eq!(
            result.get_int("caller"),
            Some(7),
            "caller write visible in buildResult"
        );
        assert_eq!(result.get_int("new"), Some(6));
    }

    /// Output-side children propagate into the shared wrapping tag (Java
    /// shared-mutation includes the child/`childrenList` subtrees).
    #[test]
    fn wrapping_child_write_visible_in_shared_cell() {
        let shared = SharedCompoundTag::new(CompoundTag::new());
        let out = TagValueOutput::create_wrapping_global(reporter(), shared.clone());
        out.child("sub").put_int("x", 1);
        out.children_list("items").add_child().put_int("v", 2);
        let tags = shared.borrow();
        let sub = tags.get_compound("sub").expect("sub compound");
        assert_eq!(sub.get_int("x"), Some(1));
        let items = tags.get_list("items").expect("items list");
        assert_eq!(items.get_compound(0).and_then(|c| c.get_int("v")), Some(2));
    }

    /// A caller that replaces a live-child field through `SharedCompoundTag`
    /// detaches the retained child — even when the replacement is an
    /// equal-value empty compound (Java object identity, not value equality).
    #[test]
    fn wrapping_external_replacement_detaches_stale_child() {
        let shared = SharedCompoundTag::new(CompoundTag::new());
        let out = TagValueOutput::create_wrapping_global(reporter(), shared.clone());
        let child = out.child("x");
        // Caller replaces the field with an equal-value empty compound.
        shared.put("x", Tag::Compound(CompoundTag::new()));
        child.put_int("a", 1);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let x = result.get_compound("x").expect("x present");
        assert!(
            x.is_empty(),
            "a stale child's write must not clobber the caller's replacement, got: {result:?}"
        );
    }

    /// The caller's replacement is visible to the output immediately, and a
    /// *fresh* child created after it writes normally.
    #[test]
    fn wrapping_external_put_then_new_child_writes() {
        let shared = SharedCompoundTag::new(CompoundTag::new());
        let out = TagValueOutput::create_wrapping_global(reporter(), shared.clone());
        out.child("x").put_int("old", 1);
        shared.put("x", Tag::Compound(CompoundTag::new()));
        out.child("x").put_int("new", 2);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let x = result.get_compound("x").expect("x present");
        assert_eq!(
            x.get_int("new"),
            Some(2),
            "a fresh child owns the recreated slot, got: {result:?}"
        );
        assert!(
            x.get_int("old").is_none(),
            "stale child detached, got: {result:?}"
        );
    }

    /// `store` on a codec that fails *without* a partial leaves a retained
    /// child attached (Java never touches the field, so the child's object
    /// stays in the map).
    #[test]
    fn store_error_without_partial_keeps_child_attached() {
        let reporter = reporter();
        let out = output(reporter.clone());
        let child = out.child("x");
        child.put_int("a", 1);
        let no_partial: Arc<dyn Codec<i32, TagContextOps>> = codec::validate(
            codec::int_codec::<TagContextOps>(),
            Arc::new(|_: &i32| rivet_serialization::DataResult::error("bad")),
        );
        out.store("x", &no_partial, &5);
        child.put_int("b", 2);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let x = result.get_compound("x").expect("x present");
        assert_eq!(
            x.get_int("a"),
            Some(1),
            "retained child's first write survives, got: {result:?}"
        );
        assert_eq!(
            x.get_int("b"),
            Some(2),
            "no-partial failure does not detach the child, got: {result:?}"
        );
    }

    /// `store` on a codec that fails *with* a partial stores the partial and
    /// detaches the retained child (Java `Error.partialValue()` is put).
    #[test]
    fn store_error_with_partial_replaces_child() {
        let reporter = reporter();
        let out = output(reporter.clone());
        let child = out.child("x");
        child.put_int("a", 1);
        let with_partial: Arc<dyn Codec<i32, TagContextOps>> = codec::validate(
            codec::int_codec::<TagContextOps>(),
            Arc::new(|_: &i32| rivet_serialization::DataResult::error_with_partial("bad", 99)),
        );
        out.store("x", &with_partial, &5);
        child.put_int("b", 2);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        assert_eq!(
            result.get_int("x"),
            Some(99),
            "the partial replaces the field, got: {result:?}"
        );
        assert!(
            reporter
                .get_report()
                .contains("Failed to encode value '5' to field 'x': bad"),
            "report was: {}",
            reporter.get_report()
        );
    }

    // -----------------------------------------------------------------------
    // In-place merge into a live child cell (Java shared-object
    // `CompoundTag.merge` recursion): a compound-over-compound merge at a
    // child-owned field must update the child's own cell — not just the parent
    // slot snapshot — so the child's next write preserves the merged fields.
    // -----------------------------------------------------------------------

    /// A `store_map` whose encoded map contains a nested compound at the
    /// child-owned field `x` (`{a: …, x: {b: …}}`).
    fn nested_store_map_codec()
    -> Arc<dyn rivet_serialization::map_codec::MapCodec<(i32, i32), TagContextOps>> {
        use rivet_serialization::map_codec::codec_of;
        use rivet_serialization::record_builder::{self, RecordCodecBuilder};
        // Inner map codec: a single-int compound {b: n}.
        let inner: Arc<dyn rivet_serialization::map_codec::MapCodec<i32, TagContextOps>> =
            record_builder::map_codec(|instance| {
                instance
                    .group(RecordCodecBuilder::of_named(
                        Arc::new(|n: &i32| *n),
                        "b".to_string(),
                        codec::int_codec::<TagContextOps>(),
                    ))
                    .apply(instance, Arc::new(|b: i32| b))
            });
        let inner_codec = codec_of(inner);
        // Outer map codec: {a: i32, x: {b: i32}}.
        record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of_named(
                    Arc::new(|t: &(i32, i32)| t.0),
                    "a".to_string(),
                    codec::int_codec::<TagContextOps>(),
                ))
                .and(RecordCodecBuilder::of_named(
                    Arc::new(|t: &(i32, i32)| t.1),
                    "x".to_string(),
                    inner_codec,
                ))
                .apply(instance, Arc::new(|a: i32, x: i32| (a, x)))
        })
    }

    /// A **populated** child + `store_map` nested merge: the merged `b` lands
    /// in the child's cell, survives in `buildResult`, and is not dropped by
    /// the child's later write.
    #[test]
    fn store_map_nested_merge_preserves_child_content_and_writes() {
        let out = output(reporter());
        let child = out.child("x");
        child.put_int("a", 1);
        let nested = nested_store_map_codec();
        out.store_map(&nested, &(0, 2));
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let x = result.get_compound("x").expect("x present");
        assert_eq!(
            x.get_int("a"),
            Some(1),
            "existing child content, got: {result:?}"
        );
        assert_eq!(
            x.get_int("b"),
            Some(2),
            "merged field visible, got: {result:?}"
        );
        child.put_int("c", 3);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let x = result.get_compound("x").expect("x present");
        assert_eq!(x.get_int("a"), Some(1), "got: {result:?}");
        assert_eq!(
            x.get_int("b"),
            Some(2),
            "merged field survives child write, got: {result:?}"
        );
        assert_eq!(x.get_int("c"), Some(3));
    }

    /// A **never-written** child + `store_map` nested merge: the merged `b`
    /// lands in the child's empty cell and survives a later write.
    #[test]
    fn store_map_nested_merge_never_written_child() {
        let out = output(reporter());
        let child = out.child("x");
        let nested = nested_store_map_codec();
        out.store_map(&nested, &(0, 5));
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let x = result.get_compound("x").expect("x present");
        assert_eq!(
            x.get_int("b"),
            Some(5),
            "merged into never-written child, got: {result:?}"
        );
        child.put_int("c", 3);
        let result = match &out {
            ValueOutput::Tag(tag) => tag.build_result(),
        };
        let x = result.get_compound("x").expect("x present");
        assert_eq!(
            x.get_int("b"),
            Some(5),
            "merged field survives child write, got: {result:?}"
        );
        assert_eq!(x.get_int("c"), Some(3));
    }

    /// `SharedCompoundTag::merge` at a live-child field merges into the child
    /// cell (Java shared-object semantics): merged fields survive a child
    /// write.
    #[test]
    fn shared_merge_nested_into_child_cell() {
        let shared = SharedCompoundTag::new(CompoundTag::new());
        let out = TagValueOutput::create_wrapping_global(reporter(), shared.clone());
        let child = out.child("x");
        child.put_int("a", 1);
        shared.merge(&CompoundTag::with_map(
            [(
                "x".to_string(),
                Tag::Compound(CompoundTag::with_map(
                    [("b".to_string(), Tag::Int(IntTag::new(2)))]
                        .into_iter()
                        .collect(),
                )),
            )]
            .into_iter()
            .collect(),
        ));
        {
            let tags = shared.borrow();
            let x = tags.get_compound("x").expect("x present");
            assert_eq!(
                x.get_int("a"),
                Some(1),
                "existing child content, got: {x:?}"
            );
            assert_eq!(x.get_int("b"), Some(2), "merged field visible, got: {x:?}");
        }
        child.put_int("c", 3);
        {
            let tags = shared.borrow();
            let x = tags.get_compound("x").expect("x present");
            assert_eq!(
                x.get_int("b"),
                Some(2),
                "merged field survives child write, got: {x:?}"
            );
            assert_eq!(x.get_int("c"), Some(3));
        }
    }

    /// `SharedCompoundTag::merge` recurses through a grandchild cell: a nested
    /// merge at `x.y` updates the grandchild cell, and both the grandchild and
    /// child writes preserve the merged field.
    #[test]
    fn shared_merge_recurses_into_grandchild_cell() {
        let shared = SharedCompoundTag::new(CompoundTag::new());
        let out = TagValueOutput::create_wrapping_global(reporter(), shared.clone());
        let child = out.child("x");
        let grandchild = child.child("y");
        grandchild.put_int("a", 1);
        shared.merge(&CompoundTag::with_map(
            [(
                "x".to_string(),
                Tag::Compound(CompoundTag::with_map(
                    [(
                        "y".to_string(),
                        Tag::Compound(CompoundTag::with_map(
                            [("b".to_string(), Tag::Int(IntTag::new(2)))]
                                .into_iter()
                                .collect(),
                        )),
                    )]
                    .into_iter()
                    .collect(),
                )),
            )]
            .into_iter()
            .collect(),
        ));
        {
            let tags = shared.borrow();
            let x = tags.get_compound("x").expect("x present");
            let y = x.get_compound("y").expect("y present");
            assert_eq!(y.get_int("a"), Some(1), "got: {x:?}");
            assert_eq!(
                y.get_int("b"),
                Some(2),
                "merged into grandchild cell, got: {x:?}"
            );
        }
        grandchild.put_int("c", 3);
        {
            let tags = shared.borrow();
            let x = tags.get_compound("x").expect("x present");
            let y = x.get_compound("y").expect("y present");
            assert_eq!(
                y.get_int("b"),
                Some(2),
                "merged field survives grandchild write, got: {x:?}"
            );
            assert_eq!(y.get_int("c"), Some(3));
        }
    }

    /// Java's `ListWrapper.discardLast()` on an empty list throws
    /// `NoSuchElementException`; the port panics with that message instead of
    /// underflowing the `usize` index.
    #[test]
    #[should_panic(expected = "NoSuchElementException")]
    fn discard_last_on_empty_panics_like_java() {
        let out = output(reporter());
        let list = out.children_list("items");
        list.discard_last();
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
