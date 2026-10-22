//! Source StructurePiece fill/write semantics shared by native structure bodies.
//! Block transforms intentionally support the material families used by the
//! executable bodies; another transforming family refuses rather than guessing.
use super::structure_body::MineshaftKind;
use super::{BoundingBox, StructureError, StructurePieceData, StructurePlacementAuthority};
use crate::level::block_shape_context::SupportType;
use crate::levelgen::heightmap::Types;
use rivet_registry::{
    Identifier, TagKey,
    block_state::BlockState,
    block_state_properties::BlockStateProperties as P,
    block_state_property::{Property, PropertyValue},
    core::{BlockPos, Direction},
    fluid_id::FluidId,
    generated::blocks::BlockId,
    holder::HolderId,
    registries::BIOME,
};
use rivet_util::RandomSource;

pub(super) fn block(name: &str) -> BlockState {
    BlockState::of(BlockId::from_name(name).expect("pinned structure material"))
}
pub(super) fn liquid(state: BlockState) -> bool {
    matches!(state.block().name(), "minecraft:water" | "minecraft:lava")
}
pub(super) fn replaceable(state: BlockState) -> bool {
    state.is_air()
        || liquid(state)
        || matches!(
            state.block().name(),
            "minecraft:glow_lichen" | "minecraft:seagrass" | "minecraft:tall_seagrass"
        )
}
pub(super) fn planks(kind: MineshaftKind) -> BlockState {
    block(if kind == MineshaftKind::Normal {
        "minecraft:oak_planks"
    } else {
        "minecraft:dark_oak_planks"
    })
}
pub(super) fn wood(kind: MineshaftKind) -> BlockState {
    block(if kind == MineshaftKind::Normal {
        "minecraft:oak_log"
    } else {
        "minecraft:dark_oak_log"
    })
}
pub(super) fn fence(kind: MineshaftKind) -> BlockState {
    block(if kind == MineshaftKind::Normal {
        "minecraft:oak_fence"
    } else {
        "minecraft:dark_oak_fence"
    })
}
fn property(direction: Direction) -> Property {
    match direction {
        Direction::North => P::NORTH,
        Direction::East => P::EAST,
        Direction::South => P::SOUTH,
        Direction::West => P::WEST,
        _ => unreachable!("horizontal property"),
    }
}
fn facing(value: PropertyValue) -> Direction {
    match value {
        PropertyValue::Enum("north") => Direction::North,
        PropertyValue::Enum("south") => Direction::South,
        PropertyValue::Enum("west") => Direction::West,
        PropertyValue::Enum("east") => Direction::East,
        _ => unreachable!("horizontal facing"),
    }
}
fn transformed_direction(mut direction: Direction, orientation: Option<Direction>) -> Direction {
    if matches!(orientation, Some(Direction::South | Direction::West))
        && matches!(direction, Direction::North | Direction::South)
    {
        direction = direction.get_opposite();
    }
    if matches!(orientation, Some(Direction::West | Direction::East)) {
        direction = direction.get_clock_wise();
    }
    direction
}
fn transform(
    mut state: BlockState,
    orientation: Option<Direction>,
) -> Result<BlockState, StructureError> {
    if matches!(orientation, None | Some(Direction::North)) {
        return Ok(state);
    }
    let name = state.block().name();
    match name {
        "minecraft:oak_fence" | "minecraft:dark_oak_fence" => {
            let original = state;
            for d in [
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West,
            ] {
                state = state
                    .set_value(
                        property(transformed_direction(d, orientation)),
                        original.get_value(property(d)).expect("fence direction"),
                    )
                    .expect("fence direction");
            }
        }
        "minecraft:wall_torch" => {
            let d = facing(
                state
                    .get_value(P::HORIZONTAL_FACING)
                    .expect("wall torch facing"),
            );
            state = state
                .set_value(P::HORIZONTAL_FACING, transformed_direction(d, orientation))
                .expect("wall torch facing");
        }
        "minecraft:rail" => {
            if matches!(orientation, Some(Direction::West | Direction::East)) {
                let shape = match state.get_value(P::RAIL_SHAPE) {
                    Some(PropertyValue::Enum("north_south")) => "east_west",
                    Some(PropertyValue::Enum("east_west")) => "north_south",
                    _ => {
                        return Err(StructureError::PlacementUnavailable(
                            "structure rotation for non-straight rail is not implemented".into(),
                        ));
                    }
                };
                state = state
                    .set_value(P::RAIL_SHAPE, PropertyValue::Enum(shape))
                    .expect("rail shape");
            }
        }
        "minecraft:oak_log" | "minecraft:dark_oak_log" | "minecraft:iron_chain" => {
            if matches!(orientation, Some(Direction::West | Direction::East)) {
                let axis = match state.get_value(P::AXIS) {
                    Some(PropertyValue::Enum("x")) => "z",
                    Some(PropertyValue::Enum("z")) => "x",
                    _ => "y",
                };
                state = state
                    .set_value(P::AXIS, PropertyValue::Enum(axis))
                    .expect("pillar axis");
            }
        }
        "minecraft:air"
        | "minecraft:cave_air"
        | "minecraft:oak_planks"
        | "minecraft:dark_oak_planks"
        | "minecraft:cobweb"
        | "minecraft:torch"
        | "minecraft:spawner" => {}
        _ => {
            return Err(StructureError::PlacementUnavailable(format!(
                "structure state transform for {name} is not implemented"
            )));
        }
    }
    Ok(state)
}
fn finite_box(b: [i32; 6]) -> Result<(), StructureError> {
    if (0..3).any(|axis| b[axis] <= b[axis + 3] && b[axis + 3] == i32::MAX) {
        return Err(StructureError::PlacementUnavailable(
            "Java structure fill loop would wrap indefinitely".into(),
        ));
    }
    Ok(())
}

