//! Port of `com.mojang.datafixers.schemas.Schema`.
//!
//! A `Schema` maps type names to lazy `TypeTemplate`s and (once built) to the
//! concrete `Type`s. The `registerTypes`/`registerEntities`/
//! `registerBlockEntities` hook chain walks up to the root schema; the base
//! `Schema` registers nothing itself (subclasses register via `registerType`
//! / `registerSimple`), so at the root the chain is a no-op.
//!
//! Java's `buildTypes` builds a `RecursiveTypeFamily` when the schema has
//! recursive types. That machinery (and the optics/recursive rewrite layer it
//! feeds) is a separate, larger unit; this foundation builds every registered
//! template via `TypeTemplate.toSimpleType()` (apply to the constant family
//! that yields `emptyPart` at every index, then `apply(-1)`), which yields the
//! concrete non-recursive type. Recursion points bottom out at `emptyPart`.
//! This is a documented deferral, not a silent divergence: the builder's
//! ordering/lifecycle/error behavior (this unit's scope) is unaffected.

use crate::data_result::DataResult;
use crate::datafixers::data_fix_utils::{get_sub_version, get_version};
use crate::datafixers::types::templates::{Const, EmptyPart, EmptyPartPassthrough, Named};
use crate::datafixers::types::{AnyValue, Type, TypeFamily, TypeTemplate};
use crate::dynamic_ops::DynamicOps;
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

/// `Schema` — the schema/version-key holder.
pub struct Schema<Ops: DynamicOps + 'static> {
    pub version_key: i32,
    pub name: String,
    pub parent: Option<Arc<Schema<Ops>>>,
    /// `recursiveTypes`: name -> index (the foundation registers none yet; the
    /// field exists for `registerType`'s signature).
    pub recursive_types: HashMap<String, i32>,
    /// `typeTemplates`: name -> lazy template.
    pub type_templates: HashMap<String, Arc<dyn TypeTemplate<Ops>>>,
    /// `types`: name -> concrete type, built by `buildTypes()`.
    pub types: HashMap<String, Arc<dyn Type<Ops>>>,
}

impl<Ops: DynamicOps + 'static> Debug for Schema<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Schema[{}]", self.name)
    }
}

