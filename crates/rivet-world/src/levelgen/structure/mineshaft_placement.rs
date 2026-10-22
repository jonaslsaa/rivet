//! Literal MineshaftPieces.postProcess algorithms. World/entity mutations are
//! delegated only through a capability-checked genuine placement authority.
use super::mineshaft::{MineshaftPiece, MineshaftPieceKind};
use super::piece_placement::{PiecePlacement, block, fence, planks, replaceable, wood};
use super::{BoundingBox, StructureError, StructurePlacementAuthority};
use crate::level::block_shape_context::SupportType;
use rivet_registry::{
    block_state_properties::BlockStateProperties as P,
    block_state_property::PropertyValue,
    core::{BlockPos, Direction},
};
use rivet_util::RandomSource;

#[cfg(test)]
#[path = "mineshaft_placement_fixture.rs"]
mod tests;

pub(super) fn post_process<R: RandomSource, A: StructurePlacementAuthority>(
    piece: &mut MineshaftPiece,
    authority: &mut A,
    r: &mut R,
    clip: BoundingBox,
) -> Result<(), StructureError> {
    authority.require_mineshaft_placement()?;
    let data = piece.data.clone();
    let b = data.bounding_box;
    let mut p = PiecePlacement {
        data: &data,
        mine_type: piece.mine_type,
        authority,
        clip,
    };
    if p.invalid_location()? {
        return Ok(());
    }
    let air = block("minecraft:cave_air");
    match &mut piece.kind {
        MineshaftPieceKind::Room { entrances } => {
            p.fill(
                [
                    b.min_x,
                    b.min_y.wrapping_add(1),
                    b.min_z,
                    b.max_x,
                    b.min_y.wrapping_add(3).min(b.max_y),
                    b.max_z,
                ],
                air,
                air,
                false,
            )?;
            for e in entrances {
                p.fill(
                    [
                        e.min_x,
                        e.max_y.wrapping_sub(2),
                        e.min_z,
                        e.max_x,
                        e.max_y,
                        e.max_z,
                    ],
                    air,
                    air,
                    false,
                )?;
            }
            p.upper_sphere(
                [
                    b.min_x,
                    b.min_y.wrapping_add(4),
                    b.min_z,
                    b.max_x,
                    b.max_y,
                    b.max_z,
                ],
                air,
                false,
            )?;
        }
        MineshaftPieceKind::Crossing { two_floored, .. } => {
            if *two_floored {
                p.fill(
                    [
                        b.min_x.wrapping_add(1),
                        b.min_y,
                        b.min_z,
                        b.max_x.wrapping_sub(1),
                        b.min_y.wrapping_add(2),
                        b.max_z,
                    ],
                    air,
                    air,
                    false,
                )?;
                p.fill(
                    [
                        b.min_x,
                        b.min_y,
                        b.min_z.wrapping_add(1),
                        b.max_x,
                        b.min_y.wrapping_add(2),
                        b.max_z.wrapping_sub(1),
                    ],
                    air,
                    air,
                    false,
                )?;
                p.fill(
                    [
                        b.min_x.wrapping_add(1),
                        b.max_y.wrapping_sub(2),
                        b.min_z,
                        b.max_x.wrapping_sub(1),
                        b.max_y,
                        b.max_z,
                    ],
                    air,
                    air,
                    false,
                )?;
                p.fill(
                    [
                        b.min_x,
                        b.max_y.wrapping_sub(2),
                        b.min_z.wrapping_add(1),
                        b.max_x,
                        b.max_y,
                        b.max_z.wrapping_sub(1),
                    ],
                    air,
                    air,
                    false,
                )?;
                p.fill(
                    [
                        b.min_x.wrapping_add(1),
                        b.min_y.wrapping_add(3),
                        b.min_z.wrapping_add(1),
                        b.max_x.wrapping_sub(1),
                        b.min_y.wrapping_add(3),
                        b.max_z.wrapping_sub(1),
                    ],
                    air,
                    air,
                    false,
                )?;
            } else {
                p.fill(
                    [
                        b.min_x.wrapping_add(1),
                        b.min_y,
                        b.min_z,
                        b.max_x.wrapping_sub(1),
                        b.max_y,
                        b.max_z,
                    ],
                    air,
                    air,
                    false,
                )?;
                p.fill(
                    [
                        b.min_x,
                        b.min_y,
                        b.min_z.wrapping_add(1),
                        b.max_x,
                        b.max_y,
                        b.max_z.wrapping_sub(1),
                    ],
                    air,
                    air,
                    false,
                )?;
            }
            for (x, z) in [
                (b.min_x.wrapping_add(1), b.min_z.wrapping_add(1)),
                (b.min_x.wrapping_add(1), b.max_z.wrapping_sub(1)),
                (b.max_x.wrapping_sub(1), b.min_z.wrapping_add(1)),
                (b.max_x.wrapping_sub(1), b.max_z.wrapping_sub(1)),
            ] {
                if !p.get(x, b.max_y.wrapping_add(1), z).is_air() {
                    let planks = planks(p.mine_type);
                    p.fill([x, b.min_y, z, x, b.max_y, z], planks, air, false)?;
                }
            }
            for x in b.min_x..=b.max_x {
                for z in b.min_z..=b.max_z {
                    p.set_planks(x, b.min_y.wrapping_sub(1), z)?;
                }
            }
        }
        MineshaftPieceKind::Stairs => {
            p.fill([0, 5, 0, 2, 7, 1], air, air, false)?;
            p.fill([0, 0, 7, 2, 2, 8], air, air, false)?;
            for i in 0..5 {
                p.fill(
                    [0, 5 - i - i32::from(i < 4), 2 + i, 2, 7 - i, 2 + i],
                    air,
                    air,
                    false,
                )?;
            }
        }
        MineshaftPieceKind::Corridor {
            has_rails,
            spider_corridor,
            has_placed_spider,
            num_sections,
        } => {
            let length = num_sections.wrapping_mul(5).wrapping_sub(1);
            p.fill([0, 0, 0, 2, 1, length], air, air, false)?;
            p.maybe_fill(r, 0.8, [0, 2, 0, 2, 2, length], air, air, false, false)?;
            if *spider_corridor {
                p.maybe_fill(
                    r,
                    0.6,
                    [0, 0, 0, 2, 1, length],
                    block("minecraft:cobweb"),
                    air,
                    false,
                    true,
                )?;
            }
            for section in 0..*num_sections {
                let z = 2i32.wrapping_add(section.wrapping_mul(5));
                place_support(&mut p, r, z)?;
                for (prob, x, dz) in [
                    (0.1, 0, -1),
                    (0.1, 2, -1),
                    (0.1, 0, 1),
                    (0.1, 2, 1),
                    (0.05, 0, -2),
                    (0.05, 2, -2),
                    (0.05, 0, 2),
                    (0.05, 2, 2),
                ] {
                    p.maybe_cobweb(r, prob, x, 2, z.wrapping_add(dz))?;
                }
                if r.next_int_bound(100) == 0 {
                    create_chest(&mut p, r, 2, 0, z.wrapping_sub(1))?;
                }
                if r.next_int_bound(100) == 0 {
                    create_chest(&mut p, r, 0, 0, z.wrapping_add(1))?;
                }
                if *spider_corridor && !*has_placed_spider {
                    let new_z = z.wrapping_sub(1).wrapping_add(r.next_int_bound(3));
                    let pos = p.pos(1, 0, new_z);
                    if p.clip.is_inside(&pos) && p.interior(1, 0, new_z) {
                        *has_placed_spider = true;
                        // Paper's Craft snapshot path consumes no FEATURE RNG.
                        p.authority.place_cave_spider_spawner(pos, 2)?;
                    }
                }
            }
            for x in 0..=2 {
                for z in 0..=length {
                    p.set_planks(x, -1, z)?;
                }
            }
            double_support(&mut p, 0, -1, 2)?;
            if *num_sections > 1 {
                double_support(&mut p, 0, -1, length.wrapping_sub(2))?;
            }
            if *has_rails {
                let rail = block("minecraft:rail")
                    .set_value(P::RAIL_SHAPE, PropertyValue::Enum("north_south"))
                    .expect("rail shape");
                for z in 0..=length {
                    let floor = p.get(1, -1, z);
                    if !floor.is_air() && floor.solid_render() {
                        let probability = if p.interior(1, 0, z) { 0.7 } else { 0.9 };
                        p.maybe_block(r, probability, 1, 0, z, rail)?;
                    }
                }
            }
        }
    }
    Ok(())
}
fn create_chest<R: RandomSource, A: StructurePlacementAuthority>(
    p: &mut PiecePlacement<'_, A>,
    r: &mut R,
    x: i32,
    y: i32,
    z: i32,
) -> Result<bool, StructureError> {
    let pos = p.pos(x, y, z);
    if p.clip.is_inside(&pos)
        && p.authority.level().get_block_state(&pos).is_air()
        && !p.authority.level().get_block_state(&pos.below()).is_air()
    {
        let shape = if r.next_boolean() {
            "north_south"
        } else {
            "east_west"
        };
        let rail = block("minecraft:rail")
            .set_value(P::RAIL_SHAPE, PropertyValue::Enum(shape))
            .expect("rail shape");
        p.place(rail, x, y, z)?;
        // Authority performs genuine EntityType.create. Only a created entity
        // consumes nextLong and receives initial position, loot and admission.
        p.authority
            .place_loot_minecart(pos, "minecraft:chests/abandoned_mineshaft", r)?;
        Ok(true)
    } else {
        Ok(false)
    }
}
fn place_support<R: RandomSource, A: StructurePlacementAuthority>(
    p: &mut PiecePlacement<'_, A>,
    r: &mut R,
    z: i32,
) -> Result<(), StructureError> {
    if !p.supporting_roof(0, 2, 2, z) {
        return Ok(());
    }
    let planks = planks(p.mine_type);
    let fence = fence(p.mine_type);
    let air = block("minecraft:cave_air");
    p.fill(
        [0, 0, z, 0, 1, z],
        fence.set_value(P::WEST, true).expect("fence west"),
        air,
        false,
    )?;
    p.fill(
        [2, 0, z, 2, 1, z],
        fence.set_value(P::EAST, true).expect("fence east"),
        air,
        false,
    )?;
    if r.next_int_bound(4) == 0 {
        p.fill([0, 2, z, 0, 2, z], planks, air, false)?;
        p.fill([2, 2, z, 2, 2, z], planks, air, false)?;
    } else {
        p.fill([0, 2, z, 2, 2, z], planks, air, false)?;
        let torch = block("minecraft:wall_torch");
        p.maybe_block(
            r,
            0.05,
            1,
            2,
            z.wrapping_sub(1),
            torch
                .set_value(P::HORIZONTAL_FACING, Direction::South)
                .expect("torch facing"),
        )?;
        p.maybe_block(
            r,
            0.05,
            1,
            2,
            z.wrapping_add(1),
            torch
                .set_value(P::HORIZONTAL_FACING, Direction::North)
                .expect("torch facing"),
        )?;
    }
    Ok(())
}
fn double_support<A: StructurePlacementAuthority>(
    p: &mut PiecePlacement<'_, A>,
    x: i32,
    y: i32,
    z: i32,
) -> Result<(), StructureError> {
    let plank = planks(p.mine_type).block();
    if p.get(x, y, z).block() == plank {
        pillar_or_chain(p, x, y, z)?;
    }
    if p.get(x.wrapping_add(2), y, z).block() == plank {
        pillar_or_chain(p, x.wrapping_add(2), y, z)?;
    }
    Ok(())
}
fn fill_column<A: StructurePlacementAuthority>(
    p: &mut PiecePlacement<'_, A>,
    pos: BlockPos,
    state: rivet_registry::block_state::BlockState,
    bottom: i32,
    top: i32,
) {
    for y in bottom..top {
        p.authority
            .level()
            .set_block(&BlockPos::new(pos.get_x(), y, pos.get_z()), state, 2);
    }
}
fn pillar_or_chain<A: StructurePlacementAuthority>(
    p: &mut PiecePlacement<'_, A>,
    x: i32,
    y: i32,
    z: i32,
) -> Result<(), StructureError> {
    let pos = p.pos(x, y, z);
    if !p.clip.is_inside(&pos) {
        return Ok(());
    }
    let world_y = pos.get_y();
    let mut below = true;
    let mut above = true;
    let mut distance = 1i32;
    while below || above {
        if below {
            let q = BlockPos::new(pos.get_x(), world_y.wrapping_sub(distance), pos.get_z());
            let state = p.authority.level().get_block_state(&q);
            let empty = replaceable(state) && state.block().name() != "minecraft:lava";
            if !empty && p.sturdy(q, state, Direction::Up, SupportType::Full)? {
                let material = wood(p.mine_type);
                fill_column(
                    p,
                    q,
                    material,
                    world_y.wrapping_sub(distance).wrapping_add(1),
                    world_y,
                );
                return Ok(());
            }
            below = distance <= 20
                && empty
                && q.get_y() > p.authority.level().get_min_y().wrapping_add(1);
        }
        if above {
            let q = BlockPos::new(pos.get_x(), world_y.wrapping_add(distance), pos.get_z());
            let state = p.authority.level().get_block_state(&q);
            let empty = replaceable(state);
            if !empty && p.hanging_support(q, state)? {
                p.authority.level().set_block(
                    &BlockPos::new(pos.get_x(), world_y.wrapping_add(1), pos.get_z()),
                    fence(p.mine_type),
                    2,
                );
                fill_column(
                    p,
                    q,
                    block("minecraft:iron_chain"),
                    world_y.wrapping_add(2),
                    world_y.wrapping_add(distance),
                );
                return Ok(());
            }
            above = distance <= 50 && empty && q.get_y() < p.authority.level().get_max_y();
        }
        distance = distance.wrapping_add(1);
    }
    Ok(())
}
