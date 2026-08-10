//! Port of `net.minecraft.world.level.storage.TagValueInput` — the NBT-backed
//! `ValueInput` (issue #382).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/TagValueInput.java`. Every read reports through the
//! `ProblemReporter` (a field-scoped child when descending into a child/list),
//! and every partial/error ordering mirrors the Java switch on the codec's
//! `DataResult`.
//!
//! Java shares the `CompoundTag` object with the parent (a parsed world tag
//! stays immutable during parsing), so the port clones the wrapped tag into the
//! wrapper — observationally identical for the read-only path.

use crate::level::storage::value_input::{
    EmptyValueInput, TypedInputList, ValueInput, ValueInputList,
};
use crate::level::storage::value_input_context_helper::{TagContextOps, ValueInputContextHelper};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::numeric_tag::NumericTag;
use rivet_nbt::tag::Tag;
use rivet_nbt::tag_type::TagType;
use rivet_registry::access::RegistryAccess;
use rivet_serialization::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_util::problem_reporter::{
    FieldPathElement, IndexedFieldPathElement, IndexedPathElement, Problem, ProblemReporter,
};
use std::rc::Rc;
use std::sync::Arc;

/// `net.minecraft.world.level.storage.TagValueInput`.
pub struct TagValueInput {
    problem_reporter: Rc<dyn ProblemReporter>,
    context: ValueInputContextHelper,
    input: CompoundTag,
}

/// The `ProblemReporter` handle — `Rc` (non-`Send`), confined to the tick
/// thread per OWNERSHIP.
type Reporter = Rc<dyn ProblemReporter>;

impl TagValueInput {
    /// `TagValueInput.create(ProblemReporter, HolderLookup.Provider,
    /// CompoundTag)`.
    pub fn create(
        problem_reporter: Reporter,
        holders: RegistryAccess,
        tag: CompoundTag,
    ) -> ValueInput {
        ValueInput::Tag(TagValueInput::new(
            problem_reporter,
            ValueInputContextHelper::new(holders, NbtOps::instance()),
            tag,
        ))
    }

    /// `TagValueInput.create(ProblemReporter, HolderLookup.Provider,
    /// List<CompoundTag>)` — the list form wrapping a list of compounds.
    pub fn create_list(
        problem_reporter: Reporter,
        holders: RegistryAccess,
        tags: Vec<CompoundTag>,
    ) -> ValueInputList {
        ValueInputList::CompoundList(CompoundListWrapper {
            problem_reporter,
            context: ValueInputContextHelper::new(holders, NbtOps::instance()),
            list: tags,
        })
    }

    fn new(
        problem_reporter: Reporter,
        context: ValueInputContextHelper,
        input: CompoundTag,
    ) -> Self {
        TagValueInput {
            problem_reporter,
            context,
            input,
        }
    }

    /// The wrapped `CompoundTag` (`TagValueInput.input`, Paper's debug accessor).
    pub fn input(&self) -> &CompoundTag {
        &self.input
    }

    /// `TagValueInput.getOptionalTypedTag` — a tag of the exact expected type,
    /// reporting an `UnexpectedTypeProblem` otherwise.
    fn get_optional_typed_tag(&self, name: &str, expected_type: TagType) -> Option<Tag> {
        let tag = self.input.get(name);
        match tag {
            None => None,
            Some(tag) => {
                let actual_type = tag.get_type();
                if actual_type != expected_type {
                    self.problem_reporter
                        .report(Rc::new(UnexpectedTypeProblem::new(
                            name.to_string(),
                            expected_type,
                            actual_type,
                        )));
                    None
                } else {
                    Some(tag.clone())
                }
            }
        }
    }

    /// `TagValueInput.getNumericTag` — any numeric tag, reporting an
    /// `UnexpectedNonNumberProblem` otherwise.
    fn get_numeric_tag(&self, name: &str) -> Option<NumericTag> {
        let tag = self.input.get(name);
        match tag {
            None => None,
            Some(tag) => match NumericTag::try_from(tag) {
                Ok(numeric) => Some(numeric),
                Err(_) => {
                    self.problem_reporter
                        .report(Rc::new(UnexpectedNonNumberProblem::new(
                            name.to_string(),
                            tag.get_type(),
                        )));
                    None
                }
            },
        }
    }

