//! Port of `net.minecraft.world.level.storage.loot.entries.*`.
//!
//! Java source (Paper 26.2):
//! - `entries/LootPoolEntry.java`
//! - `entries/LootPoolEntryContainer.java`
//! - `entries/LootPoolSingletonContainer.java`
//! - `entries/LootItem.java`
//! - `entries/EmptyLootItem.java`
//! - `entries/DynamicLoot.java`
//! - `entries/TagEntry.java`
//! - `entries/NestedLootTable.java`
//! - `entries/CompositeEntryBase.java`
//! - `entries/ComposableEntryContainer.java`
//! - `entries/AlternativesEntry.java`
//! - `entries/EntryGroup.java`
//! - `entries/SequentialEntry.java`

use std::rc::Rc;

use rivet_registry::Identifier;
use rivet_util::mth;

use super::context::LootContext;
use super::functions::LootItemFunction;
use super::item_stack::LootItemStack;
use super::predicates::LootItemCondition;
use super::table::LootTable;
use super::validation::{
    InlineLootTablePathElement, LootReferenceKey, UnreachableEntryProblem, Validatable,
    ValidationContext, validate_list,
};

/// `LootPoolSingletonContainer.DEFAULT_WEIGHT`.
pub const DEFAULT_WEIGHT: i32 = 1;

/// `LootPoolSingletonContainer.DEFAULT_QUALITY`.
pub const DEFAULT_QUALITY: i32 = 0;

/// Reference to a nested loot table (`Either<ResourceKey<LootTable>, LootTable>`).
#[derive(Debug, Clone, PartialEq)]
pub enum NestedTableRef {
    Reference(Identifier),
    Inline(Box<LootTable>),
}

/// An expanded, selectable entry ready for weighted random choice in a
/// [`super::LootPool`] roll (`LootPoolEntry`).
#[derive(Debug, Clone)]
pub struct ExpandedLootPoolEntry<'a> {
    weight: i32,
    quality: i32,
    functions: &'a [LootItemFunction],
    kind: ExpandedEntryKind<'a>,
}

