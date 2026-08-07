// `net.minecraft.nbt.visitors` package — fully ported by unit mc.nbt.visitors.
// One module per Java class (PORTING.md naming); owned by manifest unit
// mc.nbt.visitors. Declarations are controller-owned (WORKFLOWS.md principle 2).

pub mod collect_fields;
pub mod collect_to_tag;
pub mod field_selector;
pub mod field_tree;
pub mod skip_all;
pub mod skip_fields;