    /// `TagValueInput.wrapChild(String, CompoundTag)` — the empty short-circuit.
    fn wrap_child(&self, name: &str, compound: CompoundTag) -> ValueInput {
        if compound.is_empty() {
            ValueInput::Empty(EmptyValueInput::new(self.context.lookup().clone()))
        } else {
            ValueInput::Tag(TagValueInput::new(
                self.problem_reporter
                    .for_child(Rc::new(FieldPathElement(name.to_string()))),
                ValueInputContextHelper::new(self.context.lookup().clone(), NbtOps::instance()),
                compound,
            ))
        }
    }

    /// The static `wrapChild` — wraps a compound with an already-scoped
    /// reporter.
    fn wrap_child_with_reporter(
        problem_reporter: Reporter,
        context: &ValueInputContextHelper,
        compound: CompoundTag,
    ) -> ValueInput {
        if compound.is_empty() {
            ValueInput::Empty(EmptyValueInput::new(context.lookup().clone()))
        } else {
            ValueInput::Tag(TagValueInput::new(
                problem_reporter,
                ValueInputContextHelper::new(context.lookup().clone(), NbtOps::instance()),
                compound,
            ))
        }
    }

    /// `TagValueInput.wrapList` — the empty short-circuit for a children list.
    fn wrap_list(&self, name: &str, list: ListTag) -> ValueInputList {
        if list.is_empty() {
            ValueInputList::Empty
        } else {
            ValueInputList::Tag(ListWrapper {
                problem_reporter: Rc::clone(&self.problem_reporter),
                name: name.to_string(),
                context: ValueInputContextHelper::new(
                    self.context.lookup().clone(),
                    NbtOps::instance(),
                ),
                list,
            })
        }
    }

    /// `TagValueInput.wrapTypedList` — the empty short-circuit for a typed list.
    fn wrap_typed_list<A>(
        &self,
        name: &str,
        list: ListTag,
        codec: Arc<dyn Codec<A, TagContextOps>>,
    ) -> TypedInputList<A> {
        if list.is_empty() {
            TypedInputList::Empty
        } else {
            TypedInputList::Tag(TypedListWrapper {
                problem_reporter: Rc::clone(&self.problem_reporter),
                name: name.to_string(),
                context: ValueInputContextHelper::new(
                    self.context.lookup().clone(),
                    NbtOps::instance(),
                ),
                codec,
                list,
            })
        }
    }
}

// The `read` half uses `Rc`-reporter helper methods, so `TagValueInput`
// implements the enum-dispatch surface as inherent methods (see `ValueInput`).
impl TagValueInput {
    /// `ValueInput.read(String, Codec<T>)`.
    pub fn read<A>(&self, name: &str, codec: &Arc<dyn Codec<A, TagContextOps>>) -> Option<A>
    where
        A: 'static,
    {
        let ops = self.context.ops();
        let tag = self.input.get(name).cloned()?;
        let result = codec.parse(ops.as_ref(), &tag);
        if result.is_error() {
            let message = result.error_ref().unwrap().message().to_string();
            self.problem_reporter
                .report(Rc::new(DecodeFromFieldFailedProblem::new(
                    name.to_string(),
                    tag,
                    message,
                )));
        }
        result.result_or_partial_silent()
    }

    /// `ValueInput.read(MapCodec<T>)`.
    pub fn read_map<A>(&self, codec: &Arc<dyn MapCodec<A, TagContextOps>>) -> Option<A>
    where
        A: 'static,
    {
        let ops = self.context.ops();
        let result = ops
            .get_map(&Tag::Compound(self.input.clone()))
            .flat_map(|map| codec.decode(ops.as_ref(), map.as_ref()));
        if result.is_error() {
            let message = result.error_ref().unwrap().message().to_string();
            self.problem_reporter
                .report(Rc::new(DecodeFromMapFailedProblem::new(message)));
        }
        result.result_or_partial_silent()
    }