pub(super) struct PiecePlacement<'a, A: StructurePlacementAuthority> {
    pub data: &'a StructurePieceData,
    pub mine_type: MineshaftKind,
    pub authority: &'a mut A,
    pub clip: BoundingBox,
}
impl<A: StructurePlacementAuthority> PiecePlacement<'_, A> {
    pub fn pos(&self, x: i32, y: i32, z: i32) -> BlockPos {
        self.data.world_pos(x, y, z)
    }
    pub fn get(&mut self, x: i32, y: i32, z: i32) -> BlockState {
        let p = self.pos(x, y, z);
        if self.clip.is_inside(&p) {
            self.authority.level().get_block_state(&p)
        } else {
            block("minecraft:air")
        }
    }
    pub fn interior(&mut self, x: i32, y: i32, z: i32) -> bool {
        let p = self.pos(x, y.wrapping_add(1), z);
        self.clip.is_inside(&p)
            && p.get_y()
                < self
                    .authority
                    .level()
                    .get_height_at(Types::OceanFloorWg, p.get_x(), p.get_z())
    }
    pub fn sturdy(
        &mut self,
        p: BlockPos,
        state: BlockState,
        direction: Direction,
        kind: SupportType,
    ) -> Result<bool, StructureError> {
        self.authority
            .level()
            .is_face_sturdy_with(&p, &state, &direction, kind)
            .map_err(|e| {
                StructureError::PlacementUnavailable(format!("structure support shape: {e}"))
            })
    }
    pub fn place(
        &mut self,
        state: BlockState,
        x: i32,
        y: i32,
        z: i32,
    ) -> Result<(), StructureError> {
        let p = self.pos(x, y, z);
        if !self.clip.is_inside(&p) {
            return Ok(());
        }
        let existing = self.authority.level().get_block_state(&p);
        if [
            planks(self.mine_type).block(),
            wood(self.mine_type).block(),
            fence(self.mine_type).block(),
            block("minecraft:iron_chain").block(),
        ]
        .contains(&existing.block())
        {
            return Ok(());
        }
        let state = transform(state, self.data.orientation)?;
        self.authority.level().set_block(&p, state, 2);
        if self.authority.structure_updates_handled() {
            return Ok(());
        }
        let fluid = FluidId::from_id(self.authority.level().get_block_state(&p).fluid_id());
        if fluid != FluidId::EMPTY {
            self.authority.level().schedule_tick(&p, fluid, 0);
        }
        if matches!(
            state.block().name(),
            "minecraft:nether_brick_fence"
                | "minecraft:torch"
                | "minecraft:wall_torch"
                | "minecraft:oak_fence"
                | "minecraft:spruce_fence"
                | "minecraft:dark_oak_fence"
                | "minecraft:pale_oak_fence"
                | "minecraft:acacia_fence"
                | "minecraft:birch_fence"
                | "minecraft:jungle_fence"
                | "minecraft:ladder"
                | "minecraft:iron_bars"
        ) {
            self.authority.level().mark_pos_for_post_processing(&p);
        }
        Ok(())
    }
    pub fn fill(
        &mut self,
        b: [i32; 6],
        edge: BlockState,
        fill: BlockState,
        skip_air: bool,
    ) -> Result<(), StructureError> {
        finite_box(b)?;
        let [x0, y0, z0, x1, y1, z1] = b;
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if !skip_air || !self.get(x, y, z).is_air() {
                        self.place(
                            if y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1 {
                                fill
                            } else {
                                edge
                            },
                            x,
                            y,
                            z,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)] // Source generateMaybeBox's independent switches.
    pub fn maybe_fill<R: RandomSource>(
        &mut self,
        r: &mut R,
        prob: f32,
        b: [i32; 6],
        edge: BlockState,
        fill: BlockState,
        skip_air: bool,
        inside: bool,
    ) -> Result<(), StructureError> {
        finite_box(b)?;
        let [x0, y0, z0, x1, y1, z1] = b;
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if r.next_float().partial_cmp(&prob) != Some(std::cmp::Ordering::Greater)
                        && (!skip_air || !self.get(x, y, z).is_air())
                        && (!inside || self.interior(x, y, z))
                    {
                        self.place(
                            if y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1 {
                                fill
                            } else {
                                edge
                            },
                            x,
                            y,
                            z,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
    pub fn maybe_block<R: RandomSource>(
        &mut self,
        r: &mut R,
        prob: f32,
        x: i32,
        y: i32,
        z: i32,
        state: BlockState,
    ) -> Result<(), StructureError> {
        if r.next_float() < prob {
            self.place(state, x, y, z)?;
        }
        Ok(())
    }
    pub fn upper_sphere(
        &mut self,
        b: [i32; 6],
        state: BlockState,
        skip_air: bool,
    ) -> Result<(), StructureError> {
        finite_box(b)?;
        let [x0, y0, z0, x1, y1, z1] = b;
        let dx = x1.wrapping_sub(x0).wrapping_add(1) as f32;
        let dy = y1.wrapping_sub(y0).wrapping_add(1) as f32;
        let dz = z1.wrapping_sub(z0).wrapping_add(1) as f32;
        let cx = x0 as f32 + dx / 2.0;
        let cz = z0 as f32 + dz / 2.0;
        for y in y0..=y1 {
            let ny = y.wrapping_sub(y0) as f32 / dy;
            for x in x0..=x1 {
                let nx = (x as f32 - cx) / (dx * 0.5);
                for z in z0..=z1 {
                    let nz = (z as f32 - cz) / (dz * 0.5);
                    if (!skip_air || !self.get(x, y, z).is_air())
                        && nx * nx + ny * ny + nz * nz <= 1.05
                    {
                        self.place(state, x, y, z)?;
                    }
                }
            }
        }
        Ok(())
    }
    pub fn invalid_location(&mut self) -> Result<bool, StructureError> {
        let b = self.data.bounding_box;
        let x0 = b.min_x.wrapping_sub(1).max(self.clip.min_x);
        let y0 = b.min_y.wrapping_sub(1).max(self.clip.min_y);
        let z0 = b.min_z.wrapping_sub(1).max(self.clip.min_z);
        let x1 = b.max_x.wrapping_add(1).min(self.clip.max_x);
        let y1 = b.max_y.wrapping_add(1).min(self.clip.max_y);
        let z1 = b.max_z.wrapping_add(1).min(self.clip.max_z);
        finite_box([x0, y0, z0, x1, y1, z1])?;
        let p = BlockPos::new(
            x0.wrapping_add(x1) / 2,
            y0.wrapping_add(y1) / 2,
            z0.wrapping_add(z1) / 2,
        );
        let biome = self.authority.level().get_biome(&p);
        let access = self.authority.level().registry_access();
        let registry = access.lookup(&BIOME).ok_or_else(|| {
            StructureError::PlacementUnavailable("mineshaft biome registry unavailable".into())
        })?;
        let tag = TagKey::create(
            &BIOME,
            Identifier::with_default_namespace("mineshaft_blocking"),
        );
        let id = HolderId(registry.get_id(biome.value(registry)) as u32);
        if registry.get_tag_or_empty(&tag).contains(&id) {
            return Ok(true);
        }
        for x in x0..=x1 {
            for z in z0..=z1 {
                for y in [y0, y1] {
                    if liquid(
                        self.authority
                            .level()
                            .get_block_state(&BlockPos::new(x, y, z)),
                    ) {
                        return Ok(true);
                    }
                }
            }
        }
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in [z0, z1] {
                    if liquid(
                        self.authority
                            .level()
                            .get_block_state(&BlockPos::new(x, y, z)),
                    ) {
                        return Ok(true);
                    }
                }
            }
        }
        for z in z0..=z1 {
            for y in y0..=y1 {
                for x in [x0, x1] {
                    if liquid(
                        self.authority
                            .level()
                            .get_block_state(&BlockPos::new(x, y, z)),
                    ) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
    pub fn set_planks(&mut self, x: i32, y: i32, z: i32) -> Result<(), StructureError> {
        if self.interior(x, y, z) {
            let p = self.pos(x, y, z);
            let state = self.authority.level().get_block_state(&p);
            if !self.sturdy(p, state, Direction::Up, SupportType::Full)? {
                self.authority
                    .level()
                    .set_block(&p, planks(self.mine_type), 2);
            }
        }
        Ok(())
    }
    pub fn supporting_roof(&mut self, x0: i32, x1: i32, y: i32, z: i32) -> bool {
        (x0..=x1).all(|x| !self.get(x, y.wrapping_add(1), z).is_air())
    }
    pub fn sturdy_neighbors(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        required: i32,
    ) -> Result<bool, StructureError> {
        let p = self.pos(x, y, z);
        let mut count = 0;
        for direction in Direction::stream() {
            let neighbor = p.relative(&direction);
            if self.clip.is_inside(&neighbor) {
                let state = self.authority.level().get_block_state(&neighbor);
                if self.sturdy(neighbor, state, direction.get_opposite(), SupportType::Full)? {
                    count += 1;
                    if count >= required {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
    pub fn maybe_cobweb<R: RandomSource>(
        &mut self,
        r: &mut R,
        prob: f32,
        x: i32,
        y: i32,
        z: i32,
    ) -> Result<(), StructureError> {
        if self.interior(x, y, z) && r.next_float() < prob && self.sturdy_neighbors(x, y, z, 2)? {
            self.place(block("minecraft:cobweb"), x, y, z)?;
        }
        Ok(())
    }
    pub fn hanging_support(
        &mut self,
        p: BlockPos,
        state: BlockState,
    ) -> Result<bool, StructureError> {
        if state.is_in_tag("minecraft:unstable_bottom_center")
            || !self.sturdy(p, state, Direction::Down, SupportType::Center)?
        {
            return Ok(false);
        }
        // Complete pinned built-in membership, captured by the genuine Java instanceof query.
        Ok(
            !include_str!(
                "../../../../../tools/rivet-oracle/fixtures/structure-falling-blocks.txt"
            )
            .lines()
            .any(|name| name == state.block().name()),
        )
    }
}
