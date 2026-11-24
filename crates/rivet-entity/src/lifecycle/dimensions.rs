//! Port of `net.minecraft.world.entity.Pose`, `EntityAttachment`, `EntityAttachments`,
//! and `EntityDimensions` (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).

use crate::storage::Aabb;
use rivet_registry::core::Vec3;
use rivet_util::mth;

/// Port of `net.minecraft.world.entity.Pose`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Pose {
    #[default]
    Standing = 0,
    FallFlying = 1,
    Sleeping = 2,
    Swimming = 3,
    SpinAttack = 4,
    Crouching = 5,
    LongJumping = 6,
    Dying = 7,
    Croaking = 8,
    UsingTongue = 9,
    Sitting = 10,
    Roaring = 11,
    Sniffing = 12,
    Emerging = 13,
    Digging = 14,
    Sliding = 15,
    Shooting = 16,
    Inhaling = 17,
}

impl Pose {
    /// All poses in exact Paper enum/id order.
    pub const VALUES: [Pose; 18] = [
        Pose::Standing,
        Pose::FallFlying,
        Pose::Sleeping,
        Pose::Swimming,
        Pose::SpinAttack,
        Pose::Crouching,
        Pose::LongJumping,
        Pose::Dying,
        Pose::Croaking,
        Pose::UsingTongue,
        Pose::Sitting,
        Pose::Roaring,
        Pose::Sniffing,
        Pose::Emerging,
        Pose::Digging,
        Pose::Sliding,
        Pose::Shooting,
        Pose::Inhaling,
    ];

    /// Paper `Pose#id()`.
    #[must_use]
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// Paper `Pose.BY_ID` (`OutOfBoundsStrategy.ZERO` -> `Standing`).
    #[must_use]
    pub fn by_id(id: i32) -> Self {
        if id < 0 {
            return Self::Standing;
        }
        Self::VALUES
            .get(id as usize)
            .copied()
            .unwrap_or(Self::Standing)
    }

    /// Paper `Pose#getSerializedName()`.
    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Standing => "standing",
            Self::FallFlying => "fall_flying",
            Self::Sleeping => "sleeping",
            Self::Swimming => "swimming",
            Self::SpinAttack => "spin_attack",
            Self::Crouching => "crouching",
            Self::LongJumping => "long_jumping",
            Self::Dying => "dying",
            Self::Croaking => "croaking",
            Self::UsingTongue => "using_tongue",
            Self::Sitting => "sitting",
            Self::Roaring => "roaring",
            Self::Sniffing => "sniffing",
            Self::Emerging => "emerging",
            Self::Digging => "digging",
            Self::Sliding => "sliding",
            Self::Shooting => "shooting",
            Self::Inhaling => "inhaling",
        }
    }
}

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

/// Port of `net.minecraft.world.entity.EntityDimensions`.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityDimensions {
    pub width: f32,
    pub height: f32,
    pub eye_height: f32,
    pub attachments: EntityAttachments,
    pub fixed: bool,
}

impl EntityDimensions {
    /// Paper `EntityDimensions.defaultEyeHeight(float height)` = `height * 0.85F`.
    #[must_use]
    pub fn default_eye_height(height: f32) -> f32 {
        height * 0.85_f32
    }

    /// Paper `EntityDimensions.scalable(width, height)`.
    #[must_use]
    pub fn scalable(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            eye_height: Self::default_eye_height(height),
            attachments: EntityAttachments::create_default(width, height),
            fixed: false,
        }
    }

    /// Paper `EntityDimensions.fixed(width, height)`.
    #[must_use]
    pub fn fixed(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            eye_height: Self::default_eye_height(height),
            attachments: EntityAttachments::create_default(width, height),
            fixed: true,
        }
    }

    /// Paper `EntityDimensions#withEyeHeight(float)`.
    #[must_use]
    pub fn with_eye_height(mut self, eye_height: f32) -> Self {
        self.eye_height = eye_height;
        self
    }

    /// Paper `EntityDimensions#withAttachments(EntityAttachments.Builder)`.
    #[must_use]
    pub fn with_attachments(mut self, builder: EntityAttachmentsBuilder) -> Self {
        self.attachments = builder.build(self.width, self.height);
        self
    }

    /// Paper `EntityDimensions#makeBoundingBox(Vec3)`.
    #[must_use]
    pub fn make_bounding_box(&self, pos: Vec3) -> Aabb {
        self.make_bounding_box_xyz(pos.x, pos.y, pos.z)
    }

    /// Paper `EntityDimensions#makeBoundingBox(double, double, double)`.
    #[must_use]
    pub fn make_bounding_box_xyz(&self, x: f64, y: f64, z: f64) -> Aabb {
        let w = f64::from(self.width / 2.0_f32);
        let h = f64::from(self.height);
        Aabb::new(x - w, y, z - w, x + w, y + h, z + w)
    }

    /// Paper `EntityDimensions#scale(float)`.
    #[must_use]
    pub fn scale(&self, scale_factor: f32) -> Self {
        self.scale_xy(scale_factor, scale_factor)
    }

    /// Paper `EntityDimensions#scale(float widthScaleFactor, float heightScaleFactor)`.
    #[must_use]
    pub fn scale_xy(&self, width_scale: f32, height_scale: f32) -> Self {
        if !self.fixed && (width_scale != 1.0_f32 || height_scale != 1.0_f32) {
            Self {
                width: self.width * width_scale,
                height: self.height * height_scale,
                eye_height: self.eye_height * height_scale,
                attachments: self
                    .attachments
                    .scale(width_scale, height_scale, width_scale),
                fixed: false,
            }
        } else {
            self.clone()
        }
    }
}