impl<Ops: DynamicOps + 'static> Schema<Ops> {
    /// `new Schema(versionKey, parent)` — runs the register hooks then
    /// `buildTypes()`.
    pub fn new(version_key: i32, parent: Option<Arc<Schema<Ops>>>) -> Self {
        let sub_version = get_sub_version(version_key);
        let name = format!(
            "V{}{}",
            get_version(version_key),
            if sub_version == 0 {
                String::new()
            } else {
                format!(".{}", sub_version)
            }
        );
        let mut schema = Schema {
            version_key,
            name,
            parent,
            recursive_types: HashMap::new(),
            type_templates: HashMap::new(),
            types: HashMap::new(),
        };
        schema.register_types(schema.register_entities(), schema.register_block_entities());
        schema.types = schema.build_types();
        schema
    }

    /// `Schema.buildTypes()`.
    ///
    /// Ported via `TypeTemplate.toSimpleType()` (see the module docs): each
    /// registered template is applied to the constant "simple" family and
    /// evaluated at index `-1`.
    pub fn build_types(&self) -> HashMap<String, Arc<dyn Type<Ops>>> {
        let mut types: HashMap<String, Arc<dyn Type<Ops>>> = HashMap::new();
        for name in self.type_templates.keys() {
            // Java `buildTypes` builds `getTemplate(name)` (the `Named`-wrapped
            // template), so the built type carries the name like Java's.
            let template = self.get_template(name).expect("registered template");
            let ty = template.apply(&SimpleFamily::default()).apply(-1);
            types.insert(name.clone(), ty);
        }
        types
    }

    /// `Schema.types()` — the type names.
    pub fn type_names(&self) -> Vec<String> {
        self.types.keys().cloned().collect()
    }

    /// `Schema.getTypeRaw(TypeReference)`.
    pub fn get_type_raw(&self, name: &str) -> Arc<dyn Type<Ops>> {
        self.types
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("Unknown type: {}", name))
    }

    /// `Schema.getType(TypeReference)`.
    pub fn get_type(&self, name: &str) -> Arc<dyn Type<Ops>> {
        self.get_type_raw(name)
    }

    /// `Schema.resolveTemplate(name)`.
    pub fn resolve_template(&self, name: &str) -> Arc<dyn TypeTemplate<Ops>> {
        self.type_templates
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("Unknown type: {}", name))
    }

    /// `Schema.id(name)` — the recursion-point template for a recursive type,
    /// else the named template.
    pub fn id(&self, name: &str) -> Arc<dyn TypeTemplate<Ops>> {
        if self.recursive_types.contains_key(name) {
            Arc::new(RecursivePointTemplate::new(self.recursive_types[name]))
        } else {
            self.get_template(name).expect("template")
        }
    }

    /// `Schema.getTemplate(name)` — `DSL.named(name, resolveTemplate(name))`.
    pub fn get_template(&self, name: &str) -> Option<Arc<dyn TypeTemplate<Ops>>> {
        let element = self.resolve_template(name);
        Some(Arc::new(Named {
            name: name.to_string(),
            element,
        }) as Arc<dyn TypeTemplate<Ops>>)
    }

    /// `Schema.registerTypes(schema, entityTypes, blockEntityTypes)` — chains
    /// to the parent. At the root it is a no-op (the base `Schema` registers
    /// nothing; subclasses override `registerEntities`/`registerBlockEntities`
    /// to fill the maps). The entity/blockEntity maps are how Paper schemas
    /// propagate registrations up the chain; the foundation's base `Schema`
    /// leaves them empty.
    pub fn register_types(
        &mut self,
        _entity_types: HashMap<String, Arc<dyn TypeTemplate<Ops>>>,
        _block_entity_types: HashMap<String, Arc<dyn TypeTemplate<Ops>>>,
    ) {
        if let Some(parent) = &self.parent {
            // The base `Schema.registerTypes` calls `parent.registerTypes(...)`.
            // `parent` is already built; the chain exists so subclasses push
            // maps upward. The foundation has no subclasses, so this is a
            // faithful no-op pass-through.
            let _ = parent;
        }
    }

    /// `Schema.registerEntities(schema)` — walks to the root.
    pub fn register_entities(&self) -> HashMap<String, Arc<dyn TypeTemplate<Ops>>> {
        if let Some(parent) = &self.parent {
            parent.register_entities()
        } else {
            HashMap::new()
        }
    }

    /// `Schema.registerBlockEntities(schema)`.
    pub fn register_block_entities(&self) -> HashMap<String, Arc<dyn TypeTemplate<Ops>>> {
        if let Some(parent) = &self.parent {
            parent.register_block_entities()
        } else {
            HashMap::new()
        }
    }

    /// `Schema.registerSimple(map, name)` — `register(map, name, DSL::remainder)`.
    pub fn register_simple(
        &self,
        map: &mut HashMap<String, Arc<dyn TypeTemplate<Ops>>>,
        name: &str,
    ) {
        self.register(
            map,
            name.to_string(),
            Arc::new(Const::new(Arc::new(EmptyPartPassthrough::new()))),
        );
    }

    /// `Schema.register(map, name, template)`.
    pub fn register(
        &self,
        map: &mut HashMap<String, Arc<dyn TypeTemplate<Ops>>>,
        name: String,
        template: Arc<dyn TypeTemplate<Ops>>,
    ) {
        map.insert(name, template);
    }

    /// `Schema.registerType(recursive, type, template)`.
    pub fn register_type(
        &mut self,
        recursive: bool,
        type_name: &str,
        template: Arc<dyn TypeTemplate<Ops>>,
    ) {
        self.type_templates.insert(type_name.to_string(), template);
        // TODO: calculate recursiveness instead of hardcoding (Java comment).
        // The foundation defers recursive schemas; the slot is still recorded
        // so `id(name)` returns a recursion-point template.
        if recursive && !self.recursive_types.contains_key(type_name) {
            let next = self.recursive_types.len() as i32;
            self.recursive_types.insert(type_name.to_string(), next);
        }
    }

    /// `Schema.getVersionKey()`.
    pub fn get_version_key(&self) -> i32 {
        self.version_key
    }

    /// `Schema.getParent()`.
    pub fn get_parent(&self) -> Option<Arc<Schema<Ops>>> {
        self.parent.clone()
    }
}

