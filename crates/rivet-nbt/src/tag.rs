//! Port of `net.minecraft.nbt.Tag` (the sealed hierarchy root).
//!
//! Java: `sealed interface Tag permits CompoundTag, CollectionTag, PrimitiveTag,
//! EndTag`. Per OWNERSHIP.md/PORTING.md a sealed interface maps to a Rust enum.
//! The hierarchy is represented as an enum over newtype structs for the leaf
//! object types (`EndTag`, `ByteTag`, ..., `CompoundTag`); the per-class modules
//! (`byte_tag.rs`, `compound_tag.rs`, ...) hold the structs so the Java class
//! names stay greppable.

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
use crate::stream_tag_visitor::{StreamTagVisitor, ValueResult};
use crate::string_tag::StringTag;
use crate::tag_type::TagType;
use crate::tag_visitor::TagVisitor;

pub const OBJECT_HEADER: i32 = 8;
pub const ARRAY_HEADER: i32 = 12;
pub const OBJECT_REFERENCE: i32 = 4;
pub const STRING_SIZE: i32 = 28;
pub const TAG_END: i8 = 0;
pub const TAG_BYTE: i8 = 1;
pub const TAG_SHORT: i8 = 2;
pub const TAG_INT: i8 = 3;
pub const TAG_LONG: i8 = 4;
pub const TAG_FLOAT: i8 = 5;
pub const TAG_DOUBLE: i8 = 6;
pub const TAG_BYTE_ARRAY: i8 = 7;
pub const TAG_STRING: i8 = 8;
pub const TAG_LIST: i8 = 9;
pub const TAG_COMPOUND: i8 = 10;
pub const TAG_INT_ARRAY: i8 = 11;
pub const TAG_LONG_ARRAY: i8 = 12;
pub const MAX_DEPTH: i32 = 512;

/// Port of the sealed `Tag` hierarchy.
#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    End(EndTag),
    Byte(ByteTag),
    Short(ShortTag),
    Int(IntTag),
    Long(LongTag),
    Float(FloatTag),
    Double(DoubleTag),
    ByteArray(ByteArrayTag),
    String(StringTag),
    List(ListTag),
    Compound(CompoundTag),
    IntArray(IntArrayTag),
    LongArray(LongArrayTag),
}

impl Tag {
    /// `Tag.getId()`.
    pub fn id(&self) -> i8 {
        match self {
            Tag::End(_) => TAG_END,
            Tag::Byte(_) => TAG_BYTE,
            Tag::Short(_) => TAG_SHORT,
            Tag::Int(_) => TAG_INT,
            Tag::Long(_) => TAG_LONG,
            Tag::Float(_) => TAG_FLOAT,
            Tag::Double(_) => TAG_DOUBLE,
            Tag::ByteArray(_) => TAG_BYTE_ARRAY,
            Tag::String(_) => TAG_STRING,
            Tag::List(_) => TAG_LIST,
            Tag::Compound(_) => TAG_COMPOUND,
            Tag::IntArray(_) => TAG_INT_ARRAY,
            Tag::LongArray(_) => TAG_LONG_ARRAY,
        }
    }

    /// `Tag.getType()`.
    pub fn get_type(&self) -> TagType {
        TagType::from_id(self.id())
    }

    /// `Tag.copy()`.
    pub fn copy_tag(&self) -> Tag {
        match self {
            // Mutable leaves deep-copy their data (arrays, list, compound).
            Tag::ByteArray(t) => Tag::ByteArray(t.copy_tag()),
            Tag::List(t) => Tag::List(t.copy_tag()),
            Tag::Compound(t) => Tag::Compound(t.copy_tag()),
            Tag::IntArray(t) => Tag::IntArray(t.copy_tag()),
            Tag::LongArray(t) => Tag::LongArray(t.copy_tag()),
            other => other.clone(), // primitives, String, End are value/singleton
        }
    }

