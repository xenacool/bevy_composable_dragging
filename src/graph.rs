//! A graph in which equal things are one thing: every distinct node is stored once, and a node
//! that appears in many places is the same [`Id`] in all of them.
//!
//! Describe a document as nodes that name their children by [`Id`] and implement [`Node`], then
//! build it with [`Graph::intern`]. Two nodes that are equal get the same id, so a material used
//! by a terrain and by a particle emitter is one node, and a state that is copied while it is
//! dragged copies a list of nodes rather than the document. The place of a use inside a document
//! is a *path* of child indices: `[1, 0]` is the first child of the second child of a root. A
//! node is shared and a path is not, which makes a path the identity of something the pointer
//! can grab.
//!
//! Changing a document never changes a node. [`Graph::replace_at`] and [`Graph::rewrite`] build
//! the changed nodes through [`Graph::intern`] again, so a rewrite that arrives at something that
//! exists already lands on the node that exists, and the graph does not grow. Nodes that nothing
//! reaches any more stay in the graph until [`Graph::compact`] drops them.
//!
//! Handing a node to Bevy is [`Instances`], which makes one asset for every distinct node that is
//! asked for, so equal nodes never cost more than one asset between them.
//!
//! ```
//! use bevy_composable_dragging::graph::{Graph, Id, Node};
//!
//! #[derive(Clone, PartialEq, Eq, Hash)]
//! enum Sum {
//!     Number(i64),
//!     Add(Id, Id),
//! }
//!
//! impl Node for Sum {
//!     fn children(&self) -> Vec<Id> {
//!         match self {
//!             Sum::Number(_) => vec![],
//!             Sum::Add(a, b) => vec![*a, *b],
//!         }
//!     }
//!
//!     fn with_children(&self, children: &[Id]) -> Self {
//!         match self {
//!             Sum::Number(n) => Sum::Number(*n),
//!             Sum::Add(..) => Sum::Add(children[0], children[1]),
//!         }
//!     }
//! }
//!
//! let mut graph = Graph::new();
//! let two = graph.intern(Sum::Number(2));
//! let four = graph.intern(Sum::Add(two, two));
//! assert_eq!(graph.paths(four).len(), 3, "the root and two uses of two");
//! assert_eq!(graph.reachable(&[four]), 2, "but only two nodes");
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;

use bevy::asset::{Asset, Assets, Handle};
use bevy::ecs::prelude::Resource;

/// The identity of a node of one [`Graph`]: equal nodes have equal ids, and an id means nothing in
/// another graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(u32);

/// The number of the node, which names a use of it in a view.
impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Something a [`Graph`] stores: it names its children by [`Id`] and can be rebuilt with others.
///
/// Equal nodes must have equal children, and [`with_children`](Self::with_children) must keep
/// everything that is not a child, so that the graph can rebuild a node over rewritten children.
pub trait Node: Clone + Eq + Hash {
    /// The children of the node, in order.
    fn children(&self) -> Vec<Id>;

    /// This node over `children`, which are as many as [`children`](Self::children) gave and in the
    /// same order.
    #[must_use]
    fn with_children(&self, children: &[Id]) -> Self;
}

/// A set of nodes in which every distinct node is stored once. See the [module](self) for how a
/// document is described and changed.
#[derive(Clone, Debug)]
pub struct Graph<N> {
    nodes: Vec<N>,
    ids: HashMap<N, Id>,
}

impl<N> Default for Graph<N> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            ids: HashMap::new(),
        }
    }
}

impl<N: Node> Graph<N> {
    /// An empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The id of `node`, which is stored if the graph has no equal node yet.
    ///
    /// The children of `node` must be ids of this graph.
    pub fn intern(&mut self, node: N) -> Id {
        if let Some(&id) = self.ids.get(&node) {
            return id;
        }
        let id = Id(u32::try_from(self.nodes.len()).expect("a graph has fewer than 2^32 nodes"));
        self.nodes.push(node.clone());
        self.ids.insert(node, id);
        id
    }

    /// The node with `id`, which must be an id of this graph.
    #[must_use]
    pub fn node(&self, id: Id) -> &N {
        &self.nodes[id.0 as usize]
    }

    /// How many distinct nodes the graph stores, reachable or not.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph stores no node.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The node at `path` below `root`, or `None` if the path leaves the document.
    #[must_use]
    pub fn at(&self, root: Id, path: &[usize]) -> Option<Id> {
        path.iter().try_fold(root, |id, &index| {
            self.node(id).children().get(index).copied()
        })
    }

    /// The path of every use of a node below `root`, parents before their children, starting
    /// with the empty path of `root` itself. A shared node has one path for every use.
    #[must_use]
    pub fn paths(&self, root: Id) -> Vec<Vec<usize>> {
        let mut paths = Vec::new();
        self.collect_paths(root, &mut Vec::new(), &mut paths);
        paths
    }

