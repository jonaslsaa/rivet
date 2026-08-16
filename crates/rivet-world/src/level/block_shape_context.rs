//! Context-aware support, collision, and occlusion queries for dynamic blocks.

use std::collections::HashMap;
use std::fmt;

use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::block_state_property::PropertyValue;
use rivet_registry::core::{BlockPos, Direction};

/// The three `net.minecraft.world.level.block.SupportType` predicates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportType {
    Full,
    Center,
    Rigid,
}

/// A closed axis-aligned box in Paper's 0..16 block coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeBox {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl ShapeBox {
    pub const fn new(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Self {
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    fn translated(self, x: f64, y: f64, z: f64) -> Self {
        Self::new(
            self.min_x + x,
            self.min_y + y,
            self.min_z + z,
            self.max_x + x,
            self.max_y + y,
            self.max_z + z,
        )
    }
}

/// A union of axis-aligned boxes. The representation is intentionally small:
/// dynamic support in this slice only needs block-entity boxes and moved block
/// boxes, while the static registry keeps its generated fast-path masks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapeGeometry {
    boxes: Vec<ShapeBox>,
}

impl ShapeGeometry {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn block() -> Self {
        Self::from_box(ShapeBox::new(0.0, 0.0, 0.0, 16.0, 16.0, 16.0))
    }

    pub fn from_box(shape: ShapeBox) -> Self {
        Self { boxes: vec![shape] }
    }

    pub fn from_boxes(boxes: impl IntoIterator<Item = ShapeBox>) -> Self {
        Self {
            boxes: boxes.into_iter().collect(),
        }
    }

    pub fn translated(&self, x: f64, y: f64, z: f64) -> Self {
        Self::from_boxes(self.boxes.iter().map(|shape| shape.translated(x, y, z)))
    }

    pub fn union(&self, other: &Self) -> Self {
        Self::from_boxes(self.boxes.iter().chain(&other.boxes).copied())
    }

    fn face_rects(&self, direction: Direction) -> Vec<FaceRect> {
        let mut rects = Vec::new();
        for shape in &self.boxes {
            let rect =
                match direction {
                    Direction::Down if shape.min_y <= 0.0 && shape.max_y > 0.0 => Some(
                        FaceRect::new(shape.min_x, shape.min_z, shape.max_x, shape.max_z),
                    ),
                    Direction::Up if shape.min_y < 16.0 && shape.max_y >= 16.0 => Some(
                        FaceRect::new(shape.min_x, shape.min_z, shape.max_x, shape.max_z),
                    ),
                    Direction::North if shape.min_z <= 0.0 && shape.max_z > 0.0 => Some(
                        FaceRect::new(shape.min_x, shape.min_y, shape.max_x, shape.max_y),
                    ),
                    Direction::South if shape.min_z < 16.0 && shape.max_z >= 16.0 => Some(
                        FaceRect::new(shape.min_x, shape.min_y, shape.max_x, shape.max_y),
                    ),
                    Direction::West if shape.min_x <= 0.0 && shape.max_x > 0.0 => Some(
                        FaceRect::new(shape.min_z, shape.min_y, shape.max_z, shape.max_y),
                    ),
                    Direction::East if shape.min_x < 16.0 && shape.max_x >= 16.0 => Some(
                        FaceRect::new(shape.min_z, shape.min_y, shape.max_z, shape.max_y),
                    ),
                    _ => None,
                };
            if let Some(rect) = rect {
                rects.push(rect);
            }
        }
        rects
    }

    fn face_is_full(&self, direction: Direction) -> bool {
        let in_block = self
            .face_rects(direction)
            .into_iter()
            .filter(|rect| rect.is_within_block())
            .collect::<Vec<_>>();
        covers(&in_block, &[FaceRect::full()])
    }

    fn face_supports_center(&self, direction: Direction) -> bool {
        let target = match direction {
            Direction::Down | Direction::Up => FaceRect::new(7.0, 7.0, 9.0, 9.0),
            Direction::North | Direction::South | Direction::West | Direction::East => {
                FaceRect::new(7.0, 0.0, 9.0, 10.0)
            }
        };
        covers(&self.face_rects(direction), &[target])
    }

