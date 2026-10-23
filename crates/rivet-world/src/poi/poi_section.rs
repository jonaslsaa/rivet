//! Source PoiSection, with shared record identity in both indices. Packing
//! reproduces fastutil short-map order. Cross-type stream order depends on
//! JVM Holder identity hashes and is deliberately not invented here.
use super::{PackedPoiRecord, PoiRecord, PoiType};
use rivet_registry::{
    Holder,
    core::{BlockPos, JavaBlockPosSet, SectionPos},
    holder_lookup::HolderLookup,
    registry_ops::RegistryOpsLookup,
};
use rivet_serialization::{
    codec::{self, Codec},
    dynamic_ops::DynamicOps,
    record_builder::{self, RecordCodecBuilder},
};
use rivet_util::short2object_open_hash_map::Short2ObjectOpenHashMap;
use std::{collections::HashMap, sync::Arc};

#[cfg(test)]
#[path = "poi_section_fixture.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoiOccupancy {
    HasSpace,
    IsOccupied,
    Any,
}
impl PoiOccupancy {
    fn test(self, record: &PoiRecord) -> bool {
        match self {
            Self::HasSpace => record.has_space(),
            Self::IsOccupied => record.is_occupied(),
            Self::Any => true,
        }
    }
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PoiSectionError {
    #[error("POI never registered at {0:?}")]
    MissingRecord(BlockPos),
}
#[derive(Default)]
struct RecordSet {
    order: JavaBlockPosSet,
    values: HashMap<BlockPos, Arc<PoiRecord>>,
}
impl RecordSet {
    fn add(&mut self, record: Arc<PoiRecord>) {
        if self
            .order
            .try_insert(record.pos())
            .unwrap_or_else(|error| std::panic::panic_any(error))
        {
            self.values.insert(record.pos(), record);
        }
    }
    fn remove(&mut self, pos: &BlockPos) {
        self.order.remove(pos);
        self.values.remove(pos);
    }
    fn iter(&self) -> impl Iterator<Item = &Arc<PoiRecord>> {
        self.order
            .iter()
            .map(|pos| self.values.get(pos).expect("POI set index lost record"))
    }
}
pub struct PoiSection {
    records: Short2ObjectOpenHashMap<Arc<PoiRecord>>,
    // Holder equality is registry-owner/id (or Direct value), never a guessed
    // JVM identity hash. Cross-type iteration is intentionally not exposed.
    by_type: Vec<(Holder<PoiType>, RecordSet)>,
    set_dirty: Arc<dyn Fn() + Send + Sync>,
    is_valid: bool,
}
impl PoiSection {
    pub fn new(set_dirty: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            records: Short2ObjectOpenHashMap::default(),
            by_type: Vec::new(),
            set_dirty,
            is_valid: true,
        }
    }
    pub fn is_valid(&self) -> bool {
        self.is_valid
    }
    /// Moonrise's exact conjunction: retained empty byType buckets make a
    /// section non-empty even after its last current record was removed.
    pub fn is_empty(&self) -> bool {
        self.is_valid && self.records.is_empty() && self.by_type.is_empty()
    }
    pub fn pack(&self) -> PackedPoiSection {
        PackedPoiSection {
            is_valid: self.is_valid,
            records: self
                .records
                .values_stream()
                .map(|record| record.pack())
                .collect(),
        }
    }
    fn add_record(&mut self, record: Arc<PoiRecord>) -> bool {
        let pos = record.pos();
        let kind = record.poi_type().clone();
        let key = SectionPos::section_relative_pos(&pos);
        if let Some(previous) = self.records.get(key) {
            if previous.poi_type() == &kind {
                return false;
            }
            eprintln!("POI data mismatch: already registered at {pos:?}");
        }
        // Source intentionally does not remove a mismatched old record from
        // its previous byType bucket. Both objects retain independent tickets.
        self.records.put(key, Arc::clone(&record));
        if let Some((_, records)) = self.by_type.iter_mut().find(|(key, _)| key == &kind) {
            records.add(record);
        } else {
            let mut records = RecordSet::default();
            records.add(record);
            self.by_type.push((kind, records));
        }
        true
    }
    pub fn add(
        &mut self,
        pos: BlockPos,
        kind: Holder<PoiType>,
        lookup: &dyn HolderLookup<PoiType>,
    ) -> Option<Arc<PoiRecord>> {
        let record = Arc::new(PoiRecord::new(
            pos,
            kind,
            lookup,
            Arc::clone(&self.set_dirty),
        ));
        if self.add_record(Arc::clone(&record)) {
            (self.set_dirty)();
            Some(record)
        } else {
            None
        }
    }
    pub fn remove(&mut self, pos: &BlockPos) -> bool {
        let Some(record) = self.records.remove(SectionPos::section_relative_pos(pos)) else {
            eprintln!("POI data mismatch: never registered at {pos:?}");
            return false;
        };
        self.by_type
            .iter_mut()
            .find(|(kind, _)| kind == record.poi_type())
            .expect("POI record lost type bucket")
            .1
            .remove(&record.pos());
        (self.set_dirty)();
        true
    }
    /// Looks up by section-relative coordinates, like source; it deliberately
    /// does not require the caller's full position to match the stored object.
    pub fn record(&self, pos: &BlockPos) -> Option<&Arc<PoiRecord>> {
        self.records.get(SectionPos::section_relative_pos(pos))
    }
    pub fn get_free_tickets(&self, pos: &BlockPos) -> i32 {
        self.record(pos).map_or(0, |r| r.free_tickets())
    }
    pub fn get_type(&self, pos: &BlockPos) -> Option<&Holder<PoiType>> {
        self.record(pos).map(|r| r.poi_type())
    }
    pub fn exists(&self, pos: &BlockPos, predicate: impl FnOnce(&Holder<PoiType>) -> bool) -> bool {
        self.get_type(pos).is_some_and(predicate)
    }
    pub fn release(&self, pos: &BlockPos) -> Result<bool, PoiSectionError> {
        let record = self
            .record(pos)
            .ok_or(PoiSectionError::MissingRecord(*pos))?;
        let result = record.release_ticket();
        (self.set_dirty)(); // unconditional, in addition to successful record.release
        Ok(result)
    }
    /// Pure presence query: no cross-type traversal order is exposed. The
    /// predicate must be side-effect-free; callers requiring an ordered stream
    /// must supply real type traversal authority separately.
    pub fn any_record(
        &self,
        predicate: impl Fn(&Holder<PoiType>) -> bool,
        occupancy: PoiOccupancy,
    ) -> bool {
        self.by_type
            .iter()
            .any(|(kind, records)| predicate(kind) && records.iter().any(|r| occupancy.test(r)))
    }
    pub fn records_for_type<'a>(
        &'a self,
        kind: &Holder<PoiType>,
        occupancy: PoiOccupancy,
    ) -> impl Iterator<Item = &'a Arc<PoiRecord>> {
        self.by_type
            .iter()
            .find(|(key, _)| key == kind)
            .map(|(_, records)| records)
            .into_iter()
            .flat_map(RecordSet::iter)
            .filter(move |r| occupancy.test(r))
    }
    /// The caller supplies the actual selected Java byType entry order, not
    /// registry order. Each bucket retains exact Java HashSet record order.
    pub fn records_in_type_order<'a>(
        &'a self,
        types: &'a [Holder<PoiType>],
        occupancy: PoiOccupancy,
    ) -> impl Iterator<Item = &'a Arc<PoiRecord>> {
        types
            .iter()
            .flat_map(move |kind| self.records_for_type(kind, occupancy))
    }
    /// Source refresh preserves old object identity by relative key, including
    /// its old type and tickets when the supplied scan reports a new type.
    /// An updater panic retains partial reconstruction and isValid=false.
    pub fn refresh(
        &mut self,
        lookup: &dyn HolderLookup<PoiType>,
        updater: impl FnOnce(&mut dyn FnMut(BlockPos, Holder<PoiType>)),
    ) {
        if self.is_valid {
            return;
        }
        let mut old = self.records.copy_constructed();
        self.records.clear();
        self.by_type.clear();
        updater(&mut |pos, kind| {
            let key = SectionPos::section_relative_pos(&pos);
            let record = if let Some(record) = old.get(key) {
                Arc::clone(record)
            } else {
                let record = Arc::new(PoiRecord::new(
                    pos,
                    kind,
                    lookup,
                    Arc::clone(&self.set_dirty),
                ));
                old.put(key, Arc::clone(&record));
                record
            };
            self.add_record(record);
        });
        self.is_valid = true;
        (self.set_dirty)();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedPoiSection {
    pub is_valid: bool,
    pub records: Vec<PackedPoiRecord>,
}
impl PackedPoiSection {
    pub fn codec<Ops: DynamicOps + RegistryOpsLookup + 'static>() -> Arc<dyn Codec<Self, Ops>> {
        record_builder::create(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|s: &Self| s.is_valid),
                    codec::lenient_optional_field_of("Valid", codec::bool_codec::<Ops>(), false),
                ))
                .and(RecordCodecBuilder::of_named(
                    Arc::new(|s: &Self| s.records.clone()),
                    "Records".into(),
                    codec::list(PackedPoiRecord::codec::<Ops>()),
                ))
                .apply(
                    instance,
                    Arc::new(|is_valid, records| Self { is_valid, records }),
                )
        })
    }
    pub fn unpack(
        self,
        lookup: &dyn HolderLookup<PoiType>,
        set_dirty: Arc<dyn Fn() + Send + Sync>,
    ) -> PoiSection {
        let mut section = PoiSection::new(Arc::clone(&set_dirty));
        section.is_valid = self.is_valid;
        for packed in self.records {
            section.add_record(Arc::new(packed.unpack(lookup, Arc::clone(&set_dirty))));
        }
        section
    }
}
