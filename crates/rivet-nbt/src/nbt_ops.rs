//! Port of `net.minecraft.nbt.NbtOps` — `DynamicOps<Tag>`.
//!
//! Part of the irreducible SCC: `CompoundTag.CODEC`/`store`/`read` and
//! `TagParser` build on it, and it builds on the tag hierarchy.

use crate::byte_array_tag::ByteArrayTag;
use crate::byte_tag::ByteTag;
use crate::compound_tag::CompoundTag;
use crate::double_tag::DoubleTag;
use crate::end_tag::EndTag;
use crate::float_tag::FloatTag;
use crate::int_array_tag::IntArrayTag;
use crate::int_tag::IntTag;
use crate::list_tag::ListTag;
use crate::long_array_tag::LongArrayTag;
use crate::long_tag::LongTag;
use crate::short_tag::ShortTag;
use crate::string_tag::StringTag;
use crate::tag::Tag;
use rivet_serialization::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, MapLike, Pair};

/// `NbtOps.INSTANCE`.
#[derive(Debug, Default, Clone, Copy)]
pub struct NbtOps;

impl NbtOps {
    pub fn instance() -> Self {
        NbtOps
    }
}

/// `MapLike<Tag>` over a `CompoundTag` (owns a clone so the box is `'static`).
#[derive(Debug)]
struct CompoundMapLike {
    tag: CompoundTag,
}

impl MapLike<Tag> for CompoundMapLike {
    fn get(&self, key: &Tag) -> Option<Tag> {
        match key {
            Tag::String(s) => self.tag.get(&s.value).cloned(),
            other => panic!("Cannot get map entry with non-string key: {other:?}"),
        }
    }

    fn get_string(&self, key: &str) -> Option<Tag> {
        self.tag.get(key).cloned()
    }

    fn entries(&self) -> Vec<Pair<Tag, Tag>> {
        self.tag
            .entry_set()
            .map(|(k, v)| Pair::of(Tag::String(StringTag::value_of(k.clone())), v.clone()))
            .collect()
    }
}

impl DynamicOps for NbtOps {
    type Output = Tag;

    fn empty(&self) -> Tag {
        Tag::End(EndTag)
    }

    fn empty_list(&self) -> Tag {
        Tag::List(ListTag::new())
    }

    fn empty_map(&self) -> Tag {
        Tag::Compound(CompoundTag::new())
    }

    fn get_number_value(&self, input: &Tag) -> DataResult<f64> {
        // Fidelity note: Java `NbtOps.getNumberValue` returns the typed boxed
        // `Number` (see `Tag.asNumber`). The DFU stub surfaces `f64`, losing
        // the exact type and precision above 2^53; a typed `Number` enum lands
        // with the real DFU port.
        match input.as_number() {
            Some(n) => DataResult::success(n),
            None => DataResult::error("Not a number"),
        }
    }

    fn create_numeric(&self, value: f64) -> Tag {
        Tag::Double(DoubleTag::value_of(value))
    }

    fn create_byte(&self, value: i8) -> Tag {
        Tag::Byte(ByteTag::value_of(value))
    }

    fn create_short(&self, value: i16) -> Tag {
        Tag::Short(ShortTag::value_of(value))
    }

    fn create_int(&self, value: i32) -> Tag {
        Tag::Int(IntTag::value_of(value))
    }

    fn create_long(&self, value: i64) -> Tag {
        Tag::Long(LongTag::value_of(value))
    }

    fn create_float(&self, value: f32) -> Tag {
        Tag::Float(FloatTag::value_of(value))
    }

    fn create_double(&self, value: f64) -> Tag {
        Tag::Double(DoubleTag::value_of(value))
    }

    fn get_boolean_value(&self, input: &Tag) -> DataResult<bool> {
        self.get_number_value(input).map(|v| *v != 0.0)
    }

    fn create_boolean(&self, value: bool) -> Tag {
        Tag::Byte(ByteTag::value_of_bool(value))
    }

    fn get_string_value(&self, input: &Tag) -> DataResult<String> {
        match input {
            Tag::String(s) => DataResult::success(s.value.clone()),
            _ => DataResult::error("Not a string"),
        }
    }

