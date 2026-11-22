//! Port of `net.minecraft.world.level.storage.loot.{ValidationContext, Validatable, LootContextUser}`.
//!
//! Java source (Paper 26.2):
//! - `net/minecraft/world/level/storage/loot/ValidationContext.java`
//! - `net/minecraft/world/level/storage/loot/Validatable.java`
//! - `net/minecraft/world/level/storage/loot/LootContextUser.java`

use std::rc::Rc;

use indexmap::IndexSet;
use rivet_registry::Identifier;
use rivet_util::problem_reporter::{
    FieldPathElement, IndexedFieldPathElement, PathElement, Problem, ProblemReporter,
};

use super::context::LootRegistry;
use super::parameters::{ContextKey, ContextKeySet};

/// Identifies a referenced loot element (table, predicate, or item modifier)
/// for validation cycle detection and reference resolution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LootReferenceKey {
    Table(Identifier),
    Condition(Identifier),
    Function(Identifier),
}

impl LootReferenceKey {
    /// Returns the registry kind name (matching Minecraft's `Registries.*` paths).
    #[must_use]
    pub fn registry_name(&self) -> &'static str {
        match self {
            Self::Table(_) => "minecraft:loot_table",
            Self::Condition(_) => "minecraft:predicate",
            Self::Function(_) => "minecraft:item_modifier",
        }
    }

    /// Returns the element identifier.
    #[must_use]
    pub fn id(&self) -> &Identifier {
        match self {
            Self::Table(id) | Self::Condition(id) | Self::Function(id) => id,
        }
    }
}

/// `ProblemReporter.ElementReferencePathElement` — `->{<registry> / <id>}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementReferencePathElement(pub LootReferenceKey);

impl PathElement for ElementReferencePathElement {
    fn get(&self) -> String {
        format!("->{{{} / {}}}", self.0.registry_name(), self.0.id())
    }
}

/// `ProblemReporter.MapEntryPathElement` — `.<name>[<key>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapEntryPathElement(pub String, pub String);

impl PathElement for MapEntryPathElement {
    fn get(&self) -> String {
        format!(".{}[{}]", self.0, self.1)
    }
}

/// `NestedLootTable.INLINE_LOOT_TABLE_PATH_ELEMENT` — `->{inline}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InlineLootTablePathElement;

impl PathElement for InlineLootTablePathElement {
    fn get(&self) -> String {
        "->{inline}".to_string()
    }
}

/// `ValidationContext.MissingReferenceProblem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingReferenceProblem(pub LootReferenceKey);

impl Problem for MissingReferenceProblem {
    fn description(&self) -> String {
        format!(
            "Missing element {} of type {}",
            self.0.id(),
            self.0.registry_name()
        )
    }
}

/// `ValidationContext.RecursiveReferenceProblem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecursiveReferenceProblem(pub LootReferenceKey);

impl Problem for RecursiveReferenceProblem {
    fn description(&self) -> String {
        format!(
            "{} of type {} is recursively called",
            self.0.id(),
            self.0.registry_name()
        )
    }
}

/// `ValidationContext.ReferenceNotAllowedProblem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceNotAllowedProblem(pub LootReferenceKey);

impl Problem for ReferenceNotAllowedProblem {
    fn description(&self) -> String {
        format!(
            "Reference to {} of type {} was used, but references are not allowed",
            self.0.id(),
            self.0.registry_name()
        )
    }
}

/// `ValidationContext.ParametersNotProvidedProblem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParametersNotProvidedProblem(pub Vec<ContextKey>);

impl Problem for ParametersNotProvidedProblem {
    fn description(&self) -> String {
        let names: Vec<String> = self.0.iter().map(|k| k.name().to_string()).collect();
        format!(
            "Parameters [{}] are not provided in this context",
            names.join(", ")
        )
    }
}

/// `AlternativesEntry.UnreachableProblem` — `"Unreachable entry!"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnreachableEntryProblem;

impl Problem for UnreachableEntryProblem {
    fn description(&self) -> String {
        "Unreachable entry!".to_string()
    }
}

/// `net.minecraft.world.level.storage.loot.ValidationContext`.
pub struct ValidationContext<'a> {
    reporter: Rc<dyn ProblemReporter>,
    context_key_set: ContextKeySet,
    resolver: Option<&'a LootRegistry>,
    visited_elements: IndexSet<LootReferenceKey>,
}

impl<'a> ValidationContext<'a> {
    /// Creates a `ValidationContext` with an optional [`LootRegistry`] resolver.
    #[must_use]
    pub fn new(
        reporter: Rc<dyn ProblemReporter>,
        context_key_set: ContextKeySet,
        resolver: Option<&'a LootRegistry>,
    ) -> Self {
        Self {
            reporter,
            context_key_set,
            resolver,
            visited_elements: IndexSet::new(),
        }
    }

    /// Creates a `ValidationContext` without a resolver (references are reported as disallowed).
    #[must_use]
    pub fn without_resolver(
        reporter: Rc<dyn ProblemReporter>,
        context_key_set: ContextKeySet,
    ) -> Self {
        Self::new(reporter, context_key_set, None)
    }

    /// `ValidationContext.forChild(PathElement)`.
    #[must_use]
    pub fn for_child(&self, sub_context: Rc<dyn PathElement>) -> Self {
        Self {
            reporter: self.reporter.for_child(sub_context),
            context_key_set: self.context_key_set.clone(),
            resolver: self.resolver,
            visited_elements: self.visited_elements.clone(),
        }
    }