    fn face_supports_rigid(&self, direction: Direction) -> bool {
        covers(
            &self.face_rects(direction),
            &[
                FaceRect::new(0.0, 0.0, 2.0, 16.0),
                FaceRect::new(14.0, 0.0, 16.0, 16.0),
                FaceRect::new(2.0, 0.0, 14.0, 2.0),
                FaceRect::new(2.0, 14.0, 14.0, 16.0),
            ],
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FaceRect {
    min_u: f64,
    min_v: f64,
    max_u: f64,
    max_v: f64,
}

impl FaceRect {
    const fn new(min_u: f64, min_v: f64, max_u: f64, max_v: f64) -> Self {
        Self {
            min_u,
            min_v,
            max_u,
            max_v,
        }
    }

    const fn full() -> Self {
        Self::new(0.0, 0.0, 16.0, 16.0)
    }

    fn is_within_block(self) -> bool {
        self.min_u >= 0.0 && self.min_v >= 0.0 && self.max_u <= 16.0 && self.max_v <= 16.0
    }
}

fn covers(source: &[FaceRect], targets: &[FaceRect]) -> bool {
    targets.iter().all(|target| {
        if target.min_u >= target.max_u || target.min_v >= target.max_v {
            return true;
        }
        let mut u = vec![target.min_u, target.max_u];
        let mut v = vec![target.min_v, target.max_v];
        for rect in source {
            let min_u = rect.min_u.max(target.min_u);
            let max_u = rect.max_u.min(target.max_u);
            let min_v = rect.min_v.max(target.min_v);
            let max_v = rect.max_v.min(target.max_v);
            if min_u < max_u && min_v < max_v {
                u.push(min_u);
                u.push(max_u);
                v.push(min_v);
                v.push(max_v);
            }
        }
        u.sort_by(f64::total_cmp);
        v.sort_by(f64::total_cmp);
        u.dedup_by(|a, b| (*a - *b).abs() < f64::EPSILON);
        v.dedup_by(|a, b| (*a - *b).abs() < f64::EPSILON);
        u.windows(2).all(|u| {
            v.windows(2).all(|v| {
                let cu = (u[0] + u[1]) / 2.0;
                let cv = (v[0] + v[1]) / 2.0;
                source.iter().any(|rect| {
                    rect.min_u <= cu && cu <= rect.max_u && rect.min_v <= cv && cv <= rect.max_v
                })
            })
        })
    })
}

/// The five face predicates used by support, multiface attachment, and face
/// occlusion. Masks use `Direction::VALUES` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeQuery {
    support_full: u8,
    support_center: u8,
    support_rigid: u8,
    collision_full: u8,
    occlusion_full: u8,
}

impl ShapeQuery {
    pub const fn from_masks(
        support_full: u8,
        support_center: u8,
        support_rigid: u8,
        collision_full: u8,
        occlusion_full: u8,
    ) -> Self {
        Self {
            support_full,
            support_center,
            support_rigid,
            collision_full,
            occlusion_full,
        }
    }

    pub fn from_geometries(
        support: &ShapeGeometry,
        collision: &ShapeGeometry,
        occlusion: &ShapeGeometry,
    ) -> Self {
        let mut support_full = 0;
        let mut support_center = 0;
        let mut support_rigid = 0;
        let mut collision_full = 0;
        let mut occlusion_full = 0;
        for (index, direction) in Direction::VALUES.into_iter().enumerate() {
            let bit = 1 << index;
            if support.face_is_full(direction) {
                support_full |= bit;
            }
            if support.face_supports_center(direction) {
                support_center |= bit;
            }
            if support.face_supports_rigid(direction) {
                support_rigid |= bit;
            }
            if collision.face_is_full(direction) {
                collision_full |= bit;
            }
            if occlusion.face_is_full(direction) {
                occlusion_full |= bit;
            }
        }
        Self::from_masks(
            support_full,
            support_center,
            support_rigid,
            collision_full,
            occlusion_full,
        )
    }

    pub fn from_static_state(state: BlockState) -> Self {
        Self::from_masks(
            state.face_sturdy_mask(),
            state.center_support_mask(),
            state.rigid_support_mask(),
            state.collision_face_mask(),
            state.occlusion_face_mask(),
        )
    }

    fn bit(direction: Direction) -> u8 {
        1 << direction.get_3d_data_value()
    }

    pub fn is_supporting(self, support_type: SupportType, direction: Direction) -> bool {
        let mask = match support_type {
            SupportType::Full => self.support_full,
            SupportType::Center => self.support_center,
            SupportType::Rigid => self.support_rigid,
        };
        mask & Self::bit(direction) != 0
    }

    pub fn is_collision_face_full(self, direction: Direction) -> bool {
        self.collision_full & Self::bit(direction) != 0
    }

    pub fn is_occlusion_face_full(self, direction: Direction) -> bool {
        self.occlusion_full & Self::bit(direction) != 0
    }

    pub const fn support_full_mask(self) -> u8 {
        self.support_full
    }

    pub const fn support_center_mask(self) -> u8 {
        self.support_center
    }

    pub const fn support_rigid_mask(self) -> u8 {
        self.support_rigid
    }

    pub const fn collision_full_mask(self) -> u8 {
        self.collision_full
    }

    pub const fn occlusion_full_mask(self) -> u8 {
        self.occlusion_full
    }
}

/// Failure to answer a context-sensitive shape query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeQueryError {
    DynamicShapeContextUnavailable,
    DynamicShapeContextMissing { pos: BlockPos },
    DynamicShapeContextStateMismatch,
}

impl fmt::Display for ShapeQueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DynamicShapeContextUnavailable => {
                write!(f, "dynamic block shape requires a live or detached context")
            }
            Self::DynamicShapeContextMissing { pos } => {
                write!(f, "dynamic block shape context has no entity at {pos:?}")
            }
            Self::DynamicShapeContextStateMismatch => {
                write!(
                    f,
                    "dynamic block shape context does not match the block state"
                )
            }
        }
    }
}

