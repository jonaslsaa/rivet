//! Port of `net.minecraft.world.level.pathfinder.Node`, `Target`, and `BinaryHeap`.
//!
//! Per `OWNERSHIP.md`, nodes live in a tick-local [`NodeArena`] and are referenced
//! by plain `usize` indices (`came_from: Option<usize>`, heap entries `usize`).

use indexmap::IndexMap;
use rivet_util::mth;

use crate::ai::goal::context::{BlockPos, Vec3};
use crate::ai::navigation::path_type::PathType;

/// Port of `net.minecraft.world.level.pathfinder.Node` in Paper 26.2.
#[derive(Debug, Clone)]
pub struct Node {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub hash: i32,
    pub heap_idx: i32,
    pub g: f32,
    pub h: f32,
    pub f: f32,
    pub came_from: Option<usize>,
    pub closed: bool,
    pub walked_distance: f32,
    pub cost_malus: f32,
    pub path_type: PathType,
}

impl Node {
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self {
            x,
            y,
            z,
            hash: Self::create_hash(x, y, z),
            heap_idx: -1,
            g: 0.0,
            h: 0.0,
            f: 0.0,
            came_from: None,
            closed: false,
            walked_distance: 0.0,
            cost_malus: 0.0,
            path_type: PathType::Blocked,
        }
    }

    /// Exact port of `Node.createHash(int x, int y, int z)` in Paper 26.2:
    /// ```java
    /// return y & 0xFF
    ///     | (x & 32767) << 8
    ///     | (z & 32767) << 24
    ///     | (x < 0 ? Integer.MIN_VALUE : 0)
    ///     | (z < 0 ? 32768 : 0);
    /// ```
    #[must_use]
    pub const fn create_hash(x: i32, y: i32, z: i32) -> i32 {
        (y & 0xFF)
            | ((x & 32767) << 8)
            | ((z & 32767) << 24)
            | if x < 0 { i32::MIN } else { 0 }
            | if z < 0 { 32768 } else { 0 }
    }

    /// Mirrors `Node.cloneAndMove(int x, int y, int z)`.
    #[must_use]
    pub fn clone_and_move(&self, x: i32, y: i32, z: i32) -> Self {
        let mut node = Self::new(x, y, z);
        node.heap_idx = self.heap_idx;
        node.g = self.g;
        node.h = self.h;
        node.f = self.f;
        node.came_from = self.came_from;
        node.closed = self.closed;
        node.walked_distance = self.walked_distance;
        node.cost_malus = self.cost_malus;
        node.path_type = self.path_type;
        node
    }

    #[must_use]
    pub fn distance_to(&self, other: &Self) -> f32 {
        let xd = (other.x.wrapping_sub(self.x)) as f32;
        let yd = (other.y.wrapping_sub(self.y)) as f32;
        let zd = (other.z.wrapping_sub(self.z)) as f32;
        mth::sqrt(xd * xd + yd * yd + zd * zd)
    }

    #[must_use]
    pub fn distance_to_xz(&self, other: &Self) -> f32 {
        let xd = (other.x.wrapping_sub(self.x)) as f32;
        let zd = (other.z.wrapping_sub(self.z)) as f32;
        mth::sqrt(xd * xd + zd * zd)
    }

    #[must_use]
    pub fn distance_to_pos(&self, pos: BlockPos) -> f32 {
        let xd = (pos.x.wrapping_sub(self.x)) as f32;
        let yd = (pos.y.wrapping_sub(self.y)) as f32;
        let zd = (pos.z.wrapping_sub(self.z)) as f32;
        mth::sqrt(xd * xd + yd * yd + zd * zd)
    }

    #[must_use]
    pub fn distance_to_sqr(&self, other: &Self) -> f32 {
        let xd = (other.x.wrapping_sub(self.x)) as f32;
        let yd = (other.y.wrapping_sub(self.y)) as f32;
        let zd = (other.z.wrapping_sub(self.z)) as f32;
        xd * xd + yd * yd + zd * zd
    }

    #[must_use]
    pub fn distance_to_sqr_pos(&self, pos: BlockPos) -> f32 {
        let xd = (pos.x.wrapping_sub(self.x)) as f32;
        let yd = (pos.y.wrapping_sub(self.y)) as f32;
        let zd = (pos.z.wrapping_sub(self.z)) as f32;
        xd * xd + yd * yd + zd * zd
    }

    #[must_use]
    pub fn distance_manhattan(&self, other: &Self) -> f32 {
        let f = (other.x.wrapping_sub(self.x)).wrapping_abs() as f32;
        let f1 = (other.y.wrapping_sub(self.y)).wrapping_abs() as f32;
        let f2 = (other.z.wrapping_sub(self.z)).wrapping_abs() as f32;
        f + f1 + f2
    }

    #[must_use]
    pub fn distance_manhattan_pos(&self, pos: BlockPos) -> f32 {
        let f = (pos.x.wrapping_sub(self.x)).wrapping_abs() as f32;
        let f1 = (pos.y.wrapping_sub(self.y)).wrapping_abs() as f32;
        let f2 = (pos.z.wrapping_sub(self.z)).wrapping_abs() as f32;
        f + f1 + f2
    }

    #[must_use]
    pub const fn as_block_pos(&self) -> BlockPos {
        BlockPos::new(self.x, self.y, self.z)
    }

    #[must_use]
    pub const fn as_vec3(&self) -> Vec3 {
        Vec3::new(self.x as f64, self.y as f64, self.z as f64)
    }

    #[must_use]
    pub const fn same_as(&self, other: &Self) -> bool {
        self.x == other.x && self.y == other.y && self.z == other.z
    }

    #[must_use]
    pub const fn in_open_set(&self) -> bool {
        self.heap_idx >= 0
    }
}