    /// `ValidationContext.forField(String)`.
    #[must_use]
    pub fn for_field(&self, field_name: &str) -> Self {
        self.for_child(Rc::new(FieldPathElement(field_name.to_string())))
    }

    /// `ValidationContext.forIndexedField(String, int)`.
    #[must_use]
    pub fn for_indexed_field(&self, field_name: &str, index: i32) -> Self {
        self.for_child(Rc::new(IndexedFieldPathElement(
            field_name.to_string(),
            index,
        )))
    }

    /// `ValidationContext.forMapField(String, String)`.
    #[must_use]
    pub fn for_map_field(&self, field_name: &str, key: &str) -> Self {
        self.for_child(Rc::new(MapEntryPathElement(
            field_name.to_string(),
            key.to_string(),
        )))
    }

    /// `ValidationContext.enterElement(PathElement, ResourceKey)` — records the
    /// visited key and forks the reporter path.
    #[must_use]
    pub fn enter_element(
        &self,
        sub_context: Rc<dyn PathElement>,
        element: LootReferenceKey,
    ) -> Self {
        let mut visited = self.visited_elements.clone();
        visited.insert(element);
        Self {
            reporter: self.reporter.for_child(sub_context),
            context_key_set: self.context_key_set.clone(),
            resolver: self.resolver,
            visited_elements: visited,
        }
    }

    /// `ValidationContext.hasVisitedElement(ResourceKey)`.
    #[must_use]
    pub fn has_visited_element(&self, element: &LootReferenceKey) -> bool {
        self.visited_elements.contains(element)
    }

    /// `ValidationContext.reportProblem(Problem)`.
    pub fn report_problem(&self, problem: Rc<dyn Problem>) {
        self.reporter.report(problem);
    }

    /// `ValidationContext.validateContextUsage(LootContextUser)`.
    pub fn validate_context_usage<U: LootContextUser + ?Sized>(&self, user: &U) {
        let referenced = user.referenced_context_params();
        let not_allowed: Vec<ContextKey> = referenced
            .into_iter()
            .filter(|key| !self.context_key_set.is_allowed(key))
            .collect();
        if !not_allowed.is_empty() {
            self.report_problem(Rc::new(ParametersNotProvidedProblem(not_allowed)));
        }
    }

    /// `ValidationContext.allowsReferences()`.
    #[must_use]
    pub fn allows_references(&self) -> bool {
        self.resolver.is_some()
    }

    /// Returns the optional resolver.
    #[must_use]
    pub fn resolver(&self) -> Option<&'a LootRegistry> {
        self.resolver
    }

    /// `ValidationContext.setContextKeySet(ContextKeySet)`.
    #[must_use]
    pub fn with_context_key_set(&self, context_key_set: ContextKeySet) -> Self {
        Self {
            reporter: Rc::clone(&self.reporter),
            context_key_set,
            resolver: self.resolver,
            visited_elements: self.visited_elements.clone(),
        }
    }

    /// `Validatable.validateReference(ValidationContext, ResourceKey)` — checks
    /// resolver availability, recursion, presence, and recursively validates the
    /// referenced element.
    pub fn validate_reference(&self, key: LootReferenceKey) {
        let Some(resolver) = self.resolver else {
            self.report_problem(Rc::new(ReferenceNotAllowedProblem(key)));
            return;
        };

        if self.has_visited_element(&key) {
            self.report_problem(Rc::new(RecursiveReferenceProblem(key)));
            return;
        }

        let child = self.enter_element(
            Rc::new(ElementReferencePathElement(key.clone())),
            key.clone(),
        );

        match &key {
            LootReferenceKey::Table(id) => match resolver.get_table(id) {
                Some(table) => table.validate(&child),
                None => self.report_problem(Rc::new(MissingReferenceProblem(key))),
            },
            LootReferenceKey::Condition(id) => match resolver.get_condition(id) {
                Some(condition) => condition.validate(&child),
                None => self.report_problem(Rc::new(MissingReferenceProblem(key))),
            },
            LootReferenceKey::Function(id) => match resolver.get_function(id) {
                Some(function) => function.validate(&child),
                None => self.report_problem(Rc::new(MissingReferenceProblem(key))),
            },
        }
    }
}

/// `net.minecraft.world.level.storage.loot.Validatable`.
pub trait Validatable {
    /// `Validatable.validate(ValidationContext)`.
    fn validate(&self, context: &ValidationContext<'_>);
}

/// `Validatable.validate(ValidationContext, String, Validatable)`.
pub fn validate_field<T: Validatable>(context: &ValidationContext<'_>, name: &str, value: &T) {
    value.validate(&context.for_field(name));
}

/// `Validatable.validate(ValidationContext, String, Optional<Validatable>)`.
pub fn validate_optional_field<T: Validatable>(
    context: &ValidationContext<'_>,
    name: &str,
    value: Option<&T>,
) {
    if let Some(v) = value {
        v.validate(&context.for_field(name));
    }
}

/// `Validatable.validate(ValidationContext, String, List<Validatable>)`.
pub fn validate_list<T: Validatable>(context: &ValidationContext<'_>, name: &str, values: &[T]) {
    for (index, value) in values.iter().enumerate() {
        value.validate(&context.for_indexed_field(name, index as i32));
    }
}

/// `net.minecraft.world.level.storage.loot.LootContextUser`.
pub trait LootContextUser: Validatable {
    /// `LootContextUser.getReferencedContextParams()`.
    fn referenced_context_params(&self) -> IndexSet<ContextKey> {
        IndexSet::new()
    }
}
