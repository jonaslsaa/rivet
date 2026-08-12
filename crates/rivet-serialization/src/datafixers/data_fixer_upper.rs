//! Port of `com.mojang.datafixers.DataFixerUpper` and the `DataFixer` interface
//! surface the builder foundation needs.
//!
//! The `OPTIMIZATION_RULE` is the point-free rewrite loop; it runs inside
//! `Type.readAndWrite` (via `f_rule`). The optimizing rules it composes only
//! fire on the optics/recursive structures the deferred layer builds, so here
//! it reduces to `everywhere(nop, nop)` — still structurally faithful (a
//! top-down `seq` of no-ops then a bottom-up `AppNest`), just with no rules
//! that can match yet.
//!
//! The `rules` cache (`Long2ObjectMap<TypeRewriteRule>`) is dropped: without
//! recursive types the rewrite is finite, so recomputation is harmless and the
//! key (`version << 32 | newVersion`) is a pure function of the arguments.

use crate::datafixers::data_fix_utils::{get_version, make_key};
use crate::datafixers::functions::rule::{
    PointFreeRule, everywhere as pf_everywhere, nop as pf_nop, seq as pf_seq,
};
use crate::datafixers::schemas::Schema;
use crate::datafixers::type_rewrite_rule;
use crate::datafixers::types::Type;
use crate::dynamic::Dynamic;
use crate::dynamic_ops::DynamicOps;
use std::sync::Arc;

/// `DataFixerUpper.OPTIMIZATION_RULE` — the point-free rewrite loop.
///
/// Java builds `everywhere(seq(CataFuseSame, CataFuseDifferent,
/// CompRewrite.together(LensComp, SortProj, SortInj)), AppNest)`. The
/// fold/lens rules are deferred, so the sequence here is empty; `AppNest` is
/// likewise deferred (it needs `Apply` nodes). The `everywhere`/`seq` skeleton
/// is preserved.
pub fn optimization_rule<Ops: DynamicOps + 'static>() -> Arc<dyn PointFreeRule<Ops>> {
    pf_everywhere(pf_seq(vec![]), pf_nop())
}

/// `com.mojang.datafixers.DataFixerUpper` (the `DataFixer` entry surface).
///
/// Ops are pinned (`DataFixerUpper<Ops>`), mirroring the codec traits. The
/// `update`/`getSchema`/`getRule` methods are ported with `Ops` threaded
/// through.
pub struct DataFixerUpper<Ops: DynamicOps + 'static> {
    pub schemas: Vec<Arc<Schema<Ops>>>,
    pub global_list: Vec<Arc<crate::datafixers::data_fix::DataFix<Ops>>>,
    pub fixer_versions: Vec<i32>,
}

impl<Ops: DynamicOps + 'static> DataFixerUpper<Ops> {
    pub fn new(
        schemas: Vec<Arc<Schema<Ops>>>,
        global_list: Vec<Arc<crate::datafixers::data_fix::DataFix<Ops>>>,
        fixer_versions: Vec<i32>,
    ) -> Self {
        DataFixerUpper {
            schemas,
            global_list,
            fixer_versions,
        }
    }

    /// `DataFixerUpper.update(type, input, version, newVersion)`.
    pub fn update(
        &self,
        ops: &Ops,
        type_name: &str,
        input: &Dynamic<Ops::Output>,
        version: i32,
        new_version: i32,
    ) -> Dynamic<Ops::Output>
    where
        Ops::Output: Clone,
    {
        if version < new_version {
            let data_type = self.get_type(type_name, version);
            let read = data_type.read_and_write(
                ops,
                self.get_type(type_name, new_version).as_ref(),
                self.get_rule(version, new_version).as_ref(),
                optimization_rule::<Ops>().as_ref(),
                &input.value,
            );
            let result = read
                .result_or_partial(|_e| {})
                .unwrap_or_else(|| input.value.clone());
            return Dynamic::new(ops, result);
        }
        input.clone()
    }

    /// `DataFixerUpper.getSchema(key)`.
    pub fn get_schema(&self, key: i32) -> Option<Arc<Schema<Ops>>> {
        let index = get_lowest_schema_same_version(&self.schemas, key);
        self.schemas.get(index).cloned()
    }

    /// `DataFixerUpper.getType(type, version)` — `getSchema(makeKey(version))
    /// .getTypeRaw(type)`.
    pub fn get_type(&self, type_name: &str, version: i32) -> Arc<dyn Type<Ops>> {
        let schema = self
            .get_schema(make_key(version))
            .expect("schema for version");
        schema.get_type_raw(type_name)
    }

    /// `DataFixerUpper.getRule(version, newVersion)`.
    pub fn get_rule(
        &self,
        version: i32,
        new_version: i32,
    ) -> Arc<dyn type_rewrite_rule::TypeRewriteRule<Ops>> {
        if version >= new_version {
            return type_rewrite_rule::nop();
        }
        let expanded_version = self.get_lowest_fix_same_version(make_key(version));
        let mut rules: Vec<Arc<dyn type_rewrite_rule::TypeRewriteRule<Ops>>> = Vec::new();
        for fix in &self.global_list {
            let expanded_fix_version = fix.get_version_key();
            let fix_version = get_version(expanded_fix_version);
            if expanded_fix_version > expanded_version && fix_version <= new_version {
                let fix_rule = fix.get_rule();
                if fix_rule.is_nop() {
                    continue;
                }
                rules.push(fix_rule);
            }
        }
        type_rewrite_rule::seq(rules)
    }

    fn get_lowest_fix_same_version(&self, version_key: i32) -> i32 {
        if version_key < *self.fixer_versions.first().expect("first fixer version") {
            return *self.fixer_versions.first().expect("first fixer version") - 1;
        }
        *self
            .fixer_versions
            .iter()
            .take_while(|&&v| v <= version_key)
            .last()
            .expect("last fixer version <= key")
    }
}

/// `getLowestSchemaSameVersion(schemas, versionKey)`.
///
/// Java's `Int2ObjectAVLTreeMap.subMap(0, versionKey + 1).lastIntKey()`.
/// The Rust port keeps `schemas` sorted ascending by `versionKey` and finds the
/// last schema whose key is `<= versionKey` (with the pre-first clamp).
pub fn get_lowest_schema_same_version<Ops: DynamicOps + 'static>(
    schemas: &[Arc<Schema<Ops>>],
    version_key: i32,
) -> usize {
    let first = schemas.first().expect("at least one schema");
    if version_key < first.get_version_key() {
        return 0;
    }
    schemas
        .iter()
        .enumerate()
        .take_while(|(_, s)| s.get_version_key() <= version_key)
        .map(|(i, _)| i)
        .last()
        .expect("a schema with key <= version_key")
}
