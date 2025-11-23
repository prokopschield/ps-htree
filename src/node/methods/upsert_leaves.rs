use std::collections::HashSet;

use ps_hkey::Store;

use crate::{HtreeNode, LEAF_HEIGHT, MAX_CHILDREN};

impl<T> HtreeNode<T> {
    /// Upserts leaves into this node, rebalancing if necessary.
    ///
    /// Accepts both leaf and internal nodes. Returns potentially multiple sibling
    /// nodes if rebalancing causes the tree to split.
    ///
    /// # Arguments
    /// * `children` - Leaves or internal nodes to upsert
    /// * `store` - Persistence backend
    ///
    /// # Errors
    /// - [`HtreeNodeUpsertLeavesError::CorruptedLeaf`] is returned if a leaf's state is invalid.
    /// - [`HtreeNodeUpsertLeavesError::CorruptedNode`] is returned if this node's state is invalid.
    /// - [`HtreeNodeUpsertLeavesError::FromChildren`] is returned if node reconstruction fails.
    /// - [`HtreeNodeUpsertLeavesError::Store`] is returned if store operations fail.
    /// - [`HtreeNodeUpsertLeavesError::UnpackChildren`] is returned if child deserialization fails.
    pub fn upsert_leaves<I: IntoIterator<Item = Self>, S: Store>(
        &self,
        children: I,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeUpsertLeavesError<S>> {
        if self.height <= LEAF_HEIGHT + 1 {
            let mut keys = HashSet::new();
            let mut leaves = vec![];

            for child in children {
                if child.is_leaf() {
                    keys.insert(child.key);
                    leaves.push(child);

                    continue;
                }

                for leaf in child.iter_leaves(store) {
                    let leaf = leaf?;
                    keys.insert(leaf.key);
                    leaves.push(leaf);
                }
            }

            for child in self.fetch_children(store)? {
                if !keys.contains(&child.key) {
                    leaves.push(child);
                }
            }

            leaves.sort();

            return aggregate_children(leaves, store);
        }

        let mut groups: Vec<(Self, Vec<Self>)> = self
            .fetch_children(store)?
            .into_iter()
            .map(|child| (child, vec![]))
            .collect();

        if groups.is_empty() {
            groups.push((Self::default(), vec![]));
        }

        let mut push_leaf = |leaf: Self| {
            let index = groups
                .partition_point(|(node, _)| node.key <= leaf.key)
                .saturating_sub(1);

            groups[index].1.push(leaf);
        };

        for child in children {
            if child.is_leaf() {
                push_leaf(child);

                continue;
            }

            for leaf in child.iter_leaves(store) {
                push_leaf(leaf?);
            }
        }

        let mut children = Vec::new();

        for (node, leaves) in groups {
            if leaves.is_empty() {
                children.push(node);
            } else {
                children.extend(node.upsert_leaves(leaves, store)?);
            }
        }

        aggregate_children(children, store)
    }
}

fn aggregate_children<T, S: Store>(
    children: Vec<HtreeNode<T>>,
    store: &S,
) -> Result<Vec<HtreeNode<T>>, HtreeNodeUpsertLeavesError<S>> {
    if children.is_empty() {
        return Ok(Vec::new());
    }

    // Minimum nodes needed to fit all children with ≤ MAX_CHILDREN each
    let num_nodes = children.len().div_ceil(MAX_CHILDREN);
    // Fair per-node size to minimize imbalance across siblings
    let chunk_size = children.len().div_ceil(num_nodes);

    let mut children = children.into_iter();
    let mut nodes = Vec::with_capacity(num_nodes);

    for _ in 0..num_nodes {
        let chunk = children.by_ref().take(chunk_size);

        nodes.push(HtreeNode::from_children(chunk, store)?);
    }

    Ok(nodes)
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeUpsertLeavesError<S: Store> {
    #[error("Upserted leaf's state is corrupted.")]
    CorruptedLeaf,
    #[error("HtreeNode's state is corrupted.")]
    CorruptedNode,
    #[error("Node reconstruction failed.")]
    FromChildren(crate::HtreeNodeFromChildrenError<S>),
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error("Error unpacking children: {0}")]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeFetchChildrenError<S>> for HtreeNodeUpsertLeavesError<S> {
    fn from(value: crate::HtreeNodeFetchChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFetchChildrenError::CorruptedState => Self::CorruptedNode,
            crate::HtreeNodeFetchChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeFetchChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeIterLeavesError<S>> for HtreeNodeUpsertLeavesError<S> {
    fn from(value: crate::HtreeNodeIterLeavesError<S>) -> Self {
        match value {
            crate::HtreeNodeIterLeavesError::CorruptedState => Self::CorruptedLeaf,
            crate::HtreeNodeIterLeavesError::Store(err) => Self::Store(err),
            crate::HtreeNodeIterLeavesError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeFromChildrenError<S>> for HtreeNodeUpsertLeavesError<S> {
    fn from(value: crate::HtreeNodeFromChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFromChildrenError::Store(err) => Self::Store(err),
            err => Self::FromChildren(err),
        }
    }
}
