//! Java25 HashSet<BlockPos>, preserving HashMap bucket and red-black iteration.
//! BlockPos is Comparable<Vec3i>, not Comparable<BlockPos>; equal full hashes
//! inside tree bins require JVM identityHashCode and fail explicitly here.
use super::BlockPos;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaBlockPosSetError {
    IdentityHashRequired { hash: i32 },
}
impl std::fmt::Display for JavaBlockPosSetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IdentityHashRequired { hash } => write!(
                f,
                "Java BlockPos tree-bin hash {hash} requires JVM identityHashCode ordering"
            ),
        }
    }
}
impl std::error::Error for JavaBlockPosSetError {}
#[derive(Debug, Clone)]
struct Node {
    key: BlockPos,
    hash: i32,
    parent: Option<usize>,
    child: [Option<usize>; 2],
    red: bool,
}
#[derive(Debug, Clone, Default)]
struct Bin {
    order: Vec<usize>,
    tree: bool,
}
#[derive(Debug, Clone, Default)]
pub struct JavaBlockPosSet {
    bins: Vec<Bin>,
    nodes: Vec<Node>,
    size: usize,
    threshold: usize,
}

impl JavaBlockPosSet {
    pub fn new() -> Self {
        Self::default()
    }
    /// New HashMap<>().putAll(source): unlike repeated default-map put,
    /// putMapEntries pre-sizes using the source map's complete entry count.
    pub fn try_from_map_entries(
        positions: impl IntoIterator<Item = BlockPos>,
    ) -> Result<Self, JavaBlockPosSetError> {
        let positions: Vec<_> = positions.into_iter().collect();
        let mut set = Self::new();
        if !positions.is_empty() {
            // Pinned JDK putMapEntries uses double ceil, not the older
            // float(size / loadFactor + 1) implementation (different at 12).
            let target = ((positions.len() as f64) / 0.75).ceil() as usize;
            let capacity = target.min(1 << 30).next_power_of_two();
            set.bins = vec![Bin::default(); capacity];
            set.threshold = capacity * 3 / 4;
        }
        for pos in positions {
            set.try_insert(pos)?;
        }
        Ok(set)
    }
    /// Replays new HashSet<>() then adds; not the pre-sized collection constructor.
    pub fn try_from_insertions(
        positions: impl IntoIterator<Item = BlockPos>,
    ) -> Result<Self, JavaBlockPosSetError> {
        let mut set = Self::new();
        for pos in positions {
            set.try_insert(pos)?;
        }
        Ok(set)
    }
    pub fn len(&self) -> usize {
        self.size
    }
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
    pub fn iter(&self) -> impl Iterator<Item = &BlockPos> {
        self.bins
            .iter()
            .flat_map(|b| b.order.iter().map(|&id| &self.nodes[id].key))
    }
    pub fn contains(&self, key: &BlockPos) -> bool {
        !self.bins.is_empty()
            && self.bins[self.bucket(spread(key))]
                .order
                .iter()
                .any(|&id| self.nodes[id].key == *key)
    }
    pub fn try_insert(&mut self, key: BlockPos) -> Result<bool, JavaBlockPosSetError> {
        if self.bins.is_empty() {
            self.resize();
        }
        let hash = spread(&key);
        let bucket = self.bucket(hash);
        if self.bins[bucket]
            .order
            .iter()
            .any(|&id| self.nodes[id].key == key)
        {
            return Ok(false);
        }
        let tree = self.bins[bucket].tree;
        if tree || (self.bins[bucket].order.len() >= 8 && self.bins.len() >= 64) {
            let mut hashes = std::collections::HashSet::new();
            for &id in &self.bins[bucket].order {
                let existing = self.nodes[id].hash;
                if existing == hash || !hashes.insert(existing) {
                    return Err(JavaBlockPosSetError::IdentityHashRequired { hash: existing });
                }
            }
        }
        let id = self.nodes.len();
        self.nodes.push(Node {
            key,
            hash,
            parent: None,
            child: [None; 2],
            red: false,
        });
        if tree {
            let root = self.root(self.bins[bucket].order[0]);
            let parent = self.link(root, id);
            let index = self.bins[bucket]
                .order
                .iter()
                .position(|&n| n == parent)
                .unwrap();
            self.bins[bucket].order.insert(index + 1, id);
            let root = self.balance_insert(root, id);
            self.front(bucket, root);
        } else {
            self.bins[bucket].order.push(id);
            if self.bins[bucket].order.len() >= 9 {
                if self.bins.len() < 64 {
                    self.resize();
                } else {
                    self.treeify(bucket);
                }
            }
        }
        self.size += 1;
        if self.size > self.threshold {
            self.resize();
        }
        Ok(true)
    }
    pub fn remove(&mut self, key: &BlockPos) -> bool {
        self.remove_internal(key, true)
    }