    fn create_string(&self, value: String) -> Tag {
        Tag::String(StringTag::value_of(value))
    }

    fn merge_to_list(&self, list: &Tag, value: Tag) -> DataResult<Tag> {
        match create_collector(list) {
            Some(collector) => DataResult::success(collector.accept(value).result()),
            None => DataResult::error_with_partial(
                format!(
                    "mergeToList called with not a list: {}",
                    crate::string_tag_visitor::StringTagVisitor::to_string(list)
                ),
                list.clone(),
            ),
        }
    }

    fn merge_to_list_many(&self, list: &Tag, values: Vec<Tag>) -> DataResult<Tag> {
        match create_collector(list) {
            Some(collector) => {
                let mut c = collector;
                for v in values {
                    c = c.accept(v);
                }
                DataResult::success(c.result())
            }
            None => DataResult::error_with_partial(
                format!(
                    "mergeToList called with not a list: {}",
                    crate::string_tag_visitor::StringTagVisitor::to_string(list)
                ),
                list.clone(),
            ),
        }
    }

    fn merge_to_map(&self, map: &Tag, key: Tag, value: Tag) -> DataResult<Tag> {
        if !matches!(map, Tag::Compound(_) | Tag::End(_)) {
            return DataResult::error_with_partial(
                format!(
                    "mergeToMap called with not a map: {}",
                    crate::string_tag_visitor::StringTagVisitor::to_string(map)
                ),
                map.clone(),
            );
        }
        match key {
            Tag::String(s) => {
                let mut output = match map {
                    Tag::Compound(t) => t.shallow_copy(),
                    _ => CompoundTag::new(),
                };
                output.put(s.value, value);
                DataResult::success(Tag::Compound(output))
            }
            other => DataResult::error_with_partial(
                format!(
                    "key is not a string: {}",
                    crate::string_tag_visitor::StringTagVisitor::to_string(&other)
                ),
                map.clone(),
            ),
        }
    }

    fn merge_to_map_many(&self, map: &Tag, values: Vec<Pair<Tag, Tag>>) -> DataResult<Tag> {
        if !matches!(map, Tag::Compound(_) | Tag::End(_)) {
            return DataResult::error_with_partial(
                format!(
                    "mergeToMap called with not a map: {}",
                    crate::string_tag_visitor::StringTagVisitor::to_string(map)
                ),
                map.clone(),
            );
        }
        if values.is_empty() {
            return if *map == self.empty() {
                DataResult::success(self.empty_map())
            } else {
                DataResult::success(map.clone())
            };
        }
        let mut output = match map {
            Tag::Compound(t) => t.shallow_copy(),
            _ => CompoundTag::new(),
        };
        let mut missed = Vec::new();
        for entry in values {
            match entry.first {
                Tag::String(s) => {
                    output.put(s.value, entry.second);
                }
                key => missed.push(key),
            }
        }
        if !missed.is_empty() {
            let missed_snbt: Vec<String> = missed
                .iter()
                .map(crate::string_tag_visitor::StringTagVisitor::to_string)
                .collect();
            DataResult::error_with_partial(
                format!("some keys are not strings: {missed_snbt:?}"),
                Tag::Compound(output),
            )
        } else {
            DataResult::success(Tag::Compound(output))
        }
    }

    fn get_map_values(&self, input: &Tag) -> DataResult<Vec<Pair<Tag, Tag>>> {
        match input {
            Tag::Compound(t) => DataResult::success(
                t.entry_set()
                    .map(|(k, v)| Pair::of(Tag::String(StringTag::value_of(k.clone())), v.clone()))
                    .collect(),
            ),
            _ => DataResult::error(format!(
                "Not a map: {}",
                crate::string_tag_visitor::StringTagVisitor::to_string(input)
            )),
        }
    }

