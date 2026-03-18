use std::collections::HashSet;

use ps_hkey::Store;
use ps_uuid::UUID;

use crate::{HtreeNode, LEAF_HEIGHT};

impl<T> HtreeNode<T> {
    /// Replaces existing leaves with new ones. If a key isn't found, the operation fails.
    ///
    /// May return multiple sibling nodes if the updated contents cause
    /// a node to exceed `MAX_CHILDREN`. Updating is not always
    /// a one‑to‑one operation with existing leaves.
    ///
    /// Returns a vector of new nodes.
    ///
    /// # Arguments
    /// * `children` - Leaves or internal nodes to update
    /// * `store` - Persistence backend
    ///
    /// # Errors
    /// - [`HtreeNodeUpdateLeavesError::CorruptedLeaf`] is returned if a leaf's state is invalid.
    /// - [`HtreeNodeUpdateLeavesError::CorruptedNode`] is returned if this node's state is invalid.
    /// - [`HtreeNodeUpdateLeavesError::FromChildren`] is returned if node reconstruction fails.
    /// - [`HtreeNodeUpdateLeavesError::KeyNotFound`] is returned if you're updating a record that doesn't exist.
    /// - [`HtreeNodeUpdateLeavesError::Store`] is returned if store operations fail.
    /// - [`HtreeNodeUpdateLeavesError::UnpackChildren`] is returned if child deserialization fails.
    #[allow(clippy::significant_drop_tightening)]
    pub fn update_leaves<I: IntoIterator<Item = Self>, S: Store>(
        &self,
        children: I,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeUpdateLeavesError<S>> {
        let mut updated_leaves = Vec::new();

        for child in children {
            if child.is_leaf() {
                updated_leaves.push(child);

                continue;
            }

            for leaf in child.iter_leaves(store) {
                updated_leaves.push(leaf?);
            }
        }

        if updated_leaves.is_empty() {
            return Ok(vec![self.clone()]);
        }

        if self.height <= LEAF_HEIGHT + 1 {
            let current_leaves = self.fetch_children_guard(store)?;

            let current_keys: HashSet<UUID> = current_leaves.iter().map(|leaf| leaf.key).collect();
            let updated_keys: HashSet<UUID> = updated_leaves.iter().map(|leaf| leaf.key).collect();

            if let Some(key) = updated_keys.difference(&current_keys).next() {
                return Err(HtreeNodeUpdateLeavesError::KeyNotFound(*key));
            }

            let merged = current_leaves
                .iter()
                .filter(|leaf| !updated_keys.contains(&leaf.key))
                .cloned()
                .chain(updated_leaves);

            return Ok(Self::from_many_children(merged, store)?);
        }

        let mut groups: Vec<(Self, Vec<Self>)> = self
            .iter_children(store)?
            .map(|child| (child, vec![]))
            .collect();

        if groups.is_empty() {
            return Err(HtreeNodeUpdateLeavesError::KeyNotFound(
                updated_leaves[0].key,
            ));
        }

        let mut push_leaf = |leaf: Self| {
            let index = groups
                .partition_point(|(node, _)| node.key <= leaf.key)
                .saturating_sub(1);

            groups[index].1.push(leaf);
        };

        for child in updated_leaves {
            push_leaf(child);
        }

        let mut children = Vec::new();

        for (base, updates) in groups {
            if updates.is_empty() {
                children.push(base);
            } else {
                children.extend(base.update_leaves(updates, store)?);
            }
        }

        Ok(Self::from_many_children(children, store)?)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeUpdateLeavesError<S: Store> {
    #[error("Updated leaf's state is corrupted.")]
    CorruptedLeaf,
    #[error("HtreeNode's state is corrupted.")]
    CorruptedNode,
    #[error("Node reconstruction failed.")]
    FromChildren(crate::HtreeNodeFromChildrenError<S>),
    #[error("Key not found: {0}")]
    KeyNotFound(UUID),
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error("Error unpacking children: {0}")]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeFetchChildrenError<S>> for HtreeNodeUpdateLeavesError<S> {
    fn from(value: crate::HtreeNodeFetchChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFetchChildrenError::CorruptedState => Self::CorruptedNode,
            crate::HtreeNodeFetchChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeFetchChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeIterChildrenError<S>> for HtreeNodeUpdateLeavesError<S> {
    fn from(value: crate::HtreeNodeIterChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeIterChildrenError::CorruptedState => Self::CorruptedNode,
            crate::HtreeNodeIterChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeIterChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeIterLeavesError<S>> for HtreeNodeUpdateLeavesError<S> {
    fn from(value: crate::HtreeNodeIterLeavesError<S>) -> Self {
        match value {
            crate::HtreeNodeIterLeavesError::CorruptedState => Self::CorruptedLeaf,
            crate::HtreeNodeIterLeavesError::Store(err) => Self::Store(err),
            crate::HtreeNodeIterLeavesError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeFromChildrenError<S>> for HtreeNodeUpdateLeavesError<S> {
    fn from(value: crate::HtreeNodeFromChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFromChildrenError::Store(err) => Self::Store(err),
            err => Self::FromChildren(err),
        }
    }
}