    /// Java iterator.next() followed by iterator.remove(). Unlike remove(key),
    /// this must not move tree roots to the front or untreeify a small tree.
    pub fn pop_first(&mut self) -> Option<BlockPos> {
        let first = self.iter().next().copied()?;
        self.remove_internal(&first, false);
        Some(first)
    }

    fn remove_internal(&mut self, key: &BlockPos, movable: bool) -> bool {
        if self.bins.is_empty() {
            return false;
        }
        let bucket = self.bucket(spread(key));
        let Some(index) = self.bins[bucket]
            .order
            .iter()
            .position(|&id| self.nodes[id].key == *key)
        else {
            return false;
        };
        let id = self.bins[bucket].order[index];
        let root = self.root(self.bins[bucket].order[0]);
        self.bins[bucket].order.remove(index);
        self.size -= 1;
        if self.bins[bucket].tree && !self.bins[bucket].order.is_empty() {
            // Java tests pre-removal tree shape, not just the remaining count.
            let left = self.nodes[root].child[0];
            if movable
                && (self.nodes[root].child[1].is_none()
                    || left.is_none()
                    || self.nodes[left.unwrap()].child[0].is_none())
            {
                self.bins[bucket].tree = false;
            } else {
                let root = self.remove_tree(root, id);
                if movable {
                    self.front(bucket, root);
                }
            }
        } else if self.bins[bucket].order.is_empty() {
            self.bins[bucket].tree = false;
        }
        true
    }
    fn bucket(&self, hash: i32) -> usize {
        hash as u32 as usize & (self.bins.len() - 1)
    }