    fn get_map_entries(&self, input: &Tag) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&Tag, &Tag))>> {
        match input {
            Tag::Compound(t) => {
                let entries: Vec<(String, Tag)> =
                    t.entry_set().map(|(k, v)| (k.clone(), v.clone())).collect();
                DataResult::success(Box::new(move |c: &mut dyn FnMut(&Tag, &Tag)| {
                    for (k, v) in &entries {
                        c(&Tag::String(StringTag::value_of(k.clone())), v);
                    }
                }))
            }
            _ => DataResult::error(format!(
                "Not a map: {}",
                crate::string_tag_visitor::StringTagVisitor::to_string(input)
            )),
        }
    }

    fn get_map(&self, input: &Tag) -> DataResult<Box<dyn MapLike<Tag>>> {
        match input {
            Tag::Compound(t) => DataResult::success(Box::new(CompoundMapLike { tag: t.clone() })),
            _ => DataResult::error(format!(
                "Not a map: {}",
                crate::string_tag_visitor::StringTagVisitor::to_string(input)
            )),
        }
    }

    fn create_map(&self, map: Vec<Pair<Tag, Tag>>) -> Tag {
        let mut tag = CompoundTag::new();
        for entry in map {
            match entry.first {
                Tag::String(s) => {
                    tag.put(s.value, entry.second);
                }
                other => panic!("Cannot create map with non-string key: {other:?}"),
            }
        }
        Tag::Compound(tag)
    }

    fn get_stream(&self, input: &Tag) -> DataResult<Vec<Tag>> {
        match input {
            Tag::List(t) => DataResult::success(t.list.clone()),
            Tag::ByteArray(_) | Tag::IntArray(_) | Tag::LongArray(_) => {
                // Java: `CollectionTag.stream()`.
                match input {
                    Tag::ByteArray(t) => DataResult::success(
                        t.data
                            .iter()
                            .map(|v| Tag::Byte(ByteTag::value_of(*v)))
                            .collect(),
                    ),
                    Tag::IntArray(t) => DataResult::success(
                        t.data
                            .iter()
                            .map(|v| Tag::Int(IntTag::value_of(*v)))
                            .collect(),
                    ),
                    Tag::LongArray(t) => DataResult::success(
                        t.data
                            .iter()
                            .map(|v| Tag::Long(LongTag::value_of(*v)))
                            .collect(),
                    ),
                    _ => unreachable!(),
                }
            }
            _ => DataResult::error("Not a list"),
        }
    }

    fn get_list(&self, input: &Tag) -> DataResult<Box<dyn Fn(&mut dyn FnMut(&Tag))>> {
        fn snbt(input: &Tag) -> String {
            crate::string_tag_visitor::StringTagVisitor::to_string(input)
        }
        match input {
            Tag::List(t) => {
                let items = t.list.clone();
                DataResult::success(Box::new(move |c: &mut dyn FnMut(&Tag)| {
                    for v in &items {
                        c(v);
                    }
                }))
            }
            Tag::ByteArray(t) => {
                let items = t.data.clone();
                DataResult::success(Box::new(move |c: &mut dyn FnMut(&Tag)| {
                    for v in &items {
                        c(&Tag::Byte(ByteTag::value_of(*v)));
                    }
                }))
            }
            Tag::IntArray(t) => {
                let items = t.data.clone();
                DataResult::success(Box::new(move |c: &mut dyn FnMut(&Tag)| {
                    for v in &items {
                        c(&Tag::Int(IntTag::value_of(*v)));
                    }
                }))
            }
            Tag::LongArray(t) => {
                let items = t.data.clone();
                DataResult::success(Box::new(move |c: &mut dyn FnMut(&Tag)| {
                    for v in &items {
                        c(&Tag::Long(LongTag::value_of(*v)));
                    }
                }))
            }
            _ => DataResult::error(format!("Not a list: {}", snbt(input))),
        }
    }

    fn get_byte_buffer(&self, input: &Tag) -> DataResult<Vec<u8>> {
        match input {
            Tag::ByteArray(t) => DataResult::success(t.data.iter().map(|v| *v as u8).collect()),
            _ => DataResult::error(format!(
                "Not a byte buffer: {}",
                crate::string_tag_visitor::StringTagVisitor::to_string(input)
            )),
        }
    }

    fn create_byte_list(&self, input: &[u8]) -> Tag {
        Tag::ByteArray(ByteArrayTag::new(input.iter().map(|v| *v as i8).collect()))
    }

    fn get_int_stream(&self, input: &Tag) -> DataResult<Vec<i32>> {
        match input {
            Tag::IntArray(t) => DataResult::success(t.data.clone()),
            _ => DataResult::error(format!(
                "Not an int stream: {}",
                crate::string_tag_visitor::StringTagVisitor::to_string(input)
            )),
        }
    }

    fn create_int_list(&self, input: Vec<i32>) -> Tag {
        Tag::IntArray(IntArrayTag::new(input))
    }

    fn get_long_stream(&self, input: &Tag) -> DataResult<Vec<i64>> {
        match input {
            Tag::LongArray(t) => DataResult::success(t.data.clone()),
            _ => DataResult::error(format!(
                "Not a long stream: {}",
                crate::string_tag_visitor::StringTagVisitor::to_string(input)
            )),
        }
    }

    fn create_long_list(&self, input: Vec<i64>) -> Tag {
        Tag::LongArray(LongArrayTag::new(input))
    }

    fn create_list(&self, input: Vec<Tag>) -> Tag {
        Tag::List(ListTag::with_list(input))
    }

    fn remove(&self, input: Tag, key: &str) -> Tag {
        match input {
            Tag::Compound(t) => {
                let mut result = t.shallow_copy();
                result.remove(key);
                Tag::Compound(result)
            }
            other => other,
        }
    }

    fn map_builder(&self) -> Box<dyn rivet_serialization::RecordBuilder<Output = Tag>> {
        Box::new(NbtRecordBuilder)
    }

    fn convert_to<U: DynamicOps>(&self, out_ops: &U, input: &Tag) -> U::Output {
        match input {
            Tag::End(_) => out_ops.empty(),
            Tag::Byte(t) => out_ops.create_byte(t.value),
            Tag::Short(t) => out_ops.create_short(t.value),
            Tag::Int(t) => out_ops.create_int(t.value),
            Tag::Long(t) => out_ops.create_long(t.value),
            Tag::Float(t) => out_ops.create_float(t.value),
            Tag::Double(t) => out_ops.create_double(t.value),
            Tag::ByteArray(t) => {
                out_ops.create_byte_list(&t.data.iter().map(|v| *v as u8).collect::<Vec<_>>())
            }
            Tag::String(t) => out_ops.create_string(t.value.clone()),
            Tag::List(t) => {
                let converted: Vec<_> =
                    t.list.iter().map(|v| self.convert_to(out_ops, v)).collect();
                out_ops.create_list(converted)
            }
            Tag::Compound(t) => {
                let converted: Vec<_> = t
                    .entry_set()
                    .map(|(k, v)| {
                        Pair::of(
                            out_ops.create_string(k.clone()),
                            self.convert_to(out_ops, v),
                        )
                    })
                    .collect();
                out_ops.create_map(converted)
            }
            Tag::IntArray(t) => out_ops.create_int_list(t.data.clone()),
            Tag::LongArray(t) => out_ops.create_long_list(t.data.clone()),
        }
    }
}

