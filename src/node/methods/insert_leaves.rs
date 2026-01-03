use ps_hkey::Store;

use crate::{HtreeNode, LEAF_HEIGHT};

impl<T> HtreeNode<T> {
    /// Inserts leaves into this node, rebalancing if necessary.
    ///
    /// Accepts both leaf and internal nodes. Returns potentially multiple sibling
    /// nodes if rebalancing causes the tree to split.
    ///
    /// # Arguments
    /// * `children` - Leaves or internal nodes to insert
    /// * `store` - Persistence backend
    ///
    /// # Errors
    /// - [`HtreeNodeInsertLeavesError::CorruptedLeaf`] is returned if a leaf's state is invalid.
    /// - [`HtreeNodeInsertLeavesError::CorruptedNode`] is returned if this node's state is invalid.
    /// - [`HtreeNodeInsertLeavesError::FromChildren`] is returned if node reconstruction fails.
    /// - [`HtreeNodeInsertLeavesError::Store`] is returned if store operations fail.
    /// - [`HtreeNodeInsertLeavesError::UnpackChildren`] is returned if child deserialization fails.
    pub fn insert_leaves<I: IntoIterator<Item = Self>, S: Store>(
        &self,
        children: I,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeInsertLeavesError<S>> {
        if self.height <= LEAF_HEIGHT + 1 {
            let mut leaves = vec![];

            for child in self
                .fetch_children(store)?
                .into_iter()
                .chain(children.into_iter())
            {
                if child.is_leaf() {
                    leaves.push(child);

                    continue;
                }

                for leaf in child.iter_leaves(store) {
                    leaves.push(leaf?);
                }
            }

            return Self::from_many_children(leaves, store).map_err(Into::into);
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
                children.extend(node.insert_leaves(leaves, store)?);
            }
        }

        Self::from_many_children(children, store).map_err(Into::into)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeInsertLeavesError<S: Store> {
    #[error("Inserted leaf's state is corrupted.")]
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

impl<S: Store> From<crate::HtreeNodeFetchChildrenError<S>> for HtreeNodeInsertLeavesError<S> {
    fn from(value: crate::HtreeNodeFetchChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFetchChildrenError::CorruptedState => Self::CorruptedNode,
            crate::HtreeNodeFetchChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeFetchChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeIterLeavesError<S>> for HtreeNodeInsertLeavesError<S> {
    fn from(value: crate::HtreeNodeIterLeavesError<S>) -> Self {
        match value {
            crate::HtreeNodeIterLeavesError::CorruptedState => Self::CorruptedLeaf,
            crate::HtreeNodeIterLeavesError::Store(err) => Self::Store(err),
            crate::HtreeNodeIterLeavesError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeFromChildrenError<S>> for HtreeNodeInsertLeavesError<S> {
    fn from(value: crate::HtreeNodeFromChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFromChildrenError::Store(err) => Self::Store(err),
            err => Self::FromChildren(err),
        }
    }
}