#[derive(Debug, Clone)]
enum ExpandedEntryKind<'a> {
    Item(Identifier),
    Empty,
    Dynamic(&'a Identifier),
    TagAll(&'a Identifier),
    NestedTable(&'a NestedTableRef),
}

impl ExpandedLootPoolEntry<'_> {
    /// `LootPoolSingletonContainer.EntryBase.getWeight(float luck)`:
    /// `Math.max(Mth.floor(weight + quality * luck), 0)`.
    #[must_use]
    pub fn weight(&self, luck: f32) -> i32 {
        mth::floor(self.weight as f32 + self.quality as f32 * luck).max(0)
    }

    /// `LootPoolEntry.createItemStack(Consumer<ItemStack>, LootContext)` —
    /// produces items decorated with the entry's own `functions`.
    pub fn create_item_stack(
        &self,
        output: &mut dyn FnMut(LootItemStack, &mut LootContext<'_>),
        context: &mut LootContext<'_>,
    ) {
        let functions = self.functions;
        let mut emit = |stack: LootItemStack, ctx: &mut LootContext<'_>| {
            let decorated = LootItemFunction::apply_all(functions, stack, ctx);
            output(decorated, ctx);
        };

        match &self.kind {
            ExpandedEntryKind::Item(item) => {
                let max_stack = context.resolver().item_max_stack_size(item);
                emit(LootItemStack::with_count_and_max(item.clone(), 1, max_stack), context);
            }
            ExpandedEntryKind::Empty => {}
            ExpandedEntryKind::Dynamic(name) => {
                let mut collected = Vec::new();
                context.add_dynamic_drops(name, &mut |stack| collected.push(stack));
                for stack in collected {
                    emit(stack, context);
                }
            }
            ExpandedEntryKind::TagAll(tag) => {
                let items: Vec<Identifier> = context
                    .resolver()
                    .get_item_tag(tag)
                    .map(<[Identifier]>::to_vec)
                    .unwrap_or_default();
                for item in items {
                    let max_stack = context.resolver().item_max_stack_size(&item);
                    emit(LootItemStack::with_count_and_max(item, 1, max_stack), context);
                }
            }
            ExpandedEntryKind::NestedTable(table_ref) => match table_ref {
                NestedTableRef::Reference(id) => {
                    if let Some(table) = context.resolver().get_table(id).cloned() {
                        table.get_random_items_raw_by_id(Some(id), context, &mut |stack, ctx| {
                            emit(stack, ctx);
                        });
                    }
                }
                NestedTableRef::Inline(table) => {
                    table.get_random_items_raw_by_id(None, context, &mut |stack, ctx| {
                        emit(stack, ctx);
                    });
                }
            },
        }
    }
}

/// `net.minecraft.world.level.storage.loot.entries.LootPoolEntryContainer`.
#[derive(Debug, Clone, PartialEq)]
pub enum LootPoolEntryContainer {
    /// `LootItem`.
    Item {
        item: Identifier,
        weight: i32,
        quality: i32,
        conditions: Vec<LootItemCondition>,
        functions: Vec<LootItemFunction>,
    },
    /// `EmptyLootItem`.
    Empty {
        weight: i32,
        quality: i32,
        conditions: Vec<LootItemCondition>,
        functions: Vec<LootItemFunction>,
    },
    /// `DynamicLoot`.
    Dynamic {
        name: Identifier,
        weight: i32,
        quality: i32,
        conditions: Vec<LootItemCondition>,
        functions: Vec<LootItemFunction>,
    },
    /// `TagEntry`.
    Tag {
        tag: Identifier,
        expand: bool,
        weight: i32,
        quality: i32,
        conditions: Vec<LootItemCondition>,
        functions: Vec<LootItemFunction>,
    },
    /// `NestedLootTable`.
    LootTable {
        value: NestedTableRef,
        weight: i32,
        quality: i32,
        conditions: Vec<LootItemCondition>,
        functions: Vec<LootItemFunction>,
    },
    /// `AlternativesEntry` — short-circuit OR over `children`.
    Alternatives {
        children: Vec<LootPoolEntryContainer>,
        conditions: Vec<LootItemCondition>,
    },
    /// `EntryGroup` — expands all `children` in order, returns `true`.
    Group {
        children: Vec<LootPoolEntryContainer>,
        conditions: Vec<LootItemCondition>,
    },
    /// `SequentialEntry` — short-circuit AND over `children`.
    Sequence {
        children: Vec<LootPoolEntryContainer>,
        conditions: Vec<LootItemCondition>,
    },
}

impl LootPoolEntryContainer {
    /// `LootItem.lootTableItem(item)` with default weight `1` and quality `0`.
    #[must_use]
    pub fn item(item: Identifier) -> Self {
        Self::Item {
            item,
            weight: DEFAULT_WEIGHT,
            quality: DEFAULT_QUALITY,
            conditions: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// `EmptyLootItem.emptyItem()` with default weight `1` and quality `0`.
    #[must_use]
    pub fn empty() -> Self {
        Self::Empty {
            weight: DEFAULT_WEIGHT,
            quality: DEFAULT_QUALITY,
            conditions: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// `DynamicLoot.dynamicEntry(name)`.
    #[must_use]
    pub fn dynamic(name: Identifier) -> Self {
        Self::Dynamic {
            name,
            weight: DEFAULT_WEIGHT,
            quality: DEFAULT_QUALITY,
            conditions: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// `TagEntry.expandTag(tag)` (`expand = true`).
    #[must_use]
    pub fn expand_tag(tag: Identifier) -> Self {
        Self::Tag {
            tag,
            expand: true,
            weight: DEFAULT_WEIGHT,
            quality: DEFAULT_QUALITY,
            conditions: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// `TagEntry.tagContents(tag)` (`expand = false`).
    #[must_use]
    pub fn tag_contents(tag: Identifier) -> Self {
        Self::Tag {
            tag,
            expand: false,
            weight: DEFAULT_WEIGHT,
            quality: DEFAULT_QUALITY,
            conditions: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// `NestedLootTable.lootTableReference(id)`.
    #[must_use]
    pub fn loot_table_reference(id: Identifier) -> Self {
        Self::LootTable {
            value: NestedTableRef::Reference(id),
            weight: DEFAULT_WEIGHT,
            quality: DEFAULT_QUALITY,
            conditions: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// `NestedLootTable.inlineLootTable(table)`.
    #[must_use]
    pub fn inline_loot_table(table: LootTable) -> Self {
        Self::LootTable {
            value: NestedTableRef::Inline(Box::new(table)),
            weight: DEFAULT_WEIGHT,
            quality: DEFAULT_QUALITY,
            conditions: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// `AlternativesEntry.alternatives(children)`.
    #[must_use]
    pub fn alternatives(children: Vec<LootPoolEntryContainer>) -> Self {
        Self::Alternatives {
            children,
            conditions: Vec::new(),
        }
    }

    /// `EntryGroup.list(children)`.
    #[must_use]
    pub fn group(children: Vec<LootPoolEntryContainer>) -> Self {
        Self::Group {
            children,
            conditions: Vec::new(),
        }
    }

    /// `SequentialEntry.sequential(children)`.
    #[must_use]
    pub fn sequence(children: Vec<LootPoolEntryContainer>) -> Self {
        Self::Sequence {
            children,
            conditions: Vec::new(),
        }
    }

    /// Sets `weight` on a singleton entry container.
    #[must_use]
    pub fn with_weight(mut self, new_weight: i32) -> Self {
        match &mut self {
            Self::Item { weight, .. }
            | Self::Empty { weight, .. }
            | Self::Dynamic { weight, .. }
            | Self::Tag { weight, .. }
            | Self::LootTable { weight, .. } => *weight = new_weight,
            Self::Alternatives { .. } | Self::Group { .. } | Self::Sequence { .. } => {
                panic!("Composite entries do not have weight");
            }
        }
        self
    }

    /// Sets `quality` on a singleton entry container.
    #[must_use]
    pub fn with_quality(mut self, new_quality: i32) -> Self {
        match &mut self {
            Self::Item { quality, .. }
            | Self::Empty { quality, .. }
            | Self::Dynamic { quality, .. }
            | Self::Tag { quality, .. }
            | Self::LootTable { quality, .. } => *quality = new_quality,
            Self::Alternatives { .. } | Self::Group { .. } | Self::Sequence { .. } => {
                panic!("Composite entries do not have quality");
            }
        }
        self
    }

    /// Appends a condition to this entry container (`when(...)`).
    #[must_use]
    pub fn when(mut self, condition: LootItemCondition) -> Self {
        self.conditions_mut().push(condition);
        self
    }

    /// Appends a function to a singleton entry container (`apply(...)`).
    #[must_use]
    pub fn apply(mut self, function: LootItemFunction) -> Self {
        match &mut self {
            Self::Item { functions, .. }
            | Self::Empty { functions, .. }
            | Self::Dynamic { functions, .. }
            | Self::Tag { functions, .. }
            | Self::LootTable { functions, .. } => functions.push(function),
            Self::Alternatives { .. } | Self::Group { .. } | Self::Sequence { .. } => {
                panic!("Composite entries do not have functions");
            }
        }
        self
    }

    /// Returns this entry's conditions slice.
    #[must_use]
    pub fn conditions(&self) -> &[LootItemCondition] {
        match self {
            Self::Item { conditions, .. }
            | Self::Empty { conditions, .. }
            | Self::Dynamic { conditions, .. }
            | Self::Tag { conditions, .. }
            | Self::LootTable { conditions, .. }
            | Self::Alternatives { conditions, .. }
            | Self::Group { conditions, .. }
            | Self::Sequence { conditions, .. } => conditions,
        }
    }

    fn conditions_mut(&mut self) -> &mut Vec<LootItemCondition> {
        match self {
            Self::Item { conditions, .. }
            | Self::Empty { conditions, .. }
            | Self::Dynamic { conditions, .. }
            | Self::Tag { conditions, .. }
            | Self::LootTable { conditions, .. }
            | Self::Alternatives { conditions, .. }
            | Self::Group { conditions, .. }
            | Self::Sequence { conditions, .. } => conditions,
        }
    }

    /// `LootPoolEntryContainer.canRun(LootContext)`.
    pub fn can_run(&self, context: &mut LootContext<'_>) -> bool {
        LootItemCondition::test_all(self.conditions(), context)
    }

    /// `ComposableEntryContainer.expand(LootContext, Consumer<LootPoolEntry>)`.
    pub fn expand<'a>(
        &'a self,
        context: &mut LootContext<'_>,
        output: &mut dyn FnMut(ExpandedLootPoolEntry<'a>),
    ) -> bool {
        if !self.can_run(context) {
            return false;
        }

        match self {
            Self::Item {
                item,
                weight,
                quality,
                functions,
                ..
            } => {
                output(ExpandedLootPoolEntry {
                    weight: *weight,
                    quality: *quality,
                    functions,
                    kind: ExpandedEntryKind::Item(item.clone()),
                });
                true
            }
            Self::Empty {
                weight,
                quality,
                functions,
                ..
            } => {
                output(ExpandedLootPoolEntry {
                    weight: *weight,
                    quality: *quality,
                    functions,
                    kind: ExpandedEntryKind::Empty,
                });
                true
            }
            Self::Dynamic {
                name,
                weight,
                quality,
                functions,
                ..
            } => {
                output(ExpandedLootPoolEntry {
                    weight: *weight,
                    quality: *quality,
                    functions,
                    kind: ExpandedEntryKind::Dynamic(name),
                });
                true
            }
            Self::Tag {
                tag,
                expand,
                weight,
                quality,
                functions,
                ..
            } => {
                if *expand {
                    // TagEntry.expandTag: expand each item in the tag as its own weighted entry.
                    // In Paper 26.2, expandTag constructs anonymous EntryBase instances whose
                    // createItemStack emits `new ItemStack(item)` directly without applying
                    // `this.functions`.
                    let items: Vec<Identifier> = context
                        .resolver()
                        .get_item_tag(tag)
                        .map(<[Identifier]>::to_vec)
                        .unwrap_or_default();
                    for item in items {
                        output(ExpandedLootPoolEntry {
                            weight: *weight,
                            quality: *quality,
                            functions: &[],
                            kind: ExpandedEntryKind::Item(item),
                        });
                    }
                    true
                } else {
                    output(ExpandedLootPoolEntry {
                        weight: *weight,
                        quality: *quality,
                        functions,
                        kind: ExpandedEntryKind::TagAll(tag),
                    });
                    true
                }
            }
            Self::LootTable {
                value,
                weight,
                quality,
                functions,
                ..
            } => {
                output(ExpandedLootPoolEntry {
                    weight: *weight,
                    quality: *quality,
                    functions,
                    kind: ExpandedEntryKind::NestedTable(value),
                });
                true
            }
            Self::Alternatives { children, .. } => {
                // AlternativesEntry: empty -> ALWAYS_FALSE; otherwise short-circuit OR.
                for child in children {
                    if child.expand(context, output) {
                        return true;
                    }
                }
                false
            }
            Self::Group { children, .. } => {
                // EntryGroup.compose:
                // - 0 children -> ALWAYS_TRUE
                // - 1 child    -> delegates directly to that child's return value
                // - 2+ children -> expands every child in order and returns true
                match children.as_slice() {
                    [] => true,
                    [only] => only.expand(context, output),
                    many => {
                        for child in many {
                            child.expand(context, output);
                        }
                        true
                    }
                }
            }
            Self::Sequence { children, .. } => {
                // SequentialEntry: empty -> ALWAYS_TRUE; otherwise short-circuit AND.
                for child in children {
                    if !child.expand(context, output) {
                        return false;
                    }
                }
                true
            }
        }
    }
}

impl Validatable for LootPoolEntryContainer {
    fn validate(&self, context: &ValidationContext<'_>) {
        validate_list(context, "conditions", self.conditions());

        match self {
            Self::Item { functions, .. }
            | Self::Empty { functions, .. }
            | Self::Dynamic { functions, .. }
            | Self::Tag { functions, .. } => {
                validate_list(context, "functions", functions);
            }
            Self::LootTable {
                value, functions, ..
            } => {
                validate_list(context, "functions", functions);
                match value {
                    NestedTableRef::Reference(id) => {
                        context.validate_reference(LootReferenceKey::Table(id.clone()));
                    }
                    NestedTableRef::Inline(table) => {
                        table.validate(&context.for_child(Rc::new(InlineLootTablePathElement)));
                    }
                }
            }
            Self::Alternatives { children, .. } => {
                validate_list(context, "children", children);
                // AlternativesEntry.validate: report "Unreachable entry!" for any
                // non-final child with no conditions.
                for (index, child) in children.iter().enumerate() {
                    if index + 1 < children.len() && child.conditions().is_empty() {
                        context
                            .for_indexed_field("children", index as i32)
                            .report_problem(Rc::new(UnreachableEntryProblem));
                    }
                }
            }
            Self::Group { children, .. } | Self::Sequence { children, .. } => {
                validate_list(context, "children", children);
            }
        }
    }
}