    /// `ValueInput.child(String)`.
    pub fn child(&self, name: &str) -> Option<ValueInput> {
        self.get_optional_typed_tag(name, TagType::Compound)
            .map(|tag| match tag {
                Tag::Compound(compound) => self.wrap_child(name, compound),
                _ => unreachable!("get_optional_typed_tag checked the type"),
            })
    }

    /// `ValueInput.childOrEmpty(String)`.
    pub fn child_or_empty(&self, name: &str) -> ValueInput {
        match self.get_optional_typed_tag(name, TagType::Compound) {
            Some(Tag::Compound(compound)) => self.wrap_child(name, compound),
            _ => ValueInput::Empty(EmptyValueInput::new(self.context.lookup().clone())),
        }
    }

    /// `ValueInput.childrenList(String)`.
    pub fn children_list(&self, name: &str) -> Option<ValueInputList> {
        self.get_optional_typed_tag(name, TagType::List)
            .map(|tag| match tag {
                Tag::List(list) => self.wrap_list(name, list),
                _ => unreachable!("get_optional_typed_tag checked the type"),
            })
    }

    /// `ValueInput.childrenListOrEmpty(String)`.
    pub fn children_list_or_empty(&self, name: &str) -> ValueInputList {
        match self.get_optional_typed_tag(name, TagType::List) {
            Some(Tag::List(list)) => self.wrap_list(name, list),
            _ => ValueInputList::Empty,
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
        self.get_optional_typed_tag(name, TagType::List)
            .map(|tag| match tag {
                Tag::List(list) => self.wrap_typed_list(name, list, codec),
                _ => unreachable!("get_optional_typed_tag checked the type"),
            })
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
        match self.get_optional_typed_tag(name, TagType::List) {
            Some(Tag::List(list)) => self.wrap_typed_list(name, list, codec),
            _ => TypedInputList::Empty,
        }
    }

    /// `ValueInput.getBooleanOr(String, boolean)`.
    pub fn get_boolean_or(&self, name: &str, default_value: bool) -> bool {
        match self.get_numeric_tag(name) {
            Some(numeric) => numeric.byte_value() != 0,
            None => default_value,
        }
    }

    /// `ValueInput.getByteOr(String, byte)`.
    pub fn get_byte_or(&self, name: &str, default_value: i8) -> i8 {
        match self.get_numeric_tag(name) {
            Some(numeric) => numeric.byte_value(),
            None => default_value,
        }
    }

    /// `ValueInput.getShortOr(String, short)`.
    pub fn get_short_or(&self, name: &str, default_value: i16) -> i16 {
        match self.get_numeric_tag(name) {
            Some(numeric) => numeric.short_value(),
            None => default_value,
        }
    }

    /// `ValueInput.getInt(String)`.
    pub fn get_int(&self, name: &str) -> Option<i32> {
        self.get_numeric_tag(name)
            .map(|numeric| numeric.int_value())
    }

    /// `ValueInput.getIntOr(String, int)`.
    pub fn get_int_or(&self, name: &str, default_value: i32) -> i32 {
        match self.get_numeric_tag(name) {
            Some(numeric) => numeric.int_value(),
            None => default_value,
        }
    }

    /// `ValueInput.getLongOr(String, long)`.
    pub fn get_long_or(&self, name: &str, default_value: i64) -> i64 {
        match self.get_numeric_tag(name) {
            Some(numeric) => numeric.long_value(),
            None => default_value,
        }
    }

    /// `ValueInput.getLong(String)`.
    pub fn get_long(&self, name: &str) -> Option<i64> {
        self.get_numeric_tag(name)
            .map(|numeric| numeric.long_value())
    }

    /// `ValueInput.getFloatOr(String, float)`.
    pub fn get_float_or(&self, name: &str, default_value: f32) -> f32 {
        match self.get_numeric_tag(name) {
            Some(numeric) => numeric.float_value(),
            None => default_value,
        }
    }

    /// `ValueInput.getDoubleOr(String, double)`.
    pub fn get_double_or(&self, name: &str, default_value: f64) -> f64 {
        match self.get_numeric_tag(name) {
            Some(numeric) => numeric.double_value(),
            None => default_value,
        }
    }

    /// `ValueInput.getString(String)`.
    pub fn get_string(&self, name: &str) -> Option<String> {
        self.get_optional_typed_tag(name, TagType::String)
            .map(|tag| match tag {
                Tag::String(string) => string.value,
                _ => unreachable!("get_optional_typed_tag checked the type"),
            })
    }

    /// `ValueInput.getStringOr(String, String)`.
    pub fn get_string_or(&self, name: &str, default_value: &str) -> String {
        match self.get_optional_typed_tag(name, TagType::String) {
            Some(Tag::String(string)) => string.value,
            _ => default_value.to_string(),
        }
    }

    /// `ValueInput.getIntArray(String)`.
    pub fn get_int_array(&self, name: &str) -> Option<Vec<i32>> {
        self.get_optional_typed_tag(name, TagType::IntArray)
            .map(|tag| match tag {
                Tag::IntArray(array) => array.get_as_int_array().clone(),
                _ => unreachable!("get_optional_typed_tag checked the type"),
            })
    }

    /// `ValueInput.lookup()`.
    pub fn lookup(&self) -> &RegistryAccess {
        self.context.lookup()
    }
}

// ---------------------------------------------------------------------------
// Problem records
// ---------------------------------------------------------------------------

/// `TagValueInput.DecodeFromFieldFailedProblem`.
#[derive(Debug)]
pub struct DecodeFromFieldFailedProblem {
    name: String,
    tag: Tag,
    message: String,
}

impl DecodeFromFieldFailedProblem {
    fn new(name: String, tag: Tag, message: String) -> Self {
        DecodeFromFieldFailedProblem { name, tag, message }
    }
}

impl Problem for DecodeFromFieldFailedProblem {
    fn description(&self) -> String {
        format!(
            "Failed to decode value '{}' from field '{}': {}",
            self.tag, self.name, self.message
        )
    }
}

/// `TagValueInput.DecodeFromListFailedProblem`.
#[derive(Debug)]
pub struct DecodeFromListFailedProblem {
    name: String,
    index: usize,
    tag: Tag,
    message: String,
}

impl DecodeFromListFailedProblem {
    fn new(name: String, index: usize, tag: Tag, message: String) -> Self {
        DecodeFromListFailedProblem {
            name,
            index,
            tag,
            message,
        }
    }
}

impl Problem for DecodeFromListFailedProblem {
    fn description(&self) -> String {
        format!(
            "Failed to decode value '{}' from field '{}' at index {}: {}",
            self.tag, self.name, self.index, self.message
        )
    }
}

/// `TagValueInput.DecodeFromMapFailedProblem`.
#[derive(Debug)]
pub struct DecodeFromMapFailedProblem {
    message: String,
}

impl DecodeFromMapFailedProblem {
    fn new(message: String) -> Self {
        DecodeFromMapFailedProblem { message }
    }
}

impl Problem for DecodeFromMapFailedProblem {
    fn description(&self) -> String {
        format!("Failed to decode from map: {}", self.message)
    }
}

/// `TagValueInput.UnexpectedListElementTypeProblem`.
#[derive(Debug)]
pub struct UnexpectedListElementTypeProblem {
    name: String,
    index: usize,
    expected: TagType,
    actual: TagType,
}

impl Problem for UnexpectedListElementTypeProblem {
    fn description(&self) -> String {
        format!(
            "Expected list '{}' to contain at index {} value of type {}, but got {}",
            self.name,
            self.index,
            self.expected.name(),
            self.actual.name()
        )
    }
}

/// `TagValueInput.UnexpectedNonNumberProblem`.
#[derive(Debug)]
pub struct UnexpectedNonNumberProblem {
    name: String,
    actual: TagType,
}

impl UnexpectedNonNumberProblem {
    fn new(name: String, actual: TagType) -> Self {
        UnexpectedNonNumberProblem { name, actual }
    }
}

impl Problem for UnexpectedNonNumberProblem {
    fn description(&self) -> String {
        format!(
            "Expected field '{}' to contain number, but got {}",
            self.name,
            self.actual.name()
        )
    }
}

/// `TagValueInput.UnexpectedTypeProblem`.
#[derive(Debug)]
pub struct UnexpectedTypeProblem {
    name: String,
    expected: TagType,
    actual: TagType,
}

impl UnexpectedTypeProblem {
    fn new(name: String, expected: TagType, actual: TagType) -> Self {
        UnexpectedTypeProblem {
            name,
            expected,
            actual,
        }
    }
}

impl Problem for UnexpectedTypeProblem {
    fn description(&self) -> String {
        format!(
            "Expected field '{}' to contain value of type {}, but got {}",
            self.name,
            self.expected.name(),
            self.actual.name()
        )
    }
}

// ---------------------------------------------------------------------------
// List wrappers
// ---------------------------------------------------------------------------

/// `TagValueInput.CompoundListWrapper` — a `ValueInputList` over a
/// `List<CompoundTag>` (the list-form factory).
pub struct CompoundListWrapper {
    problem_reporter: Reporter,
    context: ValueInputContextHelper,
    list: Vec<CompoundTag>,
}

impl CompoundListWrapper {
    fn wrap_child(&self, index: usize, compound: CompoundTag) -> ValueInput {
        TagValueInput::wrap_child_with_reporter(
            self.problem_reporter
                .for_child(Rc::new(IndexedPathElement(index as i32))),
            &self.context,
            compound,
        )
    }

