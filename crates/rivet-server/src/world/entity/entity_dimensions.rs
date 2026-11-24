//! Port of `net.minecraft.world.entity.EntityDimensions` (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).

use crate::world::entity::entity_attachments::{EntityAttachments, EntityAttachmentsBuilder};

use rivet_registry::world::phys::AABB;
use rivet_registry::world::phys::Vec3;

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
    pub fn make_bounding_box(&self, pos: Vec3) -> AABB {
        self.make_bounding_box_xyz(pos.x, pos.y, pos.z)
    }

    /// Paper `EntityDimensions#makeBoundingBox(double, double, double)`.
    #[must_use]
    pub fn make_bounding_box_xyz(&self, x: f64, y: f64, z: f64) -> AABB {
        let w = f64::from(self.width / 2.0_f32);
        let h = f64::from(self.height);
        AABB::new(x - w, y, z - w, x + w, y + h, z + w)
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
