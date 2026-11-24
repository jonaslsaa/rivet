//! Port of `net.minecraft.world.entity.EntityAttachment` and `EntityAttachments` (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).

use rivet_registry::world::phys::Vec3;
use rivet_util::mth;

/// Port of `net.minecraft.world.entity.EntityAttachment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum EntityAttachment {
    Passenger = 0,
    Vehicle = 1,
    NameTag = 2,
    WardenChest = 3,
}

impl EntityAttachment {
    pub const VALUES: [EntityAttachment; 4] = [
        EntityAttachment::Passenger,
        EntityAttachment::Vehicle,
        EntityAttachment::NameTag,
        EntityAttachment::WardenChest,
    ];

    /// Paper `EntityAttachment#createFallbackPoints(float width, float height)`.
    #[must_use]
    pub fn create_fallback_points(self, _width: f32, height: f32) -> Vec<Vec3> {
        match self {
            Self::Passenger | Self::NameTag => vec![Vec3::new(0.0, f64::from(height), 0.0)],
            Self::Vehicle => vec![Vec3::new(0.0, 0.0, 0.0)],
            Self::WardenChest => vec![Vec3::new(0.0, f64::from(height / 2.0_f32), 0.0)],
        }
    }
}

/// Port of `net.minecraft.world.entity.EntityAttachments`.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityAttachments {
    points: [Vec<Vec3>; 4],
}

impl EntityAttachments {
    /// Paper `EntityAttachments.createDefault(width, height)`.
    #[must_use]
    pub fn create_default(width: f32, height: f32) -> Self {
        Self::builder().build(width, height)
    }

    /// Paper `EntityAttachments.builder()`.
    #[must_use]
    pub fn builder() -> EntityAttachmentsBuilder {
        EntityAttachmentsBuilder::default()
    }

    /// Paper `EntityAttachments#scale(float x, float y, float z)`.
    #[must_use]
    pub fn scale(&self, x: f32, y: f32, z: f32) -> Self {
        let sx = f64::from(x);
        let sy = f64::from(y);
        let sz = f64::from(z);
        let points = std::array::from_fn(|i| {
            self.points[i]
                .iter()
                .map(|v| Vec3::new(v.x * sx, v.y * sy, v.z * sz))
                .collect()
        });
        Self { points }
    }

    /// Paper `EntityAttachments#getNullable(EntityAttachment, int, float)`.
    #[must_use]
    pub fn get_nullable(
        &self,
        attachment: EntityAttachment,
        index: i32,
        rot_y: f32,
    ) -> Option<Vec3> {
        let list = &self.points[attachment as usize];
        if index < 0 {
            return None;
        }
        list.get(index as usize)
            .map(|&point| transform_point(point, rot_y))
    }

    /// Paper `EntityAttachments#get(EntityAttachment, int, float)`.
    ///
    /// Panics if no attachment point exists for `(attachment, index)` (matching Java
    /// `IllegalStateException`).
    #[must_use]
    pub fn get(&self, attachment: EntityAttachment, index: i32, rot_y: f32) -> Vec3 {
        self.get_nullable(attachment, index, rot_y)
            .unwrap_or_else(|| {
                panic!("Had no attachment point of type: {attachment:?} for index: {index}")
            })
    }

    /// Paper `EntityAttachments#getAverage(EntityAttachment)`.
    #[must_use]
    pub fn get_average(&self, attachment: EntityAttachment) -> Vec3 {
        let list = &self.points[attachment as usize];
        assert!(
            !list.is_empty(),
            "No attachment points of type: {attachment:?}"
        );
        let mut sum = Vec3::new(0.0, 0.0, 0.0);
        for &p in list {
            sum = Vec3::new(sum.x + p.x, sum.y + p.y, sum.z + p.z);
        }
        let inv = f64::from(1.0_f32 / (list.len() as f32));
        Vec3::new(sum.x * inv, sum.y * inv, sum.z * inv)
    }

    /// Paper `EntityAttachments#getClamped(EntityAttachment, int, float)`.
    #[must_use]
    pub fn get_clamped(&self, attachment: EntityAttachment, index: i32, rot_y: f32) -> Vec3 {
        let list = &self.points[attachment as usize];
        assert!(
            !list.is_empty(),
            "Had no attachment points of type: {attachment:?}"
        );
        let max_idx = (list.len() - 1) as i32;
        let clamped = index.clamp(0, max_idx) as usize;
        transform_point(list[clamped], rot_y)
    }
}

/// Paper `EntityAttachments.transformPoint(Vec3, float)` -> `point.yRot(-rotY * Mth.DEG_TO_RAD)`.
fn transform_point(point: Vec3, rot_y: f32) -> Vec3 {
    let angle = -rot_y * mth::DEG_TO_RAD;
    let cos = f64::from(mth::cos(f64::from(angle)));
    let sin = f64::from(mth::sin(f64::from(angle)));
    let x = point.x * cos + point.z * sin;
    let z = point.z * cos - point.x * sin;
    Vec3::new(x, point.y, z)
}

/// Builder for [`EntityAttachments`] (`EntityAttachments.Builder`).
#[derive(Debug, Default, Clone)]
pub struct EntityAttachmentsBuilder {
    points: [Option<Vec<Vec3>>; 4],
}

impl EntityAttachmentsBuilder {
    /// Paper `Builder#attach(EntityAttachment, float, float, float)`.
    pub fn attach_xyz(mut self, attachment: EntityAttachment, x: f32, y: f32, z: f32) -> Self {
        self.points[attachment as usize]
            .get_or_insert_with(|| Vec::with_capacity(1))
            .push(Vec3::new(f64::from(x), f64::from(y), f64::from(z)));
        self
    }

    /// Paper `Builder#attach(EntityAttachment, Vec3)`.
    pub fn attach(mut self, attachment: EntityAttachment, point: Vec3) -> Self {
        self.points[attachment as usize]
            .get_or_insert_with(|| Vec::with_capacity(1))
            .push(point);
        self
    }

    /// Paper `Builder#build(float width, float height)`.
    #[must_use]
    pub fn build(self, width: f32, height: f32) -> EntityAttachments {
        let points = std::array::from_fn(|i| {
            let att = EntityAttachment::VALUES[i];
            self.points[i]
                .clone()
                .unwrap_or_else(|| att.create_fallback_points(width, height))
        });
        EntityAttachments { points }
    }
}