    /// `ValueInputList.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// `ValueInputList.stream()` — Java's
    /// `Streams.mapWithIndex(...).filter(Objects::nonNull)` (every element is a
    /// compound, so nothing is dropped).
    pub fn stream(&self) -> Box<dyn Iterator<Item = ValueInput> + '_> {
        let children = self
            .list
            .iter()
            .enumerate()
            .map(|(index, value)| self.wrap_child(index, value.clone()))
            .collect::<Vec<_>>();
        Box::new(children.into_iter())
    }
}

/// `TagValueInput.ListWrapper` — a children list over a `ListTag`.
pub struct ListWrapper {
    problem_reporter: Reporter,
    name: String,
    context: ValueInputContextHelper,
    list: ListTag,
}

impl ListWrapper {
    fn reporter_for_child(&self, index: usize) -> Reporter {
        self.problem_reporter
            .for_child(Rc::new(IndexedFieldPathElement(
                self.name.clone(),
                index as i32,
            )))
    }

    fn report_index_unwrap_problem(&self, index: usize, value: &Tag) {
        self.problem_reporter
            .report(Rc::new(UnexpectedListElementTypeProblem {
                name: self.name.clone(),
                index,
                expected: TagType::Compound,
                actual: value.get_type(),
            }));
    }

    fn wrap_child(&self, index: usize, compound: CompoundTag) -> ValueInput {
        TagValueInput::wrap_child_with_reporter(
            self.reporter_for_child(index),
            &self.context,
            compound,
        )
    }

