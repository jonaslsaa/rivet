//! Port of `net.minecraft.nbt.visitors.CollectToTag` — collects a streamed tag
//! back into a `Tag` tree (used by the streaming parse path and by the SNBT
//! readers when a tree is wanted).

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
use crate::stream_tag_visitor::{EntryResult, StreamTagVisitor, ValueResult};
use crate::string_tag::StringTag;
use crate::tag::Tag;
use crate::tag_type::TagType;

/// `CollectToTag` — a `StreamTagVisitor` that rebuilds the streamed `Tag` tree.
#[derive(Debug)]
pub struct CollectToTag {
    container_stack: Vec<ContainerBuilder>,
}

impl Default for CollectToTag {
    fn default() -> Self {
        CollectToTag::new()
    }
}

/// `CollectToTag.ContainerBuilder` — one of the three private static builder
/// classes (`RootBuilder`, `CompoundBuilder`, `ListBuilder`).
#[derive(Debug)]
enum ContainerBuilder {
    Root(RootBuilder),
    Compound(CompoundBuilder),
    List(ListBuilder),
}

/// `CollectToTag.RootBuilder` — holds the final result.
#[derive(Debug, Default)]
struct RootBuilder {
    result: Option<Tag>,
}

/// `CollectToTag.CompoundBuilder` — pairs the most recent key (`last_id`) with
/// the next accepted value.
#[derive(Debug, Default)]
struct CompoundBuilder {
    compound: CompoundTag,
    last_id: String,
}

/// `CollectToTag.ListBuilder`.
#[derive(Debug, Default)]
struct ListBuilder {
    list: ListTag,
}

impl ContainerBuilder {
    /// `ContainerBuilder.acceptKey(String)` — the interface default is a no-op;
    /// only the compound builder records the last key.
    fn accept_key(&mut self, id: &str) {
        if let ContainerBuilder::Compound(builder) = self {
            builder.last_id = id.to_string();
        }
    }

    /// `ContainerBuilder.acceptValue(Tag)`.
    fn accept_value(&mut self, tag: Tag) {
        match self {
            ContainerBuilder::Root(builder) => builder.result = Some(tag),
            ContainerBuilder::Compound(builder) => {
                builder.compound.put(builder.last_id.clone(), tag);
            }
            ContainerBuilder::List(builder) => builder.list.add_and_unwrap(tag),
        }
    }

    /// `ContainerBuilder.build()` — `@Nullable Tag`.
    fn build(self) -> Option<Tag> {
        match self {
            ContainerBuilder::Root(builder) => builder.result,
            ContainerBuilder::Compound(builder) => Some(Tag::Compound(builder.compound)),
            ContainerBuilder::List(builder) => Some(Tag::List(builder.list)),
        }
    }
}

impl CollectToTag {
    /// `new CollectToTag()`.
    pub fn new() -> Self {
        CollectToTag {
            container_stack: vec![ContainerBuilder::Root(RootBuilder::default())],
        }
    }

    /// `CollectToTag.getResult()`.
    pub fn get_result(&self) -> Option<Tag> {
        match self.container_stack.first() {
            Some(ContainerBuilder::Root(builder)) => builder.result.clone(),
            _ => None,
        }
    }

    /// `CollectToTag.depth()` — `containerStack.size() - 1`.
    pub fn depth(&self) -> usize {
        self.container_stack.len() - 1
    }

    /// `CollectToTag.appendEntry(Tag)`.
    fn append_entry(&mut self, instance: Tag) {
        self.container_stack
            .last_mut()
            .unwrap()
            .accept_value(instance);
    }

    /// `CollectToTag.enterContainerIfNeeded(TagType)`.
    fn enter_container_if_needed(&mut self, ty: TagType) {
        match ty {
            TagType::Compound => self
                .container_stack
                .push(ContainerBuilder::Compound(CompoundBuilder::default())),
            TagType::List => self
                .container_stack
                .push(ContainerBuilder::List(ListBuilder::default())),
            _ => {}
        }
    }
}

impl StreamTagVisitor for CollectToTag {
    fn visit_end(&mut self) -> ValueResult {
        self.append_entry(Tag::End(EndTag));
        ValueResult::Continue
    }

    fn visit_string(&mut self, value: &str) -> ValueResult {
        self.append_entry(Tag::String(StringTag::value_of(value.to_string())));
        ValueResult::Continue
    }

    fn visit_byte(&mut self, value: i8) -> ValueResult {
        self.append_entry(Tag::Byte(ByteTag::value_of(value)));
        ValueResult::Continue
    }