    fn collect_paths(&self, id: Id, here: &mut Vec<usize>, paths: &mut Vec<Vec<usize>>) {
        paths.push(here.clone());
        for (index, child) in self.node(id).children().into_iter().enumerate() {
            here.push(index);
            self.collect_paths(child, here, paths);
            here.pop();
        }
    }

    /// How many distinct nodes are reachable from `roots`: what a document costs, however often
    /// its nodes are used.
    #[must_use]
    pub fn reachable(&self, roots: &[Id]) -> usize {
        let mut seen = HashSet::new();
        let mut pending = roots.to_vec();
        while let Some(id) = pending.pop() {
            if seen.insert(id) {
                pending.extend(self.node(id).children());
            }
        }
        seen.len()
    }

    /// `root` with the node at `path` replaced by `with`, or `None` if the path leaves the
    /// document. Only the nodes on the path are rebuilt, and each lands on an existing node if
    /// the result is one.
    pub fn replace_at(&mut self, root: Id, path: &[usize], with: Id) -> Option<Id> {
        let Some((&index, rest)) = path.split_first() else {
            return Some(with);
        };
        let mut children = self.node(root).children();
        let below = self.replace_at(*children.get(index)?, rest, with)?;
        children[index] = below;
        let rebuilt = self.node(root).with_children(&children);
        Some(self.intern(rebuilt))
    }

    /// `root` rewritten from the leaves up: `rule` gets every node over its already rewritten
    /// children and returns what takes its place. The rule may intern nodes of its own through
    /// the graph it is given.
    ///
    /// A node that is used many times is rewritten once, so a rewrite costs as many calls of
    /// `rule` as the document has distinct nodes, and nodes that come out equal are one node.
    pub fn rewrite(&mut self, root: Id, rule: &mut impl FnMut(&mut Self, N) -> N) -> Id {
        self.rewrite_memo(root, rule, &mut HashMap::new())
    }

    fn rewrite_memo(
        &mut self,
        id: Id,
        rule: &mut impl FnMut(&mut Self, N) -> N,
        done: &mut HashMap<Id, Id>,
    ) -> Id {
        if let Some(&rewritten) = done.get(&id) {
            return rewritten;
        }
        let node = self.node(id).clone();
        let children: Vec<Id> = node
            .children()
            .into_iter()
            .map(|child| self.rewrite_memo(child, rule, done))
            .collect();
        let rewritten = rule(self, node.with_children(&children));
        let rewritten = self.intern(rewritten);
        done.insert(id, rewritten);
        rewritten
    }

    /// A graph of only what `roots` reach, and the ids of `roots` in it, in order. Every other id
    /// stops meaning anything, so ids and [`Instances`] of this graph are not carried over.
    #[must_use]
    pub fn compact(&self, roots: &[Id]) -> (Self, Vec<Id>) {
        let mut compacted = Self::new();
        let mut moved = HashMap::new();
        let roots = roots
            .iter()
            .map(|&root| self.copy_into(root, &mut compacted, &mut moved))
            .collect();
        (compacted, roots)
    }

    fn copy_into(&self, id: Id, into: &mut Self, moved: &mut HashMap<Id, Id>) -> Id {
        if let Some(&copy) = moved.get(&id) {
            return copy;
        }
        let node = self.node(id);
        let children: Vec<Id> = node
            .children()
            .into_iter()
            .map(|child| self.copy_into(child, into, moved))
            .collect();
        let copy = into.intern(node.with_children(&children));
        moved.insert(id, copy);
        copy
    }
}

/// The assets of the nodes of one [`Graph`]: one asset for every distinct node that was asked
/// for, however many places use it.
///
/// A terrain and a particle emitter that both use the node of a material hold handles of the one
/// asset it made, and a material that is changed to something that exists already gets that
/// asset back instead of a copy.
#[derive(Resource)]
pub struct Instances<A: Asset> {
    handles: HashMap<Id, Handle<A>>,
}

impl<A: Asset> Default for Instances<A> {
    fn default() -> Self {
        Self {
            handles: HashMap::new(),
        }
    }
}

impl<A: Asset> Instances<A> {
    /// The handle of the asset of `id`, which `make` builds and adds to `assets` the first time
    /// `id` is asked for.
    pub fn get_or_add(
        &mut self,
        id: Id,
        assets: &mut Assets<A>,
        make: impl FnOnce() -> A,
    ) -> Handle<A> {
        self.handles
            .entry(id)
            .or_insert_with(|| assets.add(make()))
            .clone()
    }

    /// How many nodes have an asset.
    #[must_use]
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    /// Whether no node has an asset.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::Asset;
    use bevy::reflect::TypePath;

    use super::*;

    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    enum Expr {
        Number(i64),
        Add(Id, Id),
        Neg(Id),
    }

    impl Node for Expr {
        fn children(&self) -> Vec<Id> {
            match self {
                Expr::Number(_) => vec![],
                Expr::Add(a, b) => vec![*a, *b],
                Expr::Neg(a) => vec![*a],
            }
        }