    /// `ValueInputList.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// `ValueInputList.stream()` — Java's
    /// `Streams.mapWithIndex(...).filter(Objects::nonNull)`: a non-compound
    /// element reports an `UnexpectedListElementTypeProblem` and is dropped.
    pub fn stream(&self) -> Box<dyn Iterator<Item = ValueInput> + '_> {
        let mut children = Vec::new();
        for (index, value) in self
            .list
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .enumerate()
        {
            match value {
                Tag::Compound(compound) => children.push(self.wrap_child(index, compound)),
                other => self.report_index_unwrap_problem(index, &other),
            }
        }
        Box::new(children.into_iter())
    }
}

/// `TagValueInput.TypedListWrapper<T>` — a typed list over a `ListTag`.
pub struct TypedListWrapper<A> {
    problem_reporter: Reporter,
    name: String,
    context: ValueInputContextHelper,
    codec: Arc<dyn Codec<A, TagContextOps>>,
    list: ListTag,
}

impl<A> TypedListWrapper<A> {
    /// `TypedInputList.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// `TypedInputList.stream()` — Java's
    /// `Streams.mapWithIndex(...).filter(Objects::nonNull)`: a codec error
    /// reports and yields the partial value (dropped when absent).
    pub fn stream(&self) -> Box<dyn Iterator<Item = A> + '_>
    where
        A: 'static,
    {
        let ops = Arc::clone(self.context.ops());
        let mut values = Vec::new();
        for (index, value) in self
            .list
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .enumerate()
        {
            let result = self.codec.parse(ops.as_ref(), &value);
            if result.is_error() {
                let message = result.error_ref().unwrap().message().to_string();
                self.problem_reporter
                    .report(Rc::new(DecodeFromListFailedProblem::new(
                        self.name.clone(),
                        index,
                        value,
                        message,
                    )));
            }
            if let Some(decoded) = result.result_or_partial_silent() {
                values.push(decoded);
            }
        }
        Box::new(values.into_iter())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::storage::value_input::ValueInput;
    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_nbt::double_tag::DoubleTag;
    use rivet_nbt::float_tag::FloatTag;
    use rivet_nbt::int_tag::IntTag;
    use rivet_nbt::long_tag::LongTag;
    use rivet_nbt::string_tag::StringTag;
    use rivet_serialization::codec::{self, Codec};
    use rivet_util::problem_reporter::Collector;
    use std::rc::Rc;

    fn reporter() -> Rc<Collector> {
        Rc::new(Collector::new())
    }

    fn tag_input(reporter: Rc<Collector>, tag: CompoundTag) -> ValueInput {
        TagValueInput::create(reporter, RegistryAccess::empty(), tag)
    }

    /// Java `getIntOr`/`getLongOr`/... narrow a numeric tag of any width.
    #[test]
    fn numeric_readers_narrow_any_numeric_width() {
        let input = tag_input(
            reporter(),
            CompoundTag::with_map(
                [
                    ("d".to_string(), Tag::Double(DoubleTag::new(7.0))),
                    ("l".to_string(), Tag::Long(LongTag::new(2))),
                    ("f".to_string(), Tag::Float(FloatTag::new(3.5))),
                ]
                .into_iter()
                .collect(),
            ),
        );
        assert_eq!(input.get_int_or("d", -1), 7);
        assert_eq!(input.get_byte_or("l", -1), 2);
        assert_eq!(input.get_int("f"), Some(3));
        assert!(input.get_boolean_or("l", false));
    }

    /// Java's `getIntOr` on a *non-numeric* tag reports and falls back to the
    /// default.
    #[test]
    fn numeric_reader_on_string_reports_and_uses_default() {
        let reporter = reporter();
        let input = tag_input(
            reporter.clone(),
            CompoundTag::with_map(
                [(
                    "s".to_string(),
                    Tag::String(StringTag::value_of("x".to_string())),
                )]
                .into_iter()
                .collect(),
            ),
        );
        assert_eq!(input.get_int_or("s", 42), 42);
        let report = reporter.get_report();
        assert!(
            report.contains("Expected field 's' to contain number, but got STRING"),
            "report was: {report}"
        );
    }

    /// Absent numeric/string fields fall back to the default without a report.
    #[test]
    fn absent_fields_fall_back_silently() {
        let reporter = reporter();
        let input = tag_input(reporter.clone(), CompoundTag::new());
        assert_eq!(input.get_int_or("missing", 7), 7);
        assert_eq!(input.get_string_or("missing", "d"), "d");
        assert_eq!(input.get_double_or("missing", 0.5), 0.5);
        assert!(input.get_int("missing").is_none());
        assert!(input.get_string("missing").is_none());
        assert!(reporter.is_empty(), "no problems for absent fields");
    }

    /// `read` with an error codec reports and yields the partial value (Java's
    /// `Error.partialValue()`).
    #[test]
    fn read_error_yields_partial_and_reports() {
        let reporter = reporter();
        let failing: Arc<dyn Codec<i32, TagContextOps>> = codec::validate(
            codec::int_codec::<TagContextOps>(),
            Arc::new(|_: &i32| {
                rivet_serialization::DataResult::error_with_partial("bad value", 99)
            }),
        );
        let input = tag_input(
            reporter.clone(),
            CompoundTag::with_map(
                [("name".to_string(), Tag::Int(IntTag::new(5)))]
                    .into_iter()
                    .collect(),
            ),
        );
        let value = input.read("name", &failing);
        assert_eq!(value, Some(99), "error with partial yields the partial");
        let report = reporter.get_report();
        assert!(
            report.contains("Failed to decode value '5' from field 'name': bad value"),
            "report was: {report}"
        );
    }

    /// `read` with an absent field is `None` and silent.
    #[test]
    fn read_absent_is_none_and_silent() {
        let reporter = reporter();
        let input = tag_input(reporter.clone(), CompoundTag::new());
        assert!(
            input
                .read("missing", &codec::int_codec::<TagContextOps>())
                .is_none()
        );
        assert!(reporter.is_empty());
    }

    /// `child`/`childOrEmpty` on a non-compound reports the type mismatch.
    #[test]
    fn child_type_mismatch_reports() {
        let reporter = reporter();
        let input = tag_input(
            reporter.clone(),
            CompoundTag::with_map(
                [("c".to_string(), Tag::Int(IntTag::new(1)))]
                    .into_iter()
                    .collect(),
            ),
        );
        assert!(input.child("c").is_none());
        assert!(
            reporter
                .get_report()
                .contains("Expected field 'c' to contain value of type COMPOUND, but got INT"),
            "report was: {}",
            reporter.get_report()
        );
    }

    /// `childrenList` drops non-compound elements with a problem, keeping the
    /// compounds (Java's `filter(Objects::nonNull)`).
    #[test]
    fn children_list_drops_non_compounds_with_report() {
        let reporter = reporter();
        let list = rivet_nbt::list_tag::ListTag::with_list(vec![
            Tag::Compound(CompoundTag::with_map(
                [("a".to_string(), Tag::Int(IntTag::new(1)))]
                    .into_iter()
                    .collect(),
            )),
            Tag::Int(IntTag::new(2)),
            Tag::Compound(CompoundTag::with_map(
                [("b".to_string(), Tag::Int(IntTag::new(3)))]
                    .into_iter()
                    .collect(),
            )),
        ]);
        let input = tag_input(
            reporter.clone(),
            CompoundTag::with_map(
                [("list".to_string(), Tag::List(list))]
                    .into_iter()
                    .collect(),
            ),
        );
        let children = input.children_list_or_empty("list");
        let values = children.stream().collect::<Vec<_>>();
        assert_eq!(values.len(), 2, "non-compound dropped");
        assert_eq!(values[0].get_int_or("a", 0), 1);
        assert_eq!(values[1].get_int_or("b", 0), 3);
        assert!(
            reporter.get_report().contains(
                "Expected list 'list' to contain at index 1 value of type COMPOUND, but got INT"
            ),
            "report was: {}",
            reporter.get_report()
        );
    }

    /// The typed list reports per-element decode failures (with the element's
    /// index) and yields partial values.
    #[test]
    fn typed_list_reports_and_keeps_partials() {
        let reporter = reporter();
        let list = rivet_nbt::list_tag::ListTag::with_list(vec![
            Tag::Int(IntTag::new(1)),
            Tag::Int(IntTag::new(2)),
            Tag::Int(IntTag::new(3)),
        ]);
        let input = tag_input(
            reporter.clone(),
            CompoundTag::with_map(
                [("list".to_string(), Tag::List(list))]
                    .into_iter()
                    .collect(),
            ),
        );
        // A codec failing on value 2 with a partial.
        let codec: Arc<dyn Codec<i32, TagContextOps>> = codec::validate(
            codec::int_codec::<TagContextOps>(),
            Arc::new(|n: &i32| {
                if *n == 2 {
                    rivet_serialization::DataResult::error_with_partial("unlucky", 20)
                } else {
                    rivet_serialization::DataResult::success(*n)
                }
            }),
        );
        let values = input
            .list_or_empty("list", codec)
            .stream()
            .collect::<Vec<_>>();
        assert_eq!(
            values,
            vec![1, 20, 3],
            "partial replaces the failed element"
        );
        assert!(
            reporter
                .get_report()
                .contains("Failed to decode value '2' from field 'list' at index 1: unlucky"),
            "report was: {}",
            reporter.get_report()
        );
    }

    /// An empty children list yields the shared empty list without wrapping.
    #[test]
    fn empty_list_is_shared_empty() {
        let reporter = reporter();
        let input = tag_input(
            reporter.clone(),
            CompoundTag::with_map(
                [(
                    "list".to_string(),
                    Tag::List(rivet_nbt::list_tag::ListTag::new()),
                )]
                .into_iter()
                .collect(),
            ),
        );
        let children = input.children_list_or_empty("list");
        assert!(children.is_empty());
        assert_eq!(children.stream().count(), 0);
        assert!(reporter.is_empty());
    }

    /// `getString` on a non-string reports the type mismatch.
    #[test]
    fn string_type_mismatch_reports() {
        let reporter = reporter();
        let input = tag_input(
            reporter.clone(),
            CompoundTag::with_map(
                [("name".to_string(), Tag::Int(IntTag::new(1)))]
                    .into_iter()
                    .collect(),
            ),
        );
        assert!(input.get_string("name").is_none());
        assert!(
            reporter
                .get_report()
                .contains("Expected field 'name' to contain value of type STRING, but got INT")
        );
    }

    /// `lookup()` returns the provider passed to `create`.
    #[test]
    fn lookup_returns_provider() {
        let input = tag_input(reporter(), CompoundTag::new());
        assert_eq!(input.lookup().registries().len(), 0);
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

    /// A `RegistryAccess` holding one frozen registry (`minecraft:test`)
    /// populated with `minecraft:alpha` — built through the public
    /// `from_registry_of_registries` path (the same erased-boundary the
    /// registry-op tests use).
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

    /// A `RegistryFileCodec<TestElement>` — decodes an identifier string to a
    /// `Holder::Reference` through the ops' registry context.
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

    /// A registry-grounded codec resolves through the value layer's
    /// serialization-context ops (Java `ValueInputContextHelper` builds
    /// `lookup.createSerializationContext(NbtOps)`).
    #[test]
    fn registry_context_decodes_holder_reference() {
        let access = access_with_registry();
        let reporter = reporter();
        let input = TagValueInput::create(
            reporter.clone(),
            access.clone(),
            CompoundTag::with_map(
                [(
                    "e".to_string(),
                    Tag::String(StringTag::value_of("minecraft:alpha".to_string())),
                )]
                .into_iter()
                .collect(),
            ),
        );
        let holder = input.read("e", &element_codec());
        let holder = holder.expect("registry-grounded decode succeeds");
        let registry_id = access
            .lookup::<TestElement>(&registry_key())
            .expect("frozen registry")
            .registry_id();
        assert_eq!(
            holder,
            rivet_registry::Holder::reference(registry_id, 0),
            "an identifier decodes to the registry reference"
        );
        assert!(reporter.is_empty(), "no problems on the happy path");
    }

    /// The context-less ops (empty access) reports the missing registry
    /// through the value layer, matching Java's error on a plain `NbtOps`.
    #[test]
    fn registry_context_missing_registry_reports() {
        let reporter = reporter();
        let input = TagValueInput::create(
            reporter.clone(),
            RegistryAccess::empty(),
            CompoundTag::with_map(
                [(
                    "e".to_string(),
                    Tag::String(StringTag::value_of("minecraft:alpha".to_string())),
                )]
                .into_iter()
                .collect(),
            ),
        );
        let holder = input.read("e", &element_codec());
        assert!(holder.is_none(), "missing registry fails the decode");
        assert!(
            !reporter.is_empty(),
            "the decode failure is reported through the value layer"
        );
        assert!(
            reporter.get_report().contains("Failed to decode value"),
            "report was: {}",
            reporter.get_report()
        );
    }
}
