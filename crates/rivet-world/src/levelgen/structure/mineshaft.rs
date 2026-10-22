//! Pinned MineshaftStructure start graph. Terrain-writing postProcess is not
//! implemented by this module; every generated piece retains its full native data.
use super::structure_body::MineshaftKind;
use super::{BoundingBox, PiecesContainer, StructureError, StructurePiece, StructurePieceData};
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::{compound_tag::CompoundTag, list_tag::ListTag, tag::Tag};
use rivet_registry::core::{BlockPos, ChunkPos, Direction};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::RandomSource;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MineshaftPiece {
    pub data: StructurePieceData,
    pub mine_type: MineshaftKind,
    pub kind: MineshaftPieceKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MineshaftPieceKind {
    Room {
        entrances: Vec<BoundingBox>,
    },
    Corridor {
        has_rails: bool,
        spider_corridor: bool,
        has_placed_spider: bool,
        num_sections: i32,
    },
    Crossing {
        direction: Direction,
        two_floored: bool,
    },
    Stairs,
}
fn add(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}
fn span(min: i32, max: i32) -> i32 {
    add(max.wrapping_sub(min), 1)
}
fn moved(b: BoundingBox, x: i32, y: i32, z: i32) -> BoundingBox {
    // BoundingBox.move mutates fields without constructor normalization, even
    // when wrapping coordinates invert an axis.
    BoundingBox {
        min_x: add(b.min_x, x),
        min_y: add(b.min_y, y),
        min_z: add(b.min_z, z),
        max_x: add(b.max_x, x),
        max_y: add(b.max_y, y),
        max_z: add(b.max_z, z),
    }
}
impl MineshaftPiece {
    pub fn load(tag: &CompoundTag, id: &str) -> Result<Self, StructureError> {
        let kind = match id {
            "msroom" => {
                // BoundingBox.CODEC.listOf fails the entire field on a bad entry.
                let entrances = tag
                    .get_list_or_empty("Entrances")
                    .list
                    .iter()
                    .map(|v| {
                        let values = NbtOps::instance().get_int_stream(v).result().cloned()?;
                        let a: [i32; 6] = values.as_slice().try_into().ok()?;
                        Some(BoundingBox::new(a))
                    })
                    .collect::<Option<Vec<_>>>()
                    .unwrap_or_default();
                MineshaftPieceKind::Room { entrances }
            }
            "mscorridor" => MineshaftPieceKind::Corridor {
                has_rails: tag.get_boolean_or("hr", false),
                spider_corridor: tag.get_boolean_or("sc", false),
                has_placed_spider: tag.get_boolean_or("hps", false),
                num_sections: tag.get_int_or("Num", 0),
            },
            "mscrossing" => MineshaftPieceKind::Crossing {
                direction: Direction::from_2d_data_value(i32::from(tag.get_byte("D").unwrap_or(0))),
                two_floored: tag.get_boolean_or("tf", false),
            },
            "msstairs" => MineshaftPieceKind::Stairs,
            _ => return Err(StructureError::UnsupportedPiece(id.into())),
        };
        Ok(Self {
            data: StructurePieceData::load(tag)?,
            mine_type: if tag.get_int_or("MST", 0) == 1 {
                MineshaftKind::Mesa
            } else {
                MineshaftKind::Normal
            },
            kind,
        })
    }
    pub fn create_tag(&self) -> CompoundTag {
        let id = match self.kind {
            MineshaftPieceKind::Room { .. } => "minecraft:msroom",
            MineshaftPieceKind::Corridor { .. } => "minecraft:mscorridor",
            MineshaftPieceKind::Crossing { .. } => "minecraft:mscrossing",
            MineshaftPieceKind::Stairs => "minecraft:msstairs",
        };
        let mut tag = self.data.create_tag(id);
        tag.put_int(
            "MST",
            if self.mine_type == MineshaftKind::Mesa {
                1
            } else {
                0
            },
        );
        match &self.kind {
            MineshaftPieceKind::Room { entrances } => {
                tag.put(
                    "Entrances".into(),
                    Tag::List(ListTag::with_list(
                        entrances
                            .iter()
                            .map(|b| {
                                Tag::IntArray(rivet_nbt::int_array_tag::IntArrayTag::new(
                                    b.to_array().to_vec(),
                                ))
                            })
                            .collect(),
                    )),
                );
            }
            MineshaftPieceKind::Corridor {
                has_rails,
                spider_corridor,
                has_placed_spider,
                num_sections,
            } => {
                tag.put_boolean("hr", *has_rails);
                tag.put_boolean("sc", *spider_corridor);
                tag.put_boolean("hps", *has_placed_spider);
                tag.put_int("Num", *num_sections);
            }
            MineshaftPieceKind::Crossing {
                direction,
                two_floored,
            } => {
                tag.put_boolean("tf", *two_floored);
                tag.put_byte("D", direction.get_2d_data_value() as i8);
            }
            MineshaftPieceKind::Stairs => {}
        }
        tag
    }
    pub fn move_by(&mut self, dx: i32, dy: i32, dz: i32) {
        self.data.bounding_box = moved(self.data.bounding_box, dx, dy, dz);
        if let MineshaftPieceKind::Room { entrances } = &mut self.kind {
            for entrance in entrances {
                *entrance = moved(*entrance, dx, dy, dz);
            }
        }
    }
}

/// Literal eager graph construction and source height adjustment. The surface
/// query is invoked only for MESA, at the actual completed graph center.
/// Returns both the ordered pieces and the generation-point vertical offset.
pub fn generate_pieces<R: RandomSource>(
    mine_type: MineshaftKind,
    chunk: ChunkPos,
    random: &mut R,
    sea_level: i32,
    min_y: i32,
    mut mesa_surface: impl FnMut(BlockPos) -> i32,
) -> (PiecesContainer, i32) {
    random.next_double(); // MineshaftStructure.findGenerationPoint's discarded chance draw.
    let west = add(chunk.get_min_block_x(), 2);
    let north = add(chunk.get_min_block_z(), 2);
    let room = BoundingBox::new([
        west,
        50,
        north,
        add(add(west, 7), random.next_int_bound(6)),
        add(54, random.next_int_bound(6)),
        add(add(north, 7), random.next_int_bound(6)),
    ]);
    let mut graph = Graph {
        pieces: vec![MineshaftPiece {
            data: StructurePieceData {
                bounding_box: room,
                gen_depth: 0,
                orientation: None,
            },
            mine_type,
            kind: MineshaftPieceKind::Room {
                entrances: Vec::new(),
            },
        }],
    };
    graph.children(0, random);
    let mut bounds = graph.pieces[0].data.bounding_box;
    for piece in &graph.pieces[1..] {
        bounds.encapsulate(piece.data.bounding_box);
    }
    let dy = if mine_type == MineshaftKind::Mesa {
        let center = bounds.center();
        let surface = mesa_surface(center);
        let target = if surface <= sea_level {
            sea_level
        } else {
            add(
                sea_level,
                random.next_int_bound(add(surface.wrapping_sub(sea_level), 1)),
            )
        };
        target.wrapping_sub(center.get_y())
    } else {
        let limit = sea_level.wrapping_sub(10);
        let mut top = add(add(span(bounds.min_y, bounds.max_y), min_y), 1);
        if top < limit {
            top = add(top, random.next_int_bound(limit.wrapping_sub(top)));
        }
        top.wrapping_sub(bounds.max_y)
    };
    for piece in &mut graph.pieces {
        piece.move_by(0, dy, 0);
    }
    (
        PiecesContainer::new(
            graph
                .pieces
                .into_iter()
                .map(StructurePiece::Mineshaft)
                .collect(),
        ),
        dy,
    )
}
struct Graph {
    pieces: Vec<MineshaftPiece>,
}
impl Graph {
    fn collision(&self, b: BoundingBox) -> bool {
        self.pieces
            .iter()
            .any(|p| p.data.bounding_box.intersects(b))
    }
    fn child<R: RandomSource>(
        &mut self,
        random: &mut R,
        x: i32,
        y: i32,
        z: i32,
        direction: Direction,
        depth: i32,
    ) -> Option<BoundingBox> {
        let root = self.pieces[0].data.bounding_box;
        if depth > 8
            || x.wrapping_sub(root.min_x).wrapping_abs() > 80
            || z.wrapping_sub(root.min_z).wrapping_abs() > 80
        {
            return None;
        }
        let select = random.next_int_bound(100);
        let (bounds, kind, orientation) = if select >= 80 {
            let height = if random.next_int_bound(4) == 0 { 6 } else { 2 };
            let offsets = match direction {
                Direction::South => [-1, 0, 0, 3, height, 4],
                Direction::West => [-4, 0, -1, 0, height, 3],
                Direction::East => [0, 0, -1, 4, height, 3],
                _ => [-1, 0, -4, 3, height, 0],
            };
            let b = moved(BoundingBox::new(offsets), x, y, z);
            if self.collision(b) {
                return None;
            }
            (
                b,
                MineshaftPieceKind::Crossing {
                    direction,
                    two_floored: span(b.min_y, b.max_y) > 3,
                },
                None,
            )
        } else if select >= 70 {
            let offsets = match direction {
                Direction::South => [0, -5, 0, 2, 2, 8],
                Direction::West => [-8, -5, 0, 0, 2, 2],
                Direction::East => [0, -5, 0, 8, 2, 2],
                _ => [0, -5, -8, 2, 2, 0],
            };
            let b = moved(BoundingBox::new(offsets), x, y, z);
            if self.collision(b) {
                return None;
            }
            (b, MineshaftPieceKind::Stairs, Some(direction))
        } else {
            let mut sections = add(random.next_int_bound(3), 2);
            let b = loop {
                if sections <= 0 {
                    return None;
                }
                let length = sections.wrapping_mul(5);
                let offsets = match direction {
                    Direction::South => [0, 0, 0, 2, 2, length - 1],
                    Direction::West => [-(length - 1), 0, 0, 0, 2, 2],
                    Direction::East => [0, 0, 0, length - 1, 2, 2],
                    _ => [0, 0, -(length - 1), 2, 2, 0],
                };
                let b = moved(BoundingBox::new(offsets), x, y, z);
                if !self.collision(b) {
                    break b;
                }
                sections -= 1;
            };
            let has_rails = random.next_int_bound(3) == 0;
            let spider_corridor = !has_rails && random.next_int_bound(23) == 0;
            let num_sections = if matches!(direction, Direction::North | Direction::South) {
                span(b.min_z, b.max_z) / 5
            } else {
                span(b.min_x, b.max_x) / 5
            };
            (
                b,
                MineshaftPieceKind::Corridor {
                    has_rails,
                    spider_corridor,
                    has_placed_spider: false,
                    num_sections,
                },
                Some(direction),
            )
        };
        let index = self.pieces.len();
        self.pieces.push(MineshaftPiece {
            data: StructurePieceData {
                bounding_box: bounds,
                gen_depth: add(depth, 1),
                orientation,
            },
            mine_type: self.pieces[0].mine_type,
            kind,
        });
        self.children(index, random);
        Some(bounds)
    }
    fn children<R: RandomSource>(&mut self, index: usize, random: &mut R) {
        let piece = self.pieces[index].clone();
        let b = piece.data.bounding_box;
        let depth = piece.data.gen_depth;
        match piece.kind {
            MineshaftPieceKind::Room { .. } => {
                let height = (span(b.min_y, b.max_y) - 4).max(1);
                for direction in [
                    Direction::North,
                    Direction::South,
                    Direction::West,
                    Direction::East,
                ] {
                    let width = if matches!(direction, Direction::North | Direction::South) {
                        span(b.min_x, b.max_x)
                    } else {
                        span(b.min_z, b.max_z)
                    };
                    let mut pos = 0;
                    while pos < width {
                        pos = add(pos, random.next_int_bound(width));
                        if add(pos, 3) > width {
                            break;
                        }
                        let (x, z) = match direction {
                            Direction::North => (add(b.min_x, pos), add(b.min_z, -1)),
                            Direction::South => (add(b.min_x, pos), add(b.max_z, 1)),
                            Direction::West => (add(b.min_x, -1), add(b.min_z, pos)),
                            _ => (add(b.max_x, 1), add(b.min_z, pos)),
                        };
                        let y = add(add(b.min_y, random.next_int_bound(height)), 1);
                        if let Some(c) = self.child(random, x, y, z, direction, depth) {
                            let entrance = match direction {
                                Direction::North => {
                                    [c.min_x, c.min_y, b.min_z, c.max_x, c.max_y, add(b.min_z, 1)]
                                }
                                Direction::South => [
                                    c.min_x,
                                    c.min_y,
                                    add(b.max_z, -1),
                                    c.max_x,
                                    c.max_y,
                                    b.max_z,
                                ],
                                Direction::West => {
                                    [b.min_x, c.min_y, c.min_z, add(b.min_x, 1), c.max_y, c.max_z]
                                }
                                _ => [
                                    add(b.max_x, -1),
                                    c.min_y,
                                    c.min_z,
                                    b.max_x,
                                    c.max_y,
                                    c.max_z,
                                ],
                            };
                            if let MineshaftPieceKind::Room { entrances } =
                                &mut self.pieces[index].kind
                            {
                                entrances.push(BoundingBox::new(entrance));
                            }
                        }
                        pos = add(pos, 4);
                    }
                }
            }
            MineshaftPieceKind::Corridor { .. } => {
                let end = random.next_int_bound(4);
                if let Some(dir) = piece.data.orientation {
                    let (x, z, target) = match (dir, end) {
                        (Direction::North, 0 | 1) => (b.min_x, add(b.min_z, -1), dir),
                        (Direction::South, 0 | 1) => (b.min_x, add(b.max_z, 1), dir),
                        (Direction::West, 0 | 1) => (add(b.min_x, -1), b.min_z, dir),
                        (Direction::East, 0 | 1) => (add(b.max_x, 1), b.min_z, dir),
                        (Direction::North, 2) => (add(b.min_x, -1), b.min_z, Direction::West),
                        (Direction::North, _) => (add(b.max_x, 1), b.min_z, Direction::East),
                        (Direction::South, 2) => {
                            (add(b.min_x, -1), add(b.max_z, -3), Direction::West)
                        }
                        (Direction::South, _) => {
                            (add(b.max_x, 1), add(b.max_z, -3), Direction::East)
                        }
                        (Direction::West, 2) => (b.min_x, add(b.min_z, -1), Direction::North),
                        (Direction::West, _) => (b.min_x, add(b.max_z, 1), Direction::South),
                        (Direction::East, 2) => {
                            (add(b.max_x, -3), add(b.min_z, -1), Direction::North)
                        }
                        (Direction::East, _) => {
                            (add(b.max_x, -3), add(b.max_z, 1), Direction::South)
                        }
                        _ => unreachable!("horizontal corridor orientation"),
                    };
                    let y = add(add(b.min_y, -1), random.next_int_bound(3));
                    self.child(random, x, y, z, target, depth);
                }
                if depth < 8 {
                    if !matches!(
                        piece.data.orientation,
                        Some(Direction::North | Direction::South)
                    ) {
                        let mut x = add(b.min_x, 3);
                        while add(x, 3) <= b.max_x {
                            match random.next_int_bound(5) {
                                0 => {
                                    self.child(
                                        random,
                                        x,
                                        b.min_y,
                                        add(b.min_z, -1),
                                        Direction::North,
                                        add(depth, 1),
                                    );
                                }
                                1 => {
                                    self.child(
                                        random,
                                        x,
                                        b.min_y,
                                        add(b.max_z, 1),
                                        Direction::South,
                                        add(depth, 1),
                                    );
                                }
                                _ => {}
                            }
                            x = add(x, 5);
                        }
                    } else {
                        let mut z = add(b.min_z, 3);
                        while add(z, 3) <= b.max_z {
                            match random.next_int_bound(5) {
                                0 => {
                                    self.child(
                                        random,
                                        add(b.min_x, -1),
                                        b.min_y,
                                        z,
                                        Direction::West,
                                        add(depth, 1),
                                    );
                                }
                                1 => {
                                    self.child(
                                        random,
                                        add(b.max_x, 1),
                                        b.min_y,
                                        z,
                                        Direction::East,
                                        add(depth, 1),
                                    );
                                }
                                _ => {}
                            }
                            z = add(z, 5);
                        }
                    }
                }
            }
            MineshaftPieceKind::Crossing {
                direction,
                two_floored,
            } => {
                let directions = match direction {
                    Direction::South => [Direction::South, Direction::West, Direction::East],
                    Direction::West => [Direction::North, Direction::South, Direction::West],
                    Direction::East => [Direction::North, Direction::South, Direction::East],
                    _ => [Direction::North, Direction::West, Direction::East],
                };
                for d in directions {
                    self.cross_child(random, b, b.min_y, d, depth);
                }
                if two_floored {
                    for d in [
                        Direction::North,
                        Direction::West,
                        Direction::East,
                        Direction::South,
                    ] {
                        if random.next_boolean() {
                            self.cross_child(random, b, add(b.min_y, 4), d, depth);
                        }
                    }
                }
            }
            MineshaftPieceKind::Stairs => {
                if let Some(d) = piece.data.orientation {
                    let (x, z) = match d {
                        Direction::South => (b.min_x, add(b.max_z, 1)),
                        Direction::West => (add(b.min_x, -1), b.min_z),
                        Direction::East => (add(b.max_x, 1), b.min_z),
                        _ => (b.min_x, add(b.min_z, -1)),
                    };
                    self.child(random, x, b.min_y, z, d, depth);
                }
            }
        }
    }
    fn cross_child<R: RandomSource>(
        &mut self,
        random: &mut R,
        b: BoundingBox,
        y: i32,
        d: Direction,
        depth: i32,
    ) {
        let (x, z) = match d {
            Direction::North => (add(b.min_x, 1), add(b.min_z, -1)),
            Direction::South => (add(b.min_x, 1), add(b.max_z, 1)),
            Direction::West => (add(b.min_x, -1), add(b.min_z, 1)),
            _ => (add(b.max_x, 1), add(b.min_z, 1)),
        };
        self.child(random, x, y, z, d, depth);
    }
}

#[cfg(test)]
#[path = "mineshaft_test.rs"]
mod tests;