        fn with_children(&self, children: &[Id]) -> Self {
            match self {
                Expr::Number(n) => Expr::Number(*n),
                Expr::Add(..) => Expr::Add(children[0], children[1]),
                Expr::Neg(..) => Expr::Neg(children[0]),
            }
        }
    }

    #[derive(Asset, TypePath)]
    struct Paint(i64);

    fn number(graph: &mut Graph<Expr>, n: i64) -> Id {
        graph.intern(Expr::Number(n))
    }

    #[test]
    fn equal_nodes_are_one_node() {
        let mut graph = Graph::new();
        let (a, b) = (number(&mut graph, 3), number(&mut graph, 3));
        assert_eq!(a, b);
        assert_eq!(graph.len(), 1);
        let sum = graph.intern(Expr::Add(a, b));
        assert_eq!(graph.intern(Expr::Add(b, a)), sum);
        assert_eq!(graph.len(), 2);
    }

    #[test]
    fn a_shared_node_has_one_path_for_every_use() {
        let mut graph = Graph::new();
        let three = number(&mut graph, 3);
        let sum = graph.intern(Expr::Add(three, three));
        let root = graph.intern(Expr::Neg(sum));
        assert_eq!(
            graph.paths(root),
            vec![vec![], vec![0], vec![0, 0], vec![0, 1]]
        );
        assert_eq!(graph.at(root, &[0, 1]), Some(three));
        assert_eq!(graph.at(root, &[1]), None);
        assert_eq!(graph.reachable(&[root]), 3);
    }

    #[test]
    fn replacing_one_use_leaves_the_other_and_rebuilds_only_the_path() {
        let mut graph = Graph::new();
        let (three, five) = (number(&mut graph, 3), number(&mut graph, 5));
        let sum = graph.intern(Expr::Add(three, three));
        let replaced = graph.replace_at(sum, &[1], five).unwrap();
        assert_eq!(graph.node(replaced), &Expr::Add(three, five));
        assert_eq!(graph.replace_at(sum, &[2], five), None);
        assert_eq!(graph.replace_at(sum, &[], five), Some(five));
    }

    #[test]
    fn a_rewrite_that_arrives_at_an_existing_node_does_not_grow_the_graph() {
        let mut graph = Graph::new();
        let (three, five) = (number(&mut graph, 3), number(&mut graph, 5));
        let before = graph.intern(Expr::Add(three, three));
        let target = graph.intern(Expr::Add(five, five));
        let size = graph.len();
        let rewritten = graph.rewrite(before, &mut |_, node| match node {
            Expr::Number(3) => Expr::Number(5),
            other => other,
        });
        assert_eq!(rewritten, target);
        assert_eq!(graph.len(), size);
    }

    #[test]
    fn a_shared_node_is_rewritten_once() {
        let mut graph = Graph::new();
        let three = number(&mut graph, 3);
        let sum = graph.intern(Expr::Add(three, three));
        let root = graph.intern(Expr::Add(sum, sum));
        let mut calls = 0;
        graph.rewrite(root, &mut |_, node| {
            calls += 1;
            node
        });
        assert_eq!(calls, 3, "one call for each of the three distinct nodes");
    }

    #[test]
    fn a_rule_can_intern_nodes_of_its_own() {
        let mut graph = Graph::new();
        let three = number(&mut graph, 3);
        let neg = graph.intern(Expr::Neg(three));
        let rewritten = graph.rewrite(neg, &mut |graph, node| match node {
            Expr::Neg(inner) => {
                let zero = graph.intern(Expr::Number(0));
                Expr::Add(zero, inner)
            }
            other => other,
        });
        assert!(matches!(graph.node(rewritten), Expr::Add(..)));
    }

    #[test]
    fn compacting_drops_what_nothing_reaches_and_keeps_the_sharing() {
        let mut graph = Graph::new();
        let three = number(&mut graph, 3);
        for n in 10..20 {
            number(&mut graph, n);
        }
        let root = graph.intern(Expr::Add(three, three));
        assert_eq!(graph.len(), 12);
        let (compacted, roots) = graph.compact(&[root]);
        assert_eq!(compacted.len(), 2);
        assert_eq!(compacted.paths(roots[0]).len(), 3);
    }

    #[test]
    fn equal_nodes_share_one_asset() {
        let mut graph = Graph::new();
        let (a, b) = (number(&mut graph, 7), number(&mut graph, 7));
        let other = number(&mut graph, 8);
        let mut assets = Assets::<Paint>::default();
        let mut instances = Instances::default();
        let first = instances.get_or_add(a, &mut assets, || Paint(7));
        let second = instances.get_or_add(b, &mut assets, || panic!("built a copy"));
        assert_eq!(first, second);
        instances.get_or_add(other, &mut assets, || Paint(8));
        assert_eq!(assets.len(), 2);
        assert_eq!(instances.len(), 2);
        assert_eq!(assets.get(&first).unwrap().0, 7);
    }
}