    /// `Tag.sizeInBytes()`.
    pub fn size_in_bytes(&self) -> i32 {
        match self {
            Tag::End(t) => t.size_in_bytes(),
            Tag::Byte(t) => t.size_in_bytes(),
            Tag::Short(t) => t.size_in_bytes(),
            Tag::Int(t) => t.size_in_bytes(),
            Tag::Long(t) => t.size_in_bytes(),
            Tag::Float(t) => t.size_in_bytes(),
            Tag::Double(t) => t.size_in_bytes(),
            Tag::ByteArray(t) => t.size_in_bytes(),
            Tag::String(t) => t.size_in_bytes(),
            Tag::List(t) => t.size_in_bytes(),
            Tag::Compound(t) => t.size_in_bytes(),
            Tag::IntArray(t) => t.size_in_bytes(),
            Tag::LongArray(t) => t.size_in_bytes(),
        }
    }

    /// `Tag.accept(TagVisitor)`.
    pub fn accept(&self, visitor: &mut dyn TagVisitor) {
        match self {
            Tag::End(t) => visitor.visit_end(t),
            Tag::Byte(t) => visitor.visit_byte(t),
            Tag::Short(t) => visitor.visit_short(t),
            Tag::Int(t) => visitor.visit_int(t),
            Tag::Long(t) => visitor.visit_long(t),
            Tag::Float(t) => visitor.visit_float(t),
            Tag::Double(t) => visitor.visit_double(t),
            Tag::ByteArray(t) => visitor.visit_byte_array(t),
            Tag::String(t) => visitor.visit_string(t),
            Tag::List(t) => visitor.visit_list(t),
            Tag::Compound(t) => visitor.visit_compound(t),
            Tag::IntArray(t) => visitor.visit_int_array(t),
            Tag::LongArray(t) => visitor.visit_long_array(t),
        }
    }

    /// `Tag.accept(StreamTagVisitor)`.
    pub fn accept_stream(&self, visitor: &mut dyn StreamTagVisitor) -> ValueResult {
        match self {
            Tag::End(_) => visitor.visit_end(),
            Tag::Byte(t) => visitor.visit_byte(t.value),
            Tag::Short(t) => visitor.visit_short(t.value),
            Tag::Int(t) => visitor.visit_int(t.value),
            Tag::Long(t) => visitor.visit_long(t.value),
            Tag::Float(t) => visitor.visit_float(t.value),
            Tag::Double(t) => visitor.visit_double(t.value),
            Tag::ByteArray(t) => visitor.visit_byte_array(&t.data),
            Tag::String(t) => visitor.visit_string(&t.value),
            Tag::List(t) => t.accept_stream(visitor),
            Tag::Compound(t) => t.accept_stream(visitor),
            Tag::IntArray(t) => visitor.visit_int_array(&t.data),
            Tag::LongArray(t) => visitor.visit_long_array(&t.data),
        }
    }

    /// `Tag.acceptAsRoot(StreamTagVisitor)`.
    pub fn accept_as_root(&self, visitor: &mut dyn StreamTagVisitor) {
        if visitor.visit_root_entry(self.get_type()) == ValueResult::Continue {
            let _ = self.accept_stream(visitor);
        }
    }

    // ---- `asXxx` accessors (defaults on `Tag`, overrides on leaves) ----

    /// `Tag.asString()`.
    pub fn as_string(&self) -> Option<&String> {
        match self {
            Tag::String(t) => Some(&t.value),
            _ => None,
        }
    }

    /// `Tag.asNumber()` — the boxed `Number` (`NumericTag.box()`).
    pub fn as_number(&self) -> Option<rivet_serialization::number::Number> {
        crate::numeric_tag::NumericTag::try_from(self)
            .ok()
            .map(|n| n.boxed())
    }

    /// The boxed `Number` as its `doubleValue()` — `asNumber().map(Number::doubleValue)`.
    /// Fidelity note: Java narrows via `Number.doubleValue()` (exact for
    /// integral tags up to 2^53, rounded beyond). Kept as a convenience for the
    /// SNBT `bool` operation's `doubleValue() != 0.0` check.
    pub fn as_number_f64(&self) -> Option<f64> {
        self.as_number().map(|n| n.double_value())
    }

    /// `Tag.asByte()`.
    pub fn as_byte(&self) -> Option<i8> {
        match self {
            Tag::Byte(t) => Some(t.value),
            Tag::Short(t) => Some(t.byte_value()),
            Tag::Int(t) => Some(t.byte_value()),
            Tag::Long(t) => Some(t.byte_value()),
            Tag::Float(t) => Some(t.byte_value()),
            Tag::Double(t) => Some(t.byte_value()),
            _ => None,
        }
    }