/// `AbstractStringBuilder<Tag, CompoundTag>` — builds a compound from string
/// keyed entries, merging into a prefix.
#[derive(Debug, Clone, Copy)]
struct NbtRecordBuilder;

impl rivet_serialization::RecordBuilder for NbtRecordBuilder {
    type Output = Tag;

    fn build(&self, prefix: Option<Tag>) -> DataResult<Tag> {
        // This reduced builder accumulates nothing itself; the concrete
        // `NbtRecordBuilder` in the full port appends into a builder. Here we
        // only support the merge-from-prefix shape used by `CompoundTag.store`
        // via `MapCodec` — see `compound_tag.rs`.
        let _ = prefix;
        DataResult::error(
            "NbtRecordBuilder.build not yet implemented (DFU RecordBuilder port pending)",
        )
    }
}

/// Port of the private `NbtOps.ListCollector` interface.
trait ListCollector {
    fn accept(self: Box<Self>, tag: Tag) -> Box<dyn ListCollector>;
    fn result(self: Box<Self>) -> Tag;
}

/// `createCollector(Tag)`.
fn create_collector(tag: &Tag) -> Option<Box<dyn ListCollector>> {
    match tag {
        Tag::End(_) => Some(Box::new(GenericListCollector::new())),
        Tag::List(t) if t.is_empty() => Some(Box::new(GenericListCollector::new())),
        Tag::List(t) => Some(Box::new(GenericListCollector::with_list(t.clone()))),
        Tag::ByteArray(t) if t.size() == 0 => Some(Box::new(GenericListCollector::new())),
        Tag::ByteArray(t) => Some(Box::new(ByteListCollector::new(t.data.clone()))),
        Tag::IntArray(t) if t.size() == 0 => Some(Box::new(GenericListCollector::new())),
        Tag::IntArray(t) => Some(Box::new(IntListCollector::new(t.data.clone()))),
        Tag::LongArray(t) if t.size() == 0 => Some(Box::new(GenericListCollector::new())),
        Tag::LongArray(t) => Some(Box::new(LongListCollector::new(t.data.clone()))),
        _ => None,
    }
}

