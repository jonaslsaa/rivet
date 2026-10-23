//! Typed synchronized state of the supported native entities. Values are
//! projected from actual constructor/load state, never reparsed saved NBT.
use rivet_registry::core::Vec3;

#[derive(Debug, Clone, PartialEq)]
pub enum MetadataValue {
    Byte(i8),
    Int(i32),
    Float(f32),
    Boolean(bool),
    /// A real, bound farm registry holder's ID and serializer.
    Registry {
        serializer: i32,
        id: u32,
    },
    OptionalBlockState(Option<u32>),
}
#[derive(Debug, Clone, PartialEq)]
pub struct MetadataItem {
    pub id: u8,
    pub value: MetadataValue,
    pub default: MetadataValue,
}
impl MetadataItem {
    pub fn new(id: u8, value: MetadataValue, default: MetadataValue) -> Self {
        Self { id, value, default }
    }
    pub fn is_default(&self) -> bool {
        self.value == self.default
    }
}
#[derive(Debug, Clone)]
pub struct EntityTrackingSnapshot {
    pub position: Vec3,
    pub movement: Vec3,
    pub rotation: [f32; 2],
    pub head_rotation: f32,
    pub on_ground: bool,
    pub metadata: Vec<MetadataItem>,
    pub experimental_minecart: bool,
    pub living: bool,
}
#[derive(Debug, Default)]
pub struct NativeLifecycleState {
    pub in_world: bool,
    pub valid: bool,
}