impl std::error::Error for ShapeQueryError {}

/// Context queried by `WorldGenLevel` for states whose Paper shape is dynamic.
pub trait BlockShapeContext {
    fn shape_query(
        &self,
        state: &BlockState,
        pos: &BlockPos,
    ) -> Result<ShapeQuery, ShapeQueryError>;
}

/// Detached block-entity shape inputs for exact unit and oracle tests. This is
/// deliberately separate from the production region: #185/#341 own the typed
/// WorldGenRegion block-entity bridge.
#[derive(Clone, Debug, Default)]
pub struct DetachedShapeContext {
    entries: HashMap<BlockPos, DetachedShape>,
}

#[derive(Clone, Debug)]
enum DetachedShape {
    Shulker(ShulkerBoxShape),
    MovingPiston(MovingPistonShape),
}

/// Shulker box block-entity state needed by Paper's support and collision shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShulkerBoxShape {
    pub facing: Direction,
    pub progress: f32,
}

impl ShulkerBoxShape {
    pub const fn closed(facing: Direction) -> Self {
        Self {
            facing,
            progress: 0.0,
        }
    }

    pub const fn open(facing: Direction) -> Self {
        Self {
            facing,
            progress: 1.0,
        }
    }
}

/// Moving-piston block-entity state needed by Paper's delegated collision
/// shape. `moved_shape` is the already-resolved shape of the moved block;
/// source pistons instead translate `piston_head_shape`. A retracting source
/// piston also carries its stationary base/head geometry in
/// `piston_base_shape`.
#[derive(Clone, Debug, PartialEq)]
pub struct MovingPistonShape {
    pub moved_shape: ShapeGeometry,
    pub direction: Direction,
    pub extending: bool,
    pub is_source_piston: bool,
    pub progress: f32,
    pub piston_head_shape: ShapeGeometry,
    pub piston_base_shape: ShapeGeometry,
}

impl MovingPistonShape {
    pub fn new(
        moved_shape: ShapeGeometry,
        direction: Direction,
        extending: bool,
        progress: f32,
    ) -> Self {
        Self {
            moved_shape,
            direction,
            extending,
            is_source_piston: false,
            progress,
            piston_head_shape: ShapeGeometry::empty(),
            piston_base_shape: ShapeGeometry::empty(),
        }
    }
}