    /// `Tag.asShort()`.
    pub fn as_short(&self) -> Option<i16> {
        match self {
            Tag::Byte(t) => Some(t.value as i16),
            Tag::Short(t) => Some(t.value),
            Tag::Int(t) => Some(t.short_value()),
            Tag::Long(t) => Some(t.short_value()),
            Tag::Float(t) => Some(t.short_value()),
            Tag::Double(t) => Some(t.short_value()),
            _ => None,
        }
    }

    /// `Tag.asInt()`.
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Tag::Byte(t) => Some(t.value as i32),
            Tag::Short(t) => Some(t.value as i32),
            Tag::Int(t) => Some(t.value),
            Tag::Long(t) => Some(t.int_value()),
            Tag::Float(t) => Some(t.int_value()),
            Tag::Double(t) => Some(t.int_value()),
            _ => None,
        }
    }

    /// `Tag.asLong()`.
    pub fn as_long(&self) -> Option<i64> {
        match self {
            Tag::Byte(t) => Some(t.value as i64),
            Tag::Short(t) => Some(t.value as i64),
            Tag::Int(t) => Some(t.value as i64),
            Tag::Long(t) => Some(t.value),
            Tag::Float(t) => Some(t.long_value()),
            Tag::Double(t) => Some(t.long_value()),
            _ => None,
        }
    }

    /// `Tag.asFloat()`.
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Tag::Byte(t) => Some(t.value as f32),
            Tag::Short(t) => Some(t.value as f32),
            Tag::Int(t) => Some(t.value as f32),
            Tag::Long(t) => Some(t.float_value()),
            Tag::Float(t) => Some(t.value),
            Tag::Double(t) => Some(t.float_value()),
            _ => None,
        }
    }

    /// `Tag.asDouble()`.
    pub fn as_double(&self) -> Option<f64> {
        match self {
            Tag::Byte(t) => Some(t.value as f64),
            Tag::Short(t) => Some(t.value as f64),
            Tag::Int(t) => Some(t.value as f64),
            Tag::Long(t) => Some(t.value as f64),
            Tag::Float(t) => Some(t.value as f64),
            Tag::Double(t) => Some(t.value),
            _ => None,
        }
    }

    /// `Tag.asBoolean()`.
    pub fn as_boolean(&self) -> Option<bool> {
        self.as_byte().map(|b| b != 0)
    }

    /// `Tag.asByteArray()`.
    pub fn as_byte_array(&self) -> Option<&Vec<i8>> {
        match self {
            Tag::ByteArray(t) => Some(&t.data),
            _ => None,
        }
    }

    /// `Tag.asIntArray()`.
    pub fn as_int_array(&self) -> Option<&Vec<i32>> {
        match self {
            Tag::IntArray(t) => Some(&t.data),
            _ => None,
        }
    }

    /// `Tag.asLongArray()`.
    pub fn as_long_array(&self) -> Option<&Vec<i64>> {
        match self {
            Tag::LongArray(t) => Some(&t.data),
            _ => None,
        }
    }

    /// `Tag.asCompound()`.
    pub fn as_compound(&self) -> Option<&CompoundTag> {
        match self {
            Tag::Compound(t) => Some(t),
            _ => None,
        }
    }

    /// `Tag.asList()`.
    pub fn as_list(&self) -> Option<&ListTag> {
        match self {
            Tag::List(t) => Some(t),
            _ => None,
        }
    }

    /// Whether this tag is a `NumericTag` (`instanceof NumericTag`).
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Tag::Byte(_)
                | Tag::Short(_)
                | Tag::Int(_)
                | Tag::Long(_)
                | Tag::Float(_)
                | Tag::Double(_)
        )
    }

    /// Whether this tag is a `PrimitiveTag`.
    pub fn is_primitive(&self) -> bool {
        self.is_numeric() || matches!(self, Tag::String(_))
    }

    /// Whether this tag is a `CollectionTag`.
    pub fn is_collection(&self) -> bool {
        matches!(
            self,
            Tag::ByteArray(_) | Tag::IntArray(_) | Tag::LongArray(_) | Tag::List(_)
        )
    }
}

impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Java `Tag.toString()` -> StringTagVisitor.
        write!(
            f,
            "{}",
            crate::string_tag_visitor::StringTagVisitor::to_string(self)
        )
    }
}
