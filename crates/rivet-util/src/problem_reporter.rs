//! Port of `net.minecraft.util.ProblemReporter` — the serialize/deserialize
//! error collector used by the level-storage value layer (issue #382).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/util/
//! ProblemReporter.java` (a leaf of the `mc.util` manifest unit; only the
//! subset the value layer needs is ported here — `#382`).
//!
//! The path element records (`FieldPathElement`, `IndexedFieldPathElement`,
//! `IndexedPathElement`) are the ones `TagValueInput`/`TagValueOutput` build;
//! `ElementReferencePathElement`/`RootElementPathElement`/`MapEntryPathElement`
//! (registry/reporting surfaces) and `ScopedCollector` (the SLF4J logging
//! close) defer with the owners that consume them — `CrashReport`, `Entity`,
//! registry loading, and the server's logging sink.
//!
//! The `Problem`/`PathElement` traits carry a `Debug` supertrait: the frames
//! need it for their derives, and every Java implementor is a record (which has
//! value `toString`).
//!
//! Modeling note — Java's `Collector.Entry` stores the *frame* a problem was
//! reported against (an immutable-linked `parent` chain sharing one mutable
//! `LinkedHashSet` store) and resolves the path lazily when a report is
//! generated. The Rust port stores the resolved element chain in the entry at
//! report time: walking `parent` up to the terminal root at report time and at
//! report generation time are observationally identical (the chain is
//! immutable), so the output is the same while avoiding object-identity
//! comparisons. `getTreeReport` is root-anchored (`this` == the terminal root),
//! which is the only in-scope usage (the value layer reports through the root);
//! Java's non-root `this` boundary is not ported.

use std::cell::RefCell;
use std::rc::Rc;

/// `ProblemReporter.PathElement` — one segment of an accumulated report path
/// (`@FunctionalInterface String get()`).
pub trait PathElement: std::fmt::Debug {
    /// `PathElement.get()`.
    fn get(&self) -> String;
}

/// `ProblemReporter.Problem` — a reported error with a human-readable
/// description.
pub trait Problem: std::fmt::Debug {
    /// `Problem.description()`.
    fn description(&self) -> String;
}

/// `ProblemReporter.FieldPathElement(String name)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldPathElement(pub String);

impl PathElement for FieldPathElement {
    fn get(&self) -> String {
        format!(".{}", self.0)
    }
}

/// `ProblemReporter.IndexedFieldPathElement(String name, int index)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFieldPathElement(pub String, pub i32);

impl PathElement for IndexedFieldPathElement {
    fn get(&self) -> String {
        format!(".{}[{}]", self.0, self.1)
    }
}

/// `ProblemReporter.IndexedPathElement(int index)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedPathElement(pub i32);

impl PathElement for IndexedPathElement {
    fn get(&self) -> String {
        format!("[{}]", self.0)
    }
}

/// `ProblemReporter` — the mutable, forkable problem collector.
///
/// `forChild` forks a child collector that shares the parent's problem store
/// but resolves future reports against a longer path. `report` records a
/// problem against the collector it is called on.
pub trait ProblemReporter {
    /// `ProblemReporter.forChild(PathElement)`.
    fn for_child(&self, path: Rc<dyn PathElement>) -> Rc<dyn ProblemReporter>;

    /// `ProblemReporter.report(Problem)`.
    fn report(&self, problem: Rc<dyn Problem>);
}

/// `ProblemReporter.DISCARDING` — the shared no-op collector.
pub struct DiscardingReporter;

impl ProblemReporter for DiscardingReporter {
    fn for_child(&self, _path: Rc<dyn PathElement>) -> Rc<dyn ProblemReporter> {
        Rc::new(DiscardingReporter)
    }

    fn report(&self, _problem: Rc<dyn Problem>) {}
}

/// A recorded problem with the path chain of the collector it was reported
/// against (source-first, terminal-root last) — Java `Collector.Entry`'s
/// lazily-resolved path, resolved at report time (see the module doc).
#[derive(Debug)]
struct Entry {
    path: Vec<Rc<dyn PathElement>>,
    problem: Rc<dyn Problem>,
}

/// `ProblemReporter.Collector` — the frame-based collector.
///
/// `EMPTY_ROOT` is the `PathElement` returning `""` (the no-arg `Collector()`
/// root). Children created by `forChild` share `problems` with the parent.
#[derive(Debug)]
pub struct Collector {
    parent: Option<Rc<Collector>>,
    element: Rc<dyn PathElement>,
    problems: Rc<RefCell<Vec<Entry>>>,
}

impl Collector {
    /// `new Collector()` — the root with `EMPTY_ROOT`.
    pub fn new() -> Self {
        Collector::with_root(Rc::new(EmptyRootPathElement))
    }

