//! Port of `net.minecraft.world.level.pathfinder.Path` in Paper 26.2.

use crate::world::entity::ai::goal::context::{BlockPos, Vec3};
use crate::world::level::pathfinder::node::{Node, Target};

/// Port of `Path.DebugData` in Paper 26.2.
#[derive(Debug, Clone)]
pub struct PathDebugData {
    pub open_set: Vec<Node>,
    pub closed_set: Vec<Node>,
    pub target_nodes: Vec<Target>,
}

/// Port of `net.minecraft.world.level.pathfinder.Path` in Paper 26.2.
#[derive(Debug, Clone)]
pub struct Path {
    nodes: Vec<Node>,
    debug_data: Option<PathDebugData>,
    next_node_index: usize,
    target: BlockPos,
    dist_to_target: f32,
    reached: bool,
}

impl Path {
    #[must_use]
    pub fn new(nodes: Vec<Node>, target: BlockPos, reached: bool) -> Self {
        let dist_to_target = nodes
            .last()
            .map_or(f32::MAX, |last| last.distance_manhattan_pos(target));
        Self {
            nodes,
            debug_data: None,
            next_node_index: 0,
            target,
            dist_to_target,
            reached,
        }
    }

    pub fn advance(&mut self) {
        self.next_node_index += 1;
    }

    #[must_use]
    pub const fn not_started(&self) -> bool {
        self.next_node_index == 0
    }

    #[must_use]
    pub fn is_done(&self) -> bool {
        self.next_node_index >= self.nodes.len()
    }

    #[must_use]
    pub fn get_end_node(&self) -> Option<&Node> {
        self.nodes.last()
    }

    #[must_use]
    pub fn get_node(&self, index: usize) -> &Node {
        &self.nodes[index]
    }

    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Mirrors `Path.truncateNodes(int index)`.
    pub fn truncate_nodes(&mut self, index: usize) {
        if self.nodes.len() > index {
            self.nodes.truncate(index);
        }
    }

    /// Mirrors `Path.replaceNode(int index, Node replaceWith)`.
    pub fn replace_node(&mut self, index: usize, replace_with: Node) {
        self.nodes[index] = replace_with;
    }

    #[must_use]
    pub fn get_node_count(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub const fn get_next_node_index(&self) -> usize {
        self.next_node_index
    }

    pub fn set_next_node_index(&mut self, next_node_index: usize) {
        self.next_node_index = next_node_index;
    }

    /// Mirrors `Path.getEntityPosAtNode(Entity entity, int index)`.
    ///
    /// `bb_width` is `entity.getBbWidth()`.
    #[must_use]
    pub fn get_entity_pos_at_node(&self, bb_width: f32, index: usize) -> Vec3 {
        let node = &self.nodes[index];
        let x = node.x as f64 + ((bb_width + 1.0) as i32) as f64 * 0.5;
        let y = node.y as f64;
        let z = node.z as f64 + ((bb_width + 1.0) as i32) as f64 * 0.5;
        Vec3::new(x, y, z)
    }

    #[must_use]
    pub fn get_node_pos(&self, index: usize) -> BlockPos {
        self.nodes[index].as_block_pos()
    }

    #[must_use]
    pub fn get_next_entity_pos(&self, bb_width: f32) -> Vec3 {
        self.get_entity_pos_at_node(bb_width, self.next_node_index)
    }

    #[must_use]
    pub fn get_next_node_pos(&self) -> BlockPos {
        self.nodes[self.next_node_index].as_block_pos()
    }

    #[must_use]
    pub fn get_next_node(&self) -> &Node {
        &self.nodes[self.next_node_index]
    }

    #[must_use]
    pub fn get_previous_node(&self) -> Option<&Node> {
        if self.next_node_index > 0 {
            Some(&self.nodes[self.next_node_index - 1])
        } else {
            None
        }
    }

    /// Mirrors `Path.sameAs(@Nullable Path path)`: compares only the node lists
    /// by coordinates.
    #[must_use]
    pub fn same_as(&self, other: Option<&Self>) -> bool {
        let Some(other) = other else {
            return false;
        };
        if other.nodes.len() != self.nodes.len() {
            return false;
        }
        self.nodes
            .iter()
            .zip(other.nodes.iter())
            .all(|(a, b)| a.x == b.x && a.y == b.y && a.z == b.z)
    }

    #[must_use]
    pub const fn can_reach(&self) -> bool {
        self.reached
    }

    #[must_use]
    pub const fn get_dist_to_target(&self) -> f32 {
        self.dist_to_target
    }

    #[must_use]
    pub const fn get_target(&self) -> BlockPos {
        self.target
    }

    pub fn set_debug(&mut self, open_set: Vec<Node>, closed_set: Vec<Node>, targets: Vec<Target>) {
        self.debug_data = Some(PathDebugData {
            open_set,
            closed_set,
            target_nodes: targets,
        });
    }

    #[must_use]
    pub fn debug_data(&self) -> Option<&PathDebugData> {
        self.debug_data.as_ref()
    }

    /// Mirrors `Path.copy()`.
    #[must_use]
    pub fn copy(&self) -> Self {
        self.clone()
    }
}