    fn visit_short(&mut self, value: i16) -> ValueResult {
        self.append_entry(Tag::Short(ShortTag::value_of(value)));
        ValueResult::Continue
    }

    fn visit_int(&mut self, value: i32) -> ValueResult {
        self.append_entry(Tag::Int(IntTag::value_of(value)));
        ValueResult::Continue
    }

    fn visit_long(&mut self, value: i64) -> ValueResult {
        self.append_entry(Tag::Long(LongTag::value_of(value)));
        ValueResult::Continue
    }

    fn visit_float(&mut self, value: f32) -> ValueResult {
        self.append_entry(Tag::Float(FloatTag::value_of(value)));
        ValueResult::Continue
    }

    fn visit_double(&mut self, value: f64) -> ValueResult {
        self.append_entry(Tag::Double(DoubleTag::value_of(value)));
        ValueResult::Continue
    }

    fn visit_byte_array(&mut self, value: &[i8]) -> ValueResult {
        self.append_entry(Tag::ByteArray(ByteArrayTag::new(value.to_vec())));
        ValueResult::Continue
    }

    fn visit_int_array(&mut self, value: &[i32]) -> ValueResult {
        self.append_entry(Tag::IntArray(IntArrayTag::new(value.to_vec())));
        ValueResult::Continue
    }

    fn visit_long_array(&mut self, value: &[i64]) -> ValueResult {
        self.append_entry(Tag::LongArray(LongArrayTag::new(value.to_vec())));
        ValueResult::Continue
    }

    fn visit_list(&mut self, _element_type: TagType, _size: usize) -> ValueResult {
        ValueResult::Continue
    }

    fn visit_element(&mut self, ty: TagType, _index: usize) -> EntryResult {
        self.enter_container_if_needed(ty);
        EntryResult::Enter
    }

    fn visit_entry(&mut self, _ty: TagType) -> EntryResult {
        EntryResult::Enter
    }

    fn visit_entry_named(&mut self, ty: TagType, id: &str) -> EntryResult {
        self.container_stack.last_mut().unwrap().accept_key(id);
        self.enter_container_if_needed(ty);
        EntryResult::Enter
    }

    fn visit_container_end(&mut self) -> ValueResult {
        let container = self.container_stack.pop().unwrap();
        let tag = container.build();
        if let Some(tag) = tag {
            self.container_stack.last_mut().unwrap().accept_value(tag);
        }
        ValueResult::Continue
    }

    fn visit_root_entry(&mut self, ty: TagType) -> ValueResult {
        self.enter_container_if_needed(ty);
        ValueResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compound_tag::CompoundTag;
    use crate::int_tag::IntTag;
    use crate::list_tag::ListTag;
    use crate::string_tag::StringTag;

    #[test]
    fn collects_simple_compound() {
        let mut tag = CompoundTag::new();
        tag.put(
            "name".to_string(),
            Tag::String(StringTag::value_of("hero".to_string())),
        );
        tag.put("level".to_string(), Tag::Int(IntTag::value_of(10)));

        let mut collector = CollectToTag::new();
        Tag::Compound(tag).accept_as_root(&mut collector);

        let result = collector.get_result().expect("result");
        let compound = result.as_compound().expect("compound result");
        assert_eq!(
            compound.get_string("name").map(String::as_str),
            Some("hero")
        );
        assert_eq!(compound.get_int("level"), Some(10));
    }

    #[test]
    fn collects_nested_compound_and_list() {
        let mut pos = CompoundTag::new();
        pos.put("x".to_string(), Tag::Int(IntTag::value_of(1)));
        pos.put("z".to_string(), Tag::Int(IntTag::value_of(2)));
        let mut scores = ListTag::new();
        scores.add(Tag::Int(IntTag::value_of(7)));
        scores.add(Tag::Int(IntTag::value_of(8)));

        let mut root = CompoundTag::new();
        root.put("pos".to_string(), Tag::Compound(pos));
        root.put("scores".to_string(), Tag::List(scores));

        let mut collector = CollectToTag::new();
        Tag::Compound(root).accept_as_root(&mut collector);

        let result = collector.get_result().expect("result");
        let compound = result.as_compound().expect("compound result");
        let pos = compound.get_compound("pos").expect("pos");
        assert_eq!(pos.get_int("x"), Some(1));
        assert_eq!(pos.get_int("z"), Some(2));
        let scores = compound.get_list("scores").expect("scores");
        assert_eq!(scores.size(), 2);
        assert_eq!(scores.get_int(0), Some(7));
        assert_eq!(scores.get_int(1), Some(8));
    }
}