/// The constant "simple" family from `TypeTemplate.toSimpleType()`: every
/// index yields `emptyPart`.
pub struct SimpleFamily<Ops> {
    _m: std::marker::PhantomData<Ops>,
}

impl<Ops> Default for SimpleFamily<Ops> {
    fn default() -> Self {
        SimpleFamily {
            _m: std::marker::PhantomData,
        }
    }
}

impl<Ops: DynamicOps + 'static> Debug for SimpleFamily<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SimpleFamily")
    }
}

impl<Ops: DynamicOps + 'static> TypeFamily<Ops> for SimpleFamily<Ops> {
    fn apply(&self, _index: i32) -> Arc<dyn Type<Ops>> {
        Arc::new(EmptyPart::new()) as Arc<dyn Type<Ops>>
    }
}

/// `DSL.id(index)` template — a recursion-point reference.
///
/// Applied against the simple family this yields `emptyPart` (the foundation's
/// recursive-schema deferral); against a real `RecursiveTypeFamily` it would
/// return that family's slot.
#[derive(Clone, Copy)]
pub struct RecursivePointTemplate<Ops> {
    pub index: i32,
    _m: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> Debug for RecursivePointTemplate<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Id[{}]", self.index)
    }
}

impl<Ops: DynamicOps + 'static> RecursivePointTemplate<Ops> {
    pub fn new(index: i32) -> Self {
        RecursivePointTemplate {
            index,
            _m: std::marker::PhantomData,
        }
    }
}

impl<Ops: DynamicOps + 'static> TypeTemplate<Ops> for RecursivePointTemplate<Ops> {
    fn size(&self) -> i32 {
        self.index + 1
    }

    fn apply(&self, family: &dyn TypeFamily<Ops>) -> Arc<dyn TypeFamily<Ops>> {
        let result = family.apply(self.index);
        crate::datafixers::types::fn_family(Arc::new(move |_index| result.clone()))
    }

    fn template_eq(&self, other: &dyn TypeTemplate<Ops>) -> bool {
        other
            .as_any_template()
            .downcast_ref::<RecursivePointTemplate<Ops>>()
            .map(|o| self.index == o.index)
            .unwrap_or(false)
    }

    fn as_any_template(&self) -> &dyn Any {
        self
    }
}

/// `DSL.check(name, index, element)` template.
pub struct CheckTemplate<Ops: DynamicOps + 'static> {
    pub name: String,
    pub index: i32,
    pub element: Arc<dyn TypeTemplate<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for CheckTemplate<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Check[{}]", self.name)
    }
}

impl<Ops: DynamicOps + 'static> CheckTemplate<Ops> {
    pub fn new(name: String, index: i32, element: Arc<dyn TypeTemplate<Ops>>) -> Self {
        CheckTemplate {
            name,
            index,
            element,
        }
    }
}