    /// `new Collector(PathElement root)`.
    pub fn with_root(root: Rc<dyn PathElement>) -> Self {
        Collector {
            parent: None,
            element: root,
            problems: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// `Collector.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.problems.borrow().is_empty()
    }

    /// `Collector.getReport()` — one path-prefixed line per entry, grouped by
    /// path in first-seen order.
    ///
    /// Java builds the report from a `HashMultimap` (Guava preserves per-key
    /// insertion order); the Rust port mirrors that with an ordered
    /// `indexmap::IndexMap<String, Vec<Rc<dyn Problem>>>` — first-seen path
    /// order, descriptions joined `"; "` per path (Java `Collectors.joining(";
    /// ")`).
    pub fn get_report(&self) -> String {
        let mut grouped: indexmap::IndexMap<String, Vec<Rc<dyn Problem>>> =
            indexmap::IndexMap::new();
        let borrowed = self.problems.borrow();
        for entry in borrowed.iter() {
            grouped
                .entry(entry.path_string())
                .or_default()
                .push(Rc::clone(&entry.problem));
        }
        grouped
            .into_iter()
            .map(|(path, problems)| {
                let descriptions = problems
                    .iter()
                    .map(|p| p.description())
                    .collect::<Vec<_>>()
                    .join("; ");
                format!(" at {path}: {descriptions}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `Collector.getTreeReport()` — the indented tree form. Ported for
    /// completeness of the leaf (it is the form `ScopedCollector.close` logs);
    /// the report consumers in this crate use `get_report`.
    ///
    /// Root-anchored: the tree is built from `self` as the root, matching Java
    /// when `getTreeReport` is called on the root collector (the only in-scope
    /// usage).
    pub fn get_tree_report(&self) -> String {
        let root = Rc::new(ProblemTreeNode::new(Rc::clone(&self.element)));
        let borrowed = self.problems.borrow();
        for entry in borrowed.iter() {
            // Java collects the chain from `entry.source` up to (exclusive)
            // `this`; root-anchored, that is every element below the root.
            // `entry.path` is source-first, so skip the terminal root element.
            let mut node = Rc::clone(&root);
            for element in entry.path.iter().rev().skip(1) {
                node = node.child(Rc::clone(element));
            }
            node.problems.borrow_mut().push(Rc::clone(&entry.problem));
        }
        root.get_lines().join("\n")
    }

    /// Collect this frame's path chain — `[self.element, parent.element, …,
    /// root.element]` (source-first). This is Java's `forEach` inner walk
    /// (`for (current = entry.source; current != null; current =
    /// current.parent)`).
    fn resolve_chain(&self) -> Vec<Rc<dyn PathElement>> {
        let mut chain: Vec<Rc<dyn PathElement>> = Vec::new();
        let mut current = Some(self);
        while let Some(frame) = current {
            chain.push(Rc::clone(&frame.element));
            current = frame.parent.as_deref();
        }
        chain
    }
}

impl Entry {
    /// The accumulated path string, root-to-leaf (`EMPTY_ROOT` renders empty).
    fn path_string(&self) -> String {
        self.path
            .iter()
            .rev()
            .map(|element| element.get())
            .collect::<String>()
    }
}

/// The private root element (Java `Collector.EMPTY_ROOT`).
#[derive(Debug)]
struct EmptyRootPathElement;

impl PathElement for EmptyRootPathElement {
    fn get(&self) -> String {
        String::new()
    }
}

impl ProblemReporter for Collector {
    fn for_child(&self, path: Rc<dyn PathElement>) -> Rc<dyn ProblemReporter> {
        Rc::new(Collector {
            parent: Some(Rc::new(Collector {
                parent: self.parent.clone(),
                element: Rc::clone(&self.element),
                problems: Rc::clone(&self.problems),
            })),
            element: path,
            problems: Rc::clone(&self.problems),
        })
    }

    fn report(&self, problem: Rc<dyn Problem>) {
        self.problems.borrow_mut().push(Entry {
            path: self.resolve_chain(),
            problem,
        });
    }
}

impl Default for Collector {
    fn default() -> Self {
        Collector::new()
    }
}

/// `Collector.ProblemTreeNode` — the tree used by `getTreeReport`.
///
/// Children are keyed by the rendered path string. Java keys by the record's
/// field-equals; the two agree because the record fields are exactly what
/// `get()` renders (see the module doc).
#[derive(Debug)]
struct ProblemTreeNode {
    element: Rc<dyn PathElement>,
    problems: RefCell<Vec<Rc<dyn Problem>>>,
    children: RefCell<indexmap::IndexMap<String, Rc<ProblemTreeNode>>>,
}

impl ProblemTreeNode {
    fn new(element: Rc<dyn PathElement>) -> Self {
        ProblemTreeNode {
            element,
            problems: RefCell::new(Vec::new()),
            children: RefCell::new(indexmap::IndexMap::new()),
        }
    }

    /// `ProblemTreeNode.child(PathElement)` — `computeIfAbsent`.
    fn child(&self, id: Rc<dyn PathElement>) -> Rc<ProblemTreeNode> {
        let key = id.get();
        let mut children = self.children.borrow_mut();
        if let Some(existing) = children.get(&key) {
            return Rc::clone(existing);
        }
        let node = Rc::new(ProblemTreeNode::new(id));
        children.insert(key, Rc::clone(&node));
        node
    }

    /// `ProblemTreeNode.getLines()` — the four Java branches.
    fn get_lines(&self) -> Vec<String> {
        let problem_count = self.problems.borrow().len();
        let children_count = self.children.borrow().len();
        if problem_count == 0 && children_count == 0 {
            return Vec::new();
        }

        if problem_count == 0 && children_count == 1 {
            let child = self
                .children
                .borrow()
                .values()
                .next()
                .cloned()
                .expect("one child");
            let mut lines = child.get_lines();
            lines[0] = format!("{}{}", self.element.get(), lines[0]);
            return lines;
        }

        if problem_count == 1 && children_count == 0 {
            return vec![format!(
                "{}: {}",
                self.element.get(),
                self.problems.borrow()[0].description()
            )];
        }

        let mut lines = Vec::new();
        for child in self.children.borrow().values().cloned().collect::<Vec<_>>() {
            lines.extend(child.get_lines());
        }
        for line in lines.iter_mut() {
            *line = format!("  {line}");
        }
        for problem in self.problems.borrow().iter() {
            lines.push(format!("  {}", problem.description()));
        }
        lines.insert(0, format!("{}:", self.element.get()));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::problem_reporter::{Collector, DiscardingReporter, ProblemReporter};

    /// `ProblemReporter.DISCARDING` swallows reports and child forks.
    #[test]
    fn discarding_swallows_reports() {
        let discarding: Rc<dyn ProblemReporter> = Rc::new(DiscardingReporter);
        discarding.report(Rc::new(TestProblem("boom".to_string())));
        let child = discarding.for_child(Rc::new(FieldPathElement("c".to_string())));
        child.report(Rc::new(TestProblem("boom2".to_string())));
        // No panic is the observable contract; nothing to assert on the
        // reporter itself.
        let _ = child.for_child(Rc::new(IndexedPathElement(0)));
    }

    #[derive(Debug)]
    struct TestProblem(String);

    impl Problem for TestProblem {
        fn description(&self) -> String {
            self.0.clone()
        }
    }

    /// `Collector.forChild(...).report(...)` accumulates; `getReport` emits the
    /// Java path-prefixed lines in first-seen path order.
    #[test]
    fn collector_accumulates_and_reports_paths() {
        let root = Rc::new(Collector::new());
        let a = root.for_child(Rc::new(FieldPathElement("a".to_string())));
        let a_b = a.for_child(Rc::new(FieldPathElement("b".to_string())));
        a_b.report(Rc::new(TestProblem("first".to_string())));
        a_b.report(Rc::new(TestProblem("second".to_string())));
        root.report(Rc::new(TestProblem("root".to_string())));

        let report = root.get_report();
        assert_eq!(
            report, " at .a.b: first; second\n at : root",
            "same-path entries join with '; ', root path is empty"
        );
    }

    /// `isEmpty` reflects reports on child frames too (shared store).
    #[test]
    fn empty_reflects_shared_store() {
        let root = Rc::new(Collector::new());
        assert!(root.is_empty());
        root.for_child(Rc::new(FieldPathElement("x".to_string())))
            .report(Rc::new(TestProblem("p".to_string())));
        assert!(!root.is_empty());
    }

    /// `getTreeReport` matches Java's layout exactly, including the
    /// single-child path-collapsing branch (`lines.set(0, element.get() +
    /// lines.get(0))`).
    #[test]
    fn tree_report_nests_paths() {
        let root = Rc::new(Collector::with_root(Rc::new(FieldPathElement(
            "root".to_string(),
        ))));
        let child = root.for_child(Rc::new(FieldPathElement("child".to_string())));
        child.report(Rc::new(TestProblem("leaf".to_string())));
        child.report(Rc::new(TestProblem("leaf2".to_string())));

        let tree = root.get_tree_report();
        assert_eq!(
            tree, ".root.child:\n  leaf\n  leaf2",
            "Java's tree layout: a node with one child and no own problems \
             collapses its path onto the child's first line"
        );
    }

    /// A branching tree exercises the indent branch.
    #[test]
    fn tree_report_branches_and_indents() {
        let root = Rc::new(Collector::with_root(Rc::new(FieldPathElement(
            "root".to_string(),
        ))));
        let a = root.for_child(Rc::new(FieldPathElement("a".to_string())));
        a.report(Rc::new(TestProblem("p".to_string())));
        let b = root.for_child(Rc::new(FieldPathElement("b".to_string())));
        b.report(Rc::new(TestProblem("q".to_string())));

        let tree = root.get_tree_report();
        assert_eq!(
            tree, ".root:\n  .a: p\n  .b: q",
            "a root with two children goes through the indent branch"
        );
    }

    /// The exact path-element string forms used by the value layer.
    #[test]
    fn path_element_string_forms() {
        assert_eq!(FieldPathElement("a".to_string()).get(), ".a");
        assert_eq!(IndexedFieldPathElement("a".to_string(), 3).get(), ".a[3]");
        assert_eq!(IndexedPathElement(7).get(), "[7]");
    }
}
