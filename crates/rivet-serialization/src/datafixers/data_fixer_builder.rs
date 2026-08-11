//! Port of `com.mojang.datafixers.DataFixerBuilder`.
//!
//! The builder's lifecycle: `addSchema` computes the version key and the parent
//! (the lowest schema with the same version as `key - 1`), `addFixer` warns and
//! drops fixes whose version exceeds the game's `dataVersion`, and `build`
//! snapshots the schemas/fixes into a [`DataFixerUpper`].
//!
//! Java's `Result.optimize` runs async point-free optimization over required
//! types; it is part of the deferred optics/recursive layer and is not ported
//! (see `DataFixerUpper`).

use crate::datafixers::data_fix::DataFix;
use crate::datafixers::data_fix_utils::{get_version, make_key_sub};
use crate::datafixers::data_fixer_upper::{DataFixerUpper, get_lowest_schema_same_version};
use crate::datafixers::schemas::Schema;
use crate::dynamic_ops::DynamicOps;
use std::sync::Arc;

/// `com.mojang.datafixers.DataFixerBuilder`.
pub struct DataFixerBuilder<Ops: DynamicOps + 'static> {
    data_version: i32,
    /// `Int2ObjectSortedMap<Schema>` — kept sorted ascending by version key.
    schemas: Vec<Arc<Schema<Ops>>>,
    global_list: Vec<Arc<DataFix<Ops>>>,
    fixer_versions: Vec<i32>,
}

impl<Ops: DynamicOps + 'static> DataFixerBuilder<Ops> {
    /// `new DataFixerBuilder(dataVersion)`.
    pub fn new(data_version: i32) -> Self {
        DataFixerBuilder {
            data_version,
            schemas: Vec::new(),
            global_list: Vec::new(),
            fixer_versions: Vec::new(),
        }
    }

    /// `addSchema(version, factory)` — `addSchema(version, 0, factory)`.
    pub fn add_schema(
        &mut self,
        version: i32,
        factory: impl FnOnce(i32, Option<Arc<Schema<Ops>>>) -> Arc<Schema<Ops>>,
    ) -> Arc<Schema<Ops>> {
        self.add_schema_sub(version, 0, factory)
    }

    /// `addSchema(version, subVersion, factory)`.
    pub fn add_schema_sub(
        &mut self,
        version: i32,
        sub_version: i32,
        factory: impl FnOnce(i32, Option<Arc<Schema<Ops>>>) -> Arc<Schema<Ops>>,
    ) -> Arc<Schema<Ops>> {
        let key = make_key_sub(version, sub_version);
        let parent = if self.schemas.is_empty() {
            None
        } else {
            let parent_index = get_lowest_schema_same_version(&self.schemas, key - 1);
            Some(self.schemas[parent_index].clone())
        };
        let schema = factory(key, parent);
        self.add_schema_obj(schema.clone());
        schema
    }

    /// `addSchema(Schema)` — insert keeping the map sorted.
    pub fn add_schema_obj(&mut self, schema: Arc<Schema<Ops>>) {
        let key = schema.get_version_key();
        let pos = self
            .schemas
            .binary_search_by_key(&key, |s| s.get_version_key())
            .unwrap_or_else(|p| p);
        self.schemas.insert(pos, schema);
    }

    /// `addFixer(DataFix)`.
    pub fn add_fixer(&mut self, fix: Arc<DataFix<Ops>>) {
        let version = get_version(fix.get_version_key());
        if version > self.data_version {
            eprintln!(
                "Ignored fix registered for version: {} as the DataVersion of the game is: {}",
                version, self.data_version
            );
            return;
        }
        self.global_list.push(fix.clone());
        let fix_version = fix.get_version_key();
        let pos = self
            .fixer_versions
            .binary_search(&fix_version)
            .unwrap_or_else(|p| p);
        self.fixer_versions.insert(pos, fix_version);
    }

    /// `build()`.
    pub fn build(&self) -> DataFixerUpper<Ops> {
        DataFixerUpper::new(
            self.schemas.clone(),
            self.global_list.clone(),
            self.fixer_versions.clone(),
        )
    }
}