    fn root(&self, mut id: usize) -> usize {
        while let Some(parent) = self.nodes[id].parent {
            id = parent;
        }
        id
    }
    fn resize(&mut self) {
        let old_cap = self.bins.len();
        let cap = if old_cap == 0 { 16 } else { old_cap * 2 };
        let old = std::mem::replace(&mut self.bins, vec![Bin::default(); cap]);
        self.threshold = cap * 3 / 4;
        for (index, bin) in old.into_iter().enumerate() {
            let (lo, hi): (Vec<_>, Vec<_>) = bin
                .order
                .into_iter()
                .partition(|&id| self.nodes[id].hash as u32 as usize & old_cap == 0);
            let split = !lo.is_empty() && !hi.is_empty();
            for (target, order) in [(index, lo), (index + old_cap, hi)] {
                if order.is_empty() {
                    continue;
                }
                let tree = bin.tree && order.len() > 6;
                self.bins[target] = Bin { order, tree };
                if tree && split {
                    self.treeify(target);
                }
            }
        }
    }
    fn front(&mut self, bucket: usize, root: usize) {
        let order = &mut self.bins[bucket].order;
        let index = order.iter().position(|&id| id == root).unwrap();
        if index != 0 {
            order.remove(index);
            order.insert(0, root);
        }
    }
    fn treeify(&mut self, bucket: usize) {
        let order = self.bins[bucket].order.clone();
        let mut root = order[0];
        for (i, &id) in order.iter().enumerate() {
            self.nodes[id].parent = None;
            self.nodes[id].child = [None; 2];
            self.nodes[id].red = false;
            if i != 0 {
                self.link(root, id);
                root = self.balance_insert(root, id);
            }
        }
        self.bins[bucket].tree = true;
        self.front(bucket, root);
    }
    fn link(&mut self, mut parent: usize, id: usize) -> usize {
        loop {
            let dir = usize::from(self.nodes[id].hash > self.nodes[parent].hash);
            if let Some(next) = self.nodes[parent].child[dir] {
                parent = next;
            } else {
                self.nodes[parent].child[dir] = Some(id);
                self.nodes[id].parent = Some(parent);
                return parent;
            }
        }
    }
    /// dir0=rotateLeft, dir1=rotateRight.
    fn rotate(&mut self, mut root: usize, p: usize, dir: usize) -> usize {
        let r = self.nodes[p].child[1 - dir].unwrap();
        self.nodes[p].child[1 - dir] = self.nodes[r].child[dir];
        if let Some(c) = self.nodes[r].child[dir] {
            self.nodes[c].parent = Some(p);
        }
        let parent = self.nodes[p].parent;
        self.nodes[r].parent = parent;
        if let Some(pp) = parent {
            let side = usize::from(self.nodes[pp].child[1] == Some(p));
            self.nodes[pp].child[side] = Some(r);
        } else {
            root = r;
            self.nodes[r].red = false;
        }
        self.nodes[r].child[dir] = Some(p);
        self.nodes[p].parent = Some(r);
        root
    }
    fn balance_insert(&mut self, mut root: usize, mut x: usize) -> usize {
        self.nodes[x].red = true;
        loop {
            let Some(mut p) = self.nodes[x].parent else {
                self.nodes[x].red = false;
                return x;
            };
            if !self.nodes[p].red {
                return root;
            }
            let Some(mut g) = self.nodes[p].parent else {
                return root;
            };
            let side = usize::from(self.nodes[g].child[1] == Some(p));
            if let Some(u) = self.nodes[g].child[1 - side].filter(|&n| self.nodes[n].red) {
                self.nodes[u].red = false;
                self.nodes[p].red = false;
                self.nodes[g].red = true;
                x = g;
            } else {
                if self.nodes[p].child[1 - side] == Some(x) {
                    x = p;
                    root = self.rotate(root, x, side);
                    p = self.nodes[x].parent.unwrap();
                    g = self.nodes[p].parent.unwrap();
                }
                self.nodes[p].red = false;
                self.nodes[g].red = true;
                root = self.rotate(root, g, 1 - side);
            }
        }
    }
    fn remove_tree(&mut self, mut root: usize, p: usize) -> usize {
        let [left, right] = self.nodes[p].child;
        let replacement;
        if let (Some(left), Some(right)) = (left, right) {
            let mut s = right;
            while let Some(next) = self.nodes[s].child[0] {
                s = next;
            }
            let red = self.nodes[s].red;
            self.nodes[s].red = self.nodes[p].red;
            self.nodes[p].red = red;
            let sr = self.nodes[s].child[1];
            let pp = self.nodes[p].parent;
            if s == right {
                self.nodes[p].parent = Some(s);
                self.nodes[s].child[1] = Some(p);
            } else {
                let sp = self.nodes[s].parent.unwrap();
                self.nodes[p].parent = Some(sp);
                let side = usize::from(self.nodes[sp].child[1] == Some(s));
                self.nodes[sp].child[side] = Some(p);
                self.nodes[s].child[1] = Some(right);
                self.nodes[right].parent = Some(s);
            }
            self.nodes[p].child[0] = None;
            self.nodes[p].child[1] = sr;
            if let Some(sr) = sr {
                self.nodes[sr].parent = Some(p);
            }
            self.nodes[s].child[0] = Some(left);
            self.nodes[left].parent = Some(s);
            self.nodes[s].parent = pp;
            if let Some(pp) = pp {
                let side = usize::from(self.nodes[pp].child[1] == Some(p));
                self.nodes[pp].child[side] = Some(s);
            } else {
                root = s;
            }
            replacement = sr.unwrap_or(p);
        } else {
            replacement = left.or(right).unwrap_or(p);
        }
        if replacement != p {
            let pp = self.nodes[p].parent;
            self.nodes[replacement].parent = pp;
            if let Some(pp) = pp {
                let side = usize::from(self.nodes[pp].child[1] == Some(p));
                self.nodes[pp].child[side] = Some(replacement);
            } else {
                root = replacement;
                self.nodes[replacement].red = false;
            }
            self.nodes[p].child = [None; 2];
            self.nodes[p].parent = None;
        }
        if !self.nodes[p].red {
            root = self.balance_delete(root, replacement);
        }
        if replacement == p
            && let Some(pp) = self.nodes[p].parent.take()
        {
            for side in 0..2 {
                if self.nodes[pp].child[side] == Some(p) {
                    self.nodes[pp].child[side] = None;
                }
            }
        }
        root
    }
    fn red(&self, n: Option<usize>) -> bool {
        n.is_some_and(|id| self.nodes[id].red)
    }
    fn balance_delete(&mut self, mut root: usize, mut x: usize) -> usize {
        loop {
            if x == root {
                return root;
            }
            let Some(mut p) = self.nodes[x].parent else {
                self.nodes[x].red = false;
                return x;
            };
            if self.nodes[x].red {
                self.nodes[x].red = false;
                return root;
            }
            let side = usize::from(self.nodes[p].child[1] == Some(x));
            let mut sibling = self.nodes[p].child[1 - side];
            if let Some(s) = sibling.filter(|&s| self.nodes[s].red) {
                self.nodes[s].red = false;
                self.nodes[p].red = true;
                root = self.rotate(root, p, side);
                p = self.nodes[x].parent.unwrap();
                sibling = self.nodes[p].child[1 - side];
            }
            let Some(s) = sibling else {
                x = p;
                continue;
            };
            if !self.red(self.nodes[s].child[0]) && !self.red(self.nodes[s].child[1]) {
                self.nodes[s].red = true;
                x = p;
            } else {
                if !self.red(self.nodes[s].child[1 - side]) {
                    if let Some(inner) = self.nodes[s].child[side] {
                        self.nodes[inner].red = false;
                    }
                    self.nodes[s].red = true;
                    root = self.rotate(root, s, 1 - side);
                    p = self.nodes[x].parent.unwrap();
                    sibling = self.nodes[p].child[1 - side];
                }
                if let Some(s) = sibling {
                    self.nodes[s].red = self.nodes[p].red;
                    if let Some(outer) = self.nodes[s].child[1 - side] {
                        self.nodes[outer].red = false;
                    }
                }
                self.nodes[p].red = false;
                root = self.rotate(root, p, side);
                x = root;
            }
        }
    }
}
fn spread(key: &BlockPos) -> i32 {
    let hash = key.hash_code() as u32;
    (hash ^ (hash >> 16)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    fn positions(text: &str) -> Vec<BlockPos> {
        if text == "-" {
            return Vec::new();
        }
        text.split(';')
            .map(|p| {
                let c: Vec<i32> = p.split(',').map(|v| v.parse().unwrap()).collect();
                BlockPos::new(c[0], c[1], c[2])
            })
            .collect()
    }

    #[test]
    fn iteration_growth_tree_splits_and_removal_match_live_jdk25() {
        let fixture = include_str!("../../../../tools/rivet-oracle/fixtures/block-pos-set.tsv");
        let mut set = JavaBlockPosSet::new();
        let mut current = "";
        let mut rows = 0;
        for row in fixture
            .lines()
            .filter(|row| !row.starts_with('#') && !row.is_empty())
        {
            let f: Vec<_> = row.split('\t').collect();
            assert_eq!(f.len(), 7, "{row}");
            if current != f[0] {
                set = JavaBlockPosSet::new();
                current = f[0];
            }
            let mut changed = 0;
            match f[1] {
                "add" => {
                    for pos in positions(f[2]) {
                        changed += usize::from(set.try_insert(pos).unwrap());
                    }
                }
                "remove" => {
                    for pos in positions(f[2]) {
                        changed += usize::from(set.remove(&pos));
                    }
                }
                "pop" => {
                    for _ in 0..f[2].parse::<usize>().unwrap() {
                        set.pop_first().unwrap();
                        changed += 1;
                    }
                }
                "identity" => {
                    let before = set.iter().copied().collect::<Vec<_>>();
                    assert_eq!(
                        set.try_insert(positions(f[2])[0]),
                        Err(JavaBlockPosSetError::IdentityHashRequired { hash: 0 })
                    );
                    assert_eq!(set.iter().copied().collect::<Vec<_>>(), before);
                    rows += 1;
                    continue;
                }
                other => panic!("unknown fixture op {other}"),
            }
            assert_eq!(changed, f[3].parse::<usize>().unwrap(), "{row}");
            assert_eq!(set.bins.len(), f[4].parse::<usize>().unwrap(), "{row}");
            assert_eq!(
                set.bins.iter().filter(|b| b.tree).count(),
                f[5].parse::<usize>().unwrap(),
                "{row}"
            );
            assert_eq!(
                set.iter().copied().collect::<Vec<_>>(),
                positions(f[6]),
                "{row}"
            );
            assert_eq!(set.len(), set.iter().count());
            for pos in set.iter() {
                assert!(set.contains(pos));
            }
            rows += 1;
        }
        assert_eq!(rows, 26);
    }
}
