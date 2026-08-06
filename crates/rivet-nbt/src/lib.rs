//! Port of `net.minecraft.nbt` (Mojang NBT) + `net.minecraft.nbt.visitors`.
//! One module per Java class; ownership per MANIFEST.tsv units mc.nbt.*.

pub mod byte_array_tag;
pub mod byte_tag;
pub mod collection_tag;
pub mod compound_tag;
pub mod double_tag;
pub mod end_tag;
pub mod float_tag;
pub mod float_to_string;
pub mod int_array_tag;
pub mod int_tag;
pub mod list_tag;
pub mod long_array_tag;
pub mod long_tag;
pub mod mth;
pub mod nbt_accounter;
pub mod nbt_accounter_exception;
pub mod nbt_exception;
pub mod nbt_format_exception;
pub mod nbt_io;
pub mod nbt_ops;
pub mod nbt_utils;
pub mod numeric_tag;
pub mod primitive_tag;
pub mod reported_nbt_exception;
pub mod short_tag;
pub mod snbt_grammar;
pub mod snbt_operations;
pub mod snbt_printer_tag_visitor;
pub mod stream_tag_visitor;
pub mod string_tag;
pub mod string_tag_visitor;
pub mod tag;
pub mod tag_parser;
pub mod tag_type;
pub mod tag_types;
pub mod tag_visitor;
pub mod text_component_tag_visitor;

pub mod visitors;

#[cfg(test)]
mod tests {
    pub mod nbt_io;
    pub mod nbt_ops;
    pub mod round_trip;
}