impl DetachedShapeContext {
    pub fn insert_shulker_box(&mut self, pos: BlockPos, shape: ShulkerBoxShape) {
        self.entries.insert(pos, DetachedShape::Shulker(shape));
    }

    pub fn insert_moving_piston(&mut self, pos: BlockPos, shape: MovingPistonShape) {
        self.entries.insert(pos, DetachedShape::MovingPiston(shape));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl BlockShapeContext for DetachedShapeContext {
    fn shape_query(
        &self,
        state: &BlockState,
        pos: &BlockPos,
    ) -> Result<ShapeQuery, ShapeQueryError> {
        let entry = self
            .entries
            .get(pos)
            .ok_or(ShapeQueryError::DynamicShapeContextMissing { pos: *pos })?;
        match entry {
            DetachedShape::Shulker(shape) if state.block().name() == "minecraft:shulker_box" => {
                let state_facing = shulker_state_facing(*state)
                    .ok_or(ShapeQueryError::DynamicShapeContextStateMismatch)?;
                if state_facing != shape.facing {
                    return Err(ShapeQueryError::DynamicShapeContextStateMismatch);
                }
                Ok(shulker_query(ShulkerBoxShape {
                    facing: state_facing,
                    ..*shape
                }))
            }
            DetachedShape::MovingPiston(shape)
                if state.block().name() == "minecraft:moving_piston" =>
            {
                Ok(moving_piston_query(shape))
            }
            _ => Err(ShapeQueryError::DynamicShapeContextStateMismatch),
        }
    }
}

fn shulker_state_facing(state: BlockState) -> Option<Direction> {
    let PropertyValue::Enum(name) = state.get_value(BlockStateProperties::FACING)? else {
        return None;
    };
    Direction::VALUES
        .into_iter()
        .find(|direction| direction.get_serialized_name() == name)
}

fn shulker_query(shape: ShulkerBoxShape) -> ShapeQuery {
    let progress = f64::from(shape.progress.clamp(0.0, 1.0));
    let shift = progress * 16.0;
    let collision = match shape.facing {
        Direction::Down => {
            ShapeGeometry::from_box(ShapeBox::new(0.0, -shift, 0.0, 16.0, 16.0, 16.0))
        }
        Direction::Up => {
            ShapeGeometry::from_box(ShapeBox::new(0.0, 0.0, 0.0, 16.0, 16.0 + shift, 16.0))
        }
        Direction::North => {
            ShapeGeometry::from_box(ShapeBox::new(0.0, 0.0, -shift, 16.0, 16.0, 16.0))
        }
        Direction::South => {
            ShapeGeometry::from_box(ShapeBox::new(0.0, 0.0, 0.0, 16.0, 16.0, 16.0 + shift))
        }
        Direction::West => {
            ShapeGeometry::from_box(ShapeBox::new(-shift, 0.0, 0.0, 16.0, 16.0, 16.0))
        }
        Direction::East => {
            ShapeGeometry::from_box(ShapeBox::new(0.0, 0.0, 0.0, 16.0 + shift, 16.0, 16.0))
        }
    };
    let support = if progress == 0.0 {
        ShapeGeometry::block()
    } else {
        thin_support(shape.facing.get_opposite())
    };
    ShapeQuery::from_geometries(&support, &collision, &collision)
}

fn thin_support(face: Direction) -> ShapeGeometry {
    match face {
        Direction::Down => ShapeGeometry::from_box(ShapeBox::new(0.0, 0.0, 0.0, 16.0, 1.0, 16.0)),
        Direction::Up => ShapeGeometry::from_box(ShapeBox::new(0.0, 15.0, 0.0, 16.0, 16.0, 16.0)),
        Direction::North => ShapeGeometry::from_box(ShapeBox::new(0.0, 0.0, 0.0, 16.0, 16.0, 1.0)),
        Direction::South => {
            ShapeGeometry::from_box(ShapeBox::new(0.0, 0.0, 15.0, 16.0, 16.0, 16.0))
        }
        Direction::West => ShapeGeometry::from_box(ShapeBox::new(0.0, 0.0, 0.0, 1.0, 16.0, 16.0)),
        Direction::East => ShapeGeometry::from_box(ShapeBox::new(15.0, 0.0, 0.0, 16.0, 16.0, 16.0)),
    }
}

fn moving_piston_query(entity: &MovingPistonShape) -> ShapeQuery {
    let progress = f64::from(entity.progress.clamp(0.0, 1.0));
    let extended_progress = if entity.extending {
        progress - 1.0
    } else {
        1.0 - progress
    };
    let dx = f64::from(entity.direction.step_x()) * extended_progress * 16.0;
    let dy = f64::from(entity.direction.step_y()) * extended_progress * 16.0;
    let dz = f64::from(entity.direction.step_z()) * extended_progress * 16.0;
    let translated_shape = if entity.is_source_piston {
        &entity.piston_head_shape
    } else {
        &entity.moved_shape
    };
    let stationary_shape = if entity.is_source_piston && !entity.extending {
        &entity.piston_base_shape
    } else {
        &ShapeGeometry::empty()
    };
    let collision = stationary_shape.union(&translated_shape.translated(dx, dy, dz));
    ShapeQuery::from_geometries(&collision, &collision, &ShapeGeometry::empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_types_match_paper_overlap_rules() {
        let full = ShapeGeometry::block();
        let center = ShapeGeometry::from_box(ShapeBox::new(2.0, 0.0, 2.0, 14.0, 16.0, 14.0));
        let ring = ShapeGeometry::from_boxes([
            ShapeBox::new(0.0, 0.0, 0.0, 2.0, 16.0, 16.0),
            ShapeBox::new(14.0, 0.0, 0.0, 16.0, 16.0, 16.0),
            ShapeBox::new(2.0, 0.0, 0.0, 14.0, 16.0, 2.0),
            ShapeBox::new(2.0, 0.0, 14.0, 14.0, 16.0, 16.0),
        ]);
        let full_query = ShapeQuery::from_geometries(&full, &full, &full);
        assert!(full_query.is_supporting(SupportType::Full, Direction::Up));
        assert!(full_query.is_supporting(SupportType::Center, Direction::Up));
        assert!(full_query.is_supporting(SupportType::Rigid, Direction::Up));
        let center_query = ShapeQuery::from_geometries(&center, &center, &center);
        assert!(!center_query.is_supporting(SupportType::Full, Direction::Up));
        assert!(center_query.is_supporting(SupportType::Center, Direction::Up));
        assert!(!center_query.is_supporting(SupportType::Rigid, Direction::Up));
        let ring_query = ShapeQuery::from_geometries(&ring, &ring, &ring);
        assert!(!ring_query.is_supporting(SupportType::Full, Direction::Up));
        assert!(!ring_query.is_supporting(SupportType::Center, Direction::Up));
        assert!(ring_query.is_supporting(SupportType::Rigid, Direction::Up));
    }

    #[test]
    fn center_support_matches_paper_two_wide_column() {
        let centered_column = ShapeGeometry::from_box(ShapeBox::new(7.0, 0.0, 7.0, 9.0, 16.0, 9.0));
        let query =
            ShapeQuery::from_geometries(&centered_column, &centered_column, &centered_column);

        assert!(!query.is_supporting(SupportType::Full, Direction::Up));
        assert!(query.is_supporting(SupportType::Center, Direction::Up));
        assert!(query.is_supporting(SupportType::Center, Direction::Down));

        let off_center = ShapeGeometry::from_box(ShapeBox::new(6.0, 0.0, 7.0, 8.0, 16.0, 9.0));
        let off_center_query = ShapeQuery::from_geometries(&off_center, &off_center, &off_center);
        assert!(!off_center_query.is_supporting(SupportType::Center, Direction::Up));

        let north_column = ShapeGeometry::from_box(ShapeBox::new(7.0, 0.0, 0.0, 9.0, 10.0, 2.0));
        let north_query = ShapeQuery::from_geometries(&north_column, &north_column, &north_column);
        assert!(north_query.is_supporting(SupportType::Center, Direction::North));

        let short_north_column =
            ShapeGeometry::from_box(ShapeBox::new(7.0, 0.0, 0.0, 9.0, 9.0, 2.0));
        let short_north_query = ShapeQuery::from_geometries(
            &short_north_column,
            &short_north_column,
            &short_north_column,
        );
        assert!(!short_north_query.is_supporting(SupportType::Center, Direction::North));
    }

    #[test]
    fn shulker_context_changes_open_collision_and_support() {
        let closed = shulker_query(ShulkerBoxShape::closed(Direction::Up));
        let open = shulker_query(ShulkerBoxShape::open(Direction::Up));
        assert!(closed.is_collision_face_full(Direction::Up));
        assert!(open.is_collision_face_full(Direction::Up));
        assert!(!open.is_supporting(SupportType::Full, Direction::Up));
        assert!(open.is_supporting(SupportType::Full, Direction::Down));
    }

    #[test]
    fn detached_shulker_rejects_context_facing_mismatch() {
        let pos = BlockPos::new(0, 0, 0);
        let state = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:shulker_box").unwrap(),
        )
        .set_value(BlockStateProperties::FACING, Direction::Up)
        .unwrap();
        let mut context = DetachedShapeContext::default();
        context.insert_shulker_box(pos, ShulkerBoxShape::closed(Direction::North));

        assert_eq!(
            context.shape_query(&state, &pos),
            Err(ShapeQueryError::DynamicShapeContextStateMismatch)
        );
    }

    #[test]
    fn moving_piston_delegates_and_translates_moved_shape() {
        let entity = MovingPistonShape::new(ShapeGeometry::block(), Direction::East, true, 0.5);
        let query = moving_piston_query(&entity);
        assert!(query.is_collision_face_full(Direction::West));
        assert!(!query.is_collision_face_full(Direction::East));
        assert!(!query.is_occlusion_face_full(Direction::East));
    }

    #[test]
    fn source_piston_translates_head_instead_of_moved_block() {
        let mut entity = MovingPistonShape::new(ShapeGeometry::block(), Direction::East, true, 1.0);
        entity.is_source_piston = true;
        entity.piston_head_shape =
            ShapeGeometry::from_box(ShapeBox::new(0.0, 0.0, 0.0, 8.0, 16.0, 16.0));
        let query = moving_piston_query(&entity);
        assert!(query.is_collision_face_full(Direction::West));
        assert!(!query.is_collision_face_full(Direction::East));
    }

    #[test]
    fn retracting_source_piston_includes_stationary_base_shape() {
        let mut entity =
            MovingPistonShape::new(ShapeGeometry::empty(), Direction::East, false, 0.5);
        entity.is_source_piston = true;
        entity.piston_base_shape = ShapeGeometry::block();
        let query = moving_piston_query(&entity);
        assert!(query.is_collision_face_full(Direction::Down));
        assert!(query.is_collision_face_full(Direction::Up));
    }

    #[test]
    fn detached_fixtures_match_paper_probe_masks() {
        let fixtures = [
            (
                "shulker_closed_up",
                shulker_query(ShulkerBoxShape::closed(Direction::Up)),
            ),
            (
                "shulker_open_up",
                shulker_query(ShulkerBoxShape::open(Direction::Up)),
            ),
            (
                "moving_piston_half_east",
                moving_piston_query(&MovingPistonShape::new(
                    ShapeGeometry::block(),
                    Direction::East,
                    true,
                    0.5,
                )),
            ),
            (
                "moving_piston_start_east",
                moving_piston_query(&MovingPistonShape::new(
                    ShapeGeometry::block(),
                    Direction::East,
                    true,
                    0.0,
                )),
            ),
        ];
        for (name, query) in fixtures {
            let fixture = rivet_registry::generated::block_behaviors::dynamic_shape_fixture(name)
                .expect("fixture emitted by the pinned Paper probe");
            assert!(fixture.dynamic);
            assert_eq!(query.support_full_mask(), fixture.support_full, "{name}");
            assert_eq!(
                query.support_center_mask(),
                fixture.support_center,
                "{name}"
            );
            assert_eq!(query.support_rigid_mask(), fixture.support_rigid, "{name}");
            assert_eq!(
                query.collision_full_mask(),
                fixture.collision_full,
                "{name}"
            );
            assert_eq!(
                query.occlusion_full_mask(),
                fixture.occlusion_full,
                "{name}"
            );
        }
    }
}