/// Mirrors `Node.equals`: compares `hash`, `x`, `y`, `z`.
impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.x == other.x && self.y == other.y && self.z == other.z
    }
}

impl Eq for Node {}

/// Port of `net.minecraft.world.level.pathfinder.Target` in Paper 26.2.
#[derive(Debug, Clone)]
pub struct Target {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub hash: i32,
    pub best_heuristic: f32,
    pub best_node: Option<usize>,
    pub reached: bool,
}

impl Target {
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self {
            x,
            y,
            z,
            hash: Node::create_hash(x, y, z),
            best_heuristic: f32::MAX,
            best_node: None,
            reached: false,
        }
    }

    #[must_use]
    pub const fn from_node(node: &Node) -> Self {
        Self::new(node.x, node.y, node.z)
    }

    /// Mirrors `Target.updateBest(float heuristic, Node node)`.
    /// Strict `<` preserves the earlier node on ties.
    pub fn update_best(&mut self, heuristic: f32, node_idx: usize) {
        if heuristic < self.best_heuristic {
            self.best_heuristic = heuristic;
            self.best_node = Some(node_idx);
        }
    }

    #[must_use]
    pub const fn best_node(&self) -> Option<usize> {
        self.best_node
    }

    pub fn set_reached(&mut self) {
        self.reached = true;
    }

    #[must_use]
    pub const fn is_reached(&self) -> bool {
        self.reached
    }

    #[must_use]
    pub const fn as_block_pos(&self) -> BlockPos {
        BlockPos::new(self.x, self.y, self.z)
    }

    #[must_use]
    pub fn distance_to_node(&self, node: &Node) -> f32 {
        let xd = (self.x.wrapping_sub(node.x)) as f32;
        let yd = (self.y.wrapping_sub(node.y)) as f32;
        let zd = (self.z.wrapping_sub(node.z)) as f32;
        mth::sqrt(xd * xd + yd * yd + zd * zd)
    }

    #[must_use]
    pub fn distance_manhattan_node(&self, node: &Node) -> f32 {
        let f = (self.x.wrapping_sub(node.x)).wrapping_abs() as f32;
        let f1 = (self.y.wrapping_sub(node.y)).wrapping_abs() as f32;
        let f2 = (self.z.wrapping_sub(node.z)).wrapping_abs() as f32;
        f + f1 + f2
    }
}

/// Arena owning all [`Node`]s allocated during a single pathfinding search.
///
/// Mirrors `NodeEvaluator.nodes: Int2ObjectMap<Node>` keyed by `Node.createHash(x, y, z)`.
/// Uses `IndexMap` for deterministic iteration order.
#[derive(Debug, Default)]
pub struct NodeArena {
    nodes: Vec<Node>,
    by_hash: IndexMap<i32, usize>,
}

impl NodeArena {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.by_hash.clear();
    }

    /// Mirrors `NodeEvaluator.getNode(int x, int y, int z)`.
    pub fn get_or_create(&mut self, x: i32, y: i32, z: i32) -> usize {
        let hash = Node::create_hash(x, y, z);
        if let Some(&idx) = self.by_hash.get(&hash) {
            return idx;
        }
        let idx = self.nodes.len();
        self.nodes.push(Node::new(x, y, z));
        self.by_hash.insert(hash, idx);
        idx
    }

    #[must_use]
    pub fn get(&self, idx: usize) -> &Node {
        &self.nodes[idx]
    }

    pub fn get_mut(&mut self, idx: usize) -> &mut Node {
        &mut self.nodes[idx]
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter()
    }
}

/// Port of `net.minecraft.world.level.pathfinder.BinaryHeap` in Paper 26.2.
///
/// Stores indices into [`NodeArena`], maintaining `node.heap_idx` identically to
/// Paper's `Node[] heap`, including initial capacity 128, doubling on full, and
/// exact `<` tie-breaking in `upHeap` / `downHeap`.
#[derive(Debug)]
pub struct BinaryHeap {
    heap: Vec<usize>,
    size: usize,
}

impl Default for BinaryHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl BinaryHeap {
    pub const INITIAL_CAPACITY: usize = 128;

