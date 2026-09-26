//! Port of the `PolyNode`/`PolyTree` classes of clipper.{hpp,cpp}.
//!
//! The nodes are stored in an arena in the order of upstream's `AllNodes`
//! list (which matters for [`PolyTree::total()`]); the root node (the
//! `PolyTree` itself upstream) is stored separately.

use crate::Path;

/// Identifies a node of a [`PolyTree`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum NodeId {
    /// The root node (upstream: the `PolyTree` object itself).
    Root,
    /// A node in the arena (index into upstream `AllNodes`).
    Node(usize),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NodeData {
    pub(crate) contour: Path,
    pub(crate) children: Vec<usize>,
    pub(crate) parent: Option<NodeId>,
    /// Index in the children list of the parent.
    pub(crate) index: usize,
    pub(crate) is_open: bool,
}

/// Result of a clipping or offsetting operation as a tree of outlines and
/// holes (upstream `PolyTree`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PolyTree {
    pub(crate) root: NodeData,
    /// Upstream `AllNodes`.
    pub(crate) nodes: Vec<NodeData>,
}

impl PolyTree {
    /// Creates an empty tree.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the root node (which has no contour).
    pub fn root(&self) -> PolyNode<'_> {
        PolyNode {
            tree: self,
            id: NodeId::Root,
        }
    }

    /// Returns the first top-level node, if any.
    pub fn get_first(&self) -> Option<PolyNode<'_>> {
        self.root
            .children
            .first()
            .map(|&i| self.node(NodeId::Node(i)))
    }

    /// Returns the number of nodes (excluding the root).
    ///
    /// Like upstream, one node is subtracted if the first top-level node is
    /// not the first created node (to ignore the hidden outer rectangle of
    /// negative offsets).
    pub fn total(&self) -> usize {
        let result = self.nodes.len();
        if result > 0 && self.root.children.first() != Some(&0) {
            result - 1
        } else {
            result
        }
    }

    /// Removes all nodes.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root.children.clear();
    }

    /// Returns the number of top-level nodes.
    pub fn child_count(&self) -> usize {
        self.root.children.len()
    }

    /// Returns an iterator over the top-level nodes.
    pub fn children(&self) -> impl ExactSizeIterator<Item = PolyNode<'_>> + '_ {
        self.root().children()
    }

    pub(crate) fn node(&self, id: NodeId) -> PolyNode<'_> {
        PolyNode { tree: self, id }
    }

    pub(crate) fn data(&self, id: NodeId) -> &NodeData {
        match id {
            NodeId::Root => &self.root,
            NodeId::Node(i) => &self.nodes[i],
        }
    }

    pub(crate) fn data_mut(&mut self, id: NodeId) -> &mut NodeData {
        match id {
            NodeId::Root => &mut self.root,
            NodeId::Node(i) => &mut self.nodes[i],
        }
    }

    /// Upstream `PolyNode::AddChild()`.
    pub(crate) fn add_child(&mut self, parent: NodeId, child: usize) {
        let data = self.data_mut(parent);
        let cnt = data.children.len();
        data.children.push(child);
        let child = &mut self.nodes[child];
        child.parent = Some(parent);
        child.index = cnt;
    }
}

/// A node of a [`PolyTree`] (upstream `PolyNode`).
#[derive(Clone, Copy)]
pub struct PolyNode<'a> {
    tree: &'a PolyTree,
    id: NodeId,
}

impl std::fmt::Debug for PolyNode<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolyNode")
            .field("contour", self.contour())
            .field("is_hole", &self.is_hole())
            .field("is_open", &self.is_open())
            .field("children", &self.children().collect::<Vec<_>>())
            .finish()
    }
}

impl PartialEq for PolyNode<'_> {
    /// Node identity (same tree and same node).
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.tree, other.tree) && self.id == other.id
    }
}

impl Eq for PolyNode<'_> {}

impl<'a> PolyNode<'a> {
    fn data(&self) -> &'a NodeData {
        self.tree.data(self.id)
    }

    /// Returns whether this is the root node of the tree.
    pub fn is_root(&self) -> bool {
        self.id == NodeId::Root
    }

    /// Returns the outline (empty for the root node).
    pub fn contour(&self) -> &'a Path {
        &self.data().contour
    }

    /// Returns the child nodes.
    pub fn children(self) -> impl ExactSizeIterator<Item = PolyNode<'a>> + 'a {
        let tree = self.tree;
        self.data()
            .children
            .iter()
            .map(move |&i| tree.node(NodeId::Node(i)))
    }

    /// Returns the number of child nodes.
    pub fn child_count(&self) -> usize {
        self.data().children.len()
    }

    /// Returns the child at `index`, if any.
    pub fn child(&self, index: usize) -> Option<PolyNode<'a>> {
        let i = *self.data().children.get(index)?;
        Some(self.tree.node(NodeId::Node(i)))
    }

    /// Returns the parent node (`None` for the root node).
    pub fn parent(&self) -> Option<PolyNode<'a>> {
        self.data().parent.map(|id| self.tree.node(id))
    }

    /// Returns whether this node is a hole, i.e. whether it has an odd
    /// number of ancestors (including the root). Like upstream, the root
    /// node itself counts as hole.
    pub fn is_hole(&self) -> bool {
        let mut result = true;
        let mut node = self.data().parent;
        while let Some(id) = node {
            result = !result;
            node = self.tree.data(id).parent;
        }
        result
    }

    /// Returns whether this node is an open path.
    pub fn is_open(&self) -> bool {
        self.data().is_open
    }

    /// Returns the next node in depth-first order (upstream `GetNext()`).
    pub fn get_next(&self) -> Option<PolyNode<'a>> {
        match self.data().children.first() {
            Some(&i) => Some(self.tree.node(NodeId::Node(i))),
            None => self.next_sibling_up(),
        }
    }

    fn next_sibling_up(&self) -> Option<PolyNode<'a>> {
        let parent = self.parent()?;
        let siblings = &parent.data().children;
        match siblings.get(self.data().index + 1) {
            Some(&i) if self.data().index + 1 < siblings.len() => {
                Some(self.tree.node(NodeId::Node(i)))
            }
            _ => parent.next_sibling_up(),
        }
    }
}