impl<Ops: DynamicOps + 'static> TypeTemplate<Ops> for CheckTemplate<Ops> {
    fn size(&self) -> i32 {
        (self.index + 1).max(self.element.size())
    }

    fn apply(&self, family: &dyn TypeFamily<Ops>) -> Arc<dyn TypeFamily<Ops>> {
        let element_family = self.element.apply(family);
        let name = self.name.clone();
        let index = self.index;
        crate::datafixers::types::fn_family(Arc::new(move |apply_index| {
            let element_type = element_family.apply(apply_index);
            Arc::new(CheckType {
                name: name.clone(),
                index: apply_index,
                expected_index: index,
                delegate: element_type,
            }) as Arc<dyn Type<Ops>>
        }))
    }

    fn template_eq(&self, other: &dyn TypeTemplate<Ops>) -> bool {
        other
            .as_any_template()
            .downcast_ref::<CheckTemplate<Ops>>()
            .map(|o| {
                self.name == o.name
                    && self.index == o.index
                    && self.element.template_eq(o.element.as_ref())
            })
            .unwrap_or(false)
    }

    fn as_any_template(&self) -> &dyn Any {
        self
    }
}

/// `CheckType` — a `Type` that only decodes at the expected index.
pub struct CheckType<Ops: DynamicOps + 'static> {
    pub name: String,
    pub index: i32,
    pub expected_index: i32,
    pub delegate: Arc<dyn Type<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for CheckType<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CheckType[{}, {}]", self.index, self.expected_index)
    }
}

impl<Ops: DynamicOps + 'static> Type<Ops> for CheckType<Ops> {
    fn read(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(AnyValue, Ops::Output)> {
        if self.index != self.expected_index {
            return DataResult::error(format!(
                "Index mismatch: {} != {}",
                self.index, self.expected_index
            ));
        }
        self.delegate.read(ops, input)
    }

    fn write(&self, ops: &Ops, value: &AnyValue, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        self.delegate.write(ops, value, prefix)
    }

    fn equals_(
        &self,
        other: &dyn Type<Ops>,
        ignore_recursion_points: bool,
        check_index: bool,
    ) -> bool {
        if let Some(other) = other.as_any_type().downcast_ref::<CheckType<Ops>>() {
            if self.index == other.index && self.expected_index == other.expected_index {
                if !check_index {
                    return true;
                }
                return self.delegate.equals_(
                    other.delegate.as_ref(),
                    ignore_recursion_points,
                    check_index,
                );
            }
            false
        } else {
            false
        }
    }

    fn template(&self) -> Arc<dyn TypeTemplate<Ops>> {
        Arc::new(CheckTemplate::new(
            self.name.clone(),
            self.expected_index,
            self.delegate.template(),
        ))
    }

    fn find_checked_type(&self, index: i32) -> Option<Arc<dyn Type<Ops>>> {
        if index == self.expected_index {
            Some(self.delegate.clone())
        } else {
            None
        }
    }

    fn find_field_type_opt(&self, name: &str) -> Option<Arc<dyn Type<Ops>>> {
        if self.index == self.expected_index {
            self.delegate.find_field_type_opt(name)
        } else {
            None
        }
    }

    fn point(&self, ops: &Ops) -> Option<AnyValue> {
        if self.index == self.expected_index {
            self.delegate.point(ops)
        } else {
            None
        }
    }

    fn type_to_string(&self) -> String {
        format!("CheckType[{}, {}]", self.index, self.expected_index)
    }

    fn clone_ty(&self) -> Arc<dyn Type<Ops>> {
        Arc::new(CheckType {
            name: self.name.clone(),
            index: self.index,
            expected_index: self.expected_index,
            delegate: self.delegate.clone(),
        })
    }

    fn as_any_type(&self) -> &dyn Any {
        self
    }
}

/// `DSL.TypeReference` — a named type reference usable with a schema.
///
/// Java's `TypeReference.in(schema)` returns `schema.id(typeName())`. The
/// port keeps the name accessor; `Schema::id` is called directly.
pub trait TypeReference {
    /// `TypeReference.typeName()`.
    fn type_name(&self) -> &str;
}