    #[must_use]
    pub fn new() -> Self {
        Self {
            heap: vec![0; Self::INITIAL_CAPACITY],
            size: 0,
        }
    }

    /// Mirrors `BinaryHeap.insert(Node)`.
    ///
    /// # Panics
    /// Panics if the node is already in the open set (`heap_idx >= 0`), matching
    /// Paper's `IllegalStateException("OW KNOWS!")`.
    pub fn insert(&mut self, arena: &mut NodeArena, node_idx: usize) -> usize {
        assert!(
            arena.get(node_idx).heap_idx < 0,
            "Node is already in BinaryHeap (OW KNOWS!)"
        );
        if self.size == self.heap.len() {
            self.heap.resize(self.size << 1, 0);
        }
        self.heap[self.size] = node_idx;
        arena.get_mut(node_idx).heap_idx = self.size as i32;
        self.up_heap(arena, self.size);
        self.size += 1;
        node_idx
    }

    /// Mirrors `BinaryHeap.clear()` (resets size only).
    pub fn clear(&mut self) {
        self.size = 0;
    }

    #[must_use]
    pub fn peek(&self) -> usize {
        self.heap[0]
    }

    /// Mirrors `BinaryHeap.pop()`.
    pub fn pop(&mut self, arena: &mut NodeArena) -> usize {
        let node = self.heap[0];
        self.size -= 1;
        self.heap[0] = self.heap[self.size];
        self.heap[self.size] = 0;
        if self.size > 0 {
            self.down_heap(arena, 0);
        }
        arena.get_mut(node).heap_idx = -1;
        node
    }

    /// Mirrors `BinaryHeap.remove(Node)`.
    pub fn remove(&mut self, arena: &mut NodeArena, node_idx: usize) {
        let removed_idx = arena.get(node_idx).heap_idx as usize;
        self.size -= 1;
        let replacement = self.heap[self.size];
        self.heap[removed_idx] = replacement;
        self.heap[self.size] = 0;
        if self.size > removed_idx {
            if arena.get(replacement).f < arena.get(node_idx).f {
                self.up_heap(arena, removed_idx);
            } else {
                self.down_heap(arena, removed_idx);
            }
        }
        arena.get_mut(node_idx).heap_idx = -1;
    }

    /// Mirrors `BinaryHeap.changeCost(Node, float)`.
    pub fn change_cost(&mut self, arena: &mut NodeArena, node_idx: usize, new_cost: f32) {
        let old_cost = arena.get(node_idx).f;
        let heap_idx = arena.get(node_idx).heap_idx as usize;
        arena.get_mut(node_idx).f = new_cost;
        if new_cost < old_cost {
            self.up_heap(arena, heap_idx);
        } else {
            self.down_heap(arena, heap_idx);
        }
    }

    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Mirrors `BinaryHeap.getHeap()` — returns the active prefix of the heap.
    #[must_use]
    pub fn as_slice(&self) -> &[usize] {
        &self.heap[..self.size]
    }

    /// Exact port of `BinaryHeap.upHeap(int idx)`.
    fn up_heap(&mut self, arena: &mut NodeArena, mut idx: usize) {
        let node = self.heap[idx];
        let cost = arena.get(node).f;

        while idx > 0 {
            let parent_idx = (idx - 1) >> 1;
            let parent = self.heap[parent_idx];
            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            if !(cost < arena.get(parent).f) {
                break;
            }
            self.heap[idx] = parent;
            arena.get_mut(parent).heap_idx = idx as i32;
            idx = parent_idx;
        }

        self.heap[idx] = node;
        arena.get_mut(node).heap_idx = idx as i32;
    }

    /// Exact port of `BinaryHeap.downHeap(int idx)`.
    fn down_heap(&mut self, arena: &mut NodeArena, mut idx: usize) {
        let node = self.heap[idx];
        let cost = arena.get(node).f;

        loop {
            let left_idx = 1 + (idx << 1);
            let right_idx = left_idx + 1;
            if left_idx >= self.size {
                break;
            }

            let left_node = self.heap[left_idx];
            let left_cost = arena.get(left_node).f;
            let (right_node, right_cost) = if right_idx >= self.size {
                (None, f32::INFINITY)
            } else {
                let rn = self.heap[right_idx];
                (Some(rn), arena.get(rn).f)
            };

            if left_cost < right_cost {
                #[allow(clippy::neg_cmp_op_on_partial_ord)]
                if !(left_cost < cost) {
                    break;
                }
                self.heap[idx] = left_node;
                arena.get_mut(left_node).heap_idx = idx as i32;
                idx = left_idx;
            } else {
                #[allow(clippy::neg_cmp_op_on_partial_ord)]
                if !(right_cost < cost) {
                    break;
                }
                let rn = right_node.expect("right_cost < cost implies right_node exists");
                self.heap[idx] = rn;
                arena.get_mut(rn).heap_idx = idx as i32;
                idx = right_idx;
            }
        }

        self.heap[idx] = node;
        arena.get_mut(node).heap_idx = idx as i32;
    }
}