struct ByteListCollector {
    values: Vec<i8>,
}

impl ByteListCollector {
    fn new(initial: Vec<i8>) -> Self {
        ByteListCollector { values: initial }
    }
}

impl ListCollector for ByteListCollector {
    fn accept(self: Box<Self>, tag: Tag) -> Box<dyn ListCollector> {
        match tag {
            Tag::Byte(b) => Box::new(ByteListCollector {
                values: {
                    let mut values = self.values;
                    values.push(b.value);
                    values
                },
            }),
            other => Box::new(GenericListCollector::from_bytes(self.values)).accept(other),
        }
    }

    fn result(self: Box<Self>) -> Tag {
        Tag::ByteArray(ByteArrayTag::new(self.values))
    }
}

struct IntListCollector {
    values: Vec<i32>,
}

impl IntListCollector {
    fn new(initial: Vec<i32>) -> Self {
        IntListCollector { values: initial }
    }
}

impl ListCollector for IntListCollector {
    fn accept(self: Box<Self>, tag: Tag) -> Box<dyn ListCollector> {
        match tag {
            Tag::Int(v) => Box::new(IntListCollector {
                values: {
                    let mut values = self.values;
                    values.push(v.value);
                    values
                },
            }),
            other => Box::new(GenericListCollector::from_ints(self.values)).accept(other),
        }
    }

    fn result(self: Box<Self>) -> Tag {
        Tag::IntArray(IntArrayTag::new(self.values))
    }
}

struct LongListCollector {
    values: Vec<i64>,
}

impl LongListCollector {
    fn new(initial: Vec<i64>) -> Self {
        LongListCollector { values: initial }
    }
}

impl ListCollector for LongListCollector {
    fn accept(self: Box<Self>, tag: Tag) -> Box<dyn ListCollector> {
        match tag {
            Tag::Long(v) => Box::new(LongListCollector {
                values: {
                    let mut values = self.values;
                    values.push(v.value);
                    values
                },
            }),
            other => Box::new(GenericListCollector::from_longs(self.values)).accept(other),
        }
    }

    fn result(self: Box<Self>) -> Tag {
        Tag::LongArray(LongArrayTag::new(self.values))
    }
}

struct GenericListCollector {
    result: ListTag,
}

impl GenericListCollector {
    fn new() -> Self {
        GenericListCollector {
            result: ListTag::new(),
        }
    }

    fn with_list(initial: ListTag) -> Self {
        GenericListCollector { result: initial }
    }

    fn from_bytes(initials: Vec<i8>) -> Self {
        let mut result = ListTag::new();
        for v in initials {
            result.add(Tag::Byte(ByteTag::value_of(v)));
        }
        GenericListCollector { result }
    }

    fn from_ints(initials: Vec<i32>) -> Self {
        let mut result = ListTag::new();
        for v in initials {
            result.add(Tag::Int(IntTag::value_of(v)));
        }
        GenericListCollector { result }
    }

    fn from_longs(initials: Vec<i64>) -> Self {
        let mut result = ListTag::new();
        for v in initials {
            result.add(Tag::Long(LongTag::value_of(v)));
        }
        GenericListCollector { result }
    }
}

impl ListCollector for GenericListCollector {
    fn accept(self: Box<Self>, tag: Tag) -> Box<dyn ListCollector> {
        Box::new(GenericListCollector {
            result: {
                let mut result = self.result;
                result.add(tag);
                result
            },
        })
    }

    fn result(self: Box<Self>) -> Tag {
        Tag::List(self.result)
    }
}
